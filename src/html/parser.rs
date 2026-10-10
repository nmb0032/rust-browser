//! Streaming tokenization with a still-naive tree builder.
//! WHATWG tree construction and contextual fragments remain steps 6.7-6.15.

use crate::dom::{Document, DomError};

use super::{
    tokenizer::{ParseError, Tokenizer},
    tree_builder::TreeBuilder,
};

#[derive(Debug, PartialEq, Eq)]
pub enum HtmlError {
    InvalidTreeState,
    Dom(DomError),
}

impl From<DomError> for HtmlError {
    fn from(error: DomError) -> Self {
        Self::Dom(error)
    }
}

#[derive(Debug)]
pub struct ParsedHtml {
    pub document: Document,
    pub errors: Vec<ParseError>,
    pub dropped_errors: usize,
}

pub fn parse(source: &str) -> Result<Document, HtmlError> {
    let mut parser = HtmlParser::new();
    parser.feed(source)?;
    parser.finish()
}

#[derive(Debug)]
pub struct HtmlParser {
    tokenizer: Tokenizer,
    builder: TreeBuilder,
}

impl Default for HtmlParser {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlParser {
    pub fn new() -> Self {
        Self {
            tokenizer: Tokenizer::new(),
            builder: TreeBuilder::new(),
        }
    }

    pub fn feed(&mut self, chunk: &str) -> Result<(), HtmlError> {
        self.tokenizer.feed(chunk);
        self.drain()
    }

    pub fn finish(self) -> Result<Document, HtmlError> {
        Ok(self.finish_with_diagnostics()?.document)
    }

    pub fn finish_with_diagnostics(mut self) -> Result<ParsedHtml, HtmlError> {
        self.tokenizer.finish();
        self.drain()?;
        Ok(ParsedHtml {
            document: self.builder.document,
            errors: self.tokenizer.errors().to_vec(),
            dropped_errors: self.tokenizer.dropped_errors(),
        })
    }

    pub fn document(&self) -> &Document {
        &self.builder.document
    }

    pub fn node_count(&self) -> usize {
        self.builder.document.node_count()
    }

    /// Includes changes to existing text nodes, unlike `node_count`.
    pub fn revision(&self) -> usize {
        self.builder.revision
    }

    pub fn errors(&self) -> &[ParseError] {
        self.tokenizer.errors()
    }

    fn drain(&mut self) -> Result<(), HtmlError> {
        while let Some(token) = self.tokenizer.next_token() {
            if let Some(end_tag) = self.builder.process(token)? {
                self.tokenizer.enter_rawtext(&end_tag);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
