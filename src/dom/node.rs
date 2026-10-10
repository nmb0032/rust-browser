/*!
 * This module defines the structure and behavior of a Node within the Rust Browser's DOM.
 * Each node has a kind, optional parent, and a list of children.
 */

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

#[derive(Clone, Debug)]
pub struct Node {
    kind: NodeKind,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    template_host: Option<NodeId>,
}

#[derive(Clone, Debug)]
pub enum NodeKind {
    Document,
    DocumentFragment,
    Element(ElementData),
    Text(String),
    Comment(String),
    Doctype(DoctypeData),
    ProcessingInstruction { target: String, data: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DoctypeData {
    pub name: String,
    pub public_identifier: String,
    pub system_identifier: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Namespace {
    #[default]
    Html,
    Svg,
    MathMl,
    None,
    Other(String),
}

impl Namespace {
    pub fn uri(&self) -> &str {
        match self {
            Self::Html => "http://www.w3.org/1999/xhtml",
            Self::Svg => "http://www.w3.org/2000/svg",
            Self::MathMl => "http://www.w3.org/1998/Math/MathML",
            Self::None => "",
            Self::Other(uri) => uri,
        }
    }

    pub fn from_uri(uri: &str) -> Self {
        match uri {
            "" => Self::None,
            "http://www.w3.org/1999/xhtml" => Self::Html,
            "http://www.w3.org/2000/svg" => Self::Svg,
            "http://www.w3.org/1998/Math/MathML" => Self::MathMl,
            _ => Self::Other(uri.to_owned()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ElementData {
    tag_name: String,
    attributes: Vec<Attribute>,
    namespace: Namespace,
    prefix: Option<String>,
    template_contents: Option<NodeId>,
    attribute_case_sensitive: bool,
}

#[derive(Clone, Debug)]
pub struct Attribute {
    name: String,
    value: String,
    namespace: Option<String>,
    prefix: Option<String>,
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
            template_host: None,
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
    pub(super) fn template_host(&self) -> Option<NodeId> {
        self.template_host
    }
    pub(super) fn set_template_host(&mut self, host: NodeId) {
        self.template_host = Some(host);
    }

    pub(super) fn add_child(&mut self, child: NodeId) {
        self.children.push(child);
    }

    pub(super) fn append_text(&mut self, text: &str) -> bool {
        if let NodeKind::Text(contents) = &mut self.kind {
            contents.push_str(text);
            true
        } else {
            false
        }
    }

    pub fn as_element(&self) -> Option<&ElementData> {
        match &self.kind {
            NodeKind::Element(data) => Some(data),
            _ => None,
        }
    }

    pub(super) fn as_element_mut(&mut self) -> Option<&mut ElementData> {
        if let NodeKind::Element(element) = &mut self.kind {
            Some(element)
        } else {
            None
        }
    }

    pub(super) fn children_mut(&mut self) -> &mut Vec<NodeId> {
        &mut self.children
    }

    pub(super) fn set_parent(&mut self, parent: Option<NodeId>) {
        self.parent = parent;
    }
}

impl ElementData {
    pub fn new(tag_name: String, attributes: Vec<Attribute>) -> Self {
        Self {
            tag_name,
            attributes,
            namespace: Namespace::Html,
            prefix: None,
            template_contents: None,
            attribute_case_sensitive: false,
        }
    }

    pub fn namespaced(
        tag_name: String,
        attributes: Vec<Attribute>,
        namespace: Namespace,
        prefix: Option<String>,
    ) -> Self {
        let attribute_case_sensitive = namespace != Namespace::Html;
        Self {
            tag_name,
            attributes,
            namespace,
            prefix,
            template_contents: None,
            attribute_case_sensitive,
        }
    }

    pub fn xml(
        tag_name: String,
        attributes: Vec<Attribute>,
        namespace: Namespace,
        prefix: Option<String>,
    ) -> Self {
        let mut element = Self::namespaced(tag_name, attributes, namespace, prefix);
        element.attribute_case_sensitive = true;
        element
    }

    pub(super) fn clone_without_template_contents(&self) -> Self {
        let mut element = self.clone();
        element.template_contents = None;
        element
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }
    pub fn template_contents(&self) -> Option<NodeId> {
        self.template_contents
    }
    pub(super) fn set_template_contents(&mut self, id: NodeId) {
        self.template_contents = Some(id);
    }
    pub(super) fn merge_attributes(&mut self, attributes: Vec<Attribute>) {
        for attribute in attributes {
            if !self.attributes.iter().any(|existing| {
                existing.name == attribute.name && existing.namespace == attribute.namespace
            }) {
                self.attributes.push(attribute);
            }
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
            .find(|attr| {
                attr.namespace.is_none()
                    && if !self.attribute_case_sensitive {
                        attr.name.eq_ignore_ascii_case(name)
                    } else {
                        attr.name == name
                    }
            })
            .map(|attr| attr.value.as_str())
    }
}

impl Attribute {
    pub fn new(name: String, value: String) -> Self {
        Self {
            name,
            value,
            namespace: None,
            prefix: None,
        }
    }

    pub fn namespaced(
        name: String,
        value: String,
        namespace: String,
        prefix: Option<String>,
    ) -> Self {
        Self {
            name,
            value,
            namespace: Some(namespace),
            prefix,
        }
    }
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
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
