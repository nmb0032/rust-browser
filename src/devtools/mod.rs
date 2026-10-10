use crate::dom::{Document, NodeKind};
use crate::layout::{BoxKind, LayoutBox, LayoutTree};
use crate::paint::DisplayItem;

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
    dump_layout_box(layout.root(), 0);
}

fn dump_layout_box(layout_box: &LayoutBox, depth: usize) {
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

    for child in layout_box.children() {
        dump_layout_box(child, depth + 1);
    }
}
