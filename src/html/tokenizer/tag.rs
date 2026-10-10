use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagAttribute {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagToken {
    pub name: String,
    pub attributes: Vec<TagAttribute>,
    pub self_closing: bool,
}

#[derive(Debug, Default)]
pub(super) struct PendingTag {
    pub token: TagToken,
    pub is_end: bool,
    pub attribute: Option<TagAttribute>,
    names: HashSet<String>,
    duplicate: bool,
}

impl PendingTag {
    pub fn start_attribute(&mut self) {
        self.finish_attribute();
        self.attribute = Some(TagAttribute {
            name: String::new(),
            value: String::new(),
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

    pub fn name(&mut self) -> &mut String {
        &mut self.attribute.as_mut().expect("attribute name state").name
    }

    pub fn value(&mut self) -> &mut String {
        &mut self
            .attribute
            .as_mut()
            .expect("attribute value state")
            .value
    }
}
