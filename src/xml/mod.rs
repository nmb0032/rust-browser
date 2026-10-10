//! XML 1.0 / Namespaces in XML integration. No network entity resolver is installed.

mod source;

use crate::{
    dom::{Attribute, DoctypeData, Document, ElementData, Namespace, NodeId},
    html::{
        encoding::{mime_charset, mime_essence, xml_declaration_encoding, xml_declaration_label},
        tokenizer::SourcePosition,
    },
};
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};
use std::{collections::HashSet, fmt};
use xml::{
    common::{Position, XmlVersion},
    reader::{ParserConfig, XmlEvent},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlError {
    pub message: String,
    pub position: SourcePosition,
}

impl fmt::Display for XmlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "XML at {}:{}: {}",
            self.position.line, self.position.column, self.message
        )
    }
}
impl std::error::Error for XmlError {}

pub fn is_xml_mime(content_type: &str) -> bool {
    let mime = mime_essence(content_type);
    matches!(mime.as_str(), "application/xml" | "text/xml") || mime.ends_with("+xml")
}

pub fn parse_bytes(bytes: &[u8], content_type: Option<&str>) -> Result<Document, XmlError> {
    let bom = Encoding::for_bom(bytes);
    let encoding = bom
        .map(|(encoding, _)| encoding)
        .or_else(|| content_type.and_then(mime_charset))
        .or_else(|| {
            if bytes.starts_with(b"<\0?\0") {
                Some(UTF_16LE)
            } else if bytes.starts_with(b"\0<\0?") {
                Some(UTF_16BE)
            } else {
                None
            }
        })
        .or_else(|| xml_declaration_encoding(bytes))
        .unwrap_or(UTF_8);
    let bytes = &bytes[bom.map_or(0, |(_, length)| length)..];
    let source = encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .ok_or_else(|| error("malformed byte sequence in XML document", 0, 0, ""))?;
    let mut document = parse(&source)?;
    document.set_encoding(encoding.name());
    Ok(document)
}

pub fn parse(source: &str) -> Result<Document, XmlError> {
    if source.len() > crate::net::MAX_HTML_BYTES as usize {
        return Err(error("XML document exceeds byte limit", 0, 0, source));
    }
    if let Some(label) = xml_declaration_label(source.as_bytes())
        && (!label.first().is_some_and(u8::is_ascii_alphabetic)
            || label
                .iter()
                .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'.' | b'_' | b'-'))
            || Encoding::for_label(label).is_none())
    {
        return Err(error(
            "invalid or unsupported XML encoding declaration",
            0,
            0,
            source,
        ));
    }
    let config = ParserConfig::new()
        .ignore_comments(false)
        .cdata_to_characters(true)
        .whitespace_to_characters(true)
        .allow_multiple_root_elements(false)
        .override_encoding(Some(xml::Encoding::Utf8))
        .ignore_invalid_encoding_declarations(true)
        .max_entity_expansion_depth(10)
        .max_entity_expansion_length(1024 * 1024)
        .max_data_length(crate::net::MAX_HTML_BYTES as usize)
        .max_attribute_length(crate::net::MAX_HTML_BYTES as usize);
    let mut reader = config.create_reader(source.as_bytes());
    let mut locator = self::source::Locator::new(source);
    let mut document = Document::new();
    let mut open = vec![document.root_id()];
    let mut namespaces = vec![xml::namespace::Namespace::empty()];
    let doctype = prolog_doctype(source);
    let mut emitted_doctype = false;
    let mut text_bytes = 0usize;
    loop {
        let event = reader.next().map_err(|failure| {
            let position = failure.position();
            XmlError {
                message: failure.to_string(),
                position: self::source::normalized_position(
                    source,
                    locator.position(position.row as usize, position.column as usize),
                ),
            }
        })?;
        let position = reader.position();
        let position = locator.position(position.row as usize, position.column as usize);
        let diagnostic = |message: &str| XmlError {
            message: message.into(),
            position: self::source::normalized_position(source, position),
        };
        if let Some((offset, data)) = &doctype
            && !emitted_doctype
            && position.byte_offset >= *offset
        {
            document
                .append_doctype(document.root_id(), data.clone())
                .map_err(|failure| diagnostic(&format!("{failure:?}")))?;
            emitted_doctype = true;
        }
        let parent = *open
            .last()
            .ok_or_else(|| diagnostic("empty XML element stack"))?;
        match event {
            XmlEvent::StartDocument { version, .. } => {
                if version != XmlVersion::Version10 {
                    return Err(diagnostic("only XML 1.0 is supported"));
                }
            }
            XmlEvent::StartElement {
                name,
                attributes,
                namespace,
            } => {
                for (prefix, uri) in namespace.iter() {
                    if (uri == "http://www.w3.org/XML/1998/namespace" && prefix != "xml")
                        || (uri == "http://www.w3.org/2000/xmlns/" && prefix != "xmlns")
                        || (!prefix.is_empty() && uri.is_empty())
                    {
                        return Err(diagnostic("invalid reserved namespace binding"));
                    }
                }
                let mut seen = HashSet::new();
                let mut attrs = Vec::new();
                for attribute in attributes {
                    text_bytes = text_bytes.saturating_add(attribute.value.len());
                    if text_bytes > crate::net::MAX_HTML_BYTES as usize {
                        return Err(diagnostic("expanded XML data exceeds byte limit"));
                    }
                    let key = (
                        attribute.name.namespace.clone().unwrap_or_default(),
                        attribute.name.local_name.clone(),
                    );
                    if !seen.insert(key) {
                        return Err(diagnostic("duplicate expanded attribute name"));
                    }
                    attrs.push(if let Some(namespace) = attribute.name.namespace {
                        Attribute::namespaced(
                            attribute.name.local_name,
                            attribute.value,
                            namespace,
                            attribute.name.prefix,
                        )
                    } else {
                        Attribute::new(attribute.name.local_name, attribute.value)
                    });
                }
                let previous = namespaces
                    .last()
                    .ok_or_else(|| diagnostic("empty namespace stack"))?;
                // xml-rs reports the first root event at its closing delimiter.
                // Entity-expanded tags have no lexical source; use namespace deltas there.
                let prefixes = self::source::namespace_prefixes(source, position.byte_offset)
                    .unwrap_or_else(|| {
                        namespace
                            .iter()
                            .filter(|(prefix, uri)| {
                                *prefix != "xml"
                                    && *prefix != "xmlns"
                                    && previous.get(prefix) != Some(*uri)
                            })
                            .map(|(prefix, _)| prefix)
                            .collect()
                    });
                let mut seen_prefixes = HashSet::new();
                for prefix in prefixes {
                    if !seen_prefixes.insert(prefix) {
                        return Err(diagnostic("duplicate namespace declaration"));
                    }
                    let uri = namespace
                        .get(prefix)
                        .ok_or_else(|| diagnostic("unresolved namespace declaration"))?;
                    let (name, prefix) = if prefix.is_empty() {
                        ("xmlns".into(), None)
                    } else {
                        (prefix.to_owned(), Some("xmlns".into()))
                    };
                    attrs.push(Attribute::namespaced(
                        name,
                        uri.into(),
                        "http://www.w3.org/2000/xmlns/".into(),
                        prefix,
                    ));
                }
                let id = document
                    .append_element(
                        parent,
                        ElementData::xml(
                            name.local_name,
                            attrs,
                            Namespace::from_uri(name.namespace.as_deref().unwrap_or("")),
                            name.prefix,
                        ),
                    )
                    .map_err(|failure| diagnostic(&format!("{failure:?}")))?;
                open.push(id);
                namespaces.push(namespace);
            }
            XmlEvent::EndElement { .. } => {
                open.pop();
                namespaces.pop();
            }
            XmlEvent::Characters(text) | XmlEvent::CData(text) | XmlEvent::Whitespace(text) => {
                text_bytes = text_bytes.saturating_add(text.len());
                if text_bytes > crate::net::MAX_HTML_BYTES as usize {
                    return Err(diagnostic("expanded XML text exceeds byte limit"));
                }
                if parent != document.root_id() {
                    document
                        .append_text_coalesced(parent, text)
                        .map_err(|failure| diagnostic(&format!("{failure:?}")))?;
                }
            }
            XmlEvent::Comment(text) => {
                document
                    .append_comment(parent, text)
                    .map_err(|failure| diagnostic(&format!("{failure:?}")))?;
            }
            XmlEvent::ProcessingInstruction { name, data } => {
                document
                    .append_processing_instruction(parent, name, data.unwrap_or_default())
                    .map_err(|failure| diagnostic(&format!("{failure:?}")))?;
            }
            XmlEvent::EndDocument => break,
        }
    }
    Ok(document)
}

pub fn error_document(failure: &XmlError) -> Document {
    let mut document = Document::new();
    let root = document.root_id();
    let html = element(&mut document, root, "html");
    let body = element(&mut document, html, "body");
    let title = element(&mut document, body, "h1");
    document
        .append_text(title, "XML parsing error".into())
        .expect("new error heading");
    let details = element(&mut document, body, "pre");
    document
        .append_text(details, failure.to_string())
        .expect("new error details");
    document
}

fn element(document: &mut Document, parent: NodeId, name: &str) -> NodeId {
    document
        .append_element(parent, ElementData::new(name.into(), Vec::new()))
        .expect("new error element")
}
fn error(message: &str, row: usize, column: usize, source: &str) -> XmlError {
    XmlError {
        message: message.into(),
        position: SourcePosition {
            byte_offset: byte_offset(source, row, column),
            line: row + 1,
            column: column + 1,
        },
    }
}
fn byte_offset(source: &str, row: usize, column: usize) -> usize {
    let mut line = 0;
    let mut col = 0;
    for (offset, ch) in source.char_indices() {
        if line == row && col == column {
            return offset;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    source.len()
}

fn prolog_doctype(source: &str) -> Option<(usize, DoctypeData)> {
    let mut rest = source.trim_start_matches('\u{FEFF}');
    loop {
        rest = rest.trim_start();
        if rest.starts_with("<!--") {
            rest = &rest[rest.find("-->")? + 3..];
        } else if rest.starts_with("<?") {
            rest = &rest[rest.find("?>")? + 2..];
        } else {
            break;
        }
    }
    let declaration = rest.strip_prefix("<!DOCTYPE")?;
    let name_start = declaration.trim_start();
    let name_end = name_start.find(|ch: char| ch.is_whitespace() || matches!(ch, '[' | '>'))?;
    let name = &name_start[..name_end];
    let mut data = DoctypeData {
        name: name.into(),
        ..DoctypeData::default()
    };
    let mut remainder = name_start[name_end..].trim_start();
    let public = remainder.starts_with("PUBLIC");
    if public || remainder.starts_with("SYSTEM") {
        remainder = remainder[6..].trim_start();
        let (id, tail) = quoted(remainder)?;
        if public {
            data.public_identifier = id.into();
            let (id, _) = quoted(tail.trim_start())?;
            data.system_identifier = id.into();
        } else {
            data.system_identifier = id.into();
        }
    }
    Some((source.len() - rest.len(), data))
}
fn quoted(source: &str) -> Option<(&str, &str)> {
    let quote = source.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let end = source[1..].find(quote)? + 1;
    Some((&source[1..end], &source[end + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::NodeKind;
    #[test]
    fn preserves_xml_case_namespaces_comments_and_instructions() {
        let document = parse("<?note data?><!--before--><!DOCTYPE Root><Root xmlns='urn:test' xmlns:a='urn:a'><a:Child a:Attr='x' Attr='y'>a&amp;b<![CDATA[<c>]]></a:Child></Root>").unwrap();
        let root = document
            .preorder()
            .find_map(|id| {
                document
                    .node(id)?
                    .as_element()
                    .filter(|element| element.tag_name() == "Root")
            })
            .unwrap();
        assert_eq!(root.namespace().uri(), "urn:test");
        let child = document
            .preorder()
            .find_map(|id| {
                document
                    .node(id)?
                    .as_element()
                    .filter(|element| element.tag_name() == "Child")
            })
            .unwrap();
        assert_eq!(child.namespace().uri(), "urn:a");
        assert_eq!(child.prefix(), Some("a"));
        assert_eq!(child.attribute("Attr"), Some("y"));
        assert_eq!(child.attribute("attr"), None);
        assert!(document.preorder().any(|id| matches!(document.node(id).unwrap().kind(), NodeKind::Text(text) if text == "a&b<c>")));
        assert!(document.preorder().any(|id| matches!(document.node(id).unwrap().kind(), NodeKind::Doctype(data) if data.name == "Root")));
    }
    #[test]
    fn rejects_malformed_documents_and_namespace_collisions() {
        for source in [
            "<a><b></a>",
            "<a/><b/>",
            "<a x='1' x='2'/>",
            "<a xmlns:p='u' xmlns:q='u' p:x='1' q:x='2'/>",
            "<a>&unknown;</a>",
            "<p:a/>",
            "<a>\0</a>",
            "<a><!-- bad--comment --></a>",
            "<a xmlns:xml='wrong'/>",
            "<a xmlns:xmlns='urn:wrong'/>",
            "<a xmlns:a='http://www.w3.org/2000/xmlns/'/>",
            "<a xmlns='http://www.w3.org/XML/1998/namespace'/>",
            "<a xmlns:a=''/>",
            "<a xmlns:a='u' xmlns:a='v'/>",
            "<?xml version='1.0' encoding='invalid'?><a/>",
            "<?xml version='1.0' encoding='1utf8'?><a/>",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }
    #[test]
    fn decodes_utf16_and_legacy_transport_charset() {
        let source = "<?xml version='1.0'?><a>café</a>";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(source.encode_utf16().flat_map(u16::to_le_bytes));
        assert!(parse_bytes(&bytes, None).is_ok());
        assert!(
            parse_bytes(
                b"<?xml version='1.0' encoding='windows-1252'?><a>caf\xe9</a>",
                None
            )
            .is_ok()
        );
        assert!(parse_bytes(b"<a>\xff</a>", Some("application/xml; charset=utf-8")).is_err());
    }

    #[test]
    fn preserves_redundant_and_empty_namespace_declarations() {
        let document = parse("<a xmlns='' xmlns:p='urn:p' xmlns:xml='http://www.w3.org/XML/1998/namespace'><b xmlns:p='urn:p' xmlns=''/></a>").unwrap();
        let elements = document
            .preorder()
            .filter_map(|id| document.node(id)?.as_element())
            .collect::<Vec<_>>();
        assert_eq!(elements[0].attributes().len(), 3);
        assert_eq!(elements[1].attributes().len(), 2);
        document.validate().unwrap();
    }

    #[test]
    fn newlines_unicode_and_internal_entities_preserve_tree_and_positions() {
        for newline in ["\n", "\r", "\r\n"] {
            let source = format!(
                "<?xml version='1.0'?>{newline}<!--note-->{newline}<!DOCTYPE a [<!ENTITY text 'caf&#233;'>]>{newline}<a xmlns:p='urn:p'>{newline}<p:β xmlns:p='urn:p'>&text;🙂</p:β></a>"
            );
            let document = parse(&source).unwrap();
            let element = document
                .preorder()
                .filter_map(|id| document.node(id)?.as_element())
                .find(|element| element.tag_name() == "β")
                .unwrap();
            assert_eq!(element.attributes().len(), 1);
            assert!(document.preorder().any(|id| matches!(document.node(id).unwrap().kind(), NodeKind::Text(text) if text == "café🙂")));
            document.validate().unwrap();
            let failure = parse(&format!("<a>{newline}<b></a>")).unwrap_err();
            assert_eq!(failure.position.line, 2);
        }
    }

    #[test]
    fn xhtml_attribute_lookup_remains_case_sensitive() {
        let document =
            parse("<html xmlns='http://www.w3.org/1999/xhtml' Attr='x' attr='y'/>").unwrap();
        let element = document
            .preorder()
            .find_map(|id| document.node(id)?.as_element())
            .unwrap();
        assert_eq!(element.attribute("Attr"), Some("x"));
        assert_eq!(element.attribute("attr"), Some("y"));
        assert_eq!(element.attribute("ATTR"), None);
    }
}
