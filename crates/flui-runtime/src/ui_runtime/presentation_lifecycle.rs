//! Presentation observations and UI runtime scheduler aggregation.
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_rendering::binding::RendererBinding as _;
use flui_scheduler::AppLifecycleState;
use flui_view::__runtime::BindingRuntime as _;

use super::UiRuntime;
use crate::lifecycle_state::{
    derive_lifecycle_state, lifecycle_ladder, preserve_first_lifecycle_panic,
};

/// The close mode for the next terminal step of a lifecycle pass: preserving
/// once a failure is held or the thread is already unwinding (ADR-0123).
fn terminal_close_mode(failure_held: bool) -> flui_interaction::__runtime::CloseMode {
    if failure_held || std::thread::panicking() {
        flui_interaction::__runtime::CloseMode::PreservingFailure
    } else {
        flui_interaction::__runtime::CloseMode::Ordinary
    }
}

/// Forget `presentation`'s held keys. A key-cancel handler's panic is held in
/// `first`, so the caller still finishes every window-focus write and the
/// lifecycle pass (pointer cancellation included) before it propagates.
fn forget_pressed_keys(
    presentation: &crate::presentation::PresentationState,
    first: &mut Option<Box<dyn std::any::Any + Send>>,
) {
    let failure = catch_unwind(AssertUnwindSafe(|| {
        presentation.focus_manager().forget_pressed_keys();
    }))
    .err();
    preserve_first_lifecycle_panic(first, failure, "held key cancellation");
}

/// An observed Detached state is reversible; terminal lifetime is explicit.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HostLifecycle {
    Observed(AppLifecycleState),
    Stopping,
}

impl UiRuntime {
    /// Record the host's application lifecycle and deliver what it changes
    /// to every presentation. Ignored once the UI runtime is stopping.
    pub fn update_host_lifecycle(&self, state: AppLifecycleState) {
        if self.host_lifecycle.get() == HostLifecycle::Stopping {
            return;
        }
        self.host_lifecycle.set(HostLifecycle::Observed(state));
        self.reconcile_lifecycle(Vec::new());
    }

    /// Adopt presentation `id`'s whole window state at once (execution,
    /// focus, visibility), as a window reports it when it is shown, and
    /// deliver what it changes. Cancels the pointer sequences of a
    /// presentation that loses focus or stops running; one that is not
    /// focused forgets its held keys.
    pub fn synchronize_window_snapshot(
        &self,
        id: PresentationId,
        execution: flui_platform::WindowExecutionState,
        focused: bool,
        visible: bool,
    ) {
        if self.host_lifecycle.get() == HostLifecycle::Stopping {
            return;
        }
        let Some(presentation) = self.presentations.get(id) else {
            return;
        };
        if presentation.closing_requested.get() {
            return;
        }
        presentation.window_execution.set(execution);
        if execution == flui_platform::WindowExecutionState::Detached {
            presentation.overlay_style.forget_delivery();
        }
        presentation.window_visible.set(visible);
        presentation.window_focused.set(focused);
        let mut cancel = Vec::new();
        let mut first = None;
        if focused {
            for other in self.presentations.iter() {
                if other.id() != id && other.window_focused.replace(false) {
                    forget_pressed_keys(other, &mut first);
                    cancel.push(other.id());
                }
            }
            self.notify_presentation_focus_gained(id);
        } else {
            forget_pressed_keys(presentation, &mut first);
        }
        if !focused || !visible || execution != flui_platform::WindowExecutionState::Running {
            cancel.push(id);
        }
        self.set_presentation_hidden(id, !visible);
        self.reconcile_lifecycle_after_failure(cancel, first);
    }

    /// Record presentation `id`'s window execution state; a window that
    /// stops running cancels its pointer sequences.
    pub fn update_window_execution(
        &self,
        id: PresentationId,
        state: flui_platform::WindowExecutionState,
    ) {
        if self.host_lifecycle.get() == HostLifecycle::Stopping {
            return;
        }
        let Some(presentation) = self.presentations.get(id) else {
            return;
        };
        if presentation.closing_requested.get() {
            return;
        }
        let changed = presentation.window_execution.replace(state) != state;
        presentation.sync_haptics_visibility();
        self.reconcile_lifecycle(
            if changed && state != flui_platform::WindowExecutionState::Running {
                vec![id]
            } else {
                Vec::new()
            },
        );
    }

    /// Record presentation `id`'s window focus. Focus is exclusive within
    /// the UI runtime: gaining it takes it from every sibling. A window that
    /// loses it forgets its held keys: their releases go to another window.
    pub fn update_window_focus(&self, id: PresentationId, focused: bool) {
        if self.host_lifecycle.get() == HostLifecycle::Stopping {
            return;
        }
        let Some(presentation) = self.presentations.get(id) else {
            return;
        };
        if presentation.closing_requested.get() {
            return;
        }
        let mut cancel = Vec::new();
        let mut first = None;
        if focused {
            for other in self.presentations.iter() {
                if other.id() != id && other.window_focused.replace(false) {
                    forget_pressed_keys(other, &mut first);
                    cancel.push(other.id());
                }
            }
            self.notify_presentation_focus_gained(id);
        } else {
            forget_pressed_keys(presentation, &mut first);
        }
        if presentation.window_focused.replace(focused) && !focused {
            cancel.push(id);
        }
        self.reconcile_lifecycle_after_failure(cancel, first);
    }

    /// Record presentation `id`'s window visibility, gating its frame clock.
    pub fn update_window_visibility(&self, id: PresentationId, visible: bool) {
        if self.host_lifecycle.get() == HostLifecycle::Stopping {
            return;
        }
        let Some(presentation) = self.presentations.get(id) else {
            return;
        };
        if presentation.closing_requested.get() {
            return;
        }
        let changed = presentation.window_visible.replace(visible) != visible;
        self.set_presentation_hidden(id, !visible);
        self.reconcile_lifecycle(if changed && !visible {
            vec![id]
        } else {
            Vec::new()
        });
    }

    /// Commit the composition of presentation `id`'s focused field, keeping
    /// its text: the step an accepted close takes before its close guard and
    /// save run, so the guard's check for unsaved changes and the saved draft
    /// see the preedit as text (ADR-0142 item 4). A pull platform's host ends
    /// its composition; elsewhere, or when the host abandons it, the
    /// presentation commits in place. Nothing happens for a presentation that
    /// is gone or has no composition.
    ///
    /// # Panics
    ///
    /// Resumes the first failure of the owner code the commit reaches (the
    /// field's `on_changed`, the host), as
    /// [`TextInputOwner::complete_composition`](flui_interaction::TextInputOwner::complete_composition)
    /// does.
    pub fn commit_composition_for_close(&self, id: PresentationId) {
        if let Some(presentation) = self.presentations.get(id) {
            presentation.text_input().complete_composition();
        }
    }

    /// Whether presentation `id`'s close guard refuses a close for `reason`.
    /// A refused close is recorded on the guard, once per reason, and is
    /// carried out when the last hold over it is released: the host does not
    /// close the presentation now, and the next
    /// [`settle_close_guards`](Self::settle_close_guards) after that release
    /// hands the close back. `false`, and nothing recorded, for a reason the
    /// platform does not let the application refuse and for a presentation
    /// that is gone.
    #[must_use]
    pub fn close_refused(&self, id: PresentationId, reason: flui_view::CloseReason) -> bool {
        self.presentations
            .get(id)
            .is_some_and(|presentation| presentation.close_guard().refuses(reason))
    }

    /// Presentation `id`'s close guard, for the host to answer its window's
    /// close requests with.
    pub(crate) fn close_guard_of(
        &self,
        id: PresentationId,
    ) -> Option<flui_view::__runtime::CloseGuardSource> {
        self.presentations
            .get(id)
            .map(|presentation| presentation.close_guard().clone())
    }

    /// Withdraw the close presentation `id`'s guard recorded for `reason`:
    /// the platform cancelled it, as when a log-off is cancelled. Closes
    /// recorded for other reasons stay recorded.
    pub fn withdraw_close(&self, id: PresentationId, reason: flui_view::CloseReason) {
        if let Some(presentation) = self.presentations.get(id) {
            presentation.close_guard().withdraw(reason);
        }
    }

    /// Settle every presentation's close guard on the owner's turn: wake the
    /// waiters each change resolved, one by one, and return the closes now
    /// due, for the host to carry out without asking the application again,
    /// and the recorded closes the application declined to carry out.
    /// Nothing happens when no guard signalled since the last settle.
    ///
    /// A waiter whose wake panics is reported as a
    /// [`CallbackPanic`](crate::frame_failure::FrameFailureKind::CallbackPanic) of its
    /// presentation and loses neither the due closes nor the waiters after
    /// it.
    pub fn settle_close_guards(&self) -> SettledCloses {
        if !self
            .close
            .settle_owed
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return SettledCloses::default();
        }
        let settled: Vec<_> = self
            .presentations
            .iter()
            .map(|presentation| (presentation.id(), presentation.close_guard().settle()))
            .collect();
        let mut closes = SettledCloses::default();
        for (id, settled) in settled {
            for waker in settled.wake {
                if let Err(payload) = catch_unwind(AssertUnwindSafe(|| waker.wake())) {
                    self.report_contained_panic(
                        flui_foundation::PresentationAddress {
                            ui_runtime_id: self.id,
                            presentation_id: id,
                        },
                        payload.as_ref(),
                    );
                    flui_foundation::panic::retain_opaque_payload(payload);
                }
            }
            closes.due.extend(settled.close.map(|reason| (id, reason)));
            closes
                .declined
                .extend(settled.declined.into_iter().map(|reason| (id, reason)));
        }
        closes
    }

    /// Attach the host's control of presentation `id`'s window: from now on its
    /// elements reach it through `LifecycleContext::window`. Call before the
    /// root widget is attached, so `init_state` finds it. `false` when no
    /// such presentation exists.
    pub fn install_window(
        &self,
        id: PresentationId,
        window: flui_view::__runtime::WindowSource,
    ) -> bool {
        let Some(presentation) = self.presentations.get(id) else {
            return false;
        };
        let handle = window.handle();
        presentation.window_placement.set(
            handle
                .state()
                .ok()
                .map(|state| crate::presentation::ContentPlacement::of(&state)),
        );
        presentation.widgets().set_window_handle(handle);
        *presentation.window_control.borrow_mut() = Some(std::rc::Rc::new(window));
        true
    }

    /// The window of presentation `id` reported that its state may have
    /// changed: deliver the new state to the subscribers of its handle.
    ///
    /// A state that puts the content somewhere else is the window moving:
    /// its gesture geometry and input regions follow
    /// ([`Self::window_moved`]) and `moved` runs, both before any subscriber
    /// does. `moved` runs first, so the move is noted even when following it
    /// panics; the subscribers still receive the state, and the first panic
    /// resumes afterwards.
    pub(crate) fn publish_window_state(&self, id: PresentationId, moved: impl FnOnce()) {
        let Some(window) = self
            .presentations
            .get(id)
            .and_then(|presentation| presentation.window_control.borrow().clone())
        else {
            return;
        };
        let Some(state) = window.queue_current() else {
            return;
        };
        let placement = Some(crate::presentation::ContentPlacement::of(&state));
        let placed_elsewhere = self.presentations.get(id).is_some_and(|presentation| {
            presentation.window_placement.replace(placement) != placement
        });
        let mut first = FirstPanic::new();
        if placed_elsewhere {
            moved();
            first.contain(|| self.window_moved(id));
        }
        first.contain(|| window.drain());
        first.resume();
    }

    /// Deliver a system notice to every presentation's subscribers
    /// (`LifecycleHandle::subscribe_notices`), in the order notices arrive.
    ///
    /// A notice is committed to every presentation before any subscriber runs,
    /// so a subscriber sees the same ordering across presentations. A
    /// presentation that is closing is skipped. Every presentation is drained
    /// even when a subscriber panics; the first panic resumes afterwards.
    pub fn deliver_host_notice(&self, notice: &flui_platform::HostNotice) {
        // This runtime's own caches give memory back first, so a subscriber
        // finds the easy memory already free. The host's shared receivers are
        // reclaimed once per notice by the host, not once per runtime.
        let mut first = FirstPanic::new();
        if let flui_platform::HostNotice::MemoryPressure(level) = notice {
            // `None` while a measurement has the context lent: it is
            // mid-layout, and the next notice reclaims it.
            first.contain(|| {
                let _ = self.text.try_with(|text| text.reclaim(*level));
            });
        }
        let sources: Vec<_> = self
            .presentations
            .iter()
            .map(|presentation| presentation.widgets().lifecycle_source())
            .collect();
        for source in &sources {
            // A closing presentation refuses the commit and receives nothing.
            let _ = source.commit_notice(notice.clone());
        }
        for source in sources {
            first.contain(|| source.drain());
        }
        first.resume();
    }

    /// Re-derive and deliver every presentation's lifecycle from the state
    /// already recorded.
    pub fn synchronize_window_lifecycle(&self) {
        self.reconcile_lifecycle(Vec::new());
    }

    /// Begin closing every presentation: each is told it is detached and
    /// drops its held input, and later lifecycle updates are ignored.
    pub fn stop_presentations(&self) {
        self.stop_presentations_after_failure(None);
    }

    pub(crate) fn stop_presentations_after_failure(
        &self,
        failure: Option<Box<dyn std::any::Any + Send>>,
    ) {
        self.host_lifecycle.set(HostLifecycle::Stopping);
        for presentation in self.presentations.iter() {
            presentation.closing_requested.set(true);
            presentation.widgets().lifecycle_source().begin_close();
            presentation.held_pointer_input().borrow_mut().clear();
        }
        self.reconcile_lifecycle_after_failure(Vec::new(), failure);
    }

    /// Begin closing presentation `id` alone: it is told it is detached and
    /// drops its held input.
    pub(super) fn stop_presentation_after_failure(
        &self,
        id: PresentationId,
        failure: Option<Box<dyn std::any::Any + Send>>,
    ) {
        if let Some(presentation) = self.presentations.get(id) {
            presentation.closing_requested.set(true);
            presentation.widgets().lifecycle_source().begin_close();
            presentation.held_pointer_input().borrow_mut().clear();
            self.reconcile_lifecycle_after_failure(Vec::new(), failure);
        } else if let Some(failure) = failure {
            resume_unwind(failure);
        }
    }

    /// Begin closing presentation `id` without removing it from the UI runtime,
    /// so a test can observe what a closing presentation still accepts.
    /// Production closes through [`Self::close_presentation_entered`], which
    /// stops and removes it in one step.
    #[cfg(any(test, feature = "testing"))]
    pub fn stop_presentation_for_test(&self, id: PresentationId) {
        self.stop_presentation_after_failure(id, None);
    }

    fn execution_lifecycle(
        &self,
        presentation: &crate::presentation::PresentationState,
    ) -> AppLifecycleState {
        use AppLifecycleState::{Detached, Hidden, Inactive, Paused, Resumed};
        if presentation.closing_requested.get() {
            return Detached;
        }
        let execution = presentation.window_execution.get();
        if matches!(
            self.host_lifecycle.get(),
            HostLifecycle::Stopping | HostLifecycle::Observed(Detached)
        ) || execution == flui_platform::WindowExecutionState::Detached
        {
            return Detached;
        }
        if self.host_lifecycle.get() == HostLifecycle::Observed(Paused)
            || execution == flui_platform::WindowExecutionState::Suspended
        {
            return Paused;
        }
        let window = derive_lifecycle_state(
            presentation.window_visible.get(),
            presentation.window_focused.get(),
        );
        match self.host_lifecycle.get() {
            HostLifecycle::Stopping | HostLifecycle::Observed(Detached) => Detached,
            HostLifecycle::Observed(Paused) => Paused,
            HostLifecycle::Observed(Hidden) => Hidden,
            HostLifecycle::Observed(Inactive) => {
                if window == Hidden {
                    Hidden
                } else {
                    Inactive
                }
            }
            HostLifecycle::Observed(Resumed) => window,
        }
    }

    fn aggregate_lifecycle(&self) -> AppLifecycleState {
        use AppLifecycleState::{Detached, Hidden, Inactive, Paused, Resumed};
        [Resumed, Inactive, Hidden, Paused]
            .into_iter()
            .find(|state| {
                self.presentations.iter().any(|presentation| {
                    !presentation.closing_requested.get()
                        && self.execution_lifecycle(presentation) == *state
                })
            })
            .unwrap_or(Detached)
    }

    fn seed_terminal_lifecycle_recovery(&self) {
        for presentation in self
            .presentations
            .iter()
            .filter(|owner| owner.closing_requested.get())
        {
            presentation
                .widgets()
                .lifecycle_source()
                .begin_close_with_mode(flui_interaction::__runtime::CloseMode::PreservingFailure);
        }
    }

    /// Keeps the first failure of a lifecycle pass. Once any failure is held,
    /// or the thread is already unwinding, every closing presentation's
    /// lifecycle source switches to preserving mode (ADR-0123).
    fn record_lifecycle_failure(
        &self,
        first_panic: &mut Option<Box<dyn std::any::Any + Send>>,
        failure: Option<Box<dyn std::any::Any + Send>>,
        phase: &'static str,
    ) {
        if first_panic.is_some() || failure.is_some() || std::thread::panicking() {
            self.seed_terminal_lifecycle_recovery();
        }
        preserve_first_lifecycle_panic(first_panic, failure, phase);
    }

    fn reconcile_lifecycle(&self, cancel: Vec<PresentationId>) {
        self.reconcile_lifecycle_after_failure(cancel, None);
    }

    fn reconcile_lifecycle_after_failure(
        &self,
        mut cancel: Vec<PresentationId>,
        mut first_panic: Option<Box<dyn std::any::Any + Send>>,
    ) {
        // A closing presentation's terminal recovery spans this whole pass;
        // its preserving policy ends when the pass returns (ADR-0123).
        let lifecycle_windows: Vec<_> = self
            .presentations
            .iter()
            .filter(|presentation| presentation.closing_requested.get())
            .map(|presentation| presentation.widgets().lifecycle_source().close_window())
            .collect();
        if first_panic.is_some() || std::thread::panicking() {
            self.seed_terminal_lifecycle_recovery();
        }
        let transitions: Vec<_> = self
            .presentations
            .iter()
            .map(|presentation| {
                (
                    presentation,
                    presentation
                        .widgets()
                        .lifecycle_source()
                        .current()
                        .map_or_else(
                            || vec![self.execution_lifecycle(presentation)],
                            |old| lifecycle_ladder(old, self.execution_lifecycle(presentation)),
                        ),
                )
            })
            .collect();
        let rounds = transitions
            .iter()
            .map(|(_, steps)| steps.len())
            .max()
            .unwrap_or(0)
            .max(1);
        for round in 0..rounds {
            // Commit every local step before running any user callback. Input
            // cleanup can still observe the preceding aggregate scheduler state;
            // lifecycle observers run after the aggregate commits below.
            let changed: Vec<_> = transitions
                .iter()
                .filter_map(|(presentation, steps)| {
                    steps.get(round).map(|step| {
                        let source = presentation.widgets().lifecycle_source();
                        let old = source.current();
                        let committed = if presentation.closing_requested.get() {
                            source.commit_terminal(*step)
                        } else {
                            source.commit(*step)
                        };
                        debug_assert!(
                            committed.is_ok(),
                            "live presentation source accepts its owner commit"
                        );
                        (*presentation, old, *step)
                    })
                })
                .collect();
            for (presentation, _, step) in &changed {
                if matches!(
                    step,
                    AppLifecycleState::Inactive
                        | AppLifecycleState::Hidden
                        | AppLifecycleState::Paused
                        | AppLifecycleState::Detached
                ) {
                    cancel.push(presentation.id());
                }
            }
            cancel.sort_unstable();
            cancel.dedup();
            for id in cancel.drain(..) {
                // A closing presentation's gestures are cancelled by its
                // terminal close below, after every presentation-wide
                // capability is withdrawn: a recognizer's rejection callback
                // must not reenter the closing presentation (ADR-0123).
                if self
                    .presentations
                    .get(id)
                    .is_some_and(|presentation| presentation.closing_requested.get())
                {
                    continue;
                }
                let failure =
                    catch_unwind(AssertUnwindSafe(|| self.cancel_pointer_sequences_for(id))).err();
                self.record_lifecycle_failure(
                    &mut first_panic,
                    failure,
                    "presentation input cancellation",
                );
                if let Some(presentation) = self.presentations.get(id)
                    && matches!(
                        self.execution_lifecycle(presentation),
                        AppLifecycleState::Hidden
                            | AppLifecycleState::Paused
                            | AppLifecycleState::Detached
                    )
                {
                    // Focus loss still delivers Cancel and preserves hovering.
                    // Suspension also discards motion queued before the pause;
                    // this phase must run even when Cancel delivery panicked.
                    let failure = catch_unwind(AssertUnwindSafe(|| {
                        presentation.gestures().handle_lifecycle_pause();
                    }))
                    .err();
                    self.record_lifecycle_failure(
                        &mut first_panic,
                        failure,
                        "presentation paused input drain",
                    );
                }
            }
            // Resource eligibility is plain owner-local state. Commit it before
            // scheduler callbacks; redraw/wake effects follow the scheduler.
            let mut restored = Vec::new();
            for presentation in self.presentations.iter() {
                if matches!(
                    self.execution_lifecycle(presentation),
                    AppLifecycleState::Resumed | AppLifecycleState::Inactive
                ) {
                    let suspended = presentation.lifecycle()
                        == crate::presentation::PresentationLifecycle::Suspended;
                    presentation.resume();
                    if suspended {
                        restored.push(presentation);
                    }
                } else {
                    presentation.suspend();
                }
            }
            let aggregate = self.aggregate_lifecycle();
            if self.scheduler().lifecycle_state() != aggregate {
                let step = aggregate;
                let failure = catch_unwind(AssertUnwindSafe(|| {
                    self.scheduler().handle_app_lifecycle_state_change(step);
                }))
                .err();
                self.record_lifecycle_failure(
                    &mut first_panic,
                    failure,
                    "ui_runtime lifecycle scheduler",
                );
            }
            for presentation in restored {
                let failure = catch_unwind(AssertUnwindSafe(|| {
                    crate::renderer_binding::redirty_pipeline_root(
                        presentation.renderer().root_pipeline_owner(),
                    );
                    self.request_redraw_for(presentation);
                    self.wake_frame();
                }))
                .err();
                self.record_lifecycle_failure(
                    &mut first_panic,
                    failure,
                    "presentation redraw restoration",
                );
            }
            for (presentation, _, step) in changed {
                // Every path to Detached (an accepted close, the window
                // destroyed, the session's end) commits the focused field's
                // composition first, keeping its text, so the observers and
                // the save see the preedit as text (ADR-0142 item 4). The
                // field's own unmount then finds nothing left to commit. A
                // close already holding a failure runs no more owner code, and
                // a commit that fails turns this close into one that does.
                let preserving = |failed: bool| {
                    presentation.closing_requested.get()
                        && terminal_close_mode(failed)
                            == flui_interaction::__runtime::CloseMode::PreservingFailure
                };
                if step == AppLifecycleState::Detached && !preserving(first_panic.is_some()) {
                    let failure = catch_unwind(AssertUnwindSafe(|| {
                        presentation.text_input().complete_composition();
                    }))
                    .err();
                    self.record_lifecycle_failure(
                        &mut first_panic,
                        failure,
                        "presentation composition commit",
                    );
                }
                if preserving(first_panic.is_some()) {
                    let failure = catch_unwind(AssertUnwindSafe(|| {
                        presentation
                            .widgets()
                            .lifecycle_source()
                            .begin_close_with_mode(
                                flui_interaction::__runtime::CloseMode::PreservingFailure,
                            );
                    }))
                    .err();
                    self.record_lifecycle_failure(
                        &mut first_panic,
                        failure,
                        "terminal lifecycle withdrawal",
                    );
                    continue;
                }
                let failure = catch_unwind(AssertUnwindSafe(|| {
                    presentation.widgets().notify_committed_lifecycle(step);
                }))
                .err();
                self.record_lifecycle_failure(
                    &mut first_panic,
                    failure,
                    "presentation lifecycle observers",
                );
            }
        }
        // Terminal notification precedes root disposal. Observed Detached alone
        // never closes the native lifetime and can transition back to Resumed.
        for presentation in self
            .presentations
            .iter()
            .filter(|p| p.closing_requested.get())
        {
            let mode = terminal_close_mode(first_panic.is_some());
            let failure = catch_unwind(AssertUnwindSafe(|| {
                presentation
                    .widgets()
                    .lifecycle_source()
                    .finish_close_with_mode(mode);
            }))
            .err();
            self.record_lifecycle_failure(
                &mut first_panic,
                failure,
                "lifecycle subscription close",
            );
            let mode = terminal_close_mode(first_panic.is_some());
            let failure =
                catch_unwind(AssertUnwindSafe(|| presentation.close_with_mode(mode))).err();
            self.record_lifecycle_failure(
                &mut first_panic,
                failure,
                "terminal presentation cleanup",
            );
        }
        drop(lifecycle_windows);
        if let Some(payload) = first_panic {
            if std::thread::panicking() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                resume_unwind(payload);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    fn lifecycle_subscription_direct_drop_preserves_outer_unwind_and_invalidates_handle() {
        let retained = Rc::new(RefCell::new(None));
        let output = Rc::clone(&retained);
        let outer = catch_unwind(AssertUnwindSafe(move || {
            let ui_runtime = UiRuntime::for_test();
            let source = ui_runtime
                .presentations
                .primary()
                .widgets()
                .lifecycle_source();
            let handle = source.handle();
            let (_, token) = handle
                .subscribe(|_| panic!("lifecycle during unwind"))
                .expect("open");
            output.replace(Some((handle, token)));
            panic!("outer panic");
        }))
        .expect_err("outer survives");
        assert_eq!(outer.downcast_ref::<&str>(), Some(&"outer panic"));
        assert_eq!(
            retained.borrow().as_ref().expect("retained").0.snapshot(),
            Err(flui_view::LifecycleClosed)
        );
    }

    fn two_presentations() -> (UiRuntime, PresentationId, PresentationId) {
        let mut ui_runtime = UiRuntime::for_test();
        let a = ui_runtime.presentation_id();
        let b = ui_runtime.install_second_presentation_for_test();
        ui_runtime.synchronize_window_lifecycle();
        (ui_runtime, a, b)
    }

    fn window_execution_suspension_cancels_real_pointer_before_public_notification() {
        let (ui_runtime, _a, b) = two_presentations();
        ui_runtime
            .attach_root_widget_to_for_test(b, &flui_widgets::SizedBox::new(10.0, 10.0))
            .expect("root");
        let ui_runtime = Rc::new(ui_runtime);
        ui_runtime.enter(|ui_runtime| {
            ui_runtime.handle_input_addressed(
                b,
                flui_platform::PlatformInput::Pointer(
                    flui_interaction::events::make_down_event(
                        flui_foundation::geometry::Offset::new(1.0, 1.0),
                        flui_interaction::events::PointerKind::Mouse,
                    )
                    .expect("finite test position"),
                ),
            );
        });
        assert_eq!(
            ui_runtime
                .presentations
                .get(b)
                .expect("B")
                .gestures()
                .active_pointer_count(),
            1
        );
        let handle = ui_runtime
            .presentations
            .get(b)
            .expect("B")
            .widgets()
            .lifecycle_source()
            .handle();
        let weak = Rc::downgrade(&ui_runtime);
        let history = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&history);
        let (_, _subscription) = handle
            .subscribe(move |state| {
                let ui_runtime = weak.upgrade().expect("live ui_runtime");
                assert_eq!(
                    ui_runtime
                        .presentations
                        .get(b)
                        .expect("B")
                        .gestures()
                        .active_pointer_count(),
                    0
                );
                seen.borrow_mut().push(state);
            })
            .expect("subscription");
        ui_runtime.update_window_execution(b, flui_platform::WindowExecutionState::Suspended);
        assert_eq!(
            handle.snapshot().expect("live"),
            Some(AppLifecycleState::Paused)
        );
        assert_eq!(history.borrow().last(), Some(&AppLifecycleState::Paused));
        assert!(
            ui_runtime.scheduler().frames_enabled(),
            "visible sibling remains eligible"
        );
    }

    fn notice_handles(
        ui_runtime: &UiRuntime,
        ids: &[PresentationId],
    ) -> Vec<flui_view::LifecycleHandle> {
        ids.iter()
            .map(|id| {
                ui_runtime
                    .presentations
                    .get(*id)
                    .expect("presentation")
                    .widgets()
                    .lifecycle_source()
                    .handle()
            })
            .collect()
    }

    fn host_notices_reach_every_presentation_in_arrival_order() {
        use flui_platform::{HostNotice, MemoryPressureLevel};
        let (ui_runtime, a, b) = two_presentations();
        let log = Rc::new(RefCell::new(Vec::new()));
        let _tokens: Vec<_> = notice_handles(&ui_runtime, &[a, b])
            .into_iter()
            .enumerate()
            .map(|(index, handle)| {
                let seen = Rc::clone(&log);
                handle
                    .subscribe_notices(move |notice| {
                        let label = match notice {
                            HostNotice::Sleep => "sleep",
                            HostNotice::Wake => "wake",
                            HostNotice::MemoryPressure(MemoryPressureLevel::Critical) => "critical",
                            _ => "other",
                        };
                        seen.borrow_mut().push((index, label));
                    })
                    .expect("open presentation")
            })
            .collect();
        ui_runtime.deliver_host_notice(&HostNotice::Sleep);
        ui_runtime.deliver_host_notice(&HostNotice::Wake);
        ui_runtime.deliver_host_notice(&HostNotice::MemoryPressure(MemoryPressureLevel::Critical));
        assert_eq!(
            *log.borrow(),
            [
                (0, "sleep"),
                (1, "sleep"),
                (0, "wake"),
                (1, "wake"),
                (0, "critical"),
                (1, "critical")
            ],
            "every presentation hears every notice, notice by notice"
        );
    }

    fn a_notice_is_not_replayed_to_a_late_subscriber() {
        use flui_platform::HostNotice;
        let (ui_runtime, a, _b) = two_presentations();
        ui_runtime.deliver_host_notice(&HostNotice::Wake);
        let count = Rc::new(Cell::new(0));
        let seen = Rc::clone(&count);
        let _token = notice_handles(&ui_runtime, &[a])[0]
            .subscribe_notices(move |_| seen.set(seen.get() + 1))
            .expect("open presentation");
        assert_eq!(count.get(), 0, "registering calls nothing");
        ui_runtime.deliver_host_notice(&HostNotice::Sleep);
        assert_eq!(count.get(), 1, "only what happens after registration");
    }

    fn session_end_blocks_of_several_presentations_aggregate() {
        use flui_platform::{HostNotice, SessionEndBlock, SessionEndRequest};
        let (ui_runtime, a, b) = two_presentations();
        let held: Rc<RefCell<Vec<SessionEndBlock>>> = Rc::default();
        let _tokens: Vec<_> = notice_handles(&ui_runtime, &[a, b])
            .into_iter()
            .enumerate()
            .map(|(index, handle)| {
                let held = Rc::clone(&held);
                handle
                    .subscribe_notices(move |notice| {
                        if let HostNotice::SessionEnding(request) = notice {
                            held.borrow_mut().push(
                                request
                                    .block(format!("save {index}"))
                                    .expect("the book is open until the host answers"),
                            );
                        }
                    })
                    .expect("open presentation")
            })
            .collect();
        let released = Rc::new(Cell::new(0));
        let on_release = Rc::clone(&released);
        let request =
            SessionEndRequest::new().on_release(move || on_release.set(on_release.get() + 1));
        ui_runtime.deliver_host_notice(&HostNotice::SessionEnding(request.clone()));
        assert!(request.is_blocked());
        assert_eq!(request.reasons(), ["save 0", "save 1"]);
        // The host answers the platform once every presentation has heard it.
        assert_eq!(request.answer(), flui_platform::SessionEndAnswer::Block);

        let first = held.borrow_mut().remove(0);
        drop(first);
        assert!(request.is_blocked(), "one presentation still holds");
        assert_eq!(released.get(), 0);
        held.borrow_mut().clear();
        assert!(!request.is_blocked());
        assert_eq!(released.get(), 1, "the last block released the session end");
    }

    fn a_panicking_notice_subscriber_does_not_starve_the_others() {
        use flui_platform::HostNotice;
        let (ui_runtime, a, b) = two_presentations();
        let reached = Rc::new(Cell::new(false));
        let handles = notice_handles(&ui_runtime, &[a, b]);
        let _panics = handles[0]
            .subscribe_notices(|_| panic!("a subscriber fails"))
            .expect("open presentation");
        let seen = Rc::clone(&reached);
        let _hears = handles[1]
            .subscribe_notices(move |_| seen.set(true))
            .expect("open presentation");
        let failure = catch_unwind(AssertUnwindSafe(|| {
            ui_runtime.deliver_host_notice(&HostNotice::Wake);
        }))
        .expect_err("the first panic resumes after delivery");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"a subscriber fails"));
        assert!(reached.get(), "the other presentation still heard it");
    }

    #[test]
    fn host_notice_delivery() {
        crate::table_test::run_table(
            "host_notice_delivery",
            &[
                (
                    "host_notices_reach_every_presentation_in_arrival_order",
                    host_notices_reach_every_presentation_in_arrival_order as fn(),
                ),
                (
                    "a_notice_is_not_replayed_to_a_late_subscriber",
                    a_notice_is_not_replayed_to_a_late_subscriber as fn(),
                ),
                (
                    "session_end_blocks_of_several_presentations_aggregate",
                    session_end_blocks_of_several_presentations_aggregate as fn(),
                ),
                (
                    "a_panicking_notice_subscriber_does_not_starve_the_others",
                    a_panicking_notice_subscriber_does_not_starve_the_others as fn(),
                ),
            ],
        );
    }

    /// An accepted close commits the composition of the closing
    /// presentation's focused field, keeping its text, before the close guard
    /// and the save see the field (ADR-0142 item 4); asked again, or for a
    /// presentation that is gone, it commits nothing.
    ///
    /// Red-check: drop the commit from `commit_composition_for_close` — the
    /// store is still composing.
    fn an_accepted_close_commits_the_preedit_before_the_guard() {
        use flui_interaction::testing::InMemoryTextStore;
        use flui_platform::text_store::{
            Composition, LockGrant, LockOutcome, LockTiming, TextStore, Utf16Offset, Utf16Range,
        };

        let text_input: std::sync::Arc<dyn flui_platform::PlatformTextInput> =
            std::sync::Arc::new(flui_native::FakeTextInput::new());
        let mut ui_runtime = UiRuntime::for_test_with_text_input(Some(text_input));
        let id = ui_runtime.presentation_id();
        let gone = ui_runtime.install_second_presentation_for_test();
        assert!(ui_runtime.close_presentation_entered(gone));
        let store = InMemoryTextStore::new("ab");
        let erased: Rc<dyn TextStore> = store.clone(); // the presentation holds the field's store through the erased contract.
        let _token = ui_runtime
            .text_input_handle()
            .attach(flui_interaction::TextInputClient::new(Rc::clone(&erased)))
            .expect("headless presentation supports text input");
        let composing = Composition {
            range: Utf16Range::try_new(Utf16Offset::new(2), Utf16Offset::new(4)).expect("ordered"),
            hides_caret: false,
        };
        assert_eq!(
            erased.request_lock(
                LockGrant::read_write(move |session| {
                    session.insert_at_selection("東京").expect("in range");
                    session.set_composition(Some(composing)).expect("in range");
                }),
                LockTiming::Sync,
            ),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(store.composition(), Some(composing), "precondition");

        ui_runtime.commit_composition_for_close(gone);
        assert_eq!(
            store.composition(),
            Some(composing),
            "a presentation that is gone commits nothing"
        );
        ui_runtime.commit_composition_for_close(id);
        assert_eq!(store.composition(), None, "the composition is committed");
        assert_eq!(store.text(), "ab東京", "keeping its text");
        ui_runtime.commit_composition_for_close(id);
        assert_eq!(store.text(), "ab東京", "a second request changes nothing");
    }

    /// A close whose composition commit fails holds that failure: the
    /// presentation's observers, scoped and legacy, hear no Detached after it
    /// (ADR-0123), and the close resumes the commit's failure.
    ///
    /// Red-check: drop the preserving check after the commit — the legacy
    /// observer hears Detached.
    fn a_failed_close_commit_withholds_the_detached_notice() {
        use flui_interaction::testing::InMemoryTextStore;
        use flui_platform::text_store::{
            Composition, LockGrant, LockOutcome, LockTiming, TextStore, Utf16Offset, Utf16Range,
        };

        #[derive(Default)]
        struct Heard(parking_lot::Mutex<Vec<AppLifecycleState>>);

        impl flui_view::WidgetsBindingObserver for Heard {
            fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
                self.0.lock().push(state);
            }
        }

        let text_input: std::sync::Arc<dyn flui_platform::PlatformTextInput> =
            std::sync::Arc::new(flui_native::FakeTextInput::new());
        let mut ui_runtime = UiRuntime::for_test_with_text_input(Some(text_input));
        let id = ui_runtime.presentation_id();
        let _sibling = ui_runtime.install_second_presentation_for_test();
        let store = InMemoryTextStore::new("ab");
        let erased: Rc<dyn TextStore> = store.clone(); // the presentation holds the field's store through the erased contract.
        let _token = ui_runtime
            .text_input_handle()
            .attach(flui_interaction::TextInputClient::new(Rc::clone(&erased)))
            .expect("headless presentation supports text input");
        let composing = Composition {
            range: Utf16Range::try_new(Utf16Offset::new(2), Utf16Offset::new(4)).expect("ordered"),
            hides_caret: false,
        };
        assert_eq!(
            erased.request_lock(
                LockGrant::read_write(move |session| {
                    session.insert_at_selection("東京").expect("in range");
                    session.set_composition(Some(composing)).expect("in range");
                }),
                LockTiming::Sync,
            ),
            Ok(LockOutcome::Granted)
        );
        store.set_owner_listener(Some(Rc::new(|| panic!("the field's on_changed failed"))));
        let heard = std::sync::Arc::new(Heard::default());
        ui_runtime
            .presentation_widgets_for_test(id)
            .add_observer(std::sync::Arc::clone(&heard)
                as std::sync::Arc<dyn flui_view::WidgetsBindingObserver>);

        let closed = catch_unwind(AssertUnwindSafe(|| {
            ui_runtime.close_presentation_entered(id)
        }));
        let payload = closed.expect_err("the close resumes the commit's failure");
        assert_eq!(
            flui_foundation::panic::payload_text(&*payload),
            Some("the field's on_changed failed")
        );
        assert!(
            !heard.0.lock().contains(&AppLifecycleState::Detached),
            "no Detached after the failed commit: {:?}",
            heard.0.lock()
        );
    }

    #[test]
    fn presentation_lifecycle_matrix() {
        crate::table_test::run_table(
            "presentation_lifecycle_matrix",
            &[
                (
                    "lifecycle_subscription_direct_drop_preserves_outer_unwind_and_invalidates_handle",
                    lifecycle_subscription_direct_drop_preserves_outer_unwind_and_invalidates_handle
                        as fn(),
                ),
                (
                    "window_execution_suspension_cancels_real_pointer_before_public_notification",
                    window_execution_suspension_cancels_real_pointer_before_public_notification
                        as fn(),
                ),
                (
                    "an_accepted_close_commits_the_preedit_before_the_guard",
                    an_accepted_close_commits_the_preedit_before_the_guard as fn(),
                ),
                (
                    "a_failed_close_commit_withholds_the_detached_notice",
                    a_failed_close_commit_withholds_the_detached_notice as fn(),
                ),
            ],
        );
    }
}

/// What [`UiRuntime::settle_close_guards`] hands the host.
#[derive(Debug, Default)]
#[must_use = "a due close is carried out by the host"]
#[non_exhaustive]
pub struct SettledCloses {
    /// The closes now due, each for its presentation and with the reason it
    /// was recorded for.
    pub due: Vec<(PresentationId, flui_view::CloseReason)>,
    /// The recorded closes the application resolved by keeping the
    /// presentation open (`PendingClose::stay_open`).
    pub declined: Vec<(PresentationId, flui_view::CloseReason)>,
}
