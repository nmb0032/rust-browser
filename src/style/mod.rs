//! Styling, independent of resource loading, layout, and paint.
//!
//! Unsupported syntax is reported and ignored. Token-level syntax recovery keeps
//! valid rules and declarations in embedded, external, and inline CSS. At-rule
//! semantics and font-face selection come later.

mod cascade;
mod computed;
mod declarations;
mod stylesheets;

pub use cascade::{StyleResolution, resolve, resolve_with_stylesheets};
pub use computed::{ComputedStyle, ComputedStyles, Display};
pub(crate) use stylesheets::report_css_errors;
pub use stylesheets::{
    AuthorStylesheet, Diagnostic, StylesheetCollection, collect_stylesheets,
    collect_stylesheets_with_external, embedded_stylesheet_source,
};

#[cfg(test)]
mod tests;
