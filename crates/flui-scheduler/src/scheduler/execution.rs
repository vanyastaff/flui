//! Complete owner turns, including admission and outgoing ownership custody.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::{IdleDeadline, UpdateScheduler, request_frame_impl_preserving_failure};
use crate::async_driver::RetirePanic;
use crate::{Instant, OwnerFrame, SchedulerPhase};

/// Why an owner refused execution before changing frame or wake state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ExecutionError {
    /// Another frame or background turn still owns execution.
    #[error("the owner is already executing a turn")]
    AlreadyExecuting,
    /// This owner's retirement is permanent, even if its scheduler survives.
    #[error("the execution owner has retired")]
    Retired,
    /// The owner's weak scheduler authority can no longer be used.
    #[error("the execution owner's scheduler is closed")]
    SchedulerClosed,
}

#[derive(Debug, Default)]
pub(crate) struct ExecutionState {
    active: Cell<bool>,
    retired: Cell<bool>,
    failed: Rc<Cell<bool>>,
}

impl ExecutionState {
    fn enter(&self) -> Result<ExecutionPermit<'_>, ExecutionError> {
        if self.retired.get() {
            return Err(ExecutionError::Retired);
        }
        if self.active.get() {
            return Err(ExecutionError::AlreadyExecuting);
        }
        self.failed.set(false);
        self.active.set(true);
        Ok(ExecutionPermit { state: self })
    }

    pub(crate) fn retire(&self) {
        self.retired.set(true);
    }
}

/// Only framework bookkeeping belongs in Drop; outgoing user ownership is
/// explicitly retired before this permit leaves scope.
struct ExecutionPermit<'a> {
    state: &'a ExecutionState,
}

impl Drop for ExecutionPermit<'_> {
    fn drop(&mut self) {
        self.state.active.set(false);
        self.state.failed.set(false);
    }
}

struct Recovery<'a> {
    owner: &'a OwnerFrame,
    first: Option<RetirePanic>,
}

impl<'a> Recovery<'a> {
    fn new(owner: &'a OwnerFrame) -> Self {
        Self { owner, first: None }
    }

    fn keep(&mut self, payload: RetirePanic) {
        self.owner.record_execution_failure();
        if self.first.is_none() {
            self.first = Some(payload);
        } else {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }

    fn attempt<T>(&mut self, action: impl FnOnce() -> T) -> Option<T> {
        match catch_unwind(AssertUnwindSafe(action)) {
            Ok(value) => Some(value),
            Err(payload) => {
                self.keep(payload);
                None
            }
        }
    }

    fn retire<T>(&mut self, value: T) {
        if self.owner.preserving_execution_failure() || std::thread::panicking() {
            std::mem::forget(value);
        } else {
            self.attempt(|| drop(value));
        }
    }

    fn release_scheduler(&mut self, scheduler: UpdateScheduler) {
        let preserve_failure = self.owner.preserving_execution_failure();
        let failure_signal = Rc::clone(&self.owner.execution.failed);
        if let Some(Some(payload)) =
            self.attempt(|| scheduler.release_execution(preserve_failure, failure_signal))
        {
            self.keep(payload);
        }
        if self.owner.scheduler.inner.strong_count() == 0 {
            // Actual terminal release precedes owner capture retirement, so a
            // destructor cannot revive scheduler authority through a weak handle.
            let owner = self.owner;
            if let Some(Some(payload)) = self.attempt(|| owner.retire()) {
                self.keep(payload);
            }
        }
    }

    fn finish(mut self) {
        if let Some(payload) = self.first.take() {
            resume_unwind(payload);
        }
    }
}

impl OwnerFrame {
    pub(crate) fn record_execution_failure(&self) {
        self.execution.failed.set(true);
    }

    pub(crate) fn preserving_execution_failure(&self) -> bool {
        self.execution.failed.get()
    }

    pub(crate) fn execution_failure_signal(&self) -> &Cell<bool> {
        &self.execution.failed
    }

    pub(crate) fn release_scheduler_after_retirement(&self) -> Option<RetirePanic> {
        let scheduler = self.scheduler.upgrade()?;
        scheduler.release_owner_frame();
        let mut recovery = Recovery::new(self);
        recovery.release_scheduler(scheduler);
        recovery.first.take()
    }

    fn retire_refused<T>(&self, envelope: T) {
        if self.preserving_execution_failure() || std::thread::panicking() {
            std::mem::forget(envelope);
        } else if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(envelope))) {
            self.record_execution_failure();
            resume_unwind(payload);
        }
    }

    fn retire_refused_frame<P, F>(&self, prepare: P, pipeline: F) {
        let mut recovery = Recovery::new(self);
        recovery.retire(prepare);
        recovery.retire(pipeline);
        recovery.finish();
    }

    /// Execute one complete frame on this owner's scheduler.
    ///
    /// Admission precedes mutation and diagnostics and remains held through
    /// completion, callable retirement and temporary scheduler release. The
    /// preparation runs once before opening a frame, then the pipeline runs
    /// once. Both callables are borrowed from separate owned envelopes. Owner
    /// retirement
    /// during its invocation closes future owner work without preempting the
    /// admitted frame's scheduler bookkeeping.
    /// Preparation failure leaves accepted work and existing frame demand
    /// intact without opening a frame or producing completion. The caller
    /// decides when to retry that frame; this entry does not issue a new wake.
    ///
    /// # Errors
    /// Refuses recursive, retired or closed execution before invoking the
    /// preparation or pipeline. Rejected envelopes retire separately, or are
    /// retained while preserving an incoming failure.
    ///
    /// # Panics
    /// Propagates the first invocation or cleanup failure after closing an
    /// opened frame. A produced result is retained if later cleanup fails.
    /// An opaque aggregate whose own destructor double-panics remains subject
    /// to Rust's abort semantics. A rejected envelope's destructor can likewise
    /// panic instead of returning a typed refusal.
    pub fn drive_frame<R>(
        &self,
        timestamp: Instant,
        deadline: IdleDeadline,
        mut prepare: impl FnMut(),
        mut pipeline: impl FnMut() -> R,
    ) -> Result<R, ExecutionError> {
        let permit = match self.execution.enter() {
            Ok(permit) => permit,
            Err(error) => {
                self.retire_refused_frame(prepare, pipeline);
                return Err(error);
            }
        };
        let Some(scheduler) = self.scheduler.upgrade() else {
            self.retire_refused_frame(prepare, pipeline);
            return Err(ExecutionError::SchedulerClosed);
        };
        if scheduler.inner.wake.closed.load(Ordering::Acquire) {
            let mut recovery = Recovery::new(self);
            recovery.retire(prepare);
            recovery.retire(pipeline);
            recovery.release_scheduler(scheduler);
            recovery.finish();
            return Err(ExecutionError::SchedulerClosed);
        }

        let mut recovery = Recovery::new(self);
        let mut output = None;
        recovery.attempt(&mut prepare);
        let span = if recovery.first.is_none() {
            recovery.attempt(|| tracing::debug_span!("frame", id = tracing::field::Empty))
        } else {
            None
        };
        if let Some(span) = span.as_ref() {
            let entered = recovery.attempt(|| span.enter());
            if recovery.first.is_none() {
                recovery.attempt(|| {
                    *scheduler.inner.frame.idle_deadline.borrow_mut() = Some(deadline.0);
                    let id = scheduler.handle_begin_frame(timestamp, self);
                    span.record("id", tracing::field::debug(id));
                    scheduler.handle_draw_frame();
                    *scheduler.inner.frame.idle_deadline.borrow_mut() = None;
                    output = Some(pipeline());
                });
            }
            *scheduler.inner.frame.idle_deadline.borrow_mut() = None;
            if recovery.first.is_none() {
                recovery.attempt(|| scheduler.end_frame(self));
            }
            if recovery.first.is_some()
                && (scheduler.inner.frame.current_frame.borrow().is_some()
                    || scheduler.phase() != SchedulerPhase::Idle)
            {
                recovery.attempt(|| scheduler.abort_frame_impl(true));
            }
            // Begin can fail before installing a FrameTiming (for example on
            // identity exhaustion). Such an attempt owes no completion but
            // must not leave its sampled timestamp published.
            *scheduler.inner.frame.current_vsync_time.borrow_mut() = None;
            recovery.retire(prepare);
            recovery.retire(pipeline);
            // A subscriber can run arbitrary code from exit/close. Invoke
            // those cleanups explicitly while output ownership stays outside.
            if let Some(entered) = entered {
                recovery.attempt(|| drop(entered));
            }
        } else {
            recovery.retire(prepare);
            recovery.retire(pipeline);
        }
        if let Some(span) = span {
            recovery.attempt(|| drop(span));
        }
        recovery.release_scheduler(scheduler);
        if recovery.first.is_some() {
            if let Some(value) = output.take() {
                std::mem::forget(value);
            }
        }
        recovery.finish();
        drop(permit);
        Ok(output.expect("BUG: a successful frame invoked its pipeline"))
    }

    /// Consume the old wake, prepare runtime policy, then poll ready tasks once.
    ///
    /// Runs without visual frame phases, even while frames are disabled.
    /// Preparation failure skips polling, preserves indexed ready tasks and
    /// retries their delivery through the bounded wake mechanism. Self-wakes
    /// belong to a subsequent poll batch.
    ///
    /// # Errors
    /// Refuses recursive, retired or closed execution before consuming demand
    /// or invoking preparation. Rejected owned preparation follows the same
    /// retirement policy as [`Self::drive_frame`].
    ///
    /// # Panics
    /// Propagates the first preparation, poll or cleanup failure. Opaque
    /// callable ownership is retained once failure custody begins.
    pub fn pump_background(&self, mut prepare: impl FnMut()) -> Result<usize, ExecutionError> {
        let permit = match self.execution.enter() {
            Ok(permit) => permit,
            Err(error) => {
                self.retire_refused(prepare);
                return Err(error);
            }
        };
        let Some(scheduler) = self.scheduler.upgrade() else {
            self.retire_refused(prepare);
            return Err(ExecutionError::SchedulerClosed);
        };
        if scheduler.inner.wake.closed.load(Ordering::Acquire) {
            let mut recovery = Recovery::new(self);
            recovery.retire(prepare);
            recovery.release_scheduler(scheduler);
            recovery.finish();
            return Err(ExecutionError::SchedulerClosed);
        }
        let mut recovery = Recovery::new(self);
        let mut polled = 0;
        recovery.attempt(|| {
            scheduler.finish_async_pump();
            prepare();
            polled = self.poll_ready();
        });
        recovery.retire(prepare);
        if recovery.first.is_some()
            && (self.ready_task_count() != 0 || scheduler.has_scheduled_frame())
        {
            recovery.attempt(|| {
                request_frame_impl_preserving_failure(&scheduler.inner.wake, true);
            });
        }
        recovery.release_scheduler(scheduler);
        recovery.finish();
        drop(permit);
        Ok(polled)
    }
}
