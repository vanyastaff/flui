//! Constructor, ctor helpers, and run_frame orchestration for `PipelineOwner<Idle>`.

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};

use flui_layer::SceneWriter;
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::storage::RenderTree;

use crate::pipeline::{
    PipelinePhase, TextContextHandle,
    handle::DirtySender,
    notifier::SharedNotifier,
    phase::{Idle, Layout},
    scheduler::DirtyTracker,
};

use super::{DEFAULT_DIRTY_CHANNEL_CAPACITY, PIPELINE_ID_COUNTER, PipelineOwner, rebind_phase};

/// The shared handles an owner is built over: its wake, its dirty channel,
/// its same-thread inbox, its relocation seal and its leader watches.
struct OwnerHandles {
    notifier: SharedNotifier,
    dirty_sender: DirtySender,
    dirty_rx: crossbeam_channel::Receiver<super::super::handle::DirtyRequest>,
    local_dirty: std::rc::Rc<super::super::local_invalidation::LocalDirtyInbox>,
    relocation_owner_seal: std::rc::Rc<super::relocation::RelocationOwnerSeal>,
    leader_watches: std::rc::Rc<super::LeaderWatches>,
}

impl PipelineOwner<Idle> {
    /// Creates a new pipeline owner in the [`Idle`] phase with the
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

    /// Creates a new pipeline owner in the [`Idle`] phase with a custom
    /// dirty-channel capacity, measuring text through `text` (see
    /// [`Self::new`]). Use this when the default 256 doesn't match the
    /// producer profile.
    pub fn new_with_capacity(dirty_channel_capacity: usize, text: TextContextHandle) -> Self {
        let notifier = SharedNotifier::default();
        let (dirty_sender, dirty_rx) =
            DirtySender::new_pair(dirty_channel_capacity, notifier.clone());
        let local_dirty = super::super::local_invalidation::LocalDirtyInbox::new(notifier.clone());
        Self::over(
            PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            text,
            OwnerHandles {
                notifier,
                dirty_sender,
                dirty_rx,
                local_dirty,
                relocation_owner_seal: std::rc::Rc::new(super::relocation::RelocationOwnerSeal),
                leader_watches: std::rc::Rc::default(),
            },
        )
    }

    /// An owner with no tree and no queued work, over the handles of `self`:
    /// what [`Self::in_place`] leaves in the slot while the owner is out.
    ///
    /// It shares `self`'s id, notifier, channel ends, inboxes and seal
    /// instead of building its own, so it allocates nothing and mints no
    /// pipeline id. Nothing reaches it: the slot is borrowed for as long as
    /// it sits there, and dropping it only releases the shared handles.
    fn vacant(&self) -> Self {
        Self::over(
            self.id,
            self.text.clone(),
            OwnerHandles {
                notifier: self.notifier.clone(),
                dirty_sender: self.dirty_sender.clone(),
                dirty_rx: self.dirty_rx.clone(),
                local_dirty: std::rc::Rc::clone(&self.local_dirty),
                relocation_owner_seal: std::rc::Rc::clone(&self.relocation_owner_seal),
                leader_watches: std::rc::Rc::clone(&self.leader_watches),
            },
        )
    }

    /// An empty owner over `handles`, numbered `id`, measuring through `text`.
    fn over(id: u64, text: TextContextHandle, handles: OwnerHandles) -> Self {
        let OwnerHandles {
            notifier,
            dirty_sender,
            dirty_rx,
            local_dirty,
            relocation_owner_seal,
            leader_watches,
        } = handles;
        let scheduler = DirtyTracker::new(notifier.clone());
        Self {
            // A context lent while a pipeline is built is a measurement
            // building one: seed zero, and the first drain after the loan
            // applies whatever changed, with nothing recorded to lay out.
            fonts_seen: text.fonts_generation().unwrap_or(0),
            text_measurers: crate::pipeline::TextMeasurers::default(),
            text_sizing: flui_painting::TextSizing::default(),
            text,
            id,
            relocation_owner_seal,
            render_tree: RenderTree::new(),
            transform_paths: std::cell::RefCell::new(Vec::new()),
            root_id: None,
            notifier,
            scheduler,
            layout_poison: super::poison::LayoutPoison::default(),
            root_constraints: None,
            semantics_enabled: AtomicBool::new(false),
            semantics_owner: None,
            semantics_regions: super::semantics::RegionIdentities::default(),
            semantics_diagnostics: std::collections::VecDeque::new(),
            assistive_focus: flui_semantics::AssistiveFocus::default(),
            #[cfg(any(test, feature = "testing"))]
            unchecked_semantics_diagnostics: Vec::new(),
            semantics_update_callback: None,
            semantics_route_change_sink: None,
            last_follower_transforms: FxHashMap::default(),
            last_child_paint_orders: FxHashMap::default(),
            leader_watches,
            boundary_records: FxHashMap::default(),
            follower_boundaries: FxHashSet::default(),
            scene_frame: None,
            staged_scene: None,
            paint_scratch: None,
            orphaned_nodes: Vec::new(),
            retained_pixel_ratio: 1.0,
            last_hidden_follower_ids: FxHashSet::default(),
            device_pixel_ratio: 1.0,
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
    /// Consumes `self`, returns the owner back at [`Idle`] plus whether the
    /// frame painted: buffered a patch in `writer`, which the caller applies
    /// and reports with [`scene_applied`](Self::scene_applied).
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
    /// the tuple is `Err(...)`. A patch paint buffered before a semantics
    /// error stays staged: applying it and reporting it with
    /// `scene_applied` presents the paint, and a patch never reported makes
    /// the next paint repaint from the root.
    ///
    /// # Panics
    ///
    /// A panic that unwinds out of a phase (a `tracing` subscriber, a
    /// visual-update callback) leaves that phase with its unfinished work
    /// still queued, and then resumes — taking this by-value owner with it.
    /// A caller that keeps the owner in a slot runs
    /// [`run_frame_in_place`](Self::run_frame_in_place) instead, which puts
    /// the owner back before the panic continues.
    #[must_use = "dropping the returned PipelineOwner<Idle> discards the pipeline handle; thread it back into the next frame"]
    pub fn run_frame(
        self,
        writer: &mut SceneWriter<'_>,
    ) -> (
        PipelineOwner<Idle>,
        Result<bool, flui_foundation::TreeError<crate::error::RenderError>>,
    ) {
        let (owner, ended) = self.run_frame_contained(writer);
        match ended {
            Ok(result) => (owner, result),
            Err(payload) => {
                // The owner goes with the unwind; its render objects are user
                // values destroyed after a caught failure (ADR-0281 §3).
                flui_foundation::panic::drop_contained(owner);
                std::panic::resume_unwind(payload)
            }
        }
    }

    /// Runs a full frame on the owner in place: [`run_frame`](Self::run_frame)
    /// for a caller that holds the owner behind `&mut` (a [`PipelineCell`],
    /// a test harness).
    ///
    /// The same owner is in `self` on every exit. A panic that unwinds out of
    /// a phase resumes only after the owner, its render tree and its queued
    /// work are back in place, so the frame that retries finds them.
    ///
    /// # Errors
    ///
    /// The frame's first [`RenderError`](crate::error::RenderError), with the
    /// path to the node it names when it names one.
    ///
    /// [`PipelineCell`]: crate::pipeline::PipelineCell
    pub fn run_frame_in_place(
        &mut self,
        writer: &mut SceneWriter<'_>,
    ) -> Result<bool, flui_foundation::TreeError<crate::error::RenderError>> {
        // Drained on `&mut self`, so a callback that unwinds here leaves the
        // owner where it is; the drain keeps every request it accepted.
        self.drain_pending_dirty();
        self.in_place(|owner| owner.run_frame_contained(writer))
    }

    /// Drains the dirty channel and runs the layout phase alone, on the owner
    /// in place: the layout pass of the build-during-layout fixpoint
    /// (ADR-0017). As with [`run_frame_in_place`](Self::run_frame_in_place),
    /// the owner is in `self` on every exit, an unwind included.
    pub(crate) fn run_layout_in_place(
        &mut self,
    ) -> Result<(), flui_foundation::TreeError<crate::error::RenderError>> {
        self.drain_pending_dirty();
        self.in_place(|owner| {
            let mut layout = owner.into_layout();
            let ended = contain_phase(&mut layout, PipelineOwner::run_layout);
            (layout.into_idle(), ended)
        })
    }

    /// Runs `frame` on this owner moved out by value, and puts the owner it
    /// returns back before anything else happens — before a caught panic
    /// resumes.
    ///
    /// The only place an owner leaves its slot. While `frame` runs the slot
    /// holds [`Self::vacant`], which shares the owner's handles; it is dropped
    /// once the owner is back, and runs no user code (an empty tree and
    /// handle clones whose release calls nothing).
    fn in_place<R>(&mut self, frame: impl FnOnce(Self) -> (Self, Result<R, Unwind>)) -> R {
        let placeholder = self.vacant();
        let (owner, ended) = frame(std::mem::replace(self, placeholder));
        drop(std::mem::replace(self, owner));
        ended.unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    }

    /// The frame's phases, each contained: a phase that returns an error or
    /// unwinds ends the frame, and the owner comes back at [`Idle`] either
    /// way.
    ///
    /// Total by construction: between the contained phase calls there are
    /// only typestate moves, `Option::take` and a counter, none of which can
    /// panic. Anything that can (user code, `tracing`) runs inside a
    /// contained phase.
    fn run_frame_contained(
        mut self,
        writer: &mut SceneWriter<'_>,
    ) -> (Self, Result<FrameAnswer, Unwind>) {
        // Observe node-bound cross-thread invalidation before any phase runs — an
        // async decode that finished while the app idled lands in this
        // frame, not never.
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the pending-dirty drain runs contained; the owner exits the frame"
        )]
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.drain_pending_dirty();
        })) {
            return (self, Err(payload));
        }

        // Layout
        let mut owner = self.into_layout();
        match contain_phase(&mut owner, PipelineOwner::run_layout) {
            Ok(Ok(())) => {}
            ended => return (owner.into_idle(), ended.map(|r| r.map(|()| false))),
        }

        // Compositing
        let mut owner = owner.into_compositing();
        match contain_phase(&mut owner, PipelineOwner::run_compositing) {
            Ok(Ok(())) => {}
            ended => return (owner.into_idle(), ended.map(|r| r.map(|()| false))),
        }

        // Paint
        let mut owner = owner.into_paint();
        match contain_phase(&mut owner, |owner| owner.run_paint(writer)) {
            Ok(Ok(())) => {}
            ended => return (owner.into_idle(), ended.map(|r| r.map(|()| false))),
        }

        // Semantics. The phase has no `into_idle`: the transition to <Idle>
        // goes via `finish`. On an error the patch paint buffered stays in
        // `writer` and staged: the caller may still apply it and report it
        // with `scene_applied`; this failed call answers no frame.
        let mut owner = owner.into_semantics();
        #[cfg(any(test, feature = "testing"))]
        let injected = owner.semantics_error_once_for_test.take();
        #[cfg(not(any(test, feature = "testing")))]
        let injected: Option<crate::error::RenderError> = None;
        let semantics = contain_phase(&mut owner, |owner| match injected {
            Some(error) => Err(error),
            None => owner.run_semantics(),
        });
        match semantics {
            Ok(Ok(())) => {}
            ended => return (owner.finish(), ended.map(|r| r.map(|()| false))),
        }

        let painted = owner.staged_scene.is_some();
        if painted {
            owner.counters.frames_produced += 1;
        }
        (owner.finish(), Ok(Ok(painted)))
    }
}

/// The payload of a panic a contained phase caught, resumed once the owner is
/// back in its slot.
type Unwind = Box<dyn std::any::Any + Send>;

/// What a frame answers when no phase unwinds.
type FrameAnswer = Result<bool, flui_foundation::TreeError<crate::error::RenderError>>;

/// Runs one phase under containment: its error is placed in the tree (the
/// path to the node it names, whose `debug_name` is user code, so inside the
/// containment too), and a panic that unwinds out of it is caught and handed
/// back for the caller to resume once the owner is safe.
///
/// Each phase leaves its own phase and keeps its unfinished work before the
/// unwind reaches here; this only keeps the owner.
fn contain_phase<P: PipelinePhase>(
    owner: &mut PipelineOwner<P>,
    run: impl FnOnce(&mut PipelineOwner<P>) -> crate::error::RenderResult<()>,
) -> Result<Result<(), flui_foundation::TreeError<crate::error::RenderError>>, Unwind> {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a frame phase runs contained so the owner is kept (ADR-0258)"
    )]
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(&mut *owner).map_err(|error| owner.in_tree(error))
    }))
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
