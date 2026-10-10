use super::{Mode, Scope, TreeBuilder, space};
use crate::dom::{Namespace, NodeKind};
use crate::html::{
    parser::HtmlError,
    tokenizer::{InitialState, TagToken, Token},
};

impl TreeBuilder {
    pub(super) fn in_body(&mut self, token: &Token) -> Result<bool, HtmlError> {
        match token {
            Token::Character(text) => {
                if text == "\0" {
                    self.error("unexpected-null-in-body");
                    return Ok(true);
                }
                self.reconstruct()?;
                self.text(text)?;
                if !space(text) {
                    self.frameset_ok = false;
                }
            }
            Token::Comment(text) => self.comment(text, None)?,
            Token::Doctype(_) => self.error("unexpected-doctype"),
            Token::ProcessingInstruction { .. } => return Err(HtmlError::InvalidTreeState),
            Token::Eof => {
                if !self.template_modes.is_empty() {
                    self.mode = Mode::InTemplate;
                    return Ok(false);
                }
                if self.open.iter().any(|id| {
                    self.document
                        .node(*id)
                        .and_then(|node| node.as_element())
                        .is_some_and(|element| {
                            element.namespace() != &Namespace::Html
                                || !matches!(
                                    element.tag_name(),
                                    "dd" | "dt"
                                        | "li"
                                        | "optgroup"
                                        | "option"
                                        | "p"
                                        | "rb"
                                        | "rp"
                                        | "rt"
                                        | "rtc"
                                        | "tbody"
                                        | "td"
                                        | "tfoot"
                                        | "th"
                                        | "thead"
                                        | "tr"
                                        | "body"
                                        | "html"
                                )
                        })
                }) {
                    self.error("eof-with-open-elements");
                }
            }
            Token::StartTag(tag) => return self.body_start(tag),
            Token::EndTag(tag) => return self.body_end(tag),
        }
        Ok(true)
    }

    fn body_start(&mut self, tag: &TagToken) -> Result<bool, HtmlError> {
        let name = tag.name.as_str();
        let mut acknowledge = false;
        match name {
            "html" => {
                self.error("unexpected-html");
                if !self.has_template()
                    && let Some(html) = self.open.first().copied()
                {
                    self.document
                        .merge_attributes(html, Self::attributes(tag))?;
                    self.revision += 1;
                }
            }
            "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style"
            | "template" | "title" => return self.in_head(&Token::StartTag(tag.clone())),
            "body" => {
                self.error("unexpected-body");
                if self.open.len() > 1 && self.is_html(self.open[1], "body") && !self.has_template()
                {
                    self.frameset_ok = false;
                    self.document
                        .merge_attributes(self.open[1], Self::attributes(tag))?;
                    self.revision += 1;
                }
            }
            "frameset" => {
                self.error("unexpected-frameset");
                if self.frameset_ok && self.open.len() > 1 && self.is_html(self.open[1], "body") {
                    self.document.detach(self.open[1])?;
                    self.open.truncate(1);
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::InFrameset;
                }
            }
            "address" | "article" | "aside" | "blockquote" | "center" | "details" | "dialog"
            | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer" | "header"
            | "hgroup" | "main" | "menu" | "nav" | "ol" | "p" | "search" | "section"
            | "summary" | "ul" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(tag, Namespace::Html)?;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                if heading(self.current_name()) {
                    self.error("nested-heading");
                    self.pop();
                }
                self.insert(tag, Namespace::Html)?;
            }
            "pre" | "listing" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(tag, Namespace::Html)?;
                self.ignore_lf = true;
                self.frameset_ok = false;
            }
            "form" => {
                if self.form.is_some() && !self.has_template() {
                    self.error("nested-form");
                } else {
                    if self.scope("p", Scope::Button) {
                        self.close_p();
                    }
                    let id = self.insert(tag, Namespace::Html)?;
                    if !self.has_template() {
                        self.form = Some(id);
                    }
                }
            }
            "li" | "dd" | "dt" => {
                self.frameset_ok = false;
                for id in self.open.clone().iter().rev() {
                    let old = self.name(*id).to_owned();
                    if old == name || name != "li" && matches!(old.as_str(), "dd" | "dt") {
                        self.implied(Some(&old), false);
                        if self.current_name() != old {
                            self.error("unexpected-open-element");
                        }
                        self.pop_until(&old);
                        break;
                    }
                    if self.special(*id) && !matches!(old.as_str(), "address" | "div" | "p") {
                        break;
                    }
                }
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(tag, Namespace::Html)?;
            }
            "plaintext" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(tag, Namespace::Html)?;
                self.text_context = Some((InitialState::Plaintext, name.into()));
            }
            "button" => {
                if self.scope("button", Scope::Normal) {
                    self.error("nested-button");
                    self.implied(None, false);
                    self.pop_until("button");
                }
                self.reconstruct()?;
                self.insert(tag, Namespace::Html)?;
                self.frameset_ok = false;
            }
            "a" => {
                if let Some((index, id)) = self.find_formatting("a") {
                    self.error("nested-anchor");
                    self.adoption("a")?;
                    if let Some(index) = self.formatting.iter().position(|entry| *entry == Some(id))
                    {
                        self.formatting.remove(index);
                    }
                    self.open.retain(|element| *element != id);
                    let _ = index;
                }
                self.reconstruct()?;
                let id = self.insert(tag, Namespace::Html)?;
                self.add_formatting(id);
            }
            "b" | "big" | "code" | "em" | "font" | "i" | "s" | "small" | "strike" | "strong"
            | "tt" | "u" => {
                self.reconstruct()?;
                let id = self.insert(tag, Namespace::Html)?;
                self.add_formatting(id);
            }
            "nobr" => {
                self.reconstruct()?;
                if self.scope("nobr", Scope::Normal) {
                    self.error("nested-nobr");
                    self.adoption("nobr")?;
                    self.reconstruct()?;
                }
                let id = self.insert(tag, Namespace::Html)?;
                self.add_formatting(id);
            }
            "applet" | "marquee" | "object" => {
                self.reconstruct()?;
                self.insert(tag, Namespace::Html)?;
                self.formatting.push(None);
                self.frameset_ok = false;
            }
            "table" => {
                if self.document.mode() != crate::dom::DocumentMode::Quirks
                    && self.scope("p", Scope::Button)
                {
                    self.close_p();
                }
                self.insert(tag, Namespace::Html)?;
                self.frameset_ok = false;
                self.mode = Mode::InTable;
            }
            "area" | "br" | "embed" | "img" | "keygen" | "wbr" => {
                self.reconstruct()?;
                self.insert(tag, Namespace::Html)?;
                self.pop();
                self.frameset_ok = false;
                acknowledge = true;
            }
            "input" => {
                if self.fragment.as_ref().is_some_and(|context| {
                    context.namespace() == &Namespace::Html && context.tag_name() == "select"
                }) {
                    self.error("input-in-select-fragment");
                    return Ok(true);
                }
                if self.scope("select", Scope::Normal) {
                    self.error("input-in-select");
                    self.pop_until("select");
                }
                self.reconstruct()?;
                self.insert(tag, Namespace::Html)?;
                self.pop();
                acknowledge = true;
                if !tag
                    .attributes
                    .iter()
                    .any(|attr| attr.name == "type" && attr.value.eq_ignore_ascii_case("hidden"))
                {
                    self.frameset_ok = false;
                }
            }
            "param" | "source" | "track" => {
                self.insert(tag, Namespace::Html)?;
                self.pop();
                acknowledge = true;
            }
            "hr" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                if self.scope("select", Scope::Normal) {
                    self.implied(None, false);
                    if self.scope("option", Scope::Normal) || self.scope("optgroup", Scope::Normal)
                    {
                        self.error("misnested-option");
                    }
                }
                self.insert(tag, Namespace::Html)?;
                self.pop();
                self.frameset_ok = false;
                acknowledge = true;
            }
            "image" => {
                self.error("unexpected-image");
                let mut tag = tag.clone();
                tag.name = "img".into();
                return self.body_start(&tag);
            }
            "textarea" => {
                self.text_element(tag, InitialState::Rcdata)?;
                self.ignore_lf = true;
                self.frameset_ok = false;
            }
            "xmp" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.reconstruct()?;
                self.frameset_ok = false;
                self.text_element(tag, InitialState::Rawtext)?;
            }
            "iframe" => {
                self.frameset_ok = false;
                self.text_element(tag, InitialState::Rawtext)?;
            }
            "noembed" => self.text_element(tag, InitialState::Rawtext)?,
            "noscript" if self.scripting => self.text_element(tag, InitialState::Rawtext)?,
            "select" => {
                if self.fragment.as_ref().is_some_and(|context| {
                    context.namespace() == &Namespace::Html && context.tag_name() == "select"
                }) {
                    self.error("select-in-select-fragment");
                } else if self.scope("select", Scope::Normal) {
                    self.error("nested-select");
                    self.pop_until("select");
                } else {
                    self.reconstruct()?;
                    self.insert(tag, Namespace::Html)?;
                    self.frameset_ok = false;
                }
            }
            "optgroup" | "option" => {
                if self.scope("select", Scope::Normal) {
                    self.implied(
                        if name == "option" {
                            Some("optgroup")
                        } else {
                            None
                        },
                        false,
                    );
                    if self.scope("option", Scope::Normal)
                        || name == "optgroup" && self.scope("optgroup", Scope::Normal)
                    {
                        self.error("misnested-option");
                    }
                } else if self.current_name() == "option" {
                    self.pop();
                }
                self.reconstruct()?;
                self.insert(tag, Namespace::Html)?;
            }
            "rb" | "rtc" => {
                if self.scope("ruby", Scope::Normal) {
                    self.implied(None, false);
                    if self.current_name() != "ruby" {
                        self.error("unexpected-open-element");
                    }
                }
                self.insert(tag, Namespace::Html)?;
            }
            "rp" | "rt" => {
                if self.scope("ruby", Scope::Normal) {
                    self.implied(Some("rtc"), false);
                    if !matches!(self.current_name(), "rtc" | "ruby") {
                        self.error("unexpected-open-element");
                    }
                }
                self.insert(tag, Namespace::Html)?;
            }
            "math" | "svg" => {
                self.reconstruct()?;
                self.insert(
                    tag,
                    if name == "math" {
                        Namespace::MathMl
                    } else {
                        Namespace::Svg
                    },
                )?;
                if tag.self_closing {
                    self.pop();
                    acknowledge = true;
                }
            }
            "caption" | "col" | "colgroup" | "frame" | "head" | "tbody" | "td" | "tfoot" | "th"
            | "thead" | "tr" => self.error("unexpected-start-tag"),
            _ => {
                self.reconstruct()?;
                self.insert(tag, Namespace::Html)?;
            }
        }
        if tag.self_closing && !acknowledge {
            self.error("non-void-html-element-start-tag-with-trailing-solidus");
        }
        Ok(true)
    }

    fn body_end(&mut self, tag: &TagToken) -> Result<bool, HtmlError> {
        let name = tag.name.as_str();
        match name {
            "template" => return self.in_head(&Token::EndTag(tag.clone())),
            "body" | "html" => {
                if !self.scope("body", Scope::Normal) {
                    self.error("unexpected-end-tag");
                } else {
                    if self.open.iter().any(|id| {
                        !matches!(
                            self.name(*id),
                            "dd" | "dt"
                                | "li"
                                | "optgroup"
                                | "option"
                                | "p"
                                | "rb"
                                | "rp"
                                | "rt"
                                | "rtc"
                                | "tbody"
                                | "td"
                                | "tfoot"
                                | "th"
                                | "thead"
                                | "tr"
                                | "body"
                                | "html"
                        )
                    }) {
                        self.error("unexpected-open-element");
                    }
                    self.mode = Mode::AfterBody;
                    if name == "html" {
                        return Ok(false);
                    }
                }
            }
            "address" | "article" | "aside" | "blockquote" | "button" | "center" | "details"
            | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer"
            | "header" | "hgroup" | "listing" | "main" | "menu" | "nav" | "ol" | "pre"
            | "search" | "section" | "select" | "summary" | "ul" => {
                if !self.scope(name, Scope::Normal) {
                    self.error("unexpected-end-tag");
                } else {
                    self.implied(None, false);
                    if self.current_name() != name {
                        self.error("unexpected-open-element");
                    }
                    self.pop_until(name);
                }
            }
            "form" if !self.has_template() => {
                let form = self.form.take();
                if form
                    .is_none_or(|id| !self.open.contains(&id) || !self.scope("form", Scope::Normal))
                {
                    self.error("unexpected-end-tag");
                } else {
                    self.implied(None, false);
                    if self.current() != form {
                        self.error("unexpected-open-element");
                    }
                    self.open.retain(|id| Some(*id) != form);
                }
            }
            "form" => {
                if !self.scope("form", Scope::Normal) {
                    self.error("unexpected-end-tag");
                } else {
                    self.implied(None, false);
                    if self.current_name() != "form" {
                        self.error("unexpected-open-element");
                    }
                    self.pop_until("form");
                }
            }
            "p" => {
                if !self.scope("p", Scope::Button) {
                    self.error("unexpected-end-tag");
                    self.insert_named("p")?;
                }
                self.close_p();
            }
            "li" | "dd" | "dt" => {
                if !self.scope(
                    name,
                    if name == "li" {
                        Scope::List
                    } else {
                        Scope::Normal
                    },
                ) {
                    self.error("unexpected-end-tag");
                } else {
                    self.implied(Some(name), false);
                    if self.current_name() != name {
                        self.error("unexpected-open-element");
                    }
                    self.pop_until(name);
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if !["h1", "h2", "h3", "h4", "h5", "h6"]
                    .iter()
                    .any(|name| self.scope(name, Scope::Normal))
                {
                    self.error("unexpected-end-tag");
                } else {
                    self.implied(None, false);
                    if self.current_name() != name {
                        self.error("unexpected-open-element");
                    }
                    while let Some(id) = self.open.pop() {
                        if heading(self.name(id)) {
                            break;
                        }
                    }
                }
            }
            "a" | "b" | "big" | "code" | "em" | "font" | "i" | "nobr" | "s" | "small"
            | "strike" | "strong" | "tt" | "u" => self.adoption(name)?,
            "applet" | "marquee" | "object" => {
                if !self.scope(name, Scope::Normal) {
                    self.error("unexpected-end-tag");
                } else {
                    self.implied(None, false);
                    if self.current_name() != name {
                        self.error("unexpected-open-element");
                    }
                    self.pop_until(name);
                    self.clear_formatting();
                }
            }
            "br" => {
                self.error("unexpected-end-tag");
                let tag = TagToken {
                    name: "br".into(),
                    attributes: Vec::new(),
                    self_closing: false,
                };
                return self.body_start(&tag);
            }
            _ => self.generic_end(name),
        }
        Ok(true)
    }

    pub(super) fn generic_end(&mut self, name: &str) {
        for index in (0..self.open.len()).rev() {
            let id = self.open[index];
            if self.is_html(id, name) {
                self.implied(Some(name), false);
                if self.current() != Some(id) {
                    self.error("unexpected-open-element");
                }
                self.open.truncate(index);
                return;
            }
            if self.special(id) {
                self.error("unexpected-end-tag");
                return;
            }
        }
        self.error("unexpected-end-tag");
    }

    pub(super) fn special(&self, id: crate::dom::NodeId) -> bool {
        let Some(NodeKind::Element(element)) = self.document.node(id).map(|node| node.kind())
        else {
            return false;
        };
        match element.namespace() {
            Namespace::Html => matches!(
                element.tag_name(),
                "address"
                    | "applet"
                    | "area"
                    | "article"
                    | "aside"
                    | "base"
                    | "basefont"
                    | "bgsound"
                    | "blockquote"
                    | "body"
                    | "br"
                    | "button"
                    | "caption"
                    | "center"
                    | "col"
                    | "colgroup"
                    | "dd"
                    | "details"
                    | "dir"
                    | "div"
                    | "dl"
                    | "dt"
                    | "embed"
                    | "fieldset"
                    | "figcaption"
                    | "figure"
                    | "footer"
                    | "form"
                    | "frame"
                    | "frameset"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "head"
                    | "header"
                    | "hgroup"
                    | "hr"
                    | "html"
                    | "iframe"
                    | "img"
                    | "input"
                    | "keygen"
                    | "li"
                    | "link"
                    | "listing"
                    | "main"
                    | "marquee"
                    | "menu"
                    | "meta"
                    | "nav"
                    | "noembed"
                    | "noframes"
                    | "noscript"
                    | "object"
                    | "ol"
                    | "p"
                    | "param"
                    | "plaintext"
                    | "pre"
                    | "script"
                    | "search"
                    | "section"
                    | "select"
                    | "source"
                    | "style"
                    | "summary"
                    | "table"
                    | "tbody"
                    | "td"
                    | "template"
                    | "textarea"
                    | "tfoot"
                    | "th"
                    | "thead"
                    | "title"
                    | "tr"
                    | "track"
                    | "ul"
                    | "wbr"
                    | "xmp"
            ),
            Namespace::MathMl => matches!(
                element.tag_name(),
                "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml"
            ),
            Namespace::Svg => matches!(element.tag_name(), "foreignObject" | "desc" | "title"),
            _ => false,
        }
    }
}

fn heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}
