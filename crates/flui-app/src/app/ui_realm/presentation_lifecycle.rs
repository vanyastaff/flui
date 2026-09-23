//! Presentation observations and realm scheduler aggregation.
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use flui_foundation::PresentationId;
use flui_rendering::binding::RendererBinding as _;
use flui_scheduler::AppLifecycleState;

use super::UiRealm;
use crate::app::lifecycle_state::{
    derive_lifecycle_state, lifecycle_ladder, preserve_first_lifecycle_panic,
};

/// An observed Detached state is reversible; terminal lifetime is explicit.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HostLifecycle {
    Observed(AppLifecycleState),
    Stopping,
}

impl UiRealm {
    pub(crate) fn update_host_lifecycle(&self, state: AppLifecycleState) {
        if self.host_lifecycle.get() == HostLifecycle::Stopping {
            return;
        }
        self.host_lifecycle.set(HostLifecycle::Observed(state));
        self.reconcile_lifecycle(Vec::new());
    }

    #[cfg(any(test, not(any(target_arch = "wasm32", target_os = "android"))))]
    pub(crate) fn synchronize_window_snapshot(
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
        if !focused || !visible || execution != flui_platform::WindowExecutionState::Running {
            cancel.push(id);
        }
        self.set_presentation_hidden(id, !visible);
        self.reconcile_lifecycle(cancel);
    }

    pub(crate) fn update_window_execution(
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
        self.reconcile_lifecycle(
            if changed && state != flui_platform::WindowExecutionState::Running {
                vec![id]
            } else {
                Vec::new()
            },
        );
    }

    pub(crate) fn update_window_focus(&self, id: PresentationId, focused: bool) {
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

    pub(crate) fn update_window_visibility(&self, id: PresentationId, visible: bool) {
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

    pub(crate) fn synchronize_window_lifecycle(&self) {
        self.reconcile_lifecycle(Vec::new());
    }

    pub(crate) fn stop_presentations(&self) {
        self.host_lifecycle.set(HostLifecycle::Stopping);
        for presentation in self.presentations.iter() {
            presentation.closing_requested.set(true);
            presentation.widgets().lifecycle_source().begin_close();
            // PORT-CHECK-OK-LOCK: plain data: retained pointer events, no Drop
            presentation.held_pointer_input().borrow_mut().clear();
        }
        self.reconcile_lifecycle(Vec::new());
    }

    pub(crate) fn stop_presentation(&self, id: PresentationId) {
        if let Some(presentation) = self.presentations.get(id) {
            presentation.closing_requested.set(true);
            presentation.widgets().lifecycle_source().begin_close();
            // PORT-CHECK-OK-LOCK: plain data: retained pointer events, no Drop
            presentation.held_pointer_input().borrow_mut().clear();
            self.reconcile_lifecycle(Vec::new());
        }
    }

    fn execution_lifecycle(
        &self,
        presentation: &crate::app::presentation::PresentationState,
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

    fn reconcile_lifecycle(&self, mut cancel: Vec<PresentationId>) {
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
                let failure =
                    catch_unwind(AssertUnwindSafe(|| self.cancel_pointer_sequences_for(id))).err();
                preserve_first_lifecycle_panic(
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
                        == crate::app::presentation::PresentationLifecycle::Suspended;
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
                preserve_first_lifecycle_panic(
                    &mut first_panic,
                    failure,
                    "realm lifecycle scheduler",
                );
            }
            for presentation in restored {
                let failure = catch_unwind(AssertUnwindSafe(|| {
                    crate::bindings::redirty_pipeline_root(
                        presentation.renderer().root_pipeline_owner(),
                    );
                    self.request_redraw_for(presentation);
                    self.wake_frame();
                }))
                .err();
                preserve_first_lifecycle_panic(
                    &mut first_panic,
                    failure,
                    "presentation redraw restoration",
                );
            }
            for (presentation, _, step) in changed {
                let failure = catch_unwind(AssertUnwindSafe(|| {
                    presentation.widgets().notify_committed_lifecycle(step);
                }))
                .err();
                preserve_first_lifecycle_panic(
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
            let failure = catch_unwind(AssertUnwindSafe(|| {
                presentation.widgets().lifecycle_source().finish_close();
            }))
            .err();
            preserve_first_lifecycle_panic(
                &mut first_panic,
                failure,
                "lifecycle subscription close",
            );
            let failure = catch_unwind(AssertUnwindSafe(|| presentation.close())).err();
            preserve_first_lifecycle_panic(
                &mut first_panic,
                failure,
                "terminal presentation cleanup",
            );
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{presentation::PresentationLifecycle, window_test_support::TestWindow};
    use flui_view::WidgetsBindingObserver;
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
        sync::{Arc, atomic::AtomicBool},
    };

    #[derive(Clone)]
    struct SubscriptionView {
        handle: Rc<RefCell<Option<flui_view::LifecycleHandle>>>,
        events: Rc<RefCell<Vec<AppLifecycleState>>>,
        disposed: Rc<Cell<bool>>,
    }
    struct SubscriptionState {
        view: SubscriptionView,
        token: Option<flui_view::LifecycleSubscription>,
    }
    impl flui_view::StatefulView for SubscriptionView {
        type State = SubscriptionState;
        fn create_state(&self) -> Self::State {
            SubscriptionState {
                view: self.clone(),
                token: None,
            }
        }
    }
    impl flui_view::View for SubscriptionView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    impl flui_view::ViewState<SubscriptionView> for SubscriptionState {
        fn init_state(&mut self, context: &dyn flui_view::LifecycleContext) {
            let handle = context
                .lifecycle_handle()
                .expect("app presentation capability");
            let events = Rc::clone(&self.view.events);
            let (_, token) = handle
                .subscribe(move |state| events.borrow_mut().push(state))
                .expect("open presentation");
            self.token = Some(token);
            self.view.handle.replace(Some(handle));
        }
        fn build(
            &self,
            _: &SubscriptionView,
            _: &dyn flui_view::BuildContext,
        ) -> impl flui_view::IntoView {
            flui_widgets::SizedBox::new(10.0, 10.0)
        }
        fn dispose(&mut self) {
            assert_eq!(
                self.view.events.borrow().last(),
                Some(&AppLifecycleState::Detached)
            );
            assert_eq!(
                self.view
                    .handle
                    .borrow()
                    .as_ref()
                    .expect("handle")
                    .snapshot(),
                Err(flui_view::LifecycleClosed)
            );
            self.view.disposed.set(true);
        }
    }
    fn subscription_view() -> SubscriptionView {
        SubscriptionView {
            handle: Rc::default(),
            events: Rc::default(),
            disposed: Rc::default(),
        }
    }

    #[test]
    fn lifecycle_subscription_app_init_state_is_local_and_terminal_precedes_dispose_despite_legacy_panic()
     {
        struct Legacy;
        impl WidgetsBindingObserver for Legacy {
            fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
                assert!(state != AppLifecycleState::Detached, "legacy terminal");
            }
        }
        let (realm, a, b) = two_presentations();
        let views = [subscription_view(), subscription_view()];
        for (id, view) in [a, b].into_iter().zip(&views) {
            realm
                .attach_root_widget_to_for_test(id, view)
                .expect("mount captures without locking recursively");
            realm
                .presentations
                .get(id)
                .expect("presentation")
                .widgets()
                .add_observer(Arc::new(Legacy));
        }
        let mut backend = crate::app::raster_test_support::TestRasterBackend::always_presents();
        realm.enter(|realm| {
            realm.render_frame_entered(&mut backend);
        });
        assert!(
            views.iter().all(|view| view.handle.borrow().is_some()),
            "first frame ran init_state for both presentations"
        );
        realm.update_window_visibility(b, false);
        assert_eq!(
            realm.scheduler().lifecycle_state(),
            AppLifecycleState::Resumed
        );
        assert!(views[0].events.borrow().is_empty());
        assert_eq!(
            views[1].events.borrow().last(),
            Some(&AppLifecycleState::Hidden)
        );
        let failure = catch_unwind(AssertUnwindSafe(|| {
            realm.enter(UiRealm::stop_presentations);
        }))
        .expect_err("legacy panic");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"legacy terminal"));
        for view in views {
            assert!(view.disposed.get());
        }
    }

    #[test]
    fn lifecycle_subscription_direct_drop_preserves_outer_unwind_and_invalidates_handle() {
        let retained = Rc::new(RefCell::new(None));
        let output = Rc::clone(&retained);
        let outer = catch_unwind(AssertUnwindSafe(move || {
            let realm = UiRealm::for_test();
            let source = realm.presentations.primary().widgets().lifecycle_source();
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

    fn two_presentations() -> (UiRealm, PresentationId, PresentationId) {
        let mut realm = UiRealm::for_test();
        let a = realm.presentation_id();
        let b = realm.install_second_presentation_for_test();
        realm.synchronize_window_lifecycle();
        (realm, a, b)
    }

    #[test]
    #[expect(
        clippy::arc_with_non_send_sync,
        reason = "owner-local binding observer"
    )]
    fn window_lifecycle_disabled_targets_never_transiently_enable_execution() {
        // Scheduler callbacks are Send + Sync, but delivery is synchronous on
        // this test's owner thread. A weak thread-local probe avoids transferring
        // the owner-local realm across threads or retaining it through callbacks.
        thread_local! {
            static CALLBACK_REALM: RefCell<Weak<UiRealm>> = const { RefCell::new(Weak::new()) };
        }
        struct ProbeReset(Weak<UiRealm>);
        impl Drop for ProbeReset {
            fn drop(&mut self) {
                CALLBACK_REALM.with(|probe| probe.replace(std::mem::take(&mut self.0)));
            }
        }
        type Observation = (
            AppLifecycleState,
            AppLifecycleState,
            bool,
            PresentationLifecycle,
        );
        struct History {
            realm: Weak<UiRealm>,
            id: PresentationId,
            observed: Rc<RefCell<Vec<Observation>>>,
        }
        impl WidgetsBindingObserver for History {
            fn did_change_app_lifecycle_state(&self, local: AppLifecycleState) {
                let realm = self.realm.upgrade().expect("live realm");
                self.observed.borrow_mut().push((
                    local,
                    realm.scheduler().lifecycle_state(),
                    realm.scheduler().frames_enabled(),
                    realm
                        .presentations
                        .get(self.id)
                        .expect("presentation")
                        .lifecycle(),
                ));
            }
        }
        for (host, known_detached) in [AppLifecycleState::Paused, AppLifecycleState::Hidden]
            .into_iter()
            .flat_map(|host| [(host, false), (host, true)])
        {
            let mut realm = UiRealm::for_test();
            realm.synchronize_window_lifecycle();
            realm.update_host_lifecycle(host);
            let id = realm.install_second_presentation_for_test();
            if known_detached {
                realm.update_host_lifecycle(AppLifecycleState::Detached);
            }
            let realm = Rc::new(realm);
            let local = Rc::new(RefCell::new(Vec::new()));
            let scheduler = Arc::new(std::sync::Mutex::new(Vec::new()));
            let events = Arc::clone(&scheduler);
            let _probe_reset =
                ProbeReset(CALLBACK_REALM.with(|probe| probe.replace(Rc::downgrade(&realm))));
            realm
                .scheduler()
                .add_lifecycle_state_listener(Arc::new(move |state| {
                    CALLBACK_REALM.with(|probe| {
                        let realm = probe.borrow().upgrade().expect("callback owner");
                        assert!(!realm.scheduler().frames_enabled());
                        assert!(
                            realm
                                .presentations
                                .iter()
                                .all(|presentation| presentation.lifecycle()
                                    == PresentationLifecycle::Suspended),
                            "scheduler callbacks must see committed resource eligibility"
                        );
                    });
                    events.lock().expect("history").push(state);
                }));
            realm
                .presentations
                .get(id)
                .expect("secondary")
                .widgets()
                .add_observer(Arc::new(History {
                    realm: Rc::downgrade(&realm),
                    id,
                    observed: Rc::clone(&local),
                }));
            if known_detached {
                realm.update_host_lifecycle(host);
            } else {
                realm.synchronize_window_lifecycle();
            }
            let expected_local = if known_detached {
                lifecycle_ladder(AppLifecycleState::Detached, host)
            } else {
                vec![host]
            };
            assert_eq!(
                local
                    .borrow()
                    .iter()
                    .map(|event| event.0)
                    .collect::<Vec<_>>(),
                expected_local
            );
            assert!(
                local
                    .borrow()
                    .iter()
                    .all(|(_, state, enabled, resource)| *state == host
                        && !enabled
                        && *resource == PresentationLifecycle::Suspended),
                "local callback history: {:?}",
                local.borrow()
            );
            assert_eq!(
                *scheduler.lock().expect("history"),
                if known_detached { vec![host] } else { vec![] }
            );
        }
        let realm = UiRealm::new(
            Arc::new(|| {}),
            Arc::new(TestWindow::new().visible(false)),
            1.0,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("hidden realm");
        let history = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&history);
        realm
            .scheduler()
            .add_lifecycle_state_listener(Arc::new(move |state| {
                observed.lock().expect("history").push(state);
            }));
        realm.synchronize_window_lifecycle();
        assert_eq!(
            *history.lock().expect("history"),
            vec![AppLifecycleState::Hidden]
        );
    }

    #[test]
    fn window_lifecycle_initial_hidden_and_unfocused_snapshots_are_authoritative() {
        for (visible, focused, expected) in [
            (false, true, AppLifecycleState::Hidden),
            (true, false, AppLifecycleState::Inactive),
        ] {
            let window = Arc::new(TestWindow::new().visible(visible).focused(focused));
            let realm = UiRealm::new(
                Arc::new(|| {}),
                window,
                1.0,
                Arc::new(AtomicBool::new(false)),
            )
            .expect("realm");
            realm.synchronize_window_lifecycle();
            assert_eq!(realm.scheduler().lifecycle_state(), expected);
            assert_eq!(realm.presentations.primary().clock().is_hidden(), !visible);
            assert_eq!(
                realm.presentations.primary().lifecycle(),
                if visible {
                    PresentationLifecycle::SurfaceAttached
                } else {
                    PresentationLifecycle::Suspended
                }
            );
        }
    }

    #[test]
    fn window_lifecycle_focus_orders_and_suspension_preserve_observed_facts() {
        for gain_first in [false, true] {
            let (realm, a, b) = two_presentations();
            if gain_first {
                realm.update_window_focus(b, true);
                realm.update_window_focus(a, false);
            } else {
                realm.update_window_focus(a, false);
                realm.update_window_focus(b, true);
            }
            assert_eq!(
                realm.scheduler().lifecycle_state(),
                AppLifecycleState::Resumed
            );
            assert!(!realm.presentations.get(a).expect("A").window_focused.get());
            assert!(realm.presentations.get(b).expect("B").window_focused.get());
            realm.update_host_lifecycle(AppLifecycleState::Paused);
            realm.update_window_visibility(a, false);
            realm.update_window_focus(b, false);
            realm.update_window_visibility(b, true);
            assert_eq!(
                realm.scheduler().lifecycle_state(),
                AppLifecycleState::Paused
            );
            for presentation in realm.presentations.iter() {
                assert_eq!(
                    presentation.widgets().lifecycle_source().current(),
                    Some(AppLifecycleState::Paused)
                );
                assert_eq!(presentation.lifecycle(), PresentationLifecycle::Suspended);
            }
            realm.update_host_lifecycle(AppLifecycleState::Resumed);
            assert_eq!(
                realm.scheduler().lifecycle_state(),
                AppLifecycleState::Inactive
            );
            assert_eq!(
                realm
                    .presentations
                    .get(a)
                    .expect("A")
                    .widgets()
                    .lifecycle_source()
                    .current(),
                Some(AppLifecycleState::Hidden)
            );
            assert_eq!(
                realm
                    .presentations
                    .get(b)
                    .expect("B")
                    .widgets()
                    .lifecycle_source()
                    .current(),
                Some(AppLifecycleState::Inactive)
            );
        }
    }

    #[test]
    fn window_lifecycle_close_recomputes_without_inventing_focus() {
        let (mut realm, a, b) = two_presentations();
        realm.update_window_focus(b, true);
        assert!(realm.close_presentation_entered(b));
        assert_eq!(
            realm.scheduler().lifecycle_state(),
            AppLifecycleState::Inactive
        );
        assert!(
            !realm
                .presentations
                .get(a)
                .expect("survivor")
                .window_focused
                .get()
        );
        let b = realm.install_second_presentation_for_test();
        realm.synchronize_window_lifecycle();
        realm.update_window_visibility(a, false);
        assert!(realm.close_presentation_entered(b));
        assert_eq!(
            realm.scheduler().lifecycle_state(),
            AppLifecycleState::Hidden
        );
    }

    #[test]
    fn window_execution_suspension_cancels_real_pointer_before_public_notification() {
        let (realm, _a, b) = two_presentations();
        realm
            .attach_root_widget_to_for_test(b, &flui_widgets::SizedBox::new(10.0, 10.0))
            .expect("root");
        let realm = Rc::new(realm);
        realm.enter(|realm| {
            realm.handle_input_addressed(
                b,
                flui_platform::traits::PlatformInput::Pointer(
                    flui_interaction::events::make_down_event(
                        flui_types::Offset::new(flui_types::Pixels(1.0), flui_types::Pixels(1.0)),
                        flui_interaction::events::PointerType::Mouse,
                    ),
                ),
            );
        });
        assert_eq!(
            realm
                .presentations
                .get(b)
                .expect("B")
                .gestures()
                .active_pointer_count(),
            1
        );
        let handle = realm
            .presentations
            .get(b)
            .expect("B")
            .widgets()
            .lifecycle_source()
            .handle();
        let weak = Rc::downgrade(&realm);
        let history = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&history);
        let (_, _subscription) = handle
            .subscribe(move |state| {
                let realm = weak.upgrade().expect("live realm");
                assert_eq!(
                    realm
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
        realm.update_window_execution(b, flui_platform::WindowExecutionState::Suspended);
        assert_eq!(
            handle.snapshot().expect("live"),
            Some(AppLifecycleState::Paused)
        );
        assert_eq!(history.borrow().last(), Some(&AppLifecycleState::Paused));
        assert!(
            realm.scheduler().frames_enabled(),
            "visible sibling remains eligible"
        );
    }

    struct Observer {
        realm: Weak<UiRealm>,
        id: PresentationId,
        seen: Rc<RefCell<Vec<AppLifecycleState>>>,
    }
    impl WidgetsBindingObserver for Observer {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            let realm = self
                .realm
                .upgrade()
                .expect("realm lives through notification");
            let presentation = realm
                .presentations
                .get(self.id)
                .expect("not removed before notification");
            assert_eq!(
                presentation.widgets().lifecycle_source().current(),
                Some(state)
            );
            assert_eq!(
                realm.scheduler().lifecycle_state(),
                realm.aggregate_lifecycle()
            );
            if !matches!(state, AppLifecycleState::Resumed) {
                assert_eq!(
                    presentation.gestures().active_pointer_count(),
                    0,
                    "input cleanup precedes lifecycle observers"
                );
            }
            if state == AppLifecycleState::Detached {
                assert!(
                    presentation.widgets().root_element().is_some(),
                    "Detached precedes disposal"
                );
                assert_ne!(presentation.lifecycle(), PresentationLifecycle::Closed);
            }
            self.seen.borrow_mut().push(state);
        }
    }

    #[test]
    #[expect(
        clippy::arc_with_non_send_sync,
        reason = "WidgetsBinding stores Arc observers; these callbacks and their realm are owner-local"
    )]
    fn window_lifecycle_local_observers_read_committed_state_and_detached_before_dispose() {
        let (realm, a, b) = two_presentations();
        for id in [a, b] {
            realm
                .attach_root_widget_to_for_test(id, &flui_widgets::SizedBox::new(10.0, 10.0))
                .expect("root");
        }
        let realm = Rc::new(realm);
        let mut logs = Vec::new();
        for id in [a, b] {
            let seen = Rc::new(RefCell::new(Vec::new()));
            realm
                .presentations
                .get(id)
                .expect("presentation")
                .widgets()
                .add_observer(Arc::new(Observer {
                    realm: Rc::downgrade(&realm),
                    id,
                    seen: Rc::clone(&seen),
                }));
            logs.push(seen);
        }
        realm.enter(|realm| {
            realm.handle_input_addressed(
                b,
                flui_platform::traits::PlatformInput::Pointer(
                    flui_interaction::events::make_down_event(
                        flui_types::Offset::new(flui_types::Pixels(1.0), flui_types::Pixels(1.0)),
                        flui_interaction::events::PointerType::Mouse,
                    ),
                ),
            );
        });
        assert_eq!(
            realm
                .presentations
                .get(b)
                .expect("B")
                .gestures()
                .active_pointer_count(),
            1
        );
        realm.update_window_visibility(b, false);
        assert_eq!(
            realm.scheduler().lifecycle_state(),
            AppLifecycleState::Resumed
        );
        assert!(logs[0].borrow().is_empty(), "hiding B must not notify A");
        assert_eq!(logs[1].borrow().last(), Some(&AppLifecycleState::Hidden));
        realm.enter(UiRealm::stop_presentations);
        for (id, seen) in [a, b].into_iter().zip(logs) {
            assert_eq!(
                seen.borrow()
                    .iter()
                    .filter(|state| **state == AppLifecycleState::Detached)
                    .count(),
                1
            );
            assert!(
                realm
                    .presentations
                    .get(id)
                    .expect("owned until realm teardown")
                    .widgets()
                    .root_element()
                    .is_none()
            );
        }
        realm.update_host_lifecycle(AppLifecycleState::Resumed);
        realm.update_window_visibility(a, true);
        assert_eq!(
            realm.scheduler().lifecycle_state(),
            AppLifecycleState::Detached
        );
    }

    #[test]
    fn window_lifecycle_nonterminal_detached_can_resume_without_disposing_the_tree() {
        let realm = UiRealm::for_test();
        realm
            .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
            .expect("root");
        realm.synchronize_window_lifecycle();
        realm.update_host_lifecycle(AppLifecycleState::Detached);
        assert_eq!(
            realm.presentations.primary().lifecycle(),
            PresentationLifecycle::Suspended
        );
        assert!(realm.widgets().root_element().is_some());
        realm.update_host_lifecycle(AppLifecycleState::Resumed);
        assert_eq!(
            realm.presentations.primary().lifecycle(),
            PresentationLifecycle::SurfaceAttached
        );
        assert!(realm.widgets().root_element().is_some());
        assert!(realm.scheduler().frames_enabled());
    }

    #[test]
    fn window_lifecycle_unhiding_secondary_redirties_and_produces_while_primary_hidden() {
        let (realm, a, b) = two_presentations();
        for id in [a, b] {
            realm
                .attach_root_widget_to_for_test(id, &flui_widgets::SizedBox::new(10.0, 10.0))
                .expect("root");
        }
        let mut backend = crate::app::raster_test_support::TestRasterBackend::always_presents();
        realm.enter(|realm| {
            realm.render_frame_entered(&mut backend);
        });
        let before = realm
            .presentations
            .get(b)
            .expect("B")
            .clock()
            .produced_count();
        realm.update_window_visibility(b, false);
        assert!(realm.scheduler().frames_enabled());
        realm.update_window_visibility(b, true);
        assert_eq!(
            realm.scheduler().lifecycle_state(),
            AppLifecycleState::Resumed
        );
        realm.enter(|realm| {
            realm.render_frame_entered(&mut backend);
        });
        assert!(
            realm
                .presentations
                .get(b)
                .expect("B")
                .clock()
                .produced_count()
                > before,
            "addressed restoration redraws even when aggregate lifecycle is unchanged"
        );
        realm.update_window_visibility(a, false);
        realm.update_window_visibility(b, false);
        assert!(!realm.scheduler().frames_enabled());
        let a_before = realm
            .presentations
            .get(a)
            .expect("A")
            .clock()
            .produced_count();
        let b_before = realm
            .presentations
            .get(b)
            .expect("B")
            .clock()
            .produced_count();
        realm.update_window_visibility(b, true);
        assert!(realm.scheduler().frames_enabled());
        realm.enter(|realm| {
            realm.render_frame_entered(&mut backend);
        });
        assert_eq!(
            realm
                .presentations
                .get(a)
                .expect("A")
                .clock()
                .produced_count(),
            a_before
        );
        assert!(
            realm
                .presentations
                .get(b)
                .expect("B")
                .clock()
                .produced_count()
                > b_before,
            "restored secondary must execute a real segment"
        );
    }
}
