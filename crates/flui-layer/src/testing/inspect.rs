//! Structural and diagnostic inspection of a [`LayerTree`] for tests.
//!
//! The tree describes itself as an `InspectTree`, which a typed snapshot
//! reads; these free functions are its shortcuts for tests, named by layer
//! kind as the tree names it. `flui-rendering`'s render harness re-exports
//! them rather than reimplementing the walk. Every walk is `LayerTree::descendants`
//! (pre-order, siblings in paint order, explicit stack), so a deep chain
//! costs no Rust stack.

use flui_foundation::geometry::{Matrix4, RRect, Rect};
use flui_foundation::inspect::Inspect;
use flui_painting::paint::Path;

use crate::{Layer, LayerTree};

/// The variant name of `layer`, its [`Inspect`] kind.
#[must_use]
pub fn layer_kind(layer: &Layer) -> &'static str {
    layer.kind()
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
    walk(tree).map(|(_, layer)| layer.kind()).collect()
}

/// Variant names with their depth, in pre-order.
#[must_use]
pub fn structure_with_depth(tree: &LayerTree) -> Vec<(usize, &'static str)> {
    walk(tree)
        .map(|(depth, layer)| (depth, layer.kind()))
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
