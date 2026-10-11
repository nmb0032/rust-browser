//! Parses CSS rules into a syntax tree.
//!
//! The syntax tree retains selector and declaration-value source text, plus
//! declaration component tokens for boundary-preserving variable substitution.
//! Dedicated `selectors` and `values` modules parse their grammars
//! for the style stage. At-rules and
//! nested blocks are represented in the tree even when their semantics are
//! not implemented yet.
//!
//! The rule parser consumes CSS Syntax tokens and recovers at component-value
//! boundaries. Recovering entry points return the tree with bounded diagnostics;
//! strict compatibility entry points return the first error instead.
//! Property/selector grammars and at-rule semantics remain the style stage's job.

mod ast;
pub mod container;
pub(crate) mod control;
pub mod media;
mod parser;
pub mod selectors;
pub mod tokenizer;
pub mod values;

pub use ast::{
    AtRule, BlockItem, Declaration, ImportedStylesheet, QualifiedRule, Rule, Stylesheet,
};
pub use parser::{
    CssError, CssErrorKind, MAX_RULE_NESTING, ParseOutput, parse, parse_declarations,
    parse_declarations_recovering, parse_recovering,
};
