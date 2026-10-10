//! The milestone-4 selector subset: compounds, descendant combinators, and lists.

use std::{error::Error, fmt};

use crate::dom::{Document, ElementData, NodeId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    pub ids: u32,
    pub classes: u32,
    pub types: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Compound {
    tag: Option<String>,
    ids: Vec<String>,
    classes: Vec<String>,
}

impl Compound {
    fn matches(&self, element: &ElementData) -> bool {
        self.tag
            .as_ref()
            .is_none_or(|tag| element.tag_name().eq_ignore_ascii_case(tag))
            && self
                .ids
                .iter()
                .all(|id| element.attribute("id") == Some(id.as_str()))
            && self.classes.iter().all(|class| {
                element
                    .attribute("class")
                    .is_some_and(|value| value.split_ascii_whitespace().any(|token| token == class))
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    compounds: Vec<Compound>,
    specificity: Specificity,
}

impl Selector {
    pub fn specificity(&self) -> Specificity {
        self.specificity
    }

    pub fn matches(&self, document: &Document, node_id: NodeId) -> bool {
        let Some(node) = document.node(node_id) else {
            return false;
        };
        let Some(element) = node.as_element() else {
            return false;
        };
        let mut compounds = self.compounds.iter().rev();
        if !compounds.next().is_some_and(|last| last.matches(element)) {
            return false;
        }
        let mut ancestor = node.parent();
        for compound in compounds {
            loop {
                let Some(id) = ancestor else {
                    return false;
                };
                let Some(node) = document.node(id) else {
                    return false;
                };
                ancestor = node.parent();
                if node
                    .as_element()
                    .is_some_and(|element| compound.matches(element))
                {
                    break;
                }
            }
        }
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorError;

impl fmt::Display for SelectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("invalid or unsupported selector (expected type, class, ID, or descendant)")
    }
}

impl Error for SelectorError {}

pub fn parse(source: &str) -> Result<Vec<Selector>, SelectorError> {
    let source = super::without_comments(source).ok_or(SelectorError)?;
    source
        .split(',')
        .map(|selector| {
            let mut specificity = Specificity::default();
            let compounds = selector
                .split_ascii_whitespace()
                .map(|compound| parse_compound(compound, &mut specificity))
                .collect::<Result<Vec<_>, _>>()?;
            if compounds.is_empty() {
                return Err(SelectorError);
            }
            Ok(Selector {
                compounds,
                specificity,
            })
        })
        .collect()
}

fn parse_compound(source: &str, specificity: &mut Specificity) -> Result<Compound, SelectorError> {
    let mut chars = source.chars().peekable();
    let mut compound = Compound {
        tag: None,
        ids: Vec::new(),
        classes: Vec::new(),
    };
    if chars.peek() == Some(&'*') {
        chars.next();
    } else if chars.peek().is_some_and(|ch| identifier_start(*ch)) {
        compound.tag = Some(identifier(&mut chars)?);
        specificity.types += 1;
    }
    while let Some(prefix) = chars.next() {
        match prefix {
            '.' => {
                compound.classes.push(identifier(&mut chars)?);
                specificity.classes += 1;
            }
            '#' => {
                compound.ids.push(identifier(&mut chars)?);
                specificity.ids += 1;
            }
            _ => return Err(SelectorError),
        }
    }
    Ok(compound)
}

fn identifier(
    chars: &mut std::iter::Peekable<impl Iterator<Item = char>>,
) -> Result<String, SelectorError> {
    if !chars.peek().is_some_and(|ch| identifier_start(*ch)) {
        return Err(SelectorError);
    }
    let mut result = String::new();
    while let Some(ch) = chars.peek().copied() {
        if !(identifier_start(ch) || ch.is_ascii_digit()) {
            break;
        }
        result.push(ch);
        chars.next();
    }
    Ok(result)
}

fn identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || matches!(ch, '_' | '-') || !ch.is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{Attribute, ElementData};

    #[test]
    fn rejects_unterminated_selector_comments() {
        assert!(parse("p/*").is_err());
    }

    #[test]
    fn computes_lexicographic_specificity_for_compounds_and_lists() {
        let selectors = parse("main .notice p#hero.active, *, h1").unwrap();
        assert_eq!(
            selectors[0].specificity(),
            Specificity {
                ids: 1,
                classes: 2,
                types: 2
            }
        );
        assert_eq!(selectors[1].specificity(), Specificity::default());
        assert!(selectors[0].specificity() > selectors[2].specificity());
        assert_eq!(parse("p/**/.notice").unwrap()[0].specificity().classes, 1);
    }

    #[test]
    fn matches_ancestors_not_siblings_and_preserves_class_and_id_case() {
        let mut document = Document::new();
        let outer = document
            .append_element(
                document.root_id(),
                ElementData::new(
                    "main".into(),
                    vec![Attribute::new("class".into(), "notice extra".into())],
                ),
            )
            .unwrap();
        let inner = document
            .append_element(outer, ElementData::new("div".into(), vec![]))
            .unwrap();
        let paragraph = document
            .append_element(
                inner,
                ElementData::new("p".into(), vec![Attribute::new("id".into(), "Hero".into())]),
            )
            .unwrap();
        assert!(parse("MAIN.notice #Hero").unwrap()[0].matches(&document, paragraph));
        assert!(!parse("main #hero").unwrap()[0].matches(&document, paragraph));
        assert!(!parse(".not").unwrap()[0].matches(&document, paragraph));
        let sibling = document
            .append_element(document.root_id(), ElementData::new("p".into(), vec![]))
            .unwrap();
        assert!(!parse("main p").unwrap()[0].matches(&document, sibling));
        assert!(!parse("*").unwrap()[0].matches(&document, document.root_id()));
    }

    #[test]
    fn rejects_unsupported_or_empty_selectors_without_partial_matching() {
        for source in [
            "", "p,", "p > a", "p:hover", "[id=x]", ".", "#", "p+q", "p, :bad",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }
}
