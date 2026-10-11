use super::*;
use crate::{dom::Document, html::parser, paint, style, text::FontContext};
use style::typography::{Keyword as K, Property as P};
#[path = "typography_regression_tests.rs"]
mod regression;

fn fonts() -> FontContext {
    let mut database = fontdb::Database::new();
    database.load_font_data(
        include_bytes!("../../assets/fonts/typography/GeometryVariable.ttf").to_vec(),
    );
    database
        .load_font_data(include_bytes!("../../assets/fonts/typography/GeometryColor.ttf").to_vec());
    database.load_font_data(
        include_bytes!("../../assets/fonts/typography/GeometryBitmap.ttf").to_vec(),
    );
    database
        .load_font_data(include_bytes!("../../assets/fonts/typography/GeometryCFF2.otf").to_vec());
    FontContext::from_database(database)
}

fn page(css: &str, html: &str) -> (Document, LayoutTree, FontContext) {
    let document = parser::parse(&format!("<!doctype html><style>body{{margin:0}}#host{{width:200px;font:20px/24px 'Noto Sans'}}{css}</style><main id=host>{html}</main>")).unwrap();
    let fonts = fonts();
    let styles = style::resolve_with_fonts(
        &document,
        style::collect_stylesheets(&document),
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
    let mut tree = build_layout_tree_with_styles(&document, &styles.styles);
    tree.layout_viewport(448.0, 400.0, &fonts);
    (document, tree, fonts)
}

fn find<'a>(doc: &Document, tree: &'a LayoutTree, name: &str) -> &'a LayoutBox {
    let id = doc
        .preorder()
        .find(|&id| {
            doc.node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap();
    fn visit(b: &LayoutBox, id: crate::dom::NodeId) -> Option<&LayoutBox> {
        if b.node_id == Some(id) && b.pseudo.is_none() {
            return Some(b);
        }
        b.children.iter().find_map(|b| visit(b, id))
    }
    visit(tree.root(), id).unwrap()
}
fn text(b: &LayoutBox) -> String {
    b.text_fragments
        .iter()
        .map(|f| f.text.clone())
        .chain(b.children.iter().map(text))
        .collect()
}
fn fragments(b: &LayoutBox) -> Vec<&TextFragment> {
    b.text_fragments
        .iter()
        .chain(b.children.iter().flat_map(fragments))
        .collect()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.05, "{a} != {b}");
}

#[test]
fn word_break_and_line_break_use_whole_extended_graphemes() {
    for (rule, source, expected) in [
        ("word-break:normal", "AAAA", 1),
        ("word-break:break-all", "AAAA", 2),
        ("line-break:anywhere", "AAAA", 2),
        ("word-break:break-all", "A\u{0301}", 1),
    ] {
        let (doc, tree, _) = page(
            &format!("#x{{width:24px;font-family:'Geometry Variable';{rule}}}"),
            &format!("<div id=x>{source}</div>"),
        );
        assert_eq!(
            find(&doc, &tree, "x").line_boxes.len(),
            expected,
            "{rule} {source}"
        );
    }
    let (doc, normal, _) = page("#x{width:20px}", "<div id=x>漢字漢字</div>");
    let (_, keep, _) = page(
        "#x{width:20px;word-break:keep-all}",
        "<div id=x>漢字漢字</div>",
    );
    assert!(find(&doc, &normal, "x").line_boxes.len() > find(&doc, &keep, "x").line_boxes.len());
}

#[test]
fn overflow_wrap_break_word_has_distinct_intrinsic_contribution_and_emergency_fitting() {
    let (doc, tree, _) = page(
        "#a,#b{width:min-content;font-family:'Geometry Variable'}#a{overflow-wrap:anywhere}#b{overflow-wrap:break-word}#c{width:24px;overflow-wrap:break-word;font-family:'Geometry Variable'}",
        "<div id=a>AAAA</div><div id=b>AAAA</div><div id=c>AAAA</div>",
    );
    near(find(&doc, &tree, "a").content_rect.width, 12.0);
    near(find(&doc, &tree, "b").content_rect.width, 48.0);
    assert_eq!(find(&doc, &tree, "c").line_boxes.len(), 2);
    let (doc, tree, _) = page(
        "#x{width:40px;overflow-wrap:break-word;font-family:'Geometry Variable'}",
        "<div id=x>AA AA</div>",
    );
    assert_eq!(find(&doc, &tree, "x").line_boxes.len(), 2);
    for line in &find(&doc, &tree, "x").line_boxes {
        near(line.content_rect.width, 24.0);
    }
}

#[test]
fn licensed_english_auto_hyphenation_manual_none_and_language_fallback() {
    let (doc, auto, _) = page(
        "#x{width:95px;hyphens:auto}",
        "<div id=x lang=en-US>representation</div>",
    );
    assert!(text(find(&doc, &auto, "x")).contains('-'));
    assert!(auto.diagnostics().is_empty(), "{:?}", auto.diagnostics());
    let (_, unsupported, _) = page(
        "#x{width:95px;hyphens:auto}",
        "<div id=x lang=xx>representation</div>",
    );
    assert!(
        unsupported
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("English-US"))
    );
    let (doc, none, _) = page(
        "#x{width:55px;hyphens:none}",
        "<div id=x>co\u{00ad}operate</div>",
    );
    let (_, manual, _) = page(
        "#x{width:55px;hyphens:manual}",
        "<div id=x>co\u{00ad}operate</div>",
    );
    assert_eq!(find(&doc, &none, "x").line_boxes.len(), 1);
    assert!(text(find(&doc, &manual, "x")).contains('-'));
}

#[test]
fn language_casing_expands_without_changing_dom_or_source_cluster_ownership() {
    for (lang, transform, source, expected) in [
        ("de", "uppercase", "straße", "STRASSE"),
        ("tr", "uppercase", "istanbul ı", "İSTANBUL I"),
        ("tr", "lowercase", "Iİ", "ıi"),
        ("el", "lowercase", "ΟΣ ΟΣΑ", "ος οσα"),
        ("lt", "lowercase", "I\u{0301}", "i\u{0307}\u{0301}"),
        ("en", "capitalize", "hello world", "Hello World"),
        ("ja", "full-size-kana", "ぁァっ", "あアつ"),
        ("en", "full-width", "AB!", "ＡＢ！"),
    ] {
        let (doc, tree, _) = page(
            &format!("#x{{text-transform:{transform}}}"),
            &format!("<div id=x lang={lang}>{source}</div>"),
        );
        let b = find(&doc, &tree, "x");
        assert_eq!(text(b), expected.replace(' ', ""));
        let original = doc
            .preorder()
            .filter_map(|id| match doc.node(id).unwrap().kind() {
                crate::dom::NodeKind::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert!(original.contains(source));
        for f in fragments(b) {
            for g in &f.glyphs.glyphs {
                assert!(
                    g.source.is_char_boundary(g.cluster)
                        && g.source.is_char_boundary(g.cluster_end)
                );
            }
        }
    }
}

#[test]
fn justification_and_last_alignment_distribute_only_selected_line_opportunities() {
    let (doc, tree, _) = page(
        "#x{width:80px;font-family:'Geometry Variable';text-align:justify;text-align-last:justify;text-justify:inter-word}",
        "<div id=x>A A A</div>",
    );
    let b = find(&doc, &tree, "x");
    near(b.line_boxes[0].content_rect.width, 80.0);
    let f = fragments(b);
    assert!(
        f.iter()
            .flat_map(|f| f.glyphs.glyphs.iter().map(move |g| f.x + g.x))
            .all(f32::is_finite)
    );
    let (doc, tree, _) = page(
        "#x{width:80px;font-family:'Geometry Variable';text-align:justify;text-align-last:center}",
        "<div id=x>A A</div>",
    );
    let b = find(&doc, &tree, "x");
    near(b.line_boxes[0].content_rect.x - b.content_rect.x, 22.0);
    let (doc, tree, _) = page(
        "#x{width:80px;font-family:'Geometry Variable';text-align:justify;text-align-last:justify;text-justify:none}",
        "<div id=x>A A</div>",
    );
    near(
        find(&doc, &tree, "x").line_boxes[0].content_rect.width,
        36.0,
    );
    let (doc, tree, _) = page(
        "#x{width:80px;direction:rtl;text-align:justify;text-align-last:justify}",
        "<div id=x>אב אב אב</div>",
    );
    near(
        find(&doc, &tree, "x").line_boxes[0].content_rect.width,
        80.0,
    );
}

#[test]
fn spacing_kerning_features_and_actual_variable_axes_share_measure_and_masks() {
    let fonts = fonts();
    let style = style::ComputedStyle {
        font_size: 20.0,
        font_family: crate::css::values::FontFamily(vec![crate::css::values::FamilyName::Named(
            "Geometry Variable".into(),
        )]),
        ..Default::default()
    };
    let a = fonts.shape(&style, "AA");
    let mut heavy = style.clone();
    heavy.font_weight = crate::css::values::FontWeight(900.0);
    heavy.bold = true;
    let b = fonts.shape(&heavy, "AA");
    near(a.width, 24.0);
    near(b.width, 36.0);
    assert!(!b.glyphs[0].bold);
    let small = a.glyphs[0]
        .face
        .instance_bitmap(a.glyphs[0].id, 20.0, &a.glyphs[0].coordinates)
        .unwrap();
    let large = b.glyphs[0]
        .face
        .instance_bitmap(b.glyphs[0].id, 20.0, &b.glyphs[0].coordinates)
        .unwrap();
    assert_eq!(small.metrics.width, 10);
    assert_eq!(large.metrics.width, 16);
    assert_ne!(small.pixels, large.pixels);
    assert!(std::sync::Arc::ptr_eq(
        &small,
        &a.glyphs[0]
            .face
            .instance_bitmap(a.glyphs[0].id, 20.0, &a.glyphs[0].coordinates)
            .unwrap()
    ));
    let (doc, tree, _) = page(
        "#x{font-family:'Geometry Variable';letter-spacing:-1px;word-spacing:3px}",
        "<div id=x>A A</div>",
    );
    near(
        find(&doc, &tree, "x").line_boxes[0].content_rect.width,
        36.0,
    );
    let normal = fonts.shape(&style::ComputedStyle::default(), "ffi");
    let mut disabled = style::ComputedStyle::default();
    disabled.typography.set(
        P::FontFeatures,
        style::typography::Value::Settings([(*b"liga", 0.0)].into()),
        16.0,
        disabled.text_color,
    );
    assert!(fonts.shape(&disabled, "ffi").glyphs.len() > normal.glyphs.len());
}

#[test]
fn real_color_layers_compose_and_emoji_sequences_choose_one_grapheme_face() {
    let fonts = fonts();
    let style = style::ComputedStyle {
        font_size: 20.0,
        font_family: crate::css::values::FontFamily(vec![crate::css::values::FamilyName::Named(
            "Geometry Color".into(),
        )]),
        ..Default::default()
    };
    for source in ["😀", "👨\u{200d}👩\u{200d}👧", "🇺🇸", "👨🏻"] {
        let run = fonts.shape(&style, source);
        assert_eq!(run.glyphs.len(), 1, "{source}: {run:?}");
        assert_ne!(run.glyphs[0].id, 0);
        let bitmap = run.glyphs[0]
            .face
            .instance_bitmap(run.glyphs[0].id, 20.0, &run.glyphs[0].coordinates)
            .unwrap();
        assert!(bitmap.color);
        assert!(
            bitmap
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] != p[2])
        );
    }
    for scale in [1.0, 2.0] {
        let colors = |rotation| {
            let mut run = fonts.shape(&style, "😀").as_ref().clone();
            run.glyphs[0].rotation = rotation;
            let mut pixels = vec![0xffffff; 80 * 80];
            paint::rasterize_display_list(
                &[paint::DisplayItem::Text {
                    text: "😀".into(),
                    bold: false,
                    italic: false,
                    x: 20.0,
                    y: 0.0,
                    baseline: 20.0,
                    size: 20.0,
                    glyphs: std::sync::Arc::new(run),
                    color: 0x00ff00,
                }],
                &fonts,
                &mut pixels,
                80,
                80,
                scale,
                0.0,
            );
            let mut counts = std::collections::BTreeMap::new();
            for pixel in pixels {
                *counts.entry(pixel).or_insert(0usize) += 1;
            }
            assert!(counts.contains_key(&0xff0000) && counts.contains_key(&0x7f0080));
            counts
        };
        assert_eq!(
            colors(crate::text::GlyphRotation::None),
            colors(crate::text::GlyphRotation::Clockwise)
        );
    }
}

#[test]
fn decorations_propagate_ancestor_color_and_respect_atomic_barriers_and_line_styles() {
    let (doc, tree, fonts) = page(
        "#x{color:red;text-decoration:underline overline line-through double;text-decoration-thickness:2px;text-decoration-skip-ink:none}span{color:blue;text-decoration:none}#atomic{display:inline-block}",
        "<div id=x><span>A</span><span id=atomic>B</span></div>",
    );
    let b = find(&doc, &tree, "x");
    assert!(
        fragments(&b.children[0])
            .iter()
            .all(|_| !b.children[0].style.decoration_sources.is_empty())
    );
    assert!(
        find(&doc, &tree, "atomic")
            .style
            .decoration_sources
            .is_empty()
    );
    let items = paint::build_display_list(&tree, 448.0, 400.0, false);
    let strokes = items
        .iter()
        .filter_map(|i| match i {
            paint::DisplayItem::TextDecoration { color, style, .. } => {
                Some((color.to_srgb8().0, *style))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(strokes.len() >= 3);
    assert!(
        strokes
            .iter()
            .all(|&(c, s)| c == 0xff0000 && s == K::Double)
    );
    let mut pixels = vec![0; 448 * 400];
    paint::rasterize_display_list(&items, &fonts, &mut pixels, 448, 400, 1.0, 0.0);
    assert!(pixels.contains(&0xff0000));
}

#[test]
fn counter_styles_systems_padding_ranges_extends_and_cycles_are_bounded() {
    let (doc, tree, _) = page(
        "@counter-style letters{system:alphabetic;symbols:'A' 'B';negative:'(' ')';pad:3 'A';suffix:') '}@counter-style child{system:extends letters;prefix:'('}@counter-style loop{system:fixed;symbols:'A';range:2 2;fallback:loop}#x{counter-reset:n 3}#x::before{content:counter(n,child)}",
        "<div id=x>A</div><ol style='list-style:letters inside' start=3><li id=y>A</li></ol>",
    );
    assert!(text(find(&doc, &tree, "x")).starts_with("AAA"));
    assert!(text(find(&doc, &tree, "y")).starts_with("AAA)"));
    let resolution = style::resolve(&doc);
    assert_eq!(
        resolution
            .styles
            .counter_styles
            .format("upper-roman", 14, false)
            .0,
        "XIV"
    );
    assert_eq!(
        resolution
            .styles
            .counter_styles
            .format("arabic-indic", -12, false)
            .0,
        "-١٢"
    );
    assert!(
        resolution
            .styles
            .counter_styles
            .format("loop", i32::MAX, false)
            .1
            .is_some()
    );
}

#[test]
fn counters_scopes_html_reversed_start_value_and_pseudos_are_stable_across_reflow() {
    let (doc, mut tree, fonts) = page(
        "main{counter-reset:n}h2{counter-increment:n}h2::before{content:counter(n) ' '}section{display:contents;counter-reset:n}section h2::before{content:counters(n,'.') ' '}",
        "<h2 id=a>A</h2><section><h2 id=b>B</h2></section><h2 id=c>A</h2><ol reversed start=5><li id=d>A</li><li id=e value=9>B</li><li id=f>A</li></ol>",
    );
    assert!(text(find(&doc, &tree, "a")).starts_with("1"));
    assert!(text(find(&doc, &tree, "b")).starts_with("1.1"));
    assert!(text(find(&doc, &tree, "c")).starts_with("2"));
    assert_eq!(
        find(&doc, &tree, "d").list_marker.as_ref().unwrap().text,
        "5."
    );
    assert_eq!(
        find(&doc, &tree, "e").list_marker.as_ref().unwrap().text,
        "9."
    );
    assert_eq!(
        find(&doc, &tree, "f").list_marker.as_ref().unwrap().text,
        "8."
    );
    let before = text(tree.root());
    tree.layout_viewport(300.0, 400.0, &fonts);
    assert_eq!(text(tree.root()), before);
}
