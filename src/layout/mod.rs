//! CSS block/inline box generation with anonymous blocks and inline
//! continuations, CSS2 normal-flow horizontal width equations, and adjoining
//! vertical margin sets. Inline runs have baseline-aligned line boxes and
//! sliced inline decorations, Unicode line breaking and CSS whitespace.
//! Positioned boxes use padding/inline containing blocks and viewport fixed
//! space. Shared CSS2 stacking phases drive both paint and reverse-order hits.
//! Padding-edge overflow clips and document-owned nested scroll offsets are
//! carried as spatial ancestry independently of flattened stacking groups.
//! Paragraph bidi and advanced language-dependent text tailoring are deferred.
//! Consumes computed styles, never CSS source.

mod block;
mod geometry;
mod inline;
mod overflow;
mod positioning;
mod replaced;
mod stacking;
mod tree;

pub use geometry::{ActualRect, Rect};
pub use overflow::{
    ClipRect, ScrollContainer, ScrollOffset, ScrollRange, ScrollState, Scrollbar, SpatialState,
};
pub(crate) use stacking::{PaintPhase, paint_order};
pub use tree::{
    BoxKind, CoordinateSpace, InlineFragment, LayoutBox, LayoutTree, LineBox, ListMarker,
    TextFragment, build_layout_tree, build_layout_tree_with_resources,
    build_layout_tree_with_styles,
};

#[cfg(test)]
mod collapse_tests;
#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod line_break_tests;
#[cfg(test)]
mod overflow_tests;
#[cfg(test)]
mod positioning_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod width_tests;
