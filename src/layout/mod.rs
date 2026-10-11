//! CSS block/inline box generation with anonymous blocks and inline
//! continuations, CSS2 normal-flow horizontal width equations, and adjoining
//! vertical margin sets. Inline runs have baseline-aligned line boxes and
//! sliced inline decorations, Unicode line breaking and CSS whitespace.
//! Positioned boxes use padding/inline containing blocks and viewport fixed
//! space. Shared CSS2 stacking phases drive both paint and reverse-order hits.
//! Padding-edge overflow clips and document-owned nested scroll offsets are
//! carried as spatial ancestry independently of flattened stacking groups.
//! Flex containers share intrinsic/block sizing helpers, iterative flexible
//! lengths, wrapped/reversed axes, baseline alignment and atomic inline-flex.
//! Grid adds bounded named/implicit tracks, phased dense/sparse placement,
//! spanning intrinsic growth, fractional allocation and atomic inline-grid.
//! Floats share source-ordered BFC exclusions with dynamically shaped lines.
//! Tables add anonymous fixup, intrinsic/fixed columns, bounded spans/captions,
//! row baselines, separate spacing and single-painted collapsed border edges.
//! Paragraph bidi and advanced language-dependent text tailoring are deferred.
//! Consumes computed styles, never CSS source.

mod bidi;
mod block;
mod counters;
mod flex;
mod floats;
mod geometry;
mod grid;
mod inline;
mod overflow;
mod positioning;
mod replaced;
mod sizing;
mod stacking;
mod table;
mod tree;
#[cfg(test)]
mod typography_tests;
mod writing;

pub use geometry::{ActualRect, Rect};
pub use overflow::{
    ClipRect, ScrollContainer, ScrollOffset, ScrollRange, ScrollState, Scrollbar, SpatialState,
};
pub(crate) use stacking::{PaintPhase, paint_order};
pub(crate) use tree::build_layout_tree_with_marker_resources;
pub use tree::{
    BoxKind, CoordinateSpace, InlineFragment, LayoutBox, LayoutTree, LineBox, ListMarker,
    TextFragment, build_layout_tree, build_layout_tree_with_resources,
    build_layout_tree_with_styles,
};

#[cfg(test)]
mod collapse_tests;
#[cfg(test)]
mod display_sizing_tests;
#[cfg(test)]
mod flex_tests;
#[cfg(test)]
mod floats_tables_tests;
#[cfg(test)]
mod grid_tests;
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
mod transform;
#[cfg(test)]
mod width_tests;
#[cfg(test)]
mod writing_bidi_tests;
pub use transform::Mat4;
#[cfg(test)]
mod effects_tests;
