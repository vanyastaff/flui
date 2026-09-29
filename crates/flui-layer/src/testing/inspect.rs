//! Structural and diagnostic inspection of a [`LayerTree`] for tests.
//!
//! These free functions are the single source of truth for layer-tree
//! introspection — `flui-rendering`'s render harness re-exports them rather
//! than reimplementing the walk. Every walk is `LayerTree::descendants`
//! (pre-order, siblings in paint order, explicit stack), so a deep chain
//! costs no Rust stack.

use flui_foundation::geometry::{Matrix4, RRect, Rect};
use flui_foundation::{Diagnosticable, DiagnosticsNode, LayerId};
use flui_painting::paint::Path;

use crate::{Layer, LayerTree};

/// The variant name of `layer` — see [`Layer::kind_name`].
#[must_use]
pub fn layer_kind(layer: &Layer) -> &'static str {
    layer.kind_name()
}

/// Every layer in pre-order with its depth.
fn walk(tree: &LayerTree) -> impl Iterator<Item = (usize, &Layer)> + '_ {
    tree.descendants(tree.root())
        .filter_map(move |(id, depth)| tree.get_layer(id).map(|layer| (depth, layer)))
}

/// Every value `pick` returns, in pre-order.
fn collect<'a, T>(tree: &'a LayerTree, mut pick: impl FnMut(&'a Layer) -> Option<T>) -> Vec<T> {
    walk(tree).filter_map(|(_, layer)| pick(layer)).collect()
}

/// The first value `pick` returns, in pre-order.
fn find_first<T>(tree: &LayerTree, mut pick: impl FnMut(&Layer) -> Option<T>) -> Option<T> {
    walk(tree).find_map(|(_, layer)| pick(layer))
}

/// Variant names in pre-order: `["Offset", "ClipRect", "Picture", …]`.
#[must_use]
pub fn structure(tree: &LayerTree) -> Vec<&'static str> {
    walk(tree).map(|(_, layer)| layer.kind_name()).collect()
}

/// Variant names with their depth, in pre-order.
#[must_use]
pub fn structure_with_depth(tree: &LayerTree) -> Vec<(usize, &'static str)> {
    walk(tree)
        .map(|(depth, layer)| (depth, layer.kind_name()))
        .collect()
}

/// The bounds of the first `Picture` layer.
#[must_use]
pub fn first_picture_bounds(tree: &LayerTree) -> Option<Rect> {
    find_first(tree, |layer| match layer {
        Layer::Picture(picture) => picture.bounds(),
        _ => None,
    })
}

/// The whole tree as nested [`DiagnosticsNode`]s.
///
/// Recursive on purpose: the nested node is composed bottom-up, child before
/// parent, which a flat pre-order visit cannot express. This is a debug
/// dump, not one of the flat query walkers.
#[must_use]
pub fn diagnostics_tree(tree: &LayerTree) -> DiagnosticsNode {
    fn subtree(tree: &LayerTree, id: LayerId) -> DiagnosticsNode {
        let node = tree
            .get(id)
            .expect("BUG: a LayerTree child id is always a node of that tree");
        let mut diagnostics = node.to_diagnostics_node();
        for &child in node.children() {
            diagnostics.add_child(subtree(tree, child));
        }
        diagnostics
    }
    subtree(tree, tree.root())
}

/// The alpha of the first `Opacity` layer.
#[must_use]
pub fn first_opacity_alpha(tree: &LayerTree) -> Option<f64> {
    find_first(tree, |layer| match layer {
        Layer::Opacity(opacity) => Some(opacity.alpha()),
        _ => None,
    })
}

/// Every `Transform` layer's matrix, in pre-order.
#[must_use]
pub fn transform_matrices(tree: &LayerTree) -> Vec<Matrix4> {
    collect(tree, |layer| match layer {
        Layer::Transform(transform) => Some(*transform.transform()),
        _ => None,
    })
}

/// The matrix of the first `Transform` layer.
#[must_use]
pub fn first_transform_matrix(tree: &LayerTree) -> Option<Matrix4> {
    find_first(tree, |layer| match layer {
        Layer::Transform(transform) => Some(*transform.transform()),
        _ => None,
    })
}

/// Every `ClipRect` layer's rectangle, in pre-order.
#[must_use]
pub fn clip_rects(tree: &LayerTree) -> Vec<Rect> {
    collect(tree, |layer| match layer {
        Layer::ClipRect(clip) => Some(clip.clip_rect()),
        _ => None,
    })
}

/// Every `ClipRRect` layer's rounded rectangle, in pre-order.
#[must_use]
pub fn clip_rrects(tree: &LayerTree) -> Vec<RRect> {
    collect(tree, |layer| match layer {
        Layer::ClipRRect(clip) => Some(*clip.clip_rrect()),
        _ => None,
    })
}

/// Every `ClipPath` layer's path, in pre-order.
///
/// Borrows the recorded paths; assert on containment, since `Path` has no `PartialEq`.
#[must_use]
pub fn clip_paths(tree: &LayerTree) -> Vec<&Path> {
    collect(tree, |layer| match layer {
        Layer::ClipPath(clip) => Some(clip.clip_path()),
        _ => None,
    })
}

/// Whether any `Picture` layer exists.
#[must_use]
pub fn has_picture_layer(tree: &LayerTree) -> bool {
    find_first(tree, |layer| {
        matches!(layer, Layer::Picture(_)).then_some(())
    })
    .is_some()
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::Matrix4;

    use super::*;
    use crate::{OffsetLayer, OpacityLayer, TransformLayer};

    fn offset() -> Layer {
        Layer::from(OffsetLayer::zero())
    }

    #[test]
    fn walkers_visit_in_pre_order_with_siblings_in_paint_order() {
        let mut tree = LayerTree::new(offset());
        let root = tree.root();
        let a = tree.push_child(root, Layer::from(OpacityLayer::new(0.25)));
        let _ = tree.push_child(
            a,
            Layer::from(TransformLayer::new(Matrix4::scaling(2.0, 2.0, 1.0))),
        );
        let _ = tree.push_child(
            root,
            Layer::from(TransformLayer::new(Matrix4::scaling(3.0, 3.0, 1.0))),
        );

        assert_eq!(
            structure_with_depth(&tree),
            vec![
                (0, "Offset"),
                (1, "Opacity"),
                (2, "Transform"),
                (1, "Transform")
            ],
        );
        assert_eq!(first_opacity_alpha(&tree), Some(0.25));
        assert_eq!(
            transform_matrices(&tree),
            vec![
                Matrix4::scaling(2.0, 2.0, 1.0),
                Matrix4::scaling(3.0, 3.0, 1.0)
            ],
            "a's subtree is finished before b is visited",
        );
        assert_eq!(
            first_transform_matrix(&tree),
            Some(Matrix4::scaling(2.0, 2.0, 1.0))
        );
        assert!(!has_picture_layer(&tree));
        assert_eq!(structure(&LayerTree::default()), vec!["Offset"]);
    }
}
