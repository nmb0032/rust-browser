use super::{Mode, Scope, TreeBuilder, end, space, start};
use crate::dom::Namespace;
use crate::html::{parser::HtmlError, tokenizer::Token};

impl TreeBuilder {
    pub(super) fn table_mode(&mut self, token: &Token) -> Result<bool, HtmlError> {
        match self.mode {
            Mode::InTable => return self.in_table(token),
            Mode::InTableText => {
                if let Token::Character(text) = token {
                    if text == "\0" {
                        self.error("unexpected-null-in-table");
                    } else {
                        self.table_text.push_str(text);
                        self.table_positions.push(self.position);
                    }
                } else {
                    let text = std::mem::take(&mut self.table_text);
                    let positions = std::mem::take(&mut self.table_positions);
                    if !text.is_empty() {
                        let position = self.position;
                        self.position = positions[0];
                        if space(&text) {
                            self.text(&text)?;
                        } else {
                            self.error("non-space-in-table");
                            for (ch, position) in text.chars().zip(positions) {
                                self.position = position;
                                self.foster_body(&Token::Character(ch.to_string()))?;
                            }
                        }
                        self.position = position;
                    }
                    self.mode = self.original_mode;
                    return Ok(false);
                }
            }
            Mode::InCaption => {
                if end(token, &["caption"])
                    || start(
                        token,
                        &[
                            "caption", "col", "colgroup", "tbody", "td", "tfoot", "th", "thead",
                            "tr",
                        ],
                    )
                    || end(token, &["table"])
                {
                    if !self.scope("caption", Scope::Table) {
                        self.error("unexpected-table-token");
                    } else {
                        self.implied(None, false);
                        if self.current_name() != "caption" {
                            self.error("unexpected-open-element");
                        }
                        self.pop_until("caption");
                        self.clear_formatting();
                        self.mode = Mode::InTable;
                        if !end(token, &["caption"]) {
                            return Ok(false);
                        }
                    }
                } else if end(
                    token,
                    &[
                        "body", "col", "colgroup", "html", "tbody", "td", "tfoot", "th", "thead",
                        "tr",
                    ],
                ) {
                    self.error("unexpected-end-tag");
                } else {
                    return self.in_body(token);
                }
            }
            Mode::InColumnGroup => match token {
                Token::Character(text) if space(text) => self.text(text)?,
                Token::Comment(text) => self.comment(text, None)?,
                Token::Doctype(_) => self.error("unexpected-doctype"),
                _ if start(token, &["html"]) => return self.in_body(token),
                Token::StartTag(tag) if tag.name == "col" => {
                    self.insert(tag, Namespace::Html)?;
                    self.pop();
                    self.self_closing_acknowledged = true;
                }
                _ if end(token, &["colgroup"]) => {
                    if self.current_name() != "colgroup" {
                        self.error("unexpected-end-tag");
                    } else {
                        self.pop();
                        self.mode = Mode::InTable;
                    }
                }
                _ if end(token, &["col"]) => self.error("unexpected-end-tag"),
                _ if start(token, &["template"]) || end(token, &["template"]) => {
                    return self.in_head(token);
                }
                Token::Eof => return self.in_body(token),
                _ => {
                    if self.current_name() != "colgroup" {
                        self.error("unexpected-token-in-colgroup");
                    } else {
                        self.pop();
                        self.mode = Mode::InTable;
                        return Ok(false);
                    }
                }
            },
            Mode::InTableBody => {
                if let Token::StartTag(tag) = token
                    && tag.name == "tr"
                {
                    self.clear_table_context(&["tbody", "tfoot", "thead"]);
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::InRow;
                } else if start(token, &["td", "th"]) {
                    self.error("cell-without-row");
                    self.clear_table_context(&["tbody", "tfoot", "thead"]);
                    self.insert_named("tr")?;
                    self.mode = Mode::InRow;
                    return Ok(false);
                } else if let Token::EndTag(tag) = token
                    && matches!(tag.name.as_str(), "tbody" | "tfoot" | "thead")
                {
                    if !self.scope(&tag.name, Scope::Table) {
                        self.error("unexpected-end-tag");
                    } else {
                        self.clear_table_context(&["tbody", "tfoot", "thead"]);
                        self.pop();
                        self.mode = Mode::InTable;
                    }
                } else if start(
                    token,
                    &["caption", "col", "colgroup", "tbody", "tfoot", "thead"],
                ) || end(token, &["table"])
                {
                    if !["tbody", "thead", "tfoot"]
                        .iter()
                        .any(|name| self.scope(name, Scope::Table))
                    {
                        self.error("unexpected-table-token");
                    } else {
                        self.clear_table_context(&["tbody", "tfoot", "thead"]);
                        self.pop();
                        self.mode = Mode::InTable;
                        return Ok(false);
                    }
                } else if end(
                    token,
                    &[
                        "body", "caption", "col", "colgroup", "html", "td", "th", "tr",
                    ],
                ) {
                    self.error("unexpected-end-tag");
                } else {
                    return self.in_table(token);
                }
            }
            Mode::InRow => {
                if let Token::StartTag(tag) = token
                    && matches!(tag.name.as_str(), "td" | "th")
                {
                    self.clear_table_context(&["tr"]);
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::InCell;
                    self.formatting.push(None);
                } else if end(token, &["tr"])
                    || start(
                        token,
                        &[
                            "caption", "col", "colgroup", "tbody", "tfoot", "thead", "tr",
                        ],
                    )
                    || end(token, &["table", "tbody", "tfoot", "thead"])
                {
                    if let Token::EndTag(tag) = token
                        && matches!(tag.name.as_str(), "tbody" | "tfoot" | "thead")
                        && !self.scope(&tag.name, Scope::Table)
                    {
                        self.error("unexpected-end-tag");
                        return Ok(true);
                    }
                    if !self.scope("tr", Scope::Table) {
                        self.error("unexpected-table-token");
                    } else {
                        self.clear_table_context(&["tr"]);
                        self.pop();
                        self.mode = Mode::InTableBody;
                        if !end(token, &["tr"]) {
                            return Ok(false);
                        }
                    }
                } else if end(
                    token,
                    &["body", "caption", "col", "colgroup", "html", "td", "th"],
                ) {
                    self.error("unexpected-end-tag");
                } else {
                    return self.in_table(token);
                }
            }
            Mode::InCell => {
                if let Token::EndTag(tag) = token
                    && matches!(tag.name.as_str(), "td" | "th")
                {
                    if !self.scope(&tag.name, Scope::Table) {
                        self.error("unexpected-end-tag");
                    } else {
                        self.implied(None, false);
                        if self.current_name() != tag.name {
                            self.error("unexpected-open-element");
                        }
                        self.pop_until(&tag.name);
                        self.clear_formatting();
                        self.mode = Mode::InRow;
                    }
                } else if end(token, &["body", "caption", "col", "colgroup", "html"]) {
                    self.error("unexpected-end-tag");
                } else if start(
                    token,
                    &[
                        "caption", "col", "colgroup", "tbody", "td", "tfoot", "th", "thead", "tr",
                    ],
                ) || end(token, &["table", "tbody", "tfoot", "thead", "tr"])
                {
                    if let Token::EndTag(tag) = token
                        && !self.scope(&tag.name, Scope::Table)
                    {
                        self.error("unexpected-end-tag");
                    } else if self.scope("td", Scope::Table) || self.scope("th", Scope::Table) {
                        self.close_cell();
                        return Ok(false);
                    } else {
                        self.error("unexpected-table-token");
                    }
                } else {
                    return self.in_body(token);
                }
            }
            _ => return Err(HtmlError::InvalidTreeState),
        }
        Ok(true)
    }

    fn in_table(&mut self, token: &Token) -> Result<bool, HtmlError> {
        match token {
            Token::Character(_)
                if matches!(
                    self.current_name(),
                    "table" | "tbody" | "tfoot" | "thead" | "tr"
                ) =>
            {
                self.table_text.clear();
                self.table_positions.clear();
                self.original_mode = self.mode;
                self.mode = Mode::InTableText;
                return Ok(false);
            }
            Token::Comment(text) => self.comment(text, None)?,
            Token::Doctype(_) => self.error("unexpected-doctype"),
            Token::StartTag(tag) => match tag.name.as_str() {
                "caption" => {
                    self.clear_table_context(&["table"]);
                    self.formatting.push(None);
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::InCaption;
                }
                "colgroup" => {
                    self.clear_table_context(&["table"]);
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::InColumnGroup;
                }
                "col" => {
                    self.clear_table_context(&["table"]);
                    self.insert_named("colgroup")?;
                    self.mode = Mode::InColumnGroup;
                    return Ok(false);
                }
                "tbody" | "tfoot" | "thead" => {
                    self.clear_table_context(&["table"]);
                    self.insert(tag, Namespace::Html)?;
                    self.mode = Mode::InTableBody;
                }
                "td" | "th" | "tr" => {
                    self.clear_table_context(&["table"]);
                    self.insert_named("tbody")?;
                    self.mode = Mode::InTableBody;
                    return Ok(false);
                }
                "table" => {
                    self.error("nested-table");
                    if self.scope("table", Scope::Table) {
                        self.pop_until("table");
                        self.reset_mode();
                        return Ok(false);
                    }
                }
                "style" | "script" | "template" => return self.in_head(token),
                "input"
                    if tag.attributes.iter().any(|attr| {
                        attr.name == "type" && attr.value.eq_ignore_ascii_case("hidden")
                    }) =>
                {
                    self.error("hidden-input-in-table");
                    self.insert(tag, Namespace::Html)?;
                    self.pop();
                    self.self_closing_acknowledged = true;
                }
                "form" => {
                    self.error("form-in-table");
                    if self.form.is_none() || self.has_template() {
                        let form = self.insert(tag, Namespace::Html)?;
                        if !self.has_template() {
                            self.form = Some(form);
                        }
                        self.pop();
                    }
                }
                _ => return self.foster_body(token),
            },
            Token::EndTag(tag) => match tag.name.as_str() {
                "table" => {
                    if !self.scope("table", Scope::Table) {
                        self.error("unexpected-end-tag");
                    } else {
                        self.pop_until("table");
                        self.reset_mode();
                    }
                }
                "body" | "caption" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot"
                | "th" | "thead" | "tr" => self.error("unexpected-end-tag"),
                "template" => return self.in_head(token),
                _ => return self.foster_body(token),
            },
            Token::Eof => return self.in_body(token),
            _ => return self.foster_body(token),
        }
        Ok(true)
    }

    fn foster_body(&mut self, token: &Token) -> Result<bool, HtmlError> {
        self.error("unexpected-token-in-table");
        self.foster = true;
        let result = self.in_body(token);
        self.foster = false;
        result
    }
    fn clear_table_context(&mut self, names: &[&str]) {
        while !self.open.is_empty()
            && !names.contains(&self.current_name())
            && !matches!(self.current_name(), "html" | "template")
        {
            self.pop();
        }
    }
    fn close_cell(&mut self) {
        self.implied(None, false);
        if !matches!(self.current_name(), "td" | "th") {
            self.error("unexpected-open-element");
        }
        while let Some(id) = self.open.pop() {
            if self.is_html(id, "td") || self.is_html(id, "th") {
                break;
            }
        }
        self.clear_formatting();
        self.mode = Mode::InRow;
    }
}
