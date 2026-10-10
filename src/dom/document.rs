/*!
 * This module defines the structure and behavior of the Document for the Rust Browser's DOM.
 */

use super::node::{ElementData, Node, NodeId, NodeKind};

#[derive(Clone, Debug)]
pub struct Document {
    root: NodeId,
    nodes: Vec<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomError {
    InvalidParent(NodeId),
    TextNodeCannotHaveChildren(NodeId),
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let root = NodeId::from_index(0);
        let root_node = Node::new(NodeKind::Document, None);

        Self {
            root,
            nodes: vec![root_node],
        }
    }

    pub fn root_id(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn preorder(&self) -> impl Iterator<Item = NodeId> + '_ {
        let mut pending = vec![self.root];
        std::iter::from_fn(move || {
            let id = pending.pop()?;
            pending.extend(self.nodes[id.index()].children().iter().rev().copied());
            Some(id)
        })
    }

    pub fn append_element(
        &mut self,
        parent: NodeId,
        element: ElementData,
    ) -> Result<NodeId, DomError> {
        self.append_node(parent, NodeKind::Element(element))
    }

    pub fn append_text(&mut self, parent: NodeId, text: String) -> Result<NodeId, DomError> {
        self.append_node(parent, NodeKind::Text(text))
    }

    pub(crate) fn append_text_coalesced(
        &mut self,
        parent: NodeId,
        text: String,
    ) -> Result<NodeId, DomError> {
        let parent_node = self.node(parent).ok_or(DomError::InvalidParent(parent))?;
        if let Some(last) = parent_node.children().last().copied()
            && self.nodes[last.index()].append_text(&text)
        {
            return Ok(last);
        }
        self.append_text(parent, text)
    }

    fn append_node(&mut self, parent: NodeId, kind: NodeKind) -> Result<NodeId, DomError> {
        let parent_index = parent.index();
        {
            let parent_node = self
                .nodes
                .get(parent_index)
                .ok_or(DomError::InvalidParent(parent))?;
            if matches!(parent_node.kind(), NodeKind::Text(_)) {
                return Err(DomError::TextNodeCannotHaveChildren(parent));
            }
        }

        let child_id = NodeId::from_index(self.nodes.len());
        self.nodes.push(Node::new(kind, Some(parent)));
        self.nodes[parent_index].add_child(child_id);
        Ok(child_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_has_a_document_root() {
        let document = Document::new();
        let root_id = document.root_id();
        let root = document.node(root_id).unwrap();

        assert!(matches!(root.kind(), NodeKind::Document));
        assert_eq!(root.parent(), None);
        assert!(root.children().is_empty());
    }

    #[test]
    fn appending_element_and_text_keeps_tree_links_consistent() {
        let mut document = Document::new();
        let root_id = document.root_id();
        let paragraph_id = document
            .append_element(root_id, ElementData::new("p".to_string(), Vec::new()))
            .unwrap();
        let text_id = document
            .append_text(paragraph_id, "hello".to_string())
            .unwrap();

        assert_eq!(document.node(root_id).unwrap().children(), &[paragraph_id]);
        assert_eq!(document.node(paragraph_id).unwrap().parent(), Some(root_id));
        assert_eq!(document.node(paragraph_id).unwrap().children(), &[text_id]);
        assert_eq!(document.node(text_id).unwrap().parent(), Some(paragraph_id));
        assert!(matches!(
            document.node(paragraph_id).unwrap().kind(),
            NodeKind::Element(element) if element.tag_name() == "p"
        ));
        assert!(matches!(
            document.node(text_id).unwrap().kind(),
            NodeKind::Text(text) if text == "hello"
        ));
    }

    #[test]
    fn appending_to_an_invalid_parent_returns_an_error_without_mutation() {
        let mut document = Document::new();
        let invalid_parent = NodeId::from_index(usize::MAX);
        let original_node_count = document.nodes.len();

        let result = document.append_text(invalid_parent, "orphan".to_string());

        assert_eq!(result, Err(DomError::InvalidParent(invalid_parent)));
        assert_eq!(document.nodes.len(), original_node_count);
    }

    #[test]
    fn text_nodes_cannot_have_children() {
        let mut document = Document::new();
        let text_id = document
            .append_text(document.root_id(), "parent".to_string())
            .unwrap();
        let original_node_count = document.nodes.len();

        let result =
            document.append_element(text_id, ElementData::new("span".to_string(), Vec::new()));

        assert_eq!(result, Err(DomError::TextNodeCannotHaveChildren(text_id)));
        assert_eq!(document.nodes.len(), original_node_count);
        assert!(document.node(text_id).unwrap().children().is_empty());
    }

    #[test]
    fn parser_text_coalescing_keeps_ids_and_parent_links_stable() {
        let mut document = Document::new();
        let root = document.root_id();
        let first = document.append_text_coalesced(root, "one".into()).unwrap();
        assert_eq!(
            document.append_text_coalesced(root, "two".into()).unwrap(),
            first
        );
        assert_eq!(document.node_count(), 2);
        assert_eq!(document.node(first).unwrap().parent(), Some(root));
        assert!(
            matches!(document.node(first).unwrap().kind(), NodeKind::Text(text) if text == "onetwo")
        );
        let element = document
            .append_element(root, ElementData::new("p".into(), Vec::new()))
            .unwrap();
        let second = document
            .append_text_coalesced(root, "three".into())
            .unwrap();
        assert_ne!(second, first);
        assert_eq!(
            document.node(root).unwrap().children(),
            [first, element, second]
        );
        assert_eq!(
            document.append_text_coalesced(first, "invalid".into()),
            Err(DomError::TextNodeCannotHaveChildren(first))
        );
        assert_eq!(
            document.append_text_coalesced(NodeId::from_index(usize::MAX), "invalid".into()),
            Err(DomError::InvalidParent(NodeId::from_index(usize::MAX)))
        );
    }
}
