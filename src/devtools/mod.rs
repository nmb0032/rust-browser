use crate::dom::{Document, NodeKind};
use crate::layout::{BoxKind, LayoutBox, LayoutTree};
use crate::paint::DisplayItem;

fn flex_summary(style: &crate::style::ComputedStyle) -> String {
    format!(
        "display={:?} flex-direction={:?} flex-wrap={:?} flex-grow={} flex-shrink={} flex-basis={:?} order={} justify-content={:?} align-items={:?} align-self={:?} align-content={:?} gap={:?}/{:?} min-size={:?}/{:?} max-size={:?}/{:?}",
        style.display,
        style.flex_direction,
        style.flex_wrap,
        style.flex_grow,
        style.flex_shrink,
        style.flex_basis,
        style.order,
        style.justify_content,
        style.align_items,
        style.align_self,
        style.align_content,
        style.row_gap,
        style.column_gap,
        style.min_width,
        style.min_height,
        style.max_width,
        style.max_height
    )
}

/// Explicit length stages, with used/actual geometry from the laid-out tree.
/// "Actual" here is unclipped rectangle quantization, not glyph rasterization.
pub fn format_values(tree: &LayoutTree, device_scale: f32) -> String {
    use std::fmt::Write;
    fn walk(tree: &LayoutTree, layout_box: &LayoutBox, scale: f32, output: &mut String) {
        if let Some(node) = layout_box.dom_node_id() {
            writeln!(output, "Node {node:?} {}", layout_box.tag_name()).unwrap();
        } else {
            writeln!(output, "Anonymous {}", layout_box.tag_name()).unwrap();
        }
        for (property, value) in &layout_box.style().specified_lengths {
            writeln!(output, "  specified {property}: {value:?}").unwrap();
        }
        for (property, value) in &layout_box.style().specified_margins {
            if *value == crate::css::values::LengthOrAuto::Auto {
                writeln!(output, "  specified {property}: Auto").unwrap();
            }
        }
        for (property, value) in &layout_box.style().specified_colors {
            writeln!(output, "  specified {property}: {value:?}").unwrap();
        }
        writeln!(
            output,
            "  computed color: {:?}",
            layout_box.style().text_color
        )
        .unwrap();
        writeln!(
            output,
            "  computed -webkit-text-fill-color: {:?}",
            layout_box.style().text_fill_color
        )
        .unwrap();
        writeln!(
            output,
            "  computed box-sizing: {:?}",
            layout_box.style().box_sizing
        )
        .unwrap();
        writeln!(output, "  computed height: {:?}", layout_box.style().height).unwrap();
        writeln!(
            output,
            "  computed background-color: {:?}",
            layout_box.style().background
        )
        .unwrap();
        writeln!(output, "  computed width: {:?}", layout_box.style().width).unwrap();
        if layout_box.style().display.is_flex() || layout_box.is_flex_item() {
            writeln!(
                output,
                "  computed flex: {}",
                flex_summary(layout_box.style())
            )
            .unwrap();
            writeln!(output, "  used flex-item: {}", layout_box.is_flex_item()).unwrap();
        }
        writeln!(
            output,
            "  computed position: {:?} inset={:?} z-index={:?}",
            layout_box.style().position,
            layout_box.style().inset,
            layout_box.style().z_index
        )
        .unwrap();
        writeln!(
            output,
            "  used coordinate-space: {:?}",
            layout_box.coordinate_space()
        )
        .unwrap();
        writeln!(
            output,
            "  computed overflow: {:?} {:?}",
            layout_box.style().overflow_x,
            layout_box.style().overflow_y
        )
        .unwrap();
        if let Some(container) = tree.box_scroll_container(layout_box) {
            writeln!(output, "  used scroll-port: {:?}", container.port).unwrap();
            writeln!(output, "  used scroll-extent: {:?}", container.extent).unwrap();
            writeln!(
                output,
                "  used scroll-range: {:?} offset={:?}",
                container.range,
                tree.box_scroll_offset(layout_box)
            )
            .unwrap();
            if let Some(node) = layout_box.dom_node_id()
                && layout_box.pseudo_element().is_none()
            {
                writeln!(
                    output,
                    "  visual clipped-border: {:?}",
                    tree.visual_node_rect(node, tree.viewport_scroll().y)
                )
                .unwrap();
            }
        }
        writeln!(
            output,
            "  computed direction: {:?}",
            layout_box.style().direction
        )
        .unwrap();
        writeln!(
            output,
            "  computed text-align: {:?}",
            layout_box.style().text_align
        )
        .unwrap();
        writeln!(
            output,
            "  computed text-align-last: {:?}",
            layout_box.style().text_align_last
        )
        .unwrap();
        writeln!(
            output,
            "  computed white-space-collapse: {:?}",
            layout_box.style().white_space_collapse
        )
        .unwrap();
        writeln!(
            output,
            "  computed text-wrap-mode: {:?}",
            layout_box.style().text_wrap_mode
        )
        .unwrap();
        writeln!(
            output,
            "  computed vertical-align: {:?}",
            layout_box.style().vertical_align
        )
        .unwrap();
        writeln!(
            output,
            "  computed baseline-shift: {:?}",
            layout_box.style().baseline_shift
        )
        .unwrap();
        writeln!(
            output,
            "  computed baseline-source: {:?}",
            layout_box.style().baseline_source
        )
        .unwrap();
        for line in layout_box.line_boxes() {
            writeln!(output, "  used line: {line:?}").unwrap();
        }
        for fragment in layout_box.inline_fragments() {
            writeln!(output, "  used inline-fragment: {fragment:?}").unwrap();
        }
        for fragment in layout_box.text_fragments() {
            writeln!(output, "  used shaped-glyphs: {:?}", fragment.glyphs).unwrap();
            writeln!(
                output,
                "  used text: {:?} baseline={} ink={:?}",
                fragment.text, fragment.baseline, fragment.ink_rect
            )
            .unwrap();
        }
        writeln!(
            output,
            "  computed margins: {:?}",
            layout_box.style().margin
        )
        .unwrap();
        writeln!(output, "  used margins: {:?}", layout_box.used_margins()).unwrap();
        writeln!(
            output,
            "  computed font-size: Px({:?})",
            layout_box.style().font_size
        )
        .unwrap();
        writeln!(
            output,
            "  computed line-height: {:?}",
            layout_box.style().line_height
        )
        .unwrap();
        writeln!(
            output,
            "  used content-width: {:?}",
            layout_box.used_width()
        )
        .unwrap();
        writeln!(
            output,
            "  used content-height: {:?}",
            crate::css::values::UsedLength(layout_box.content_rect().height)
        )
        .unwrap();
        writeln!(
            output,
            "  used content-rect: {:?}",
            layout_box.content_rect()
        )
        .unwrap();
        writeln!(
            output,
            "  actual border-rect @ {scale}x: {:?}",
            layout_box.actual_rect(scale)
        )
        .unwrap();
        for child in layout_box.children() {
            walk(tree, child, scale, output);
        }
    }
    let mut output = String::new();
    writeln!(
        output,
        "Viewport overflow: {:?} offset={:?}",
        tree.viewport_scroll_container(),
        tree.viewport_scroll()
    )
    .unwrap();
    walk(tree, tree.root(), device_scale, &mut output);
    output
}

/// Typed computed values only: percentages in box properties remain unresolved.
pub fn format_style(document: &Document, styles: &crate::style::ComputedStyles) -> String {
    use crate::css::values::Color;
    use std::fmt::Write;

    let mut output = String::new();
    for id in document.preorder() {
        let Some(element) = document.node(id).and_then(|node| node.as_element()) else {
            continue;
        };
        let Some(style) = styles.get(id) else {
            continue;
        };
        writeln!(
            output,
            "Element {id:?} <{}> id={:?}",
            element.tag_name(),
            element.attribute("id")
        )
        .expect("writing a string cannot fail");
        let color = |color: crate::css::values::ResolvedColor| {
            if color == crate::css::values::ResolvedColor::TRANSPARENT {
                Color::Transparent
            } else if let Some(rgb) = color.opaque_rgb() {
                Color::Rgb(rgb)
            } else {
                Color::Absolute(color)
            }
        };
        writeln!(output, "  display: {:?}", style.display).unwrap();
        if style.display.is_flex() {
            writeln!(output, "  flex: {}", flex_summary(style)).unwrap();
        }
        writeln!(output, "  overflow-x: {:?}", style.overflow_x).unwrap();
        writeln!(output, "  overflow-y: {:?}", style.overflow_y).unwrap();
        writeln!(
            output,
            "  position: {:?} inset={:?} z-index={:?}",
            style.position, style.inset, style.z_index
        )
        .unwrap();
        writeln!(output, "  direction: {:?}", style.direction).unwrap();
        writeln!(output, "  text-align: {:?}", style.text_align).unwrap();
        writeln!(output, "  text-align-last: {:?}", style.text_align_last).unwrap();
        writeln!(
            output,
            "  white-space-collapse: {:?}",
            style.white_space_collapse
        )
        .unwrap();
        writeln!(output, "  text-wrap-mode: {:?}", style.text_wrap_mode).unwrap();
        writeln!(output, "  vertical-align: {:?}", style.vertical_align).unwrap();
        writeln!(output, "  baseline-shift: {:?}", style.baseline_shift).unwrap();
        writeln!(output, "  baseline-source: {:?}", style.baseline_source).unwrap();
        if style.box_sizing != crate::style::BoxSizing::ContentBox {
            writeln!(output, "  box-sizing: {:?}", style.box_sizing).unwrap();
        }
        if style.container_type != crate::css::container::ContainerType::Normal
            || !style.container_names.is_empty()
        {
            writeln!(output, "  container-type: {:?}", style.container_type).unwrap();
            writeln!(output, "  container-name: {:?}", style.container_names).unwrap();
        }
        writeln!(output, "  color: {:?}", color(style.text_color)).unwrap();
        if style.text_fill_color != style.text_color {
            writeln!(
                output,
                "  -webkit-text-fill-color: {:?}",
                color(style.text_fill_color)
            )
            .unwrap();
        }
        writeln!(output, "  background-color: {:?}", color(style.background)).unwrap();
        writeln!(output, "  background-image: {:?}", style.background_image).unwrap();
        for (name, value) in &style.custom_properties {
            let preview_name: String = name.chars().take(80).collect();
            let name = if name.chars().count() > 80 {
                format!("{preview_name}…")
            } else {
                preview_name
            };
            match value {
                Some(tokens) => {
                    // Structured tokens make preserved boundaries and empty
                    // values visible, without ambiguous CSS serialization.
                    let preview = format!("{:?}", &tokens[..tokens.len().min(64)]);
                    let suffix = if preview.chars().count() > 1024 {
                        "… (preview truncated)"
                    } else {
                        ""
                    };
                    let preview: String = preview.chars().take(1024).collect();
                    writeln!(output, "  {name}: Tokens({preview}{suffix})").unwrap();
                    if tokens.len() > 64 {
                        writeln!(
                            output,
                            "    ({} additional tokens omitted)",
                            tokens.len() - 64
                        )
                        .unwrap();
                        if let Some(height) = &style.height {
                            writeln!(output, "  height: {height:?}").unwrap();
                        }
                    }
                }
                None => writeln!(output, "  {name}: GuaranteedInvalid").unwrap(),
            }
        }
        writeln!(output, "  font-size: Px({:?})", style.font_size).unwrap();
        writeln!(output, "  font-family: {:?}", style.font_family).unwrap();
        writeln!(output, "  font-weight: {:?}", style.font_weight).unwrap();
        writeln!(output, "  font-style: {:?}", style.font_style).unwrap();
        writeln!(output, "  line-height: {:?}", style.line_height).unwrap();
        match &style.width {
            Some(width) => writeln!(output, "  width: {width:?}"),
            None => writeln!(output, "  width: Auto"),
        }
        .unwrap();
        for (name, margin, padding, width, border_style, border_color) in [
            (
                "top",
                &style.margin.top,
                &style.padding.top,
                style.border_width.top,
                style.border_style.top,
                style.borders.top,
            ),
            (
                "right",
                &style.margin.right,
                &style.padding.right,
                style.border_width.right,
                style.border_style.right,
                style.borders.right,
            ),
            (
                "bottom",
                &style.margin.bottom,
                &style.padding.bottom,
                style.border_width.bottom,
                style.border_style.bottom,
                style.borders.bottom,
            ),
            (
                "left",
                &style.margin.left,
                &style.padding.left,
                style.border_width.left,
                style.border_style.left,
                style.borders.left,
            ),
        ] {
            match margin {
                crate::css::values::LengthOrAuto::Length(length) => {
                    writeln!(output, "  margin-{name}: {length:?}").unwrap();
                }
                crate::css::values::LengthOrAuto::Auto => {
                    writeln!(output, "  margin-{name}: Auto").unwrap();
                }
            }
            writeln!(output, "  padding-{name}: {padding:?}").unwrap();
            writeln!(output, "  border-{name}-width: Px({width:?})").unwrap();
            writeln!(output, "  border-{name}-style: {border_style:?}").unwrap();
            writeln!(output, "  border-{name}-color: {:?}", color(border_color)).unwrap();
        }
        for pseudo in crate::style::PseudoElement::ALL {
            if let Some(style) = styles.pseudo(id, pseudo) {
                writeln!(output, "  ::{} computed (separate target)", pseudo.name()).unwrap();
                writeln!(output, "    display: {:?}", style.display).unwrap();
                writeln!(
                    output,
                    "    position: {:?} inset={:?} z-index={:?}",
                    style.position, style.inset, style.z_index
                )
                .unwrap();
                writeln!(output, "    color: {:?}", color(style.text_color)).unwrap();
                writeln!(
                    output,
                    "    background-color: {:?}",
                    color(style.background)
                )
                .unwrap();
                writeln!(output, "    font-size: Px({:?})", style.font_size).unwrap();
                writeln!(output, "    line-height: {:?}", style.line_height).unwrap();
                writeln!(output, "    text-align: {:?}", style.text_align).unwrap();
                writeln!(
                    output,
                    "    white-space-collapse: {:?}",
                    style.white_space_collapse
                )
                .unwrap();
                writeln!(output, "    text-wrap-mode: {:?}", style.text_wrap_mode).unwrap();
                writeln!(output, "    vertical-align: {:?}", style.vertical_align).unwrap();
                writeln!(output, "    baseline-shift: {:?}", style.baseline_shift).unwrap();
                writeln!(output, "    baseline-source: {:?}", style.baseline_source).unwrap();
                writeln!(output, "    width: {:?}", style.width).unwrap();
                let content = format!("{:?}", style.content);
                writeln!(
                    output,
                    "    content: {}",
                    content.chars().take(1024).collect::<String>()
                )
                .unwrap();
            }
        }
    }
    output
}

pub fn format_css_tokens(source: &str) -> String {
    use crate::css::tokenizer::Tokenizer;
    use std::fmt::Write;

    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let mut output = String::new();
    while let Some(token) = tokenizer.next_token() {
        writeln!(
            output,
            "  {}:{} (bytes {}..{}): {:?}",
            token.span.start.line,
            token.span.start.column,
            token.span.start.byte_offset,
            token.span.end.byte_offset,
            token.kind
        )
        .expect("writing a string cannot fail");
    }
    for error in tokenizer.errors() {
        writeln!(
            output,
            "  error {}:{} (byte {}): {}",
            error.position.line, error.position.column, error.position.byte_offset, error.code
        )
        .expect("writing a string cannot fail");
    }
    if tokenizer.dropped_errors() > 0 {
        writeln!(
            output,
            "  {} additional CSS tokenization errors omitted",
            tokenizer.dropped_errors()
        )
        .expect("writing a string cannot fail");
    }
    output
}

pub fn format_document_css_tokens(frame: &crate::engine::Frame) -> String {
    use std::fmt::Write;

    let mut output = String::new();
    for node_id in frame.document.preorder() {
        if let Some(sheet) = frame.external_stylesheet_sources.get(&node_id) {
            writeln!(output, "Stylesheet ({}, node {node_id:?})", sheet.url)
                .expect("writing a string cannot fail");
            output.push_str(&format_css_tokens(&sheet.source));
        }
        if let Some(source) = crate::style::embedded_stylesheet_source(&frame.document, node_id) {
            writeln!(output, "Stylesheet (<style>, node {node_id:?})")
                .expect("writing a string cannot fail");
            output.push_str(&format_css_tokens(&source));
        }
        if let Some(source) = frame
            .document
            .node(node_id)
            .and_then(|node| node.as_element())
            .and_then(|element| element.attribute("style"))
        {
            writeln!(output, "Inline style (node {node_id:?})")
                .expect("writing a string cannot fail");
            output.push_str(&format_css_tokens(source));
        }
    }
    output
}

pub fn dump_stylesheets(stylesheets: &[crate::style::AuthorStylesheet]) {
    print!("{}", format_stylesheets(stylesheets));
}

pub fn format_stylesheets(stylesheets: &[crate::style::AuthorStylesheet]) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    for (index, sheet) in stylesheets.iter().enumerate() {
        let source = sheet.url.as_ref().map_or("<style>", |url| url.as_str());
        writeln!(output, "Stylesheet {} ({source})", index + 1)
            .expect("writing a string cannot fail");
        for rule in sheet.stylesheet.rules() {
            format_rule(rule, 1, &mut output);
        }
    }
    output
}

fn format_rule(rule: &crate::css::Rule, depth: usize, output: &mut String) {
    use crate::css::{BlockItem, Rule};
    use std::fmt::Write;
    let indent = "  ".repeat(depth);
    let items = match rule {
        Rule::Qualified(rule) => {
            writeln!(output, "{indent}{} {{", rule.prelude())
                .expect("writing a string cannot fail");
            rule.items()
        }
        Rule::At(rule) => {
            let prelude = if rule.prelude().is_empty() {
                String::new()
            } else {
                format!(" {}", rule.prelude())
            };
            if let Some(items) = rule.block() {
                writeln!(output, "{indent}@{}{prelude} {{", rule.name())
                    .expect("writing a string cannot fail");
                items
            } else {
                writeln!(output, "{indent}@{}{prelude};", rule.name())
                    .expect("writing a string cannot fail");
                if let Some(imported) = rule.imported() {
                    writeln!(
                        output,
                        "{indent}  Imported stylesheet ({}) {{",
                        imported.url
                    )
                    .expect("writing a string");
                    for rule in imported.stylesheet.rules() {
                        format_rule(rule, depth + 2, output);
                    }
                    writeln!(output, "{indent}  }}").expect("writing a string");
                }
                return;
            }
        }
    };
    for item in items {
        match item {
            BlockItem::Declaration(declaration) => {
                let important = if declaration.is_important() {
                    " !important"
                } else {
                    ""
                };
                writeln!(
                    output,
                    "{indent}  {}: {}{important};",
                    declaration.property(),
                    declaration.value()
                )
                .expect("writing a string cannot fail");
            }
            BlockItem::Rule(rule) => format_rule(rule, depth + 1, output),
        }
    }
    writeln!(output, "{indent}}}").expect("writing a string cannot fail");
}

pub fn dump_dom(document: &Document) {
    print!("{}", format_dom(document));
}

pub fn format_dom(document: &Document) -> String {
    use crate::dom::Namespace;
    use std::fmt::Write;
    let mut output = String::new();
    let mut pending = vec![(document.root_id(), 0)];
    while let Some((id, depth)) = pending.pop() {
        let node = document.node(id).expect("valid DOM arena link");
        let indent = "  ".repeat(depth);
        match node.kind() {
            NodeKind::Document => writeln!(output, "{indent}#document"),
            NodeKind::DocumentFragment => writeln!(output, "{indent}#document-fragment"),
            NodeKind::Element(element) => {
                let namespace = match element.namespace() {
                    Namespace::Html => String::new(),
                    Namespace::Svg => "svg ".into(),
                    Namespace::MathMl => "math ".into(),
                    Namespace::None => "null ".into(),
                    Namespace::Other(uri) => format!("{{{uri}}} "),
                };
                let prefix = element
                    .prefix()
                    .map_or(String::new(), |prefix| format!("{prefix}:"));
                write!(output, "{indent}<{namespace}{prefix}{}", element.tag_name())
                    .expect("writing a string");
                for attribute in element.attributes() {
                    let prefix = attribute
                        .prefix()
                        .map_or(String::new(), |prefix| format!("{prefix}:"));
                    let namespace = attribute
                        .namespace()
                        .map_or(String::new(), |uri| format!("{{{uri}}}"));
                    write!(
                        output,
                        " {namespace}{prefix}{}={:?}",
                        attribute.name(),
                        attribute.value()
                    )
                    .expect("writing a string");
                }
                if let Some(contents) = element.template_contents() {
                    pending.push((contents, depth + 1));
                }
                writeln!(output, ">")
            }
            NodeKind::Text(text) => writeln!(output, "{indent}{text:?}"),
            NodeKind::Comment(text) => writeln!(output, "{indent}<!--{text}-->"),
            NodeKind::Doctype(data) => writeln!(
                output,
                "{indent}<!DOCTYPE {} {:?} {:?}>",
                data.name, data.public_identifier, data.system_identifier
            ),
            NodeKind::ProcessingInstruction { target, data } => {
                writeln!(output, "{indent}<?{target} {data}?>")
            }
        }
        .expect("writing a string");
        pending.extend(node.children().iter().rev().map(|id| (*id, depth + 1)));
    }
    output
}

pub fn dump_display_list(display_list: &[DisplayItem]) {
    println!("{display_list:#?}");
}

pub fn dump_layout(layout: &LayoutTree) {
    println!(
        "Viewport overflow: {:?} offset={:?}",
        layout.viewport_scroll_container(),
        layout.viewport_scroll()
    );
    dump_layout_box(layout, layout.root(), 0);
}

fn dump_layout_box(tree: &LayoutTree, layout_box: &LayoutBox, depth: usize) {
    if layout_box.kind() == BoxKind::Text
        && layout_box.text().is_some_and(|text| text.trim().is_empty())
    {
        return;
    }

    let rect = layout_box.rect();
    let indent = "  ".repeat(depth);
    let contents = layout_box
        .text()
        .map(|text| format!(" {text:?}"))
        .unwrap_or_default();
    println!(
        "{indent}{} {:?} x={:.1} y={:.1} width={:.1} height={:.1}{contents}",
        layout_box.tag_name(),
        layout_box.kind(),
        rect.x,
        rect.y,
        rect.width,
        rect.height
    );
    println!(
        "{indent}  position={:?} inset={:?} z-index={:?} coordinate-space={:?}",
        layout_box.style().position,
        layout_box.style().inset,
        layout_box.style().z_index,
        layout_box.coordinate_space()
    );
    println!(
        "{indent}  overflow-x={:?} overflow-y={:?}",
        layout_box.style().overflow_x,
        layout_box.style().overflow_y
    );
    if layout_box.style().display.is_flex() || layout_box.is_flex_item() {
        println!(
            "{indent}  flex-item={} {}",
            layout_box.is_flex_item(),
            flex_summary(layout_box.style())
        );
    }
    if let Some(container) = tree.box_scroll_container(layout_box) {
        println!(
            "{indent}  clip-port={:?} scroll-extent={:?} scroll-range={:?} scroll-offset={:?}",
            container.port,
            container.extent,
            container.range,
            tree.box_scroll_offset(layout_box)
        );
    }
    for line in layout_box.line_boxes() {
        println!(
            "{indent}  line x={:.3} y={:.3} width={:.3} height={:.3} baseline={:.3} ascent={:.3} descent={:.3} content-x={:.3} content-width={:.3}",
            line.rect.x,
            line.rect.y,
            line.rect.width,
            line.rect.height,
            line.baseline,
            line.ascent,
            line.descent,
            line.content_rect.x,
            line.content_rect.width
        );
    }
    for fragment in layout_box.inline_fragments() {
        println!(
            "{indent}  inline-fragment {:?} baseline={:.3} first={} last={}",
            fragment.rect, fragment.baseline, fragment.first, fragment.last
        );
    }
    for fragment in layout_box.text_fragments() {
        println!(
            "{indent}  text {:?} baseline={:.3} ink={:?}",
            fragment.text, fragment.baseline, fragment.ink_rect
        );
        for glyph in &fragment.glyphs.glyphs {
            println!(
                "{indent}    face={:?}[{}] glyph={} cluster={}..{} advance={:.3} position=({:.3},{:.3}) offset=({:.3},{:.3}) rtl={}",
                glyph.face.name,
                glyph.face.index,
                glyph.id,
                glyph.cluster,
                glyph.cluster_end,
                glyph.advance,
                glyph.x,
                glyph.y,
                glyph.x_offset,
                glyph.y_offset,
                glyph.rtl
            );
        }
    }

    for child in layout_box.children() {
        dump_layout_box(tree, child, depth + 1);
    }
}
