use super::{Mode, TreeBuilder, end, space, start};
use crate::dom::{DoctypeData, DocumentMode, Namespace};
use crate::html::{
    parser::HtmlError,
    tokenizer::{DoctypeToken, InitialState, Token},
};

impl TreeBuilder {
    pub(super) fn other_mode(&mut self, token: &Token) -> Result<bool, HtmlError> {
        match self.mode {
            Mode::Initial => match token {
                Token::Character(text) if space(text) => {}
                Token::Comment(text) => self.comment(text, Some(self.document.root_id()))?,
                Token::Doctype(data) => {
                    if data.name.as_deref() != Some("html")
                        || data.public_identifier.is_some()
                        || data
                            .system_identifier
                            .as_deref()
                            .is_some_and(|id| id != "about:legacy-compat")
                    {
                        self.error("non-conforming-doctype");
                    }
                    self.document.append_doctype(
                        self.document.root_id(),
                        DoctypeData {
                            name: data.name.clone().unwrap_or_default(),
                            public_identifier: data.public_identifier.clone().unwrap_or_default(),
                            system_identifier: data.system_identifier.clone().unwrap_or_default(),
                        },
                    )?;
                    self.document.set_mode(doctype_mode(data));
                    self.revision += 1;
                    self.mode = Mode::BeforeHtml;
                }
                _ => {
                    self.error("missing-doctype");
                    self.document.set_mode(DocumentMode::Quirks);
                    self.mode = Mode::BeforeHtml;
                    return Ok(false);
                }
            },
            Mode::BeforeHtml => match token {
                Token::Doctype(_) => self.error("unexpected-doctype"),
                Token::Comment(text) => self.comment(text, Some(self.document.root_id()))?,
                Token::Character(text) if space(text) => {}
                Token::StartTag(tag) if tag.name == "html" => {
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::BeforeHead;
                }
                Token::EndTag(_) if !end(token, &["head", "body", "html", "br"]) => {
                    self.error("unexpected-end-tag")
                }
                _ => {
                    self.insert_named("html")?;
                    self.mode = Mode::BeforeHead;
                    return Ok(false);
                }
            },
            Mode::BeforeHead => match token {
                Token::Character(text) if space(text) => {}
                Token::Comment(text) => self.comment(text, None)?,
                Token::Doctype(_) => self.error("unexpected-doctype"),
                Token::StartTag(tag) if tag.name == "html" => return self.in_body(token),
                Token::StartTag(tag) if tag.name == "head" => {
                    self.head = Some(self.insert(tag, Namespace::Html)?);
                    self.mode = Mode::InHead;
                }
                Token::EndTag(_) if !end(token, &["head", "body", "html", "br"]) => {
                    self.error("unexpected-end-tag")
                }
                _ => {
                    self.head = Some(self.insert_named("head")?);
                    self.mode = Mode::InHead;
                    return Ok(false);
                }
            },
            Mode::InHead => return self.in_head(token),
            Mode::InHeadNoscript => match token {
                Token::Doctype(_) => self.error("unexpected-doctype"),
                Token::StartTag(tag) if tag.name == "html" => return self.in_body(token),
                Token::EndTag(tag) if tag.name == "noscript" => {
                    self.pop();
                    self.mode = Mode::InHead;
                }
                Token::Character(text) if space(text) => return self.in_head(token),
                Token::Comment(_) => return self.in_head(token),
                _ if start(
                    token,
                    &["basefont", "bgsound", "link", "meta", "noframes", "style"],
                ) =>
                {
                    return self.in_head(token);
                }
                _ if start(token, &["head", "noscript"])
                    || matches!(token, Token::EndTag(_)) && !end(token, &["br"]) =>
                {
                    self.error("unexpected-token-in-noscript")
                }
                _ => {
                    self.error("unexpected-token-in-noscript");
                    self.pop();
                    self.mode = Mode::InHead;
                    return Ok(false);
                }
            },
            Mode::AfterHead => match token {
                Token::Character(text) if space(text) => self.text(text)?,
                Token::Comment(text) => self.comment(text, None)?,
                Token::Doctype(_) => self.error("unexpected-doctype"),
                Token::StartTag(tag) if tag.name == "html" => return self.in_body(token),
                Token::StartTag(tag) if tag.name == "body" => {
                    self.insert(tag, Namespace::Html)?;
                    self.frameset_ok = false;
                    self.mode = Mode::InBody;
                }
                Token::StartTag(tag) if tag.name == "frameset" => {
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::InFrameset;
                }
                _ if start(
                    token,
                    &[
                        "base", "basefont", "bgsound", "link", "meta", "noframes", "script",
                        "style", "template", "title",
                    ],
                ) =>
                {
                    self.error("unexpected-token-after-head");
                    let head = self.head.ok_or(HtmlError::InvalidTreeState)?;
                    self.open.push(head);
                    let handled = self.in_head(token)?;
                    if let Some(index) = self.open.iter().position(|id| *id == head) {
                        self.open.remove(index);
                    }
                    return Ok(handled);
                }
                _ if end(token, &["template"]) => return self.in_head(token),
                _ if start(token, &["head"])
                    || matches!(token, Token::EndTag(_))
                        && !end(token, &["body", "html", "br"]) =>
                {
                    self.error("unexpected-token-after-head")
                }
                _ => {
                    self.insert_named("body")?;
                    self.mode = Mode::InBody;
                    return Ok(false);
                }
            },
            Mode::Text => match token {
                Token::Character(text) => self.text(text)?,
                Token::Eof => {
                    self.error("eof-in-text");
                    self.flush_style()?;
                    self.pop();
                    self.mode = self.original_mode;
                    return Ok(false);
                }
                Token::EndTag(_) => {
                    self.flush_style()?;
                    self.pop();
                    self.mode = self.original_mode;
                }
                _ => return Err(HtmlError::InvalidTreeState),
            },
            Mode::InTemplate => {
                if matches!(
                    token,
                    Token::Character(_) | Token::Comment(_) | Token::Doctype(_)
                ) {
                    return self.in_body(token);
                }
                if start(
                    token,
                    &[
                        "base", "basefont", "bgsound", "link", "meta", "noframes", "script",
                        "style", "template", "title",
                    ],
                ) || end(token, &["template"])
                {
                    return self.in_head(token);
                }
                if matches!(token, Token::EndTag(_)) {
                    self.error("unexpected-end-tag-in-template");
                } else if matches!(token, Token::Eof) {
                    if !self.has_template() {
                        return Ok(true);
                    }
                    self.error("eof-in-template");
                    self.pop_until("template");
                    self.clear_formatting();
                    self.template_modes.pop();
                    self.reset_mode();
                    return Ok(false);
                } else {
                    let mode = if start(token, &["caption", "colgroup", "tbody", "tfoot", "thead"])
                    {
                        Mode::InTable
                    } else if start(token, &["col"]) {
                        Mode::InColumnGroup
                    } else if start(token, &["tr"]) {
                        Mode::InTableBody
                    } else if start(token, &["td", "th"]) {
                        Mode::InRow
                    } else {
                        Mode::InBody
                    };
                    if let Some(current) = self.template_modes.last_mut() {
                        *current = mode;
                    }
                    self.mode = mode;
                    return Ok(false);
                }
            }
            Mode::AfterBody | Mode::AfterAfterBody => match token {
                Token::Character(text) if space(text) => return self.in_body(token),
                Token::Comment(text) => {
                    let parent = if self.mode == Mode::AfterAfterBody {
                        self.document.root_id()
                    } else {
                        *self.open.first().ok_or(HtmlError::InvalidTreeState)?
                    };
                    self.comment(text, Some(parent))?;
                }
                Token::Doctype(_) => return self.in_body(token),
                _ if start(token, &["html"]) => return self.in_body(token),
                _ if end(token, &["html"]) && self.mode == Mode::AfterBody => {
                    if self.fragment.is_some() {
                        self.error("unexpected-end-tag");
                    } else {
                        self.mode = Mode::AfterAfterBody;
                    }
                }
                Token::Eof => {}
                _ => {
                    self.error("unexpected-token-after-body");
                    self.mode = Mode::InBody;
                    return Ok(false);
                }
            },
            Mode::InFrameset | Mode::AfterFrameset | Mode::AfterAfterFrameset => {
                return self.in_frameset(token);
            }
            _ => return Err(HtmlError::InvalidTreeState),
        }
        Ok(true)
    }

    pub(super) fn in_head(&mut self, token: &Token) -> Result<bool, HtmlError> {
        match token {
            Token::Character(text) if space(text) => self.text(text)?,
            Token::Comment(text) => self.comment(text, None)?,
            Token::Doctype(_) => self.error("unexpected-doctype"),
            Token::StartTag(tag) => match tag.name.as_str() {
                "html" => return self.in_body(token),
                "base" | "basefont" | "bgsound" | "link" | "meta" => {
                    if tag.name == "meta" && self.encoding_declaration.is_none() {
                        self.encoding_declaration =
                            crate::html::encoding::meta_encoding(tag.attributes.iter().map(
                                |attribute| (attribute.name.as_str(), attribute.value.as_str()),
                            ));
                    }
                    self.insert(tag, Namespace::Html)?;
                    self.pop();
                }
                "title" => self.text_element(tag, InitialState::Rcdata)?,
                "noframes" | "style" => self.text_element(tag, InitialState::Rawtext)?,
                "noscript" if self.scripting => self.text_element(tag, InitialState::Rawtext)?,
                "noscript" => {
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::InHeadNoscript;
                }
                "script" => self.text_element(tag, InitialState::ScriptData)?,
                "template" => {
                    self.insert(tag, Namespace::Html)?;
                    self.formatting.push(None);
                    self.frameset_ok = false;
                    self.mode = Mode::InTemplate;
                    self.template_modes.push(Mode::InTemplate);
                }
                "head" => self.error("unexpected-head"),
                _ => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                    return Ok(false);
                }
            },
            Token::EndTag(tag) => match tag.name.as_str() {
                "head" => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                }
                "template" => {
                    if !self.has_template() {
                        self.error("unexpected-end-tag");
                    } else {
                        self.implied(None, true);
                        if self.current_name() != "template" {
                            self.error("unexpected-open-element");
                        }
                        self.pop_until("template");
                        self.clear_formatting();
                        self.template_modes.pop();
                        self.reset_mode();
                    }
                }
                "body" | "html" | "br" => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                    return Ok(false);
                }
                _ => self.error("unexpected-end-tag"),
            },
            _ => {
                self.pop();
                self.mode = Mode::AfterHead;
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn in_frameset(&mut self, token: &Token) -> Result<bool, HtmlError> {
        match token {
            Token::Character(text) if space(text) => self.text(text)?,
            Token::Comment(text) => self.comment(
                text,
                if self.mode == Mode::AfterAfterFrameset {
                    Some(self.document.root_id())
                } else {
                    None
                },
            )?,
            Token::Doctype(_) => self.error("unexpected-doctype"),
            _ if start(token, &["html"]) => return self.in_body(token),
            Token::StartTag(tag) if tag.name == "frameset" && self.mode == Mode::InFrameset => {
                self.insert(tag, Namespace::Html)?;
            }
            Token::EndTag(tag) if tag.name == "frameset" && self.mode == Mode::InFrameset => {
                if self.current_name() == "html" {
                    self.error("unexpected-end-tag");
                } else {
                    self.pop();
                    if self.fragment.is_none() && self.current_name() != "frameset" {
                        self.mode = Mode::AfterFrameset;
                    }
                }
            }
            Token::StartTag(tag) if tag.name == "frame" && self.mode == Mode::InFrameset => {
                self.insert(tag, Namespace::Html)?;
                self.pop();
            }
            _ if start(token, &["noframes"]) => return self.in_head(token),
            _ if end(token, &["html"]) && self.mode == Mode::AfterFrameset => {
                self.mode = Mode::AfterAfterFrameset
            }
            Token::Eof => {
                if self.mode == Mode::InFrameset && self.current_name() != "html" {
                    self.error("eof-in-frameset");
                }
            }
            _ => self.error("unexpected-token-in-frameset"),
        }
        Ok(true)
    }
}

fn doctype_mode(data: &DoctypeToken) -> DocumentMode {
    let public = data
        .public_identifier
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let system = data
        .system_identifier
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if data.force_quirks
        || data.name.as_deref() != Some("html")
        || matches!(
            public.as_str(),
            "-//w3o//dtd w3 html strict 3.0//en//" | "-/w3c/dtd html 4.0 transitional/en" | "html"
        )
        || system == "http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd"
        || QUIRKS_PREFIXES
            .iter()
            .any(|prefix| public.starts_with(prefix))
        || data.system_identifier.is_none()
            && [
                "-//w3c//dtd html 4.01 frameset//",
                "-//w3c//dtd html 4.01 transitional//",
            ]
            .iter()
            .any(|prefix| public.starts_with(prefix))
    {
        DocumentMode::Quirks
    } else if [
        "-//w3c//dtd xhtml 1.0 frameset//",
        "-//w3c//dtd xhtml 1.0 transitional//",
    ]
    .iter()
    .any(|prefix| public.starts_with(prefix))
        || data.system_identifier.is_some()
            && [
                "-//w3c//dtd html 4.01 frameset//",
                "-//w3c//dtd html 4.01 transitional//",
            ]
            .iter()
            .any(|prefix| public.starts_with(prefix))
    {
        DocumentMode::LimitedQuirks
    } else {
        DocumentMode::NoQuirks
    }
}

// HTML 13.2.6.4.1's ASCII-case-insensitive legacy public identifier prefixes.
const QUIRKS_PREFIXES: &[&str] = &[
    "+//silmaril//dtd html pro v0r11 19970101//",
    "-//as//dtd html 3.0 aswedit + extensions//",
    "-//advasoft ltd//dtd html 3.0 aswedit + extensions//",
    "-//ietf//dtd html 2.0 level 1//",
    "-//ietf//dtd html 2.0 level 2//",
    "-//ietf//dtd html 2.0 strict level 1//",
    "-//ietf//dtd html 2.0 strict level 2//",
    "-//ietf//dtd html 2.0 strict//",
    "-//ietf//dtd html 2.0//",
    "-//ietf//dtd html 2.1e//",
    "-//ietf//dtd html 3.0//",
    "-//ietf//dtd html 3.2 final//",
    "-//ietf//dtd html 3.2//",
    "-//ietf//dtd html 3//",
    "-//ietf//dtd html level 0//",
    "-//ietf//dtd html level 1//",
    "-//ietf//dtd html level 2//",
    "-//ietf//dtd html level 3//",
    "-//ietf//dtd html strict level 0//",
    "-//ietf//dtd html strict level 1//",
    "-//ietf//dtd html strict level 2//",
    "-//ietf//dtd html strict level 3//",
    "-//ietf//dtd html strict//",
    "-//ietf//dtd html//",
    "-//metrius//dtd metrius presentational//",
    "-//microsoft//dtd internet explorer 2.0 html strict//",
    "-//microsoft//dtd internet explorer 2.0 html//",
    "-//microsoft//dtd internet explorer 2.0 tables//",
    "-//microsoft//dtd internet explorer 3.0 html strict//",
    "-//microsoft//dtd internet explorer 3.0 html//",
    "-//microsoft//dtd internet explorer 3.0 tables//",
    "-//netscape comm. corp.//dtd html//",
    "-//netscape comm. corp.//dtd strict html//",
    "-//o'reilly and associates//dtd html 2.0//",
    "-//o'reilly and associates//dtd html extended 1.0//",
    "-//o'reilly and associates//dtd html extended relaxed 1.0//",
    "-//softquad software//dtd hotmetal pro 6.0::19990601::extensions to html 4.0//",
    "-//softquad//dtd hotmetal pro 4.0::19971010::extensions to html 4.0//",
    "-//spyglass//dtd html 2.0 extended//",
    "-//sq//dtd html 2.0 hotmetal + extensions//",
    "-//sun microsystems corp.//dtd hotjava html//",
    "-//sun microsystems corp.//dtd hotjava strict html//",
    "-//w3c//dtd html 3 1995-03-24//",
    "-//w3c//dtd html 3.2 draft//",
    "-//w3c//dtd html 3.2 final//",
    "-//w3c//dtd html 3.2//",
    "-//w3c//dtd html 3.2s draft//",
    "-//w3c//dtd html 4.0 frameset//",
    "-//w3c//dtd html 4.0 transitional//",
    "-//w3c//dtd html experimental 19960712//",
    "-//w3c//dtd html experimental 970421//",
    "-//w3c//dtd w3 html//",
    "-//w3o//dtd w3 html 3.0//",
    "-//webtechs//dtd mozilla html 2.0//",
    "-//webtechs//dtd mozilla html//",
];
