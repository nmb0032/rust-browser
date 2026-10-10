use crate::dom::{Document, NodeId, NodeKind};

const PAGE_PADDING: f32 = 24.0;
const LIST_INDENT: f32 = 32.0;

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "dd",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "html",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "ul",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxKind {
    Block,
    Inline,
    Text,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextFragment {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListMarker {
    pub x: f32,
    pub y: f32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBox {
    node_id: NodeId,
    tag_name: String,
    kind: BoxKind,
    text: Option<String>,
    rect: Rect,
    children: Vec<LayoutBox>,
    text_fragments: Vec<TextFragment>,
    list_marker: Option<ListMarker>,
}

impl LayoutBox {
    pub fn dom_node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn tag_name(&self) -> &str {
        &self.tag_name
    }

    pub fn kind(&self) -> BoxKind {
        self.kind
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn children(&self) -> &[LayoutBox] {
        &self.children
    }

    pub(crate) fn text_fragments(&self) -> &[TextFragment] {
        &self.text_fragments
    }

    pub(crate) fn list_marker(&self) -> Option<&ListMarker> {
        self.list_marker.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutTree {
    root: LayoutBox,
}

impl LayoutTree {
    pub fn root(&self) -> &LayoutBox {
        &self.root
    }

    pub fn layout(&mut self, viewport_width: f32, font: &fontdue::Font) {
        let viewport_width = viewport_width.max(0.0);
        let content_x = PAGE_PADDING.min(viewport_width);
        let content_width = (viewport_width - 2.0 * PAGE_PADDING).max(0.0);
        let base_style = TextStyle::default();
        let content_height = layout_block_children(
            &mut self.root,
            content_x,
            PAGE_PADDING,
            content_width,
            font,
            base_style,
        );
        self.root.rect = Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_width,
            height: PAGE_PADDING + content_height + PAGE_PADDING,
        };
    }
}

pub fn build_layout_tree(document: &Document) -> LayoutTree {
    let root = build_box(document, document.root_id())
        .expect("the document root should always produce a layout box");
    LayoutTree { root }
}

fn build_box(document: &Document, node_id: NodeId) -> Option<LayoutBox> {
    let node = document.node(node_id)?;
    match node.kind() {
        NodeKind::Document => Some(LayoutBox {
            node_id,
            tag_name: "#document".to_string(),
            kind: BoxKind::Block,
            text: None,
            rect: Rect::default(),
            children: node
                .children()
                .iter()
                .filter_map(|child_id| build_box(document, *child_id))
                .collect(),
            text_fragments: Vec::new(),
            list_marker: None,
        }),
        NodeKind::Element(element) => {
            let tag_name = element.tag_name();
            if matches_tag(tag_name, &["head", "script", "style", "title"]) {
                return None;
            }

            Some(LayoutBox {
                node_id,
                tag_name: format!("<{tag_name}>"),
                kind: classify(tag_name),
                text: None,
                rect: Rect::default(),
                children: node
                    .children()
                    .iter()
                    .filter_map(|child_id| build_box(document, *child_id))
                    .collect(),
                text_fragments: Vec::new(),
                list_marker: None,
            })
        }
        NodeKind::Text(text) => Some(LayoutBox {
            node_id,
            tag_name: "#text".to_string(),
            kind: BoxKind::Text,
            text: Some(text.clone()),
            rect: Rect::default(),
            children: Vec::new(),
            text_fragments: Vec::new(),
            list_marker: None,
        }),
    }
}

fn classify(tag_name: &str) -> BoxKind {
    if BLOCK_TAGS
        .iter()
        .any(|block_tag| tag_name.eq_ignore_ascii_case(block_tag))
    {
        BoxKind::Block
    } else {
        BoxKind::Inline
    }
}

#[derive(Clone, Copy)]
struct TextStyle {
    font_size: f32,
    bold: bool,
    italic: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            bold: false,
            italic: false,
        }
    }
}

fn style_for(layout_box: &LayoutBox, inherited: TextStyle) -> TextStyle {
    let tag_name = layout_box.tag_name.trim_matches(['<', '>']);
    let mut style = inherited;

    match tag_name {
        "h1" => style.font_size = 32.0,
        "h2" => style.font_size = 24.0,
        "h3" => style.font_size = 18.72,
        "h4" => style.font_size = 16.0,
        "h5" => style.font_size = 13.28,
        "h6" => style.font_size = 10.72,
        _ => {}
    }

    if matches_tag(tag_name, &["h1", "h2", "h3", "h4", "h5", "h6"]) {
        style.bold = true;
    }
    if matches_tag(tag_name, &["b", "strong"]) {
        style.bold = true;
    }
    if matches_tag(tag_name, &["i", "em"]) {
        style.italic = true;
    }
    style
}

fn layout_block_children(
    layout_box: &mut LayoutBox,
    x: f32,
    y: f32,
    width: f32,
    font: &fontdue::Font,
    inherited_style: TextStyle,
) -> f32 {
    let style = style_for(layout_box, inherited_style);
    let is_list = matches_tag(layout_box.tag_name(), &["<ul>", "<ol>"]);
    let content_x = x + if is_list { LIST_INDENT } else { 0.0 };
    let content_width = (width - if is_list { LIST_INDENT } else { 0.0 }).max(0.0);
    let mut cursor_y = y;
    let mut flow = InlineFlow::new(content_x, cursor_y, content_width, style.font_size);
    let is_ordered_list = layout_box.tag_name() == "<ol>";
    let mut list_index = 0;

    for child in &mut layout_box.children {
        if child.kind == BoxKind::Block {
            cursor_y += flow.content_height();

            if is_list && child.tag_name() == "<li>" {
                list_index += 1;
                child.list_marker = Some(ListMarker {
                    x: content_x - LIST_INDENT + 6.0,
                    y: cursor_y,
                    text: if is_ordered_list {
                        format!("{list_index}.")
                    } else {
                        "\u{2022}".to_string()
                    },
                });
            }

            let child_style = style_for(child, style);
            let (margin_top, margin_bottom) = vertical_margins(child, child_style.font_size);
            cursor_y += margin_top;
            let child_height =
                layout_block_children(child, content_x, cursor_y, content_width, font, style);
            cursor_y += child_height + margin_bottom;
            flow = InlineFlow::new(content_x, cursor_y, content_width, style.font_size);
        } else {
            layout_inline(child, &mut flow, style, font);
        }
    }

    cursor_y += flow.content_height();
    layout_box.rect = Rect {
        x,
        y,
        width,
        height: cursor_y - y,
    };
    cursor_y - y
}

fn vertical_margins(layout_box: &LayoutBox, font_size: f32) -> (f32, f32) {
    let tag_name = layout_box.tag_name();
    let em = match tag_name {
        "<p>" | "<ul>" | "<ol>" => 1.0,
        "<h1>" => 0.67,
        "<h2>" => 0.83,
        "<h3>" => 1.0,
        "<h4>" => 1.33,
        "<h5>" => 1.67,
        "<h6>" => 2.33,
        _ => return (0.0, 0.0),
    };
    let margin = font_size * em;
    (margin, margin)
}

fn layout_inline(
    layout_box: &mut LayoutBox,
    flow: &mut InlineFlow,
    inherited_style: TextStyle,
    font: &fontdue::Font,
) {
    let style = style_for(layout_box, inherited_style);

    if layout_box.tag_name == "<br>" {
        layout_box.rect = flow.break_line();
        return;
    }

    if layout_box.kind == BoxKind::Text {
        let text = layout_box.text.as_deref().unwrap_or_default();
        let fragments = flow.add_text(text, style, font);
        layout_box.rect = bounds_for_fragments(&fragments);
        layout_box.text_fragments = fragments;
        return;
    }

    let child_start = flow.fragments.len();
    for child in &mut layout_box.children {
        layout_inline(child, flow, style, font);
    }
    layout_box.rect = bounds_for_fragments(&flow.fragments[child_start..]);
}

fn bounds_for_fragments(fragments: &[TextFragment]) -> Rect {
    let Some(first) = fragments.first() else {
        return Rect::default();
    };
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x + first.width;
    let mut bottom = first.y + first.height;

    for fragment in &fragments[1..] {
        left = left.min(fragment.x);
        top = top.min(fragment.y);
        right = right.max(fragment.x + fragment.width);
        bottom = bottom.max(fragment.y + fragment.height);
    }

    Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

struct InlineFlow {
    start_x: f32,
    start_y: f32,
    y: f32,
    current_x: f32,
    width: f32,
    base_line_height: f32,
    line_height: f32,
    line_has_content: bool,
    has_line: bool,
    pending_space: bool,
    fragments: Vec<TextFragment>,
}

impl InlineFlow {
    fn new(x: f32, y: f32, width: f32, font_size: f32) -> Self {
        let line_height = font_size * 1.25;
        Self {
            start_x: x,
            start_y: y,
            y,
            current_x: x,
            width,
            base_line_height: line_height,
            line_height,
            line_has_content: false,
            has_line: false,
            pending_space: false,
            fragments: Vec::new(),
        }
    }

    fn add_text(
        &mut self,
        text: &str,
        style: TextStyle,
        font: &fontdue::Font,
    ) -> Vec<TextFragment> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            self.pending_space |= text.chars().any(char::is_whitespace);
            return Vec::new();
        }

        let mut fragments = Vec::with_capacity(words.len());
        let starts_with_space = text.chars().next().is_some_and(char::is_whitespace);
        let ends_with_space = text.chars().next_back().is_some_and(char::is_whitespace);

        for (index, word) in words.iter().enumerate() {
            if index > 0 || (index == 0 && starts_with_space) {
                self.pending_space = true;
            }
            let fragment = self.add_word(word, style, font);
            self.fragments.push(fragment.clone());
            fragments.push(fragment);
            self.pending_space = index + 1 < words.len() || ends_with_space;
        }

        fragments
    }

    fn add_word(&mut self, word: &str, style: TextStyle, font: &fontdue::Font) -> TextFragment {
        let word_width = measure_text(font, word, style.font_size);
        let space_width = font.metrics(' ', style.font_size).advance_width;
        let separator_width = if self.pending_space && self.line_has_content {
            space_width
        } else {
            0.0
        };

        if self.line_has_content
            && self.current_x + separator_width + word_width > self.start_x + self.width
        {
            self.next_line();
        } else {
            self.current_x += separator_width;
        }

        self.line_height = self.line_height.max(style.font_size * 1.25);
        let fragment = TextFragment {
            x: self.current_x,
            y: self.y,
            width: word_width,
            height: style.font_size * 1.25,
            text: word.to_string(),
            font_size: style.font_size,
            bold: style.bold,
            italic: style.italic,
        };
        self.current_x += word_width;
        self.line_has_content = true;
        self.has_line = true;
        self.pending_space = false;
        fragment
    }

    fn break_line(&mut self) -> Rect {
        let rect = Rect {
            x: self.current_x,
            y: self.y,
            width: 1.0,
            height: self.line_height,
        };
        self.next_line();
        rect
    }

    fn next_line(&mut self) {
        self.y += self.line_height;
        self.current_x = self.start_x;
        self.line_height = self.base_line_height;
        self.line_has_content = false;
        self.has_line = true;
        self.pending_space = false;
    }

    fn content_height(&self) -> f32 {
        if self.has_line {
            self.y + self.line_height - self.start_y
        } else {
            0.0
        }
    }
}

fn measure_text(font: &fontdue::Font, text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|character| font.metrics(character, font_size).advance_width)
        .sum()
}

fn matches_tag(tag_name: &str, tags: &[&str]) -> bool {
    tags.iter().any(|tag| tag_name.eq_ignore_ascii_case(tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{html::parser, paint::load_bundled_font};

    fn layout(source: &str, width: f32) -> LayoutTree {
        let document = parser::parse(source).unwrap();
        let mut tree = build_layout_tree(&document);
        tree.layout(width, &load_bundled_font().unwrap());
        tree
    }

    fn find_box<'a>(layout_box: &'a LayoutBox, tag_name: &str) -> Option<&'a LayoutBox> {
        if layout_box.tag_name() == tag_name {
            return Some(layout_box);
        }
        layout_box
            .children()
            .iter()
            .find_map(|child| find_box(child, tag_name))
    }

    #[test]
    fn builds_block_and_inline_boxes_and_skips_non_rendered_elements() {
        let document = parser::parse(
            "<head><title>hidden</title></head><div>shown <em>emphasis</em></div>\
             <script>script</script><style>style</style>",
        )
        .unwrap();
        let tree = build_layout_tree(&document);
        let div = find_box(tree.root(), "<div>").unwrap();

        assert_eq!(div.kind(), BoxKind::Block);
        assert_eq!(div.children()[0].kind(), BoxKind::Text);
        assert_eq!(div.children()[1].kind(), BoxKind::Inline);
        assert_eq!(div.children()[1].tag_name(), "<em>");
        assert!(find_box(tree.root(), "<head>").is_none());
        assert!(find_box(tree.root(), "<script>").is_none());
        assert!(find_box(tree.root(), "<style>").is_none());
    }

    #[test]
    fn stacks_blocks_at_parent_width_and_applies_paragraph_and_heading_spacing() {
        let tree = layout(
            "<div><h1>Heading</h1><p>First</p><p>Second</p></div>",
            400.0,
        );
        let heading = find_box(tree.root(), "<h1>").unwrap();
        let div = find_box(tree.root(), "<div>").unwrap();
        let paragraphs = div
            .children()
            .iter()
            .filter(|child| child.tag_name() == "<p>")
            .collect::<Vec<_>>();
        let first = paragraphs[0];
        let second = paragraphs[1];
        let heading_text = find_box(heading, "#text").unwrap();

        assert!((heading.rect().width - 352.0).abs() < 0.01);
        assert_eq!(heading_text.text_fragments()[0].font_size, 32.0);
        assert!(heading_text.text_fragments()[0].bold);
        assert!(first.rect().y > heading.rect().y + heading.rect().height);
        assert!(tree.root().rect().height > second.rect().y);
    }

    #[test]
    fn classifies_common_structural_elements_as_blocks() {
        for tag in ["body", "section", "main", "ul", "li", "h6", "p"] {
            assert_eq!(classify(tag), BoxKind::Block, "{tag} should be block");
        }
        for tag in ["a", "span", "strong", "em", "br"] {
            assert_eq!(classify(tag), BoxKind::Inline, "{tag} should be inline");
        }
    }

    #[test]
    fn inline_flow_wraps_words_and_respects_explicit_line_breaks() {
        let tree = layout("<p>alpha beta<br>gamma</p>", 110.0);
        let paragraph = find_box(tree.root(), "<p>").unwrap();
        let text_boxes: Vec<&LayoutBox> = paragraph
            .children()
            .iter()
            .filter(|child| child.kind() == BoxKind::Text)
            .collect();
        let first_line_y = text_boxes[0].text_fragments()[0].y;
        let last_line_y = text_boxes[1]
            .text_fragments()
            .last()
            .expect("the final text node should contain a fragment")
            .y;

        assert!(last_line_y > first_line_y);
        assert!(paragraph.rect().height >= last_line_y - paragraph.rect().y);
    }

    #[test]
    fn emphasis_styles_are_inherited_by_nested_text() {
        let tree = layout("<p><strong>bold <em>bold italic</em></strong></p>", 400.0);
        let strong = find_box(tree.root(), "<strong>").unwrap();
        let em = find_box(strong, "<em>").unwrap();
        let bold = find_box(strong, "#text").unwrap();
        let bold_italic = find_box(em, "#text").unwrap();

        assert!(bold.text_fragments()[0].bold);
        assert!(!bold.text_fragments()[0].italic);
        assert!(bold_italic.text_fragments()[0].bold);
        assert!(bold_italic.text_fragments()[0].italic);
    }

    #[test]
    fn list_items_receive_bullets_and_ordered_numbers() {
        let tree = layout(
            "<ul><li>one</li></ul><ol><li>first</li><li>second</li></ol>",
            400.0,
        );
        fn collect_list_items<'a>(layout_box: &'a LayoutBox, items: &mut Vec<&'a LayoutBox>) {
            if layout_box.tag_name() == "<li>" {
                items.push(layout_box);
            }
            for child in layout_box.children() {
                collect_list_items(child, items);
            }
        }

        let mut list_items = Vec::new();
        collect_list_items(tree.root(), &mut list_items);

        assert_eq!(list_items.len(), 3);
        assert_eq!(list_items[0].list_marker().unwrap().text, "\u{2022}");
        assert_eq!(list_items[1].list_marker().unwrap().text, "1.");
        assert_eq!(list_items[2].list_marker().unwrap().text, "2.");
        assert!(list_items[0].rect().x > tree.root().rect().x);
    }
}
