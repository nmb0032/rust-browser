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
use crate::dom::{Attribute, Document, DomError, ElementData, NodeId};

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
    let mut parser = HtmlParser::new();
    parser.feed(source)?;
    parser.finish()
}

#[derive(Debug)]
pub struct HtmlParser {
    buffer: String,
    document: Document,
    elements_stack: Vec<NodeId>,
    pending_carriage_return: bool,
}

impl Default for HtmlParser {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlParser {
    pub fn new() -> Self {
        let document = Document::new();
        let elements_stack = vec![document.root_id()];
        Self {
            buffer: String::new(),
            document,
            elements_stack,
            pending_carriage_return: false,
        }
    }

    pub fn feed(&mut self, chunk: &str) -> Result<(), HtmlError> {
        let mut input = String::new();
        if self.pending_carriage_return {
            input.push('\r');
            self.pending_carriage_return = false;
        }
        input.push_str(chunk);

        if input.ends_with('\r') {
            input.pop();
            self.pending_carriage_return = true;
        }
        self.buffer
            .push_str(&input.replace("\r\n", "\n").replace('\r', "\n"));

        self.process_available(false)
    }

    pub fn finish(mut self) -> Result<Document, HtmlError> {
        if self.pending_carriage_return {
            self.buffer.push('\n');
            self.pending_carriage_return = false;
        }
        self.process_available(true)?;
        Ok(self.document)
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn node_count(&self) -> usize {
        self.document.node_count()
    }

    fn process_available(&mut self, is_final: bool) -> Result<(), HtmlError> {
        let mut position = 0;

        while position < self.buffer.len() {
            let remaining = &self.buffer[position..];

            if remaining.starts_with("<!--") {
                match remaining.find("-->") {
                    Some(end) => position += end + "-->".len(),
                    None if !is_final => break,
                    None => return Err(HtmlError::UnterminatedComment),
                }
                continue;
            }

            if is_doctype_start(remaining) {
                match find_tag_end(remaining) {
                    Ok(end) => position += end + 1,
                    Err(error) if !is_final && is_incomplete_markup_error(&error) => break,
                    Err(error) => return Err(error),
                }
                continue;
            }

            if !remaining.starts_with('<') {
                let Some(bytes_used) = remaining.find('<') else {
                    if !is_final {
                        break;
                    }
                    let text = remaining.to_string();
                    self.append_token(Token::Text(text))?;
                    position = self.buffer.len();
                    continue;
                };

                let text = remaining[..bytes_used].to_string();
                self.append_token(Token::Text(text))?;
                position += bytes_used;
                continue;
            }

            let (token, bytes_used) = match parse_tag(remaining) {
                Ok(parsed) => parsed,
                Err(error) if !is_final && is_incomplete_markup_error(&error) => break,
                Err(error) => return Err(error),
            };
            self.append_token(token)?;
            position += bytes_used;
        }

        self.buffer.drain(..position);
        Ok(())
    }

    fn append_token(&mut self, token: Token) -> Result<(), HtmlError> {
        match token {
            Token::StartTag { name, attributes } => {
                let parent = *self
                    .elements_stack
                    .last()
                    .ok_or(HtmlError::InvalidTreeState)?;

                let is_void = is_void_element(&name);
                let element_id = self
                    .document
                    .append_element(parent, ElementData::new(name, attributes))?;

                if !is_void {
                    self.elements_stack.push(element_id);
                }
            }
            Token::Text(text) => {
                let parent = *self
                    .elements_stack
                    .last()
                    .ok_or(HtmlError::InvalidTreeState)?;
                self.document.append_text(parent, text)?;
            }
            Token::EndTag { name } => {
                let matching_index = self
                    .elements_stack
                    .iter()
                    .enumerate()
                    .skip(1)
                    .rev()
                    .find_map(|(index, id)| {
                        let element = self.document.node(*id)?.as_element()?;
                        (element.tag_name() == name).then_some(index)
                    });

                if let Some(index) = matching_index {
                    self.elements_stack.truncate(index);
                }
            }
        }
        Ok(())
    }
}

fn is_incomplete_markup_error(error: &HtmlError) -> bool {
    matches!(
        error,
        HtmlError::UnterminatedTag | HtmlError::UnterminatedAttributeValue
    )
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
    fn parses_stream_of_text() {
        let chunks = [r#"<p title="left"#, r#" > right">caf"#, "é</p>"];
        let mut parser = HtmlParser::new();

        for chunk in chunks {
            parser.feed(chunk).unwrap();
        }

        let document = parser.finish().unwrap();
        let root = document.node(document.root_id()).unwrap();
        assert_eq!(root.children().len(), 1);

        let paragraph = document.node(root.children()[0]).unwrap();
        assert_eq!(
            paragraph.as_element().unwrap().attribute("title"),
            Some("left > right")
        );
        assert_eq!(paragraph.children().len(), 1);
        assert!(matches!(
            document.node(paragraph.children()[0]).unwrap().kind(),
            NodeKind::Text(text) if text == "café"
        ));
    }

    #[test]
    fn streaming_parser_matches_at_every_character_boundary() {
        for source in [
            "",
            "plain text\r",
            "\r\r\n",
            "<!DOCTYPE html><!-- head > note --><p title=\"left > right\">café\r\n世界</p>",
            "<p>before<!-- ignored -->after<br>last</p>",
            "<p title='世界 > café' disabled data-value=hello>text</p>",
            "</unknown><div><p>nested</div>tail",
            include_str!("../../tests/pages/basic.html"),
            include_str!("../../tests/pages/dom_dump.html"),
        ] {
            let expected = document_signature(&parse(source).unwrap());

            for split in source
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(source.len()))
            {
                let mut parser = HtmlParser::new();
                parser.feed(&source[..split]).unwrap();
                parser.feed(&source[split..]).unwrap();

                assert_eq!(
                    document_signature(&parser.finish().unwrap()),
                    expected,
                    "DOM differed when {source:?} was split at byte {split}"
                );
            }
        }
    }

    #[test]
    fn exposes_complete_content_before_finish_and_keeps_snapshots_independent() {
        let mut parser = HtmlParser::new();
        parser.feed("<p>visible</p><p title=\"unfinished").unwrap();
        let snapshot = parser.document().clone();
        let expected = document_signature(&parse("<p>visible</p>").unwrap());

        assert_eq!(document_signature(&snapshot), expected);
        parser.feed("\">next</p>").unwrap();
        assert_eq!(document_signature(&snapshot), expected);
        assert_eq!(
            document_signature(&parser.finish().unwrap()),
            document_signature(&parse("<p>visible</p><p title=\"unfinished\">next</p>").unwrap())
        );
    }

    #[test]
    fn streaming_errors_match_whole_input_errors_at_every_boundary() {
        for source in ["<p", "<p title=\"unfinished>", "<!-- unfinished", "<p=bad>"] {
            let expected = parse(source).unwrap_err();

            for split in 0..=source.len() {
                let mut parser = HtmlParser::new();
                let result = parser
                    .feed(&source[..split])
                    .and_then(|()| parser.feed(&source[split..]))
                    .and_then(|()| parser.finish());

                assert_eq!(result.unwrap_err(), expected, "split at byte {split}");
            }
        }
    }

    #[test]
    fn streaming_parser_accepts_one_character_chunks() {
        let source = "<p title=\"left > right\">café\r\n世界</p>";
        let expected = document_signature(&parse(source).unwrap());
        let mut parser = HtmlParser::new();

        for character in source.chars() {
            parser.feed(&character.to_string()).unwrap();
        }

        assert_eq!(document_signature(&parser.finish().unwrap()), expected);
    }

    fn document_signature(document: &Document) -> String {
        fn node_signature(document: &Document, id: NodeId) -> String {
            let node = document.node(id).unwrap();
            let children = node
                .children()
                .iter()
                .map(|child| node_signature(document, *child))
                .collect::<Vec<_>>()
                .join(",");

            match node.kind() {
                NodeKind::Document => format!("document[{children}]"),
                NodeKind::Element(element) => {
                    let attributes = element
                        .attributes()
                        .iter()
                        .map(|attribute| format!("{:?}={:?}", attribute.name(), attribute.value()))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("element:{}({attributes})[{children}]", element.tag_name())
                }
                NodeKind::Text(text) => format!("text:{text:?}"),
            }
        }

        node_signature(document, document.root_id())
    }

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
