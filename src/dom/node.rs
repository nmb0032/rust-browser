/*!
 * This module defines the structure and behavior of a Node within the Rust Browser's DOM.
 * Each node has a kind, optional parent, and a list of children.
 */

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

#[derive(Debug)]
pub struct Node {
    kind: NodeKind,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
}

#[derive(Debug)]
pub enum NodeKind {
    Document,
    Element(ElementData),
    Text(String),
}

#[derive(Debug)]
pub struct ElementData {
    tag_name: String,
    attributes: Vec<Attribute>,
}

#[derive(Debug)]
pub struct Attribute {
    name: String,
    value: String,
}

impl NodeId {
    pub(super) fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(super) fn index(self) -> usize {
        self.0
    }
}

impl Node {
    pub(super) fn new(kind: NodeKind, parent: Option<NodeId>) -> Self {
        Self {
            kind,
            parent,
            children: Vec::new(),
        }
    }

    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    pub(super) fn add_child(&mut self, child: NodeId) {
        self.children.push(child);
    }

    pub fn as_element(&self) -> Option<&ElementData> {
        match &self.kind {
            NodeKind::Element(data) => Some(data),
            NodeKind::Document | NodeKind::Text(_) => None,
        }
    }
}

impl ElementData {
    pub fn new(tag_name: String, attributes: Vec<Attribute>) -> Self {
        Self {
            tag_name,
            attributes,
        }
    }

    pub fn tag_name(&self) -> &str {
        &self.tag_name
    }

    pub fn attributes(&self) -> &[Attribute] {
        &self.attributes
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attr| attr.name.eq_ignore_ascii_case(name))
            .map(|attr| attr.value.as_str())
    }
}

impl Attribute {
    pub fn new(name: String, value: String) -> Self {
        Self { name, value }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn node_ids_are_copyable_and_usable_as_hash_keys() {
        let first = NodeId::from_index(3);
        let second = NodeId::from_index(3);
        let copied = first;

        assert_eq!(first, second);
        assert_eq!(first, copied);

        let mut ids = HashSet::new();
        ids.insert(first);
        assert!(ids.contains(&second));
    }

    #[test]
    fn element_data_looks_up_attributes_case_insensitively() {
        let element = ElementData::new(
            "a".to_string(),
            vec![Attribute::new("HREF".to_string(), "/next".to_string())],
        );

        assert_eq!(element.tag_name(), "a");
        assert_eq!(element.attribute("href"), Some("/next"));
        assert_eq!(element.attribute("class"), None);
    }

    #[test]
    fn as_element_returns_data_only_for_element_nodes() {
        let element = Node::new(
            NodeKind::Element(ElementData::new("p".to_string(), Vec::new())),
            None,
        );
        let text = Node::new(NodeKind::Text("content".to_string()), None);
        let document = Node::new(NodeKind::Document, None);

        assert_eq!(element.as_element().unwrap().tag_name(), "p");
        assert!(text.as_element().is_none());
        assert!(document.as_element().is_none());
    }

    #[test]
    fn node_exposes_parent_and_ordered_children() {
        let parent = NodeId::from_index(1);
        let first_child = NodeId::from_index(2);
        let second_child = NodeId::from_index(3);
        let mut node = Node::new(
            NodeKind::Element(ElementData::new("div".to_string(), Vec::new())),
            Some(parent),
        );

        node.add_child(first_child);
        node.add_child(second_child);

        assert_eq!(node.parent(), Some(parent));
        assert_eq!(node.children(), &[first_child, second_child]);
    }
}
