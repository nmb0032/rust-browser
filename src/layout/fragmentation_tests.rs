use super::*;
use crate::{
    css::media::MediaEnvironment,
    dom::{Document, NodeId},
    engine::{Frame, PrintOptions},
    html::parser,
    paint, style,
    text::FontContext,
};

fn page(css: &str, html: &str) -> (Frame, LayoutTree) {
    let document = parser::parse(&format!(
        "<!doctype html><style>html,body{{margin:0}}{css}</style>{html}"
    ))
    .unwrap();
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://example.test/").unwrap(),
        document,
        MediaEnvironment {
            viewport_width: 400.0,
            viewport_height: 300.0,
            ..MediaEnvironment::default()
        },
    );
    let tree = frame.layout_with_container_queries(&FontContext::bundled());
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    (frame, tree)
}
fn id(d: &Document, name: &str) -> NodeId {
    d.preorder()
        .find(|&id| {
            d.node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}
fn boxes(b: &LayoutBox, node: NodeId) -> Vec<&LayoutBox> {
    let mut result = Vec::new();
    if b.node_id == Some(node) {
        result.push(b);
    }
    for c in &b.children {
        result.extend(boxes(c, node));
    }
    result
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.02, "{a} != {b}");
}
fn print_options() -> PrintOptions {
    PrintOptions {
        width: 200.0,
        height: 100.0,
        margin: 0.0,
        ..PrintOptions::default()
    }
}

#[test]
fn balanced_blocks_have_real_fragments_and_stable_dom_hits() {
    let (frame, tree) = page(
        "#c{width:220px;columns:2;column-gap:20px}.b{height:30px;background:red}",
        "<div id=c><a id=a class=b style='display:block'></a><div id=b class=b></div><div id=d class=b></div><div id=e class=b></div></div>",
    );
    let c = boxes(tree.root(), id(&frame.document, "c"))[0];
    close(c.content_rect.width, 220.0);
    close(c.content_rect.height, 60.0);
    let a = boxes(tree.root(), id(&frame.document, "a"))[0];
    let d = boxes(tree.root(), id(&frame.document, "d"))[0];
    close(a.rect.width, 100.0);
    close(d.rect.x - a.rect.x, 120.0);
    close(d.rect.y, a.rect.y);
    assert_eq!(tree.hit_test(d.rect.x + 2.0, d.rect.y + 2.0), d.node_id);
    assert!(a.fragment.is_some());
}
#[test]
fn columns_reflow_native_shaped_lines_and_preserve_indexed_glyphs() {
    let (frame, tree) = page(
        "#c{width:160px;columns:2;column-gap:0;font:16px/20px monospace}",
        "<div id=c><a id=a>one two three four five six seven eight nine ten</a></div>",
    );
    let a = boxes(tree.root(), id(&frame.document, "a"));
    assert!(a.len() >= 2, "{a:#?}");
    let texts = a
        .iter()
        .flat_map(|b| b.children.iter())
        .flat_map(|b| &b.text_fragments)
        .collect::<Vec<_>>();
    assert!(texts.len() >= 4);
    assert!(texts.iter().all(|t| t.glyphs.width <= 80.01));
    assert!(texts.iter().any(|t| t.x >= 104.0));
    assert!(
        texts
            .iter()
            .all(|t| t.glyphs.glyphs.iter().all(|g| g.id != 0))
    );
    for b in a {
        if let Some(f) = b.inline_fragments.first() {
            assert_eq!(
                tree.hit_test(f.rect.x + 1.0, f.rect.y + 1.0),
                Some(id(&frame.document, "a"))
            );
        }
    }
}
#[test]
fn forced_column_breaks_are_not_page_breaks_and_auto_height_balances() {
    let (frame, tree) = page(
        "#c{width:210px;columns:2;column-gap:10px;height:80px;column-fill:auto}.b{height:30px}#b{break-before:column}#d{break-before:page}",
        "<div id=c><div id=a class=b></div><div id=b class=b></div><div id=d class=b></div></div>",
    );
    let a = boxes(tree.root(), id(&frame.document, "a"))[0];
    let b = boxes(tree.root(), id(&frame.document, "b"))[0];
    let d = boxes(tree.root(), id(&frame.document, "d"))[0];
    close(b.rect.x - a.rect.x, 110.0);
    close(d.rect.x, b.rect.x);
    close(d.rect.y - b.rect.y, 30.0);
}
#[test]
fn spanners_create_independent_balanced_bands_and_gap_center_rules() {
    let (frame, tree) = page(
        "#c{width:220px;columns:2;column-gap:20px;column-rule:2px solid blue}.b{height:30px}#s{column-span:all;height:10px}",
        "<div id=c><div id=a class=b></div><div class=b></div><div id=s></div><div id=d class=b></div><div class=b></div></div>",
    );
    let c = boxes(tree.root(), id(&frame.document, "c"))[0];
    let s = boxes(tree.root(), id(&frame.document, "s"))[0];
    let d = boxes(tree.root(), id(&frame.document, "d"))[0];
    close(c.rect.height, 70.0);
    close(s.rect.width, 220.0);
    close(s.rect.y, 54.0);
    close(d.rect.y, 64.0);
    assert_eq!(c.column_rules.len(), 2);
    close(c.column_rules[0].rect.x, 133.0);
    close(c.column_rules[0].rect.height, 30.0);
}
#[test]
fn relayout_restores_source_boxes_not_fragment_clones() {
    let (_, mut tree) = page(
        "#c{width:220px;columns:2;column-gap:20px}.b{height:30px}",
        "<div id=c><div class=b></div><div class=b></div><div class=b></div><div class=b></div></div>",
    );
    let before = tree.root().rect;
    tree.layout_viewport(400.0, 300.0, &FontContext::bundled());
    assert_eq!(before, tree.root().rect);
    assert!(tree.diagnostics().is_empty());
}
#[test]
fn typed_fragmentation_values_reset_and_invalid_declarations_do_not_win() {
    let document=parser::parse("<div id=c style='--n:3;columns:100px var(--n);column-count:0;column-width:20%;column-fill:balance-all;column-rule:2px solid red;widows:3;orphans:4;box-decoration-break:clone;page-break-before:always'></div>").unwrap();
    let resolution = style::resolve(&document);
    assert_eq!(resolution.stylesheets.diagnostics.len(), 3);
    let s = resolution.styles.get(id(&document, "c")).unwrap();
    assert_eq!(s.fragmentation.count(), Some(3));
    assert_eq!(s.fragmentation.before(), style::fragmentation::Break::Page);
    assert!(s.fragmentation.clone_decoration());
    assert_eq!(s.fragmentation.widows(), 3);
    assert_eq!(s.fragmentation.orphans(), 4);
}
#[test]
fn print_media_page_cascade_true_pages_and_fixed_repetition() {
    let (frame, _) = page(
        "@media print{.b{height:60px;background:red}}@page{size:200px 100px;margin:0}#f{position:fixed;top:0;right:0;width:10px;height:10px;background:blue}",
        "<div id=f></div><div id=a class=b></div><div id=b class=b></div>",
    );
    let printed = frame.print_layout(print_options()).unwrap();
    assert!(printed.diagnostics.is_empty(), "{:?}", printed.diagnostics);
    assert_eq!(printed.pages.len(), 2);
    for p in &printed.pages {
        assert_eq!(p.sheet.width, 200.0);
        let f = boxes(p.tree.root(), id(&frame.document, "f"));
        assert_eq!(f.len(), 1);
        close(f[0].rect.x, 190.0);
        close(f[0].rect.y, 0.0);
    }
    close(
        printed.pages[1]
            .tree
            .node_rect(id(&frame.document, "b"))
            .unwrap()
            .y,
        0.0,
    );
    assert_eq!(
        frame.media_environment().media_type,
        crate::css::media::MediaType::Screen
    );
}
#[test]
fn page_side_break_inserts_one_real_blank_sheet_and_page_counters_shape() {
    let (frame, _) = page(
        "@page{size:200px 100px;margin:10px;@top-center{content:counter(page) '/' counter(pages);font-size:8px}}@page:blank{background:yellow}.b{height:40px}#b{break-before:right}",
        "<div id=a class=b></div><div id=b class=b></div>",
    );
    let printed = frame.print_layout(print_options()).unwrap();
    assert!(printed.diagnostics.is_empty(), "{:?}", printed.diagnostics);
    assert_eq!(printed.pages.len(), 3);
    assert!(printed.pages[1].blank);
    assert!(printed.pages[1].tree.root().children.is_empty());
    assert_eq!(printed.pages[0].margin_text, ["1/3"]);
    assert_eq!(printed.pages[2].margin_text, ["3/3"]);
    assert!(printed.pages[2].display_list.iter().any(
        |d| matches!(d,paint::DisplayItem::ColorText{glyphs,..} if !glyphs.glyphs.is_empty())
    ));
}

#[test]
fn vertical_columns_are_native_inline_axis_rows_not_rotated_horizontal_bitmaps() {
    let (frame, tree) = page(
        "#c{writing-mode:vertical-rl;height:160px;columns:2;column-gap:20px;font:16px/20px monospace;column-rule:2px solid blue}",
        "<div id=c><a id=a>one two three four five six seven eight nine ten</a></div>",
    );
    let c = boxes(tree.root(), id(&frame.document, "c"))[0];
    let a = boxes(tree.root(), id(&frame.document, "a"));
    assert!(a.len() >= 2, "{a:#?}");
    assert!(a.iter().any(|b| b.rect.y >= 114.0));
    assert!(!c.column_rules.is_empty());
    assert!(c.column_rules[0].rect.width > c.column_rules[0].rect.height);
    let glyphs = a
        .iter()
        .flat_map(|b| &b.children)
        .flat_map(|b| &b.text_fragments)
        .flat_map(|f| &f.glyphs.glyphs)
        .collect::<Vec<_>>();
    assert!(!glyphs.is_empty());
    assert!(glyphs.iter().all(|g| g.id != 0));
}

#[test]
fn avoid_units_widows_orphans_and_oversized_atomic_make_forward_progress() {
    let (frame, tree) = page(
        "#c{width:140px;columns:2;column-gap:20px;height:40px;column-fill:auto;font:16px/20px monospace}#a{height:20px}#b{break-inside:avoid}",
        "<div id=c><div id=a></div><div id=b><div style='height:20px'></div><div style='height:20px'></div></div><div id=d>AA BB CC DD EE FF GG HH</div></div>",
    );
    let a = boxes(tree.root(), id(&frame.document, "a"))[0];
    let b = boxes(tree.root(), id(&frame.document, "b"))[0];
    assert_eq!(boxes(tree.root(), id(&frame.document, "b")).len(), 1);
    close(b.rect.x - a.rect.x, 80.0);
    close(b.rect.y, a.rect.y);
    let fragments = boxes(tree.root(), id(&frame.document, "d"));
    assert_eq!(fragments.len(), 2);
    assert!(fragments.iter().all(|f| f.line_boxes.len() == 2));
    let (frame, _) = page(
        ".a{display:flex;height:150px}.b{height:20px}",
        "<div class=a id=a></div><div class=b id=b></div>",
    );
    let print = frame.print_layout(print_options()).unwrap();
    assert_eq!(print.pages.len(), 2);
    assert!(
        print
            .diagnostics
            .iter()
            .any(|d| d.message.contains("oversized indivisible"))
    );
    assert!(
        print.pages[1]
            .tree
            .node_rect(id(&frame.document, "b"))
            .is_some()
    );
}
#[test]
fn slice_and_clone_decorations_are_fragment_edges_not_duplicate_source_boxes() {
    for clone in [false, true] {
        let (frame, tree) = page(
            &format!(
                "#c{{width:140px;columns:2;column-gap:20px;height:48px;column-fill:auto}}#b{{font:16px/20px monospace;border:2px solid red;box-decoration-break:{}}}",
                if clone { "clone" } else { "slice" }
            ),
            "<div id=c><div id=b>AA BB CC DD EE FF GG HH</div></div>",
        );
        let pieces = boxes(tree.root(), id(&frame.document, "b"));
        assert_eq!(pieces.len(), 2);
        assert_eq!(
            pieces[0].fragment.as_ref().unwrap().source_box,
            pieces[1].fragment.as_ref().unwrap().source_box
        );
        assert_ne!(pieces[0].box_id, pieces[1].box_id);
        assert!(pieces[0].fragment.as_ref().unwrap().first);
        assert!(pieces[1].fragment.as_ref().unwrap().last);
        close(
            pieces[0].style.border_width.bottom,
            if clone { 2.0 } else { 0.0 },
        );
        close(
            pieces[1].style.border_width.top,
            if clone { 2.0 } else { 0.0 },
        );
        if clone {
            close(pieces[1].line_boxes[0].rect.y - pieces[1].rect.y, 2.0);
        }
    }
}
#[test]
fn rtl_column_rules_are_between_columns_and_width_count_equations_are_native() {
    let (frame, tree) = page(
        "#c{width:220px;direction:rtl;columns:80px 4;column-gap:20px;column-rule:2px solid red}.b{height:20px}",
        "<div id=c><div class=b id=a></div><div class=b id=b></div></div>",
    );
    let c = boxes(tree.root(), id(&frame.document, "c"))[0];
    let a = boxes(tree.root(), id(&frame.document, "a"))[0];
    let b = boxes(tree.root(), id(&frame.document, "b"))[0];
    close(a.rect.width, 100.0);
    close(a.rect.x - b.rect.x, 120.0);
    close(c.column_rules[0].rect.x, 133.0);
}
#[test]
fn named_first_left_page_cascade_uses_page_fonts_and_area_metrics_before_layout() {
    let (frame, _) = page(
        "@page{size:200px 100px;margin:10px}@page:first{margin-top:20px}@page chapter{font-size:10px;size:20em 10em;padding:2px;border:1px solid red;@top-center{content:counter(page);font-size:inherit}}@page chapter:left{margin-top:15px}.b{height:30px}body{page:chapter}",
        "<div class=b></div><div class=b></div><div class=b></div>",
    );
    let print = frame.print_layout(print_options()).unwrap();
    assert!(print.diagnostics.is_empty(), "{:?}", print.diagnostics);
    assert_eq!(print.pages.len(), 2);
    for p in &print.pages {
        assert_eq!(p.name.as_deref(), Some("chapter"));
        close(p.sheet.width, 200.0);
        close(p.sheet.height, 100.0);
        close(p.area.x, 13.0);
        close(p.area.width, 174.0);
    }
    close(print.pages[0].area.y, 23.0);
    close(print.pages[1].area.y, 18.0);
    assert!(
        print.pages[0]
            .display_list
            .iter()
            .any(|item| matches!(item,paint::DisplayItem::ColorText {size,..} if *size==10.0))
    );
}
#[test]
fn letter_and_a4_orientation_and_percent_page_margins_have_physical_units() {
    for (size, width, height) in [
        ("letter landscape", 1056.0, 816.0),
        ("a4 portrait", 793.7008, 1122.5197),
    ] {
        let (frame, _) = page(
            &format!("@page{{size:{size};margin:10%}}"),
            "<div style='height:1px'></div>",
        );
        let print = frame.print_layout(PrintOptions::default()).unwrap();
        let p = &print.pages[0];
        close(p.sheet.width, width);
        close(p.sheet.height, height);
        close(p.area.x, width * 0.1);
        close(p.area.y, height * 0.1);
    }
}
#[test]
fn every_long_paragraph_line_and_source_glyph_cluster_survives_page_fragmentation_once() {
    let (frame, _) = page(
        "p{margin:0;font:16px/20px monospace}",
        &format!("<p id=p>{}</p>", "AA BB CC DD EE FF GG HH ".repeat(20)),
    );
    let profile = PrintOptions {
        width: 240.0,
        height: 60.0,
        margin: 0.0,
        ..PrintOptions::default()
    };
    let print = frame.print_layout(profile).unwrap();
    let unbroken = frame
        .print_layout(PrintOptions {
            height: 4000.0,
            ..profile
        })
        .unwrap();
    fn runs(tree: &LayoutTree) -> Vec<(String, Vec<(u16, u32)>)> {
        fn visit(b: &LayoutBox, out: &mut Vec<(String, Vec<(u16, u32)>)>) {
            for f in &b.text_fragments {
                out.push((
                    f.text.clone(),
                    f.glyphs
                        .glyphs
                        .iter()
                        .map(|g| (g.id, g.cluster as u32))
                        .collect(),
                ));
            }
            for c in &b.children {
                visit(c, out);
            }
        }
        let mut out = Vec::new();
        visit(tree.root(), &mut out);
        out
    }
    let fragmented = print
        .pages
        .iter()
        .flat_map(|p| runs(&p.tree))
        .collect::<Vec<_>>();
    assert_eq!(fragmented, runs(&unbroken.pages[0].tree));
    assert!(print.pages.len() > 5);
    assert!(print.diagnostics.is_empty());
    assert!(
        frame
            .print_layout(PrintOptions {
                max_pages: 1,
                ..profile
            })
            .is_err()
    );
}
#[test]
fn float_exclusion_and_clearance_do_not_leak_into_next_column() {
    let (frame, tree) = page(
        "#c{width:220px;columns:2;column-gap:20px;height:40px;column-fill:auto}#f{float:left;width:30px;height:40px}#a{height:40px}#b{height:20px;clear:both}",
        "<div id=c><div id=f></div><div id=a></div><div id=b></div></div>",
    );
    let f = boxes(tree.root(), id(&frame.document, "f"))[0];
    let b = boxes(tree.root(), id(&frame.document, "b"))[0];
    close(b.rect.x - f.rect.x, 120.0);
    close(b.rect.y, f.rect.y);
}
