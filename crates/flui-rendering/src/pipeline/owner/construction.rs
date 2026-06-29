//! Constructor, ctor helpers, and run_frame orchestration for `PipelineOwner<Idle>`.

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};

use flui_layer::LayerTree;
#[cfg(any(test, feature = "testing"))]
use rustc_hash::FxHashMap;

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::storage::RenderTree;

use crate::pipeline::{
    handle::PipelineOwnerHandle,
    notifier::VisualUpdateNotifier,
    phase::{Idle, Layout},
    scheduler::DirtyTracker,
};

use super::{DEFAULT_DIRTY_CHANNEL_CAPACITY, PIPELINE_ID_COUNTER, PipelineOwner, rebind_phase};

impl PipelineOwner<Idle> {
    /// Creates a new pipeline owner in the [`Idle`] phase with the
    /// default dirty-channel capacity (`DEFAULT_DIRTY_CHANNEL_CAPACITY`,
    /// 256).
    pub fn new() -> Self {
        Self::new_with_capacity(DEFAULT_DIRTY_CHANNEL_CAPACITY)
    }

    /// Creates a new pipeline owner in the [`Idle`] phase with a custom
    /// dirty-channel capacity. Use this when the default 256 doesn't match
    /// the producer profile.
    pub fn new_with_capacity(dirty_channel_capacity: usize) -> Self {
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(VisualUpdateNotifier::new()));
        let (handle, dirty_rx) =
            PipelineOwnerHandle::new_pair(dirty_channel_capacity, std::sync::Arc::clone(&notifier));
        let scheduler = DirtyTracker::new(std::sync::Arc::clone(&notifier));
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier,
            scheduler,
            root_constraints: None,
            semantics_enabled: AtomicBool::new(false),
            last_layer_tree: None,
            device_pixel_ratio: 1.0,
            deferred_mutations: crate::pipeline::deferred::DeferredMutations::new(),
            handle,
            dirty_rx,
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            pending_child_requests: Vec::new(),
            pending_retain_bands: Vec::new(),
            _phase: PhantomData,
        }
    }

    /// Records harness parent metadata for `child_id`, cloned into the
    /// transient child slots before each layout walk.
    ///
    /// A second call for the same `child_id` replaces the previous seed.
    #[cfg(any(test, feature = "testing"))]
    pub fn seed_parent_data(&mut self, child_id: flui_foundation::RenderId, seed: ParentDataSeed) {
        self.parent_data_seeds.insert(child_id, seed);
    }

    /// Creates a new pipeline owner with callbacks in the [`Idle`] phase.
    pub fn with_callbacks<F, G, H>(
        on_need_visual_update: Option<F>,
        on_semantics_owner_created: Option<G>,
        on_semantics_owner_disposed: Option<H>,
    ) -> Self
    where
        F: Fn() + Send + Sync + 'static,
        G: Fn() + Send + Sync + 'static,
        H: Fn() + Send + Sync + 'static,
    {
        let mut notifier = VisualUpdateNotifier::new();
        if let Some(f) = on_need_visual_update {
            notifier.set_need_visual_update(f);
        }
        if let Some(f) = on_semantics_owner_created {
            notifier.set_semantics_owner_created(f);
        }
        if let Some(f) = on_semantics_owner_disposed {
            notifier.set_semantics_owner_disposed(f);
        }
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(notifier));
        let (handle, dirty_rx) = PipelineOwnerHandle::new_pair(
            DEFAULT_DIRTY_CHANNEL_CAPACITY,
            std::sync::Arc::clone(&notifier),
        );
        let scheduler = DirtyTracker::new(std::sync::Arc::clone(&notifier));
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier,
            scheduler,
            root_constraints: None,
            semantics_enabled: AtomicBool::new(false),
            last_layer_tree: None,
            device_pixel_ratio: 1.0,
            deferred_mutations: crate::pipeline::deferred::DeferredMutations::new(),
            handle,
            dirty_rx,
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            pending_child_requests: Vec::new(),
            pending_retain_bands: Vec::new(),
            _phase: PhantomData,
        }
    }

    /// Transitions an idle pipeline into the [`Layout`] phase.
    ///
    /// Consumes `self`; once transitioned out of `Idle`, the legacy
    /// idle-only API (constructors, `run_frame`) is no longer reachable
    /// until you return through [`finish`](PipelineOwner::<crate::pipeline::phase::Semantics>::finish).
    #[must_use]
    pub fn into_layout(self) -> PipelineOwner<Layout> {
        rebind_phase(self)
    }

    // ========================================================================
    // Full-frame orchestrator (Mythos Step 7)
    // ========================================================================

    /// Runs a full frame: layout -> compositing-bits -> paint -> semantics.
    /// Consumes `self`, returns the owner back at [`Idle`] plus a
    /// [`RenderResult`](crate::RenderResult) indicating whether the frame produced a layer
    /// tree or failed mid-phase.
    ///
    /// The phase transitions are the load-bearing mechanism here -- each
    /// `run_*` method lives only on its matching phase's impl block, so
    /// the type system enforces the ordering. There is no runtime branch
    /// that could call `run_paint` before `run_layout`.
    ///
    /// # Mythos Step 12 -- error handling
    ///
    /// If any phase returns [`crate::error::RenderError`] (most notably
    /// [`crate::error::RenderError::Poisoned`] from a panicking render
    /// object), the in-flight frame is dropped, the owner is returned at
    /// [`Idle`] (no in-flight layer tree), and the second element of the
    /// tuple is `Err(...)`. The owner is **always** usable for a
    /// subsequent frame on the success and error paths alike.
    #[must_use = "dropping the returned PipelineOwner<Idle> discards the pipeline handle; thread it back into the next frame"]
    pub fn run_frame(
        mut self,
    ) -> (
        PipelineOwner<Idle>,
        crate::error::RenderResult<Option<LayerTree>>,
    ) {
        // Observe cross-thread dirty requests (RepaintHandle /
        // PipelineOwnerHandle producers) before any phase runs — an
        // async decode that finished while the app idled lands in this
        // frame, not never.
        self.drain_pending_dirty();

        // Layout
        let mut owner = self.into_layout();
        if let Err(e) = owner.run_layout() {
            return (owner.into_idle(), Err(e));
        }

        // Compositing
        let mut owner = owner.into_compositing();
        if let Err(e) = owner.run_compositing() {
            return (owner.into_idle(), Err(e));
        }

        // Paint
        let mut owner = owner.into_paint();
        if let Err(e) = owner.run_paint() {
            return (owner.into_idle(), Err(e));
        }

        // Semantics
        let mut owner = owner.into_semantics();
        if let Err(e) = owner.run_semantics() {
            // Semantics phase has no `into_idle` because the transition
            // to <Idle> goes via `finish`. Use `finish` to recover the
            // owner for the error path -- the layer tree from the paint
            // phase is discarded on error to keep the invariant "Err =>
            // no layer tree".
            return (owner.finish(), Err(e));
        }

        let layer_tree = owner.take_layer_tree();
        (owner.finish(), Ok(layer_tree))
    }
}
