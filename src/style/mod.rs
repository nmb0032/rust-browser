//! Milestone-4 styling, independent of layout and paint.
//!
//! Unsupported syntax is reported and ignored. The existing syntax parser still
//! rejects a whole malformed stylesheet/declaration list; token-level recovery,
//! at-rule semantics, external sheets, and font-face selection come later.

mod cascade;
mod computed;
mod declarations;
mod stylesheets;

pub use cascade::{StyleResolution, resolve};
pub use computed::{ComputedStyle, ComputedStyles, Display};
pub use stylesheets::{AuthorStylesheet, Diagnostic, StylesheetCollection, collect_stylesheets};

#[cfg(test)]
mod tests;
