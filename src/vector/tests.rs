use std::sync::Arc;

use super::*;
use crate::{
    dom::Document,
    engine::Frame,
    html::parser,
    layout::{LayoutBox, LayoutTree},
    paint,
    text::FontContext,
};

fn frame(html: &str, fonts: &FontContext) -> (Frame, LayoutTree) {
    let document = parser::parse(&format!(
        "<!doctype html><style>body{{margin:0}}</style>{html}"
    ))
    .unwrap();
    let mut frame = Frame::new(url::Url::parse("file:///vectors.html").unwrap(), document);
    let tree = frame.layout_with_container_queries(fonts);
    (frame, tree)
}
fn id(doc: &Document, name: &str) -> NodeId {
    doc.preorder()
        .find(|id| {
            doc.node(*id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}
fn find(tree: &LayoutTree, node: NodeId) -> &LayoutBox {
    fn visit(b: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
        if b.dom_node_id() == Some(node) {
            Some(b)
        } else {
            b.children().iter().find_map(|b| visit(b, node))
        }
    }
    visit(tree.root(), node).unwrap()
}
fn paths(scene: &Scene) -> Vec<&tiny_skia::Path> {
    let mut result = Vec::new();
    for command in &scene.commands {
        match command {
            Command::Path { path, .. } => result.push(path),
            Command::Group { scene, .. } => result.extend(paths(scene)),
            _ => {}
        }
    }
    result
}
fn text(scene: &Scene) -> Vec<&crate::text::ShapedText> {
    let mut result = Vec::new();
    for command in &scene.commands {
        match command {
            Command::Text { glyphs, .. } => result.push(glyphs.as_ref()),
            Command::Group { scene, .. } => result.extend(text(scene)),
            _ => {}
        }
    }
    result
}

#[test]
fn svg_paths_use_real_curves_arcs_relative_smooth_and_evenodd_coverage() {
    let (frame, tree) = frame(
        r#"<svg id=s width=80 height=40 viewBox="0 0 80 40">
      <path fill=red d="M0 0h40v40H0z M10 10v20h20V10z" fill-rule=evenodd />
      <path fill=blue d="M40 20 A20 20 0 1 1 80 20 A20 20 0 1 1 40 20z"/>
      <path fill=none stroke=green d="M2 2 q2 4 4 0t4 0 C12 8 16 8 18 2s4 -2 6 0"/>
    </svg>"#,
        &FontContext::bundled(),
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let scene = find(&tree, id(&frame.document, "s"))
        .vector_scene()
        .unwrap();
    assert_eq!(paths(scene).len(), 3);
    assert!(
        paths(scene)[1]
            .segments()
            .any(|s| matches!(s, tiny_skia::PathSegment::CubicTo(..)))
    );
    for scale in [1, 2] {
        let pixels = raster::render(scene, 80 * scale, 40 * scale, frame.font_context()).unwrap();
        let pixel = |x, y| pixels[(y * scale * 80 * scale + x * scale) as usize];
        assert_eq!(pixel(5, 25), 0xffff0000);
        assert_eq!(pixel(20, 20), 0);
        assert_eq!(pixel(60, 20), 0xff0000ff);
        assert_eq!(pixel(79, 0), 0);
    }
}

#[test]
fn gradients_patterns_and_group_opacity_cover_native_pixels() {
    let (frame, tree) = frame(
        r##"<svg id=s width=80 height=40><defs>
      <linearGradient id=g><stop offset=0 stop-color=red /><stop offset="50%" stop-color=red />
      <stop offset="50%" stop-color=blue /><stop offset=1 stop-color=blue /></linearGradient>
      <radialGradient id=r><stop offset=0 stop-color=red /><stop offset=".5" stop-color=red />
      <stop offset=".5" stop-color=blue /><stop offset=1 stop-color=blue /></radialGradient>
      <pattern id=p patternUnits=userSpaceOnUse width=8 height=8>
      <rect width=8 height=8 fill=red /><rect width=4 height=4 fill=blue /></pattern></defs>
      <rect width=40 height=20 fill="url(#g)"/><rect x=40 width=40 height=40 fill="url(#r)"/>
      <rect y=20 width=20 height=20 fill="url(#p)"/>
      <g opacity=".5"><rect x=20 y=20 width=16 height=20 fill=red /><rect x=24 y=20 width=16 height=20 fill=red /></g>
    </svg>"##,
        &FontContext::bundled(),
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let scene = find(&tree, id(&frame.document, "s"))
        .vector_scene()
        .unwrap();
    for scale in [1, 2] {
        let pixels = raster::render(scene, 80 * scale, 40 * scale, frame.font_context()).unwrap();
        let p = |x, y| pixels[(y * scale * 80 * scale + x * scale) as usize];
        assert_eq!(p(5, 5), 0xffff0000);
        assert_eq!(p(35, 5), 0xff0000ff);
        assert_eq!(p(60, 20), 0xffff0000);
        assert_eq!(p(42, 2), 0xff0000ff);
        assert_eq!(p(1, 21), 0xffff0000);
        assert_eq!(p(1, 24), 0xff0000ff);
        assert_eq!(p(22, 25) >> 24, 128);
        assert_eq!(
            p(28, 25) >> 24,
            128,
            "overlap must not compound group opacity"
        );
    }
}

#[test]
fn local_use_symbol_xlink_and_auto_rotated_markers_expand_bounded_vector_geometry() {
    let (frame, tree) = frame(
        r##"<svg id=s width=60 height=40><defs>
      <symbol id=box viewBox="0 0 10 10"><rect width=10 height=10 fill=red /></symbol>
      <marker id=m markerUnits=userSpaceOnUse markerWidth=8 markerHeight=8 refX=0 refY=4 orient=auto>
      <rect width=8 height=8 fill=blue /></marker></defs>
      <use xlink:href="#box" x=10 y=10 width=20 height=20 />
      <path d="M40 0V20" fill=none stroke=green stroke-width=2 marker-end="url(#m)"/>
    </svg>"##,
        &FontContext::bundled(),
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let scene = find(&tree, id(&frame.document, "s"))
        .vector_scene()
        .unwrap();
    assert_eq!(paths(scene).len(), 3);
    let pixels = raster::render(scene, 60, 40, frame.font_context()).unwrap();
    assert_eq!(pixels[15 * 60 + 15], 0xffff0000);
    assert_eq!(
        pixels[23 * 60 + 40],
        0xff0000ff,
        "marker is rotated along the vertical tangent"
    );
    assert_eq!(pixels[23 * 60 + 48], 0);
}

#[test]
fn svg_text_keeps_document_face_clusters_and_original_dom_source_ranges() {
    let (frame, tree) = frame(
        r#"<svg id=s width=120 height=60><g font-size=24>
      <text x=0 y=30>  ffi  <tspan dx=2>A</tspan></text></g></svg>"#,
        &FontContext::bundled(),
    );
    let scene = find(&tree, id(&frame.document, "s"))
        .vector_scene()
        .unwrap();
    let runs = text(scene);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].glyphs.len(), 1, "real ffi ligature");
    let glyph = &runs[0].glyphs[0];
    assert_eq!(glyph.size, 24.0, "presentation font-size is inherited");
    assert!(Arc::ptr_eq(
        &glyph.face,
        &frame
            .font_context()
            .select(find(&tree, id(&frame.document, "s")).style())
    ));
    assert_eq!(
        glyph
            .source_spans
            .iter()
            .map(|s| &s.source[s.start..s.end])
            .collect::<String>(),
        "ffi"
    );
    assert!(
        glyph
            .source_spans
            .iter()
            .all(|s| s.node_id.is_some_and(|n| frame
                .document
                .node(n)
                .is_some_and(|n| matches!(n.kind(), crate::dom::NodeKind::Text(_)))))
    );
    assert!(
        raster::render(scene, 120, 60, frame.font_context())
            .unwrap()
            .iter()
            .any(|p| p >> 24 != 0)
    );
}

#[test]
fn math_fraction_scripts_radical_tables_and_phantoms_are_measured_not_flattened_text() {
    let (frame, tree) = frame(
        r#"<style>math{font-size:24px}</style>
      <math id=equation><mrow><mo>(</mo><mfrac><mi>a</mi><mi>b</mi></mfrac>
      <mo>+</mo><msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup>
      <mo>=</mo><mroot><mi>c</mi><mn>3</mn></mroot><mo>)</mo></mrow></math>
      <math id=matrix><mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>22</mn></mtd></mtr>
      <mtr><mtd><mn>333</mn></mtd><mtd><mn>4</mn></mtd></mtr></mtable></math>
      <math id=phantom><mphantom><mn>123</mn></mphantom><mspace width=2em height=1em /></math>"#,
        &FontContext::bundled(),
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let equation = find(&tree, id(&frame.document, "equation"));
    let scene = equation.vector_scene().unwrap();
    assert!(scene.baseline.unwrap() > 24.0);
    assert!(scene.height > 48.0);
    assert!(
        paths(scene).len() >= 2,
        "fraction rule and radical are native paths"
    );
    let runs = text(scene);
    assert!(runs.iter().flat_map(|r| &r.glyphs).any(|g| g.size < 24.0));
    assert!(
        equation.children().is_empty(),
        "foreign contents don't also become HTML text boxes"
    );
    let matrix = find(&tree, id(&frame.document, "matrix"))
        .vector_scene()
        .unwrap();
    assert!(matrix.height > 48.0 && matrix.width > 48.0);
    let phantom = find(&tree, id(&frame.document, "phantom"))
        .vector_scene()
        .unwrap();
    assert!(phantom.width > 48.0 && phantom.commands.is_empty());
    let line = tree
        .root()
        .children()
        .iter()
        .flat_map(|b| b.children())
        .flat_map(|b| b.line_boxes())
        .next();
    if let Some(line) = line {
        assert!((line.baseline - equation.content_rect().y - scene.baseline.unwrap()).abs() < 0.01);
    }
}

#[test]
fn svg_and_math_use_shared_block_flex_grid_table_object_fit_and_atomic_fragment_contracts() {
    let (frame, tree) = frame(
        r#"<style>section{display:flex}article{display:grid;grid-template-columns:80px 80px}
      svg{width:60px;height:40px;object-fit:contain}math{font-size:20px}</style>
      <section><svg id=s width=20 height=10><rect width=20 height=10 fill=red /></svg><math id=m><msup><mi>x</mi><mn>2</mn></msup></math></section>
      <article><svg id=g width=20 height=10><rect width=20 height=10 fill=blue /></svg><math><mfrac><mn>1</mn><mn>2</mn></mfrac></math></article>
      <table><tr><td><svg id=t width=20 height=10><rect width=20 height=10 /></svg></td></tr></table>"#,
        &FontContext::bundled(),
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let svg = find(&tree, id(&frame.document, "s"));
    assert_eq!(
        (svg.content_rect().width, svg.content_rect().height),
        (60.0, 40.0)
    );
    assert_eq!(
        (svg.object_rect().width, svg.object_rect().height),
        (60.0, 30.0)
    );
    assert_eq!(svg.object_rect().y - svg.content_rect().y, 5.0);
    for name in ["s", "g", "t", "m"] {
        let b = find(&tree, id(&frame.document, name));
        assert!(b.content_rect().width > 0.0 && b.content_rect().height > 0.0);
        assert!(b.children().is_empty());
        assert_eq!(
            tree.hit_test(b.rect().x + 1.0, b.rect().y + 1.0),
            b.dom_node_id()
        );
    }
    let clone = tree.clone();
    assert!(Arc::ptr_eq(
        svg.vector_scene().unwrap(),
        find(&clone, id(&frame.document, "s"))
            .vector_scene()
            .unwrap()
    ));
}

#[test]
fn unsupported_and_cyclic_content_is_diagnosed_and_vector_limits_fail_before_allocation() {
    let (_, tree) = frame(
        r##"<svg width=40 height=40><defs><g id=cycle><use href="#cycle"/></g></defs>
      <use href="#cycle"/><path d="not a path"/><foreignObject/><use href="https://bad.test/image.svg#x"/></svg>
      <math><menclose><mi>x</mi></menclose></math>"##,
        &FontContext::bundled(),
    );
    let messages = tree
        .diagnostics()
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    for expected in [
        "cyclic",
        "invalid SVG path",
        "foreignObject",
        "within this document",
        "menclose",
    ] {
        assert!(messages.contains(expected), "{messages}");
    }
    let scene = Scene::empty(8000.0, 8000.0);
    let list = vec![paint::DisplayItem::Vector {
        scene: Arc::new(scene),
        rect: crate::layout::Rect {
            x: 0.0,
            y: 0.0,
            width: 8000.0,
            height: 8000.0,
        },
    }];
    assert!(
        paint::compositor::validate(&list, 800, 600, 1.0)
            .unwrap_err()
            .contains("4096")
    );
    assert!(crate::media::DecodedImage::decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="https://bad.test/x.png"/></svg>"#).is_err());
}

#[test]
fn downloaded_svg_images_keep_vectors_and_reject_scripts_external_io_and_entity_expansion() {
    let bytes=br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 2 1"><path d="M0 0H2V1H0Z" fill="red"/></svg>"#;
    let image = crate::media::DecodedImage::decode(bytes).unwrap();
    assert!(image.rgba().is_empty());
    assert_eq!((image.width(), image.height()), (20, 10));
    let fractional=crate::media::DecodedImage::decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="4.5" height="2.5"><rect width="4.5" height="2.5"/></svg>"#).unwrap();
    assert_eq!(fractional.natural_size(), (4.5, 2.5));
    let mut utf16 = vec![0xff, 0xfe];
    for word in std::str::from_utf8(bytes).unwrap().encode_utf16() {
        utf16.extend(word.to_le_bytes());
    }
    assert!(
        crate::media::DecodedImage::decode(&utf16)
            .unwrap()
            .vector()
            .is_some()
    );
    assert!(
        raster::render(image.vector().unwrap(), 40, 20, &FontContext::bundled())
            .unwrap()
            .iter()
            .all(|p| *p == 0xffff0000)
    );
    assert!(
        crate::media::DecodedImage::decode(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><script>bad()</script></svg>"#
        )
        .is_err()
    );
    assert!(crate::media::DecodedImage::decode(br#"<!DOCTYPE svg SYSTEM "https://bad.test/entity"><svg xmlns="http://www.w3.org/2000/svg">&external;</svg>"#).is_err());
}

#[test]
fn math_inline_baseline_aligns_with_surrounding_text_and_screen_print_clones_keep_sources() {
    let (frame, tree) = frame(
        r#"<p id=host style="font:20px/24px sans-serif;margin:0">A<math id=m><mfrac><mi>x</mi><mn>2</mn></mfrac></math>A</p>"#,
        &FontContext::bundled(),
    );
    let math = find(&tree, id(&frame.document, "m"));
    let scene = math.vector_scene().unwrap();
    let line = &find(&tree, id(&frame.document, "host")).line_boxes()[0];
    assert!((line.baseline - math.content_rect().y - scene.baseline.unwrap()).abs() < 0.01);
    let clone = tree.clone();
    let copied = find(&clone, id(&frame.document, "m"))
        .vector_scene()
        .unwrap();
    assert!(Arc::ptr_eq(scene, copied));
    let a = text(scene);
    let b = text(copied);
    assert!(Arc::ptr_eq(&a[0].glyphs[0].face, &b[0].glyphs[0].face));
    let print = frame
        .print_layout(crate::engine::PrintOptions {
            width: 300.0,
            height: 200.0,
            margin: 10.0,
            ..Default::default()
        })
        .unwrap();
    assert!(print.diagnostics.is_empty(), "{:?}", print.diagnostics);
    let commands = print
        .pages
        .iter()
        .flat_map(|p| &p.display_list)
        .filter_map(|item| {
            if let paint::DisplayItem::Vector { scene, .. } = item {
                Some(scene)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert!(!commands.is_empty());
    assert_eq!(
        text(commands[0])[0].glyphs[0].source_spans,
        a[0].glyphs[0].source_spans
    );
}

#[test]
fn continuous_gradients_interpolate_and_repeat_at_independent_pixel_centers() {
    let (frame, tree) = frame(
        r##"<svg id=s width=40 height=40><defs>
                  <linearGradient id=linear gradientUnits=userSpaceOnUse x2=10 spreadMethod=repeat>
                  <stop stop-color=red /><stop offset=1 stop-color=blue /></linearGradient>
                  <radialGradient id=radial><stop stop-color=red /><stop offset=1 stop-color=blue /></radialGradient>
                  </defs><rect width=40 height=20 fill="url(#linear)"/>
                  <rect y=20 width=40 height=20 fill="url(#radial)"/></svg>"##,
        &FontContext::bundled(),
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let scene = find(&tree, id(&frame.document, "s"))
        .vector_scene()
        .unwrap();
    for scale in [1, 2] {
        let pixels = raster::render(scene, 40 * scale, 40 * scale, frame.font_context()).unwrap();
        for x in [2, 5, 12, 15, 22, 35] {
            let p = pixels[(5 * scale * 40 * scale + x * scale) as usize];
            let t = ((x as f32 + 0.5 / scale as f32) / 10.0).fract();
            let expected = [
                (255.0 * (1.0 - t)).round() as i32,
                (255.0 * t).round() as i32,
            ];
            assert!((((p >> 16) & 255) as i32 - expected[0]).abs() <= 1);
            assert!(((p & 255) as i32 - expected[1]).abs() <= 1);
        }
        let x = 20.0 + 0.5 / scale as f32;
        let y = 30.0 + 0.5 / scale as f32;
        let t = (((x - 20.0) / 40.0).powi(2) + ((y - 30.0) / 20.0).powi(2)).sqrt() / 0.5;
        let p = pixels[(30 * scale * 40 * scale + 20 * scale) as usize];
        assert!(((p & 255) as i32 - (255.0 * t).round() as i32).abs() <= 1);
    }
}

#[test]
fn selected_real_opentype_math_constants_drive_script_scale_axis_and_fraction_rule() {
    // Extend the repository's original CC0 geometric font in memory with an
    // original MATH constants table. Outlines remain real TrueType glyphs.
    let original = include_bytes!("../../assets/fonts/typography/GeometryVariable.ttf");
    let count = u16::from_be_bytes(original[4..6].try_into().unwrap()) as usize;
    let mut constants = vec![0u8; 214];
    for (offset, value) in [
        (0, 50_i16),
        (2, 50),
        (12, 250),
        (24, 250),
        (36, 400),
        (52, 200),
        (136, 150),
        (144, 50),
        (188, 100),
    ] {
        constants[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    let mut math = vec![0, 1, 0, 0, 0, 10, 0, 0, 0, 0];
    math.extend(constants);
    let checksum = |bytes: &[u8]| {
        bytes.as_chunks::<4>().0.iter().fold(0u32, |sum, word| {
            sum.wrapping_add(u32::from_be_bytes(*word))
        })
    };
    let mut padded = math.clone();
    while padded.len() % 4 != 0 {
        padded.push(0);
    }
    let mut records = Vec::<[u8; 16]>::new();
    for record in original[12..12 + count * 16].as_chunks::<16>().0 {
        let mut record = *record;
        let offset = u32::from_be_bytes(record[8..12].try_into().unwrap()) + 16;
        record[8..12].copy_from_slice(&offset.to_be_bytes());
        records.push(record);
    }
    let mut header = original[..12].to_vec();
    let n = (count + 1) as u16;
    let power = 1u16 << n.ilog2();
    header[4..6].copy_from_slice(&n.to_be_bytes());
    header[6..8].copy_from_slice(&(power * 16).to_be_bytes());
    header[8..10].copy_from_slice(&(n.ilog2() as u16).to_be_bytes());
    header[10..12].copy_from_slice(&(n * 16 - power * 16).to_be_bytes());
    let offset = (original.len() + 16).next_multiple_of(4) as u32;
    let mut record = [0u8; 16];
    record[..4].copy_from_slice(b"MATH");
    record[4..8].copy_from_slice(&checksum(&padded).to_be_bytes());
    record[8..12].copy_from_slice(&offset.to_be_bytes());
    record[12..16].copy_from_slice(&(math.len() as u32).to_be_bytes());
    records.push(record);
    records.sort_by_key(|r| r[..4].to_vec());
    let mut bytes = header;
    for record in &records {
        bytes.extend(record);
    }
    bytes.extend_from_slice(&original[12 + count * 16..]);
    bytes.resize(offset as usize, 0);
    bytes.extend(padded);
    let head = records.iter().find(|r| &r[..4] == b"head").unwrap();
    let adjustment = u32::from_be_bytes(head[8..12].try_into().unwrap()) as usize + 8;
    bytes[adjustment..adjustment + 4].fill(0);
    let value = 0xb1b0afba_u32.wrapping_sub(checksum(&bytes));
    bytes[adjustment..adjustment + 4].copy_from_slice(&value.to_be_bytes());
    let mut database = fontdb::Database::new();
    database.load_font_data(bytes);
    let fonts = FontContext::from_database(database);
    let (frame, tree) = frame(
        r#"<math id=m style="font:20px 'Geometry Variable'"><mfrac><mn>A</mn><mn>A</mn></mfrac></math>"#,
        &fonts,
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let scene = find(&tree, id(&frame.document, "m"))
        .vector_scene()
        .unwrap();
    assert_eq!(text(scene)[0].glyphs[0].size, 10.0);
    assert!(
        text(scene)[0].glyphs[0]
            .face
            .shaper()
            .tables()
            .math
            .is_some()
    );
    assert_eq!(scene.baseline, Some(18.5));
    assert_eq!(scene.height, 27.0);
    let rules = paths(scene);
    assert_eq!(rules[0].bounds().height(), 1.0);
    assert_eq!(rules[0].bounds().y(), 13.0);
}

#[test]
fn css_sized_svg_uses_final_viewport_without_rescaling_unviewboxed_fonts_or_user_units() {
    let (frame, tree) = frame(
        r#"<svg id=s width=20 height=10 style="display:block;width:100px;height:50px;font-size:20px">
      <rect width="50%" height=10 fill=red /><text x=0 y=40>A</text></svg>"#,
        &FontContext::bundled(),
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    let scene = find(&tree, id(&frame.document, "s"))
        .vector_scene()
        .unwrap();
    assert_eq!((scene.width, scene.height), (100.0, 50.0));
    assert_eq!(paths(scene)[0].bounds().width(), 50.0);
    assert_eq!(text(scene)[0].glyphs[0].size, 20.0);
    let pixels = raster::render(scene, 100, 50, frame.font_context()).unwrap();
    assert_eq!(pixels[5 * 100 + 49], 0xffff0000);
    assert_eq!(pixels[5 * 100 + 50], 0);
}
