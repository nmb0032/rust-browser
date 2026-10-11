use std::sync::Arc;

use super::tokenizer::TokenKind;

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
    pub(crate) imported: Option<ImportedStylesheet>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedStylesheet {
    pub stylesheet: Arc<Stylesheet>,
    pub url: url::Url,
    pub source: Arc<str>,
}

impl AtRule {
    pub fn imported(&self) -> Option<&ImportedStylesheet> {
        self.imported.as_ref()
    }
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

impl Stylesheet {
    pub(crate) fn rules_mut(&mut self) -> &mut [Rule] {
        &mut self.rules
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
    pub(super) tokens: Arc<[TokenKind]>,
    pub(super) important: bool,
}

impl Declaration {
    pub fn property(&self) -> &str {
        &self.property
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// Original component tokens, without the declaration's `!important`.
    /// Whitespace and boundaries discarded by textual concatenation are retained.
    pub fn value_tokens(&self) -> &[TokenKind] {
        &self.tokens
    }

    pub fn is_important(&self) -> bool {
        self.important
    }
}
