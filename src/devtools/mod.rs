use crate::dom::{Document, NodeId, NodeKind};
use crate::layout::{BoxKind, LayoutBox, LayoutTree};
use crate::paint::DisplayItem;

pub fn dump_stylesheets(stylesheets: &[crate::style::AuthorStylesheet]) {
    print!("{}", format_stylesheets(stylesheets));
}

pub fn format_stylesheets(stylesheets: &[crate::style::AuthorStylesheet]) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    for (index, sheet) in stylesheets.iter().enumerate() {
        writeln!(output, "Stylesheet {} (<style>)", index + 1)
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
    dump_node(document, document.root_id(), 0);
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

fn dump_node(document: &Document, id: NodeId, depth: usize) {
    let node = document
        .node(id)
        .expect("DOM child ID must refer to an existing node");
    let indent = "  ".repeat(depth);

    match node.kind() {
        NodeKind::Document => println!("{indent}#document"),
        NodeKind::Element(element) => {
            print!("{indent}<{}", element.tag_name());

            for attribute in element.attributes() {
                print!(" {}={:?}", attribute.name(), attribute.value());
            }

            println!(">");
        }
        NodeKind::Text(text) => println!("{indent}{text:?}"),
    }

    for child_id in node.children() {
        dump_node(document, *child_id, depth + 1);
    }
}
