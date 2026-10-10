//! The UI runtime's owner-local frame state, and post-frame capabilities.
//!
//! [`OwnerFrame`] is what a frame needs from its owner thread that the
//! [`UpdateScheduler`] delegates to it: the post-frame queue and async task
//! store. A UI runtime (or a headless binding) owns
//! exactly one and is the only strong owner; every frame entry point takes it
//! by reference — [`UpdateScheduler::drive_frame`],
//! [`UpdateScheduler::handle_begin_frame`], [`UpdateScheduler::end_frame`],
//! [`UpdateScheduler::execute_frame`] — so no frame can poll or drain
//! "nothing".
//!
//! All post-frame callbacks share one `Rc` queue; eligible callbacks run in
//! registration order. Presentation-bound handles wait for that presentation's
//! coherent logical segment, independently of the host scheduler frame.
//! [`PostFrameHandle`] holds a weak pointer to that exact queue. Construction
//! may stage callbacks before the first owner frame; claiming the frame
//! transfers the strong queue reference to it. A stale handle cannot attach
//! callbacks to a replacement owner frame.
//!
//! There is no thread-local registry and no "currently active lane" concept:
//! a handle always addresses the one owner frame it was minted from, and the
//! frame drive drains an owner frame by receiving it as an explicit
//! parameter, never by reading ambient state.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use crate::async_driver::{RetirePanic, TaskStore};
use crate::{AsyncDriver, CallbackId, FrameTiming, UpdateScheduler, WeakUpdateScheduler};

pub(crate) type OwnerPostFrameCallback = Box<dyn FnOnce(&FrameTiming) + 'static>;

pub(crate) struct PostFrameEntry {
    pub(crate) id: CallbackId,
    pub(crate) callback: OwnerPostFrameCallback,
    target: CallbackTarget,
}

#[derive(Clone)]
enum CallbackTarget {
    RuntimeFrame,
    Presentation {
        scope: Weak<PresentationScopeState>,
        epoch: Rc<CompletionEpoch>,
    },
}

impl CallbackTarget {
    fn eligible(&self) -> bool {
        match self {
            Self::RuntimeFrame => true,
            Self::Presentation { scope, epoch } => scope.upgrade().is_some_and(|scope| {
                !scope.closed.get()
                    && epoch.completed.get()
                    && matches!(
                        scope.phase.get(),
                        SegmentPhase::Idle | SegmentPhase::Completing
                    )
            }),
        }
    }

    fn belongs_to(&self, scope: &Rc<PresentationScopeState>) -> bool {
        matches!(self, Self::Presentation { scope: target, .. } if target.ptr_eq(&Rc::downgrade(scope)))
    }
}

#[derive(Default)]
struct CompletionEpoch {
    completed: Cell<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SegmentPhase {
    Idle,
    Running,
    Suspended,
    Servicing,
    Completing,
}

struct PresentationScopeState {
    closed: Cell<bool>,
    phase: Cell<SegmentPhase>,
    current: RefCell<Rc<CompletionEpoch>>,
    next: RefCell<Rc<CompletionEpoch>>,
    next_demand: Cell<bool>,
}

pub(crate) struct PostFrameQueue {
    queue: RefCell<Vec<PostFrameEntry>>,
    active: RefCell<Vec<PostFrameEntry>>,
    /// Set by retirement: no callback is admitted afterwards.
    closed: Cell<bool>,
    scopes: RefCell<Vec<Weak<PresentationScopeState>>>,
}

impl PostFrameQueue {
    pub(crate) fn contains(&self, id: CallbackId) -> bool {
        self.queue
            .borrow()
            .iter()
            .chain(self.active.borrow().iter())
            .any(|entry| entry.id == id)
    }

    fn new() -> Rc<Self> {
        Rc::new(Self {
            queue: RefCell::new(Vec::new()),
            active: RefCell::new(Vec::new()),
            closed: Cell::new(false),
            scopes: RefCell::new(Vec::new()),
        })
    }
}

/// Construction may admit callbacks before the first owner frame exists.
/// Claiming transfers the only strong queue reference into that frame; later
/// owners receive fresh storage, so a stale weak handle cannot follow them.
pub(crate) enum PostFrameStorage {
    Unclaimed(Rc<PostFrameQueue>),
    Owned(Weak<PostFrameQueue>),
}

impl PostFrameStorage {
    pub(crate) fn new() -> Self {
        Self::Unclaimed(PostFrameQueue::new())
    }

    pub(crate) fn lane(&self) -> Option<Rc<PostFrameQueue>> {
        match self {
            Self::Unclaimed(lane) => Some(lane.clone()),
            Self::Owned(lane) => lane.upgrade(),
        }
    }

    pub(crate) fn claim(&mut self) -> Rc<PostFrameQueue> {
        let lane = self.lane().unwrap_or_else(PostFrameQueue::new);
        *self = Self::Owned(Rc::downgrade(&lane));
        lane
    }
}

/// The UI runtime's owner-local frame state: its post-frame queue and its async
/// tasks.
///
/// Runtime-internal: public only because the UI runtime and the headless bindings
/// live in sibling crates. `!Send + !Sync` through its `Rc` storage, and not
/// `Clone`: its owner — the UI runtime, or a headless binding — holds the only
/// strong reference to the tasks and callbacks, so they are created, run and
/// dropped on the owner thread and never outlive it. Widgets reach it through
/// `Weak` handles ([`AsyncDriver`], [`PostFrameHandle`]).
///
/// Dropping it [retires](Self::retire) whatever is still queued.
#[doc(hidden)]
pub struct OwnerFrame {
    scheduler: WeakUpdateScheduler,
    post_frame: Rc<PostFrameQueue>,
    tasks: Rc<TaskStore>,
}

impl OwnerFrame {
    /// Owner-local frame state for `scheduler`'s frames. Task wakes request a
    /// frame through `scheduler`'s [`FrameWaker`](crate::FrameWaker).
    ///
    /// A scheduler has at most one live owner frame. A frame drive polls only
    /// the owner it is handed, and its pump clears the scheduler's shared
    /// frame demand, so a task admitted to a second owner the host never
    /// drives would keep its ready flag set with no frame ever owed to it.
    /// The slot frees when the owner drops.
    ///
    /// # Errors
    ///
    /// [`OwnerFrameError::AlreadyOwned`] while another `OwnerFrame` for
    /// `scheduler` lives.
    pub fn new(scheduler: &UpdateScheduler) -> Result<Self, OwnerFrameError> {
        Self::with_tasks(scheduler, TaskStore::new())
    }

    /// As [`new`](Self::new), issuing task ids from `first`: a test reaches
    /// the identity-exhaustion boundary without spawning `u64::MAX` tasks.
    #[cfg(test)]
    pub(crate) fn with_first_task_id(
        scheduler: &UpdateScheduler,
        first: u64,
    ) -> Result<Self, OwnerFrameError> {
        Self::with_tasks(scheduler, TaskStore::with_first_id(first))
    }

    fn with_tasks(scheduler: &UpdateScheduler, tasks: TaskStore) -> Result<Self, OwnerFrameError> {
        if !scheduler.claim_owner_frame() {
            return Err(OwnerFrameError::AlreadyOwned);
        }
        let waker = scheduler.frame_waker();
        tasks.set_request_frame(std::sync::Arc::new(move || waker.request_frame()));
        Ok(Self {
            scheduler: scheduler.downgrade(),
            post_frame: scheduler.inner_post_frame_storage().borrow_mut().claim(),
            tasks: Rc::new(tasks),
        })
    }

    /// A widget's handle to this frame's tasks: `Weak`, so it keeps nothing
    /// alive past the owner.
    #[must_use]
    pub fn async_driver(&self) -> AsyncDriver {
        AsyncDriver::new(&self.tasks)
    }

    /// A `!Send` handle addressed directly at this frame's post-frame queue.
    #[must_use]
    pub fn post_frame_handle(&self) -> PostFrameHandle {
        PostFrameHandle {
            scheduler: self.scheduler.clone(),
            lane: Rc::downgrade(&self.post_frame),
            scope: None,
        }
    }

    /// Mint a completion authority for one presentation assembled by this owner.
    /// Retain it for that presentation's lifetime; its handles are weak.
    #[must_use]
    pub fn presentation_scope(&self) -> PresentationFrameScope {
        PresentationFrameScope::mint(self.scheduler.clone(), &self.post_frame)
    }

    /// Weak assembly authority for factories which create presentations later.
    /// A widget's scoped callback handle cannot mint sibling authorities.
    #[must_use]
    pub fn presentation_scope_factory(&self) -> PresentationScopeFactory {
        PresentationScopeFactory {
            scheduler: self.scheduler.clone(),
            lane: Rc::downgrade(&self.post_frame),
        }
    }

    /// Poll every task whose waker fired since the last poll; returns the
    /// number polled.
    ///
    /// The frame's mid-frame slot calls it from
    /// [`UpdateScheduler::handle_begin_frame`]; a wake that runs no frame
    /// calls it after [`UpdateScheduler::finish_async_pump`]. Never from
    /// build, layout or paint. Tasks are polled in ascending id order; a task
    /// that completes or is cancelled is removed; a task woken during this
    /// call is polled next time — the driver never spins. Cost scales with
    /// ready tasks, not resident ones.
    ///
    /// # Panics
    ///
    /// Propagates a task's poll panic after removing that task and keeping
    /// every unreached sibling indexed for the next poll.
    pub fn poll_ready(&self) -> usize {
        debug_assert!(
            self.scheduler.upgrade().is_none_or(
                |scheduler| scheduler.phase() != crate::SchedulerPhase::PersistentCallbacks
            ),
            "BUG: owner-local tasks must not be polled during build/layout/paint; the \
             poll belongs between the transient and persistent callbacks"
        );
        self.tasks.poll_ready()
    }

    /// Number of live tasks currently indexed as due a poll. A same-batch
    /// duplicate index entry counts twice until the next poll collapses it.
    #[must_use]
    pub fn ready_task_count(&self) -> usize {
        self.tasks.ready_task_count()
    }

    /// Drop every queued post-frame callback, then every task, on this (the
    /// owner) thread, each under its own catch, and admit nothing afterwards.
    ///
    /// A UI runtime's teardown calls it in its own order, before it resumes any
    /// earlier failure, so the captures of both queues are dropped on the
    /// owner exactly once. Returns the first panic a destructor raised; later
    /// ones are retained, never dropped. During an existing unwind the values
    /// are retained without running their destructors. Idempotent; `Drop`
    /// calls it for an owner that did not.
    #[must_use = "the first destructor panic is returned for the owner to raise"]
    pub fn retire(&self) -> Option<RetirePanic> {
        self.post_frame.closed.set(true);
        let mut callbacks = self.post_frame.queue.take();
        callbacks.extend(self.post_frame.active.take());
        // Active entries are removed with swap_remove; neither collection
        // alone retains registration order after a partial frame dispatch.
        callbacks.sort_unstable_by_key(|entry| entry.id);
        let tasks = self.tasks.detach_for_retirement();
        let mut first: Option<RetirePanic> = None;
        for entry in callbacks {
            if std::thread::panicking() {
                std::mem::forget(entry);
                continue;
            }
            if let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(entry))) {
                keep_first(&mut first, payload);
            }
        }
        if let Some(payload) = tasks.retire() {
            keep_first(&mut first, payload);
        }
        first
    }

    /// Whether this frame belongs to `scheduler`'s frames.
    pub(crate) fn belongs_to(&self, scheduler: &UpdateScheduler) -> bool {
        self.scheduler
            .upgrade()
            .is_some_and(|owner| owner.is_same_instance(scheduler))
    }

    /// Drain the post-frame queue for `scheduler`'s frame drive — but only if
    /// `scheduler` is genuinely the one this frame was made for.
    ///
    /// A queue drained by another scheduler would hand its callbacks a foreign
    /// frame's `FrameTiming`, remove them before their own frame ever runs
    /// them, and interleave `CallbackId`s from another id sequence into a sort
    /// where they carry no meaning. On mismatch (or a since-dropped owning
    /// scheduler) this returns `Err` and leaves the queue untouched.
    pub(crate) fn take_post_frame_queue_for(
        &self,
        scheduler: &UpdateScheduler,
    ) -> Result<Vec<CallbackId>, PostFrameScheduleError> {
        let Some(owner) = self.scheduler.upgrade() else {
            tracing::error!(
                driving_scheduler = scheduler.debug_ptr(),
                "an OwnerFrame's scheduler is already gone; refusing to drain it from this \
                 (necessarily unrelated) frame drive"
            );
            return Err(PostFrameScheduleError::Closed);
        };
        if !owner.is_same_instance(scheduler) {
            tracing::error!(
                owner_frame_scheduler = owner.debug_ptr(),
                driving_scheduler = scheduler.debug_ptr(),
                "an OwnerFrame was handed to a frame drive on a scheduler it does not belong \
                 to; refusing to drain or poll it — its own scheduler's next drive still does"
            );
            return Err(PostFrameScheduleError::WrongScheduler);
        }
        let (eligible, blocked): (Vec<_>, Vec<_>) = self
            .post_frame
            .queue
            .take()
            .into_iter()
            .partition(|entry| entry.target.eligible());
        let ids = eligible.iter().map(|entry| entry.id).collect();
        self.post_frame.queue.borrow_mut().extend(blocked);
        self.post_frame.active.borrow_mut().extend(eligible);
        Ok(ids)
    }

    pub(crate) fn finish_post_frame_batch(&self) {
        self.post_frame.scopes.borrow_mut().retain(|scope| {
            let Some(scope) = scope.upgrade() else {
                return false;
            };
            if scope.phase.get() == SegmentPhase::Completing {
                scope.phase.set(SegmentPhase::Idle);
            }
            true
        });
    }

    /// Return an uninvoked tail to the already-validated queue. Original IDs
    /// retain FIFO order ahead of newer registrations on the next drain.
    pub(crate) fn restore_post_frame_queue(&self) {
        let entries = self.post_frame.active.take();
        self.post_frame.queue.borrow_mut().extend(entries);
    }

    pub(crate) fn take_active_post_frame(&self, id: CallbackId) -> Option<PostFrameEntry> {
        let mut active = self.post_frame.active.borrow_mut();
        let index = active.iter().position(|entry| entry.id == id)?;
        if !active[index].target.eligible() {
            return None;
        }
        Some(active.swap_remove(index))
    }
}

fn keep_first(first: &mut Option<RetirePanic>, payload: RetirePanic) {
    if first.is_none() {
        *first = Some(payload);
    } else {
        flui_foundation::panic::retain_opaque_payload(payload);
    }
}

impl Drop for OwnerFrame {
    fn drop(&mut self) {
        let first = self.retire();
        // Freed only after retirement: a destructor retirement runs cannot
        // mint a second owner while this one still holds tasks.
        if let Some(scheduler) = self.scheduler.upgrade() {
            scheduler.release_owner_frame();
        }
        if let Some(payload) = first {
            if std::thread::panicking() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

impl std::fmt::Debug for OwnerFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerFrame")
            .field(
                "post_frame_callbacks",
                &self
                    .post_frame
                    .queue
                    .try_borrow()
                    .map(|queue| queue.len())
                    .ok(),
            )
            .field("tasks", &self.tasks)
            .finish_non_exhaustive()
    }
}

/// Why an [`OwnerFrame`] could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OwnerFrameError {
    /// Another `OwnerFrame` for this scheduler is still alive. Frames poll
    /// only the owner they are handed, so a second owner's tasks would be
    /// stranded; drop the first owner before making another.
    #[error("the UpdateScheduler already has a live OwnerFrame")]
    AlreadyOwned,
}

/// Why an owner-local post-frame callback could not be registered, or an
/// owner frame's queue could not be drained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PostFrameScheduleError {
    /// The owning UI runtime (its owner frame or its scheduler) is gone or retired
    /// — there is no frame left for the callback to observe, and it is
    /// guaranteed never to run.
    #[error("the handle's owner queue is closed")]
    Closed,
    /// An owner frame was handed to a frame drive on a different
    /// `UpdateScheduler` than the one it was made for. Draining it anyway
    /// would hand its callbacks a foreign frame's `FrameTiming`, so the drive
    /// refuses to drain it at all; its own scheduler's next drive still
    /// delivers it.
    #[error("the owner queue belongs to a different UpdateScheduler than the one draining it")]
    WrongScheduler,
}

/// Owner-local completion authority for one presentation's logical geometry
/// segment. Host frames may finish while this segment remains suspended.
///
/// This is a runtime assembly capability, not a widget capability. Widgets get
/// its weak [`PostFrameHandle`]. Cancellation retains completion debt: resume
/// the same authority for a replacement segment, or close the presentation.
#[doc(hidden)]
pub struct PresentationFrameScope {
    scheduler: WeakUpdateScheduler,
    lane: Weak<PostFrameQueue>,
    state: Rc<PresentationScopeState>,
}

/// Weak owner-local authority to assemble a presentation completion scope.
/// This retains neither the scheduler nor its callback queue.
#[doc(hidden)]
#[derive(Clone)]
pub struct PresentationScopeFactory {
    scheduler: WeakUpdateScheduler,
    lane: Weak<PostFrameQueue>,
}

impl PresentationScopeFactory {
    /// Mint a fresh presentation authority while the original owner is live.
    pub fn create(&self) -> Result<PresentationFrameScope, PresentationFrameError> {
        let lane = self
            .lane
            .upgrade()
            .filter(|lane| !lane.closed.get())
            .ok_or(PresentationFrameError::Closed)?;
        if self.scheduler.upgrade().is_none() {
            return Err(PresentationFrameError::Closed);
        }
        Ok(PresentationFrameScope::mint(self.scheduler.clone(), &lane))
    }
}

impl std::fmt::Debug for PresentationScopeFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationScopeFactory")
            .field("owner_alive", &(self.lane.strong_count() > 0))
            .finish_non_exhaustive()
    }
}

impl PresentationFrameScope {
    fn mint(scheduler: WeakUpdateScheduler, lane: &Rc<PostFrameQueue>) -> Self {
        let state = Rc::new(PresentationScopeState {
            closed: Cell::new(lane.closed.get()),
            phase: Cell::new(SegmentPhase::Idle),
            current: RefCell::new(Rc::default()),
            next: RefCell::new(Rc::default()),
            next_demand: Cell::new(false),
        });
        lane.scopes.borrow_mut().push(Rc::downgrade(&state));
        Self {
            scheduler,
            lane: Rc::downgrade(lane),
            state,
        }
    }

    /// The weak callback handle for this exact presentation authority.
    #[must_use]
    pub fn post_frame_handle(&self) -> PostFrameHandle {
        PostFrameHandle {
            scheduler: self.scheduler.clone(),
            lane: self.lane.clone(),
            scope: Some(Rc::downgrade(&self.state)),
        }
    }

    fn check_live(&self) -> Result<(), PresentationFrameError> {
        if self.state.closed.get()
            || self.lane.upgrade().is_none_or(|lane| lane.closed.get())
            || self.scheduler.upgrade().is_none()
        {
            return Err(PresentationFrameError::Closed);
        }
        Ok(())
    }

    /// Begin a new segment. Registrations made while idle already address it.
    /// A suspended segment must be resumed instead, preserving its debt.
    pub fn enter_segment(&self) -> Result<(), PresentationFrameError> {
        self.check_live()?;
        if self.state.phase.get() != SegmentPhase::Idle {
            return Err(PresentationFrameError::SegmentActive);
        }
        *self.state.current.borrow_mut() = self.state.next.replace(Rc::default());
        self.state.next_demand.set(false);
        self.state.phase.set(SegmentPhase::Running);
        Ok(())
    }

    /// Accepted successor callbacks need a coherent completion even when the
    /// presentation has no dirty tree work. Runtime admission must also check
    /// host eligibility and existing coherent geometry before entering it.
    #[must_use]
    pub fn has_completion_demand(&self) -> bool {
        self.check_live().is_ok() && self.state.next_demand.get()
    }

    /// Reissue retained successor demand after the host clears its wake latch.
    /// The runtime calls this only for an eligible presentation; hidden or
    /// suspended geometry retains its debt without forcing an idle busy loop.
    pub fn rearm_completion_demand(&self) {
        if self.has_completion_demand()
            && let Some(scheduler) = self.scheduler.upgrade()
        {
            scheduler.request_frame();
        }
    }

    /// Retain the current completion epoch across a host-frame return.
    pub fn suspend_segment(&self) -> Result<(), PresentationFrameError> {
        self.check_live()?;
        if !matches!(
            self.state.phase.get(),
            SegmentPhase::Running | SegmentPhase::Servicing
        ) {
            return Err(PresentationFrameError::NoRunningSegment);
        }
        self.state.phase.set(SegmentPhase::Suspended);
        Ok(())
    }

    /// Before detached native service, prevent reentrant registrations from
    /// joining the retained segment. They belong to its successor.
    pub fn service_segment(&self) -> Result<(), PresentationFrameError> {
        self.check_live()?;
        if !matches!(
            self.state.phase.get(),
            SegmentPhase::Running | SegmentPhase::Suspended
        ) {
            return Err(PresentationFrameError::NoRunningSegment);
        }
        self.state.phase.set(SegmentPhase::Servicing);
        Ok(())
    }

    /// Resume the retained epoch, including replacement geometry after
    /// cancellation. No callback IDs or completion promises are discarded.
    pub fn resume_segment(&self) -> Result<(), PresentationFrameError> {
        self.check_live()?;
        if !matches!(
            self.state.phase.get(),
            SegmentPhase::Suspended | SegmentPhase::Servicing
        ) {
            return Err(PresentationFrameError::NoSuspendedSegment);
        }
        self.state.phase.set(SegmentPhase::Running);
        Ok(())
    }

    /// Commit coherent geometry before the host's post-frame dispatch.
    /// The eligible batch receives that host's `FrameTiming`.
    pub fn complete_segment(&self) -> Result<(), PresentationFrameError> {
        self.check_live()?;
        if self.state.phase.get() != SegmentPhase::Running {
            return Err(PresentationFrameError::NoRunningSegment);
        }
        self.state.current.borrow().completed.set(true);
        self.state.phase.set(SegmentPhase::Completing);
        Ok(())
    }

    /// Withdraw partial geometry while transferring its callback debt to a
    /// replacement. Resume after installing the replacement's owned inputs.
    pub fn cancel_segment(&self) -> Result<(), PresentationFrameError> {
        self.check_live()?;
        if !matches!(
            self.state.phase.get(),
            SegmentPhase::Running | SegmentPhase::Suspended | SegmentPhase::Servicing
        ) {
            return Err(PresentationFrameError::NoRunningSegment);
        }
        self.state.phase.set(SegmentPhase::Suspended);
        Ok(())
    }

    /// Permanently revoke this presentation and retire its queued and active
    /// callback captures outside all queue guards. Return the first retirement
    /// failure for the runtime's existing recovery scope to preserve.
    #[must_use = "preserve the first retirement failure in the owner's recovery scope"]
    pub fn close(&self) -> Option<RetirePanic> {
        let retired = self.detach_callbacks();
        let mut first = None;
        for entry in retired {
            if std::thread::panicking() {
                std::mem::forget(entry);
            } else if let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(entry))) {
                keep_first(&mut first, payload);
            }
        }
        first
    }

    /// Revoke and retain only this scope's callbacks when the enclosing owner
    /// already preserves a failure. No capture destructor or diagnostic runs;
    /// healthy sibling queues remain owned and deliverable.
    #[doc(hidden)]
    pub fn close_preserving(&self) {
        for entry in self.detach_callbacks() {
            std::mem::forget(entry.callback);
        }
    }

    fn detach_callbacks(&self) -> Vec<PostFrameEntry> {
        self.withdraw();
        let Some(lane) = self.lane.upgrade() else {
            return Vec::new();
        };
        let mut retired = Vec::new();
        for storage in [&lane.queue, &lane.active] {
            let (outgoing, retained): (Vec<_>, Vec<_>) = storage
                .take()
                .into_iter()
                .partition(|entry| entry.target.belongs_to(&self.state));
            storage.borrow_mut().extend(retained);
            retired.extend(outgoing);
        }
        retired.sort_unstable_by_key(|entry| entry.id);
        retired
    }

    /// Revoke completion authority without destroying any callback capture.
    /// Runtime teardown commits this before closing graph, focus, text and
    /// gesture authority, then calls [`Self::close`] outside those guards.
    /// Withdrawal never consumes the later retirement obligation.
    #[doc(hidden)]
    pub fn withdraw(&self) {
        self.state.closed.set(true);
        self.state.next_demand.set(false);
    }
}

impl Drop for PresentationFrameScope {
    fn drop(&mut self) {
        if let Some(payload) = self.close() {
            if std::thread::panicking() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

impl std::fmt::Debug for PresentationFrameScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationFrameScope")
            .field("closed", &self.state.closed.get())
            .finish_non_exhaustive()
    }
}

/// An invalid owner operation on a presentation's logical segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PresentationFrameError {
    /// The presentation, owner frame, or scheduler has retired.
    #[error("the presentation completion authority is closed")]
    Closed,
    /// A retained epoch cannot be silently replaced by a new epoch.
    #[error("a logical segment is already active; resume it instead")]
    SegmentActive,
    /// The requested transition requires an active logical segment.
    #[error("there is no running logical segment")]
    NoRunningSegment,
    /// Resume requires a retained suspended or servicing segment.
    #[error("there is no suspended logical segment")]
    NoSuspendedSegment,
}

/// Schedules owner-local work after a completed frame's layout and paint.
/// A presentation-bound handle waits for its own logical geometry completion;
/// an unscoped handle retains host scheduler-frame semantics.
///
/// `!Send`: holds a [`Weak`] pointer directly at its owner frame's `Rc`
/// queue, so it can capture non-`Send` state (`Rc`/`RefCell`) in the
/// callbacks it schedules. Moving one to another thread is a compile error,
/// not a runtime check.
#[derive(Clone)]
pub struct PostFrameHandle {
    scheduler: WeakUpdateScheduler,
    lane: Weak<PostFrameQueue>,
    scope: Option<Weak<PresentationScopeState>>,
}

impl PostFrameHandle {
    /// Address the queue of this scheduler's current owner or construction.
    #[must_use]
    pub fn new(scheduler: &UpdateScheduler) -> Self {
        let lane = scheduler.inner_post_frame_storage().borrow().lane();
        Self {
            scheduler: scheduler.downgrade(),
            lane: lane.as_ref().map_or_else(Weak::new, Rc::downgrade),
            scope: None,
        }
    }
    /// Schedule an owner-local callback after the next eligible completion.
    /// A presentation registration made during its running segment joins that
    /// segment. Idle, suspended, servicing and completing registrations target
    /// the successor segment. Self-registration therefore never runs twice in
    /// one completion batch.
    ///
    /// The callback may capture `Rc`/`RefCell` state. On error (the owning
    /// UI runtime is gone or retired) the callback is dropped without running —
    /// provably: nothing retains it once this call returns `Err`.
    ///
    /// Eligible registrations run in callback identity order. A blocked
    /// presentation never holds back a completed sibling or runtime callback.
    pub fn schedule(
        &self,
        callback: impl FnOnce(&FrameTiming) + 'static,
    ) -> Result<(), PostFrameScheduleError> {
        let lane = self
            .lane
            .upgrade()
            .filter(|lane| !lane.closed.get())
            .ok_or(PostFrameScheduleError::Closed)?;
        let scheduler = self
            .scheduler
            .upgrade()
            .ok_or(PostFrameScheduleError::Closed)?;
        let mut successor_scope = None;
        let target = if let Some(scope) = &self.scope {
            let scope = scope
                .upgrade()
                .filter(|scope| !scope.closed.get())
                .ok_or(PostFrameScheduleError::Closed)?;
            let epoch = if scope.phase.get() == SegmentPhase::Running {
                scope.current.borrow().clone()
            } else {
                successor_scope = Some(scope.clone());
                scope.next.borrow().clone()
            };
            CallbackTarget::Presentation {
                scope: Rc::downgrade(&scope),
                epoch,
            }
        } else {
            CallbackTarget::RuntimeFrame
        };
        scheduler.with_post_frame_registration(|id| {
            lane.queue.borrow_mut().push(PostFrameEntry {
                id,
                callback: Box::new(callback),
                target,
            });
            if let Some(scope) = &successor_scope {
                scope.next_demand.set(true);
            }
        });
        if successor_scope.is_some() {
            scheduler.request_frame();
        }
        Ok(())
    }

    /// The number of owner-local callbacks queued on this handle's owner frame
    /// and not yet drained by a completed frame; `0` once it is closed.
    ///
    /// A self-rescheduling callback (one that queues its successor from
    /// inside its own run) keeps this at `1` after every frame, so a reading
    /// of `0` after a frame is how an owner proves such a loop has stopped.
    ///
    /// A test probe, not part of the documented API: no production path reads
    /// it. It exists so an owner crate's integration tests can observe the
    /// queue without a test-only global counter.
    #[doc(hidden)]
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.lane
            .upgrade()
            .map_or(0, |lane| lane.queue.borrow().len())
    }

    /// Whether this handle targets `other`.
    #[must_use]
    pub fn targets_same_scheduler(&self, other: &UpdateScheduler) -> bool {
        self.scheduler
            .upgrade()
            .is_some_and(|scheduler| scheduler.is_same_instance(other))
    }
}

impl std::fmt::Debug for PostFrameHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostFrameHandle")
            .field("lane_alive", &(self.lane.strong_count() > 0))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use static_assertions::assert_not_impl_any;

    use super::*;
    use crate::SchedulerPhase;

    assert_not_impl_any!(UpdateScheduler: Send, Sync);
    assert_not_impl_any!(OwnerFrame: Send, Sync);
    assert_not_impl_any!(PostFrameHandle: Send, Sync);

    fn post_frame_panic_restores_idle_and_later_scheduling_works() {
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
        owner
            .post_frame_handle()
            .schedule(|_| panic!("post-frame probe"))
            .expect("lane alive");
        assert!(catch_unwind(AssertUnwindSafe(|| scheduler.execute_frame(&owner))).is_err());
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
        let fired = Rc::new(Cell::new(false));
        let callback = Rc::clone(&fired);
        owner
            .post_frame_handle()
            .schedule(move |_| callback.set(true))
            .expect("gate remains usable");
        scheduler.execute_frame(&owner);
        assert!(fired.get());
    }

    fn post_frame_panic_preserves_uninvoked_mixed_tail_before_reentrant_work() {
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
        let log = Arc::new(Mutex::new(Vec::new()));
        let panic_calls = Rc::new(Cell::new(0));
        let counted = panic_calls.clone();
        let reentrant = owner.post_frame_handle();
        let reentrant_log = log.clone();
        owner
            .post_frame_handle()
            .schedule(move |_| {
                counted.set(counted.get() + 1);
                reentrant
                    .schedule(move |_| {
                        reentrant_log.lock().expect("log").push(3);
                    })
                    .expect("lane alive");
                panic!("first callback poisons this frame, not the uninvoked queue");
            })
            .expect("lane alive");
        let shared_log = log.clone();
        scheduler.add_post_frame_callback(Box::new(move |_| {
            shared_log.lock().expect("log").push(1);
        }));
        let local_log = log.clone();
        owner
            .post_frame_handle()
            .schedule(move |_| {
                local_log.lock().expect("log").push(2);
            })
            .expect("lane alive");

        let (panicked, _) = flui_testing::log_capture::capture(|| {
            catch_unwind(AssertUnwindSafe(|| scheduler.execute_frame(&owner)))
        });
        assert!(panicked.is_err());
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
        assert!(
            log.lock().expect("log").is_empty(),
            "the poisoned frame stops delivery"
        );
        let (_retry_frame, _) =
            flui_testing::log_capture::capture(|| scheduler.execute_frame(&owner));
        assert_eq!(
            *log.lock().expect("log"),
            [1, 2, 3],
            "uninvoked callbacks retain original registration order across both lanes"
        );
        assert_eq!(
            panic_calls.get(),
            1,
            "the callback that already ran is not retried"
        );
    }

    /// Retirement drops queued owner-local callbacks once, unrun, keeps the
    /// first destructor panic, and closes the queue to later registrations.
    fn retirement_drops_queued_callbacks_and_closes_the_queue() {
        struct Probe {
            drops: Rc<Cell<usize>>,
            panics: bool,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
                assert!(!self.panics, "callback capture probe");
            }
        }

        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
        let handle = owner.post_frame_handle();
        let drops = Rc::new(Cell::new(0));
        let ran = Rc::new(Cell::new(false));
        for panics in [true, false] {
            let probe = Probe {
                drops: Rc::clone(&drops),
                panics,
            };
            let ran = Rc::clone(&ran);
            handle
                .schedule(move |_| {
                    let _probe = probe;
                    ran.set(true);
                })
                .expect("lane alive");
        }
        let first = owner.retire().expect("a capture's destructor panicked");
        assert_eq!(
            first.downcast_ref::<&str>().copied(),
            Some("callback capture probe")
        );
        assert_eq!(drops.get(), 2, "both captures dropped once");
        assert!(!ran.get(), "a retired callback never runs");
        assert_eq!(handle.schedule(|_| {}), Err(PostFrameScheduleError::Closed));
        scheduler.execute_frame(&owner);
        assert!(!ran.get());
    }

    #[test]
    fn post_frame_panic_matrix() {
        crate::table_test::run_table(
            "post_frame_panic_matrix",
            &[
                (
                    "post_frame_panic_restores_idle_and_later_scheduling_works",
                    post_frame_panic_restores_idle_and_later_scheduling_works as fn(),
                ),
                (
                    "post_frame_panic_preserves_uninvoked_mixed_tail_before_reentrant_work",
                    post_frame_panic_preserves_uninvoked_mixed_tail_before_reentrant_work as fn(),
                ),
                (
                    "retirement_drops_queued_callbacks_and_closes_the_queue",
                    retirement_drops_queued_callbacks_and_closes_the_queue as fn(),
                ),
            ],
        );
    }
}
