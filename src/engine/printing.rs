//! Explicit native print layout. No DOM cloning, bitmap slicing, PDF backend,
//! JavaScript readiness or operating-system printer is involved.
use super::Frame;
use crate::{
    css::{
        media::{MediaEnvironment, MediaType},
        values::{LengthOrAuto, Side},
    },
    layout::{
        LayoutBox, LayoutTree, Rect,
        fragmentation::{MAX_FRAGMENTAINERS, Opportunities, Range, adjust_fixed, fragment_tree},
    },
    paint::{self, DisplayItem},
    style::{self, Content, ContentItem, Diagnostic, Direction, fragmentation::Break},
    text::FontContext,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintOptions {
    /// Fallback physical sheet, in CSS pixels (96 px/in).
    pub width: f32,
    pub height: f32,
    pub margin: f32,
    pub max_pages: usize,
}
impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            width: 816.0,
            height: 1056.0,
            margin: 48.0,
            max_pages: MAX_FRAGMENTAINERS,
        }
    }
}
impl PrintOptions {
    pub fn validate(self) -> Result<(), &'static str> {
        if [self.width, self.height]
            .iter()
            .any(|v| !v.is_finite() || !(1.0..=4096.0).contains(v))
            || !self.margin.is_finite()
            || self.margin < 0.0
            || self.width <= 2.0 * self.margin
            || self.height <= 2.0 * self.margin
            || !(1..=MAX_FRAGMENTAINERS).contains(&self.max_pages)
        {
            return Err(
                "invalid print profile: positive sheets <=4096px, nonempty page area, 1..1024 pages required",
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct PrintedPage {
    pub number: usize,
    pub name: Option<String>,
    pub blank: bool,
    pub sheet: Rect,
    pub area: Rect,
    pub tree: LayoutTree,
    pub display_list: Vec<DisplayItem>,
    /// Independently shaped page marker text, useful for accessibility/debugging.
    pub margin_text: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct PrintLayout {
    pub pages: Vec<PrintedPage>,
    pub diagnostics: Vec<Diagnostic>,
    pub fonts: FontContext,
}

fn side_requirement(value: Break, rtl: bool) -> Option<bool> {
    match value {
        Break::Left => Some(false),
        Break::Right => Some(true),
        Break::Recto => Some(!rtl),
        Break::Verso => Some(rtl),
        _ => None,
    }
}
fn validate_page(page: &style::paged::PageStyle) -> Result<(), &'static str> {
    let r = page.sheet;
    if [r.width, r.height]
        .iter()
        .any(|v| !v.is_finite() || !(1.0..=4096.0).contains(v))
        || [page.area.width, page.area.height]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        || [page.area.x, page.area.y]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        || page.area.x + page.area.width > r.width
        || page.area.y + page.area.height > r.height
    {
        return Err("@page produces an invalid or empty bounded physical page");
    }
    Ok(())
}

impl Frame {
    /// Print a retained document snapshot through the production print-media
    /// cascade, font/resource context, box layout and software display list.
    /// Screen style, scroll identity, animation clock and DOM remain untouched.
    pub fn print_layout(&self, options: PrintOptions) -> Result<PrintLayout, &'static str> {
        options.validate()?;
        let mut frame = self.clone();
        let fonts = self.font_context().clone();
        let mut environment = MediaEnvironment {
            media_type: MediaType::Print,
            viewport_width: options.width - 2.0 * options.margin,
            viewport_height: options.height - 2.0 * options.margin,
            resolution_dppx: Some(1.0),
            ..self.media_environment()
        };
        let mut settled = None;
        for _ in 0..16 {
            frame.set_media_environment(environment);
            frame
                .load_pending_web_fonts(|_| Ok(()), || false)
                .map_err(|_| "print font resources failed")?;
            let source = frame.layout_with_container_queries(&fonts);
            let rules =
                style::paged::Rules::collect(&frame.document, &frame.stylesheets, environment);
            let root_style = source
                .root_element()
                .map_or_else(style::ComputedStyle::default, |b| b.style().clone());
            let mut diagnostics = rules.diagnostics.clone();
            let opportunities = Opportunities::collect(source.root(), true);
            let first_name = first_name(source.root()).or_else(|| {
                opportunities
                    .names
                    .iter()
                    .rfind(|(at, _)| *at <= 0.001)
                    .and_then(|(_, n)| n.clone())
            });
            let first = rules.resolve(
                &root_style,
                frame.document.root_id(),
                first_name.as_deref(),
                0,
                false,
                root_style.direction == Direction::Rtl,
                (options.width, options.height),
                options.margin,
                environment,
                frame.font_context(),
                &mut diagnostics,
            );
            validate_page(&first)?;
            if (environment.viewport_width - first.area.width).abs() < 0.001
                && (environment.viewport_height - first.area.height).abs() < 0.001
            {
                settled = Some((source, rules, root_style, first, diagnostics));
                break;
            }
            environment.viewport_width = first.area.width;
            environment.viewport_height = first.area.height;
        }
        let Some((mut source, rules, root_style, first, mut diagnostics)) = settled else {
            return Err("print page-area/style feedback did not converge within 16 passes");
        };
        frame
            .load_pending_effect_images(|_| Ok(()), || false)
            .map_err(|_| "print effect image resources failed")?;
        source.effect_images = frame.effect_images.clone();
        diagnostics.extend(frame.stylesheets.diagnostics.clone());
        diagnostics.extend(source.diagnostics().iter().cloned());
        let opportunities = Opportunities::collect(source.root(), true);
        let extent = source.root().content_rect().height.max(0.0);
        let rtl = root_style.direction == Direction::Rtl;
        if root_style.writing_mode.vertical() {
            return Err(
                "paged vertical block progression is outside the finite native print profile",
            );
        }
        let mut pages = Vec::new();
        let mut page_styles = Vec::new();
        let mut start = 0.0;
        let mut force = Break::Auto;
        if let Some(p) = source.root().children().iter().find_map(first_forced) {
            force = p;
        }
        while start < extent - 0.001 || pages.is_empty() {
            if pages.len() >= options.max_pages {
                return Err("print page limit exceeded");
            }
            let index = pages.len();
            let name = if start <= 0.001 {
                first_name(source.root())
            } else {
                None
            }
            .or_else(|| {
                opportunities
                    .names
                    .iter()
                    .rfind(|(at, _)| *at <= start + 0.001)
                    .and_then(|(_, name)| name.clone())
            });
            let right = index.is_multiple_of(2) != rtl;
            let blank = side_requirement(force, rtl).is_some_and(|desired| desired != right);
            let page = rules.resolve(
                &root_style,
                frame.document.root_id(),
                name.as_deref(),
                index,
                blank,
                rtl,
                (options.width, options.height),
                options.margin,
                environment,
                frame.font_context(),
                &mut diagnostics,
            );
            validate_page(&page)?;
            if (page.area.width - first.area.width).abs() > 0.001 {
                return Err(
                    "page-specific inline-size changes require continuation reflow (outside the finite print profile)",
                );
            }
            let mut range = if blank {
                Range {
                    start,
                    end: start,
                    next: start,
                    forced: Break::Auto,
                }
            } else {
                opportunities.next(start, page.area.height, extent, true)
            };
            // Named page changes create class-A page boundaries, even without a
            // break-before declaration. A same-name descendant is not a break.
            if !blank
                && let Some((at, _)) = opportunities.names.iter().find(|(at, next_name)| {
                    *at > start + 0.001 && *at < range.end - 0.001 && *next_name != name
                })
            {
                range.end = *at;
                range.next = *at;
                range.forced = Break::Page;
            }
            let mut tree = fragment_tree(&source, range, page.area, page.sheet, index, blank);
            adjust_fixed(&mut tree, page.area.height - first.area.height);
            if !blank && range.end - range.start > page.area.height + 0.001 {
                diagnostics.push(Diagnostic { node_id: None, message: format!("page {}: oversized indivisible formatting unit overflows the page area; clipped by the physical sheet", index + 1) });
            }
            let list = page_display_list(&tree, &page, environment);
            pages.push(PrintedPage {
                number: index + 1,
                name,
                blank,
                sheet: page.sheet,
                area: page.area,
                tree,
                display_list: list,
                margin_text: Vec::new(),
            });
            page_styles.push(page);
            force = if blank { force } else { range.forced };
            if !blank {
                start = range.next;
            }
        }
        // Total pages do not influence flow: these independently shaped margin
        // boxes are resolved after the bounded content pass, not DOM widgets.
        let total = pages.len();
        for (page, style) in pages.iter_mut().zip(page_styles) {
            append_margin_boxes(page, &style, total, frame.font_context(), &mut diagnostics);
        }
        diagnostics.dedup();
        Ok(PrintLayout {
            pages,
            diagnostics,
            fonts: frame.font_context().clone(),
        })
    }
}

fn first_forced(b: &LayoutBox) -> Option<Break> {
    let force = b.style().fragmentation.before();
    if force.forced(true) {
        return Some(force);
    }
    b.children()
        .iter()
        .find(|c| !c.style().position.out_of_flow())
        .and_then(first_forced)
}
fn first_name(b: &LayoutBox) -> Option<String> {
    b.style().fragmentation.page().or_else(|| {
        b.children()
            .iter()
            .find(|c| !c.style().position.out_of_flow())
            .and_then(first_name)
    })
}

fn page_display_list(
    tree: &LayoutTree,
    page: &style::paged::PageStyle,
    _environment: MediaEnvironment,
) -> Vec<DisplayItem> {
    let mut list = paint::build_display_list(tree, page.sheet.width, page.sheet.height, false);
    let mut decorations = Vec::new();
    if page.style.background.alpha > 0.0 {
        decorations.push(DisplayItem::ColorRect {
            x: 0.0,
            y: 0.0,
            w: page.sheet.width,
            h: page.sheet.height,
            color: page.style.background,
        });
    }
    let used = |side: Side, basis: f32| match page.style.margin.get(side) {
        LengthOrAuto::Auto => 0.0,
        LengthOrAuto::Length(v) => v.resolve(page.style.font_size, basis),
    };
    let margin = crate::css::values::Edges {
        top: used(Side::Top, page.sheet.height),
        bottom: used(Side::Bottom, page.sheet.height),
        left: used(Side::Left, page.sheet.width),
        right: used(Side::Right, page.sheet.width),
    };
    let border_rect = Rect {
        x: margin.left,
        y: margin.top,
        width: page.sheet.width - margin.left - margin.right,
        height: page.sheet.height - margin.top - margin.bottom,
    };
    for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
        let width = *page.style.border_width.get(side);
        if width <= 0.0 {
            continue;
        }
        let r = match side {
            Side::Top => Rect {
                height: width,
                ..border_rect
            },
            Side::Bottom => Rect {
                y: border_rect.y + border_rect.height - width,
                height: width,
                ..border_rect
            },
            Side::Left => Rect {
                width,
                ..border_rect
            },
            Side::Right => Rect {
                x: border_rect.x + border_rect.width - width,
                width,
                ..border_rect
            },
        };
        decorations.push(DisplayItem::ColorRect {
            x: r.x,
            y: r.y,
            w: r.width,
            h: r.height,
            color: *page.style.borders.get(side),
        });
    }
    list.splice(1..1, decorations);
    list
}

fn append_margin_boxes(
    page: &mut PrintedPage,
    style: &style::paged::PageStyle,
    total: usize,
    fonts: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    page.display_list.push(DisplayItem::CoordinateSpace(
        crate::layout::CoordinateSpace::Viewport,
    ));
    page.display_list.push(DisplayItem::Spatial(Box::default()));
    for (position, computed) in &style.margins {
        let Content::Items(items) = &computed.content else {
            continue;
        };
        let mut text = String::new();
        for item in items {
            match item {
                ContentItem::Text(value) => text.push_str(value),
                ContentItem::Counter { name, style, separator: None } if style == "decimal" && matches!(name.as_str(), "page" | "pages") => text.push_str(&if name == "page" { page.number } else { total }.to_string()),
                _ => diagnostics.push(Diagnostic { node_id: None, message: "page margin content supports strings and decimal counter(page)/counter(pages), not running elements or attributes".into() }),
            }
        }
        let glyphs = fonts.shape(computed, &text);
        let sheet = page.sheet;
        let area = page.area;
        let horizontal = position.starts_with("top-") || position.starts_with("bottom-");
        let (x, y, w, h) = if horizontal {
            let y = if position.starts_with("top-") {
                0.0
            } else {
                area.y + area.height
            };
            let h = if position.starts_with("top-") {
                area.y
            } else {
                sheet.height - y
            };
            let (x, w) = if position.ends_with("left-corner") {
                (0.0, area.x)
            } else if position.ends_with("right-corner") {
                (area.x + area.width, sheet.width - area.x - area.width)
            } else {
                (area.x, area.width)
            };
            (x, y, w, h)
        } else {
            let x = if position.starts_with("left-") {
                0.0
            } else {
                area.x + area.width
            };
            let w = if position.starts_with("left-") {
                area.x
            } else {
                sheet.width - x
            };
            (x, area.y, w, area.height)
        };
        let ox = if horizontal && position.contains("left") {
            0.0
        } else if horizontal && position.contains("right") {
            w - glyphs.width
        } else {
            (w - glyphs.width) * 0.5
        };
        let oh = glyphs.ascent + glyphs.descent;
        let oy = if !horizontal && position.ends_with("top") {
            0.0
        } else if !horizontal && position.ends_with("bottom") {
            h - oh
        } else {
            (h - oh) * 0.5
        };
        if computed.background.alpha > 0.0 {
            page.display_list.push(DisplayItem::ColorRect {
                x,
                y,
                w,
                h,
                color: computed.background,
            });
        }
        page.display_list.push(DisplayItem::ObjectClip(Rect {
            x,
            y,
            width: w,
            height: h,
        }));
        page.display_list.push(DisplayItem::ColorText {
            x: x + ox,
            y: y + oy,
            baseline: y + oy + glyphs.ascent,
            text: text.clone(),
            glyphs,
            size: computed.font_size,
            color: computed.text_fill_color,
            bold: computed.font_weight.0 >= 600.0,
            italic: computed.font_style != crate::css::values::FontStyle::Normal,
        });
        page.display_list.push(DisplayItem::ObjectClipEnd);
        page.margin_text.push(text);
    }
}
