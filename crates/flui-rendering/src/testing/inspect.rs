//! Protocol-agnostic inspection of a laid-out / painted render tree.
//!
//! Two layers live here:
//!
//! - free functions generic over the pipeline phase
//!   ([`render_offset`], [`box_geometry`], [`sliver_geometry`],
//!   [`hit_path`]) plus [`LayerTree`] walkers ([`layer_structure`],
//!   [`layer_structure_with_depth`], [`first_picture_bounds`]). These are
//!   the bodies previously duplicated across the integration tests.
//! - the [`Probe`] trait, an ergonomic wrapper the run results
//!   ([`crate::testing::LayoutRun`] / [`crate::testing::FrameRun`])
//!   implement so Box and Sliver are inspected identically regardless of
//!   how far the pipeline was driven.

use flui_foundation::{DiagnosticsNode, RenderId};
use flui_types::{Matrix4, Offset, Pixels, Size, geometry::px};

use crate::{
    constraints::SliverGeometry,
    hit_testing::HitTestResult,
    pipeline::{PipelineOwner, PipelinePhase},
    storage::RenderNode,
    testing::tree::RenderLabelRegistry,
};

// The layer-tree walkers live in `flui_layer` (the crate that owns
// `LayerTree`), the architecturally correct home. Re-exported here under
// their historical names so the render harness and the migrated tests keep
// using `inspect::layer_structure` / `inspect::first_picture_bounds`.
pub use flui_layer::testing::inspect::{
    first_opacity_alpha, first_picture_bounds, has_picture_layer, layer_kind,
    structure as layer_structure, structure_with_depth as layer_structure_with_depth,
};

// ============================================================================
// Phase-generic render-tree readers
// ============================================================================

/// Returns the committed paint offset of `id`, or `None` if no such node
/// exists.
pub fn render_offset<P: PipelinePhase>(owner: &PipelineOwner<P>, id: RenderId) -> Option<Offset> {
    owner.render_tree().get(id).map(RenderNode::offset)
}

/// Returns the committed Box geometry (size) of `id`, or `None` if the node
/// is missing, is not a Box node, or has not been laid out yet.
pub fn box_geometry<P: PipelinePhase>(owner: &PipelineOwner<P>, id: RenderId) -> Option<Size> {
    owner.render_tree().get(id)?.as_box()?.state().geometry()
}

/// Returns the committed Sliver geometry of `id`, or `None` if the node is
/// missing, is not a Sliver node, or has not been laid out yet.
pub fn sliver_geometry<P: PipelinePhase>(
    owner: &PipelineOwner<P>,
    id: RenderId,
) -> Option<SliverGeometry> {
    owner.render_tree().get(id)?.as_sliver()?.state().geometry()
}

/// Hit-tests at root-local `(x, y)` (logical pixels) and returns the
/// leaf-first path of hit `RenderId`s.
pub fn hit_path<P: PipelinePhase + Sync>(
    owner: &PipelineOwner<P>,
    x: f32,
    y: f32,
) -> Vec<RenderId> {
    let mut result = HitTestResult::new();
    owner.hit_test(Offset::new(px(x), px(y)), &mut result);
    result.path().iter().map(|entry| entry.target).collect()
}

/// Hit-tests at root-local `(x, y)` (logical pixels) and returns the
/// leaf-first path of `(RenderId, recorded_transform)` pairs.
///
/// The recorded value is the globalized global-to-local transform that
/// `HitTestResult::add` unconditionally attaches to every entry
/// (`crates/flui-interaction/src/routing/hit_test.rs`'s `add`, which sets
/// `entry.transform = Some(self.last_transform())` regardless of whether any
/// offset/transform was ever pushed — an empty stack still folds to
/// `Matrix4::identity()`). The `Option` wrapper exists for the field's
/// general shape, not because `None` is reachable through this path.
///
/// Pair with [`localize_hit_point`] to assert that the recorded transform
/// correctly maps a global hit point to the child's local coordinate space.
///
/// [`hit_test_child_at_layout_offset`]: crate::context::BoxHitTestContext::hit_test_child_at_layout_offset
pub fn hit_path_with_transforms<P: PipelinePhase + Sync>(
    owner: &PipelineOwner<P>,
    x: f32,
    y: f32,
) -> Vec<(RenderId, Option<Matrix4>)> {
    let mut result = HitTestResult::new();
    owner.hit_test(Offset::new(px(x), px(y)), &mut result);
    result
        .path()
        .iter()
        .map(|entry| (entry.target, entry.transform))
        .collect()
}

/// Applies the global-to-local `transform` recorded in a `HitTestEntry` to
/// a global hit point, returning the equivalent point in the entry's local
/// coordinate space.
///
/// `transform` already maps global to local -- `HitTestResult` composes it by
/// left-multiplying each ancestor level's own inverse as the walk descends
/// (see `HitTestEntry::transform`'s doc), so this applies it directly with no
/// further inversion.
///
/// Returns `None` when `transform` is singular (non-invertible). This is
/// guaranteed whenever a degenerate ancestor transform (e.g. a zero-scale
/// `Transform`) contributed an exactly-zero determinant somewhere in the
/// composed chain. For a merely near-singular ancestor (`0 < |det| <
/// f32::EPSILON`, which `Matrix4::is_invertible` also rejects) the composed
/// `transform` is not guaranteed to stay singular -- determinants compose
/// multiplicatively, so a large-determinant ancestor elsewhere in the chain
/// can lift the product back above `f32::EPSILON`. In that case this
/// function returns `Some` with a meaningless local point instead of
/// `None`, so a `Some` result is not proof the whole chain was
/// well-conditioned.
///
/// # Usage
///
/// ```ignore
/// let entries = hit_path_with_transforms(owner, 50.0, 50.0);
/// let (_, transform) = entries.iter().find(|(id, _)| *id == child_id).unwrap();
/// let transform = transform.expect("child entry must carry a recorded transform");
/// let local = localize_hit_point(transform, 50.0, 50.0)
///     .expect("transform must be invertible");
/// assert_eq!(local, Offset::new(px(20.0), px(20.0)));
/// ```
pub fn localize_hit_point(transform: Matrix4, global_x: f32, global_y: f32) -> Option<Offset> {
    if !transform.is_invertible() {
        return None;
    }
    let (local_x, local_y) = transform.transform_point(Pixels(global_x), Pixels(global_y));
    Some(Offset::new(local_x, local_y))
}

// ============================================================================
// Render-tree diagnostics dump
// ============================================================================

/// Builds a [`DiagnosticsNode`] tree mirroring the render hierarchy. Each
/// node self-describes via the render object's
/// [`Diagnosticable`](flui_foundation::Diagnosticable) impl (color, padding,
/// direction, ...); the committed geometry/offset is layered on, and the tree
/// links supply the parent/child structure.
///
/// Delegates to [`PipelineOwner::debug_diagnostics_tree`] so the harness and
/// the production debug dump share one walk.
pub fn render_diagnostics<P: PipelinePhase>(owner: &PipelineOwner<P>) -> DiagnosticsNode {
    owner
        .debug_diagnostics_tree()
        .unwrap_or_else(|| DiagnosticsNode::new("<no root>"))
}

// ============================================================================
// Probe: the ergonomic inspection surface shared by both run results
// ============================================================================

/// Inspection surface shared by every run result, independent of protocol
/// and of how far the pipeline was driven.
///
/// Implementors supply the underlying [`PipelineOwner`] and the label
/// [`RenderLabelRegistry`]; the provided methods do the rest. Box trees read
/// [`box_geometry`](Probe::box_geometry); Sliver trees read
/// [`sliver_geometry`](Probe::sliver_geometry); everything else is common.
pub trait Probe {
    /// The pipeline phase the implementor holds its owner in.
    type Phase: PipelinePhase + Sync;

    /// The live pipeline owner backing the inspection.
    fn pipeline(&self) -> &PipelineOwner<Self::Phase>;

    /// The label -> id registry built while mounting the tree.
    fn registry(&self) -> &RenderLabelRegistry;

    /// A diagnostics tree mirroring the render hierarchy, with each node's
    /// own properties plus committed geometry/offset.
    fn diagnostics(&self) -> DiagnosticsNode {
        render_diagnostics(self.pipeline())
    }

    /// The value of diagnostics property `name` on node `id` (e.g.
    /// `property(child, "color")`), for structured assertions that don't
    /// rely on substring-matching the dump.
    fn property(&self, id: RenderId, name: &str) -> Option<String> {
        self.pipeline()
            .debug_node_diagnostics(id)
            .and_then(|node| node.get_property(name).map(str::to_owned))
    }

    /// Parses a numeric diagnostics property on `id`, if present.
    fn property_f64(&self, id: RenderId, name: &str) -> Option<f64> {
        self.pipeline()
            .debug_node_diagnostics(id)
            .and_then(|node| node.get_property_f64(name))
    }

    /// Returns the first descendant's property value matched by render-object
    /// type name (e.g. `"RenderColoredBox"`, `"RenderFlex"`).
    fn descendant_property(&self, type_name: &str, property: &str) -> Option<String> {
        self.diagnostics()
            .find_descendant_unique(type_name)
            .ok()?
            .get_property(property)
            .map(str::to_owned)
    }

    /// Parses a numeric property on the first descendant matched by `type_name`.
    fn descendant_property_f64(&self, type_name: &str, property: &str) -> Option<f64> {
        self.diagnostics()
            .find_descendant_unique(type_name)
            .ok()?
            .get_property_f64(property)
    }

    /// A printable, indented dump of the render-tree diagnostics — what a
    /// failing assertion should print to show *why*.
    fn dump(&self) -> String {
        self.diagnostics().to_string()
    }

    /// Resolves a node label to its `RenderId`, panicking if the label was
    /// never registered (a test-authoring error).
    fn id(&self, label: &str) -> RenderId {
        self.try_id(label)
            .unwrap_or_else(|| panic!("no render node labeled {label:?} in the test tree"))
    }

    /// Resolves a node label to its `RenderId`, or `None` if unknown.
    fn try_id(&self, label: &str) -> Option<RenderId> {
        self.registry().get(label)
    }

    /// The committed paint offset of `id`.
    fn offset(&self, id: RenderId) -> Offset {
        render_offset(self.pipeline(), id).expect("node must exist in the render tree")
    }

    /// The committed Box geometry (size) of `id`.
    fn box_geometry(&self, id: RenderId) -> Size {
        box_geometry(self.pipeline(), id).expect("node must be a laid-out Box node")
    }

    /// The committed Box geometry (size) of `id`, or `None`.
    fn try_box_geometry(&self, id: RenderId) -> Option<Size> {
        box_geometry(self.pipeline(), id)
    }

    /// The committed Sliver geometry of `id`.
    fn sliver_geometry(&self, id: RenderId) -> SliverGeometry {
        sliver_geometry(self.pipeline(), id).expect("node must be a laid-out Sliver node")
    }

    /// The committed Sliver geometry of `id`, or `None`.
    fn try_sliver_geometry(&self, id: RenderId) -> Option<SliverGeometry> {
        sliver_geometry(self.pipeline(), id)
    }

    /// Hit-tests at root-local `(x, y)` (logical pixels), returning the
    /// leaf-first path of hit `RenderId`s.
    fn hit(&self, x: f32, y: f32) -> Vec<RenderId> {
        hit_path(self.pipeline(), x, y)
    }

    /// The first hit `RenderId` at `(x, y)`, if anything was hit.
    fn hit_first(&self, x: f32, y: f32) -> Option<RenderId> {
        self.hit(x, y).first().copied()
    }

    /// Hit-tests at root-local `(x, y)` (logical pixels), returning the
    /// leaf-first path of `(RenderId, recorded_transform)` pairs.
    ///
    /// The `Option<Matrix4>` is `Some` when the entry's node was visited via
    /// [`hit_test_child_at_layout_offset`] (which pushes the child's paint
    /// offset onto `HitTestResult`'s transform stack before recursing), and
    /// `None` when the node was added without a recorded offset transform.
    ///
    /// Use [`localize_hit_point`] to assert that the recorded transform maps
    /// the global hit position to the expected child-local coordinate.
    ///
    /// [`hit_test_child_at_layout_offset`]: crate::context::BoxHitTestContext::hit_test_child_at_layout_offset
    fn hit_with_transforms(&self, x: f32, y: f32) -> Vec<(RenderId, Option<Matrix4>)> {
        hit_path_with_transforms(self.pipeline(), x, y)
    }
}
