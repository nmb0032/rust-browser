//! A naive single-document browser engine; session history and Fetch come later.

mod css_imports;
mod frame;
mod history;
mod loader;
mod navigation;
mod printing;
pub(crate) mod resource_policy;
mod resources;
mod timing;
mod urls;
mod web_fonts;

pub use frame::{FragmentTarget, Frame, StylesheetSource};
pub use history::{History, HistoryEntry};
pub(crate) use loader::load_local_page;
pub use loader::{LoadOptions, load_page};
pub use navigation::{LoadEvent, Navigation};
pub use printing::{PrintLayout, PrintOptions, PrintedPage};
pub use timing::{StageTimings, frame_trace_enabled};
pub use urls::{document_base_url, link_url, resolve_reference};
