//! HTML tree construction (WHATWG HTML 13.2.6).

mod body;
mod foreign;
mod formatting;
mod modes;
mod selectedcontent;
mod tables;

use super::{
    parser::HtmlError,
    tokenizer::{InitialState, MAX_PARSE_ERRORS, ParseError, SourcePosition, TagToken, Token},
};
use crate::dom::{Attribute, Document, DocumentMode, ElementData, Namespace, NodeId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

#[derive(Debug)]
pub(super) struct TreeBuilder {
    pub document: Document,
    open: Vec<NodeId>,
    formatting: Vec<Option<NodeId>>,
    mode: Mode,
    original_mode: Mode,
    template_modes: Vec<Mode>,
    head: Option<NodeId>,
    form: Option<NodeId>,
    frameset_ok: bool,
    foster: bool,
    scripting: bool,
    fragment: Option<ElementData>,
    ignore_lf: bool,
    table_text: String,
    table_position: Option<SourcePosition>,
    style_text: Option<String>,
    pub revision: usize,
    pub errors: Vec<ParseError>,
    pub dropped_errors: usize,
    position: SourcePosition,
    text_context: Option<(InitialState, String)>,
    pub encoding_declaration: Option<&'static encoding_rs::Encoding>,
    open_options: Vec<NodeId>,
    pending_selects: Vec<NodeId>,
    selectedcontent_selects: std::collections::HashSet<NodeId>,
}

impl TreeBuilder {
    pub fn new(scripting: bool) -> Self {
        Self {
            document: Document::new(),
            open: Vec::new(),
            formatting: Vec::new(),
            mode: Mode::Initial,
            original_mode: Mode::Initial,
            template_modes: Vec::new(),
            head: None,
            form: None,
            frameset_ok: true,
            foster: false,
            scripting,
            fragment: None,
            ignore_lf: false,
            table_text: String::new(),
            table_position: None,
            style_text: None,
            revision: 0,
            errors: Vec::new(),
            dropped_errors: 0,
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            text_context: None,
            encoding_declaration: None,
            open_options: Vec::new(),
            pending_selects: Vec::new(),
            selectedcontent_selects: std::collections::HashSet::new(),
        }
    }

    pub fn fragment(
        context: ElementData,
        ancestors: &[ElementData],
        mode: DocumentMode,
        scripting: bool,
    ) -> Result<Self, HtmlError> {
        let mut builder = Self::new(scripting);
        builder.document = Document::new_fragment();
        builder.document.set_mode(mode);
        let html = builder.document.append_element(
            builder.document.root_id(),
            ElementData::new("html".into(), Vec::new()),
        )?;
        builder.open.push(html);
        if let Some(form) = std::iter::once(&context)
            .chain(ancestors)
            .find(|element| element.namespace() == &Namespace::Html && element.tag_name() == "form")
        {
            let id = builder
                .document
                .append_element(builder.document.root_id(), form.clone())?;
            builder.document.detach(id)?;
            builder.form = Some(id);
        }
        if context.namespace() == &Namespace::Html && context.tag_name() == "template" {
            builder.template_modes.push(Mode::InTemplate);
            builder.formatting.push(None);
        }
        builder.fragment = Some(context);
        builder.reset_mode();
        Ok(builder)
    }

    pub fn finish_fragment(&mut self) -> Result<(), HtmlError> {
        if self.fragment.is_some() {
            let html = self
                .document
                .node(self.document.root_id())
                .and_then(|node| node.children().first())
                .copied()
                .ok_or(HtmlError::InvalidTreeState)?;
            let children = self
                .document
                .node(html)
                .ok_or(HtmlError::InvalidTreeState)?
                .children()
                .to_vec();
            for child in children {
                self.document
                    .insert_before(self.document.root_id(), child, None)?;
            }
            self.document.detach(html)?;
        }
        Ok(())
    }

    pub fn process(
        &mut self,
        token: Token,
        position: SourcePosition,
        positions: &[SourcePosition],
    ) -> Result<Option<(InitialState, String)>, HtmlError> {
        self.position = position;
        let eof = matches!(token, Token::Eof);
        let foreign = self.use_foreign_rules(&token);
        if let Token::Character(text) = token {
            if !self.ignore_lf
                && !text.contains('\0')
                && (self.mode == Mode::Text || self.mode == Mode::InBody && !foreign)
            {
                if self.mode == Mode::InBody {
                    self.reconstruct()?;
                    if !space(&text) {
                        self.frameset_ok = false;
                    }
                }
                self.text(&text)?;
                self.update_closed_options(false)?;
                return Ok(self.text_context.take());
            }
            // Tree-construction rules are defined per character. This also keeps
            // table buffering and LF suppression independent of transport chunks.
            for (ch, position) in text.chars().zip(positions) {
                self.position = *position;
                if self.ignore_lf {
                    self.ignore_lf = false;
                    if ch == '\n' {
                        continue;
                    }
                }
                self.process_one(Token::Character(ch.to_string()))?;
            }
        } else {
            self.ignore_lf = false;
            self.process_one(token)?;
        }
        self.update_closed_options(eof)?;
        Ok(self.text_context.take())
    }

    fn advance_position(&mut self, ch: char) {
        self.position.byte_offset += ch.len_utf8();
        if ch == '\n' {
            self.position.line += 1;
            self.position.column = 1;
        } else {
            self.position.column += ch.len_utf16();
        }
    }

    fn process_one(&mut self, token: Token) -> Result<(), HtmlError> {
        loop {
            if self.mode != Mode::InTableText
                && let Token::ProcessingInstruction { target, data } = &token
            {
                let parent = match self.mode {
                    Mode::Initial
                    | Mode::BeforeHtml
                    | Mode::AfterAfterBody
                    | Mode::AfterAfterFrameset => self.document.root_id(),
                    Mode::AfterBody => *self.open.first().ok_or(HtmlError::InvalidTreeState)?,
                    _ => self.location(None)?.0,
                };
                self.document.append_processing_instruction(
                    parent,
                    target.clone(),
                    data.clone(),
                )?;
                self.revision += 1;
                return Ok(());
            }
            if self.mode != Mode::InTableText
                && self.use_foreign_rules(&token)
                && self.foreign(&token)?
            {
                return Ok(());
            }
            let handled = self.html_rules(&token)?;
            if handled {
                return Ok(());
            }
        }
    }

    fn html_rules(&mut self, token: &Token) -> Result<bool, HtmlError> {
        match self.mode {
            Mode::InBody => self.in_body(token),
            Mode::InTable
            | Mode::InTableText
            | Mode::InCaption
            | Mode::InColumnGroup
            | Mode::InTableBody
            | Mode::InRow
            | Mode::InCell => self.table_mode(token),
            _ => self.other_mode(token),
        }
    }

    fn error(&mut self, code: &'static str) {
        if self.errors.len() < MAX_PARSE_ERRORS {
            self.errors.push(ParseError {
                code,
                position: self.position,
            });
        } else {
            self.dropped_errors += 1;
        }
    }

    fn current(&self) -> Option<NodeId> {
        self.open.last().copied()
    }
    fn name(&self, id: NodeId) -> &str {
        self.document
            .node(id)
            .and_then(|node| node.as_element())
            .map_or("", ElementData::tag_name)
    }
    fn current_name(&self) -> &str {
        self.current().map_or("", |id| self.name(id))
    }
    fn is_html(&self, id: NodeId, name: &str) -> bool {
        self.document
            .node(id)
            .and_then(|node| node.as_element())
            .is_some_and(|element| {
                element.namespace() == &Namespace::Html && element.tag_name() == name
            })
    }
    fn has_template(&self) -> bool {
        self.open.iter().any(|id| self.is_html(*id, "template"))
    }
    fn attributes(tag: &TagToken) -> Vec<Attribute> {
        tag.attributes
            .iter()
            .map(|attr| Attribute::new(attr.name.clone(), attr.value.clone()))
            .collect()
    }
    fn location(
        &self,
        override_target: Option<NodeId>,
    ) -> Result<(NodeId, Option<NodeId>), HtmlError> {
        let target = override_target
            .or(self.current())
            .unwrap_or(self.document.root_id());
        let (mut parent, before) = if self.foster
            && matches!(
                self.name(target),
                "table" | "tbody" | "tfoot" | "thead" | "tr"
            ) {
            let template = self
                .open
                .iter()
                .rposition(|id| self.is_html(*id, "template"));
            let table = self.open.iter().rposition(|id| self.is_html(*id, "table"));
            if template.is_some_and(|template| table.is_none_or(|table| template > table)) {
                (self.open[template.expect("template index")], None)
            } else if let Some(table) = table {
                let id = self.open[table];
                if let Some(parent) = self.document.node(id).and_then(|node| node.parent()) {
                    (parent, Some(id))
                } else {
                    (
                        *self
                            .open
                            .get(table.wrapping_sub(1))
                            .ok_or(HtmlError::InvalidTreeState)?,
                        None,
                    )
                }
            } else {
                (*self.open.first().ok_or(HtmlError::InvalidTreeState)?, None)
            }
        } else {
            (target, None)
        };
        if let Some(contents) = self
            .document
            .node(parent)
            .and_then(|node| node.as_element())
            .and_then(ElementData::template_contents)
        {
            parent = contents;
        }
        Ok((parent, before))
    }

    fn insert(&mut self, tag: &TagToken, namespace: Namespace) -> Result<NodeId, HtmlError> {
        let (parent, before) = self.location(None)?;
        let element = self.adjust_element(tag, namespace);
        let template = element.namespace() == &Namespace::Html && element.tag_name() == "template";
        let id = self.document.append_element(parent, element)?;
        if before.is_some() {
            self.document.insert_before(parent, id, before)?;
        }
        if template {
            self.document.create_template_contents(id)?;
        }
        self.open.push(id);
        if self.is_html(id, "option") {
            self.open_options.push(id);
            if let Some(select) = self.option_select(id) {
                self.pending_selects.push(select);
            }
        } else if self.is_html(id, "selectedcontent")
            && let Some(select) = self.selectedcontent_select(id)
        {
            self.selectedcontent_selects.insert(select);
            self.pending_selects.push(select);
        }
        self.revision += 1;
        Ok(id)
    }
    fn insert_named(&mut self, name: &str) -> Result<NodeId, HtmlError> {
        self.insert(
            &TagToken {
                name: name.into(),
                attributes: Vec::new(),
                self_closing: false,
            },
            Namespace::Html,
        )
    }
    fn text(&mut self, text: &str) -> Result<(), HtmlError> {
        if let Some(pending) = &mut self.style_text {
            pending.push_str(text);
            return Ok(());
        }
        let (parent, before) = self.location(None)?;
        self.document.insert_text(parent, before, text.into())?;
        self.revision += 1;
        Ok(())
    }
    fn comment(&mut self, text: &str, parent: Option<NodeId>) -> Result<(), HtmlError> {
        let parent = parent.unwrap_or(self.location(None)?.0);
        self.document.append_comment(parent, text.into())?;
        self.revision += 1;
        Ok(())
    }
    fn pop(&mut self) {
        self.open.pop();
    }
    fn pop_until(&mut self, name: &str) {
        if let Some(index) = self.open.iter().rposition(|id| self.is_html(*id, name)) {
            self.open.truncate(index);
        }
    }
    fn scope(&self, name: &str, kind: Scope) -> bool {
        for id in self.open.iter().rev() {
            if self.is_html(*id, name) {
                return true;
            }
            if self.scope_boundary(*id, kind) {
                return false;
            }
        }
        false
    }
    fn scope_boundary(&self, id: NodeId, kind: Scope) -> bool {
        let Some(element) = self.document.node(id).and_then(|node| node.as_element()) else {
            return true;
        };
        let name = element.tag_name();
        match kind {
            Scope::Table => {
                element.namespace() == &Namespace::Html
                    && matches!(name, "html" | "table" | "template")
            }
            _ => match element.namespace() {
                Namespace::Html => {
                    matches!(
                        name,
                        "applet"
                            | "caption"
                            | "html"
                            | "table"
                            | "td"
                            | "th"
                            | "marquee"
                            | "object"
                            | "select"
                            | "template"
                    ) || kind == Scope::List && matches!(name, "ol" | "ul")
                        || kind == Scope::Button && name == "button"
                }
                Namespace::MathMl => {
                    matches!(name, "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml")
                }
                Namespace::Svg => matches!(name, "foreignObject" | "desc" | "title"),
                _ => false,
            },
        }
    }
    fn implied(&mut self, except: Option<&str>, thorough: bool) {
        while self.current_name() != except.unwrap_or("")
            && (matches!(
                self.current_name(),
                "dd" | "dt" | "li" | "optgroup" | "option" | "p" | "rb" | "rp" | "rt" | "rtc"
            ) || thorough
                && matches!(
                    self.current_name(),
                    "caption" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr"
                ))
        {
            self.pop();
        }
    }
    fn close_p(&mut self) {
        self.implied(Some("p"), false);
        if self.current_name() != "p" {
            self.error("unexpected-open-element");
        }
        self.pop_until("p");
    }
    fn text_element(&mut self, tag: &TagToken, state: InitialState) -> Result<(), HtmlError> {
        self.insert(tag, Namespace::Html)?;
        if tag.name == "style" {
            self.style_text = Some(String::new());
        }
        self.original_mode = self.mode;
        self.mode = Mode::Text;
        self.text_context = Some((state, tag.name.clone()));
        Ok(())
    }
    fn flush_style(&mut self) -> Result<(), HtmlError> {
        if let Some(text) = self.style_text.take()
            && !text.is_empty()
        {
            self.text(&text)?;
        }
        Ok(())
    }
    fn reset_mode(&mut self) {
        for (index, id) in self.open.iter().enumerate().rev() {
            let last = index == 0;
            let element = if last {
                self.fragment
                    .as_ref()
                    .or_else(|| self.document.node(*id).and_then(|node| node.as_element()))
            } else {
                self.document.node(*id).and_then(|node| node.as_element())
            };
            let Some(element) = element else {
                continue;
            };
            if element.namespace() != &Namespace::Html {
                continue;
            }
            self.mode = match element.tag_name() {
                "td" | "th" if !last => Mode::InCell,
                "tr" => Mode::InRow,
                "tbody" | "thead" | "tfoot" => Mode::InTableBody,
                "caption" => Mode::InCaption,
                "colgroup" => Mode::InColumnGroup,
                "table" => Mode::InTable,
                "template" => *self.template_modes.last().unwrap_or(&Mode::InTemplate),
                "head" if !last => Mode::InHead,
                "body" => Mode::InBody,
                "frameset" => Mode::InFrameset,
                "html" => {
                    if self.head.is_none() {
                        Mode::BeforeHead
                    } else {
                        Mode::AfterHead
                    }
                }
                _ if last => Mode::InBody,
                _ => continue,
            };
            return;
        }
        self.mode = Mode::InBody;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Normal,
    List,
    Button,
    Table,
}

fn space(text: &str) -> bool {
    text.chars()
        .all(|ch| matches!(ch, '\t' | '\n' | '\u{000C}' | '\r' | ' '))
}
fn start(token: &Token, names: &[&str]) -> bool {
    matches!(token, Token::StartTag(tag) if names.contains(&tag.name.as_str()))
}
fn end(token: &Token, names: &[&str]) -> bool {
    matches!(token, Token::EndTag(tag) if names.contains(&tag.name.as_str()))
}
