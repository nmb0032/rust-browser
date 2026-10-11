use super::*;

#[test]
fn compatibility_resolution_aliases_use_numbers_as_dppx_not_resolution_dimensions() {
    let environment = MediaEnvironment {
        resolution_dppx: Some(2.0),
        ..MediaEnvironment::default()
    };
    for (source, expected) in [
        ("(-webkit-device-pixel-ratio:2)", true),
        ("(-webkit-device-pixel-ratio:1)", false),
        ("(-webkit-min-device-pixel-ratio:1.5)", true),
        ("(-webkit-max-device-pixel-ratio:1.5)", false),
        ("(1 < -webkit-device-pixel-ratio <= 2)", true),
        ("(-webkit-device-pixel-ratio)", true),
        ("(-webkit-device-pixel-ratio:2dppx)", false),
        ("(min--webkit-device-pixel-ratio:1)", false),
        ("(max--webkit-device-pixel-ratio:3)", false),
        ("(-webkit-transform-3d)", false),
        ("(-webkit-transform-3d:0)", true),
        ("(-webkit-transform-3d:1)", false),
        ("not (-webkit-transform-3d)", true),
        ("(-webkit-transform-3d:2)", false),
        ("(-webkit-transform-3d > 0)", false),
    ] {
        assert_eq!(
            MediaQueryList::parse(source).matches(environment),
            expected,
            "{source}"
        );
    }
    assert!(
        !MediaQueryList::parse("not (-webkit-device-pixel-ratio:2)")
            .matches(MediaEnvironment::default())
    );
    assert!(
        !MediaQueryList::parse_container("(-webkit-device-pixel-ratio:2)")
            .container_evaluation(environment, (100.0, 100.0), LengthContext::default())
            .0
    );
}
