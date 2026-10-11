use super::*;

#[test]
fn transformed_clusters_retain_original_dom_byte_spans_across_selected_lines() {
    let (doc, tree, _) = page(
        "#x{width:45px;text-transform:uppercase;word-break:break-all}",
        "<div id=x lang=de>straße</div>",
    );
    let b = find(&doc, &tree, "x");
    assert_eq!(text(b), "STRASSE");
    assert!(b.line_boxes.len() > 1);
    let glyphs = fragments(b)
        .into_iter()
        .flat_map(|f| &f.glyphs.glyphs)
        .collect::<Vec<_>>();
    let expanded = glyphs
        .iter()
        .filter(|g| g.cluster == 4 && g.cluster_end == 6)
        .collect::<Vec<_>>();
    assert_eq!(expanded.len(), 2);
    for glyph in glyphs {
        assert_eq!(&*glyph.source, "straße");
        for span in &*glyph.source_spans {
            let original = doc.node(span.node_id.unwrap()).unwrap();
            let crate::dom::NodeKind::Text(text) = original.kind() else {
                panic!("non-text source owner")
            };
            assert_eq!(text, &*span.source);
            assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
        }
    }
    let (doc, tree, _) = page(
        "#x{word-break:break-all}#accent{color:red;text-decoration:underline}",
        "<div id=x><span>e</span><span id=accent>&#x301;</span></div>",
    );
    let glyphs = fragments(find(&doc, &tree, "x"))
        .into_iter()
        .flat_map(|f| &f.glyphs.glyphs)
        .collect::<Vec<_>>();
    assert!(glyphs.iter().any(|g| g.source_spans.len() == 2));
}

#[test]
fn cff2_blend_outlines_and_png_bitmap_strikes_are_actual_bounded_font_pixels() {
    let fonts = fonts();
    let style = |name| style::ComputedStyle {
        font_size: 20.0,
        font_family: crate::css::values::FontFamily(vec![crate::css::values::FamilyName::Named(
            name,
        )]),
        ..Default::default()
    };
    let mut regular = style("Geometry CFF2".into());
    let a = fonts.shape(&regular, "A");
    regular.font_weight = crate::css::values::FontWeight(900.0);
    let b = fonts.shape(&regular, "A");
    assert_eq!(a.glyphs[0].face.name, "Geometry CFF2");
    let mask = |g: &crate::text::ShapedGlyph, size| {
        g.face.instance_bitmap(g.id, size, &g.coordinates).unwrap()
    };
    assert_eq!(mask(&a.glyphs[0], 20.0).metrics.width, 10);
    assert_eq!(mask(&b.glyphs[0], 20.0).metrics.width, 16);
    let run = fonts.shape(&style("Geometry Bitmap".into()), "A");
    let bitmap = mask(&run.glyphs[0], 20.0);
    assert!(bitmap.color);
    assert_eq!((bitmap.metrics.width, bitmap.metrics.height), (10, 14));
    assert_eq!(&bitmap.pixels[..4], &[255, 0, 0, 255]);
    assert_eq!(mask(&run.glyphs[0], 40.0).metrics.width, 20);
    assert!(
        run.glyphs[0]
            .face
            .instance_bitmap(run.glyphs[0].id, f32::INFINITY, &[])
            .is_none()
    );
    assert!(
        run.glyphs[0]
            .face
            .instance_bitmap(run.glyphs[0].id, 2049.0, &[])
            .is_none()
    );
}

#[test]
fn numeric_axes_and_features_are_sorted_and_cache_isolated() {
    let fonts = fonts();
    let doc = parser::parse("<style>#a{font-family:'Geometry Variable';font-weight:900;font-variation-settings:'wght' 100}#b{font-family:'Geometry Variable';font-weight:100;font-variation-settings:'wght' 900}#c{font-variation-settings:'wght' 900,'opsz' 20}#d{font-variation-settings:'opsz' 20,'wght' 900}</style><div id=a>A</div><div id=b>A</div><div id=c>A</div><div id=d>A</div>").unwrap();
    let resolution = style::resolve_with_fonts(
        &doc,
        style::collect_stylesheets(&doc),
        Default::default(),
        &Default::default(),
        &Default::default(),
        &fonts,
    );
    let id = |name| {
        doc.preorder()
            .find(|&id| {
                doc.node(id)
                    .unwrap()
                    .as_element()
                    .is_some_and(|e| e.attribute("id") == Some(name))
            })
            .unwrap()
    };
    let a = fonts.shape(resolution.styles.get(id("a")).unwrap(), "A");
    let b = fonts.shape(resolution.styles.get(id("b")).unwrap(), "A");
    assert!(a.width < b.width);
    assert_ne!(a.glyphs[0].coordinates, b.glyphs[0].coordinates);
    let masks = [&a.glyphs[0], &b.glyphs[0]].map(|g| {
        g.face
            .instance_bitmap(g.id, g.size, &g.coordinates)
            .unwrap()
    });
    assert!(masks[0].metrics.advance_width < masks[1].metrics.advance_width);
    assert!(
        a.glyphs[0]
            .face
            .instance_bitmap(a.glyphs[0].id, 20.0, &[(*b"wght", f32::NAN)])
            .is_none()
    );
    assert!(!a.glyphs[0].bold && !b.glyphs[0].bold);
    assert_eq!(
        resolution
            .styles
            .get(id("c"))
            .unwrap()
            .typography
            .settings(P::FontVariations),
        resolution
            .styles
            .get(id("d"))
            .unwrap()
            .typography
            .settings(P::FontVariations)
    );
}

#[test]
fn counter_set_follows_increment_css_overrides_html_and_contents_reversed_count() {
    let (doc, tree, _) = page(
        "#a{counter-increment:none}#b{counter-increment:n}#c{counter-increment:list-item 0}ol{counter-reset:none}",
        "<ol start=99><li id=a>A</li><li id=b>A</li><li id=c>A</li></ol>",
    );
    for (id, marker) in [("a", "1."), ("b", "2."), ("c", "2.")] {
        assert_eq!(
            find(&doc, &tree, id).list_marker.as_ref().unwrap().text,
            marker
        );
    }
    let (doc, tree, _) = page(
        "#a{counter-increment:list-item 7;counter-set:list-item 4}#b::before{display:list-item;list-style-type:upper-alpha;counter-set:list-item 3;content:'A'}section{display:contents}",
        "<ol start=2><li id=a value=99>A</li><li id=b>B</li></ol><ol reversed><section><li id=c>A</li><li id=d>A</li></section><li id=e>A</li></ol>",
    );
    assert_eq!(
        find(&doc, &tree, "a").list_marker.as_ref().unwrap().text,
        "4."
    );
    assert_eq!(
        find(&doc, &tree, "b").list_marker.as_ref().unwrap().text,
        "5."
    );
    let pseudo = &find(&doc, &tree, "b").children[0];
    assert_eq!(pseudo.list_marker.as_ref().unwrap().text, "C.");
    for (id, marker) in [("c", "3."), ("d", "2."), ("e", "1.")] {
        assert_eq!(
            find(&doc, &tree, id).list_marker.as_ref().unwrap().text,
            marker
        );
    }
}

#[test]
fn conditional_and_layer_counter_style_collection_uses_active_source_priority() {
    let (doc, tree, _) = page(
        "@layer low,high;@layer high{@counter-style sample{system:cyclic;symbols:'B';suffix:''}}@layer low{@counter-style sample{system:cyclic;symbols:'A';suffix:''}}@media (min-width:99999px){@counter-style sample{system:cyclic;symbols:'X'}}@supports (word-break:break-all){@counter-style available{system:fixed;symbols:'A'}}#x::before{content:counter(n,sample)}",
        "<div id=x>A</div>",
    );
    assert_eq!(text(find(&doc, &tree, "x")), "BA");
    assert!(
        style::resolve(&doc)
            .styles
            .counter_styles
            .format("available", 1, false)
            .1
            .is_none()
    );
}

#[test]
fn vertical_justification_and_decoration_styles_produce_native_geometry() {
    let (doc, tree, fonts) = page(
        "#x{writing-mode:vertical-rl;height:80px;font-family:'Geometry Variable';text-align:justify;text-align-last:justify;text-justify:inter-character;text-decoration:underline wavy;text-decoration-thickness:2px;text-decoration-skip-ink:none}",
        "<div id=x>AAAA</div>",
    );
    let b = find(&doc, &tree, "x");
    assert!(
        b.line_boxes
            .iter()
            .all(|l| l.rect.width.is_finite() && l.rect.height.is_finite())
    );
    let items = paint::build_display_list(&tree, 448.0, 400.0, false);
    assert!(items.iter().any(|i| matches!(
        i,
        paint::DisplayItem::TextDecoration {
            horizontal: false,
            style: K::Wavy,
            ..
        }
    )));
    let mut pixels = vec![0xffffff; 448 * 400];
    paint::rasterize_display_list(&items, &fonts, &mut pixels, 448, 400, 1.0, 0.0);
    assert!(pixels.iter().any(|&p| p != 0xffffff));
}
