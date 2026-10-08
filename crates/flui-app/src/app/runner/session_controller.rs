//! Session ownership shared by UIKit's adapter and host lifecycle tests.
use super::host::APP_RUNTIME;
use super::installed_host::Installation;
use super::owner_dispatch::{PresentationDispatcher, close_this_window};
use crate::app::hot_reload::WorkerWatcherGuard;
use flui_platform::HostWindow;
use flui_platform::shared::window_installation::WindowInstallation;
use flui_runtime::dev_agent::DevAgentAttachment;
use std::{collections::HashMap, hash::Hash, sync::Arc};

pub(super) fn contain(body: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        std::mem::forget(payload);
    }
}

type SessionInstaller = Box<dyn FnMut(Arc<dyn HostWindow>) -> anyhow::Result<Installation>>;

enum SessionState {
    Installing {
        receipt: Installation,
        acknowledgement: WindowInstallation,
        window: Arc<dyn HostWindow>,
    },
    Ready {
        dispatcher: PresentationDispatcher,
        window: std::sync::Weak<dyn HostWindow>,
    },
}

fn acknowledge(installation: WindowInstallation) {
    if let Err(error) = installation.complete() {
        tracing::warn!(%error, "scene is installed; native publication awaits another owner opportunity");
    }
}

fn retire_session(state: SessionState) {
    match state {
        SessionState::Ready { dispatcher, .. } => contain(|| close_this_window(dispatcher)),
        SessionState::Installing {
            receipt,
            acknowledgement,
            window,
        } => {
            receipt.cancel();
            contain(|| drop(acknowledgement));
            contain(|| window.close());
        }
    }
}

pub(in crate::app) struct SessionController<K: Eq + Hash> {
    installer: Option<SessionInstaller>,
    sessions: HashMap<K, SessionState>,
    watcher: Option<WorkerWatcherGuard>,
    /// The loop's development agent attachment; dropping it detaches the
    /// hook.
    agent: Option<DevAgentAttachment>,
}
impl<K: Eq + Hash + Clone> SessionController<K> {
    pub(super) fn new(
        installer: impl FnMut(Arc<dyn HostWindow>) -> anyhow::Result<Installation> + 'static,
        watcher: Option<WorkerWatcherGuard>,
        agent: Option<DevAgentAttachment>,
    ) -> Self {
        Self {
            installer: Some(Box::new(installer)),
            sessions: HashMap::new(),
            watcher,
            agent,
        }
    }

    pub(super) fn connect(
        &mut self,
        key: K,
        window: Arc<dyn HostWindow>,
        reconnect: bool,
        acknowledgement: WindowInstallation,
    ) -> anyhow::Result<Option<PresentationDispatcher>> {
        if let Some(state) = self.sessions.get_mut(&key) {
            match state {
                SessionState::Ready {
                    dispatcher,
                    window: retained,
                } => {
                    if !std::sync::Weak::ptr_eq(retained, &Arc::downgrade(&window)) {
                        anyhow::bail!("session reconnect changed its logical window");
                    }
                    let dispatcher = *dispatcher;
                    if !APP_RUNTIME.with(|slot| {
                        slot.borrow()
                            .installed_host
                            .native()
                            .contains_address(dispatcher.address)
                    }) {
                        anyhow::bail!("reconnected session runtime has retired");
                    }
                    acknowledge(acknowledgement);
                    return Ok(Some(dispatcher));
                }
                SessionState::Installing {
                    acknowledgement: previous,
                    window: retained,
                    ..
                } => {
                    if !Arc::ptr_eq(retained, &window) {
                        anyhow::bail!("pending session reconnect changed its logical window");
                    }
                    let previous = std::mem::replace(previous, acknowledgement);
                    contain(|| drop(previous));
                    self.poll();
                    return Ok(self.ready(&key));
                }
            }
        }
        if reconnect {
            anyhow::bail!("reconnected session has no retained ui_runtime");
        }
        let receipt =
            (self
                .installer
                .as_mut()
                .expect("BUG: live controller owns installer"))(Arc::clone(&window))?;
        if let Some(Err(error)) = receipt.outcome() {
            contain(|| window.close());
            return Err(error.into());
        }
        self.sessions.insert(
            key.clone(),
            SessionState::Installing {
                receipt,
                acknowledgement,
                window,
            },
        );
        self.poll();
        Ok(self.ready(&key))
    }

    fn ready(&self, key: &K) -> Option<PresentationDispatcher> {
        match self.sessions.get(key) {
            Some(SessionState::Ready { dispatcher, .. }) => Some(*dispatcher),
            _ => None,
        }
    }

    pub(super) fn poll(&mut self) {
        let completed: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(key, state)| {
                let SessionState::Installing { receipt, .. } = state else {
                    return None;
                };
                receipt.outcome().map(|outcome| (key.clone(), outcome))
            })
            .collect();
        for (key, outcome) in completed {
            let Some(SessionState::Installing {
                receipt,
                acknowledgement,
                window,
            }) = self.sessions.remove(&key)
            else {
                continue;
            };
            let live = APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .installed_host
                    .native()
                    .contains_address(receipt.address)
            });
            if outcome.is_ok() && live {
                let dispatcher = PresentationDispatcher {
                    owner_thread: std::thread::current().id(),
                    address: receipt.address,
                };
                self.sessions.insert(
                    key,
                    SessionState::Ready {
                        dispatcher,
                        window: Arc::downgrade(&window),
                    },
                );
                acknowledge(acknowledgement);
            } else {
                contain(|| drop(acknowledgement));
                contain(|| window.close());
                if let Err(error) = outcome {
                    contain(|| tracing::error!(%error, "scene runtime installation failed"));
                }
            }
        }
    }

    pub(super) fn discard(&mut self, key: &K) {
        if let Some(state) = self.sessions.remove(key) {
            retire_session(state);
        }
    }

    pub(super) fn dispatchers(&self) -> Vec<PresentationDispatcher> {
        self.sessions
            .values()
            .filter_map(|state| match state {
                SessionState::Ready { dispatcher, .. } => Some(*dispatcher),
                SessionState::Installing { .. } => None,
            })
            .collect()
    }

    pub(super) fn has_pending(&self) -> bool {
        self.sessions
            .values()
            .any(|state| matches!(state, SessionState::Installing { .. }))
    }
}
impl<K: Eq + Hash> Drop for SessionController<K> {
    fn drop(&mut self) {
        for (key, state) in std::mem::take(&mut self.sessions) {
            contain(|| drop(key));
            retire_session(state);
        }
        let installer = self.installer.take();
        let watcher = self.watcher.take();
        let agent = self.agent.take();
        contain(|| drop(installer));
        contain(|| drop(watcher));
        contain(|| drop(agent));
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        host::APP_RUNTIME,
        owner_dispatch::{prepare_ui_runtime_alongside, teardown_platform_ui_runtime},
    };
    use super::*;
    use flui_platform::Platform;
    use flui_view::{StatefulView, View, ViewState};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[derive(Clone)]
    struct Probe {
        states: Rc<RefCell<Vec<Rc<Cell<usize>>>>>,
        disposed: Rc<Cell<usize>>,
        model: Rc<Cell<usize>>,
        history: Rc<RefCell<Vec<String>>>,
    }
    struct State {
        local: Rc<Cell<usize>>,
        disposed: Rc<Cell<usize>>,
        history: Rc<RefCell<Vec<String>>>,
        subscription: Option<flui_view::LifecycleSubscription>,
    }
    impl StatefulView for Probe {
        type State = State;
        fn create_state(&self) -> State {
            let local = Rc::new(Cell::new(7));
            self.states.borrow_mut().push(local.clone());
            State {
                local,
                disposed: self.disposed.clone(),
                history: self.history.clone(),
                subscription: None,
            }
        }
    }
    impl View for Probe {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    impl ViewState<Probe> for State {
        fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
            let history = self.history.clone();
            let (_, subscription) = ctx
                .lifecycle_handle()
                .expect("mounted source")
                .subscribe(move |state| history.borrow_mut().push(format!("{state:?}")))
                .expect("live source");
            self.subscription = Some(subscription);
        }
        fn build(&self, view: &Probe, _: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
            view.model.set(view.model.get() + 1);
            flui_widgets::SizedBox::new(self.local.get() as f64, 20.0)
        }
        fn dispose(&mut self) {
            self.disposed.set(self.disposed.get() + 1);
            self.history.borrow_mut().push("disposed".into());
        }
    }
    fn reconnect_retains_tree_discard_creates_fresh_state_without_restarting_service() {
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                teardown_platform_ui_runtime();
            }
        }
        let _cleanup = Cleanup;
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            runtime.reopen_lifecycles();
            runtime.ensure_execution();
        });
        let services = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let starts = services.clone();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let service_cancelled = cancelled.clone();
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .start_service(&crate::ServiceDefinition::new(
                    "session-test",
                    crate::ServiceLifetime::KeepsAppAlive,
                    move |context| {
                        starts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let cancelled = service_cancelled.clone();
                        Box::pin(async move {
                            context.cancellation().cancelled().await;
                            cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
                        })
                    },
                ))
                .expect("service started once by process");
        });
        let probe = Probe {
            states: Rc::default(),
            disposed: Rc::new(Cell::new(0)),
            model: Rc::new(Cell::new(0)),
            history: Rc::default(),
        };
        let root = probe.clone();
        let mut controller = SessionController::new(
            move |window| {
                let ui_runtime = crate::app::ui_runtime::UiRuntime::for_test();
                ui_runtime
                    .enter(|ui_runtime| ui_runtime.attach_root_widget(&root))
                    .expect("root mounted");
                let _ = ui_runtime.draw_frame(flui_rendering::constraints::BoxConstraints::tight(
                    flui_foundation::geometry::Size::new(80.0, 80.0),
                ));
                let window: Arc<dyn flui_platform::PlatformWindow> = window;
                Ok(prepare_ui_runtime_alongside(ui_runtime, window).submit())
            },
            None,
            None,
        );
        let saved = Rc::new(RefCell::new(None));
        let installed = Rc::clone(&saved);
        Box::new(flui_platform::HeadlessPlatform::new())
            .run(Box::new(move |owner| {
                *installed.borrow_mut() = Some(owner);
                Ok(())
            }))
            .expect("headless owner");
        let platform = saved.borrow_mut().take().expect("owner");
        let acknowledgement = || WindowInstallation::channel(platform.proxy()).0;
        let first = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("first native owner")
            .try_ready()
            .expect("ready window");
        let first_dispatcher = controller
            .connect("first", first.clone(), false, acknowledgement())
            .expect("first install")
            .expect("initial installation completes inline");
        probe.states.borrow()[0].set(42);
        for native_acknowledged in [false, true] {
            let reconnected = controller
                .connect(
                    "first",
                    Arc::clone(&first),
                    native_acknowledged,
                    acknowledgement(),
                )
                .expect("retained reconnect")
                .expect("retained ready session");
            assert_eq!(first_dispatcher.address, reconnected.address);
        }
        assert_eq!(controller.dispatchers().len(), 1);
        assert_eq!(probe.states.borrow().len(), 1);
        assert_eq!(probe.states.borrow()[0].get(), 42);
        assert_eq!(probe.disposed.get(), 0);
        controller.discard(&"first");
        controller.discard(&"first");
        assert_eq!(probe.disposed.get(), 1);
        assert!(
            !cancelled.load(std::sync::atomic::Ordering::SeqCst),
            "zero sessions must not stop the process service"
        );
        assert!(
            probe
                .history
                .borrow()
                .windows(2)
                .any(|pair| pair == ["Detached", "disposed"]),
            "terminal lifecycle precedes ViewState disposal"
        );
        let old_model = probe.model.get();
        let second = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("new native owner")
            .try_ready()
            .expect("ready window");
        let fresh = controller
            .connect("second", second, false, acknowledgement())
            .expect("fresh install")
            .expect("fresh ready session");
        assert_ne!(
            fresh.address.ui_runtime_id,
            first_dispatcher.address.ui_runtime_id
        );
        assert_eq!(probe.states.borrow().len(), 2);
        assert_eq!(probe.states.borrow()[1].get(), 7);
        assert!(
            probe.model.get() > old_model,
            "same application-owned model survives"
        );
        assert_eq!(services.load(std::sync::atomic::Ordering::SeqCst), 1);
        controller.discard(&"second");
        assert_eq!(probe.disposed.get(), 2);
        assert!(controller.dispatchers().is_empty());
        teardown_platform_ui_runtime();
        assert!(
            cancelled.load(std::sync::atomic::Ordering::SeqCst),
            "process shutdown joins the service"
        );
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum PendingCase {
        Complete,
        Reconnect,
        Discard,
    }

    fn pending_session_acknowledges_only_completed_publication() {
        pending_case(PendingCase::Complete);
    }
    fn pending_reconnect_replaces_native_acknowledgement_without_rebuilding() {
        pending_case(PendingCase::Reconnect);
    }
    fn pending_discard_cannot_publish_late() {
        pending_case(PendingCase::Discard);
    }

    fn abandoned_receipt_cannot_publish_without_a_native_close() {
        use super::super::owner_dispatch::{
            RuntimeTask, dispatch_platform_ui_runtime, install_platform_ui_runtime,
        };
        let _clear = super::super::host::OwnerHostClearGuard::arm();
        Box::new(flui_platform::HeadlessPlatform::new()).run(Box::new(move |owner| {
            let window = owner.open_window(flui_platform::WindowOptions::default())?.try_ready()?;
            let retained_window = Arc::clone(&window);
            let outer = install_platform_ui_runtime(crate::app::ui_runtime::UiRuntime::for_test(), &crate::app::window_test_support::headless_test_window());
            let address = Rc::new(Cell::new(None));
            let admitted = Rc::clone(&address);
            dispatch_platform_ui_runtime(outer, RuntimeTask::TestCallback(Box::new(move |_| {
                let receipt = prepare_ui_runtime_alongside(crate::app::ui_runtime::UiRuntime::for_test(), window).submit();
                assert!(receipt.outcome().is_none(), "publication waits for the current owner callback");
                admitted.set(Some(receipt.address));
                drop(receipt);
            }))).expect("owner drains abandoned installation");
            let registered = APP_RUNTIME.with(|slot| slot.borrow().installed_host.native().contains_address(address.get().expect("admitted address")));
            assert!(!registered, "dropping pending readiness authority cancels publication without a native close callback");
            retained_window.close();
            Ok(())
        })).expect("headless receipt cancellation host");
    }

    fn pending_case(case: PendingCase) {
        use super::super::owner_dispatch::{
            RuntimeTask, dispatch_platform_ui_runtime, install_platform_ui_runtime,
        };
        let _clear = super::super::host::OwnerHostClearGuard::arm();
        Box::new(flui_platform::HeadlessPlatform::new())
            .run(Box::new(move |owner| {
                let window = owner
                    .open_window(flui_platform::WindowOptions::default())?
                    .try_ready()?;
                let outer = install_platform_ui_runtime(
                    crate::app::ui_runtime::UiRuntime::for_test(),
                    &crate::app::window_test_support::headless_test_window(),
                );
                let built = Rc::new(Cell::new(0));
                let builds = Rc::clone(&built);
                let address = Rc::new(Cell::new(None));
                let constructed = Rc::clone(&address);
                let controller = Rc::new(RefCell::new(SessionController::new(
                    move |window| {
                        builds.set(builds.get() + 1);
                        let runtime = crate::app::ui_runtime::UiRuntime::for_test();
                        runtime.attach_root_widget(&flui_widgets::SizedBox::new(20.0, 30.0))?;
                        let installation = prepare_ui_runtime_alongside(runtime, window).submit();
                        constructed.set(Some(installation.address));
                        Ok(installation)
                    },
                    None,
                    None,
                )));
                let (initial, initial_native) = WindowInstallation::channel(owner.proxy());
                let (replacement, replacement_native) = WindowInstallation::channel(owner.proxy());
                let installing = Rc::clone(&controller);
                dispatch_platform_ui_runtime(
                    outer,
                    RuntimeTask::TestCallback(Box::new(move |_| {
                        let mut controller = installing.borrow_mut();
                        assert!(
                            controller
                                .connect("scene", Arc::clone(&window), false, initial)
                                .expect("admitted")
                                .is_none()
                        );
                        assert!(controller.has_pending());
                        assert!(
                            controller.dispatchers().is_empty(),
                            "unfinished scenes cannot receive background frame work"
                        );
                        match case {
                            PendingCase::Complete => {}
                            PendingCase::Reconnect => {
                                assert!(
                                    controller
                                        .connect("scene", window, false, replacement)
                                        .expect("pending attachment replacement")
                                        .is_none()
                                );
                            }
                            PendingCase::Discard => controller.discard(&"scene"),
                        }
                    })),
                )
                .expect("owner completes runtime publication");
                match case {
                    PendingCase::Complete => assert!(
                        initial_native.outcome().is_none(),
                        "native publication awaits controller acknowledgement"
                    ),
                    PendingCase::Reconnect => {
                        assert!(
                            initial_native
                                .outcome()
                                .expect("old acknowledgement retired")
                                .is_err()
                        );
                        assert!(replacement_native.outcome().is_none());
                    }
                    PendingCase::Discard => assert!(
                        initial_native
                            .outcome()
                            .expect("discard cancels native installation")
                            .is_err()
                    ),
                }
                controller.borrow_mut().poll();
                assert_eq!(
                    built.get(),
                    1,
                    "one logical installation across attachment changes"
                );
                assert!(!controller.borrow().has_pending());
                match case {
                    PendingCase::Complete => assert_eq!(initial_native.outcome(), Some(Ok(()))),
                    PendingCase::Reconnect => {
                        assert_eq!(replacement_native.outcome(), Some(Ok(())));
                    }
                    PendingCase::Discard => assert!(controller.borrow().dispatchers().is_empty()),
                }
                assert_eq!(
                    controller.borrow().dispatchers().len(),
                    usize::from(case != PendingCase::Discard)
                );
                let registered = APP_RUNTIME.with(|slot| {
                    slot.borrow()
                        .installed_host
                        .native()
                        .contains_address(address.get().expect("constructed session"))
                });
                assert_eq!(
                    registered,
                    case != PendingCase::Discard,
                    "discard fences actual native publication, not just the controller entry"
                );
                controller.borrow_mut().discard(&"scene");
                Ok(())
            }))
            .expect("headless session host");
    }

    #[test]
    fn session_installation_contract() {
        crate::table_test::run_table(
            "session_installation_contract",
            &[
                (
                    "reconnect_retains_tree_discard_creates_fresh_state_without_restarting_service",
                    reconnect_retains_tree_discard_creates_fresh_state_without_restarting_service
                        as fn(),
                ),
                (
                    "pending_session_acknowledges_only_completed_publication",
                    pending_session_acknowledges_only_completed_publication as fn(),
                ),
                (
                    "pending_reconnect_replaces_native_acknowledgement_without_rebuilding",
                    pending_reconnect_replaces_native_acknowledgement_without_rebuilding as fn(),
                ),
                (
                    "pending_discard_cannot_publish_late",
                    pending_discard_cannot_publish_late as fn(),
                ),
                (
                    "abandoned_receipt_cannot_publish_without_a_native_close",
                    abandoned_receipt_cannot_publish_without_a_native_close as fn(),
                ),
            ],
        );
    }
}
