use super::*;
use crate::{html::parser, paint::load_bundled_font};

fn layout(source: &str, width: f32) -> LayoutTree {
    let document = parser::parse(source).unwrap();
    let mut tree = build_layout_tree(&document);
    tree.layout(width, &load_bundled_font().unwrap());
    tree
}

fn find<'a>(layout_box: &'a LayoutBox, tag: &str) -> Option<&'a LayoutBox> {
    if layout_box.tag_name() == tag {
        return Some(layout_box);
    }
    layout_box
        .children()
        .iter()
        .find_map(|child| find(child, tag))
}

#[test]
fn wrapped_link_hit_testing_uses_fragments_not_whitespace_between_lines() {
    let document =
        parser::parse("<p><a href='next'><b>one two three four five six seven</b></a></p>")
            .unwrap();
    let mut tree = build_layout_tree(&document);
    tree.layout(160.0, &load_bundled_font().unwrap());
    let text = find(tree.root(), "#text").unwrap();
    let fragments = text.text_fragments();
    assert!(fragments.iter().any(|fragment| fragment.y > fragments[0].y));
    for fragment in fragments {
        let hit = tree
            .hit_test(fragment.x + fragment.width / 2.0, fragment.y + 1.0)
            .unwrap();
        assert_eq!(hit, text.dom_node_id());
        assert!(
            crate::engine::link_url(
                &document,
                hit,
                &url::Url::parse("https://example.org/").unwrap()
            )
            .unwrap()
            .is_some()
        );
    }
    let last = fragments.last().unwrap();
    let hit = tree
        .hit_test(last.x + last.width + 3.0, last.y + 1.0)
        .unwrap();
    assert!(
        crate::engine::link_url(
            &document,
            hit,
            &url::Url::parse("https://example.org/").unwrap()
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn builds_block_and_inline_boxes_and_skips_non_rendered_elements() {
    let tree = layout(
        "<head><title>hidden</title></head><div>shown <em>emphasis</em></div>\
         <script>script</script><style>style</style>",
        400.0,
    );
    let div = find(tree.root(), "<div>").unwrap();
    assert_eq!(div.kind(), BoxKind::Block);
    assert_eq!(div.children()[0].kind(), BoxKind::Text);
    assert_eq!(div.children()[1].kind(), BoxKind::Inline);
    for tag in ["<head>", "<script>", "<style>"] {
        assert!(find(tree.root(), tag).is_none());
    }
}

#[test]
fn stacks_blocks_at_parent_width_and_applies_paragraph_and_heading_spacing() {
    let tree = layout(
        "<div><h1>Heading</h1><p>First</p><p>Second</p></div>",
        400.0,
    );
    let heading = find(tree.root(), "<h1>").unwrap();
    let paragraph = find(tree.root(), "<p>").unwrap();
    assert!((heading.rect().width - 352.0).abs() < 0.01);
    let fragment = &find(heading, "#text").unwrap().text_fragments()[0];
    assert_eq!(fragment.font_size, 32.0);
    assert!(fragment.bold);
    assert!(paragraph.rect().y > heading.rect().y + heading.rect().height);
    assert!(tree.root().rect().height > paragraph.rect().y);
}

#[test]
fn ua_styles_classify_common_structural_elements_as_blocks() {
    for tag in ["body", "section", "main", "ul", "li", "h6", "p"] {
        let tree = layout(&format!("<{tag}>text</{tag}>"), 400.0);
        assert_eq!(
            find(tree.root(), &format!("<{tag}>")).unwrap().kind(),
            BoxKind::Block
        );
    }
    for tag in ["a", "span", "strong", "em", "br"] {
        let tree = layout(&format!("<{tag}>text</{tag}>"), 400.0);
        assert_eq!(
            find(tree.root(), &format!("<{tag}>")).unwrap().kind(),
            BoxKind::Inline
        );
    }
}

#[test]
fn inline_flow_wraps_words_and_respects_explicit_line_breaks() {
    let tree = layout("<p>alpha beta<br>gamma</p>", 110.0);
    let paragraph = find(tree.root(), "<p>").unwrap();
    let texts: Vec<_> = paragraph
        .children()
        .iter()
        .filter(|child| child.kind() == BoxKind::Text)
        .collect();
    let first = texts[0].text_fragments()[0].y;
    let last = texts[1].text_fragments().last().unwrap().y;
    assert!(last > first);
    assert!(paragraph.rect().height >= last - paragraph.rect().y);
}

#[test]
fn emphasis_styles_are_inherited_by_nested_text() {
    let tree = layout("<p><strong>bold <em>bold italic</em></strong></p>", 400.0);
    let strong = find(tree.root(), "<strong>").unwrap();
    let em = find(strong, "<em>").unwrap();
    let bold = &find(strong, "#text").unwrap().text_fragments()[0];
    let both = &find(em, "#text").unwrap().text_fragments()[0];
    assert!(bold.bold && !bold.italic);
    assert!(both.bold && both.italic);
}

#[test]
fn list_items_receive_bullets_and_ordered_numbers() {
    let tree = layout(
        "<ul><li>one</li></ul><ol><li>first</li><li>second</li></ol>",
        400.0,
    );
    let ul = find(tree.root(), "<ul>").unwrap();
    let ol = find(tree.root(), "<ol>").unwrap();
    assert_eq!(ul.children()[0].list_marker().unwrap().text, "\u{2022}");
    assert_eq!(ol.children()[0].list_marker().unwrap().text, "1.");
    assert_eq!(ol.children()[1].list_marker().unwrap().text, "2.");
    assert_eq!(ul.children()[0].rect().x, 56.0);
}

#[test]
fn box_model_resolves_content_width_padding_border_and_margins() {
    let tree = layout(
        "<div style='width:50%; margin:10px 20px; padding:5px 10%; border-width:2px'>\
         <span>text</span></div><p>next</p>",
        448.0,
    );
    let div = find(tree.root(), "<div>").unwrap();
    assert_eq!(div.content_rect().width, 200.0);
    assert_eq!(div.rect().width, 284.0);
    assert_eq!(div.rect().x, 44.0);
    assert_eq!(div.rect().y, 34.0);
    assert_eq!(div.content_rect().x, 86.0);
    assert_eq!(div.content_rect().y, 41.0);
    assert_eq!(div.rect().height, 34.0);
    assert_eq!(find(tree.root(), "<p>").unwrap().rect().y, 94.0);
}

#[test]
fn auto_width_and_em_lengths_use_the_computed_font_size() {
    let tree = layout(
        "<div style='font-size:20px; padding:1em; margin:5%'>text</div>",
        448.0,
    );
    let div = find(tree.root(), "<div>").unwrap();
    assert_eq!(div.content_rect().width, 320.0);
    assert_eq!(div.rect().width, 360.0);
    assert_eq!(div.content_rect().x, 64.0);
    assert_eq!(div.content_rect().y, 64.0);
    assert_eq!(div.rect().height, 65.0);
}

#[test]
fn display_none_prunes_subtrees_and_author_display_controls_box_generation() {
    let tree = layout(
        "<style>p {display:inline} span {display:block}</style>\
         <div style='display:none'><h1>hidden</h1></div><p>first</p><span>second</span>",
        400.0,
    );
    assert!(find(tree.root(), "<div>").is_none());
    assert!(find(tree.root(), "<h1>").is_none());
    assert_eq!(find(tree.root(), "<p>").unwrap().kind(), BoxKind::Inline);
    assert_eq!(find(tree.root(), "<span>").unwrap().kind(), BoxKind::Block);
}

#[test]
fn nested_block_display_interrupts_inline_flow() {
    let tree = layout(
        "<p><em>before<span style='display:block; margin:0'>middle</span>after</em></p>",
        400.0,
    );
    let em = find(tree.root(), "<em>").unwrap();
    let before = em.children()[0].rect();
    let middle = em.children()[1].rect();
    let after = em.children()[2].rect();
    assert!(middle.y >= before.y + before.height);
    assert!(after.y >= middle.y + middle.height);
}

#[test]
fn inline_padding_and_borders_reserve_space_and_have_geometry() {
    let tree = layout(
        "<div><span style='padding:2px 4px; border-width:1px'>A</span>B</div>",
        400.0,
    );
    let div = find(tree.root(), "<div>").unwrap();
    let span = &div.children()[0];
    assert_eq!(span.rect().width, span.content_rect().width + 10.0);
    assert_eq!(span.rect().height, span.content_rect().height + 6.0);
    assert!(div.children()[1].rect().x >= span.rect().x + span.rect().width);
}

#[test]
fn relayout_recomputes_percentages_and_discards_stale_fragments() {
    let mut tree = layout(
        "<div style='width:50%; padding:5%'>one two three four five six</div>",
        448.0,
    );
    let initial_width = find(tree.root(), "<div>").unwrap().rect().width;
    let font = load_bundled_font().unwrap();
    tree.layout(248.0, &font);
    assert!(find(tree.root(), "<div>").unwrap().rect().width < initial_width);
    let once = tree.clone();
    tree.layout(248.0, &font);
    assert_eq!(tree, once);
}
