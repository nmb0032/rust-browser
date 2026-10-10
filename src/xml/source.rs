use crate::html::tokenizer::SourcePosition;

pub(super) struct Locator<'a> {
    source: &'a str,
    offset: usize,
    row: usize,
    column: usize,
}

impl<'a> Locator<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            row: 0,
            column: 0,
        }
    }

    pub fn position(&mut self, row: usize, column: usize) -> SourcePosition {
        if (row, column) < (self.row, self.column) {
            self.offset = 0;
            self.row = 0;
            self.column = 0;
        }
        while (self.row, self.column) < (row, column) {
            let Some(ch) = self.source[self.offset..].chars().next() else {
                break;
            };
            self.offset += ch.len_utf8();
            if ch == '\n' {
                self.row += 1;
                self.column = 0;
            } else {
                self.column += 1;
            }
        }
        SourcePosition {
            byte_offset: self.offset,
            line: row + 1,
            column: column + 1,
        }
    }
}

pub(super) fn normalized_position(source: &str, mut position: SourcePosition) -> SourcePosition {
    position.line = 1;
    position.column = 1;
    let mut cr = false;
    for ch in source[..position.byte_offset].chars() {
        if ch == '\r' || ch == '\n' && !cr {
            position.line += 1;
            position.column = 1;
        } else if !(ch == '\n' && cr) {
            position.column += ch.len_utf16();
        }
        cr = ch == '\r';
    }
    position
}

pub(super) fn namespace_prefixes(source: &str, position: usize) -> Option<Vec<&str>> {
    let start = if source.as_bytes().get(position) == Some(&b'<') {
        position
    } else {
        source[..position].rfind('<')?
    };
    let mut rest = source[start..].strip_prefix('<')?;
    if rest.starts_with(['!', '?', '/']) {
        return None;
    }
    let end = rest.find(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '/' | '>'))?;
    rest = &rest[end..];
    let mut prefixes = Vec::new();
    loop {
        rest = rest.trim_start_matches(['\t', '\n', '\r', ' ']);
        if rest.starts_with(['/', '>']) {
            let end = source.len() - rest.len() + usize::from(rest.starts_with('/'));
            return (position <= end).then_some(prefixes);
        }
        let end = rest.find(|ch: char| ch.is_ascii_whitespace() || ch == '=')?;
        let name = &rest[..end];
        rest = rest[end..]
            .trim_start_matches(['\t', '\n', '\r', ' '])
            .strip_prefix('=')?
            .trim_start_matches(['\t', '\n', '\r', ' ']);
        let quote = rest.chars().next()?;
        if !matches!(quote, '"' | '\'') {
            return None;
        }
        rest = &rest[1..];
        rest = &rest[rest.find(quote)? + 1..];
        if name == "xmlns" {
            prefixes.push("");
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            prefixes.push(prefix);
        }
    }
}
