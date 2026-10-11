//! Conservative prefilters. Hash collisions and ASCII folding can only admit
//! extra candidates; the selector matcher remains the authority.

use crate::dom::ElementData;

use super::{Combinator, Selector, SimpleSelector, css_whitespace};

const WORDS: usize = 8;
const BITS: usize = WORDS * 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LookupKey(u64);

impl LookupKey {
    fn new(kind: u8, value: &str) -> Self {
        let mut hash = 0xcbf29ce484222325_u64;
        for byte in std::iter::once(kind).chain(value.bytes().map(|byte| byte.to_ascii_lowercase()))
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        Self(hash)
    }

    fn positions(self) -> [usize; 2] {
        [
            self.0 as usize & (BITS - 1),
            (self.0 >> 32) as usize & (BITS - 1),
        ]
    }
}

pub(crate) fn element_keys(element: &ElementData) -> impl Iterator<Item = LookupKey> + '_ {
    std::iter::once(LookupKey::new(b't', element.tag_name()))
        .chain(element.attribute("id").map(|id| LookupKey::new(b'i', id)))
        .chain(
            element
                .attribute("class")
                .into_iter()
                .flat_map(|classes| classes.split(css_whitespace))
                .filter(|class| !class.is_empty())
                .map(|class| LookupKey::new(b'c', class)),
        )
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Bloom([u64; WORDS]);

impl Bloom {
    fn insert(&mut self, key: LookupKey) {
        for bit in key.positions() {
            self.0[bit / 64] |= 1 << (bit % 64);
        }
    }

    fn contains(&self, required: &Self) -> bool {
        self.0
            .iter()
            .zip(required.0)
            .all(|(actual, required)| actual & required == required)
    }
}

/// A counting filter needs constant space, rather than a filter snapshot for
/// every ancestor. Rehashing on exit avoids storing every ancestor's keys.
pub(crate) struct AncestorBloom {
    bits: Bloom,
    counts: [usize; BITS],
}

impl Default for AncestorBloom {
    fn default() -> Self {
        Self {
            bits: Bloom::default(),
            counts: [0; BITS],
        }
    }
}

impl AncestorBloom {
    pub(crate) fn push(&mut self, element: &ElementData) {
        for key in element_keys(element) {
            for bit in key.positions() {
                self.counts[bit] += 1;
                self.bits.0[bit / 64] |= 1 << (bit % 64);
            }
        }
    }

    pub(crate) fn pop(&mut self, element: &ElementData) {
        for key in element_keys(element) {
            for bit in key.positions() {
                self.counts[bit] -= 1;
                if self.counts[bit] == 0 {
                    self.bits.0[bit / 64] &= !(1 << (bit % 64));
                }
            }
        }
    }

    pub(crate) fn may_contain(&self, required: &Bloom) -> bool {
        self.bits.contains(required)
    }
}

impl Selector {
    pub(crate) fn index_key(&self) -> Option<LookupKey> {
        let rightmost = self.compounds.last().expect("nonempty parsed selector");
        rightmost
            .simple
            .iter()
            .find_map(|simple| match simple {
                SimpleSelector::Id(id) => Some(LookupKey::new(b'i', id)),
                _ => None,
            })
            .or_else(|| {
                rightmost.simple.iter().find_map(|simple| match simple {
                    SimpleSelector::Class(class) => Some(LookupKey::new(b'c', class)),
                    _ => None,
                })
            })
            .or_else(|| rightmost.tag.as_ref().map(|tag| LookupKey::new(b't', tag)))
    }

    pub(crate) fn ancestor_requirements(&self) -> Bloom {
        let mut required = Bloom::default();
        for (compound, combinator) in self.compounds.iter().zip(&self.combinators) {
            // Moving left across a vertical edge always reaches a proper
            // ancestor of the subject (also from its/its ancestor's sibling).
            // A sibling itself is NOT an ancestor. Never inspect :not().
            if !matches!(combinator, Combinator::Child | Combinator::Descendant) {
                continue;
            }
            if let Some(tag) = &compound.tag {
                required.insert(LookupKey::new(b't', tag));
            }
            for simple in &compound.simple {
                match simple {
                    SimpleSelector::Id(id) => required.insert(LookupKey::new(b'i', id)),
                    SimpleSelector::Class(class) => required.insert(LookupKey::new(b'c', class)),
                    _ => {}
                }
            }
        }
        required
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::Attribute;

    #[test]
    fn bloom_collisions_only_admit_extra_candidates_and_pop_preserves_counts() {
        let mut seen = std::collections::HashMap::new();
        let (original, collision) = (0..=BITS * BITS)
            .find_map(|i| {
                let name = format!("collision{i}");
                seen.insert(LookupKey::new(b'c', &name).positions(), name.clone())
                    .map(|previous| (previous, name))
            })
            .expect("pigeonhole guarantees a deterministic Bloom collision");
        let a = ElementData::new(
            "div".into(),
            vec![Attribute::new(
                "class".into(),
                format!("{original} {original}"),
            )],
        );
        let required =
            super::super::parse(&format!(".{collision} p")).unwrap()[0].ancestor_requirements();
        let mut bloom = AncestorBloom::default();
        assert!(!bloom.may_contain(&required));
        bloom.push(&a);
        bloom.push(&a);
        assert!(bloom.may_contain(&required));
        bloom.pop(&a);
        assert!(bloom.may_contain(&required));
        bloom.pop(&a);
        assert!(!bloom.may_contain(&required));
    }
}
