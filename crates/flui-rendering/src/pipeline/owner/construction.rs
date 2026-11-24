//! Constructors and the frame orchestration of `PipelineOwner`.

use flui_layer::SceneWriter;
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::storage::RenderTree;

use crate::pipeline::{
    TextContextHandle, handle::DirtySender, notifier::SharedNotifier, scheduler::DirtyTracker,
};

use super::{DEFAULT_DIRTY_CHANNEL_CAPACITY, PipelineOwner};

impl PipelineOwner {
    /// Creates a new pipeline owner with the
    /// default dirty-channel capacity (`DEFAULT_DIRTY_CHANNEL_CAPACITY`,
    /// 256), measuring text through `text`.
    ///
    /// `text` is lent to every measurement this pipeline makes: layout,
    /// intrinsics, dry layout and dry baselines (ADR-0092 §10 step 3). A
    /// UI runtime passes the context it built over the app's font collection; a
    /// pipeline with no UI runtime behind it passes
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

    /// Creates a new pipeline owner with a custom
    /// dirty-channel capacity, measuring text through `text` (see
    /// [`Self::new`]). Use this when the default 256 doesn't match the
    /// producer profile.
    pub fn new_with_capacity(dirty_channel_capacity: usize, text: TextContextHandle) -> Self {
        let notifier = SharedNotifier::default();
        let (dirty_sender, dirty_rx) =
            DirtySender::new_pair(dirty_channel_capacity, notifier.clone());
        let local_dirty = super::super::local_invalidation::LocalDirtyInbox::new(notifier.clone());
        let scheduler = DirtyTracker::new(notifier.clone());
        Self {
            // A context lent while a pipeline is built is a measurement
            // building one: seed zero, and the first drain after the loan
            // applies whatever changed, with nothing recorded to lay out.
            fonts_seen: text.fonts_generation().unwrap_or(0),
            text_measurers: crate::pipeline::TextMeasurers::default(),
            text_sizing: flui_painting::TextSizing::default(),
            text,
            relocation_owner_seal: std::rc::Rc::new(super::relocation::RelocationOwnerSeal),
            render_tree: RenderTree::new(),
            transform_paths: super::buffer_pool::BufferPool::default(),
            root_id: None,
            notifier,
            scheduler,
            layout_poison: super::poison::LayoutPoison::default(),
            layout_buffers: super::subtree_arena::LayoutBuffers::default(),
            root_constraints: None,
            semantics_enabled: false,
            semantics_owner: None,
            semantics_regions: super::semantics::RegionIdentities::default(),
            frame_arena: flui_foundation::arena::FrameArena::new(),
            built_arena: flui_foundation::arena::FrameArena::new(),
            semantics_diagnostics: std::collections::VecDeque::new(),
            assistive_focus: flui_semantics::AssistiveFocus::default(),
            #[cfg(any(test, feature = "testing"))]
            unchecked_semantics_diagnostics: Vec::new(),
            semantics_update_callback: None,
            semantics_route_change_sink: None,
            last_follower_transforms: FxHashMap::default(),
            last_child_paint_orders: FxHashMap::default(),
            leader_watches: std::rc::Rc::default(),
            boundary_records: FxHashMap::default(),
            follower_boundaries: FxHashSet::default(),
            scene_frame: None,
            staged_scene: None,
            paint_scratch: None,
            orphaned_nodes: Vec::new(),
            subtree_buffers: crate::storage::SubtreeBuffers::default(),
            removed_nodes: Vec::new(),
            removal_batch: super::tree_edit::RemovalBatch::default(),
            teardown: super::teardown::Teardown::default(),
            abandoned_nodes: Vec::new(),
            retained_pixel_ratio: flui_foundation::geometry::DevicePixelRatio::ONE,
            unrecorded_paint: false,
            last_hidden_follower_ids: FxHashSet::default(),
            device_pixel_ratio: flui_foundation::geometry::DevicePixelRatio::ONE,
            dirty_sender,
            dirty_rx,
            local_dirty,
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            #[cfg(any(test, feature = "testing"))]
            semantics_error_once_for_test: None,
            pending_child_requests: Vec::new(),
            pending_retain_bands: Vec::new(),
            counters: super::PipelineCounters::default(),
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

    // ========================================================================
    // Full-frame orchestrator
    // ========================================================================

    /// Runs a full frame on the owner in place: layout, compositing bits,
    /// paint and semantics, each through its [`FramePass`](super::FramePass).
    /// Returns whether the frame painted: buffered a patch in `writer`, which
    /// the caller applies and reports with
    /// [`scene_applied`](Self::scene_applied).
    ///
    /// The passes hold the order: each phase's `run_*` is a method of its own
    /// pass only. The owner never leaves `self` (ADR-0399), so a panic that
    /// unwinds out of a phase (a `tracing` subscriber, a visual-update
    /// callback) leaves it here with the work that phase kept queued, and the
    /// frame that retries finds it.
    ///
    /// # Errors
    ///
    /// The frame's first [`RenderError`](crate::error::RenderError), with the
    /// path to the node it names when it names one. A patch paint buffered
    /// before a semantics error stays staged: applying it and reporting it
    /// with `scene_applied` presents the paint, and a patch never reported
    /// makes the next paint repaint from the root.
    pub fn run_frame(
        &mut self,
        writer: &mut SceneWriter<'_>,
    ) -> Result<bool, flui_foundation::TreeError<crate::error::RenderError>> {
        // Observe node-bound cross-thread invalidation before any phase runs —
        // an async decode that finished while the app idled lands in this
        // frame, not never. The drain keeps every request it accepted.
        self.drain_pending_dirty();

        let mut layout = self.layout_pass();
        layout.run_layout().map_err(|error| layout.in_tree(error))?;

        let mut compositing = layout.into_compositing();
        compositing.run_compositing();

        let mut paint = compositing.into_paint();
        paint
            .run_paint(writer)
            .map_err(|error| paint.in_tree(error))?;

        // On an error the patch paint buffered stays in `writer` and staged:
        // the caller may still apply it and report it with `scene_applied`;
        // this failed call answers no frame.
        let mut semantics = paint.into_semantics();
        #[cfg(any(test, feature = "testing"))]
        let assembled = match semantics.take_semantics_error_for_test() {
            Some(error) => Err(error),
            None => semantics.run_semantics(),
        };
        #[cfg(not(any(test, feature = "testing")))]
        let assembled = semantics.run_semantics();
        assembled.map_err(|error| semantics.in_tree(error))?;

        let painted = self.staged_scene.is_some();
        if painted {
            self.counters.frames_produced += 1;
        }
        Ok(painted)
    }

    /// Drains the dirty channel and runs the layout phase alone: the layout
    /// pass of the build-during-layout fixpoint (ADR-0017). As with
    /// [`run_frame`](Self::run_frame), the owner stays in `self` on every
    /// exit, an unwind included.
    pub(crate) fn run_layout_alone(
        &mut self,
    ) -> Result<(), flui_foundation::TreeError<crate::error::RenderError>> {
        self.drain_pending_dirty();
        let mut layout = self.layout_pass();
        layout.run_layout().map_err(|error| layout.in_tree(error))
    }

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
