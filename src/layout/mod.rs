//! Naive block/inline layout consuming computed styles, never CSS source.

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
mod tests;
