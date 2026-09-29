//! PipelineOwner manages the rendering pipeline.
//!
//! As finalized on 2026-05-20, the four pipeline phases own their work
//! as `run_*` methods on the phase-specific impls. The
//! legacy `flush_*` aliases on `PipelineOwner<Idle>` are gone. Calling
//! `run_paint` on `<Idle>` is a compile error -- see the `compile_fail`
//! doctest at the end of `pipeline/phase.rs`.

mod subtree_arena;

mod accessors;
mod actions;
mod cell;
mod compositing;
mod construction;
mod counters;
mod diagnostics;
mod layout;
mod paint;
mod poison;
mod query;
mod reassemble;
mod relocation;
mod semantics;

pub use cell::{PipelineCell, WeakPipelineCell};
pub use counters::PipelineCounters;
pub use relocation::{
    AttachRenderSubtreesError, AttachRenderSubtreesFailure, DetachRenderSubtreesError,
    DetachedRenderSubtrees, ReleaseDetachedRenderSubtreesError,
    ReleaseDetachedRenderSubtreesFailure,
};

use std::{
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU64},
};

use flui_foundation::RenderId;
use flui_foundation::geometry::Offset;
use flui_layer::LayerTree;
use flui_semantics::SemanticsOwner;
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::{constraints::BoxConstraints, storage::RenderTree};

use super::{
    handle::{DirtyRequest, DirtySender},
    notifier::VisualUpdateNotifier,
    phase::{Idle, PipelinePhase},
    scheduler::DirtyTracker,
};

/// Default bounded capacity of the dirty-request channel between
/// node-bound [`crate::pipeline::RenderInvalidationHandle`] producers and the owner receiver.
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
/// -- it used `Arc<RwLock<PipelineOwner>>` for tree nodes, an
/// anti-pattern this crate refuses.
pub struct PipelineOwner<Phase: PipelinePhase = Idle> {
    /// Unique identifier for this pipeline owner.
    id: u64,

    /// Allocation identity binding linear relocation tokens to this owner.
    /// Pointer identity is sufficient; unlike a numeric id it cannot collide
    /// or require process-global token bookkeeping.
    relocation_owner_seal: Rc<relocation::RelocationOwnerSeal>,

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

    /// Bounded-retry poison state for layout failures: consecutive-failure
    /// counters and poison flags per node, owned here (not per-node) so a
    /// healthy tree carries zero overhead. See [`poison::LayoutPoison`].
    layout_poison: poison::LayoutPoison,

    /// Constraints to pass to [`Self::layout_dirty_root`] when the
    /// dirty entry is the tree root (`root_id`) and the root has no
    /// cached `state.constraints()` yet (first frame).
    ///
    /// The binding layer (`flui-view` /
    /// `flui-app` / `flui-hot-reload`) sets this once per
    /// configuration via [`Self::set_root_constraints`] before the
    /// first `run_frame` invocation. On subsequent frames the root's
    /// cached constraints (post-layout) supersede this field; the
    /// fallback only fires on the very first layout pass.
    root_constraints: Option<BoxConstraints>,

    /// Whether semantics are enabled.
    semantics_enabled: AtomicBool,

    /// The semantics tree owner (flui-semantics ARCHITECTURE.md, semantics assembly). `None` until semantics is enabled via
    /// [`Self::set_semantics_enabled`]`(true)`, which lazily creates it and
    /// fires `fire_semantics_owner_created`; disposed (firing
    /// `fire_semantics_owner_disposed`) on the next `false` transition.
    /// `Semantics::run_semantics` (`pipeline/owner/semantics.rs`) is the
    /// sole writer of its tree contents.
    semantics_owner: Option<SemanticsOwner>,

    /// The platform-publish callback for semantics owners this pipeline
    /// creates.
    ///
    /// Set by the composition root once it holds a platform accessibility
    /// bridge ([`Self::set_semantics_update_callback`]); consumed at every
    /// lazy [`SemanticsOwner`] creation in [`Self::set_semantics_enabled`],
    /// and swapped onto a live owner immediately. `None` keeps the
    /// documented placeholder behaviour: an owner that assembles a tree and
    /// publishes nowhere (the state before a bridge is installed).
    semantics_update_callback: Option<flui_semantics::SemanticsUpdateCallback>,

    /// The layer tree produced by the last paint phase.
    last_layer_tree: Option<LayerTree>,

    /// Composite-resolved offsets for `Layer::Follower` render nodes,
    /// keyed by `RenderId` (flui-rendering ARCHITECTURE.md, follower hit-testing) — a per-frame byproduct mirroring
    /// `last_layer_tree`. Populated post-paint
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

    /// Painted output kept per repaint boundary, reused when that boundary is
    /// clean on a later frame.
    ///
    /// Keyed by the boundary's `RenderId`, holding the layer subtree its last
    /// paint produced. `run_paint` grafts the entry instead of descending when
    /// the boundary is absent from the paint queue, which is only sound
    /// because `run_layout` queues exactly the boundaries layout touched and
    /// `mark_needs_paint` queues the rest.
    ///
    /// Entries are replaced on repaint and evicted when their node leaves the
    /// tree. A stale entry cannot be served to a different node: `RenderId` is
    /// generational, so a recycled slab slot carries a new generation and
    /// misses the map.
    retained_boundaries: FxHashMap<RenderId, paint::RetainedSubtree>,

    /// `RenderId`s of `Layer::Follower` nodes correlated during the last
    /// paint phase that resolved to `None` (unlinked with
    /// `show_when_unlinked == false`) — the follower hit-test side table's companion to
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
    device_pixel_ratio: f64,

    /// Private sender cloned only into node-bound invalidation capabilities.
    dirty_sender: DirtySender,

    /// Receiver end of the bounded dirty-request channel. Drained into
    /// `dirty` by `drain_pending_dirty` at phase boundaries.
    dirty_rx: crossbeam_channel::Receiver<DirtyRequest>,

    /// Harness-only parent-data presets keyed by child [`RenderId`].
    ///
    /// Cloned into per-walk [`ErasedChildState`](crate::protocol::ErasedChildState) /
    /// [`ErasedSliverChildState`](crate::protocol::ErasedSliverChildState)
    /// slots before layout so headless tests can express widget-level
    /// configuration (stack positioning, flex factors, future animation
    /// parent slots) without an element tree.
    #[cfg(any(test, feature = "testing"))]
    parent_data_seeds: FxHashMap<RenderId, ParentDataSeed>,

    /// One-shot semantics failure injected after paint by cross-crate frame
    /// integration tests. Absent from production builds.
    #[cfg(any(test, feature = "testing"))]
    semantics_error_once_for_test: Option<crate::error::RenderError>,

    /// Child-build requests accumulated during the most recent layout pass
    /// by request-strategy slivers.  Each entry is `(sliver_id,
    /// logical_index)`.  The binding layer drains this via
    /// [`Self::take_pending_child_requests`] after the frame to service the
    /// requests through the element tree.  Empty between frames.
    pending_child_requests: Vec<(RenderId, usize)>,
    /// Retain-band signals from element-owned slivers (the removal half).
    ///
    /// Each entry is `(sliver_id, cache_first, cache_last)`: the retained
    /// logical index band `[first, last)` emitted by
    /// `RenderSliverList::perform_layout` after each walk.  The binding layer
    /// consumes this via [`Self::take_pending_retain_bands`] after every frame
    /// to drive `SparseChildren::retain_band` through the element tree,
    /// evicting out-of-band lazy children. The render side never disposes a
    /// child itself, which is what avoids double-removing element-owned
    /// nodes.
    pending_retain_bands: Vec<(RenderId, usize, usize)>,

    /// Monotonic work totals since construction, read through
    /// [`Self::counters`]. `layout_roots` is not stored here: the scheduler
    /// already counts it and the accessor fills it in.
    counters: PipelineCounters,

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
        relocation_owner_seal: from.relocation_owner_seal,
        render_tree: from.render_tree,
        root_id: from.root_id,
        notifier: from.notifier,
        scheduler: from.scheduler,
        layout_poison: from.layout_poison,
        root_constraints: from.root_constraints,
        semantics_enabled: from.semantics_enabled,
        semantics_owner: from.semantics_owner,
        semantics_update_callback: from.semantics_update_callback,
        last_layer_tree: from.last_layer_tree,
        last_follower_offsets: from.last_follower_offsets,
        retained_boundaries: from.retained_boundaries,
        last_hidden_follower_ids: from.last_hidden_follower_ids,
        device_pixel_ratio: from.device_pixel_ratio,
        dirty_sender: from.dirty_sender,
        dirty_rx: from.dirty_rx,
        #[cfg(any(test, feature = "testing"))]
        parent_data_seeds: from.parent_data_seeds,
        #[cfg(any(test, feature = "testing"))]
        semantics_error_once_for_test: from.semantics_error_once_for_test,
        pending_child_requests: from.pending_child_requests,
        pending_retain_bands: from.pending_retain_bands,
        counters: from.counters,
        _phase: PhantomData,
    }
}

// ============================================================================
// Cross-phase accessors
// ============================================================================

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Takes all child-build requests accumulated during the most recent
    /// layout pass, leaving the buffer empty.
    ///
    /// Each entry is `(sliver_id, logical_index)`: the sliver whose
    /// `request_child_build` fired, and the logical item index it could not
    /// materialize synchronously.  The binding layer consumes this
    /// after every frame to drive the element-tree child manager.
    ///
    /// `pub` (not `pub(super)`) because the only caller is external — no
    /// in-module consumer exists.
    #[must_use]
    pub fn take_pending_child_requests(&mut self) -> Vec<(RenderId, usize)> {
        std::mem::take(&mut self.pending_child_requests)
    }

    /// Takes all retain-band signals accumulated during the most recent layout
    /// pass (the removal half), leaving the buffer empty.
    ///
    /// Each entry is `(sliver_id, cache_first, cache_last)`.  The binding
    /// layer calls this to drive `SparseChildren::retain_band` through the
    /// element tree, evicting out-of-band lazy children. The render side
    /// never disposes a child itself, which is what avoids an ABA
    /// double-remove of element-owned render nodes.
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

    use flui_foundation::Leaf;
    use flui_foundation::geometry::Size;

    use super::*;
    use crate::{context::BoxLayoutContext, parent_data::BoxParentData, traits::RenderBox};

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
                size: Size::new(10.0, 10.0),
            }
        }

        fn boundary_labeled(label: &'static str) -> Self {
            Self {
                label: Some(label),
                boundary: true,
                merge_descendants: false,
                exclude_descendants: false,
                size: Size::new(10.0, 10.0),
            }
        }

        fn empty() -> Self {
            Self {
                label: None,
                boundary: false,
                merge_descendants: false,
                exclude_descendants: false,
                size: Size::new(10.0, 10.0),
            }
        }
    }

    impl flui_foundation::Diagnosticable for SemanticLeaf {}

    /// A leaf whose label can change between passes — what a live widget
    /// does. `SemanticLeaf`'s `&'static str` label is frozen at insertion,
    /// so it can prove structure but never "the published content follows
    /// the render object's current state through a mark-scoped pass".
    #[derive(Debug)]
    struct MutableLeaf {
        label: std::sync::Arc<std::sync::Mutex<String>>,
        boundary: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl MutableLeaf {
        fn labeled(text: &str) -> (Self, std::sync::Arc<std::sync::Mutex<String>>) {
            let label = std::sync::Arc::new(std::sync::Mutex::new(text.to_owned()));
            (
                Self {
                    label: std::sync::Arc::clone(&label),
                    boundary: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                },
                label,
            )
        }
    }

    impl flui_foundation::Diagnosticable for MutableLeaf {}

    impl RenderBox for MutableLeaf {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constraints().constrain(Size::new(10.0, 10.0))
        }

        fn describe_semantics_configuration(
            &self,
            config: &mut crate::semantics::SemanticsConfiguration,
        ) {
            config.set_semantics_boundary(self.boundary.load(std::sync::atomic::Ordering::Relaxed));
            config.set_label(
                self.label
                    .lock()
                    .expect("BUG: label mutex is uncontended in single-threaded tests")
                    .as_str(),
            );
        }
    }

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
            if self.merge_descendants {
                config.set_merging_semantics_of_descendants(true);
            }
            if let Some(label) = self.label {
                config.set_label(label);
            }
        }

        fn excludes_semantics_subtree(&self) -> bool {
            self.exclude_descendants
        }
    }

    // test_pipeline_owner_hierarchy removed along with the
    // adopt_child/drop_child/child_count/children API. Multi-PipelineOwner
    // scenarios (multi-window) are now owned by flui-app side-by-side.

    /// The pipeline-level idle contract: `run_semantics` reassembles the
    /// whole arena whenever anything is marked (flui-semantics ARCHITECTURE.md, semantics assembly), but a pass whose
    /// assembly reproduces the same tree must deliver NOTHING to the
    /// platform — the owner's flush diffs per-node payloads against the
    /// last delivered update. Before that diff, every marked-but-unchanged
    /// pass republished the entire tree.
    #[test]
    fn an_identical_second_semantics_pass_publishes_nothing() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&captured);

        let mut owner = PipelineOwner::new();
        owner.set_semantics_update_callback(std::sync::Arc::new(
            move |update: &flui_semantics::TreeUpdate| {
                sink.lock()
                    .expect("BUG: capture mutex is uncontended in this single-threaded test")
                    .push(update.clone());
            },
        ));
        let root_id = owner.set_root_render_object(Box::new(SemanticLeaf::labeled("Submit")));
        owner.set_semantics_enabled(true);

        let owner = owner.into_layout().into_compositing().into_paint();
        let mut owner = owner.into_semantics();
        owner.run_semantics().expect("first semantics pass");
        let mut owner = owner.finish();
        assert_eq!(
            captured
                .lock()
                .expect("BUG: capture mutex is uncontended in this single-threaded test")
                .len(),
            1,
            "the first pass publishes the initializing full tree"
        );

        // Mark the root again without changing anything observable — the
        // shape of "a repaint touched this subtree, semantics did not move".
        owner.mark_needs_semantics(root_id);
        let owner = owner.into_layout().into_compositing().into_paint();
        let mut owner = owner.into_semantics();
        owner.run_semantics().expect("second semantics pass");

        assert_eq!(
            captured
                .lock()
                .expect("BUG: capture mutex is uncontended in this single-threaded test")
                .len(),
            1,
            "a pass that reassembles an identical tree must publish nothing"
        );
    }

    /// The mark-scoped assembly contract: a change under one boundary
    /// re-assembles that boundary's subtree — counted in render-node visits
    /// — and republishes only the node whose payload changed. The sibling
    /// branch is neither re-visited nor republished.
    #[test]
    fn a_local_change_reassembles_only_the_affected_subtree() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&captured);
        let mut owner = PipelineOwner::new();
        owner.set_semantics_update_callback(std::sync::Arc::new(
            move |update: &flui_semantics::TreeUpdate| {
                sink.lock()
                    .expect("BUG: capture mutex is uncontended in this single-threaded test")
                    .push(update.clone());
            },
        ));
        let root = owner.set_root_render_object(Box::new(SemanticLeaf::empty()));
        let branch_a = owner
            .insert_child_render_object(root, Box::new(SemanticLeaf::boundary_labeled("A")))
            .expect("branch A inserted");
        let (leaf, label) = MutableLeaf::labeled("original");
        let a_leaf = owner
            .insert_child_render_object(branch_a, Box::new(leaf))
            .expect("A's leaf inserted");
        let branch_b = owner
            .insert_child_render_object(root, Box::new(SemanticLeaf::boundary_labeled("B")))
            .expect("branch B inserted");
        for _ in 0..8 {
            owner
                .insert_child_render_object(branch_b, Box::new(SemanticLeaf::labeled("filler")))
                .expect("B filler inserted");
        }
        owner.set_semantics_enabled(true);

        super::semantics::reset_assembly_visits();
        let owner = owner.into_layout().into_compositing().into_paint();
        let mut owner = owner.into_semantics();
        owner.run_semantics().expect("first semantics pass");
        let mut owner = owner.finish();
        let full_visits = super::semantics::assembly_visits();
        assert_eq!(full_visits, 12, "the initializing pass walks all 12 nodes");

        *label
            .lock()
            .expect("BUG: label mutex is uncontended in this single-threaded test") =
            "changed".to_owned();
        owner.mark_needs_semantics(a_leaf);

        super::semantics::reset_assembly_visits();
        let owner = owner.into_layout().into_compositing().into_paint();
        let mut owner = owner.into_semantics();
        owner.run_semantics().expect("mark-scoped semantics pass");

        assert_eq!(
            super::semantics::assembly_visits(),
            2,
            "only the anchoring boundary A and its leaf are re-assembled — \
             never the root or the 9-node B branch"
        );

        let updates = captured
            .lock()
            .expect("BUG: capture mutex is uncontended in this single-threaded test");
        assert_eq!(updates.len(), 2, "the change itself is delivered");
        let diff = &updates[1];
        assert_eq!(
            diff.nodes.len(),
            1,
            "exactly the changed boundary is republished"
        );
        let published = diff.nodes[0].1.label().expect("the boundary has a label");
        assert!(
            published.contains("changed") && !published.contains("original"),
            "and it carries the render object's current content: {published:?}"
        );
        assert!(
            !diff.nodes.iter().any(|(_, node)| node.label() == Some("B")),
            "the untouched sibling branch must not ride along"
        );
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

        owner.scheduler.schedule_paint_boundary(RenderId::new(2), 1);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            2,
            "a new paint entry must wake the platform",
        );
        owner.scheduler.schedule_paint_boundary(RenderId::new(2), 1);
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    // ========================================================================
    // catch_unwind plumbing
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
        size: flui_foundation::geometry::Size,
    }

    impl PanickingPaintBox {
        fn new() -> Self {
            Self {
                size: flui_foundation::geometry::Size::ZERO,
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
            _size: flui_foundation::geometry::Size,
        ) {
            panic!("PanickingPaintBox::paint_raw -- intentional test panic");
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
            _size: flui_foundation::geometry::Size,
            _hit_child: &mut dyn FnMut(
                usize,
                Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
                Option<flui_foundation::geometry::Matrix4>,
            ) -> bool,
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
            _size: flui_foundation::geometry::Size,
        ) {
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
            _size: flui_foundation::geometry::Size,
            _hit_child: &mut dyn FnMut(
                usize,
                Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
                Option<flui_foundation::geometry::Matrix4>,
            ) -> bool,
        ) -> crate::traits::HitTestOutcome {
            crate::traits::HitTestOutcome::miss()
        }
    }

    /// A panicking `paint` call must surface as
    /// `RenderError::Poisoned { phase: PoisonPhase::Paint, .. }` and not
    /// abort. The owner must remain usable for a subsequent frame.
    #[test]
    fn test_run_frame_catches_paint_panic() {
        use crate::constraints::BoxConstraints;
        use crate::error::{PoisonPhase, RenderError};

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
        owner.set_root_constraints(Some(BoxConstraints::new(0.0, 200.0, 0.0, 200.0)));

        let (owner, result) = owner.run_frame();

        std::panic::set_hook(prev);

        // The frame produces an error of the Poisoned variant.
        let err = result.expect_err("paint should panic, surface as Err");
        match err {
            RenderError::Poisoned { phase, .. } => {
                assert_eq!(phase, PoisonPhase::Paint, "phase should be Paint");
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
        owner.mark_needs_paint(root_id);

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
    /// `RenderError::Poisoned { phase: PoisonPhase::Layout, .. }` through
    /// `RenderEntry::layout`. This verifies the catch_unwind wrapper on
    /// the layout call site.
    ///
    /// Note: `RenderEntry::layout` is not yet wired into the pipeline
    /// owner's `run_layout` (the propagation stubs are empty per the
    /// Mythos Outstanding Refactors list), so this test exercises the
    /// entry directly rather than through `run_frame`.
    #[test]
    fn test_render_entry_layout_catches_panic() {
        use crate::error::{PoisonPhase, RenderError};
        use crate::storage::RenderEntry;
        use flui_foundation::geometry::Size;

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
                assert_eq!(phase, PoisonPhase::Layout, "phase should be Layout");
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

    // ========================================================================
    // PipelineOwner::mark_needs_layout walk tests
    // ========================================================================
    //
    // Verifies the Flutter `markNeedsLayout` shape ported here:
    //   - propagation walks the ancestor chain
    //   - flag is set on every visited node (NEEDS_LAYOUT)
    //   - propagation stops at the first relayout boundary or root
    //   - dirty.needs_layout receives exactly the boundary id
    //   - re-marking an already-dirty node is a no-op
    //   - stale RenderIds (post-removal) terminate the walk silently

    /// Leaf `RenderObject<BoxProtocol>` returning a fixed size regardless of
    /// the constraints — used to drive the layout-output debug assertion on
    /// the leaf commit path (`RenderEntry::layout_leaf_only`).
    #[derive(Debug)]
    struct FixedSizeLeaf {
        size: flui_foundation::geometry::Size,
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
            _size: flui_foundation::geometry::Size,
        ) {
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
            _size: flui_foundation::geometry::Size,
            _hit_child: &mut dyn FnMut(
                usize,
                Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
                Option<flui_foundation::geometry::Matrix4>,
            ) -> bool,
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
            size: flui_foundation::geometry::Size::new(999.0, 999.0),
        })
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(
            flui_foundation::geometry::Size::new(100.0, 100.0),
        )));

        let (_, result) = owner.run_frame();
        match result {
            Err(RenderError::InvalidGeometry { reason, .. }) => {
                assert!(reason.contains("does not satisfy"));
            }
            other => panic!("expected InvalidGeometry, got {other:?}"),
        }
    }
}
