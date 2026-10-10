//! Styling, independent of resource loading, layout, and paint.
//!
//! Unsupported syntax is reported and ignored. The existing syntax parser still
//! rejects a whole malformed stylesheet/declaration list; token-level recovery,
//! at-rule semantics and font-face selection come later.

mod cascade;
mod computed;
mod declarations;
mod stylesheets;

pub use cascade::{StyleResolution, resolve, resolve_with_stylesheets};
pub use computed::{ComputedStyle, ComputedStyles, Display};
pub use stylesheets::{
    AuthorStylesheet, Diagnostic, StylesheetCollection, collect_stylesheets,
    collect_stylesheets_with_external, embedded_stylesheet_source,
};

#[cfg(test)]
mod tests;
