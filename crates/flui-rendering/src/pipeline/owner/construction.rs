//! Constructor, ctor helpers, and run_frame orchestration for `PipelineOwner<Idle>`.

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};

use flui_layer::LayerTree;
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::storage::RenderTree;

use crate::pipeline::{
    PipelinePhase, TextContextHandle,
    handle::DirtySender,
    notifier::VisualUpdateNotifier,
    phase::{Idle, Layout},
    scheduler::DirtyTracker,
};

use super::{DEFAULT_DIRTY_CHANNEL_CAPACITY, PIPELINE_ID_COUNTER, PipelineOwner, rebind_phase};

impl PipelineOwner<Idle> {
    /// Creates a new pipeline owner in the [`Idle`] phase with the
    /// default dirty-channel capacity (`DEFAULT_DIRTY_CHANNEL_CAPACITY`,
    /// 256), measuring text through `text`.
    ///
    /// `text` is lent to every measurement this pipeline makes: layout,
    /// intrinsics, dry layout and dry baselines (ADR-0092 §10 step 3). A
    /// realm passes the context it built over the app's font collection; a
    /// pipeline with no realm behind it passes
    /// [`TextContextHandle::standalone`]. There is no constructor without a
    /// context, so no pipeline measures on one it made up for itself:
    /// `tests/compile_fail.rs` pins the missing-context and missing-`Default`
    /// diagnostics alongside a passing context-backed pipeline caller.
    ///
    /// ```compile_fail
    /// let owner = flui_rendering::PipelineOwner::new();
    /// ```
    ///
    /// and no `Default` to pick one:
    ///
    /// ```compile_fail
    /// let owner: flui_rendering::PipelineOwner = Default::default();
    /// ```
    pub fn new(text: TextContextHandle) -> Self {
        Self::new_with_capacity(DEFAULT_DIRTY_CHANNEL_CAPACITY, text)
    }

    /// Creates a new pipeline owner in the [`Idle`] phase with a custom
    /// dirty-channel capacity, measuring text through `text` (see
    /// [`Self::new`]). Use this when the default 256 doesn't match the
    /// producer profile.
    pub fn new_with_capacity(dirty_channel_capacity: usize, text: TextContextHandle) -> Self {
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(VisualUpdateNotifier::new()));
        let (dirty_sender, dirty_rx) =
            DirtySender::new_pair(dirty_channel_capacity, std::sync::Arc::clone(&notifier));
        let scheduler = DirtyTracker::new(std::sync::Arc::clone(&notifier));
        Self {
            // A context lent while a pipeline is built is a measurement
            // building one: seed zero, and the first drain after the loan
            // applies whatever changed, with nothing recorded to lay out.
            fonts_seen: text.fonts_generation().unwrap_or(0),
            text_measurers: crate::pipeline::TextMeasurers::default(),
            text,
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            relocation_owner_seal: std::rc::Rc::new(super::relocation::RelocationOwnerSeal),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier,
            scheduler,
            layout_poison: super::poison::LayoutPoison::default(),
            root_constraints: None,
            semantics_enabled: AtomicBool::new(false),
            semantics_owner: None,
            semantics_update_callback: None,
            last_layer_tree: None,
            last_follower_offsets: FxHashMap::default(),
            retained_boundaries: FxHashMap::default(),
            root_content: None,
            last_hidden_follower_ids: FxHashSet::default(),
            device_pixel_ratio: 1.0,
            dirty_sender,
            dirty_rx,
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            #[cfg(any(test, feature = "testing"))]
            semantics_error_once_for_test: None,
            pending_child_requests: Vec::new(),
            pending_retain_bands: Vec::new(),
            counters: super::PipelineCounters::default(),
            _phase: PhantomData,
        }
    }

    /// Moves this owner out, leaving an empty one in its place that measures
    /// through the same text context.
    ///
    /// For a caller that holds the owner behind `&mut` and must pass it by
    /// value through a typestate transition (`run_frame`, `into_layout`),
    /// then put the result back. The placeholder is what the slot holds if
    /// that transition unwinds, so it keeps the pipeline's text context
    /// rather than a context of its own.
    #[must_use = "the taken owner holds the render tree; put it back"]
    pub fn take_idle(&mut self) -> Self {
        let placeholder = Self::new(self.text.clone());
        std::mem::replace(self, placeholder)
    }

    /// Records harness parent metadata for `child_id`, cloned into the
    /// transient child slots before each layout walk.
    ///
    /// A second call for the same `child_id` replaces the previous seed.
    #[cfg(any(test, feature = "testing"))]
    pub fn seed_parent_data(&mut self, child_id: flui_foundation::RenderId, seed: ParentDataSeed) {
        self.parent_data_seeds.insert(child_id, seed);
    }

    /// Fail the next frame's semantics stage after paint has committed.
    ///
    /// This one-shot integration-test seam is absent from production builds.
    /// It exists so upper-layer frame drivers can verify how they consume a
    /// painted tree and its leader/follower registry when semantics fails.
    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn fail_next_semantics_after_paint_for_test(&mut self, error: crate::error::RenderError) {
        self.semantics_error_once_for_test = Some(error);
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
    // Full-frame orchestrator
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
    /// # Error handling
    ///
    /// If any phase returns [`crate::error::RenderError`] (most notably
    /// [`crate::error::RenderError::Poisoned`] from a panicking render
    /// object), the owner is returned at [`Idle`] and the second element of
    /// the tuple is `Err(...)`. A paint tree already committed before a
    /// semantics error remains in the owner's `last_layer_tree`; it is not
    /// returned from this failed call, but a retry can repaint or submit the
    /// retained visual result. The owner is **always** usable for a
    /// subsequent frame on the success and error paths alike.
    #[must_use = "dropping the returned PipelineOwner<Idle> discards the pipeline handle; thread it back into the next frame"]
    pub fn run_frame(
        self,
    ) -> (
        PipelineOwner<Idle>,
        crate::error::RenderResult<Option<LayerTree>>,
    ) {
        self.run_frame_impl()
    }

    fn run_frame_impl(
        mut self,
    ) -> (
        PipelineOwner<Idle>,
        crate::error::RenderResult<Option<LayerTree>>,
    ) {
        // Observe node-bound cross-thread invalidation before any phase runs — an
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
        #[cfg(any(test, feature = "testing"))]
        let semantics_result = match owner.semantics_error_once_for_test.take() {
            Some(error) => Err(error),
            None => owner.run_semantics(),
        };
        #[cfg(not(any(test, feature = "testing")))]
        let semantics_result = owner.run_semantics();
        if let Err(e) = semantics_result {
            // Semantics phase has no `into_idle` because the transition
            // to <Idle> goes via `finish`. Use `finish` to recover the
            // owner for the error path. The painted tree remains in
            // `last_layer_tree`; this failed call returns no tree, while a
            // retry can reuse the retained visual result.
            return (owner.finish(), Err(e));
        }

        let layer_tree = owner.take_layer_tree();
        if layer_tree.is_some() {
            owner.counters.frames_produced += 1;
        }
        (owner.finish(), Ok(layer_tree))
    }
}

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// The text context this pipeline was built with.
    pub(crate) fn text_handle(&self) -> TextContextHandle {
        self.text.clone()
    }

    /// The text context this pipeline was built with, so a test can tell
    /// whose context its layout measures with.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn text_context_for_test(&self) -> &TextContextHandle {
        &self.text
    }
}
