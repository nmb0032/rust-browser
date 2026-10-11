use super::*;
use crate::{
    dom::{Document, NodeId},
    html::parser,
    paint::{self, DisplayItem, load_bundled_font},
    style,
};

fn layout(css: &str, contents: &str) -> (Document, LayoutTree, crate::text::FontContext) {
    let document = parser::parse(&format!(
        "<!doctype html><style>body{{margin:0}}#host{{width:160px;font:16px/20px sans-serif}}{css}</style>\
         <div id='host'>{contents}</div>"
    )).unwrap();
    let styles = style::resolve(&document);
    assert!(
        styles.stylesheets.diagnostics.is_empty(),
        "{:?}",
        styles.stylesheets.diagnostics
    );
    let font = load_bundled_font().unwrap();
    let mut tree = build_layout_tree_with_styles(&document, &styles.styles);
    tree.layout(448.0, &font);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    (document, tree, font)
}

fn id(document: &Document, name: &str) -> NodeId {
    document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|element| element.attribute("id") == Some(name))
        })
        .unwrap()
}

fn box_for<'a>(document: &Document, tree: &'a LayoutTree, name: &str) -> &'a LayoutBox {
    fn find(root: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
        if root.node_id == Some(node) && root.pseudo.is_none() {
            Some(root)
        } else {
            root.children.iter().find_map(|child| find(child, node))
        }
    }
    find(tree.root(), id(document, name)).unwrap()
}

fn texts(root: &LayoutBox) -> Vec<&TextFragment> {
    let mut result: Vec<_> = root.text_fragments.iter().collect();
    for child in &root.children {
        result.extend(texts(child));
    }
    result
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

fn metrics(font: &fontdue::Font, size: f32, height: f32) -> (f32, f32) {
    let m = font.horizontal_line_metrics(size).unwrap();
    let leading = (height as f64 - m.ascent as f64 + m.descent as f64) / 2.0;
    (
        (m.ascent as f64 + leading) as f32,
        (-m.descent as f64 + leading) as f32,
    )
}

#[test]
fn nested_mixed_sizes_use_face_metrics_half_leading_and_one_baseline() {
    let (document, tree, font) = layout(
        "#large{font:32px/40px sans-serif}#small{font:10px/10px sans-serif}",
        "H<span id='large'>H<span id='small'>H</span></span>",
    );
    let host = box_for(&document, &tree, "host");
    let line = &host.line_boxes()[0];
    let pairs = [
        metrics(&font, 16.0, 20.0),
        metrics(&font, 32.0, 40.0),
        metrics(&font, 10.0, 10.0),
    ];
    let above = pairs
        .iter()
        .map(|pair| pair.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let below = pairs
        .iter()
        .map(|pair| pair.1)
        .fold(f32::NEG_INFINITY, f32::max);
    close(line.ascent, above);
    close(line.descent, below);
    close(line.baseline, host.content_rect().y + above);
    close(line.rect.height, above + below);
    for text in texts(host) {
        assert_eq!(text.baseline, line.baseline);
        close(
            text.y,
            line.baseline - font.horizontal_line_metrics(text.font_size).unwrap().ascent,
        );
        assert!(text.ink_rect.y >= text.y - 1.0);
    }
    for name in ["large", "small"] {
        assert_eq!(
            box_for(&document, &tree, name).inline_fragments()[0].baseline,
            line.baseline
        );
    }
}

#[test]
fn parent_strut_and_inline_ancestor_leading_participate_even_with_small_child() {
    let (document, tree, font) = layout(
        "#outer{font:30px/60px sans-serif}#inner{font:8px/8px sans-serif}",
        "<span id='outer'><span id='inner'>x</span></span>",
    );
    let host = box_for(&document, &tree, "host");
    let outer = metrics(&font, 30.0, 60.0);
    close(host.line_boxes()[0].ascent, outer.0);
    close(host.line_boxes()[0].descent, outer.1);
    assert_eq!(host.content_rect().height, 60.0);
    let (document, tree, font) = layout(
        "#host{line-height:80px}",
        "<span style='font:8px/8px sans-serif'>x</span>",
    );
    let host = box_for(&document, &tree, "host");
    close(
        host.line_boxes()[0].baseline,
        host.content_rect().y + metrics(&font, 16.0, 80.0).0,
    );
    assert_eq!(host.content_rect().height, 80.0);
}

#[test]
fn normal_uses_bundled_horizontal_line_metrics() {
    let (document, tree, font) = layout("#host{line-height:normal}", "H");
    let host = box_for(&document, &tree, "host");
    let m = font.horizontal_line_metrics(16.0).unwrap();
    close(host.content_rect().height, m.new_line_size);
    close(
        host.line_boxes()[0].baseline,
        host.content_rect().y + m.ascent + m.line_gap / 2.0,
    );
}

#[test]
fn baseline_contract_reaches_display_and_scaled_scrolled_raster_pixels() {
    let (document, tree, font) = layout(
        "#large{font:32px/40px sans-serif;color:red}",
        "H <span id='large'>H</span>",
    );
    let host = box_for(&document, &tree, "host");
    let list = paint::build_display_list(&tree, 448.0, 160.0, false);
    let text_list: Vec<_> = list
        .iter()
        .filter(|item| matches!(item, DisplayItem::Text { .. }))
        .cloned()
        .collect();
    assert_eq!(text_list.len(), 2);
    for (item, fragment) in text_list.iter().zip(texts(host)) {
        assert!(
            matches!(item, DisplayItem::Text {baseline,y,..} if *baseline == fragment.baseline && *y == fragment.y)
        );
    }
    for (scale, scroll) in [(1.0, 0.0), (1.5, 8.25), (2.0, 12.0)] {
        let mut pixels = vec![0xffffff; 672 * 240];
        paint::rasterize_display_list(&text_list, &font, &mut pixels, 672, 240, scale, scroll);
        for item in &text_list {
            let DisplayItem::Text {
                x,
                baseline,
                size,
                color,
                ..
            } = item
            else {
                unreachable!()
            };
            let (m, bitmap) = font.rasterize('H', *size * scale as f32);
            let left = (*x * scale as f32).round() as i32 + m.xmin;
            let top =
                ((*baseline - scroll) * scale as f32).round() as i32 - m.ymin - m.height as i32;
            for gy in 0..m.height {
                for gx in 0..m.width {
                    let coverage = bitmap[gy * m.width + gx] as u32;
                    let expected_channel = |shift: u32| {
                        (((*color >> shift) & 255u32) * coverage + 255 * (255 - coverage)) / 255
                    };
                    let expected =
                        expected_channel(16) << 16 | expected_channel(8) << 8 | expected_channel(0);
                    assert_eq!(
                        pixels[(top as usize + gy) * 672 + left as usize + gx],
                        expected
                    );
                }
            }
        }
        // Inspection-only y must not influence rasterization.
        let altered: Vec<_> = text_list
            .iter()
            .cloned()
            .map(|mut item| {
                if let DisplayItem::Text { y, .. } = &mut item {
                    *y = -1e9;
                }
                item
            })
            .collect();
        let mut comparison = vec![0xffffff; 672 * 240];
        paint::rasterize_display_list(&altered, &font, &mut comparison, 672, 240, scale, scroll);
        assert_eq!(pixels, comparison);
    }
}

#[test]
fn physical_and_logical_alignment_inherit_and_ignore_child_direction() {
    for (align, direction, factor) in [
        ("left", "ltr", 0.0),
        ("center", "ltr", 0.5),
        ("right", "ltr", 1.0),
        ("start", "ltr", 0.0),
        ("end", "ltr", 1.0),
        ("start", "rtl", 1.0),
        ("end", "rtl", 0.0),
        ("left", "rtl", 0.0),
        ("center", "rtl", 0.5),
        ("right", "rtl", 1.0),
    ] {
        let (document, tree, font) = layout(
            &format!("#host{{direction:{direction};text-align:{align}}}"),
            "<span id='span' style='direction:ltr'>H<img id='image' style='width:20px;height:10px'></span>",
        );
        let host = box_for(&document, &tree, "host");
        let width = font.metrics('H', 16.0).advance_width + 20.0;
        let shift = (160.0 - width) * factor;
        close(
            texts(host)[0].x,
            host.content_rect().x + shift + if direction == "rtl" { 20.0 } else { 0.0 },
        );
        close(
            box_for(&document, &tree, "span").inline_fragments()[0]
                .rect
                .x,
            host.content_rect().x + shift,
        );
        close(
            box_for(&document, &tree, "image").rect.x,
            host.content_rect().x
                + shift
                + if direction == "rtl" {
                    0.0
                } else {
                    width - 20.0
                },
        );
        assert_eq!(
            box_for(&document, &tree, "span").style.text_align,
            host.style.text_align
        );
    }
}

#[test]
fn final_br_and_wrapped_lines_align_without_trailing_space_advance() {
    for (align, factor) in [("left", 0.0), ("center", 0.5), ("right", 1.0)] {
        let (document, tree, font) = layout(
            &format!("#host{{width:80px;text-align:{align}}}"),
            "<span id='span'>Hello Hello<br>H   </span>",
        );
        let host = box_for(&document, &tree, "host");
        assert_eq!(host.line_boxes().len(), 3);
        let fragments = texts(host);
        for (index, fragment) in fragments.iter().enumerate() {
            let width = super::inline::measure_text(&font, &fragment.text, 16.0);
            close(host.line_boxes()[index].content_rect.width, width);
            close(fragment.x, host.content_rect().x + (80.0 - width) * factor);
            close(fragment.baseline, host.line_boxes()[index].baseline);
        }
    }
}

#[test]
fn negative_margins_and_overflow_use_signed_alignment_free_space() {
    for (align, factor) in [("left", 0.0), ("center", 0.5), ("right", 1.0)] {
        for margin in [-20.0, 10.0] {
            let (document, tree, font) = layout(
                &format!(
                    "#host{{width:20px;text-align:{align}}}#span{{margin-left:{margin}px;margin-right:-2px;padding:0 3px;border:1px solid}}"
                ),
                "<span id='span'>Hello</span>",
            );
            let host = box_for(&document, &tree, "host");
            let word = super::inline::measure_text(&font, "Hello", 16.0);
            let advance = margin + 4.0 + word + 4.0 - 2.0;
            let shift = (20.0 - advance) * factor;
            close(
                texts(host)[0].x,
                host.content_rect().x + shift + margin + 4.0,
            );
            close(
                box_for(&document, &tree, "span").inline_fragments()[0]
                    .rect
                    .x,
                host.content_rect().x + shift + margin,
            );
            assert_eq!(host.line_boxes().len(), 1);
        }
    }
}

#[test]
fn breaks_terminate_actual_lines_and_never_manufacture_a_trailing_strut() {
    for (contents, lines) in [
        ("", 0),
        (" \n<span></span> ", 0),
        ("H", 1),
        ("<br>", 1),
        ("H<br>", 1),
        ("<br><br>", 2),
        ("H<br><br>", 2),
        ("<br>H", 2),
        ("H<br>H<br>", 2),
        ("H<br><span></span> ", 1),
    ] {
        let (document, tree, _) = layout("", contents);
        let host = box_for(&document, &tree, "host");
        assert_eq!(host.line_boxes().len(), lines, "{contents}");
        assert_eq!(
            host.content_rect().height,
            lines as f32 * 20.0,
            "{contents}"
        );
    }
}

#[test]
fn small_and_zero_line_heights_keep_signed_metrics_visible_glyph_overflow_and_real_lines() {
    for height in [0.0, 2.0, 8.0] {
        let (document, tree, font) = layout(&format!("#host{{line-height:{height}px}}"), "H<br>H");
        let host = box_for(&document, &tree, "host");
        let pair = metrics(&font, 16.0, height);
        assert_eq!(host.line_boxes().len(), 2);
        close(host.content_rect().height, height * 2.0);
        for (index, line) in host.line_boxes().iter().enumerate() {
            close(line.ascent, pair.0);
            close(line.descent, pair.1);
            close(
                line.baseline,
                host.content_rect().y + height * index as f32 + pair.0,
            );
            close(line.rect.height, height);
        }
        let fragment = texts(host)[0];
        assert!(fragment.height > height);
        assert!(fragment.ink_rect.height > 0.0);
        assert!(
            tree.hit_test(fragment.ink_rect.x + 1.0, fragment.ink_rect.y + 1.0)
                .is_some()
        );
    }
}

#[test]
fn wrapped_nested_inline_fragments_slice_only_first_last_horizontal_edges() {
    let (document, tree, font) = layout(
        "#host{width:85px}#outer{padding:8px 4px;border:2px solid green;background:yellow}\
         #inner{padding:12px 3px;border:1px solid red;background:blue}",
        "<span id='outer'><span id='inner'>Hello Hello Hello</span></span>",
    );
    let host = box_for(&document, &tree, "host");
    assert_eq!(host.line_boxes().len(), 3);
    assert_eq!(host.content_rect().height, 60.0);
    let word = super::inline::measure_text(&font, "Hello", 16.0);
    for (name, decoration, extra) in [("outer", 6.0, 4.0), ("inner", 4.0, 0.0)] {
        let inline = box_for(&document, &tree, name);
        let fragments = inline.inline_fragments();
        assert_eq!(fragments.len(), 3);
        for (index, fragment) in fragments.iter().enumerate() {
            assert_eq!(fragment.first, index == 0);
            assert_eq!(fragment.last, index == 2);
            close(
                fragment.rect.width,
                word + if index == 1 { 0.0 } else { decoration + extra },
            );
            assert_eq!(fragment.baseline, host.line_boxes()[index].baseline);
            close(
                fragment.content_rect.height,
                font.horizontal_line_metrics(16.0).unwrap().ascent
                    - font.horizontal_line_metrics(16.0).unwrap().descent,
            );
        }
    }
    let display = paint::build_display_list(&tree, 448.0, 200.0, false);
    let yellow: Vec<_> = display
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Rect {
                x,
                y,
                w,
                h,
                color: 0xffff00,
            } => Some(Rect {
                x: *x,
                y: *y,
                width: *w,
                height: *h,
            }),
            _ => None,
        })
        .collect();
    assert_eq!(
        yellow,
        box_for(&document, &tree, "outer")
            .inline_fragments()
            .iter()
            .map(|fragment| fragment.rect)
            .collect::<Vec<_>>()
    );
}

#[test]
fn wrapped_decorated_link_hits_fragments_not_the_union_gap_and_outlines_are_fragmented() {
    let (document, tree, _) = layout(
        "#host{width:70px;line-height:40px}#link{background:yellow;border:1px solid}",
        "<a id='link' href='next.html'>Hello Hello Hello</a>",
    );
    let link = box_for(&document, &tree, "link");
    assert_eq!(link.inline_fragments().len(), 3);
    for fragment in link.inline_fragments() {
        assert_eq!(
            tree.hit_test(fragment.rect.x + 0.2, fragment.rect.y + 0.2),
            Some(id(&document, "link"))
        );
    }
    let first = &link.inline_fragments()[0];
    let gap_y = first.rect.y + first.rect.height + 2.0;
    assert_ne!(
        tree.hit_test(first.rect.x + 1.0, gap_y),
        Some(id(&document, "link"))
    );
    assert_eq!(tree.node_rect(id(&document, "link")), Some(link.rect()));
    let outlines = paint::build_display_list(&tree, 448.0, 200.0, true);
    assert!(
        !outlines
            .iter()
            .any(|item| matches!(item, DisplayItem::OutlineRect{x,y,w,h,..}
        if (*x,*y,*w,*h) == (link.rect.x,link.rect.y,link.rect.width,link.rect.height)))
    );
    for fragment in link.inline_fragments() {
        assert!(outlines.iter().any(|item| matches!(item, DisplayItem::OutlineRect{x,y,w,h,color:0xff0000}
            if (*x,*y,*w,*h) == (fragment.rect.x,fragment.rect.y,fragment.rect.width,fragment.rect.height))));
    }
}

#[test]
fn vertical_inline_decorations_do_not_enlarge_lines_but_horizontal_edges_wrap() {
    for decoration in [
        "padding:50px 0",
        "border-top:50px solid;border-bottom:50px solid",
        "margin:50px 0",
    ] {
        let (document, tree, _) = layout(
            &format!("#span{{{decoration}}}"),
            "<span id='span'>H</span>",
        );
        assert_eq!(box_for(&document, &tree, "host").content_rect.height, 20.0);
    }
    let (document, tree, _) = layout(
        "#host{width:80px}#span{padding:0 10px;border:2px solid}",
        "<span id='span'>Hello Hello</span>",
    );
    assert_eq!(box_for(&document, &tree, "host").line_boxes().len(), 2);
}

#[test]
fn replaced_images_use_bottom_margin_edge_baseline_and_full_margin_box_metrics() {
    let (document, tree, font) = layout(
        "#image{width:20px;height:60px;padding:3px 4px;border:2px solid;margin:5px 7px 9px}",
        "H <img id='image' alt='H'>",
    );
    let host = box_for(&document, &tree, "host");
    let image = box_for(&document, &tree, "image");
    let line = &host.line_boxes()[0];
    assert_eq!(line.ascent, 5.0 + 70.0 + 9.0);
    close(line.descent, metrics(&font, 16.0, 20.0).1);
    assert_eq!(image.rect().y + image.rect().height + 9.0, line.baseline);
    assert_eq!(image.rect().y - 5.0, line.rect.y);
    assert_eq!(image.rect().width, 32.0);
    let alt = &image.text_fragments()[0];
    close(
        alt.baseline,
        image.content_rect.y + 60.0 + font.horizontal_line_metrics(16.0).unwrap().descent,
    );
    close(
        alt.ink_rect.y,
        alt.baseline - font.metrics('H', 16.0).height as f32 - font.metrics('H', 16.0).ymin as f32,
    );
    assert_eq!(texts(host)[0].baseline, line.baseline);
}

#[test]
fn image_wraps_and_zero_replaced_boxes_are_real_separators() {
    let (document, tree, _) = layout(
        "#host{width:40px}#image{width:30px;height:25px}",
        "Hello<img id='image'>H",
    );
    let host = box_for(&document, &tree, "host");
    assert_eq!(host.line_boxes().len(), 3);
    assert_eq!(
        box_for(&document, &tree, "image").rect.y + 25.0,
        host.line_boxes()[1].baseline
    );
    let (document, tree, _) = layout(
        "#host{line-height:0}",
        "<img id='image' style='width:0;height:0'>",
    );
    let host = box_for(&document, &tree, "host");
    assert_eq!(host.line_boxes().len(), 1);
    assert!(!host.line_boxes().is_empty());
}

#[test]
fn style_boundaries_neither_inject_space_nor_create_breaks_inside_words() {
    let (document, tree, font) = layout("#host{width:35px}", "A Hel<span id='span'>lo</span>");
    let host = box_for(&document, &tree, "host");
    assert_eq!(host.line_boxes().len(), 2);
    let fragments = texts(host);
    assert_eq!(
        fragments
            .iter()
            .map(|fragment| fragment.text.as_str())
            .collect::<Vec<_>>(),
        ["A", "Hel", "lo"]
    );
    assert_eq!(fragments[1].baseline, fragments[2].baseline);
    close(
        fragments[2].x,
        fragments[1].x + super::inline::measure_text(&font, "Hel", 16.0),
    );
    close(
        host.line_boxes()[1].content_rect.width,
        super::inline::measure_text(&font, "Hello", 16.0),
    );
    let (document, tree, font) = layout("", "H <span> H</span>   <em>H</em>");
    let host = box_for(&document, &tree, "host");
    let fragments = texts(host);
    let word = font.metrics('H', 16.0).advance_width;
    let space = font.metrics(' ', 16.0).advance_width;
    close(fragments[1].x - fragments[0].x, word + space);
    close(fragments[2].x - fragments[1].x, word + space);
}

#[test]
fn block_in_inline_continuations_keep_identity_and_sliced_edges_after_line_wrapping() {
    let (document, tree, _) = layout(
        "#host{width:70px}#span{padding:4px 3px;border:2px solid}#block{display:block;height:12px}",
        "<span id='span'>Hello Hello<b id='block'></b>Hello Hello</span>",
    );
    let host = box_for(&document, &tree, "host");
    assert_eq!(host.children().len(), 3);
    let first = &host.children()[0].children()[0];
    let last = &host.children()[2].children()[0];
    assert_eq!(first.node_id, last.node_id);
    assert_eq!(first.inline_fragments().len(), 2);
    assert_eq!(last.inline_fragments().len(), 2);
    assert!(first.inline_fragments()[0].first);
    assert!(
        first
            .inline_fragments()
            .iter()
            .all(|fragment| !fragment.last)
    );
    assert!(
        last.inline_fragments()
            .iter()
            .all(|fragment| !fragment.first)
    );
    assert!(last.inline_fragments()[1].last);
    close(host.content_rect.height, 92.0);
}

#[test]
fn resize_resets_line_fragment_baseline_and_diagnostic_geometry_exactly_once() {
    let (document, mut tree, font) = layout(
        "#host{width:100%;text-align:right}#span{padding:2px;border:1px solid}",
        "<span id='span'>Hello Hello Hello Hello Hello Hello</span>",
    );
    tree.layout(148.0, &font);
    let small = tree.clone();
    assert!(box_for(&document, &tree, "span").inline_fragments().len() > 1);
    tree.layout(848.0, &font);
    assert_eq!(
        box_for(&document, &tree, "span").inline_fragments().len(),
        1
    );
    tree.layout(148.0, &font);
    assert_eq!(tree, small);
    let values = crate::devtools::format_values(&tree, 1.5);
    assert!(values.contains("used line: LineBox"));
    assert!(values.contains("used inline-fragment: InlineFragment"));
    assert!(values.contains("baseline="));
}

#[test]
fn pseudo_content_and_outside_marker_use_first_content_baseline_without_affecting_height() {
    let (document, tree, _) = layout(
        "#host{display:list-item}#host::marker{content:'!';font-size:40px}#host::before{content:'H';font-size:24px;line-height:30px}",
        "<span style='font-size:32px;line-height:40px'>H</span>",
    );
    let host = box_for(&document, &tree, "host");
    let baseline = host.line_boxes()[0].baseline;
    assert_eq!(host.list_marker().unwrap().baseline, baseline);
    for fragment in texts(host) {
        assert_eq!(fragment.baseline, baseline);
    }
    assert_eq!(host.content_rect.height, 40.0);
    assert!(paint::build_display_list(&tree, 448.0, 200.0, false).iter().any(|item|
        matches!(item, DisplayItem::Text{text,baseline:b,..} if text=="!" && *b==baseline)));
}

#[test]
fn whitespace_at_inline_boundaries_keeps_opening_edges_with_words_and_trims_final_end() {
    let (document, tree, font) = layout(
        "#host{width:65px}#span{padding-left:5px;border-left:2px solid}",
        "Hello <span id='span'> Hello</span>",
    );
    let host = box_for(&document, &tree, "host");
    let span = box_for(&document, &tree, "span");
    assert_eq!(host.line_boxes().len(), 2);
    assert_eq!(span.inline_fragments().len(), 1);
    assert_eq!(span.inline_fragments()[0].rect.x, host.content_rect.x);
    assert_eq!(texts(host)[1].x, host.content_rect.x + 7.0);
    let (document, tree, _) = layout(
        "#host{width:40px}#span{padding-right:10px}",
        "<span id='span'>Hello   </span>",
    );
    let host = box_for(&document, &tree, "host");
    assert_eq!(host.line_boxes().len(), 1);
    close(
        host.line_boxes()[0].content_rect.width,
        super::inline::measure_text(&font, "Hello", 16.0) + 10.0,
    );
    let (document, tree, _) = layout(
        "#span{padding-top:10px;padding-left:2px}",
        "<span id='span'>H<br></span>",
    );
    assert_eq!(box_for(&document, &tree, "host").line_boxes().len(), 1);
    assert!(box_for(&document, &tree, "span").inline_fragments()[0].last);
}

#[test]
fn collapsed_whitespace_uses_the_surviving_space_font_across_style_nodes() {
    let (document, tree, font) =
        layout("", "<span style='font-size:32px'>H </span> <span>H</span>");
    let fragments = texts(box_for(&document, &tree, "host"));
    close(
        fragments[1].x - fragments[0].x,
        font.metrics('H', 32.0).advance_width + font.metrics(' ', 32.0).advance_width,
    );
}

#[test]
fn surviving_styled_spaces_keep_their_ancestor_metrics_and_decorations() {
    let (document, tree, font) = layout(
        "#span{font:100px/120px serif;background:blue}",
        "H<span id='span'> </span>H",
    );
    let host = box_for(&document, &tree, "host");
    let span = box_for(&document, &tree, "span");
    let space = font.metrics(' ', 100.0).advance_width;
    assert_eq!(host.line_boxes()[0].rect.height, 120.0);
    close(span.inline_fragments()[0].rect.width, space);
    close(
        span.inline_fragments()[0].rect.x,
        texts(host)[0].x + texts(host)[0].width,
    );
    close(texts(host)[1].x, span.inline_fragments()[0].rect.x + space);
    let (document, tree, _) = layout(
        "#outer{font:60px/80px serif}#inner{font:10px/10px serif}",
        "H<span id='outer'><span id='inner'> </span></span>H",
    );
    assert_eq!(
        box_for(&document, &tree, "host").line_boxes()[0]
            .rect
            .height,
        80.0
    );
    let (document, tree, _) = layout(
        "#span{font:100px/120px serif;background:blue}",
        "H <span id='span'> </span>H",
    );
    // The first (16px) space survives; the following large space collapses.
    assert_eq!(
        box_for(&document, &tree, "host").line_boxes()[0]
            .rect
            .height,
        20.0
    );
    let (document, tree, _) = layout("#span{font:0px/100px serif}", "H<span id='span'> </span>H");
    assert_eq!(
        box_for(&document, &tree, "host").line_boxes()[0]
            .rect
            .height,
        100.0
    );
}

#[test]
fn extreme_inline_geometry_is_finite_diagnosed_and_stable_on_relayout() {
    let document = parser::parse(
        "<div style='font:16px/1e38 sans-serif;width:100px;text-align:right'>\
         <span style='padding:1e38px;border:1e38px solid;margin:-1e38px'>H H<br>H</span></div>",
    )
    .unwrap();
    let font = load_bundled_font().unwrap();
    let mut tree = build_layout_tree(&document);
    tree.layout(448.0, &font);
    assert!(!tree.diagnostics().is_empty());
    fn finite(root: &LayoutBox) {
        for rect in [root.rect, root.content_rect] {
            assert!(
                [rect.x, rect.y, rect.width, rect.height]
                    .into_iter()
                    .all(f32::is_finite)
            );
        }
        for line in root.line_boxes() {
            assert!(line.baseline.is_finite());
        }
        for fragment in texts(root) {
            assert!(
                [
                    fragment.x,
                    fragment.y,
                    fragment.baseline,
                    fragment.ink_rect.x,
                    fragment.ink_rect.y,
                    fragment.ink_rect.width,
                    fragment.ink_rect.height
                ]
                .into_iter()
                .all(f32::is_finite)
            );
        }
        for child in root.children() {
            finite(child);
        }
    }
    finite(tree.root());
    let count = tree.diagnostics().len();
    assert!(count < 100, "{count}");
    let stable = tree.clone();
    tree.layout(448.0, &font);
    assert_eq!(tree, stable);
}
