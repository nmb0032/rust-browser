use crate::style::{
    ComputedStyle,
    typography::{Property, Transform, Value},
};
use unicode_normalization::char::canonical_combining_class;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) struct Transformed {
    pub text: String,
    /// (display byte start, original byte start, original byte end).
    pub mapping: Vec<(usize, usize, usize)>,
}

pub(crate) fn apply(source: &str, style: &ComputedStyle, before: &str, after: &str) -> Transformed {
    let transform = match style.typography.get(Property::TextTransform) {
        Value::Transform(t) => t,
        _ => Transform::default(),
    };
    let language = style
        .font_language
        .as_deref()
        .unwrap_or("")
        .split('-')
        .next()
        .unwrap_or("");
    let turkic = language.eq_ignore_ascii_case("tr") || language.eq_ignore_ascii_case("az");
    let lithuanian = language.eq_ignore_ascii_case("lt");
    let mut result = Transformed {
        text: String::new(),
        mapping: Vec::new(),
    };
    let word_starts = source
        .split_word_bound_indices()
        .filter_map(|(start, word)| {
            word.char_indices()
                .find(|(_, c)| c.is_alphabetic())
                .map(|(i, _)| start + i)
        })
        .collect::<Vec<_>>();
    for (offset, c) in source.char_indices() {
        let prior = &source[..offset];
        let following = &source[offset + c.len_utf8()..];
        let case = if transform.case == 3 {
            if word_starts.binary_search(&offset).is_ok()
                && (offset != 0
                    || before
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_alphanumeric()))
            {
                1
            } else {
                0
            }
        } else {
            transform.case
        };
        let cased = |c: char| c.is_uppercase() || c.is_lowercase();
        let ignorable = |c: char| {
            canonical_combining_class(c) != 0
                || matches!(c, '\'' | '\u{2019}' | '\u{00ad}' | '\u{034f}')
        };
        let mut mapped = match (case, c) {
            (1, 'i') if turkic => "İ".into(),
            (1, 'ı') if turkic => "I".into(),
            (2, 'İ') if turkic => "i".into(),
            (2, 'I') if turkic => {
                let dot = following
                    .chars()
                    .take_while(|&c| canonical_combining_class(c) != 0)
                    .any(|c| c == '\u{0307}');
                if dot { "i".into() } else { "ı".into() }
            }
            (2, '\u{0307}')
                if turkic
                    && prior
                        .chars()
                        .rev()
                        .find(|&c| canonical_combining_class(c) == 0)
                        == Some('I') =>
            {
                String::new()
            }
            (2, 'Σ') => {
                let left = prior
                    .chars()
                    .rev()
                    .chain(before.chars().rev())
                    .find(|&c| !ignorable(c))
                    .is_some_and(cased);
                let right = following
                    .chars()
                    .chain(after.chars())
                    .find(|&c| !ignorable(c))
                    .is_some_and(cased);
                if left && !right {
                    "ς".into()
                } else {
                    "σ".into()
                }
            }
            (2, 'I' | 'J' | 'Į') if lithuanian => {
                let above = following
                    .chars()
                    .take_while(|&c| canonical_combining_class(c) != 0)
                    .any(|c| canonical_combining_class(c) == 230);
                let mut s: String = c.to_lowercase().collect();
                if above && !following.starts_with('\u{0307}') {
                    s.push('\u{0307}');
                }
                s
            }
            (2, 'Ì' | 'Í' | 'Ĩ') if lithuanian => format!(
                "i\u{0307}{}",
                match c {
                    'Ì' => '\u{0300}',
                    'Í' => '\u{0301}',
                    _ => '\u{0303}',
                }
            ),
            (1, '\u{0307}')
                if lithuanian
                    && prior
                        .chars()
                        .rev()
                        .find(|&c| canonical_combining_class(c) == 0)
                        .is_some_and(|c| matches!(c, 'i' | 'j' | 'į')) =>
            {
                String::new()
            }
            (1, _) => c.to_uppercase().collect(),
            (2, _) => c.to_lowercase().collect(),
            _ => c.to_string(),
        };
        if transform.full_width {
            mapped = mapped
                .chars()
                .map(|c| match c {
                    ' ' => '\u{3000}',
                    '!'..='~' => char::from_u32(c as u32 + 0xfee0).unwrap_or(c),
                    _ => c,
                })
                .collect();
        }
        if transform.full_size_kana {
            mapped = mapped
                .chars()
                .map(|c| match c {
                    'ぁ' => 'あ',
                    'ぃ' => 'い',
                    'ぅ' => 'う',
                    'ぇ' => 'え',
                    'ぉ' => 'お',
                    'っ' => 'つ',
                    'ゃ' => 'や',
                    'ゅ' => 'ゆ',
                    'ょ' => 'よ',
                    'ゎ' => 'わ',
                    'ゕ' => 'か',
                    'ゖ' => 'け',
                    'ァ' => 'ア',
                    'ィ' => 'イ',
                    'ゥ' => 'ウ',
                    'ェ' => 'エ',
                    'ォ' => 'オ',
                    'ッ' => 'ツ',
                    'ャ' => 'ヤ',
                    'ュ' => 'ユ',
                    'ョ' => 'ヨ',
                    'ヮ' => 'ワ',
                    'ヵ' => 'カ',
                    'ヶ' => 'ケ',
                    'ㇰ' => 'ク',
                    'ㇱ' => 'シ',
                    'ㇲ' => 'ス',
                    'ㇳ' => 'ト',
                    'ㇴ' => 'ヌ',
                    'ㇵ' => 'ハ',
                    'ㇶ' => 'ヒ',
                    'ㇷ' => 'フ',
                    'ㇸ' => 'ヘ',
                    'ㇹ' => 'ホ',
                    'ㇺ' => 'ム',
                    'ㇻ' => 'ラ',
                    'ㇼ' => 'リ',
                    'ㇽ' => 'ル',
                    'ㇾ' => 'レ',
                    'ㇿ' => 'ロ',
                    _ => c,
                })
                .collect();
        }
        for (i, _) in mapped.char_indices() {
            result
                .mapping
                .push((result.text.len() + i, offset, offset + c.len_utf8()));
        }
        result.text.push_str(&mapped);
    }
    result
}
