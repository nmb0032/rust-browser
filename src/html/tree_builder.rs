//! The earlier append/ancestor-closing tree builder, isolated for steps 6.7-6.15.
//! This is deliberately not the WHATWG tree-construction algorithm.

use crate::dom::{Attribute, Document, ElementData, NodeId};

use super::{
    parser::HtmlError,
    tokenizer::{TagToken, Token},
};

#[derive(Debug)]
pub(super) struct TreeBuilder {
    pub document: Document,
    elements: Vec<NodeId>,
    pub revision: usize,
    style_text: Option<String>,
}

impl TreeBuilder {
    pub fn new() -> Self {
        let document = Document::new();
        Self {
            elements: vec![document.root_id()],
            document,
            revision: 0,
            style_text: None,
        }
    }

    /// Returns the tokenizer's new RAWTEXT context when inserting a style.
    pub fn process(&mut self, token: Token) -> Result<Option<String>, HtmlError> {
        match token {
            Token::StartTag(TagToken {
                name, attributes, ..
            }) => {
                let rawtext = (name == "style").then(|| name.clone());
                if rawtext.is_some() {
                    self.style_text = Some(String::new());
                }
                let is_void = is_void_element(&name);
                let parent = *self.elements.last().ok_or(HtmlError::InvalidTreeState)?;
                let attributes = attributes
                    .into_iter()
                    .map(|attribute| Attribute::new(attribute.name, attribute.value))
                    .collect();
                let id = self
                    .document
                    .append_element(parent, ElementData::new(name, attributes))?;
                if !is_void {
                    self.elements.push(id);
                }
                self.revision += 1;
                return Ok(rawtext);
            }
            Token::EndTag(tag) => {
                if tag.name == "style" {
                    self.flush_style_text()?;
                }
                let matching =
                    self.elements
                        .iter()
                        .enumerate()
                        .skip(1)
                        .rev()
                        .find_map(|(index, id)| {
                            let element = self.document.node(*id)?.as_element()?;
                            (element.tag_name() == tag.name).then_some(index)
                        });
                if let Some(index) = matching {
                    self.elements.truncate(index);
                }
            }
            Token::Character(text) => {
                if let Some(pending) = &mut self.style_text {
                    pending.push_str(&text);
                    return Ok(None);
                }
                let parent = *self.elements.last().ok_or(HtmlError::InvalidTreeState)?;
                self.document.append_text_coalesced(parent, text)?;
                self.revision += 1;
            }
            // Comment/doctype nodes and insertion modes arrive in 6.3 and 6.7.
            Token::Markup(_) => {}
            Token::Eof => self.flush_style_text()?,
        }
        Ok(None)
    }

    fn flush_style_text(&mut self) -> Result<(), HtmlError> {
        // Keep incomplete CSS out of progressive snapshots until close or EOF,
        // as before; the HTML tokenizer still consumes each character only once.
        if let Some(text) = self.style_text.take()
            && !text.is_empty()
        {
            let parent = *self.elements.last().ok_or(HtmlError::InvalidTreeState)?;
            self.document.append_text_coalesced(parent, text)?;
            self.revision += 1;
        }
        Ok(())
    }
}

fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}
