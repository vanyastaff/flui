//! [`LayerOutline`]: the layers of a presented scene, as a text a test compares.

use std::fmt;

use flui_layer::LayerTree;
use flui_rendering::testing::inspect::layer_structure_with_depth;

/// The layers of the scene a window presented last, depth first, each its kind and its depth:
/// what [`Frame::layers`](super::Frame::layers) reads. It prints one layer a line, indented by
/// depth, so a test compares a whole outline as text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerOutline {
    layers: Vec<(usize, &'static str)>,
}

impl LayerOutline {
    /// The outline of `tree`; empty when no scene was presented.
    pub(super) fn of(tree: Option<&LayerTree>) -> Self {
        Self {
            layers: tree.map_or_default(layer_structure_with_depth),
        }
    }

    /// Each layer's depth, from 0 at the root, and its kind (`"Offset"`, `"ClipRect"`,
    /// `"Picture"`, …), depth first.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (usize, &'static str)> + '_ {
        self.layers.iter().copied()
    }

    /// Each layer's kind, depth first.
    pub fn kinds(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        self.layers.iter().map(|(_, kind)| *kind)
    }
}

impl fmt::Display for LayerOutline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (depth, kind)) in self.layers.iter().enumerate() {
            if index > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{:indent$}{kind}", "", indent = depth * 2)?;
        }
        Ok(())
    }
}
