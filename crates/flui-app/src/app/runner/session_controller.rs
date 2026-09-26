//! Session ownership shared by UIKit's adapter and host lifecycle tests.
use super::realm_dispatch::{RealmDispatcher, close_this_window};
use crate::app::hot_reload::WorkerWatcherGuard;
use flui_platform::HostWindow;
use std::{collections::HashMap, hash::Hash, sync::Arc};

pub(super) fn contain(body: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        std::mem::forget(payload);
    }
}

type SessionInstaller = Box<dyn FnMut(Arc<dyn HostWindow>) -> anyhow::Result<RealmDispatcher>>;

pub(in crate::app) struct SessionController<K: Eq + Hash> {
    installer: Option<SessionInstaller>,
    sessions: HashMap<K, RealmDispatcher>,
    watcher: Option<WorkerWatcherGuard>,
}
impl<K: Eq + Hash> SessionController<K> {
    pub(super) fn new(
        installer: impl FnMut(Arc<dyn HostWindow>) -> anyhow::Result<RealmDispatcher> + 'static,
        watcher: Option<WorkerWatcherGuard>,
    ) -> Self {
        Self {
            installer: Some(Box::new(installer)),
            sessions: HashMap::new(),
            watcher,
        }
    }

    pub(super) fn connect(
        &mut self,
        key: K,
        window: Arc<dyn HostWindow>,
        reconnect: bool,
    ) -> anyhow::Result<RealmDispatcher> {
        if reconnect {
            return self
                .sessions
                .get(&key)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("reconnected session has no retained realm"));
        }
        if self.sessions.contains_key(&key) {
            anyhow::bail!("session already has an installed realm");
        }
        let dispatcher = (self
            .installer
            .as_mut()
            .expect("BUG: live controller owns installer"))(window)?;
        self.sessions.insert(key, dispatcher);
        Ok(dispatcher)
    }

    pub(super) fn discard(&mut self, key: &K) {
        if let Some(dispatcher) = self.sessions.remove(key) {
            contain(|| close_this_window(dispatcher));
        }
    }

    pub(super) fn dispatchers(&self) -> Vec<RealmDispatcher> {
        self.sessions.values().copied().collect()
    }
}
impl<K: Eq + Hash> Drop for SessionController<K> {
    fn drop(&mut self) {
        let installer = self.installer.take();
        let watcher = self.watcher.take();
        contain(|| drop(installer));
        contain(|| drop(watcher));
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        host::APP_RUNTIME,
        realm_dispatch::{install_realm_alongside, teardown_platform_realm},
    };
    use super::*;
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
            flui_widgets::SizedBox::new(self.local.get() as f32, 20.0)
        }
        fn dispose(&mut self) {
            self.disposed.set(self.disposed.get() + 1);
            self.history.borrow_mut().push("disposed".into());
        }
    }
    #[test]
    fn reconnect_retains_tree_discard_creates_fresh_state_without_restarting_service() {
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                teardown_platform_realm();
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
                let realm = crate::app::ui_realm::UiRealm::for_test();
                realm
                    .enter(|realm| realm.attach_root_widget(&root))
                    .expect("root mounted");
                let _ = realm.draw_frame(flui_rendering::constraints::BoxConstraints::tight(
                    flui_types::Size::new(
                        flui_types::geometry::px(80.0),
                        flui_types::geometry::px(80.0),
                    ),
                ));
                let window: Arc<dyn flui_platform::PlatformWindow> = window;
                Ok(install_realm_alongside(realm, &window)?)
            },
            None,
        );
        let platform = flui_platform::headless_platform();
        let first = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("first native owner");
        let first_dispatcher = controller
            .connect("first", first.clone(), false)
            .expect("first install");
        probe.states.borrow()[0].set(42);
        let reconnected = controller
            .connect("first", first, true)
            .expect("retained reconnect");
        assert_eq!(first_dispatcher.address, reconnected.address);
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
            .expect("new native owner");
        let fresh = controller
            .connect("second", second, false)
            .expect("fresh install");
        assert_ne!(fresh.address.realm_id, first_dispatcher.address.realm_id);
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
        teardown_platform_realm();
        assert!(
            cancelled.load(std::sync::atomic::Ordering::SeqCst),
            "process shutdown joins the service"
        );
    }
}
