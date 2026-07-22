//! `LayerTester` — an ergonomic wrapper that mounts a [`LayerSpec`] and
//! exposes the inspection surface. Named to mirror
//! `flui_rendering::testing::RenderTester`.

use flui_foundation::{DiagnosticsNode, LayerId};
use flui_types::Rect;

use crate::{
    LayerTree,
    testing::{
        inspect,
        spec::{self, LayerLabelRegistry, LayerSpec},
    },
};

/// A mounted layer tree plus its label registry, ready to inspect.
#[derive(Debug)]
pub struct LayerTester {
    tree: LayerTree,
    root: LayerId,
    registry: LayerLabelRegistry,
}

impl LayerTester {
    /// Mounts a [`LayerSpec`] into a fresh [`LayerTree`].
    #[must_use]
    pub fn mount(spec: LayerSpec) -> Self {
        let mut tree = LayerTree::new();
        let (root, registry) = spec::mount(&mut tree, spec);
        Self {
            tree,
            root,
            registry,
        }
    }

    /// The root layer id.
    #[must_use]
    pub fn root(&self) -> LayerId {
        self.root
    }

    /// The underlying layer tree.
    #[must_use]
    pub fn tree(&self) -> &LayerTree {
        &self.tree
    }

    /// Mutable access to the underlying layer tree (mutate between checks).
    pub fn tree_mut(&mut self) -> &mut LayerTree {
        &mut self.tree
    }

    /// Resolves a label to its id, panicking if it was never registered.
    #[must_use]
    pub fn id(&self, label: &str) -> LayerId {
        self.try_id(label)
            .unwrap_or_else(|| panic!("no layer labeled {label:?} in the test tree"))
    }

    /// Resolves a label to its id, or `None` if unknown.
    #[must_use]
    pub fn try_id(&self, label: &str) -> Option<LayerId> {
        self.registry.get(label)
    }

    /// The kind name of the layer at `id`.
    #[must_use]
    pub fn kind(&self, id: LayerId) -> &'static str {
        self.tree
            .get(id)
            .expect("layer id must be live")
            .layer()
            .kind_name()
    }

    /// The layer kinds in pre-order.
    #[must_use]
    pub fn structure(&self) -> Vec<&'static str> {
        inspect::structure(&self.tree)
    }

    /// The layer kinds in pre-order with depth.
    #[must_use]
    pub fn structure_with_depth(&self) -> Vec<(usize, &'static str)> {
        inspect::structure_with_depth(&self.tree)
    }

    /// The bounds of the first picture layer, if any.
    #[must_use]
    pub fn first_picture_bounds(&self) -> Option<Rect> {
        inspect::first_picture_bounds(&self.tree)
    }

    /// A `Diagnosticable`-backed diagnostics tree mirroring the hierarchy.
    #[must_use]
    pub fn diagnostics(&self) -> Option<DiagnosticsNode> {
        inspect::diagnostics_tree(&self.tree)
    }

    /// A printable, indented dump of the diagnostics tree.
    #[must_use]
    pub fn dump(&self) -> String {
        self.diagnostics()
            .map(|node| node.to_string())
            .unwrap_or_default()
    }
}
