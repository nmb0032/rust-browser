use rust_browser::{
    css::values::{ColorSpace, ResolvedColor, parse_color},
    display::{ColorProfile, ViewScale},
    engine::PrintOptions,
    media::{DecodedImage, ImageColorSource},
    reftest,
    screenshot::{self, Options, PrintCapture, Screenshot},
    text::FontContext,
};
use std::{path::PathBuf, process::Command};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture(name: &str) -> PathBuf {
    root().join("tests/fixtures/display").join(name)
}

fn at(image: &Screenshot, x: u32, y: u32) -> [u8; 4] {
    image.rgba[((y * image.width + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}

fn node(frame: &rust_browser::engine::Frame, name: &str) -> rust_browser::dom::NodeId {
    frame
        .document
        .preorder()
        .find(|&id| {
            frame
                .document
                .node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

#[test]
fn known_css_primary_matrices_are_not_a_clipped_srgb_round_trip() {
    let srgb_red = ResolvedColor::from_rgb(0xff0000);
    let p3 = srgb_red.convert(ColorSpace::DisplayP3);
    // Independently published CSS Color 4 D65 conversion (not an engine capture).
    for (actual, expected) in
        p3.channels
            .into_iter()
            .zip([0.9174875573, 0.2002868077, 0.1385605912])
    {
        assert!((actual - expected).abs() < 1e-8);
    }
    assert_eq!(ColorProfile::DisplayP3.color(srgb_red), (0xea3323, 255));
    let wide = parse_color("color(display-p3 1 0 0)")
        .unwrap()
        .resolve(ResolvedColor::BLACK)
        .unwrap();
    let xyz = wide.convert(ColorSpace::XyzD65);
    for (actual, expected) in xyz
        .channels
        .into_iter()
        .zip([0.4865709486, 0.2289745641, 0.0])
    {
        assert!((actual - expected).abs() < 1e-9);
    }
    assert_eq!(ColorProfile::DisplayP3.color(wide), (0xff0000, 255));
    assert_ne!(
        ColorProfile::DisplayP3.color(wide).0,
        ColorProfile::DisplayP3.rgb(wide.to_srgb8().0)
    );
}

#[test]
fn original_icc_png_cicp_and_gamma_fixtures_convert_without_touching_alpha() {
    let decoded = DecodedImage::decode(&std::fs::read(fixture("p3-alpha.png")).unwrap()).unwrap();
    assert_eq!(decoded.color_source(), ImageColorSource::Icc);
    assert_eq!(
        decoded.rgba_for(ColorProfile::DisplayP3),
        [128, 180, 40, 128]
    );
    for (&actual, expected) in decoded.rgba().iter().zip([112u8, 182, 0, 128]) {
        assert!(actual.abs_diff(expected) <= 1, "{:?}", decoded.rgba());
    }
    let cicp = DecodedImage::decode(&std::fs::read(fixture("p3-cicp.png")).unwrap()).unwrap();
    assert_eq!(cicp.color_source(), ImageColorSource::PngCicp);
    assert_eq!(cicp.rgba_for(ColorProfile::DisplayP3), [255, 0, 0, 255]);
    let gamma = DecodedImage::decode(&std::fs::read(fixture("linear.png")).unwrap()).unwrap();
    assert_eq!(gamma.color_source(), ImageColorSource::PngGamma);
    assert_eq!(gamma.rgba(), [188, 188, 188, 255]);
    for file in ["invalid-icc.png", "unsupported-hdr.png"] {
        assert!(
            DecodedImage::decode(&std::fs::read(fixture(file)).unwrap()).is_err(),
            "{file}"
        );
    }
}

#[test]
fn independent_profile_rect_image_gradient_pattern_references_match_at_both_dprs() {
    for scale in [1.0, 2.0] {
        for (profile, name) in [
            (ColorProfile::Srgb, "srgb"),
            (ColorProfile::DisplayP3, "p3"),
        ] {
            let actual = screenshot::capture(
                &fixture("profile.html"),
                &root(),
                &Options {
                    width: 80,
                    height: 40,
                    scale,
                    color_profile: profile,
                    ..Default::default()
                },
            )
            .unwrap();
            // Read independently generated encoded bytes, not a second engine render.
            let raw = image::open(fixture(&format!("profile-{name}-{}.png", scale as u32)))
                .unwrap()
                .to_rgba8();
            let expected = Screenshot {
                width: raw.width(),
                height: raw.height(),
                rgba: raw.into_raw(),
                color_profile: profile,
            };
            let (metrics, _) = reftest::compare(&actual, &expected).unwrap();
            assert!(
                metrics.max_difference <= 1,
                "{scale} {profile:?}: {metrics:?}"
            );
            if profile == ColorProfile::DisplayP3 {
                assert_eq!(
                    at(&actual, (25.0 * scale) as u32, (5.0 * scale) as u32),
                    [255, 0, 0, 255]
                );
                assert_eq!(
                    at(&actual, (5.0 * scale) as u32, (5.0 * scale) as u32),
                    [234, 51, 35, 255]
                );
            }
        }
    }
}

#[test]
fn png_output_is_tagged_with_correct_d65_full_range_profile_and_round_trips() {
    std::fs::create_dir_all(root().join("target/display-tests")).unwrap();
    for profile in [ColorProfile::Srgb, ColorProfile::DisplayP3] {
        let image = screenshot::capture(
            &fixture("profile.html"),
            &root(),
            &Options {
                width: 80,
                height: 40,
                color_profile: profile,
                ..Default::default()
            },
        )
        .unwrap();
        let path = root().join(format!("target/display-tests/{profile:?}.png"));
        image.save(&path).unwrap();
        let reader =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()))
                .read_info()
                .unwrap();
        if profile == ColorProfile::Srgb {
            assert!(reader.info().srgb.is_some());
            assert!(reader.info().icc_profile.is_none());
        } else {
            assert!(reader.info().srgb.is_none());
            assert_eq!(
                &reader.info().icc_profile.as_ref().unwrap()[24..36],
                &[0x07, 0xea, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0]
            );
            let icc =
                moxcms::ColorProfile::new_from_slice(reader.info().icc_profile.as_ref().unwrap())
                    .unwrap();
            let cicp = icc.cicp.unwrap();
            assert_eq!(cicp.color_primaries, moxcms::CicpColorPrimaries::Smpte432);
            assert_eq!(
                cicp.matrix_coefficients,
                moxcms::MatrixCoefficients::Identity
            );
            assert!(cicp.full_range);
        }
        let read = Screenshot::read_png_profile(&path, profile).unwrap();
        assert_eq!(read, image);
        let bytes = std::fs::read(&path).unwrap();
        image.save(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn profile_conversion_reaches_borders_shadows_alpha_transform_css_svg_and_math_text() {
    for (dpr, zoom) in [(1.0, 1.0), (2.0, 1.0), (1.0, 2.0), (2.0, 2.0)] {
        let actual = screenshot::capture(
            &fixture("effects.html"),
            &root(),
            &Options {
                width: (140.0 * zoom) as u32,
                height: (100.0 * zoom) as u32,
                scale: dpr,
                zoom,
                color_profile: ColorProfile::DisplayP3,
                ..Default::default()
            },
        )
        .unwrap();
        let scale = (dpr * zoom) as u32;
        for ((x, y), color) in [
            ((1, 1), [255, 0, 0, 255]),      // P3 border
            ((5, 5), [234, 51, 35, 255]),    // sRGB background
            ((55, 5), [255, 0, 0, 255]),     // P3 drop shadow
            ((75, 5), [255, 127, 127, 255]), // one group opacity application
            ((115, 5), [255, 0, 0, 255]),    // transformed group, identity CSS filter
            ((5, 40), [255, 0, 0, 255]),     // original rectangular CSS glyph
            ((35, 40), [255, 0, 0, 255]),    // exact SVG glyph face/id
            ((65, 40), [255, 0, 0, 255]),    // MathML glyph
        ] {
            assert_eq!(at(&actual, x * scale, y * scale), color, "{dpr}: {x},{y}");
        }
    }
}

#[test]
fn fractional_dpr_zoom_rasterizes_original_rectangular_glyph_at_device_time() {
    let options = Options {
        width: 80,
        height: 80,
        scale: 2.0,
        zoom: 1.25,
        color_profile: ColorProfile::DisplayP3,
        ..Default::default()
    };
    let actual = screenshot::capture(&fixture("glyph.html"), &root(), &options).unwrap();
    // Original CC0 geometric font: 1000 UPM; width 500, top 700, advance 600.
    // Direct independent fontdue outline raster at 41 * (2 * 1.25) ppem.
    let font = fontdue::Font::from_bytes(
        include_bytes!("../assets/fonts/typography/GeometryVariable.ttf").as_slice(),
        fontdue::FontSettings::default(),
    )
    .unwrap();
    let (metrics, mask) = font.rasterize('A', 102.5);
    let mut expected = vec![255; 160 * 160 * 4];
    let left = (3.0f32 * 2.5).round() as i32 + metrics.xmin;
    // Font ascent 800/1000, line-height=font-size, baseline=3+32.8 CSS px.
    let top = ((3.0f32 + 32.8) * 2.5).round() as i32 - metrics.ymin - metrics.height as i32;
    for y in 0..metrics.height {
        for x in 0..metrics.width {
            let offset = ((top as usize + y) * 160 + left as usize + x) * 4;
            let coverage = mask[y * metrics.width + x];
            expected[offset..offset + 4].copy_from_slice(&[
                255,
                255 - coverage,
                255 - coverage,
                255,
            ]);
        }
    }
    assert_eq!(actual.rgba, expected);
    assert_eq!((metrics.width, metrics.height), (52, 72));
    assert!(mask.iter().any(|&coverage| coverage > 0 && coverage < 255));
}
#[test]
fn zoom_contract_reflows_media_queries_scrolling_fixed_boxes_and_css_hits() {
    for dpr in [1.0, 2.0] {
        let options = Options {
            width: 200,
            height: 160,
            scale: dpr,
            zoom: 2.0,
            scroll: rust_browser::layout::ScrollOffset { x: 0.0, y: 100.0 },
            ..Default::default()
        };
        let mut frame = screenshot::load(&fixture("zoom.html"), &root(), &options).unwrap();
        assert_eq!(frame.view_scale(), ViewScale::new(dpr, 2.0).unwrap());
        assert_eq!(frame.media_environment().viewport_width, 100.0);
        assert_eq!(frame.media_environment().viewport_height, 80.0);
        assert_eq!(
            frame.media_environment().resolution_dppx,
            Some((2.0 * dpr) as f32)
        );
        let actual = screenshot::render(&mut frame, &options).unwrap();
        assert_eq!(
            (actual.width, actual.height),
            ((200.0 * dpr) as u32, (160.0 * dpr) as u32)
        );
        let effective = dpr as u32 * 2;
        assert_eq!(at(&actual, 2 * effective, 2 * effective), [0, 128, 0, 255]);
        assert_eq!(
            at(&actual, 11 * effective, 11 * effective),
            [255, 0, 0, 255]
        );
        assert_eq!(
            at(&actual, 41 * effective, 11 * effective),
            [0, 0, 255, 255]
        );
        let tree = frame.layout_with_container_queries(&FontContext::bundled());
        assert_eq!(tree.viewport_scroll().y, 100.0);
        let link = node(&frame, "link");
        assert_eq!(
            tree.hit_test_viewport(11.0, 11.0, tree.viewport_scroll().y),
            Some(link)
        );
        assert_ne!(
            tree.hit_test_viewport(31.0, 11.0, tree.viewport_scroll().y),
            Some(link)
        );
    }
}

#[test]
fn zoom_is_retained_and_does_not_rewrite_resources_or_author_colors() {
    let mut frame = screenshot::load(
        &fixture("profile.html"),
        &root(),
        &Options {
            width: 80,
            height: 40,
            ..Default::default()
        },
    )
    .unwrap();
    let id = node(&frame, "image");
    let image = frame.images[&id].clone();
    let document = frame.clone();
    frame.set_media_environment(rust_browser::css::media::MediaEnvironment {
        small_viewport: Some((80.0, 30.0)),
        large_viewport: Some((80.0, 40.0)),
        dynamic_viewport: Some((80.0, 35.0)),
        ..frame.media_environment()
    });
    assert!(frame.set_page_zoom(2.0).unwrap());
    assert_eq!(frame.media_environment().viewport_width, 40.0);
    assert_eq!(frame.media_environment().small_viewport, Some((40.0, 15.0)));
    assert_eq!(frame.media_environment().large_viewport, Some((40.0, 20.0)));
    assert_eq!(
        frame.media_environment().dynamic_viewport,
        Some((40.0, 17.5))
    );
    assert!(!frame.set_page_zoom(2.0).unwrap());
    assert!(std::sync::Arc::ptr_eq(&image, &frame.images[&id]));
    assert_eq!(frame.scroll_state(), document.scroll_state());
    assert_eq!(
        frame
            .author_styles
            .get(node(&frame, "p3"))
            .unwrap()
            .background
            .space,
        ColorSpace::DisplayP3
    );
    assert!(frame.clone().view_scale().zoom == 2.0);
    assert!(frame.set_page_zoom(f64::NAN).is_err());
    assert_eq!(frame.view_scale().zoom, 2.0);
}

#[test]
fn zoom_fragment_reveal_uses_css_scroll_coordinates_at_every_dpr() {
    for dpr in [1.0, 2.0] {
        let options = Options {
            width: 200,
            height: 160,
            scale: dpr,
            zoom: 2.0,
            fragment: Some("link".into()),
            ..Default::default()
        };
        let mut frame = screenshot::load(&fixture("zoom.html"), &root(), &options).unwrap();
        let actual = screenshot::render(&mut frame, &options).unwrap();
        assert_eq!(frame.scroll_state().viewport_scroll().y, 110.0);
        let effective = (2.0 * dpr) as u32;
        assert_eq!(at(&actual, 11 * effective, effective), [255, 0, 0, 255]);
    }
}

#[test]
fn output_and_effective_scale_fail_explicitly_at_finite_bounds() {
    for (dpr, zoom) in [
        (f64::NAN, 1.0),
        (1.0, f64::INFINITY),
        (0.125, 0.25),
        (4.0, 4.0),
        (1.0, 0.0),
    ] {
        assert!(ViewScale::new(dpr, zoom).is_err());
        assert!(
            Options {
                scale: dpr,
                zoom,
                ..Default::default()
            }
            .dimensions()
            .is_err()
        );
    }
    assert!(
        ColorProfile::DisplayP3
            .validate_native()
            .unwrap_err()
            .contains("cannot tag/present")
    );
    assert!(ColorProfile::Srgb.validate_native().is_ok());
    let wide = Screenshot {
        width: 1,
        height: 1,
        rgba: vec![255, 0, 0, 255],
        color_profile: ColorProfile::DisplayP3,
    };
    let srgb = Screenshot {
        color_profile: ColorProfile::Srgb,
        ..wide.clone()
    };
    assert!(reftest::compare(&wide, &srgb).is_err());
}

#[test]
fn physical_print_geometry_ignores_page_zoom_but_raster_magnification_uses_it() {
    let mut options = Options {
        width: 80,
        height: 40,
        print: Some(PrintCapture {
            options: PrintOptions {
                width: 80.0,
                height: 40.0,
                margin: 0.0,
                ..Default::default()
            },
            page: 1,
        }),
        ..Default::default()
    };
    let frame = screenshot::load(&fixture("profile.html"), &root(), &options).unwrap();
    let layout = frame.print_layout(options.print.unwrap().options).unwrap();
    let sheet = layout.pages[0].sheet;
    let one = screenshot::capture(&fixture("profile.html"), &root(), &options).unwrap();
    options.zoom = 2.0;
    let zoomed_frame = screenshot::load(&fixture("profile.html"), &root(), &options).unwrap();
    let zoomed_layout = zoomed_frame
        .print_layout(options.print.unwrap().options)
        .unwrap();
    assert_eq!(zoomed_layout.pages[0].sheet, sheet);
    assert_eq!(
        zoomed_layout.pages[0].display_list,
        layout.pages[0].display_list
    );
    let two = screenshot::capture(&fixture("profile.html"), &root(), &options).unwrap();
    assert_eq!((one.width, one.height), (80, 40));
    assert_eq!((two.width, two.height), (160, 80));
    assert_eq!(at(&two, 50, 10), at(&one, 25, 5));
}

#[test]
fn real_cli_zoom_and_profile_metadata_work_and_native_p3_is_rejected_before_window_creation() {
    std::fs::create_dir_all(root().join("target/display-tests")).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .current_dir(root())
        .args([
            "tests/pages/display.html",
            "--screenshot",
            "target/display-tests/demo.png",
            "--viewport",
            "600x500",
            "--device-scale",
            "2",
            "--page-zoom",
            "1.25",
            "--color-profile",
            "display-p3",
            "--resource-root",
            ".",
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let reader = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(root().join("target/display-tests/demo.png")).unwrap(),
    ))
    .read_info()
    .unwrap();
    assert_eq!((reader.info().width, reader.info().height), (1200, 1000));
    assert!(reader.info().icc_profile.is_some());
    let native = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .current_dir(root())
        .args(["tests/pages/display.html", "--color-profile", "display-p3"])
        .output()
        .unwrap();
    assert!(!native.status.success());
    assert!(String::from_utf8_lossy(&native.stderr).contains("cannot tag/present"));
}
