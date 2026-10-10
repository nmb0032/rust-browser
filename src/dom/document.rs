/*!
 * This module defines the structure and behavior of the Document for the Rust Browser's DOM.
 */

use super::node::{Attribute, DoctypeData, ElementData, Node, NodeId, NodeKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocumentMode {
    #[default]
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

#[derive(Clone, Debug)]
pub struct Document {
    root: NodeId,
    nodes: Vec<Node>,
    mode: DocumentMode,
    encoding: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomError {
    InvalidParent(NodeId),
    TextNodeCannotHaveChildren(NodeId),
    LeafNodeCannotHaveChildren(NodeId),
    InvalidNode(NodeId),
    HierarchyRequest,
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
            mode: DocumentMode::NoQuirks,
            encoding: "UTF-8".into(),
        }
    }

    pub fn root_id(&self) -> NodeId {
        self.root
    }

    pub fn new_fragment() -> Self {
        let mut document = Self::new();
        document.nodes[0] = Node::new(NodeKind::DocumentFragment, None);
        document
    }

    pub fn mode(&self) -> DocumentMode {
        self.mode
    }
    pub fn encoding(&self) -> &str {
        &self.encoding
    }
    pub(crate) fn set_encoding(&mut self, encoding: &str) {
        self.encoding = encoding.into();
    }
    pub(crate) fn set_mode(&mut self, mode: DocumentMode) {
        self.mode = mode;
    }

    pub fn append_comment(&mut self, parent: NodeId, text: String) -> Result<NodeId, DomError> {
        self.append_node(parent, NodeKind::Comment(text))
    }

    pub fn append_doctype(
        &mut self,
        parent: NodeId,
        data: DoctypeData,
    ) -> Result<NodeId, DomError> {
        self.append_node(parent, NodeKind::Doctype(data))
    }

    pub fn append_processing_instruction(
        &mut self,
        parent: NodeId,
        target: String,
        data: String,
    ) -> Result<NodeId, DomError> {
        self.append_node(parent, NodeKind::ProcessingInstruction { target, data })
    }

    pub(crate) fn create_template_contents(
        &mut self,
        template: NodeId,
    ) -> Result<NodeId, DomError> {
        let node = self
            .nodes
            .get_mut(template.index())
            .ok_or(DomError::InvalidNode(template))?;
        let element = node.as_element_mut().ok_or(DomError::HierarchyRequest)?;
        if let Some(id) = element.template_contents() {
            return Ok(id);
        }
        let id = NodeId::from_index(self.nodes.len());
        self.nodes[template.index()]
            .as_element_mut()
            .expect("element")
            .set_template_contents(id);
        self.nodes.push(Node::new(NodeKind::DocumentFragment, None));
        self.nodes[id.index()].set_template_host(template);
        Ok(id)
    }

    pub(crate) fn merge_attributes(
        &mut self,
        id: NodeId,
        attributes: Vec<Attribute>,
    ) -> Result<(), DomError> {
        self.nodes
            .get_mut(id.index())
            .ok_or(DomError::InvalidNode(id))?
            .as_element_mut()
            .ok_or(DomError::HierarchyRequest)?
            .merge_attributes(attributes);
        Ok(())
    }

    pub fn detach(&mut self, id: NodeId) -> Result<(), DomError> {
        let parent = self.node(id).ok_or(DomError::InvalidNode(id))?.parent();
        if id == self.root {
            return Err(DomError::HierarchyRequest);
        }
        if let Some(parent) = parent {
            self.nodes[parent.index()]
                .children_mut()
                .retain(|child| *child != id);
        }
        self.nodes[id.index()].set_parent(None);
        Ok(())
    }

    pub fn clone_subtree(&mut self, parent: NodeId, source: NodeId) -> Result<NodeId, DomError> {
        self.validate_parent(parent)?;
        let original_count = self.nodes.len();
        let mut pending = vec![(parent, source)];
        let mut result = None;
        while let Some((parent, source)) = pending.pop() {
            if source.index() >= original_count {
                return Err(DomError::InvalidNode(source));
            }
            let node = self.nodes[source.index()].clone();
            let kind = match node.kind() {
                NodeKind::Document => return Err(DomError::HierarchyRequest),
                NodeKind::Element(element) => {
                    NodeKind::Element(element.clone_without_template_contents())
                }
                kind => kind.clone(),
            };
            let id = self.append_node(parent, kind)?;
            result.get_or_insert(id);
            pending.extend(node.children().iter().rev().map(|child| (id, *child)));
            if let Some(contents) = node.as_element().and_then(ElementData::template_contents) {
                let cloned_contents = self.create_template_contents(id)?;
                let children = self
                    .node(contents)
                    .ok_or(DomError::InvalidNode(contents))?
                    .children();
                pending.extend(children.iter().rev().map(|child| (cloned_contents, *child)));
            }
        }
        result.ok_or(DomError::InvalidNode(source))
    }

    pub fn insert_before(
        &mut self,
        parent: NodeId,
        id: NodeId,
        before: Option<NodeId>,
    ) -> Result<(), DomError> {
        self.validate_parent(parent)?;
        self.node(id).ok_or(DomError::InvalidNode(id))?;
        if self.nodes[id.index()].template_host().is_some() {
            return Err(DomError::HierarchyRequest);
        }
        if before.is_some_and(|before| self.node(before).and_then(Node::parent) != Some(parent)) {
            return Err(DomError::HierarchyRequest);
        }
        let mut ancestor = Some(parent);
        while let Some(current) = ancestor {
            if current == id {
                return Err(DomError::HierarchyRequest);
            }
            ancestor = self.nodes[current.index()]
                .parent()
                .or(self.nodes[current.index()].template_host());
        }
        if before == Some(id) {
            return Ok(());
        }
        self.detach(id)?;
        let children = self.nodes[parent.index()].children_mut();
        let index = before
            .and_then(|before| children.iter().position(|child| *child == before))
            .unwrap_or(children.len());
        children.insert(index, id);
        self.nodes[id.index()].set_parent(Some(parent));
        Ok(())
    }

    pub(crate) fn insert_text(
        &mut self,
        parent: NodeId,
        before: Option<NodeId>,
        text: String,
    ) -> Result<NodeId, DomError> {
        if before.is_none() {
            return self.append_text_coalesced(parent, text);
        }
        self.validate_parent(parent)?;
        let children = self.nodes[parent.index()].children();
        let index = match before {
            Some(before) => children
                .iter()
                .position(|child| *child == before)
                .ok_or(DomError::HierarchyRequest)?,
            None => children.len(),
        };
        if index > 0 {
            let previous = children[index - 1];
            if self.nodes[previous.index()].append_text(&text) {
                return Ok(previous);
            }
        }
        let id = self.append_text(parent, text)?;
        self.insert_before(parent, id, before)?;
        Ok(id)
    }

    fn validate_parent(&self, parent: NodeId) -> Result<(), DomError> {
        match self
            .node(parent)
            .ok_or(DomError::InvalidParent(parent))?
            .kind()
        {
            NodeKind::Text(_) => Err(DomError::TextNodeCannotHaveChildren(parent)),
            NodeKind::Comment(_)
            | NodeKind::Doctype(_)
            | NodeKind::ProcessingInstruction { .. } => {
                Err(DomError::LeafNodeCannotHaveChildren(parent))
            }
            _ => Ok(()),
        }
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn validate(&self) -> Result<(), DomError> {
        if self.nodes[self.root.index()].parent().is_some() {
            return Err(DomError::HierarchyRequest);
        }
        let mut owners = vec![None; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            let parent = NodeId::from_index(index);
            for child in node.children() {
                let child_node = self.node(*child).ok_or(DomError::InvalidNode(*child))?;
                if child_node.parent() != Some(parent)
                    || owners[child.index()].replace(parent).is_some()
                {
                    return Err(DomError::HierarchyRequest);
                }
                if let Some(contents) = node.as_element().and_then(ElementData::template_contents) {
                    let fragment = self.node(contents).ok_or(DomError::InvalidNode(contents))?;
                    if !matches!(fragment.kind(), NodeKind::DocumentFragment)
                        || fragment.parent().is_some()
                        || fragment.template_host() != Some(parent)
                    {
                        return Err(DomError::HierarchyRequest);
                    }
                }
                if let Some(host) = node.template_host()
                    && self
                        .node(host)
                        .and_then(Node::as_element)
                        .and_then(ElementData::template_contents)
                        != Some(parent)
                {
                    return Err(DomError::HierarchyRequest);
                }
            }
        }
        if self
            .nodes
            .iter()
            .enumerate()
            .any(|(index, node)| node.parent() != owners[index])
        {
            return Err(DomError::HierarchyRequest);
        }
        let mut color = vec![0; self.nodes.len()];
        for index in 0..self.nodes.len() {
            let mut path = Vec::new();
            let mut current = Some(NodeId::from_index(index));
            while let Some(id) = current {
                match color[id.index()] {
                    1 => return Err(DomError::HierarchyRequest),
                    2 => break,
                    _ => {}
                }
                color[id.index()] = 1;
                path.push(id);
                current = self.nodes[id.index()]
                    .parent()
                    .or(self.nodes[id.index()].template_host());
            }
            for id in path {
                color[id.index()] = 2;
            }
        }
        Ok(())
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
        self.validate_parent(parent)?;

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
    fn reparenting_preserves_ids_order_and_rejects_cycles_without_mutation() {
        let mut document = Document::new();
        let root = document.root_id();
        let parent = document
            .append_element(root, ElementData::new("p".into(), vec![]))
            .unwrap();
        let first = document.append_text(parent, "first".into()).unwrap();
        let last = document.append_comment(parent, "last".into()).unwrap();
        document.insert_before(parent, last, Some(first)).unwrap();
        assert_eq!(document.node(parent).unwrap().children(), &[last, first]);
        let before = format!("{document:?}");
        assert_eq!(
            document.insert_before(parent, parent, None),
            Err(DomError::HierarchyRequest)
        );
        assert_eq!(
            document.insert_before(parent, root, None),
            Err(DomError::HierarchyRequest)
        );
        assert_eq!(format!("{document:?}"), before);
        document.detach(first).unwrap();
        document.insert_before(root, first, Some(parent)).unwrap();
        assert_eq!(document.node(first).unwrap().parent(), Some(root));
        document.validate().unwrap();
    }

    #[test]
    fn cloning_template_contents_uses_new_arena_ids() {
        let mut document = Document::new();
        let root = document.root_id();
        let template = document
            .append_element(root, ElementData::new("template".into(), vec![]))
            .unwrap();
        let contents = document.create_template_contents(template).unwrap();
        assert_eq!(
            document.insert_before(contents, template, None),
            Err(DomError::HierarchyRequest)
        );
        let text = document.append_text(contents, "hidden".into()).unwrap();
        let cloned = document.clone_subtree(root, template).unwrap();
        let cloned_contents = document
            .node(cloned)
            .unwrap()
            .as_element()
            .unwrap()
            .template_contents()
            .unwrap();
        assert_ne!(contents, cloned_contents);
        assert_ne!(document.node(cloned_contents).unwrap().children(), &[text]);
        document.validate().unwrap();
    }

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
