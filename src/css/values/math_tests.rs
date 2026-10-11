use super::math::{MAX_MATH_DEPTH, MAX_MATH_TOKENS};
use super::*;

fn used(source: &str, reference: f32) -> f32 {
    Length::parse(source)
        .unwrap()
        .used(&LengthContext::default(), reference)
        .unwrap()
        .0
}

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn percentage_dependent_exception_reports_the_compatibility_fallback() {
    let computed = SpecifiedLength::parse("calc(1000px / (100% - 200px) * 1px)")
        .unwrap()
        .compute(&LengthContext::default())
        .unwrap();
    assert_eq!(computed.use_value(400.0), Some(UsedLength(5.0)));
    assert_eq!(computed.use_value(200.0), None);
    let mut diagnostic = None;
    assert_eq!(
        computed
            .0
            .resolve_with_diagnostic(16.0, 200.0, |message| diagnostic = Some(message)),
        0.0
    );
    let diagnostic = diagnostic.unwrap();
    for context in [
        "invalid at used-value time",
        "non-finite CSS length result",
        "value=Math(",
        "font-size=16px",
        "reference=200px",
        "compatibility fallback is 0px",
    ] {
        assert!(diagnostic.contains(context), "{diagnostic}");
    }
    assert_eq!(computed.use_value(200.0), None);
    assert_eq!(
        computed
            .0
            .resolve_with_diagnostic(16.0, 400.0, |_| panic!("finite value reported")),
        5.0
    );
}

#[test]
fn used_line_height_rejects_overflow_before_compatibility_resolution() {
    assert_eq!(
        LineHeight::Number(2.0).used_value(16.0),
        Some(UsedLength(32.0))
    );
    for (line_height, font_size) in [
        (LineHeight::Normal, f32::MAX),
        (LineHeight::Number(1e38), 16.0),
        (LineHeight::Length(Length::Em(1e38)), 16.0),
    ] {
        assert_eq!(line_height.used_value(font_size), None);
        let mut diagnostic = None;
        assert_eq!(
            line_height.used_with_diagnostic(font_size, |message| diagnostic = Some(message)),
            0.0
        );
        let diagnostic = diagnostic.unwrap();
        assert!(diagnostic.contains("non-finite CSS line-height result"));
        assert!(diagnostic.contains(&format!("value={line_height:?}")));
        assert!(diagnostic.contains(&format!("font-size={font_size}px")));
        assert!(diagnostic.contains("compatibility fallback is 0px"));
    }
}

#[test]
fn arithmetic_obeys_css_tokens_whitespace_precedence_and_types() {
    for (source, expected) in [
        ("calc(100% - 2rem)", 168.0),
        ("calc(1px + 2px * 3)", 7.0),
        ("calc((1px + 2px) * 3)", 9.0),
        ("calc(20px/2/2)", 5.0),
        ("calc(-1px - -2px)", 1.0),
        ("calc(2px * 3px / 2px)", 3.0),
        ("calc(2s / 1s * 3px)", 6.0),
        ("c\\61 lc(1p\\78  + 2px)", 3.0),
        ("calc(1px /*ok*/ + /*ok*/ 2px)", 3.0),
    ] {
        near(used(source, 200.0), expected);
    }
    for source in [
        "calc(1px+2px)",
        "calc(1px +2px)",
        "calc(1px+ 2px)",
        "calc(1px -2px)",
        "calc(1px/**/+/**/2px)",
        "calc(1px + 2)",
        "calc(1px + 1s)",
        "calc(1px * 1px)",
        "calc(1 / 1px)",
        "calc(1px / 1px)",
        "calc(0)",
        "calc()",
        "calc(1px,2px)",
        "calc(1px +)",
        "calc(- 1px)",
        "min(1px,)",
        "min()",
        "clamp(1px,2px)",
        "clamp(1px,2px,3px,4px)",
        "(1px)",
        "1px 2px",
        "calc(1px))",
        "calc(1qu)",
        "pi",
    ] {
        assert!(Length::parse(source).is_none(), "{source}");
    }
}

#[test]
fn comparison_and_math_functions_preserve_percentage_dependencies() {
    for (source, expected) in [
        ("min(20px, 50%, 3rem)", 20.0),
        ("max(20px, 50%, 3rem)", 100.0),
        ("clamp(20px, 50%, 80px)", 80.0),
        ("clamp(90px, 10px, 20px)", 90.0),
        ("calc(min(20px, 50%) + max(2px, 1px))", 22.0),
        ("abs(-10px)", 10.0),
        ("hypot(3px,4px)", 5.0),
        ("mod(-5px,3px)", 1.0),
        ("mod(5px,-3px)", -1.0),
        ("rem(-5px,3px)", -2.0),
        ("round(15px,10px)", 20.0),
        ("round(nearest,-15px,10px)", -10.0),
        ("round(up,11px,10px)", 20.0),
        ("round(down,-11px,10px)", -20.0),
        ("round(to-zero,-11px,10px)", -10.0),
        ("calc(sign(-3px) * 10px)", -10.0),
        ("calc(sin(90deg) * 10px)", 10.0),
        ("calc(cos(0) * 10px)", 10.0),
        ("calc(tan(0deg) * 10px)", 0.0),
        ("calc(atan2(1px,1px) / 45deg * 10px)", 10.0),
        ("calc(asin(1) / 90deg * 10px)", 10.0),
        ("calc(acos(0) / 90deg * 10px)", 10.0),
        ("calc(atan(1) / 45deg * 10px)", 10.0),
        ("calc(pow(2,3) * 1px)", 8.0),
        ("calc(sqrt(9) * 1px)", 3.0),
        ("calc(log(8,2) * 1px)", 3.0),
        ("calc(log(e) * 1px)", 1.0),
        ("calc(exp(0) * 1px)", 1.0),
        ("calc(round(2.5) * 1px)", 3.0),
    ] {
        near(used(source, 200.0), expected);
    }
    let expression = Length::parse("clamp(20px, 50%, 80px)").unwrap();
    let computed = expression.computed(&LengthContext::default()).unwrap();
    assert!(computed.0.has_percentage());
    near(computed.use_value(60.0).unwrap().0, 30.0);
    near(computed.use_value(400.0).unwrap().0, 80.0);
    for source in [
        "sin(1px)",
        "abs(1px,2px)",
        "atan2(1px,1s)",
        "pow(1px,2)",
        "sqrt(1px)",
        "log()",
        "exp(1,2)",
        "round(bad,2px,1px)",
        "round(2px)",
    ] {
        assert!(
            MathExpression::parse(source, Some(Dimension::LENGTH)).is_none(),
            "{source}"
        );
    }
}

#[test]
fn unit_context_and_value_stages_do_not_lose_relative_or_percentage_information() {
    let context = LengthContext {
        font_size: 20.0,
        root_font_size: 30.0,
        zero_advance: 11.0,
        x_height: 9.0,
        line_height: 40.0,
        root_line_height: 50.0,
        small_viewport: (300.0, 400.0),
        large_viewport: (500.0, 700.0),
        dynamic_viewport: (400.0, 600.0),
        ..LengthContext::default()
    };
    for (source, expected) in [
        ("2em", 40.0),
        ("2rem", 60.0),
        ("2ch", 22.0),
        ("2ex", 18.0),
        ("2lh", 80.0),
        ("2rlh", 100.0),
        ("10vw", 50.0),
        ("10vh", 70.0),
        ("10vmin", 50.0),
        ("10vmax", 70.0),
        ("10svw", 30.0),
        ("10svh", 40.0),
        ("10svmin", 30.0),
        ("10svmax", 40.0),
        ("10lvw", 50.0),
        ("10lvh", 70.0),
        ("10lvmin", 50.0),
        ("10lvmax", 70.0),
        ("10dvw", 40.0),
        ("10dvh", 60.0),
        ("10dvmin", 40.0),
        ("10dvmax", 60.0),
        ("10vi", 50.0),
        ("10vb", 70.0),
        ("10svi", 30.0),
        ("10dvb", 60.0),
        ("1in", 96.0),
        ("2.54cm", 96.0),
        ("25.4mm", 96.0),
        ("101.6q", 96.0),
        ("72pt", 96.0),
        ("6pc", 96.0),
    ] {
        near(
            Length::parse(source)
                .unwrap()
                .computed(&context)
                .unwrap()
                .use_value(100.0)
                .unwrap()
                .0,
            expected,
        );
    }
    let specified = SpecifiedLength::parse("calc(100% - 2rem)").unwrap();
    let computed = specified.compute(&context).unwrap();
    assert_ne!(specified.0, computed.0);
    let physical = SpecifiedLength::parse("1in").unwrap();
    assert_eq!(physical.0, Length::Absolute(1.0, AbsoluteUnit::In));
    assert_eq!(physical.compute(&context).unwrap().0, Length::Px(96.0));
    near(computed.use_value(200.0).unwrap().0, 140.0);
    assert_eq!(
        UsedLength(10.25).actual(0.25, 2.0).unwrap().device_pixels,
        21.0
    );
    assert!(UsedLength(10.0).actual(0.0, 0.0).is_none());
}

#[test]
fn standalone_angles_times_and_resolutions_are_typed_and_canonical() {
    near(
        Angle::parse("calc(1turn - 100grad)").unwrap().degrees,
        270.0,
    );
    near(Angle::parse("asin(1)").unwrap().degrees, 90.0);
    near(Angle::parse("3.141592653589793rad").unwrap().degrees, 180.0);
    near(Time::parse("calc(1s + 500ms)").unwrap().seconds, 1.5);
    near(Time::parse("-500ms").unwrap().seconds, -0.5);
    near(Resolution::parse("192dpi").unwrap().dppx, 2.0);
    near(Resolution::parse("37.79527559dpcm").unwrap().dppx, 1.0);
    near(Resolution::parse("2x").unwrap().dppx, 2.0);
    assert!(Angle::parse("1s").is_none());
    assert!(Time::parse("calc(1s + 1px)").is_none());
    assert!(Resolution::parse("100%").is_none());
    assert!(Resolution::parse("-1dppx").is_none());
    assert!(Angle::parse("0").is_none());
    near(
        Time::parse_with_context(
            "calc(2em / 1px * 1ms)",
            &LengthContext {
                font_size: 40.0,
                ..LengthContext::default()
            },
        )
        .unwrap()
        .seconds,
        0.08,
    );
}

#[test]
fn invalid_results_and_parser_limits_fail_without_unbounded_recursion() {
    for source in [
        "calc(1px / 0)",
        "calc(sqrt(-1) * 1px)",
        "mod(1px,0px)",
        "round(2px,0px)",
        "round(2px,-1px)",
        "calc(exp(1000) * 1px)",
        "calc(1e38px * 10)",
        "calc(100% + sqrt(-1) * 1px)",
    ] {
        let parsed = Length::parse(source).unwrap();
        assert!(
            parsed.computed(&LengthContext::default()).is_none(),
            "{source}"
        );
    }
    for source in [
        "1e99px",
        "1e38in",
        "NaNpx",
        "infinity",
        "calc(NaN * 1px)",
        "calc(infinity * 1px)",
    ] {
        assert!(Length::parse(source).is_none(), "{source}");
    }
    let nested = format!(
        "{}1px{}",
        "calc(".repeat(MAX_MATH_DEPTH + 1),
        ")".repeat(MAX_MATH_DEPTH + 1)
    );
    assert!(Length::parse(&nested).is_none());
    let linear = format!("calc({})", vec!["1px"; MAX_MATH_DEPTH + 1].join(" + "));
    assert!(Length::parse(&linear).is_none());
    let wide = format!("min({})", vec!["1px"; MAX_MATH_TOKENS].join(","));
    assert!(Length::parse(&wide).is_none());
    let bounded = format!("min({})", vec!["1px"; 1000].join(","));
    near(used(&bounded, 0.0), 1.0);
}
