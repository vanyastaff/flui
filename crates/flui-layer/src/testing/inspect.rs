//! Structural and diagnostic inspection of a [`LayerTree`].
//!
//! These free functions are the single source of truth for layer-tree
//! introspection — `flui-rendering`'s render harness re-uses them rather than
//! reimplementing the walk.

use std::ops::ControlFlow;

use flui_foundation::{Diagnosticable, DiagnosticsNode, LayerId};
use flui_painting::DisplayListCore;
use flui_types::{Matrix4, RRect, Rect, painting::Path};

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
    let mut out = Vec::with_capacity(tree.len());
    pre_order(tree, |depth, layer| {
        out.push((depth, layer.kind_name()));
        ControlFlow::Continue(())
    });
    out
}

/// Visits every reachable layer in pre-order (parent before children,
/// siblings in paint order), handing `visit` the node's depth from the root
/// (root = 0) and its layer; a `Break` stops the walk early.
///
/// An explicit stack rather than recursion: a deep composited tree is
/// ordinary (one `OffsetLayer` per nested repaint boundary), and the
/// inspection helpers must not be the thing that overflows. A node whose id
/// is stale is skipped together with its subtree, as a recursive walk that
/// returned on a missing node would skip it.
fn pre_order(tree: &LayerTree, mut visit: impl FnMut(usize, &Layer) -> ControlFlow<()>) {
    let Some(root) = tree.root() else {
        return;
    };
    let mut stack = vec![(root, 0usize)];
    while let Some((id, depth)) = stack.pop() {
        let Some(node) = tree.get(id) else {
            continue;
        };
        if visit(depth, node.layer()).is_break() {
            return;
        }
        // Push reversed so siblings pop back in paint order.
        stack.extend(
            node.children()
                .iter()
                .rev()
                .map(|&child| (child, depth + 1)),
        );
    }
}

/// Returns the first value `pick` yields over a pre-order walk, or `None`.
fn find_first<T>(tree: &LayerTree, mut pick: impl FnMut(&Layer) -> Option<T>) -> Option<T> {
    let mut found = None;
    pre_order(tree, |_, layer| match pick(layer) {
        Some(value) => {
            found = Some(value);
            ControlFlow::Break(())
        }
        None => ControlFlow::Continue(()),
    });
    found
}

/// Returns the bounds of the first `Picture` layer found in pre-order, or
/// `None` if the tree contains no picture.
#[must_use]
pub fn first_picture_bounds(tree: &LayerTree) -> Option<Rect> {
    find_first(tree, |layer| match layer {
        Layer::Picture(picture) => Some(picture.picture().bounds()),
        _ => None,
    })
}

/// Builds a [`DiagnosticsNode`] tree mirroring the layer hierarchy: each
/// node self-describes via [`Diagnosticable::to_diagnostics_node`] and the
/// tree links supply the parent/child structure.
#[must_use]
pub fn diagnostics_tree(tree: &LayerTree) -> Option<DiagnosticsNode> {
    // Recursive on purpose: the nested `DiagnosticsNode` is composed
    // bottom-up, child before parent, which a flat pre-order visit cannot
    // express. This is a debug dump, not one of the flat query walkers.
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
    find_first(tree, |layer| match layer {
        Layer::Opacity(opacity) => Some(opacity.alpha()),
        _ => None,
    })
}

/// Returns the transform matrix of every [`Layer::Transform`] node in
/// pre-order (parent before children) as a flat list.
#[must_use]
pub fn transform_matrices(tree: &LayerTree) -> Vec<Matrix4> {
    let mut out = Vec::new();
    pre_order(tree, |_, layer| {
        if let Layer::Transform(t) = layer {
            out.push(*t.transform());
        }
        ControlFlow::Continue(())
    });
    out
}

/// Returns the matrix of the first [`Layer::Transform`] node in pre-order, or
/// `None` if the tree contains no transform layer.
#[must_use]
pub fn first_transform_matrix(tree: &LayerTree) -> Option<Matrix4> {
    find_first(tree, |layer| match layer {
        Layer::Transform(t) => Some(*t.transform()),
        _ => None,
    })
}

/// Returns the clip rectangle of every [`Layer::ClipRect`] node in pre-order
/// (parent before children) as a flat list.
#[must_use]
pub fn clip_rects(tree: &LayerTree) -> Vec<Rect> {
    let mut out = Vec::new();
    pre_order(tree, |_, layer| {
        if let Layer::ClipRect(c) = layer {
            out.push(c.clip_rect());
        }
        ControlFlow::Continue(())
    });
    out
}

/// Returns the rounded rectangle of every [`Layer::ClipRRect`] node in
/// pre-order (parent before children) as a flat list.
///
/// Mirrors [`clip_rects`] for the rounded-rect shape: [`RRect`] derives
/// `PartialEq`, so callers compare the whole value directly rather than
/// picking apart `rect` plus the four corner radii by hand.
#[must_use]
pub fn clip_rrects(tree: &LayerTree) -> Vec<RRect> {
    let mut out = Vec::new();
    pre_order(tree, |_, layer| {
        if let Layer::ClipRRect(c) = layer {
            out.push(*c.clip_rrect());
        }
        ControlFlow::Continue(())
    });
    out
}

/// Returns the clip path of every [`Layer::ClipPath`] node in pre-order
/// (parent before children) as a flat list of owned clones.
///
/// Unlike [`clip_rects`]/[`clip_rrects`], callers must not compare the
/// result with `==`: `Path` derives `PartialEq` over a memoised bounding-box
/// cache alongside its command list, so two paths with identical commands
/// can compare unequal (or, after both have had bounds computed, equal by
/// coincidence) depending on unrelated history — whether something already
/// forced the cache to populate. Compare paths by sampling
/// [`Path::contains`] at points that discriminate the shapes under test
/// instead.
#[must_use]
pub fn clip_paths(tree: &LayerTree) -> Vec<Path> {
    let mut out = Vec::new();
    pre_order(tree, |_, layer| {
        if let Layer::ClipPath(c) = layer {
            out.push(c.clip_path().clone());
        }
        ControlFlow::Continue(())
    });
    out
}

/// Returns whether the tree contains any [`Layer::Picture`] node in pre-order.
#[must_use]
pub fn has_picture_layer(tree: &LayerTree) -> bool {
    find_first(tree, |layer| {
        matches!(layer, Layer::Picture(_)).then_some(())
    })
    .is_some()
}

#[cfg(test)]
mod tests {
    use flui_types::{Matrix4, geometry::px, painting::Clip};

    use super::*;
    use crate::{
        ClipPathLayer, ClipRRectLayer, ClipRectLayer, OffsetLayer, OpacityLayer, TransformLayer,
    };

    fn offset() -> Layer {
        Layer::Offset(OffsetLayer::zero())
    }

    /// root → [a → [a1], b] with distinct kinds on the branches, so the
    /// order the walkers report is observable: pre-order, parent before
    /// children, siblings in the order they were attached.
    #[test]
    fn walkers_visit_in_pre_order_with_siblings_in_paint_order() {
        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        let a = tree.insert(Layer::Opacity(OpacityLayer::new(0.25)));
        let a1 = tree.insert(Layer::Transform(TransformLayer::new(Matrix4::scaling(
            2.0, 2.0, 1.0,
        ))));
        let b = tree.insert(Layer::Transform(TransformLayer::new(Matrix4::scaling(
            3.0, 3.0, 1.0,
        ))));
        tree.set_root(Some(root));
        tree.add_child(root, a);
        tree.add_child(a, a1);
        tree.add_child(root, b);

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
    }

    /// One `OffsetLayer` per nested repaint boundary makes a deep composited
    /// tree ordinary. The walkers must not be what overflows: a recursive
    /// walk spends a stack frame per level, which this thread's stack cannot
    /// hold for a chain this long, while the explicit-stack walk heap-allocates
    /// its work list and finishes.
    #[test]
    fn walkers_survive_a_deep_chain_on_a_small_stack() {
        const DEPTH: usize = 10_000;

        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        tree.set_root(Some(root));
        let mut parent = root;
        for _ in 1..DEPTH {
            let child = tree.insert(offset());
            tree.add_child(parent, child);
            parent = child;
        }
        let leaf = tree.insert(Layer::Transform(TransformLayer::new(Matrix4::IDENTITY)));
        tree.add_child(parent, leaf);

        let walked = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let kinds = structure(&tree);
                let matrices = transform_matrices(&tree);
                let first = first_transform_matrix(&tree);
                (kinds.len(), matrices.len(), first)
            })
            .expect("spawn the small-stack walker thread")
            .join()
            .expect("the walkers must not overflow a small stack on a deep chain");

        assert_eq!(walked, (DEPTH + 1, 1, Some(Matrix4::IDENTITY)));
    }

    /// root → [outer → [inner]], both `ClipRect` layers with distinct rects,
    /// so the walk order (outer before inner) is observable in the result.
    #[test]
    fn clip_rects_returns_every_clip_rect_layer_in_pre_order() {
        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        let outer_rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let inner_rect = Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0));
        let outer = tree.insert(Layer::ClipRect(ClipRectLayer::new(
            outer_rect,
            Clip::HardEdge,
        )));
        let inner = tree.insert(Layer::ClipRect(ClipRectLayer::new(
            inner_rect,
            Clip::HardEdge,
        )));
        tree.set_root(Some(root));
        tree.add_child(root, outer);
        tree.add_child(outer, inner);

        assert_eq!(clip_rects(&tree), vec![outer_rect, inner_rect]);
    }

    /// root → [outer → [inner]], both `ClipRRect` layers with distinct
    /// rounded rects (different corner radii, not just different rects), so
    /// the walk order (outer before inner) is observable in the result and a
    /// comparison that ignored the radii could not pass by accident.
    #[test]
    fn clip_rrects_returns_every_clip_rrect_layer_in_pre_order() {
        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        let outer_rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            px(8.0),
        );
        let inner_rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0)),
            px(4.0),
        );
        let outer = tree.insert(Layer::ClipRRect(ClipRRectLayer::new(
            outer_rrect,
            Clip::AntiAlias,
        )));
        let inner = tree.insert(Layer::ClipRRect(ClipRRectLayer::new(
            inner_rrect,
            Clip::AntiAlias,
        )));
        tree.set_root(Some(root));
        tree.add_child(root, outer);
        tree.add_child(outer, inner);

        assert_eq!(clip_rrects(&tree), vec![outer_rrect, inner_rrect]);
    }

    /// root → [outer → [inner]], both `ClipPath` layers with distinct paths
    /// (a whole-box rect and a left-half rect), so walk order (outer before
    /// inner) AND shape are observable — through `Path::contains` at a probe
    /// point in the half NOT covered by the inner path, never `==` (see
    /// [`clip_paths`]'s own doc for why equality is unsafe here).
    #[test]
    fn clip_paths_returns_every_clip_path_layer_in_pre_order() {
        use flui_types::geometry::Point;

        let mut tree = LayerTree::new();
        let root = tree.insert(offset());
        let mut outer_path = Path::new();
        outer_path.add_rect(Rect::from_xywh(px(0.0), px(0.0), px(20.0), px(20.0)));
        let mut inner_path = Path::new();
        inner_path.add_rect(Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(20.0)));
        let outer = tree.insert(Layer::ClipPath(Box::new(ClipPathLayer::new(
            outer_path.clone(),
            Clip::AntiAlias,
        ))));
        let inner = tree.insert(Layer::ClipPath(Box::new(ClipPathLayer::new(
            inner_path.clone(),
            Clip::AntiAlias,
        ))));
        tree.set_root(Some(root));
        tree.add_child(root, outer);
        tree.add_child(outer, inner);

        let paths = clip_paths(&tree);
        assert_eq!(paths.len(), 2, "one ClipPath layer per node: {paths:?}");
        let probe = Point::new(px(15.0), px(10.0));
        assert!(
            paths[0].contains(probe),
            "walk order: the OUTER path (whole box) is visited first and \
             contains the probe on the right half",
        );
        assert!(
            !paths[1].contains(probe),
            "walk order: the INNER path (left half only) is visited second \
             and excludes the probe on the right half",
        );
    }
}
