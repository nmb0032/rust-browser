/*!
 * This module defines the structure and behavior of the Document Object Model (DOM) for the Rust Browser.
 *
 * DOM will be modeled as an owned tree with stable IDs
 * Document -> Vec<Node> and has a root NodeId
 * Each node has a kind, ordered children, and parentId optionally
 *
 * Starting kind: Document, Element, Text
 *
 * Not as simple as Recursively nested Nodes but makes referencing later more convenient
 */
mod document;
mod node;

pub use document::{Document, DomError};
pub use node::{Attribute, ElementData, Node, NodeId, NodeKind};
