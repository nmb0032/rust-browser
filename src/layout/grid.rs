//! A bounded horizontal Grid Level 1 model: expanded explicit lines, phased
//! occupancy placement, intrinsic track growth, then flexible allocation.
//! Item contents remain engine block/inline/flex/grid boxes and shaped runs.
use std::collections::BTreeMap;

use crate::{
    css::values::{Edges, Length},
    style::{
        AlignKeyword as A, Alignment, Diagnostic, Direction, GridLine, GridRepeat, GridTrack,
        TrackBreadth as B, TrackSize as S,
    },
};

use super::{
    LayoutBox, Rect, block, flex, positioning, sizing,
    tree::{finite_used, resolve_edges, resolve_length, resolve_margins},
};

const LIMIT: usize = crate::style::grid::MAX_GRID_TRACKS;
const SEARCH_LIMIT: usize = 1_048_576;

fn report(b: &LayoutBox, message: &str, diagnostics: &mut Vec<Diagnostic>) {
    if !diagnostics
        .iter()
        .any(|d| d.node_id == b.node_id && d.message == message)
    {
        diagnostics.push(Diagnostic {
            node_id: b.node_id,
            message: message.into(),
        });
    }
}

fn length(
    b: &LayoutBox,
    l: &Length,
    reference: Option<f32>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f32> {
    if l.has_percentage() && reference.is_none() {
        return None;
    }
    Some(
        resolve_length(
            l,
            b.style.font_size,
            reference.unwrap_or(0.0),
            b.node_id,
            "grid length",
            diagnostics,
        )
        .max(0.0),
    )
}

fn gap(
    b: &LayoutBox,
    l: Option<&Length>,
    reference: Option<f32>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    l.map_or(0.0, |l| {
        length(b, l, reference, diagnostics).unwrap_or_else(|| {
            report(
                b,
                "grid: cyclic percentage gap contributes 0px in an indefinite axis",
                diagnostics,
            );
            0.0
        })
    })
}

#[derive(Default)]
struct Axis {
    specs: Vec<S>,
    names: BTreeMap<String, Vec<i32>>,
    auto_fit: Vec<bool>,
    explicit: usize,
    origin: i32,
}

fn repeat_size(
    b: &LayoutBox,
    s: &S,
    reference: Option<f32>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f64 {
    let resolve = |breadth: &B, diagnostics: &mut Vec<Diagnostic>| match breadth {
        B::Length(l) => length(b, l, reference, diagnostics),
        _ => None,
    };
    match s {
        S::Breadth(breadth) => resolve(breadth, diagnostics).unwrap_or(0.0) as f64,
        S::MinMax(min, max) => resolve(max, diagnostics)
            .unwrap_or(0.0)
            .max(resolve(min, diagnostics).unwrap_or(0.0)) as f64,
        S::FitContent(_) => 0.0,
    }
}

fn expand(
    b: &LayoutBox,
    source: &[GridTrack],
    automatic: &[GridTrack],
    reference: Option<f32>,
    gutter: f32,
    horizontal: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Axis {
    fn fixed_sum(
        b: &LayoutBox,
        values: &[GridTrack],
        reference: Option<f32>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> (f64, f64) {
        let mut result = (0.0, 0.0);
        for value in values {
            match value {
                GridTrack::Size(s) => {
                    result.0 += repeat_size(b, s, reference, diagnostics);
                    result.1 += 1.0;
                }
                GridTrack::Repeat(GridRepeat::Count(n), values) => {
                    let part = fixed_sum(b, values, reference, diagnostics);
                    result.0 += part.0 * *n as f64;
                    result.1 += part.1 * *n as f64;
                }
                _ => {}
            }
        }
        result
    }
    fn append(
        axis: &mut Axis,
        values: &[GridTrack],
        fit: bool,
        b: &LayoutBox,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for value in values {
            match value {
                GridTrack::Names(names) => {
                    for name in names {
                        let line = axis.specs.len() as i32;
                        let lines = axis.names.entry(name.clone()).or_default();
                        if lines.last() != Some(&line) {
                            lines.push(line);
                        }
                    }
                }
                GridTrack::Size(size) => {
                    if axis.specs.len() == LIMIT {
                        report(
                            b,
                            "grid: 256-track axis limit reached; excess tracks omitted",
                            diagnostics,
                        );
                        return;
                    }
                    axis.specs.push(size.clone());
                    axis.auto_fit.push(fit);
                }
                _ => {}
            }
        }
    }
    let mut axis = Axis::default();
    let other = fixed_sum(b, source, reference, diagnostics);
    for value in source {
        if let GridTrack::Repeat(count, values) = value {
            let n = match count {
                GridRepeat::Count(n) => *n,
                _ => {
                    if let Some(reference) = reference {
                        let pattern = fixed_sum(b, values, Some(reference), diagnostics);
                        let denominator = pattern.0.max(pattern.1) + gutter as f64 * pattern.1;
                        (((reference as f64 - other.0 - gutter as f64 * other.1 + gutter as f64)
                            / denominator.max(1.0))
                        .floor()
                        .max(1.0) as usize)
                            .min(LIMIT + 1)
                    } else {
                        1
                    }
                }
            };
            let pattern_tracks = values
                .iter()
                .filter(|v| matches!(v, GridTrack::Size(_)))
                .count();
            if n.saturating_mul(pattern_tracks)
                .saturating_add(axis.specs.len())
                > LIMIT
            {
                report(
                    b,
                    "grid: 256-track axis limit reached; excess tracks omitted",
                    diagnostics,
                );
            }
            for _ in 0..n.min(LIMIT) {
                append(
                    &mut axis,
                    values,
                    *count == GridRepeat::AutoFit,
                    b,
                    diagnostics,
                );
                if axis.specs.len() == LIMIT {
                    break;
                }
            }
        } else {
            append(
                &mut axis,
                std::slice::from_ref(value),
                false,
                b,
                diagnostics,
            );
        }
    }
    let areas = &b.style.grid_template_areas;
    let area_count = if horizontal {
        areas.first().map_or(0, Vec::len)
    } else {
        areas.len()
    };
    let sized_explicit = axis.specs.len();
    while axis.specs.len() < area_count.min(LIMIT) {
        axis.specs.push(auto_spec(
            automatic,
            axis.specs.len() as i32 - sized_explicit as i32,
        ));
        axis.auto_fit.push(false);
    }
    let mut area_bounds = BTreeMap::<&str, (i32, i32)>::new();
    for (row, cells) in areas.iter().enumerate() {
        for (column, name) in cells.iter().enumerate() {
            let Some(name) = name else { continue };
            let start = if horizontal { column } else { row } as i32;
            let end = start + 1;
            let bounds = area_bounds.entry(name).or_insert((start, end));
            bounds.0 = bounds.0.min(start);
            bounds.1 = bounds.1.max(end);
        }
    }
    for (name, (start, end)) in area_bounds {
        for (suffix, line) in [("start", start), ("end", end)] {
            let lines = axis.names.entry(format!("{name}-{suffix}")).or_default();
            if !lines.contains(&line) {
                lines.push(line);
                lines.sort_unstable();
            }
        }
    }
    axis.explicit = axis.specs.len();
    axis
}

fn auto_spec(values: &[GridTrack], index: i32) -> S {
    if values.is_empty() {
        return S::default();
    }
    let index = index.rem_euclid(values.len() as i32) as usize;
    match &values[index] {
        GridTrack::Size(s) => s.clone(),
        _ => S::default(),
    }
}

fn line_index(line: &GridLine, axis: &Axis, end: bool) -> Option<i32> {
    let GridLine::Line(number, name) = line else {
        return None;
    };
    let number = *number as i64;
    let value = if let Some(name) = name {
        let area_name = format!("{name}-{}", if end { "end" } else { "start" });
        let lines = axis.names.get(name).or_else(|| axis.names.get(&area_name));
        let count = lines.map_or(0, Vec::len) as i64;
        if number > 0 {
            if number <= count {
                lines.unwrap()[(number - 1) as usize] as i64
            } else {
                axis.explicit as i64 + number - count
            }
        } else if -number <= count {
            lines.unwrap()[(count + number) as usize] as i64
        } else {
            number + count
        }
    } else if number > 0 {
        number - 1
    } else {
        axis.explicit as i64 + 1 + number
    };
    Some(value.clamp(-(LIMIT as i64), LIMIT as i64) as i32)
}

#[derive(Clone, Copy)]
struct Placement {
    start: Option<i32>,
    span: usize,
}

fn placement(
    b: &LayoutBox,
    start: &GridLine,
    end: &GridLine,
    axis: &Axis,
    diagnostics: &mut Vec<Diagnostic>,
) -> Placement {
    let first = line_index(start, axis, false);
    let last = line_index(end, axis, true);
    if first.is_none()
        && last.is_none()
        && matches!(start, GridLine::Span(_, Some(_)) | GridLine::Auto)
        && [start, end]
            .iter()
            .any(|line| matches!(line, GridLine::Span(_, Some(_))))
    {
        report(
            b,
            "grid: unanchored named span uses integer span fallback; named auto-span search is deferred",
            diagnostics,
        );
    }
    let span = |line: &GridLine, anchor: Option<i32>, forward: bool| {
        if let GridLine::Span(count, name) = line {
            let count = (*count).min(LIMIT);
            if let (Some(name), Some(anchor)) = (name, anchor) {
                let mut remaining = count;
                for delta in 1..=LIMIT {
                    let pos = anchor
                        + if forward {
                            delta as i32
                        } else {
                            -(delta as i32)
                        };
                    if pos < 0
                        || pos > axis.explicit as i32
                        || axis
                            .names
                            .get(name)
                            .is_some_and(|lines| lines.contains(&pos))
                    {
                        remaining -= 1;
                        if remaining == 0 {
                            return delta;
                        }
                    }
                }
                return LIMIT;
            }
            count
        } else {
            1
        }
    };
    if [start, end].iter().any(|l| match l {
        GridLine::Span(n, _) => *n > LIMIT,
        GridLine::Line(n, _) => n.unsigned_abs() as usize > LIMIT + 1,
        _ => false,
    }) {
        report(
            b,
            "grid: line/span exceeds 256-track axis limit; placement clamped",
            diagnostics,
        );
    }
    match (first, last) {
        (Some(a), Some(z)) => Placement {
            start: Some(a.min(z)),
            span: (z - a).unsigned_abs().max(1).min(LIMIT as u32) as usize,
        },
        (Some(a), None) => Placement {
            start: Some(a),
            span: span(end, Some(a), true),
        },
        (None, Some(z)) => {
            let n = span(start, Some(z), false);
            Placement {
                start: Some(z - n as i32),
                span: n,
            }
        }
        _ => Placement {
            start: None,
            span: if matches!(start, GridLine::Span(_, _)) {
                span(start, None, true)
            } else {
                span(end, None, true)
            },
        },
    }
}

#[derive(Clone)]
struct Cell {
    index: usize,
    col: Placement,
    row: Placement,
}

struct Grid {
    columns: Axis,
    rows: Axis,
    cells: Vec<Cell>,
}

fn place(
    b: &LayoutBox,
    references: (Option<f32>, Option<f32>),
    gaps: (f32, f32),
    diagnostics: &mut Vec<Diagnostic>,
) -> Grid {
    let mut columns = expand(
        b,
        &b.style.grid_template_columns,
        &b.style.grid_auto_columns,
        references.0,
        gaps.0,
        true,
        diagnostics,
    );
    let mut rows = expand(
        b,
        &b.style.grid_template_rows,
        &b.style.grid_auto_rows,
        references.1,
        gaps.1,
        false,
        diagnostics,
    );
    let mut indices: Vec<_> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_grid_item)
        .map(|(i, c)| (c.style.order, i))
        .collect();
    indices.sort_by_key(|&(order, i)| (order, i));
    let mut cells: Vec<_> = indices
        .into_iter()
        .map(|(_, index)| {
            let c = &b.children[index];
            Cell {
                index,
                col: placement(
                    c,
                    &c.style.grid_column_start,
                    &c.style.grid_column_end,
                    &columns,
                    diagnostics,
                ),
                row: placement(
                    c,
                    &c.style.grid_row_start,
                    &c.style.grid_row_end,
                    &rows,
                    diagnostics,
                ),
            }
        })
        .collect();
    columns.origin = cells
        .iter()
        .filter_map(|c| c.col.start)
        .min()
        .unwrap_or(0)
        .min(0)
        .max(-(LIMIT as i32 - 1));
    rows.origin = cells
        .iter()
        .filter_map(|c| c.row.start)
        .min()
        .unwrap_or(0)
        .min(0)
        .max(-(LIMIT as i32 - 1));
    for cell in &mut cells {
        for (p, origin) in [
            (&mut cell.col, columns.origin),
            (&mut cell.row, rows.origin),
        ] {
            p.span = p.span.clamp(1, LIMIT);
            if let Some(start) = &mut p.start {
                let raw = *start - origin;
                *start = raw.clamp(0, LIMIT as i32 - 1);
                let span = p.span.min(LIMIT - *start as usize);
                if *start != raw || span != p.span {
                    report(
                        &b.children[cell.index],
                        "grid: implicit extent exceeds 256-track axis limit; placement clamped",
                        diagnostics,
                    );
                }
                p.span = span;
            }
        }
    }
    let column_flow = b.style.grid_auto_flow.column;
    let minor_explicit = if column_flow {
        rows.explicit as i32 - rows.origin
    } else {
        columns.explicit as i32 - columns.origin
    };
    let major = |c: &Cell| if column_flow { c.col } else { c.row };
    let minor = |c: &Cell| if column_flow { c.row } else { c.col };
    let mut occupied = vec![false; LIMIT * LIMIT];
    let mut budget = SEARCH_LIMIT;
    let free =
        |occupied: &[bool], a: usize, z: usize, spans: (usize, usize), budget: &mut usize| {
            a + spans.0 <= LIMIT
                && z + spans.1 <= LIMIT
                && (a..a + spans.0).all(|r| {
                    (z..z + spans.1).all(|c| {
                        if *budget == 0 {
                            return false;
                        }
                        *budget -= 1;
                        !occupied[r * LIMIT + c]
                    })
                })
        };
    let install = |c: &mut Cell, a: usize, z: usize, occupied: &mut [bool]| {
        let (p, q) = if column_flow {
            (&mut c.col, &mut c.row)
        } else {
            (&mut c.row, &mut c.col)
        };
        p.start = Some(a as i32);
        q.start = Some(z as i32);
        for r in a..a + p.span {
            for k in z..z + q.span {
                occupied[r * LIMIT + k] = true;
            }
        }
    };
    let mut minor_extent = (minor_explicit as usize).clamp(1, LIMIT);
    for cell in &cells {
        let m = minor(cell);
        minor_extent = minor_extent
            .max(m.start.map_or(m.span, |s| s as usize + m.span))
            .min(LIMIT);
    }
    // Fully definite items overlap deliberately, without moving to a free cell.
    for cell in &mut cells {
        if let (Some(a), Some(z)) = (major(cell).start, minor(cell).start) {
            install(cell, a as usize, z as usize, &mut occupied);
        }
    }
    // Items locked to the major axis precede the remaining cursor placement.
    let mut locked_cursors = vec![0; LIMIT];
    for cell in &mut cells {
        if let (Some(a), None) = (major(cell).start, minor(cell).start) {
            let a = a as usize;
            let spans = (major(cell).span, minor(cell).span);
            let begin = if b.style.grid_auto_flow.dense {
                0
            } else {
                locked_cursors[a]
            };
            let z = (begin..=LIMIT - spans.1).find(|&z| {
                if budget == 0 {
                    return false;
                }
                free(&occupied, a, z, spans, &mut budget)
            });
            let z = z.unwrap_or_else(|| { report(b, "grid: occupancy search/axis limit reached; remaining items overlap at bounded fallback cells", diagnostics); LIMIT - spans.1 });
            install(cell, a, z, &mut occupied);
            minor_extent = minor_extent.max(z + spans.1);
            locked_cursors[a] = z + spans.1;
        }
    }
    let mut cursor = (0usize, 0usize);
    for cell in &mut cells {
        if major(cell).start.is_some() {
            continue;
        }
        if b.style.grid_auto_flow.dense {
            cursor = (0, 0);
        }
        let spans = (major(cell).span, minor(cell).span);
        let locked = minor(cell).start.map(|s| s as usize);
        if let Some(z) = locked {
            if z < cursor.1 {
                cursor.0 += 1;
            }
            cursor.1 = z;
        }
        let mut found = None;
        while cursor.0 + spans.0 <= LIMIT && budget > 0 {
            if cursor.1 + spans.1 > minor_extent {
                cursor = (cursor.0 + 1, locked.unwrap_or(0));
                continue;
            }
            if free(&occupied, cursor.0, cursor.1, spans, &mut budget) {
                found = Some(cursor);
                break;
            }
            if locked.is_some() {
                cursor.0 += 1;
            } else {
                cursor.1 += 1;
            }
        }
        let (a, z) = found.unwrap_or_else(|| {
            report(b, "grid: occupancy search/axis limit reached; remaining items overlap at bounded fallback cells", diagnostics);
            (LIMIT - spans.0, minor_extent - spans.1)
        });
        install(cell, a, z, &mut occupied);
    }
    let column_count = cells
        .iter()
        .map(|c| c.col.start.unwrap() as usize + c.col.span)
        .max()
        .unwrap_or(0)
        .max((columns.explicit as i32 - columns.origin) as usize)
        .min(LIMIT);
    let row_count = cells
        .iter()
        .map(|c| c.row.start.unwrap() as usize + c.row.span)
        .max()
        .unwrap_or(0)
        .max((rows.explicit as i32 - rows.origin) as usize)
        .min(LIMIT);
    for (axis, count, values) in [
        (&mut columns, column_count, &b.style.grid_auto_columns),
        (&mut rows, row_count, &b.style.grid_auto_rows),
    ] {
        let explicit_specs = std::mem::take(&mut axis.specs);
        let explicit_fit = std::mem::take(&mut axis.auto_fit);
        for index in 0..count {
            let line = index as i32 + axis.origin;
            if line >= 0 && (line as usize) < explicit_specs.len() {
                axis.specs.push(explicit_specs[line as usize].clone());
                axis.auto_fit.push(explicit_fit[line as usize]);
            } else {
                let pattern_index = if line < 0 {
                    line
                } else {
                    line - axis.explicit as i32
                };
                axis.specs.push(auto_spec(values, pattern_index));
                axis.auto_fit.push(false);
            }
        }
    }
    Grid {
        columns,
        rows,
        cells,
    }
}

#[derive(Clone)]
struct Track {
    min: B,
    max: B,
    base: f64,
    limit: f64,
    fit: f64,
    fraction: f64,
    collapsed: bool,
}

fn tracks(
    b: &LayoutBox,
    axis: &Axis,
    cells: &[Cell],
    horizontal: bool,
    reference: Option<f32>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Track> {
    axis.specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let (min, max, fit) = match spec {
                S::Breadth(B::Fraction(f)) => (B::Auto, B::Fraction(*f), f64::INFINITY),
                S::Breadth(v) => (v.clone(), v.clone(), f64::INFINITY),
                S::MinMax(a, z) => (a.clone(), z.clone(), f64::INFINITY),
                S::FitContent(l) => (
                    B::Auto,
                    B::MaxContent,
                    length(b, l, reference, diagnostics).map_or(f64::INFINITY, f64::from),
                ),
            };
            let normalize = |v: B, diagnostics: &mut Vec<Diagnostic>| {
                if matches!(&v, B::Length(l) if l.has_percentage() && reference.is_none()) {
                    report(
                        b,
                        "grid: cyclic percentage track uses auto in an indefinite axis",
                        diagnostics,
                    );
                    B::Auto
                } else {
                    v
                }
            };
            let min = normalize(min, diagnostics);
            let max = normalize(max, diagnostics);
            let base = if let B::Length(l) = &min {
                length(b, l, reference, diagnostics).unwrap_or(0.0) as f64
            } else {
                0.0
            };
            let limit = if let B::Length(l) = &max {
                (length(b, l, reference, diagnostics).unwrap_or(0.0) as f64).max(base)
            } else {
                f64::INFINITY
            };
            let fraction = if let B::Fraction(f) = &max {
                *f as f64
            } else {
                0.0
            };
            let collapsed = axis.auto_fit[i]
                && !cells.iter().any(|c| {
                    let p = if horizontal { c.col } else { c.row };
                    (p.start.unwrap() as usize..p.start.unwrap() as usize + p.span).contains(&i)
                });
            Track {
                min,
                max,
                base: if collapsed { 0.0 } else { base },
                limit: if collapsed { 0.0 } else { limit },
                fit,
                fraction: if collapsed { 0.0 } else { fraction },
                collapsed,
            }
        })
        .collect()
}

fn gutter_sum(tracks: &[Track], gap: f32) -> f64 {
    tracks
        .iter()
        .filter(|t| !t.collapsed)
        .count()
        .saturating_sub(1) as f64
        * gap as f64
}

#[derive(Clone, Copy)]
struct Contribution {
    start: usize,
    span: usize,
    minimum: f64,
    min_content: f64,
    max_content: f64,
}

/// Water-fill a deficit only into eligible tracks, respecting each fixed or
/// fit-content growth ceiling; spanning items do not split into equal columns.
fn grow(tracks: &mut [Track], indices: &[usize], target: f64, limits: bool, weights: bool) {
    let current: f64 = tracks
        .iter()
        .map(|t| if limits { t.limit } else { t.base })
        .sum();
    let mut remaining = (target - current).max(0.0);
    let mut active = indices.to_vec();
    for _ in 0..=indices.len() {
        if remaining <= 1e-7 || active.is_empty() {
            break;
        }
        let factors: f64 = active.iter().map(|&i| tracks[i].fraction).sum();
        let weights = weights && factors > 0.0;
        let weight_sum = if weights {
            factors
        } else {
            active.len() as f64
        };
        let mut consumed = 0.0;
        active.retain(|&i| {
            let track = &mut tracks[i];
            let cap = if limits {
                track.fit.max(track.base)
            } else {
                track.limit
            };
            let value = if limits {
                &mut track.limit
            } else {
                &mut track.base
            };
            let share = remaining * if weights { track.fraction } else { 1.0 } / weight_sum;
            let delta = share.min((cap - *value).max(0.0));
            *value += delta;
            consumed += delta;
            *value < cap - 1e-7
        });
        remaining -= consumed;
        if consumed <= 1e-7 {
            break;
        }
    }
}

fn intrinsic_growth(tracks: &mut [Track], contributions: &[Contribution], gap: f32) {
    let mut contributions = contributions.to_vec();
    contributions.sort_by_key(|c| (c.span, c.start));
    // Base-size steps precede growth-limit steps. Flexible-spanning items are
    // handled after nonflexible spans with flex-factor-weighted growth.
    for flexible in [false, true] {
        for stage in 0..3 {
            for group in contributions.chunk_by(|a, b| a.span == b.span) {
                let mut planned = vec![0.0f64; tracks.len()];
                for c in group {
                    let original = &tracks[c.start..c.start + c.span];
                    let has_flex = original.iter().any(|t| matches!(t.max, B::Fraction(_)));
                    if has_flex != flexible {
                        continue;
                    }
                    let mut part = original.to_vec();
                    let mut indices: Vec<_> = part
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| {
                            !t.collapsed
                                && match stage {
                                    0 => matches!(t.min, B::Auto | B::MinContent | B::MaxContent),
                                    1 => matches!(t.min, B::MinContent | B::MaxContent),
                                    _ => matches!(t.min, B::MaxContent),
                                }
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if flexible {
                        indices.retain(|&i| matches!(part[i].max, B::Fraction(_)));
                    }
                    let value = match stage {
                        0 => c.minimum,
                        1 => c.min_content,
                        _ => c.max_content,
                    };
                    let value = (value - gutter_sum(&part, gap)).max(0.0);
                    grow(&mut part, &indices, value, false, flexible);
                    for (i, (old, new)) in original.iter().zip(&part).enumerate() {
                        planned[c.start + i] = planned[c.start + i].max(new.base - old.base);
                    }
                }
                // Equal-span items see the same bases; planned increases are
                // merged by maximum, not accumulated in source order.
                for (t, delta) in tracks.iter_mut().zip(planned) {
                    t.base += delta;
                    t.limit = t.limit.max(t.base);
                }
            }
        }
    }
    for t in tracks.iter_mut() {
        if !matches!(t.max, B::Length(_) | B::Fraction(_)) && !t.collapsed {
            t.limit = t.base;
        }
    }
    for stage in 0..2 {
        for group in contributions.chunk_by(|a, b| a.span == b.span) {
            let mut planned = vec![0.0f64; tracks.len()];
            for c in group {
                let original = &tracks[c.start..c.start + c.span];
                if original.iter().any(|t| matches!(t.max, B::Fraction(_))) {
                    continue;
                }
                let mut part = original.to_vec();
                let indices: Vec<_> = part
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| {
                        !t.collapsed
                            && if stage == 0 {
                                matches!(t.max, B::MinContent | B::MaxContent | B::Auto)
                            } else {
                                matches!(t.max, B::MaxContent | B::Auto)
                            }
                    })
                    .map(|(i, _)| i)
                    .collect();
                let value = if stage == 0 {
                    c.min_content
                } else {
                    c.max_content
                };
                let target = (value - gutter_sum(&part, gap)).max(0.0);
                grow(&mut part, &indices, target, true, false);
                for &i in &indices {
                    planned[c.start + i] =
                        planned[c.start + i].max(part[i].limit - original[i].limit);
                }
            }
            for (t, delta) in tracks.iter_mut().zip(planned) {
                t.limit += delta;
            }
        }
    }
}

fn fraction(tracks: &[Track], space: f64) -> f64 {
    let mut active: Vec<_> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.fraction > 0.0 && !t.collapsed)
        .map(|(i, _)| i)
        .collect();
    let mut used: f64 = tracks
        .iter()
        .filter(|t| t.fraction == 0.0 || t.collapsed)
        .map(|t| t.base)
        .sum();
    for _ in 0..=tracks.len() {
        let factors: f64 = active.iter().map(|&i| tracks[i].fraction).sum();
        let unit = ((space - used) / factors.max(1.0)).max(0.0);
        let previous = active.len();
        active.retain(|&i| {
            if unit * tracks[i].fraction < tracks[i].base {
                used += tracks[i].base;
                false
            } else {
                true
            }
        });
        if active.len() == previous || active.is_empty() {
            return unit;
        }
    }
    0.0
}

fn finish_tracks(
    tracks: &mut [Track],
    contributions: &[Contribution],
    gap: f32,
    available: Option<f32>,
    align: Alignment,
    min_intrinsic: bool,
) {
    intrinsic_growth(tracks, contributions, gap);
    let gutters = gutter_sum(tracks, gap);
    if min_intrinsic {
        return;
    }
    if let Some(available) = available {
        let indices: Vec<_> = tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| !matches!(t.max, B::Fraction(_)) && !t.collapsed)
            .map(|(i, _)| i)
            .collect();
        grow(
            tracks,
            &indices,
            (available as f64 - gutters).max(0.0),
            false,
            false,
        );
    } else {
        for t in tracks
            .iter_mut()
            .filter(|t| !matches!(t.max, B::Fraction(_)))
        {
            t.base = t.limit;
        }
    }
    let unit = if let Some(available) = available {
        fraction(tracks, available as f64 - gutters)
    } else {
        let mut unit = tracks
            .iter()
            .filter(|t| t.fraction > 0.0)
            .map(|t| t.base / t.fraction.min(1.0))
            .fold(0.0, f64::max);
        for c in contributions {
            let part = &tracks[c.start..c.start + c.span];
            if part.iter().any(|t| t.fraction > 0.0) {
                unit = unit.max(fraction(
                    part,
                    (c.max_content - gutter_sum(part, gap)).max(0.0),
                ));
            }
        }
        unit
    };
    for t in tracks.iter_mut() {
        t.base = t.base.max(unit * t.fraction);
    }
    if let Some(available) = available
        && matches!(align.keyword, A::Normal | A::Stretch)
    {
        let remaining =
            (available as f64 - gutters - tracks.iter().map(|t| t.base).sum::<f64>()).max(0.0);
        let count = tracks
            .iter()
            .filter(|t| !t.collapsed && matches!(t.max, B::Auto))
            .count();
        if count > 0 {
            for t in tracks
                .iter_mut()
                .filter(|t| !t.collapsed && matches!(t.max, B::Auto))
            {
                t.base += remaining / count as f64;
            }
        }
    }
}

struct Geometry {
    starts: Vec<f32>,
    ends: Vec<f32>,
}

fn geometry(
    b: &LayoutBox,
    tracks: &[Track],
    gap: f32,
    available: Option<f32>,
    align: Alignment,
    diagnostics: &mut Vec<Diagnostic>,
) -> Geometry {
    let natural = finite_used(
        tracks.iter().map(|t| t.base).sum::<f64>() + gutter_sum(tracks, gap),
        b.node_id,
        "grid track extent",
        diagnostics,
    );
    let remaining = available.unwrap_or(natural) - natural;
    let count = tracks.iter().filter(|t| !t.collapsed).count();
    let (offset, extra_gap) = flex::distribution(align, remaining, count, false);
    let mut cursor = offset as f64;
    let mut had_track = false;
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for t in tracks {
        if !t.collapsed && had_track {
            cursor += gap as f64 + extra_gap as f64;
        }
        starts.push(finite_used(
            cursor,
            b.node_id,
            "grid line coordinate",
            diagnostics,
        ));
        cursor += t.base;
        ends.push(finite_used(
            cursor,
            b.node_id,
            "grid line coordinate",
            diagnostics,
        ));
        had_track |= !t.collapsed;
    }
    Geometry { starts, ends }
}

fn area(g: &Geometry, placement: Placement) -> (f32, f32) {
    let start = placement.start.unwrap() as usize;
    (
        g.starts[start],
        (g.ends[start + placement.span - 1] - g.starts[start]).max(0.0),
    )
}

struct Item {
    index: usize,
    col: Placement,
    row: Placement,
    horizontal: f32,
    vertical: f32,
    margins: Edges<Option<f32>>,
    min_content: f32,
    max_content: f32,
    width: Option<f32>,
    height: Option<f32>,
    justify: Alignment,
    align: Alignment,
}

fn effective(mut own: Alignment, parent: Alignment, replaced: bool) -> Alignment {
    if own.keyword == A::Auto {
        own = parent;
    }
    if own.keyword == A::Normal {
        own.keyword = if replaced { A::Start } else { A::Stretch };
    }
    own
}

fn automatic_minimum(
    child: &LayoutBox,
    part: &[Track],
    horizontal: bool,
    content: f32,
    preferred: Option<f32>,
) -> f32 {
    let scrollable = if horizontal {
        child.style.overflow_x.scroll_container()
    } else {
        child.style.overflow_y.scroll_container()
    };
    if scrollable
        || !part.iter().any(|t| matches!(t.min, B::Auto))
        || part.len() > 1 && part.iter().any(|t| matches!(t.max, B::Fraction(_)))
    {
        return 0.0;
    }

    preferred.map_or(content, |size| content.min(size))
}

fn item_bounds(
    child: &LayoutBox,
    horizontal: bool,
    reference: Option<f32>,
    decor: f32,
    auto_min: f32,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    let (min, max) = if horizontal {
        (&child.style.min_width, &child.style.max_width)
    } else {
        (&child.style.min_height, &child.style.max_height)
    };
    let auto_min = if min.is_none() {
        (if horizontal {
            sizing::definite_size(child, max.as_ref(), reference, decor, diagnostics)
        } else {
            sizing::definite_height(child, max.as_ref(), reference, decor, diagnostics)
        })
        .map_or(auto_min, |cap| auto_min.min(cap))
    } else {
        auto_min
    };
    sizing::bounds(child, horizontal, reference, decor, auto_min, diagnostics)
}

fn item_data(
    b: &LayoutBox,
    cells: &[Cell],
    reference: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Item> {
    cells
        .iter()
        .map(|cell| {
            let c = &b.children[cell.index];
            let padding = resolve_edges(
                &c.style.padding,
                c.style.font_size,
                reference,
                c.node_id,
                "grid padding",
                diagnostics,
            )
            .map(|v| v.max(0.0));
            let border = c.style.border_width;
            let horizontal = finite_used(
                padding.left as f64
                    + padding.right as f64
                    + border.left as f64
                    + border.right as f64,
                c.node_id,
                "grid horizontal decorations",
                diagnostics,
            );
            let vertical = finite_used(
                padding.top as f64
                    + padding.bottom as f64
                    + border.top as f64
                    + border.bottom as f64,
                c.node_id,
                "grid vertical decorations",
                diagnostics,
            );
            let margins = resolve_margins(&c.style, reference, c.node_id, diagnostics);
            let (mut min_content, mut max_content) =
                positioning::intrinsic_content(c, reference, font, diagnostics);
            let height =
                sizing::definite_height(c, c.style.height.as_ref(), None, vertical, diagnostics);
            if let Some(transferred) =
                height.and_then(|h| sizing::ratio_width(c, h, horizontal, vertical))
            {
                min_content = if c.image.is_some() {
                    min_content.min(transferred)
                } else {
                    min_content.max(transferred)
                };
                max_content = max_content.max(transferred);
            }
            Item {
                index: cell.index,
                col: cell.col,
                row: cell.row,
                horizontal,
                vertical,
                margins,
                min_content,
                max_content,
                width: sizing::definite_size(
                    c,
                    c.style.width.as_ref(),
                    None,
                    horizontal,
                    diagnostics,
                ),
                height: sizing::definite_height(
                    c,
                    c.style.height.as_ref(),
                    None,
                    vertical,
                    diagnostics,
                ),
                justify: effective(
                    c.style.justify_self,
                    b.style.justify_items,
                    c.image.is_some() || c.style.aspect_ratio.ratio.is_some(),
                ),
                align: effective(
                    c.style.align_self,
                    b.style.align_items,
                    c.image.is_some() || c.style.aspect_ratio.ratio.is_some(),
                ),
            }
        })
        .collect()
}

fn width_contributions(
    b: &LayoutBox,
    items: &[Item],
    tracks: &[Track],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Contribution> {
    items
        .iter()
        .map(|item| {
            let c = &b.children[item.index];
            let start = item.col.start.unwrap() as usize;
            let part = &tracks[start..start + item.col.span];
            let auto_min = automatic_minimum(c, part, true, item.min_content, item.width);
            let limits = item_bounds(c, true, None, item.horizontal, auto_min, diagnostics);
            let extra = item.horizontal
                + item.margins.left.unwrap_or(0.0)
                + item.margins.right.unwrap_or(0.0);
            let specified = item.width.map(|w| w.clamp(limits.0, limits.1));
            let cap = sizing::definite_size(
                c,
                c.style.max_width.as_ref(),
                None,
                item.horizontal,
                diagnostics,
            )
            .unwrap_or(f32::INFINITY)
            .max(limits.0);
            Contribution {
                start,
                span: item.col.span,
                minimum: (specified.unwrap_or(limits.0).min(cap) + extra) as f64,
                min_content: (specified
                    .unwrap_or(item.min_content)
                    .clamp(limits.0, limits.1)
                    + extra) as f64,
                max_content: (specified
                    .unwrap_or(item.max_content)
                    .clamp(limits.0, limits.1)
                    + extra) as f64,
            }
        })
        .collect()
}

fn format_width(
    b: &mut LayoutBox,
    item: &Item,
    column_tracks: &[Track],
    area_width: f32,
    row_reference: Option<f32>,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let c = &mut b.children[item.index];
    let start = item.col.start.unwrap() as usize;
    let part = &column_tracks[start..start + item.col.span];
    let width = sizing::definite_size(
        c,
        c.style.width.as_ref(),
        Some(area_width),
        item.horizontal,
        diagnostics,
    );
    let height = sizing::definite_height(
        c,
        c.style.height.as_ref(),
        row_reference,
        item.vertical,
        diagnostics,
    );
    let width = width
        .or_else(|| height.and_then(|h| sizing::ratio_width(c, h, item.horizontal, item.vertical)));
    let auto_min = automatic_minimum(c, part, true, item.min_content, width);
    let limits = item_bounds(
        c,
        true,
        Some(area_width),
        item.horizontal,
        auto_min,
        diagnostics,
    );
    let available = (area_width
        - item.horizontal
        - item.margins.left.unwrap_or(0.0)
        - item.margins.right.unwrap_or(0.0))
    .max(0.0);
    let stretch = item.justify.keyword == A::Stretch
        && item.margins.left.is_some()
        && item.margins.right.is_some();
    let width = width
        .unwrap_or_else(|| {
            if stretch {
                available
            } else {
                item.min_content.max(available).min(item.max_content)
            }
        })
        .clamp(limits.0, limits.1);
    c.reset();
    block::layout_sized(
        c,
        area_width,
        width,
        height,
        (true, height.is_some()),
        font,
        diagnostics,
    );
    let height_limits = sizing::bounds(c, false, row_reference, item.vertical, 0.0, diagnostics);
    let constrained = c
        .content_rect
        .height
        .clamp(height_limits.0, height_limits.1);
    if constrained != c.content_rect.height {
        c.reset();
        block::layout_sized(
            c,
            area_width,
            width,
            Some(constrained),
            (true, height.is_some()),
            font,
            diagnostics,
        );
    }
}

fn offset(
    align: Alignment,
    available: f32,
    size: f32,
    before: Option<f32>,
    after: Option<f32>,
) -> (f32, f32, f32) {
    let remaining = available - size - before.unwrap_or(0.0) - after.unwrap_or(0.0);
    if remaining > 0.0 && (before.is_none() || after.is_none()) {
        let share =
            remaining / (usize::from(before.is_none()) + usize::from(after.is_none())) as f32;
        let before = before.unwrap_or(share);
        return (before, before, after.unwrap_or(share));
    }
    let shift = flex::distribution(align, remaining, 1, false).0;
    (
        before.unwrap_or(0.0) + shift,
        before.unwrap_or(0.0),
        after.unwrap_or(0.0),
    )
}

pub(super) fn layout_grid(
    b: &mut LayoutBox,
    (x, y, width): (f32, f32, f32),
    height: Option<f32>,
    definite_height: Option<f32>,
    height_bounds: (f32, f32),
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    // Intrinsic queries use None; this pass has the resolved grid content
    // width, including a shrink-to-fit inline grid's used width.
    let references = (Some(width), definite_height);
    let column_gap = gap(b, b.style.column_gap.as_ref(), references.0, diagnostics);
    let mut row_gap = gap(b, b.style.row_gap.as_ref(), references.1, diagnostics);
    let grid = place(b, references, (column_gap, row_gap), diagnostics);
    let mut columns = tracks(
        b,
        &grid.columns,
        &grid.cells,
        true,
        references.0,
        diagnostics,
    );
    let mut rows = tracks(b, &grid.rows, &grid.cells, false, references.1, diagnostics);
    let mut items = item_data(b, &grid.cells, 0.0, font, diagnostics);
    let contributions = width_contributions(b, &items, &columns, diagnostics);
    finish_tracks(
        &mut columns,
        &contributions,
        column_gap,
        Some(width),
        b.style.justify_content,
        false,
    );
    let mut justify_content = b.style.justify_content;
    if justify_content.keyword == A::Left {
        justify_content.keyword = if b.style.direction == Direction::Rtl {
            A::End
        } else {
            A::Start
        };
    } else if justify_content.keyword == A::Right {
        justify_content.keyword = if b.style.direction == Direction::Rtl {
            A::Start
        } else {
            A::End
        };
    }
    let cg = geometry(
        b,
        &columns,
        column_gap,
        Some(width),
        justify_content,
        diagnostics,
    );
    for item in &mut items {
        let (_, area_width) = area(&cg, item.col);
        // Percentage padding/margins resolve against the grid area, not the
        // entire grid. Intrinsic contributions above use an indefinite axis.
        let child = &b.children[item.index];
        let padding = resolve_edges(
            &child.style.padding,
            child.style.font_size,
            area_width,
            child.node_id,
            "grid area padding",
            diagnostics,
        )
        .map(|v| v.max(0.0));
        item.horizontal = padding.left
            + padding.right
            + child.style.border_width.left
            + child.style.border_width.right;
        item.vertical = padding.top
            + padding.bottom
            + child.style.border_width.top
            + child.style.border_width.bottom;
        item.margins = resolve_margins(&child.style, area_width, child.node_id, diagnostics);
        let row_start = item.row.start.unwrap() as usize;
        let row_part = &rows[row_start..row_start + item.row.span];
        let fixed_height = row_part
            .iter()
            .all(|t| matches!(t.min, B::Length(_)) && matches!(t.max, B::Length(_)))
            .then(|| {
                (row_part.iter().map(|t| t.base).sum::<f64>() + gutter_sum(row_part, row_gap))
                    as f32
            });
        format_width(
            b,
            item,
            &columns,
            area_width,
            fixed_height,
            font,
            diagnostics,
        );
    }
    let mut baselines = vec![0.0f32; rows.len()];
    let mut descents = vec![0.0f32; rows.len()];
    let mut last_descents = vec![0.0f32; rows.len()];
    let mut last_ascents = vec![0.0f32; rows.len()];
    let mut row_contributions = Vec::new();
    for item in &items {
        let c = &b.children[item.index];
        let start = item.row.start.unwrap() as usize;
        let part = &rows[start..start + item.row.span];
        let content = c.content_rect.height;
        let extra =
            item.vertical + item.margins.top.unwrap_or(0.0) + item.margins.bottom.unwrap_or(0.0);
        let auto_min = automatic_minimum(c, part, false, content, item.height);
        let limits = item_bounds(c, false, None, item.vertical, auto_min, diagnostics);
        let minimum = item.height.unwrap_or(limits.0).min(limits.1);
        row_contributions.push(Contribution {
            start,
            span: item.row.span,
            minimum: (minimum + extra) as f64,
            min_content: (content + extra) as f64,
            max_content: (content + extra) as f64,
        });
        if item.align.keyword == A::Baseline
            && item.margins.top.is_some()
            && item.margins.bottom.is_some()
        {
            let baseline = c.first_local_baseline().unwrap_or(c.rect.height);
            baselines[start] = baselines[start].max(baseline + item.margins.top.unwrap_or(0.0));
            descents[start] =
                descents[start].max(c.rect.height - baseline + item.margins.bottom.unwrap_or(0.0));
        }
        if item.align.keyword == A::LastBaseline
            && item.margins.top.is_some()
            && item.margins.bottom.is_some()
        {
            let end = start + item.row.span - 1;
            let baseline = c.last_local_baseline().unwrap_or(c.rect.height);
            last_ascents[end] = last_ascents[end].max(baseline + item.margins.top.unwrap_or(0.0));
            last_descents[end] = last_descents[end]
                .max(c.rect.height - baseline + item.margins.bottom.unwrap_or(0.0));
        }
    }
    for (start, (&baseline, &descent)) in baselines.iter().zip(&descents).enumerate() {
        if baseline > 0.0 {
            row_contributions.push(Contribution {
                start,
                span: 1,
                minimum: (baseline + descent) as f64,
                min_content: (baseline + descent) as f64,
                max_content: (baseline + descent) as f64,
            });
        }
        for (start, (&ascent, &descent)) in last_ascents.iter().zip(&last_descents).enumerate() {
            if ascent + descent > 0.0 {
                row_contributions.push(Contribution {
                    start,
                    span: 1,
                    minimum: (ascent + descent) as f64,
                    min_content: (ascent + descent) as f64,
                    max_content: (ascent + descent) as f64,
                });
            }
        }
    }
    finish_tracks(
        &mut rows,
        &row_contributions,
        row_gap,
        height,
        b.style.align_content,
        false,
    );
    let intrinsic_height = finite_used(
        rows.iter().map(|t| t.base).sum::<f64>() + gutter_sum(&rows, row_gap),
        b.node_id,
        "grid natural height",
        diagnostics,
    );
    let used_height = height
        .unwrap_or(intrinsic_height)
        .clamp(height_bounds.0, height_bounds.1);
    let cyclic_track = |s: &S| match s {
        S::Breadth(B::Length(l)) | S::FitContent(l) => l.has_percentage(),
        S::MinMax(a, z) => [a, z]
            .iter()
            .any(|v| matches!(v, B::Length(l) if l.has_percentage())),
        _ => false,
    };
    let cyclic = definite_height.is_none()
        && (grid.rows.specs.iter().any(cyclic_track)
            || b.style.row_gap.as_ref().is_some_and(Length::has_percentage));
    let constrained = height.is_none() && used_height != intrinsic_height;
    if cyclic || constrained {
        // Resolve once against the intrinsic container height, without feeding
        // percentage-induced overflow back into that height (CSS cyclic sizing).
        row_gap = gap(b, b.style.row_gap.as_ref(), Some(used_height), diagnostics);
        rows = tracks(
            b,
            &grid.rows,
            &grid.cells,
            false,
            Some(used_height),
            diagnostics,
        );
        finish_tracks(
            &mut rows,
            &row_contributions,
            row_gap,
            Some(used_height),
            b.style.align_content,
            false,
        );
    }
    let rg = geometry(
        b,
        &rows,
        row_gap,
        (height.is_some() || constrained).then_some(used_height),
        b.style.align_content,
        diagnostics,
    );
    let rtl = b.style.direction == Direction::Rtl;
    for item in &items {
        let (cx, area_width) = area(&cg, item.col);
        let (ry, area_height) = area(&rg, item.row);
        let start = item.row.start.unwrap() as usize;
        let c = &mut b.children[item.index];
        let explicit = sizing::definite_height(
            c,
            c.style.height.as_ref(),
            Some(area_height),
            item.vertical,
            diagnostics,
        );
        let auto_min = automatic_minimum(
            c,
            &rows[start..start + item.row.span],
            false,
            c.content_rect.height,
            explicit,
        );
        let bounds = item_bounds(
            c,
            false,
            Some(area_height),
            item.vertical,
            auto_min,
            diagnostics,
        );
        let stretch = item.align.keyword == A::Stretch
            && item.margins.top.is_some()
            && item.margins.bottom.is_some();
        let height = explicit
            .unwrap_or_else(|| {
                if stretch {
                    (area_height
                        - item.vertical
                        - item.margins.top.unwrap_or(0.0)
                        - item.margins.bottom.unwrap_or(0.0))
                    .max(0.0)
                } else {
                    c.content_rect.height
                }
            })
            .clamp(bounds.0, bounds.1);
        let item_width = c.content_rect.width;
        let height_definite = explicit.is_some() || stretch;
        if height != c.content_rect.height || c.definite_height.is_some() != height_definite {
            c.reset();
            block::layout_sized(
                c,
                area_width,
                item_width,
                Some(height),
                (true, height_definite),
                font,
                diagnostics,
            );
        }
        let (before, after) = if rtl {
            (item.margins.right, item.margins.left)
        } else {
            (item.margins.left, item.margins.right)
        };
        let mut justify = item.justify;
        if justify.keyword == A::Left {
            justify.keyword = if rtl { A::End } else { A::Start };
        }
        if justify.keyword == A::Right {
            justify.keyword = if rtl { A::Start } else { A::End };
        }
        if matches!(justify.keyword, A::SelfStart | A::SelfEnd)
            && c.style.direction != b.style.direction
        {
            justify.keyword = if justify.keyword == A::SelfStart {
                A::SelfEnd
            } else {
                A::SelfStart
            };
        }
        let (dx, mb, ma) = offset(justify, area_width, c.rect.width, before, after);
        let (mut dy, mt, mz) = offset(
            item.align,
            area_height,
            c.rect.height,
            item.margins.top,
            item.margins.bottom,
        );
        if item.align.keyword == A::Baseline
            && item.margins.top.is_some()
            && item.margins.bottom.is_some()
        {
            dy = baselines[start] - c.first_local_baseline().unwrap_or(c.rect.height);
        }
        if item.align.keyword == A::LastBaseline
            && item.margins.top.is_some()
            && item.margins.bottom.is_some()
        {
            dy = area_height
                - last_descents[start + item.row.span - 1]
                - c.last_local_baseline().unwrap_or(c.rect.height);
        }
        c.margin = Edges {
            left: if rtl { ma } else { mb },
            right: if rtl { mb } else { ma },
            top: mt,
            bottom: mz,
        };
        let target_x = if rtl {
            x + width - cx - dx - c.rect.width
        } else {
            x + cx + dx
        };
        let move_x = target_x - c.rect.x;
        positioning::translate(c, move_x, 0.0, diagnostics);
        // Preserve local Y until the shared tree finalizer; glyph-internal
        // positions and descendants must not be shifted twice.
        block::place_block(c, (y + ry + dy) as f64, diagnostics);
    }
    if matches!(b.style.justify_items.keyword, A::Baseline | A::LastBaseline)
        || b.children.iter().any(|c| {
            c.is_grid_item && matches!(c.style.justify_self.keyword, A::Baseline | A::LastBaseline)
        })
    {
        report(
            b,
            "grid: inline-axis baseline uses start fallback in horizontal writing",
            diagnostics,
        );
    }
    if matches!(b.style.align_content.keyword, A::Baseline | A::LastBaseline) {
        report(
            b,
            "grid: align-content baseline uses start fallback",
            diagnostics,
        );
    }
    for child in b
        .children
        .iter_mut()
        .filter(|c| c.style.position.out_of_flow())
    {
        let positioned_area = |first: &GridLine,
                               last: &GridLine,
                               axis: &Axis,
                               g: &Geometry,
                               defaults: (f32, f32),
                               diagnostics: &mut Vec<Diagnostic>| {
            let p = placement(child, first, last, axis, diagnostics);
            let line = |index: i32, end: bool| {
                let index = index - axis.origin;
                if end && index > 0 {
                    g.ends.get(index as usize - 1).copied()
                } else {
                    g.starts
                        .get(index.max(0) as usize)
                        .copied()
                        .filter(|_| index >= 0)
                }
            };
            let mut start = line_index(first, axis, false).and_then(|index| line(index, false));
            let mut end = line_index(last, axis, true).and_then(|index| line(index, true));
            if matches!(first, GridLine::Span(_, _)) && end.is_some() {
                start = p.start.and_then(|index| line(index, false));
            }
            if matches!(last, GridLine::Span(_, _)) && start.is_some() {
                end = p.start.and_then(|index| line(index + p.span as i32, true));
            }
            let start = start.unwrap_or(defaults.0);
            let end = end.unwrap_or(defaults.1);
            (start.min(end), (end - start).abs())
        };
        let owned = child.position_parent == Some(b.box_id);
        let horizontal_defaults = if owned {
            if rtl {
                (-b.padding.right, width + b.padding.left)
            } else {
                (-b.padding.left, width + b.padding.right)
            }
        } else {
            (0.0, width)
        };
        let natural_height = used_height;
        let vertical_defaults = if owned {
            (-b.padding.top, natural_height + b.padding.bottom)
        } else {
            (0.0, natural_height)
        };
        let (cx, cw) = positioned_area(
            &child.style.grid_column_start,
            &child.style.grid_column_end,
            &grid.columns,
            &cg,
            horizontal_defaults,
            diagnostics,
        );
        let (ry, rh) = positioned_area(
            &child.style.grid_row_start,
            &child.style.grid_row_end,
            &grid.rows,
            &rg,
            vertical_defaults,
            diagnostics,
        );
        let rect = Rect {
            x: if rtl { x + width - cx - cw } else { x + cx },
            y: y + ry,
            width: cw,
            height: rh,
        };
        child.rect.x = rect.x;
        child.rect.y = rect.y;
        child.grid_area = Some((b.box_id, rect));
        child.flex_static = Some(flex::StaticPosition {
            area: rect,
            row: true,
            main_reverse: rtl,
            cross_reverse: false,
            main_logical_reverse: false,
            cross_logical_reverse: false,
            justify: effective(child.style.justify_self, b.style.justify_items, true),
            align: effective(child.style.align_self, b.style.align_items, true),
        });
    }
    used_height
}

pub(super) fn intrinsic_widths(
    b: &LayoutBox,
    reference: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    let column_gap = gap(b, b.style.column_gap.as_ref(), None, diagnostics);
    let grid = place(b, (None, None), (column_gap, 0.0), diagnostics);
    let items = item_data(b, &grid.cells, 0.0, font, diagnostics);
    let mut min_tracks = tracks(b, &grid.columns, &grid.cells, true, None, diagnostics);
    let contributions = width_contributions(b, &items, &min_tracks, diagnostics);
    let mut max_tracks = min_tracks.clone();
    finish_tracks(
        &mut min_tracks,
        &contributions,
        column_gap,
        None,
        b.style.justify_content,
        true,
    );
    finish_tracks(
        &mut max_tracks,
        &contributions,
        column_gap,
        None,
        b.style.justify_content,
        false,
    );
    let min = finite_used(
        min_tracks.iter().map(|t| t.base).sum::<f64>() + gutter_sum(&min_tracks, column_gap),
        b.node_id,
        "grid min-content width",
        diagnostics,
    );
    let max = finite_used(
        max_tracks.iter().map(|t| t.base).sum::<f64>() + gutter_sum(&max_tracks, column_gap),
        b.node_id,
        "grid max-content width",
        diagnostics,
    );
    let _ = reference;
    (min, max.max(min))
}
