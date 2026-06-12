//! Structural and diagnostic inspection of a [`LayerTree`].
//!
//! These free functions are the single source of truth for layer-tree
//! introspection — `flui-rendering`'s render harness re-uses them rather than
//! reimplementing the walk.

use flui_foundation::{Diagnosticable, DiagnosticsNode, LayerId};
use flui_painting::DisplayListCore;
use flui_types::Rect;

use crate::{Layer, LayerTree};

/// Returns the short kind name of a layer (e.g. `"Picture"`).
#[must_use]
pub fn layer_kind(layer: &Layer) -> &'static str {
    layer.kind_name()
}

/// Returns the layer kinds in pre-order (parent before children) as a flat
/// list.
#[must_use]
pub fn structure(tree: &LayerTree) -> Vec<&'static str> {
    structure_with_depth(tree)
        .into_iter()
        .map(|(_, kind)| kind)
        .collect()
}

/// Returns the layer kinds in pre-order, each paired with its depth from the
/// root (root = 0).
#[must_use]
pub fn structure_with_depth(tree: &LayerTree) -> Vec<(usize, &'static str)> {
    fn walk(tree: &LayerTree, id: LayerId, depth: usize, out: &mut Vec<(usize, &'static str)>) {
        let Some(node) = tree.get(id) else {
            return;
        };
        out.push((depth, node.layer().kind_name()));
        for &child in node.children() {
            walk(tree, child, depth + 1, out);
        }
    }

    let mut out = Vec::new();
    if let Some(root) = tree.root() {
        walk(tree, root, 0, &mut out);
    }
    out
}

/// Returns the bounds of the first `Picture` layer found in pre-order, or
/// `None` if the tree contains no picture.
#[must_use]
pub fn first_picture_bounds(tree: &LayerTree) -> Option<Rect> {
    fn find(tree: &LayerTree, id: LayerId) -> Option<Rect> {
        let node = tree.get(id)?;
        if let Layer::Picture(picture) = node.layer() {
            return Some(picture.picture().bounds());
        }
        node.children().iter().find_map(|&child| find(tree, child))
    }
    find(tree, tree.root()?)
}

/// Builds a [`DiagnosticsNode`] tree mirroring the layer hierarchy: each
/// node self-describes via [`Diagnosticable::to_diagnostics_node`] and the
/// tree links supply the parent/child structure.
#[must_use]
pub fn diagnostics_tree(tree: &LayerTree) -> Option<DiagnosticsNode> {
    fn subtree(tree: &LayerTree, id: LayerId) -> Option<DiagnosticsNode> {
        let node = tree.get(id)?;
        let mut diagnostics = node.to_diagnostics_node();
        for &child in node.children() {
            if let Some(child_diagnostics) = subtree(tree, child) {
                diagnostics.add_child(child_diagnostics);
            }
        }
        Some(diagnostics)
    }
    subtree(tree, tree.root()?)
}

/// Returns the alpha of the first [`Layer::Opacity`] node in pre-order, or
/// `None` if the tree contains no opacity layer (fully-opaque subtrees paint
/// directly per Flutter parity).
#[must_use]
pub fn first_opacity_alpha(tree: &LayerTree) -> Option<f32> {
    fn find(tree: &LayerTree, id: LayerId) -> Option<f32> {
        let node = tree.get(id)?;
        if let Layer::Opacity(opacity) = node.layer() {
            return Some(opacity.alpha());
        }
        node.children().iter().find_map(|&child| find(tree, child))
    }
    find(tree, tree.root()?)
}

/// Returns whether the tree contains any [`Layer::Picture`] node in pre-order.
#[must_use]
pub fn has_picture_layer(tree: &LayerTree) -> bool {
    fn find(tree: &LayerTree, id: LayerId) -> bool {
        let Some(node) = tree.get(id) else {
            return false;
        };
        matches!(node.layer(), Layer::Picture(_))
            || node.children().iter().any(|&child| find(tree, child))
    }
    tree.root().is_some_and(|root| find(tree, root))
}
