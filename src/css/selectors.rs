//! Token-based selectors, specificity, and DOM matching.
//! https://www.w3.org/TR/selectors-4/

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::dom::{Document, DocumentMode, ElementData, NodeId, NodeKind};

use super::tokenizer::{HashKind, TokenKind, Tokenizer};

mod functional;
pub(crate) mod matching;
mod states;
pub use states::focusable;

pub const MAX_SELECTOR_NESTING: usize = 64;
pub const MAX_MATCH_STEPS: usize = 100_000;
const MAX_MATCH_LOGS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MatchLimitExceeded;

impl fmt::Display for MatchLimitExceeded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "selector matching exceeded the {MAX_MATCH_STEPS}-step limit; match rejected"
        )
    }
}

impl Error for MatchLimitExceeded {}

/// Explicit, snapshot-local state. No history/visited information is accepted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InteractionState {
    pub hovered: Option<NodeId>,
    pub active: Option<NodeId>,
    pub focused: Option<NodeId>,
    pub focus_visible: bool,
    pub target: Option<NodeId>,
}

#[derive(Clone, Copy)]
struct MatchContext<'a> {
    scope: Option<NodeId>,
    state: &'a InteractionState,
    remaining: &'a Cell<usize>,
}

impl MatchContext<'_> {
    fn step(self) -> bool {
        let remaining = self.remaining.get();
        self.remaining.set(remaining.saturating_sub(1));
        remaining != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoElement {
    Before,
    After,
    Marker,
    FirstLine,
    FirstLetter,
    Placeholder,
    Selection,
    Backdrop,
}

impl PseudoElement {
    pub const ALL: [Self; 8] = [
        Self::Before,
        Self::After,
        Self::Marker,
        Self::FirstLine,
        Self::FirstLetter,
        Self::Placeholder,
        Self::Selection,
        Self::Backdrop,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
            Self::Marker => "marker",
            Self::FirstLine => "first-line",
            Self::FirstLetter => "first-letter",
            Self::Placeholder => "placeholder",
            Self::Selection => "selection",
            Self::Backdrop => "backdrop",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    pub ids: u32,
    pub classes: u32,
    pub types: u32,
}

impl Specificity {
    fn add(&mut self, other: Self) {
        self.ids = self.ids.saturating_add(other.ids);
        self.classes = self.classes.saturating_add(other.classes);
        self.types = self.types.saturating_add(other.types);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Compound {
    tag: Option<String>,
    namespace: NamespaceConstraint,
    simple: Vec<SimpleSelector>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum NamespaceConstraint {
    Any,
    Uri(String),
}

impl NamespaceConstraint {
    fn matches(&self, uri: &str) -> bool {
        matches!(self, Self::Any) || matches!(self, Self::Uri(expected) if expected == uri)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Namespaces {
    pub default: Option<String>,
    pub prefixes: HashMap<String, String>,
}

impl Compound {
    fn matches(&self, document: &Document, node_id: NodeId, context: MatchContext<'_>) -> bool {
        if !context.step() {
            return false;
        }
        let Some(element) = document.node(node_id).and_then(|node| node.as_element()) else {
            return false;
        };
        self.namespace.matches(element.namespace().uri())
            && self.tag.as_ref().is_none_or(|tag| {
                equal(element.tag_name(), tag, !element.has_case_sensitive_names())
            })
            && self
                .simple
                .iter()
                .all(|simple| simple.matches(document, node_id, element, context))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SimpleSelector {
    Id(String),
    Class(String),
    Attribute(AttributeSelector),
    Pseudo(PseudoClass),
}

impl SimpleSelector {
    fn specificity(&self) -> Specificity {
        match self {
            Self::Id(_) => Specificity {
                ids: 1,
                ..Specificity::default()
            },
            Self::Pseudo(
                PseudoClass::Not(selectors)
                | PseudoClass::Is(selectors)
                | PseudoClass::Has(selectors),
            ) => selectors
                .iter()
                .map(Selector::specificity)
                .max()
                .unwrap_or_default(),
            Self::Pseudo(PseudoClass::Parent(selectors)) => selectors
                .iter()
                .map(Selector::specificity)
                .max()
                .unwrap_or_default(),
            Self::Pseudo(PseudoClass::Where(_)) => Specificity::default(),
            Self::Pseudo(PseudoClass::ScopeAnchor) => Specificity::default(),
            Self::Pseudo(PseudoClass::Nth(nth)) => {
                let mut specificity = Specificity {
                    classes: 1,
                    ..Specificity::default()
                };
                specificity.add(
                    nth.of
                        .iter()
                        .map(Selector::specificity)
                        .max()
                        .unwrap_or_default(),
                );
                specificity
            }
            _ => Specificity {
                classes: 1,
                ..Specificity::default()
            },
        }
    }

    fn matches(
        &self,
        document: &Document,
        node_id: NodeId,
        element: &ElementData,
        context: MatchContext<'_>,
    ) -> bool {
        let quirks = document.mode() == DocumentMode::Quirks;
        match self {
            Self::Id(id) => element
                .attribute("id")
                .is_some_and(|value| equal(value, id, quirks)),
            Self::Class(class) => element.attribute("class").is_some_and(|value| {
                value
                    .split(css_whitespace)
                    .any(|token| equal(token, class, quirks))
            }),
            Self::Attribute(selector) => selector.matches(element),
            Self::Pseudo(pseudo) => pseudo.matches(document, node_id, element, context),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttributeOperator {
    Exists,
    Equals,
    Includes,
    DashMatch,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttributeSelector {
    name: String,
    namespace: NamespaceConstraint,
    operator: AttributeOperator,
    value: String,
    case_insensitive: Option<bool>,
}

impl AttributeSelector {
    fn matches(&self, element: &ElementData) -> bool {
        element.attributes().iter().any(|attribute| {
            self.namespace.matches(attribute.namespace().unwrap_or(""))
                && equal(
                    attribute.name(),
                    &self.name,
                    attribute.namespace().is_none() && !element.has_case_sensitive_names(),
                )
                && self.matches_value(element, attribute.value(), attribute.namespace().is_none())
        })
    }

    fn matches_value(&self, element: &ElementData, actual: &str, unnamespaced: bool) -> bool {
        let insensitive = self.case_insensitive.unwrap_or_else(|| {
            unnamespaced
                && !element.has_case_sensitive_names()
                && html_case_insensitive_attribute(&self.name.to_ascii_lowercase())
        });
        let expected = self.value.as_str();
        let bytes = actual.as_bytes();
        let matches = |value: &[u8]| {
            if insensitive {
                value.eq_ignore_ascii_case(expected.as_bytes())
            } else {
                value == expected.as_bytes()
            }
        };
        match self.operator {
            AttributeOperator::Exists => true,
            AttributeOperator::Equals => matches(bytes),
            AttributeOperator::Includes => {
                !expected.is_empty()
                    && !expected.chars().any(css_whitespace)
                    && actual
                        .split(css_whitespace)
                        .any(|token| matches(token.as_bytes()))
            }
            AttributeOperator::DashMatch => {
                matches(bytes)
                    || bytes.get(..expected.len()).is_some_and(matches)
                        && bytes.get(expected.len()) == Some(&b'-')
            }
            AttributeOperator::Prefix => {
                !expected.is_empty() && bytes.get(..expected.len()).is_some_and(matches)
            }
            AttributeOperator::Suffix => {
                !expected.is_empty()
                    && bytes
                        .len()
                        .checked_sub(expected.len())
                        .and_then(|start| bytes.get(start..))
                        .is_some_and(matches)
            }
            AttributeOperator::Substring => {
                !expected.is_empty() && bytes.windows(expected.len()).any(matches)
            }
        }
    }
}

// HTML defines these value comparisons independently of the element's tag.
// https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-selectors
fn html_case_insensitive_attribute(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "accept-charset"
            | "align"
            | "alink"
            | "axis"
            | "bgcolor"
            | "charset"
            | "checked"
            | "clear"
            | "codetype"
            | "color"
            | "compact"
            | "declare"
            | "defer"
            | "dir"
            | "direction"
            | "disabled"
            | "enctype"
            | "face"
            | "frame"
            | "hreflang"
            | "http-equiv"
            | "lang"
            | "language"
            | "link"
            | "media"
            | "method"
            | "multiple"
            | "nohref"
            | "noresize"
            | "noshade"
            | "nowrap"
            | "readonly"
            | "rel"
            | "rev"
            | "rules"
            | "scope"
            | "scrolling"
            | "selected"
            | "shape"
            | "target"
            | "text"
            | "type"
            | "valign"
            | "valuetype"
            | "vlink"
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PseudoClass {
    Root,
    Scope,
    ScopeAnchor,
    Empty,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Not(Vec<Selector>),
    Is(Vec<Selector>),
    Where(Vec<Selector>),
    Has(Vec<Selector>),
    Nth(functional::Nth),
    State(states::State),
    Parent(Arc<[Selector]>),
}

impl PseudoClass {
    fn matches(
        &self,
        document: &Document,
        node_id: NodeId,
        element: &ElementData,
        context: MatchContext<'_>,
    ) -> bool {
        let node = document.node(node_id).expect("compound matched an element");
        match self {
            Self::Scope | Self::ScopeAnchor => context.scope.map_or_else(
                || Self::Root.matches(document, node_id, element, context),
                |root| root == node_id,
            ),
            Self::Root => node
                .parent()
                .and_then(|id| document.node(id))
                .is_some_and(|parent| matches!(parent.kind(), NodeKind::Document)),
            // Match shipping browser behavior: even whitespace text prevents :empty.
            Self::Empty => !node.children().iter().any(|&id| {
                document.node(id).is_some_and(|child| match child.kind() {
                    NodeKind::Element(_) => true,
                    NodeKind::Text(text) => !text.is_empty(),
                    _ => false,
                })
            }),
            Self::Not(selectors) => !selectors
                .iter()
                .any(|selector| selector.matches_context(document, node_id, context)),
            Self::Is(selectors) | Self::Where(selectors) => selectors
                .iter()
                .any(|selector| selector.matches_context(document, node_id, context)),
            Self::Parent(selectors) => selectors.iter().any(|selector| {
                selector.pseudo_element.is_none()
                    && selector.matches_context(document, node_id, context)
            }),
            Self::Has(selectors) => selectors
                .iter()
                .any(|selector| selector.matches_relative(document, node_id, context)),
            Self::Nth(nth) => nth.matches(document, node_id, element, context),
            Self::State(state) => state.matches(document, node_id, element, context),
            _ => {
                let of_type = matches!(
                    self,
                    Self::FirstOfType | Self::LastOfType | Self::OnlyOfType
                );
                let relevant = |id: &NodeId| {
                    document
                        .node(*id)
                        .and_then(|node| node.as_element())
                        .is_some_and(|sibling| {
                            !of_type
                                || sibling.namespace() == element.namespace()
                                    && equal(
                                        sibling.tag_name(),
                                        element.tag_name(),
                                        !element.has_case_sensitive_names(),
                                    )
                        })
                };
                let (before, after) = sibling_slices(document, node_id);
                let first = !before.iter().any(relevant);
                let last = !after.iter().any(relevant);
                match self {
                    Self::FirstChild | Self::FirstOfType => first,
                    Self::LastChild | Self::LastOfType => last,
                    Self::OnlyChild | Self::OnlyOfType => first && last,
                    _ => unreachable!("non-structural pseudo-classes handled above"),
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    compounds: Vec<Compound>,
    combinators: Vec<Combinator>,
    specificity: Specificity,
    pseudo_element: Option<PseudoElement>,
    relative: Option<Combinator>,
}

impl Selector {
    pub fn specificity(&self) -> Specificity {
        self.specificity
    }

    pub fn matches(&self, document: &Document, node_id: NodeId) -> bool {
        self.matches_with_scope(document, node_id, None)
    }

    pub fn pseudo_element(&self) -> Option<PseudoElement> {
        self.pseudo_element
    }

    pub fn matches_with_state(
        &self,
        document: &Document,
        node_id: NodeId,
        state: &InteractionState,
    ) -> bool {
        self.matches_target(document, node_id, None, state, None)
    }

    pub(crate) fn matches_with_scope(
        &self,
        document: &Document,
        node_id: NodeId,
        scope: Option<NodeId>,
    ) -> bool {
        self.matches_target(document, node_id, scope, &InteractionState::default(), None)
    }

    pub(crate) fn matches_target(
        &self,
        document: &Document,
        node_id: NodeId,
        scope: Option<NodeId>,
        state: &InteractionState,
        pseudo: Option<PseudoElement>,
    ) -> bool {
        match self.try_matches_target(document, node_id, scope, state, pseudo) {
            Ok(matched) => matched,
            Err(error) => {
                static REPORTS: AtomicUsize = AtomicUsize::new(0);
                if let Ok(count) =
                    REPORTS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                        (count < MAX_MATCH_LOGS).then_some(count + 1)
                    })
                {
                    if count + 1 == MAX_MATCH_LOGS {
                        eprintln!("CSS: {error}; additional selector matching diagnostics omitted");
                    } else {
                        eprintln!("CSS at {node_id:?}: {error}");
                    }
                }
                false
            }
        }
    }

    pub(crate) fn try_matches_target(
        &self,
        document: &Document,
        node_id: NodeId,
        scope: Option<NodeId>,
        state: &InteractionState,
        pseudo: Option<PseudoElement>,
    ) -> Result<bool, MatchLimitExceeded> {
        if self.pseudo_element != pseudo {
            return Ok(false);
        }
        let remaining = Cell::new(MAX_MATCH_STEPS);
        let result = self.matches_context(
            document,
            node_id,
            MatchContext {
                scope,
                state,
                remaining: &remaining,
            },
        );
        // Negation can turn a nested failure into true, but cannot erase the
        // shared budget's exhaustion.
        if remaining.get() == 0 {
            Err(MatchLimitExceeded)
        } else {
            Ok(result)
        }
    }

    fn matches_context(
        &self,
        document: &Document,
        node_id: NodeId,
        context: MatchContext<'_>,
    ) -> bool {
        if self.compounds.len() == 1 {
            return self.compounds[0].matches(document, node_id, context);
        }
        let mut pending = vec![(self.compounds.len() - 1, node_id)];
        let mut visited = HashSet::new();
        while let Some((index, id)) = pending.pop() {
            if !context.step() {
                return false;
            }
            if !visited.insert((index, id)) || !self.compounds[index].matches(document, id, context)
            {
                continue;
            }
            if index == 0 {
                return true;
            }
            let node = document.node(id).expect("compound matched an element");
            match self.combinators[index - 1] {
                Combinator::Descendant => {
                    // Keep all candidates: the nearest match may fail a combinator
                    // farther left. Memoized states avoid exponential backtracking.
                    let mut ancestor = node.parent();
                    while let Some(id) = ancestor {
                        if !context.step() {
                            return false;
                        }
                        pending.push((index - 1, id));
                        ancestor = document.node(id).and_then(|node| node.parent());
                    }
                }
                Combinator::Child => {
                    if let Some(parent) = node.parent() {
                        pending.push((index - 1, parent));
                    }
                }
                Combinator::NextSibling | Combinator::SubsequentSibling => {
                    let (before, _) = sibling_slices(document, id);
                    for &sibling in before.iter().rev() {
                        if !context.step() {
                            return false;
                        }
                        if document
                            .node(sibling)
                            .is_some_and(|node| node.as_element().is_some())
                        {
                            pending.push((index - 1, sibling));
                            if self.combinators[index - 1] == Combinator::NextSibling {
                                break;
                            }
                        }
                    }
                }
            }
        }
        false
    }
}

fn sibling_slices(document: &Document, node_id: NodeId) -> (&[NodeId], &[NodeId]) {
    let siblings = document
        .node(node_id)
        .and_then(|node| node.parent())
        .and_then(|id| document.node(id))
        .map(|parent| parent.children())
        .unwrap_or(&[]);
    match siblings.iter().position(|&id| id == node_id) {
        Some(index) => (&siblings[..index], &siblings[index + 1..]),
        None => (&[], &[]),
    }
}

const HOVER: u8 = 1;
const ACTIVE: u8 = 2;
const FOCUS: u8 = 4;
const FOCUS_VISIBLE: u8 = 8;
const FOCUS_WITHIN: u8 = 16;
const TARGET: u8 = 32;

/// Compounds that test interaction state, reduced to their context-free
/// tag/id/class/attribute parts. A compound `C = rest ∧ state` can only change
/// its result on an element whose state changed and which matches `rest`;
/// nested selectors (`:is`, `:not`, `:has`, `&`, ...) are indexed the same way.
/// If no trigger fires, every selector match is unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateTriggers {
    compounds: Vec<(Compound, u8)>,
}

impl StateTriggers {
    pub(crate) fn add(&mut self, selector: &Selector) {
        for compound in &selector.compounds {
            self.add_compound(compound);
        }
    }

    fn add_compound(&mut self, compound: &Compound) {
        let mut mask = 0;
        for simple in &compound.simple {
            match simple {
                SimpleSelector::Pseudo(PseudoClass::State(state)) => {
                    mask |= match state {
                        states::State::Hover => HOVER,
                        states::State::Active => ACTIVE,
                        states::State::Focus => FOCUS,
                        states::State::FocusVisible => FOCUS_VISIBLE,
                        states::State::FocusWithin => FOCUS_WITHIN,
                        states::State::Target => TARGET,
                        _ => 0,
                    }
                }
                SimpleSelector::Pseudo(
                    PseudoClass::Not(inner)
                    | PseudoClass::Is(inner)
                    | PseudoClass::Where(inner)
                    | PseudoClass::Has(inner),
                ) => inner.iter().for_each(|s| self.add(s)),
                SimpleSelector::Pseudo(PseudoClass::Nth(nth)) => {
                    nth.of.iter().for_each(|s| self.add(s))
                }
                SimpleSelector::Pseudo(PseudoClass::Parent(inner)) => {
                    inner.iter().for_each(|s| self.add(s))
                }
                _ => {}
            }
        }
        if mask == 0 {
            return;
        }
        let rest = Compound {
            tag: compound.tag.clone(),
            namespace: compound.namespace.clone(),
            simple: compound
                .simple
                .iter()
                .filter(|s| !matches!(s, SimpleSelector::Pseudo(_)))
                .cloned()
                .collect(),
        };
        if !self
            .compounds
            .last()
            .is_some_and(|(r, m)| *m == mask && *r == rest)
        {
            self.compounds.push((rest, mask));
        }
    }

    /// Whether changing `old` to `new` can change any indexed selector match.
    pub fn affected(
        &self,
        document: &Document,
        old: &InteractionState,
        new: &InteractionState,
    ) -> bool {
        let chain = |mut id: Option<NodeId>| {
            let mut out = Vec::new();
            while let Some(node) = id {
                out.push(node);
                id = document.node(node).and_then(|n| n.parent());
            }
            out
        };
        let diff = |a: Vec<NodeId>, b: Vec<NodeId>| {
            let mut out: Vec<NodeId> = a.iter().filter(|id| !b.contains(id)).copied().collect();
            out.extend(b.iter().filter(|id| !a.contains(id)));
            out
        };
        let single = |a: Option<NodeId>, b: Option<NodeId>| {
            if a == b {
                Vec::new()
            } else {
                a.into_iter().chain(b).collect()
            }
        };
        let mut changed: Vec<(u8, Vec<NodeId>)> = Vec::new();
        if old.hovered != new.hovered {
            changed.push((HOVER, diff(chain(old.hovered), chain(new.hovered))));
        }
        if old.active != new.active {
            changed.push((ACTIVE, diff(chain(old.active), chain(new.active))));
        }
        if old.focused != new.focused {
            changed.push((FOCUS, single(old.focused, new.focused)));
            changed.push((FOCUS_WITHIN, diff(chain(old.focused), chain(new.focused))));
        }
        let visible = |s: &InteractionState| s.focused.filter(|_| s.focus_visible);
        changed.push((FOCUS_VISIBLE, single(visible(old), visible(new))));
        changed.push((TARGET, single(old.target, new.target)));
        let state = InteractionState::default();
        let remaining = Cell::new(usize::MAX);
        let context = MatchContext {
            scope: None,
            state: &state,
            remaining: &remaining,
        };
        self.compounds.iter().any(|(rest, mask)| {
            changed.iter().any(|(bit, nodes)| {
                mask & bit != 0 && nodes.iter().any(|&id| rest.matches(document, id, context))
            })
        })
    }
}

fn equal(actual: &str, expected: &str, insensitive: bool) -> bool {
    if insensitive {
        actual.eq_ignore_ascii_case(expected)
    } else {
        actual == expected
    }
}

fn css_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorError;

impl fmt::Display for SelectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid or unsupported selector, or selector nesting limit exceeded")
    }
}

impl Error for SelectorError {}

pub fn parse(source: &str) -> Result<Vec<Selector>, SelectorError> {
    parse_context(source, &Namespaces::default(), None)
}

pub(crate) fn parse_context(
    source: &str,
    namespaces: &Namespaces,
    parent: Option<Arc<[Selector]>>,
) -> Result<Vec<Selector>, SelectorError> {
    parse_scoped(source, namespaces, parent, false)
}

pub(crate) fn parse_scoped(
    source: &str,
    namespaces: &Namespaces,
    parent: Option<Arc<[Selector]>>,
    scoped: bool,
) -> Result<Vec<Selector>, SelectorError> {
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let tokens = std::iter::from_fn(|| tokenizer.next_token())
        .map(|token| token.kind)
        .collect();
    if !tokenizer.errors().is_empty() || tokenizer.dropped_errors() != 0 {
        return Err(SelectorError);
    }
    Parser {
        tokens,
        position: 0,
        namespaces: namespaces.clone(),
        parent,
        scoped,
        in_has: false,
        allow_pseudo: true,
    }
    .list(false, 0)
}

struct Parser {
    tokens: Vec<TokenKind>,
    position: usize,
    namespaces: Namespaces,
    parent: Option<Arc<[Selector]>>,
    scoped: bool,
    in_has: bool,
    allow_pseudo: bool,
}

impl Parser {
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.position]
    }

    fn whitespace(&mut self) -> bool {
        let start = self.position;
        while self.peek() == &TokenKind::Whitespace {
            self.position += 1;
        }
        self.position != start
    }

    fn consume(&mut self, kind: TokenKind) -> Result<(), SelectorError> {
        if self.peek() != &kind {
            return Err(SelectorError);
        }
        self.position += 1;
        Ok(())
    }

    fn ident(&mut self) -> Result<String, SelectorError> {
        let TokenKind::Ident(name) = self.peek() else {
            return Err(SelectorError);
        };
        let name = name.clone();
        self.position += 1;
        Ok(name)
    }

    fn list(&mut self, nested: bool, depth: usize) -> Result<Vec<Selector>, SelectorError> {
        if depth > MAX_SELECTOR_NESTING {
            return Err(SelectorError);
        }
        let mut selectors = Vec::new();
        loop {
            self.whitespace();
            selectors.push(self.selector(depth)?);
            match self.peek() {
                TokenKind::Comma => self.position += 1,
                TokenKind::CloseParen if nested => {
                    self.position += 1;
                    return Ok(selectors);
                }
                TokenKind::Eof if !nested => return Ok(selectors),
                _ => return Err(SelectorError),
            }
        }
    }

    fn selector(&mut self, depth: usize) -> Result<Selector, SelectorError> {
        let mut selector = Selector {
            compounds: Vec::new(),
            combinators: Vec::new(),
            specificity: Specificity::default(),
            pseudo_element: None,
            relative: None,
        };
        // Each nesting selector is an :is() of the complete parent list, not
        // an expanded Cartesian product with branch-specific specificity.
        let mut end = self.position;
        let mut nesting = 0usize;
        while end < self.tokens.len() {
            match self.tokens[end] {
                TokenKind::Function(_) | TokenKind::OpenParen | TokenKind::OpenSquare => {
                    nesting += 1
                }
                TokenKind::CloseParen | TokenKind::CloseSquare if nesting > 0 => nesting -= 1,
                TokenKind::Comma | TokenKind::CloseParen | TokenKind::Eof if nesting == 0 => break,
                _ => {}
            }
            end += 1;
        }
        let has_scope = self.tokens[self.position..end].windows(2).any(|pair| matches!(pair, [TokenKind::Colon, TokenKind::Ident(name)] if name.eq_ignore_ascii_case("scope")));
        let implicit = (self.parent.is_some() || self.scoped && !has_scope)
            && !self.tokens[self.position..end]
                .iter()
                .any(|token| matches!(token, TokenKind::Delim('&')));
        if implicit && depth == 0 {
            let pseudo = if let Some(parent) = &self.parent {
                selector.specificity.add(
                    parent
                        .iter()
                        .map(Selector::specificity)
                        .max()
                        .unwrap_or_default(),
                );
                PseudoClass::Parent(parent.clone())
            } else {
                PseudoClass::ScopeAnchor
            };
            selector.compounds.push(Compound {
                tag: None,
                namespace: NamespaceConstraint::Any,
                simple: vec![SimpleSelector::Pseudo(pseudo)],
            });
            let combinator = match self.peek() {
                TokenKind::Delim('>') => Some(Combinator::Child),
                TokenKind::Delim('+') => Some(Combinator::NextSibling),
                TokenKind::Delim('~') => Some(Combinator::SubsequentSibling),
                _ => None,
            };
            if combinator.is_some() {
                self.position += 1;
                self.whitespace();
            }
            selector
                .combinators
                .push(combinator.unwrap_or(Combinator::Descendant));
        }
        loop {
            let compound = self.compound(depth)?;
            if compound.tag.is_some() {
                selector.specificity.add(Specificity {
                    types: 1,
                    ..Specificity::default()
                });
            }
            for simple in &compound.simple {
                selector.specificity.add(simple.specificity());
            }
            selector.compounds.push(compound);
            if matches!(self.peek(), TokenKind::Colon) {
                if !self.allow_pseudo {
                    return Err(SelectorError);
                }
                self.position += 1;
                let double = matches!(self.peek(), TokenKind::Colon);
                if double {
                    self.position += 1;
                }
                let name = self.ident()?.to_ascii_lowercase();
                let pseudo = match name.as_str() {
                    "before" => PseudoElement::Before,
                    "after" => PseudoElement::After,
                    "first-line" => PseudoElement::FirstLine,
                    "first-letter" => PseudoElement::FirstLetter,
                    "marker" if double => PseudoElement::Marker,
                    "placeholder" if double => PseudoElement::Placeholder,
                    "selection" if double => PseudoElement::Selection,
                    "backdrop" if double => PseudoElement::Backdrop,
                    _ => return Err(SelectorError),
                };
                selector.pseudo_element = Some(pseudo);
                selector.specificity.add(Specificity {
                    types: 1,
                    ..Specificity::default()
                });
                self.whitespace();
                if !matches!(
                    self.peek(),
                    TokenKind::Comma | TokenKind::CloseParen | TokenKind::Eof
                ) {
                    return Err(SelectorError);
                }
                break;
            }
            let whitespace = self.whitespace();
            let combinator = match self.peek() {
                TokenKind::Delim('>') => Combinator::Child,
                TokenKind::Delim('+') => Combinator::NextSibling,
                TokenKind::Delim('~') => Combinator::SubsequentSibling,
                TokenKind::Comma | TokenKind::CloseParen | TokenKind::Eof => break,
                _ if whitespace => Combinator::Descendant,
                _ => return Err(SelectorError),
            };
            if combinator != Combinator::Descendant {
                self.position += 1;
                self.whitespace();
            }
            selector.combinators.push(combinator);
        }
        Ok(selector)
    }

    fn compound(&mut self, depth: usize) -> Result<Compound, SelectorError> {
        let start = self.position;
        let namespace = self.namespace(false)?;
        let tag = match self.peek() {
            TokenKind::Ident(_) => Some(self.ident()?),
            TokenKind::Delim('*') => {
                self.position += 1;
                None
            }
            _ => None,
        };
        let mut simple = Vec::new();
        loop {
            let selector = match self.peek().clone() {
                TokenKind::Hash {
                    value,
                    kind: HashKind::Id,
                } => {
                    self.position += 1;
                    SimpleSelector::Id(value)
                }
                TokenKind::Delim('.') => {
                    self.position += 1;
                    SimpleSelector::Class(self.ident()?)
                }
                TokenKind::OpenSquare => SimpleSelector::Attribute(self.attribute()?),
                TokenKind::Colon => {
                    if self.tokens.get(self.position + 1) == Some(&TokenKind::Colon)
                        || matches!(self.tokens.get(self.position + 1), Some(TokenKind::Ident(name)) if matches!(name.to_ascii_lowercase().as_str(), "before" | "after" | "first-line" | "first-letter"))
                    {
                        // A bare pseudo-element has an implicit universal subject.
                        if self.position == start {
                            return Ok(Compound {
                                tag: None,
                                namespace,
                                simple,
                            });
                        }
                        break;
                    }
                    self.position += 1;
                    SimpleSelector::Pseudo(self.pseudo(depth)?)
                }
                TokenKind::Delim('&') => {
                    self.position += 1;
                    SimpleSelector::Pseudo(if let Some(parent) = &self.parent {
                        PseudoClass::Parent(parent.clone())
                    } else if self.scoped {
                        PseudoClass::ScopeAnchor
                    } else {
                        return Err(SelectorError);
                    })
                }
                _ => break,
            };
            simple.push(selector);
        }
        if self.position == start {
            return Err(SelectorError);
        }
        Ok(Compound {
            tag,
            namespace,
            simple,
        })
    }

    fn namespace(&mut self, attribute: bool) -> Result<NamespaceConstraint, SelectorError> {
        let prefix = match self.peek() {
            TokenKind::Delim('|') => Some((String::new(), 1)),
            TokenKind::Ident(name)
                if self.tokens.get(self.position + 1) == Some(&TokenKind::Delim('|'))
                    && (!attribute
                        || self.tokens.get(self.position + 2) != Some(&TokenKind::Delim('='))) =>
            {
                Some((name.clone(), 2))
            }
            TokenKind::Delim('*')
                if self.tokens.get(self.position + 1) == Some(&TokenKind::Delim('|')) =>
            {
                Some(("*".into(), 2))
            }
            _ => None,
        };
        if let Some((prefix, advance)) = prefix {
            self.position += advance;
            if !matches!(self.peek(), TokenKind::Ident(_) | TokenKind::Delim('*')) {
                return Err(SelectorError);
            }
            return match prefix.as_str() {
                "*" => Ok(NamespaceConstraint::Any),
                "" => Ok(NamespaceConstraint::Uri(String::new())),
                _ => self
                    .namespaces
                    .prefixes
                    .get(&prefix)
                    .cloned()
                    .map(NamespaceConstraint::Uri)
                    .ok_or(SelectorError),
            };
        }
        Ok(if attribute {
            NamespaceConstraint::Uri(String::new())
        } else {
            self.namespaces
                .default
                .clone()
                .map(NamespaceConstraint::Uri)
                .unwrap_or(NamespaceConstraint::Any)
        })
    }

    fn attribute(&mut self) -> Result<AttributeSelector, SelectorError> {
        self.position += 1;
        self.whitespace();
        let namespace = self.namespace(true)?;
        let name = self.ident()?;
        self.whitespace();
        if self.peek() == &TokenKind::CloseSquare {
            self.position += 1;
            return Ok(AttributeSelector {
                name,
                namespace,
                operator: AttributeOperator::Exists,
                value: String::new(),
                case_insensitive: None,
            });
        }
        let operator = match self.peek() {
            TokenKind::Delim('=') => AttributeOperator::Equals,
            TokenKind::Delim('~') => AttributeOperator::Includes,
            TokenKind::Delim('|') => AttributeOperator::DashMatch,
            TokenKind::Delim('^') => AttributeOperator::Prefix,
            TokenKind::Delim('$') => AttributeOperator::Suffix,
            TokenKind::Delim('*') => AttributeOperator::Substring,
            _ => return Err(SelectorError),
        };
        self.position += 1;
        if operator != AttributeOperator::Equals {
            self.consume(TokenKind::Delim('='))?;
        }
        self.whitespace();
        let value = match self.peek() {
            TokenKind::Ident(value) | TokenKind::String(value) => value.clone(),
            _ => return Err(SelectorError),
        };
        self.position += 1;
        self.whitespace();
        let case_insensitive = match self.peek() {
            TokenKind::Ident(flag) if flag.eq_ignore_ascii_case("i") => Some(true),
            TokenKind::Ident(flag) if flag.eq_ignore_ascii_case("s") => Some(false),
            _ => None,
        };
        if case_insensitive.is_some() {
            self.position += 1;
            self.whitespace();
        }
        self.consume(TokenKind::CloseSquare)?;
        Ok(AttributeSelector {
            name,
            namespace,
            operator,
            value,
            case_insensitive,
        })
    }

    fn pseudo(&mut self, depth: usize) -> Result<PseudoClass, SelectorError> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.position += 1;
                match name.to_ascii_lowercase().as_str() {
                    "root" => Ok(PseudoClass::Root),
                    "scope" => Ok(PseudoClass::Scope),
                    "empty" => Ok(PseudoClass::Empty),
                    "first-child" => Ok(PseudoClass::FirstChild),
                    "last-child" => Ok(PseudoClass::LastChild),
                    "only-child" => Ok(PseudoClass::OnlyChild),
                    "first-of-type" => Ok(PseudoClass::FirstOfType),
                    "last-of-type" => Ok(PseudoClass::LastOfType),
                    "only-of-type" => Ok(PseudoClass::OnlyOfType),
                    _ => states::State::parse(&name)
                        .map(PseudoClass::State)
                        .ok_or(SelectorError),
                }
            }
            TokenKind::Function(name) if name.eq_ignore_ascii_case("not") => {
                self.position += 1;
                Ok(PseudoClass::Not(self.functional_list(
                    depth + 1,
                    false,
                    false,
                )?))
            }
            TokenKind::Function(name)
                if name.eq_ignore_ascii_case("is") || name.eq_ignore_ascii_case("where") =>
            {
                self.position += 1;
                let selectors = self.functional_list(depth + 1, true, false)?;
                Ok(if name.eq_ignore_ascii_case("is") {
                    PseudoClass::Is(selectors)
                } else {
                    PseudoClass::Where(selectors)
                })
            }
            TokenKind::Function(name) if name.eq_ignore_ascii_case("has") => {
                if self.in_has {
                    return Err(SelectorError);
                }
                self.position += 1;
                Ok(PseudoClass::Has(self.functional_list(
                    depth + 1,
                    false,
                    true,
                )?))
            }
            TokenKind::Function(name)
                if matches!(
                    name.to_ascii_lowercase().as_str(),
                    "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type"
                ) =>
            {
                self.position += 1;
                self.nth(&name.to_ascii_lowercase(), depth + 1)
                    .map(PseudoClass::Nth)
            }
            _ => Err(SelectorError),
        }
    }
}

#[cfg(test)]
mod level4_tests;
#[cfg(test)]
mod tests;
