//! Parses CSS rules into a syntax tree.
//!
//! Selectors and declaration values are retained as source text so they can
//! be parsed by dedicated selector and value parsers later. At-rules and
//! nested blocks are represented in the tree even when their semantics are
//! not implemented yet.
//!
//! This parser is not yet the full CSS Syntax specification: it does not
//! tokenize selector/value grammars or perform browser-style error recovery.

mod ast;
mod parser;

pub use ast::{AtRule, BlockItem, Declaration, QualifiedRule, Rule, Stylesheet};
pub use parser::{CssError, CssErrorKind, parse};
