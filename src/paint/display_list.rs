use std::sync::Arc;

use crate::css::values::{BorderStyle, Edges, ResolvedColor};
use crate::layout::{BoxKind, CoordinateSpace, LayoutBox, LayoutTree, PaintPhase, Rect};
use crate::media::DecodedImage;

pub(crate) const DEFAULT_VIEWPORT_WIDTH: f32 = crate::css::media::DEFAULT_VIEWPORT_WIDTH;
pub(crate) const DEFAULT_VIEWPORT_HEIGHT: f32 = crate::css::media::DEFAULT_VIEWPORT_HEIGHT;
pub(crate) const LINE_HEIGHT: f32 = 20.0;
const BACKGROUND_COLOR: u32 = 0x00FF_FFFF;
const OUTLINE_COLOR: u32 = 0x00FF_0000;

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayItem {
    Vector {
        rect: Rect,
        scene: Arc<crate::vector::Scene>,
    },
    BeginLayer(Box<super::compositor::Layer>),
    EndLayer,
    TextDecoration {
        rect: Rect,
        horizontal: bool,
        style: crate::style::typography::Keyword,
        color: ResolvedColor,
    },
    ObjectClip(Rect),
    ObjectClipEnd,
    /// Subsequent drawing commands use this coordinate space until changed.
    CoordinateSpace(CoordinateSpace),
    /// Complete state for the next flattened paint phase, never a DFS stack.
    Spatial(Box<crate::layout::SpatialState>),
    ColorRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: ResolvedColor,
    },
    ColorText {
        x: f32,
        /// Font metric top for inspection; rasterization uses the baseline.
        y: f32,
        baseline: f32,
        text: String,
        glyphs: Arc<crate::text::ShapedText>,
        size: f32,
        color: ResolvedColor,
        bold: bool,
        italic: bool,
    },
    ViewportBackground {
        w: f32,
        h: f32,
        color: u32,
    },
    Text {
        x: f32,
        y: f32,
        baseline: f32,
        text: String,
        glyphs: Arc<crate::text::ShapedText>,
        size: f32,
        color: u32,
        bold: bool,
        italic: bool,
    },
    Image {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        image: Arc<DecodedImage>,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: u32,
    },
    OutlineRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: u32,
    },
}

#[cfg(test)]
pub(crate) fn load_bundled_font() -> Result<crate::text::FontContext, Box<dyn std::error::Error>> {
    Ok(crate::text::FontContext::bundled())
}

pub fn build_display_list(
    layout: &LayoutTree,
    viewport_width: f32,
    viewport_height: f32,
    show_outlines: bool,
) -> Vec<DisplayItem> {
    let mut display_list = vec![DisplayItem::ViewportBackground {
        w: viewport_width,
        h: viewport_height,
        color: BACKGROUND_COLOR,
    }];
    // CSS2 14.2: the root background covers the canvas. For HTML, a
    // transparent root propagates the body's background instead.
    let canvas = layout.root_element().and_then(|root| {
        if root.style().background.alpha > 0.0 {
            Some(root)
        } else if root.tag_name() == "<html>" {
            root.children().iter().find(|b| b.tag_name() == "<body>")
        } else {
            None
        }
    });
    if let Some(canvas) = canvas.filter(|b| b.style().background.alpha > 0.0) {
        let area = layout.page_clip.unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_width,
            height: viewport_height,
        });
        display_list.push(DisplayItem::CoordinateSpace(CoordinateSpace::Viewport));
        display_list.push(rect_item(DisplayItem::ColorRect {
            x: area.x,
            y: area.y,
            w: area.width,
            h: area.height,
            color: canvas.style().background,
        }));
        display_list.push(DisplayItem::CoordinateSpace(CoordinateSpace::Document));
    }

    let (decoration, content) = layout.spatial_state_pair();
    let scrollbars = layout.scrollbars();
    let mut space = CoordinateSpace::Document;
    let mut spatial = crate::layout::SpatialState::default();
    let mut definitions = crate::hash::FastMap::default();
    fn collect<'a>(b: &'a LayoutBox, map: &mut crate::hash::FastMap<usize, &'a LayoutBox>) {
        map.entry(b.box_id).or_insert(b);
        for c in b.children() {
            collect(c, map);
        }
    }
    collect(layout.root(), &mut definitions);
    // Nearest grouped ancestor-or-self per box, so each paint item walks
    // only the grouped boxes above it.
    let mut nearest_group: crate::hash::FastMap<usize, Option<usize>> =
        crate::hash::FastMap::default();
    let mut group_of = |id: usize| -> Option<usize> {
        let mut chain = Vec::new();
        let mut candidate = Some(id);
        let mut found = None;
        while let Some(current) = candidate {
            if let Some(&known) = nearest_group.get(&current) {
                found = known;
                break;
            }
            let Some(b) = definitions.get(&current) else {
                break;
            };
            chain.push(current);
            if b.style().effects.grouped() {
                found = Some(current);
                break;
            }
            candidate = b.semantic_parent();
        }
        for visited in chain {
            nearest_group.insert(visited, found);
        }
        found
    };
    let mut groups: Vec<usize> = Vec::new();
    for (layout_box, phase) in crate::layout::paint_order(layout) {
        let mut path = Vec::new();
        let mut candidate = group_of(layout_box.box_id);
        while let Some(id) = candidate {
            path.push(id);
            candidate = definitions[&id].semantic_parent().and_then(&mut group_of);
        }
        path.reverse();
        let common = groups.iter().zip(&path).take_while(|(a, b)| a == b).count();
        for _ in common..groups.len() {
            display_list.push(DisplayItem::EndLayer);
        }
        for id in &path[common..] {
            let b = definitions[id];
            display_list.push(DisplayItem::BeginLayer(Box::new(
                super::compositor::Layer {
                    effects: b.style().effects.clone(),
                    rect: b.rect(),
                    content: b.content_rect(),
                    size: b.style().font_size,
                    padding: Rect {
                        x: b.rect().x + b.style().border_width.left,
                        y: b.rect().y + b.style().border_width.top,
                        width: (b.rect().width
                            - b.style().border_width.left
                            - b.style().border_width.right)
                            .max(0.0),
                        height: (b.rect().height
                            - b.style().border_width.top
                            - b.style().border_width.bottom)
                            .max(0.0),
                    },
                    color: b.style().text_color,
                    spatial: decoration[id].clone(),
                    space: b.coordinate_space(),
                    images: layout.effect_images.clone(),
                },
            )));
        }
        groups = path;
        if layout_box.coordinate_space() != space {
            space = layout_box.coordinate_space();
            display_list.push(DisplayItem::CoordinateSpace(space));
        }
        let next = if matches!(phase, PaintPhase::Content | PaintPhase::Text(_)) {
            &content[&layout_box.box_id]
        } else {
            &decoration[&layout_box.box_id]
        };
        if spatial != *next {
            spatial = next.clone();
            display_list.push(DisplayItem::Spatial(Box::new(spatial.clone())));
        }
        if phase != PaintPhase::Scrollbars {
            append_box_items(
                layout_box,
                phase,
                canvas.is_none_or(|b| !std::ptr::eq(b, layout_box)),
                &mut display_list,
            );
        }
        if phase == PaintPhase::Scrollbars {
            for bar in scrollbars
                .iter()
                .filter(|bar| bar.owner == layout_box.box_id && !bar.viewport)
            {
                display_list.push(DisplayItem::Spatial(Box::new(bar.spatial.clone())));
                spatial = bar.spatial.clone();
                for (rect, color) in [(bar.track, bar.track_color), (bar.thumb, bar.thumb_color)] {
                    display_list.push(rect_item(DisplayItem::ColorRect {
                        x: rect.x,
                        y: rect.y,
                        w: rect.width,
                        h: rect.height,
                        color,
                    }));
                }
            }
        }
    }
    for _ in groups {
        display_list.push(DisplayItem::EndLayer);
    }
    for bar in scrollbars.iter().filter(|b| b.viewport) {
        display_list.push(DisplayItem::CoordinateSpace(CoordinateSpace::Viewport));
        display_list.push(DisplayItem::Spatial(Box::new(bar.spatial.clone())));
        for (r, color) in [(bar.track, bar.track_color), (bar.thumb, bar.thumb_color)] {
            display_list.push(DisplayItem::ColorRect {
                x: r.x,
                y: r.y,
                w: r.width,
                h: r.height,
                color,
            });
        }
    }
    if show_outlines {
        display_list.push(DisplayItem::CoordinateSpace(CoordinateSpace::Document));
        fn outlines(
            b: &LayoutBox,
            decoration: &crate::layout::SpatialStates,
            content: &crate::layout::SpatialStates,
            list: &mut Vec<DisplayItem>,
        ) {
            list.push(DisplayItem::CoordinateSpace(b.coordinate_space()));
            list.push(DisplayItem::Spatial(Box::new(
                if b.kind() == BoxKind::Text {
                    content[&b.box_id].clone()
                } else {
                    decoration[&b.box_id].clone()
                },
            )));
            append_own_outline(b, &content[&b.box_id], list);
            for child in b.children() {
                outlines(child, decoration, content, list);
            }
        }
        outlines(layout.root(), &decoration, &content, &mut display_list);
    }
    display_list
}

fn append_box_items(
    layout_box: &LayoutBox,
    phase: PaintPhase,
    paint_background: bool,
    display_list: &mut Vec<DisplayItem>,
) {
    if phase == PaintPhase::TableBorders {
        for segment in layout_box.table_border_segments() {
            let r = segment.rect;
            append_border(
                display_list,
                (r.x, r.y, r.width, r.height),
                segment.color,
                segment.style,
                r.width >= r.height,
            );
        }
        return;
    }
    if matches!(
        phase,
        PaintPhase::Decoration | PaintPhase::InlineDecoration(_)
    ) {
        let fragment = if let PaintPhase::InlineDecoration(index) = phase {
            Some(index)
        } else {
            None
        };
        append_decoration_items(layout_box, paint_background, fragment, display_list);
        if phase == PaintPhase::Decoration {
            for segment in layout_box.column_rules() {
                let r = segment.rect;
                append_border(
                    display_list,
                    (r.x, r.y, r.width, r.height),
                    segment.color,
                    segment.style,
                    r.width >= r.height,
                );
            }
        }
        return;
    }
    if phase == PaintPhase::Content
        && let Some(scene) = layout_box.vector_scene()
    {
        let rect = layout_box.object_rect();
        if rect.width > 0.0 && rect.height > 0.0 {
            display_list.push(DisplayItem::ObjectClip(layout_box.content_rect()));
            display_list.push(DisplayItem::Vector {
                rect,
                scene: scene.clone(),
            });
            display_list.push(DisplayItem::ObjectClipEnd);
        }
    } else if phase == PaintPhase::Content
        && let Some(image) = layout_box.decoded_image()
    {
        let rect = layout_box.object_rect();
        if rect.width > 0.0 && rect.height > 0.0 {
            display_list.push(DisplayItem::ObjectClip(layout_box.content_rect()));
            display_list.push(DisplayItem::Image {
                x: rect.x,
                y: rect.y,
                w: rect.width,
                h: rect.height,
                image: Arc::clone(image),
            });
            display_list.push(DisplayItem::ObjectClipEnd);
        }
    }
    if phase == PaintPhase::Content
        && let Some(marker) = layout_box.list_marker()
        && marker.style.text_fill_color.alpha > 0.0
        && marker.style.font_size > 0.0
    {
        if let Some(image) = &marker.image {
            if let Some(scene) = image.vector() {
                display_list.push(DisplayItem::Vector {
                    rect: Rect {
                        x: marker.x,
                        y: marker.y,
                        width: image.width() as f32,
                        height: image.height() as f32,
                    },
                    scene: scene.clone(),
                });
            } else {
                display_list.push(DisplayItem::Image {
                    x: marker.x,
                    y: marker.y,
                    w: image.width() as f32,
                    h: image.height() as f32,
                    image: image.clone(),
                });
            }
        }
        if marker.image.is_none() {
            display_list.push(text_item(DisplayItem::ColorText {
                x: marker.x,
                y: marker.y,
                baseline: marker.baseline,
                text: marker.text.clone(),
                glyphs: marker.glyphs.clone(),
                size: marker.style.font_size,
                color: marker.style.text_fill_color,
                bold: marker.style.bold,
                italic: marker.style.italic,
            }));
        }
    }

    let fragments = if let PaintPhase::Text(index) = phase {
        &layout_box.text_fragments()[index..=index]
    } else {
        layout_box.text_fragments()
    };
    for fragment in fragments {
        super::decoration::append(layout_box, fragment, display_list);
        if fragment.text_color.alpha == 0.0 {
            continue;
        }
        if fragment.font_size == 0.0 {
            continue;
        }
        display_list.push(text_item(DisplayItem::ColorText {
            x: fragment.x,
            y: fragment.y,
            baseline: fragment.baseline,
            text: fragment.text.clone(),
            glyphs: fragment.glyphs.clone(),
            size: fragment.font_size,
            color: fragment.text_color,
            bold: fragment.bold,
            italic: fragment.italic,
        }));
    }

    fn append_decoration_items(
        layout_box: &LayoutBox,
        paint_background: bool,
        selected: Option<usize>,
        display_list: &mut Vec<DisplayItem>,
    ) {
        if layout_box.hides_empty_cell() {
            return;
        }
        if layout_box.kind() == BoxKind::Inline
            && !layout_box.is_replaced_image()
            && !layout_box.style().display.is_flow_root()
            && !layout_box.style().display.is_item_container()
            && !layout_box.style().display.is_table()
        {
            for (index, fragment) in layout_box.inline_fragments().iter().enumerate() {
                if selected.is_some_and(|selected| selected != index) {
                    continue;
                }
                let mut widths = layout_box.style().border_width;
                if !fragment.first {
                    widths.left = 0.0;
                }
                if !fragment.last {
                    widths.right = 0.0;
                }
                append_decoration_rect(
                    layout_box,
                    fragment.rect,
                    widths,
                    paint_background,
                    display_list,
                );
            }
        } else {
            append_decoration_rect(
                layout_box,
                layout_box.table_paint_rect().unwrap_or(layout_box.rect()),
                if layout_box.suppresses_table_borders() {
                    Edges::default()
                } else {
                    layout_box.inline_edges(layout_box.style().border_width)
                },
                paint_background,
                display_list,
            );
        }
    }

    fn append_decoration_rect(
        layout_box: &LayoutBox,
        rect: Rect,
        widths: Edges<f32>,
        paint_background: bool,
        display_list: &mut Vec<DisplayItem>,
    ) {
        let style = layout_box.style();
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        if paint_background && style.background.alpha > 0.0 {
            display_list.push(rect_item(DisplayItem::ColorRect {
                x: rect.x,
                y: rect.y,
                w: rect.width,
                h: rect.height,
                color: style.background,
            }));
        }
        let colors = style.borders;
        let styles = style.border_style;
        let inner_height = (rect.height - widths.top - widths.bottom).max(0.0);
        for (x, y, w, h, color, border_style, horizontal) in [
            (
                rect.x, rect.y, rect.width, widths.top, colors.top, styles.top, true,
            ),
            (
                rect.x,
                rect.y + rect.height - widths.bottom,
                rect.width,
                widths.bottom,
                colors.bottom,
                styles.bottom,
                true,
            ),
            (
                rect.x,
                rect.y + widths.top,
                widths.left,
                inner_height,
                colors.left,
                styles.left,
                false,
            ),
            (
                rect.x + rect.width - widths.right,
                rect.y + widths.top,
                widths.right,
                inner_height,
                colors.right,
                styles.right,
                false,
            ),
        ] {
            if w > 0.0 && h > 0.0 && color.alpha > 0.0 {
                append_border(display_list, (x, y, w, h), color, border_style, horizontal);
            }
        }
    }
}

fn append_border(
    list: &mut Vec<DisplayItem>,
    (x, y, w, h): (f32, f32, f32, f32),
    color: ResolvedColor,
    style: BorderStyle,
    horizontal: bool,
) {
    let length = if horizontal { w } else { h };
    let thickness = if horizontal { h } else { w };
    let mut rect = |offset: f32, across: f32, length: f32, thickness: f32| {
        let (x, y, w, h) = if horizontal {
            (x + offset, y + across, length, thickness)
        } else {
            (x + across, y + offset, thickness, length)
        };
        list.push(rect_item(DisplayItem::ColorRect { x, y, w, h, color }));
    };
    match style {
        BorderStyle::None | BorderStyle::Hidden => {}
        BorderStyle::Solid => rect(0.0, 0.0, length, thickness),
        BorderStyle::Double if thickness >= 3.0 => {
            rect(0.0, 0.0, length, thickness / 3.0);
            rect(0.0, thickness * 2.0 / 3.0, length, thickness / 3.0);
        }
        BorderStyle::Double => rect(0.0, 0.0, length, thickness),
        BorderStyle::Dashed | BorderStyle::Dotted => {
            let dash = thickness
                * if style == BorderStyle::Dashed {
                    3.0
                } else {
                    1.0
                };
            // Bound commands even for enormous author-provided box dimensions.
            let stride = (dash * 2.0).max(length / 4096.0);
            for index in 0..4096 {
                let offset = index as f32 * stride;
                if offset >= length {
                    break;
                }
                rect(offset, 0.0, dash.min(length - offset), thickness);
            }
        }
    }
}

// Preserve the existing display-list/debug API for exact opaque sRGB bytes.
// Every other color stays typed until the rasterizer's framebuffer boundary.
fn rect_item(item: DisplayItem) -> DisplayItem {
    if let DisplayItem::ColorRect { x, y, w, h, color } = item
        && let Some(color) = color.opaque_rgb()
    {
        return DisplayItem::Rect { x, y, w, h, color };
    }
    item
}

fn text_item(item: DisplayItem) -> DisplayItem {
    match item {
        DisplayItem::ColorText {
            x,
            y,
            baseline,
            text,
            glyphs,
            size,
            color,
            bold,
            italic,
        } => {
            if let Some(rgb) = color.opaque_rgb() {
                DisplayItem::Text {
                    x,
                    y,
                    baseline,
                    text,
                    glyphs,
                    size,
                    color: rgb,
                    bold,
                    italic,
                }
            } else {
                DisplayItem::ColorText {
                    x,
                    y,
                    baseline,
                    text,
                    glyphs,
                    size,
                    color,
                    bold,
                    italic,
                }
            }
        }
        other => other,
    }
}

fn append_own_outline(
    layout_box: &LayoutBox,
    content: &crate::layout::SpatialState,
    display_list: &mut Vec<DisplayItem>,
) {
    let mut outline = |rect: Rect, color| {
        if rect.width > 0.0 && rect.height > 0.0 {
            display_list.push(DisplayItem::OutlineRect {
                x: rect.x,
                y: rect.y,
                w: rect.width,
                h: rect.height,
                color,
            });
        }
    };
    if layout_box.kind() == BoxKind::Text {
        for fragment in layout_box.text_fragments() {
            outline(fragment.ink_rect, OUTLINE_COLOR);
        }
    } else if layout_box.kind() == BoxKind::Inline
        && !layout_box.is_replaced_image()
        && !layout_box.style().display.is_item_container()
    {
        for fragment in layout_box.inline_fragments() {
            outline(fragment.rect, OUTLINE_COLOR);
        }
    } else {
        outline(layout_box.rect(), OUTLINE_COLOR);
    }
    display_list.push(DisplayItem::Spatial(Box::new(content.clone())));
    for line in layout_box.line_boxes() {
        let rect = line.rect;
        if rect.width > 0.0 && rect.height > 0.0 {
            display_list.push(DisplayItem::OutlineRect {
                x: rect.x,
                y: rect.y,
                w: rect.width,
                h: rect.height,
                color: 0x008080,
            });
        }
        display_list.push(DisplayItem::Rect {
            x: line.rect.x,
            y: line.baseline,
            w: line.rect.width,
            h: 1.0,
            color: 0x0000ff,
        });
    }
}

pub(crate) fn max_scroll_offset(layout: &LayoutTree, viewport_height: f32) -> f32 {
    if viewport_height.is_finite()
        && viewport_height > 0.0
        && layout
            .viewport_scroll_container()
            .overflow_y
            .scroll_container()
    {
        (layout.document_bottom() - viewport_height).max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{html::parser, layout};

    fn layout(source: &str, width: f32) -> LayoutTree {
        let document = parser::parse(source).unwrap();
        let mut tree = layout::build_layout_tree(&document);
        tree.layout(width, &load_bundled_font().unwrap());
        tree
    }

    fn text_items(display_list: &[DisplayItem]) -> Vec<&DisplayItem> {
        display_list
            .iter()
            .filter(|item| matches!(item, DisplayItem::Text { .. }))
            .collect()
    }

    #[test]
    fn border_styles_draw_supported_patterns_and_none_hidden_do_not_paint() {
        for (style, expected) in [
            ("solid", 1),
            ("double", 2),
            ("dashed", 4),
            ("dotted", 10),
            ("none", 0),
            ("hidden", 0),
        ] {
            let tree = layout(
                &format!(
                    "<div style='width:60px; border-top:3px {style} red;
                font:16px/20px serif'>text</div>"
                ),
                400.0,
            );
            let items = build_display_list(&tree, 400.0, 600.0, false);
            let red = items
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        DisplayItem::Rect {
                            color: 0xff0000,
                            ..
                        }
                    )
                })
                .count();
            assert_eq!(red, expected, "{style}: {items:?}");
        }
        let mut items = Vec::new();
        append_border(
            &mut items,
            (0.0, 0.0, 1e30, 1.0),
            ResolvedColor::BLACK,
            BorderStyle::Dotted,
            true,
        );
        assert!(items.len() <= 4096);
    }

    #[test]
    fn loaded_images_and_failed_image_alt_text_reach_paint_and_hidden_images_do_not() {
        use std::{io::Cursor, sync::Arc};
        let source = image::ImageBuffer::from_pixel(2, 1, image::Rgba([255u8, 0, 0, 255]));
        let mut bytes = Cursor::new(Vec::new());
        source
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let image = Arc::new(DecodedImage::decode(&bytes.into_inner()).unwrap());
        let document = parser::parse("<img id='loaded' width='10' style='padding:2px; border-width:1px; border-style:solid; background-color:blue'>\
            <img alt='Unavailable'><img id='hidden' style='display:none'>").unwrap();
        let mut images = crate::media::ImageMap::new();
        for id in document.preorder() {
            if document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|element| element.attribute("id").is_some())
            {
                images.insert(id, Arc::clone(&image));
            }
        }
        let resolution = crate::style::resolve(&document);
        let mut tree =
            layout::build_layout_tree_with_resources(&document, &resolution.styles, &images);
        tree.layout(400.0, &load_bundled_font().unwrap());
        let list = build_display_list(&tree, 400.0, 600.0, false);
        assert_eq!(
            list.iter()
                .filter(|item| matches!(item, DisplayItem::Image { .. }))
                .count(),
            1
        );
        assert!(list.iter().any(|item| matches!(
            item,
            DisplayItem::Image {
                w: 10.0,
                h: 5.0,
                ..
            }
        )));
        assert!(
            list.iter().any(
                |item| matches!(item, DisplayItem::Text { text, .. } if text == "Unavailable")
            )
        );
        let decoration = list
            .iter()
            .position(|item| {
                matches!(
                    item,
                    DisplayItem::Rect {
                        color: 0x0000FF,
                        ..
                    }
                )
            })
            .unwrap();
        let bitmap = list
            .iter()
            .position(|item| matches!(item, DisplayItem::Image { .. }))
            .unwrap();
        assert!(decoration < bitmap);
    }

    #[test]
    fn display_list_uses_layout_fragments_and_skips_non_rendered_elements() {
        let tree = layout(
            "<head><title>hidden title</title></head><p>first <em>nested</em></p>\
             <script>hidden script</script><style>hidden style</style><p>last</p>",
            800.0,
        );
        let display_list = build_display_list(&tree, 800.0, 600.0, false);
        let rendered_text = text_items(&display_list)
            .into_iter()
            .map(|item| match item {
                DisplayItem::Text { text, .. } => text.as_str(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered_text, ["first", "nested", "last"]);
        assert!(
            !display_list
                .iter()
                .any(|item| matches!(item, DisplayItem::OutlineRect { .. }))
        );
    }

    #[test]
    fn narrower_viewports_wrap_inline_fragments() {
        let source = "<p>one two three four five six</p>";
        let narrow = layout(source, 180.0);
        let wide = layout(source, 800.0);
        let narrow_list = build_display_list(&narrow, 180.0, 600.0, false);
        let wide_list = build_display_list(&wide, 800.0, 600.0, false);
        let line_count = |items: &[DisplayItem]| {
            let mut lines = text_items(items)
                .into_iter()
                .filter_map(|item| match item {
                    DisplayItem::Text { y, .. } => Some(*y),
                    _ => None,
                })
                .collect::<Vec<_>>();
            lines.sort_by(f32::total_cmp);
            lines.dedup_by(|left, right| (*left - *right).abs() < 0.01);
            lines.len()
        };

        assert!(line_count(&narrow_list) > line_count(&wide_list));
    }

    #[test]
    fn list_markers_and_inline_emphasis_reach_the_display_list() {
        let tree = layout(
            "<ul><li><strong>bold</strong> <em>italic</em></li></ul>",
            400.0,
        );
        let display_list = build_display_list(&tree, 400.0, 600.0, true);
        let texts = text_items(&display_list);

        assert!(texts.iter().any(|item| matches!(
            item,
            DisplayItem::Text { text, .. } if text == "\u{2022}"
        )));
        assert!(texts.iter().any(|item| matches!(
            item,
            DisplayItem::Text { text, bold: true, .. } if text == "bold"
        )));
        assert!(texts.iter().any(|item| matches!(
            item,
            DisplayItem::Text { text, italic: true, .. } if text == "italic"
        )));
        assert!(
            display_list
                .iter()
                .any(|item| matches!(item, DisplayItem::OutlineRect { .. }))
        );
    }

    #[test]
    fn max_scroll_offset_tracks_layout_content_height() {
        let short = layout("<p>short</p>", 800.0);
        let long = layout(&format!("<p>{}</p>", "word ".repeat(1000)), 800.0);

        assert_eq!(max_scroll_offset(&short, 600.0), 0.0);
        assert!(max_scroll_offset(&long, 600.0) > 0.0);
        assert_eq!(max_scroll_offset(&long, 0.0), 0.0);
        assert_eq!(max_scroll_offset(&long, f32::NAN), 0.0);
    }

    #[test]
    fn paints_parent_background_and_each_border_before_child_content() {
        let tree = layout(
            "<div style='background-color:#123; border-width:1px 2px 3px 4px; border-style:solid;\
             border-color:red green blue yellow; padding:5px'>\
             <span style='color:rebeccapurple; background-color:white'>text</span></div>",
            400.0,
        );
        let list = build_display_list(&tree, 400.0, 600.0, false);
        assert!(matches!(list[0], DisplayItem::ViewportBackground { .. }));
        assert!(
            matches!(
                list[1],
                DisplayItem::Rect {
                    color: 0x112233,
                    ..
                }
            ),
            "{list:?}"
        );
        assert!(matches!(
            list[2],
            DisplayItem::Rect {
                h: 1.0,
                color: 0xFF0000,
                ..
            }
        ));
        assert!(matches!(
            list[3],
            DisplayItem::Rect {
                h: 3.0,
                color: 0x0000FF,
                ..
            }
        ));
        assert!(matches!(
            list[4],
            DisplayItem::Rect {
                w: 4.0,
                color: 0xFFFF00,
                ..
            }
        ));
        assert!(matches!(
            list[5],
            DisplayItem::Rect {
                w: 2.0,
                color: 0x008000,
                ..
            }
        ));
        assert!(matches!(
            list[6],
            DisplayItem::Rect {
                color: 0xFFFFFF,
                ..
            }
        ));
        assert!(
            matches!(&list[7], DisplayItem::Text { color: 0x663399, text, .. } if text == "text")
        );
    }

    #[test]
    fn list_markers_inherit_size_color_and_emphasis() {
        let tree = layout(
            "<ul style='color:red; font-size:24px; font-weight:bold'><li>item</li></ul>",
            400.0,
        );
        let list = build_display_list(&tree, 400.0, 600.0, false);
        assert!(list.iter().any(|item| matches!(item,
            DisplayItem::Text { text, size:24.0, color:0xFF0000, bold:true, .. } if text == "\u{2022}"
        )));
    }

    #[test]
    fn transparent_and_zero_size_text_do_not_emit_glyphs() {
        let tree = layout(
            "<p style='color:transparent'>hidden</p>\
             <p style='font-size:0'>zero</p><p>visible</p>",
            400.0,
        );
        let list = build_display_list(&tree, 400.0, 600.0, false);
        let text = text_items(&list);
        assert_eq!(text.len(), 1);
        assert!(matches!(text[0], DisplayItem::Text { text, .. } if text == "visible"));
    }
}
