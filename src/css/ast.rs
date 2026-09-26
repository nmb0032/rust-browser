#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stylesheet {
    pub(super) rules: Vec<Rule>,
}

impl Stylesheet {
    pub(super) fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rule {
    Qualified(QualifiedRule),
    At(AtRule),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualifiedRule {
    pub(super) prelude: String,
    pub(super) items: Vec<BlockItem>,
}

impl QualifiedRule {
    pub fn prelude(&self) -> &str {
        &self.prelude
    }

    pub fn items(&self) -> &[BlockItem] {
        &self.items
    }

    pub fn declarations(&self) -> impl Iterator<Item = &Declaration> {
        self.items.iter().filter_map(|item| match item {
            BlockItem::Declaration(declaration) => Some(declaration),
            BlockItem::Rule(_) => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtRule {
    pub(super) name: String,
    pub(super) prelude: String,
    pub(super) block: Option<Vec<BlockItem>>,
}

impl AtRule {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn prelude(&self) -> &str {
        &self.prelude
    }

    pub fn block(&self) -> Option<&[BlockItem]> {
        self.block.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockItem {
    Declaration(Declaration),
    Rule(Rule),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub(super) property: String,
    pub(super) value: String,
    pub(super) important: bool,
}

impl Declaration {
    pub fn property(&self) -> &str {
        &self.property
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn is_important(&self) -> bool {
        self.important
    }
}
