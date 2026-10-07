//! Presentation observations and UI runtime scheduler aggregation.
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use flui_foundation::PresentationId;
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
    /// presentation that loses focus or stops running.
    pub fn synchronize_window_snapshot(
        &self,
        id: PresentationId,
        execution: flui_platform_api::WindowExecutionState,
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
        presentation.window_visible.set(visible);
        presentation.window_focused.set(focused);
        let mut cancel = Vec::new();
        if focused {
            for other in self.presentations.iter() {
                if other.id() != id && other.window_focused.replace(false) {
                    cancel.push(other.id());
                }
            }
            self.notify_presentation_focus_gained(id);
        }
        if !focused || !visible || execution != flui_platform_api::WindowExecutionState::Running {
            cancel.push(id);
        }
        self.set_presentation_hidden(id, !visible);
        self.reconcile_lifecycle(cancel);
    }

    /// Record presentation `id`'s window execution state; a window that
    /// stops running cancels its pointer sequences.
    pub fn update_window_execution(
        &self,
        id: PresentationId,
        state: flui_platform_api::WindowExecutionState,
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
        self.reconcile_lifecycle(
            if changed && state != flui_platform_api::WindowExecutionState::Running {
                vec![id]
            } else {
                Vec::new()
            },
        );
    }

    /// Record presentation `id`'s window focus. Focus is exclusive within
    /// the UI runtime: gaining it takes it from every sibling.
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
        if focused {
            for other in self.presentations.iter() {
                if other.id() != id && other.window_focused.replace(false) {
                    cancel.push(other.id());
                }
            }
            self.notify_presentation_focus_gained(id);
        }
        if presentation.window_focused.replace(focused) && !focused {
            cancel.push(id);
        }
        self.reconcile_lifecycle(cancel);
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

    /// Re-derive and deliver every presentation's lifecycle from the state
    /// already recorded.
    pub fn synchronize_window_lifecycle(&self) {
        self.reconcile_lifecycle(Vec::new());
    }

    /// Begin closing every presentation: each is told it is detached and
    /// drops its held input, and later lifecycle updates are ignored.
    pub fn stop_presentations(&self) {
        self.host_lifecycle.set(HostLifecycle::Stopping);
        for presentation in self.presentations.iter() {
            presentation.closing_requested.set(true);
            presentation.widgets().lifecycle_source().begin_close();
            presentation.held_pointer_input().borrow_mut().clear();
        }
        self.reconcile_lifecycle(Vec::new());
    }

    /// Begin closing presentation `id` alone: it is told it is detached and
    /// drops its held input.
    pub(crate) fn stop_presentation(&self, id: PresentationId) {
        if let Some(presentation) = self.presentations.get(id) {
            presentation.closing_requested.set(true);
            presentation.widgets().lifecycle_source().begin_close();
            presentation.held_pointer_input().borrow_mut().clear();
            self.reconcile_lifecycle(Vec::new());
        }
    }

    /// Begin closing presentation `id` without removing it from the UI runtime,
    /// so a test can observe what a closing presentation still accepts.
    /// Production closes through [`Self::close_presentation_entered`], which
    /// stops and removes it in one step.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stop_presentation_for_test(&self, id: PresentationId) {
        self.stop_presentation(id);
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
        ) || execution == flui_platform_api::WindowExecutionState::Detached
        {
            return Detached;
        }
        if self.host_lifecycle.get() == HostLifecycle::Observed(Paused)
            || execution == flui_platform_api::WindowExecutionState::Suspended
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

    fn reconcile_lifecycle(&self, mut cancel: Vec<PresentationId>) {
        // A closing presentation's terminal recovery spans this whole pass;
        // its preserving policy ends when the pass returns (ADR-0123).
        let lifecycle_windows: Vec<_> = self
            .presentations
            .iter()
            .filter(|presentation| presentation.closing_requested.get())
            .map(|presentation| presentation.widgets().lifecycle_source().close_window())
            .collect();
        if std::thread::panicking() {
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
        let mut first_panic = None;
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
                if presentation.closing_requested.get()
                    && terminal_close_mode(first_panic.is_some())
                        == flui_interaction::__runtime::CloseMode::PreservingFailure
                {
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

    use std::{cell::RefCell, rc::Rc};

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
                flui_platform_api::PlatformInput::Pointer(
                    flui_interaction::events::make_down_event(
                        flui_foundation::geometry::Offset::new(1.0, 1.0),
                        flui_interaction::events::PointerType::Mouse,
                    ),
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
        ui_runtime.update_window_execution(b, flui_platform_api::WindowExecutionState::Suspended);
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
            ],
        );
    }
}
