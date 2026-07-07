//! PipelineOwner manages the rendering pipeline.
//!
//! Mythos Step 7 finalization (2026-05-20): the four pipeline phases now
//! own their work as `run_*` methods on the phase-specific impls. The
//! legacy `flush_*` aliases on `PipelineOwner<Idle>` are gone. Calling
//! `run_paint` on `<Idle>` is a compile error -- see the `compile_fail`
//! doctest at the end of `pipeline/phase.rs`.

mod subtree_arena;

mod accessors;
mod compositing;
mod construction;
mod diagnostics;
mod layout;
mod paint;
mod query;
mod reassemble;
mod semantics;

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, AtomicU64},
};

use flui_foundation::RenderId;
use flui_layer::LayerTree;
use flui_semantics::SemanticsOwner;
use flui_types::Offset;
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::{constraints::BoxConstraints, storage::RenderTree};

use super::{
    deferred::DeferredMutations,
    handle::{DirtyRequest, PipelineOwnerHandle},
    notifier::VisualUpdateNotifier,
    phase::{Idle, PipelinePhase},
    scheduler::DirtyTracker,
};

/// Default bounded capacity of the dirty-request channel between
/// [`PipelineOwnerHandle`] producers and the [`PipelineOwner`] receiver.
/// 256 is a heuristic: more than peak burst from a typical async asset
/// loader completion storm, low enough that producers feel backpressure
/// rather than silently growing the queue. Tunable at owner construction
/// via [`PipelineOwner::new_with_capacity`].
const DEFAULT_DIRTY_CHANNEL_CAPACITY: usize = 256;

// ============================================================================
// Pipeline ID Counter
// ============================================================================

/// Global counter for unique pipeline owner IDs.
static PIPELINE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

// ============================================================================
// PipelineOwner
// ============================================================================

/// Manages the rendering pipeline for a tree of render objects.
///
/// The pipeline owner:
/// - Stores the root render object
/// - Tracks dirty nodes needing layout/paint/semantics
/// - Coordinates phase work via consuming phase transitions
/// - Holds the layer tree produced by the most recent paint phase
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `PipelineOwner` class in
/// `rendering/object.dart`. Where Flutter uses runtime `_debugDoingThis*`
/// asserts to enforce phase ordering, FLUI lifts the question into the
/// type system: each phase's `run_*` method lives only on the matching
/// `PipelineOwner<PhaseMarker>` impl block.
///
/// # Pipeline Phases
///
/// Use [`run_frame`](Self::run_frame) for the typestate-driven orchestration:
///
/// ```text
/// Idle ─into_layout()──▶ Layout ─run_layout()──▶ into_compositing()
///        ▲                                        │
///        │                                        ▼
///        │                                   Compositing ─run_compositing()─▶ into_paint()
///        │                                                                     │
///        │                                                                     ▼
///        │                                                                Paint ─run_paint()─▶ into_semantics()
///        │                                                                                      │
///        │                                                                                      ▼
///        │                                                                                  Semantics ─run_semantics()─▶ finish()
///        └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
/// ```
///
/// # Multi-window
///
/// Each PipelineOwner manages one render tree. Multi-window applications
/// own multiple PipelineOwner instances side-by-side; the previous
/// hierarchical-pipelines API (`adopt_child` / `drop_child`) was removed
/// in Mythos Step 9 -- it used `Arc<RwLock<PipelineOwner>>` for tree
/// nodes, an anti-pattern this crate refuses.
pub struct PipelineOwner<Phase: PipelinePhase = Idle> {
    /// Unique identifier for this pipeline owner.
    id: u64,

    /// The render tree storing all RenderObjects (Slab-based).
    render_tree: RenderTree,

    /// The root render object ID of this pipeline.
    root_id: Option<RenderId>,

    /// Consolidated visual-update + semantics-owner-lifecycle callback
    /// notifier. Replaces three previously-separate `Box<dyn Fn() + Send +
    /// Sync>` fields. See [`VisualUpdateNotifier`].
    ///
    /// The owner keeps its own Arc clone for the `set_on_*` callback setters;
    /// `scheduler` holds a second clone pointing at the same allocation for
    /// the wake-on-mark path.
    notifier: std::sync::Arc<parking_lot::RwLock<VisualUpdateNotifier>>,

    /// Dirty-work scheduling subsystem: dirty sets, mid-phase side queue,
    /// phase-guard flags, and the wake-on-mark notifier clone.
    ///
    /// Disjoint from `render_tree`, so `scheduler.mark_needs_layout(
    /// &mut self.render_tree, id)` compiles as a split borrow.
    scheduler: DirtyTracker,

    /// Constraints to pass to [`Self::layout_dirty_root`] when the
    /// dirty entry is the tree root (`root_id`) and the root has no
    /// cached `state.constraints()` yet (first frame).
    ///
    /// **D-block PR-A1 U23:** the binding layer (`flui-view` /
    /// `flui-app` / `flui-hot-reload`) sets this once per
    /// configuration via [`Self::set_root_constraints`] before the
    /// first `run_frame` invocation. On subsequent frames the root's
    /// cached constraints (post-layout) supersede this field; the
    /// fallback only fires on the very first layout pass.
    root_constraints: Option<BoxConstraints>,

    /// Whether semantics are enabled.
    semantics_enabled: AtomicBool,

    /// The semantics tree owner (ADR-0014 D1; Flutter `PipelineOwner
    /// ._semanticsOwner` parity). `None` until semantics is enabled via
    /// [`Self::set_semantics_enabled`]`(true)`, which lazily creates it and
    /// fires `fire_semantics_owner_created`; disposed (firing
    /// `fire_semantics_owner_disposed`) on the next `false` transition.
    /// `Semantics::run_semantics` (`pipeline/owner/semantics.rs`) is the
    /// sole writer of its tree contents.
    semantics_owner: Option<SemanticsOwner>,

    /// The layer tree produced by the last paint phase.
    last_layer_tree: Option<LayerTree>,

    /// The leader/follower link registry produced as a byproduct of the
    /// last paint phase's `FragmentComposer` walk (see `paint.rs`'s
    /// `FragmentComposer::link_registry`). Handed to `Scene::with_links`
    /// by the binding layer so `flui-engine` can resolve `Layer::Follower`
    /// positions at render time against this same frame's `last_layer_tree`.
    last_link_registry: Option<flui_layer::LinkRegistry>,

    /// Composite-resolved offsets for `Layer::Follower` render nodes,
    /// keyed by `RenderId` (ADR-0015 D1) — a per-frame byproduct mirroring
    /// `last_layer_tree`/`last_link_registry`. Populated post-paint
    /// (`paint.rs::run_paint`) by resolving each `FragmentComposer`-recorded
    /// follower correlation via the SAME `flui_layer::resolve_follower_offset`
    /// the GPU path (flui-engine's `render_layer_recursive`) resolves
    /// against. Present ⟹ the follower is visible this frame at the
    /// translated offset; absent ⟹ either not a follower, or a hidden one
    /// (see `last_hidden_follower_ids`). Consulted generically by the
    /// hit-test walk (`accessors.rs`) so a visually-displaced
    /// `RenderFollowerLayer` hit-tests at its RESOLVED on-screen position,
    /// not its plain tree-relative position — Flutter's `getLastTransform()`
    /// cache-from-last-composite contract, one frame stale by design.
    last_follower_offsets: FxHashMap<RenderId, Offset>,

    /// `RenderId`s of `Layer::Follower` nodes correlated during the last
    /// paint phase that resolved to `None` (unlinked with
    /// `show_when_unlinked == false`) — ADR-0015 D1/D4's companion to
    /// `last_follower_offsets`. The hit-test walk must distinguish "not a
    /// follower" (fall through to normal traversal) from "a follower that
    /// is currently hidden" (skip the subtree entirely, mirroring
    /// `resolve_follower_offset -> None -> don't descend` on the render
    /// path); `last_follower_offsets`'s absence alone conflates both
    /// cases, so this set carries follower identity independent of
    /// resolution outcome.
    last_hidden_follower_ids: FxHashSet<RenderId>,

    /// Device pixel ratio threaded into every paint pass (text shaping
    /// and hairline snapping are DPR-dependent). Set by the platform
    /// binding on surface creation / DPI change; defaults to 1.0 for
    /// headless tests.
    device_pixel_ratio: f32,

    /// Deferred mutation queue for re-entrant layout.
    ///
    /// During layout, render objects may enqueue child insertions,
    /// removals, or property updates. These are applied after the
    /// layout pass completes, outside the `&mut` borrow scope of
    /// the layout walk.
    ///
    /// This is the Rust-native alternative to Flutter's
    /// `invokeLayoutCallback` which uses unsafe re-entrant mutation.
    deferred_mutations: DeferredMutations,

    /// Prototype handle held by the owner so `handle()` can clone it for
    /// each caller without re-allocating the channel. See
    /// [`PipelineOwnerHandle`].
    handle: PipelineOwnerHandle,

    /// Receiver end of the bounded dirty-request channel. Drained into
    /// `dirty` by `drain_pending_dirty` at phase boundaries.
    dirty_rx: crossbeam_channel::Receiver<DirtyRequest>,

    /// Harness-only parent-data presets keyed by child [`RenderId`].
    ///
    /// Cloned into per-walk [`ErasedChildState`] / [`ErasedSliverChildState`]
    /// slots before layout so headless tests can express widget-level
    /// configuration (stack positioning, flex factors, future animation
    /// parent slots) without an element tree.
    #[cfg(any(test, feature = "testing"))]
    parent_data_seeds: FxHashMap<RenderId, ParentDataSeed>,

    /// Child-build requests accumulated during the most recent layout pass
    /// by request-strategy slivers (U4.2).  Each entry is `(sliver_id,
    /// logical_index)`.  The binding layer drains this via
    /// [`Self::take_pending_child_requests`] after the frame to service the
    /// requests through the element tree (U4.3).  Empty between frames.
    pending_child_requests: Vec<(RenderId, usize)>,
    /// Retain-band signals from element-owned slivers (U4.3 removal half).
    ///
    /// Each entry is `(sliver_id, cache_first, cache_last)`: the retained
    /// logical index band `[first, last)` emitted by
    /// `RenderSliverList::perform_layout` after each walk.  The binding layer
    /// consumes this via [`Self::take_pending_retain_bands`] after every frame
    /// to drive `SparseChildren::retain_band` through the element tree, evicting
    /// out-of-band lazy children rather than calling `dispose_box_child` from
    /// the render tree (which would double-remove element-owned nodes).
    pending_retain_bands: Vec<(RenderId, usize, usize)>,

    /// Phantom marker for the typestate phase. Always zero-sized.
    /// See `crates/flui-rendering/src/pipeline/phase.rs`.
    _phase: PhantomData<Phase>,
}

impl<Phase: PipelinePhase> std::fmt::Debug for PipelineOwner<Phase> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("phase", &Phase::NAME)
            .field("id", &self.id)
            .field("root_id", &self.root_id)
            .field("render_tree_len", &self.render_tree.len())
            .field("nodes_needing_layout", &self.scheduler.layout_queue_len())
            .field("nodes_needing_paint", &self.scheduler.paint_queue_len())
            .field("debug_doing_layout", &self.scheduler.debug_doing_layout())
            .field("debug_doing_paint", &self.scheduler.debug_doing_paint())
            .field(
                "debug_doing_semantics",
                &self.scheduler.debug_doing_semantics(),
            )
            .field("has_layer_tree", &self.last_layer_tree.is_some())
            .field("has_link_registry", &self.last_link_registry.is_some())
            .field("follower_offset_count", &self.last_follower_offsets.len())
            .field(
                "hidden_follower_count",
                &self.last_hidden_follower_ids.len(),
            )
            .field("has_semantics_owner", &self.semantics_owner.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for PipelineOwner<Idle> {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal helper: shifts the `Phase` phantom parameter without touching any
/// runtime field. Behaviour-preserving by construction.
#[inline]
fn rebind_phase<From, To>(from: PipelineOwner<From>) -> PipelineOwner<To>
where
    From: PipelinePhase,
    To: PipelinePhase,
{
    PipelineOwner {
        id: from.id,
        render_tree: from.render_tree,
        root_id: from.root_id,
        notifier: from.notifier,
        scheduler: from.scheduler,
        root_constraints: from.root_constraints,
        semantics_enabled: from.semantics_enabled,
        semantics_owner: from.semantics_owner,
        last_layer_tree: from.last_layer_tree,
        last_link_registry: from.last_link_registry,
        last_follower_offsets: from.last_follower_offsets,
        last_hidden_follower_ids: from.last_hidden_follower_ids,
        device_pixel_ratio: from.device_pixel_ratio,
        deferred_mutations: from.deferred_mutations,
        handle: from.handle,
        dirty_rx: from.dirty_rx,
        #[cfg(any(test, feature = "testing"))]
        parent_data_seeds: from.parent_data_seeds,
        pending_child_requests: from.pending_child_requests,
        pending_retain_bands: from.pending_retain_bands,
        _phase: PhantomData,
    }
}

// ============================================================================
// Cross-phase accessors
// ============================================================================

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Takes all child-build requests accumulated during the most recent
    /// layout pass (U4.2), leaving the buffer empty.
    ///
    /// Each entry is `(sliver_id, logical_index)`: the sliver whose
    /// `request_child_build` fired, and the logical item index it could not
    /// materialize synchronously.  The binding layer (U4.3) consumes this
    /// after every frame to drive the element-tree child manager.
    ///
    /// This is the U4.3 entry point — `pub` (not `pub(super)`) because no
    /// in-module consumer exists in U4.2; the only caller is external.
    #[must_use]
    pub fn take_pending_child_requests(&mut self) -> Vec<(RenderId, usize)> {
        std::mem::take(&mut self.pending_child_requests)
    }

    /// Takes all retain-band signals accumulated during the most recent layout
    /// pass (U4.3 removal half), leaving the buffer empty.
    ///
    /// Each entry is `(sliver_id, cache_first, cache_last)`.  The binding
    /// layer calls this to drive `SparseChildren::retain_band` through the
    /// element tree, evicting out-of-band lazy children via the element tree
    /// rather than via `dispose_box_child` (which would ABA-double-remove
    /// element-owned render nodes).
    #[must_use]
    pub fn take_pending_retain_bands(&mut self) -> Vec<(RenderId, usize, usize)> {
        std::mem::take(&mut self.pending_retain_bands)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_tree::Leaf;
    use flui_types::{Color, Point, Rect, Size, geometry::px};

    use super::*;
    use crate::{context::BoxLayoutContext, parent_data::BoxParentData, traits::RenderBox};

    /// Minimal leaf that paints a colored rect.
    ///
    /// Concrete objects (RenderColoredBox, RenderSizedBox, …) now live in the
    /// `flui_objects` crate. We use a local stub here because the pipeline/owner
    /// tests are intra-crate unit tests that must not take a dependency on
    /// flui_objects — they exercise the scheduling and wake-up contract, not any
    /// object-specific behavior.
    #[derive(Debug)]
    struct PaintingLeaf {
        color: [f32; 4],
        size: Size,
    }

    impl PaintingLeaf {
        fn red(width: f32, height: f32) -> Self {
            Self {
                color: [1.0, 0.0, 0.0, 1.0],
                size: Size::new(px(width), px(height)),
            }
        }
    }

    impl flui_foundation::Diagnosticable for PaintingLeaf {}

    impl RenderBox for PaintingLeaf {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constraints().constrain(self.size)
        }

        fn paint(&self, ctx: &mut crate::context::PaintCx<'_, Leaf>) {
            let rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            let color = Color::from_rgba_f32_array(self.color);
            ctx.canvas()
                .draw_rect(rect, &crate::pipeline::Paint::fill(color));
        }
    }

    /// Minimal leaf that contributes semantics without depending on
    /// `flui-objects`.
    #[derive(Debug)]
    struct SemanticLeaf {
        label: Option<&'static str>,
        boundary: bool,
        merge_descendants: bool,
        exclude_descendants: bool,
        size: Size,
    }

    impl SemanticLeaf {
        fn labeled(label: &'static str) -> Self {
            Self {
                label: Some(label),
                boundary: false,
                merge_descendants: false,
                exclude_descendants: false,
                size: Size::new(px(10.0), px(10.0)),
            }
        }

        fn boundary_labeled(label: &'static str) -> Self {
            Self {
                label: Some(label),
                boundary: true,
                merge_descendants: false,
                exclude_descendants: false,
                size: Size::new(px(10.0), px(10.0)),
            }
        }

        fn merge_labeled(label: &'static str) -> Self {
            Self {
                label: Some(label),
                boundary: true,
                merge_descendants: true,
                exclude_descendants: false,
                size: Size::new(px(10.0), px(10.0)),
            }
        }

        fn excluding() -> Self {
            Self {
                label: None,
                boundary: false,
                merge_descendants: false,
                exclude_descendants: true,
                size: Size::new(px(10.0), px(10.0)),
            }
        }

        fn empty() -> Self {
            Self {
                label: None,
                boundary: false,
                merge_descendants: false,
                exclude_descendants: false,
                size: Size::new(px(10.0), px(10.0)),
            }
        }
    }

    impl flui_foundation::Diagnosticable for SemanticLeaf {}

    impl RenderBox for SemanticLeaf {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constraints().constrain(self.size)
        }

        fn describe_semantics_configuration(
            &self,
            config: &mut crate::semantics::SemanticsConfiguration,
        ) {
            config.set_semantics_boundary(self.boundary);
            config.set_merging_semantics_of_descendants(self.merge_descendants);
            if let Some(label) = self.label {
                config.set_label(label);
            }
        }

        fn excludes_semantics_subtree(&self) -> bool {
            self.exclude_descendants
        }
    }

    #[test]
    fn test_pipeline_owner_new() {
        let owner = PipelineOwner::new();
        assert!(owner.root_id().is_none());
        assert!(owner.nodes_needing_layout().is_empty());
        assert!(owner.nodes_needing_paint().is_empty());
        assert!(!owner.debug_doing_layout());
        assert!(!owner.debug_doing_paint());
    }

    #[test]
    fn test_pipeline_owner_id_unique() {
        let owner1 = PipelineOwner::new();
        let owner2 = PipelineOwner::new();
        assert_ne!(owner1.id(), owner2.id());
    }

    #[test]
    fn test_pipeline_owner_dirty_nodes() {
        let mut owner = PipelineOwner::new();

        owner.add_node_needing_layout(RenderId::new(1), 0);
        owner.add_node_needing_layout(RenderId::new(2), 1);
        owner.add_node_needing_paint(RenderId::new(3), 2);

        assert_eq!(owner.nodes_needing_layout().len(), 2);
        assert_eq!(owner.nodes_needing_paint().len(), 1);
        assert_eq!(owner.dirty_node_count(), 3);
        assert!(owner.has_dirty_nodes());
    }

    #[test]
    fn test_pipeline_owner_run_layout() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(RenderId::new(1), 0);
        owner.add_node_needing_layout(RenderId::new(2), 1);

        let mut owner = owner.into_layout();
        owner.run_layout().expect("layout phase should succeed");

        assert!(owner.nodes_needing_layout().is_empty());
    }

    #[test]
    fn test_pipeline_owner_run_frame() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(RenderId::new(1), 0);
        owner.add_node_needing_paint(RenderId::new(2), 1);
        owner.add_node_needing_compositing_bits_update(RenderId::new(3), 2);

        let (owner, result) = owner.run_frame();
        let _layer_tree = result.expect("frame should succeed");

        assert!(!owner.has_dirty_nodes());
    }

    #[test]
    fn test_run_layout_sorts_by_depth_shallow_first() {
        let mut owner = PipelineOwner::new();
        // Add nodes in reverse depth order
        owner.add_node_needing_layout(RenderId::new(3), 2); // deepest
        owner.add_node_needing_layout(RenderId::new(1), 0); // shallowest
        owner.add_node_needing_layout(RenderId::new(2), 1); // middle

        // Before flush, they're in insertion order
        assert_eq!(owner.nodes_needing_layout()[0].depth, 2);
        assert_eq!(owner.nodes_needing_layout()[1].depth, 0);
        assert_eq!(owner.nodes_needing_layout()[2].depth, 1);

        let mut owner = owner.into_layout();
        owner.run_layout().expect("layout phase should succeed");

        // After flush, list is cleared
        assert!(owner.nodes_needing_layout().is_empty());
    }

    #[test]
    fn test_run_paint_sorts_by_depth_deep_first() {
        let mut owner = PipelineOwner::new();
        // Add nodes in shallow-first order
        owner.add_node_needing_paint(RenderId::new(1), 0); // shallowest
        owner.add_node_needing_paint(RenderId::new(2), 1); // middle
        owner.add_node_needing_paint(RenderId::new(3), 2); // deepest

        let owner = owner.into_layout().into_compositing();
        let mut owner = owner.into_paint();
        owner.run_paint().expect("paint phase should succeed");

        // After flush, list is cleared
        assert!(owner.nodes_needing_paint().is_empty());
    }

    // test_pipeline_owner_hierarchy removed in Mythos Step 9 along with the
    // adopt_child/drop_child/child_count/children API. Multi-PipelineOwner
    // scenarios (multi-window) are now owned by flui-app side-by-side.

    #[test]
    fn test_pipeline_owner_semantics_enabled() {
        let mut owner = PipelineOwner::new();
        assert!(!owner.semantics_enabled());

        owner.set_semantics_enabled(true);
        assert!(owner.semantics_enabled());

        owner.set_semantics_enabled(false);
        assert!(!owner.semantics_enabled());
    }

    #[test]
    fn run_semantics_builds_owner_tree() {
        let mut owner = PipelineOwner::new();
        owner.set_root_render_object(Box::new(SemanticLeaf::labeled("Submit")));
        owner.set_semantics_enabled(true);

        let owner = owner.into_layout().into_compositing().into_paint();
        let mut owner = owner.into_semantics();
        owner
            .run_semantics()
            .expect("semantics build should succeed");

        let semantics_owner = owner
            .semantics_owner()
            .expect("test installed a semantics owner");
        let root = semantics_owner.root().expect("root semantics node");
        let root_node = semantics_owner.get(root).expect("root node is live");
        assert_eq!(root_node.label(), Some("Submit"));
    }

    #[test]
    fn run_semantics_merges_non_boundary_child_into_root() {
        let mut owner = PipelineOwner::new();
        let root_id = owner.set_root_render_object(Box::new(SemanticLeaf::empty()));
        owner
            .insert_child_render_object(root_id, Box::new(SemanticLeaf::labeled("Child label")))
            .expect("child inserted");
        owner.set_semantics_enabled(true);

        let owner = owner.into_layout().into_compositing().into_paint();
        let mut owner = owner.into_semantics();
        owner
            .run_semantics()
            .expect("semantics build should succeed");

        let semantics_owner = owner
            .semantics_owner()
            .expect("test installed a semantics owner");
        let root = semantics_owner.root().expect("root semantics node");
        let root_node = semantics_owner.get(root).expect("root node is live");
        assert_eq!(root_node.label(), Some("Child label"));
        assert!(
            root_node.children().is_empty(),
            "non-boundary child config should merge into the root node"
        );
    }

    #[test]
    fn run_semantics_merge_descendants_collapses_boundary_grandchild() {
        let mut owner = PipelineOwner::new();
        let root_id = owner.set_root_render_object(Box::new(SemanticLeaf::empty()));
        let merge_id = owner
            .insert_child_render_object(root_id, Box::new(SemanticLeaf::merge_labeled("Group")))
            .expect("merge child inserted");
        owner
            .insert_child_render_object(merge_id, Box::new(SemanticLeaf::boundary_labeled("Child")))
            .expect("boundary grandchild inserted");
        owner.set_semantics_enabled(true);

        let owner = owner.into_layout().into_compositing().into_paint();
        let mut owner = owner.into_semantics();
        owner
            .run_semantics()
            .expect("semantics build should succeed");

        let semantics_owner = owner
            .semantics_owner()
            .expect("test installed a semantics owner");
        let root = semantics_owner.root().expect("root semantics node");
        let root_node = semantics_owner.get(root).expect("root node is live");
        assert_eq!(root_node.children().len(), 1);

        let merged = semantics_owner
            .get(root_node.children()[0])
            .expect("merged child node is live");
        assert_eq!(merged.label(), Some("Group Child"));
        assert!(
            merged.children().is_empty(),
            "merge-descendants should suppress descendant boundary nodes"
        );
    }

    #[test]
    fn run_semantics_excluding_node_skips_descendant_subtree() {
        let mut owner = PipelineOwner::new();
        let root_id = owner.set_root_render_object(Box::new(SemanticLeaf::empty()));
        let excluding_id = owner
            .insert_child_render_object(root_id, Box::new(SemanticLeaf::excluding()))
            .expect("excluding child inserted");
        owner
            .insert_child_render_object(
                excluding_id,
                Box::new(SemanticLeaf::labeled("Hidden label")),
            )
            .expect("excluded grandchild inserted");
        owner.set_semantics_enabled(true);

        let owner = owner.into_layout().into_compositing().into_paint();
        let mut owner = owner.into_semantics();
        owner
            .run_semantics()
            .expect("semantics build should succeed");

        let semantics_owner = owner
            .semantics_owner()
            .expect("test installed a semantics owner");
        let root = semantics_owner.root().expect("root semantics node");
        let root_node = semantics_owner.get(root).expect("root node is live");
        assert_eq!(root_node.label(), None);
        assert!(root_node.children().is_empty());
    }

    #[test]
    fn test_pipeline_owner_clear_dirty_nodes() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(RenderId::new(1), 0);
        owner.add_node_needing_paint(RenderId::new(2), 1);
        owner.add_node_needing_semantics(RenderId::new(3), 2);

        owner.clear_all_dirty_nodes();

        assert!(!owner.has_dirty_nodes());
        assert_eq!(owner.dirty_node_count(), 0);
    }

    #[test]
    fn test_pipeline_owner_with_callbacks() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let owner = PipelineOwner::with_callbacks(
            Some(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        owner.request_visual_update();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        owner.request_visual_update();
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    /// Idle-wake contract: scheduling NEW dirty work fires the
    /// visual-update callback exactly once per new queue entry, so a
    /// quiescent platform loop wakes for the frame — and duplicate
    /// marks (a frame is already scheduled) don't spam wakes.
    #[test]
    fn dirty_marks_fire_visual_update_once_per_new_entry() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let mut owner = PipelineOwner::with_callbacks(
            Some(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        owner.add_node_needing_layout(RenderId::new(1), 0);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "a new layout entry must wake the platform",
        );
        owner.add_node_needing_layout(RenderId::new(1), 0);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "a duplicate entry means a frame is already scheduled — no second wake",
        );

        owner.add_node_needing_paint(RenderId::new(2), 1);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            2,
            "a new paint entry must wake the platform",
        );
        owner.add_node_needing_paint(RenderId::new(2), 1);
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    // 1.2 RED tests: compositing and semantics marks must fire the visual-update
    // wake exactly once per new entry (Flutter: markNeedsCompositingBitsUpdate /
    // markNeedsSemanticsUpdate both call owner.requestVisualUpdate on a new entry).
    #[test]
    fn compositing_mark_fires_visual_update_on_new_entry_and_deduplicates() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_clone = Arc::clone(&wake_count);
        let mut owner = PipelineOwner::with_callbacks(
            Some(move || {
                wake_count_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        // An initial layout mark wakes; clear so we start from 0 for this test.
        owner.clear_all_dirty_nodes();
        let baseline = wake_count.load(Ordering::Relaxed);

        // First compositing mark on a fresh owner — must wake.
        owner.add_node_needing_compositing_bits_update(RenderId::new(10), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "add_node_needing_compositing_bits_update: first entry must fire \
             fire_need_visual_update (the GIF-frozen-until-you-scroll bug)"
        );

        // Duplicate mark — frame already scheduled, must NOT double-wake.
        owner.add_node_needing_compositing_bits_update(RenderId::new(10), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "add_node_needing_compositing_bits_update: duplicate entry must not \
             fire a second wake"
        );
    }

    #[test]
    fn semantics_mark_fires_visual_update_on_new_entry_and_deduplicates() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_clone = Arc::clone(&wake_count);
        let mut owner = PipelineOwner::with_callbacks(
            Some(move || {
                wake_count_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        owner.clear_all_dirty_nodes();
        let baseline = wake_count.load(Ordering::Relaxed);

        // First semantics mark on a fresh owner — must wake.
        owner.add_node_needing_semantics(RenderId::new(20), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "add_node_needing_semantics: first entry must fire \
             fire_need_visual_update"
        );

        // Duplicate mark — frame already scheduled, must NOT double-wake.
        owner.add_node_needing_semantics(RenderId::new(20), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "add_node_needing_semantics: duplicate entry must not fire a \
             second wake"
        );
    }

    /// The boundary-walking `mark_needs_layout` fires the wake when it
    /// enqueues the boundary, and stays silent when the boundary is
    /// already queued.
    #[test]
    fn mark_needs_layout_fires_visual_update_on_boundary_enqueue() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let mut owner = PipelineOwner::with_callbacks(
            Some(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        let id = owner.insert(Box::new(PaintingLeaf::red(10.0, 10.0))
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.clear_all_dirty_nodes();
        let base = counter.load(Ordering::Relaxed);

        owner.mark_needs_layout(id);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            base + 1,
            "enqueueing the relayout boundary must wake the platform",
        );
        owner.mark_needs_layout(id);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            base + 1,
            "boundary already queued — no extra wake",
        );
    }

    // ========================================================================
    // Mythos Step 12: catch_unwind plumbing
    // ========================================================================
    //
    // Verifies that a render object panicking inside a third-party trait
    // call (paint, perform_layout_raw) surfaces as
    // RenderError::Poisoned rather than aborting the process, and that
    // the owner remains usable for a subsequent frame.

    /// Direct (non-RenderBox) RenderObject<BoxProtocol> impl whose
    /// `paint` method panics on demand. Used by the catch_unwind tests
    /// below.
    ///
    /// We bypass the RenderBox blanket impl (whose paint is a no-op)
    /// because we want to exercise the actual third-party paint call
    /// site the pipeline owner wraps in `catch_unwind`.
    #[derive(Debug)]
    struct PanickingPaintBox {
        size: flui_types::Size,
    }

    impl PanickingPaintBox {
        fn new() -> Self {
            Self {
                size: flui_types::Size::ZERO,
            }
        }
    }

    impl flui_foundation::Diagnosticable for PanickingPaintBox {}

    impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for PanickingPaintBox {
        fn perform_layout_raw(
            &mut self,
            _ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<
                '_,
            >,
        ) -> crate::error::RenderResult<
            crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        > {
            Ok(self.size)
        }

        fn paint_raw(
            &self,
            _recorder: &mut crate::context::FragmentRecorder,
            _child_count: usize,
            _size: flui_types::Size,
        ) {
            panic!("PanickingPaintBox::paint_raw -- intentional test panic");
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
            _size: flui_types::Size,
            _hit_child: &mut (
                     dyn FnMut(
                usize,
                Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
            ) -> bool
                         + Send
                         + Sync
                 ),
        ) -> crate::traits::HitTestOutcome {
            crate::traits::HitTestOutcome::miss()
        }
    }

    /// Direct (non-RenderBox) RenderObject<BoxProtocol> impl whose
    /// `perform_layout_raw` panics. Used to test catch_unwind on the
    /// layout phase through `RenderEntry::layout`.
    #[derive(Debug)]
    struct PanickingLayoutBox;

    impl PanickingLayoutBox {
        fn new() -> Self {
            Self
        }
    }

    impl flui_foundation::Diagnosticable for PanickingLayoutBox {}

    impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for PanickingLayoutBox {
        fn perform_layout_raw(
            &mut self,
            _ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<
                '_,
            >,
        ) -> crate::error::RenderResult<
            crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        > {
            // Intentional unstructured panic — exercises the catch_unwind →
            // Poisoned path in `RenderEntry::layout_leaf_only`. This test
            // fixture is one explicit way to produce
            // `RenderError::Poisoned`; in production any third-party
            // panic in user widget code (`panic!`, `unwrap()`, assertion
            // failure inside `RenderBox::perform_layout`) reaches the
            // same path. Bridge-detected contract violations go through
            // the typed `Result` chain instead and surface as
            // `RenderError::ContractViolation`.
            panic!("PanickingLayoutBox::perform_layout_raw -- intentional test panic");
        }

        fn paint_raw(
            &self,
            _recorder: &mut crate::context::FragmentRecorder,
            _child_count: usize,
            _size: flui_types::Size,
        ) {
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
            _size: flui_types::Size,
            _hit_child: &mut (
                     dyn FnMut(
                usize,
                Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
            ) -> bool
                         + Send
                         + Sync
                 ),
        ) -> crate::traits::HitTestOutcome {
            crate::traits::HitTestOutcome::miss()
        }
    }

    /// A panicking `paint` call must surface as
    /// `RenderError::Poisoned { phase: "paint", .. }` and not abort.
    /// The owner must remain usable for a subsequent frame.
    #[test]
    fn test_run_frame_catches_paint_panic() {
        use crate::constraints::BoxConstraints;
        use crate::error::RenderError;
        use flui_types::geometry::px;

        // Silence the default panic hook for the duration of this test
        // so cargo test output isn't polluted by the intentional panic.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let mut owner = PipelineOwner::new();
        let root_id = owner.insert(Box::new(PanickingPaintBox::new())
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.set_root_id(Some(root_id));
        // Root layout needs binding constraints on the first frame; without
        // them run_layout skips the dirty entry, NEEDS_LAYOUT stays set, and
        // the paint guard (Flutter object.dart:3497) correctly skips paint —
        // which would make this test miss the intentional paint panic.
        owner.set_root_constraints(Some(BoxConstraints::new(
            px(0.0),
            px(200.0),
            px(0.0),
            px(200.0),
        )));

        let (owner, result) = owner.run_frame();

        std::panic::set_hook(prev);

        // The frame produces an error of the Poisoned variant.
        let err = result.expect_err("paint should panic, surface as Err");
        match err {
            RenderError::Poisoned { phase, .. } => {
                assert_eq!(phase, "paint", "phase should be 'paint'");
            }
            other => panic!("expected RenderError::Poisoned, got {other:?}"),
        }

        // Owner is reusable for a subsequent frame -- it's back at <Idle>
        // and another `run_frame` call must not panic. We re-mark the
        // panicking node dirty to force the paint path to run again,
        // since the first frame already cleared its paint dirty flag.
        // The second frame must hit the panic site once more and
        // surface the same Err(Poisoned).
        let mut owner = owner;
        owner.add_node_needing_paint(root_id, 0);

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let (owner, second_result) = owner.run_frame();
        std::panic::set_hook(prev);

        let _second_err =
            second_result.expect_err("re-marked paint should hit the panicking node again");

        // The owner is still at Idle after the second frame and can be
        // dropped cleanly -- the catch_unwind plumbing has not left any
        // resources poisoned.
        drop(owner);
    }

    /// A panicking `perform_layout_raw` surfaces as
    /// `RenderError::Poisoned { phase: "layout", .. }` through
    /// `RenderEntry::layout`. This verifies the catch_unwind wrapper on
    /// the layout call site (Mythos Step 12).
    ///
    /// Note: `RenderEntry::layout` is not yet wired into the pipeline
    /// owner's `run_layout` (the propagation stubs are empty per the
    /// Mythos Outstanding Refactors list), so this test exercises the
    /// entry directly rather than through `run_frame`.
    #[test]
    fn test_render_entry_layout_catches_panic() {
        use crate::error::RenderError;
        use crate::storage::RenderEntry;
        use flui_types::Size;

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let mut entry =
            RenderEntry::<crate::protocol::BoxProtocol>::new(Box::new(PanickingLayoutBox::new())
                as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);

        let result = entry.layout_leaf_only(crate::constraints::BoxConstraints::tight(Size::ZERO));

        std::panic::set_hook(prev);

        let err = result.expect_err("perform_layout_raw should panic, surface as Err");
        match err {
            RenderError::Poisoned { phase, .. } => {
                assert_eq!(phase, "layout", "phase should be 'layout'");
            }
            other => panic!("expected RenderError::Poisoned, got {other:?}"),
        }

        // After a poisoned layout, the entry's NEEDS_LAYOUT flag is
        // still set (geometry was never updated).
        assert!(
            entry.needs_layout(),
            "needs_layout should remain true on the panic path"
        );
    }

    /// `RenderObject::debug_name` returns the concrete type name via
    /// vtable dispatch through `core::any::type_name::<Self>()` in the
    /// monomorphized default body. This is the static identifier used
    /// in `RenderError::Poisoned`.
    #[test]
    fn test_debug_name_via_dyn_dispatch() {
        let panicking: Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>> =
            Box::new(PanickingPaintBox::new());

        let name = panicking.debug_name();
        // Type names include the module path. We only assert that it
        // contains the concrete type's identifier to keep the test
        // independent of compiler-version formatting.
        assert!(
            name.contains("PanickingPaintBox"),
            "debug_name() should resolve to the concrete type via vtable; got `{name}`"
        );
    }

    // ========================================================================
    // D-block PR-A1 U15 — PipelineOwner::mark_needs_layout walk tests
    // ========================================================================
    //
    // Verifies the Flutter `markNeedsLayout` shape ported in U15:
    //   - propagation walks the ancestor chain
    //   - flag is set on every visited node (NEEDS_LAYOUT)
    //   - propagation stops at the first relayout boundary or root
    //   - dirty.needs_layout receives exactly the boundary id
    //   - re-marking an already-dirty node is a no-op
    //   - stale RenderIds (post-removal) terminate the walk silently

    /// Build a 3-level chain root → middle → leaf with `PanickingPaintBox`
    /// mocks via the public `insert` / `insert_child_render_object` APIs,
    /// then clear the dirty queue so tests can observe only post-clear
    /// marks. Returns `(owner, root_id, middle_id, leaf_id)`.
    fn build_three_level_chain() -> (PipelineOwner<Idle>, RenderId, RenderId, RenderId) {
        let mut owner = PipelineOwner::new();
        let root_id = owner.insert(Box::new(PanickingPaintBox::new())
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        let middle_id = owner
            .insert_child_render_object(
                root_id,
                Box::new(PanickingPaintBox::new())
                    as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
            )
            .expect("middle should attach under root");
        let leaf_id = owner
            .insert_child_render_object(
                middle_id,
                Box::new(PanickingPaintBox::new())
                    as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
            )
            .expect("leaf should attach under middle");
        owner.clear_all_dirty_nodes();
        for id in [root_id, middle_id, leaf_id] {
            if let Some(node) = owner.render_tree.get_mut(id) {
                match node {
                    crate::storage::RenderNode::Box(entry) => {
                        entry.state().clear_needs_layout();
                    }
                    crate::storage::RenderNode::Sliver(entry) => {
                        entry.state().clear_needs_layout();
                    }
                }
            }
        }
        (owner, root_id, middle_id, leaf_id)
    }

    /// Marking a leaf where no relayout boundary is set propagates the
    /// `NEEDS_LAYOUT` flag up to root and pushes the root onto
    /// `dirty.needs_layout` (root is the implicit boundary).
    #[test]
    fn mark_needs_layout_walks_to_root_when_no_boundary_set() {
        let (mut owner, root_id, middle_id, leaf_id) = build_three_level_chain();
        assert!(owner.nodes_needing_layout().is_empty());

        owner.mark_needs_layout(leaf_id);

        for (id, label) in [(leaf_id, "leaf"), (middle_id, "middle"), (root_id, "root")] {
            let node = owner.render_tree.get(id).expect(label);
            assert!(
                node.needs_layout(),
                "{label} should have NEEDS_LAYOUT set after walk",
            );
        }
        let dirty = owner.nodes_needing_layout();
        assert_eq!(
            dirty.len(),
            1,
            "exactly one boundary should land on dirty queue, got {dirty:?}",
        );
        assert_eq!(dirty[0].id, root_id, "boundary should be the root id");
    }

    /// Re-marking an already-dirty node short-circuits at step 1 of the
    /// walk — no second push, no flag toggle (flags are idempotent anyway).
    #[test]
    fn mark_needs_layout_is_idempotent_on_repeat() {
        let (mut owner, _root_id, _middle_id, leaf_id) = build_three_level_chain();
        owner.mark_needs_layout(leaf_id);
        let first_dirty_len = owner.nodes_needing_layout().len();
        owner.mark_needs_layout(leaf_id);
        assert_eq!(
            owner.nodes_needing_layout().len(),
            first_dirty_len,
            "second mark on already-dirty subtree must not re-push",
        );
    }

    /// When an intermediate ancestor IS a relayout boundary, propagation
    /// stops at that ancestor — the root above stays clean and the
    /// boundary id (not root) is the one pushed to the dirty queue.
    #[test]
    fn mark_needs_layout_stops_at_intermediate_relayout_boundary() {
        let (mut owner, root_id, middle_id, leaf_id) = build_three_level_chain();
        // Promote `middle` to a relayout boundary via the storage flag (U17
        // wires this from `RenderEntry::layout`'s post-set_constraints
        // compute_relayout_boundary call; this test pre-bootstraps the
        // flag directly to isolate U15 walk behaviour from U17 bootstrap).
        if let Some(crate::storage::RenderNode::Box(entry)) = owner.render_tree.get_mut(middle_id) {
            entry.state().set_relayout_boundary(true);
        }

        owner.mark_needs_layout(leaf_id);

        assert!(
            owner.render_tree.get(leaf_id).expect("leaf").needs_layout(),
            "leaf should be marked",
        );
        assert!(
            owner
                .render_tree
                .get(middle_id)
                .expect("middle")
                .needs_layout(),
            "boundary itself should be marked",
        );
        assert!(
            !owner.render_tree.get(root_id).expect("root").needs_layout(),
            "root above the boundary stays clean",
        );
        let dirty = owner.nodes_needing_layout();
        assert_eq!(dirty.len(), 1);
        assert_eq!(
            dirty[0].id, middle_id,
            "dirty entry should be the boundary, not the root",
        );
    }

    /// Marking a stale `RenderId` (post-removal) terminates the walk
    /// silently with no dirty-queue mutation.
    #[test]
    fn mark_needs_layout_stale_id_is_silent_noop() {
        let mut owner = PipelineOwner::new();
        let phantom = RenderId::new(99);
        owner.mark_needs_layout(phantom);
        assert!(owner.nodes_needing_layout().is_empty());
    }

    /// Leaf `RenderObject<BoxProtocol>` returning a fixed size regardless of
    /// the constraints — used to drive the layout-output debug assertion on
    /// the leaf commit path (`RenderEntry::layout_leaf_only`).
    #[derive(Debug)]
    struct FixedSizeLeaf {
        size: flui_types::Size,
    }

    impl flui_foundation::Diagnosticable for FixedSizeLeaf {}

    impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for FixedSizeLeaf {
        fn perform_layout_raw(
            &mut self,
            _ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<
                '_,
            >,
        ) -> crate::error::RenderResult<
            crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        > {
            Ok(self.size)
        }

        fn paint_raw(
            &self,
            _recorder: &mut crate::context::FragmentRecorder,
            _child_count: usize,
            _size: flui_types::Size,
        ) {
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
            _size: flui_types::Size,
            _hit_child: &mut (
                     dyn FnMut(
                usize,
                Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
            ) -> bool
                         + Send
                         + Sync
                 ),
        ) -> crate::traits::HitTestOutcome {
            crate::traits::HitTestOutcome::miss()
        }
    }

    /// A leaf committing a size that violates the constraints it was laid out
    /// under surfaces `RenderError::InvalidGeometry` on the leaf commit path —
    /// a node returning 999×999 under tight 100×100 is a layout bug.
    #[test]
    fn leaf_committing_a_constraint_violating_size_returns_invalid_geometry() {
        use crate::error::RenderError;

        let mut owner = PipelineOwner::new();
        let root = owner.insert(Box::new(FixedSizeLeaf {
            size: flui_types::Size::new(
                flui_types::geometry::px(999.0),
                flui_types::geometry::px(999.0),
            ),
        })
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(flui_types::Size::new(
            flui_types::geometry::px(100.0),
            flui_types::geometry::px(100.0),
        ))));

        let (_, result) = owner.run_frame();
        match result {
            Err(RenderError::InvalidGeometry { reason, .. }) => {
                assert!(reason.contains("does not satisfy"));
            }
            other => panic!("expected InvalidGeometry, got {other:?}"),
        }
    }

    // 1.1 equivalence test (dead-code removal — NOT a behavior fix).
    // Verifies that removing the paint-retention `retained_subtrees` map
    // does not change the painted output: a fully-dirty frame and a
    // subsequently-clean frame must both produce a non-None layer tree
    // (the boundary paints unconditionally on every dirty frame; a second
    // frame with no dirty marks produces no paint output, not a stale
    // retained result).
    #[test]
    fn repaint_boundary_paints_unconditionally_after_retention_removal() {
        use flui_types::geometry::px;

        let mut owner = PipelineOwner::new();
        let root_node = owner.insert(Box::new(PaintingLeaf::red(40.0, 40.0))
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.set_root_id(Some(root_node));
        owner.set_root_constraints(Some(BoxConstraints::tight(flui_types::Size::new(
            px(40.0),
            px(40.0),
        ))));

        // Frame 1: fully dirty, must paint.
        let (owner, frame1) = owner.run_frame();
        assert!(
            frame1.expect("frame 1 must not error").is_some(),
            "frame 1 (fully dirty) must produce a layer tree"
        );

        // Frame 2: nothing was re-dirtied — idle frame.
        let (_owner, frame2) = owner.run_frame();
        assert!(
            frame2.expect("frame 2 must not error").is_none(),
            "frame 2 (no dirty nodes) must produce no layer tree (equivalence: \
             removing retention does not conjure stale output on clean frames)"
        );
    }
}
