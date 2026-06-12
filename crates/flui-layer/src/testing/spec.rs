//! Declarative `LayerTree` construction.
//!
//! A [`LayerSpec`] describes a layer plus its children and an optional label;
//! [`mount`] inserts the spec into a [`LayerTree`], wiring parent/child links
//! and setting the root, and returns a label -> [`LayerId`] registry.

use std::collections::HashMap;

use flui_foundation::LayerId;

use crate::{Layer, LayerTree};

/// A node in a declarative layer-tree spec.
///
/// Build one with [`layer`], nest children with [`child`](LayerSpec::child) /
/// [`children`](LayerSpec::children), and tag it with
/// [`label`](LayerSpec::label) for lookup after mounting.
pub struct LayerSpec {
    layer: Layer,
    label: Option<&'static str>,
    children: Vec<LayerSpec>,
}

/// Creates a [`LayerSpec`] from any value convertible into a [`Layer`]
/// (e.g. `CanvasLayer`, `OffsetLayer`, or a `Layer` directly).
pub fn layer(layer: impl Into<Layer>) -> LayerSpec {
    LayerSpec {
        layer: layer.into(),
        label: None,
        children: Vec::new(),
    }
}

impl LayerSpec {
    /// Tags this node with a label for post-mount lookup.
    #[must_use]
    pub fn label(mut self, label: &'static str) -> Self {
        self.label = Some(label);
        self
    }

    /// Appends a single child spec.
    #[must_use]
    pub fn child(mut self, child: LayerSpec) -> Self {
        self.children.push(child);
        self
    }

    /// Appends every child spec from an iterator.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = LayerSpec>) -> Self {
        self.children.extend(children);
        self
    }
}

/// Maps `&'static str` labels to the `LayerId`s minted while mounting a spec.
#[derive(Debug, Default, Clone)]
pub struct LayerLabelRegistry {
    by_label: HashMap<&'static str, LayerId>,
}

impl LayerLabelRegistry {
    fn record(&mut self, label: &'static str, id: LayerId) {
        let previous = self.by_label.insert(label, id);
        assert!(
            previous.is_none(),
            "duplicate layer label in test tree: {label:?}"
        );
    }

    /// Returns the id for `label`, if one was registered.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<LayerId> {
        self.by_label.get(label).copied()
    }
}

/// Inserts `spec` into `tree`, wires children, sets the root, and returns the
/// root id plus the label registry.
pub fn mount(tree: &mut LayerTree, spec: LayerSpec) -> (LayerId, LayerLabelRegistry) {
    let mut registry = LayerLabelRegistry::default();
    let root = mount_node(tree, spec, &mut registry);
    tree.set_root(Some(root));
    (root, registry)
}

fn mount_node(tree: &mut LayerTree, spec: LayerSpec, registry: &mut LayerLabelRegistry) -> LayerId {
    let id = tree.insert(spec.layer);
    if let Some(label) = spec.label {
        registry.record(label, id);
    }
    for child in spec.children {
        let child_id = mount_node(tree, child, registry);
        tree.add_child(id, child_id);
    }
    id
}
