use std::collections::HashMap;

use crate::css::selectors::{
    InteractionState, MatchLimitExceeded, PseudoElement, Selector,
    matching::{AncestorBloom, LookupKey},
};
use crate::dom::NodeId;

use super::Diagnostic;

pub(super) const MAX_MATCH_DIAGNOSTICS: usize = 32;

#[derive(Default)]
pub(super) struct MatchDiagnostics {
    pub(super) entries: Vec<Diagnostic>,
}

impl MatchDiagnostics {
    pub(super) fn query_error(&mut self, node_id: NodeId, message: String) {
        if self.entries.iter().any(|entry| entry.message == message) {
            return;
        }
        if self.entries.len() < MAX_MATCH_DIAGNOSTICS {
            self.entries.push(Diagnostic {
                node_id: Some(node_id),
                message,
            });
        } else if self.entries.len() == MAX_MATCH_DIAGNOSTICS {
            self.entries.push(Diagnostic {
                node_id: Some(node_id),
                message: "additional container-query diagnostics omitted".into(),
            });
        }
    }

    pub(super) fn matched(
        &mut self,
        outcome: Result<bool, MatchLimitExceeded>,
        node_id: NodeId,
    ) -> bool {
        match outcome {
            Ok(matched) => matched,
            Err(error) => {
                self.report(node_id, error);
                false
            }
        }
    }

    pub(super) fn report(&mut self, node_id: NodeId, error: MatchLimitExceeded) {
        if self.entries.len() < MAX_MATCH_DIAGNOSTICS {
            self.entries.push(Diagnostic {
                node_id: Some(node_id),
                message: error.to_string(),
            });
        } else if self.entries.len() == MAX_MATCH_DIAGNOSTICS {
            self.entries.push(Diagnostic {
                node_id: Some(node_id),
                message: "additional selector matching diagnostics omitted".into(),
            });
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct RuleIndex {
    keyed: HashMap<LookupKey, Vec<usize>>,
    universal: Vec<usize>,
}

impl RuleIndex {
    pub(super) fn add_rule(&mut self, rule: usize, selectors: &[Selector]) {
        let Some(mut keys) = selectors
            .iter()
            .map(Selector::index_key)
            .collect::<Option<Vec<_>>>()
        else {
            // One unindexable list member makes the whole rule a fallback.
            self.universal.push(rule);
            return;
        };
        keys.sort_unstable();
        keys.dedup();
        for key in keys {
            self.keyed.entry(key).or_default().push(rule);
        }
    }

    pub(super) fn collect(&self, keys: &[LookupKey], scratch: &mut Candidates, rules: usize) {
        scratch.next(rules);
        for &rule in &self.universal {
            scratch.add(rule);
        }
        for key in keys {
            if let Some(rules) = self.keyed.get(key) {
                for &rule in rules {
                    scratch.add(rule);
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Candidates {
    pub(super) rules: Vec<usize>,
    seen: Vec<u32>,
    generation: u32,
}

impl Candidates {
    fn next(&mut self, count: usize) {
        self.rules.clear();
        self.seen.resize(self.seen.len().max(count), 0);
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.seen.fill(0);
            self.generation = 1;
        }
    }

    fn add(&mut self, rule: usize) {
        if self.seen[rule] != self.generation {
            self.seen[rule] = self.generation;
            self.rules.push(rule);
        }
    }
}

#[derive(Default)]
pub(super) struct MatchingContext {
    pub(super) containers: super::ContainerSnapshot,
    pub(super) environment: crate::css::media::MediaEnvironment,
    pub(super) state: InteractionState,
    pub(super) pseudo: Option<PseudoElement>,
    pub(super) ancestors: AncestorBloom,
    pub(super) keys: Vec<LookupKey>,
    pub(super) candidates: Candidates,
    pub(super) diagnostics: MatchDiagnostics,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{css::selectors, dom::ElementData};

    #[test]
    fn candidates_deduplicate_rules_and_reset_between_sheets_and_generation_wrap() {
        let mut index = RuleIndex::default();
        index.add_rule(0, &selectors::parse("#x, .a, .b, .a").unwrap());
        index.add_rule(1, &selectors::parse(":not(.a), #x").unwrap());
        index.add_rule(2, &selectors::parse("DIV").unwrap());
        index.add_rule(3, &selectors::parse(".absent").unwrap());
        let element = ElementData::new(
            "div".into(),
            vec![
                crate::dom::Attribute::new("id".into(), "x".into()),
                crate::dom::Attribute::new("class".into(), "a b a".into()),
            ],
        );
        let keys = crate::css::selectors::matching::element_keys(&element).collect::<Vec<_>>();
        let mut scratch = Candidates::default();
        index.collect(&keys, &mut scratch, 4);
        scratch.rules.sort_unstable();
        assert_eq!(scratch.rules, [0, 1, 2]);
        let mut other = RuleIndex::default();
        other.add_rule(0, &selectors::parse("*").unwrap());
        other.collect(&keys, &mut scratch, 1);
        assert_eq!(scratch.rules, [0]);
        scratch.generation = u32::MAX;
        index.collect(&keys, &mut scratch, 4);
        scratch.rules.sort_unstable();
        assert_eq!(scratch.rules, [0, 1, 2]);
    }
}
