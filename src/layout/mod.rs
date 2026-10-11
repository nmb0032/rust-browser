//! CSS block/inline box generation with anonymous blocks and inline
//! continuations, CSS2 normal-flow horizontal width equations, and adjoining
//! vertical margin sets. Height sizing and text flow remain limited.
//! Consumes computed styles, never CSS source.

mod block;
mod geometry;
mod inline;
mod replaced;
mod tree;

pub use geometry::{ActualRect, Rect};
pub use tree::{
    BoxKind, LayoutBox, LayoutTree, ListMarker, TextFragment, build_layout_tree,
    build_layout_tree_with_resources, build_layout_tree_with_styles,
};

#[cfg(test)]
mod collapse_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod width_tests;
