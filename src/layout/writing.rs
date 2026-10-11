//! Formatting-context axis conversion. Only a context's own lines and inline
//! fragments are finalized here; orthogonal/block descendants lay out natively.
use super::{
    BoxKind, LayoutBox, LineBox, Rect,
    tree::{finite_rect, finite_used, resolve_edges, resolve_length, resolve_margins},
};
use crate::{
    css::values::Edges,
    style::{ComputedStyle, Diagnostic, WritingMode},
    text::{FontContext, GlyphRotation},
};

#[derive(Clone, Copy)]
pub(super) struct FlowAxes {
    pub mode: WritingMode,
    pub origin: (f32, f32),
    pub block: f32,
    pub inline: f32,
}

impl FlowAxes {
    pub fn point(self, inline: f32, block: f32) -> (f32, f32) {
        let x = if self.mode.block_reverse() {
            self.block - block
        } else {
            block
        };
        let y = if self.mode == WritingMode::SidewaysLr {
            self.inline - inline
        } else {
            inline
        };
        (self.origin.0 + x, self.origin.1 + y)
    }
    pub fn rect(self, r: Rect) -> Rect {
        let a = self.point(r.x, r.y);
        let b = self.point(r.x + r.width, r.y + r.height);
        Rect {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            width: (a.0 - b.0).abs(),
            height: (a.1 - b.1).abs(),
        }
    }
    pub fn logical_edges<T: Clone>(self, e: &Edges<T>) -> Edges<T> {
        Edges {
            top: if self.mode.block_reverse() {
                &e.right
            } else {
                &e.left
            }
            .clone(),
            bottom: if self.mode.block_reverse() {
                &e.left
            } else {
                &e.right
            }
            .clone(),
            left: if self.mode == WritingMode::SidewaysLr {
                &e.bottom
            } else {
                &e.top
            }
            .clone(),
            right: if self.mode == WritingMode::SidewaysLr {
                &e.top
            } else {
                &e.bottom
            }
            .clone(),
        }
    }
    pub fn physical_edges<T: Clone>(self, e: &Edges<T>) -> Edges<T> {
        Edges {
            top: if self.mode == WritingMode::SidewaysLr {
                &e.right
            } else {
                &e.left
            }
            .clone(),
            bottom: if self.mode == WritingMode::SidewaysLr {
                &e.left
            } else {
                &e.right
            }
            .clone(),
            left: if self.mode.block_reverse() {
                &e.bottom
            } else {
                &e.top
            }
            .clone(),
            right: if self.mode.block_reverse() {
                &e.top
            } else {
                &e.bottom
            }
            .clone(),
        }
    }
    pub fn line(
        self,
        line: &mut LineBox,
        node: Option<crate::dom::NodeId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let baseline = self.point(line.rect.x, line.baseline).0;
        line.rect = finite_rect(self.rect(line.rect), node, diagnostics);
        line.content_rect = finite_rect(self.rect(line.content_rect), node, diagnostics);
        line.baseline = finite_used(baseline as f64, node, "vertical line baseline", diagnostics);
    }
}

pub(super) fn inline_style(style: &ComputedStyle) -> ComputedStyle {
    let mut s = style.clone();
    if s.writing_mode.vertical() {
        let axes = FlowAxes {
            mode: s.writing_mode,
            origin: (0.0, 0.0),
            block: 0.0,
            inline: 0.0,
        };
        s.margin = axes.logical_edges(&s.margin);
        s.padding = axes.logical_edges(&s.padding);
        s.border_width = axes.logical_edges(&s.border_width);
        std::mem::swap(&mut s.width, &mut s.height);
        std::mem::swap(&mut s.min_width, &mut s.min_height);
        std::mem::swap(&mut s.max_width, &mut s.max_height);
    }
    s
}

fn finalize_atomic(b: &mut LayoutBox, axes: FlowAxes, diagnostics: &mut Vec<Diagnostic>) {
    if b.normal_block() || b.atomic_inline() || b.floating() {
        let logical = Rect {
            width: b.rect.height,
            height: b.rect.width,
            ..b.rect
        };
        let target = finite_rect(axes.rect(logical), b.node_id, diagnostics);
        let dx = target.x - b.rect.x;
        super::positioning::translate(b, dx, 0.0, diagnostics);
        let offset_y = b.content_rect.y - b.rect.y;
        b.content_rect.y = offset_y;
        super::block::place_block(b, target.y as f64, diagnostics);
    }
}

fn measure_native(
    child: &mut LayoutBox,
    inline: f32,
    block: f32,
    font: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    child.reset();
    if child.style.writing_mode.vertical() {
        layout(
            child,
            0.0,
            block,
            Some(inline),
            None,
            None,
            inline,
            font,
            diagnostics,
        );
    } else {
        let intrinsic = super::positioning::intrinsic_content(child, block, font, diagnostics);
        let width = child
            .style
            .width
            .as_ref()
            .map(|l| l.resolve(child.style.font_size, block))
            .unwrap_or(block.max(intrinsic.0).min(intrinsic.1));
        super::block::layout_sized_in(
            child,
            block,
            width,
            None,
            (true, false),
            inline,
            font,
            diagnostics,
        );
    }
}

/// The allocation solver sees geometry-only measurement boxes in logical
/// axes. Real children are always re-laid out in their own writing mode; no
/// descendants or glyph masks are rotated to approximate orthogonal layout.
fn allocate(
    b: &mut LayoutBox,
    inline: f32,
    block: Option<f32>,
    available_block: f32,
    font: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, Vec<(usize, Rect)>) {
    let mut oracle = b.clone();
    *oracle.style = inline_style(&b.style);
    oracle.style.writing_mode = WritingMode::HorizontalTb;
    oracle.definite_height = block;
    oracle.definite_width = true;
    oracle.line_boxes.clear();
    for (child, proxy) in b.children.iter_mut().zip(&mut oracle.children) {
        if child.style.position.out_of_flow() {
            proxy.children.clear();
            *proxy.style = inline_style(&child.style);
            proxy.style.writing_mode = WritingMode::HorizontalTb;
            continue;
        }
        measure_native(
            child,
            inline,
            block.unwrap_or(available_block),
            font,
            diagnostics,
        );
        let intrinsic = if child.style.writing_mode.vertical()
            && !child.style.display.is_item_container()
            && !child.style.display.is_table()
            && child.image.is_none()
        {
            super::inline::intrinsic_inline(
                &child.children,
                &child.style,
                inline,
                font,
                diagnostics,
            )
        } else {
            (child.content_rect.height, child.content_rect.height)
        };
        *proxy.style = inline_style(&child.style);
        // Projection applies to physical declarations, not to glyph shaping:
        // these disposable boxes have no text, images or descendants.
        if !child.style.writing_mode.vertical() {
            std::mem::swap(&mut proxy.style.width, &mut proxy.style.height);
            std::mem::swap(&mut proxy.style.min_width, &mut proxy.style.min_height);
            std::mem::swap(&mut proxy.style.max_width, &mut proxy.style.max_height);
            let axes = FlowAxes {
                mode: b.style.writing_mode,
                origin: (0.0, 0.0),
                block: 0.0,
                inline,
            };
            proxy.style.margin = axes.logical_edges(&child.style.margin);
            proxy.style.padding = axes.logical_edges(&child.style.padding);
            proxy.style.border_width = axes.logical_edges(&child.style.border_width);
        }
        proxy.style.writing_mode = WritingMode::HorizontalTb;
        proxy.children.clear();
        proxy.image = None;
        proxy.text = None;
        proxy.text_fragments.clear();
        proxy.inline_fragments.clear();
        proxy.line_boxes.clear();
        proxy.axis_measurement = Some(Rect {
            x: intrinsic.0,
            y: intrinsic.1,
            width: child.content_rect.width,
            height: child.content_rect.width * 0.5,
        });
    }
    let mut natural = 0.0;
    let mut stable = false;
    for _ in 0..4 {
        for child in &mut oracle.children {
            child.reset();
        }
        natural = if b.style.display.is_grid() {
            super::grid::layout_grid(
                &mut oracle,
                (0.0, 0.0, inline),
                block,
                block,
                (0.0, f32::MAX),
                font,
                diagnostics,
            )
        } else {
            super::flex::layout_flex(
                &mut oracle,
                (0.0, 0.0, inline),
                block,
                block,
                font,
                diagnostics,
            )
        };
        stable = true;
        for (child, proxy) in b.children.iter_mut().zip(&mut oracle.children) {
            if child.style.position.out_of_flow() {
                continue;
            }
            child.reset();
            super::block::layout_sized_in(
                child,
                block.unwrap_or(available_block),
                proxy.content_rect.height,
                Some(proxy.content_rect.width),
                (proxy.definite_height.is_some(), proxy.definite_width),
                inline,
                font,
                diagnostics,
            );
            // Feedback uses the actual text's natural block extent, not the
            // allocated/stretch size. This matters for vertical line wrapping.
            if let Some(m) = &mut proxy.axis_measurement
                && child.style.writing_mode.vertical()
                && child.style.width.is_none()
                && !child.style.display.is_item_container()
            {
                let content_style = child.style.clone();
                let (_, extent) = super::inline::layout_inline_run(
                    &mut child.children.clone(),
                    &content_style,
                    child.node_id,
                    (0.0, 0.0, proxy.content_rect.width, None),
                    font,
                    diagnostics,
                    None,
                );
                if (m.width - extent as f32).abs() > 0.01 {
                    m.width = extent as f32;
                    m.height = m.width * 0.5;
                    stable = false;
                }
            }
        }
        if stable {
            break;
        }
    }
    if !stable {
        diagnostics.push(Diagnostic {
            node_id: b.node_id,
            message: "orthogonal flex/grid intrinsic feedback reached the four-pass limit".into(),
        });
    }
    let positions = oracle
        .children
        .iter()
        .enumerate()
        .map(|(i, c)| (i, c.rect))
        .collect();
    for (child, proxy) in b.children.iter_mut().zip(&oracle.children) {
        child.margin = FlowAxes {
            mode: b.style.writing_mode,
            origin: (0.0, 0.0),
            block: 0.0,
            inline,
        }
        .physical_edges(&proxy.margin);
        child.flex_static = proxy.flex_static;
        child.grid_area = proxy.grid_area;
    }
    (natural, positions)
}
fn table_oracle(
    real: &mut LayoutBox,
    proxy: &mut LayoutBox,
    axes: FlowAxes,
    inline: f32,
    block: f32,
    font: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    *proxy.style = real.style.as_ref().clone();
    proxy.style.margin = axes.logical_edges(&real.style.margin);
    proxy.style.padding = axes.logical_edges(&real.style.padding);
    proxy.style.border_width = axes.logical_edges(&real.style.border_width);
    proxy.style.border_style = axes.logical_edges(&real.style.border_style);
    proxy.style.borders = axes.logical_edges(&real.style.borders);
    std::mem::swap(&mut proxy.style.width, &mut proxy.style.height);
    std::mem::swap(&mut proxy.style.min_width, &mut proxy.style.min_height);
    std::mem::swap(&mut proxy.style.max_width, &mut proxy.style.max_height);
    std::mem::swap(
        &mut proxy.style.border_spacing.0,
        &mut proxy.style.border_spacing.1,
    );
    proxy.style.writing_mode = WritingMode::HorizontalTb;
    if real.style.display == crate::style::Display::TableCell
        || real.style.display == crate::style::Display::TableCaption
    {
        measure_native(real, inline, block, font, diagnostics);
        let intrinsic = if real.style.writing_mode.vertical() {
            super::inline::intrinsic_inline(&real.children, &real.style, inline, font, diagnostics)
        } else {
            (real.content_rect.height, real.content_rect.height)
        };
        proxy.axis_measurement = Some(Rect {
            x: intrinsic.0,
            y: intrinsic.1,
            width: real.content_rect.width,
            height: real.content_rect.width * 0.5,
        });
        proxy.children.clear();
        proxy.text = None;
        proxy.image = None;
        proxy.text_fragments.clear();
        proxy.line_boxes.clear();
    } else {
        for (child, p) in real.children.iter_mut().zip(&mut proxy.children) {
            table_oracle(child, p, axes, inline, block, font, diagnostics);
        }
    }
}

fn install_table(
    real: &mut LayoutBox,
    proxy: &LayoutBox,
    axes: FlowAxes,
    parent_y: f32,
    font: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let target = finite_rect(axes.rect(proxy.rect), real.node_id, diagnostics);
    let local_y = target.y - parent_y;
    if real.style.display == crate::style::Display::TableCell
        || real.style.display == crate::style::Display::TableCaption
    {
        real.reset();
        real.used_border = proxy.used_border.map(|e| axes.physical_edges(&e));
        super::block::layout_sized_in(
            real,
            axes.block,
            proxy.content_rect.height,
            Some(proxy.content_rect.width),
            (true, true),
            axes.inline,
            font,
            diagnostics,
        );
        super::positioning::translate(real, target.x - real.rect.x, 0.0, diagnostics);
        super::block::place_block(real, local_y as f64, diagnostics);
    } else {
        real.rect = Rect {
            y: local_y,
            ..target
        };
        let content = axes.rect(proxy.content_rect);
        real.content_rect = Rect {
            y: content.y - parent_y,
            ..content
        };
        for (child, p) in real.children.iter_mut().zip(&proxy.children) {
            install_table(child, p, axes, target.y, font, diagnostics);
        }
    }
    real.collapsed_table_part = proxy.collapsed_table_part;
    real.hidden_empty_cell = proxy.hidden_empty_cell;
}

pub(super) fn finalize_inline(
    b: &mut LayoutBox,
    axes: FlowAxes,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if b.normal_block() || b.atomic_inline() || b.floating() {
        finalize_atomic(b, axes, diagnostics);
        return;
    }
    b.rect = finite_rect(axes.rect(b.rect), b.node_id, diagnostics);
    b.content_rect = finite_rect(axes.rect(b.content_rect), b.node_id, diagnostics);
    b.padding = axes.physical_edges(&b.padding);
    b.margin = axes.physical_edges(&b.margin);
    for f in &mut b.inline_fragments {
        f.rect = finite_rect(axes.rect(f.rect), b.node_id, diagnostics);
        f.content_rect = finite_rect(axes.rect(f.content_rect), b.node_id, diagnostics);
        f.baseline = finite_used(
            axes.point(0.0, f.baseline).0 as f64,
            b.node_id,
            "vertical inline baseline",
            diagnostics,
        );
    }
    for f in &mut b.text_fragments {
        let origin = axes.point(f.x, f.baseline);
        let advance = finite_rect(
            axes.rect(Rect {
                x: f.x,
                y: f.y,
                width: f.width,
                height: f.height,
            }),
            b.node_id,
            diagnostics,
        );
        f.x = finite_used(
            origin.0 as f64,
            b.node_id,
            "vertical glyph origin x",
            diagnostics,
        );
        f.baseline = finite_used(
            origin.1 as f64,
            b.node_id,
            "vertical glyph origin y",
            diagnostics,
        );
        f.y = advance.y;
        f.width = advance.width;
        f.height = advance.height;
        let run = std::sync::Arc::make_mut(&mut f.glyphs);
        for g in &mut run.glyphs {
            let (x, y) = (g.x, g.y);
            if axes.mode == WritingMode::SidewaysLr {
                g.x = -y;
                g.y = x;
                g.rotation = GlyphRotation::CounterClockwise;
            } else {
                g.x = y;
                g.y = -x;
                g.rotation = if g.upright {
                    GlyphRotation::None
                } else {
                    GlyphRotation::Clockwise
                };
            }
        }
        f.ink_rect = finite_rect(
            run.ink_bounds(f.font_size, f.x, f.baseline),
            b.node_id,
            diagnostics,
        );
    }
    // Glyph bounds were virtual during line assembly. Install physical ink.
    if b.kind == BoxKind::Text {
        b.rect = b
            .text_fragments
            .iter()
            .fold(Rect::default(), |r, f| r.union(f.ink_rect));
        b.content_rect = b.rect;
    }
    for child in &mut b.children {
        finalize_inline(child, axes, diagnostics);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn layout(
    b: &mut LayoutBox,
    x: f32,
    available_width: f32,
    containing_height: Option<f32>,
    forced_width: Option<f32>,
    forced_height: Option<f32>,
    percentage_reference: f32,
    font: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut style = b.style.clone();
    if style.text_orientation == crate::style::TextOrientation::Upright {
        style.direction = crate::style::Direction::Ltr;
    }
    let margin = resolve_margins(&style, percentage_reference, b.node_id, diagnostics)
        .map(|v| v.unwrap_or(0.0));
    let padding = resolve_edges(
        &style.padding,
        style.font_size,
        percentage_reference,
        b.node_id,
        "padding",
        diagnostics,
    )
    .map(|v| v.max(0.0));
    let border = b.used_border.unwrap_or(style.border_width);
    let horizontal = finite_used(
        padding.left as f64 + padding.right as f64 + border.left as f64 + border.right as f64,
        b.node_id,
        "vertical horizontal decorations",
        diagnostics,
    );
    let vertical = finite_used(
        padding.top as f64 + padding.bottom as f64 + border.top as f64 + border.bottom as f64,
        b.node_id,
        "vertical vertical decorations",
        diagnostics,
    );
    let height = forced_height
        .or_else(|| {
            style
                .height
                .as_ref()
                .filter(|l| !l.has_percentage() || containing_height.is_some())
                .map(|l| {
                    style.box_sizing.content_size(
                        resolve_length(
                            l,
                            style.font_size,
                            containing_height.unwrap_or(0.0),
                            b.node_id,
                            "height",
                            diagnostics,
                        ),
                        vertical,
                    )
                })
        })
        .unwrap_or(
            containing_height.unwrap_or(crate::css::media::DEFAULT_VIEWPORT_HEIGHT - 48.0)
                - vertical
                - margin.top
                - margin.bottom,
        )
        .max(0.0);
    let height_bounds =
        super::sizing::bounds(b, false, containing_height, vertical, 0.0, diagnostics);
    let height = height.clamp(height_bounds.0, height_bounds.1);
    let explicit_width = forced_width.or_else(|| {
        style.width.as_ref().map(|l| {
            style.box_sizing.content_size(
                resolve_length(
                    l,
                    style.font_size,
                    available_width,
                    b.node_id,
                    "width",
                    diagnostics,
                ),
                horizontal,
            )
        })
    });
    b.margin = margin;
    b.padding = padding;
    b.flow_width = available_width;
    b.definite_height = Some(height);
    b.definite_width = explicit_width.is_some();
    let axes = FlowAxes {
        mode: style.writing_mode,
        origin: (0.0, 0.0),
        block: 0.0,
        inline: height,
    };
    let mut cursor = 0.0f32;
    let mut floats = super::floats::FloatContext::default();
    let mut ranges = Vec::new();
    let mut placed = Vec::new();
    let mut adjoining: Option<f32> = None;
    let table = if style.display.is_table() {
        let mut proxy = b.clone();
        table_oracle(
            b,
            &mut proxy,
            axes,
            height,
            explicit_width.unwrap_or(available_width),
            font,
            diagnostics,
        );
        cursor = super::table::layout_table(
            &mut proxy,
            (0.0, 0.0, height),
            explicit_width,
            font,
            diagnostics,
        );
        super::tree::absolutize_y(&mut proxy, 0.0, diagnostics);
        Some(proxy)
    } else {
        None
    };
    let allocation = if style.display.is_item_container() {
        let (natural, positions) = allocate(
            b,
            height,
            explicit_width,
            available_width,
            font,
            diagnostics,
        );
        cursor = natural;
        Some(positions)
    } else {
        None
    };
    let mut index = 0;
    while table.is_none() && allocation.is_none() && index < b.children.len() {
        if !b.children[index].normal_block() {
            adjoining = None;
            let end = b.children[index..]
                .iter()
                .position(|c| c.normal_block())
                .map_or(b.children.len(), |i| index + i);
            let (lines, extent) = super::inline::layout_inline_run(
                &mut b.children[index..end],
                &style,
                b.node_id,
                (0.0, cursor, height, explicit_width),
                font,
                diagnostics,
                Some(&mut floats),
            );
            cursor += extent as f32;
            b.line_boxes.extend(lines);
            ranges.push(index..end);
            index = end;
            continue;
        }
        let child = &mut b.children[index];
        let cleared = floats.clear_y(child.style.clear, cursor);
        if cleared > cursor {
            adjoining = None;
        }
        cursor = cleared;
        if child.style.writing_mode.vertical() {
            layout(
                child,
                0.0,
                explicit_width.unwrap_or(available_width),
                Some(height),
                None,
                None,
                height,
                font,
                diagnostics,
            );
        } else {
            // An orthogonal context computes its own inline intrinsic width,
            // not the outer context's vertical line length.
            let available = explicit_width.unwrap_or(available_width);
            let intrinsic =
                super::positioning::intrinsic_content(child, available, font, diagnostics);
            let decor = child.style.border_width.left
                + child.style.border_width.right
                + child
                    .style
                    .padding
                    .left
                    .resolve(child.style.font_size, height)
                + child
                    .style
                    .padding
                    .right
                    .resolve(child.style.font_size, height);
            let width = child
                .style
                .width
                .as_ref()
                .map(|l| l.resolve(child.style.font_size, available))
                .unwrap_or(available.max(intrinsic.0).min(intrinsic.1));
            super::block::layout_sized_in(
                child,
                available,
                (width - decor).max(0.0),
                None,
                (true, false),
                height,
                font,
                diagnostics,
            );
        }
        let edges = axes.logical_edges(&child.margin);
        let parallel = child.style.writing_mode == style.writing_mode;
        if let Some(previous) = adjoining.filter(|_| parallel) {
            let collapsed = previous.max(0.0).max(edges.top.max(0.0))
                + previous.min(0.0).min(edges.top.min(0.0));
            cursor += collapsed - previous;
        } else {
            cursor += edges.top;
        }
        placed.push((index, cursor));
        cursor += child.rect.width + edges.bottom;
        adjoining = parallel.then_some(edges.bottom);
        index += 1;
    }
    let width_bounds =
        super::sizing::bounds(b, true, Some(available_width), horizontal, 0.0, diagnostics);
    let width = finite_used(
        explicit_width.unwrap_or(cursor.max(floats.bottom())) as f64,
        b.node_id,
        "vertical block extent",
        diagnostics,
    )
    .clamp(width_bounds.0, width_bounds.1);
    let border_x = finite_used(
        x as f64 + margin.left as f64,
        b.node_id,
        "vertical border x",
        diagnostics,
    );
    let content_x = finite_used(
        border_x as f64 + padding.left as f64 + border.left as f64,
        b.node_id,
        "vertical content x",
        diagnostics,
    );
    let content_y = finite_used(
        padding.top as f64 + border.top as f64,
        b.node_id,
        "vertical content y",
        diagnostics,
    );
    let axes = FlowAxes {
        origin: (content_x, content_y),
        block: width,
        ..axes
    };
    if let Some(proxy) = table {
        for (child, p) in b.children.iter_mut().zip(&proxy.children) {
            install_table(child, p, axes, 0.0, font, diagnostics);
        }
        b.table_rect = proxy.table_rect.map(|r| axes.rect(r));
        b.table_borders = proxy
            .table_borders
            .into_iter()
            .map(|mut s| {
                s.rect = axes.rect(s.rect);
                s
            })
            .collect();
    }
    if let Some(positions) = allocation {
        for (index, rect) in positions {
            let child = &mut b.children[index];
            if let Some(anchor) = &mut child.flex_static {
                anchor.area = axes.rect(anchor.area);
                let old_row = anchor.row;
                anchor.row = !old_row;
                anchor.main_reverse ^= if old_row {
                    style.writing_mode == WritingMode::SidewaysLr
                } else {
                    style.writing_mode.block_reverse()
                };
                anchor.cross_reverse ^= if old_row {
                    style.writing_mode.block_reverse()
                } else {
                    style.writing_mode == WritingMode::SidewaysLr
                };
            }
            if let Some((_, area)) = &mut child.grid_area {
                *area = axes.rect(*area);
            }
            let target = axes.rect(rect);
            if child.style.position.out_of_flow() {
                child.rect = target;
                child.content_rect = target;
                continue;
            }
            super::positioning::translate(child, target.x - child.rect.x, 0.0, diagnostics);
            let local_y = child.content_rect.y - child.rect.y;
            child.content_rect.y = local_y;
            super::block::place_block(child, target.y as f64, diagnostics);
        }
    }
    for range in ranges {
        for child in &mut b.children[range] {
            finalize_inline(child, axes, diagnostics);
        }
    }
    for (index, v) in placed {
        let child = &mut b.children[index];
        let logical = Rect {
            x: child.margin.top,
            y: v,
            width: child.rect.height,
            height: child.rect.width,
        };
        let target = finite_rect(axes.rect(logical), child.node_id, diagnostics);
        super::positioning::translate(child, target.x - child.rect.x, 0.0, diagnostics);
        let local_y = child.content_rect.y - child.rect.y;
        child.content_rect.y = local_y;
        super::block::place_block(child, target.y as f64, diagnostics);
    }
    for line in &mut b.line_boxes {
        axes.line(line, b.node_id, diagnostics);
    }
    b.content_rect = Rect {
        x: content_x,
        y: content_y,
        width,
        height,
    };
    b.rect = super::tree::finite_rect(
        Rect {
            x: border_x,
            y: 0.0,
            width: width + horizontal,
            height: height + vertical,
        },
        b.node_id,
        diagnostics,
    );
    b.content_rect = super::tree::finite_rect(b.content_rect, b.node_id, diagnostics);
}
