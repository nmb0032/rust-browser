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

fn assert_normalized(layout_box: &LayoutBox) {
    if layout_box.kind() == BoxKind::Block {
        assert!(
            layout_box
                .children()
                .iter()
                .all(|child| child.kind() == BoxKind::Block)
                || layout_box
                    .children()
                    .iter()
                    .all(|child| child.kind() != BoxKind::Block),
            "mixed children of {}",
            layout_box.tag_name()
        );
    } else {
        assert!(
            layout_box
                .children()
                .iter()
                .all(|child| child.kind() != BoxKind::Block)
        );
    }
    for child in layout_box.children() {
        assert_normalized(child);
    }
}

#[test]
fn anonymous_blocks_group_leading_middle_trailing_and_multiple_inline_runs() {
    for (contents, expected) in [
        ("Leading<em>inline</em><div>Block</div>", vec![true, false]),
        ("<div>Block</div>Trailing<em>inline</em>", vec![false, true]),
        (
            "<div>First</div>Middle<em>inline</em><div>Last</div>",
            vec![false, true, false],
        ),
        (
            "Leading<div>First</div>Middle<div>Last</div>Trailing",
            vec![true, false, true, false, true],
        ),
    ] {
        let tree = layout(
            &format!("<section style='font:16px/20px serif'>{contents}</section>"),
            400.0,
        );
        let section = find(tree.root(), "<section>").unwrap();
        assert_eq!(
            section
                .children()
                .iter()
                .map(LayoutBox::is_anonymous)
                .collect::<Vec<_>>(),
            expected,
            "{contents}"
        );
        assert_normalized(tree.root());
        for (index, child) in section.children().iter().enumerate() {
            assert_eq!(child.kind(), BoxKind::Block);
            assert_eq!(child.rect().x, section.content_rect().x);
            assert_eq!(child.rect().width, section.content_rect().width);
            assert_eq!(
                child.rect().y,
                section.content_rect().y + index as f32 * 20.0
            );
            assert_eq!(child.rect().height, 20.0);
            if child.is_anonymous() {
                assert_eq!(child.dom_node_id(), None);
                assert_eq!(child.pseudo_element(), None);
                assert!(
                    child
                        .children()
                        .iter()
                        .all(|child| child.kind() != BoxKind::Block)
                );
            }
        }
    }
}

#[test]
fn anonymous_generation_discards_only_collapsible_whitespace_runs_and_preserves_pure_trees() {
    let tree = layout(
        "<section> \t\n<div>First</div> \r\n <div>Last</div>\n </section>\
         <article> \t Inline <em>only</em> \n </article>\
         <main><div>Only</div><div>blocks</div></main>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    assert_eq!(section.children().len(), 2);
    assert!(section.children().iter().all(|child| !child.is_anonymous()));
    let article = find(tree.root(), "<article>").unwrap();
    assert_eq!(article.children().len(), 3);
    assert_eq!(article.children()[0].text(), Some(" \t Inline "));
    assert_eq!(article.children()[1].kind(), BoxKind::Inline);
    assert_eq!(article.children()[2].text(), Some(" \n "));
    assert!(article.children().iter().all(|child| !child.is_anonymous()));
    let main = find(tree.root(), "<main>").unwrap();
    assert_eq!(main.children().len(), 2);
    assert!(main.children().iter().all(|child| !child.is_anonymous()));
    assert_normalized(tree.root());

    // NBSP is not collapsible CSS whitespace. Empty inline elements and
    // explicit line breaks are real inline boxes, not ignorable text runs.
    let tree = layout(
        "<section>&nbsp;<div>Block</div><span></span><br></section>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    assert_eq!(section.children().len(), 3);
    assert_eq!(section.children()[0].children()[0].text(), Some("\u{a0}"));
    assert_eq!(section.children()[2].children().len(), 2);
    assert_eq!(section.children()[2].children()[0].tag_name(), "<span>");
    assert_eq!(section.children()[2].children()[1].tag_name(), "<br>");

    let tree = layout(
        "<section>Visible<span style='display:none'><div>Hidden</div></span></section>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    assert_eq!(section.children().len(), 1);
    assert_eq!(section.children()[0].kind(), BoxKind::Text);
}

#[test]
fn anonymous_styles_inherit_typography_and_reset_noninherited_box_properties() {
    use crate::{
        css::values::Edges,
        style::{ComputedStyle, Display},
    };
    let tree = layout(
        "<style>section {--tone:blue;color:red;-webkit-text-fill-color:blue;\
         font:italic bold 22px/2 serif;width:200px;height:160px;\
         box-sizing:border-box;margin:7px;padding:9px;border:3px solid green;\
         background:yellow;container:owner / size}</style>\
         <section>Before<div>Block</div>After</section>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    let mut expected = ComputedStyle::inherit(section.style());
    expected.display = Display::Block;
    for child in [&section.children()[0], &section.children()[2]] {
        assert!(child.is_anonymous());
        assert_eq!(child.style(), &expected);
        assert_eq!(child.style().font_size, 22.0);
        assert!(child.style().bold && child.style().italic);
        assert_eq!(child.style().color, Some(0xff0000));
        assert_eq!(child.style().text_fill_color.legacy(), Some(0x0000ff));
        assert_eq!(
            child.style().custom_properties,
            section.style().custom_properties
        );
        assert_eq!(child.style().width, None);
        assert_eq!(child.style().height, None);
        assert_eq!(child.style().margin, Edges::default());
        assert_eq!(child.style().padding, Edges::default());
        assert_eq!(child.style().border_width, Edges::default());
        assert_eq!(child.style().background_color, None);
        assert_eq!(
            child.style().container_type,
            crate::css::container::ContainerType::Normal
        );
        assert!(child.style().container_names.is_empty());
        assert_eq!(child.rect(), child.content_rect());
        assert_eq!(child.rect().width, section.content_rect().width);
        assert_eq!(child.rect().height, 44.0);
    }
    assert!(tree.diagnostics().is_empty());
}

#[test]
fn blocks_split_every_inline_ancestor_without_losing_styles_order_or_containing_width() {
    let tree = layout(
        "<style>section{width:220px;font:16px/20px serif}\
         em{color:red;padding:2px 8px;margin:0 5px;border:3px solid green}\
         strong{color:blue} .block{display:block;margin:0}</style>\
         <section>Outside <em>Before <strong>Deep <span class='block'>Block</span>\
         Return</strong> After</em> End</section>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    assert_eq!(section.children().len(), 3);
    assert_normalized(tree.root());
    let before = &section.children()[0];
    let block = &section.children()[1];
    let after = &section.children()[2];
    assert!(before.is_anonymous() && after.is_anonymous());
    assert_eq!(block.tag_name(), "<span>");
    assert_eq!(block.style().color, Some(0x0000ff));
    assert_eq!(block.rect().x, section.content_rect().x);
    assert_eq!(block.rect().width, 220.0);
    assert!(block.rect().y >= before.rect().y + before.rect().height);
    assert!(after.rect().y >= block.rect().y + block.rect().height);
    let first_em = find(before, "<em>").unwrap();
    let last_em = find(after, "<em>").unwrap();
    assert_eq!(first_em.dom_node_id(), last_em.dom_node_id());
    assert_eq!(first_em.style(), last_em.style());
    assert_eq!(first_em.style().color, Some(0xff0000));
    assert!(first_em.inline_start && !first_em.inline_end);
    assert!(!last_em.inline_start && last_em.inline_end);
    assert_eq!(first_em.rect().width, first_em.content_rect().width + 11.0);
    assert_eq!(last_em.rect().width, last_em.content_rect().width + 11.0);
    assert_eq!(first_em.padding.right, 0.0);
    assert_eq!(last_em.padding.left, 0.0);
    let first_strong = find(before, "<strong>").unwrap();
    let last_strong = find(after, "<strong>").unwrap();
    assert_eq!(first_strong.dom_node_id(), last_strong.dom_node_id());
    assert_eq!(first_strong.style(), last_strong.style());
    assert_eq!(find(first_strong, "#text").unwrap().text(), Some("Deep "));
    assert_eq!(find(last_strong, "#text").unwrap().text(), Some("Return"));
}

#[test]
fn inline_splits_keep_empty_end_continuations_but_not_between_consecutive_blocks() {
    let tree = layout(
        "<style>.block{display:block;height:12px} em{border:2px solid blue}</style>\
         <section><em><span class='block'></span><span class='block'></span></em></section>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    assert_eq!(section.children().len(), 4);
    assert!(section.children()[0].is_anonymous());
    assert!(section.children()[3].is_anonymous());
    for child in [&section.children()[0], &section.children()[3]] {
        assert_eq!(child.children().len(), 1);
        assert!(child.children()[0].children().is_empty());
        assert!(child.rect().height > 0.0, "empty decorated continuation");
    }
    assert_normalized(tree.root());
    let tree = layout(
        "<section><em><span style='display:block;height:12px'></span></em></section>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    assert_eq!(section.children().len(), 3);
    assert_eq!(section.children()[0].rect().height, 0.0);
    assert_eq!(section.children()[2].rect().height, 0.0);
    assert_eq!(section.rect().height, 12.0);
}

#[test]
fn generated_replaced_and_list_item_content_participate_in_anonymous_generation() {
    use crate::{
        paint::{self, DisplayItem},
        style::PseudoElement,
    };
    let tree = layout(
        "<style>section{font:16px/20px serif}\
         section::before{content:'Prelude';display:block;height:20px}\
         em::before{content:'Inner';display:block;height:20px}\
         section::after{content:'End'}\
         .item{display:list-item}</style>\
         <section>Before<img alt='Image' width='30' height='20'>\
         <em>Inside</em><span class='item'>Item</span></section>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    assert_eq!(section.children().len(), 6);
    assert_eq!(
        section.children()[0].pseudo_element(),
        Some(PseudoElement::Before)
    );
    assert!(section.children()[1].is_anonymous());
    let image = find(&section.children()[1], "<img>").unwrap();
    assert_eq!(image.content_rect().width, 30.0);
    assert_eq!(image.content_rect().height, 20.0);
    assert_eq!(
        section.children()[2].pseudo_element(),
        Some(PseudoElement::Before)
    );
    assert_eq!(section.children()[2].tag_name(), "::before");
    assert!(section.children()[3].is_anonymous());
    assert_eq!(
        section.children()[4].list_marker().unwrap().text,
        "\u{2022}"
    );
    assert!(section.children()[5].is_anonymous());
    let after = find(&section.children()[5], "::after").unwrap();
    assert_eq!(after.pseudo_element(), Some(PseudoElement::After));
    assert_normalized(tree.root());
    let display = paint::build_display_list(&tree, 400.0, 300.0, false);
    for text in [
        "Prelude", "Before", "Image", "Inner", "Inside", "Item", "End",
    ] {
        assert!(
            display
                .iter()
                .any(|item| matches!(item, DisplayItem::Text{text:t,..} if t==text)),
            "{text}"
        );
    }
}

#[test]
fn anonymous_hit_testing_and_node_geometry_use_real_inline_continuations_not_wrappers_or_pseudos() {
    let document = parser::parse(
        "<style>section{width:240px;font:16px/20px serif}\
         a{background:yellow}\
         a::before{content:'Generated';display:block;width:200px;height:20px;color:blue}\
         .block{display:block;width:80px;height:20px}</style>\
         <section><a href='next'>Before<span class='block'>Block</span>After</a></section>",
    )
    .unwrap();
    let original_dom = crate::devtools::format_dom(&document);
    let mut tree = build_layout_tree(&document);
    let font = load_bundled_font().unwrap();
    tree.layout(400.0, &font);
    let section = find(tree.root(), "<section>").unwrap();
    let link = find(tree.root(), "<a>").unwrap().dom_node_id().unwrap();
    let mut links = Vec::new();
    for anonymous in section
        .children()
        .iter()
        .filter(|child| child.is_anonymous())
    {
        links.push(find(anonymous, "<a>").unwrap());
        assert_eq!(anonymous.dom_node_id(), None);
        let whitespace_x = anonymous.rect().x + anonymous.rect().width - 1.0;
        assert_eq!(
            tree.hit_test(whitespace_x, anonymous.rect().y + 1.0),
            section.dom_node_id()
        );
    }
    assert_eq!(links.len(), 3); // Empty continuation before the block pseudo, then Before/After.
    let union = links
        .iter()
        .fold(Rect::default(), |rect, piece| rect.union(piece.rect()));
    assert_eq!(tree.node_rect(link), Some(union));
    assert!(
        union.width < 200.0,
        "block pseudo must not enlarge originating node geometry"
    );
    let pseudo = find(tree.root(), "::before").unwrap();
    let fragment = &pseudo.children()[0].text_fragments()[0];
    assert_eq!(
        tree.hit_test(fragment.x + 1.0, fragment.y + 1.0),
        Some(link)
    );
    for piece in &links[1..] {
        let text = find(piece, "#text").unwrap();
        let fragment = &text.text_fragments()[0];
        let hit = tree.hit_test(fragment.x + 1.0, fragment.y + 1.0);
        assert_eq!(hit, text.dom_node_id());
        assert!(
            crate::engine::link_url(
                &document,
                hit.unwrap(),
                &url::Url::parse("https://example.test/").unwrap()
            )
            .unwrap()
            .is_some()
        );
    }
    assert_eq!(crate::devtools::format_dom(&document), original_dom);
    tree.layout(400.0, &font);
    let once = tree.clone();
    tree.layout(400.0, &font);
    assert_eq!(tree, once);
}

#[test]
fn anonymous_boxes_are_visible_in_value_dumps_and_debug_outlines_without_decorations() {
    use crate::paint::{self, DisplayItem};
    let tree = layout(
        "<style>section{width:200px;font:16px/20px serif;background:yellow;border:2px solid blue}</style>\
         <section>Before<div>Block</div>After</section>",
        400.0,
    );
    let section = find(tree.root(), "<section>").unwrap();
    let plain = paint::build_display_list(&tree, 400.0, 300.0, false);
    let debug = paint::build_display_list(&tree, 400.0, 300.0, true);
    let values = crate::devtools::format_values(&tree, 1.0);
    assert_eq!(values.matches("Anonymous #anonymous-block").count(), 2);
    assert!(!values.contains("Node None"));
    for child in section
        .children()
        .iter()
        .filter(|child| child.is_anonymous())
    {
        let rect = child.rect();
        assert!(debug.iter().any(|item| matches!(
            item, DisplayItem::OutlineRect {x,y,w,h,..}
            if (*x,*y,*w,*h)==(rect.x,rect.y,rect.width,rect.height)
        )));
        assert!(!plain.iter().any(|item| matches!(
            item, DisplayItem::Rect {x,y,w,h,..}
            if (*x,*y,*w,*h)==(rect.x,rect.y,rect.width,rect.height)
        )));
    }
    let mut pixels = vec![0xffffff; 400 * 300];
    paint::rasterize_display_list(
        &debug,
        &load_bundled_font().unwrap(),
        &mut pixels,
        400,
        300,
        1.0,
        0.0,
    );
    let rect = section.children()[2].rect();
    let offset = rect.y as usize * 400 + (rect.x + rect.width - 1.0) as usize;
    assert_eq!(
        pixels[offset], 0xff0000,
        "anonymous block's full-width debug outline"
    );
}

#[test]
fn compatibility_box_sizing_uses_content_floor_and_replaced_dimensions() {
    for (sizing, content_width, content_height) in
        [("content-box", 80.0, 30.0), ("border-box", 56.0, 6.0)]
    {
        for display in ["block", "inline"] {
            let tree = layout(&format!("<!doctype html><style>body{{margin:0}}
                img{{display:{display};-webkit-box-sizing:{sizing};width:80px;height:30px;padding:10px;border:2px solid blue}}
                </style><img alt='alt'>"), 400.0);
            let image = find(tree.root(), "<img>").unwrap();
            assert_eq!(
                image.content_rect().width,
                content_width,
                "{display}: {sizing}"
            );
            assert_eq!(
                image.content_rect().height,
                content_height,
                "{display}: {sizing}"
            );
            assert_eq!(image.rect().width, content_width + 24.0);
            assert_eq!(image.rect().height, content_height + 24.0);
        }
        let tree = layout(
            &format!(
                "<!doctype html><style>p{{box-sizing:{sizing};width:80px;height:30px;padding:10px;border:2px solid blue}}</style><p></p>"
            ),
            400.0,
        );
        let p = find(tree.root(), "<p>").unwrap();
        assert_eq!(p.content_rect().width, content_width);
        assert_eq!(p.content_rect().height, content_height);
    }
    let tree = layout(
        "<!doctype html><p style='box-sizing:border-box;width:1px;height:1px;padding:10px;border:2px solid red'></p>",
        400.0,
    );
    let p = find(tree.root(), "<p>").unwrap();
    assert_eq!(p.content_rect().width, 0.0);
    assert_eq!(p.content_rect().height, 0.0);
    assert_eq!(p.rect().width, 24.0);
    assert_eq!(p.rect().height, 24.0);
    let tree = layout(
        "<!doctype html><img width='80' height='30' style='box-sizing:border-box;padding:10px;border:2px solid red'>",
        400.0,
    );
    let image = find(tree.root(), "<img>").unwrap();
    assert_eq!(image.content_rect().width, 56.0);
    assert_eq!(image.content_rect().height, 6.0);
    let tree = layout(
        "<!doctype html><img width='80' height='30' style='height:auto'>",
        400.0,
    );
    let image = find(tree.root(), "<img>").unwrap();
    assert_eq!(image.content_rect().width, 80.0);
    assert_eq!(image.content_rect().height, 100.0);
}

#[test]
fn compatibility_text_fill_reaches_glyphs_generated_text_markers_and_alt_text_only() {
    use crate::paint::{self, DisplayItem};
    let tree = layout(
        "<!doctype html><style>
        body{margin:0;color:red;-webkit-text-fill-color:blue}
        p{margin:0;border:2px solid currentcolor;background:currentcolor}
        p::before{content:'Before'}
        li::marker{content:'Mark'}
        </style><p>Origin</p><ul><li>Item</li></ul><img alt='Alt'>",
        400.0,
    );
    let display = paint::build_display_list(&tree, 400.0, 300.0, false);
    for text in ["Origin", "Before", "Item", "Mark", "Alt"] {
        assert!(
            display
                .iter()
                .any(|item| matches!(item, DisplayItem::Text{text:t,color:0x0000ff,..} if t==text)),
            "{text}: {display:?}"
        );
    }
    assert!(display.iter().any(|item| matches!(
        item,
        DisplayItem::Rect {
            color: 0xff0000,
            ..
        }
    )));
    let mut pixels = vec![0xffffff; 400 * 300];
    paint::rasterize_display_list(
        &display,
        &load_bundled_font().unwrap(),
        &mut pixels,
        400,
        300,
        1.0,
        0.0,
    );
    assert!(pixels.contains(&0xff0000));
    assert!(pixels.contains(&0x0000ff));
}

#[test]
fn generated_content_uses_separate_boxes_styles_hit_targets_and_paint_without_dom_mutation() {
    use crate::{
        paint::{self, DisplayItem},
        style::{self, PseudoElement},
    };
    let document = parser::parse(
        "<style>body {margin:0} #target {color:red; margin:0}
        #target::before {content:'Before-' attr(data-label); color:blue; background:yellow; padding:2px}
        #target::after {content:'After'; color:green; display:block; width:100px; background:purple}
        #target::first-line {color:purple}
        #hidden::before {content:'Hidden'; display:none}
        #empty::before {content:''; padding:2px; background:yellow}
        #normal::before {content:normal}
        #input::before {content:'Not-a-widget'}
        </style><p id='target' data-label='Label'>Origin</p><p id='hidden'></p>
        <p id='empty'></p><p id='normal'></p><input id='input'>",
    ).unwrap();
    let dom_before = crate::devtools::format_dom(&document);
    let count = document.node_count();
    let resolution = style::resolve(&document);
    let mut tree = build_layout_tree_with_styles(&document, &resolution.styles);
    let font = load_bundled_font().unwrap();
    tree.layout(320.0, &font);
    let before = find(tree.root(), "::before").unwrap();
    let after = find(tree.root(), "::after").unwrap();
    assert_eq!(before.pseudo_element(), Some(PseudoElement::Before));
    assert_eq!(after.pseudo_element(), Some(PseudoElement::After));
    assert_eq!(before.style().color, Some(0x0000ff));
    assert_eq!(after.rect().width, 100.0);
    assert_eq!(before.children()[0].text(), Some("Before-Label"));
    assert_eq!(before.children()[0].dom_node_id(), before.dom_node_id());
    let fragment = &before.children()[0].text_fragments()[0];
    assert_eq!(
        tree.hit_test(fragment.x + 1.0, fragment.y + 1.0),
        before.dom_node_id()
    );
    let list = paint::build_display_list(&tree, 320.0, 300.0, false);
    for (text, color) in [
        ("Before-Label", 0x0000ff),
        ("Origin", 0xff0000),
        ("After", 0x008000),
    ] {
        assert!(
            list.iter().any(
                |item| matches!(item, DisplayItem::Text {text:t,color:c,..} if t==text && *c==color)
            ),
            "{text}"
        );
    }
    assert!(!list.iter().any(
        |item| matches!(item, DisplayItem::Text {text,..} if text=="Hidden" || text=="Not-a-widget")
    ));
    assert!(list.iter().any(|item| matches!(
        item,
        DisplayItem::Rect {
            color: 0xffff00,
            ..
        }
    )));
    let mut pixels = vec![0xffffff; 320 * 300];
    paint::rasterize_display_list(&list, &font, &mut pixels, 320, 300, 1.0, 0.0);
    assert!(pixels.contains(&0xffff00));
    assert!(pixels.contains(&0x800080));
    assert_eq!(document.node_count(), count);
    assert_eq!(crate::devtools::format_dom(&document), dom_before);
    assert_eq!(resolution.stylesheets.diagnostics.len(), 1);
    assert!(
        crate::devtools::format_style(&document, &resolution.styles)
            .contains("::first-line computed (separate target)")
    );
}

#[test]
fn marker_font_color_and_content_have_a_real_layout_and_paint_hook() {
    use crate::paint::{self, DisplayItem};
    let tree = layout(
        "<style>li {color:red} li::marker {content:'Mark'; color:blue; font-size:24px; font-weight:bold}
        #none::marker {content:none}</style><ol><li>first</li><li id='none'>second</li></ol>",
        400.0,
    );
    let list = find(tree.root(), "<ol>").unwrap();
    let marker = list.children()[0].list_marker().unwrap();
    assert_eq!(marker.text, "Mark");
    assert_eq!(marker.style.font_size, 24.0);
    assert_eq!(list.children()[0].style().color, Some(0xff0000));
    assert_eq!(list.children()[1].list_marker().unwrap().text, "");
    let display = paint::build_display_list(&tree, 400.0, 300.0, false);
    assert!(display.iter().any(|item| matches!(item,DisplayItem::Text {text,color:0x0000ff,size:24.0,bold:true,..} if text=="Mark")));
}

#[test]
fn generated_text_limit_reports_literal_attribute_and_combined_overflow_and_survives_relayout() {
    use crate::style;
    fn find_node(layout_box: &LayoutBox, id: crate::dom::NodeId) -> Option<&LayoutBox> {
        if layout_box.dom_node_id() == Some(id) {
            return Some(layout_box);
        }
        layout_box
            .children()
            .iter()
            .find_map(|child| find_node(child, id))
    }
    let oversized = "x".repeat(65_537);
    let boundary = "x".repeat(65_536);
    let document = parser::parse(&format!(
        "<style>
        #overflow::before {{content:attr(data-big)}}
        #overflow::after {{content:'x' attr(data-big)}}
        #overflow::marker {{content:'{boundary}' 'x'}}
        #boundary::before {{content:attr(data-big)}}
        #missing::before {{content:attr(missing)}}
        </style><ul><li id='overflow' data-big='{oversized}'></li></ul>
        <p id='boundary' data-big='{boundary}'></p><p id='missing'></p>"
    ))
    .unwrap();
    let id = |name| {
        document
            .preorder()
            .find(|&id| {
                document
                    .node(id)
                    .unwrap()
                    .as_element()
                    .is_some_and(|element| element.attribute("id") == Some(name))
            })
            .unwrap()
    };
    let overflow = id("overflow");
    let missing = id("missing");
    let resolution = style::resolve(&document);
    assert!(
        resolution.stylesheets.diagnostics.is_empty(),
        "{:?}",
        resolution.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&document, &resolution.styles);
    assert_eq!(tree.diagnostics().len(), 3);
    for (diagnostic, pseudo) in tree.diagnostics().iter().zip(["before", "after", "marker"]) {
        assert_eq!(diagnostic.node_id, Some(overflow));
        assert!(
            diagnostic
                .message
                .starts_with(&format!("content::{pseudo}:"))
        );
        assert!(diagnostic.message.contains("65536-byte limit"));
        assert!(diagnostic.message.contains("content omitted"));
    }
    let original = tree.diagnostics().to_vec();
    let marker = find(tree.root(), "<li>").unwrap();
    assert!(marker.children().is_empty());
    assert_eq!(
        find(tree.root(), "::before").unwrap().children()[0]
            .text()
            .unwrap()
            .len(),
        65_536
    );
    let missing_box = find_node(tree.root(), missing).unwrap();
    assert_eq!(missing_box.children()[0].children()[0].text(), Some(""));
    let font = load_bundled_font().unwrap();
    for width in [400.0, 500.0] {
        tree.layout(width, &font);
        assert_eq!(tree.diagnostics(), original);
        assert_eq!(
            find(tree.root(), "<li>")
                .unwrap()
                .list_marker()
                .unwrap()
                .text,
            ""
        );
    }
}

#[test]
fn generated_content_distinguishes_empty_missing_and_normal_from_byte_overflow() {
    use crate::{
        dom::{Attribute, ElementData},
        style::{Content, ContentItem},
    };
    let element = ElementData::new(
        "p".into(),
        vec![Attribute::new("data-value".into(), "é".repeat(32_768))],
    );
    for (content, expected) in [
        (Content::Normal, None),
        (Content::None, None),
        (
            Content::Items(vec![ContentItem::Attribute("missing".into())]),
            Some(String::new()),
        ),
        (
            Content::Items(vec![ContentItem::Text(String::new())]),
            Some(String::new()),
        ),
        (
            Content::Items(vec![ContentItem::Attribute("data-value".into())]),
            Some("é".repeat(32_768)),
        ),
    ] {
        assert_eq!(
            content.text_with_diagnostic(&element, |_| panic!("valid content diagnosed")),
            expected
        );
    }
    let content = Content::Items(vec![
        ContentItem::Attribute("data-value".into()),
        ContentItem::Text("x".into()),
    ]);
    let mut error = None;
    assert_eq!(
        content.text_with_diagnostic(&element, |message| error = Some(message)),
        None
    );
    assert!(error.unwrap().contains("65536-byte limit"));
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
        assert_eq!(Some(hit), text.dom_node_id());
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
fn font_line_height_and_visible_border_shorthands_reach_box_geometry() {
    let tree = layout(
        "<div style='width:100px; font:20px/2 serif; border:6px solid red;
        border-left:hidden; padding:2px'><span>one<br>two</span></div>",
        400.0,
    );
    let div = find(tree.root(), "<div>").unwrap();
    assert_eq!(div.content_rect().height, 80.0);
    assert_eq!(div.rect().width, 110.0);
    assert_eq!(div.rect().height, 96.0);
    let text = find(div, "#text").unwrap();
    assert_eq!(text.text_fragments()[0].height, 40.0);
    assert_eq!(div.style().border_width.left, 0.0);
    assert_eq!(div.style().border_width.right, 6.0);
    let short = layout(
        "<p style='margin:0; font:20px/0.5 serif'>one<br>two</p>",
        400.0,
    );
    assert_eq!(
        find(short.root(), "<p>").unwrap().content_rect().height,
        20.0
    );
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
        "<div style='width:50%; margin:10px 20px; padding:5px 10%; border-width:2px; border-style:solid'>\
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
    // The div's 10px bottom and paragraph's 16px top collapse to 16px.
    assert_eq!(find(tree.root(), "<p>").unwrap().rect().y, 84.0);
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
    let paragraph = find(tree.root(), "<p>").unwrap();
    let before = paragraph.children()[0].rect();
    let middle = paragraph.children()[1].rect();
    let after = paragraph.children()[2].rect();
    assert!(paragraph.children()[0].is_anonymous());
    assert_eq!(paragraph.children()[1].tag_name(), "<span>");
    assert!(paragraph.children()[2].is_anonymous());
    assert!(middle.y >= before.y + before.height);
    assert!(after.y >= middle.y + middle.height);
}

#[test]
fn inline_padding_and_borders_reserve_space_and_have_geometry() {
    let tree = layout(
        "<div><span style='padding:2px 4px; border-width:1px; border-style:solid'>A</span>B</div>",
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

#[test]
fn used_math_failures_are_observable_and_consistent_across_layout_paths_and_relayout() {
    let expression = "calc(1000px / (100% - 200px) * 1px)";
    let font = load_bundled_font().unwrap();
    for (tag, display, has_width) in [
        ("div", "block", true),
        ("span", "inline", false),
        ("img", "inline", true),
        ("img", "block", true),
    ] {
        let width = if has_width {
            format!("width:{expression};")
        } else {
            String::new()
        };
        let document = parser::parse(&format!(
            "<style>body{{margin:0}}</style>\
             <{tag} style='display:{display};{width}margin:{expression};padding:{expression}'></{tag}>"
        )).unwrap();
        let styles = crate::style::resolve(&document);
        assert!(styles.stylesheets.diagnostics.is_empty());
        let mut tree = build_layout_tree_with_styles(&document, &styles.styles);
        tree.layout(448.0, &font);
        assert!(tree.diagnostics().is_empty());
        let target = find(tree.root(), &format!("<{tag}>")).unwrap();
        assert_eq!(target.padding, crate::css::values::Edges::all(5.0));
        if has_width {
            assert_eq!(target.used_width().0, 5.0);
        }
        let wide = tree.clone();

        tree.layout(248.0, &font);
        let target = find(tree.root(), &format!("<{tag}>")).unwrap();
        assert_eq!(target.padding, crate::css::values::Edges::all(0.0));
        assert_eq!(target.used_width().0, 0.0);
        assert_eq!(target.rect().width, 0.0);
        assert_eq!(
            target.rect().x,
            find(tree.root(), "<body>").unwrap().content_rect().x
        );
        assert!(target.actual_rect(2.0).is_some());
        if let Some(width) = target.style().computed_width() {
            assert_eq!(width.use_value(200.0), None);
        }
        assert_eq!(tree.diagnostics().len(), if has_width { 9 } else { 8 });
        for diagnostic in tree.diagnostics() {
            assert_eq!(diagnostic.node_id, target.dom_node_id());
            assert!(diagnostic.message.contains("non-finite CSS length result"));
            assert!(diagnostic.message.contains("value=Math("));
            assert!(diagnostic.message.contains("font-size=16px"));
            assert!(diagnostic.message.contains("reference=200px"));
            assert!(diagnostic.message.contains("compatibility fallback is 0px"));
        }
        for property in ["margin", "padding"] {
            assert_eq!(
                tree.diagnostics()
                    .iter()
                    .filter(|d| d.message.starts_with(property))
                    .count(),
                4
            );
        }
        let narrow = tree.clone();
        tree.layout(248.0, &font);
        assert_eq!(tree, narrow);
        tree.layout(448.0, &font);
        assert_eq!(tree, wide);
    }
}

#[test]
fn overflowing_unitless_line_height_reports_layout_diagnostics_and_keeps_finite_geometry() {
    let tree = layout("<div style='line-height:1e38'>text</div>", 448.0);
    let target = find(tree.root(), "<div>").unwrap();
    assert_eq!(target.rect().height, 0.0);
    assert!(target.actual_rect(2.0).is_some());
    let text = find(target, "#text").unwrap();
    assert_eq!(text.text_fragments()[0].height, 0.0);
    assert_eq!(tree.diagnostics().len(), 2);
    for diagnostic in tree.diagnostics() {
        assert!(diagnostic.message.starts_with("line-height:"));
        assert!(
            diagnostic
                .message
                .contains("non-finite CSS line-height result")
        );
        assert!(diagnostic.message.contains("compatibility fallback is 0px"));
    }
}

#[test]
fn actual_rectangles_reject_nonfinite_edge_sums_and_scale_products() {
    let finite = Rect {
        x: 0.25,
        y: 0.5,
        width: 10.25,
        height: 20.25,
    };
    assert_eq!(
        finite.actual(2.0),
        Some(ActualRect {
            left: 0,
            top: 1,
            right: 21,
            bottom: 42
        })
    );
    for rect in [
        Rect {
            x: f32::MAX,
            ..finite
        },
        Rect {
            y: f32::MAX,
            ..finite
        },
        Rect {
            width: f32::MAX,
            ..finite
        },
        Rect {
            height: f32::MAX,
            ..finite
        },
    ] {
        assert!(rect.actual(2.0).is_none(), "{rect:?}");
    }
    for rect in [
        Rect {
            x: f32::MAX,
            width: f32::MAX,
            ..finite
        },
        Rect {
            y: f32::MAX,
            height: f32::MAX,
            ..finite
        },
    ] {
        assert!(rect.actual(0.5).is_none(), "{rect:?}");
    }
    assert!(finite.actual(f32::INFINITY).is_none());
    assert!(finite.actual(0.0).is_none());
}
