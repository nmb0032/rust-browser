//! CSS block/inline box generation with anonymous blocks and inline
//! continuations, CSS2 normal-flow horizontal width equations, and adjoining
//! vertical margin sets. Inline runs have baseline-aligned line boxes and
//! sliced inline decorations, Unicode line breaking and CSS whitespace.
//! Shaping and language-dependent text tailoring remain deferred.
//! Consumes computed styles, never CSS source.

mod block;
mod geometry;
mod inline;
mod replaced;
mod tree;
pub(crate) use inline::nonprinting;

pub use geometry::{ActualRect, Rect};
pub use tree::{
    BoxKind, InlineFragment, LayoutBox, LayoutTree, LineBox, ListMarker, TextFragment,
    build_layout_tree, build_layout_tree_with_resources, build_layout_tree_with_styles,
};

#[cfg(test)]
mod collapse_tests;
#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod line_break_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod width_tests;
