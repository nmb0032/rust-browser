use super::*;
use crate::{
    css::values::Length,
    dom::{Document, NodeId},
    html::parser,
    media::{DecodedImage, ImageMap},
    style,
};
use std::{io::Cursor, sync::Arc};

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

fn find(b: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
    if b.node_id == Some(node) && b.pseudo.is_none() {
        return Some(b);
    }
    b.children.iter().find_map(|b| find(b, node))
}

fn page(css: &str, html: &str) -> (Document, LayoutTree) {
    let doc = parser::parse(&format!("<!doctype html><style>body{{margin:0}}#host{{width:200px;font:16px/20px 'Noto Sans'}}{css}</style><main id=host>{html}</main>")).unwrap();
    let mut tree = build_layout_tree(&doc);
    tree.layout_viewport(448.0, 400.0, &crate::text::FontContext::bundled());
    (doc, tree)
}
fn rect(doc: &Document, tree: &LayoutTree, name: &str) -> Rect {
    find(tree.root(), id(doc, name)).unwrap().content_rect
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}

fn bitmap() -> Arc<DecodedImage> {
    let mut bytes = Cursor::new(Vec::new());
    let pixels = image::ImageBuffer::from_fn(4, 2, |x, _| {
        image::Rgba(if x < 2 {
            [255u8, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        })
    });
    pixels
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Arc::new(DecodedImage::decode(bytes.get_ref()).unwrap())
}

#[test]
fn display_grammar_is_atomic_and_preserves_outer_inner_list_components() {
    for (source, expected) in [
        ("inline flow-root", style::Display::InlineBlock),
        ("flow-root block", style::Display::FlowRoot),
        ("inline flex", style::Display::InlineFlex),
        ("grid inline", style::Display::InlineGrid),
        ("inline table", style::Display::InlineTable),
        (
            "list-item inline flow-root",
            style::Display::InlineFlowRootListItem,
        ),
        ("inline list-item", style::Display::InlineListItem),
        ("flow-root list-item", style::Display::FlowRootListItem),
    ] {
        let (doc, _) = page(&format!("#x{{display:{source}}}"), "<div id=x></div>");
        assert_eq!(
            style::resolve(&doc)
                .styles
                .get(id(&doc, "x"))
                .unwrap()
                .display,
            expected
        );
    }
    for bad in [
        "block inline",
        "flex grid",
        "list-item flex",
        "contents block",
        "table-row inline",
        "flow flow",
        "inline-block flow-root",
        "list-item list-item",
    ] {
        let (doc, _) = page(
            &format!("#x{{display:block;display:{bad}}}"),
            "<div id=x></div>",
        );
        let result = style::resolve(&doc);
        assert_eq!(
            result.styles.get(id(&doc, "x")).unwrap().display,
            style::Display::Block,
            "{bad}"
        );
        assert!(!result.stylesheets.diagnostics.is_empty(), "{bad}");
    }
}

#[test]
fn contents_flattens_before_anonymous_generation_but_retains_inheritance_and_dom_identity() {
    let (doc, tree) = page(
        "#flat{display:contents;color:blue;position:relative;z-index:2;background:red;border:9px solid red;overflow:hidden}#flat a{color:inherit}#flat::before{content:'A';display:contents}#flat::after{content:'B'}#block{height:20px}#abs{position:absolute;left:0;top:0;width:10px;height:10px}#host{position:relative}",
        "<section id=flat>text<div id=block></div>tail<a id=link href='#'>link</a><div id=abs></div></section><div id=hidden style='display:none'>hidden</div>",
    );
    assert!(find(tree.root(), id(&doc, "flat")).is_none());
    assert!(find(tree.root(), id(&doc, "hidden")).is_none());
    let host = find(tree.root(), id(&doc, "host")).unwrap();
    let absolute = find(tree.root(), id(&doc, "abs")).unwrap();
    assert_eq!(absolute.position_parent, Some(host.box_id));
    near(absolute.rect.x, host.content_rect.x);
    near(absolute.rect.y, host.content_rect.y);
    let link = find(tree.root(), id(&doc, "link")).unwrap();
    assert_eq!(link.style.color, Some(0x0000ff));
    let f = link.inline_fragments.first().unwrap();
    assert_eq!(
        tree.hit_test(f.rect.x + 1.0, f.rect.y + 1.0),
        Some(id(&doc, "link"))
    );
    let flat_text = doc.node(id(&doc, "flat")).unwrap().children()[0];
    assert!(find(tree.root(), flat_text).is_some());
    let list = crate::paint::build_display_list(&tree, 448.0, 400.0, false);
    assert!(!list.iter().any(|i| matches!(
        i,
        crate::paint::DisplayItem::Rect {
            color: 0xff0000,
            ..
        }
    )));
}

#[test]
fn contents_children_are_real_flex_items_and_replaced_contents_is_none() {
    let (doc, tree) = page(
        "#host{display:flex}#flat{display:contents}#x,#y{width:30px;height:10px}#x{display:inline}img{display:contents}",
        "<div id=flat><span id=x></span><div id=y></div><img id=img></div>",
    );
    let x = find(tree.root(), id(&doc, "x")).unwrap();
    let y = find(tree.root(), id(&doc, "y")).unwrap();
    assert!(x.is_flex_item && y.is_flex_item);
    assert_eq!(
        style::resolve(&doc)
            .styles
            .get(id(&doc, "x"))
            .unwrap()
            .display,
        style::Display::Block
    );
    near(y.rect.x, x.rect.x + 30.0);
    assert!(find(tree.root(), id(&doc, "img")).is_none());
}

#[test]
fn flow_root_contains_floats_and_prevents_child_margin_collapse() {
    let (doc, tree) = page(
        "#x{display:flow-root}#float{float:left;width:30px;height:40px}#child{height:10px;margin-top:20px}#after{height:5px}",
        "<div id=x><div id=float></div><div id=child></div></div><div id=after></div>",
    );
    near(rect(&doc, &tree, "x").height, 40.0);
    near(
        rect(&doc, &tree, "child").y,
        rect(&doc, &tree, "x").y + 20.0,
    );
    near(
        rect(&doc, &tree, "after").y,
        rect(&doc, &tree, "x").y + 40.0,
    );
    let (doc, tree) = page(
        "#x{display:inline flow-root;width:30px;height:12px;background:blue}",
        "A<span id=x></span>B",
    );
    let x = find(tree.root(), id(&doc, "x")).unwrap();
    near(x.content_rect.width, 30.0);
    near(x.content_rect.height, 12.0);
    assert!(x.atomic_inline());
    assert!(
        crate::paint::build_display_list(&tree, 448.0, 400.0, false)
            .iter()
            .any(
                |i| matches!(i,crate::paint::DisplayItem::Rect {color:0x0000ff,w,..} if *w == 30.0)
            )
    );
}

#[test]
fn intrinsic_keywords_use_selected_shaping_and_legal_breaks_not_viewport() {
    let font = crate::text::FontContext::bundled();
    for (source, kind) in [
        ("min-content", 0),
        ("max-content", 1),
        ("fit-content", 2),
        ("fit-content(60px)", 3),
        ("fit-content(calc(25% + 10px))", 3),
    ] {
        let (doc, mut tree) = page(&format!("#x{{width:{source}}}"), "<div id=x>aaaa bb</div>");
        let x = find(tree.root(), id(&doc, "x")).unwrap();
        let min = font.shape(&x.style, "aaaa").width;
        let max = font.shape(&x.style, "aaaa bb").width;
        let expected = match kind {
            0 => min,
            1 | 2 => max,
            _ => min.max(60.0).min(max),
        };
        near(x.content_rect.width, expected);
        assert!(x.style.width.as_ref().unwrap().intrinsic());
        tree.layout_viewport(300.0, 400.0, &font);
        near(rect(&doc, &tree, "x").width, expected);
        assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    }
}

#[test]
fn intrinsic_min_max_logical_sizes_and_math_limits_clamp_real_content() {
    let (doc, tree) = page(
        "#x{width:10px;min-width:min-content;max-width:max-content}#y{width:200px;max-width:fit-content(45px)}#v{writing-mode:vertical-rl;inline-size:max-content;block-size:20px}",
        "<div id=x>aaaa bb</div><div id=y>aaaa bb</div><div id=v>aaaa bb</div>",
    );
    let x = find(tree.root(), id(&doc, "x")).unwrap();
    let min = crate::text::FontContext::bundled()
        .shape(&x.style, "aaaa")
        .width;
    near(x.content_rect.width, min);
    near(rect(&doc, &tree, "y").width, min.max(45.0));
    assert!(rect(&doc, &tree, "v").height > 40.0);
    near(rect(&doc, &tree, "v").width, 20.0);
}

#[test]
fn aspect_ratio_transfers_only_auto_axes_and_honors_border_box_and_bounds() {
    let (doc, tree) = page(
        "#a{width:80px;aspect-ratio:2}#b{width:80px;height:10px;aspect-ratio:2}#c{width:80px;aspect-ratio:2;min-height:50px}#d{box-sizing:border-box;width:100px;padding:10px;aspect-ratio:2}#e{display:inline-block;height:30px;aspect-ratio:2}#f{position:absolute;left:0;top:0;height:30px;aspect-ratio:2}",
        "<div id=a></div><div id=b></div><div id=c></div><div id=d></div><span id=e></span><div id=f></div>",
    );
    for (name, w, h) in [
        ("a", 80.0, 40.0),
        ("b", 80.0, 10.0),
        ("c", 80.0, 50.0),
        ("d", 80.0, 30.0),
        ("e", 60.0, 30.0),
        ("f", 60.0, 30.0),
    ] {
        let r = rect(&doc, &tree, name);
        near(r.width, w);
        near(r.height, h);
    }
}

#[test]
fn intrinsic_sizes_work_in_flex_grid_positioned_table_and_native_vertical() {
    for display in ["flex", "grid", "block"] {
        let (doc, tree) = page(
            &format!("#host{{display:{display}}}#x{{width:min-content}}"),
            "<div id=x>aaaa bb</div>",
        );
        let x = find(tree.root(), id(&doc, "x")).unwrap();
        near(
            x.content_rect.width,
            crate::text::FontContext::bundled()
                .shape(&x.style, "aaaa")
                .width,
        );
    }
    let (doc, tree) = page(
        "#x{position:fixed;width:max-content}table{width:max-content;border-spacing:0}td{padding:0}#v{writing-mode:vertical-rl;inline-size:min-content;block-size:20px}",
        "<div id=x>aaaa bb</div><table id=t><tr><td>aaaa</td><td>bb</td></tr></table><div id=v>aaaa bb</div>",
    );
    assert!(rect(&doc, &tree, "x").width > 50.0);
    assert!(rect(&doc, &tree, "t").width > 50.0);
    assert!(rect(&doc, &tree, "v").height > 30.0);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
}

#[test]
fn replaced_elements_have_real_intrinsic_boxes_even_without_bitmap_or_script() {
    let (doc, tree) = page(
        "#host{width:360px}#percent{width:50%}#ratio{width:80px;aspect-ratio:4/1}#canvas{width:80px}iframe,video,canvas{display:block}",
        "<video id=video></video><canvas id=default></canvas><iframe id=iframe></iframe><canvas id=canvas width=40 height=10></canvas><video id=attrs width=70 height=20></video><iframe id=percent width='50%'></iframe><canvas id=ratio></canvas><div id=div></div>",
    );
    for name in ["video", "default", "iframe"] {
        let r = rect(&doc, &tree, name);
        near(r.width, 300.0);
        near(r.height, 150.0);
    }
    near(rect(&doc, &tree, "canvas").height, 20.0);
    near(rect(&doc, &tree, "attrs").width, 70.0);
    near(rect(&doc, &tree, "attrs").height, 20.0);
    near(rect(&doc, &tree, "percent").width, 180.0);
    near(rect(&doc, &tree, "ratio").height, 20.0);
    assert!(find(tree.root(), id(&doc, "div")).unwrap().image.is_none());
}

#[test]
fn natural_ratio_auto_precedence_and_object_fit_do_not_resize_or_expand_hits() {
    for (fit, w, h) in [
        ("fill", 40.0, 40.0),
        ("contain", 40.0, 20.0),
        ("cover", 80.0, 40.0),
        ("none", 4.0, 2.0),
        ("scale-down", 4.0, 2.0),
    ] {
        let doc = parser::parse(&format!("<!doctype html><style>body{{margin:0}}img{{width:40px;height:40px;object-fit:{fit};object-position:right bottom}}</style><img id=x>")).unwrap();
        let result = style::resolve(&doc);
        let mut images = ImageMap::new();
        images.insert(id(&doc, "x"), bitmap());
        let mut tree = build_layout_tree_with_resources(&doc, &result.styles, &images);
        tree.layout_viewport(300.0, 200.0, &crate::text::FontContext::bundled());
        let x = find(tree.root(), id(&doc, "x")).unwrap();
        near(x.content_rect.width, 40.0);
        near(x.content_rect.height, 40.0);
        near(x.object_rect().width, w);
        near(x.object_rect().height, h);
        near(x.object_rect().x, x.content_rect.x + 40.0 - w);
        assert_ne!(
            tree.hit_test(x.content_rect.x - 1.0, x.content_rect.y + 1.0),
            Some(id(&doc, "x"))
        );
        for scale in [1.0, 2.0] {
            let list = crate::paint::build_display_list(&tree, 300.0, 200.0, false);
            let mut pixels = vec![0; (300.0 * scale) as usize * (200.0 * scale) as usize];
            crate::paint::rasterize_display_list(
                &list,
                &crate::text::FontContext::bundled(),
                &mut pixels,
                (300.0 * scale) as usize,
                (200.0 * scale) as usize,
                scale,
                0.0,
            );
            assert_eq!(
                pixels
                    [(25.0 * scale) as usize * (300.0 * scale) as usize + (23.0 * scale) as usize],
                0xffffff
            );
            assert!(pixels.contains(&0x0000ff));
        }
    }
}

#[test]
fn replaced_auto_ratio_and_min_max_sizing_preserve_bitmap_shape() {
    let doc=parser::parse("<!doctype html><style>body{margin:0}img{display:block;width:80px;height:auto}#auto{aspect-ratio:auto 4/1}#css{aspect-ratio:4/1}#max{width:auto;max-width:2px}#min{width:auto;min-width:8px}</style><img id=auto><img id=css><img id=max><img id=min>").unwrap();
    let styles = style::resolve(&doc).styles;
    let images = ["auto", "css", "max", "min"]
        .into_iter()
        .map(|name| (id(&doc, name), bitmap()))
        .collect();
    let mut tree = build_layout_tree_with_resources(&doc, &styles, &images);
    tree.layout(300.0, &crate::text::FontContext::bundled());
    for (name, w, h) in [
        ("auto", 80.0, 40.0),
        ("css", 80.0, 20.0),
        ("max", 2.0, 1.0),
        ("min", 8.0, 4.0),
    ] {
        let r = rect(&doc, &tree, name);
        near(r.width, w);
        near(r.height, h);
    }
}

#[test]
fn safe_alignment_clamps_overflow_and_rtl_physical_positions_are_distinct() {
    for display in ["flex", "grid"] {
        let (doc, tree) = page(
            &format!(
                "#host{{display:{display};width:50px;justify-content:safe center}}#x{{width:80px;height:10px;flex-shrink:0}}"
            ),
            "<div id=x></div>",
        );
        near(rect(&doc, &tree, "x").x, rect(&doc, &tree, "host").x);
        let (doc, tree) = page(
            &format!(
                "#host{{display:{display};width:100px;direction:rtl;justify-content:left;grid-template-columns:20px}}#x{{width:20px;height:10px}}"
            ),
            "<div id=x></div>",
        );
        near(rect(&doc, &tree, "x").x, rect(&doc, &tree, "host").x);
    }
    let (doc, tree) = page(
        "#x{height:50px;align-content:center}#y{height:10px}",
        "<div id=x><div id=y></div></div>",
    );
    near(rect(&doc, &tree, "y").y, rect(&doc, &tree, "x").y + 20.0);
}

#[test]
fn first_and_last_baseline_groups_align_the_actual_line_baselines() {
    for display in ["flex", "grid"] {
        for baseline in ["first", "last"] {
            let (doc, tree) = page(
                &format!(
                    "#host{{display:{display};align-items:{baseline} baseline;grid-template-columns:80px 80px}}#x,#y{{width:80px}}#y{{font-size:24px;line-height:30px}}"
                ),
                "<div id=x>a<br>b</div><div id=y>a</div>",
            );
            let x = find(tree.root(), id(&doc, "x")).unwrap();
            let y = find(tree.root(), id(&doc, "y")).unwrap();
            let bx = if baseline == "first" {
                x.first_local_baseline()
            } else {
                x.last_local_baseline()
            }
            .unwrap();
            let by = if baseline == "first" {
                y.first_local_baseline()
            } else {
                y.last_local_baseline()
            }
            .unwrap();
            near(bx, by);
        }
    }
}

#[test]
fn sizing_parser_rejects_invalid_combinations_and_diagnoses_indefinite_percentages() {
    for source in [
        "width:min-content max-content",
        "height:fit-content(-2px)",
        "margin:min-content",
        "padding:max-content",
        "aspect-ratio:auto auto",
        "aspect-ratio:-1/2",
        "aspect-ratio:1/2/3",
        "aspect-ratio:1 auto /2",
        "object-fit:cover contain",
        "object-position:left right",
        "object-position:top 10px",
        "object-position:left 2px right 3px",
        "place-items:safe stretch",
    ] {
        let declarations = crate::css::parse_declarations(source).unwrap();
        assert!(!style::metadata::property(declarations[0].property()).is_none());
        let doc = parser::parse(&format!("<!doctype html><div style='{source}'></div>")).unwrap();
        assert!(
            !style::resolve(&doc).stylesheets.diagnostics.is_empty(),
            "{source}"
        );
    }
    let (doc, tree) = page(
        "#x{height:50%;max-height:50%;width:fit-content(calc(20px + 25%))}",
        "<div id=x>aaaa bb</div>",
    );
    assert!(rect(&doc, &tree, "x").height > 0.0);
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("percentage height"))
    );
    assert!(matches!(
        find(tree.root(), id(&doc, "x")).unwrap().style.width,
        Some(Length::FitContent(_))
    ));
}

#[test]
fn object_position_edge_offsets_inherit_compute_and_crop_at_physical_axes() {
    for (source, x, y) in [
        ("left", 0.0, 19.0),
        ("bottom", 18.0, 38.0),
        ("25% 75%", 9.0, 28.5),
        ("right 3px bottom 25%", 33.0, 28.5),
        ("bottom 4px left 10%", 3.6, 34.0),
        ("center right 2px", 34.0, 19.0),
        ("top 2px center", 18.0, 2.0),
    ] {
        let doc=parser::parse(&format!("<!doctype html><style>body{{margin:0}}div{{object-position:{source}}}img{{display:block;width:40px;height:40px;object-fit:none}}</style><div><img id=x></div>")).unwrap();
        let result = style::resolve(&doc);
        assert!(
            result.stylesheets.diagnostics.is_empty(),
            "{source}: {:?}",
            result.stylesheets.diagnostics
        );
        let images = [(id(&doc, "x"), bitmap())].into_iter().collect();
        let mut tree = build_layout_tree_with_resources(&doc, &result.styles, &images);
        tree.layout(300.0, &crate::text::FontContext::bundled());
        let b = find(tree.root(), id(&doc, "x")).unwrap();
        near(b.object_rect().x - b.content_rect.x, x);
        near(b.object_rect().y - b.content_rect.y, y);
    }
    let (doc, tree) = page(
        "#host{--ratio:3/2;--pos:right 1em bottom 10%;object-position:var(--pos)}#x{width:60px;aspect-ratio:var(--ratio);place-self:safe center;object-fit:cover}",
        "<canvas id=x></canvas>",
    );
    near(rect(&doc, &tree, "x").height, 40.0);
    let x = find(tree.root(), id(&doc, "x")).unwrap();
    assert_eq!(x.style.object_position.x.offset, Length::Px(16.0));
    assert_eq!(
        x.style.align_self,
        style::Alignment {
            keyword: style::AlignKeyword::Center,
            safe: true
        }
    );
    assert_eq!(x.style.justify_self, x.style.align_self);
}

#[test]
fn replaced_physical_geometry_and_crop_are_not_rotated_by_vertical_flow() {
    for display in ["block", "inline"] {
        let doc=parser::parse(&format!("<!doctype html><style>body{{margin:0}}main{{writing-mode:vertical-rl;width:100px;height:100px}}img{{display:{display};width:40px;height:20px;object-fit:cover;object-position:right center;background:yellow}}</style><main><img id=x></main>")).unwrap();
        let result = style::resolve(&doc);
        let images = [(id(&doc, "x"), bitmap())].into_iter().collect();
        let mut tree = build_layout_tree_with_resources(&doc, &result.styles, &images);
        tree.layout_viewport(300.0, 200.0, &crate::text::FontContext::bundled());
        let b = find(tree.root(), id(&doc, "x")).unwrap();
        near(b.content_rect.width, 40.0);
        near(b.content_rect.height, 20.0);
        near(b.object_rect().width, 40.0);
        near(b.object_rect().height, 20.0);
        assert_eq!(
            tree.hit_test(b.rect.x + 1.0, b.rect.y + 1.0),
            Some(id(&doc, "x"))
        );
        assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    }
    let (doc, tree) = page(
        "#x{writing-mode:vertical-rl;width:80px;aspect-ratio:2}",
        "<div id=x></div>",
    );
    near(rect(&doc, &tree, "x").height, 40.0);
}

#[test]
fn intrinsic_flex_basis_and_aspect_ratio_grid_items_use_real_minimums() {
    let (doc, tree) = page(
        "#host{display:inline-flex;width:max-content}#x{flex-basis:min-content}",
        "<div id=x>aaaa bb</div>",
    );
    assert!(rect(&doc, &tree, "x").width > 35.0);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let (doc, tree) = page(
        "#host{display:grid;grid-template-columns:100px;grid-template-rows:100px}#x{width:80px;aspect-ratio:2}",
        "<div id=x></div>",
    );
    near(rect(&doc, &tree, "x").height, 40.0);
    let (doc, tree) = page(
        "#x{height:min-content;width:40px;min-height:max-content}",
        "<div id=x>a<br>b</div>",
    );
    near(rect(&doc, &tree, "x").height, 40.0);
}

#[test]
fn replaced_ratio_box_sizing_and_conflicting_min_max_constraints_are_finite() {
    for display in ["block", "inline"] {
        let doc=parser::parse(&format!("<!doctype html><style>body{{margin:0}}img{{display:{display};box-sizing:border-box;width:100px;height:auto;padding:10px;aspect-ratio:2}}#natural{{aspect-ratio:auto 2}}#max{{width:auto;padding:0;max-height:1px}}#conflict{{width:auto;padding:0;min-width:8px;max-height:1px}}</style><img id=x><img id=natural><img id=max><img id=conflict>")).unwrap();
        let styles = style::resolve(&doc).styles;
        let images = ["x", "natural", "max", "conflict"]
            .into_iter()
            .map(|name| (id(&doc, name), bitmap()))
            .collect();
        let mut tree = build_layout_tree_with_resources(&doc, &styles, &images);
        tree.layout(300.0, &crate::text::FontContext::bundled());
        for (name, w, h) in [
            ("x", 80.0, 30.0),
            ("natural", 80.0, 40.0),
            ("max", 2.0, 1.0),
            ("conflict", 8.0, 1.0),
        ] {
            let r = rect(&doc, &tree, name);
            near(r.width, w);
            near(r.height, h);
        }
    }
}

#[test]
fn degenerate_ratios_are_auto_and_invalid_canvas_dimensions_fall_back() {
    for ratio in ["0", "0/1", "1/0", "0/0", "auto 0/0"] {
        let (doc, tree) = page(
            &format!("#x{{width:40px;aspect-ratio:{ratio}}}"),
            "<div id=x></div>",
        );
        near(rect(&doc, &tree, "x").height, 0.0);
        assert!(style::resolve(&doc).stylesheets.diagnostics.is_empty());
    }
    let (doc, tree) = page(
        "canvas{display:block}",
        "<canvas id=x width='-1' height='99999999999999999999'></canvas><canvas id=y width=0 height=0></canvas>",
    );
    near(rect(&doc, &tree, "x").width, 300.0);
    near(rect(&doc, &tree, "x").height, 150.0);
    near(rect(&doc, &tree, "y").width, 0.0);
    near(rect(&doc, &tree, "y").height, 0.0);
}

#[test]
fn root_contents_generates_a_block_and_nested_contents_items_compute_blockification() {
    let doc = parser::parse("<!doctype html><style>html{display:contents}body{margin:0}main{display:grid;grid-template-columns:40px 40px}section,article{display:contents}span{display:inline flow-root;width:20px;height:10px}</style><main><section><article><span id=x></span></article></section><span id=y></span></main>").unwrap();
    let result = style::resolve(&doc);
    let html = doc
        .preorder()
        .find(|&n| {
            doc.node(n)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.tag_name() == "html")
        })
        .unwrap();
    assert_eq!(
        result.styles.get(html).unwrap().display,
        style::Display::Block
    );
    for name in ["x", "y"] {
        assert_eq!(
            result.styles.get(id(&doc, name)).unwrap().display,
            style::Display::FlowRoot
        );
    }
    let mut tree = build_layout_tree_with_styles(&doc, &result.styles);
    tree.layout(300.0, &crate::text::FontContext::bundled());
    near(rect(&doc, &tree, "y").x - rect(&doc, &tree, "x").x, 40.0);
    assert!(find(tree.root(), id(&doc, "x")).unwrap().is_grid_item);
}

#[test]
fn inline_list_items_paint_markers_on_the_retained_inline_baseline() {
    for display in ["inline list-item", "inline flow-root list-item"] {
        let (doc, tree) = page(
            &format!("#x{{display:{display}}}#x::marker{{content:'M';color:red}}"),
            "a<span id=x>b</span>c",
        );
        let x = find(tree.root(), id(&doc, "x")).unwrap();
        let marker = x.list_marker.as_ref().unwrap();
        assert_eq!(marker.text, "M");
        let baseline = if x.atomic_inline() {
            x.first_local_baseline().unwrap()
        } else {
            x.inline_fragments.first().unwrap().baseline
        };
        near(marker.baseline, baseline);
        assert!(crate::paint::build_display_list(&tree, 448.0, 400.0, false).iter().any(|i| matches!(i, crate::paint::DisplayItem::Text { text, color: 0xff0000, .. } if text == "M")));
    }
}

#[test]
fn block_ratio_automatic_minimum_contains_content_unless_explicit_or_scrollable() {
    let (doc, tree) = page(
        "#x{height:40px;aspect-ratio:2}#wide{height:40px;aspect-ratio:2}#zero{height:40px;aspect-ratio:2;min-width:0}#tall,#scroll,#small,#cap{width:20px;aspect-ratio:1}#scroll{overflow:auto}#small{min-height:0}#cap{max-height:30px}span{display:inline-block;width:100px;height:10px}",
        "<div id=x></div><div id=wide><span></span></div><div id=zero><span></span></div><div id=tall>a<br>b<br>c</div><div id=scroll>a<br>b<br>c</div><div id=small>a<br>b<br>c</div><div id=cap>a<br>b<br>c</div>",
    );
    near(rect(&doc, &tree, "x").width, 80.0);
    near(rect(&doc, &tree, "wide").width, 100.0);
    near(rect(&doc, &tree, "zero").width, 80.0);
    near(rect(&doc, &tree, "tall").height, 60.0);
    near(rect(&doc, &tree, "scroll").height, 20.0);
    near(rect(&doc, &tree, "small").height, 20.0);
    near(rect(&doc, &tree, "cap").height, 30.0);
}

#[test]
fn fallback_replaced_dimensions_do_not_invent_a_natural_aspect_ratio() {
    for display in ["block", "inline"] {
        let (doc, tree) = page(
            &format!(
                "video,iframe{{display:{display};width:80px;height:auto}}#css{{aspect-ratio:4}}#auto{{aspect-ratio:auto 4;box-sizing:border-box;padding:10px}}"
            ),
            "<video id=video></video><iframe id=iframe></iframe><iframe id=css></iframe><iframe id=auto></iframe>",
        );
        for name in ["video", "iframe"] {
            near(rect(&doc, &tree, name).width, 80.0);
            near(rect(&doc, &tree, name).height, 150.0);
        }
        near(rect(&doc, &tree, "css").height, 20.0);
        near(rect(&doc, &tree, "auto").height, 15.0);
        let (doc, tree) = page(
            &format!("iframe{{display:{display};max-width:100px}}"),
            "<iframe id=x></iframe>",
        );
        near(rect(&doc, &tree, "x").width, 100.0);
        near(rect(&doc, &tree, "x").height, 150.0);
    }
}

#[test]
fn replaced_intrinsic_keywords_measure_natural_dimensions_before_resolving_their_cache() {
    for display in ["block", "inline", "inline-block"] {
        let doc = parser::parse(&format!("<!doctype html><style>body{{margin:0}}img{{display:{display};width:min-content;height:max-content}}#ratio{{width:auto;height:auto;aspect-ratio:4}}#border{{width:auto;height:auto;aspect-ratio:2;box-sizing:border-box;padding:1px}}</style><img id=x><img id=ratio><img id=border>")).unwrap();
        let styles = style::resolve(&doc).styles;
        let images = ["x", "ratio", "border"]
            .into_iter()
            .map(|name| (id(&doc, name), bitmap()))
            .collect();
        let mut tree = build_layout_tree_with_resources(&doc, &styles, &images);
        tree.layout(300.0, &crate::text::FontContext::bundled());
        near(rect(&doc, &tree, "x").width, 4.0);
        near(rect(&doc, &tree, "x").height, 2.0);
        near(rect(&doc, &tree, "ratio").height, 1.0);
        near(rect(&doc, &tree, "border").width, 4.0);
        near(rect(&doc, &tree, "border").height, 1.0);
    }
}

#[test]
fn fit_content_limits_stretch_the_margin_box_and_ratio_native_minimums_are_physical() {
    let (doc, tree) = page(
        "#x{width:fit-content(80px);padding:5px;margin:0 10px}#v{writing-mode:vertical-rl;height:20px;aspect-ratio:1}#small{writing-mode:vertical-rl;height:20px;aspect-ratio:1;min-width:0}#auto{width:80px;aspect-ratio:auto 2;box-sizing:border-box;padding:10px}",
        "<div id=x>aa bb cc dd ee</div><div id=v>a<br>b<br>c</div><div id=small>a<br>b<br>c</div><div id=auto></div>",
    );
    near(rect(&doc, &tree, "x").width, 50.0);
    near(rect(&doc, &tree, "v").width, 60.0);
    near(rect(&doc, &tree, "small").width, 20.0);
    near(rect(&doc, &tree, "auto").height, 30.0);
}

#[test]
fn block_self_alignment_inherits_items_uses_intrinsic_width_and_respects_auto_margins() {
    let (doc, tree) = page(
        "#host{justify-items:center}#x{width:40px;height:10px}#y{justify-self:end}#safe{width:240px;justify-self:safe center}#auto{width:40px;margin-left:auto;justify-self:start}#rtl{width:100px;direction:rtl;justify-items:left}#left{width:20px;height:10px}#self{width:20px;direction:ltr;justify-self:self-start}",
        "<div id=x></div><div id=y>aaaa</div><div id=safe></div><div id=auto></div><div id=rtl><div id=left></div><div id=self></div></div>",
    );
    near(rect(&doc, &tree, "x").x - rect(&doc, &tree, "host").x, 80.0);
    let y = rect(&doc, &tree, "y");
    near(y.x + y.width, rect(&doc, &tree, "host").x + 200.0);
    near(
        y.width,
        crate::text::FontContext::bundled()
            .shape(find(tree.root(), id(&doc, "y")).unwrap().style(), "aaaa")
            .width,
    );
    near(rect(&doc, &tree, "safe").x, rect(&doc, &tree, "host").x);
    near(
        rect(&doc, &tree, "auto").x - rect(&doc, &tree, "host").x,
        160.0,
    );
    near(rect(&doc, &tree, "left").x, rect(&doc, &tree, "rtl").x);
    near(rect(&doc, &tree, "self").x, rect(&doc, &tree, "rtl").x);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let (_, tree) = page(
        "#x{writing-mode:vertical-rl;justify-self:center}",
        "<div id=x>a</div>",
    );
    assert!(
        tree.diagnostics()
            .iter()
            .any(|d| d.message.contains("native block justify-self"))
    );
}
