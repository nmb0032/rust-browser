use super::{Scope, TreeBuilder};
use crate::dom::{ElementData, NodeId};
use crate::html::parser::HtmlError;

impl TreeBuilder {
    pub(super) fn clear_formatting(&mut self) {
        while let Some(entry) = self.formatting.pop() {
            if entry.is_none() {
                break;
            }
        }
    }

    pub(super) fn find_formatting(&self, name: &str) -> Option<(usize, NodeId)> {
        self.formatting
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, id)| id.is_some())
            .find_map(|(index, id)| {
                id.filter(|id| self.is_html(*id, name))
                    .map(|id| (index, id))
            })
    }

    pub(super) fn add_formatting(&mut self, id: NodeId) {
        let element = self.element(id);
        let equivalent = self
            .formatting
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, entry)| entry.is_some())
            .filter_map(|(index, entry)| {
                entry
                    .filter(|old| equivalent_elements(self.element(*old), element))
                    .map(|_| index)
            })
            .collect::<Vec<_>>();
        if equivalent.len() >= 3 {
            self.formatting
                .remove(*equivalent.last().expect("three matches"));
        }
        self.formatting.push(Some(id));
    }

    fn element(&self, id: NodeId) -> &ElementData {
        self.document
            .node(id)
            .and_then(|node| node.as_element())
            .expect("open element")
    }

    fn clone_element(&mut self, id: NodeId, parent: NodeId) -> Result<NodeId, HtmlError> {
        let element = self.element(id).clone();
        let id = self.document.append_element(parent, element)?;
        self.revision += 1;
        Ok(id)
    }

    pub(super) fn reconstruct(&mut self) -> Result<(), HtmlError> {
        let Some(Some(last)) = self.formatting.last() else {
            return Ok(());
        };
        if self.open.contains(last) {
            return Ok(());
        }
        let mut index = self.formatting.len() - 1;
        while index > 0 {
            match self.formatting[index - 1] {
                Some(id) if !self.open.contains(&id) => index -= 1,
                _ => break,
            }
        }
        for index in index..self.formatting.len() {
            let old = self.formatting[index].ok_or(HtmlError::InvalidTreeState)?;
            let (parent, before) = self.location(None)?;
            let id = self.clone_element(old, parent)?;
            if before.is_some() {
                self.document.insert_before(parent, id, before)?;
            }
            self.open.push(id);
            self.formatting[index] = Some(id);
        }
        Ok(())
    }

    pub(super) fn adoption(&mut self, name: &str) -> Result<(), HtmlError> {
        if let Some(current) = self.current()
            && self.is_html(current, name)
            && !self.formatting.contains(&Some(current))
        {
            self.pop();
            return Ok(());
        }
        for _ in 0..8 {
            let Some((format_index, formatting)) = self.find_formatting(name) else {
                self.generic_end(name);
                return Ok(());
            };
            let Some(open_index) = self.open.iter().position(|id| *id == formatting) else {
                self.error("formatting-element-not-open");
                self.formatting.remove(format_index);
                return Ok(());
            };
            if !self.scope(name, Scope::Normal) {
                self.error("formatting-element-not-in-scope");
                return Ok(());
            }
            if self.current() != Some(formatting) {
                self.error("misnested-formatting-element");
            }
            let Some(furthest_index) =
                (open_index + 1..self.open.len()).find(|index| self.special(self.open[*index]))
            else {
                self.open.truncate(open_index);
                self.formatting.remove(format_index);
                return Ok(());
            };
            let furthest = self.open[furthest_index];
            let common = *self
                .open
                .get(open_index.wrapping_sub(1))
                .ok_or(HtmlError::InvalidTreeState)?;
            let mut bookmark = format_index;
            let mut last = furthest;
            let mut index = furthest_index;
            let mut counter = 0;
            loop {
                index -= 1;
                let node = self.open[index];
                if node == formatting {
                    break;
                }
                counter += 1;
                let mut entry = self
                    .formatting
                    .iter()
                    .position(|entry| *entry == Some(node));
                if counter > 3
                    && let Some(position) = entry
                {
                    self.formatting.remove(position);
                    if position < bookmark {
                        bookmark -= 1;
                    }
                    entry = None;
                }
                let Some(entry) = entry else {
                    self.open.remove(index);
                    continue;
                };
                let replacement = self.clone_element(node, common)?;
                self.formatting[entry] = Some(replacement);
                self.open[index] = replacement;
                if last == furthest {
                    bookmark = entry + 1;
                }
                self.document.insert_before(replacement, last, None)?;
                last = replacement;
            }
            let old_foster = self.foster;
            self.foster = true;
            let (parent, before) = self.location(Some(common))?;
            self.foster = old_foster;
            self.document.insert_before(parent, last, before)?;
            let replacement = self.clone_element(formatting, furthest)?;
            let children = self
                .document
                .node(furthest)
                .ok_or(HtmlError::InvalidTreeState)?
                .children()
                .to_vec();
            for child in children {
                if child != replacement {
                    self.document.insert_before(replacement, child, None)?;
                }
            }
            if let Some(index) = self
                .formatting
                .iter()
                .position(|entry| *entry == Some(formatting))
            {
                self.formatting.remove(index);
                if index < bookmark {
                    bookmark -= 1;
                }
            }
            self.formatting
                .insert(bookmark.min(self.formatting.len()), Some(replacement));
            self.open.retain(|id| *id != formatting);
            let index = self
                .open
                .iter()
                .position(|id| *id == furthest)
                .ok_or(HtmlError::InvalidTreeState)?;
            self.open.insert(index + 1, replacement);
            self.revision += 1;
        }
        Ok(())
    }
}

fn equivalent_elements(left: &ElementData, right: &ElementData) -> bool {
    left.tag_name() == right.tag_name()
        && left.namespace() == right.namespace()
        && left.attributes().len() == right.attributes().len()
        && left.attributes().iter().all(|left| {
            right.attributes().iter().any(|right| {
                left.name() == right.name()
                    && left.value() == right.value()
                    && left.namespace() == right.namespace()
            })
        })
}
