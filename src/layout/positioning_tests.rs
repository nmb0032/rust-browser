use super::*;
use crate::{
    css::{
        media::MediaEnvironment,
        values::{Edges, Length, LengthOrAuto},
    },
    dom::{Document, NodeId},
    engine::Frame,
    html::parser,
    paint::{self, DisplayItem},
    style::{self, Position, ZIndex},
    text::FontContext,
};

fn id(doc: &Document, name: &str) -> NodeId {
    doc.preorder()
        .find(|&id| {
            doc.node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

fn box_for(tree: &LayoutTree, node: NodeId) -> &LayoutBox {
    fn find(b: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
        if b.dom_node_id() == Some(node) && b.pseudo_element().is_none() {
            return Some(b);
        }
        b.children().iter().find_map(|child| find(child, node))
    }
    find(tree.root(), node).unwrap()
}

fn layout(source: &str, width: f32, height: f32) -> (Document, LayoutTree) {
    let doc = parser::parse(&format!("<!doctype html><style>html,body {{margin:0}} body {{font:16px/20px sans-serif}} {source}</style>")).unwrap();
    let resolution = style::resolve(&doc);
    assert!(
        resolution.stylesheets.diagnostics.is_empty(),
        "{:?}",
        resolution.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&doc, &resolution.styles);
    tree.layout_viewport(width, height, &FontContext::bundled());
    (doc, tree)
}

fn page(css: &str, html: &str, width: f32, height: f32) -> (Document, LayoutTree) {
    let source = format!(
        "<!doctype html><style>html,body {{margin:0}} body {{font:16px/20px sans-serif}} {css}</style>{html}"
    );
    let doc = parser::parse(&source).unwrap();
    let resolution = style::resolve(&doc);
    assert!(
        resolution.stylesheets.diagnostics.is_empty(),
        "{:?}",
        resolution.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&doc, &resolution.styles);
    tree.layout_viewport(width, height, &FontContext::bundled());
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    (doc, tree)
}

fn pixels(tree: &LayoutTree, width: usize, height: usize, scroll: f32, outlines: bool) -> Vec<u32> {
    let list = paint::build_display_list(tree, width as f32, height as f32, outlines);
    let mut pixels = vec![0; width * height];
    paint::rasterize_display_list(
        &list,
        &FontContext::bundled(),
        &mut pixels,
        width,
        height,
        1.0,
        scroll,
    );
    pixels
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.002, "{actual} != {expected}");
}

#[test]
fn overlapping_lines_paint_fragments_in_line_order_and_hit_the_same_order() {
    let (doc, tree) = page(
        "#line {line-height:0} #inline {background:red}",
        "<div id='line'><span id='inline'>MMMM<br>.</span></div>",
        248.0,
        200.0,
    );
    let inline = box_for(&tree, id(&doc, "inline"));
    assert_eq!(inline.inline_fragments().len(), 2);
    assert_eq!(inline.inline_fragments()[0].line_index, 0);
    assert_eq!(inline.inline_fragments()[1].line_index, 1);
    let first = &inline.children()[0].text_fragments()[0];
    let list = paint::build_display_list(&tree, 248.0, 200.0, false);
    let backgrounds: Vec<_> = list
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            matches!(
                item,
                DisplayItem::Rect {
                    color: 0xff0000,
                    ..
                }
            )
            .then_some(i)
        })
        .collect();
    let first_text = list
        .iter()
        .position(|item| matches!(item, DisplayItem::Text {text,..} if text == "MMMM"))
        .unwrap();
    assert!(backgrounds[0] < first_text && first_text < backgrounds[1]);
    let x = (first.ink_rect.x + 1.0).floor();
    let y = (first.ink_rect.y + 2.0).floor();
    assert_eq!(
        pixels(&tree, 248, 200, 0.0, false)[y as usize * 248 + x as usize],
        0xff0000
    );
    assert_eq!(tree.hit_test(x + 0.5, y + 0.5), Some(id(&doc, "inline")));
}

#[test]
fn non_html_document_element_is_the_root_stacking_context() {
    use crate::dom::{Attribute, ElementData};
    let mut doc = Document::new();
    let root = doc
        .append_element(
            doc.root_id(),
            ElementData::new(
                "page".into(),
                vec![Attribute::new(
                    "style".into(),
                    "display:block;background:blue;height:100px".into(),
                )],
            ),
        )
        .unwrap();
    let negative = doc.append_element(root, ElementData::new("overlay".into(), vec![
        Attribute::new("style".into(), "position:absolute;inset:0 auto auto 0;width:50px;height:50px;z-index:-1;background:red".into()),
    ])).unwrap();
    let mut tree = build_layout_tree(&doc);
    tree.layout_viewport(248.0, 200.0, &FontContext::bundled());
    let raster = pixels(&tree, 248, 200, 0.0, false);
    assert_eq!(raster[10 * 248 + 10], 0xff0000);
    assert_eq!(raster[180 * 248 + 200], 0x0000ff);
    assert_eq!(tree.hit_test(10.0, 10.0), Some(negative));
}

#[test]
fn relative_offsets_translate_fragments_without_changing_flow_or_glyphs() {
    let html = "<div id='a'><span id='text'>Hello</span></div><div id='b'>Following</div>";
    let (doc, shifted) = page(
        "#a {position:relative; left:-12px; top:30px}",
        html,
        248.0,
        200.0,
    );
    let (base_doc, base) = page("", html, 248.0, 200.0);
    let a = box_for(&shifted, id(&doc, "a"));
    let plain = box_for(&base, id(&base_doc, "a"));
    close(a.rect().x, plain.rect().x - 12.0);
    close(a.rect().y, plain.rect().y + 30.0);
    assert_eq!(
        shifted.node_rect(id(&doc, "b")),
        base.node_rect(id(&base_doc, "b"))
    );
    let text = &box_for(&shifted, id(&doc, "text")).children()[0].text_fragments()[0];
    let original = &box_for(&base, id(&base_doc, "text")).children()[0].text_fragments()[0];
    assert_eq!(text.glyphs, original.glyphs);
    close(text.baseline, original.baseline + 30.0);
    close(text.ink_rect.x, original.ink_rect.x - 12.0);
    assert_eq!(
        shifted.hit_test(text.ink_rect.x + 1.0, text.ink_rect.y + 1.0),
        box_for(&shifted, id(&doc, "text")).children()[0].dom_node_id()
    );
}

#[test]
fn relative_percentage_and_opposing_insets_use_flow_containing_block() {
    let (doc, tree) = page(
        "#parent {width:200px;height:100px} #a {position:relative;left:10%;right:50%;top:25%;bottom:80%;width:20px;height:20px}",
        "<div id='parent'><div id='a'></div><div id='b' style='height:10px'></div></div>",
        448.0,
        200.0,
    );
    let parent = box_for(&tree, id(&doc, "parent"));
    let a = box_for(&tree, id(&doc, "a"));
    close(a.rect().x, parent.content_rect().x + 20.0);
    close(a.rect().y, parent.content_rect().y + 25.0);
    close(
        box_for(&tree, id(&doc, "b")).rect().y,
        parent.content_rect().y + 20.0,
    );
}

#[test]
fn absolute_is_removed_from_block_flow_margin_collapse_and_line_breaks() {
    let (doc, tree) = page(
        "#a {height:20px;margin-bottom:30px} #b {height:10px;margin-top:40px} #overlay {position:absolute; width:60px;height:900px;margin:200px}",
        "<div id='a'></div><div id='overlay'></div><div id='b'></div>",
        248.0,
        200.0,
    );
    close(
        box_for(&tree, id(&doc, "b")).rect().y,
        box_for(&tree, id(&doc, "a")).rect().y + 60.0,
    );
    close(tree.root().content_rect().height, 70.0);
}

#[test]
fn absolute_in_inline_stream_does_not_split_or_expand_lines() {
    let (doc, tree) = page(
        "#overlay {position:absolute;width:800px;height:300px} #line {width:150px}",
        "<div id='line'>Hello<span id='overlay'>Overlay</span> world</div>",
        248.0,
        200.0,
    );
    let line = box_for(&tree, id(&doc, "line"));
    assert_eq!(line.line_boxes().len(), 1);
    close(line.content_rect().height, 20.0);
    let overlay = box_for(&tree, id(&doc, "overlay"));
    assert_eq!(overlay.kind(), BoxKind::Block);
    let first = &line.children()[0].text_fragments()[0];
    close(overlay.rect().x, first.x + first.width);
    close(overlay.rect().y, line.content_rect().y);
}

#[test]
fn absolute_padding_box_percentages_margins_and_two_inset_equations() {
    let (doc, tree) = page(
        "#cb {position:relative;width:200px;height:100px;padding:10px;border:5px solid black} #a {position:absolute;left:10%;right:20%;top:10%;bottom:20%;padding:5%;border:2px solid red;margin:5%}",
        "<div id='cb'><div id='a'></div></div>",
        448.0,
        300.0,
    );
    let cb = box_for(&tree, id(&doc, "cb"));
    let a = box_for(&tree, id(&doc, "a"));
    // Containing padding box is 220 x 120. Padding AND all margins use 220.
    close(a.rect().x, cb.rect().x + 5.0 + 22.0 + 11.0);
    close(a.rect().y, cb.rect().y + 5.0 + 12.0 + 11.0);
    close(a.content_rect().width, 220.0 - 22.0 - 44.0 - 22.0 - 26.0);
    close(a.content_rect().height, 120.0 - 12.0 - 24.0 - 22.0 - 26.0);
    close(a.used_margins().bottom.0, 11.0);
    assert_eq!(a.padding, Edges::all(11.0));
}

#[test]
fn positioned_explicit_size_auto_margins_border_box_and_overconstraint() {
    let (doc, tree) = page(
        "#cb {position:relative;width:200px;height:100px} #a {position:absolute;inset:10px;width:100px;height:40px;box-sizing:border-box;padding:5px;border:2px solid red;margin:auto}",
        "<div id='cb'><div id='a'></div></div>",
        448.0,
        200.0,
    );
    let cb = box_for(&tree, id(&doc, "cb"));
    let a = box_for(&tree, id(&doc, "a"));
    close(a.rect().x, cb.rect().x + 50.0);
    close(a.rect().y, cb.rect().y + 30.0);
    assert_eq!(a.content_rect().width, 86.0);
    assert_eq!(a.content_rect().height, 26.0);
    assert_eq!(a.used_margins().left.0, 40.0);
    assert_eq!(a.used_margins().top.0, 20.0);
}

#[test]
fn positioned_natural_and_percentage_heights_and_nested_containing_blocks() {
    let (doc, tree) = page(
        "#cb {position:relative;width:200px;height:160px} #a {position:absolute;left:10px;right:20px;top:20px;height:50%;padding:5px} #nested {position:absolute;right:5px;bottom:10px;width:50%;height:25%} #flow {height:50%}",
        "<div id='cb'><div id='a'><div id='flow'></div><div id='nested'></div></div></div>",
        448.0,
        300.0,
    );
    let cb = box_for(&tree, id(&doc, "cb"));
    let a = box_for(&tree, id(&doc, "a"));
    let nested = box_for(&tree, id(&doc, "nested"));
    close(a.content_rect().width, 160.0);
    close(a.content_rect().height, 80.0);
    close(a.rect().x, cb.rect().x + 10.0);
    close(box_for(&tree, id(&doc, "flow")).content_rect().height, 40.0);
    close(nested.content_rect().width, 85.0);
    close(nested.content_rect().height, 22.5);
    close(nested.rect().x, a.rect().x + 170.0 - 5.0 - 85.0);
    close(nested.rect().y, a.rect().y + 90.0 - 10.0 - 22.5);
    let (doc, tree) = page(
        "#a {position:absolute;top:5px;right:10px;width:100px;padding:2px} #a p {height:20px;margin:10px 0}",
        "<div id='a'><p></p><p></p></div>",
        248.0,
        200.0,
    );
    close(box_for(&tree, id(&doc, "a")).content_rect().height, 70.0);
}

#[test]
fn shrink_to_fit_uses_actual_shaping_whitespace_and_legal_breaks() {
    let (doc, tree) = page(
        "#a {position:absolute;top:0;left:0} #b {position:absolute;top:50px;left:0;right:180px}",
        "<div id='a'>Hello world</div><div id='b'>Hello world</div>",
        248.0,
        200.0,
    );
    let font = FontContext::bundled();
    let a = box_for(&tree, id(&doc, "a"));
    let hello = font.shape(a.style(), "Hello").width;
    let world = font.shape(a.style(), "world").width;
    let space = font.shape(a.style(), " ").width;
    close(a.content_rect().width, hello + space + world);
    // Both opposing insets stretch, rather than shrink.
    close(box_for(&tree, id(&doc, "b")).content_rect().width, 68.0);
    let (doc, tree) = page(
        "#a {position:absolute;left:200px;top:0}",
        "<div id='a'>Hello world</div>",
        248.0,
        200.0,
    );
    close(
        box_for(&tree, id(&doc, "a")).content_rect().width,
        hello.max(world).max(48.0),
    );
}

#[test]
fn fixed_boxes_and_all_descendants_keep_pixels_and_hits_when_scrolled() {
    let (doc, mut tree) = page(
        "#header {position:fixed;inset:0 0 auto;height:40px;background:red;z-index:2} #child {position:absolute;right:5px;top:5px;width:20px;height:20px;background:blue} #flow {height:1000px;background:green}",
        "<div id='header'><span>Hello</span><div id='child'></div></div><div id='flow'></div>",
        248.0,
        200.0,
    );
    let header = box_for(&tree, id(&doc, "header"));
    close(header.rect().x, 0.0);
    close(header.rect().y, 0.0);
    close(header.rect().width, 248.0);
    assert_eq!(
        box_for(&tree, id(&doc, "child")).coordinate_space(),
        CoordinateSpace::Viewport
    );
    let first = pixels(&tree, 248, 200, 0.0, false);
    let scrolled = pixels(&tree, 248, 200, 150.0, false);
    assert_eq!(&first[..248 * 40], &scrolled[..248 * 40]);
    assert_eq!(scrolled[10 * 248 + 235], 0x0000ff);
    assert_eq!(
        tree.hit_test_viewport(235.0, 10.0, 150.0),
        Some(id(&doc, "child"))
    );
    assert_eq!(
        tree.hit_test_viewport(100.0, 30.0, 150.0),
        Some(id(&doc, "header"))
    );
    tree.layout_viewport(348.0, 300.0, &FontContext::bundled());
    close(box_for(&tree, id(&doc, "child")).rect().x, 323.0);
    assert_eq!(
        tree.hit_test_viewport(330.0, 10.0, 150.0),
        Some(id(&doc, "child"))
    );
    let debug = pixels(&tree, 348, 300, 0.0, true);
    let debug_scroll = pixels(&tree, 348, 300, 100.0, true);
    assert_eq!(debug[10 * 348], 0xff0000);
    assert_eq!(debug_scroll[10 * 348], 0xff0000);
}

#[test]
fn nested_fixed_ignores_absolute_ancestor_but_absolute_uses_fixed_padding_box() {
    let (doc, tree) = page(
        "#a {position:absolute;left:100px;top:70px;width:100px;height:100px} #f {position:fixed;right:10px;bottom:20px;width:80px;height:60px;padding:5px} #b {position:absolute;left:10%;top:10%;width:10px;height:10px}",
        "<div id='a'><div id='f'><div id='b'></div></div></div>",
        248.0,
        200.0,
    );
    let fixed = box_for(&tree, id(&doc, "f"));
    close(fixed.rect().x, 148.0);
    close(fixed.rect().y, 110.0);
    let b = box_for(&tree, id(&doc, "b"));
    close(b.rect().x, 157.0);
    close(b.rect().y, 117.0);
    assert_eq!(b.coordinate_space(), CoordinateSpace::Viewport);
}

#[test]
fn inline_containing_block_uses_first_and_last_padding_fragments() {
    let (doc, tree) = page(
        "#line {width:80px} #inline {position:relative;padding:2px;border:1px solid black} #a {position:absolute;inset:0;width:10px;height:10px}",
        "<div id='line'><span id='inline'>Hello world Hello<span id='a'></span></span></div>",
        248.0,
        200.0,
    );
    let inline = box_for(&tree, id(&doc, "inline"));
    assert!(inline.inline_fragments().len() >= 2);
    let first = inline.inline_fragments().first().unwrap();
    let a = box_for(&tree, id(&doc, "a"));
    close(a.rect().x, first.rect.x + 1.0);
    close(a.rect().y, first.rect.y + 1.0);
}

#[test]
fn split_inline_continuations_share_containing_block_not_anonymous_identity() {
    let (doc, tree) = page(
        "#inline {position:relative;left:10px} #a {position:absolute;left:0;top:0;width:10px;height:10px}",
        "<div><span id='inline'>Before<div style='height:30px'>Block</div>After<span id='a'></span></span></div>",
        248.0,
        200.0,
    );
    let inline = box_for(&tree, id(&doc, "inline"));
    let first = inline.inline_fragments().first().unwrap();
    let a = box_for(&tree, id(&doc, "a"));
    close(a.rect().x, first.rect.x);
    close(a.rect().y, first.rect.y);
    assert_eq!(a.position_parent, Some(inline.box_id));
}

#[test]
fn negative_zero_positive_contexts_are_atomic_and_ties_follow_source_order() {
    let html = "<div id='cb'><div id='negative'></div><div id='auto'><div id='escaped'></div></div><div id='zero'><div id='trapped'></div></div><div id='positive'></div><div id='tie'></div></div>";
    let css = "#cb {position:relative;z-index:0;width:100px;height:100px;background:white}
        #cb div {position:absolute;inset:0;width:100px;height:100px}
        #negative {z-index:-1;background:red} #auto {background:blue}
        #escaped {z-index:5;background:purple} #zero {z-index:0;background:green}
        #trapped {z-index:99;background:yellow} #positive {z-index:1;background:black}
        #tie {z-index:1;background:red}";
    let (doc, tree) = page(css, html, 248.0, 200.0);
    assert_eq!(pixels(&tree, 248, 200, 0.0, false)[30 * 248 + 30], 0x800080);
    assert_eq!(tree.hit_test(30.0, 30.0), Some(id(&doc, "escaped")));
    let (_, isolated) = page(
        &css.replace(
            "#auto {background:blue}",
            "#auto {z-index:0;background:blue}",
        ),
        html,
        248.0,
        200.0,
    );
    assert_eq!(
        pixels(&isolated, 248, 200, 0.0, false)[30 * 248 + 30],
        0xff0000
    );
}

#[test]
fn negative_context_is_above_root_background_but_below_inflow_block_backgrounds() {
    let html = "<div id='negative'></div><div id='block'></div>";
    let css = "html {background:blue} #negative {position:absolute;left:0;top:0;width:150px;height:150px;z-index:-1;background:red} #block {width:50px;height:50px;background:green}";
    let (doc, tree) = page(css, html, 248.0, 200.0);
    let raster = pixels(&tree, 248, 200, 0.0, false);
    assert_eq!(raster[30 * 248 + 30], 0x008000);
    assert_eq!(raster[100 * 248 + 100], 0xff0000);
    assert_eq!(tree.hit_test(100.0, 100.0), Some(id(&doc, "negative")));
}

#[test]
fn block_backgrounds_precede_earlier_sibling_text_not_whole_box_dfs() {
    let (doc, tree) = page(
        "#a {line-height:0} #b {height:30px;background:red}",
        "<div id='a'>MMMM</div><div id='b'></div>",
        248.0,
        200.0,
    );
    let a = box_for(&tree, id(&doc, "a"));
    let fragment = &a.children()[0].text_fragments()[0];
    let list = paint::build_display_list(&tree, 248.0, 200.0, false);
    let bg = list
        .iter()
        .position(|item| {
            matches!(
                item,
                DisplayItem::Rect {
                    color: 0xff0000,
                    ..
                }
            )
        })
        .unwrap();
    let text = list
        .iter()
        .position(|item| matches!(item, DisplayItem::Text {text,..} if text == "MMMM"))
        .unwrap();
    assert!(bg < text);
    let raster = pixels(&tree, 248, 200, 0.0, false);
    let hit_y = (fragment.ink_rect.y + fragment.ink_rect.height - 1.0).floor() as usize;
    assert!(
        raster[hit_y * 248 + 24..hit_y * 248 + 60]
            .iter()
            .any(|p| *p != 0xff0000)
    );
    assert_eq!(
        tree.hit_test(
            fragment.ink_rect.x + 1.0,
            fragment.ink_rect.y + fragment.ink_rect.height - 1.0
        ),
        a.children()[0].dom_node_id()
    );
}

#[test]
fn root_scroll_extents_exclude_fixed_but_include_relative_and_absolute_ink() {
    let (_, fixed) = page(
        "#a {position:fixed;top:1000px;width:10px;height:500px}",
        "<div id='a'></div>",
        248.0,
        200.0,
    );
    assert_eq!(paint::max_scroll_offset(&fixed, 200.0), 0.0);
    let (_, absolute) = page(
        "#a {position:absolute;top:1000px;width:10px;height:500px}",
        "<div id='a'></div>",
        248.0,
        200.0,
    );
    assert_eq!(paint::max_scroll_offset(&absolute, 200.0), 1300.0);
    let (_, relative) = page(
        "#a {position:relative;top:1000px;width:10px;height:500px}",
        "<div id='a'></div>",
        248.0,
        200.0,
    );
    assert_eq!(paint::max_scroll_offset(&relative, 200.0), 1324.0);
}

#[test]
fn relayout_frame_cloning_and_container_feedback_are_idempotent() {
    let doc = parser::parse("<!doctype html><style>html,body {margin:0} #cb {position:relative;width:200px;container-type:inline-size}
        #a {position:absolute;left:10%;width:20px;height:20px;z-index:2}
        #f {position:fixed;right:0;bottom:0;width:30px;height:30px}
        @container (width > 100px) {#a {width:50cqw}}
        </style><div id='cb'><div id='a'></div><div style='height:40px'></div></div><div id='f'></div>").unwrap();
    let mut frame = Frame::new_with_environment(
        url::Url::parse("about:blank").unwrap(),
        doc,
        MediaEnvironment {
            viewport_width: 248.0,
            viewport_height: 200.0,
            ..MediaEnvironment::default()
        },
    );
    let font = FontContext::bundled();
    let mut tree = frame.layout_with_container_queries(&font);
    let stable = tree.clone();
    tree.layout_viewport(248.0, 200.0, &font);
    assert_eq!(tree, stable);
    assert_eq!(frame.layout_with_container_queries(&font), stable);
    assert_eq!(frame.clone().layout_with_container_queries(&font), stable);
    close(
        box_for(&stable, id(&frame.document, "a"))
            .content_rect()
            .width,
        100.0,
    );
    assert!(frame.stylesheets.diagnostics.is_empty());
    let dump = crate::devtools::format_values(&stable, 1.0);
    assert!(dump.contains("computed position: Absolute"));
    assert!(dump.contains("z-index=Integer(2)"));
    assert!(dump.contains("coordinate-space: Viewport"));
}

#[test]
fn position_inset_zindex_cascade_reset_variables_and_strict_grammar() {
    let doc = parser::parse("<!doctype html><style>
        #parent {position:relative;inset:10px 20px;z-index:-4}
        #a {position:inherit;inset:inherit;z-index:inherit}
        #b {position:absolute;inset:1px;z-index:5;all:initial}
        #c {--p:fixed;--i:2px auto 10% -3px;--z:+7;position:var(--p);inset:var(--i);z-index:var(--z)}
        #d {position:relative;position:sticky;z-index:2;z-index:1.0;z-index:1e0;left:3px;inset:1px 2px 3px 4px 5px}
        @layer low,high;
        @layer low {#e {position:absolute;left:10px;z-index:2}}
        @layer high {#e {position:fixed;left:20px;z-index:3;position:revert-layer;left:revert-layer;z-index:revert-layer}}
        </style><div id='parent'><span id='a'></span><span id='b'></span><span id='c'></span><span id='d'></span><span id='e'></span></div>").unwrap();
    let result = style::resolve(&doc);
    let get = |name| result.styles.get(id(&doc, name)).unwrap();
    assert_eq!(get("a").position, Position::Relative);
    assert_eq!(get("a").inset, get("parent").inset);
    assert_eq!(get("a").z_index, ZIndex::Integer(-4));
    assert_eq!(get("b").position, Position::Static);
    assert_eq!(get("b").inset, Edges::all(LengthOrAuto::Auto));
    assert_eq!(get("b").z_index, ZIndex::Auto);
    assert_eq!(get("c").position, Position::Fixed);
    assert_eq!(get("c").z_index, ZIndex::Integer(7));
    assert_eq!(get("c").inset.left, LengthOrAuto::Length(Length::Px(-3.0)));
    assert_eq!(get("d").position, Position::Sticky);
    assert_eq!(get("d").z_index, ZIndex::Integer(2));
    assert_eq!(get("d").inset.left, LengthOrAuto::Length(Length::Px(3.0)));
    assert_eq!(get("e").position, Position::Absolute);
    assert_eq!(get("e").z_index, ZIndex::Integer(2));
    assert_eq!(result.stylesheets.diagnostics.len(), 3);
    let (_, tree) = layout("", 248.0, 200.0);
    assert!(tree.root().rect().height.is_finite());
}

#[test]
fn hypothetical_static_positions_distinguish_block_and_inline_display() {
    for display in ["block", "inline"] {
        let (doc, tree) = page(
            &format!(
                "#cb {{position:relative;width:100px}} #a {{position:absolute;display:{display}}}"
            ),
            "<div id='cb'>Hi<span id='a'>Hello world</span> there</div>",
            248.0,
            200.0,
        );
        let cb = box_for(&tree, id(&doc, "cb"));
        let first = &cb.children()[0].text_fragments()[0];
        let a = box_for(&tree, id(&doc, "a"));
        if display == "inline" {
            close(a.rect().x, first.x + first.width);
            close(a.rect().y, cb.rect().y);
            close(
                a.content_rect().width,
                (100.0 - first.width)
                    .min(FontContext::bundled().shape(a.style(), "Hello world").width),
            );
        } else {
            close(a.rect().x, cb.content_rect().x);
            close(a.rect().y, cb.rect().y + 20.0);
        }
        close(cb.content_rect().height, 20.0);
    }
}

#[test]
fn rtl_containing_direction_and_negative_auto_margin_equations() {
    for direction in ["ltr", "rtl"] {
        let (doc, tree) = page(
            &format!("#cb {{position:relative;direction:{direction};width:100px;height:100px}}
                #a {{position:absolute;direction:ltr;inset:10px;width:120px;height:120px;margin:auto}}
                #b {{position:absolute;direction:ltr;left:10px;right:20px;top:0;width:40px;height:10px;margin:0}}
                #c {{position:relative;direction:ltr;left:10px;right:20px;width:10px;height:10px;margin-right:auto}}"),
            "<div id='cb'><div id='a'></div><div id='b'></div><div id='c'></div></div>",
            248.0, 200.0,
        );
        let cb = box_for(&tree, id(&doc, "cb"));
        let a = box_for(&tree, id(&doc, "a"));
        close(a.used_margins().top.0, -20.0);
        close(a.used_margins().bottom.0, -20.0);
        close(
            a.used_margins().left.0,
            if direction == "rtl" { -40.0 } else { 0.0 },
        );
        close(
            a.used_margins().right.0,
            if direction == "rtl" { 0.0 } else { -40.0 },
        );
        close(
            a.rect().x,
            cb.rect().x + if direction == "rtl" { -30.0 } else { 10.0 },
        );
        close(
            box_for(&tree, id(&doc, "b")).rect().x,
            cb.rect().x + if direction == "rtl" { 40.0 } else { 10.0 },
        );
        close(
            box_for(&tree, id(&doc, "c")).rect().x,
            cb.rect().x + if direction == "rtl" { -20.0 } else { 10.0 },
        );
    }
}

#[test]
fn relative_inline_moves_lifted_blocks_and_their_positioned_descendants() {
    let html = "<div><span id='inline'>Before<div id='block' style='height:30px;position:relative'><span id='a' style='position:absolute;left:0;top:0;width:10px;height:10px'></span></div>After</span></div>";
    let (doc, moved) = page(
        "#inline {position:relative;left:13px;top:17px}",
        html,
        248.0,
        200.0,
    );
    let (base_doc, base) = page("#inline {position:relative}", html, 248.0, 200.0);
    for name in ["inline", "block", "a"] {
        let shifted = moved.node_rect(id(&doc, name)).unwrap();
        let original = base.node_rect(id(&base_doc, name)).unwrap();
        close(shifted.x, original.x + 13.0);
        close(shifted.y, original.y + 17.0);
    }
    close(
        moved.root().content_rect().height,
        base.root().content_rect().height,
    );
}

#[test]
fn fixed_images_markers_and_glyphs_share_viewport_space() {
    use std::{io::Cursor, sync::Arc};
    let doc = parser::parse("<!doctype html><style>html,body {margin:0} body {background:red} #fixed {position:fixed;left:50px;top:0;width:100px;z-index:1;background:white}
        #flow {height:800px;background:red} ul {margin:0;padding:0} li {font:16px/20px sans-serif}
        </style><div id='fixed'><img id='img' alt='Alt'><ul><li>Item</li></ul></div><div id='flow'></div>").unwrap();
    let source = image::ImageBuffer::from_pixel(12, 8, image::Rgba([0u8, 0, 255, 255]));
    let mut bytes = Cursor::new(Vec::new());
    source
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let image = Arc::new(crate::media::DecodedImage::decode(&bytes.into_inner()).unwrap());
    let styles = style::resolve(&doc);
    let mut images = crate::media::ImageMap::new();
    images.insert(id(&doc, "img"), image);
    let mut tree = build_layout_tree_with_resources(&doc, &styles.styles, &images);
    tree.layout_viewport(248.0, 200.0, &FontContext::bundled());
    let fixed = box_for(&tree, id(&doc, "fixed"));
    let marker = fixed
        .children()
        .iter()
        .find(|b| b.tag_name() == "<ul>")
        .unwrap()
        .children()[0]
        .list_marker()
        .unwrap();
    assert!(marker.x < fixed.rect().x);
    let raster = pixels(&tree, 248, 200, 0.0, false);
    let scrolled = pixels(&tree, 248, 200, 100.0, false);
    let height = fixed.rect().height.ceil() as usize;
    for y in 0..height {
        assert_eq!(
            &raster[y * 248 + 30..y * 248 + 150],
            &scrolled[y * 248 + 30..y * 248 + 150]
        );
    }
    let img = box_for(&tree, id(&doc, "img"));
    close(img.content_rect().width, 12.0);
    assert_eq!(img.coordinate_space(), CoordinateSpace::Viewport);
    let x = img.content_rect().x as usize;
    let y = img.content_rect().y as usize;
    assert_eq!(scrolled[y * 248 + x], 0x0000ff);
    assert_eq!(
        tree.hit_test_viewport(x as f32 + 1.0, y as f32 + 1.0, 100.0),
        Some(id(&doc, "img"))
    );
}

#[test]
fn html_canvas_background_propagation_is_scroll_invariant_and_not_double_blended() {
    for selector in ["html", "body"] {
        let (_, tree) = page(
            &format!("{selector} {{background:rgb(255 0 0 / 50%)}}"),
            "",
            248.0,
            200.0,
        );
        let raster = pixels(&tree, 248, 200, 0.0, false);
        let scrolled = pixels(&tree, 248, 200, 80.0, false);
        assert_eq!(raster[150 * 248 + 150], 0xff7f7f);
        assert_eq!(raster, scrolled);
    }
}

#[test]
fn absolute_replaced_auto_size_does_not_stretch_to_opposing_insets() {
    use std::{io::Cursor, sync::Arc};
    let doc = parser::parse(
        "<!doctype html><style>html,body {margin:0}
            #cb {position:relative;width:200px;height:100px}
            img {position:absolute;left:10px;right:20px;top:0}
            #percent {top:20px;height:50%}
            </style><div id='cb'><img id='natural'><img id='percent'></div>",
    )
    .unwrap();
    let source = image::DynamicImage::new_rgb8(12, 8);
    let mut bytes = Cursor::new(Vec::new());
    source
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let image = Arc::new(crate::media::DecodedImage::decode(&bytes.into_inner()).unwrap());
    let styles = style::resolve(&doc);
    let mut images = crate::media::ImageMap::new();
    images.insert(id(&doc, "natural"), image.clone());
    images.insert(id(&doc, "percent"), image);
    let mut tree = build_layout_tree_with_resources(&doc, &styles.styles, &images);
    tree.layout_viewport(248.0, 200.0, &FontContext::bundled());
    let cb = box_for(&tree, id(&doc, "cb"));
    let natural = box_for(&tree, id(&doc, "natural"));
    close(natural.rect().x, cb.rect().x + 10.0);
    close(natural.content_rect().width, 12.0);
    close(natural.content_rect().height, 8.0);
    let percent = box_for(&tree, id(&doc, "percent"));
    close(percent.content_rect().height, 50.0);
    close(percent.content_rect().width, 75.0);
}

#[test]
fn shrink_to_fit_includes_block_replaced_intrinsic_width() {
    use std::{io::Cursor, sync::Arc};
    let doc = parser::parse(
        "<!doctype html><style>html,body {margin:0}
        #a {position:absolute;left:0;top:0}
        img {display:block;padding:2px;border:1px solid black;margin:0 5px}
        </style><div id='a'><img id='image'></div>",
    )
    .unwrap();
    let source = image::DynamicImage::new_rgb8(12, 8);
    let mut bytes = Cursor::new(Vec::new());
    source
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let image = Arc::new(crate::media::DecodedImage::decode(&bytes.into_inner()).unwrap());
    let styles = style::resolve(&doc);
    let mut images = crate::media::ImageMap::new();
    images.insert(id(&doc, "image"), image);
    let mut tree = build_layout_tree_with_resources(&doc, &styles.styles, &images);
    tree.layout_viewport(248.0, 200.0, &FontContext::bundled());
    close(box_for(&tree, id(&doc, "a")).content_rect().width, 28.0);
    close(box_for(&tree, id(&doc, "image")).content_rect().width, 12.0);
}

#[test]
fn positioned_list_items_keep_markers_and_normal_sibling_ordinals() {
    let (doc, tree) = page(
        "#a {position:fixed;left:50px;top:10px;width:100px} ol {margin:0;padding:0}",
        "<ol><li id='a'>One</li><li id='b'>Two</li></ol>",
        248.0,
        200.0,
    );
    let a = box_for(&tree, id(&doc, "a"));
    let b = box_for(&tree, id(&doc, "b"));
    assert_eq!(a.list_marker().unwrap().text, "1.");
    assert_eq!(b.list_marker().unwrap().text, "2.");
    assert_eq!(a.coordinate_space(), CoordinateSpace::Viewport);
    close(b.rect().y, 24.0);
    close(tree.root().content_rect().height, 20.0);
    let marker = a.list_marker().unwrap();
    assert_eq!(
        tree.hit_test_viewport(marker.x + 1.0, marker.y + 1.0, 100.0),
        Some(id(&doc, "a"))
    );
}

#[test]
fn negative_descendant_context_escapes_auto_but_not_integer_zero_ancestor() {
    let html = "<div id='parent'><div id='negative'></div></div>";
    for (index, inside) in [("auto", 0x008000), ("0", 0xff0000)] {
        let (doc, tree) = page(
                &format!("html {{background:blue}} #parent {{position:relative;z-index:{index};width:50px;height:50px;background:green}}
                    #negative {{position:absolute;inset:0 auto auto 0;width:150px;height:150px;z-index:-1;background:red}}"),
                html, 248.0, 200.0,
            );
        let raster = pixels(&tree, 248, 200, 0.0, false);
        assert_eq!(raster[30 * 248 + 30], inside);
        assert_eq!(raster[100 * 248 + 100], 0xff0000);
        assert_eq!(
            tree.hit_test(30.0, 30.0),
            Some(id(
                &doc,
                if index == "auto" {
                    "parent"
                } else {
                    "negative"
                }
            ))
        );
    }
}
