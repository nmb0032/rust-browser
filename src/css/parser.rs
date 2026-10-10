use std::{error::Error, fmt};

use super::{
    ast::{AtRule, BlockItem, Declaration, QualifiedRule, Rule, Stylesheet},
    tokenizer::{MAX_PARSE_ERRORS, Token, TokenKind, Tokenizer},
};

pub const MAX_RULE_NESTING: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssErrorKind {
    ExpectedAtRuleName,
    ExpectedDeclarationColon,
    ExpectedQualifiedRuleBlock,
    EmptyPropertyName,
    EmptyQualifiedRulePrelude,
    ExpectedAtRuleTerminator,
    InvalidPropertyName,
    InvalidDeclarationValue,
    InvalidQualifiedRulePrelude,
    InvalidAtRulePrelude,
    UnexpectedClosingBrace,
    UnterminatedBlock,
    UnterminatedComment,
    UnterminatedString,
    UnbalancedDelimiter(char),
    Tokenization(&'static str),
    NestingLimitExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssError {
    kind: CssErrorKind,
    position: usize,
}

impl CssError {
    fn new(kind: CssErrorKind, position: usize) -> Self {
        Self { kind, position }
    }

    pub fn kind(&self) -> CssErrorKind {
        self.kind
    }

    pub fn position(&self) -> usize {
        self.position
    }
}

impl fmt::Display for CssError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "CSS parse error {:?} at byte {}",
            self.kind, self.position
        )
    }
}

impl Error for CssError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseOutput<T> {
    pub value: T,
    pub errors: Vec<CssError>,
    pub dropped_errors: usize,
}

impl<T> ParseOutput<T> {
    fn into_result(self) -> Result<T, CssError> {
        match self.errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(self.value),
        }
    }
}

/// Strict compatibility entry point. Use `parse_recovering` for author CSS.
pub fn parse(source: &str) -> Result<Stylesheet, CssError> {
    parse_recovering(source).into_result()
}

/// Strict compatibility entry point for callers that require valid syntax.
pub fn parse_declarations(source: &str) -> Result<Vec<Declaration>, CssError> {
    parse_declarations_recovering(source).into_result()
}

/// Parse CSS Syntax rules, retaining recoverable output and lexical/syntax errors.
pub fn parse_recovering(source: &str) -> ParseOutput<Stylesheet> {
    let mut parser = Parser::new(source);
    let mut rules = Vec::new();
    let end = parser.tokens.len() - 1;
    while parser.position < end {
        match parser.kind(parser.position) {
            TokenKind::Whitespace | TokenKind::Cdo | TokenKind::Cdc => parser.position += 1,
            TokenKind::AtKeyword(_) => {
                if let Some(rule) = parser.at_rule(end, false, 0) {
                    rules.push(rule);
                }
            }
            _ => {
                if let Some(rule) = parser.qualified_rule(end, false, 0) {
                    rules.push(rule);
                }
            }
        }
    }
    parser.finish(Stylesheet::new(rules))
}

/// Parse an inline declaration list without treating braces as an enclosing block.
pub fn parse_declarations_recovering(source: &str) -> ParseOutput<Vec<Declaration>> {
    let mut parser = Parser::new(source);
    let end = parser.tokens.len() - 1;
    let mut declarations = Vec::new();
    while parser.position < end {
        match parser.kind(parser.position) {
            TokenKind::Whitespace | TokenKind::Semicolon => parser.position += 1,
            TokenKind::AtKeyword(_) => {
                let position = parser.position;
                parser.at_rule(end, false, 0);
                parser.error(CssErrorKind::InvalidPropertyName, position);
            }
            _ => {
                let start = parser.position;
                let boundary = parser.declaration_end(start, end);
                if let Some(declaration) = parser.declaration(start, boundary) {
                    declarations.push(declaration);
                }
                parser.position = boundary;
            }
        }
    }
    parser.finish(declarations)
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    next_component: Vec<usize>,
    closed_components: Vec<bool>,
    invalid_prefix: Vec<usize>,
    position: usize,
    errors: Vec<CssError>,
    dropped_errors: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        let mut tokenizer = Tokenizer::default();
        tokenizer.feed(source);
        tokenizer.finish();
        let tokens: Vec<_> = std::iter::from_fn(|| tokenizer.next_token()).collect();
        let mut parser = Self {
            source,
            next_component: (1..=tokens.len()).collect(),
            closed_components: vec![false; tokens.len()],
            invalid_prefix: Vec::with_capacity(tokens.len() + 1),
            tokens,
            position: 0,
            errors: tokenizer
                .errors()
                .iter()
                .map(|error| {
                    let kind = match error.code {
                        "eof-in-comment" => CssErrorKind::UnterminatedComment,
                        "eof-in-string" => CssErrorKind::UnterminatedString,
                        code => CssErrorKind::Tokenization(code),
                    };
                    CssError::new(kind, error.position.byte_offset)
                })
                .collect(),
            dropped_errors: tokenizer.dropped_errors(),
        };
        // Index components iteratively so deep functions/blocks cannot overflow
        // the stack, and declaration/rule lookahead can skip each in constant time.
        let mut stack = Vec::new();
        parser.invalid_prefix.push(0);
        for index in 0..parser.tokens.len() - 1 {
            let mut invalid =
                matches!(parser.kind(index), TokenKind::BadString | TokenKind::BadUrl);
            if closing(parser.kind(index)).is_some() {
                stack.push(index);
            } else if matches!(
                parser.kind(index),
                TokenKind::CloseParen | TokenKind::CloseSquare | TokenKind::CloseCurly
            ) {
                if stack.last().is_some_and(|&open| {
                    closing(parser.kind(open)).as_ref() == Some(parser.kind(index))
                }) {
                    let open = stack.pop().expect("matching opener");
                    parser.next_component[open] = index + 1;
                    parser.closed_components[open] = true;
                } else {
                    invalid = true;
                }
            }
            parser
                .invalid_prefix
                .push(parser.invalid_prefix[index] + usize::from(invalid));
        }
        let eof = parser.tokens.len() - 1;
        while let Some(open) = stack.pop() {
            parser.next_component[open] = eof;
            let kind = match parser.kind(open) {
                TokenKind::OpenCurly => CssErrorKind::UnterminatedBlock,
                TokenKind::OpenSquare => CssErrorKind::UnbalancedDelimiter('['),
                _ => CssErrorKind::UnbalancedDelimiter('('),
            };
            parser.error(kind, eof);
        }
        parser
    }

    fn finish<T>(self, value: T) -> ParseOutput<T> {
        ParseOutput {
            value,
            errors: self.errors,
            dropped_errors: self.dropped_errors,
        }
    }

    fn kind(&self, index: usize) -> &TokenKind {
        &self.tokens[index].kind
    }

    fn error(&mut self, kind: CssErrorKind, index: usize) {
        let error = CssError::new(kind, self.tokens[index].span.start.byte_offset);
        let insertion = self
            .errors
            .partition_point(|existing| existing.position() <= error.position());
        if self.errors.len() < MAX_PARSE_ERRORS {
            self.errors.insert(insertion, error);
        } else {
            self.dropped_errors += 1;
            if insertion < MAX_PARSE_ERRORS {
                self.errors.insert(insertion, error);
                self.errors.pop();
            }
        }
    }

    fn skip_whitespace(&self, mut index: usize, end: usize) -> usize {
        while index < end && self.kind(index) == &TokenKind::Whitespace {
            index += 1;
        }
        index
    }

    fn text(&self, start: usize, end: usize) -> String {
        self.source_text(
            self.tokens[start].span.start.byte_offset,
            self.tokens[end].span.start.byte_offset,
        )
    }

    fn source_text(&self, start: usize, end: usize) -> String {
        self.source[start..end]
            .trim_matches(css_whitespace)
            .to_string()
    }

    fn invalid(&self, start: usize, end: usize) -> bool {
        self.invalid_prefix[end] != self.invalid_prefix[start]
    }

    fn at_rule(&mut self, end: usize, nested: bool, depth: usize) -> Option<Rule> {
        let start = self.position;
        let TokenKind::AtKeyword(name) = self.kind(start) else {
            unreachable!("at-rule starts with an at-keyword");
        };
        let name = name.clone();
        self.position += 1;
        let prelude_start = self.position;
        while self.position < end {
            match self.kind(self.position) {
                TokenKind::Semicolon => break,
                TokenKind::CloseCurly if nested => break,
                TokenKind::OpenCurly => {
                    let prelude_end = self.position;
                    let prelude = self.source_text(
                        self.tokens[start].span.end.byte_offset,
                        self.tokens[prelude_end].span.start.byte_offset,
                    );
                    let invalid = self.invalid(prelude_start, prelude_end);
                    if invalid {
                        self.position = self.next_component[self.position];
                        self.error(CssErrorKind::InvalidAtRulePrelude, prelude_start);
                        return None;
                    }
                    let block = self.block(end, depth);
                    return block.map(|block| {
                        Rule::At(AtRule {
                            name,
                            prelude,
                            block: Some(block),
                        })
                    });
                }
                _ => self.position = self.next_component[self.position],
            }
        }
        let prelude_end = self.position;
        if self.position < end && self.kind(self.position) == &TokenKind::Semicolon {
            self.position += 1;
        }
        if self.invalid(prelude_start, prelude_end) {
            self.error(CssErrorKind::InvalidAtRulePrelude, prelude_start);
            return None;
        }
        Some(Rule::At(AtRule {
            name,
            prelude: self.source_text(
                self.tokens[start].span.end.byte_offset,
                self.tokens[prelude_end].span.start.byte_offset,
            ),
            block: None,
        }))
    }

    fn qualified_rule(&mut self, end: usize, nested: bool, depth: usize) -> Option<Rule> {
        let start = self.position;
        while self.position < end {
            match self.kind(self.position) {
                TokenKind::Semicolon if nested => break,
                TokenKind::OpenCurly => {
                    let prelude_end = self.position;
                    let empty = self.skip_whitespace(start, prelude_end) == prelude_end;
                    let invalid = self.invalid(start, prelude_end)
                        || self
                            .property_header(start, prelude_end)
                            .is_ok_and(|(name, _)| name.starts_with("--"));
                    if empty || invalid {
                        self.position = self.next_component[self.position];
                        self.error(
                            if empty {
                                CssErrorKind::EmptyQualifiedRulePrelude
                            } else {
                                CssErrorKind::InvalidQualifiedRulePrelude
                            },
                            start,
                        );
                        return None;
                    }
                    let prelude = self.text(start, prelude_end);
                    return self
                        .block(end, depth)
                        .map(|items| Rule::Qualified(QualifiedRule { prelude, items }));
                }
                _ => self.position = self.next_component[self.position],
            }
        }
        self.error(CssErrorKind::ExpectedQualifiedRuleBlock, self.position);
        None
    }

    fn block(&mut self, end: usize, depth: usize) -> Option<Vec<BlockItem>> {
        let open = self.position;
        let next = self.next_component[open];
        let block_end = if self.closed_components[open] {
            next - 1
        } else {
            end
        };
        if depth >= MAX_RULE_NESTING {
            self.error(CssErrorKind::NestingLimitExceeded, open);
            self.position = next;
            return None;
        }
        self.position += 1;
        let mut items = Vec::new();
        while self.position < block_end {
            match self.kind(self.position) {
                TokenKind::Whitespace | TokenKind::Semicolon => self.position += 1,
                TokenKind::AtKeyword(_) => {
                    if let Some(rule) = self.at_rule(block_end, true, depth + 1) {
                        items.push(BlockItem::Rule(rule));
                    }
                }
                _ => {
                    let start = self.position;
                    let mut boundary = start;
                    while boundary < block_end
                        && !matches!(
                            self.kind(boundary),
                            TokenKind::Semicolon | TokenKind::OpenCurly
                        )
                    {
                        boundary = self.next_component[boundary];
                    }
                    if boundary < block_end && self.kind(boundary) == &TokenKind::OpenCurly {
                        if self.brace_is_declaration(start, boundary, block_end) {
                            boundary = self.declaration_end(boundary, block_end);
                        } else {
                            if let Some(rule) = self.qualified_rule(block_end, true, depth + 1) {
                                items.push(BlockItem::Rule(rule));
                            }
                            continue;
                        }
                    }
                    if let Some(declaration) = self.declaration(start, boundary) {
                        items.push(BlockItem::Declaration(declaration));
                    }
                    self.position = boundary;
                }
            }
        }
        self.position = next;
        Some(items)
    }

    fn property_header(&self, start: usize, end: usize) -> Result<(&str, usize), CssErrorKind> {
        let TokenKind::Ident(name) = self.kind(start) else {
            return Err(if self.kind(start) == &TokenKind::Colon {
                CssErrorKind::EmptyPropertyName
            } else {
                CssErrorKind::InvalidPropertyName
            });
        };
        if name == "--" {
            return Err(CssErrorKind::InvalidPropertyName);
        }
        let colon = self.skip_whitespace(start + 1, end);
        if colon >= end || self.kind(colon) != &TokenKind::Colon {
            return Err(CssErrorKind::ExpectedDeclarationColon);
        }
        Ok((name, colon))
    }

    fn brace_is_declaration(&self, start: usize, open: usize, end: usize) -> bool {
        let Ok((name, colon)) = self.property_header(start, open) else {
            return false;
        };
        if name.starts_with("--") {
            return true;
        }
        if self.skip_whitespace(colon + 1, open) != open {
            return false;
        }
        let mut after = self.skip_whitespace(self.next_component[open], end);
        if after < end && self.kind(after) == &TokenKind::Delim('!') {
            after = self.skip_whitespace(after + 1, end);
            if !matches!(self.kind(after), TokenKind::Ident(name) if name.eq_ignore_ascii_case("important"))
            {
                return false;
            }
            after = self.skip_whitespace(after + 1, end);
        }
        after == end || self.kind(after) == &TokenKind::Semicolon
    }

    fn declaration_end(&self, mut index: usize, end: usize) -> usize {
        while index < end && self.kind(index) != &TokenKind::Semicolon {
            index = self.next_component[index];
        }
        index
    }

    fn declaration(&mut self, start: usize, end: usize) -> Option<Declaration> {
        let (name, colon) = match self.property_header(start, end) {
            Ok(header) => header,
            Err(kind) => {
                self.error(kind, start);
                return None;
            }
        };
        let property = name.to_string();
        let value_start = colon + 1;
        if self.invalid(value_start, end) {
            self.error(CssErrorKind::InvalidDeclarationValue, value_start);
            return None;
        }
        let mut previous = None;
        let mut last = None;
        let mut index = value_start;
        while index < end {
            if self.kind(index) != &TokenKind::Whitespace {
                previous = last;
                last = Some(index);
            }
            index = self.next_component[index];
        }
        let important_start = previous.zip(last).and_then(|(bang, ident)| {
            (self.kind(bang) == &TokenKind::Delim('!')
                && matches!(self.kind(ident), TokenKind::Ident(name) if name.eq_ignore_ascii_case("important")))
                .then_some(bang)
        });
        let value_end = important_start.unwrap_or(end);
        if !property.starts_with("--") {
            let mut index = value_start;
            let mut values = 0;
            let mut curly = false;
            while index < value_end {
                if self.kind(index) != &TokenKind::Whitespace {
                    values += 1;
                    curly |= self.kind(index) == &TokenKind::OpenCurly;
                }
                index = self.next_component[index];
            }
            if curly && values != 1 {
                self.error(CssErrorKind::InvalidDeclarationValue, value_start);
                return None;
            }
        }
        Some(Declaration {
            property,
            value: self.source_text(
                self.tokens[colon].span.end.byte_offset,
                self.tokens[value_end].span.start.byte_offset,
            ),
            important: important_start.is_some(),
        })
    }
}

fn closing(kind: &TokenKind) -> Option<TokenKind> {
    match kind {
        TokenKind::OpenCurly => Some(TokenKind::CloseCurly),
        TokenKind::OpenSquare => Some(TokenKind::CloseSquare),
        TokenKind::OpenParen | TokenKind::Function(_) => Some(TokenKind::CloseParen),
        _ => None,
    }
}

fn css_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000c}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qualified(sheet: &Stylesheet, index: usize) -> &QualifiedRule {
        let Rule::Qualified(rule) = &sheet.rules()[index] else {
            panic!("expected a qualified rule");
        };
        rule
    }

    fn declarations(rule: &QualifiedRule) -> Vec<&Declaration> {
        rule.declarations().collect()
    }

    #[test]
    fn parses_qualified_rules_and_declarations() {
        let stylesheet = parse("h1, p { color: red; font-size: 16px }").unwrap();
        assert_eq!(stylesheet.rules().len(), 1);
        let rule = qualified(&stylesheet, 0);
        assert_eq!(rule.prelude(), "h1, p");
        let declarations = declarations(rule);
        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].property(), "color");
        assert_eq!(declarations[0].value(), "red");
        assert!(!declarations[0].is_important());
        assert_eq!(declarations[1].property(), "font-size");
        assert_eq!(declarations[1].value(), "16px");
    }

    #[test]
    fn keeps_delimiters_inside_strings_urls_functions_and_comments() {
        let stylesheet = parse(
            r#"/* header */ :is(h1, [lang="en; }"]) {
                content: "a;b}";
                background-image: url("data:image/svg+xml;a,b");
                other: url(data:foo;a{b}c);
                font-family: "café;";
                color: red /* ; } */;
            }"#,
        )
        .unwrap();
        let rule = qualified(&stylesheet, 0);
        assert_eq!(rule.prelude(), r#":is(h1, [lang="en; }"])"#);
        let declarations = declarations(rule);
        assert_eq!(declarations.len(), 5);
        assert_eq!(declarations[0].value(), r#""a;b}""#);
        assert_eq!(declarations[1].value(), r#"url("data:image/svg+xml;a,b")"#);
        assert_eq!(declarations[2].value(), "url(data:foo;a{b}c)");
        assert_eq!(declarations[3].value(), r#""café;""#);
        assert!(declarations[4].value().contains("/* ; } */"));
    }

    #[test]
    fn important_uses_the_last_two_top_level_non_whitespace_tokens() {
        let source = r#"content: "!important"; a: f(!important); b: red ! IMPORTANT;
            c: red !/**/ImPoRtAnT/**/; d: red !\69mportant; e: !important red"#;
        let parsed = parse_declarations(source).unwrap();
        assert_eq!(parsed.len(), 6);
        assert_eq!(parsed[0].value(), r#""!important""#);
        assert!(!parsed[0].is_important());
        assert!(!parsed[1].is_important());
        for declaration in &parsed[2..5] {
            assert_eq!(declaration.value(), "red");
            assert!(declaration.is_important());
        }
        assert!(!parsed[5].is_important());
    }

    #[test]
    fn decodes_property_and_at_rule_names_but_preserves_other_source_text() {
        let parsed = parse(r"@\6d edia screen { p { \63 olor/**/: r\65 d; --\78 : 1; } }").unwrap();
        let Rule::At(rule) = &parsed.rules()[0] else {
            panic!("expected at-rule");
        };
        assert_eq!(rule.name(), "media");
        let Some([BlockItem::Rule(Rule::Qualified(nested))]) = rule.block() else {
            panic!("expected nested rule");
        };
        let declarations = declarations(nested);
        assert_eq!(declarations[0].property(), "color");
        assert_eq!(declarations[0].value(), r"r\65 d");
        assert_eq!(declarations[1].property(), "--x");
    }

    #[test]
    fn preserves_custom_property_blocks_and_nested_rule_order() {
        let stylesheet = parse(
            "article { --theme: { color: red; } other { nested: yes; };\
             &:hover { color: blue; } color: green; p:hover { color: pink } }",
        )
        .unwrap();
        let rule = qualified(&stylesheet, 0);
        assert_eq!(rule.items().len(), 4);
        let BlockItem::Declaration(custom_property) = &rule.items()[0] else {
            panic!("expected a custom property declaration");
        };
        assert_eq!(custom_property.property(), "--theme");
        assert_eq!(
            custom_property.value(),
            "{ color: red; } other { nested: yes; }"
        );
        let BlockItem::Rule(Rule::Qualified(nested_rule)) = &rule.items()[1] else {
            panic!("expected a nested qualified rule");
        };
        assert_eq!(nested_rule.prelude(), "&:hover");
        assert_eq!(declarations(nested_rule)[0].value(), "blue");
        assert!(matches!(&rule.items()[2], BlockItem::Declaration(_)));
        let BlockItem::Rule(Rule::Qualified(nested_rule)) = &rule.items()[3] else {
            panic!("expected type-prefixed nested rule");
        };
        assert_eq!(nested_rule.prelude(), "p:hover");
    }

    #[test]
    fn parses_at_rules_with_nested_rules_and_declarations() {
        let stylesheet = parse(
            r#"@import url("theme.css");
               @media (min-width: 40rem) {
                   @supports (display: grid) { .card:hover { color: blue; } }
               }
               @font-face { font-family: "Demo"; src: url("font.woff2"); }"#,
        )
        .unwrap();
        assert_eq!(stylesheet.rules().len(), 3);
        let Rule::At(import) = &stylesheet.rules()[0] else {
            panic!("expected an at-rule");
        };
        assert_eq!(import.name(), "import");
        assert!(import.block().is_none());
        let Rule::At(media) = &stylesheet.rules()[1] else {
            panic!("expected an at-rule");
        };
        let Some([BlockItem::Rule(Rule::At(supports))]) = media.block() else {
            panic!("expected a nested supports at-rule");
        };
        let Some([BlockItem::Rule(Rule::Qualified(card))]) = supports.block() else {
            panic!("expected a nested qualified rule");
        };
        assert_eq!(card.prelude(), ".card:hover");
        assert_eq!(declarations(card)[0].value(), "blue");
        let Rule::At(font_face) = &stylesheet.rules()[2] else {
            panic!("expected an at-rule");
        };
        let Some([BlockItem::Declaration(family), BlockItem::Declaration(src)]) = font_face.block()
        else {
            panic!("expected font-face declarations");
        };
        assert_eq!(family.property(), "font-family");
        assert_eq!(src.property(), "src");
    }

    #[test]
    fn accepts_empty_stylesheets_trivia_and_top_level_html_comments() {
        for source in ["", " /* only a comment */ \n ", "<!-- -->"] {
            assert!(parse(source).unwrap().rules().is_empty(), "{source:?}");
        }
        let sheet = parse("<!-- p { ; color: red; } --> h1 { color:blue }").unwrap();
        assert_eq!(sheet.rules().len(), 2);
        assert_eq!(qualified(&sheet, 0).prelude(), "p");
        assert_eq!(qualified(&sheet, 1).prelude(), "h1");
    }

    #[test]
    fn malformed_declarations_drop_only_themselves() {
        let parsed = parse_recovering(
            "p { color:red; missing colon; :empty; 123:bad; co/**/lor:bad;\
             width:1px; color:blue); padding:2px; } h1 { color:green }",
        );
        assert_eq!(parsed.errors.len(), 5);
        assert_eq!(parsed.value.rules().len(), 2);
        let declarations = declarations(qualified(&parsed.value, 0));
        assert_eq!(
            declarations
                .iter()
                .map(|declaration| declaration.property())
                .collect::<Vec<_>>(),
            ["color", "width", "padding"]
        );
        assert_eq!(declarations[0].value(), "red");
        assert_eq!(declarations[2].value(), "2px");
        assert_eq!(qualified(&parsed.value, 1).prelude(), "h1");
    }

    #[test]
    fn malformed_rules_drop_their_block_not_following_rules() {
        let parsed = parse_recovering(
            "{} } bad { color:red } --custom:hover { color:pink }\
             p { color:blue; broken; @media screen { { bad:x } h1 {color:green} } }",
        );
        assert_eq!(parsed.errors.len(), 5);
        assert_eq!(parsed.value.rules().len(), 1);
        let rule = qualified(&parsed.value, 0);
        assert_eq!(rule.prelude(), "p");
        assert_eq!(declarations(rule)[0].value(), "blue");
        let BlockItem::Rule(Rule::At(media)) = &rule.items()[1] else {
            panic!("expected recovered at-rule");
        };
        let Some([BlockItem::Rule(Rule::Qualified(h1))]) = media.block() else {
            panic!("expected surviving nested rule");
        };
        assert_eq!(h1.prelude(), "h1");
    }

    #[test]
    fn bad_strings_and_urls_do_not_discard_later_declarations_or_rules() {
        let parsed = parse_recovering(
            "p { content:\"bad\n; color:red; image:url(bad\";ignored); width:2px }\
             h1 { color:blue }",
        );
        assert_eq!(parsed.errors.len(), 4);
        let declarations = declarations(qualified(&parsed.value, 0));
        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].property(), "color");
        assert_eq!(declarations[1].property(), "width");
        assert_eq!(parsed.value.rules().len(), 2);
    }

    #[test]
    fn invalid_at_rule_preludes_skip_their_body_and_keep_following_rules() {
        let parsed = parse_recovering("@unknown \"bad\n { broken; p { bad } } h1 {color:red}");
        assert_eq!(parsed.errors.len(), 2);
        assert_eq!(parsed.value.rules().len(), 1);
        assert_eq!(qualified(&parsed.value, 0).prelude(), "h1");
        let parsed = parse("@unknown/**/ tokens; p {color:/**/red/**/}").unwrap();
        let Rule::At(rule) = &parsed.rules()[0] else {
            panic!("expected at-rule");
        };
        assert_eq!(rule.prelude(), "/**/ tokens");
        assert_eq!(
            declarations(qualified(&parsed, 1))[0].value(),
            "/**/red/**/"
        );
    }

    #[test]
    fn inline_recovery_consumes_entire_invalid_components_and_at_rules() {
        let parsed = parse_declarations_recovering(
            "color:red; nope x([; {fake:yes}]); @media screen {fake:yes}\
             width:2px; background:bad]; padding:3px",
        );
        assert_eq!(parsed.errors.len(), 3);
        assert_eq!(
            parsed
                .value
                .iter()
                .map(Declaration::property)
                .collect::<Vec<_>>(),
            ["color", "width", "padding"]
        );
        assert!(parse_declarations("color red").is_err());
        let parsed = parse_declarations_recovering("color:red} p {color:blue}; width:1px");
        assert_eq!(parsed.value.len(), 1);
        assert_eq!(parsed.value[0].property(), "width");
    }

    #[test]
    fn eof_recovers_blocks_final_declarations_and_at_rule_terminators() {
        let parsed = parse_recovering("p {color:red; width:2px");
        assert_eq!(parsed.errors[0].kind(), CssErrorKind::UnterminatedBlock);
        assert_eq!(declarations(qualified(&parsed.value, 0)).len(), 2);
        let parsed = parse("@import url(theme.css)").unwrap();
        assert!(matches!(&parsed.rules()[0], Rule::At(rule) if rule.block().is_none()));
        let parsed = parse("@media screen { @unknown tokens } p {color:red}").unwrap();
        assert_eq!(parsed.rules().len(), 2);
        let parsed = parse_recovering("p { color:rgb(1, 2; width:3px; h1 {color:red}");
        assert!(!parsed.errors.is_empty());
        assert_eq!(parsed.value.rules().len(), 1);
        assert_eq!(declarations(qualified(&parsed.value, 0)).len(), 1);
        assert!(
            declarations(qualified(&parsed.value, 0))[0]
                .value()
                .contains("h1")
        );
    }

    #[test]
    fn semicolons_stop_nested_rules_but_not_top_level_qualified_preludes() {
        let sheet = parse("broken; p { color:red } h1 {color:blue}").unwrap();
        assert_eq!(qualified(&sheet, 0).prelude(), "broken; p");
        let parsed = parse_recovering("p { broken; h1 {color:blue} width:1px }");
        assert_eq!(parsed.errors.len(), 1);
        let rule = qualified(&parsed.value, 0);
        assert_eq!(rule.items().len(), 2);
        assert!(
            matches!(&rule.items()[0], BlockItem::Rule(Rule::Qualified(rule)) if rule.prelude() == "h1")
        );
        assert_eq!(declarations(rule)[0].property(), "width");
    }

    #[test]
    fn non_custom_curly_values_must_be_the_entire_value() {
        let parsed = parse_declarations_recovering(
            "a: {nested:yes} ! important; b: x {nested:yes};\
             c: {nested:yes} x; --d: x {nested:yes} y; width:2px",
        );
        assert_eq!(parsed.errors.len(), 2);
        assert_eq!(
            parsed
                .value
                .iter()
                .map(Declaration::property)
                .collect::<Vec<_>>(),
            ["a", "--d", "width"]
        );
        assert!(parsed.value[0].is_important());
        let parsed = parse("p { a:{x:y}; --b:; width:2px }").unwrap();
        assert_eq!(declarations(qualified(&parsed, 0)).len(), 3);
    }

    #[test]
    fn diagnostics_preserve_original_utf8_offsets_and_are_bounded() {
        let source = "/* café\r\n */ p {color:red; broken; width:1px}";
        let parsed = parse_recovering(source);
        assert_eq!(parsed.errors.len(), 1);
        assert_eq!(parsed.errors[0].position(), source.find("broken").unwrap());
        let parsed = parse_declarations_recovering(&format!(
            "{}color:red",
            "broken;".repeat(MAX_PARSE_ERRORS + 3)
        ));
        assert_eq!(parsed.errors.len(), MAX_PARSE_ERRORS);
        assert_eq!(parsed.dropped_errors, 3);
        assert_eq!(parsed.value.len(), 1);
        let parsed = parse_declarations_recovering(&format!(
            "{}color:red",
            "bad:\"x\n;".repeat(MAX_PARSE_ERRORS)
        ));
        assert_eq!(parsed.errors.len(), MAX_PARSE_ERRORS);
        assert_eq!(parsed.dropped_errors, MAX_PARSE_ERRORS);
        assert_eq!(parsed.value[0].value(), "red");
        let source = format!("broken; {}color:red", "bad:\"x\n;".repeat(MAX_PARSE_ERRORS));
        let parsed = parse_declarations_recovering(&source);
        assert_eq!(parsed.errors.len(), MAX_PARSE_ERRORS);
        assert_eq!(parsed.errors[0].position(), 0);
        assert_eq!(
            parsed.errors[0].kind(),
            CssErrorKind::ExpectedDeclarationColon
        );
        assert_eq!(parsed.dropped_errors, MAX_PARSE_ERRORS + 1);
        assert_eq!(parse_declarations(&source).unwrap_err(), parsed.errors[0]);
    }

    #[test]
    fn deep_components_and_overdeep_rules_recover_without_stack_overflow() {
        let count = 10_000;
        let source = format!(
            "p {{ --deep:{}x{}; color:red }}",
            "f([".repeat(count),
            "])".repeat(count)
        );
        let parsed = parse(&source).unwrap();
        assert_eq!(declarations(qualified(&parsed, 0)).len(), 2);
        let source = format!(
            "{}p {{ color:red }}{} h1 {{color:blue}}",
            "@media all {".repeat(MAX_RULE_NESTING + 10),
            "}".repeat(MAX_RULE_NESTING + 10)
        );
        let parsed = parse_recovering(&source);
        assert_eq!(parsed.errors.len(), 1);
        assert_eq!(parsed.errors[0].kind(), CssErrorKind::NestingLimitExceeded);
        assert_eq!(parsed.value.rules().len(), 2);
        assert_eq!(qualified(&parsed.value, 1).prelude(), "h1");
    }

    #[test]
    fn strict_wrappers_still_surface_lexical_and_syntax_errors() {
        for (source, kind) in [
            ("h1;", CssErrorKind::ExpectedQualifiedRuleBlock),
            ("p { color red; }", CssErrorKind::ExpectedDeclarationColon),
            ("/* unfinished", CssErrorKind::UnterminatedComment),
            (
                "p { content: \"unfinished",
                CssErrorKind::UnterminatedString,
            ),
        ] {
            assert_eq!(parse(source).unwrap_err().kind(), kind, "{source}");
        }
    }

    #[test]
    fn malformed_token_combinations_terminate_with_positioned_diagnostics() {
        let fragments = [
            "",
            " ",
            "p",
            "{",
            "}",
            "[",
            "]",
            "(",
            ")",
            ";",
            ":",
            "@media ",
            "url(",
            "f(",
            "\"",
            "\n",
            "/*",
            "*/",
            "!important",
            "--x:",
            "\\",
            "\0",
            "é",
        ];
        for left in fragments {
            for right in fragments {
                let input = format!("{left}{right}; width:2px");
                let inline = parse_declarations_recovering(&input);
                let stylesheet = format!("p {{color:red; {input}}} h1 {{color:blue}}");
                let sheet = parse_recovering(&stylesheet);
                for (errors, source) in [(&inline.errors, &input), (&sheet.errors, &stylesheet)] {
                    assert!(errors.len() <= MAX_PARSE_ERRORS);
                    assert!(
                        errors.iter().all(|error| {
                            error.position() <= source.len()
                                && source.is_char_boundary(error.position())
                        }),
                        "{source:?}: {errors:?}"
                    );
                    assert!(
                        errors
                            .windows(2)
                            .all(|pair| pair[0].position() <= pair[1].position())
                    );
                }
            }
        }
    }
}
