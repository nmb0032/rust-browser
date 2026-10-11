use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Link,
    AnyLink,
    Visited,
    Enabled,
    Disabled,
    Checked,
    Default,
    Required,
    Optional,
    ReadOnly,
    ReadWrite,
    PlaceholderShown,
    Indeterminate,
    Hover,
    Active,
    Focus,
    FocusVisible,
    FocusWithin,
    Target,
}

impl State {
    pub(super) fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "link" => Self::Link,
            "any-link" => Self::AnyLink,
            "visited" => Self::Visited,
            "enabled" => Self::Enabled,
            "disabled" => Self::Disabled,
            "checked" => Self::Checked,
            "default" => Self::Default,
            "required" => Self::Required,
            "optional" => Self::Optional,
            "read-only" => Self::ReadOnly,
            "read-write" => Self::ReadWrite,
            "placeholder-shown" => Self::PlaceholderShown,
            "indeterminate" => Self::Indeterminate,
            "hover" => Self::Hover,
            "active" => Self::Active,
            "focus" => Self::Focus,
            "focus-visible" => Self::FocusVisible,
            "focus-within" => Self::FocusWithin,
            "target" => Self::Target,
            _ => return None,
        })
    }

    pub(super) fn matches(
        self,
        document: &Document,
        id: NodeId,
        e: &ElementData,
        context: MatchContext<'_>,
    ) -> bool {
        let html = !e.has_case_sensitive_names();
        let tag = e.tag_name().to_ascii_lowercase();
        let input_type = normalized_input_type(e);
        let control = html
            && matches!(
                tag.as_str(),
                "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
            );
        let required_control = html
            && (matches!(tag.as_str(), "select" | "textarea")
                || tag == "input"
                    && matches!(
                        input_type.as_str(),
                        "text"
                            | "search"
                            | "url"
                            | "tel"
                            | "email"
                            | "password"
                            | "date"
                            | "month"
                            | "week"
                            | "time"
                            | "datetime-local"
                            | "number"
                            | "checkbox"
                            | "radio"
                            | "file"
                    ));
        let editable_control = html
            && (tag == "textarea"
                || tag == "input"
                    && matches!(
                        input_type.as_str(),
                        "text"
                            | "search"
                            | "url"
                            | "tel"
                            | "email"
                            | "password"
                            | "date"
                            | "month"
                            | "week"
                            | "time"
                            | "datetime-local"
                            | "number"
                    ));
        match self {
            Self::Link | Self::AnyLink => {
                html && matches!(tag.as_str(), "a" | "area") && e.attribute("href").is_some()
            }
            // Deliberately no history oracle, including inside :has()/:not().
            Self::Visited => false,
            Self::Enabled => control && !disabled(document, id, e, context),
            Self::Disabled => control && disabled(document, id, e, context),
            Self::Checked => {
                html && (tag == "input"
                    && matches!(input_type.as_str(), "checkbox" | "radio")
                    && e.attribute("checked").is_some()
                    || tag == "option" && e.attribute("selected").is_some())
            }
            Self::Default => {
                if html
                    && (tag == "input"
                        && matches!(input_type.as_str(), "checkbox" | "radio")
                        && e.attribute("checked").is_some()
                        || tag == "option" && e.attribute("selected").is_some())
                {
                    return true;
                }
                if !html || !submit(e) {
                    return false;
                }
                let Some(owner) = form_owner(document, id, e, context) else {
                    return false;
                };
                document
                    .preorder()
                    .take_while(|_| context.remaining.get() > 0)
                    .find(|&candidate| {
                        if !context.step() {
                            return false;
                        }
                        document
                            .node(candidate)
                            .and_then(|n| n.as_element())
                            .is_some_and(|e| {
                                submit(e)
                                    && form_owner(document, candidate, e, context) == Some(owner)
                            })
                    })
                    == Some(id)
            }
            Self::Required => required_control && e.attribute("required").is_some(),
            Self::Optional => {
                html && matches!(tag.as_str(), "input" | "select" | "textarea")
                    && !(required_control && e.attribute("required").is_some())
            }
            Self::ReadWrite | Self::ReadOnly => {
                let writable = editable_control
                    && e.attribute("readonly").is_none()
                    && !disabled(document, id, e, context)
                    || html && content_editable(document, id, context);
                writable == matches!(self, Self::ReadWrite)
            }
            Self::PlaceholderShown => {
                html && (tag == "textarea"
                    || tag == "input"
                        && matches!(
                            input_type.as_str(),
                            "text" | "search" | "url" | "tel" | "email" | "password" | "number"
                        ))
                    && e.attribute("placeholder").is_some_and(|s| !s.is_empty())
                    && if tag == "textarea" {
                        !document.node(id).unwrap().children().iter().any(|&child| {
                            document.node(child).is_some_and(
                                |n| matches!(n.kind(), NodeKind::Text(s) if !s.is_empty()),
                            )
                        })
                    } else {
                        e.attribute("value").is_none_or(str::is_empty)
                    }
            }
            Self::Hover => within(document, context.state.hovered, id, context),
            Self::Active => within(document, context.state.active, id, context),
            Self::Focus => context.state.focused == Some(id),
            Self::FocusVisible => context.state.focus_visible && context.state.focused == Some(id),
            Self::FocusWithin => within(document, context.state.focused, id, context),
            Self::Target => context.state.target == Some(id),
            Self::Indeterminate => {
                if html && tag == "progress" {
                    return e.attribute("value").is_none();
                }
                if !html || tag != "input" || input_type != "radio" {
                    return false;
                }
                let owner = form_owner(document, id, e, context);
                let name = e.attribute("name").unwrap_or("");
                !document
                    .preorder()
                    .take_while(|_| context.remaining.get() > 0)
                    .any(|candidate| {
                        if !context.step() {
                            return false;
                        }
                        document
                            .node(candidate)
                            .and_then(|n| n.as_element())
                            .is_some_and(|other| {
                                other.tag_name() == "input"
                                    && normalized_input_type(other) == "radio"
                                    && (candidate == id
                                        || !name.is_empty()
                                            && other.attribute("name") == Some(name))
                                    && form_owner(document, candidate, other, context) == owner
                                    && other.attribute("checked").is_some()
                            })
                    })
            }
        }
    }
}

fn normalized_input_type(e: &ElementData) -> String {
    let value = e.attribute("type").unwrap_or("text").to_ascii_lowercase();
    if matches!(
        value.as_str(),
        "hidden"
            | "text"
            | "search"
            | "url"
            | "tel"
            | "email"
            | "password"
            | "date"
            | "month"
            | "week"
            | "time"
            | "datetime-local"
            | "number"
            | "range"
            | "color"
            | "checkbox"
            | "radio"
            | "file"
            | "submit"
            | "image"
            | "reset"
            | "button"
    ) {
        value
    } else {
        "text".into()
    }
}

fn submit(e: &ElementData) -> bool {
    !e.has_case_sensitive_names()
        && (e.tag_name() == "input"
            && matches!(normalized_input_type(e).as_str(), "submit" | "image")
            || e.tag_name() == "button"
                && !e
                    .attribute("type")
                    .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "button" | "reset")))
}

fn form_owner(
    document: &Document,
    id: NodeId,
    e: &ElementData,
    context: MatchContext<'_>,
) -> Option<NodeId> {
    if let Some(name) = e.attribute("form") {
        let candidate = document
            .preorder()
            .take_while(|_| context.remaining.get() > 0)
            .find(|&candidate| {
                context.step()
                    && document
                        .node(candidate)
                        .and_then(|n| n.as_element())
                        .is_some_and(|e| e.attribute("id") == Some(name))
            })?;
        return document
            .node(candidate)
            .and_then(|n| n.as_element())
            .filter(|e| e.tag_name() == "form")
            .map(|_| candidate);
    }
    let mut parent = document.node(id).and_then(|n| n.parent());
    while let Some(id) = parent {
        if !context.step() {
            return None;
        }
        let node = document.node(id)?;
        if node.as_element().is_some_and(|e| e.tag_name() == "form") {
            return Some(id);
        }
        parent = node.parent();
    }
    None
}

pub fn focusable(document: &Document, id: NodeId) -> bool {
    let Some(e) = document.node(id).and_then(|n| n.as_element()) else {
        return false;
    };
    let state = InteractionState::default();
    let remaining = Cell::new(MAX_MATCH_STEPS);
    let context = MatchContext {
        scope: None,
        state: &state,
        remaining: &remaining,
    };
    let result = !e.has_case_sensitive_names()
        && !disabled(document, id, e, context)
        && (e
            .attribute("tabindex")
            .is_some_and(|v| v.trim().parse::<i32>().is_ok())
            || matches!(e.tag_name(), "a" | "area") && e.attribute("href").is_some()
            || matches!(e.tag_name(), "button" | "select" | "textarea")
            || e.tag_name() == "input"
                && !e
                    .attribute("type")
                    .is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
            || content_editable(document, id, context));
    result && remaining.get() > 0
}

fn within(
    document: &Document,
    mut child: Option<NodeId>,
    ancestor: NodeId,
    context: MatchContext<'_>,
) -> bool {
    while let Some(id) = child {
        if !context.step() {
            return false;
        }
        if id == ancestor {
            return true;
        }
        child = document.node(id).and_then(|n| n.parent());
    }
    false
}

fn disabled(document: &Document, id: NodeId, e: &ElementData, context: MatchContext<'_>) -> bool {
    if e.has_case_sensitive_names()
        || !matches!(
            e.tag_name().to_ascii_lowercase().as_str(),
            "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
        )
    {
        return false;
    }
    if e.attribute("disabled").is_some() {
        return true;
    }
    let tag = e.tag_name();
    if tag == "option" {
        return document
            .node(id)
            .and_then(|n| n.parent())
            .and_then(|p| document.node(p))
            .and_then(|n| n.as_element())
            .is_some_and(|p| p.tag_name() == "optgroup" && p.attribute("disabled").is_some());
    }
    if !matches!(tag, "button" | "input" | "select" | "textarea" | "fieldset") {
        return false;
    }
    let mut parent = document.node(id).and_then(|n| n.parent());
    while let Some(p) = parent {
        if !context.step() {
            return false;
        }
        let node = document.node(p).unwrap();
        if node
            .as_element()
            .is_some_and(|e| e.tag_name() == "fieldset" && e.attribute("disabled").is_some())
        {
            let legend = node.children().iter().copied().find(|&id| {
                document
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|e| e.tag_name() == "legend")
            });
            if !legend.is_some_and(|legend| within(document, Some(id), legend, context)) {
                return true;
            }
        }
        parent = node.parent();
    }
    false
}

fn content_editable(document: &Document, mut id: NodeId, context: MatchContext<'_>) -> bool {
    loop {
        if !context.step() {
            return false;
        }
        let Some(node) = document.node(id) else {
            return false;
        };
        if let Some(value) = node
            .as_element()
            .and_then(|e| e.attribute("contenteditable"))
        {
            if value.is_empty()
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("plaintext-only")
            {
                return true;
            }
            if value.eq_ignore_ascii_case("false") {
                return false;
            }
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        id = parent;
    }
}
