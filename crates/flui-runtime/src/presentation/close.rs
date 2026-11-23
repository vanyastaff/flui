//! Closing a presentation: the close path, the withdrawal of its retained
//! ownership and the recovery that keeps the first failure of a close.

use std::cell::Cell;
use std::sync::{Arc, Weak};

use flui_foundation::panic::FirstPanic;
use flui_platform::{Cursor, CursorIcon};
use flui_view::__runtime::BindingRuntime as _;

use super::{PresentationLifecycle, PresentationState};

impl PresentationState {
    fn close_interaction_with_mode(
        &self,
        lane: Option<&flui_interaction::pointer::InteractionLane>,
        mode: flui_interaction::__runtime::CloseMode,
    ) {
        if let Some(handle) = self.interaction_dispatch() {
            if let Some(lane) = lane {
                flui_interaction::__runtime::close_dispatch_in(lane, handle, mode);
            } else {
                flui_interaction::__runtime::close_dispatch(handle, mode);
            }
        }
    }

    /// Withdraw dispatch authority, keeping the captures for
    /// [`flui_interaction::__runtime::retire_dispatch`].
    fn withdraw_interaction(
        &self,
        lane: Option<&flui_interaction::pointer::InteractionLane>,
        mode: flui_interaction::__runtime::CloseMode,
    ) -> Option<flui_interaction::__runtime::DispatchCustody> {
        let handle = self.interaction_dispatch()?;
        match lane {
            Some(lane) => flui_interaction::__runtime::withdraw_dispatch_in(lane, handle, mode),
            None => flui_interaction::__runtime::withdraw_dispatch(handle, mode),
        }
    }

    pub(crate) fn close_with_mode_in(
        &self,
        mode: flui_interaction::__runtime::CloseMode,
        lane: &flui_interaction::pointer::InteractionLane,
    ) {
        self.close_impl(mode, Some(lane));
    }

    pub(crate) fn close(&self) {
        self.close_with_mode(flui_interaction::__runtime::CloseMode::Ordinary);
    }

    pub(crate) fn close_with_mode(&self, mode: flui_interaction::__runtime::CloseMode) {
        self.close_impl(mode, None);
    }

    fn close_impl(
        &self,
        mode: flui_interaction::__runtime::CloseMode,
        lane: Option<&flui_interaction::pointer::InteractionLane>,
    ) {
        self.withdraw_animation();
        use flui_interaction::__runtime::{
            close_focus, close_gestures, close_mouse_tracker, close_text_input,
        };
        // One reentry window spans every owner this close reaches.
        let _lifecycle_window = self.widgets.lifecycle_source().close_window();
        let mut window = flui_interaction::__runtime::CloseWindow::new();
        if let Some(handle) = self.interaction_dispatch() {
            window.dispatch(handle);
        }
        window.gestures(&self.gestures);
        window.focus(&self.focus);
        window.text_input(&self.text_input);
        let mut failure = PresentationCloseRecovery::new(mode, &self.close_mode, window);

        if matches!(
            self.lifecycle.get(),
            PresentationLifecycle::Closing | PresentationLifecycle::Closed
        ) {
            if failure.preserving() {
                self.withdraw_retained_ownership(&mut failure, lane);
            }
            self.finish_animation(&mut failure);
            failure.finish();
            return;
        }
        self.lifecycle.set(PresentationLifecycle::Closing);
        self.pipeline.withdraw_layout();
        let saved_text = std::mem::take(&mut *self.text_preparation.borrow_mut());
        failure.retire(saved_text);
        if let Some(scope) = &self.completion_scope {
            scope.withdraw();
        }
        let window = self.window.upgrade();
        let bridge = self.accessibility.as_ref().and_then(Weak::upgrade);
        // Every capability is withdrawn before any capture is destroyed: a
        // dispatch target's destructor may hold saved handles, and must find
        // the graph, keys, focus and text input already closed.
        let mode = failure.mode();
        let withdrawn = self.withdrawn_dispatch.borrow_mut().take();
        let dispatch = withdrawn.or_else(|| {
            failure
                .invoke_with(|| self.withdraw_interaction(lane, mode))
                .flatten()
        });
        self.alive.borrow_mut().take();
        self.held_pointer_input.borrow_mut().clear();
        // Owner authority is withdrawn before any final user notification.
        // The withdrawn key owners stay in custody until focus, text input,
        // gestures and mouse tracking are closed too: a key's destructor may
        // hold a saved focus node or text-input handle (ADR-0123).
        let withdrawn_keys = if failure.preserving() {
            failure.invoke(|| self.widgets.withdraw_root_owner(true));
            Vec::new()
        } else {
            failure
                .invoke_with(|| self.widgets.withdraw_owner_authority())
                .unwrap_or_default()
        };
        let agent = self.dev_agent.take();
        if let Some(agent) = &agent {
            agent.withdraw();
        }
        self.abandon_agent_jobs();
        let window_control = self.window_control.take();
        if let Some(window) = &window_control {
            failure.invoke(|| window.close());
        }

        let source = self.widgets.lifecycle_source();
        source.begin_close();
        let _ = source.commit_terminal(flui_scheduler::AppLifecycleState::Detached);
        failure.run(|| source.drain());
        let mode = failure.mode();
        failure.invoke(|| source.finish_close_with_mode(mode));
        let mode = failure.mode();
        failure.invoke(|| close_mouse_tracker(self.gestures.mouse_tracker(), mode));
        failure.retire(agent);
        failure.retire(window_control);
        let mode = failure.mode();
        failure.invoke(|| close_focus(&self.focus, mode));
        let mode = failure.mode();
        failure.invoke(|| close_text_input(&self.text_input, mode));
        // Queued haptics resolve and their outcomes get one final delivery
        // pass; after a failure the observer is retained, never run.
        if failure.preserving() {
            failure.invoke(|| self.haptics.withdraw());
        } else {
            failure.invoke(|| {
                flui_foundation::panic::RecoveryScope::boundary(|recovery| {
                    self.haptics.close(recovery);
                });
            });
        }
        // Gesture cancellation runs recognizer callbacks, so it follows every
        // withdrawal above: a rejected recognizer finds the presentation's
        // graph, keys, agent, focus and text input already closed.
        let mode = failure.mode();
        failure.invoke(|| close_gestures(&self.gestures, mode));
        self.retire_completion_scope(&mut failure);
        // Every presentation authority is revoked: the withdrawn keys retire,
        // one at a time, each under containment, so a failing destructor
        // stops none of the rest (ADR-0281 §3).
        for key in withdrawn_keys {
            failure.retire(key);
        }
        if let Some(dispatch) = dispatch {
            let mode = failure.mode();
            failure.invoke(|| flui_interaction::__runtime::retire_dispatch(dispatch, mode));
        }
        if let Some(bridge) = &bridge {
            failure.invoke(|| bridge.set_activation_listener(Arc::new(|_| {})));
            failure.invoke(|| bridge.set_action_listener(Arc::new(|_| {})));
        }
        if self.pipeline.is_free() {
            failure.invoke(|| {
                self.pipeline.with_mut(|owner| {
                    if owner.semantics_enabled() {
                        owner.set_semantics_enabled(false);
                    }
                });
            });
        }
        if let Some(window) = &window {
            failure.invoke(|| {
                let _ = window.set_cursor(&Cursor::Icon(CursorIcon::Default));
            });
        }

        self.finish_animation(&mut failure);
        if !failure.preserving() {
            failure.invoke(|| self.widgets.withdraw_root_owner(false));
            // What an earlier frame left undrained is not this close's.
            drop(self.widgets.take_recovered_panics());
            failure.run(|| self.widgets.detach_root_widget());
            // The tree contains each `dispose` that panics so the rest still
            // dispose; no frame follows to report it, so the first one is
            // this close's failure (ADR-0258 §2).
            failure.run(|| {
                if let Some(first) = self.widgets.take_recovered_panics().into_iter().next() {
                    let text = first.payload_text.map_or(first.error.message, String::from);
                    std::panic::resume_unwind(Box::new(text));
                }
            });
        }
        // Reached in preserving mode, or when the healthy disposal above
        // failed part way: what the tree still owns is not disposed; it is
        // destroyed with the binding, each destructor contained.
        if failure.preserving() {
            self.withdraw_retained_ownership(&mut failure, lane);
        }
        // The window and the accessibility bridge are framework-owned: they
        // are released even after a failure (ADR-0281 §3).
        failure.retire(bridge);
        failure.retire(window);

        self.lifecycle.set(PresentationLifecycle::Closed);
        failure.finish();
    }
    /// Withdraw this presentation's authority without running user code, as
    /// a UI runtime closing several presentations does for every one of them
    /// before any closes: dispatch targets, liveness, held input, the graph,
    /// rebuild and key authority, agent ports, focus and text input become
    /// unavailable, so a sibling's callbacks cannot drive this presentation
    /// (ADR-0123). The withdrawn key owners are returned for the caller to
    /// retire; the presentation's own close later retires everything else.
    pub(crate) fn withdraw_for_ui_runtime_close(
        &self,
        lane: &flui_interaction::pointer::InteractionLane,
    ) -> Vec<flui_view::__runtime::WithdrawnKey> {
        self.withdraw_animation();
        use flui_view::__runtime::BindingRuntime as _;
        if matches!(
            self.lifecycle.get(),
            PresentationLifecycle::Closing | PresentationLifecycle::Closed
        ) {
            return Vec::new();
        }
        self.pipeline.withdraw_layout();
        if let Some(scope) = &self.completion_scope {
            scope.withdraw();
        }
        if self.withdrawn_dispatch.borrow().is_none()
            && let Some(custody) = self.withdraw_interaction(Some(lane), self.close_mode.get())
        {
            self.withdrawn_dispatch.replace(Some(custody));
        }
        self.alive.borrow_mut().take();
        self.held_pointer_input.borrow_mut().clear();
        let keys = self.widgets.withdraw_owner_authority();
        if let Some(agent) = &*self.dev_agent.borrow() {
            agent.withdraw();
        }
        self.abandon_agent_jobs();
        flui_interaction::__runtime::withdraw_focus(&self.focus);
        flui_interaction::__runtime::withdraw_text_input(&self.text_input);
        keys
    }

    fn withdraw_animation(&self) {
        if self.animation_retirement.borrow().is_none() {
            let retirement = self.vsync_driver.prepare_close();
            self.animation_retirement.replace(Some(retirement));
        }
    }

    fn finish_animation(&self, failure: &mut PresentationCloseRecovery<'_>) {
        let retirement = self.animation_retirement.borrow_mut().take();
        if let Some(retirement) = retirement {
            if failure.preserving() {
                let mut recovery = flui_foundation::panic::PanicRecovery::new();
                recovery.inherit_failure();
                retirement.publish_with(&mut recovery.scope());
                failure.invoke(|| recovery.finish());
            } else {
                failure.invoke(|| retirement.finish());
            }
        }
    }

    /// Withdraws the root owner and closes every input owner in preserving
    /// mode, so the values they still hold are retained rather than dropped
    /// (ADR-0123, ADR-0127).
    fn withdraw_retained_ownership(
        &self,
        failure: &mut PresentationCloseRecovery<'_>,
        lane: Option<&flui_interaction::pointer::InteractionLane>,
    ) {
        use flui_interaction::__runtime::{
            CloseMode, close_focus, close_gestures, close_mouse_tracker, close_text_input,
        };
        failure.invoke(|| self.widgets.withdraw_root_owner(true));
        failure.invoke(|| close_gestures(&self.gestures, CloseMode::PreservingFailure));
        failure.invoke(|| {
            close_mouse_tracker(self.gestures.mouse_tracker(), CloseMode::PreservingFailure);
        });
        failure.invoke(|| close_focus(&self.focus, CloseMode::PreservingFailure));
        failure.invoke(|| close_text_input(&self.text_input, CloseMode::PreservingFailure));
        failure.invoke(|| self.close_interaction_with_mode(lane, CloseMode::PreservingFailure));
        self.retire_completion_scope(failure);
    }

    fn retire_completion_scope(&self, failure: &mut PresentationCloseRecovery<'_>) {
        if let Some(scope) = &self.completion_scope {
            if failure.preserving() {
                scope.close_preserving();
            } else {
                failure.invoke(|| {
                    if let Some(payload) = scope.close() {
                        std::panic::resume_unwind(payload);
                    }
                });
            }
        }
    }
    /// Answers every job an agent parked on this presentation when it closes before a frame
    /// could finish them: a reading an action owes is failed (the action ran), a wait is `gone`,
    /// and every event stream ends after a last `window` event.
    pub(crate) fn abandon_agent_jobs(&self) {
        use crate::ui_runtime::deliver_each;
        // Every stage runs to the end whatever an earlier one did: a reply goes to a receiver
        // whose drop or trace is user code, and a job left unanswered here is never answered.
        // The first panic resumes after all of them.
        let mut ended = FirstPanic::new();
        ended.contain(|| {
            self.end_agent_events();
        });
        let owed = std::mem::take(&mut *self.agent_reads.borrow_mut());
        let reads = deliver_each(owed, crate::ui_runtime::PendingActRead::abandon);
        let injected = self.agent_input.borrow_mut().abandon();
        let inputs = deliver_each(injected, crate::ui_runtime::AbandonedInput::answer);
        let waits = std::mem::take(&mut *self.agent_waits.borrow_mut());
        let waited = deliver_each(waits, crate::ui_runtime::PendingWait::abandon);
        let layers = self.abandon_agent_layer_reads();
        for stage in [reads, inputs, waited, layers] {
            ended.absorb(stage);
        }
        if let Some(payload) = ended.into_payload() {
            std::panic::resume_unwind(payload);
        }
    }
}

struct PresentationCloseRecovery<'a> {
    first: FirstPanic,
    terminal: &'a Cell<flui_interaction::__runtime::CloseMode>,
    window: flui_interaction::__runtime::CloseWindow,
}

impl<'a> PresentationCloseRecovery<'a> {
    fn new(
        mode: flui_interaction::__runtime::CloseMode,
        terminal: &'a Cell<flui_interaction::__runtime::CloseMode>,
        window: flui_interaction::__runtime::CloseWindow,
    ) -> Self {
        // An incoming unwind is a held failure, as in `flui-interaction`'s
        // close: optional callbacks are skipped, values are still destroyed.
        if mode == flui_interaction::__runtime::CloseMode::PreservingFailure
            || std::thread::panicking()
        {
            terminal.set(flui_interaction::__runtime::CloseMode::PreservingFailure);
        }
        if terminal.get() == flui_interaction::__runtime::CloseMode::PreservingFailure {
            window.preserve();
        }
        Self {
            first: FirstPanic::new(),
            terminal,
            window,
        }
    }
    fn mode(&self) -> flui_interaction::__runtime::CloseMode {
        self.terminal.get()
    }
    fn preserving(&self) -> bool {
        self.mode() == flui_interaction::__runtime::CloseMode::PreservingFailure
    }
    fn invoke(&mut self, callback: impl FnOnce()) {
        let _ = self.invoke_with(callback);
    }
    fn invoke_with<T>(&mut self, callback: impl FnOnce() -> T) -> Option<T> {
        // A close callback's panic is held so later callbacks still run.
        let mut caught = FirstPanic::new();
        let value = caught.contain(callback);
        if value.is_none() {
            if self.preserving() {
                drop(caught);
            } else {
                self.first.absorb(caught);
            }
            self.terminal
                .set(flui_interaction::__runtime::CloseMode::PreservingFailure);
            self.window.preserve();
        }
        value
    }
    fn run(&mut self, callback: impl FnOnce()) {
        if !self.preserving() {
            self.invoke(callback);
        }
    }
    /// Drops a withdrawn value under this close's containment, after a
    /// failure too: the first failure stays the one resumed (ADR-0281 §3).
    fn retire<T>(&mut self, value: T) {
        self.invoke(|| drop(value));
    }
    fn finish(self) {
        self.first.resume();
    }
}
