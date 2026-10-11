//! Bounded horizontal CSS2 table construction, intrinsic columns and cell grid.
use super::{
    BoxKind, LayoutBox, Rect,
    tree::{finite_used, resolve_edges, resolve_length},
};
use crate::{
    css::values::{BorderStyle, Edges, ResolvedColor, Side},
    dom::NodeId,
    style::{
        BorderCollapse, CaptionSide, ComputedStyle, Diagnostic, Display, EmptyCells, TableLayout,
        VerticalAlign,
    },
};

const MAX_COLUMNS: usize = 128;
const MAX_ROWS: usize = 512;
const MAX_SLOTS: usize = 65_536;
const MAX_EXTENT: f32 = 1_000_000.0;

pub(super) fn bounded_extent(
    value: f32,
    node_id: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    if !value.is_finite() || value > MAX_EXTENT {
        if !diagnostics
            .iter()
            .any(|d| d.node_id == node_id && d.message.starts_with("table extent limit"))
        {
            diagnostics.push(Diagnostic {
                node_id,
                message: format!(
                    "table extent limit {MAX_EXTENT}px reached; used sizes/contributions clamped"
                ),
            });
        }
        MAX_EXTENT
    } else {
        value.max(0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BorderSegment {
    pub rect: Rect,
    pub color: ResolvedColor,
    pub style: BorderStyle,
}

pub(super) fn html_span(
    value: Option<&str>,
    row: bool,
    diagnostics: &mut Vec<Diagnostic>,
    node_id: Option<NodeId>,
) -> usize {
    let Some(value) = value else { return 1 };
    let Some(value) = crate::html::attributes::nonnegative_integer_digits(value) else {
        return 1;
    };
    let mut result = 0usize;
    for c in value.bytes() {
        result = result
            .saturating_mul(10)
            .saturating_add((c - b'0') as usize);
    }
    let limit = if row { MAX_ROWS } else { MAX_COLUMNS };
    if result > limit {
        diagnostics.push(Diagnostic {
            node_id,
            message: format!("table span {result} exceeds layout limit {limit}; clamped"),
        });
    }
    if row && result == 0 {
        0
    } else {
        result.clamp(1, limit)
    }
}

fn whitespace(b: &LayoutBox) -> bool {
    b.kind == BoxKind::Text
        && b.text.as_ref().is_none_or(|s| {
            s.chars()
                .all(|c| matches!(c, ' ' | '\n' | '\r' | '\t' | '\u{c}'))
        })
}

pub(super) fn normalize(
    b: &mut LayoutBox,
    create: &mut dyn FnMut(ComputedStyle, Vec<LayoutBox>) -> LayoutBox,
) {
    let display = b.style.display;
    let expected = if display.is_table() {
        Some(Display::TableRowGroup)
    } else if display.row_group() {
        Some(Display::TableRow)
    } else if display == Display::TableRow {
        Some(Display::TableCell)
    } else {
        None
    };
    if display == Display::TableColumn {
        b.children.clear();
        return;
    }
    let children = std::mem::take(&mut b.children);
    let mut run = Vec::new();
    let flush =
        |b: &mut LayoutBox,
         run: &mut Vec<LayoutBox>,
         create: &mut dyn FnMut(ComputedStyle, Vec<LayoutBox>) -> LayoutBox| {
            if run.is_empty() {
                return;
            }
            let mut style = ComputedStyle::inherit(&b.style);
            style.display = expected.unwrap_or(if b.kind == BoxKind::Inline {
                Display::InlineTable
            } else {
                Display::Table
            });
            let mut anonymous = create(style, std::mem::take(run));
            // Repair inner levels using the same identity allocator.
            normalize(&mut anonymous, create);
            anonymous.parent_box = Some(b.box_id);
            b.children.push(anonymous);
        };
    for child in children {
        let d = child.style.display;
        let valid = if display.is_table() {
            d.row_group()
                || matches!(
                    d,
                    Display::TableColumn | Display::TableColumnGroup | Display::TableCaption
                )
        } else if display.row_group() {
            d == Display::TableRow
        } else if display == Display::TableRow {
            d == Display::TableCell
        } else if display == Display::TableColumnGroup {
            d == Display::TableColumn
        } else {
            !d.table_internal() && d != Display::TableCaption
        };
        if whitespace(&child) && (expected.is_some() || display == Display::TableColumnGroup) {
            continue;
        }
        if valid || child.style.position.out_of_flow() {
            flush(b, &mut run, create);
            b.children.push(child);
        } else if display == Display::TableColumnGroup {
            continue;
        } else {
            run.push(child);
        }
    }
    flush(b, &mut run, create);
}

#[derive(Clone)]
struct Cell {
    group: usize,
    row_child: usize,
    child: usize,
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
}
struct Model {
    rows: Vec<(usize, usize)>,
    cells: Vec<Cell>,
    columns: Vec<(usize, Option<usize>)>,
    count: usize,
}

fn cell<'a>(b: &'a LayoutBox, c: &Cell) -> &'a LayoutBox {
    &b.children[c.group].children[c.row_child].children[c.child]
}
fn cell_mut<'a>(b: &'a mut LayoutBox, c: &Cell) -> &'a mut LayoutBox {
    &mut b.children[c.group].children[c.row_child].children[c.child]
}
fn model(b: &LayoutBox, diagnostics: &mut Vec<Diagnostic>) -> Model {
    let mut groups: Vec<_> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.style.display.row_group())
        .collect();
    groups.sort_by_key(|(_, c)| match c.style.display {
        Display::TableHeaderGroup => 0,
        Display::TableFooterGroup => 2,
        _ => 1,
    });
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut count = 0;
    let mut limited = false;
    for (g, group) in groups {
        let available = group
            .children
            .iter()
            .filter(|r| r.style.display == Display::TableRow)
            .count()
            .min(MAX_ROWS - rows.len());
        let end = rows.len() + available;
        let mut occupied = vec![0usize; MAX_COLUMNS];
        for (r, row) in group
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.style.display == Display::TableRow)
        {
            if rows.len() == MAX_ROWS {
                limited = true;
                break;
            }
            let row_index = rows.len();
            rows.push((g, r));
            let mut col = 0;
            for (i, c) in row
                .children
                .iter()
                .enumerate()
                .filter(|(_, c)| c.style.display == Display::TableCell)
            {
                while col < MAX_COLUMNS && occupied[col] > row_index {
                    col += 1;
                }
                let cols = c.table_span.0.min(MAX_COLUMNS - col);
                if cols == 0 {
                    limited = true;
                    break;
                }
                let rowspan = if c.table_span.1 == 0 {
                    end - row_index
                } else {
                    c.table_span.1.min(end - row_index)
                }
                .max(1);
                // Skip occupied rowspan slots rather than overlap cells.
                while col + cols <= MAX_COLUMNS
                    && occupied[col..col + cols]
                        .iter()
                        .any(|&until| until > row_index)
                {
                    col += 1;
                }
                if col + cols > MAX_COLUMNS {
                    limited = true;
                    break;
                }
                occupied[col..col + cols].fill(row_index + rowspan);
                cells.push(Cell {
                    group: g,
                    row_child: r,
                    child: i,
                    row: row_index,
                    col,
                    rows: rowspan,
                    cols,
                });
                col += cols;
                count = count.max(col);
            }
        }
    }
    let mut columns = Vec::new();
    for (g, c) in b.children.iter().enumerate() {
        if c.style.display == Display::TableColumn {
            limited |= c.table_span.0 > MAX_COLUMNS - columns.len();
            columns.extend(std::iter::repeat_n(
                (g, None),
                c.table_span.0.min(MAX_COLUMNS - columns.len()),
            ));
        } else if c.style.display == Display::TableColumnGroup {
            if c.children.is_empty() {
                limited |= c.table_span.0 > MAX_COLUMNS - columns.len();
                columns.extend(std::iter::repeat_n(
                    (g, None),
                    c.table_span.0.min(MAX_COLUMNS - columns.len()),
                ));
            } else {
                for (i, col) in c.children.iter().enumerate() {
                    limited |= col.table_span.0 > MAX_COLUMNS - columns.len();
                    columns.extend(std::iter::repeat_n(
                        (g, Some(i)),
                        col.table_span.0.min(MAX_COLUMNS - columns.len()),
                    ));
                }
            }
        }
    }
    count = count.max(columns.len()).min(MAX_COLUMNS);
    if limited || rows.len().saturating_mul(count) > MAX_SLOTS {
        diagnostics.push(Diagnostic { node_id: b.node_id, message: format!("table grid limit ({MAX_ROWS} rows, {MAX_COLUMNS} columns, {MAX_SLOTS} slots) reached; excess grid content omitted") });
    }
    Model {
        rows,
        cells,
        columns,
        count,
    }
}

fn spacing(b: &LayoutBox, diagnostics: &mut Vec<Diagnostic>) -> (f32, f32) {
    if b.style.border_collapse == BorderCollapse::Collapse {
        return (0.0, 0.0);
    }
    let (x, y) = &b.style.border_spacing;
    (
        resolve_length(
            x,
            b.style.font_size,
            0.0,
            b.node_id,
            "border-spacing",
            diagnostics,
        ),
        resolve_length(
            y,
            b.style.font_size,
            0.0,
            b.node_id,
            "border-spacing",
            diagnostics,
        ),
    )
}
fn decorations(b: &LayoutBox, width: f32, diagnostics: &mut Vec<Diagnostic>) -> f32 {
    let p = resolve_edges(
        &b.style.padding,
        b.style.font_size,
        width,
        b.node_id,
        "table cell padding",
        diagnostics,
    );
    let border = b.used_border.unwrap_or(b.style.border_width);
    bounded_extent(
        p.left + p.right + border.left + border.right,
        b.node_id,
        diagnostics,
    )
}
fn grow(widths: &mut [f32], desired: f32) {
    let extra = (desired - widths.iter().sum::<f32>()).max(0.0) / widths.len().max(1) as f32;
    for width in widths {
        *width += extra;
    }
}
fn column<'a>(b: &'a LayoutBox, &(g, i): &(usize, Option<usize>)) -> &'a LayoutBox {
    i.map_or(&b.children[g], |i| &b.children[g].children[i])
}
fn column_width(
    b: &LayoutBox,
    address: &(usize, Option<usize>),
    reference: f32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f32> {
    let c = column(b, address);
    let owner = if c.style.width.is_some() {
        c
    } else {
        &b.children[address.0]
    };
    super::sizing::definite_size(
        owner,
        owner.style.width.as_ref(),
        Some(reference),
        0.0,
        diagnostics,
    )
}
fn columns(
    b: &LayoutBox,
    m: &Model,
    reference: f32,
    gap: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<f32>, Vec<f32>, Vec<Option<f32>>) {
    let mut min = vec![0.0; m.count];
    let mut max = min.clone();
    let mut preferred = vec![None; m.count];
    for (i, address) in m.columns.iter().enumerate() {
        let col = column(b, address);
        preferred[i] = column_width(b, address, reference, diagnostics);
        let bounds = super::sizing::bounds(col, true, Some(reference), 0.0, 0.0, diagnostics);
        preferred[i] = preferred[i].map(|w| bounded_extent(w, col.node_id, diagnostics));
        min[i] = bounded_extent(
            preferred[i].unwrap_or(0.0).clamp(bounds.0, bounds.1),
            col.node_id,
            diagnostics,
        );
        max[i] = min[i];
    }
    let mut cells: Vec<_> = m.cells.iter().collect();
    cells.sort_by_key(|c| c.cols);
    for c in cells {
        let box_ = cell(b, c);
        let decor = decorations(box_, reference, diagnostics);
        let (lo, hi) = super::positioning::intrinsic_content(box_, reference, font, diagnostics);
        let limits = super::sizing::bounds(box_, true, Some(reference), decor, 0.0, diagnostics);
        let specified = super::sizing::definite_size(
            box_,
            box_.style.width.as_ref(),
            Some(reference),
            decor,
            diagnostics,
        );
        let lo = bounded_extent(
            lo.max(specified.unwrap_or(0.0)).clamp(limits.0, limits.1) + decor,
            box_.node_id,
            diagnostics,
        );
        let hi = bounded_extent(
            hi.max(specified.unwrap_or(0.0))
                .clamp(limits.0, limits.1)
                .max(lo - decor)
                + decor,
            box_.node_id,
            diagnostics,
        );
        let gaps = gap * c.cols.saturating_sub(1) as f32;
        grow(&mut min[c.col..c.col + c.cols], (lo - gaps).max(0.0));
        grow(&mut max[c.col..c.col + c.cols], (hi - gaps).max(0.0));
        if c.cols == 1
            && let Some(w) = specified
        {
            preferred[c.col] = Some(bounded_extent(
                preferred[c.col].unwrap_or(0.0).max(w + decor),
                box_.node_id,
                diagnostics,
            ));
        }
    }
    for i in 0..m.count {
        max[i] = max[i].max(min[i]);
    }
    (min, max, preferred)
}

pub(super) fn intrinsic_widths(
    b: &LayoutBox,
    reference: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    let m = model(b, diagnostics);
    let (gap, _) = spacing(b, diagnostics);
    let (min, max, _) = columns(b, &m, reference, gap, font, diagnostics);
    let gaps = if m.count > 0 {
        (m.count + 1) as f32 * gap
    } else {
        0.0
    };
    let mut result = (
        min.iter().sum::<f32>() + gaps,
        max.iter().sum::<f32>() + gaps,
    );
    for caption in b
        .children
        .iter()
        .filter(|c| c.style.display == Display::TableCaption)
    {
        let (min, max) =
            super::positioning::intrinsic_content(caption, reference, font, diagnostics);
        let decor = decorations(caption, reference, diagnostics);
        result.0 = result.0.max(min + decor);
        result.1 = result.1.max(max + decor);
    }
    result
}

#[derive(Clone, Copy)]
struct Edge {
    width: f32,
    style: BorderStyle,
    color: ResolvedColor,
    origin: u8,
    source: usize,
}
impl Default for Edge {
    fn default() -> Self {
        Self {
            width: 0.0,
            style: BorderStyle::None,
            color: ResolvedColor::TRANSPARENT,
            origin: 0,
            source: usize::MAX,
        }
    }
}
fn rank(style: BorderStyle) -> u8 {
    match style {
        BorderStyle::Hidden => 6,
        BorderStyle::Double => 5,
        BorderStyle::Solid => 4,
        BorderStyle::Dashed => 3,
        BorderStyle::Dotted => 2,
        BorderStyle::None => 0,
    }
}
fn offer(target: &mut Edge, b: &LayoutBox, side: Side, origin: u8, source: usize) {
    let candidate = Edge {
        width: *b.style.border_width.get(side),
        style: *b.style.border_style.get(side),
        color: *b.style.borders.get(side),
        origin,
        source,
    };
    let stronger = candidate.style == BorderStyle::Hidden && target.style != BorderStyle::Hidden
        || target.style != BorderStyle::Hidden
            && (candidate.width > target.width
                || candidate.width == target.width
                    && (rank(candidate.style), origin, std::cmp::Reverse(source))
                        > (
                            rank(target.style),
                            target.origin,
                            std::cmp::Reverse(target.source),
                        ));
    if stronger {
        *target = candidate;
    }
}
struct Borders {
    horizontal: Vec<Edge>,
    vertical: Vec<Edge>,
    rows: usize,
    cols: usize,
}
fn collapsed(b: &LayoutBox, m: &Model) -> Borders {
    let (rows, cols) = (m.rows.len(), m.count);
    let mut result = Borders {
        horizontal: vec![Edge::default(); (rows + 1) * cols],
        vertical: vec![Edge::default(); rows * (cols + 1)],
        rows,
        cols,
    };
    let mut rectangle =
        |b: &LayoutBox, r: usize, c: usize, rs: usize, cs: usize, origin: u8, source: usize| {
            for x in c..c + cs {
                offer(
                    &mut result.horizontal[r * cols + x],
                    b,
                    Side::Top,
                    origin,
                    source,
                );
                offer(
                    &mut result.horizontal[(r + rs) * cols + x],
                    b,
                    Side::Bottom,
                    origin,
                    source,
                );
            }
            for y in r..r + rs {
                offer(
                    &mut result.vertical[y * (cols + 1) + c],
                    b,
                    Side::Left,
                    origin,
                    source,
                );
                offer(
                    &mut result.vertical[y * (cols + 1) + c + cs],
                    b,
                    Side::Right,
                    origin,
                    source,
                );
            }
        };
    rectangle(b, 0, 0, rows, cols, 1, 0);
    for (i, &(g, child)) in m.columns.iter().enumerate() {
        let col = column(b, &(g, child));
        if col.style.display == Display::TableColumn {
            rectangle(col, 0, i, rows, 1, 3, i);
        }
    }
    for (g, group) in b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.style.display == Display::TableColumnGroup)
    {
        if let Some(start) = m.columns.iter().position(|&(owner, _)| owner == g) {
            let count = m.columns.iter().filter(|&&(owner, _)| owner == g).count();
            rectangle(group, 0, start, rows, count, 2, g);
        }
    }
    for (r, &(g, i)) in m.rows.iter().enumerate() {
        rectangle(&b.children[g].children[i], r, 0, 1, cols, 5, r);
    }
    for (g, group) in b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.style.display.row_group())
    {
        if let Some(start) = m.rows.iter().position(|&(group, _)| group == g) {
            let count = m.rows.iter().filter(|&&(group, _)| group == g).count();
            rectangle(group, start, 0, count, cols, 4, g);
        }
    }
    for (source, c) in m.cells.iter().enumerate() {
        rectangle(cell(b, c), c.row, c.col, c.rows, c.cols, 6, source);
    }
    for c in &m.cells {
        for row in c.row + 1..c.row + c.rows {
            result.horizontal[row * cols + c.col..row * cols + c.col + c.cols]
                .fill(Edge::default());
        }
        for row in c.row..c.row + c.rows {
            for col in c.col + 1..c.col + c.cols {
                result.vertical[row * (cols + 1) + col] = Edge::default();
            }
        }
    }
    result
}

pub(super) fn layout_table(
    b: &mut LayoutBox,
    (x, y, width): (f32, f32, f32),
    height: Option<f32>,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let m = model(b, diagnostics);
    let (gap_x, gap_y) = spacing(b, diagnostics);
    let (gap_x, gap_y) = (
        bounded_extent(gap_x, b.node_id, diagnostics),
        bounded_extent(gap_y, b.node_id, diagnostics),
    );
    let collapse = b.style.border_collapse == BorderCollapse::Collapse;
    let borders = collapse.then(|| collapsed(b, &m));
    if let Some(edges) = &borders {
        for c in &m.cells {
            let edge_width = |edge: &Edge| {
                if edge.style == BorderStyle::Hidden {
                    0.0
                } else {
                    edge.width / 2.0
                }
            };
            let used = Edges {
                top: (c.col..c.col + c.cols)
                    .map(|col| edge_width(&edges.horizontal[c.row * m.count + col]))
                    .fold(0.0, f32::max),
                bottom: (c.col..c.col + c.cols)
                    .map(|col| edge_width(&edges.horizontal[(c.row + c.rows) * m.count + col]))
                    .fold(0.0, f32::max),
                left: (c.row..c.row + c.rows)
                    .map(|row| edge_width(&edges.vertical[row * (m.count + 1) + c.col]))
                    .fold(0.0, f32::max),
                right: (c.row..c.row + c.rows)
                    .map(|row| edge_width(&edges.vertical[row * (m.count + 1) + c.col + c.cols]))
                    .fold(0.0, f32::max),
            };
            cell_mut(b, c).used_border = Some(used);
        }
    }
    let (min, max, preferred) = columns(b, &m, width, gap_x, font, diagnostics);
    let gaps = if m.count > 0 {
        (m.count + 1) as f32 * gap_x
    } else {
        0.0
    };
    let budget = (width - gaps).max(0.0);
    let fixed = b.style.table_layout == TableLayout::Fixed && b.style.width.is_some();
    let mut widths = if fixed {
        let mut widths = preferred
            .iter()
            .enumerate()
            .map(|(i, _)| {
                m.columns
                    .get(i)
                    .and_then(|address| column_width(b, address, width, diagnostics))
            })
            .collect::<Vec<_>>();
        for c in m.cells.iter().filter(|c| c.row == 0) {
            let box_ = cell(b, c);
            let decor = decorations(box_, width, diagnostics);
            if let Some(w) = super::sizing::definite_size(
                box_,
                box_.style.width.as_ref(),
                Some(width),
                decor,
                diagnostics,
            ) {
                let assigned: f32 = widths[c.col..c.col + c.cols].iter().flatten().sum();
                let missing = widths[c.col..c.col + c.cols]
                    .iter()
                    .filter(|w| w.is_none())
                    .count();
                let share = ((w + decor - gap_x * c.cols.saturating_sub(1) as f32 - assigned)
                    .max(0.0))
                    / missing.max(1) as f32;
                for w in &mut widths[c.col..c.col + c.cols] {
                    if w.is_none() {
                        *w = Some(share);
                    }
                }
            }
        }
        let assigned: f32 = widths.iter().flatten().sum();
        let missing = widths.iter().filter(|w| w.is_none()).count();
        let share = (budget - assigned).max(0.0) / missing.max(1) as f32;
        let mut widths = widths
            .into_iter()
            .map(|w| w.unwrap_or(share))
            .collect::<Vec<_>>();
        grow(&mut widths, budget);
        widths
    } else {
        let total_min: f32 = min.iter().sum();
        let total_max: f32 = max.iter().sum();
        let mut widths = min.clone();
        if budget > total_min {
            let fraction = ((budget - total_min) / (total_max - total_min).max(0.001)).min(1.0);
            for i in 0..m.count {
                widths[i] += (max[i] - min[i]) * fraction;
            }
            // Widths above max-content go to auto columns before specified ones.
            let free = (budget - widths.iter().sum::<f32>()).max(0.0);
            let auto = preferred.iter().filter(|w| w.is_none()).count();
            let total_weight: f32 = widths
                .iter()
                .enumerate()
                .filter(|(i, _)| auto == 0 || preferred[*i].is_none())
                .map(|(_, w)| w.max(1.0))
                .sum();
            for i in 0..m.count {
                if auto == 0 || preferred[i].is_none() {
                    widths[i] += free * widths[i].max(1.0) / total_weight.max(1.0);
                }
            }
        }
        widths
    };
    for w in &mut widths {
        *w = bounded_extent(
            finite_used(*w as f64, b.node_id, "table column width", diagnostics),
            b.node_id,
            diagnostics,
        );
    }
    let grid_width = widths.iter().sum::<f32>() + gaps;
    // Table minima may overflow the available width; preserve real scroll extents.
    let used_width = width.max(grid_width);
    let mut xs = vec![x + gap_x];
    for w in &widths {
        xs.push(xs.last().unwrap() + w + gap_x);
    }
    let mut top = 0.0;
    let caption_x = x - b.padding.left - b.style.border_width.left;
    let caption_width = used_width
        + b.padding.left
        + b.padding.right
        + b.style.border_width.left
        + b.style.border_width.right;
    for caption in b.children.iter_mut().filter(|c| {
        c.style.display == Display::TableCaption
            && c.style.caption_side == CaptionSide::Top
            && !c.style.position.out_of_flow()
    }) {
        super::block::layout_caption(caption, caption_x, caption_width, font, diagnostics);
        top += caption.margin.top;
        super::block::place_block(caption, (y + top) as f64, diagnostics);
        top += caption.rect.height + caption.margin.bottom;
    }
    let mut row_heights = vec![0.0f32; m.rows.len()];
    let mut baselines = row_heights.clone();
    for (r, &(g, i)) in m.rows.iter().enumerate() {
        let row = &b.children[g].children[i];
        let bounds = super::sizing::bounds(row, false, height, 0.0, 0.0, diagnostics);
        row_heights[r] = bounded_extent(
            super::sizing::definite_size(row, row.style.height.as_ref(), height, 0.0, diagnostics)
                .unwrap_or(0.0)
                .clamp(bounds.0, bounds.1),
            row.node_id,
            diagnostics,
        );
    }
    for c in &m.cells {
        let w = widths[c.col..c.col + c.cols].iter().sum::<f32>()
            + gap_x * c.cols.saturating_sub(1) as f32;
        let cell = cell_mut(b, c);
        let decor = decorations(cell, used_width, diagnostics);
        let padding = resolve_edges(
            &cell.style.padding,
            cell.style.font_size,
            used_width,
            cell.node_id,
            "table cell padding",
            diagnostics,
        );
        let border = cell.used_border.unwrap_or(cell.style.border_width);
        let vertical = padding.top + padding.bottom + border.top + border.bottom;
        let preferred_height = super::sizing::definite_size(
            cell,
            cell.style.height.as_ref(),
            height,
            vertical,
            diagnostics,
        )
        .or_else(|| {
            height.map(|h| (h * c.rows as f32 / m.rows.len().max(1) as f32 - vertical).max(0.0))
        });
        let preferred_height =
            preferred_height.map(|h| bounded_extent(h, cell.node_id, diagnostics));
        super::block::layout_sized(
            cell,
            used_width,
            (w - decor).max(0.0),
            preferred_height,
            (true, true),
            font,
            diagnostics,
        );
        cell.margin = Edges::default();
        cell.rect.width = w;
        let cell_height = super::sizing::definite_size(
            cell,
            cell.style.height.as_ref(),
            height,
            0.0,
            diagnostics,
        )
        .unwrap_or(0.0);
        cell.rect.height =
            bounded_extent(cell.rect.height.max(cell_height), cell.node_id, diagnostics);
        let baseline = cell.first_local_baseline().unwrap_or(
            cell.rect.height
                - cell.padding.bottom
                - cell.used_border.unwrap_or(cell.style.border_width).bottom,
        );
        if c.rows == 1 {
            row_heights[c.row] = row_heights[c.row].max(cell.rect.height);
            if cell.style.vertical_align == VerticalAlign::Baseline {
                baselines[c.row] = baselines[c.row].max(baseline);
            }
        }
    }
    for c in m.cells.iter().filter(|c| c.rows == 1) {
        let cell = cell(b, c);
        if cell.style.vertical_align == VerticalAlign::Baseline {
            let baseline = cell.first_local_baseline().unwrap_or(cell.rect.height);
            row_heights[c.row] =
                row_heights[c.row].max(baselines[c.row] + cell.rect.height - baseline);
        }
    }
    let mut spanning: Vec<_> = m.cells.iter().filter(|c| c.rows > 1).collect();
    spanning.sort_by_key(|c| c.rows);
    for c in spanning {
        let needed = cell(b, c).rect.height - gap_y * c.rows.saturating_sub(1) as f32;
        grow(&mut row_heights[c.row..c.row + c.rows], needed);
    }
    if let Some(height) = height {
        let gaps = gap_y * (row_heights.len() + 1) as f32;
        grow(&mut row_heights, (height - gaps).max(0.0));
    }
    for h in &mut row_heights {
        *h = bounded_extent(*h, b.node_id, diagnostics);
    }
    for c in &m.cells {
        let allocated = row_heights[c.row..c.row + c.rows].iter().sum::<f32>()
            + gap_y * c.rows.saturating_sub(1) as f32;
        let cell = cell_mut(b, c);
        if height.is_some()
            || cell
                .style
                .height
                .as_ref()
                .is_some_and(|h| !h.has_percentage())
        {
            let used_border = cell.used_border;
            let border = used_border.unwrap_or(cell.style.border_width);
            let vertical = cell.padding.top + cell.padding.bottom + border.top + border.bottom;
            let w = widths[c.col..c.col + c.cols].iter().sum::<f32>()
                + gap_x * c.cols.saturating_sub(1) as f32;
            let decor = decorations(cell, used_width, diagnostics);
            cell.reset();
            cell.used_border = used_border;
            super::block::layout_sized(
                cell,
                used_width,
                (w - decor).max(0.0),
                Some((allocated - vertical).max(0.0)),
                (true, true),
                font,
                diagnostics,
            );
            cell.margin = Edges::default();
            cell.rect.width = w;
            if cell.style.vertical_align == VerticalAlign::Baseline {
                baselines[c.row] =
                    baselines[c.row].max(cell.first_local_baseline().unwrap_or(cell.rect.height));
            }
        }
    }
    let mut ys = vec![y + top + gap_y];
    for h in &row_heights {
        ys.push(ys.last().unwrap() + h + gap_y);
    }
    let grid_height = row_heights.iter().sum::<f32>()
        + if row_heights.is_empty() {
            0.0
        } else {
            (row_heights.len() + 1) as f32 * gap_y
        };
    for (g, group) in b
        .children
        .iter_mut()
        .enumerate()
        .filter(|(_, c)| c.style.display.row_group())
    {
        if let Some(start) = m.rows.iter().position(|&(group, _)| group == g) {
            let end = start + m.rows.iter().filter(|&&(group, _)| group == g).count();
            group.rect = Rect {
                x,
                y: ys[start],
                width: used_width,
                height: ys[end] - ys[start] - gap_y,
            };
            group.content_rect = group.rect;
            group.collapsed_table_part = collapse;
            for r in start..end {
                let row = &mut group.children[m.rows[r].1];
                row.rect = Rect {
                    x,
                    y: ys[r] - ys[start],
                    width: used_width,
                    height: row_heights[r],
                };
                row.content_rect = row.rect;
                row.collapsed_table_part = collapse;
            }
        }
    }
    for c in &m.cells {
        let h = row_heights[c.row..c.row + c.rows].iter().sum::<f32>()
            + gap_y * c.rows.saturating_sub(1) as f32;
        let cell = cell_mut(b, c);
        let extra = (h - cell.rect.height).max(0.0);
        let shift = match cell.style.vertical_align {
            VerticalAlign::Top => 0.0,
            VerticalAlign::Middle => extra / 2.0,
            VerticalAlign::Bottom => extra,
            VerticalAlign::Baseline => (baselines[c.row]
                - cell.first_local_baseline().unwrap_or(cell.rect.height))
            .max(0.0),
        };
        // Shift content, not the background/border, inside the row's allocated height.
        for child in &mut cell.children {
            super::positioning::translate_local_y(child, shift, diagnostics);
        }
        for line in &mut cell.line_boxes {
            line.rect.y += shift;
            line.content_rect.y += shift;
            line.baseline += shift;
        }
        cell.content_rect.y += shift;
        super::positioning::translate(cell, xs[c.col] - cell.rect.x, 0.0, diagnostics);
        cell.rect.y = 0.0;
        cell.rect.height = h;
        cell.collapsed_table_part = collapse;
        cell.hidden_empty_cell =
            !collapse && cell.style.empty_cells == EmptyCells::Hide && !has_content(cell);
    }
    for (i, &(g, child)) in m.columns.iter().enumerate() {
        let col = if let Some(i) = child {
            &mut b.children[g].children[i]
        } else {
            &mut b.children[g]
        };
        let rect = Rect {
            x: xs[i],
            y: y + top,
            width: widths[i],
            height: grid_height,
        };
        col.rect = if col.rect.width == 0.0 {
            rect
        } else {
            col.rect.union(rect)
        };
        col.content_rect = col.rect;
        col.collapsed_table_part = collapse;
    }
    for group in b
        .children
        .iter_mut()
        .filter(|c| c.style.display == Display::TableColumnGroup && !c.children.is_empty())
    {
        group.rect = group
            .children
            .iter()
            .map(|c| c.rect)
            .reduce(Rect::union)
            .unwrap_or_default();
        group.content_rect = group.rect;
        group.collapsed_table_part = collapse;
        for child in &mut group.children {
            child.rect.y -= group.rect.y;
            child.content_rect.y -= group.rect.y;
        }
    }
    if let Some(edges) = borders {
        let edge = |e: Edge, rect: Rect| {
            (e.style.visible() && e.width > 0.0).then_some(BorderSegment {
                rect,
                color: e.color,
                style: e.style,
            })
        };
        for (r, &row_y) in ys.iter().enumerate().take(edges.rows + 1) {
            for c in 0..edges.cols {
                let e = edges.horizontal[r * edges.cols + c];
                let outer_left = if c == 0 && r < edges.rows {
                    edges.vertical[r * (edges.cols + 1)].width / 2.0
                } else {
                    0.0
                };
                let outer_right = if c + 1 == edges.cols && r < edges.rows {
                    edges.vertical[r * (edges.cols + 1) + edges.cols].width / 2.0
                } else {
                    0.0
                };
                if let Some(segment) = edge(
                    e,
                    Rect {
                        x: xs[c] - outer_left,
                        y: row_y - e.width / 2.0,
                        width: widths[c] + outer_left + outer_right,
                        height: e.width,
                    },
                ) {
                    b.table_borders.push(segment);
                }
            }
        }
        for r in 0..edges.rows {
            for (c, &col_x) in xs.iter().enumerate().take(edges.cols + 1) {
                let e = edges.vertical[r * (edges.cols + 1) + c];
                let half = |row: usize| {
                    let left = c.saturating_sub(1);
                    let right = c.min(edges.cols.saturating_sub(1));
                    [left, right]
                        .into_iter()
                        .map(|col| edges.horizontal[row * edges.cols + col])
                        .filter(|e| e.style.visible())
                        .map(|e| e.width / 2.0)
                        .fold(0.0, f32::max)
                };
                let (top, bottom) = (half(r), half(r + 1));
                if let Some(segment) = edge(
                    e,
                    Rect {
                        x: col_x - e.width / 2.0,
                        y: ys[r] + top,
                        width: e.width,
                        height: (row_heights[r] - top - bottom).max(0.0),
                    },
                ) {
                    b.table_borders.push(segment);
                }
            }
        }
    }
    let padding = b.padding;
    let border = b.style.border_width;
    b.table_rect = Some(Rect {
        x: x - padding.left - border.left,
        y: top,
        width: used_width + padding.left + padding.right + border.left + border.right,
        height: grid_height + padding.top + padding.bottom + border.top + border.bottom,
    });
    b.collapsed_table_part = collapse;
    let mut bottom = y + top + grid_height;
    for caption in b.children.iter_mut().filter(|c| {
        c.style.display == Display::TableCaption
            && c.style.caption_side == CaptionSide::Bottom
            && !c.style.position.out_of_flow()
    }) {
        super::block::layout_caption(caption, caption_x, caption_width, font, diagnostics);
        bottom += caption.margin.top;
        super::block::place_block(caption, bottom as f64, diagnostics);
        bottom += caption.rect.height + caption.margin.bottom;
    }
    fn sanitize(b: &mut LayoutBox, diagnostics: &mut Vec<Diagnostic>) {
        b.rect = super::tree::finite_rect(b.rect, b.node_id, diagnostics);
        b.content_rect = super::tree::finite_rect(b.content_rect, b.node_id, diagnostics);
        if let Some(rect) = b.table_rect {
            b.table_rect = Some(super::tree::finite_rect(rect, b.node_id, diagnostics));
        }
        for edge in &mut b.table_borders {
            edge.rect = super::tree::finite_rect(edge.rect, b.node_id, diagnostics);
        }
        for child in &mut b.children {
            sanitize(child, diagnostics);
        }
    }
    sanitize(b, diagnostics);
    finite_used(
        (bottom as f64 - y as f64).max(0.0),
        b.node_id,
        "table height",
        diagnostics,
    )
}

fn has_content(b: &LayoutBox) -> bool {
    b.image.is_some()
        || !b.line_boxes.is_empty()
        || !b.text_fragments.is_empty()
        || b.children.iter().any(has_content)
}
