//! Audited diagnostic updates to the pinned, historical fixtures.
//! Keep their additive raw counts; these changes apply only to the current-spec gate.

pub(super) fn diagnostic_count(legacy: &[String], new: &[String]) -> usize {
    let mut remaining = legacy.to_vec();
    for error in new {
        let code = diagnostic_name(error);
        if let Some(index) = remaining
            .iter()
            .position(|old| renamed(diagnostic_name(old), code))
        {
            remaining.remove(index);
        }
    }
    remaining.len() + new.len()
}

fn diagnostic_name(line: &str) -> &str {
    if line.starts_with('(') {
        line.split_once("): ")
            .or_else(|| line.split_once(") "))
            .map_or(line, |(_, name)| name)
    } else {
        line
    }
}

fn renamed(old: &str, new: &str) -> bool {
    if old == new {
        return true;
    }
    match old {
        "incorrect-comment" => new == "abrupt-closing-of-empty-comment",
        "expected-numeric-entity" => matches!(
            new,
            "absence-of-digits-in-numeric-character-reference"
                | "missing-semicolon-after-character-reference"
        ),
        "illegal-codepoint-for-numeric-entity" => matches!(
            new,
            "character-reference-outside-unicode-range"
                | "control-character-reference"
                | "noncharacter-character-reference"
                | "null-character-reference"
                | "surrogate-character-reference"
        ),
        "attributes-in-end-tag" => new == "end-tag-with-attributes",
        "self-closing-flag-on-end-tag" => new == "end-tag-with-trailing-solidus",
        "expected-tag-name" | "expected-closing-tag-but-got-char" => {
            matches!(
                new,
                "eof-before-tag-name" | "invalid-first-character-of-tag-name"
            )
        }
        "expected-closing-tag-but-got-eof" => new == "eof-before-tag-name",
        "eof-in-comment-double-dash" => new == "eof-in-comment",
        "expected-script-data-but-got-eof"
        | "eof-in-script-in-script"
        | "unexpected-eof-in-text-mode"
        | "unexpected-EOF-in-text-mode" => new == "eof-in-script-html-comment-like-text",
        "eof-in-attribute-value-double-quote"
        | "eof-in-tag-name"
        | "eof-in-attribute-name"
        | "expected-attribute-name-but-got-eof"
        | "unexpected-EOF-after-solidus-in-tag" => new == "eof-in-tag",
        "unexpected-bang-after-double-dash-in-comment" => new == "incorrectly-closed-comment",
        "expected-dashes-or-doctype" => {
            matches!(new, "incorrectly-opened-comment" | "cdata-in-html-content")
        }
        "expected-space-or-right-bracket-in-doctype" => {
            new == "invalid-character-sequence-after-doctype-name"
        }
        "expected-doctype-name-but-got-right-bracket" => new == "missing-doctype-name",
        "unexpected-end-of-doctype" => new == "missing-doctype-public-identifier",
        "unexpected-char-in-doctype" => matches!(
            new,
            "missing-doctype-system-identifier"
                | "missing-quote-before-doctype-public-identifier"
                | "missing-quote-before-doctype-system-identifier"
                | "missing-whitespace-after-doctype-public-keyword"
                | "missing-whitespace-between-doctype-public-and-system-identifiers"
        ),
        "named-entity-without-semicolon"
        | "numeric-entity-without-semicolon"
        | "eof-in-numeric-entity" => new == "missing-semicolon-after-character-reference",
        "need-space-after-doctype" => new == "missing-whitespace-before-doctype-name",
        "unexpected-char-in-comment" => new == "nested-comment",
        "non-void-element-with-trailing-solidus" => {
            new == "non-void-html-element-start-tag-with-trailing-solidus"
        }
        "expected-named-entity" => new == "unknown-named-character-reference",
        "invalid-character-in-attribute-name" => new == "unexpected-character-in-attribute-name",
        "equals-in-unquoted-attribute-value" => {
            new == "unexpected-character-in-unquoted-attribute-value"
        }
        "invalid-codepoint" => new == "unexpected-null-character",
        "expected-tag-name-but-got-question-mark" => {
            new == "unexpected-question-mark-instead-of-tag-name"
        }
        "unexpected-character-after-solidus-in-tag" => new == "unexpected-solidus-in-tag",
        _ => {
            old.contains("Self-closing syntax")
                && new == "non-void-html-element-start-tag-with-trailing-solidus"
        }
    }
}

pub(super) fn tree_delta(file: &str, case: usize) -> isize {
    // HTML 13.2.6.4.10: these runs start with a table/tbody/thead/tfoot/tr
    // current node, unlike unbuffered text under foster-parented elements.
    // Each buffered non-whitespace run adds one error before per-character
    // fallback. The pinned expectations include only the latter.
    let buffered_runs = match (file, case) {
        ("adoption01.dat", 12) | ("tests19.dat", 90 | 94) | ("tests7.dat", 32 | 33) => 2,
        ("adoption01.dat", 11)
        | ("domjs-unsafe.dat", 37)
        | ("namespace-sensitivity.dat", 1)
        | ("pending-spec-changes-plain-text-unsafe.dat", 1)
        | ("tables01.dat", 4 | 19)
        | ("template.dat", 92)
        | ("tests1.dat", 33 | 78..=80)
        | ("tests15.dat", 7..=11)
        | ("tests18.dat", 24 | 25)
        | ("tests19.dat", 25 | 27)
        | ("tests2.dat", 15)
        | ("tests26.dat", 3)
        | ("tests6.dat", 26)
        | ("tests7.dat", 9 | 20 | 31)
        | ("tests8.dat", 7 | 8)
        | ("webkit02.dat", 6) => 1,
        _ => 0,
    };
    let other = match (file, case) {
        // HTML 13.2.6.4.9: closing a table pops intervening elements without
        // the obsolete end-tag-too-early-named diagnostic.
        ("adoption02.dat", 3) => -1,
        // HTML 13.2.6.5: after a foreign mismatch, reaching the HTML root
        // reprocesses the end tag using HTML rules, producing a second error.
        ("foreign-fragment.dat", 4) | ("math.dat" | "svg.dat", 6..=8) => 1,
        // HTML 13.2.6.4.9: whitespace under the template node (after the row
        // closes) goes through the table fallback, not pending table text.
        ("template.dat", 111) => 1,
        // These newer output fixtures omit diagnostics altogether. HTML
        // 13.2.6.4.1, 13.2.6.4.7, and the adoption agency algorithm require
        // missing-doctype, misnested button/formatting, and open-element EOF.
        ("webkit02.dat", 45 | 47..=49) => 3,
        ("webkit02.dat", 46) => 4,
        _ => 0,
    };
    buffered_runs + other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_do_not_remove_independent_tree_errors_or_unpaired_new_errors() {
        let legacy = [
            "(1,2): invalid-codepoint",
            "(1,2): invalid-codepoint-in-body",
            "(1,3): expected-named-closing-tag-but-got-eof",
        ]
        .map(str::to_owned);
        let new = ["(1:2) unexpected-null-character", "(1:4) eof-in-tag"].map(str::to_owned);
        assert_eq!(diagnostic_count(&legacy, &new), 4);
        assert_eq!(diagnostic_count(&[], &new), 2);
    }

    #[test]
    fn only_audited_buffered_table_runs_receive_the_extra_run_error() {
        assert_eq!(tree_delta("tests7.dat", 32), 2);
        assert_eq!(tree_delta("tests19.dat", 91), 0);
        assert_eq!(tree_delta("template.dat", 112), 0);
        assert_eq!(tree_delta("adoption02.dat", 3), -1);
    }
}
