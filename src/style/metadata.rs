//! Frozen specification metadata is not a promise of executable CSS support.
//! Only generated finite dispatch routes enter the existing semantic parsers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    Implemented,
    Partial,
    Unimplemented,
    AliasUnimplemented,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grammar {
    Unsupported,
    /// Executable shared color parser; not the complete <color> grammar.
    ColorSubset,
    /// Executable generated keyword subset, not the entire spec production.
    Keywords(&'static [&'static str]),
    /// Executable dispatch to a handwritten finite semantic parser.
    SemanticSubset,
}

#[derive(Clone, Copy, Debug)]
pub struct PropertyMetadata {
    pub name: &'static str,
    pub href: &'static str,
    pub syntax: Option<&'static str>,
    pub initial: Option<&'static str>,
    pub inheritance: Option<&'static str>,
    pub inherited: Option<bool>,
    pub computed_value: Option<&'static str>,
    pub animation_type: Option<&'static str>,
    pub alias_of: Option<&'static str>,
    pub longhands: &'static [&'static str],
    pub reset_longhands: &'static [&'static str],
    pub grammar: Grammar,
    pub support: Support,
}

#[derive(Clone, Copy, Debug)]
pub struct SyntaxMetadata {
    pub name: &'static str,
    pub scopes: &'static [&'static str],
    pub href: &'static str,
    pub syntax: Option<&'static str>,
    pub initial: Option<&'static str>,
}

pub use super::generated_properties::{FUNCTIONS, PROPERTIES, PROPERTY_DESCRIPTORS, TYPES};

/// ASCII-case-insensitive lookup of public properties, including unsupported
/// ones. Custom property names are case-sensitive and are not snapshot entries.
pub fn property(name: &str) -> Option<&'static PropertyMetadata> {
    let name = name.to_ascii_lowercase();
    PROPERTIES
        .binary_search_by_key(&name.as_str(), |entry| entry.name)
        .ok()
        .map(|index| &PROPERTIES[index])
}

pub fn css_coverage() -> &'static str {
    include_str!("../../docs/css-coverage.md")
}
