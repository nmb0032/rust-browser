use super::*;
use crate::css::tokenizer::NumericKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Nth {
    a: i64,
    b: i64,
    last: bool,
    of_type: bool,
    pub(super) of: Vec<Selector>,
}

impl Nth {
    pub(super) fn matches(
        &self,
        document: &Document,
        id: NodeId,
        element: &ElementData,
        context: MatchContext<'_>,
    ) -> bool {
        let relevant = |other: NodeId| {
            if !context.step() {
                return false;
            }
            document
                .node(other)
                .and_then(|n| n.as_element())
                .is_some_and(|e| {
                    (!self.of_type
                        || e.namespace() == element.namespace()
                            && equal(
                                e.tag_name(),
                                element.tag_name(),
                                !element.has_case_sensitive_names(),
                            ))
                        && (self.of.is_empty()
                            || self
                                .of
                                .iter()
                                .any(|s| s.matches_context(document, other, context)))
                })
        };
        if !relevant(id) {
            return false;
        }
        let (before, after) = sibling_slices(document, id);
        let siblings = if self.last { after } else { before };
        let mut index = 1i128;
        for &other in siblings {
            if !context.step() {
                return false;
            }
            if relevant(other) {
                index += 1;
            }
        }
        let difference = index - i128::from(self.b);
        let a = i128::from(self.a);
        if a == 0 {
            difference == 0
        } else {
            difference % a == 0 && difference / a >= 0
        }
    }
}

impl Selector {
    pub(super) fn matches_relative(
        &self,
        document: &Document,
        anchor: NodeId,
        context: MatchContext<'_>,
    ) -> bool {
        let mut pending = Vec::new();
        let mut seen = HashSet::new();
        if !self.successors(
            document,
            anchor,
            self.relative.expect("relative selector"),
            0,
            context,
            &mut pending,
        ) {
            return false;
        }
        while let Some((index, id)) = pending.pop() {
            if !context.step() {
                return false;
            }
            if !seen.insert((index, id)) || !self.compounds[index].matches(document, id, context) {
                continue;
            }
            if index + 1 == self.compounds.len() {
                return true;
            }
            if !self.successors(
                document,
                id,
                self.combinators[index],
                index + 1,
                context,
                &mut pending,
            ) {
                return false;
            }
        }
        false
    }

    fn successors(
        &self,
        document: &Document,
        id: NodeId,
        axis: Combinator,
        index: usize,
        context: MatchContext<'_>,
        pending: &mut Vec<(usize, NodeId)>,
    ) -> bool {
        match axis {
            Combinator::Child | Combinator::Descendant => {
                let initial = document.node(id).map(|n| n.children()).unwrap_or(&[]);
                if initial.len() >= context.remaining.get() {
                    context.remaining.set(0);
                    return false;
                }
                let mut children = initial.to_vec();
                while let Some(child) = children.pop() {
                    if !context.step() {
                        return false;
                    }
                    if let Some(node) = document.node(child) {
                        if node.as_element().is_some() {
                            pending.push((index, child));
                        }
                        if axis == Combinator::Descendant {
                            if node.children().len().saturating_add(children.len())
                                >= context.remaining.get()
                            {
                                context.remaining.set(0);
                                return false;
                            }
                            children.extend_from_slice(node.children());
                        }
                    }
                }
            }
            Combinator::NextSibling | Combinator::SubsequentSibling => {
                for &sibling in sibling_slices(document, id).1 {
                    if !context.step() {
                        return false;
                    }
                    if document
                        .node(sibling)
                        .is_some_and(|n| n.as_element().is_some())
                    {
                        pending.push((index, sibling));
                        if axis == Combinator::NextSibling {
                            break;
                        }
                    }
                }
            }
        }
        true
    }
}

impl Parser {
    /// Split only top-level commas. Invalid forgiving branches never consume
    /// the following branch; strict lists reject the complete function.
    pub(super) fn functional_list(
        &mut self,
        depth: usize,
        forgiving: bool,
        relative: bool,
    ) -> Result<Vec<Selector>, SelectorError> {
        if depth > MAX_SELECTOR_NESTING {
            return Err(SelectorError);
        }
        let start = self.position;
        let mut end = start;
        let mut nesting = Vec::new();
        let mut ranges = Vec::new();
        let mut branch = start;
        loop {
            match self.tokens.get(end).ok_or(SelectorError)? {
                TokenKind::Function(_) | TokenKind::OpenParen => {
                    nesting.push(TokenKind::CloseParen)
                }
                TokenKind::OpenSquare => nesting.push(TokenKind::CloseSquare),
                TokenKind::OpenCurly => nesting.push(TokenKind::CloseCurly),
                TokenKind::CloseParen if nesting.is_empty() => {
                    ranges.push(branch..end);
                    break;
                }
                TokenKind::CloseParen | TokenKind::CloseSquare | TokenKind::CloseCurly => {
                    if nesting.pop().as_ref() != self.tokens.get(end) {
                        return Err(SelectorError);
                    }
                }
                TokenKind::Comma if nesting.is_empty() => {
                    ranges.push(branch..end);
                    branch = end + 1;
                }
                TokenKind::Eof => return Err(SelectorError),
                _ => {}
            }
            end += 1;
        }
        self.position = end + 1;
        let mut selectors = Vec::new();
        for range in ranges {
            let mut parser = Parser {
                tokens: self.tokens[range]
                    .iter()
                    .cloned()
                    .chain([TokenKind::Eof])
                    .collect(),
                position: 0,
                namespaces: self.namespaces.clone(),
                parent: self.parent.clone(),
                scoped: self.scoped,
                in_has: self.in_has || relative,
                allow_pseudo: false,
            };
            parser.whitespace();
            let axis = if relative {
                let axis = match parser.peek() {
                    TokenKind::Delim('>') => Some(Combinator::Child),
                    TokenKind::Delim('+') => Some(Combinator::NextSibling),
                    TokenKind::Delim('~') => Some(Combinator::SubsequentSibling),
                    _ => None,
                };
                if axis.is_some() {
                    parser.position += 1;
                    parser.whitespace();
                }
                Some(axis.unwrap_or(Combinator::Descendant))
            } else {
                None
            };
            match parser.selector(depth) {
                Ok(mut selector) if parser.peek() == &TokenKind::Eof => {
                    selector.relative = axis;
                    selectors.push(selector);
                }
                _ if forgiving => {}
                _ => return Err(SelectorError),
            }
        }
        Ok(selectors)
    }

    pub(super) fn nth(&mut self, name: &str, depth: usize) -> Result<Nth, SelectorError> {
        if depth > MAX_SELECTOR_NESTING {
            return Err(SelectorError);
        }
        self.whitespace();
        let start = self.position;
        let mut end = start;
        while !matches!(
            self.tokens.get(end),
            Some(TokenKind::CloseParen | TokenKind::Eof)
        ) && !matches!(self.tokens.get(end), Some(TokenKind::Ident(name)) if name.eq_ignore_ascii_case("of"))
        {
            end += 1;
        }
        let (a, b) = an_b(&self.tokens[start..end]).ok_or(SelectorError)?;
        let of_type = name.ends_with("of-type");
        self.position = end;
        let of = if matches!(self.peek(), TokenKind::Ident(_)) {
            if of_type || end == start || self.tokens[end - 1] != TokenKind::Whitespace {
                return Err(SelectorError);
            }
            self.position += 1;
            self.functional_list(depth, false, false)?
        } else {
            self.consume(TokenKind::CloseParen)?;
            Vec::new()
        };
        Ok(Nth {
            a,
            b,
            last: name.contains("last"),
            of_type,
            of,
        })
    }
}

fn integer(token: &TokenKind) -> Option<i64> {
    match token {
        TokenKind::Number(number) if number.kind == NumericKind::Integer => {
            number.representation.parse().ok()
        }
        _ => None,
    }
}

fn an_b(tokens: &[TokenKind]) -> Option<(i64, i64)> {
    let mut start = 0;
    let mut end = tokens.len();
    while start < end && tokens[start] == TokenKind::Whitespace {
        start += 1;
    }
    while end > start && tokens[end - 1] == TokenKind::Whitespace {
        end -= 1;
    }
    let mut tokens = &tokens[start..end];
    if let [TokenKind::Ident(name)] = tokens {
        if name.eq_ignore_ascii_case("odd") {
            return Some((2, 1));
        }
        if name.eq_ignore_ascii_case("even") {
            return Some((2, 0));
        }
    }
    if tokens.len() == 1
        && let Some(b) = integer(&tokens[0])
    {
        return Some((0, b));
    }
    let plus = tokens.first() == Some(&TokenKind::Delim('+'));
    if plus {
        tokens = &tokens[1..];
    }
    let (a, unit) = match tokens.first()? {
        TokenKind::Dimension { number, unit } if !plus && number.kind == NumericKind::Integer => (
            number.representation.parse().ok()?,
            unit.to_ascii_lowercase(),
        ),
        TokenKind::Ident(unit) => {
            let unit = unit.to_ascii_lowercase();
            if let Some(unit) = unit.strip_prefix('-') {
                if plus {
                    return None;
                }
                (-1, unit.to_string())
            } else {
                (1, unit)
            }
        }
        _ => return None,
    };
    let tail = &tokens[1..];
    let suffix = unit.strip_prefix('n')?;
    if !suffix.is_empty() && suffix != "-" {
        if !suffix.starts_with('-')
            || !suffix[1..].bytes().all(|b| b.is_ascii_digit())
            || !tail.is_empty()
        {
            return None;
        }
        return Some((a, suffix.parse().ok()?));
    }
    let tail: Vec<_> = tail
        .iter()
        .filter(|t| **t != TokenKind::Whitespace)
        .collect();
    let b = if suffix == "-" {
        match tail.as_slice() {
            [token] => {
                let TokenKind::Number(n) = token else {
                    return None;
                };
                if n.sign().is_some() {
                    return None;
                }
                integer(token)?.checked_neg()?
            }
            _ => return None,
        }
    } else {
        match tail.as_slice() {
            [] => 0,
            [token] => {
                let TokenKind::Number(n) = token else {
                    return None;
                };
                n.sign()?;
                integer(token)?
            }
            [TokenKind::Delim(sign @ ('+' | '-')), token] => {
                let TokenKind::Number(n) = token else {
                    return None;
                };
                if n.sign().is_some() {
                    return None;
                }
                let value = integer(token)?;
                if *sign == '-' {
                    value.checked_neg()?
                } else {
                    value
                }
            }
            _ => return None,
        }
    };
    Some((a, b))
}
