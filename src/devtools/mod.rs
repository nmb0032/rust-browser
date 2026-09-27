use crate::dom::{Document, NodeId, NodeKind};

pub fn dump_dom(document: &Document) {
    dump_node(document, document.root_id(), 0);
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
