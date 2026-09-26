use std::{error::Error, fmt};

use super::ast::{AtRule, BlockItem, Declaration, QualifiedRule, Rule, Stylesheet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssErrorKind {
    ExpectedAtRuleName,
    ExpectedDeclarationColon,
    ExpectedQualifiedRuleBlock,
    EmptyPropertyName,
    EmptyQualifiedRulePrelude,
    ExpectedAtRuleTerminator,
    InvalidPropertyName,
    UnexpectedClosingBrace,
    UnterminatedBlock,
    UnterminatedComment,
    UnterminatedString,
    UnbalancedDelimiter(char),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Boundary {
    position: usize,
    delimiter: Option<char>,
}

struct Parser<'a> {
    source: &'a str,
    position: usize,
}

pub fn parse(source: &str) -> Result<Stylesheet, CssError> {
    Parser::new(source).parse_stylesheet()
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    fn parse_stylesheet(mut self) -> Result<Stylesheet, CssError> {
        let mut rules = Vec::new();

        loop {
            self.skip_trivia()?;
            match self.peek_char() {
                None => return Ok(Stylesheet::new(rules)),
                Some('}') => {
                    return Err(self.error(CssErrorKind::UnexpectedClosingBrace, self.position));
                }
                Some('@') => rules.push(self.parse_at_rule()?),
                Some(_) => rules.push(self.parse_qualified_rule()?),
            }
        }
    }

    fn parse_at_rule(&mut self) -> Result<Rule, CssError> {
        let at_position = self.position;
        self.advance_char();

        let name_start = self.position;
        while let Some(ch) = self.peek_char() {
            if is_css_name_char(ch) || ch == '\\' {
                self.advance_char();
                if ch == '\\' && self.peek_char().is_some() {
                    self.advance_char();
                }
            } else {
                break;
            }
        }

        let name = self.source[name_start..self.position].to_string();
        if name.is_empty() {
            return Err(self.error(CssErrorKind::ExpectedAtRuleName, at_position));
        }

        let boundary =
            self.scan_boundary(self.position, self.source.len(), &[';', '{', '}'], false)?;
        let prelude = self.source[self.position..boundary.position]
            .trim()
            .to_string();

        match boundary.delimiter {
            Some(';') => {
                self.position = boundary.position + 1;
                Ok(Rule::At(AtRule {
                    name,
                    prelude,
                    block: None,
                }))
            }
            Some('{') => {
                self.position = boundary.position + 1;
                let block = self.parse_block_items()?;
                Ok(Rule::At(AtRule {
                    name,
                    prelude,
                    block: Some(block),
                }))
            }
            Some('}') => Err(self.error(CssErrorKind::UnexpectedClosingBrace, boundary.position)),
            None => Err(self.error(CssErrorKind::ExpectedAtRuleTerminator, self.source.len())),
            _ => unreachable!("only at-rule delimiters are scanned"),
        }
    }

    fn parse_qualified_rule(&mut self) -> Result<Rule, CssError> {
        let prelude_start = self.position;
        let boundary =
            self.scan_boundary(self.position, self.source.len(), &['{', ';', '}'], false)?;

        match boundary.delimiter {
            Some('{') => {
                if self.is_trivia_only(prelude_start, boundary.position)? {
                    return Err(self.error(CssErrorKind::EmptyQualifiedRulePrelude, prelude_start));
                }

                let prelude = self.source[prelude_start..boundary.position]
                    .trim()
                    .to_string();
                self.position = boundary.position + 1;
                let items = self.parse_block_items()?;
                Ok(Rule::Qualified(QualifiedRule { prelude, items }))
            }
            Some(';') | None => {
                Err(self.error(CssErrorKind::ExpectedQualifiedRuleBlock, boundary.position))
            }
            Some('}') => Err(self.error(CssErrorKind::UnexpectedClosingBrace, boundary.position)),
            _ => unreachable!("only qualified-rule delimiters are scanned"),
        }
    }

    fn parse_block_items(&mut self) -> Result<Vec<BlockItem>, CssError> {
        let mut items = Vec::new();

        loop {
            self.skip_trivia()?;
            match self.peek_char() {
                None => {
                    return Err(self.error(CssErrorKind::UnterminatedBlock, self.position));
                }
                Some('}') => {
                    self.advance_char();
                    return Ok(items);
                }
                Some('@') => items.push(BlockItem::Rule(self.parse_at_rule()?)),
                Some(_) => {
                    let item_start = self.position;
                    let boundary =
                        self.scan_boundary(item_start, self.source.len(), &['{', ';', '}'], false)?;

                    match boundary.delimiter {
                        Some('{')
                            if self
                                .is_custom_property_prelude(item_start, boundary.position)? =>
                        {
                            items.push(BlockItem::Declaration(self.parse_declaration()?));
                        }
                        Some('{') => {
                            items.push(BlockItem::Rule(self.parse_qualified_rule()?));
                        }
                        Some(';') => {
                            if self.is_trivia_only(item_start, boundary.position)? {
                                self.position = boundary.position + 1;
                            } else {
                                items.push(BlockItem::Declaration(self.parse_declaration()?));
                            }
                        }
                        Some('}') => {
                            if self.is_trivia_only(item_start, boundary.position)? {
                                self.position = boundary.position;
                            } else {
                                items.push(BlockItem::Declaration(self.parse_declaration()?));
                            }
                        }
                        None => {
                            return Err(
                                self.error(CssErrorKind::UnterminatedBlock, self.source.len())
                            );
                        }
                        _ => unreachable!("only block-item delimiters are scanned"),
                    }
                }
            }
        }
    }

    fn parse_declaration(&mut self) -> Result<Declaration, CssError> {
        let start = self.position;
        let boundary = self.scan_boundary(start, self.source.len(), &[';', '}'], true)?;
        let colon = self
            .find_top_level_delimiter(start, boundary.position, ':')?
            .ok_or_else(|| self.error(CssErrorKind::ExpectedDeclarationColon, start))?;

        let property = self.source[start..colon].trim();
        if property.is_empty() {
            return Err(self.error(CssErrorKind::EmptyPropertyName, start));
        }
        if !is_valid_property_name(property) {
            return Err(self.error(CssErrorKind::InvalidPropertyName, start));
        }

        let raw_value = self.source[colon + 1..boundary.position].trim();
        let (value, important) = split_important(raw_value);
        self.position = match boundary.delimiter {
            Some(';') => boundary.position + 1,
            Some('}') => boundary.position,
            None => {
                return Err(self.error(CssErrorKind::UnterminatedBlock, self.source.len()));
            }
            _ => unreachable!("only declaration delimiters are scanned"),
        };

        Ok(Declaration {
            property: property.to_string(),
            value,
            important,
        })
    }

    fn is_custom_property_prelude(&self, start: usize, end: usize) -> Result<bool, CssError> {
        let Some(colon) = self.find_top_level_delimiter(start, end, ':')? else {
            return Ok(false);
        };
        Ok(self.source[start..colon].trim().starts_with("--"))
    }

    fn find_top_level_delimiter(
        &self,
        start: usize,
        end: usize,
        delimiter: char,
    ) -> Result<Option<usize>, CssError> {
        let boundary = self.scan_boundary(start, end, &[delimiter], true)?;
        Ok(boundary.delimiter.map(|_| boundary.position))
    }

    fn scan_boundary(
        &self,
        start: usize,
        end: usize,
        delimiters: &[char],
        track_curly_braces: bool,
    ) -> Result<Boundary, CssError> {
        let mut position = start;
        let mut quote = None;
        let mut parentheses = 0usize;
        let mut brackets = 0usize;
        let mut curly_braces = 0usize;

        while position < end {
            let remaining = &self.source[position..end];

            if let Some(quote_char) = quote {
                let ch = remaining
                    .chars()
                    .next()
                    .expect("position must be before the end");
                position += ch.len_utf8();
                if ch == '\\' && position < end {
                    position += self.source[position..end]
                        .chars()
                        .next()
                        .expect("position must be before the end")
                        .len_utf8();
                } else if ch == quote_char {
                    quote = None;
                }
                continue;
            }

            if let Some(comment) = remaining.strip_prefix("/*") {
                let Some(comment_end) = comment.find("*/") else {
                    return Err(self.error(CssErrorKind::UnterminatedComment, position));
                };
                position += comment_end + 4;
                continue;
            }

            let ch = remaining
                .chars()
                .next()
                .expect("position must be before the end");
            let char_len = ch.len_utf8();

            if ch == '\\' {
                position += char_len;
                if position < end {
                    position += self.source[position..end]
                        .chars()
                        .next()
                        .expect("position must be before the end")
                        .len_utf8();
                }
                continue;
            }

            if ch == '"' || ch == '\'' {
                quote = Some(ch);
                position += char_len;
                continue;
            }

            match ch {
                '(' => parentheses += 1,
                ')' if parentheses > 0 => parentheses -= 1,
                ')' => {
                    return Err(self.error(CssErrorKind::UnbalancedDelimiter(')'), position));
                }
                '[' => brackets += 1,
                ']' if brackets > 0 => brackets -= 1,
                ']' => {
                    return Err(self.error(CssErrorKind::UnbalancedDelimiter(']'), position));
                }
                '{' if track_curly_braces && parentheses == 0 && brackets == 0 => {
                    curly_braces += 1;
                }
                '}' if track_curly_braces && parentheses == 0 && brackets == 0 => {
                    if curly_braces > 0 {
                        curly_braces -= 1;
                    } else if delimiters.contains(&ch) {
                        return Ok(Boundary {
                            position,
                            delimiter: Some(ch),
                        });
                    }
                }
                _ => {}
            }

            if parentheses == 0
                && brackets == 0
                && curly_braces == 0
                && delimiters.contains(&ch)
                && !(track_curly_braces && (ch == '{' || ch == '}'))
            {
                return Ok(Boundary {
                    position,
                    delimiter: Some(ch),
                });
            }

            position += char_len;
        }

        if quote.is_some() {
            return Err(self.error(CssErrorKind::UnterminatedString, end));
        }
        if parentheses > 0 {
            return Err(self.error(CssErrorKind::UnbalancedDelimiter('('), end));
        }
        if brackets > 0 {
            return Err(self.error(CssErrorKind::UnbalancedDelimiter('['), end));
        }
        if curly_braces > 0 {
            return Err(self.error(CssErrorKind::UnbalancedDelimiter('{'), end));
        }

        Ok(Boundary {
            position: end,
            delimiter: None,
        })
    }

    fn skip_trivia(&mut self) -> Result<(), CssError> {
        loop {
            while self.peek_char().is_some_and(is_css_whitespace) {
                self.advance_char();
            }

            let remaining = &self.source[self.position..];
            if !remaining.starts_with("/*") {
                return Ok(());
            }

            let Some(comment_end) = remaining[2..].find("*/") else {
                return Err(self.error(CssErrorKind::UnterminatedComment, self.position));
            };
            self.position += comment_end + 4;
        }
    }

    fn is_trivia_only(&self, start: usize, end: usize) -> Result<bool, CssError> {
        let mut position = start;
        while position < end {
            let remaining = &self.source[position..end];
            let ch = remaining
                .chars()
                .next()
                .expect("position must be before the end");

            if is_css_whitespace(ch) {
                position += ch.len_utf8();
            } else if let Some(comment) = remaining.strip_prefix("/*") {
                let Some(comment_end) = comment.find("*/") else {
                    return Err(self.error(CssErrorKind::UnterminatedComment, position));
                };
                position += comment_end + 4;
            } else {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn advance_char(&mut self) {
        if let Some(ch) = self.peek_char() {
            self.position += ch.len_utf8();
        }
    }

    fn error(&self, kind: CssErrorKind, position: usize) -> CssError {
        CssError::new(kind, position)
    }
}

fn is_css_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000c}')
}

fn is_css_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || !ch.is_ascii()
}

fn is_css_name_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_' || !ch.is_ascii()
}

fn is_valid_property_name(name: &str) -> bool {
    let mut chars = name.chars().peekable();
    let Some(first) = chars.next() else {
        return false;
    };

    if first == '-' {
        match chars.next() {
            Some('-') => {
                if chars.peek().is_none() {
                    return false;
                }
            }
            Some(ch) if is_css_name_start(ch) || ch == '\\' => {
                if ch == '\\' && chars.next().is_none() {
                    return false;
                }
            }
            _ => return false,
        }
    } else if first == '\\' {
        if chars.next().is_none() {
            return false;
        }
    } else if !is_css_name_start(first) {
        return false;
    }

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if chars.next().is_none() {
                return false;
            }
        } else if !is_css_name_char(ch) {
            return false;
        }
    }

    true
}

fn split_important(value: &str) -> (String, bool) {
    let value = value.trim();
    if let Some(index) = value.rfind('!')
        && value[index..].trim().eq_ignore_ascii_case("!important")
    {
        return (value[..index].trim_end().to_string(), true);
    }

    (value.to_string(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declarations(rule: &QualifiedRule) -> Vec<&Declaration> {
        rule.declarations().collect()
    }

    #[test]
    fn parses_qualified_rules_and_declarations() {
        let stylesheet = parse("h1, p { color: red; font-size: 16px }").unwrap();
        assert_eq!(stylesheet.rules().len(), 1);

        let Rule::Qualified(rule) = &stylesheet.rules()[0] else {
            panic!("expected a qualified rule");
        };
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
    fn keeps_delimiters_inside_strings_and_functions_in_values() {
        let stylesheet =
            parse(r#"p { content: "a;b}"; background-image: url("data:image/svg+xml;a,b"); }"#)
                .unwrap();
        let Rule::Qualified(rule) = &stylesheet.rules()[0] else {
            panic!("expected a qualified rule");
        };
        let declarations = declarations(rule);

        assert_eq!(declarations[0].value(), r#""a;b}""#);
        assert_eq!(declarations[1].value(), r#"url("data:image/svg+xml;a,b")"#);
    }

    #[test]
    fn keeps_delimiters_inside_comments_and_handles_unicode() {
        let stylesheet = parse(
            r#"/* header */ :is(h1, [lang="en; }"]) {
                font-family: "café;";
                color: red /* ; } */;
            }"#,
        )
        .unwrap();
        let Rule::Qualified(rule) = &stylesheet.rules()[0] else {
            panic!("expected a qualified rule");
        };
        let declarations = declarations(rule);

        assert_eq!(rule.prelude(), r#":is(h1, [lang="en; }"])"#);
        assert_eq!(declarations[0].value(), r#""café;""#);
        assert!(declarations[1].value().contains("/* ; } */"));
    }

    #[test]
    fn parses_important_declarations_without_matching_inside_strings() {
        let stylesheet = parse(r#"p { content: "!important"; color: red !IMPORTANT; }"#).unwrap();
        let Rule::Qualified(rule) = &stylesheet.rules()[0] else {
            panic!("expected a qualified rule");
        };
        let declarations = declarations(rule);

        assert_eq!(declarations[0].value(), r#""!important""#);
        assert!(!declarations[0].is_important());
        assert_eq!(declarations[1].value(), "red");
        assert!(declarations[1].is_important());
    }

    #[test]
    fn preserves_custom_property_blocks_and_nested_rules() {
        let stylesheet =
            parse("article { --theme: { color: red; }; &:hover { color: blue; } }").unwrap();
        let Rule::Qualified(rule) = &stylesheet.rules()[0] else {
            panic!("expected a qualified rule");
        };

        assert_eq!(rule.items().len(), 2);
        let BlockItem::Declaration(custom_property) = &rule.items()[0] else {
            panic!("expected a custom property declaration");
        };
        assert_eq!(custom_property.property(), "--theme");
        assert_eq!(custom_property.value(), "{ color: red; }");

        let BlockItem::Rule(Rule::Qualified(nested_rule)) = &rule.items()[1] else {
            panic!("expected a nested qualified rule");
        };
        assert_eq!(nested_rule.prelude(), "&:hover");
        assert_eq!(declarations(nested_rule)[0].value(), "blue");
    }

    #[test]
    fn parses_at_rules_with_nested_rules_and_declarations() {
        let stylesheet = parse(
            r#"@import url("theme.css");
               @media (min-width: 40rem) {
                   @supports (display: grid) {
                       .card:hover { color: blue; }
                   }
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
        assert_eq!(media.name(), "media");
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
    fn accepts_empty_stylesheets_and_comments() {
        assert!(parse("").unwrap().rules().is_empty());
        assert!(
            parse(" /* only a comment */ \n ")
                .unwrap()
                .rules()
                .is_empty()
        );
        assert!(parse("p { ; color: red; }").is_ok());
    }

    #[test]
    fn reports_malformed_stylesheet_syntax() {
        assert_eq!(
            parse("h1;").unwrap_err().kind(),
            CssErrorKind::ExpectedQualifiedRuleBlock
        );
        assert_eq!(
            parse("p { color red; }").unwrap_err().kind(),
            CssErrorKind::ExpectedDeclarationColon
        );
        assert_eq!(
            parse("p { content: \"unfinished; }").unwrap_err().kind(),
            CssErrorKind::UnterminatedString
        );
        assert_eq!(
            parse("p { color: red;").unwrap_err().kind(),
            CssErrorKind::UnterminatedBlock
        );
        assert_eq!(
            parse("@import url(theme.css)").unwrap_err().kind(),
            CssErrorKind::ExpectedAtRuleTerminator
        );
        assert_eq!(
            parse("p { color: rgb(1, 2, 3; }").unwrap_err().kind(),
            CssErrorKind::UnbalancedDelimiter('(')
        );
        assert_eq!(
            parse("/* unfinished").unwrap_err().kind(),
            CssErrorKind::UnterminatedComment
        );
    }
}
