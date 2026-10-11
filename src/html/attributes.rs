/// The HTML non-negative integer algorithm accepts a leading integer, not CSS.
/// https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#non-negative-integers
pub(crate) fn image_dimension(value: &str) -> Option<f32> {
    let value: f32 = nonnegative_integer_digits(value)?.parse().ok()?;
    (value.is_finite() && value <= 1_000_000.0).then_some(value)
}

pub(crate) fn nonnegative_integer_digits(value: &str) -> Option<&str> {
    let value = value.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
    let digits = value.strip_prefix('+').unwrap_or(value);
    let end = digits
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(digits.len());
    (end != 0).then_some(&digits[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_dimensions_follow_nonnegative_integer_rules_and_are_bounded() {
        for value in ["20", " 20 ", "+20", "20px", "20%"] {
            assert_eq!(image_dimension(value), Some(20.0));
        }
        for value in [
            "",
            "-20",
            "++20",
            ".2",
            "NaN",
            "9999999999999999999999999999",
        ] {
            assert_eq!(image_dimension(value), None);
        }
        assert_eq!(image_dimension("0"), Some(0.0));
    }
}
