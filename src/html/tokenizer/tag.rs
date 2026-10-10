use super::buffer::TextBuffer;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagAttribute<T = String> {
    pub name: T,
    pub value: T,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagToken<T = String> {
    pub name: T,
    pub attributes: Vec<TagAttribute<T>>,
    pub self_closing: bool,
}

impl<T> TagToken<T> {
    pub(super) fn map_text<U>(self, map: &mut impl FnMut(T) -> U) -> TagToken<U> {
        TagToken {
            name: map(self.name),
            attributes: self
                .attributes
                .into_iter()
                .map(|attribute| TagAttribute {
                    name: map(attribute.name),
                    value: map(attribute.value),
                })
                .collect(),
            self_closing: self.self_closing,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct PendingTag {
    pub token: TagToken<TextBuffer>,
    pub is_end: bool,
    pub attribute: Option<TagAttribute<TextBuffer>>,
    names: HashSet<TextBuffer>,
    duplicate: bool,
}

impl PendingTag {
    pub fn start_attribute(&mut self) {
        self.finish_attribute();
        self.attribute = Some(TagAttribute {
            name: TextBuffer::default(),
            value: TextBuffer::default(),
        });
        self.duplicate = false;
    }

    pub fn finish_name(&mut self) -> bool {
        let attribute = self.attribute.as_ref().expect("attribute name state");
        self.duplicate = !self.names.insert(attribute.name.clone());
        self.duplicate
    }

    pub fn finish_attribute(&mut self) {
        if let Some(attribute) = self.attribute.take()
            && !self.duplicate
        {
            self.token.attributes.push(attribute);
        }
    }

    pub fn name(&mut self) -> &mut TextBuffer {
        &mut self.attribute.as_mut().expect("attribute name state").name
    }

    pub fn value(&mut self) -> &mut TextBuffer {
        &mut self
            .attribute
            .as_mut()
            .expect("attribute value state")
            .value
    }
}
