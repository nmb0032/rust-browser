use super::tests::{append, matches};
use super::*;

#[test]
fn forgiving_is_where_recover_balanced_branches_without_changing_strict_lists() {
    let mut document = Document::new();
    let root = document.root_id();
    let p = append(&mut document, root, "p", &[("class", "ok")]);
    for source in [
        ":is(:unknown, .ok)",
        ":is(, .ok,,)",
        ":is(:unknown(a, [x='a,b']), .ok)",
        ":where(> bad, .ok)",
        ":is(::before, .ok)",
        ":is(:not(:unknown), .ok)",
        ":is(:has(:has(p)), .ok)",
        ":not(:is(:unknown))",
        ":is(:where(:unknown, .ok))",
    ] {
        assert!(matches(&document, p, source), "{source}");
    }
    for source in [":is()", ":where(,,)", ":is(:unknown)", ":is(> p)"] {
        assert!(!matches(&document, p, source), "{source}");
    }
    for source in [
        ":not()",
        ":not(, .ok)",
        ":not(.ok,)",
        ":not(.ok, :unknown)",
        ":has()",
        ":has(, p)",
        ":has(p,)",
        ":has(p, :unknown)",
        ":has(:has(p))",
        ":has(:not(:has(p)))",
        ":has(::before)",
        ":has(:is(::before))",
        ":not(::before)",
        ":nth-child(2 of ::before)",
        ":is(.ok",
        ":where([x)",
        ":has(> .ok))",
    ] {
        // An invalid branch in :is()/:where() is discarded, even in :has().
        if source == ":has(:is(::before))" {
            assert!(!matches(&document, p, source));
        } else {
            assert!(parse(source).is_err(), "{source}");
        }
    }
}

#[test]
fn specificity_is_maximum_not_matched_branch_and_where_is_zero() {
    for (source, expected) in [
        (":where(#x, .a.b, main)", Specificity::default()),
        (
            ":is(.a, #unmatched)",
            Specificity {
                ids: 1,
                classes: 0,
                types: 0,
            },
        ),
        (
            ":is(:unsupported, .a)",
            Specificity {
                ids: 0,
                classes: 1,
                types: 0,
            },
        ),
        (
            "section:has(> #x, + .a.b)",
            Specificity {
                ids: 1,
                classes: 0,
                types: 1,
            },
        ),
        (
            ":not(:is(#x, .a)):where(#y)",
            Specificity {
                ids: 1,
                classes: 0,
                types: 0,
            },
        ),
        (
            "li:nth-child(2n of .a, main > #unmatched)",
            Specificity {
                ids: 1,
                classes: 1,
                types: 2,
            },
        ),
        (
            ":nth-child(2 of :where(#x))",
            Specificity {
                ids: 0,
                classes: 1,
                types: 0,
            },
        ),
        (
            "p:nth-last-of-type(-n+3)",
            Specificity {
                ids: 0,
                classes: 1,
                types: 1,
            },
        ),
        (
            "p:is(.a, #x)::before",
            Specificity {
                ids: 1,
                classes: 0,
                types: 2,
            },
        ),
    ] {
        assert_eq!(
            parse(source).unwrap()[0].specificity(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn has_anchors_all_relative_axes_and_non_elements_do_not_break_siblings() {
    let mut document = Document::new();
    let root = document.root_id();
    let section = append(&mut document, root, "section", &[("class", "ancestor")]);
    let a = append(&mut document, section, "article", &[("id", "a")]);
    let nested = append(&mut document, a, "div", &[]);
    let p = append(&mut document, nested, "p", &[("class", "leaf")]);
    document.append_text(section, "text".into()).unwrap();
    document.append_comment(section, "comment".into()).unwrap();
    let b = append(&mut document, section, "aside", &[("class", "next")]);
    let inside = append(&mut document, b, "span", &[("class", "inside")]);
    let c = append(&mut document, section, "footer", &[("class", "last")]);
    for source in [
        ":has(p)",
        ":has(> div)",
        ":has(> div > p.leaf)",
        ":has(+ aside)",
        ":has(+ aside > span.inside)",
        ":has(~ footer)",
        ":has(+ aside + footer)",
        ":has(> div .leaf, + .missing)",
        ":has(:is(.missing, .leaf))",
        ":not(:has(> p))",
        ":is(:not(:has(> .bad)), :has(.leaf))",
    ] {
        assert!(matches(&document, a, source), "{source}");
    }
    for source in [
        ":has(> p)",
        ":has(+ footer)",
        ":has(+ .inside)",
        ":has(~ p)",
        ":has(article)",
        ":has(.ancestor)",
        ":has(> div + aside)",
        ":has(+ aside > .missing)",
        ":has(:scope)",
    ] {
        assert!(!matches(&document, a, source), "{source}");
    }
    assert!(matches(
        &document,
        section,
        "section:has(> article .leaf):has(> aside + footer)"
    ));
    assert!(matches(&document, p, "article:has(+ aside) .leaf"));
    assert!(!matches(&document, inside, "article:has(.inside) span"));
    assert!(!matches(&document, c, ":has(+ *)"));
}

#[test]
fn nested_functional_chains_preserve_complex_selector_backtracking() {
    let document = crate::html::parser::parse(
        "<main id='outer'><section class='a'><section class='a'><p class='target'>one</p></section><aside class='tail'>two</aside></section></main>",
    ).unwrap();
    let p = document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.tag_name() == "p")
        })
        .unwrap();
    for source in [
        ":is(main > .a .target, .missing)",
        ":not(:not(:is(main > .a .target)))",
        ".a:has(> aside) :is(.target:not(:has(> .bad)))",
        "main:has(> .a > .a > .target):not(:has(> .target)) .target",
        ":where(.a:has(> .tail)) :nth-child(1 of :is(main p, #missing))",
    ] {
        assert!(matches(&document, p, source), "{source}");
    }
}

#[test]
fn nth_token_grammar_accepts_escapes_signs_and_rejects_token_splicing() {
    for source in [
        "odd",
        "EVEN",
        "2",
        "+2",
        "-2",
        "0",
        "n",
        "+n",
        "-n",
        "2n",
        "-2n",
        "2N-1",
        "-n+3",
        "n + 3",
        "n - 3",
        "n- 3",
        "-n- 3",
        "2n- 3",
        "n/**/+/**/3",
        "n/**/-/**/3",
        "\\6e + 1",
        "2\\6e +1",
        "\\6f dd",
        "2n of .a, main > p",
        "-n+3 of :is(.a, #x)",
        "n of :not(main > .a)",
    ] {
        assert!(parse(&format!(":nth-child({source})")).is_ok(), "{source}");
    }
    for source in [
        "",
        "2.0",
        "1e0",
        "2 n",
        "n 2",
        "+ n",
        "n + -2",
        "n - +2",
        "n - -2",
        "n--2",
        "2n1",
        "odd + 1",
        "2n + 1.0",
        "n- +2",
        "n- -2",
        "2n +",
        "of .a",
        "2 of",
        "2 of ,p",
        "2 of p,",
        "2 of .a,:unknown",
        "2 of > .a",
        "n of :has(:has(p))",
        "9223372036854775808",
        "9223372036854775808n",
    ] {
        assert!(parse(&format!(":nth-child({source})")).is_err(), "{source}");
    }
    assert!(parse(":nth-of-type(2 of p)").is_err());
    assert!(parse(":nth-last-of-type(2 of p)").is_err());
}

#[test]
fn nth_positive_negative_zero_last_type_and_complex_of_lists() {
    let mut document = Document::new();
    let root = document.root_id();
    let parent = append(&mut document, root, "main", &[]);
    let mut nodes = Vec::new();
    for index in 1..=7 {
        document.append_text(parent, "\n".into()).unwrap();
        nodes.push(append(
            &mut document,
            parent,
            if index % 2 == 1 { "p" } else { "span" },
            &[("class", if index != 3 { "eligible" } else { "excluded" })],
        ));
    }
    for (source, indices) in [
        (":nth-child(odd)", vec![1, 3, 5, 7]),
        (":nth-child(2n)", vec![2, 4, 6]),
        (":nth-child(-n+3)", vec![1, 2, 3]),
        (":nth-child(-2n+6)", vec![2, 4, 6]),
        (":nth-child(3n-2)", vec![1, 4, 7]),
        (":nth-child(0n+3)", vec![3]),
        (":nth-child(-3)", vec![]),
        (":nth-last-child(2n)", vec![2, 4, 6]),
        (":nth-last-child(-n+2)", vec![6, 7]),
        ("p:nth-of-type(2n)", vec![3, 7]),
        ("span:nth-last-of-type(2)", vec![4]),
        (":nth-child(2 of .eligible)", vec![2]),
        (":nth-child(3 of .eligible)", vec![4]),
        (":nth-last-child(2 of .eligible)", vec![6]),
        (
            ":nth-child(2n of main > :is(p.eligible, span.eligible))",
            vec![2, 5, 7],
        ),
        (":nth-child(1 of .missing)", vec![]),
        (":nth-child(2 of main p)", vec![3]),
        (":nth-child(1 of :is(.eligible,:unknown))", vec![1]),
    ] {
        let actual: Vec<_> = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, &id)| matches(&document, id, source).then_some(index + 1))
            .collect();
        assert_eq!(actual, indices, "{source}");
    }
    let detached = append(&mut document, root, "p", &[]);
    document.detach(detached).unwrap();
    assert!(matches(
        &document,
        detached,
        ":nth-child(1):nth-last-of-type(1)"
    ));
}

#[test]
fn matching_observes_dom_order_after_moves_detach_and_attribute_mutation() {
    let mut document = Document::new();
    let root = document.root_id();
    let parent = append(&mut document, root, "main", &[]);
    let a = append(&mut document, parent, "p", &[("class", "eligible")]);
    let b = append(&mut document, parent, "p", &[("class", "eligible")]);
    let c = append(&mut document, parent, "aside", &[]);
    assert!(matches(&document, a, ":has(+ p + aside)"));
    assert!(matches(&document, b, ":nth-child(2 of .eligible)"));
    document.insert_before(parent, b, Some(a)).unwrap();
    assert!(!matches(&document, a, ":has(+ p + aside)"));
    assert!(matches(&document, a, ":nth-child(2 of .eligible)"));
    document.detach(c).unwrap();
    assert!(!matches(&document, a, ":has(+ aside)"));
    document.insert_before(parent, c, Some(a)).unwrap();
    assert!(matches(&document, a, ":nth-child(2 of .eligible)"));
    document
        .merge_attributes(
            c,
            vec![crate::dom::Attribute::new(
                "class".into(),
                "eligible".into(),
            )],
        )
        .unwrap();
    assert!(matches(&document, a, ":nth-child(3 of .eligible)"));
}

#[test]
fn pseudo_elements_target_only_separate_subjects_and_have_terminal_grammar() {
    let mut document = Document::new();
    let root = document.root_id();
    let p = append(&mut document, root, "p", &[]);
    let state = InteractionState::default();
    for pseudo in PseudoElement::ALL {
        let selector = parse(&format!("p::{}", pseudo.name()))
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(selector.pseudo_element(), Some(pseudo));
        assert!(!selector.matches(&document, p));
        assert!(selector.matches_target(&document, p, None, &state, Some(pseudo)));
        assert!(!selector.matches_target(
            &document,
            p,
            None,
            &state,
            Some(PseudoElement::Before).filter(|p| *p != pseudo)
        ));
    }
    assert!(parse("::before").unwrap()[0].matches_target(
        &document,
        p,
        None,
        &state,
        Some(PseudoElement::Before)
    ));
    for source in ["p:before", "p:after", ":first-line", ":first-letter"] {
        assert!(parse(source).is_ok(), "{source}");
    }
    for source in [
        ":marker",
        "::unknown",
        "p::before.x",
        "p::before > span",
        "p::before::after",
        ":not(p::before)",
        ":has(p::marker)",
        "p::placeholder:hover",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn static_form_link_states_and_disabled_fieldset_legend_exception() {
    let document = crate::html::parser::parse(
        "<a id='link' href='' disabled>link</a><a id='anchor'>anchor</a><form id='f'>
        <fieldset disabled><legend><input id='legend'></legend><legend><input id='second'></legend>
        <div><input id='disabled'></div></fieldset>
        <input id='required' required><input id='optional' type='hidden' required>
        <input id='checked' type='checkbox' checked><input id='plain'>
        <input id='readonly' readonly><textarea id='placeholder' placeholder='hint'></textarea>
        <textarea id='filled' placeholder='hint'>value</textarea>
        <input id='badtype' type='unknown' required>
        <select><optgroup disabled><option id='option' selected>x</option></optgroup></select>
        <button id='default'>Submit</button><input id='submit' type='submit'>
        <input id='radio' type='radio' name='r'><input id='radio2' type='radio' name='r'>
        </form><input id='external' type='submit' form='f'><progress id='progress'></progress>
        <div contenteditable><span id='edit'>editable</span><span id='noedit' contenteditable='false'>no</span></div>",
    ).unwrap();
    let find = |name: &str| {
        document
            .preorder()
            .find(|&id| {
                document
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|e| e.attribute("id") == Some(name))
            })
            .unwrap()
    };
    for (name, selector) in [
        ("link", ":link:any-link:not(:visited)"),
        ("anchor", ":not(:any-link)"),
        ("legend", ":enabled"),
        ("second", ":disabled"),
        ("disabled", ":disabled:read-only"),
        ("required", ":required:read-write"),
        ("optional", ":optional:read-only"),
        ("checked", ":checked:default"),
        ("plain", ":optional:read-write:not(:checked)"),
        ("readonly", ":read-only"),
        ("placeholder", ":placeholder-shown"),
        ("filled", ":not(:placeholder-shown)"),
        ("option", ":disabled:checked:default"),
        ("default", ":default"),
        ("submit", ":not(:default)"),
        ("external", ":not(:default)"),
        ("radio", ":indeterminate"),
        ("radio2", ":indeterminate"),
        ("progress", ":indeterminate"),
        ("edit", ":read-write"),
        ("noedit", ":read-only"),
        ("badtype", ":required:read-write"),
    ] {
        assert!(
            matches(&document, find(name), selector),
            "{name}: {selector}"
        );
    }
    assert!(!matches(&document, find("link"), ":has(:visited)"));
    assert!(focusable(&document, find("link")));
    assert!(!matches(&document, find("plain"), ":indeterminate"));
}

#[test]
fn explicit_interaction_context_propagates_ancestors_and_keeps_focus_exact() {
    let document =
        crate::html::parser::parse("<main><a href='/'><span>text</span></a></main>").unwrap();
    let find = |tag: &str| {
        document
            .preorder()
            .find(|&id| {
                document
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|e| e.tag_name() == tag)
            })
            .unwrap()
    };
    let main = find("main");
    let link = find("a");
    let span = find("span");
    let state = InteractionState {
        hovered: Some(span),
        active: Some(span),
        focused: Some(link),
        focus_visible: true,
        target: Some(main),
    };
    for (id, source) in [
        (main, ":hover:active:focus-within:target:not(:focus)"),
        (link, ":focus:focus-visible:hover:active:focus-within"),
        (main, ":has(> a:focus > span:hover)"),
        (span, ":hover:not(:focus):not(:focus-within)"),
        (link, ":nth-child(1 of :focus)"),
    ] {
        let selector = parse(source).unwrap().pop().unwrap();
        assert!(
            selector.matches_with_state(&document, id, &state),
            "{source}"
        );
        assert!(!selector.matches(&document, id), "{source}");
    }
}

#[test]
fn scopes_and_namespaces_remain_distinct_from_relative_has_anchor() {
    let mut document = Document::new();
    let root = document.root_id();
    let outer = append(&mut document, root, "main", &[]);
    let subject = append(&mut document, outer, "div", &[]);
    let child = append(&mut document, subject, "span", &[]);
    let selector = parse(":scope > div:has(> span):not(:has(> :scope))")
        .unwrap()
        .pop()
        .unwrap();
    assert!(selector.matches_with_scope(&document, subject, Some(outer)));
    assert!(!selector.matches_with_scope(&document, subject, Some(subject)));
    assert!(parse("div:has(> :scope)").unwrap()[0].matches_with_scope(
        &document,
        subject,
        Some(child)
    ));
    let mut namespaces = Namespaces::default();
    namespaces
        .prefixes
        .insert("h".into(), crate::dom::Namespace::Html.uri().into());
    assert!(
        parse_context(
            "h|div:has(> h|span):nth-child(1 of h|div)",
            &namespaces,
            None
        )
        .unwrap()[0]
            .matches(&document, subject)
    );
    assert!(parse_context("h|div:has(> unknown|span)", &namespaces, None).is_err());
    assert!(
        parse_context("h|div:is(unknown|span, h|div)", &namespaces, None).unwrap()[0]
            .matches(&document, subject)
    );
}

#[test]
fn nested_matching_budget_fails_closed_even_under_negation() {
    let mut document = Document::new();
    let root = document.root_id();
    let subject = append(&mut document, root, "main", &[]);
    for _ in 0..100 {
        let branch = append(&mut document, subject, "div", &[]);
        for _ in 0..100 {
            append(&mut document, branch, "p", &[]);
        }
    }
    let remaining = Cell::new(10);
    let state = InteractionState::default();
    let selector = parse(":not(:has(.missing))").unwrap().pop().unwrap();
    let _ = selector.matches_context(
        &document,
        subject,
        MatchContext {
            scope: None,
            state: &state,
            remaining: &remaining,
        },
    );
    assert_eq!(remaining.get(), 0);
    assert!(!matches(
        &document,
        subject,
        ":not(:has(:nth-child(999999 of p)))"
    ));
    let deep = format!(
        "{}:has(.missing){}",
        ":not(".repeat(MAX_SELECTOR_NESTING - 1),
        ")".repeat(MAX_SELECTOR_NESTING - 1)
    );
    assert!(parse(&deep).is_ok());
}

#[test]
fn full_matching_budget_reports_exhaustion_even_if_a_function_negates_or_masks_failure() {
    let mut document = Document::new();
    let root = document.root_id();
    let subject = append(&mut document, root, "main", &[]);
    for _ in 0..400 {
        append(&mut document, subject, "p", &[]);
    }
    let state = InteractionState::default();
    for source in [
        ":not(:has(> :nth-child(999999 of p)))",
        ":not(:not(:has(> :nth-child(999999 of p))))",
        ":is(:has(> :nth-child(999999 of p)), main)",
    ] {
        let selector = parse(source).unwrap().pop().unwrap();
        assert_eq!(
            selector.try_matches_target(&document, subject, None, &state, None),
            Err(MatchLimitExceeded),
            "{source}"
        );
        assert!(!selector.matches(&document, subject), "{source}");
    }
    for (source, expected) in [
        ("main:not(:has(> .missing))", true),
        ("main:has(> p)", true),
        ("main:has(> .missing)", false),
    ] {
        let selector = parse(source).unwrap().pop().unwrap();
        assert_eq!(
            selector.try_matches_target(&document, subject, None, &state, None),
            Ok(expected)
        );
        assert_eq!(selector.matches(&document, subject), expected);
    }
}
