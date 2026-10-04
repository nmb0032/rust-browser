/*!
 * Parses a limited subset of HTML into a DOM Document.
 *
 * Current HTML5/WHATWG parsing gaps:
 * - This is not the standard's tokenizer and tree-construction algorithm.
 * - Character references such as `&amp;` and `&#38;` are not decoded.
 * - Comments are discarded, and doctypes are skipped without selecting a
 *   document mode or recording doctype information.
 * - The parser does not synthesize `html`, `head`, or `body` elements, or
 *   implement insertion modes, implied end tags, table foster parenting, or
 *   adoption-agency recovery for formatting elements.
 * - Raw-text and RCDATA elements such as `script`, `style`, `title`, and
 *   `textarea` do not have their special tokenization rules.
 * - SVG/MathML foreign content, namespaces, and their tag-name adjustments
 *   are not supported.
 * - Error recovery is intentionally small: unmatched end tags are ignored,
 *   matching ancestors close intervening elements, and open elements are
 *   accepted at end-of-input. Other malformed markup may differ from browsers.
 */
use crate::dom::{Attribute, Document, DomError, ElementData};

#[derive(Debug, PartialEq, Eq)]
pub enum HtmlError {
    ExpectedTagStart,
    UnterminatedTag,
    UnterminatedComment,
    UnterminatedAttributeValue,
    EmptyTagName,
    InvalidTagName,
    InvalidAttributeName,
    InvalidAttributeValue,
    InvalidEndTag,
    UnsupportedTagSyntax,
    InvalidTreeState,
    Dom(DomError),
}

impl From<DomError> for HtmlError {
    fn from(error: DomError) -> Self {
        HtmlError::Dom(error)
    }
}

pub fn parse(source: &str) -> Result<Document, HtmlError> {
    let normalized_source = source.replace("\r\n", "\n").replace('\r', "\n");
    let tokens = tokenize(&normalized_source)?;
    build_document(tokens)
}

fn tokenize(source: &str) -> Result<Vec<Token>, HtmlError> {
    // Track how many bytes of the input have been turned into tokens. Each
    // update below moves to the next unparsed portion of the source.
    let mut position = 0;
    let mut tokens = Vec::new();
    while position < source.len() {
        // Work only with the input that has not been tokenized yet.
        let remaining = &source[position..];

        if remaining.starts_with("<!--") {
            let end = remaining
                .find("-->")
                .ok_or(HtmlError::UnterminatedComment)?;
            position += end + "-->".len();
            continue;
        }

        if is_doctype_start(remaining) {
            let end = find_tag_end(remaining)?;
            position += end + 1;
            continue;
        }

        if !remaining.starts_with('<') {
            // A text token ends at the next `<`, or at the end of the input if
            // there are no more tags. `find` gives a byte offset for slicing.
            let bytes_used = remaining.find('<').unwrap_or(remaining.len());
            tokens.push(Token::Text(remaining[..bytes_used].to_string()));
            // Advance past the text just added so the next pass starts at the
            // next possible tag.
            position += bytes_used;
        } else {
            // At `<`, delegate parsing to `parse_tag`. It reports both the
            // token it found and how many input bytes it consumed.
            let (token, bytes_used) = parse_tag(remaining)?;
            tokens.push(token);
            position += bytes_used;
        }
    }
    Ok(tokens)
}

fn parse_tag(source: &str) -> Result<(Token, usize), HtmlError> {
    if !source.starts_with('<') {
        return Err(HtmlError::ExpectedTagStart);
    }

    let end = find_tag_end(source)?;
    let mut cursor = Cursor::new(&source[1..end]);
    let is_end_tag = cursor.consume_if('/');
    let raw_name = cursor.take_while(|ch| !is_html_whitespace(ch) && ch != '/');
    let name = normalize_tag_name(raw_name)?;

    if is_end_tag {
        cursor.skip_whitespace();
        if !cursor.is_empty() {
            return Err(HtmlError::InvalidEndTag);
        }

        Ok((Token::EndTag { name }, end + 1))
    } else {
        let attributes = parse_attributes(&mut cursor)?;
        Ok((Token::StartTag { name, attributes }, end + 1))
    }
}

fn build_document(tokens: Vec<Token>) -> Result<Document, HtmlError> {
    let mut document = Document::new();
    let mut elements_stack = vec![document.root_id()];

    for token in tokens {
        match token {
            Token::StartTag { name, attributes } => {
                let parent = *elements_stack.last().ok_or(HtmlError::InvalidTreeState)?;

                let is_void = is_void_element(&name);
                let element_id =
                    document.append_element(parent, ElementData::new(name, attributes))?;

                if !is_void {
                    elements_stack.push(element_id);
                }
            }

            Token::Text(text) => {
                let parent = *elements_stack.last().ok_or(HtmlError::InvalidTreeState)?;

                document.append_text(parent, text)?;
            }

            Token::EndTag { name } => {
                let matching_index =
                    elements_stack
                        .iter()
                        .enumerate()
                        .skip(1)
                        .rev()
                        .find_map(|(index, id)| {
                            let element = document.node(*id)?.as_element()?;
                            (element.tag_name() == name).then_some(index)
                        });

                if let Some(index) = matching_index {
                    elements_stack.truncate(index);
                }
            }
        }
    }

    Ok(document)
}

fn parse_attributes(cursor: &mut Cursor<'_>) -> Result<Vec<Attribute>, HtmlError> {
    let mut attributes = Vec::new();

    loop {
        cursor.skip_whitespace();
        if cursor.is_empty() {
            break;
        }

        if cursor.consume_if('/') {
            cursor.skip_whitespace();
            if cursor.is_empty() {
                break;
            }
            return Err(HtmlError::UnsupportedTagSyntax);
        }

        let raw_name = cursor.take_while(|ch| {
            !is_html_whitespace(ch) && !matches!(ch, '=' | '/' | '<' | '>' | '"' | '\'' | '\0')
        });
        if raw_name.is_empty() {
            return Err(HtmlError::InvalidAttributeName);
        }

        let name = raw_name.to_ascii_lowercase();
        cursor.skip_whitespace();
        let value = if cursor.consume_if('=') {
            parse_attribute_value(cursor)?
        } else {
            String::new()
        };

        // HTML ignores later occurrences of an attribute with the same name.
        if !attributes
            .iter()
            .any(|attribute: &Attribute| attribute.name() == name)
        {
            attributes.push(Attribute::new(name, value));
        }
    }

    Ok(attributes)
}

fn parse_attribute_value(cursor: &mut Cursor<'_>) -> Result<String, HtmlError> {
    cursor.skip_whitespace();

    if matches!(cursor.peek(), Some('"') | Some('\'')) {
        let quote = cursor.advance().ok_or(HtmlError::InvalidAttributeValue)?;
        let value = cursor.take_while(|ch| ch != quote).to_string();
        if !cursor.consume_if(quote) {
            return Err(HtmlError::UnterminatedAttributeValue);
        }
        return Ok(value);
    }

    let value = cursor.take_while(|ch| !is_html_whitespace(ch));
    if value
        .chars()
        .any(|ch| matches!(ch, '"' | '\'' | '<' | '=' | '`' | '\0'))
    {
        return Err(HtmlError::InvalidAttributeValue);
    }

    Ok(value.to_string())
}

fn normalize_tag_name(raw_name: &str) -> Result<String, HtmlError> {
    let mut characters = raw_name.chars();
    let Some(first) = characters.next() else {
        return Err(HtmlError::EmptyTagName);
    };

    if !first.is_ascii_alphabetic()
        || !characters.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | ':'))
    {
        return Err(HtmlError::InvalidTagName);
    }

    Ok(raw_name.to_ascii_lowercase())
}

fn find_tag_end(source: &str) -> Result<usize, HtmlError> {
    let mut quote = None;

    for (index, ch) in source.char_indices().skip(1) {
        match quote {
            Some(delimiter) if ch == delimiter => quote = None,
            Some(_) => {}
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '>' => return Ok(index),
                _ => {}
            },
        }
    }

    if quote.is_some() {
        Err(HtmlError::UnterminatedAttributeValue)
    } else {
        Err(HtmlError::UnterminatedTag)
    }
}

fn is_doctype_start(source: &str) -> bool {
    let Some(prefix) = source.get(..9) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case("<!doctype") {
        return false;
    }

    match source.get(9..).and_then(|rest| rest.chars().next()) {
        Some(ch) => is_html_whitespace(ch) || ch == '>',
        None => true,
    }
}

fn is_html_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000C}')
}

struct Cursor<'a> {
    source: &'a str,
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.position += ch.len_utf8();
        Some(ch)
    }

    fn consume_if(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if !is_html_whitespace(ch) {
                break;
            }
            self.position += ch.len_utf8();
        }
    }

    fn take_while(&mut self, mut predicate: impl FnMut(char) -> bool) -> &'a str {
        let start = self.position;
        while let Some(ch) = self.peek() {
            if !predicate(ch) {
                break;
            }
            self.position += ch.len_utf8();
        }
        &self.source[start..self.position]
    }

    fn is_empty(&self) -> bool {
        self.position == self.source.len()
    }
}

enum Token {
    StartTag {
        name: String,
        attributes: Vec<Attribute>,
    },
    EndTag {
        name: String,
    },
    Text(String),
}

fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::NodeKind;

    #[test]
    fn parses_nested_elements_text_and_attributes() {
        let document = parse(r#"<p class="intro">Hello <a href='/next'>there</a></p>"#).unwrap();
        let root = document.node(document.root_id()).unwrap();
        let paragraph_id = root.children()[0];
        let paragraph = document.node(paragraph_id).unwrap();
        let paragraph_element = paragraph.as_element().unwrap();

        assert_eq!(paragraph_element.tag_name(), "p");
        assert_eq!(paragraph_element.attribute("CLASS"), Some("intro"));
        assert_eq!(paragraph.children().len(), 2);
        assert!(matches!(
            document.node(paragraph.children()[0]).unwrap().kind(),
            NodeKind::Text(text) if text == "Hello "
        ));

        let link = document.node(paragraph.children()[1]).unwrap();
        let link_element = link.as_element().unwrap();
        assert_eq!(link_element.tag_name(), "a");
        assert_eq!(link_element.attribute("href"), Some("/next"));
        assert!(matches!(
            document.node(link.children()[0]).unwrap().kind(),
            NodeKind::Text(text) if text == "there"
        ));
    }

    #[test]
    fn normalizes_html_newlines_in_text() {
        let document = parse("<p>one\r\ntwo\rthree\nfour</p>").unwrap();
        let root = document.node(document.root_id()).unwrap();
        let paragraph = document.node(root.children()[0]).unwrap();

        assert!(matches!(
            document.node(paragraph.children()[0]).unwrap().kind(),
            NodeKind::Text(text) if text == "one\ntwo\nthree\nfour"
        ));
    }

    #[test]
    fn quoted_greater_than_and_unicode_text_are_preserved() {
        let document = parse(r#"<p title="left > right">café 世界</p>"#).unwrap();
        let root = document.node(document.root_id()).unwrap();
        let paragraph = document.node(root.children()[0]).unwrap();

        assert_eq!(
            paragraph.as_element().unwrap().attribute("title"),
            Some("left > right")
        );
        assert!(matches!(
            document.node(paragraph.children()[0]).unwrap().kind(),
            NodeKind::Text(text) if text == "café 世界"
        ));
    }

    #[test]
    fn parses_boolean_and_unquoted_attributes() {
        let document = parse("<input disabled value=hello>").unwrap();
        let root = document.node(document.root_id()).unwrap();
        let input = document.node(root.children()[0]).unwrap();
        let element = input.as_element().unwrap();

        assert_eq!(element.attribute("disabled"), Some(""));
        assert_eq!(element.attribute("value"), Some("hello"));
    }

    #[test]
    fn void_elements_do_not_capture_following_text() {
        let document = parse("<div>before<br>after</div>").unwrap();
        let root = document.node(document.root_id()).unwrap();
        let div = document.node(root.children()[0]).unwrap();

        assert_eq!(div.children().len(), 3);
        assert!(matches!(
            document.node(div.children()[0]).unwrap().kind(),
            NodeKind::Text(text) if text == "before"
        ));
        assert_eq!(
            document
                .node(div.children()[1])
                .unwrap()
                .as_element()
                .unwrap()
                .tag_name(),
            "br"
        );
        assert!(matches!(
            document.node(div.children()[2]).unwrap().kind(),
            NodeKind::Text(text) if text == "after"
        ));
    }

    #[test]
    fn ignores_doctypes_and_comments() {
        let document =
            parse("<!DOCTYPE html><!-- head > note --><p>content</p><!-- end -->").unwrap();
        let root = document.node(document.root_id()).unwrap();

        assert_eq!(root.children().len(), 1);
        assert_eq!(
            document
                .node(root.children()[0])
                .unwrap()
                .as_element()
                .unwrap()
                .tag_name(),
            "p"
        );
    }

    #[test]
    fn rejects_malformed_tag_names() {
        for source in ["<$>", "<p=bad>", "<<p>"] {
            assert!(
                matches!(parse(source), Err(HtmlError::InvalidTagName)),
                "expected invalid tag name for {source:?}"
            );
        }
    }

    #[test]
    fn reports_unterminated_tags_attributes_and_comments() {
        assert!(matches!(parse("<p"), Err(HtmlError::UnterminatedTag)));
        assert!(matches!(
            parse(r#"<p title="unfinished>"#),
            Err(HtmlError::UnterminatedAttributeValue)
        ));
        assert!(matches!(
            parse("<!-- unfinished"),
            Err(HtmlError::UnterminatedComment)
        ));
    }

    #[test]
    fn recovers_from_unmatched_closing_tags_and_unclosed_elements() {
        let document = parse("</unknown><div><p>nested</div>tail").unwrap();
        let root = document.node(document.root_id()).unwrap();
        let div = document.node(root.children()[0]).unwrap();

        assert_eq!(root.children().len(), 2);
        assert_eq!(div.children().len(), 1);
        assert_eq!(
            document
                .node(div.children()[0])
                .unwrap()
                .as_element()
                .unwrap()
                .tag_name(),
            "p"
        );
        assert!(matches!(
            document.node(root.children()[1]).unwrap().kind(),
            NodeKind::Text(text) if text == "tail"
        ));

        let unclosed = parse("<p>unfinished").unwrap();
        let root = unclosed.node(unclosed.root_id()).unwrap();
        assert_eq!(root.children().len(), 1);
    }
}
