//! Parses CSS rules into a syntax tree.
//!
//! The syntax tree retains selector and declaration-value source text.
//! Dedicated `selectors` and `values` modules parse the milestone-4 subset
//! for the style stage. At-rules and
//! nested blocks are represented in the tree even when their semantics are
//! not implemented yet.
//!
//! This parser is not yet the full CSS Syntax specification: it does not
//! consume the spec tokenizer or perform browser-style error recovery.
//! `tokenizer` independently implements the CSS Syntax lexical stage.

mod ast;
mod parser;
pub mod selectors;
pub mod tokenizer;
pub mod values;

pub use ast::{AtRule, BlockItem, Declaration, QualifiedRule, Rule, Stylesheet};
pub use parser::{CssError, CssErrorKind, parse, parse_declarations};

pub(crate) fn without_comments(source: &str) -> Option<String> {
    let mut result = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut quote = None;
    while let Some(ch) = chars.next() {
        if quote.is_none() && ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut terminated = false;
            while let Some(ch) = chars.next() {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    terminated = true;
                    break;
                }
            }
            if !terminated {
                return None;
            }
        } else {
            result.push(ch);
            if ch == '\\' {
                if let Some(escaped) = chars.next() {
                    result.push(escaped);
                }
            } else if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() && matches!(ch, '\'' | '"') {
                quote = Some(ch);
            }
        }
    }
    Some(result)
}
