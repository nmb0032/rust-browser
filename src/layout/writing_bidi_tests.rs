use super::*;
use crate::{
    dom::{Document, NodeId},
    html::parser,
    paint, style,
    text::{FontContext, GlyphRotation},
};

fn fonts() -> FontContext {
    let mut db = fontdb::Database::new();
    for bytes in [
        include_bytes!("../../assets/fonts/noto-sans/NotoSans.ttf").as_slice(),
        include_bytes!("../../assets/fonts/noto-sans/NotoSansArabic-Regular.ttf").as_slice(),
        include_bytes!("../../assets/fonts/noto-sans/NotoSansHebrew-Regular.ttf").as_slice(),
        include_bytes!("../../assets/fonts/noto-sans/NotoSansSC-fixture.ttf").as_slice(),
        include_bytes!("../../assets/fonts/noto-sans/NotoSansJP-fixture.ttf").as_slice(),
    ] {
        db.load_font_data(bytes.to_vec());
    }
    db.set_sans_serif_family("Noto Sans");
    FontContext::from_database(db)
}
fn layout(css: &str, text: &str) -> (Document, LayoutTree, FontContext) {
    let doc = parser::parse(&format!("<!doctype html><style>body{{margin:0}}#host{{width:240px;font:16px/16px 'Noto Sans JP','Noto Sans SC','Noto Sans Arabic','Noto Sans Hebrew','Noto Sans'}}{css}</style><div id=host>{text}</div>")).unwrap();
    let fonts = fonts();
    let styles = style::resolve_with_fonts(
        &doc,
        style::collect_stylesheets(&doc),
        Default::default(),
        &Default::default(),
        &Default::default(),
        &fonts,
    );
    assert!(
        styles.stylesheets.diagnostics.is_empty(),
        "{:?}",
        styles.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&doc, &styles.styles);
    tree.layout_viewport(400.0, 300.0, &fonts);
    (doc, tree, fonts)
}
fn node(doc: &Document, name: &str) -> NodeId {
    doc.preorder()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}
fn find(b: &LayoutBox, id: NodeId) -> &LayoutBox {
    if b.node_id == Some(id) {
        b
    } else {
        b.children
            .iter()
            .find_map(|c| {
                fn search(b: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
                    if b.node_id == Some(id) {
                        Some(b)
                    } else {
                        b.children.iter().find_map(|c| search(c, id))
                    }
                }
                search(c, id)
            })
            .unwrap()
    }
}
fn text(b: &LayoutBox) -> Vec<&TextFragment> {
    b.text_fragments
        .iter()
        .chain(b.children.iter().flat_map(|c| text(c)))
        .collect()
}
fn descendant(doc: &Document, hit: Option<NodeId>, ancestor: NodeId) -> bool {
    let mut hit = hit;
    while let Some(id) = hit {
        if id == ancestor {
            return true;
        }
        hit = doc.node(id).and_then(|n| n.parent());
    }
    false
}

#[test]
fn uax9_paragraph_numbers_brackets_and_explicit_isolation_are_not_character_reversal() {
    let source = "A אבג 123 (xy) Z";
    let info = unicode_bidi::BidiInfo::new(source, Some(unicode_bidi::Level::ltr()));
    assert_eq!(
        info.reorder_line(&info.paragraphs[0], 0..source.len()),
        "A 123 גבא (xy) Z"
    );
    let (doc, tree, _) = layout("#host{direction:ltr}", source);
    let mut glyphs: Vec<_> = text(find(tree.root(), node(&doc, "host")))
        .iter()
        .flat_map(|f| {
            f.glyphs
                .glyphs
                .iter()
                .map(move |g| (f.x + g.x, g.source[g.cluster..g.cluster_end].to_string()))
        })
        .collect();
    glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let visual: String = glyphs.into_iter().map(|g| g.1).collect();
    assert_eq!(visual, "A123גבא(xy)Z");
    let (doc, tree, _) = layout("", "A <bdi id=island dir=rtl>אבג 123</bdi> Z");
    let island = find(tree.root(), node(&doc, "island"));
    assert_eq!(island.style.unicode_bidi, style::UnicodeBidi::Isolate);
    let mut glyphs: Vec<_> = text(island)
        .iter()
        .flat_map(|f| {
            f.glyphs
                .glyphs
                .iter()
                .map(move |g| (f.x + g.x, g.source[g.cluster..g.cluster_end].to_string()))
        })
        .collect();
    glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(
        glyphs.into_iter().map(|g| g.1).collect::<String>(),
        "123גבא"
    );
}

#[test]
fn arabic_visual_positions_match_the_shaper_without_double_rtl_reversal() {
    let (doc, tree, fonts) = layout("#host{direction:rtl}", "<a id=link href=/x>سلام</a>");
    let host = find(tree.root(), node(&doc, "host"));
    let link = find(tree.root(), node(&doc, "link"));
    let expected = fonts.shape_resolved(&host.style, "سلام", &[], "", "", Some(true));
    let mut actual: Vec<_> = text(link)
        .iter()
        .flat_map(|f| f.glyphs.glyphs.iter().map(move |g| (f.x + g.x, g.id)))
        .collect();
    actual.sort_by(|a, b| a.0.total_cmp(&b.0));
    let origin = host.line_boxes[0].content_rect.x;
    assert_eq!(actual.len(), expected.glyphs.len());
    for ((x, id), glyph) in actual.iter().zip(&expected.glyphs) {
        assert_eq!(*id, glyph.id);
        assert!(
            (*x - origin - glyph.x).abs() < 0.001,
            "{actual:?} {expected:?}"
        );
        assert_ne!(glyph.id, 0);
    }
    assert!(text(link).iter().any(|f| f.ink_rect.width > 0.0));
    let ink = text(link)[0].ink_rect;
    assert!(descendant(
        &doc,
        tree.hit_test(ink.x + ink.width * 0.5, ink.y + ink.height * 0.5),
        node(&doc, "link")
    ));
}

#[test]
fn html_auto_bdo_and_css_override_use_paragraph_semantics() {
    let (doc, tree, _) = layout(
        "",
        "<span id=auto dir=auto><bdi>abc</bdi>אבג 2</span><bdo id=override dir=rtl>abc123</bdo>",
    );
    assert_eq!(
        find(tree.root(), node(&doc, "auto")).style.direction,
        style::Direction::Rtl
    );
    let override_box = find(tree.root(), node(&doc, "override"));
    assert_eq!(
        override_box.style.unicode_bidi,
        style::UnicodeBidi::IsolateOverride
    );
    let mut positions: Vec<_> = text(override_box)
        .iter()
        .flat_map(|f| {
            f.glyphs
                .glyphs
                .iter()
                .map(move |g| (f.x + g.x, g.source[g.cluster..g.cluster_end].to_owned()))
        })
        .collect();
    positions.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(
        positions.into_iter().map(|p| p.1).collect::<String>(),
        "321cba"
    );
}

#[test]
fn vertical_columns_use_inline_height_and_real_vertical_opentype_metrics() {
    for mode in ["vertical-rl", "vertical-lr"] {
        let (doc, tree, _) = layout(
            &format!("#host{{writing-mode:{mode};width:64px;height:32px}}"),
            "水水水水",
        );
        let host = find(tree.root(), node(&doc, "host"));
        assert_eq!(host.line_boxes.len(), 2);
        assert_eq!(host.line_boxes[0].rect.height, 32.0);
        assert_eq!(host.line_boxes[0].rect.width, 16.0);
        assert_eq!(
            host.line_boxes[1].rect.x - host.line_boxes[0].rect.x,
            if mode == "vertical-rl" { -16.0 } else { 16.0 }
        );
        assert!(text(host).iter().all(|f| {
            f.glyphs
                .glyphs
                .iter()
                .all(|g| g.upright && g.rotation == GlyphRotation::None && g.id != 0)
        }));
        for f in text(host) {
            assert!(f.ink_rect.width > 0.0 && f.ink_rect.height > 0.0);
            assert!(
                f.ink_rect.x >= host.content_rect.x - 1.0
                    && f.ink_rect.x + f.ink_rect.width <= host.content_rect.x + 64.0 + 1.0
            );
        }
    }
}

#[test]
fn vertical_mixed_upright_sideways_and_nested_orthogonal_blocks_are_distinct() {
    for (orientation, latin_upright) in [("mixed", false), ("upright", true), ("sideways", false)] {
        let (doc, tree, _) = layout(
            &format!(
                "#host{{writing-mode:vertical-rl;text-orientation:{orientation};height:100px;width:100px}}#inner{{writing-mode:horizontal-tb;width:32px}}"
            ),
            "水A<div id=inner>AA</div><div id=next>水</div>",
        );
        let host = find(tree.root(), node(&doc, "host"));
        let glyphs: Vec<_> = text(&host.children[0])
            .iter()
            .flat_map(|f| &f.glyphs.glyphs)
            .collect();
        let latin = glyphs
            .iter()
            .find(|g| &g.source[g.cluster..g.cluster_end] == "A")
            .unwrap();
        assert_eq!(latin.upright, latin_upright);
        assert_eq!(
            latin.rotation,
            if latin_upright {
                GlyphRotation::None
            } else {
                GlyphRotation::Clockwise
            }
        );
        let inner = find(tree.root(), node(&doc, "inner"));
        assert!(text(inner).iter().all(|f| {
            f.glyphs
                .glyphs
                .iter()
                .all(|g| g.rotation == GlyphRotation::None)
        }));
        assert!(find(tree.root(), node(&doc, "next")).rect.x < inner.rect.x);
    }
}

#[test]
fn physical_vertical_ink_matches_scaled_raster_hit_and_relayout() {
    let (doc, mut tree, fonts) = layout(
        "#host{writing-mode:vertical-rl;width:80px;height:80px;overflow:hidden}a{color:red}",
        "<a id=link href=/x>水A水</a>",
    );
    let original = tree.clone();
    for scale in [1.0, 1.5, 2.0] {
        let list = paint::build_display_list(&tree, 400.0, 300.0, false);
        let w = (400.0 * scale) as usize;
        let h = (300.0 * scale) as usize;
        let mut pixels = vec![0xffffff; w * h];
        paint::rasterize_display_list(&list, &fonts, &mut pixels, w, h, scale, 0.0);
        let link = node(&doc, "link");
        let mut ink = 0;
        for (i, p) in pixels.iter().enumerate() {
            if *p != 0xffffff {
                ink += 1;
                assert!(descendant(
                    &doc,
                    tree.hit_test(
                        (i % w) as f32 / scale as f32 + 0.5 / scale as f32,
                        (i / w) as f32 / scale as f32 + 0.5 / scale as f32
                    ),
                    link
                ));
            }
        }
        assert!(ink > 20);
    }
    tree.layout_viewport(400.0, 300.0, &fonts);
    assert_eq!(tree.root, original.root);
}

#[test]
fn vertical_flex_and_grid_allocate_in_logical_axes_but_shape_native_children() {
    for css in [
        "#host{display:flex;flex-direction:row}#a,#b{flex:1}",
        "#host{display:grid;grid-template-columns:1fr 1fr;grid-template-rows:1fr}",
    ] {
        let (doc, tree, _) = layout(
            &format!("#host{{writing-mode:vertical-rl;width:80px;height:100px}}{css}"),
            "<div id=a>水水</div><div id=b>水水</div>",
        );
        let a = find(tree.root(), node(&doc, "a"));
        let b = find(tree.root(), node(&doc, "b"));
        assert!((a.rect.height - 50.0).abs() < 0.001, "{a:?}");
        assert!((b.rect.height - 50.0).abs() < 0.001);
        assert!((b.rect.y - a.rect.y - 50.0).abs() < 0.001);
        assert!((a.rect.width - 80.0).abs() < 0.001);
        assert!(text(a).iter().all(|f| {
            f.glyphs
                .glyphs
                .iter()
                .all(|g| g.upright && g.rotation == GlyphRotation::None)
        }));
        assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    }
}

#[test]
fn vertical_floats_exclude_inline_space_and_clear_advances_block_flow() {
    let (doc, tree, _) = layout(
        "#host{writing-mode:vertical-rl;width:100px;height:64px}#float{float:left;width:32px;height:32px}#clear{clear:both}",
        "<span id=float>水</span>水水水水<div id=clear>水</div>",
    );
    let host = find(tree.root(), node(&doc, "host"));
    let float = find(tree.root(), node(&doc, "float"));
    assert_eq!(float.rect.width, 32.0);
    assert_eq!(float.rect.height, 32.0);
    let line = host
        .children
        .iter()
        .find_map(|c| c.line_boxes.first())
        .unwrap();
    assert_eq!(line.rect.height, 32.0);
    let clear = find(tree.root(), node(&doc, "clear"));
    assert!(
        clear.rect.x + clear.rect.width <= float.rect.x + 0.001,
        "{clear:?} {float:?}"
    );
}

#[test]
fn vertical_table_rows_and_cells_use_block_and_inline_axes() {
    for collapse in ["separate", "collapse"] {
        let (doc, tree, _) = layout(
            &format!(
                "#host{{writing-mode:vertical-rl;width:80px;height:64px}}table{{border-collapse:{collapse};border-spacing:0}}td{{border:1px solid;padding:0}}"
            ),
            "<table id=table><tr><td id=a>水</td><td id=b>水</td></tr><tr><td id=c>水</td><td>水</td></tr></table>",
        );
        let a = find(tree.root(), node(&doc, "a"));
        let b = find(tree.root(), node(&doc, "b"));
        let c = find(tree.root(), node(&doc, "c"));
        assert!(b.rect.y > a.rect.y, "{a:?} {b:?}");
        assert!(c.rect.x < a.rect.x, "{a:?} {c:?}");
        assert!(
            text(a)
                .iter()
                .all(|f| f.glyphs.glyphs.iter().all(|g| g.upright && g.id != 0))
        );
        assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    }
}

fn visual_text(b: &LayoutBox, line: usize, vertical: bool) -> String {
    let mut glyphs: Vec<_> = text(b)
        .into_iter()
        .filter(|f| f.line_index == line)
        .flat_map(|f| {
            f.glyphs.glyphs.iter().map(move |g| {
                (
                    if vertical {
                        f.baseline - g.y
                    } else {
                        f.x + g.x
                    },
                    g.source[g.cluster..g.cluster_end].to_owned(),
                )
            })
        })
        .collect();
    glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
    glyphs
        .into_iter()
        .map(|g| g.1)
        .collect::<String>()
        .chars()
        .filter(|c| !c.is_whitespace() && !crate::text::invisible(*c))
        .collect()
}

#[test]
fn bidi_reference_vectors_controls_neutrals_and_hebrew_have_real_glyph_ink() {
    for (source, rtl) in [
        ("אבג (12) xyz", true),
        ("a \u{202b}אב 12\u{202c} z", false),
        ("a \u{2067}אב \u{2066}xy 12\u{2069} גד\u{2069} z", false),
        ("א \u{202d}ab12\u{202c} ב", true),
        ("a \u{2068}אב 12\u{2069} z", false),
        ("אב 12 [xy] גד", true),
        ("a \u{200f}12 אב z", false),
    ] {
        let base = if rtl {
            unicode_bidi::Level::rtl()
        } else {
            unicode_bidi::Level::ltr()
        };
        let info = unicode_bidi::BidiInfo::new(source, Some(base));
        let reference = info
            .reorder_line(&info.paragraphs[0], 0..source.len())
            .chars()
            .filter(|c| !c.is_whitespace() && !crate::text::invisible(*c))
            .collect::<String>();
        let (doc, tree, _) = layout(
            &format!("#host{{direction:{}}}", if rtl { "rtl" } else { "ltr" }),
            source,
        );
        let host = find(tree.root(), node(&doc, "host"));
        assert_eq!(visual_text(host, 0, false), reference, "{source:?}");
        assert!(
            text(host)
                .iter()
                .flat_map(|f| &f.glyphs.glyphs)
                .all(|g| g.id != 0),
            "{source:?}"
        );
        assert!(text(host).iter().any(|f| f.ink_rect.width > 0.0));
    }
}

#[test]
fn plaintext_paragraph_bases_and_wrapped_l1_trailing_tabs_reset_per_line() {
    let (doc, tree, _) = layout(
        "#host{unicode-bidi:plaintext;white-space:pre;direction:ltr}",
        "אב 12\t\nxy 34",
    );
    let host = find(tree.root(), node(&doc, "host"));
    assert_eq!(host.line_boxes.len(), 2);
    assert_eq!(visual_text(host, 0, false), "12בא");
    assert_eq!(visual_text(host, 1, false), "xy34");
    assert!(host.line_boxes[0].content_rect.x > host.line_boxes[1].content_rect.x);
    let (doc, tree, _) = layout(
        "#host{direction:rtl;white-space:pre-wrap;width:48px}",
        "אב 12   גד 34",
    );
    let host = find(tree.root(), node(&doc, "host"));
    assert!(host.line_boxes.len() >= 2);
    let mut retained = String::new();
    for line in 0..host.line_boxes.len() {
        let visual = visual_text(host, line, false);
        assert!(!visual.contains("21") && !visual.contains("43"), "{visual}");
        retained.push_str(&visual);
    }
    assert!(retained.contains("12") && retained.contains("34"));
}

#[test]
fn vertical_bidi_sideways_modes_orthogonal_inlines_and_replaced_sizes_are_native() {
    for mode in ["vertical-rl", "vertical-lr", "sideways-rl", "sideways-lr"] {
        let (doc, tree, _) = layout(
            &format!(
                "#host{{writing-mode:{mode};width:100px;height:150px}}#orth{{writing-mode:horizontal-tb;width:32px;height:20px}}img{{width:40px;height:20px}}"
            ),
            "水<span id=orth>AB</span><img id=image alt=''>水",
        );
        let orth = find(tree.root(), node(&doc, "orth"));
        assert!(orth.atomic_inline());
        assert_eq!((orth.rect.width, orth.rect.height), (32.0, 20.0));
        assert!(
            text(orth)
                .iter()
                .flat_map(|f| &f.glyphs.glyphs)
                .all(|g| g.rotation == GlyphRotation::None)
        );
        let image = find(tree.root(), node(&doc, "image"));
        assert_eq!((image.rect.width, image.rect.height), (40.0, 20.0));
        assert!(image.rect.y >= 0.0 && image.rect.y + image.rect.height <= 150.0);
    }
    let (doc, tree, _) = layout(
        "#orth{writing-mode:vertical-rl;width:32px;height:32px}",
        "A<span id=orth>水水</span>B",
    );
    let orth = find(tree.root(), node(&doc, "orth"));
    assert!(orth.atomic_inline());
    assert_eq!((orth.rect.width, orth.rect.height), (32.0, 32.0));
    assert!(
        text(orth)
            .iter()
            .flat_map(|f| &f.glyphs.glyphs)
            .all(|g| g.upright && g.rotation == GlyphRotation::None)
    );
    let (doc, tree, _) = layout(
        "#host{writing-mode:vertical-lr;direction:rtl;text-orientation:sideways;width:32px;height:150px}",
        "אב 12",
    );
    assert_eq!(
        visual_text(find(tree.root(), node(&doc, "host")), 0, true),
        "12בא"
    );
}

#[test]
fn vertical_logical_positioning_collapsed_sibling_margins_and_negative_scroll_range() {
    let (doc, mut tree, fonts) = layout(
        "#host{writing-mode:vertical-rl;position:relative;width:32px;height:32px;overflow-block:scroll;overflow-inline:hidden}#a,#b{width:32px;height:32px}#a{margin-block-end:12px}#b{margin-block-start:8px}#abs{position:absolute;inset-block-start:2px;inset-inline-start:3px;width:8px;height:8px}#fixed{position:fixed;inset-block-start:5px;inset-inline-start:7px;width:8px;height:8px}",
        "<div id=a>水水</div><div id=b>水水</div><div id=abs></div><div id=fixed></div>",
    );
    let host = node(&doc, "host");
    let a = find(tree.root(), node(&doc, "a"));
    let b = find(tree.root(), node(&doc, "b"));
    assert!(
        (a.rect.x - b.rect.x - b.rect.width - 12.0).abs() < 0.001,
        "{a:?} {b:?}"
    );
    let absolute = find(tree.root(), node(&doc, "abs"));
    let host_rect = find(tree.root(), host).rect;
    assert_eq!(
        (absolute.rect.x - host_rect.x, absolute.rect.y - host_rect.y),
        (22.0, 3.0)
    );
    let fixed = find(tree.root(), node(&doc, "fixed"));
    assert_eq!((fixed.rect.x, fixed.rect.y), (387.0, 7.0));
    assert_eq!(fixed.coordinate_space, CoordinateSpace::Viewport);
    let container = tree.scroll_container(host).unwrap();
    assert!(container.range.min.x <= -44.0, "{container:?}");
    assert_eq!(container.range.max.x, 0.0);
    assert!(tree.set_scroll_offset(host, ScrollOffset { x: -44.0, y: 0.0 }));
    tree.layout_viewport(400.0, 300.0, &fonts);
    assert_eq!(tree.scroll_offset(host).x, -44.0);
    let host_box = find(tree.root(), host);
    assert!(tree.scroll_wheel(
        host_box.rect.x + 16.0,
        host_box.rect.y + 16.0,
        ScrollOffset { x: 20.0, y: 0.0 }
    ));
    assert_eq!(tree.scroll_offset(host).x, -24.0);
}

#[test]
fn japanese_vertical_glyphs_and_positions_match_independent_opentype_ttb_output() {
    let source = "日本語、「水」。";
    let raw = rustybuzz::Face::from_slice(
        include_bytes!("../../assets/fonts/noto-sans/NotoSansJP-fixture.ttf"),
        0,
    )
    .unwrap();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(source);
    buffer.guess_segment_properties();
    buffer.set_direction(rustybuzz::Direction::TopToBottom);
    let reference = rustybuzz::shape(&raw, &[], buffer);
    assert!(
        reference.glyph_infos().iter().any(|g| g.glyph_id
            != u32::from(
                raw.glyph_index(source[g.cluster as usize..].chars().next().unwrap())
                    .unwrap()
                    .0
            )),
        "fixture must exercise vert/vrt2"
    );
    let (doc, tree, _) = layout(
        "#host{writing-mode:vertical-rl;text-orientation:upright;width:32px;height:200px}",
        source,
    );
    let host = find(tree.root(), node(&doc, "host"));
    let mut actual: Vec<_> = text(host)
        .into_iter()
        .flat_map(|f| {
            f.glyphs
                .glyphs
                .iter()
                .map(move |g| (f.x + g.x, f.baseline - g.y, g.id))
        })
        .collect();
    actual.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(actual.len(), reference.glyph_infos().len());
    let scale = 16.0 / raw.units_per_em() as f32;
    let mut pen = 0.0;
    for ((x, y, id), (info, pos)) in actual.into_iter().zip(
        reference
            .glyph_infos()
            .iter()
            .zip(reference.glyph_positions()),
    ) {
        assert_eq!(u32::from(id), info.glyph_id);
        assert_ne!(id, 0);
        assert!(
            (x - host.line_boxes[0].baseline - pos.x_offset as f32 * scale).abs() < 0.001,
            "x={x} baseline={} offset={} id={id}",
            host.line_boxes[0].baseline,
            pos.x_offset as f32 * scale
        );
        assert!((y - host.content_rect.y - pen + pos.y_offset as f32 * scale).abs() < 0.001);
        pen -= pos.y_advance as f32 * scale;
    }
}

#[test]
fn css_embedding_reopens_at_paragraph_breaks_and_anonymous_runs_keep_block_override() {
    let (doc, tree, _) = layout(
        "#host{white-space:pre}",
        "# <bdo id=override dir=rtl>ab12\ncd34</bdo> !",
    );
    let host = find(tree.root(), node(&doc, "host"));
    assert_eq!(visual_text(host, 0, false), "#21ba");
    assert_eq!(visual_text(host, 1, false), "43dc!");
    let (doc, tree, _) = layout(
        "#host{unicode-bidi:bidi-override;direction:rtl}",
        "ab12<div>xy34</div>cd56",
    );
    let host = find(tree.root(), node(&doc, "host"));
    assert!(host.children[0].is_anonymous());
    assert_eq!(visual_text(&host.children[0], 0, false), "21ba");
    assert_eq!(visual_text(host.children.last().unwrap(), 0, false), "65dc");
}

#[test]
fn arabic_joining_context_survives_color_owner_and_font_size_boundaries() {
    let (doc, tree, fonts) = layout(
        "#host{direction:rtl}#first{color:red}#second{font-size:20px;color:blue}",
        "<a id=first href=/a>س</a><a id=second href=/b>لام</a>",
    );
    let host = find(tree.root(), node(&doc, "host"));
    let first = find(tree.root(), node(&doc, "first"));
    let second = find(tree.root(), node(&doc, "second"));
    let reference = fonts.shape_resolved(&host.style, "سلام", &[], "", "", Some(true));
    let expected: Vec<_> = reference.glyphs.iter().map(|g| g.id).collect();
    let mut actual: Vec<_> = text(host)
        .into_iter()
        .flat_map(|f| f.glyphs.glyphs.iter().map(move |g| (f.x + g.x, g.id)))
        .collect();
    actual.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(
        actual.into_iter().map(|g| g.1).collect::<Vec<_>>(),
        expected
    );
    assert!(
        text(first)
            .iter()
            .flat_map(|f| &f.glyphs.glyphs)
            .all(|g| g.size == 16.0)
    );
    assert!(
        text(second)
            .iter()
            .flat_map(|f| &f.glyphs.glyphs)
            .all(|g| g.size == 20.0)
    );
    for name in ["first", "second"] {
        let b = find(tree.root(), node(&doc, name));
        let ink = text(b)[0].ink_rect;
        assert!(descendant(
            &doc,
            tree.hit_test(ink.x + ink.width * 0.5, ink.y + ink.height * 0.5),
            node(&doc, name)
        ));
    }
}

#[test]
fn vertical_percentage_decorations_use_containing_inline_axis_and_sizes_stay_physical() {
    let (doc, tree, _) = layout(
        "#host{writing-mode:vertical-rl;width:80px;height:100px}#a{width:30px;height:50%;max-inline-size:30px;box-sizing:border-box;border:1px solid;padding-inline:10%;margin-block:10%}#orth{writing-mode:horizontal-tb;width:20px;padding:10%}",
        "<div id=a>水</div><div id=orth>A</div>",
    );
    let a = find(tree.root(), node(&doc, "a"));
    assert_eq!((a.padding.top, a.padding.bottom), (10.0, 10.0));
    assert_eq!((a.margin.left, a.margin.right), (10.0, 10.0));
    assert_eq!((a.rect.width, a.rect.height), (30.0, 30.0));
    assert_eq!(a.content_rect.height, 8.0);
    let orth = find(tree.root(), node(&doc, "orth"));
    assert_eq!(
        orth.padding,
        crate::css::values::Edges {
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0
        }
    );
}

#[test]
fn unrepresentable_vertical_axis_sums_diagnose_and_keep_fragment_geometry_finite() {
    let (_, tree, _) = layout(
        "#host{writing-mode:vertical-rl;height:64px}#host>div{width:32px;margin-block:1e38px}",
        "<div>水</div><div>水</div><div>水</div>",
    );
    assert!(!tree.diagnostics().is_empty());
    fn check(b: &LayoutBox) {
        for r in [b.rect, b.content_rect]
            .into_iter()
            .chain(b.line_boxes.iter().flat_map(|l| [l.rect, l.content_rect]))
            .chain(b.text_fragments.iter().map(|f| f.ink_rect))
            .chain(
                b.inline_fragments
                    .iter()
                    .flat_map(|f| [f.rect, f.content_rect]),
            )
        {
            assert!(
                [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()),
                "{r:?}"
            );
        }
        assert!(b.line_boxes.iter().all(|l| l.baseline.is_finite()));
        assert!(
            b.text_fragments
                .iter()
                .all(|f| f.x.is_finite() && f.baseline.is_finite())
        );
        for c in &b.children {
            check(c);
        }
    }
    check(tree.root());
}
