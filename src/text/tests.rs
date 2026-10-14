use super::*;

pub(super) fn fixture_context() -> FontContext {
    let mut database = Database::new();
    for bytes in [
        BUNDLED,
        include_bytes!("../../assets/fonts/noto-sans/NotoSansArabic-Regular.ttf").as_slice(),
        include_bytes!("../../assets/fonts/noto-sans/NotoSansSC-fixture.ttf").as_slice(),
        include_bytes!("../../assets/fonts/noto-sans/NotoSansBold-fixture.ttf").as_slice(),
        include_bytes!("../../assets/fonts/noto-sans/NotoSansItalic-fixture.ttf").as_slice(),
    ] {
        database.load_font_data(bytes.to_vec());
    }
    database.set_sans_serif_family("Noto Sans");
    database.set_serif_family("Noto Sans SC");
    database.set_monospace_family("Noto Sans Arabic");
    FontContext::from_database(database)
}

fn named(name: &str) -> ComputedStyle {
    ComputedStyle {
        font_family: crate::css::values::FontFamily(vec![FamilyName::Named(name.into())]),
        ..ComputedStyle::default()
    }
}

#[test]
fn bundled_ligatures_kerning_clusters_and_marks_use_opentype_output() {
    let fonts = FontContext::bundled();
    let style = ComputedStyle::default();
    let ffi = fonts.shape(&style, "ffi");
    assert_eq!(ffi.glyphs.len(), 1);
    assert_eq!((ffi.glyphs[0].cluster, ffi.glyphs[0].cluster_end), (0, 3));
    assert_ne!(ffi.glyphs[0].id, 0);
    let av = fonts.shape(&style, "AV");
    let nominal = fonts.metrics('A', 16.0).advance_width + fonts.metrics('V', 16.0).advance_width;
    assert!(av.width < nominal);
    let face = bundled_face();
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str("AV");
    buffer.guess_segment_properties();
    let output = rustybuzz::shape(&face.shaper(), &[], buffer);
    let reference = output
        .glyph_positions()
        .iter()
        .map(|p| p.x_advance)
        .sum::<i32>() as f32
        * 16.0
        / face.shaper().units_per_em() as f32;
    assert!((av.width - reference).abs() < 0.001);
    let marks = fonts.shape(&style, "x\u{301}");
    assert_eq!(marks.glyphs.len(), 2);
    assert!(
        marks
            .glyphs
            .iter()
            .any(|g| g.advance == 0.0 && g.x_offset != 0.0)
    );
    assert!(marks.glyphs.iter().all(|g| g.cluster == 0));
}

#[test]
fn named_generic_weight_and_italic_select_real_faces_without_double_synthesis() {
    let fonts = fixture_context();
    let mut style = named("Noto Sans");
    let regular = fonts.select(&style);
    assert_eq!(regular.weight, 400);
    style.font_weight.0 = 700.0;
    style.bold = true;
    let bold = fonts.shape(&style, "AVffi");
    assert_eq!(bold.glyphs[0].face.weight, 700);
    assert!(!bold.glyphs[0].bold);
    assert!(bold.width != fonts.shape(&named("Noto Sans"), "AVffi").width);
    style.font_weight.0 = 400.0;
    style.bold = false;
    style.font_style = FontStyle::Italic;
    style.italic = true;
    let italic = fonts.shape(&style, "AV");
    assert!(italic.glyphs[0].face.italic);
    assert!(!italic.glyphs[0].italic);
    for (generic, expected) in [
        ("sans-serif", "Noto Sans"),
        ("serif", "Noto Sans SC"),
        ("monospace", "Noto Sans Arabic"),
    ] {
        let style = ComputedStyle {
            font_family: crate::css::values::FontFamily(vec![FamilyName::Generic(generic.into())]),
            ..Default::default()
        };
        assert_eq!(fonts.select(&style).name, expected);
    }
    let style = ComputedStyle {
        font_family: crate::css::values::FontFamily(vec![
            FamilyName::Named("missing family".into()),
            FamilyName::Named("Noto Sans Arabic".into()),
        ]),
        ..Default::default()
    };
    assert_eq!(fonts.select(&style).name, "Noto Sans Arabic");
}

#[test]
fn fallback_preserves_complex_script_graphemes_and_rtl_visual_positions() {
    let fonts = fixture_context();
    let style = named("Noto Sans");
    let run = fonts.shape(&style, "ffi سلام 水");
    let arabic: Vec<_> = run
        .glyphs
        .iter()
        .filter(|g| g.face.name == "Noto Sans Arabic")
        .collect();
    assert!(arabic.len() >= 3);
    assert!(arabic.iter().all(|g| g.id != 0 && g.rtl));
    assert!(
        arabic
            .windows(2)
            .all(|pair| pair[0].x <= pair[1].x && pair[0].cluster >= pair[1].cluster)
    );
    let cjk = run
        .glyphs
        .iter()
        .find(|g| g.face.name == "Noto Sans SC")
        .unwrap();
    assert_ne!(cjk.id, 0);
    let pixels = cjk.face.rasterized(cjk.id, 24.0).unwrap();
    assert!(pixels.pixels.iter().any(|&p| p != 0));
    let marked = fonts.shape(&style, "س\u{64e}\u{651}لام");
    assert!(
        marked
            .glyphs
            .iter()
            .all(|g| g.face.name == "Noto Sans Arabic" && g.id != 0)
    );
    assert!(
        marked
            .glyphs
            .iter()
            .any(|g| g.advance == 0.0 && (g.x_offset != 0.0 || g.y_offset != 0.0))
    );
}

#[test]
fn joining_controls_reach_the_shaper_and_change_arabic_forms() {
    let fonts = fixture_context();
    let style = named("Noto Sans Arabic");
    let ids = |text| {
        fonts
            .shape(&style, text)
            .glyphs
            .iter()
            .filter(|g| g.advance != 0.0)
            .map(|g| g.id)
            .collect::<Vec<_>>()
    };
    assert_ne!(ids("بب"), ids("ب\u{200c}ب"));
    assert_eq!(ids("بب"), ids("ب\u{200d}ب"));
    assert_eq!(
        fonts
            .shape(&style, "ب\u{200d}ب")
            .glyphs
            .iter()
            .map(|g| g.cluster_end)
            .max()
            .unwrap(),
        "ب\u{200d}ب".len()
    );
}

#[test]
fn face_identity_collection_indices_and_failed_loading_are_stable() {
    // Construct a TTC from two licensed fixture TTFs. Offsets in each member's
    // directory are collection-relative, exactly like installed OS collections.
    let members = [
        BUNDLED,
        include_bytes!("../../assets/fonts/noto-sans/NotoSansArabic-Regular.ttf").as_slice(),
    ];
    let mut data = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
    data.extend_from_slice(&[0; 8]);
    for (index, member) in members.iter().enumerate() {
        let offset = data.len();
        data[12 + index * 4..16 + index * 4].copy_from_slice(&(offset as u32).to_be_bytes());
        data.extend_from_slice(member);
        let tables = u16::from_be_bytes([member[4], member[5]]) as usize;
        for table in 0..tables {
            let location = offset + 12 + table * 16 + 8;
            let old = u32::from_be_bytes(data[location..location + 4].try_into().unwrap());
            data[location..location + 4].copy_from_slice(&(old + offset as u32).to_be_bytes());
        }
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
    }
    let mut database = Database::new();
    database.load_font_data(data);
    assert_eq!(database.len(), 2);
    let fonts = FontContext::from_database(database);
    let style = named("Noto Sans Arabic");
    let face = fonts.select(&style);
    assert_eq!(face.index, 1);
    let local = fonts.local("NotoSansArabic-Regular").unwrap();
    assert!(Arc::ptr_eq(&face, &local));
    assert_eq!(local.index, 1);
    assert!(Arc::ptr_eq(&face, &fonts.clone().select(&style)));
    let run = fonts.shape(&style, "سلام");
    let separate = fixture_context().shape(&style, "سلام");
    assert_eq!(
        run.glyphs.iter().map(|g| g.id).collect::<Vec<_>>(),
        separate.glyphs.iter().map(|g| g.id).collect::<Vec<_>>()
    );
    assert_eq!(
        face.rasterize_indexed(run.glyphs[0].id, 32.0).1,
        separate.glyphs[0]
            .face
            .rasterize_indexed(separate.glyphs[0].id, 32.0)
            .1
    );

    let mut database = Database::new();
    database.load_font_data(BUNDLED.to_vec());
    let mut broken = database.faces().next().unwrap().clone();
    broken.families = vec![("Broken".into(), fontdb::Language::English_UnitedStates)];
    broken.source = Source::Binary(Arc::new(vec![0u8; 4]));
    database.push_face_info(broken);
    let fonts = FontContext::from_database(database);
    for _ in 0..3 {
        assert!(Arc::ptr_eq(
            &fonts.select(&named("Broken")),
            &bundled_face()
        ));
    }
    assert_eq!(fonts.diagnostics().len(), 1);
    assert!(fonts.diagnostics()[0].contains("invalid OpenType face"));
    assert!(fonts.shape(&named("Broken"), "ffi").glyphs[0].id != 0);
}

#[test]
fn local_full_and_postscript_names_do_not_substitute_another_family_face() {
    let fonts = fixture_context();
    assert!(fonts.local("Definitely Missing").is_none());
    let bold = fonts.local("NotoSans-Bold").unwrap();
    assert_eq!(bold.weight, 700);
    let italic = fonts.local("Noto Sans Italic").unwrap();
    assert!(italic.italic);
    assert!(Arc::ptr_eq(&bold, &fonts.local("nOtOsAnS-bOlD").unwrap()));
}

#[test]
fn author_weight_matching_uses_css_search_order_and_precedes_installed_families() {
    use crate::css::font_face::{FontDescriptor, FontSource};
    let fonts = fixture_context();
    let regular = fonts.select(&named("Noto Sans"));
    let bold = fonts.local("NotoSans-Bold").unwrap();
    let italic = fonts.local("NotoSans-Italic").unwrap();
    let author = |weight, face| AuthorFont {
        descriptor: FontDescriptor {
            family: "Noto Sans".into(),
            sources: vec![FontSource::Local("fixture".into())],
            weight,
            weight_range: (weight, weight),
            style: FontStyle::Normal,
            ranges: vec![(0, 0x10ffff)],
        },
        face,
    };
    let overlay = fonts.with_author_fonts(vec![
        author(400, regular.clone()),
        author(500, italic.clone()),
        author(600, bold.clone()),
    ]);
    for (request, expected) in [(450.0, &italic), (390.0, &regular), (550.0, &bold)] {
        let mut style = named("Noto Sans");
        style.font_weight.0 = request;
        assert!(Arc::ptr_eq(&overlay.select(&style), expected));
    }
    assert!(Arc::ptr_eq(&fonts.select(&named("Noto Sans")), &regular));
}

#[test]
fn oversized_text_and_raster_resources_are_bounded_with_a_real_fallback() {
    let fonts = fixture_context();
    assert!(
        fonts
            .shape(&ComputedStyle::default(), &"a".repeat(MAX_TEXT_BYTES + 1))
            .glyphs
            .is_empty()
    );
    assert_eq!(fonts.diagnostics().len(), 1);
    let face = bundled_face();
    assert!(
        face.rasterized(face.lookup_glyph_index('A'), f32::INFINITY)
            .is_none()
    );
    assert!(
        face.rasterized(face.lookup_glyph_index('A'), 4096.0)
            .is_none()
    );
    assert!(
        face.rasterized(face.lookup_glyph_index('A'), 16.0)
            .unwrap()
            .pixels
            .iter()
            .any(|&p| p != 0)
    );
}

#[test]
fn empty_discovery_still_reports_unavailable_glyphs_and_paints_bundled_fallback() {
    let fonts = FontContext::from_database(Database::new());
    let run = fonts.shape(&ComputedStyle::default(), "水");
    assert_eq!(run.glyphs.len(), 1);
    assert_eq!(run.glyphs[0].id, 0);
    assert!(fonts.diagnostics().iter().any(|d| d.contains("U+6C34")));
    assert!(
        run.glyphs[0]
            .face
            .rasterized(0, 16.0)
            .unwrap()
            .pixels
            .iter()
            .any(|&p| p != 0)
    );
    assert_ne!(
        fonts.shape(&ComputedStyle::default(), "ffi").glyphs[0].id,
        0
    );
}

#[test]
fn earlier_missing_glyph_searches_do_not_disable_later_script_fallback() {
    let fonts = fixture_context();
    let style = named("Noto Sans");
    let primary = fonts.select(&style);
    // A truncated search must not memoize missing coverage or spend a
    // process-wide budget needed by the next shaping operation.
    let exhausted = fonts.fallback(&style, "سلام", &primary, &mut 0);
    assert!(Arc::ptr_eq(&exhausted, &bundled_face()));
    assert!(fonts.0.cache.lock().unwrap().missing.is_empty());
    let run = fonts.shape(&style, "سلام");
    assert!(
        run.glyphs
            .iter()
            .all(|g| g.face.name == "Noto Sans Arabic" && g.id != 0)
    );
}

fn frame(source: &str, fonts: &FontContext) -> (crate::engine::Frame, crate::layout::LayoutTree) {
    let document = crate::html::parser::parse(source).unwrap();
    let mut frame =
        crate::engine::Frame::new(url::Url::parse("https://text.test/").unwrap(), document);
    let tree = frame.layout_with_container_queries(fonts);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    (frame, tree)
}

fn element<'a>(
    tree: &'a crate::layout::LayoutTree,
    frame: &crate::engine::Frame,
    name: &str,
) -> &'a crate::layout::LayoutBox {
    let id = frame
        .document
        .preorder()
        .find(|&id| {
            frame
                .document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap();
    fn find(
        node: &crate::layout::LayoutBox,
        id: crate::dom::NodeId,
    ) -> Option<&crate::layout::LayoutBox> {
        if node.dom_node_id() == Some(id) {
            return Some(node);
        }
        node.children().iter().find_map(|child| find(child, id))
    }
    find(tree.root(), id).unwrap()
}

fn fragments(node: &crate::layout::LayoutBox) -> Vec<&crate::layout::TextFragment> {
    let mut text: Vec<_> = node.text_fragments().iter().collect();
    for child in node.children() {
        text.extend(fragments(child));
    }
    text
}

#[test]
fn shaped_width_controls_wrapping_alignment_and_discretionary_context() {
    let fonts = FontContext::bundled();
    let style = ComputedStyle::default();
    let expected = fonts.shape(&style, "ffi ffi").width;
    let (frame, tree) = frame(
        &format!(
            "<style>body{{margin:0}}#host{{width:{}px;text-align:right;font:16px/20px sans-serif}}</style><div id='host'>ffi ffi</div>",
            expected + 1.0
        ),
        &fonts,
    );
    let host = element(&tree, &frame, "host");
    assert_eq!(host.line_boxes().len(), 1);
    assert!((host.line_boxes()[0].content_rect.width - expected).abs() < 0.001);
    assert!((host.line_boxes()[0].content_rect.x - host.content_rect().x - 1.0).abs() < 0.001);
    assert_eq!(
        fragments(host)
            .iter()
            .map(|f| f.glyphs.glyphs.len())
            .sum::<usize>(),
        2
    );
    let (_, overflow) = self::frame("<div style='width:1px'>ffi</div>", &fonts);
    assert_eq!(
        fragments(overflow.root())
            .iter()
            .map(|f| f.glyphs.glyphs.len())
            .sum::<usize>(),
        1
    );
    let (_, wide) = self::frame("<div style='width:100px'>f&shy;fi</div>", &fonts);
    let fragment = fragments(wide.root())[0];
    assert_eq!(fragment.text, "ffi");
    assert_eq!(
        fragment
            .glyphs
            .glyphs
            .iter()
            .filter(|g| g.advance != 0.0)
            .count(),
        1
    );
    assert!((fragment.width - fonts.shape(&style, "ffi").width).abs() < 0.001);
    let hyphen_width = fonts.shape(&style, "f-").width;
    let (frame, narrow) = self::frame(
        &format!(
            "<div id='host' style='font:16px/20px sans-serif;width:{}px'>f&shy;fi</div>",
            hyphen_width + 0.01
        ),
        &fonts,
    );
    let host = element(&narrow, &frame, "host");
    assert_eq!(host.line_boxes().len(), 2);
    assert_eq!(
        fragments(host)
            .iter()
            .map(|f| f.text.as_str())
            .collect::<Vec<_>>(),
        ["f-", "fi"]
    );
    assert!((host.line_boxes()[0].content_rect.width - hyphen_width).abs() < 0.001);
}

#[test]
fn inline_advance_agrees_with_shaping_for_spaces_and_punctuation() {
    let fonts = FontContext::bundled();
    for text in ["T T", "A V", "V A", "To To", "A.W.", "T\u{a0}T"] {
        let (frame, tree) = frame(
            &format!("<div id='host' style='font:16px/20px sans-serif'>{text}</div>"),
            &fonts,
        );
        let host = element(&tree, &frame, "host");
        let expected = fonts.shape(&ComputedStyle::default(), text).width;
        assert!(
            (host.line_boxes()[0].content_rect.width - expected).abs() < 0.001,
            "{text:?}: layout={}, shaped={expected}",
            host.line_boxes()[0].content_rect.width
        );
    }
}

#[test]
fn explicit_zero_width_opportunities_reshape_ligatures_without_losing_wbr() {
    let fonts = FontContext::bundled();
    let style = ComputedStyle::default();
    let first_width = fonts.shape(&style, "f").width;
    for text in ["f<wbr>fi", "f\u{200b}fi"] {
        let (frame, tree) = frame(
            &format!(
                "<div id='host' style='font:16px/20px sans-serif;width:{}px'>{text}</div>",
                first_width + 0.01
            ),
            &fonts,
        );
        let host = element(&tree, &frame, "host");
        assert_eq!(host.line_boxes().len(), 2, "{text}");
        assert!((host.line_boxes()[0].content_rect.width - first_width).abs() < 0.001);
        assert!(
            (host.line_boxes()[1].content_rect.width - fonts.shape(&style, "fi").width).abs()
                < 0.001
        );
        assert_eq!(
            fragments(host)
                .iter()
                .map(|f| f.text.as_str())
                .collect::<Vec<_>>(),
            ["f", "fi"]
        );
    }
    let (frame, tree) = frame(
        "<div id='host' style='font:16px/20px sans-serif'>A<span>V</span></div>",
        &fonts,
    );
    let host = element(&tree, &frame, "host");
    assert!(
        (host.line_boxes()[0].content_rect.width - fonts.shape(&style, "AV").width).abs() < 0.001
    );
}

#[test]
fn media_font_units_use_the_context_initial_face_not_the_document_root() {
    let fonts = fixture_context();
    let initial = fonts.select(&ComputedStyle::default()).units(16.0).ex;
    assert!((initial - 8.0).abs() > 0.01);
    let threshold = (800.0 / initial + 800.0 / 8.0) / 2.0;
    let (frame, tree) = frame(
        &format!(
            "<style>:root{{font-family:'Noto Sans Arabic';font-size:40px}}@media (min-width:{threshold}ex){{#host{{color:red}}}}</style><div id='host'>ffi</div>"
        ),
        &fonts,
    );
    let expected = if 800.0 >= threshold * initial {
        0xff0000
    } else {
        0
    };
    assert_eq!(
        element(&tree, &frame, "host")
            .style()
            .text_color
            .opaque_rgb(),
        Some(expected)
    );
}

#[test]
fn split_nodes_pseudos_anonymous_wrappers_and_links_keep_shaping_and_ownership() {
    let fonts = FontContext::bundled();
    let (frame, mut tree) = frame(
        "<style>body{margin:0}a{color:inherit}#host{font:16px/20px sans-serif}#host::before{content:'ffi '}#red{color:red}</style><div id='host'>f<a id='link' href='/'>fi</a><div>block</div>f<span id='red'>fi</span></div>",
        &fonts,
    );
    let host = element(&tree, &frame, "host");
    assert!(host.children().iter().any(|c| c.is_anonymous()));
    let first = host.children().iter().find(|c| c.is_anonymous()).unwrap();
    let text = fragments(first);
    assert_eq!(
        text.iter()
            .map(|f| f.glyphs.glyphs.iter().filter(|g| g.advance != 0.0).count())
            .sum::<usize>(),
        2
    );
    let link = element(&tree, &frame, "link");
    let rect = link.inline_fragments()[0].content_rect;
    let hit = tree
        .hit_test(rect.x + rect.width * 0.75, rect.y + 3.0)
        .unwrap();
    assert_eq!(
        crate::engine::link_url(&frame.document, hit, &frame.url)
            .unwrap()
            .unwrap()
            .path(),
        "/"
    );
    let last = host
        .children()
        .iter()
        .rev()
        .find(|c| c.is_anonymous())
        .unwrap();
    assert_eq!(
        fragments(last)
            .iter()
            .map(|f| f.glyphs.glyphs.iter().filter(|g| g.advance != 0.0).count())
            .sum::<usize>(),
        2,
        "optional cross-color ligatures are disabled, but the red fi still ligates"
    );
    let before = tree.clone();
    let list = crate::paint::build_display_list(&tree, 800.0, 600.0, false);
    let cloned = list.clone();
    for (a, b) in list.iter().zip(&cloned) {
        if let (
            crate::paint::DisplayItem::Text { glyphs: a, .. },
            crate::paint::DisplayItem::Text { glyphs: b, .. },
        ) = (a, b)
        {
            assert!(Arc::ptr_eq(a, b));
        }
    }
    tree.layout(800.0, &fonts);
    assert_eq!(tree, before);
}

#[test]
fn actual_indexed_pixels_match_rustybuzz_offsets_at_multiple_scales_and_scroll() {
    let fonts = FontContext::bundled();
    let text = "x\u{301}AVffi";
    let (_, tree) = frame(
        &format!("<div style='font:24px/0 sans-serif'>{text}</div>"),
        &fonts,
    );
    let fragment = fragments(tree.root())[0];
    assert_eq!(fragment.font_size, 24.0);
    assert!(fragment.ink_rect.height > 0.0);
    let items: Vec<_> = crate::paint::build_display_list(&tree, 400.0, 200.0, false)
        .into_iter()
        .filter(|i| matches!(i, crate::paint::DisplayItem::Text { .. }))
        .collect();
    let face = bundled_face();
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let output = rustybuzz::shape(&face.shaper(), &[], buffer);
    for (scale, scroll) in [(1.0, 0.0), (1.5, 0.25), (2.0, 12.125)] {
        let (width, height) = (800, 200);
        let mut actual = vec![0xffffff; width * height];
        let mut expected = actual.clone();
        crate::paint::rasterize_display_list(
            &items,
            &fonts,
            &mut actual,
            width,
            height,
            scale as f64,
            scroll,
        );
        let css = 24.0 / face.shaper().units_per_em() as f32;
        let mut pen = 0.0;
        for (info, position) in output.glyph_infos().iter().zip(output.glyph_positions()) {
            let (metrics, bitmap) = face.rasterize_indexed(info.glyph_id as u16, 24.0 * scale);
            let x = ((fragment.x + pen + position.x_offset as f32 * css) * scale).round() as i32
                + metrics.xmin;
            let y = ((fragment.baseline - scroll - position.y_offset as f32 * css) * scale).round()
                as i32
                - metrics.ymin
                - metrics.height as i32;
            for row in 0..metrics.height {
                for column in 0..metrics.width {
                    let (x, y) = (x + column as i32, y + row as i32);
                    if x >= 0 && y >= 0 && x < width as i32 && y < height as i32 {
                        let target = y as usize * width + x as usize;
                        let coverage = u32::from(bitmap[row * metrics.width + column]);
                        let gray = (expected[target] & 255) * (255 - coverage) / 255;
                        expected[target] = gray * 0x010101;
                    }
                }
            }
            pen += position.x_advance as f32 * css;
        }
        assert_eq!(actual, expected, "scale={scale} scroll={scroll}");
        assert!(actual.iter().any(|&pixel| pixel != 0xffffff));
    }
}

#[test]
fn selected_metrics_root_units_and_container_feedback_share_the_font_environment() {
    let fonts = fixture_context();
    let source = "<style>:root{font:20px normal;font-family:'Noto Sans Arabic'}body{margin:0}
        #host{container-type:inline-size;width:40ch;font:24px/normal 'Noto Sans SC'}
        #child{font:10px/normal 'Noto Sans';width:calc(1ch + 1ex + 1cap + 1ic + 1rch + 1rex + 1rcap + 1ric + 1lh + 1rlh + 1cqw)}
        @container (width > 10ch){#child{color:red}}</style><div id='host'><span id='child'>AVffi</span> 水</div>";
    let (mut frame, tree) = frame(source, &fonts);
    let host = element(&tree, &frame, "host");
    assert_eq!(host.style().font_face.name, "Noto Sans SC");
    let child = element(&tree, &frame, "child");
    assert_eq!(child.style().text_color.opaque_rgb(), Some(0xff0000));
    let child_units = child.style().font_face.units(10.0);
    let root_units = fonts.select(&named("Noto Sans Arabic")).units(20.0);
    let expected = child_units.ch
        + child_units.ex
        + child_units.cap
        + child_units.ic
        + root_units.ch
        + root_units.ex
        + root_units.cap
        + root_units.ic
        + child.style().used_line_height()
        + fonts
            .select(&named("Noto Sans Arabic"))
            .horizontal_line_metrics(20.0)
            .unwrap()
            .new_line_size
        + host.content_rect().width / 100.0;
    assert!((child.style().width.as_ref().unwrap().resolve(10.0, 0.0) - expected).abs() < 0.001);
    assert_eq!(tree, frame.layout_with_container_queries(&fonts));
    assert!(Arc::ptr_eq(
        &fonts.shape(&named("Noto Sans"), "ffi"),
        &fonts.clone().shape(&named("Noto Sans"), "ffi")
    ));
}

#[test]
fn non_latin_demo_headless_has_real_fallback_glyphs_baselines_and_paint() {
    let fonts = fixture_context();
    let (frame, tree) = frame(include_str!("../../tests/pages/text_shaping.html"), &fonts);
    for name in ["latin", "marks", "arabic", "cjk"] {
        let host = element(&tree, &frame, name);
        let glyphs: Vec<_> = fragments(host)
            .iter()
            .flat_map(|f| &f.glyphs.glyphs)
            .filter(|g| g.advance != 0.0)
            .collect();
        assert!(!glyphs.is_empty(), "{name}");
        assert!(glyphs.iter().all(|g| g.id != 0), "{name}");
        if name == "arabic" {
            assert!(
                glyphs
                    .iter()
                    .all(|g| g.rtl && g.face.name == "Noto Sans Arabic")
            );
        }
        if name == "cjk" {
            assert!(glyphs.iter().all(|g| g.face.name == "Noto Sans SC"));
        }
        assert!(
            fragments(host)
                .iter()
                .all(|f| host.line_boxes().iter().any(|l| l.baseline == f.baseline))
        );
    }
    let list = crate::paint::build_display_list(&tree, 800.0, 600.0, false);
    let mut pixels = vec![0xffffff; 800 * 600];
    let arabic_y = element(&tree, &frame, "arabic").rect().y;
    crate::paint::rasterize_display_list(&list, &fonts, &mut pixels, 800, 600, 1.0, arabic_y);
    assert!(pixels.iter().any(|&pixel| pixel != 0xffffff));
    let dump = crate::devtools::format_values(&tree, 1.0);
    assert!(dump.contains("Noto Sans Arabic"));
    assert!(dump.contains("ShapedGlyph"));
}

#[test]
fn cross_style_graphemes_marks_and_arabic_context_are_not_severed() {
    let fonts = fixture_context();
    let (frame, tree) = frame("<style>body{margin:0}#mark{color:red;font-size:24px;font-family:'Noto Sans Arabic'}</style>
            <div id='host' style='font:16px/20px \"Noto Sans\"'>x<span id='mark'>́</span></div>
            <div id='arabic' lang='ar' style='font:16px/20px \"Noto Sans Arabic\"'>ب<span style='font-size:24px'>ب</span></div>", &fonts);
    let host = element(&tree, &frame, "host");
    let mark = element(&tree, &frame, "mark");
    let fragment = fragments(mark)[0];
    assert_eq!(fragment.width, 0.0);
    assert_eq!(fragment.text_color.opaque_rgb(), Some(0xff0000));
    assert!(
        fragment
            .glyphs
            .glyphs
            .iter()
            .any(|g| g.advance == 0.0 && g.x_offset != 0.0)
    );
    assert!(
        fragment
            .glyphs
            .glyphs
            .iter()
            .all(|g| g.face.name == "Noto Sans" && g.size == 16.0 && g.cluster == 0)
    );
    assert_eq!(
        fragments(host).iter().map(|f| f.width).sum::<f32>(),
        fonts.shape(&named("Noto Sans"), "x́").width
    );
    let arabic = element(&tree, &frame, "arabic");
    assert_eq!(arabic.style().font_language.as_deref(), Some("ar"));
    let ids: Vec<_> = fragments(arabic)
        .iter()
        .flat_map(|f| &f.glyphs.glyphs)
        .filter(|g| g.advance != 0.0)
        .map(|g| g.id)
        .collect();
    let mut reference: Vec<_> = fonts
        .shape(&named("Noto Sans Arabic"), "بب")
        .glyphs
        .iter()
        .filter(|g| g.advance != 0.0)
        .map(|g| g.id)
        .collect();
    reference.reverse(); // Styling runs remain in logical order, not paragraph bidi.
    assert_eq!(ids, reference);
}

#[test]
fn mandatory_cross_color_ligature_uses_paint_slices_and_fallback_line_metrics() {
    let fonts = fixture_context();
    let (frame, tree) = frame("<div id='host' style='font:16px/20px \"Noto Sans Arabic\"'>ل<span style='color:red'>ا</span></div>
            <div id='mixed' style='font:16px/normal \"Noto Sans\"'>x سلام 水</div>", &fonts);
    let host = element(&tree, &frame, "host");
    let glyphs: Vec<_> = fragments(host)
        .iter()
        .flat_map(|f| &f.glyphs.glyphs)
        .filter(|g| g.advance != 0.0)
        .collect();
    assert_eq!(glyphs.len(), 2);
    assert_eq!(glyphs[0].id, glyphs[1].id);
    assert!(glyphs.iter().all(|g| g.clip.is_some()));
    let mixed = element(&tree, &frame, "mixed");
    let line = &mixed.line_boxes()[0];
    let expected_above = fragments(mixed)
        .iter()
        .filter_map(|f| f.glyphs.line_extents)
        .map(|(a, _)| a)
        .fold(0.0, f32::max);
    let expected_below = fragments(mixed)
        .iter()
        .filter_map(|f| f.glyphs.line_extents)
        .map(|(_, b)| b)
        .fold(0.0, f32::max);
    assert!((line.ascent - expected_above).abs() < 0.001);
    assert!((line.descent - expected_below).abs() < 0.001);
    let items = crate::paint::build_display_list(&tree, 800.0, 600.0, false);
    let mut pixels = vec![0xffffff; 800 * 80];
    crate::paint::rasterize_display_list(&items, &fonts, &mut pixels, 800, 80, 1.5, 0.0);
    assert!(pixels.contains(&0));
    assert!(
        pixels
            .iter()
            .any(|p| (p >> 16) & 255 == 255 && p & 255 < 255)
    );
}

#[test]
fn alt_markers_tabs_and_space_runs_share_selected_faces_and_explicit_baselines() {
    let fonts = fixture_context();
    let (frame, tree) = frame("<style>body{margin:0}li::marker{content:'ffi';font:24px/normal \"Noto Sans\"}</style>
            <ul><li id='li' style='font:16px/20px \"Noto Sans Arabic\"'>سلام</li></ul>
            <img id='image' alt='ffi' style='font:24px/normal \"Noto Sans\"'>
            <pre id='pre' style='font:16px/20px \"Noto Sans\";white-space:pre-wrap'>ffi\tffi  ffi</pre>", &fonts);
    let li = element(&tree, &frame, "li");
    let marker = li.list_marker().unwrap();
    assert_eq!(marker.glyphs.glyphs.len(), 1);
    assert_eq!(marker.glyphs.glyphs[0].face.name, "Noto Sans");
    assert_eq!(marker.baseline, li.line_boxes()[0].baseline);
    let image = element(&tree, &frame, "image");
    assert_eq!(image.text_fragments()[0].glyphs.glyphs.len(), 1);
    assert_eq!(image.text_fragments()[0].glyphs.glyphs[0].size, 24.0);
    let pre = element(&tree, &frame, "pre");
    let tab = fragments(pre).into_iter().find(|f| f.text == "\t").unwrap();
    assert!(tab.glyphs.glyphs.is_empty());
    assert!(tab.width > 0.0);
    assert_eq!(
        fragments(pre)
            .iter()
            .map(|f| f.text.as_str())
            .collect::<String>(),
        "ffi\tffi  ffi"
    );
    assert!(
        fragments(pre)
            .iter()
            .filter(|f| f.text == "ffi")
            .all(|f| f.glyphs.glyphs.len() == 1)
    );
}

#[test]
fn cache_caps_and_unsupported_outline_format_fail_explicitly() {
    let fonts = fixture_context();
    for size in 1..=180 {
        fonts.shape(
            &ComputedStyle {
                font_size: size as f32,
                ..Default::default()
            },
            "ffi",
        );
    }
    let cache = fonts.0.cache.lock().unwrap();
    assert!(cache.run_count <= MAX_RUNS && cache.run_bytes <= MAX_RUN_BYTES);
    assert!(cache.previous_run_count <= MAX_RUNS);
    assert!(
        cache.faces.len() <= MAX_FACES
            && cache.bytes <= MAX_SOURCE_BYTES
            && cache.glyphs <= MAX_GLYPHS
    );
    drop(cache);
    let mut bytes = BUNDLED.to_vec();
    let count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    for table in 0..count {
        let tag = 12 + table * 16;
        if &bytes[tag..tag + 4] == b"glyf" {
            bytes[tag..tag + 4].copy_from_slice(b"sbix");
        }
    }
    let mut database = Database::new();
    database.load_font_data(bytes);
    let mut info = database.faces().next().unwrap().clone();
    info.families = vec![("Bitmap test".into(), fontdb::Language::English_UnitedStates)];
    database.push_face_info(info);
    let fonts = FontContext::from_database(database);
    assert_eq!(fonts.select(&named("Bitmap test")).name, "Noto Sans");
    assert!(
        fonts
            .diagnostics()
            .iter()
            .any(|d| d.contains("unsupported raster format"))
    );
    assert!(fonts.shape(&named("Bitmap test"), "ffi").glyphs[0].id != 0);
}
