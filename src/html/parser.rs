//! Streaming HTML document and contextual-fragment parsing.

use crate::dom::{Document, DocumentMode, DomError, ElementData, Namespace};

use super::{
    tokenizer::{InitialState, MAX_PARSE_ERRORS, ParseError, Tokenizer},
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

#[derive(Clone, Copy, Debug, Default)]
pub struct ParserOptions {
    pub scripting: bool,
}

#[derive(Clone, Debug)]
pub struct FragmentContext {
    pub element: ElementData,
    /// Ancestors in nearest-first order, for ancestor-sensitive form parsing.
    pub ancestors: Vec<ElementData>,
    pub document_mode: DocumentMode,
    pub scripting: bool,
}

impl FragmentContext {
    pub fn html(name: &str) -> Self {
        Self {
            element: ElementData::new(name.into(), Vec::new()),
            ancestors: Vec::new(),
            document_mode: DocumentMode::NoQuirks,
            scripting: false,
        }
    }
}

pub fn parse_fragment(source: &str, context: FragmentContext) -> Result<Document, HtmlError> {
    let mut parser = HtmlParser::fragment(context)?;
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
        Self::with_options(ParserOptions::default())
    }

    pub fn with_options(options: ParserOptions) -> Self {
        Self {
            tokenizer: Tokenizer::new(),
            builder: TreeBuilder::new(options.scripting),
        }
    }

    pub fn fragment(context: FragmentContext) -> Result<Self, HtmlError> {
        let state = if context.element.namespace() != &Namespace::Html {
            InitialState::Data
        } else {
            match context.element.tag_name() {
                "title" | "textarea" => InitialState::Rcdata,
                "style" | "xmp" | "iframe" | "noembed" | "noframes" => InitialState::Rawtext,
                "script" => InitialState::ScriptData,
                "noscript" if context.scripting => InitialState::Rawtext,
                "plaintext" => InitialState::Plaintext,
                _ => InitialState::Data,
            }
        };
        let mut tokenizer = Tokenizer::with_state(state, None);
        let builder = TreeBuilder::fragment(
            context.element,
            &context.ancestors,
            context.document_mode,
            context.scripting,
        )?;
        tokenizer.allow_cdata = builder.allow_cdata();
        Ok(Self { tokenizer, builder })
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
        self.builder.finish_fragment()?;
        let errors = self.errors();
        let dropped_errors = self.dropped_errors();
        Ok(ParsedHtml {
            document: self.builder.document,
            errors,
            dropped_errors,
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

    pub fn errors(&self) -> Vec<ParseError> {
        let mut errors = self.tokenizer.errors().to_vec();
        errors.extend_from_slice(&self.builder.errors);
        errors.sort_by_key(|error| error.position.byte_offset);
        errors.truncate(MAX_PARSE_ERRORS);
        errors
    }
    pub fn dropped_errors(&self) -> usize {
        self.tokenizer.dropped_errors()
            + self.builder.dropped_errors
            + (self.tokenizer.errors().len() + self.builder.errors.len())
                .saturating_sub(MAX_PARSE_ERRORS)
    }
    pub(crate) fn take_encoding_declaration(&mut self) -> Option<&'static encoding_rs::Encoding> {
        self.builder.encoding_declaration.take()
    }

    fn drain(&mut self) -> Result<(), HtmlError> {
        while let Some(token) = self.tokenizer.next_token() {
            if let Some((state, end_tag)) = self.builder.process(
                token,
                self.tokenizer.position(),
                self.tokenizer.emitted_character_positions(),
            )? {
                self.tokenizer.enter_text(state, &end_tag);
            }
            self.tokenizer.allow_cdata = self.builder.allow_cdata();
        }
        Ok(())
    }
}

#[cfg(test)]
mod construction_tests;
#[cfg(test)]
mod tests;
