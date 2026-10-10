//! Encoding selection before tokenization (HTML 13.2.3 and the Encoding Standard).

use super::parser::{HtmlError, HtmlParser, ParsedHtml};
use crate::dom::Document;
use encoding_rs::{Decoder, Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252, X_USER_DEFINED};
use std::collections::HashSet;

pub fn mime_essence(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

pub fn mime_charset(content_type: &str) -> Option<&'static Encoding> {
    let mut rest = content_type.split_once(';')?.1;
    loop {
        rest = rest.trim_start_matches(['\t', '\n', '\r', ' ']);
        let end = rest.find(['=', ';']).unwrap_or(rest.len());
        let name = &rest[..end];
        rest = &rest[end..];
        if let Some(value) = rest.strip_prefix('=') {
            rest = value;
            let mut value = String::new();
            if let Some(tail) = rest.strip_prefix('"') {
                rest = tail;
                while let Some(ch) = rest.chars().next() {
                    rest = &rest[ch.len_utf8()..];
                    if ch == '"' {
                        break;
                    }
                    if ch == '\\' {
                        if let Some(escaped) = rest.chars().next() {
                            value.push(escaped);
                            rest = &rest[escaped.len_utf8()..];
                        }
                    } else {
                        value.push(ch);
                    }
                }
            } else {
                let end = rest.find(';').unwrap_or(rest.len());
                value = rest[..end].trim_end_matches(['\t', '\n', '\r', ' ']).into();
                rest = &rest[end..];
            }
            if name.eq_ignore_ascii_case("charset") {
                return Encoding::for_label(value.as_bytes());
            }
        }
        rest = rest.split_once(';')?.1;
    }
}

pub(crate) fn meta_encoding<'a>(
    attributes: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<&'static Encoding> {
    let mut pragma = false;
    let mut need_pragma = None;
    let mut charset = None;
    let mut seen = HashSet::new();
    for (name, value) in attributes {
        if !seen.insert(name) {
            continue;
        }
        match name {
            "http-equiv" if value.eq_ignore_ascii_case("content-type") => pragma = true,
            "charset" => {
                charset = Encoding::for_label(value.as_bytes());
                need_pragma = Some(false);
            }
            "content" if need_pragma.is_none() => {
                if let Some(encoding) = content_encoding(value) {
                    charset = Some(encoding);
                    need_pragma = Some(true);
                }
            }
            _ => {}
        }
    }
    if need_pragma.is_none() || need_pragma == Some(true) && !pragma {
        return None;
    }
    charset.map(normalize_meta)
}

fn content_encoding(content: &str) -> Option<&'static Encoding> {
    let lower = content.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(index) = rest.find("charset") {
        rest = &rest[index + 7..];
        let value = rest.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
        let Some(value) = value.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
        let label = if let Some(quote) = value.chars().next().filter(|ch| matches!(ch, '"' | '\''))
        {
            let value = &value[1..];
            &value[..value.find(quote)?]
        } else {
            &value[..value
                .find(|ch: char| ch.is_ascii_whitespace() || ch == ';')
                .unwrap_or(value.len())]
        };
        return Encoding::for_label(label.as_bytes());
    }
    None
}

fn normalize_meta(encoding: &'static Encoding) -> &'static Encoding {
    if encoding == UTF_16LE || encoding == UTF_16BE {
        UTF_8
    } else if encoding == X_USER_DEFINED {
        WINDOWS_1252
    } else {
        encoding
    }
}

pub fn prescan(bytes: &[u8]) -> Option<&'static Encoding> {
    prescan_tags(bytes).or_else(|| xml_declaration_encoding(&bytes[..bytes.len().min(1024)]))
}

fn html_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

fn prescan_tags(bytes: &[u8]) -> Option<&'static Encoding> {
    let bytes = &bytes[..bytes.len().min(1024)];
    if bytes.starts_with(b"<\0?\0x\0") {
        return Some(UTF_16LE);
    }
    if bytes.starts_with(b"\0<\0?\0x") {
        return Some(UTF_16BE);
    }
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"<!--") {
            let end = bytes[index + 2..]
                .windows(3)
                .position(|window| window == b"-->")?;
            index += 2 + end + 3;
            continue;
        }
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        let begin = index;
        index += 1;
        let end_tag = bytes.get(index) == Some(&b'/');
        if end_tag {
            index += 1;
        }
        if bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
            let name_start = index;
            while bytes
                .get(index)
                .is_some_and(|byte| !html_space(*byte) && !matches!(byte, b'/' | b'>'))
            {
                index += 1;
            }
            let meta = !end_tag && bytes[name_start..index].eq_ignore_ascii_case(b"meta");
            let mut attributes = Vec::new();
            loop {
                while bytes
                    .get(index)
                    .is_some_and(|byte| html_space(*byte) || *byte == b'/')
                {
                    index += 1;
                }
                if *bytes.get(index)? == b'>' {
                    index += 1;
                    break;
                }
                let start = index;
                if bytes.get(index) == Some(&b'=') {
                    index += 1;
                }
                while bytes
                    .get(index)
                    .is_some_and(|byte| !html_space(*byte) && !matches!(byte, b'=' | b'/' | b'>'))
                {
                    index += 1;
                }
                if index == start {
                    index += 1;
                    continue;
                }
                let name = String::from_utf8_lossy(&bytes[start..index]).to_ascii_lowercase();
                while bytes.get(index).is_some_and(|byte| html_space(*byte)) {
                    index += 1;
                }
                let mut value = String::new();
                if *bytes.get(index)? == b'=' {
                    index += 1;
                    while bytes.get(index).is_some_and(|byte| html_space(*byte)) {
                        index += 1;
                    }
                    let quote = *bytes.get(index)?;
                    if matches!(quote, b'"' | b'\'') {
                        index += 1;
                        let start = index;
                        while *bytes.get(index)? != quote {
                            index += 1;
                        }
                        value = String::from_utf8_lossy(&bytes[start..index]).to_ascii_lowercase();
                        index += 1;
                    } else {
                        let start = index;
                        while bytes
                            .get(index)
                            .is_some_and(|byte| !html_space(*byte) && *byte != b'>')
                        {
                            index += 1;
                        }
                        value = String::from_utf8_lossy(&bytes[start..index]).to_ascii_lowercase();
                    }
                }
                attributes.push((name, value));
            }
            if meta
                && let Some(encoding) = meta_encoding(
                    attributes
                        .iter()
                        .map(|(name, value)| (name.as_str(), value.as_str())),
                )
            {
                return Some(encoding);
            }
        } else {
            index = begin + bytes[begin..].iter().position(|byte| *byte == b'>')? + 1;
        }
    }
    None
}

pub fn xml_declaration_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    Encoding::for_label(xml_declaration_label(bytes)?)
}

pub(crate) fn xml_declaration_label(bytes: &[u8]) -> Option<&[u8]> {
    let declaration = bytes.strip_prefix(b"<?xml")?;
    if !declaration
        .first()
        .is_some_and(|byte| matches!(byte, b'\t' | b'\n' | b'\r' | b' '))
    {
        return None;
    }
    let declaration = &declaration[..declaration.iter().position(|byte| *byte == b'>')?];
    let index = declaration
        .windows(8)
        .position(|window| window == b"encoding")?;
    let rest = declaration[index + 8..]
        .strip_prefix(b" ")
        .unwrap_or(&declaration[index + 8..]);
    let rest = rest
        .trim_ascii_start()
        .strip_prefix(b"=")?
        .trim_ascii_start();
    let quote = *rest.first()?;
    if !matches!(quote, b'"' | b'\'') {
        return None;
    }
    let label = &rest[1..1 + rest[1..].iter().position(|byte| *byte == quote)?];
    Some(label)
}

pub struct DecodedHtml {
    pub parsed: ParsedHtml,
    pub encoding: &'static Encoding,
    pub had_replacements: bool,
}

pub struct HtmlByteParser {
    parser: HtmlParser,
    decoder: Option<Decoder>,
    encoding: &'static Encoding,
    certain: bool,
    header: Option<&'static Encoding>,
    history: Vec<u8>,
    revision: usize,
    had_replacements: bool,
}

impl HtmlByteParser {
    pub fn new(content_type: Option<&str>) -> Self {
        Self {
            parser: HtmlParser::new(),
            decoder: None,
            encoding: WINDOWS_1252,
            certain: false,
            header: content_type.and_then(mime_charset),
            history: Vec::new(),
            revision: 0,
            had_replacements: false,
        }
    }
    pub fn document(&self) -> &Document {
        self.parser.document()
    }
    pub fn revision(&self) -> usize {
        self.revision
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), HtmlError> {
        let previous = self.parser.revision();
        if !self.certain || self.decoder.is_none() {
            self.history.extend_from_slice(bytes);
        }
        if self.decoder.is_none() {
            if self.history.len() < 3
                || self.history.starts_with(b"<\0") && self.history.len() < 6
                || self.history.starts_with(b"\0<") && self.history.len() < 6
                || (b"<?xml".starts_with(&self.history) || self.history.starts_with(b"<?xml"))
                    && self.history.len() < 1024
                    && !self.history.contains(&b'>')
            {
                return Ok(());
            }
            self.start_decoder();
            let history = self.history.clone();
            self.decode(&history, false)?;
        } else {
            self.decode(bytes, false)?;
        }
        let restarted = self.check_declaration()?;
        if restarted || self.parser.revision() != previous {
            self.revision += 1;
        }
        if self.certain {
            self.history.clear();
        }
        Ok(())
    }
    fn start_decoder(&mut self) {
        let bom = Encoding::for_bom(&self.history).map(|(encoding, _)| encoding);
        self.encoding = bom
            .or(self.header)
            .or_else(|| prescan(&self.history))
            .unwrap_or(WINDOWS_1252);
        self.certain = bom.is_some() || self.header.is_some();
        self.decoder = Some(self.encoding.new_decoder_with_bom_removal());
    }
    fn decode(&mut self, bytes: &[u8], last: bool) -> Result<(), HtmlError> {
        let decoder = self.decoder.as_mut().expect("selected encoding");
        let mut output = String::with_capacity(
            decoder
                .max_utf8_buffer_length(bytes.len())
                .expect("bounded response size"),
        );
        let (_, read, replacements) = decoder.decode_to_string(bytes, &mut output, last);
        assert_eq!(read, bytes.len(), "decoder buffer covers bounded input");
        self.had_replacements |= replacements;
        self.parser.feed(&output)
    }
    fn check_declaration(&mut self) -> Result<bool, HtmlError> {
        let Some(encoding) = self.parser.take_encoding_declaration() else {
            return Ok(false);
        };
        if self.certain {
            return Ok(false);
        }
        self.certain = true;
        if encoding == self.encoding || self.encoding == UTF_16BE || self.encoding == UTF_16LE {
            return Ok(false);
        }
        self.encoding = encoding;
        self.decoder = Some(encoding.new_decoder_without_bom_handling());
        self.parser = HtmlParser::new();
        self.had_replacements = false;
        let history = self.history.clone();
        self.decode(&history, false)?;
        Ok(true)
    }
    pub fn finish(mut self) -> Result<DecodedHtml, HtmlError> {
        if self.decoder.is_none() {
            self.start_decoder();
            let history = self.history.clone();
            self.decode(&history, false)?;
        }
        self.check_declaration()?;
        self.decode(&[], true)?;
        if self.check_declaration()? {
            self.decode(&[], true)?;
        }
        let mut parsed = self.parser.finish_with_diagnostics()?;
        parsed.document.set_encoding(self.encoding.name());
        Ok(DecodedHtml {
            parsed,
            encoding: self.encoding,
            had_replacements: self.had_replacements,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::NodeKind;
    fn text(document: &Document) -> String {
        document
            .preorder()
            .filter_map(|id| match document.node(id)?.kind() {
                NodeKind::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn sniffing_preserves_precedence_and_skips_comments_and_attributes() {
        assert_eq!(
            mime_charset("text/html; note=\";charset=windows-1252\"; charset=utf-8"),
            Some(UTF_8)
        );
        assert_eq!(mime_charset("text/html; charset=\"utf\\-8\""), Some(UTF_8));
        assert_eq!(
            mime_charset("text/html; charset=invalid; charset=utf-8"),
            None
        );
        assert_eq!(prescan(b"</meta charset=utf-8>"), None);
        assert_eq!(prescan(b"<meta =charset=utf-8>"), None);
        assert_eq!(prescan(b"<meta\x0Bcharset=utf-8>"), None);
        assert_eq!(
            prescan(b"<!-- <meta charset=utf-8> --><meta charset=shift_jis>"),
            Some(encoding_rs::SHIFT_JIS)
        );
        assert_eq!(
            prescan(b"<p title='<meta charset=utf-8>'><meta charset=windows-1252>"),
            Some(WINDOWS_1252)
        );
        assert_eq!(prescan(b"<meta content='text/html; charset=utf-8'>"), None);
        assert_eq!(
            prescan(b"<meta content='text/html; charset=utf-8' http-equiv=Content-Type>"),
            Some(UTF_8)
        );
        assert_eq!(prescan(b"<meta charset=utf-16>"), Some(UTF_8));
        assert_eq!(
            prescan(b"<meta charset=x-user-defined>"),
            Some(WINDOWS_1252)
        );
    }
    #[test]
    fn legacy_bom_and_malformed_decoding_work_at_every_byte_boundary() {
        for (source, header, expected) in [
            (b"<p>caf\xe9".as_slice(), None, "café"),
            (
                b"<meta charset=utf-8><p>caf\xc3\xa9".as_slice(),
                None,
                "café",
            ),
            (
                b"\xef\xbb\xbf<p>caf\xc3\xa9".as_slice(),
                Some("text/html; charset=windows-1252"),
                "café",
            ),
            (
                b"<p>\xff\xc3".as_slice(),
                Some("text/html; charset=utf-8"),
                "\u{FFFD}\u{FFFD}",
            ),
        ] {
            for size in 1..=source.len() {
                let mut parser = HtmlByteParser::new(header);
                for chunk in source.chunks(size) {
                    parser.feed(chunk).unwrap();
                }
                assert_eq!(
                    text(&parser.finish().unwrap().parsed.document),
                    expected,
                    "size {size}"
                );
            }
        }
    }
    #[test]
    fn late_meta_restarts_from_original_bytes_and_does_not_override_transport() {
        let mut source = b"<p>caf\xe9</p>".to_vec();
        source.extend(vec![b' '; 1100]);
        source.extend_from_slice(b"<meta charset=utf-8><p>caf\xc3\xa9</p>");
        for size in [1, 7, 8192] {
            let mut parser = HtmlByteParser::new(None);
            for chunk in source.chunks(size) {
                parser.feed(chunk).unwrap();
            }
            let decoded = parser.finish().unwrap();
            assert_eq!(decoded.encoding, UTF_8);
            assert!(decoded.had_replacements);
            assert!(text(&decoded.parsed.document).starts_with("caf\u{FFFD}"));
            assert!(text(&decoded.parsed.document).ends_with("café"));
        }
    }

    #[test]
    fn first_encoding_declaration_wins_and_utf16_decoders_retain_split_code_units() {
        for source in [
            b"<meta charset=utf-8><meta charset=windows-1252><p>caf\xc3\xa9".to_vec(),
            [
                vec![b' '; 1100],
                b"<meta charset=utf-8><meta charset=windows-1252><p>caf\xc3\xa9".to_vec(),
            ]
            .concat(),
        ] {
            for size in [1, 7, 8192] {
                let mut parser = HtmlByteParser::new(None);
                for chunk in source.chunks(size) {
                    parser.feed(chunk).unwrap();
                }

                let decoded = parser.finish().unwrap();
                assert_eq!(decoded.encoding, UTF_8);
                assert!(text(&decoded.parsed.document).ends_with("café"));
            }
        }
        for big_endian in [false, true] {
            let mut bytes = if big_endian {
                vec![0xFE, 0xFF]
            } else {
                vec![0xFF, 0xFE]
            };
            bytes.extend("<p>café🙂".encode_utf16().flat_map(|unit| {
                if big_endian {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                }
            }));
            for size in 1..=bytes.len() {
                let mut parser = HtmlByteParser::new(Some("text/html; charset=windows-1252"));
                for chunk in bytes.chunks(size) {
                    parser.feed(chunk).unwrap();
                }
                assert_eq!(text(&parser.finish().unwrap().parsed.document), "café🙂");
            }
        }
    }

    #[test]
    fn xml_declaration_fallback_is_independent_of_transport_chunks() {
        let source = b"<?xml version='1.0' encoding='utf-8'?><p>caf\xc3\xa9";
        for size in 1..=source.len() {
            let mut parser = HtmlByteParser::new(None);
            for chunk in source.chunks(size) {
                parser.feed(chunk).unwrap();
            }
            let decoded = parser.finish().unwrap();
            assert_eq!(decoded.encoding, UTF_8);
            assert_eq!(text(&decoded.parsed.document), "café");
        }
        assert_eq!(prescan(b"<?xml-stylesheet encoding='utf-8'?>"), None);
    }
}
