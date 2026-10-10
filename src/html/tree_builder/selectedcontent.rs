use super::TreeBuilder;
use crate::{dom::NodeId, html::parser::HtmlError};

impl TreeBuilder {
    pub(super) fn option_select(&self, id: NodeId) -> Option<NodeId> {
        let mut ancestor = self.document.node(id)?.parent();
        let mut optgroup = false;
        while let Some(id) = ancestor {
            if self.is_html(id, "select") {
                return Some(id);
            }
            if self.is_html(id, "option") || self.is_html(id, "datalist") || self.is_html(id, "hr")
            {
                return None;
            }
            if self.is_html(id, "optgroup") {
                if optgroup {
                    return None;
                }
                optgroup = true;
            }
            ancestor = self.document.node(id)?.parent();
        }
        None
    }

    pub(super) fn selectedcontent_select(&self, id: NodeId) -> Option<NodeId> {
        let mut ancestor = self.document.node(id)?.parent();
        let mut select = None;
        while let Some(id) = ancestor {
            if self.is_html(id, "option") || self.is_html(id, "selectedcontent") {
                return None;
            }
            if self.is_html(id, "select") {
                if select.is_some() {
                    return None;
                }
                select = Some(id);
            }
            ancestor = self.document.node(id)?.parent();
        }
        select
    }

    pub(super) fn update_closed_options(&mut self, eof: bool) -> Result<(), HtmlError> {
        let closed = self
            .open_options
            .iter()
            .copied()
            .filter(|id| eof || !self.open.contains(id))
            .collect::<Vec<_>>();
        for option in closed {
            self.open_options.retain(|id| *id != option);
            if let Some(select) = self.option_select(option) {
                self.pending_selects.push(select);
            }
        }
        let mut updated = Vec::new();
        for select in std::mem::take(&mut self.pending_selects) {
            if !updated.contains(&select) {
                self.update_selectedcontent(select)?;
                updated.push(select);
            }
        }
        Ok(())
    }

    fn update_selectedcontent(&mut self, select: NodeId) -> Result<(), HtmlError> {
        if !self.selectedcontent_selects.contains(&select) {
            return Ok(());
        }
        let element = self
            .document
            .node(select)
            .and_then(|node| node.as_element())
            .ok_or(HtmlError::InvalidTreeState)?;
        if element.attribute("multiple").is_some() {
            return Ok(());
        }
        let size = element
            .attribute("size")
            .and_then(|size| {
                size.trim_start_matches(|ch: char| ch.is_ascii_whitespace())
                    .strip_prefix('+')
                    .unwrap_or(size.trim_start_matches(|ch: char| ch.is_ascii_whitespace()))
                    .split(|ch: char| !ch.is_ascii_digit())
                    .next()?
                    .parse::<usize>()
                    .ok()
            })
            .filter(|size| *size > 0)
            .unwrap_or(1);
        let mut options = Vec::new();
        let mut targets = Vec::new();
        let mut pending = vec![select];
        while let Some(id) = pending.pop() {
            if self.is_html(id, "option") && self.option_select(id) == Some(select) {
                options.push(id);
            }
            if self.is_html(id, "selectedcontent")
                && self.selectedcontent_select(id) == Some(select)
            {
                targets.push(id);
            }
            pending.extend(
                self.document
                    .node(id)
                    .ok_or(HtmlError::InvalidTreeState)?
                    .children()
                    .iter()
                    .rev()
                    .copied(),
            );
        }
        let selected = options
            .iter()
            .rev()
            .copied()
            .find(|id| {
                self.document
                    .node(*id)
                    .and_then(|node| node.as_element())
                    .is_some_and(|element| element.attribute("selected").is_some())
            })
            .or_else(|| {
                if size == 1 {
                    options
                        .iter()
                        .copied()
                        .find(|id| !self.disabled_option(*id))
                } else {
                    None
                }
            });
        for target in targets {
            let children = self
                .document
                .node(target)
                .ok_or(HtmlError::InvalidTreeState)?
                .children()
                .to_vec();
            for child in children {
                self.document.detach(child)?;
            }
            if let Some(option) = selected {
                let children = self
                    .document
                    .node(option)
                    .ok_or(HtmlError::InvalidTreeState)?
                    .children()
                    .to_vec();
                for child in children {
                    self.document.clone_subtree(target, child)?;
                }
            }
            self.revision += 1;
        }
        Ok(())
    }

    fn disabled_option(&self, option: NodeId) -> bool {
        let mut current = Some(option);
        while let Some(id) = current {
            if self.is_html(id, "select") {
                return false;
            }
            if (id == option || self.is_html(id, "optgroup"))
                && self
                    .document
                    .node(id)
                    .and_then(|node| node.as_element())
                    .is_some_and(|element| element.attribute("disabled").is_some())
            {
                return true;
            }
            current = self.document.node(id).and_then(|node| node.parent());
        }
        false
    }
}
