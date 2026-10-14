//! Unregistered custom properties: flat tokens, iterative dependency analysis,
//! and substitution without serialization or retokenization.

use std::{
    collections::{BTreeMap, VecDeque},
    ops::Range,
    sync::Arc,
};

use crate::{
    css::tokenizer::{MAX_PARSE_ERRORS, TokenKind},
    dom::NodeId,
};

use super::{
    Diagnostic,
    declarations::{Value, WideKeyword},
    property::Registrations,
};

pub(super) const MAX_VALUE_TOKENS: usize = 16_384;
pub(super) const MAX_VALUE_BYTES: usize = 1_048_576;
pub(super) const MAX_ENV_TOKENS: usize = 65_536;
pub(super) const MAX_CUSTOM_PROPERTIES: usize = 4_096;
const MAX_ENV_BYTES: usize = 4 * MAX_VALUE_BYTES;

pub(super) type Environment = BTreeMap<String, Option<Arc<[TokenKind]>>>;

#[derive(Clone, Debug)]
struct Reference {
    name: String,
    end: usize,
    fallback: Option<Range<usize>>,
}

#[derive(Debug)]
pub(super) struct TokenValue {
    tokens: Arc<[TokenKind]>,
    // None means a valid but oversized value. It must still win the cascade.
    references: Option<BTreeMap<usize, Reference>>,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Failure {
    Missing,
    Limit,
    Grammar,
}

impl Failure {
    fn description(self) -> &'static str {
        match self {
            Self::Missing => "missing or guaranteed-invalid custom property",
            Self::Limit => "CSS variable resource limit exceeded",
            Self::Grammar => "invalid grammar after CSS variable substitution",
        }
    }
}

pub(super) fn contains_var(tokens: &[TokenKind]) -> bool {
    tokens
        .iter()
        .any(|token| matches!(token, TokenKind::Function(name) if name.eq_ignore_ascii_case("var")))
}

fn token_bytes(token: &TokenKind) -> usize {
    match token {
        TokenKind::Ident(name)
        | TokenKind::Function(name)
        | TokenKind::AtKeyword(name)
        | TokenKind::String(name)
        | TokenKind::Url(name) => name.len(),
        TokenKind::Hash { value, .. } => value.len(),
        TokenKind::Number(number) | TokenKind::Percentage(number) => number.representation.len(),
        TokenKind::Dimension { number, unit } => {
            number.representation.len().saturating_add(unit.len())
        }
        _ => 1,
    }
}

fn value_bytes(tokens: &[TokenKind]) -> usize {
    tokens.iter().fold(0usize, |total, token| {
        total.saturating_add(token_bytes(token))
    })
}

fn closer(token: &TokenKind) -> Option<TokenKind> {
    match token {
        TokenKind::Function(_) | TokenKind::OpenParen => Some(TokenKind::CloseParen),
        TokenKind::OpenSquare => Some(TokenKind::CloseSquare),
        TokenKind::OpenCurly => Some(TokenKind::CloseCurly),
        _ => None,
    }
}

fn whitespace(token: &TokenKind) -> bool {
    matches!(token, TokenKind::Whitespace)
}

impl TokenValue {
    pub(super) fn new(tokens: &[TokenKind]) -> Result<Self, Failure> {
        let tokens: Arc<[TokenKind]> = tokens.into();
        if tokens.len() > MAX_VALUE_TOKENS || value_bytes(&tokens) > MAX_VALUE_BYTES {
            return Ok(Self {
                tokens,
                references: None,
            });
        }
        let mut ends = vec![0; tokens.len()];
        let mut stack = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            if closer(token).is_some() {
                stack.push(index);
            } else if matches!(
                token,
                TokenKind::CloseParen | TokenKind::CloseSquare | TokenKind::CloseCurly
            ) {
                let open = stack.pop().ok_or(Failure::Grammar)?;
                if closer(&tokens[open]).as_ref() != Some(token) {
                    return Err(Failure::Grammar);
                }
                ends[open] = index;
            } else if matches!(
                token,
                TokenKind::BadString | TokenKind::BadUrl | TokenKind::Eof
            ) || stack.is_empty()
                && matches!(token, TokenKind::Delim('!') | TokenKind::Semicolon)
            {
                return Err(Failure::Grammar);
            }
        }
        if !stack.is_empty() {
            return Err(Failure::Grammar);
        }
        let mut references = BTreeMap::new();
        for (index, token) in tokens.iter().enumerate() {
            if !matches!(token, TokenKind::Function(name) if name.eq_ignore_ascii_case("var")) {
                continue;
            }
            let end = ends[index];
            let mut first = index + 1;
            while first < end && whitespace(&tokens[first]) {
                first += 1;
            }
            let Some(TokenKind::Ident(name)) = tokens.get(first) else {
                return Err(Failure::Grammar);
            };
            if !name.starts_with("--") || name == "--" {
                return Err(Failure::Grammar);
            }
            let mut after = first + 1;
            while after < end && whitespace(&tokens[after]) {
                after += 1;
            }
            let fallback = if after == end {
                None
            } else if matches!(tokens[after], TokenKind::Comma) {
                let mut part = after + 1;
                while part < end {
                    if matches!(tokens[part], TokenKind::Delim('!') | TokenKind::Semicolon) {
                        return Err(Failure::Grammar);
                    }
                    part = if ends[part] > part {
                        ends[part] + 1
                    } else {
                        part + 1
                    };
                }
                Some(after + 1..end)
            } else {
                return Err(Failure::Grammar);
            };
            references.insert(
                index,
                Reference {
                    name: name.clone(),
                    end,
                    fallback,
                },
            );
        }
        Ok(Self {
            tokens,
            references: Some(references),
        })
    }

    pub(super) fn substitute(
        &self,
        environment: &Environment,
        budget: &mut Budget,
    ) -> Result<Arc<[TokenKind]>, Failure> {
        let references = self.references.as_ref().ok_or(Failure::Limit)?;
        let mut output = Vec::new();
        let mut bytes = 0usize;
        let mut work: Vec<Range<usize>> = std::iter::once(0..self.tokens.len()).collect();
        while let Some(mut range) = work.pop() {
            while range.start < range.end {
                if let Some(reference) = references.get(&range.start) {
                    if let Some(Some(value)) = environment.get(&reference.name) {
                        append(&mut output, &mut bytes, value, budget)?;
                        range.start = reference.end + 1;
                    } else if let Some(fallback) = &reference.fallback {
                        work.push(reference.end + 1..range.end);
                        range = fallback.clone();
                    } else {
                        return Err(Failure::Missing);
                    }
                } else {
                    append(
                        &mut output,
                        &mut bytes,
                        &self.tokens[range.start..range.start + 1],
                        budget,
                    )?;
                    range.start += 1;
                }
            }
        }
        Ok(output.into())
    }
}

#[derive(Default)]
pub(super) struct Budget {
    tokens: usize,
    bytes: usize,
}

fn append(
    output: &mut Vec<TokenKind>,
    bytes: &mut usize,
    tokens: &[TokenKind],
    budget: &mut Budget,
) -> Result<(), Failure> {
    let added = value_bytes(tokens);
    if output.len().saturating_add(tokens.len()) > MAX_VALUE_TOKENS
        || bytes.saturating_add(added) > MAX_VALUE_BYTES
        || budget.tokens.saturating_add(tokens.len()) > MAX_ENV_TOKENS
        || budget.bytes.saturating_add(added) > MAX_ENV_BYTES
    {
        return Err(Failure::Limit);
    }
    *bytes += added;
    budget.tokens += tokens.len();
    budget.bytes += added;
    output.extend_from_slice(tokens);
    Ok(())
}

pub(super) fn report(
    diagnostics: &mut Vec<Diagnostic>,
    node_id: NodeId,
    name: &str,
    failure: Failure,
) {
    // Variable computation is per element; do not multiply untrusted diagnostics
    // by the number of matched nodes. Existing parser diagnostics are retained.
    let name: String = name.chars().take(80).collect();
    report_message(
        diagnostics,
        Some(node_id),
        format!(
            "{name}: {} (invalid at computed-value time)",
            failure.description()
        ),
    );
}

pub(super) fn report_message(
    diagnostics: &mut Vec<Diagnostic>,
    node_id: Option<NodeId>,
    message: String,
) {
    if diagnostics.len().saturating_add(1) < MAX_PARSE_ERRORS {
        diagnostics.push(Diagnostic { node_id, message });
    } else if diagnostics.len() < MAX_PARSE_ERRORS {
        diagnostics.push(Diagnostic {
            node_id,
            message: "additional CSS variable diagnostics omitted".into(),
        });
    }
}

pub(super) fn resolve(
    parent: &Arc<Environment>,
    specified: &BTreeMap<String, Value>,
    budget: &mut Budget,
    node_id: NodeId,
    diagnostics: &mut Vec<Diagnostic>,
    registrations: &Registrations,
) -> Arc<Environment> {
    if specified.is_empty() && registrations.is_empty() {
        return Arc::clone(parent);
    }
    Arc::new(resolve_owned(
        parent,
        specified,
        budget,
        node_id,
        diagnostics,
        registrations,
    ))
}

fn resolve_owned(
    parent: &Environment,
    specified: &BTreeMap<String, Value>,
    budget: &mut Budget,
    node_id: NodeId,
    diagnostics: &mut Vec<Diagnostic>,
    registrations: &Registrations,
) -> Environment {
    let mut environment = parent.clone();
    for (name, registration) in registrations {
        environment.insert(name.clone(), registration.default_value(parent, name));
    }
    let mut raw = Vec::new();
    for (name, value) in specified {
        match value {
            Value::Wide(WideKeyword::Inherit) => {
                environment.insert(
                    name.clone(),
                    parent.get(name).cloned().flatten().or_else(|| {
                        registrations
                            .get(name)
                            .and_then(|registration| registration.initial.clone())
                    }),
                );
            }
            Value::Wide(WideKeyword::Unset) => {
                environment.insert(
                    name.clone(),
                    registrations.get(name).map_or_else(
                        || parent.get(name).cloned().flatten(),
                        |registration| registration.default_value(parent, name),
                    ),
                );
            }
            Value::Wide(WideKeyword::Initial) => {
                environment.insert(
                    name.clone(),
                    registrations
                        .get(name)
                        .and_then(|registration| registration.initial.clone()),
                );
            }
            Value::Custom(value) => {
                if !registrations.contains_key(name) {
                    environment.insert(name.clone(), None);
                }
                raw.push((name, value));
            }
            _ => unreachable!("custom properties contain tokens or CSS-wide keywords"),
        }
    }
    if environment.len() > MAX_CUSTOM_PROPERTIES {
        report(diagnostics, node_id, "custom properties", Failure::Limit);
        return Environment::new();
    }
    let input_tokens = raw.iter().fold(0usize, |total, (_, value)| {
        total.saturating_add(value.tokens.len())
    });
    let input_bytes = raw.iter().fold(0usize, |total, (_, value)| {
        total.saturating_add(value_bytes(&value.tokens))
    });
    if input_tokens > MAX_ENV_TOKENS || input_bytes > MAX_ENV_BYTES {
        report(diagnostics, node_id, "custom properties", Failure::Limit);
        return environment;
    }
    let indices: BTreeMap<_, _> = raw
        .iter()
        .enumerate()
        .map(|(id, (name, _))| (name.as_str(), id))
        .collect();
    let mut graph = vec![Vec::new(); raw.len()];
    let mut reverse = vec![Vec::new(); raw.len()];
    for (id, (_, value)) in raw.iter().enumerate() {
        if let Some(references) = &value.references {
            for reference in references.values() {
                if let Some(&dependency) = indices.get(reference.name.as_str()) {
                    graph[id].push(dependency);
                }
            }
            graph[id].sort_unstable();
            graph[id].dedup();
            for &dependency in &graph[id] {
                reverse[dependency].push(id);
            }
        }
    }
    let cyclic = cycles(&graph, &reverse);
    if cyclic.iter().any(|&cycle| cycle) {
        report_message(diagnostics, Some(node_id), "CSS variable dependency cycle (including fallback references); unregistered members are guaranteed-invalid and registered members use their inherited or initial default".into());
    }
    // Cycle members are already invalid; other nodes must still be allowed to
    // recover through fallbacks. Inherited values have no unresolved edges.
    let mut remaining: Vec<_> = graph
        .iter()
        .map(|edges| edges.iter().filter(|&&id| !cyclic[id]).count())
        .collect();
    let mut ready: VecDeque<_> = (0..raw.len())
        .filter(|&id| !cyclic[id] && remaining[id] == 0)
        .collect();
    while let Some(id) = ready.pop_front() {
        let (name, value) = raw[id];
        match value.substitute(&environment, budget) {
            Ok(value) => {
                if let Some(registration) = registrations.get(name) {
                    if let Some(value) = registration.compute(&value) {
                        environment.insert(name.clone(), Some(value));
                    } else {
                        report_message(
                            diagnostics,
                            Some(node_id),
                            format!(
                                "{name}: invalid registered typed value (relative registered lengths/currentcolor are unsupported); using inherited or initial value"
                            ),
                        );
                    }
                } else {
                    environment.insert(name.clone(), Some(value));
                }
            }
            Err(failure) => report(diagnostics, node_id, name, failure),
        }
        for &dependent in &reverse[id] {
            if !cyclic[dependent] {
                remaining[dependent] -= 1;
                if remaining[dependent] == 0 {
                    ready.push_back(dependent);
                }
            }
        }
    }
    environment
}

fn cycles(graph: &[Vec<usize>], reverse: &[Vec<usize>]) -> Vec<bool> {
    let mut visited = vec![false; graph.len()];
    let mut order = Vec::new();
    for root in 0..graph.len() {
        if visited[root] {
            continue;
        }
        let mut stack = vec![(root, false)];
        while let Some((id, exit)) = stack.pop() {
            if exit {
                order.push(id);
            } else if !visited[id] {
                visited[id] = true;
                stack.push((id, true));
                stack.extend(graph[id].iter().rev().map(|&next| (next, false)));
            }
        }
    }
    visited.fill(false);
    let mut cyclic = vec![false; graph.len()];
    for &root in order.iter().rev() {
        if visited[root] {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![root];
        visited[root] = true;
        while let Some(id) = stack.pop() {
            component.push(id);
            for &next in &reverse[id] {
                if !visited[next] {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }
        if component.len() > 1 || graph[root].contains(&root) {
            for id in component {
                cyclic[id] = true;
            }
        }
    }
    cyclic
}
