//! Token-based selectors, specificity, and DOM matching.
//! https://www.w3.org/TR/selectors-4/

use std::{collections::HashSet, error::Error, fmt};

use crate::dom::{Document, DocumentMode, ElementData, NodeId, NodeKind};

use super::tokenizer::{HashKind, TokenKind, Tokenizer};

pub const MAX_SELECTOR_NESTING: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    pub ids: u32,
    pub classes: u32,
    pub types: u32,
}

impl Specificity {
    fn add(&mut self, other: Self) {
        self.ids = self.ids.saturating_add(other.ids);
        self.classes = self.classes.saturating_add(other.classes);
        self.types = self.types.saturating_add(other.types);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Compound {
    tag: Option<String>,
    simple: Vec<SimpleSelector>,
}

impl Compound {
    fn matches(&self, document: &Document, node_id: NodeId) -> bool {
        let Some(element) = document.node(node_id).and_then(|node| node.as_element()) else {
            return false;
        };
        self.tag
            .as_ref()
            .is_none_or(|tag| equal(element.tag_name(), tag, !element.has_case_sensitive_names()))
            && self
                .simple
                .iter()
                .all(|simple| simple.matches(document, node_id, element))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SimpleSelector {
    Id(String),
    Class(String),
    Attribute(AttributeSelector),
    Pseudo(PseudoClass),
}

impl SimpleSelector {
    fn specificity(&self) -> Specificity {
        match self {
            Self::Id(_) => Specificity {
                ids: 1,
                ..Specificity::default()
            },
            Self::Pseudo(PseudoClass::Not(selectors)) => selectors
                .iter()
                .map(Selector::specificity)
                .max()
                .unwrap_or_default(),
            _ => Specificity {
                classes: 1,
                ..Specificity::default()
            },
        }
    }

    fn matches(&self, document: &Document, node_id: NodeId, element: &ElementData) -> bool {
        let quirks = document.mode() == DocumentMode::Quirks;
        match self {
            Self::Id(id) => element
                .attribute("id")
                .is_some_and(|value| equal(value, id, quirks)),
            Self::Class(class) => element.attribute("class").is_some_and(|value| {
                value
                    .split(css_whitespace)
                    .any(|token| equal(token, class, quirks))
            }),
            Self::Attribute(selector) => selector.matches(element),
            Self::Pseudo(pseudo) => pseudo.matches(document, node_id, element),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttributeOperator {
    Exists,
    Equals,
    Includes,
    DashMatch,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttributeSelector {
    name: String,
    operator: AttributeOperator,
    value: String,
    case_insensitive: Option<bool>,
}

impl AttributeSelector {
    fn matches(&self, element: &ElementData) -> bool {
        let Some(actual) = element.attribute(&self.name) else {
            return false;
        };
        let insensitive = self.case_insensitive.unwrap_or_else(|| {
            !element.has_case_sensitive_names()
                && html_case_insensitive_attribute(&self.name.to_ascii_lowercase())
        });
        let expected = self.value.as_str();
        let bytes = actual.as_bytes();
        let matches = |value: &[u8]| {
            if insensitive {
                value.eq_ignore_ascii_case(expected.as_bytes())
            } else {
                value == expected.as_bytes()
            }
        };
        match self.operator {
            AttributeOperator::Exists => true,
            AttributeOperator::Equals => matches(bytes),
            AttributeOperator::Includes => {
                !expected.is_empty()
                    && !expected.chars().any(css_whitespace)
                    && actual
                        .split(css_whitespace)
                        .any(|token| matches(token.as_bytes()))
            }
            AttributeOperator::DashMatch => {
                matches(bytes)
                    || bytes.get(..expected.len()).is_some_and(matches)
                        && bytes.get(expected.len()) == Some(&b'-')
            }
            AttributeOperator::Prefix => {
                !expected.is_empty() && bytes.get(..expected.len()).is_some_and(matches)
            }
            AttributeOperator::Suffix => {
                !expected.is_empty()
                    && bytes
                        .len()
                        .checked_sub(expected.len())
                        .and_then(|start| bytes.get(start..))
                        .is_some_and(matches)
            }
            AttributeOperator::Substring => {
                !expected.is_empty() && bytes.windows(expected.len()).any(matches)
            }
        }
    }
}

// HTML defines these value comparisons independently of the element's tag.
// https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-selectors
fn html_case_insensitive_attribute(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "accept-charset"
            | "align"
            | "alink"
            | "axis"
            | "bgcolor"
            | "charset"
            | "checked"
            | "clear"
            | "codetype"
            | "color"
            | "compact"
            | "declare"
            | "defer"
            | "dir"
            | "direction"
            | "disabled"
            | "enctype"
            | "face"
            | "frame"
            | "hreflang"
            | "http-equiv"
            | "lang"
            | "language"
            | "link"
            | "media"
            | "method"
            | "multiple"
            | "nohref"
            | "noresize"
            | "noshade"
            | "nowrap"
            | "readonly"
            | "rel"
            | "rev"
            | "rules"
            | "scope"
            | "scrolling"
            | "selected"
            | "shape"
            | "target"
            | "text"
            | "type"
            | "valign"
            | "valuetype"
            | "vlink"
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PseudoClass {
    Root,
    Empty,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Not(Vec<Selector>),
}

impl PseudoClass {
    fn matches(&self, document: &Document, node_id: NodeId, element: &ElementData) -> bool {
        let node = document.node(node_id).expect("compound matched an element");
        match self {
            Self::Root => node
                .parent()
                .and_then(|id| document.node(id))
                .is_some_and(|parent| matches!(parent.kind(), NodeKind::Document)),
            // Match shipping browser behavior: even whitespace text prevents :empty.
            Self::Empty => !node.children().iter().any(|&id| {
                document.node(id).is_some_and(|child| match child.kind() {
                    NodeKind::Element(_) => true,
                    NodeKind::Text(text) => !text.is_empty(),
                    _ => false,
                })
            }),
            Self::Not(selectors) => !selectors
                .iter()
                .any(|selector| selector.matches(document, node_id)),
            _ => {
                let of_type = matches!(
                    self,
                    Self::FirstOfType | Self::LastOfType | Self::OnlyOfType
                );
                let relevant = |id: &NodeId| {
                    document
                        .node(*id)
                        .and_then(|node| node.as_element())
                        .is_some_and(|sibling| {
                            !of_type
                                || sibling.namespace() == element.namespace()
                                    && equal(
                                        sibling.tag_name(),
                                        element.tag_name(),
                                        !element.has_case_sensitive_names(),
                                    )
                        })
                };
                let (before, after) = sibling_slices(document, node_id);
                let first = !before.iter().any(relevant);
                let last = !after.iter().any(relevant);
                match self {
                    Self::FirstChild | Self::FirstOfType => first,
                    Self::LastChild | Self::LastOfType => last,
                    Self::OnlyChild | Self::OnlyOfType => first && last,
                    _ => unreachable!("non-structural pseudo-classes handled above"),
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    compounds: Vec<Compound>,
    combinators: Vec<Combinator>,
    specificity: Specificity,
}

impl Selector {
    pub fn specificity(&self) -> Specificity {
        self.specificity
    }

    pub fn matches(&self, document: &Document, node_id: NodeId) -> bool {
        let mut pending = vec![(self.compounds.len() - 1, node_id)];
        let mut visited = HashSet::new();
        while let Some((index, id)) = pending.pop() {
            if !visited.insert((index, id)) || !self.compounds[index].matches(document, id) {
                continue;
            }
            if index == 0 {
                return true;
            }
            let node = document.node(id).expect("compound matched an element");
            match self.combinators[index - 1] {
                Combinator::Descendant => {
                    // Keep all candidates: the nearest match may fail a combinator
                    // farther left. Memoized states avoid exponential backtracking.
                    let mut ancestor = node.parent();
                    while let Some(id) = ancestor {
                        pending.push((index - 1, id));
                        ancestor = document.node(id).and_then(|node| node.parent());
                    }
                }
                Combinator::Child => {
                    if let Some(parent) = node.parent() {
                        pending.push((index - 1, parent));
                    }
                }
                Combinator::NextSibling | Combinator::SubsequentSibling => {
                    let (before, _) = sibling_slices(document, id);
                    for &sibling in before.iter().rev() {
                        if document
                            .node(sibling)
                            .is_some_and(|node| node.as_element().is_some())
                        {
                            pending.push((index - 1, sibling));
                            if self.combinators[index - 1] == Combinator::NextSibling {
                                break;
                            }
                        }
                    }
                }
            }
        }
        false
    }
}

fn sibling_slices(document: &Document, node_id: NodeId) -> (&[NodeId], &[NodeId]) {
    let siblings = document
        .node(node_id)
        .and_then(|node| node.parent())
        .and_then(|id| document.node(id))
        .map(|parent| parent.children())
        .unwrap_or(&[]);
    match siblings.iter().position(|&id| id == node_id) {
        Some(index) => (&siblings[..index], &siblings[index + 1..]),
        None => (&[], &[]),
    }
}

fn equal(actual: &str, expected: &str, insensitive: bool) -> bool {
    if insensitive {
        actual.eq_ignore_ascii_case(expected)
    } else {
        actual == expected
    }
}

fn css_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorError;

impl fmt::Display for SelectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid or unsupported selector, or selector nesting limit exceeded")
    }
}

impl Error for SelectorError {}

pub fn parse(source: &str) -> Result<Vec<Selector>, SelectorError> {
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let tokens = std::iter::from_fn(|| tokenizer.next_token())
        .map(|token| token.kind)
        .collect();
    if !tokenizer.errors().is_empty() || tokenizer.dropped_errors() != 0 {
        return Err(SelectorError);
    }
    Parser {
        tokens,
        position: 0,
    }
    .list(false, 0)
}

struct Parser {
    tokens: Vec<TokenKind>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.position]
    }

    fn whitespace(&mut self) -> bool {
        let start = self.position;
        while self.peek() == &TokenKind::Whitespace {
            self.position += 1;
        }
        self.position != start
    }

    fn consume(&mut self, kind: TokenKind) -> Result<(), SelectorError> {
        if self.peek() != &kind {
            return Err(SelectorError);
        }
        self.position += 1;
        Ok(())
    }

    fn ident(&mut self) -> Result<String, SelectorError> {
        let TokenKind::Ident(name) = self.peek() else {
            return Err(SelectorError);
        };
        let name = name.clone();
        self.position += 1;
        Ok(name)
    }

    fn list(&mut self, nested: bool, depth: usize) -> Result<Vec<Selector>, SelectorError> {
        if depth > MAX_SELECTOR_NESTING {
            return Err(SelectorError);
        }
        let mut selectors = Vec::new();
        loop {
            self.whitespace();
            selectors.push(self.selector(depth)?);
            match self.peek() {
                TokenKind::Comma => self.position += 1,
                TokenKind::CloseParen if nested => {
                    self.position += 1;
                    return Ok(selectors);
                }
                TokenKind::Eof if !nested => return Ok(selectors),
                _ => return Err(SelectorError),
            }
        }
    }

    fn selector(&mut self, depth: usize) -> Result<Selector, SelectorError> {
        let mut selector = Selector {
            compounds: Vec::new(),
            combinators: Vec::new(),
            specificity: Specificity::default(),
        };
        loop {
            let compound = self.compound(depth)?;
            if compound.tag.is_some() {
                selector.specificity.add(Specificity {
                    types: 1,
                    ..Specificity::default()
                });
            }
            for simple in &compound.simple {
                selector.specificity.add(simple.specificity());
            }
            selector.compounds.push(compound);
            let whitespace = self.whitespace();
            let combinator = match self.peek() {
                TokenKind::Delim('>') => Combinator::Child,
                TokenKind::Delim('+') => Combinator::NextSibling,
                TokenKind::Delim('~') => Combinator::SubsequentSibling,
                TokenKind::Comma | TokenKind::CloseParen | TokenKind::Eof => break,
                _ if whitespace => Combinator::Descendant,
                _ => return Err(SelectorError),
            };
            if combinator != Combinator::Descendant {
                self.position += 1;
                self.whitespace();
            }
            selector.combinators.push(combinator);
        }
        Ok(selector)
    }

    fn compound(&mut self, depth: usize) -> Result<Compound, SelectorError> {
        let start = self.position;
        let tag = match self.peek() {
            TokenKind::Ident(_) => Some(self.ident()?),
            TokenKind::Delim('*') => {
                self.position += 1;
                None
            }
            _ => None,
        };
        let mut simple = Vec::new();
        loop {
            let selector = match self.peek().clone() {
                TokenKind::Hash {
                    value,
                    kind: HashKind::Id,
                } => {
                    self.position += 1;
                    SimpleSelector::Id(value)
                }
                TokenKind::Delim('.') => {
                    self.position += 1;
                    SimpleSelector::Class(self.ident()?)
                }
                TokenKind::OpenSquare => SimpleSelector::Attribute(self.attribute()?),
                TokenKind::Colon => {
                    self.position += 1;
                    SimpleSelector::Pseudo(self.pseudo(depth)?)
                }
                _ => break,
            };
            simple.push(selector);
        }
        if self.position == start {
            return Err(SelectorError);
        }
        Ok(Compound { tag, simple })
    }

    fn attribute(&mut self) -> Result<AttributeSelector, SelectorError> {
        self.position += 1;
        self.whitespace();
        let name = self.ident()?;
        self.whitespace();
        if self.peek() == &TokenKind::CloseSquare {
            self.position += 1;
            return Ok(AttributeSelector {
                name,
                operator: AttributeOperator::Exists,
                value: String::new(),
                case_insensitive: None,
            });
        }
        let operator = match self.peek() {
            TokenKind::Delim('=') => AttributeOperator::Equals,
            TokenKind::Delim('~') => AttributeOperator::Includes,
            TokenKind::Delim('|') => AttributeOperator::DashMatch,
            TokenKind::Delim('^') => AttributeOperator::Prefix,
            TokenKind::Delim('$') => AttributeOperator::Suffix,
            TokenKind::Delim('*') => AttributeOperator::Substring,
            _ => return Err(SelectorError),
        };
        self.position += 1;
        if operator != AttributeOperator::Equals {
            self.consume(TokenKind::Delim('='))?;
        }
        self.whitespace();
        let value = match self.peek() {
            TokenKind::Ident(value) | TokenKind::String(value) => value.clone(),
            _ => return Err(SelectorError),
        };
        self.position += 1;
        self.whitespace();
        let case_insensitive = match self.peek() {
            TokenKind::Ident(flag) if flag.eq_ignore_ascii_case("i") => Some(true),
            TokenKind::Ident(flag) if flag.eq_ignore_ascii_case("s") => Some(false),
            _ => None,
        };
        if case_insensitive.is_some() {
            self.position += 1;
            self.whitespace();
        }
        self.consume(TokenKind::CloseSquare)?;
        Ok(AttributeSelector {
            name,
            operator,
            value,
            case_insensitive,
        })
    }

    fn pseudo(&mut self, depth: usize) -> Result<PseudoClass, SelectorError> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.position += 1;
                match name.to_ascii_lowercase().as_str() {
                    "root" => Ok(PseudoClass::Root),
                    "empty" => Ok(PseudoClass::Empty),
                    "first-child" => Ok(PseudoClass::FirstChild),
                    "last-child" => Ok(PseudoClass::LastChild),
                    "only-child" => Ok(PseudoClass::OnlyChild),
                    "first-of-type" => Ok(PseudoClass::FirstOfType),
                    "last-of-type" => Ok(PseudoClass::LastOfType),
                    "only-of-type" => Ok(PseudoClass::OnlyOfType),
                    _ => Err(SelectorError),
                }
            }
            TokenKind::Function(name) if name.eq_ignore_ascii_case("not") => {
                self.position += 1;
                Ok(PseudoClass::Not(self.list(true, depth + 1)?))
            }
            _ => Err(SelectorError),
        }
    }
}

#[cfg(test)]
mod tests;
