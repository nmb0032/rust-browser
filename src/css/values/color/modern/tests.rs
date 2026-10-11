use super::*;
use crate::css::values::parse_color;

fn color(source: &str) -> ResolvedColor {
    parse_color(source)
        .unwrap_or_else(|| panic!("parse: {source}"))
        .resolve(ResolvedColor::BLACK)
        .unwrap()
}
fn close(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
    for i in 0..3 {
        assert!(
            (actual[i] - expected[i]).abs() < tolerance,
            "{actual:?} != {expected:?}"
        );
    }
}

#[test]
fn neutral_primary_reference_vectors_and_adaptation() {
    close(
        color("color(srgb 1 0 0)")
            .convert(ColorSpace::XyzD65)
            .channels,
        [0.4123907993, 0.2126390059, 0.0193308187],
        1e-9,
    );
    close(
        color("color(display-p3 1 0 0)")
            .convert(ColorSpace::XyzD65)
            .channels,
        [0.4865709486, 0.2289745641, 0.0],
        1e-9,
    );
    close(
        color("color(a98-rgb 1 0 0)")
            .convert(ColorSpace::XyzD65)
            .channels,
        [0.5766690429, 0.2973449753, 0.0270313614],
        1e-9,
    );
    close(
        color("color(rec2020 1 0 0)")
            .convert(ColorSpace::XyzD65)
            .channels,
        [0.6369580483, 0.2627002120, 0.0],
        1e-9,
    );
    close(
        color("color(prophoto-rgb 1 0 0)")
            .convert(ColorSpace::XyzD50)
            .channels,
        [0.7977666449, 0.2880748288, 0.0],
        1e-9,
    );
    close(
        color("white").convert(ColorSpace::XyzD50).channels,
        D50,
        1e-7,
    );
    close(
        color("lab(100 0 0)").convert(ColorSpace::Srgb).channels,
        [1.0; 3],
        1e-6,
    );
    close(
        color("lab(0 0 0)").convert(ColorSpace::Srgb).channels,
        [0.0; 3],
        1e-7,
    );
    close(
        color("red").convert(ColorSpace::Lab).channels,
        [54.29054, 80.80492, 69.89099],
        1e-4,
    );
    close(
        color("red").convert(ColorSpace::Oklab).channels,
        [0.62795536, 0.22486306, 0.12584630],
        1e-7,
    );
    close(
        color("color(srgb-linear .2140411405 .2140411405 .2140411405)")
            .convert(ColorSpace::Srgb)
            .channels,
        [0.5; 3],
        1e-9,
    );
}

#[test]
fn conversions_round_trip_extended_range_all_spaces() {
    for space in [
        ColorSpace::Srgb,
        ColorSpace::SrgbLinear,
        ColorSpace::DisplayP3,
        ColorSpace::A98Rgb,
        ColorSpace::ProphotoRgb,
        ColorSpace::Rec2020,
        ColorSpace::XyzD50,
        ColorSpace::XyzD65,
        ColorSpace::Lab,
        ColorSpace::Lch,
        ColorSpace::Oklab,
        ColorSpace::Oklch,
        ColorSpace::Hsl,
        ColorSpace::Hwb,
    ] {
        for rgb in [[0.2, 0.4, 0.7], [1.0, 0.0, 0.0], [0.5; 3]] {
            let original = ResolvedColor::srgb(rgb[0], rgb[1], rgb[2], 0.37);
            let intermediate = original.convert(space);
            let back = intermediate.convert(ColorSpace::Srgb);
            close(back.channels, rgb, 1e-6);
            assert_eq!(back.alpha, 0.37);
        }
        if !matches!(space, ColorSpace::Hsl | ColorSpace::Hwb) {
            let rgb = ResolvedColor::srgb(-0.1, 1.2, 0.7, 1.0);
            close(
                rgb.convert(space).convert(ColorSpace::Srgb).channels,
                rgb.channels,
                1e-6,
            );
        }
    }
}

#[test]
fn modern_legacy_grammar_alpha_and_math() {
    for source in [
        "rgb(255,0,0)",
        "rgba(255, 0, 0, 1)",
        "rgb(100% 0 0%)",
        "hsl(0 100% 50%)",
        "hsla(360,100%,50%,1)",
        "hwb(0 0% 0%)",
        "hsl(0 100 50)",
        "hsl(1turn 100% 50%)",
        "rgb(calc(250 + 5) 0 0)",
    ] {
        assert_eq!(color(source).to_srgb8(), (0xff0000, 255), "{source}");
    }
    assert_eq!(color("#f008").to_srgb8(), (0xff0000, 136));
    assert_eq!(color("#ff000080").to_srgb8(), (0xff0000, 128));
    assert_eq!(color("rgba(255 0 0 / 50%)").to_srgb8(), (0xff0000, 128));
    assert_eq!(color("rgb(-100 999 0 / 2)").to_srgb8(), (0x00ff00, 255));
    assert_eq!(color("hwb(0 80% 80%)").to_srgb8(), (0x808080, 255));
    close(
        color("lab(50% 20% -20%)").channels,
        [50.0, 25.0, -25.0],
        1e-8,
    );
    close(color("oklch(50% 50% 90)").channels, [0.5, 0.2, 90.0], 1e-8);
    for source in [
        "rgb(1,2%,3)",
        "rgb(1,2,3 / .5)",
        "rgb(1 2,3)",
        "rgb(1/**/2/**/3)",
        "rgb(1+2+3)",
        "rgb(1 2)",
        "rgb(1 2 3) red",
        "rgb(1, none, 3)",
        "hsl(0,100,50)",
        "hwb(0, 0%, 0%)",
        "lab(50 1 2 3)",
        "color(lab 50 0 0)",
        "color(foo 1 2 3)",
        "rgb(1e999 0 0)",
        "color(srgb NaN 0 0)",
        "rgb(1 2 3 / .5 / .5)",
        "rgb(calc(1px) 0 0)",
        "hsl(50% 100% 50%)",
        "#ff",
        "#ggg",
        "\\ red\\ ",
        "rgb(from red calc(r + 10%) g b)",
        "rgb(from red x g b)",
    ] {
        assert!(parse_color(source).is_none(), "{source}");
    }
}

#[test]
fn relative_channels_retain_origin_space_and_currentcolor_dependency() {
    let r = parse_color("rgb(from currentcolor calc(r / 2) g b / calc(alpha * .5))").unwrap();
    let result = r.resolve(ResolvedColor::srgb(1.0, 0.2, 0.3, 0.6)).unwrap();
    close(result.channels, [0.5, 0.2, 0.3], 1e-7);
    assert!((result.alpha - 0.3).abs() < 1e-7);
    close(
        color("color(from color(display-p3 .7 .2 .1) display-p3 calc(r + .1) g b)").channels,
        [0.8, 0.2, 0.1],
        1e-7,
    );
    close(
        color("hsl(from red calc(h + 120) s l)")
            .convert(ColorSpace::Srgb)
            .channels,
        [0.0, 1.0, 0.0],
        1e-7,
    );
    close(
        color("oklch(from oklch(.6 .2 45 / .3) l calc(c / 2) h)").channels,
        [0.6, 0.1, 45.0],
        1e-7,
    );
    assert!((color("oklch(from oklch(.6 .2 45 / .3) l c h)").alpha - 0.3).abs() < 1e-7);
    let extended = color("rgb(from red calc(r * 2) g b)");
    assert_eq!(extended.channels[0], 2.0);
    assert_eq!(extended.to_srgb8(), (0xff0000, 255));
    close(
        color("oklch(from oklch(.8 .2 90) calc(l * 2) calc(c * -1) h)").channels,
        [1.6, -0.2, 90.0],
        1e-8,
    );
    close(
        color("hsl(from red h calc(s * 2) calc(l * 3))").channels,
        [0.0, 2.0, 1.5],
        1e-8,
    );
}

#[test]
fn interpolation_percentages_premultiplied_alpha_and_hue_arcs() {
    assert_eq!(
        color("color-mix(in srgb, red, blue)").to_srgb8(),
        (0x800080, 255)
    );
    assert_eq!(
        color("color-mix(in srgb-linear, red, blue)").to_srgb8(),
        (0xbc00bc, 255)
    );
    assert_eq!(
        color("color-mix(in srgb, red, transparent)").to_srgb8(),
        (0xff0000, 128)
    );
    let p = color("color-mix(in srgb, rgb(255 0 0 / .5) 25%, blue 25%)");
    close(p.channels, [1.0 / 3.0, 0.0, 2.0 / 3.0], 1e-7);
    assert_eq!(p.alpha, 0.375);
    assert_eq!(color("color-mix(in srgb, red 80%, blue 80%)").alpha, 1.0);
    assert_eq!(
        color("color-mix(in srgb, red 20%, blue)").to_srgb8(),
        (0x3300cc, 255)
    );
    assert_eq!(
        color("color-mix(in srgb, 20% red, blue)").to_srgb8(),
        (0x3300cc, 255)
    );
    assert_eq!(
        color("color-mix(in srgb, red calc(10% + 10%), blue)").to_srgb8(),
        (0x3300cc, 255)
    );
    for (method, expected) in [
        ("shorter", 0.0),
        ("longer", 180.0),
        ("increasing", 0.0),
        ("decreasing", 180.0),
    ] {
        let source = format!("color-mix(in lch {method} hue, lch(50 20 350), lch(50 20 10))");
        assert!(
            (color(&source).channels[2] - expected).abs() < 1e-7,
            "{source}"
        );
    }
    for source in [
        "color-mix(in srgb longer hue, red, blue)",
        "color-mix(in srgb, red 0%, blue 0%)",
        "color-mix(in srgb, red 101%, blue)",
        "color-mix(in srgb, red, blue, green)",
        "color-mix(in lch foo hue, red, blue)",
        "color-mix(in srgb, red20%, blue)",
    ] {
        assert!(parse_color(source).is_none(), "{source}");
    }
}

#[test]
fn missing_and_powerless_components_are_not_prematurely_zeroed() {
    let c = color("color-mix(in oklch, oklch(.5 none none / none), oklch(.7 .2 40 / .6))");
    close(c.channels, [0.6, 0.2, 40.0], 1e-7);
    assert_eq!(c.alpha, 0.6);
    let c = color("color-mix(in hsl, hsl(0 0% 50%), hsl(120 100% 50%))");
    assert!((c.channels[0] - 120.0).abs() < 1e-7);
    let c = color("color-mix(in lch, lch(50 .001 350), lch(50 20 40))");
    close(c.channels, [50.0, 10.0, 40.0], 1e-7);
    let c = color("color-mix(in oklch, gray, oklch(.6 .2 40))");
    assert!((c.channels[2] - 40.0).abs() < 1e-7);
    assert_eq!(color("gray").convert(ColorSpace::Oklch).missing & 4, 4);
    let c = color("color-mix(in srgb, rgb(none 0 0), rgb(none 255 255))");
    assert_eq!(c.missing, 1);
    assert_eq!(c.to_srgb8().0, 0x008080);
    let c = color("lab(none 10 20 / none)").convert(ColorSpace::Oklch);
    assert_eq!(c.missing & 9, 9);
    assert_eq!(color("rgb(from rgb(none 20 30) r g b)").missing, 1);
}

#[test]
fn wide_gamut_is_retained_until_output_and_resources_are_bounded() {
    let c = color("color(display-p3 1 0 0)");
    let srgb = c.convert(ColorSpace::Srgb);
    assert!(srgb.channels[0] > 1.0 && srgb.channels[1] < 0.0);
    assert_eq!(c.to_srgb8(), (0xff0000, 255));
    assert_eq!(c.space, ColorSpace::DisplayP3);
    assert_eq!(color("canvas").to_srgb8().0, 0xffffff);
    assert_eq!(color("CanvasText").to_srgb8().0, 0);
    let mut nested = "red".to_string();
    for _ in 0..MAX_DEPTH + 2 {
        nested = format!("rgb(from {nested} r g b)");
    }
    assert!(parse_color(&nested).is_none());
    assert!(parse_color(&format!("rgb({}1 2 3)", " ".repeat(MAX_TOKENS * 2))).is_some());
    assert!(parse_color(&format!("rgb(calc({}) 2 3)", "1 + ".repeat(MAX_TOKENS))).is_none());
}
