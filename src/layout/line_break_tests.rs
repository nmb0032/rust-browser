use super::*;
use crate::{
    dom::{Document, NodeId},
    html::parser,
    paint::{self, load_bundled_font},
    style,
};
use unicode_linebreak::{
    BreakOpportunity::{Allowed, Mandatory},
    linebreaks,
};

fn id(document: &Document, name: &str) -> NodeId {
    document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

fn find(root: &LayoutBox, node: NodeId) -> &LayoutBox {
    fn search(root: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
        if root.node_id == Some(node) && root.pseudo.is_none() {
            return Some(root);
        }
        root.children.iter().find_map(|child| search(child, node))
    }
    search(root, node).unwrap()
}

fn layout(
    width: f32,
    mode: &str,
    content: &str,
    extra: &str,
) -> (Document, LayoutTree, crate::text::FontContext) {
    let document = parser::parse(&format!(
        "<!doctype html><style>body{{margin:0}}#host{{width:{width}px;font:16px/20px serif;\
         white-space:{mode}}}{extra}</style><div id='host'>{content}</div>"
    ))
    .unwrap();
    let styles = style::resolve(&document);
    assert!(
        styles.stylesheets.diagnostics.is_empty(),
        "{:?}",
        styles.stylesheets.diagnostics
    );
    let font = load_bundled_font().unwrap();
    let mut tree = build_layout_tree_with_styles(&document, &styles.styles);
    tree.layout(448.0, &font);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    (document, tree, font)
}

fn fragments(root: &LayoutBox) -> Vec<&TextFragment> {
    let mut result: Vec<_> = root.text_fragments.iter().collect();
    for child in &root.children {
        result.extend(fragments(child));
    }
    result
}

fn lines(root: &LayoutBox) -> Vec<String> {
    root.line_boxes()
        .iter()
        .map(|line| {
            let mut text: Vec<_> = fragments(root)
                .into_iter()
                .filter(|f| f.baseline == line.baseline)
                .collect();
            text.sort_by(|a, b| a.x.total_cmp(&b.x));
            text.iter().map(|f| f.text.as_str()).collect()
        })
        .collect()
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

fn advance(font: &crate::text::FontContext, text: &str) -> f32 {
    super::inline::measure_text(font, text, 16.0)
}

#[test]
fn public_uax_examples_and_utf8_punctuation_indices_are_the_breaking_contract() {
    // Crate's public example, plus UAX #14 LB13/LB14 punctuation and LB21 slash/hyphen.
    assert_eq!(
        linebreaks("a b \nc").collect::<Vec<_>>(),
        [(2, Allowed), (5, Mandatory), (6, Mandatory)]
    );
    assert_eq!(
        linebreaks("你好，世界").collect::<Vec<_>>(),
        [(3, Allowed), (9, Allowed), (12, Allowed), (15, Mandatory)]
    );
    assert_eq!(
        linebreaks("https://example.com/a-b/c").collect::<Vec<_>>(),
        [
            (8, Allowed),
            (20, Allowed),
            (22, Allowed),
            (24, Allowed),
            (25, Mandatory)
        ]
    );
    assert_eq!(linebreaks("a\u{a0}b").collect::<Vec<_>>(), [(4, Mandatory)]);
    assert_eq!(
        linebreaks("a\u{2060}b").collect::<Vec<_>>(),
        [(5, Mandatory)]
    );
    assert_eq!(
        linebreaks("a\u{200b}b").collect::<Vec<_>>(),
        [(4, Allowed), (5, Mandatory)]
    );
    assert_eq!(
        linebreaks("a\u{301}b").collect::<Vec<_>>(),
        [(4, Mandatory)]
    );
}

#[test]
fn urls_and_cjk_backtrack_to_last_legal_opportunity_not_every_style_chunk() {
    let font = load_bundled_font().unwrap();
    assert_eq!(font.lookup_glyph_index('你'), 0); // fallback is still milestone 8.6
    let text = "https://example.com/a-b/c";
    let width = advance(&font, "https://") + 0.01;
    let (doc, tree, _) = layout(width, "normal", text, "");
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(lines(host), ["https://", "example.com/", "a-b/c"]);
    for (line, text) in host.line_boxes().iter().zip(lines(host)) {
        close(line.content_rect.width, advance(&font, &text));
    }
    let width = advance(&font, "你好") + 0.01;
    let (doc, tree, _) = layout(width, "normal", "你好，世界", "");
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(lines(host), ["你", "好，", "世界"]);
    // Even when too narrow for a prohibited pair, it overflows intact.
    let (doc, tree, _) = layout(1.0, "normal", "（你好）", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["（你", "好）"]);
}

#[test]
fn logical_stream_crosses_styles_pseudos_combining_sequences_and_multibyte_scalars() {
    let font = load_bundled_font().unwrap();
    let (doc, tree, _) = layout(
        advance(&font, "ab-") + 0.01,
        "normal",
        "a<span>b-</span><em>cd</em>",
        "",
    );
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["ab-", "cd"]);
    let (doc, tree, _) = layout(1.0, "normal", "<b>Hel</b><i>lo</i>", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["Hello"]);
    let (doc, tree, _) = layout(1.0, "normal", "<span>你</span><b>好</b>", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["你", "好"]);
    let (doc, tree, _) = layout(
        1.0,
        "normal",
        "b",
        "#host::before{content:'a-'}#host::after{content:'-c'}",
    );
    assert_eq!(
        lines(find(tree.root(), id(&doc, "host"))),
        ["a-", "b-", "c"]
    );
    for content in [
        "é<span>té</span>",
        "a<span>&#x301;b</span>",
        "你<span>&#x301;</span>好",
        "🇯<span>🇵</span>🇫🇷",
        "👩<span>&#x200d;</span>👩",
    ] {
        let (doc, tree, _) = layout(1.0, "normal", content, "");
        let output = lines(find(tree.root(), id(&doc, "host")));
        let expected = if content.starts_with(['你', '🇯']) {
            2
        } else {
            1
        };
        assert_eq!(output.len(), expected, "{content}: {output:?}");
    }
}

#[test]
fn nbsp_unicode_spaces_joiners_zero_width_spaces_and_discretionary_hyphens() {
    let font = load_bundled_font().unwrap();
    for content in [
        "a&nbsp;b",
        "a&#x202f;b",
        "a<span>&#x2060;</span>b",
        "a&#xfeff;b",
    ] {
        let (doc, tree, _) = layout(1.0, "normal", content, "");
        let host = find(tree.root(), id(&doc, "host"));
        assert_eq!(host.line_boxes().len(), 1, "{content}");
        assert!(host.line_boxes()[0].content_rect.width > 1.0);
    }
    let (doc, tree, _) = layout(200.0, "normal", "a&nbsp;&nbsp;b&#x2003;&#x2003;c", "");
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(lines(host), ["a\u{a0}\u{a0}b\u{2003}\u{2003}c"]);
    close(
        host.line_boxes()[0].content_rect.width,
        advance(&font, "a\u{a0}\u{a0}b\u{2003}\u{2003}c"),
    );
    let (doc, tree, _) = layout(1.0, "normal", "a&#x200b;b", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a", "b"]);
    let (doc, tree, _) = layout(1.0, "normal", "a<wbr>b", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a", "b"]);
    let (doc, tree, _) = layout(1.0, "nowrap", "a<wbr>b", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["ab"]);
    let (doc, tree, _) = layout(200.0, "normal", "co&shy;operate", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["cooperate"]);
    let width = advance(&font, "co-") + 0.01;
    let (doc, tree, _) = layout(
        width,
        "normal",
        "<span id='s'>co&shy;</span>operate",
        "#s{border:1px solid;padding:0 1px}",
    );
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(lines(host), ["co-", "operate"]);
    let s = find(tree.root(), id(&doc, "s"));
    close(
        s.inline_fragments()[0].content_rect.width,
        advance(&font, "co-"),
    );
    assert!(s.inline_fragments()[0].last);
    let (doc, tree, _) = layout(1.0, "nowrap", "a\u{2028}b\u{85}c", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a", "b", "c"]);
}

#[test]
fn six_modes_have_distinct_collapse_preservation_and_wrapping_combinations() {
    let font = load_bundled_font().unwrap();
    let width = advance(&font, "a b") + 0.01;
    for (mode, expected) in [
        ("normal", vec!["ab", "c"]),
        ("nowrap", vec!["abc"]),
        ("pre", vec![" a  b", "c "]),
        ("pre-wrap", vec![" a  ", "b", "c "]),
        ("pre-line", vec!["ab", "c"]),
        ("break-spaces", vec![" a  ", "b", "c "]),
    ] {
        let (doc, tree, _) = layout(width, mode, " a  b\nc ", "");
        let host = find(tree.root(), id(&doc, "host"));
        assert_eq!(lines(host), expected, "{mode}");
        close(host.content_rect.height, expected.len() as f32 * 20.0);
    }
}

#[test]
fn trailing_consecutive_newlines_and_br_do_not_add_a_terminal_strut() {
    for mode in ["pre", "pre-wrap", "pre-line", "break-spaces"] {
        for (content, count) in [
            ("", 0),
            ("\n", 1),
            ("\n\n", 2),
            ("a\n", 1),
            ("a\n\n", 2),
            ("a\n\nb", 3),
        ] {
            let (doc, tree, _) = layout(200.0, mode, content, "");
            assert_eq!(
                find(tree.root(), id(&doc, "host")).line_boxes().len(),
                count,
                "{mode}: {content:?}"
            );
        }
    }
    for mode in ["normal", "nowrap"] {
        let (doc, tree, _) = layout(1.0, mode, "a<br><br>b<br>", "");
        assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a", "", "b"]);
        let (doc, tree, _) = layout(1.0, mode, " \n \t ", "");
        assert!(find(tree.root(), id(&doc, "host")).line_boxes().is_empty());
    }
    let (doc, tree, _) = layout(200.0, "normal", "a\n\n b", "");
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(lines(host), ["ab"]);
    close(
        fragments(host)[1].x - fragments(host)[0].x,
        advance(&load_bundled_font().unwrap(), "a "),
    );
}

#[test]
fn ascii_segment_break_collapse_has_explicit_han_kana_and_cr_rules() {
    for (source, expected) in [
        ("你 \n <span> 好</span>", "你好"),
        ("你\n\n好", "你好"),
        ("a\nb", "ab"),
        ("한\n글", "한글"),
        ("你\nb", "你b"),
        ("a&#x200b;\nb", "ab"),
    ] {
        let (doc, tree, font) = layout(200.0, "normal", source, "");
        let host = find(tree.root(), id(&doc, "host"));
        assert_eq!(lines(host), [expected]);
        let logical = match source {
            "a\nb" => "a b",
            "한\n글" => "한 글",
            "你\nb" => "你 b",
            _ => expected,
        };
        close(
            host.line_boxes()[0].content_rect.width,
            advance(&font, logical),
        );
    }
    // HTML input normalization happens before CSS; a character-reference CR
    // survives into the DOM and, per CSS Text, is treated as a space, not LF.
    let (doc, tree, _) = layout(200.0, "pre", "a\r\nb\rc&#13;d", "");
    assert_eq!(
        lines(find(tree.root(), id(&doc, "host"))),
        ["a", "b", "c d"]
    );
    let (doc, tree, _) = layout(200.0, "pre-line", "a \t\n \tb", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a", "b"]);
    let (doc, tree, font) = layout(200.0, "normal", "a&#x2003; b", "");
    close(
        find(tree.root(), id(&doc, "host")).line_boxes()[0]
            .content_rect
            .width,
        advance(&font, "a\u{2003} b"),
    );
}

#[test]
fn fragment_source_crlf_normalizes_but_programmatic_dom_cr_is_a_css_space() {
    let doc = parser::parse_fragment(
        "<div id='host' style='white-space:pre;font:16px/20px serif'>a\r\nb\rc</div>",
        parser::FragmentContext::html("div"),
    )
    .unwrap();
    let font = load_bundled_font().unwrap();
    let styles = style::resolve(&doc);
    let mut tree = build_layout_tree_with_styles(&doc, &styles.styles);
    tree.layout(448.0, &font);
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a", "b", "c"]);
    let mut doc =
        parser::parse("<div id='host' style='white-space:pre;font:16px/20px serif'></div>")
            .unwrap();
    let node = id(&doc, "host");
    doc.append_text(node, "a\r".into()).unwrap();
    doc.append_text(node, "\nb".into()).unwrap();
    let styles = style::resolve(&doc);
    let mut tree = build_layout_tree_with_styles(&doc, &styles.styles);
    tree.layout(448.0, &font);
    assert_eq!(lines(find(tree.root(), node)), ["a ", "b"]);
    close(
        find(tree.root(), node).line_boxes()[0].content_rect.width,
        advance(&font, "a "),
    );
    assert!(tree.diagnostics().is_empty());
}

#[test]
fn preserved_tabs_use_block_font_eight_space_stops_and_restart_at_each_line_origin() {
    let font = load_bundled_font().unwrap();
    let tab = advance(&font, " ") * 8.0;
    let (doc, tree, _) = layout(
        200.0,
        "pre",
        "a\tb\tc\n\t<span id='big'>d</span>",
        "#host{padding-left:17px}#big{font-size:32px}",
    );
    let host = find(tree.root(), id(&doc, "host"));
    let text = fragments(host);
    close(
        text.iter().find(|f| f.text == "b").unwrap().x - host.content_rect.x,
        tab,
    );
    close(
        text.iter().find(|f| f.text == "c").unwrap().x - host.content_rect.x,
        tab * 2.0,
    );
    close(
        text.iter().find(|f| f.text == "d").unwrap().x - host.content_rect.x,
        tab,
    );
    assert!(
        text.iter()
            .filter(|f| f.text == "\t")
            .all(|f| f.ink_rect.width == 0.0 && f.ink_rect.height == 0.0)
    );
    let (doc, tree, _) = layout(tab + advance(&font, "b") + 0.01, "pre-wrap", "a\tb\tc", "");
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(lines(host), ["a\tb\t", "c"]);
    close(
        fragments(host).iter().find(|f| f.text == "c").unwrap().x,
        host.content_rect.x,
    );
    let (doc, tree, _) = layout(tab + 0.01, "break-spaces", "a\tb\tc", "");
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(lines(host), ["a\t", "b\t", "c"]);
    for fragment in fragments(host).iter().filter(|f| f.text == "\t") {
        close(fragment.x + fragment.width - host.content_rect.x, tab);
    }
    // A stop less than .5ch away is skipped, as CSS Text requires.
    let (doc, tree, _) = layout(
        200.0,
        "pre",
        "<span id='lead'>a</span>\tb",
        &format!(
            "#lead{{padding-right:{}px}}",
            tab - advance(&font, "a") - 1.0
        ),
    );
    let host = find(tree.root(), id(&doc, "host"));
    close(
        fragments(host).iter().find(|f| f.text == "b").unwrap().x - host.content_rect.x,
        tab * 2.0,
    );
}

#[test]
fn hanging_trailing_spaces_do_not_size_align_or_create_extra_lines_but_keep_hit_surfaces() {
    let font = load_bundled_font().unwrap();
    let word = advance(&font, "a");
    let space = advance(&font, " ");
    for (mode, content, width) in [
        ("normal", " a ", word),
        ("nowrap", " a ", word),
        ("pre-line", " a ", word),
        ("pre", " a ", word + 2.0 * space),
        ("pre-wrap", "a ", word + space),
        ("break-spaces", "a ", word + space),
    ] {
        let (doc, tree, _) = layout(100.0, mode, content, "#host{text-align:right}");
        let host = find(tree.root(), id(&doc, "host"));
        close(host.line_boxes()[0].content_rect.width, width);
        close(
            host.line_boxes()[0].content_rect.x,
            host.content_rect.x + 100.0 - width,
        );
    }
    let (doc, tree, _) = layout(word + 0.01, "pre-wrap", "a   ", "#host{text-align:right}");
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(host.line_boxes().len(), 1);
    close(host.line_boxes()[0].content_rect.width, word);
    let whitespace = fragments(host).into_iter().find(|f| f.text == " ").unwrap();
    assert_eq!(
        tree.hit_test(whitespace.x + 1.0, whitespace.y + 1.0),
        host.children[0].node_id
    );
    let (doc, tree, _) = layout(word + space + 0.01, "break-spaces", "a   ", "");
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a ", "  "]);
    // pre-wrap hangs unconditionally at a soft break, conditionally at LF/EOF.
    let (doc, tree, _) = layout(
        advance(&font, "a b") + 0.01,
        "pre-wrap",
        "a b c",
        "#host{text-align:center}",
    );
    let host = find(tree.root(), id(&doc, "host"));
    close(
        host.line_boxes()[0].content_rect.width,
        advance(&font, "a b"),
    );
    assert_eq!(lines(host), ["a b ", "c"]);
    let (doc, tree, _) = layout(100.0, "pre-wrap", "a \n", "");
    close(
        find(tree.root(), id(&doc, "host")).line_boxes()[0]
            .content_rect
            .width,
        word + space,
    );
}

#[test]
fn per_character_wrap_scope_and_surviving_space_metrics_ignore_intervening_box_boundaries() {
    let (doc, tree, font) = layout(
        1.0,
        "normal",
        "<span id='s'>a b-c</span>d",
        "#s{white-space:nowrap}",
    );
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(lines(host), ["ab-cd"]);
    close(
        host.line_boxes()[0].content_rect.width,
        advance(&font, "a b-cd"),
    );
    let (doc, tree, _) = layout(
        1.0,
        "nowrap",
        "<span id='s'>a b</span>c",
        "#s{white-space:normal}",
    );
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a", "bc"]);
    // Boundary between separate normal spans uses the containing nowrap block.
    let (doc, tree, _) = layout(
        1.0,
        "nowrap",
        "<span>你</span><span>好</span>",
        "#host span{white-space:normal}",
    );
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["你好"]);
    let (doc, tree, _) = layout(
        1.0,
        "nowrap",
        "<span>a </span>b",
        "#host span{white-space:pre-wrap}",
    );
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a b"]);
    let (doc, tree, font) = layout(
        200.0,
        "normal",
        "<span id='big'>a </span><i> </i>b",
        "#big{font:32px/40px serif}",
    );
    let host = find(tree.root(), id(&doc, "host"));
    let text = fragments(host);
    close(
        text[1].x - text[0].x,
        super::inline::measure_text(&font, "a ", 32.0),
    );
    close(host.line_boxes()[0].rect.height, 40.0);
    let (doc, tree, _) = layout(
        200.0,
        "normal",
        "a<span id='pre'>  </span> b",
        "#pre{white-space:pre}",
    );
    assert_eq!(lines(find(tree.root(), id(&doc, "host"))), ["a  b"]);
}

#[test]
fn preserved_only_anonymous_runs_are_real_and_descendant_styles_survive_block_splitting() {
    for (mode, anonymous) in [
        ("normal", 0),
        ("pre", 2),
        ("pre-wrap", 2),
        ("break-spaces", 2),
        ("pre-line", 2),
    ] {
        let (doc, tree, _) = layout(
            200.0,
            mode,
            " \n<div id='block' style='height:10px'></div>\n ",
            "",
        );
        let host = find(tree.root(), id(&doc, "host"));
        let wrappers: Vec<_> = host.children.iter().filter(|c| c.is_anonymous()).collect();
        assert_eq!(wrappers.len(), anonymous, "{mode}");
        if anonymous != 0 {
            assert!(wrappers.iter().all(|c| !c.line_boxes().is_empty()));
            assert_eq!(
                find(tree.root(), id(&doc, "block")).rect.y - host.content_rect.y,
                20.0
            );
        }
    }
    let (doc, tree, _) = layout(
        200.0,
        "normal",
        "<span id='s'> <div id='block'></div> </span>",
        "#s{white-space:pre}#block{height:10px}",
    );
    let host = find(tree.root(), id(&doc, "host"));
    assert_eq!(host.children.len(), 3);
    assert!(host.children[0].is_anonymous());
    assert_eq!(host.children[0].line_boxes().len(), 1);
    assert_eq!(host.children[2].line_boxes().len(), 1);
    assert_eq!(host.content_rect.height, 50.0);
    assert_eq!(
        find(tree.root(), id(&doc, "block"))
            .style
            .white_space_collapse,
        style::WhiteSpaceCollapse::Preserve
    );
    let (doc, tree, _) = layout(200.0, "pre", "  ", "#host{line-height:0;margin:10px 0}");
    assert_eq!(find(tree.root(), id(&doc, "host")).line_boxes().len(), 1);
}

#[test]
fn preserved_whitespace_prevents_margin_collapse_through_even_with_zero_line_height() {
    for (mode, line_height, expected_gap, count) in [
        ("normal", 20, 60.0, 0),
        ("pre-line", 20, 60.0, 0),
        ("pre", 20, 120.0, 1),
        ("pre", 0, 100.0, 1),
    ] {
        let (doc, tree, _) = layout(
            200.0,
            "normal",
            "<div id='a'></div><div id='space'> \t </div><div id='b'></div>",
            &format!(
                "#host{{padding-top:1px}}#a,#b{{height:20px}}#a{{margin-bottom:30px}}\
             #space{{white-space:{mode};line-height:{line_height}px;margin:40px 0 50px}}#b{{margin-top:60px}}"
            ),
        );
        let a = find(tree.root(), id(&doc, "a"));
        let b = find(tree.root(), id(&doc, "b"));
        assert_eq!(
            b.rect.y - a.rect.y - a.rect.height,
            expected_gap,
            "{mode}/{line_height}"
        );
        assert_eq!(find(tree.root(), id(&doc, "space")).line_boxes.len(), count);
    }
}

#[test]
fn nonprinting_controls_and_positional_tabs_never_rasterize_missing_glyph_boxes() {
    let raster = |tree: &LayoutTree, font: &crate::text::FontContext| {
        let display = paint::build_display_list(tree, 448.0, 100.0, false);
        let mut pixels = vec![0xffffff; 448 * 100];
        paint::rasterize_display_list(&display, font, &mut pixels, 448, 100, 1.0, 0.0);
        pixels
    };
    let (_, plain, font) = layout(200.0, "normal", "cooperate", "");
    for source in [
        "co&shy;operate",
        "co&#x200b;operate",
        "co&#x2060;operate",
        "co&#xfeff;operate",
    ] {
        let (_, tree, _) = layout(200.0, "normal", source, "");
        assert_eq!(raster(&tree, &font), raster(&plain, &font), "{source}");
    }
    let (_, tree, _) = layout(200.0, "pre", "a\tb", "");
    let display = paint::build_display_list(&tree, 448.0, 100.0, false);
    let filtered: Vec<_> = display
        .iter()
        .filter(|item| !matches!(item, paint::DisplayItem::Text{text,..} if text == "\t"))
        .cloned()
        .collect();
    let mut pixels = vec![0xffffff; 448 * 100];
    paint::rasterize_display_list(&filtered, &font, &mut pixels, 448, 100, 1.0, 0.0);
    assert_eq!(raster(&tree, &font), pixels);
}

#[test]
fn wrapped_decorations_baselines_paint_resize_and_long_content_stay_finite() {
    let (doc, mut tree, font) = layout(
        100.0,
        "pre-wrap",
        "<a id='link' href='#host'>你好，世界 /one/two/three a\tb c </a>",
        "#host{width:100%;text-align:center}#link{padding:2px 3px;border:1px solid}",
    );
    tree.layout(148.0, &font);
    let small = tree.clone();
    let host = find(tree.root(), id(&doc, "host"));
    let link = find(tree.root(), id(&doc, "link"));
    assert_eq!(link.inline_fragments.len(), host.line_boxes.len());
    assert!(link.inline_fragments[0].first);
    assert!(link.inline_fragments.last().unwrap().last);
    for fragment in fragments(host) {
        assert!(
            host.line_boxes
                .iter()
                .any(|line| line.baseline == fragment.baseline)
        );
    }
    let display = paint::build_display_list(&tree, 148.0, 500.0, false);
    assert!(!display.iter().any(|item| matches!(item, paint::DisplayItem::Text{text,..} if text.contains('\u{200b}') || text.contains('\u{2060}'))));
    tree.layout(848.0, &font);
    tree.layout(148.0, &font);
    assert_eq!(tree, small);
    let content = format!("{}{}", "a".repeat(20_000), "你好/ ".repeat(2_000));
    let (doc, tree, _) = layout(100.0, "normal", &content, "");
    let host = find(tree.root(), id(&doc, "host"));
    assert!(host.line_boxes.len() > 100);
    assert!(
        host.line_boxes
            .iter()
            .all(|line| line.rect.y.is_finite() && line.content_rect.width.is_finite())
    );
    assert!(tree.diagnostics().is_empty());
}
