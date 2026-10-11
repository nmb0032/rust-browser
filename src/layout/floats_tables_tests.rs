use super::*;
use crate::{
    dom::{Document, NodeId},
    html::parser,
    paint, style,
};

fn scene(css: &str, html: &str) -> (Document, LayoutTree) {
    let doc = parser::parse(&format!("<!doctype html><style>body{{margin:0}}#host{{width:200px;font:16px/20px monospace}}table{{border-spacing:0}}td,th{{padding:0}}{css}</style><main id=host>{html}</main>")).unwrap();
    let styles = style::resolve(&doc);
    assert!(
        styles.stylesheets.diagnostics.is_empty(),
        "{:?}",
        styles.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&doc, &styles.styles);
    tree.layout(448.0, &crate::text::FontContext::bundled());
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
fn find(b: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
    if b.node_id == Some(node) {
        Some(b)
    } else {
        b.children.iter().find_map(|c| find(c, node))
    }
}
fn box_for<'a>(doc: &Document, tree: &'a LayoutTree, name: &str) -> &'a LayoutBox {
    find(tree.root(), id(doc, name)).unwrap()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.05, "{a} != {b}");
}
fn lines(b: &LayoutBox) -> Vec<&LineBox> {
    b.line_boxes
        .iter()
        .chain(b.children.iter().filter(|c| !c.floating()).flat_map(lines))
        .collect()
}

#[test]
fn float_exclusions_change_each_line_at_each_height_band() {
    let (doc, tree) = scene(
        "#l{float:left;width:60px;height:40px}#r{float:right;width:40px;height:60px}",
        &format!("<div id=l></div><div id=r></div>{}", "aa ".repeat(40)),
    );
    let host = box_for(&doc, &tree, "host");
    let lines = lines(host);
    assert!(lines.len() > 4);
    near(lines[0].rect.x - host.rect.x, 60.0);
    near(lines[0].rect.width, 100.0);
    near(lines[1].rect.width, 100.0);
    near(lines[2].rect.x, host.rect.x);
    near(lines[2].rect.width, 160.0);
    near(lines[3].rect.width, 200.0);
    assert!(tree.diagnostics().is_empty());
}

#[test]
fn multiple_left_and_right_float_collisions_preserve_source_top_order() {
    let (doc, tree) = scene(
        "#a,#b,#c{float:left;width:80px;height:30px}#b{float:right;height:50px}",
        "<div id=a></div><div id=b></div><div id=c></div>",
    );
    let (a, b, c) = (
        box_for(&doc, &tree, "a").rect,
        box_for(&doc, &tree, "b").rect,
        box_for(&doc, &tree, "c").rect,
    );
    near(b.x - a.x, 120.0);
    near(b.y, a.y);
    near(c.y - a.y, 30.0);
    near(c.x, a.x);
}

#[test]
fn floats_inside_inline_are_source_events_not_lifted_normal_blocks() {
    let (doc, tree) = scene(
        "#f{float:left;width:50px;height:40px}",
        "<span id=s>aa <i id=f></i>aa aa aa aa aa aa aa aa aa</span>",
    );
    let host = box_for(&doc, &tree, "host");
    let span = box_for(&doc, &tree, "s");
    assert_eq!(span.children[1].dom_node_id(), Some(id(&doc, "f")));
    near(lines(host)[0].rect.x - host.rect.x, 50.0);
    near(box_for(&doc, &tree, "f").rect.y, lines(host)[0].rect.y);
    assert!(!span.inline_fragments.is_empty());
}

#[test]
fn later_float_that_cannot_fit_preceding_text_moves_below_current_line() {
    let (doc, tree) = scene(
        "#f{float:right;width:160px;height:40px}",
        "<span>aaaa aaaa<i id=f></i> aa aa aa aa aa aa</span>",
    );
    let host = box_for(&doc, &tree, "host");
    near(box_for(&doc, &tree, "f").rect.y - host.rect.y, 20.0);
    near(lines(host)[0].rect.width, 200.0);
    assert!(
        lines(host)
            .iter()
            .skip(1)
            .any(|line| line.rect.width == 40.0)
    );
}

#[test]
fn unbreakable_word_moves_below_exclusions_instead_of_overlapping_float() {
    let (doc, tree) = scene(
        "#f{float:left;width:170px;height:40px}",
        "<div id=f></div>longword",
    );
    let host = box_for(&doc, &tree, "host");
    near(lines(host)[0].rect.y - host.rect.y, 40.0);
    near(lines(host)[0].rect.width, 200.0);
}

#[test]
fn ordinary_nested_blocks_share_outer_exclusions_without_shrinking_border_boxes() {
    let (doc, tree) = scene(
        "#f{float:left;width:60px;height:60px}#p{margin:0}",
        "<div id=f></div><section><p id=p>aa aa aa aa aa aa aa aa</p></section>",
    );
    let p = box_for(&doc, &tree, "p");
    near(p.rect.width, 200.0);
    near(lines(p)[0].rect.width, 140.0);
    near(lines(p)[0].rect.x - p.rect.x, 60.0);
}

#[test]
fn bfc_avoids_outer_floats_and_contains_its_own_floats() {
    let (doc, tree) = scene(
        "#f{float:left;width:60px;height:70px}#panel{overflow:hidden}#inner{float:left;width:30px;height:40px}",
        "<div id=f></div><section id=panel><div id=inner></div></section><div id=clear style='clear:both;height:10px'></div>",
    );
    let host = box_for(&doc, &tree, "host");
    let panel = box_for(&doc, &tree, "panel");
    near(panel.rect.x - host.rect.x, 60.0);
    near(panel.rect.width, 140.0);
    near(panel.rect.height, 40.0);
    near(box_for(&doc, &tree, "clear").rect.y - host.rect.y, 70.0);
}

#[test]
fn fixed_width_bfc_that_cannot_fit_is_displaced_below_float() {
    let (doc, tree) = scene(
        "#f{float:left;width:60px;height:50px}#p{overflow:auto;width:180px;height:20px}",
        "<div id=f></div><div id=p></div>",
    );
    near(
        box_for(&doc, &tree, "p").rect.y - box_for(&doc, &tree, "f").rect.y,
        50.0,
    );
}

#[test]
fn clear_physical_sides_and_clearance_break_parent_collapse() {
    for (clear, expected) in [
        ("left", 40.0),
        ("right", 60.0),
        ("both", 60.0),
        ("none", 10.0),
    ] {
        let (doc, tree) = scene(
            &format!(
                "#host{{padding-top:1px}}#l{{float:left;width:40px;height:40px}}#r{{float:right;width:40px;height:60px}}#c{{clear:{clear};height:10px;margin-top:10px}}"
            ),
            "<div id=l></div><div id=r></div><section><div id=c></div></section>",
        );
        near(
            box_for(&doc, &tree, "c").rect.y - box_for(&doc, &tree, "l").rect.y,
            expected,
        );
    }
}

#[test]
fn float_shrink_to_fit_includes_decorations_zero_auto_margins_and_minmax() {
    let (doc, tree) = scene(
        "#f{float:left;padding:5px;border:2px solid;margin:auto;min-width:40px;max-width:60px}",
        "<div id=f><div style='width:50px;height:10px'></div></div>",
    );
    let f = box_for(&doc, &tree, "f");
    near(f.content_rect.width, 50.0);
    near(f.rect.width, 64.0);
    near(f.margin.left, 0.0);
    near(f.rect.height, 24.0);
}

#[test]
fn float_images_use_intrinsic_ratio_and_relative_offsets_do_not_move_exclusions() {
    let (doc, tree) = scene(
        "#f{float:left;width:40px;height:30px;position:relative;left:5px;top:7px}",
        "<img id=f alt='x'>aa aa aa",
    );
    let host = box_for(&doc, &tree, "host");
    near(box_for(&doc, &tree, "f").rect.x - host.rect.x, 5.0);
    near(lines(host)[0].rect.x - host.rect.x, 40.0);
    near(lines(host)[0].rect.y, host.rect.y);
}

#[test]
fn floated_flex_and_grid_contents_and_positioned_descendants_are_laid_out() {
    for display in ["flex", "grid"] {
        let (doc, tree) = scene(
            &format!(
                "#f{{float:left;display:{display};width:100px;position:relative}}#a,#b{{width:40px;height:20px}}#abs{{position:absolute;right:0;top:0;width:10px;height:10px}}"
            ),
            "<div id=f><div id=a></div><div id=b></div><div id=abs></div></div>aa aa aa",
        );
        let f = box_for(&doc, &tree, "f");
        near(f.content_rect.width, 100.0);
        near(box_for(&doc, &tree, "abs").rect.x + 10.0, f.rect.x + 100.0);
        assert!(f.rect.height >= 20.0);
    }
}

#[test]
fn float_and_clear_are_ignored_for_flex_and_grid_items() {
    for display in ["flex", "grid"] {
        let (doc, tree) = scene(
            &format!(
                "#host{{display:{display}}}#a,#b{{float:left;clear:both;width:50px;height:20px}}"
            ),
            "<div id=a></div><div id=b></div>",
        );
        assert!(!box_for(&doc, &tree, "a").floating());
        assert_eq!(box_for(&doc, &tree, "a").rect.height, 20.0);
        assert!(tree.diagnostics().is_empty());
    }
}

#[test]
fn float_paint_group_is_above_blocks_below_inline_and_positioned_descendants_escape() {
    let (doc, tree) = scene(
        "#f{float:left;width:80px;height:40px;background:red}#f span{position:absolute;left:0;top:0;width:5px;height:5px;z-index:2;background:lime}#p{height:40px;background:blue}#s{position:relative;left:-70px;background:yellow}",
        "<div id=f><span id=abs></span></div><div id=p><span id=s>aa</span></div>",
    );
    let list = paint::build_display_list(&tree, 448.0, 200.0, false);
    let index = |color| {
        list.iter()
            .position(|item| matches!(item,paint::DisplayItem::Rect{color:c,..} if *c==color))
            .unwrap()
    };
    assert!(index(0x0000ff) < index(0xff0000));
    assert!(index(0xff0000) < index(0xffff00));
    assert!(index(0xffff00) < index(0x00ff00));
    let s = box_for(&doc, &tree, "s").inline_fragments[0].rect;
    assert_eq!(tree.hit_test(s.x + 1.0, s.y + 1.0), Some(id(&doc, "s")));
}

#[test]
fn automatic_table_columns_use_unequal_intrinsic_min_and_max_widths() {
    let (doc, tree) = scene(
        "table{width:200px}",
        "<table id=t><tr id=r><td id=a><div style='width:40px;height:10px'></div></td><td id=b><div style='width:100px;height:10px'></div></td></tr></table>",
    );
    let a = box_for(&doc, &tree, "a").rect;
    let b = box_for(&doc, &tree, "b").rect;
    assert!(b.width > a.width * 2.0);
    near(a.width + b.width, 200.0);
    near(a.y, b.y);
    near(b.x, a.x + a.width);
    near(box_for(&doc, &tree, "r").rect.height, 10.0);
}

#[test]
fn fixed_table_uses_columns_then_first_row_not_later_content() {
    let (doc, tree) = scene(
        "table{table-layout:fixed;width:200px}col{width:40px}#a{width:60px}",
        "<table><colgroup><col id=col></colgroup><tr><td></td><td id=a></td><td id=b></td></tr><tr><td><div style='width:500px;height:10px'></div></td><td></td><td></td></tr></table>",
    );
    near(box_for(&doc, &tree, "col").rect.width, 40.0);
    near(box_for(&doc, &tree, "a").rect.width, 60.0);
    near(box_for(&doc, &tree, "b").rect.width, 100.0);
}

#[test]
fn fixed_first_row_colspan_distributes_unassigned_columns() {
    let (doc, tree) = scene(
        "table{table-layout:fixed;width:200px}#span{width:120px}",
        "<table><tr><td id=span colspan=2></td><td id=last></td></tr><tr><td id=a></td><td id=b></td><td></td></tr></table>",
    );
    near(box_for(&doc, &tree, "a").rect.width, 60.0);
    near(box_for(&doc, &tree, "b").rect.width, 60.0);
    near(box_for(&doc, &tree, "last").rect.width, 80.0);
}

#[test]
fn automatic_colspan_constraints_grow_actual_spanned_columns() {
    let (doc, tree) = scene(
        "#span{width:150px}#a{width:30px}#b{width:50px}",
        "<table><tr><td id=span colspan=2></td></tr><tr><td id=a></td><td id=b></td></tr></table>",
    );
    let a = box_for(&doc, &tree, "a").rect;
    let b = box_for(&doc, &tree, "b").rect;
    near(a.width + b.width, 150.0);
    assert!(b.width > a.width);
}

#[test]
fn rowspan_occupancy_and_height_distribute_without_overlap() {
    let (doc, tree) = scene(
        "#span{width:40px;height:80px}#a,#b{height:20px;width:60px}",
        "<table><tbody id=g><tr id=r1><td id=span rowspan=2></td><td id=a></td></tr><tr id=r2><td id=b></td></tr></tbody></table>",
    );
    let span = box_for(&doc, &tree, "span").rect;
    let a = box_for(&doc, &tree, "a").rect;
    let b = box_for(&doc, &tree, "b").rect;
    near(span.height, 80.0);
    near(a.height, 40.0);
    near(b.y - a.y, 40.0);
    near(b.x, a.x);
    near(box_for(&doc, &tree, "g").rect.height, 80.0);
}

#[test]
fn zero_rowspan_stops_at_rowgroup_and_header_footer_geometry_is_reordered() {
    let (doc, tree) = scene(
        "td{width:40px;height:20px}",
        "<table><tfoot><tr><td id=foot></td></tr></tfoot><tbody><tr><td id=span rowspan=0></td><td id=a></td></tr><tr><td id=b></td></tr></tbody><thead><tr><td id=head></td></tr></thead><tbody><tr><td id=next></td></tr></tbody></table>",
    );
    let head = box_for(&doc, &tree, "head").rect;
    near(box_for(&doc, &tree, "span").rect.height, 40.0);
    near(box_for(&doc, &tree, "next").rect.x, head.x);
    near(box_for(&doc, &tree, "foot").rect.y - head.y, 80.0);
}

#[test]
fn shaped_legal_break_minima_and_widths_produce_wrapped_cells_and_row_heights() {
    let (doc, tree) = scene(
        "table{width:120px}#a{width:40px}#b{width:80px}",
        "<table><tr id=r><td id=a>aa aa aa aa</td><td id=b>aa</td></tr></table>",
    );
    let a = box_for(&doc, &tree, "a");
    assert!(a.content_rect.width < 70.0);
    assert!(lines(a).len() > 1);
    near(
        box_for(&doc, &tree, "r").rect.height,
        lines(a).len() as f32 * 20.0,
    );
    near(box_for(&doc, &tree, "b").rect.height, a.rect.height);
}

#[test]
fn cell_baselines_and_top_middle_bottom_align_content_not_border_boxes() {
    let (doc, tree) = scene(
        "table{width:200px}td{width:50px}#big{height:60px}#top{vertical-align:top}#mid{vertical-align:middle}#bot{vertical-align:bottom}",
        "<table><tr><td id=big>aa</td><td id=top>aa</td><td id=mid>aa</td><td id=bot>aa</td></tr></table>",
    );
    let top = box_for(&doc, &tree, "top");
    let mid = box_for(&doc, &tree, "mid");
    let bot = box_for(&doc, &tree, "bot");
    near(top.rect.y, mid.rect.y);
    near(lines(mid)[0].rect.y - lines(top)[0].rect.y, 20.0);
    near(lines(bot)[0].rect.y - lines(top)[0].rect.y, 40.0);
    near(bot.rect.height, 60.0);
    let (doc, tree) = scene(
        "td{vertical-align:baseline}#a{font-size:12px}#b{font-size:30px}",
        "<table><tr><td id=a>aa</td><td id=b>aa</td></tr></table>",
    );
    near(
        lines(box_for(&doc, &tree, "a"))[0].baseline,
        lines(box_for(&doc, &tree, "b"))[0].baseline,
    );
}

#[test]
fn captions_above_and_below_are_included_in_wrapper_height_and_share_width() {
    let (doc, tree) = scene(
        "table{width:100px}caption{height:20px;margin:5px 0}#bottom{caption-side:bottom}td{height:30px}",
        "<table id=t><caption id=top>top</caption><caption id=bottom>bottom</caption><tr><td id=cell></td></tr></table><div id=after style='height:10px'></div>",
    );
    let t = box_for(&doc, &tree, "t").rect;
    near(t.height, 90.0);
    near(box_for(&doc, &tree, "cell").rect.y - t.y, 30.0);
    near(box_for(&doc, &tree, "bottom").rect.y - t.y, 65.0);
    near(box_for(&doc, &tree, "top").rect.width, 100.0);
    near(box_for(&doc, &tree, "after").rect.y, t.y + t.height);
}

#[test]
fn css_table_anonymous_fixup_handles_orphan_cells_and_invalid_table_children() {
    let (doc, tree) = scene(
        ".cell{display:table-cell;width:40px;height:20px}.row{display:table-row}.group{display:table-row-group}.table{display:table}",
        "<div id=table class=table><div class=group><div class=row><span id=a class=cell>aa</span>bb<span id=b class=cell>cc</span></div></div><span id=c>dd</span></div><div id=orphan class=cell>ee</div>",
    );
    let a = box_for(&doc, &tree, "a").rect;
    let b = box_for(&doc, &tree, "b").rect;
    near(a.y, b.y);
    assert!(b.x > a.x + 40.0);
    near(
        box_for(&doc, &tree, "c").inline_fragments[0].baseline,
        lines(box_for(&doc, &tree, "a"))[0].baseline + 20.0,
    );
    assert!(box_for(&doc, &tree, "orphan").rect.height > 0.0);
    assert!(tree.root.children.iter().any(|c| !c.is_anonymous()));
}

#[test]
fn separate_spacing_has_outer_and_inner_gaps_and_table_group_bounds() {
    let (doc, tree) = scene(
        "table{width:110px;border-spacing:5px 7px}td{width:40px;height:20px}",
        "<table id=t><tbody id=g><tr id=r><td id=a></td><td id=b></td></tr></tbody></table>",
    );
    let t = box_for(&doc, &tree, "t").rect;
    let a = box_for(&doc, &tree, "a").rect;
    let b = box_for(&doc, &tree, "b").rect;
    near(a.x - t.x, 5.0);
    near(b.x - a.x - a.width, 5.0);
    near(a.y - t.y, 7.0);
    near(t.height, 34.0);
    assert_eq!(
        tree.node_rect(id(&doc, "r")),
        Some(box_for(&doc, &tree, "r").rect)
    );
    assert_eq!(tree.hit_test(a.x + 1.0, a.y + 1.0), Some(id(&doc, "a")));
}

#[test]
fn collapsed_border_conflicts_choose_width_style_origin_and_source() {
    let (doc, tree) = scene(
        "table{border-collapse:collapse;width:100px}td{height:30px}#a{border-right:2px solid red}#b{border-left:6px dashed blue}",
        "<table id=t><tr><td id=a></td><td id=b></td></tr></table>",
    );
    let t = box_for(&doc, &tree, "t");
    let a = box_for(&doc, &tree, "a");
    assert!(a.used_border.unwrap().right == 3.0);
    assert!(
        t.table_borders
            .iter()
            .any(|edge| edge.color.opaque_rgb() == Some(0x0000ff) && edge.rect.width == 6.0)
    );
    assert!(
        !t.table_borders
            .iter()
            .any(|edge| edge.color.opaque_rgb() == Some(0xff0000))
    );
    let (doc, tree) = scene(
        "table{border-collapse:collapse;width:100px}td{height:30px;border:4px solid red}#b{border-left:4px double blue}",
        "<table id=t><tr><td id=a></td><td id=b></td></tr></table>",
    );
    assert!(
        box_for(&doc, &tree, "t")
            .table_borders
            .iter()
            .any(|e| e.style == crate::css::values::BorderStyle::Double)
    );
}

#[test]
fn hidden_collapsed_edges_suppress_even_wider_visible_borders() {
    let (doc, tree) = scene(
        "table{border-collapse:collapse;width:100px}td{height:20px}#a{border-right:20px solid red}#b{border-left:1px hidden}",
        "<table id=t><tr><td id=a></td><td id=b></td></tr></table>",
    );
    assert!(box_for(&doc, &tree, "t").table_borders.is_empty());
    near(box_for(&doc, &tree, "a").used_border.unwrap().right, 0.0);
}

#[test]
fn empty_cells_hide_only_the_separate_cell_decoration_not_geometry() {
    for collapse in ["separate", "collapse"] {
        let (doc, tree) = scene(
            &format!(
                "table{{width:100px;border-collapse:{collapse}}}td{{width:50px;height:20px;background:red;empty-cells:hide;border:2px solid blue}}"
            ),
            "<table><tr><td id=empty></td><td id=text>aa</td></tr></table>",
        );
        let empty = box_for(&doc, &tree, "empty");
        assert_eq!(empty.hidden_empty_cell, collapse == "separate");
        assert!(empty.rect.width >= 50.0);
        assert!(!box_for(&doc, &tree, "text").hidden_empty_cell);
        let list = paint::build_display_list(&tree, 448.0, 200.0, false);
        let red = list
            .iter()
            .filter(|item| {
                matches!(
                    item,
                    paint::DisplayItem::Rect {
                        color: 0xff0000,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(red, if collapse == "separate" { 1 } else { 2 });
    }
}

#[test]
fn inline_table_is_atomic_and_uses_first_row_baseline() {
    let (doc, tree) = scene(
        "table{display:inline-table;width:80px}td{height:20px}",
        "<span id=s>aa<table id=t><tr><td id=c>aa</td></tr></table>bb</span>",
    );
    let host = box_for(&doc, &tree, "host");
    let table = box_for(&doc, &tree, "t");
    assert_eq!(table.kind, BoxKind::Inline);
    assert!(table.rect.width == 80.0);
    near(
        lines(host)[0].baseline,
        lines(box_for(&doc, &tree, "c"))[0].baseline,
    );
}

#[test]
fn positioned_table_and_float_table_use_intrinsic_columns() {
    for position in ["position:absolute;left:10px;top:15px", "float:right"] {
        let (doc, tree) = scene(
            &format!(
                "#host{{position:relative}}table{{{position}}}#a{{width:40px;height:20px}}#b{{width:80px}}"
            ),
            "<table id=t><tr><td id=a></td><td id=b></td></tr></table>",
        );
        let t = box_for(&doc, &tree, "t");
        near(t.content_rect.width, 120.0);
        near(box_for(&doc, &tree, "a").rect.width, 40.0);
        near(box_for(&doc, &tree, "b").rect.width, 80.0);
        assert!(tree.diagnostics().is_empty());
    }
}

#[test]
fn table_intrinsics_feed_flex_and_grid_layout_without_equal_columns() {
    for display in ["flex", "grid"] {
        let (doc, tree) = scene(
            &format!(
                "#host{{display:{display};grid-template-columns:auto auto}}#a{{width:40px;height:20px}}#b{{width:80px}}"
            ),
            "<table id=t><tr><td id=a></td><td id=b></td></tr></table><div style='width:20px'></div>",
        );
        assert!(box_for(&doc, &tree, "t").rect.width >= 120.0);
        assert!(box_for(&doc, &tree, "b").rect.width > box_for(&doc, &tree, "a").rect.width);
    }
}

#[test]
fn table_minima_overflow_scroll_port_with_stable_clone_and_resize_geometry() {
    let (doc, mut tree) = scene(
        "#host{overflow:auto;height:40px}table{width:100px}td{width:150px;height:60px}",
        "<table><tr><td></td><td></td></tr></table>",
    );
    let host = box_for(&doc, &tree, "host");
    let port = tree.scroll_container(id(&doc, "host")).unwrap();
    assert!(port.range.max.x >= 100.0);
    assert!(port.range.max.y >= 20.0);
    let origin = host.content_rect;
    let stable = tree.clone();
    tree.layout(448.0, &crate::text::FontContext::bundled());
    assert_eq!(tree, stable);
    tree.layout(600.0, &crate::text::FontContext::bundled());
    near(box_for(&doc, &tree, "host").content_rect.x, origin.x);
}

#[test]
fn hostile_spans_and_grid_caps_are_diagnosed_and_finite() {
    let contents = format!(
        "<table id=t><tr><td id=a colspan=999999999999999999999 rowspan=999999999999999999999></td></tr>{}</table>",
        "<tr><td></td></tr>".repeat(520)
    );
    let (doc, tree) = scene("", &contents);
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("span") && d.message.contains("clamped"))
    );
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("grid limit"))
    );
    assert!(box_for(&doc, &tree, "t").rect.width.is_finite());
    assert!(box_for(&doc, &tree, "a").rect.height.is_finite());
}

fn pixels(tree: &LayoutTree, width: usize, height: usize) -> Vec<u32> {
    let mut buffer = vec![0; width * height];
    paint::rasterize_display_list(
        &paint::build_display_list(tree, width as f32, height as f32, false),
        &crate::text::FontContext::bundled(),
        &mut buffer,
        width,
        height,
        1.0,
        0.0,
    );
    buffer
}

#[test]
fn collapsed_alpha_edges_and_crossings_are_rasterized_once() {
    let (doc, tree) = scene(
        "table{width:100px;border-collapse:collapse}td{height:30px;border:4px solid rgb(255 0 0 / 0.5)}",
        "<table id=t><tr><td id=a></td><td></td></tr><tr><td></td><td></td></tr></table>",
    );
    let colors: std::collections::BTreeSet<_> = pixels(&tree, 448, 200)
        .into_iter()
        .filter(|&p| p != 0xffffff)
        .collect();
    assert_eq!(
        colors.len(),
        1,
        "translucent edges must not double-composite: {colors:?}"
    );
    let a = box_for(&doc, &tree, "a").rect;
    let buffer = pixels(&tree, 448, 200);
    let straight = buffer[(a.y + 10.0) as usize * 448 + (a.x + a.width) as usize];
    let crossing = buffer[(a.y + a.height) as usize * 448 + (a.x + a.width) as usize];
    assert_ne!(straight, 0xffffff);
    assert_eq!(crossing, straight);
}

#[test]
fn table_background_layers_follow_table_columns_groups_rows_cells() {
    let (doc, tree) = scene(
        "table{width:120px;background:green}colgroup{background:blue}#col{background:red}tbody{background:yellow}#row{background:purple}#cell{background:lime}td{height:20px}",
        "<table><colgroup><col id=col><col></colgroup><tbody><tr><td id=a></td><td></td></tr><tr id=row><td id=cell></td><td id=b></td></tr></tbody></table>",
    );
    let buffer = pixels(&tree, 448, 200);
    for (name, color) in [("a", 0xffff00), ("cell", 0x00ff00), ("b", 0x800080)] {
        let r = box_for(&doc, &tree, name).rect;
        assert_eq!(
            buffer[(r.y + 5.0) as usize * 448 + (r.x + 5.0) as usize],
            color
        );
        assert_eq!(tree.hit_test(r.x + 5.0, r.y + 5.0), Some(id(&doc, name)));
    }
}

#[test]
fn collapsed_row_borders_do_not_cross_rowspan_interiors() {
    let (doc, tree) = scene(
        "table{width:100px;border-collapse:collapse}tr{border-bottom:4px solid red}td{height:30px}#span{height:60px}",
        "<table id=t><tr><td id=span rowspan=2></td><td></td></tr><tr><td id=b></td></tr></table>",
    );
    let span = box_for(&doc, &tree, "span").rect;
    let b = box_for(&doc, &tree, "b").rect;
    let buffer = pixels(&tree, 448, 200);
    assert_eq!(
        buffer[b.y as usize * 448 + (span.x + 10.0) as usize],
        0xffffff
    );
    assert_eq!(buffer[b.y as usize * 448 + (b.x + 10.0) as usize], 0xff0000);
}

#[test]
fn bfc_percent_sizes_resolve_against_containing_block_not_float_opportunity() {
    let (doc, tree) = scene(
        "#f{float:left;width:60px;height:40px}#b{overflow:hidden;width:100%;height:20px}",
        "<div id=f></div><div id=b></div>",
    );
    near(box_for(&doc, &tree, "b").rect.width, 200.0);
    near(
        box_for(&doc, &tree, "b").rect.y - box_for(&doc, &tree, "f").rect.y,
        40.0,
    );
}

#[test]
fn floats_in_nonbfc_child_contain_at_root_but_do_not_inflate_normal_parent() {
    let (doc, tree) = scene(
        "#f{float:left;width:40px;height:80px}",
        "<section id=parent><div id=f></div></section><div id=after style='height:10px'></div>",
    );
    near(box_for(&doc, &tree, "parent").rect.height, 0.0);
    near(
        box_for(&doc, &tree, "after").rect.y,
        box_for(&doc, &tree, "parent").rect.y,
    );
    assert!(tree.root.rect.height >= box_for(&doc, &tree, "f").rect.y + 80.0);
}

#[test]
fn caption_padding_borders_auto_margins_and_width_belong_to_wrapper_layout() {
    let (doc, tree) = scene(
        "table{width:100px}caption{width:50px;padding:5px;border:2px solid;margin:3px auto}td{height:20px}",
        "<table id=t><caption id=c>aa</caption><tr><td></td></tr></table>",
    );
    let (t, c) = (
        box_for(&doc, &tree, "t").rect,
        box_for(&doc, &tree, "c").rect,
    );
    near(c.width, 64.0);
    near(c.x - t.x, 18.0);
    near(t.height, 60.0);
}

#[test]
fn border_spacing_is_computed_before_inheritance_and_invalid_pairs_do_not_apply() {
    let doc = parser::parse("<style>#p{font-size:10px;border-spacing:1em 2em;border-collapse:collapse;caption-side:bottom;empty-cells:hide}#c{font-size:30px;border-spacing:2px -1px}</style><div id=p><table id=c></table></div>").unwrap();
    let result = style::resolve(&doc);
    let c = result.styles.get(id(&doc, "c")).unwrap();
    // The UA table spacing wins inheritance unless explicitly inherited.
    assert_eq!(
        c.border_spacing,
        (
            crate::css::values::Length::Px(2.0),
            crate::css::values::Length::Px(2.0)
        )
    );
    assert_eq!(c.border_collapse, style::BorderCollapse::Collapse);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("border-spacing"))
    );
    let doc = parser::parse("<style>#p{font-size:10px;border-spacing:1em 2em}#c{font-size:30px;border-spacing:inherit}</style><div id=p><table id=c></table></div>").unwrap();
    let result = style::resolve(&doc);
    assert_eq!(
        result.styles.get(id(&doc, "c")).unwrap().border_spacing,
        (
            crate::css::values::Length::Px(10.0),
            crate::css::values::Length::Px(20.0)
        )
    );
}

#[test]
fn computed_float_blockification_and_positioning_override() {
    let doc = parser::parse("<style>#a{display:inline-table;float:left}#b{display:table-cell;float:right}#c{display:inline;float:left;position:absolute}#d{float:both;clear:center}</style><div id=a></div><div id=b></div><div id=c></div><div id=d></div>").unwrap();
    let result = style::resolve(&doc);
    assert_eq!(
        result.styles.get(id(&doc, "a")).unwrap().display,
        style::Display::Table
    );
    assert_eq!(
        result.styles.get(id(&doc, "b")).unwrap().display,
        style::Display::Block
    );
    assert_eq!(
        result.styles.get(id(&doc, "c")).unwrap().float,
        style::Float::None
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 2);
}

#[test]
fn floats_tables_fixture_pixels_hits_and_relayout_are_deterministic() {
    let doc = parser::parse(include_str!("../../tests/pages/floats_tables.html")).unwrap();
    let mut tree = build_layout_tree(&doc);
    let font = crate::text::FontContext::bundled();
    tree.layout_viewport(800.0, 600.0, &font);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let buffer = pixels(&tree, 800, 600);
    for (name, color) in [("left", 0x183b42), ("right", 0xe7be5c), ("clear", 0x7b9d95)] {
        let r = box_for(&doc, &tree, name).rect;
        let (x, y) = ((r.x + 5.0) as usize, (r.y + r.height - 5.0) as usize);
        assert_eq!(buffer[y * 800 + x], color);
        assert_eq!(tree.hit_test(x as f32, y as f32), Some(id(&doc, name)));
    }
    near(box_for(&doc, &tree, "name-column").rect.width, 140.0);
    assert!(
        box_for(&doc, &tree, "row-span").rect.height > box_for(&doc, &tree, "details").rect.height
    );
    assert!(
        tree.scroll_container(id(&doc, "scroll"))
            .unwrap()
            .range
            .max
            .x
            > 0.0
    );
    let list = paint::build_display_list(&tree, 800.0, 600.0, false);
    let stable = tree.clone();
    tree.layout_viewport(800.0, 600.0, &font);
    assert_eq!(stable, tree);
    assert_eq!(list, paint::build_display_list(&tree, 800.0, 600.0, false));
    assert_eq!(buffer, pixels(&tree, 800, 600));
}

#[test]
fn frame_float_table_resize_font_overlay_container_feedback_and_scroll_are_idempotent() {
    use crate::{css::media::MediaEnvironment, engine::Frame, text::FontContext};
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../assets/fonts/noto-sans/NotoSansItalic-fixture.ttf"
    ));
    let doc = parser::parse(&format!("<style>body{{margin:0}}@font-face{{font-family:Downloaded;src:url(data:font/ttf;base64,{data});font-style:italic}}#panel{{height:80px;overflow:auto;container:panel / inline-size;font:italic 20px/24px Downloaded}}#float{{float:left;width:50px;height:40px}}table{{width:500px;border-spacing:0}}td{{height:200px;padding:0}}#unit{{width:50cqw;height:20px}}@container panel (width > 300px){{#cell{{background:blue}}}}</style><section id=panel><div id=float></div><table><tr><td id=cell>AVAVAV<div id=unit></div></td></tr></table></section>")).unwrap();
    let panel = id(&doc, "panel");
    let cell = id(&doc, "cell");
    let unit = id(&doc, "unit");
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://tables.test/fonts").unwrap(),
        doc,
        MediaEnvironment::new(500.0, 240.0),
    );
    let mut tree = frame.layout_with_container_queries(&FontContext::bundled());
    near(find(tree.root(), unit).unwrap().content_rect.width, 226.0);
    assert_eq!(
        find(tree.root(), cell).unwrap().style.background_color,
        Some(0x0000ff)
    );
    assert!(tree.set_scroll_offset(panel, ScrollOffset { x: 25.0, y: 50.0 }));
    frame.retain_scroll_state(&tree);
    frame.set_media_environment(MediaEnvironment::new(400.0, 240.0));
    let small = frame.layout_with_container_queries(&FontContext::bundled());
    near(find(small.root(), unit).unwrap().content_rect.width, 176.0);
    assert_eq!(
        small.scroll_offset(panel),
        ScrollOffset { x: 25.0, y: 50.0 }
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
        ScrollOffset { x: 25.0, y: 50.0 }
    );
    assert_eq!(
        loaded,
        loading.layout_with_container_queries(&FontContext::bundled())
    );
}

#[test]
fn definite_table_and_cell_heights_resolve_percentage_descendants() {
    for css in ["table{height:100px}", "td{height:100px}"] {
        let (doc, tree) = scene(
            &format!("{css}#child{{height:50%;width:20px}}"),
            "<table><tr><td id=cell><div id=child></div></td></tr></table>",
        );
        near(box_for(&doc, &tree, "child").rect.height, 50.0);
        near(box_for(&doc, &tree, "cell").rect.height, 100.0);
        assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    }
    let (_, tree) = scene(
        "#child{height:50%;width:20px}",
        "<table><tr><td><div id=child></div></td></tr></table>",
    );
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("no definite"))
    );
}

#[test]
fn explicit_percentage_columns_group_widths_and_cell_minmax_are_not_equal_columns() {
    let (doc, tree) = scene(
        "table{table-layout:fixed;width:200px}#group{width:30%}#col{width:40%}",
        "<table><colgroup id=group><col id=first><col id=second></colgroup><col id=col><tr><td id=a></td><td id=b></td><td id=c></td></tr></table>",
    );
    near(box_for(&doc, &tree, "a").rect.width, 60.0);
    near(box_for(&doc, &tree, "b").rect.width, 60.0);
    near(box_for(&doc, &tree, "c").rect.width, 80.0);
    let (doc, tree) = scene(
        "table{width:140px}#a{min-width:60px;max-width:70px}#b{width:80px}",
        "<table><tr><td id=a>aa aa</td><td id=b></td></tr></table>",
    );
    near(box_for(&doc, &tree, "a").rect.width, 60.0);
    near(box_for(&doc, &tree, "b").rect.width, 80.0);
}

#[test]
fn table_extent_limits_diagnose_huge_finite_css_without_nonfinite_geometry() {
    let (_, tree) = scene(
        "table{width:1e38px;border-spacing:1e38px}td{width:1e38px;height:1e38px}",
        "<table><tr><td></td><td></td></tr></table>",
    );
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("extent limit"))
    );
    fn finite(b: &LayoutBox) {
        for r in [b.rect, b.content_rect] {
            assert!(
                r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite(),
                "{r:?}"
            );
        }
        for child in &b.children {
            finite(child)
        }
    }
    finite(tree.root());
}

#[test]
fn float_count_limit_uses_diagnosed_conservative_exclusion_and_stays_finite() {
    let (_, tree) = scene(
        "#host{overflow:hidden}.f{float:left;width:200px;height:1px}",
        &format!("{}aa", "<div class=f></div>".repeat(4098)),
    );
    assert_eq!(
        tree.diagnostics()
            .iter()
            .filter(|d| d.message.starts_with("float exclusion limit"))
            .count(),
        1
    );
    assert!(tree.root.rect.height.is_finite());
}

#[test]
fn html_span_microsyntax_reuses_integer_scanner_with_saturating_layout_bounds() {
    let mut diagnostics = Vec::new();
    for (input, expected) in [(" +2junk", 2), ("++2", 1), ("-2", 1), ("0", 1), ("0003", 3)] {
        assert_eq!(
            super::table::html_span(Some(input), false, &mut diagnostics, None),
            expected
        );
    }

    assert_eq!(
        super::table::html_span(Some("0"), true, &mut diagnostics, None),
        0
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn bfc_avoidance_checks_full_border_height_against_delayed_float_bands() {
    let (doc, tree) = scene(
        "#a{float:left;width:150px;height:40px}#b{float:right;width:100px;height:80px}#panel{overflow:hidden;height:60px}",
        "<div id=a></div><div id=b></div><section id=panel>aa</section>",
    );
    let host = box_for(&doc, &tree, "host").rect;
    let panel = box_for(&doc, &tree, "panel").rect;
    near(panel.y - host.y, 40.0);
    near(panel.x, host.x);
    near(panel.width, 100.0);
}
