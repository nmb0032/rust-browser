use std::collections::HashMap;

use crate::{
    css::values::{Edges, Length},
    dom::NodeId,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    Block,
    #[default]
    Inline,
    ListItem,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: Option<u32>,
    pub background_color: Option<u32>,
    pub font_size: f32,
    pub font_family: String,
    pub bold: bool,
    pub italic: bool,
    pub width: Option<Length>,
    pub margin: Edges<Length>,
    pub padding: Edges<Length>,
    pub border_width: Edges<f32>,
    pub border_color: Edges<Option<u32>>,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            color: Some(0),
            background_color: None,
            font_size: 16.0,
            font_family: "sans-serif".into(),
            bold: false,
            italic: false,
            width: None,
            margin: Edges::default(),
            padding: Edges::default(),
            border_width: Edges::default(),
            border_color: Edges::all(Some(0)),
        }
    }
}

impl ComputedStyle {
    pub(super) fn inherit(parent: &Self) -> Self {
        Self {
            color: parent.color,
            font_size: parent.font_size,
            font_family: parent.font_family.clone(),
            bold: parent.bold,
            italic: parent.italic,
            border_color: Edges::all(parent.color),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct ComputedStyles {
    pub(super) nodes: HashMap<NodeId, ComputedStyle>,
}

impl ComputedStyles {
    pub fn get(&self, node_id: NodeId) -> Option<&ComputedStyle> {
        self.nodes.get(&node_id)
    }
}
