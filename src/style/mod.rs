//! Styling, independent of resource loading, layout, and paint.
//!
//! Unsupported syntax is reported and ignored. Token-level syntax recovery keeps
//! valid rules and declarations in embedded, external, and inline CSS.
//! Three-valued media queries gate rules and stylesheet media attributes.
//! Container conditions/units use an explicit completed-layout snapshot phase.
//! Layer, supports, namespace, scope, nesting, and bounded property-registration
//! semantics are compiled from retained ASTs; font-face selection comes later.
//!
//! The cascade orders user-agent, user, and author origins, reversing their
//! precedence for `!important`. Optional user CSS is supplied through
//! `resolve_with_user_stylesheets`. CSS-wide keywords apply to every supported
//! property and expand through the supported shorthands.
//! Layer registries span each origin's sheets. Cascade candidates preserve
//! origin/layer rollback, including important intervals and inline exceptions.
//! Unregistered custom properties inherit computed token streams; `var()`
//! substitution runs after cascading, including pending shorthand expansion.
//! Invalid computed values become unset rather than reviving earlier declarations.
//! HTML quirks are declaration-local and retained by pending substitutions.
//! Only the exact supported Compat alias enters generated canonical dispatch;
//! text fill remains separate from color and its currentcolor dependencies.
//!
//! Compatibility entry points use a screen viewport of 800 × 600 CSS pixels.
//! The `*_with_environment` / `*_and_environment` variants accept
//! [`crate::css::media::MediaEnvironment`]. Resolve from collected, unresolved
//! sheets on each viewport change; [`crate::engine::Frame`] retains that input
//! separately to avoid accumulating resolution diagnostics during resize.

mod cascade;
mod compat;
mod computed;
mod container;
mod control;
mod declarations;
mod generated_properties;
mod matching;
pub mod metadata;
mod property;
mod stylesheets;
mod variables;

pub use crate::css::selectors::{InteractionState, PseudoElement};
pub use cascade::resolve_with_container_environment;
#[cfg(feature = "style-bench-reference")]
#[doc(hidden)]
pub use cascade::{
    BenchmarkMatching, resolve_for_benchmark, resolve_for_benchmark_with_containers,
    resolve_for_benchmark_with_environment, resolve_for_benchmark_with_stylesheets,
};
pub use cascade::{
    StyleResolution, resolve, resolve_with_environment, resolve_with_style_environment,
    resolve_with_stylesheets, resolve_with_stylesheets_and_environment,
    resolve_with_user_stylesheets, resolve_with_user_stylesheets_and_environment,
};
pub use computed::{
    BaselineShift, BaselineSource, BoxSizing, ComputedStyle, ComputedStyles, Content, ContentItem,
    Direction, Display, TextAlign, TextAlignLast, VerticalAlign,
};
pub use container::ContainerSnapshot;
pub(crate) use stylesheets::report_css_errors;
pub(crate) fn import_supported(
    import: &crate::css::control::Import,
    context: compat::DeclarationContext,
) -> bool {
    import.supports.as_ref().is_none_or(|source| {
        control::supports_with_context(
            source,
            &crate::css::selectors::Namespaces::default(),
            None,
            &mut Vec::new(),
            context,
        )
    })
}
pub(crate) use compat::DeclarationContext;
pub use stylesheets::{
    AuthorStylesheet, Diagnostic, StylesheetCollection, collect_stylesheets,
    collect_stylesheets_with_external, embedded_stylesheet_source,
};

#[cfg(test)]
mod alignment_tests;
#[cfg(test)]
mod color_tests;
#[cfg(test)]
mod compat_tests;
#[cfg(test)]
mod container_tests;
#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod margin_tests;
#[cfg(test)]
mod math_tests;
#[cfg(test)]
mod media_tests;
#[cfg(test)]
mod metadata_tests;
#[cfg(test)]
mod selectors_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod values_tests;
#[cfg(test)]
mod variables_tests;
