use super::*;
use crate::{
    dom::{Document, NodeId},
    html::parser,
    paint::load_bundled_font,
};

fn scene(css: &str, contents: &str) -> (Document, LayoutTree) {
    let doc = parser::parse(&format!("<!doctype html><style>body{{margin:0}}#host{{display:grid;width:300px}}{css}</style><main id='host'>{contents}</main>")).unwrap();
    let mut tree = build_layout_tree(&doc);
    tree.layout(600.0, &load_bundled_font().unwrap());
    (doc, tree)
}

fn id(doc: &Document, name: &str) -> NodeId {
    doc.preorder()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

fn find(b: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
    if b.node_id == Some(id) {
        Some(b)
    } else {
        b.children.iter().find_map(|c| find(c, id))
    }
}

fn box_for<'a>(doc: &Document, tree: &'a LayoutTree, name: &str) -> &'a LayoutBox {
    find(tree.root(), id(doc, name)).unwrap()
}

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.05, "{actual} != {expected}");
}

#[test]
fn fixed_and_fractional_tracks() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:60px 1fr 2fr;gap:10px;grid-auto-rows:40px}",
        "<div id='a'></div><div id='b'></div><div id='c'></div>",
    );
    let host = box_for(&doc, &tree, "host");
    near(box_for(&doc, &tree, "a").rect.width, 60.0);
    near(box_for(&doc, &tree, "b").rect.width, 220.0 / 3.0);
    near(box_for(&doc, &tree, "c").rect.width, 440.0 / 3.0);
    near(box_for(&doc, &tree, "b").rect.x - host.content_rect.x, 70.0);
    near(host.content_rect.height, 40.0);
}

#[test]
fn fractional_factors_below_one_leave_free_space_and_freeze_intrinsic_minima() {
    for (tracks, a, b, end) in [
        ("0.25fr 0.25fr", 75.0, 75.0, 150.0),
        ("minmax(100px, 0.25fr) 0.25fr", 100.0, 50.0, 150.0),
        ("minmax(0px, 1fr) 2fr", 100.0, 200.0, 300.0),
    ] {
        let (doc, tree) = scene(
            &format!("#host{{grid-template-columns:{tracks};justify-content:start}}"),
            "<div id='a'></div><div id='b'></div>",
        );
        let host = box_for(&doc, &tree, "host").content_rect;
        near(box_for(&doc, &tree, "a").rect.width, a);
        near(box_for(&doc, &tree, "b").rect.width, b);
        let r = box_for(&doc, &tree, "b").rect;
        near(r.x + r.width - host.x, end);
    }
}

#[test]
fn repeat_minmax_fit_content_and_named_lines_have_real_geometry() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:[first] repeat(2,[card] minmax(40px,80px)) [last] fit-content(60px);justify-content:start;gap:10px}#a{grid-column:card 2 / last}#b{grid-column:3;white-space:nowrap}#b i{display:block;width:50px;height:20px}",
        "<div id='a'></div><div id='b'><i></i></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "a").rect.x - host.x, 90.0);
    near(box_for(&doc, &tree, "a").rect.width, 80.0);
    near(box_for(&doc, &tree, "b").rect.width, 50.0);
    near(box_for(&doc, &tree, "b").rect.x - host.x, 180.0);
}

#[test]
fn spanning_growth_respects_fixed_track_and_inherent_contribution_not_equal_columns() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:40px auto auto;justify-content:start;gap:10px}#span{grid-column:1 / span 3;width:180px;height:15px}#a{grid-column:2;width:60px;height:10px}#b{grid-column:3;height:10px}",
        "<div id='span'></div><div id='a'></div><div id='b'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "a").rect.x - host.x, 50.0);
    near(box_for(&doc, &tree, "b").rect.x - host.x, 150.0);
    near(box_for(&doc, &tree, "b").rect.width, 30.0);
    near(host.height, 35.0);
}

#[test]
fn equal_span_planned_increases_are_order_independent_not_accumulated() {
    for contents in [
        "<div id='a'></div><div id='b'></div><div id='one'></div><div id='two'></div><div id='three'></div>",
        "<div id='b'></div><div id='a'></div><div id='one'></div><div id='two'></div><div id='three'></div>",
    ] {
        let (doc, tree) = scene(
            "#host{grid-template-columns:auto auto auto;justify-content:start}#a{grid-column:1/span 2;width:100px}#b{grid-column:2/span 2;width:100px}#one,#two,#three{grid-row:2}#one{grid-column:1}#two{grid-column:2}#three{grid-column:3}",
            contents,
        );
        for name in ["one", "two", "three"] {
            near(box_for(&doc, &tree, name).rect.width, 50.0);
        }
    }
}

#[test]
fn static_grid_stacking_ties_use_order_then_stable_source_order() {
    for (order, winner, color) in [(0, "b", 0x0000ff), (2, "a", 0xff0000)] {
        let (doc, tree) = scene(
            &format!(
                "#host{{grid-template-columns:50px;grid-template-rows:50px}}#a,#b{{grid-area:1/1;z-index:3}}#a{{background:red;order:{order}}}#b{{background:blue}}"
            ),
            "<div id='a'></div><div id='b'></div>",
        );
        let r = box_for(&doc, &tree, "host").content_rect;
        let (x, y) = ((r.x + 5.0) as usize, (r.y + 5.0) as usize);
        assert_eq!(pixels(&tree)[y * 600 + x], color);
        assert_eq!(tree.hit_test(x as f32, y as f32), Some(id(&doc, winner)));
    }
}

#[test]
fn spanning_flexible_zero_minimum_can_shrink_and_intrinsic_auto_minimum_cannot() {
    let contents = "<div id='a'>WWWWWWWWWWWWWWWWWWWW</div><div id='b'></div>";
    let (doc, automatic) = scene("#host{width:80px;grid-template-columns:1fr 1fr}", contents);
    assert!(box_for(&doc, &automatic, "a").rect.width > 80.0);
    let (doc, zero) = scene(
        "#host{width:80px;grid-template-columns:minmax(0px,1fr) minmax(0px,1fr)}#a{grid-column:1 / span 2}",
        contents,
    );
    near(box_for(&doc, &zero, "a").rect.width, 80.0);
    let (doc, zero) = scene(
        "#host{width:80px;grid-template-columns:1fr 1fr}#a{min-width:0}",
        contents,
    );
    near(box_for(&doc, &zero, "a").rect.width, 40.0);
}

#[test]
fn row_dense_fills_holes_but_sparse_cursor_preserves_them() {
    for (flow, x, y) in [("row", 200.0, 50.0), ("row dense", 200.0, 0.0)] {
        let (doc, tree) = scene(
            &format!(
                "#host{{grid-template-columns:repeat(3,100px);grid-auto-rows:50px;grid-auto-flow:{flow}}}#a,#b{{grid-column:span 2}}"
            ),
            "<div id='a'></div><div id='b'></div><div id='c'></div>",
        );
        let host = box_for(&doc, &tree, "host").content_rect;
        let c = box_for(&doc, &tree, "c").rect;
        near(c.x - host.x, x);
        near(c.y - host.y, y);
    }
}

#[test]
fn column_dense_and_locked_axes_use_phased_placement() {
    let (doc, tree) = scene(
        "#host{grid-template-rows:repeat(3,40px);grid-auto-columns:50px;grid-auto-flow:column dense;justify-content:start}#a,#b{grid-row:span 2}",
        "<div id='a'></div><div id='b'></div><div id='c'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    let c = box_for(&doc, &tree, "c").rect;
    near(c.x - host.x, 0.0);
    near(c.y - host.y, 80.0);
    let (doc, tree) = scene(
        "#host{grid-template-columns:repeat(3,50px);grid-auto-rows:40px;justify-content:start}#locked{grid-row:1}#explicit{grid-row:1;grid-column:1}#minor{grid-column:2}",
        "<div id='auto'></div><div id='locked'></div><div id='explicit'></div><div id='minor'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "locked").rect.x - host.x, 50.0);
    near(box_for(&doc, &tree, "auto").rect.x - host.x, 100.0);
    near(box_for(&doc, &tree, "minor").rect.y - host.y, 40.0);
}

#[test]
fn negative_explicit_lines_and_preceding_cyclic_implicit_tracks() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:60px 70px;grid-auto-columns:20px 30px;grid-auto-rows:10px;justify-content:start}#a{grid-column:-3/-1;grid-row:1}#before{grid-column:-4/-3;grid-row:1}#after{grid-column:3/5;grid-row:1}",
        "<div id='a'></div><div id='before'></div><div id='after'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "before").rect.width, 30.0);
    near(box_for(&doc, &tree, "a").rect.width, 130.0);
    near(box_for(&doc, &tree, "a").rect.x - host.x, 30.0);
    near(box_for(&doc, &tree, "after").rect.width, 50.0);
}

#[test]
fn areas_generate_lines_and_extent_and_named_spans_search_forward() {
    let (doc, tree) = scene(
        "#host{grid-template-areas:'header header' 'side content';grid-template-columns:[edge] 70px [edge] 1fr [edge];grid-template-rows:30px 60px;gap:10px}#a{grid-area:header}#b{grid-area:content}#c{grid-column:edge / span 2 edge;grid-row:2}",
        "<div id='a'></div><div id='b'></div><div id='c'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "a").rect.width, 300.0);
    near(box_for(&doc, &tree, "b").rect.x - host.x, 80.0);
    near(box_for(&doc, &tree, "b").rect.width, 220.0);
    near(box_for(&doc, &tree, "c").rect.width, 300.0);
    near(host.height, 100.0);
    let (_, tree) = scene(
        "#host{grid-template-columns:[edge] 50px [edge] 50px}#a{grid-column:span 2 edge}",
        "<div id='a'></div>",
    );
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("unanchored named span"))
    );
}

#[test]
fn auto_fill_retains_empty_tracks_auto_fit_collapses_them_and_gaps() {
    for (repeat, width) in [("auto-fill", 100.0), ("auto-fit", 300.0)] {
        let (doc, tree) = scene(
            &format!(
                "#host{{width:320px;grid-template-columns:repeat({repeat},minmax(80px,1fr));gap:10px}}"
            ),
            "<div id='a'></div>",
        );
        near(
            box_for(&doc, &tree, "a").rect.width,
            if repeat == "auto-fit" { 320.0 } else { width },
        );
    }
    let (doc, tree) = scene(
        "#host{grid-template-columns:40px repeat(auto-fit,minmax(60px,1fr)) 40px;gap:10px}#a{grid-column:1}#b{grid-column:-2}",
        "<div id='a'></div><div id='b'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "b").rect.x - host.x, 50.0);
}

#[test]
fn content_distribution_safe_alignment_stretch_and_auto_margins() {
    let (doc, tree) = scene(
        "#host{height:140px;grid-template-columns:50px 50px;grid-template-rows:40px;justify-content:space-between;align-content:center}#a{width:20px;height:10px;place-self:end center}#b{width:20px;height:10px;margin:auto}",
        "<div id='a'></div><div id='b'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    let a = box_for(&doc, &tree, "a").rect;
    let b = box_for(&doc, &tree, "b").rect;
    near(a.x - host.x, 15.0);
    near(a.y - host.y, 80.0);
    near(b.x - host.x, 265.0);
    near(b.y - host.y, 65.0);
    let (doc, tree) = scene(
        "#host{width:40px;grid-template-columns:40px;justify-items:safe center}#a{width:80px;height:10px}",
        "<div id='a'></div>",
    );
    near(
        box_for(&doc, &tree, "a").rect.x,
        box_for(&doc, &tree, "host").content_rect.x,
    );
}

#[test]
fn rtl_places_first_column_at_right_and_preserves_logical_alignment() {
    let (doc, tree) = scene(
        "#host{direction:rtl;grid-template-columns:100px 200px;grid-template-rows:40px}#a{width:20px;justify-self:start}#b{width:30px;justify-self:end}",
        "<div id='a'></div><div id='b'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "a").rect.x - host.x, 280.0);
    near(box_for(&doc, &tree, "b").rect.x - host.x, 0.0);
}

#[test]
fn rows_use_wrapped_content_height_and_horizontal_first_baselines() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:100px 100px;align-items:baseline;gap:10px}#a{font-size:30px}#b{font-size:12px}#c{grid-column:1/-1}",
        "<div id='a'>Big</div><div id='b'>small</div><div id='c'>Text wraps into the next row and retains its glyph positions</div>",
    );
    let a = box_for(&doc, &tree, "a");
    let b = box_for(&doc, &tree, "b");
    near(a.line_boxes[0].baseline, b.line_boxes[0].baseline);
    assert!(box_for(&doc, &tree, "c").line_boxes.len() >= 2);
    let c = box_for(&doc, &tree, "c");
    near(c.rect.y - a.rect.y, a.rect.height + 10.0);
}

#[test]
fn percentages_use_definite_areas_and_indefinite_row_cycles_do_not_reiterate() {
    let (doc, tree) = scene(
        "#host{height:200px;grid-template-columns:25% 1fr;grid-template-rows:50% 1fr;gap:10px}#a{width:50%;height:50%;place-self:start}",
        "<div id='a'></div><div id='b'></div><div id='c'></div>",
    );
    near(box_for(&doc, &tree, "a").content_rect.width, 37.5);
    near(box_for(&doc, &tree, "a").content_rect.height, 50.0);
    let (doc, mut tree) = scene(
        "#host{grid-template-columns:100px;grid-template-rows:50%;row-gap:10%}#a{height:30px}",
        "<div id='a'></div>",
    );
    near(box_for(&doc, &tree, "host").content_rect.height, 30.0);
    near(box_for(&doc, &tree, "a").rect.height, 30.0);
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("cyclic percentage"))
    );
    let original = tree.clone();
    tree.layout(600.0, &load_bundled_font().unwrap());
    assert_eq!(tree, original);
}

#[test]
fn inline_grid_intrinsic_widths_baseline_and_nested_flex_grid() {
    let (doc, tree) = scene(
        "#host{display:block}#inline{display:inline-grid;grid-template-columns:40px auto;gap:10px}#nested{display:flex}#nested i{display:grid;grid-template-columns:20px 30px}#nested b{display:block;width:30px}",
        "before <span id='inline'><i></i><span id='nested'><i><b></b><b></b></i><b></b></span></span> after",
    );
    let inline = box_for(&doc, &tree, "inline");
    near(inline.content_rect.width, 130.0);
    assert_eq!(inline.kind, BoxKind::Inline);
    assert!(!inline.inline_fragments.is_empty() || inline.inline_line.is_some());
    let host = box_for(&doc, &tree, "host");
    near(
        inline.first_local_baseline().unwrap(),
        host.line_boxes[0].baseline,
    );
}

#[test]
fn anonymous_text_pseudos_whitespace_and_relative_offsets_do_not_move_tracks() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:repeat(4,50px);grid-auto-rows:30px}#host::before{content:'before'}#a{position:relative;left:10px;top:5px}",
        " \n <span id='a'></span>Text<div id='b'></div> \n ",
    );
    let host = box_for(&doc, &tree, "host");
    assert_eq!(host.children.len(), 4);
    assert!(host.children.iter().all(|c| c.is_grid_item()));
    assert!(
        host.children
            .iter()
            .any(|c| c.is_anonymous() && c.tag_name == "#anonymous-grid-item")
    );
    near(box_for(&doc, &tree, "a").rect.x - host.content_rect.x, 60.0);
    near(box_for(&doc, &tree, "a").rect.y - host.content_rect.y, 5.0);
    near(
        box_for(&doc, &tree, "b").rect.x - host.content_rect.x,
        150.0,
    );
}

#[test]
fn positioned_grid_area_requires_the_grid_containing_block_and_never_occupies_tracks() {
    let (doc, tree) = scene(
        "#host{position:relative;grid-template-columns:80px 1fr;grid-template-rows:40px 60px;gap:10px}#abs{position:absolute;grid-area:2/2/3/3;inset:0}#fixed{position:fixed;left:0;top:0;width:10px;height:10px}",
        "<div id='flow'></div><div id='abs'></div><div id='fixed'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    let abs = box_for(&doc, &tree, "abs").rect;
    near(abs.x - host.x, 90.0);
    near(abs.y - host.y, 50.0);
    near(abs.width, 210.0);
    near(abs.height, 60.0);
    assert!(!box_for(&doc, &tree, "abs").is_grid_item());
    near(box_for(&doc, &tree, "flow").rect.x, host.x);
    near(box_for(&doc, &tree, "fixed").rect.x, 0.0);
    let (doc, tree) = scene(
        "#host{grid-template-columns:80px 1fr;grid-template-rows:40px}#abs{position:absolute;grid-column:2;width:20px;height:10px;justify-self:center;align-self:center}",
        "<div id='abs'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "abs").rect.x - host.x, 180.0);
    near(box_for(&doc, &tree, "abs").rect.y - host.y, 15.0);
}

fn pixels(tree: &LayoutTree) -> Vec<u32> {
    let mut buffer = vec![0; 600 * 240];
    crate::paint::rasterize_display_list(
        &crate::paint::build_display_list(tree, 600.0, 240.0, false),
        &crate::text::FontContext::bundled(),
        &mut buffer,
        600,
        240,
        1.0,
        0.0,
    );
    buffer
}

#[test]
fn order_and_static_grid_z_index_paint_and_hit_share_stable_context_order() {
    for (z, color, hit) in [
        ("0", 0x0000ff, "b"),
        ("auto", 0x008000, "child"),
        ("-1", 0x0000ff, "b"),
    ] {
        let (doc, tree) = scene(
            &format!(
                "#host{{grid-template-columns:100px;grid-template-rows:100px}}#a,#b{{grid-area:1/1;order:1}}#a{{z-index:{z};background:red}}#b{{z-index:1;background:blue}}#child{{position:relative;height:100px;background:green;z-index:100}}"
            ),
            "<div id='a'><div id='child'></div></div><div id='b'></div>",
        );
        let r = box_for(&doc, &tree, "a").rect;
        let (x, y) = ((r.x + 10.0) as usize, (r.y + 10.0) as usize);
        assert_eq!(pixels(&tree)[y * 600 + x], color);
        assert_eq!(tree.hit_test(x as f32, y as f32), Some(id(&doc, hit)));
    }
    let (doc, tree) = scene(
        "#host{grid-template-columns:100px;grid-template-rows:100px}#a,#b{grid-area:1/1}#a{order:2;background:red}#b{order:-1;background:blue}",
        "<div id='a'></div><div id='b'></div>",
    );
    let r = box_for(&doc, &tree, "a").rect;
    assert_eq!(tree.hit_test(r.x + 10.0, r.y + 10.0), Some(id(&doc, "a")));
    assert_eq!(
        box_for(&doc, &tree, "host").children[0].node_id,
        Some(id(&doc, "a"))
    );
}

#[test]
fn hostile_repeats_spans_and_lines_are_bounded_and_diagnosed_without_panics() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:repeat(1000000000,1px);grid-auto-rows:1px}#a{grid-column:span 2147483647;grid-row:-2147483647}#b{grid-column:2147483647}",
        "<div id='a'></div><div id='b'></div>",
    );
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("256-track"))
    );
    assert!(box_for(&doc, &tree, "host").content_rect.height.is_finite());
    assert!(box_for(&doc, &tree, "a").rect.width <= 256.0);
}

#[test]
fn item_minmax_border_padding_percentages_and_replaced_ratio_are_preserved() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:100px 200px;grid-template-rows:80px}#a{box-sizing:border-box;min-width:70px;max-width:90px;padding:10%;border:2px solid red;margin:5px}",
        "<div id='a'>WWWWWWWWWWWWWWWWWW</div><div id='b'></div>",
    );
    let a = box_for(&doc, &tree, "a");
    near(a.rect.width, 90.0);
    near(a.content_rect.width, 66.0);
    near(a.rect.height, 70.0);
    let (doc, tree) = scene(
        "#host{grid-template-columns:1fr 1fr;width:100px}#a{max-width:30px}",
        "<div id='a'>WWWWWWWWWWWWWWWWWW</div><div id='b'></div>",
    );
    near(box_for(&doc, &tree, "a").content_rect.width, 30.0);
    let doc = parser::parse("<style>body{margin:0}main{display:grid;width:200px;grid-template-columns:auto 1fr;grid-template-rows:auto}img{height:20px}</style><main id='host'><img id='a'><div id='b'></div></main>").unwrap();
    let image = image::RgbaImage::from_pixel(80, 40, image::Rgba([255; 4]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let decoded =
        std::sync::Arc::new(crate::media::DecodedImage::decode(&encoded.into_inner()).unwrap());
    let styles = crate::style::resolve(&doc);
    let mut images = crate::media::ImageMap::new();
    images.insert(id(&doc, "a"), decoded);
    let mut tree = build_layout_tree_with_resources(&doc, &styles.styles, &images);
    tree.layout(600.0, &load_bundled_font().unwrap());
    near(box_for(&doc, &tree, "a").rect.width, 40.0);
    near(box_for(&doc, &tree, "a").rect.height, 20.0);
    near(box_for(&doc, &tree, "b").rect.width, 160.0);
}

#[test]
fn zero_fraction_cyclic_gaps_and_percentage_inline_grid_are_finite() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:0fr 1fr}#a{width:30px}",
        "<div id='a'></div><div id='b'></div>",
    );
    near(box_for(&doc, &tree, "a").rect.width, 30.0);
    near(box_for(&doc, &tree, "b").rect.width, 270.0);
    let (doc, tree) = scene(
        "#host{grid-template-columns:100px;row-gap:20%}div{height:20px}",
        "<div id='a'></div><div id='b'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(host.height, 40.0);
    near(box_for(&doc, &tree, "b").rect.y - host.y, 28.0);
    let (doc, tree) = scene(
        "#host{display:block}#a{display:inline-grid;grid-template-columns:50% auto;gap:10px}#a i{display:block;width:40px;height:10px}",
        "<span id='a'><i id='first'></i><i id='second'></i></span>",
    );
    near(box_for(&doc, &tree, "a").rect.width, 90.0);
    near(
        box_for(&doc, &tree, "second").rect.x - box_for(&doc, &tree, "a").rect.x,
        55.0,
    );
}

#[test]
fn positioned_auto_edges_use_padding_area_and_siblings_remain_out_of_flow() {
    let (doc, tree) = scene(
        "#host{position:relative;padding:10px;border:2px solid black;grid-template-columns:50px 50px 50px;grid-template-rows:40px}#a{position:absolute;grid-column:2/auto;inset:0}#b{position:absolute;grid-column:auto/2;inset:0}",
        "<div id='a'></div><div id='b'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    let a = box_for(&doc, &tree, "a").rect;
    near(a.x - host.x, 50.0);
    near(a.width, 260.0);
    near(a.y - host.y, -10.0);
    near(a.height, 60.0);
    let b = box_for(&doc, &tree, "b").rect;
    near(b.x - host.x, -10.0);
    near(b.width, 60.0);
}

#[test]
fn frame_grid_container_feedback_nested_scroll_resize_and_font_overlay_converge() {
    use crate::{css::media::MediaEnvironment, engine::Frame, text::FontContext};
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../assets/fonts/noto-sans/NotoSansItalic-fixture.ttf"
    ));
    let doc = parser::parse(&format!("<style>body{{margin:0}}@font-face{{font-family:Downloaded;src:url(data:font/ttf;base64,{data});font-style:italic}}main{{display:grid;grid-template-columns:minmax(0px,1fr) 100px;gap:20px}}#panel{{min-width:0;height:80px;overflow:auto;container:panel / inline-size;font:italic 20px/24px Downloaded}}#child{{width:500px;height:300px}}#unit{{display:block;width:50cqw;height:20px}}@container panel (width > 200px){{#child{{background:blue}}}}</style><main><section id='panel'><div id='child'>AVAVAV<span id='unit'></span></div></section><div></div></main>")).unwrap();
    let panel = id(&doc, "panel");
    let unit = id(&doc, "unit");
    let child = id(&doc, "child");
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://grid.test/fonts").unwrap(),
        doc,
        MediaEnvironment::new(500.0, 240.0),
    );
    let mut tree = frame.layout_with_container_queries(&FontContext::bundled());
    near(find(tree.root(), panel).unwrap().content_rect.width, 332.0);
    near(find(tree.root(), unit).unwrap().content_rect.width, 166.0);
    assert_eq!(
        find(tree.root(), child).unwrap().style.background_color,
        Some(0x0000ff)
    );
    assert!(tree.set_scroll_offset(panel, ScrollOffset { x: 35.0, y: 60.0 }));
    frame.retain_scroll_state(&tree);
    frame.set_media_environment(MediaEnvironment::new(400.0, 240.0));
    let small = frame.layout_with_container_queries(&FontContext::bundled());
    near(find(small.root(), panel).unwrap().content_rect.width, 232.0);
    near(find(small.root(), unit).unwrap().content_rect.width, 116.0);
    assert_eq!(
        small.scroll_offset(panel),
        ScrollOffset { x: 35.0, y: 60.0 }
    );
    let mut loading = frame.clone();
    loading
        .load_pending_web_fonts(|_| Ok(()), || false)
        .unwrap();
    loading.copy_scroll_state_from(&frame);
    let loaded = loading.layout_with_container_queries(&FontContext::bundled());
    assert!(loading.styles.get(panel).unwrap().font_face.italic);
    assert_eq!(
        loaded.scroll_offset(panel),
        ScrollOffset { x: 35.0, y: 60.0 }
    );
    near(
        find(loaded.root(), panel).unwrap().content_rect.width,
        232.0,
    );
    let r = find(loaded.root(), panel).unwrap().content_rect;
    assert_eq!(
        loaded.hit_test_viewport(r.x + 5.0, r.y + 5.0, 0.0),
        Some(child)
    );
    assert_eq!(
        loaded,
        loading.layout_with_container_queries(&FontContext::bundled())
    );
}

#[test]
fn grid_fixture_pixels_hits_glyphs_and_relayout_are_deterministic() {
    use crate::{paint, text::FontContext};
    let doc = parser::parse(include_str!("../../tests/pages/grid.html")).unwrap();
    let mut tree = build_layout_tree(&doc);
    let font = FontContext::bundled();
    tree.layout_viewport(800.0, 600.0, &font);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let mut buffer = vec![0; 800 * 600];
    paint::rasterize_display_list(
        &paint::build_display_list(&tree, 800.0, 600.0, false),
        &font,
        &mut buffer,
        800,
        600,
        1.0,
        0.0,
    );
    let cards: Vec<_> = ["card-a", "card-b", "card-c"]
        .iter()
        .map(|name| box_for(&doc, &tree, name).rect)
        .collect();
    for card in &cards {
        near(card.width, 180.0);
        near(card.y, cards[0].y);
        let (x, y) = ((card.x + 4.0) as usize, (card.y + 4.0) as usize);
        assert_eq!(buffer[y * 800 + x], 0xffffff);
    }
    let gap = (cards[0].x + cards[0].width + 6.0) as usize;
    assert_eq!(buffer[(cards[0].y + 4.0) as usize * 800 + gap], 0xeef3f2);
    assert_eq!(buffer[30 * 800 + 30], 0x183b42);
    let badge = box_for(&doc, &tree, "badge").rect;
    let (x, y) = ((badge.x + 5.0) as usize, (badge.y + 5.0) as usize);
    assert_eq!(buffer[y * 800 + x], 0xe7be5c);
    assert_eq!(tree.hit_test(x as f32, y as f32), Some(id(&doc, "badge")));
    fn glyphs(b: &LayoutBox, font: &FontContext) -> usize {
        let mut count = 0;
        for fragment in &b.text_fragments {
            let shaped = font.shape(&b.style, &fragment.text);
            assert_eq!(
                fragment
                    .glyphs
                    .glyphs
                    .iter()
                    .map(|g| (g.id, g.x, g.y))
                    .collect::<Vec<_>>(),
                shaped
                    .glyphs
                    .iter()
                    .map(|g| (g.id, g.x, g.y))
                    .collect::<Vec<_>>()
            );
            near(fragment.width, shaped.width);
            count += fragment.glyphs.glyphs.len();
        }
        count + b.children.iter().map(|c| glyphs(c, font)).sum::<usize>()
    }
    assert!(glyphs(tree.root(), &font) > 150);
    let original = tree.clone();
    tree.layout_viewport(800.0, 600.0, &font);
    assert_eq!(tree, original);
}

#[test]
fn atomic_inline_grid_paints_padding_and_uses_first_row_text_baseline() {
    let (doc, tree) = scene(
        "#host{display:block}#a{display:inline-grid;grid-template-columns:50px;padding:4px;border:2px solid black;background:red}#b{background:blue;font-size:24px}",
        "X<span id='a'><span id='b'>Hi</span></span>Y",
    );
    let a = box_for(&doc, &tree, "a");
    let b = box_for(&doc, &tree, "b");
    near(
        box_for(&doc, &tree, "host").line_boxes[0].baseline,
        b.line_boxes[0].baseline,
    );
    let (x, y) = ((a.rect.x + 3.0) as usize, (a.rect.y + 3.0) as usize);
    assert_eq!(pixels(&tree)[y * 600 + x], 0xff0000);
    assert_eq!(tree.hit_test(x as f32, y as f32), Some(id(&doc, "a")));
}

#[test]
fn track_limit_boundary_and_span_conflict_preserve_exact_extent() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:repeat(256,1px);justify-content:start}#a{grid-column:1/-1}#b{grid-column:span 2/span 5}",
        "<div id='a'></div><div id='b'></div>",
    );
    near(box_for(&doc, &tree, "a").rect.width, 256.0);
    near(box_for(&doc, &tree, "b").rect.width, 2.0);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let (_, tree) = scene(
        "#host{grid-template-columns:repeat(256,1px 1px)}",
        "<div></div>",
    );
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("256-track"))
    );
}

#[test]
fn grid_container_height_limits_reflow_tracks_before_content_alignment() {
    for constraints in ["height:200px;max-height:100px", "min-height:100px"] {
        let (doc, tree) = scene(
            &format!(
                "#host{{{constraints};grid-template-columns:100px;grid-template-rows:20px;align-content:center}}"
            ),
            "<div id='a'></div>",
        );
        let host = box_for(&doc, &tree, "host").content_rect;
        near(host.height, 100.0);
        near(box_for(&doc, &tree, "a").rect.y - host.y, 40.0);
    }
    let (doc, tree) = scene(
        "#host{min-height:100px;grid-template-columns:100px}",
        "<div id='a'></div>",
    );
    near(box_for(&doc, &tree, "a").rect.height, 100.0);
}
#[test]
fn indefinite_fractional_intrinsics_use_largest_hypothetical_fr_not_equal_tracks() {
    let (doc, tree) = scene(
        "#host{display:block}#a{display:inline-grid;grid-template-columns:1fr 2fr;gap:10px}#one{width:40px;height:10px}#two{width:20px;height:10px}#probe{grid-column:2;grid-row:2}",
        "<span id='a'><i id='one'></i><i id='two'></i><i id='probe'></i></span>",
    );
    near(box_for(&doc, &tree, "a").rect.width, 130.0);
    near(box_for(&doc, &tree, "probe").rect.width, 80.0);
    near(
        box_for(&doc, &tree, "probe").rect.x - box_for(&doc, &tree, "a").rect.x,
        50.0,
    );
}

#[test]
fn fit_content_and_min_max_content_tracks_measure_shaped_text_and_wrap_at_the_cap() {
    let (doc, tree) = scene(
        "#host{grid-template-columns:fit-content(60px) max-content min-content;justify-content:start;gap:10px}#a,#b,#c{min-width:0}",
        "<div id='a'>Hi Hi Hi Hi Hi Hi</div><div id='b'>Hello world</div><div id='c'>Hello world</div>",
    );
    let font = crate::text::FontContext::bundled();
    let b = box_for(&doc, &tree, "b");
    let c = box_for(&doc, &tree, "c");
    near(b.rect.width, font.shape(b.style(), "Hello world").width);
    near(
        c.rect.width,
        font.shape(c.style(), "Hello")
            .width
            .max(font.shape(c.style(), "world").width),
    );
    near(box_for(&doc, &tree, "a").rect.width, 60.0);
    assert!(box_for(&doc, &tree, "a").line_boxes.len() > 1);
}

#[test]
fn grid_item_scroll_clips_pixels_and_transforms_hits_without_affecting_neighbor_tracks() {
    let (doc, mut tree) = scene(
        "#host{grid-template-columns:100px 100px;grid-template-rows:80px;gap:10px}#panel{overflow:auto;min-width:0}#one{height:50px;background:blue}#two{height:150px;background:red}#side{background:green}",
        "<div id='panel'><div id='one'></div><div id='two'></div></div><div id='side'></div>",
    );
    let r = box_for(&doc, &tree, "panel").content_rect;
    let side = box_for(&doc, &tree, "side").rect;
    let (x, y) = ((r.x + 5.0) as usize, (r.y + 10.0) as usize);
    assert_eq!(pixels(&tree)[y * 600 + x], 0x0000ff);
    assert_eq!(
        tree.hit_test_viewport(x as f32, y as f32, 0.0),
        Some(id(&doc, "one"))
    );
    assert!(tree.set_scroll_offset(id(&doc, "panel"), ScrollOffset { x: 0.0, y: 60.0 }));
    assert_eq!(pixels(&tree)[y * 600 + x], 0xff0000);
    assert_eq!(
        tree.hit_test_viewport(x as f32, y as f32, 0.0),
        Some(id(&doc, "two"))
    );
    assert_ne!(
        pixels(&tree)[y * 600 + (r.x + r.width + 5.0) as usize],
        0xff0000
    );
    assert_eq!(box_for(&doc, &tree, "side").rect, side);
}
