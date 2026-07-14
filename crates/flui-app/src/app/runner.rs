//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-platform.

use flui_view::{StatelessView, View};

use super::{AppBinding, AppConfig};

/// Run a FLUI application with default configuration.
///
/// This is the internal implementation called by `run_app()`.
pub fn run_app_impl<V>(root: V)
where
    V: View + StatelessView + Clone + 'static,
{
    run_app_with_config_impl(root, AppConfig::default());
}

/// Run a FLUI application with custom configuration.
///
/// This is the internal implementation called by `run_app_with_config()`.
pub fn run_app_with_config_impl<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    // Initialize logging
    init_logging();

    tracing::info!(
        title = %config.title,
        size = ?config.size,
        fps = config.target_fps,
        "Starting FLUI application"
    );

    // Run platform-specific event loop
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        run_desktop(root, config);
    }

    #[cfg(target_os = "android")]
    {
        let _ = (root, config);
        panic!(
            "On Android, use flui_app::run_app_android() from android_main() \
             instead of run_app(). AndroidApp must be provided by the system."
        );
    }

    #[cfg(target_os = "ios")]
    {
        run_ios(config);
    }

    #[cfg(target_arch = "wasm32")]
    {
        run_web(root, config);
    }
}

/// Initialize logging based on environment.
fn init_logging() {
    // Use flui_foundation::log for cross-platform logging (desktop, Android, iOS, WASM).
    // Module was merged from the standalone flui-log crate.
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        "info,flui_app=debug,flui_view=debug,flui_rendering=debug,wgpu=warn".to_string()
    });

    flui_foundation::log::Logger::new()
        .with_filter(&filter)
        // TRACE ceiling: the per-target filter (RUST_LOG / the default
        // string above) decides what's emitted — a DEBUG ceiling here
        // silently made every trace! unreachable no matter what the
        // user put in RUST_LOG.
        .with_level(flui_foundation::log::Level::TRACE)
        .init();
}

// ============================================================================
// Platform-neutral owner-thread realm host (ADR-0027)
// ============================================================================

#[cfg(not(target_os = "ios"))]
thread_local! {
    /// Transitional owner-thread host shared by desktop, Android, and wasm.
    /// The platform callback surface still requires `Send`, so the `!Send`
    /// realm remains in owner TLS until that seam is retired. Access is only
    /// through the stamped FIFO dispatcher below.
    static PLATFORM_REALM_HOST: std::cell::RefCell<RealmHost> =
        const { std::cell::RefCell::new(RealmHost::new()) };
}

#[cfg(not(target_os = "ios"))]
struct RealmHost {
    realm: Option<super::ui_realm::UiRealm>,
    queue: std::collections::VecDeque<RealmEvent>,
    draining: bool,
    owner_thread: Option<std::thread::ThreadId>,
    realm_id: Option<flui_foundation::RealmId>,
}

#[cfg(not(target_os = "ios"))]
impl RealmHost {
    const fn new() -> Self {
        Self {
            realm: None,
            queue: std::collections::VecDeque::new(),
            draining: false,
            owner_thread: None,
            realm_id: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg(not(target_os = "ios"))]
struct RealmDispatcher {
    owner_thread: std::thread::ThreadId,
    realm_id: flui_foundation::RealmId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(target_os = "ios"))]
enum RealmDispatchError {
    WrongThread,
    StaleRealm,
    RealmUnavailable,
}

#[cfg(not(target_os = "ios"))]
enum RealmEvent {
    Input(flui_platform::traits::PlatformInput),
    Resize {
        size: flui_types::Size<flui_types::geometry::Pixels>,
        scale_factor: f32,
        apply_surface: Box<dyn FnOnce()>,
    },
    Lifecycle(flui_platform::traits::LifecycleEvent),
    Active(bool),
    Frame(Box<dyn FnOnce(&super::ui_realm::UiRealm)>),
}

#[cfg(not(target_os = "ios"))]
impl RealmEvent {
    fn run(self, realm: &super::ui_realm::UiRealm) {
        match self {
            Self::Input(input) => AppBinding::instance().handle_input(input),
            Self::Resize {
                size,
                scale_factor,
                apply_surface,
            } => {
                apply_surface();
                AppBinding::instance()
                    .render_pipeline_mut()
                    .set_device_pixel_ratio(scale_factor);
                AppBinding::instance().request_redraw();
                tracing::trace!(?size, scale_factor, "realm resize committed");
            }
            Self::Lifecycle(event) => AppBinding::instance().transition_lifecycle(event),
            Self::Active(active) => {
                let event = if active {
                    flui_platform::traits::LifecycleEvent::Activated
                } else {
                    flui_platform::traits::LifecycleEvent::Deactivated
                };
                AppBinding::instance().transition_lifecycle(event);
            }
            Self::Frame(run) => run(realm),
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn install_platform_realm(realm: super::ui_realm::UiRealm) -> RealmDispatcher {
    let owner_thread = std::thread::current().id();
    let realm_id = realm.realm_id();
    PLATFORM_REALM_HOST.with(|slot| {
        let mut state = slot.borrow_mut();
        state.realm = Some(realm);
        state.owner_thread = Some(owner_thread);
        state.realm_id = Some(realm_id);
    });
    RealmDispatcher {
        owner_thread,
        realm_id,
    }
}

#[cfg(not(target_os = "ios"))]
fn dispatch_platform_realm(
    dispatcher: RealmDispatcher,
    event: RealmEvent,
) -> Result<(), RealmDispatchError> {
    if std::thread::current().id() != dispatcher.owner_thread {
        tracing::error!(?dispatcher, "rejecting realm callback on non-owner thread");
        return Err(RealmDispatchError::WrongThread);
    }
    let realm = PLATFORM_REALM_HOST.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.realm_id != Some(dispatcher.realm_id) {
            return Err(if state.realm_id.is_some() {
                RealmDispatchError::StaleRealm
            } else {
                RealmDispatchError::RealmUnavailable
            });
        }
        state.queue.push_back(event);
        if state.draining || state.realm.is_none() {
            return Ok(None);
        }
        let first = state
            .queue
            .pop_front()
            .expect("BUG: event was enqueued before starting realm dispatch");
        state.draining = true;
        Ok(state.realm.take().map(|realm| (realm, first)))
    })?;
    let Some((realm, first)) = realm else {
        return Ok(());
    };

    // Never hold the TLS RefCell borrow across user/platform callbacks. Catch
    // only to restore the host invariants; the original panic is resumed.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut next = Some(first);
        while let Some(event) = next {
            realm.enter(|realm| event.run(realm));
            next = PLATFORM_REALM_HOST.with(|slot| slot.borrow_mut().queue.pop_front());
        }
    }));
    PLATFORM_REALM_HOST.with(|slot| {
        let mut state = slot.borrow_mut();
        state.realm = Some(realm);
        state.draining = false;
    });
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
    Ok(())
}

#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
fn teardown_platform_realm() {
    let (realm, queued) = PLATFORM_REALM_HOST.with(|slot| {
        let mut state = slot.borrow_mut();
        let realm = state.realm.take();
        let queued = std::mem::take(&mut state.queue);
        state.draining = false;
        state.owner_thread = None;
        state.realm_id = None;
        (realm, queued)
    });
    // Destructors may re-enter platform/framework code. Drop only after the
    // TLS borrow and incarnation identity have been released.
    drop(queued);
    drop(realm);
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn queued_hot_reload_hook(
    sender: super::ui_realm::UiCommandSender,
) -> impl Fn() + Send + Sync + 'static {
    move || {
        if let Err(error) = sender.request_hot_reload(flui_hot_reload::HotReloadTier::HotReload) {
            tracing::warn!(
                ?error,
                "ignoring hot-reload request for a dead or busy realm"
            );
        }
    }
}

#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod realm_dispatch_tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    fn install_test_realm() -> RealmDispatcher {
        let app = AppBinding::instance();
        install_platform_realm(super::super::ui_realm::UiRealm::for_test(app))
    }

    #[test]
    fn reentrant_frame_event_is_queued_fifo() {
        let dispatcher = install_test_realm();
        let order = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&order);
        dispatch_platform_realm(
            dispatcher,
            RealmEvent::Frame(Box::new(move |_| {
                outer.borrow_mut().push(1);
                let nested = Rc::clone(&outer);
                dispatch_platform_realm(
                    dispatcher,
                    RealmEvent::Frame(Box::new(move |_| {
                        nested.borrow_mut().push(3);
                    })),
                )
                .expect("nested event queues");
                outer.borrow_mut().push(2);
            })),
        )
        .expect("outer event dispatches");
        assert_eq!(*order.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn late_event_never_crosses_realm_incarnations() {
        let stale = install_test_realm();
        PLATFORM_REALM_HOST.with(|slot| {
            let mut state = slot.borrow_mut();
            let realm = state.realm.take();
            state.queue.clear();
            state.realm_id = None;
            drop(state);
            drop(realm);
        });
        assert_eq!(
            dispatch_platform_realm(stale, RealmEvent::Frame(Box::new(|_| {}))),
            Err(RealmDispatchError::RealmUnavailable)
        );

        let current = install_test_realm();
        assert_eq!(
            dispatch_platform_realm(stale, RealmEvent::Frame(Box::new(|_| {}))),
            Err(RealmDispatchError::StaleRealm)
        );
        dispatch_platform_realm(current, RealmEvent::Frame(Box::new(|_| {})))
            .expect("current incarnation dispatches");
    }

    #[test]
    fn panic_restores_dispatch_host_for_next_event() {
        let dispatcher = install_test_realm();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = dispatch_platform_realm(
                dispatcher,
                RealmEvent::Frame(Box::new(|_| panic!("test panic"))),
            );
        }));
        assert!(panic.is_err());

        let ran = Rc::new(RefCell::new(false));
        let ran_in_event = Rc::clone(&ran);
        dispatch_platform_realm(
            dispatcher,
            RealmEvent::Frame(Box::new(move |_| {
                *ran_in_event.borrow_mut() = true;
            })),
        )
        .expect("host restored");
        assert!(*ran.borrow());
    }

    #[test]
    fn callback_on_wrong_thread_is_rejected() {
        let dispatcher = install_test_realm();
        let result = std::thread::spawn(move || {
            dispatch_platform_realm(dispatcher, RealmEvent::Frame(Box::new(|_| {})))
        })
        .join()
        .expect("worker test thread");
        assert_eq!(result, Err(RealmDispatchError::WrongThread));
    }

    #[test]
    fn nested_resize_and_lifecycle_wait_until_frame_returns() {
        let dispatcher = install_test_realm();
        let order = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&order);
        dispatch_platform_realm(
            dispatcher,
            RealmEvent::Frame(Box::new(move |_| {
                outer.borrow_mut().push(1);
                dispatch_platform_realm(
                    dispatcher,
                    RealmEvent::Lifecycle(flui_platform::traits::LifecycleEvent::Activated),
                )
                .expect("lifecycle queues");
                let resize = Rc::clone(&outer);
                dispatch_platform_realm(
                    dispatcher,
                    RealmEvent::Resize {
                        size: flui_types::Size::new(
                            flui_types::geometry::px(640.0),
                            flui_types::geometry::px(480.0),
                        ),
                        scale_factor: 2.0,
                        apply_surface: Box::new(move || resize.borrow_mut().push(3)),
                    },
                )
                .expect("resize queues");
                outer.borrow_mut().push(2);
            })),
        )
        .expect("frame dispatches");
        assert_eq!(*order.borrow(), vec![1, 2, 3]);
        assert_eq!(
            AppBinding::instance().lifecycle_state(),
            super::super::lifecycle::LifecycleState::Active
        );
    }

    #[test]
    fn teardown_drops_queued_destructors_outside_tls_borrow() {
        struct ReenterOnDrop {
            dispatcher: RealmDispatcher,
            dropped: Rc<RefCell<bool>>,
        }

        impl Drop for ReenterOnDrop {
            fn drop(&mut self) {
                let result =
                    dispatch_platform_realm(self.dispatcher, RealmEvent::Frame(Box::new(|_| {})));
                assert_eq!(result, Err(RealmDispatchError::RealmUnavailable));
                *self.dropped.borrow_mut() = true;
            }
        }

        let dispatcher = install_test_realm();
        let dropped = Rc::new(RefCell::new(false));
        let probe = ReenterOnDrop {
            dispatcher,
            dropped: Rc::clone(&dropped),
        };
        PLATFORM_REALM_HOST.with(|slot| {
            slot.borrow_mut()
                .queue
                .push_back(RealmEvent::Frame(Box::new(move |_| drop(probe))));
        });
        teardown_platform_realm();
        assert!(*dropped.borrow());
    }

    #[test]
    fn old_registered_hot_reload_hook_cannot_touch_recreated_realm() {
        use flui_hot_reload::{register_request_rebuild, request_rebuild};

        let runtime_a = super::super::ui_realm::UiRealm::for_test(AppBinding::instance());
        let sender_a = runtime_a.command_sender();
        let old_a_hook = queued_hot_reload_hook(sender_a.clone());
        let registration_a = register_request_rebuild(queued_hot_reload_hook(sender_a));
        let _realm_a = install_platform_realm(runtime_a);
        teardown_platform_realm();

        let runtime_b = super::super::ui_realm::UiRealm::for_test(AppBinding::instance());
        let sender_b = runtime_b.command_sender();
        let realm_b = install_platform_realm(runtime_b);
        let registration_b = register_request_rebuild(queued_hot_reload_hook(sender_b));
        drop(registration_a);

        old_a_hook();
        let after_old = Rc::new(RefCell::new(None));
        let after_old_in_frame = Rc::clone(&after_old);
        dispatch_platform_realm(
            realm_b,
            RealmEvent::Frame(Box::new(move |realm| {
                *after_old_in_frame.borrow_mut() = Some(realm.drain_commands());
            })),
        )
        .expect("B frame dispatches");
        assert_eq!(
            *after_old.borrow(),
            Some(super::super::ui_realm::DrainReport::default()),
            "stale A hook must not enqueue into B"
        );

        std::thread::spawn(request_rebuild)
            .join()
            .expect("worker-side rebuild request");
        let after_current = Rc::new(RefCell::new(None));
        let after_current_in_frame = Rc::clone(&after_current);
        dispatch_platform_realm(
            realm_b,
            RealmEvent::Frame(Box::new(move |realm| {
                *after_current_in_frame.borrow_mut() = Some(realm.drain_commands());
            })),
        )
        .expect("B frame dispatches");
        assert_eq!(
            after_current.borrow().as_ref().map(|report| report.invoked),
            Some(1),
            "current B hook must dispatch exactly once"
        );

        drop(registration_b);
        teardown_platform_realm();
    }

    #[test]
    fn whole_frame_event_keeps_realm_global_key_scope_active() {
        let app = AppBinding::instance();
        let realm = super::super::ui_realm::UiRealm::for_test(app);
        let key = flui_view::GlobalKey::<()>::new();
        let element = flui_foundation::ElementId::new(91);
        realm
            .widgets()
            .with_build_owner_mut(|owner| owner.register_global_key(key.id(), element));
        let dispatcher = install_platform_realm(realm);
        let key_after_frame = key.clone();

        assert_eq!(key.current_element(), None, "scope starts inactive");
        dispatch_platform_realm(
            dispatcher,
            RealmEvent::Frame(Box::new(move |_| {
                assert_eq!(key.current_element(), Some(element));
            })),
        )
        .expect("frame dispatches");
        assert_eq!(
            key_after_frame.current_element(),
            None,
            "frame scope is restored"
        );
        teardown_platform_realm();
    }
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn run_desktop<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_hot_reload::{
        HotReloadTier, RebuildHookRegistration, WorkerPollOutcome, WorkerReloadDriver, engine::env,
        register_request_rebuild,
    };
    use flui_platform::{
        Platform, WindowOptions,
        traits::{DispatchEventResult, LifecycleEvent, PlatformInput},
    };
    use flui_scheduler::Scheduler;
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting desktop platform via flui-platform");

    let worker_driver = config
        .worker_plugin_path
        .clone()
        .or_else(|| std::env::var(env::WORKER_PLUGIN).ok().map(Into::into))
        .map(WorkerReloadDriver::new);
    let has_worker_driver = worker_driver.is_some();
    let worker_driver = Arc::new(Mutex::new(worker_driver));

    let platform = flui_platform::current_platform().expect("Failed to initialize platform");

    // `rebuild_registration`'s `Drop` detaches the hot-reload hook and must
    // stay alive until the event loop exits — but it (like the window and
    // every callback below) can only be created from inside `on_ready`, so
    // it is threaded back out through this cell instead of a plain local.
    let rebuild_registration: Rc<RefCell<Option<RebuildHookRegistration>>> =
        Rc::new(RefCell::new(None));
    let rebuild_registration_slot = Rc::clone(&rebuild_registration);

    // Bootstrap can fail fatally (GPU init, `UiRealm` construction, root
    // widget attach) from inside `on_ready`, which has no return path back
    // to this function's caller — thread the failure out through this cell
    // instead, same pattern as `rebuild_registration`.
    let bootstrap_error: Rc<RefCell<Option<anyhow::Error>>> = Rc::new(RefCell::new(None));
    let bootstrap_error_slot = Rc::clone(&bootstrap_error);

    /// The actual desktop bootstrap: opens the window, initializes the GPU
    /// renderer, mounts the widget tree, and wires every platform/window
    /// callback. Runs exactly once, synchronously, inside `on_ready` (see
    /// `Platform::run`'s doc) — never before, since the winit backend can
    /// only create a window from inside a running event loop
    /// (`ActiveEventLoop` is unreachable beforehand, and `open_window` fails
    /// fast rather than deadlock if called too early).
    ///
    /// Pulled out of the `on_ready` closure into a named fn so rustfmt
    /// actually formats it — rustfmt does not reliably reformat very large
    /// closure literals passed as call arguments.
    fn bootstrap_desktop<V>(
        platform: &dyn Platform,
        root: V,
        config: AppConfig,
        has_worker_driver: bool,
        worker_driver: Arc<Mutex<Option<WorkerReloadDriver>>>,
        rebuild_registration_slot: Rc<RefCell<Option<RebuildHookRegistration>>>,
        bootstrap_error_slot: Rc<RefCell<Option<anyhow::Error>>>,
    ) where
        V: View + StatelessView + Clone + 'static,
    {
        tracing::info!("Platform ready");

        // 1. Open window now that the event loop is running.
        let options: WindowOptions = (&config).into();
        let window = platform
            .open_window(options)
            .expect("Failed to create window");

        // 2. Create GPU renderer directly (no DesktopEmbedder)
        let phys_size = window.physical_size();
        let renderer = pollster::block_on(async {
            let handle = PlatformWindowHandle(window.as_ref());
            Renderer::new(&handle).await
        });
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                *bootstrap_error_slot.borrow_mut() =
                    Some(anyhow::anyhow!(e).context("GPU init failed"));
                platform.quit();
                return;
            }
        };
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

        // 3. Mount root widget at the LOGICAL size; the framework lays out
        // in logical pixels and the paint root's DPR transform maps to the
        // physical surface. Set the DPR BEFORE attach so the RenderView
        // configuration and the first frame agree on the scale.
        let scale_factor = window.scale_factor() as f32;
        AppBinding::instance()
            .render_pipeline_mut()
            .set_device_pixel_ratio(scale_factor);
        let ui_realm =
            match super::ui_realm::UiRealm::new(AppBinding::instance().frame_wake_callback()) {
                Ok(realm) => realm,
                Err(e) => {
                    tracing::error!(error = %e, "UiRealm construction failed");
                    *bootstrap_error_slot.borrow_mut() =
                        Some(anyhow::anyhow!(e).context("UiRealm construction failed"));
                    platform.quit();
                    return;
                }
            };
        ui_realm.bind_to_app(AppBinding::instance());
        let logical = window.logical_size();
        let attach = ui_realm.enter(|realm| {
            AppBinding::instance().attach_root_widget_with_size(
                realm,
                &root,
                logical.width.0,
                logical.height.0,
            )
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            *bootstrap_error_slot.borrow_mut() =
                Some(anyhow::anyhow!(e).context("Root widget attach failed"));
            platform.quit();
            return;
        }
        register_hit_test_render_view();

        // 3b. Wire the wake chain (E0a).
        //
        // `on_need_frame` fires whenever `handle_build_scheduled` determines a new
        // frame is required (e.g. after setState).  The closure calls `wake_frame`
        // which sets `needs_redraw` atomically AND calls `PlatformWindow::
        // request_redraw()` so the winit event loop wakes from idle.
        //
        // Deadlock analysis:
        // * `wake_frame` acquires only `active_window` (leaf Mutex).
        // * The closure is called from `handle_build_scheduled`, which holds no
        //   `inner`/`widgets` lock (see `WidgetsBinding::handle_build_scheduled`
        //   doc).
        // * `on_need_frame` itself is a separate `RwLock` on `WidgetsBinding`,
        //   never held across any `inner` critical section.
        // Therefore: no lock ordering conflict.
        {
            let widgets = ui_realm.widgets();
            let wake = AppBinding::instance().frame_wake_callback();
            widgets.set_on_need_frame(move || wake());
        }

        // Wire `on_build_scheduled` on the BuildOwner so a dirty-element
        // registration (e.g. from setState inside an element build) wakes the
        // platform loop. The callback fires from inside `schedule_build_for`,
        // which runs during a build while the AppBinding `widgets` write lock is
        // held — so it must NOT re-lock `widgets`. It calls `wake_frame`
        // directly (the same effect as the `on_need_frame` callback above),
        // which touches only the `active_window` leaf lock. The callback must not
        // re-enter widget state while `BuildOwner` is scheduling; realm entry is
        // reserved for the outer event/frame dispatch boundary.
        {
            let widgets = ui_realm.widgets();
            widgets.with_build_owner_mut(|build_owner| {
                let wake = AppBinding::instance().frame_wake_callback();
                build_owner.set_on_build_scheduled(move || wake());
            });
        }

        // 3c. Construct the per-window owner and its bounded command inbox.
        // The wake is the existing chain: `wake_frame` sets
        // `needs_redraw` and queues a `RedrawRequested`, so a command sent to an
        // idle loop produces the frame whose drain observes it.
        //
        tracing::info!(
            realm_id = ?ui_realm.realm_id(),
            inbox_capacity = ui_realm.command_sender().capacity(),
            "UiRealm constructed"
        );
        let hot_reload_sender = ui_realm.command_sender();
        let realm_dispatch = install_platform_realm(ui_realm);
        *rebuild_registration_slot.borrow_mut() = has_worker_driver
            .then(|| register_request_rebuild(queued_hot_reload_hook(hot_reload_sender)));

        // 4. Wrap renderer for callback sharing
        let renderer = Arc::new(Mutex::new(renderer));

        // 5. Register input callback -> AppBinding::handle_input()
        window.on_input(Box::new(move |input: PlatformInput| {
            let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Input(input));
            DispatchEventResult::resolved(false, true)
        }));

        // 6. Register frame callback -> scheduler + AppBinding::render_frame()
        let renderer_frame = Arc::clone(&renderer);
        let worker_driver_frame = Arc::clone(&worker_driver);
        let frame_budget =
            std::time::Duration::from_secs_f64(1.0 / f64::from(config.target_fps.max(1)));
        let last_frame_started = Arc::new(Mutex::new(None::<web_time::Instant>));
        window.on_request_frame(Box::new(move || {
        let renderer_frame = Arc::clone(&renderer_frame);
        let worker_driver_frame = Arc::clone(&worker_driver_frame);
        let last_frame_started = Arc::clone(&last_frame_started);
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Frame(Box::new(move |realm| {
            if let Some(ref mut driver) = *worker_driver_frame.lock()
                && matches!(driver.poll(), WorkerPollOutcome::Reloaded { .. })
            {
                AppBinding::instance()
                    .perform_hot_reload_entered(realm, HotReloadTier::HotReload);
            }

            let binding = AppBinding::instance();
            let scheduler = Scheduler::instance();

        // Owner-inbox drain: commands and worker results
        // commit HERE, at the frame boundary while the scheduler phase is
        // Idle — never inside the frame transaction below. Runs before the
        // dirty gate so a command-driven redraw request is observed by the
        // very frame its wake produced.
        //
        // The runtime is TAKEN out of the slot for the drain (and restored
        // after) so drained user closures never run under the RefCell
        // borrow: a command that re-enters this frame callback through a
        // nested platform pump then finds an empty slot and skips the
        // drain, instead of panicking the borrow.
            let inbox_redraw = {
                let report = realm.drain_commands();
                if report != super::ui_realm::DrainReport::default() {
                    tracing::trace!(?report, "owner inbox drained");
                }
                realm.take_redraw_request()
            };

            let dirty =
                inbox_redraw || binding.needs_redraw() || binding.has_pending_work(realm);
            if !dirty && !scheduler.is_frame_scheduled() {
                return;
            }

        // On-demand rendering: skip frame if nothing changed. A frame
        // the SCHEDULER scheduled (a pending animation ticker callback)
        // counts as work: `needs_redraw` is cleared by `mark_rendered`
        // at the end of the previous frame, so without this check the
        // gate starves tickers after one frame — the wake hook gets the
        // event loop here, and this lets the pump actually run.
        // Pace pure ticker-driven frames to the configured target FPS.
        // WM_PAINT-style redraw requests carry no vsync: an animation
        // re-requesting a redraw every frame would otherwise spin the
        // render loop as fast as the CPU allows (observed: ~30 000 fps
        // with a Mailbox present mode). Dirty work renders immediately.
            if !dirty && let Some(started) = *last_frame_started.lock() {
            let elapsed = started.elapsed();
            if elapsed < frame_budget {
                std::thread::sleep(
                    frame_budget
                        .checked_sub(elapsed)
                        .expect("BUG: `elapsed < frame_budget` was checked on the previous line"),
                );
            }
        }

            let now = web_time::Instant::now();
            *last_frame_started.lock() = Some(now);

        // Scheduler callbacks (animations). NOTE: the global `Scheduler` is driven
        // off this per-frame `Instant::now()`, while the tree-bound `Vsync`
        // (AppBinding::draw_frame) ticks off `AppBinding`'s own `start` origin —
        // two separate clocks ON PURPOSE: the controller sets are disjoint (implicit
        // animations register with `Vsync`; plain controllers carry a private
        // `Scheduler` ticker, never the global one), so the origins never need to
        // agree and no controller is advanced twice.
        // The ONE shared frame ordering — begin (transient +
        // microtasks + the single async-driver poll) -> persistent callbacks ->
        // the pipeline below -> post-frame callbacks -> Idle. `HeadlessBinding`
        // drives the same helper on its binding-local scheduler.
            scheduler.drive_frame(now, || {
            // Render frame via AppBinding
            let mut r = renderer_frame.lock();
                binding.render_frame_entered(realm, &mut *r);

            // GPU device-loss recovery: if the device was lost during this frame
            // (detected by the wgpu callback that fired between render_frame calls),
            // attempt a synchronous rebuild on the runner thread. `pollster` is
            // already a dep and safe to use here — the desktop runner owns this
            // synchronous callback, not an async executor.
            if r.is_device_lost() {
                match pollster::block_on(r.recover()) {
                    Ok(()) => {
                        tracing::warn!("GPU device lost — recovered successfully");
                        // `wake_frame` (not `request_redraw`) so an idle winit loop
                        // actually queues a `RedrawRequested`: device loss is
                        // detected on a quiescent loop, where only flipping the
                        // `needs_redraw` flag would leave the recovered renderer
                        // idle until the next external input/resize.
                        AppBinding::instance().wake_frame();
                    }
                    Err(e) => {
                        // Driver may still be resetting. Log and let the next frame
                        // retry; the device-lost flag remains set so recover() will
                        // be tried again.
                        tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                    }
                }
            }
            });
        })));
    }));

        // 7. Register resize callback -> renderer.resize()
        let renderer_resize = Arc::clone(&renderer);
        window.on_resize(Box::new(move |size, scale_factor| {
            let apply_size = size;
            let renderer_resize = Arc::clone(&renderer_resize);
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmEvent::Resize {
                    size,
                    scale_factor,
                    apply_surface: Box::new(move || {
                        let w = (apply_size.width.0 * scale_factor) as u32;
                        let h = (apply_size.height.0 * scale_factor) as u32;
                        renderer_resize.lock().resize(w, h);
                    }),
                },
            );
        }));

        // 8. Lifecycle callbacks

        // Platform quit -> transition to Terminating
        platform.on_quit(Box::new(move || {
            tracing::info!("Platform quit");
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmEvent::Lifecycle(LifecycleEvent::Terminating),
            );
        }));

        // Window close -> log and let the platform handle quit
        // (Windows window proc already calls PostQuitMessage on WM_DESTROY)
        window.on_close(Box::new(move || {
            tracing::info!("Window closed");
        }));

        // Window should-close -> allow by default
        window.on_should_close(Box::new(|| {
            tracing::debug!("Window close requested, allowing");
            true
        }));

        // Window active status -> lifecycle Activated/Deactivated
        window.on_active_status_change(Box::new(move |active| {
            let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Active(active));
        }));

        // Mark lifecycle as started
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Lifecycle(LifecycleEvent::Started),
        );

        // 9. Request initial redraw
        window.request_redraw();

        // 10. Store window in AppBinding for runtime access
        AppBinding::instance().set_window(window);

        tracing::info!("Desktop platform initialized with callbacks");
    }

    // Window creation, GPU/renderer setup, and callback wiring all run
    // inside `on_ready` rather than before `run()`. The winit backend can
    // only create a window from inside a running event loop (`ActiveEventLoop`
    // is unreachable beforehand); opening it earlier would deadlock forever
    // waiting for a pump that never started. `on_ready` runs exactly once,
    // synchronously, on this same thread — see `Platform::run`'s doc.
    platform.run(Box::new(move |platform: &dyn Platform| {
        bootstrap_desktop(
            platform,
            root,
            config,
            has_worker_driver,
            worker_driver,
            rebuild_registration_slot,
            bootstrap_error_slot,
        );
    }));

    // Event loop exited: drop the runtime now (releases the at-most-one
    // claim; outstanding senders turn `OwnerGone`) instead of at thread
    // death.
    drop(rebuild_registration.borrow_mut().take());
    teardown_platform_realm();

    // Surface a fatal bootstrap failure (GPU init, `UiRealm` construction,
    // root widget attach) now that the event loop has exited — those
    // failures happen inside `on_ready`, with no return path back here
    // except through `bootstrap_error`, and quitting the platform on them
    // (see `bootstrap_desktop`) must not look like a clean exit.
    if let Some(err) = bootstrap_error.borrow_mut().take() {
        panic!("desktop bootstrap failed: {err:?}");
    }
}

/// Register the hit-test root `RenderView` with the `RendererBinding`
/// (`view_id = 0`).
///
/// `WidgetsBinding::attach_root_widget` bootstraps the *paint* render tree
/// (`RootRenderElement` → `PipelineOwner`), but hit testing routes through the
/// `RendererBinding`'s own per-view registry. These two `RenderView`s are
/// kept mapped independently by design: the paint root lives in the
/// `PipelineOwner`; the hit-test root is registered here by the runner
/// after attach.
fn register_hit_test_render_view() {
    use std::sync::Arc;

    use flui_rendering::{binding::RendererBinding, view::RenderView};

    let renderer = AppBinding::instance().renderer();
    let view = Arc::new(parking_lot::RwLock::new(RenderView::new()));
    renderer.add_render_view_with_config(0, view);
    tracing::info!("RenderView registered for hit testing (view_id=0)");
}

// ============================================================================
// Android Implementation
// ============================================================================

/// Run a FLUI application on Android with default configuration.
///
/// This is the primary entry point for Android apps. Call this from your
/// `android_main()` function:
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     flui_app::run_app_android(app, MyRootView);
/// }
/// ```
#[cfg(target_os = "android")]
pub fn run_app_android<V>(app: android_activity::AndroidApp, root: V)
where
    V: View + StatelessView + Clone + 'static,
{
    run_app_android_with_config(app, root, AppConfig::default());
}

/// Run a FLUI application on Android with custom configuration.
///
/// Like [`run_app_android`] but allows specifying app configuration.
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     let config = AppConfig::new()
///         .with_title("My App")
///         .with_size(800, 600);
///     flui_app::run_app_android_with_config(app, MyRootView, config);
/// }
/// ```
#[cfg(target_os = "android")]
pub fn run_app_android_with_config<V>(app: android_activity::AndroidApp, root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    init_logging();

    tracing::info!(
        title = %config.title,
        "Starting FLUI application on Android"
    );

    run_android(root, config, app);
}

#[cfg(target_os = "android")]
fn run_android<V>(root: V, config: AppConfig, app: android_activity::AndroidApp)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::{path::PathBuf, sync::Arc};

    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_hot_reload::HotReloadDriver;
    use flui_platform::{
        AndroidPlatform, Platform, WindowOptions,
        traits::{DispatchEventResult, LifecycleEvent, PlatformInput},
    };
    use flui_scheduler::Scheduler;
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting Android platform via flui-platform");

    // Hot-reload: build plugin path from app's internal data directory
    let plugin_path: PathBuf = app
        .internal_data_path()
        .map(|p| p.join("libflui_scene.so"))
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/libflui_scene.so"));

    let hot_reload = Arc::new(Mutex::new(HotReloadDriver::new(&plugin_path)));

    let platform: Box<dyn Platform> = Box::new(AndroidPlatform::new(app));

    // 1. Open window (wraps the existing ANativeWindow) before run()
    let options: WindowOptions = (&config).into();
    let window = platform
        .open_window(options)
        .expect("Failed to create Android window");

    // 2. Create GPU renderer (Vulkan backend on Android)
    let phys_size = window.physical_size();
    let renderer = pollster::block_on(async {
        let handle = PlatformWindowHandle(window.as_ref());
        Renderer::new(&handle).await
    });
    let mut renderer = match renderer {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GPU init failed: {:?}", e);
            return;
        }
    };
    renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

    // 3. Mount root widget (used when no plugin is active) at the
    // LOGICAL size; the paint root's DPR transform maps to physical.
    let scale_factor = window.scale_factor() as f32;
    AppBinding::instance()
        .render_pipeline_mut()
        .set_device_pixel_ratio(scale_factor);
    let ui_realm = match super::ui_realm::UiRealm::new(AppBinding::instance().frame_wake_callback())
    {
        Ok(realm) => realm,
        Err(error) => {
            tracing::error!(%error, "UiRealm construction failed");
            return;
        }
    };
    ui_realm.bind_to_app(AppBinding::instance());
    let logical = window.logical_size();
    let attach = ui_realm.enter(|realm| {
        AppBinding::instance().attach_root_widget_with_size(
            realm,
            &root,
            logical.width.0 as f32,
            logical.height.0 as f32,
        )
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return;
    }
    let realm_dispatch = install_platform_realm(ui_realm);
    register_hit_test_render_view();

    // 4. Wrap renderer for callback sharing
    let renderer = Arc::new(Mutex::new(renderer));

    // 5. Register input callback -> AppBinding::handle_input()
    window.on_input(Box::new(move |input: PlatformInput| {
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Input(input));
        DispatchEventResult::resolved(false, true)
    }));

    // 6. Register frame callback -- with hot-reload plugin override
    let renderer_frame = Arc::clone(&renderer);
    let hot_reload_frame = Arc::clone(&hot_reload);
    window.on_request_frame(Box::new(move || {
        let renderer_frame = Arc::clone(&renderer_frame);
        let hot_reload_frame = Arc::clone(&hot_reload_frame);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Frame(Box::new(move |realm| {
                let mut r = renderer_frame.lock();
                let (w, h) = r.size();
                let mut hr = hot_reload_frame.lock();

                // Poll for plugin updates (mtime check, auto-reload).
                hr.poll(w as f32, h as f32);

                // If a scene plugin is active it owns this presentation frame,
                // but the callback still executes inside the realm entry scope.
                if let Some(scene) = hr.build_scene(w as f32, h as f32) {
                    drop(hr);
                    if let Err(e) = r.render_scene(&scene) {
                        tracing::error!("Plugin render failed: {:?}", e);
                    }
                    return;
                }
                drop(hr);
                drop(r);

                let binding = AppBinding::instance();
                let has_pending = binding.has_pending_work(realm);
                if !binding.needs_redraw()
                    && !has_pending
                    && !Scheduler::instance().is_frame_scheduled()
                {
                    return;
                }

                let now = web_time::Instant::now();
                let scheduler = Scheduler::instance();
                // Scheduler callbacks and rendering share ONE `UiRealm::enter`
                // dynamic extent; callbacks may legally resolve realm-local
                // capabilities throughout the complete frame transaction.
                scheduler.drive_frame(now, || {
                    let mut r = renderer_frame.lock();
                    binding.render_frame_entered(realm, &mut *r);

                    if r.is_device_lost() {
                        match pollster::block_on(r.recover()) {
                            Ok(()) => {
                                tracing::warn!("GPU device lost — recovered successfully");
                                AppBinding::instance().wake_frame();
                            }
                            Err(e) => {
                                tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                            }
                        }
                    }
                });
            })),
        );
    }));

    // 7. Register resize callback -> renderer.resize()
    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let apply_size = size;
        let renderer_resize = Arc::clone(&renderer_resize);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Resize {
                size,
                scale_factor,
                apply_surface: Box::new(move || {
                    let w = (apply_size.width.0 * scale_factor) as u32;
                    let h = (apply_size.height.0 * scale_factor) as u32;
                    renderer_resize.lock().resize(w, h);
                }),
            },
        );
    }));

    // 8. Lifecycle callbacks

    // Platform quit -> transition to Terminating
    platform.on_quit(Box::new(move || {
        tracing::info!("Platform quit");
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Lifecycle(LifecycleEvent::Terminating),
        );
    }));

    // Window close (fired by Android Destroy event)
    window.on_close(Box::new(move || {
        tracing::info!("Window closed");
    }));

    // Window active status -> lifecycle Activated/Deactivated
    window.on_active_status_change(Box::new(move |active| {
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Active(active));
    }));

    // Mark lifecycle as started
    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmEvent::Lifecycle(LifecycleEvent::Started),
    );

    // 9. Request initial redraw
    window.request_redraw();

    // 10. Store window in AppBinding for runtime access
    AppBinding::instance().set_window(window);

    tracing::info!("Android platform initialized with callbacks (hot-reload enabled)");

    // Run the event loop (takes ownership of the platform)
    platform.run(Box::new(|_platform| {
        tracing::info!("Android platform ready");
    }));
    teardown_platform_realm();
}

// ============================================================================
// iOS Implementation
// ============================================================================

#[cfg(target_os = "ios")]
fn run_ios(_config: AppConfig) {
    tracing::info!("iOS platform - not yet implemented");
    // TODO: Implement UIKit integration
}

// ============================================================================
// Web Implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
fn run_web<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::sync::Arc;

    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, LifecycleEvent, PlatformInput},
    };
    use flui_scheduler::Scheduler;
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting web platform via flui-platform");

    let platform = flui_platform::current_platform().expect("Failed to initialize web platform");

    // 1. Open window (creates canvas) before run() since run() takes ownership
    let options: WindowOptions = (&config).into();
    let window: Arc<dyn flui_platform::PlatformWindow> = Arc::from(
        platform
            .open_window(options)
            .expect("Failed to create canvas window"),
    );

    // 2. Shared renderer slot — starts as None, filled async once the WebGPU
    //    adapter is available. `Option` lets the frame callback skip frames that
    //    arrive before the renderer is ready.
    let renderer: Arc<Mutex<Option<Renderer>>> = Arc::new(Mutex::new(None));

    let phys_size = window.physical_size();
    let renderer_init = Arc::clone(&renderer);
    let renderer_window = Arc::clone(&window);

    // The future owns a strong window reference. This is required because the
    // browser platform installs RAF and returns immediately, and startup can
    // also return early before the window reaches AppBinding.
    wasm_bindgen_futures::spawn_local(async move {
        let handle = PlatformWindowHandle(renderer_window.as_ref());
        let mut r = match Renderer::new(&handle).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                return;
            }
        };
        r.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);
        tracing::info!("WebGPU renderer initialized");
        *renderer_init.lock() = Some(r);
    });

    // 3. Mount root widget at the LOGICAL size; the paint root's DPR
    // transform maps to the physical canvas.
    let scale_factor = window.scale_factor() as f32;
    AppBinding::instance()
        .render_pipeline_mut()
        .set_device_pixel_ratio(scale_factor);
    let ui_realm = match super::ui_realm::UiRealm::new(AppBinding::instance().frame_wake_callback())
    {
        Ok(realm) => realm,
        Err(error) => {
            tracing::error!(%error, "UiRealm construction failed");
            return;
        }
    };
    ui_realm.bind_to_app(AppBinding::instance());
    let logical = window.logical_size();
    let attach = ui_realm.enter(|realm| {
        AppBinding::instance().attach_root_widget_with_size(
            realm,
            &root,
            logical.width.0 as f32,
            logical.height.0 as f32,
        )
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return;
    }
    let realm_dispatch = install_platform_realm(ui_realm);
    register_hit_test_render_view();

    // 4. Register input callback
    window.on_input(Box::new(move |input: PlatformInput| {
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Input(input));
        DispatchEventResult::resolved(false, true)
    }));

    // 5. Register frame callback
    let renderer_frame = Arc::clone(&renderer);
    window.on_request_frame(Box::new(move || {
        let renderer_frame = Arc::clone(&renderer_frame);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Frame(Box::new(move |realm| {
                let binding = AppBinding::instance();
                let has_pending = binding.has_pending_work(realm);
                if !binding.needs_redraw()
                    && !has_pending
                    && !Scheduler::instance().is_frame_scheduled()
                {
                    return;
                }

                let now = web_time::Instant::now();
                let scheduler = Scheduler::instance();
                // Scheduler callbacks and rendering share one realm entry.
                scheduler.drive_frame(now, || {
                    let mut slot = renderer_frame.lock();
                    let Some(r) = slot.as_mut() else {
                        return;
                    };

                    binding.render_frame_entered(realm, r);

                    if r.is_device_lost() {
                        drop(slot);
                        let renderer_recover = Arc::clone(&renderer_frame);
                        wasm_bindgen_futures::spawn_local(async move {
                            // Never hold the renderer mutex across `.await`.
                            let Some(mut renderer) = renderer_recover.lock().take() else {
                                return;
                            };
                            let result = renderer.recover().await;
                            *renderer_recover.lock() = Some(renderer);
                            match result {
                                Ok(()) => {
                                    tracing::warn!("GPU device lost — recovered successfully");
                                    AppBinding::instance().wake_frame();
                                }
                                Err(e) => {
                                    tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                                }
                            }
                        });
                    }
                });
            })),
        );
    }));

    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let apply_size = size;
        let renderer_resize = Arc::clone(&renderer_resize);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Resize {
                size,
                scale_factor,
                apply_surface: Box::new(move || {
                    if let Some(renderer) = renderer_resize.lock().as_mut() {
                        let width = (apply_size.width.0 * scale_factor) as u32;
                        let height = (apply_size.height.0 * scale_factor) as u32;
                        renderer.resize(width, height);
                    }
                }),
            },
        );
    }));

    // 6. Lifecycle callbacks
    platform.on_quit(Box::new(move || {
        tracing::info!("Web platform quit");
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Lifecycle(LifecycleEvent::Terminating),
        );
    }));

    window.on_close(Box::new(move || {
        tracing::info!("Canvas window closed");
        // On web, no explicit quit mechanism needed
    }));

    window.on_active_status_change(Box::new(move |active| {
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Active(active));
    }));

    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmEvent::Lifecycle(LifecycleEvent::Started),
    );

    // 7. Store window
    AppBinding::instance().set_shared_window(window);

    tracing::info!("Web platform initialized with callbacks");

    // Run the event loop (takes ownership of the platform)
    platform.run(Box::new(|_platform| {
        tracing::info!("Web platform ready");
    }));
    // `WebPlatform::run` installs the RAF callback and returns immediately;
    // tearing down here would destroy the realm before the first frame. The
    // host therefore remains owner-TLS resident for the page lifetime. An
    // explicit web detach/quit ownership hook is deferred until the platform
    // exposes a callback whose lifetime encloses the RAF registration.
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_types::geometry::px;
    use flui_view::{BuildContext, IntoView, View, ViewExt};

    use super::*;

    // TODO: Will be used in future integration tests for run_app_impl
    #[allow(dead_code)]
    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            TestView.boxed()
        }
    }

    impl View for TestView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[derive(Clone)]
    struct OwnerLocalRoot {
        value: Rc<Cell<usize>>,
    }

    impl StatelessView for OwnerLocalRoot {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.value.set(self.value.get() + 1);
            TestView.boxed()
        }
    }

    impl View for OwnerLocalRoot {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[test]
    fn runner_entrypoints_accept_owner_local_root_state() {
        static_assertions::assert_not_impl_any!(OwnerLocalRoot: Send, Sync);

        std::hint::black_box(run_app_impl::<OwnerLocalRoot> as fn(OwnerLocalRoot));
        std::hint::black_box(
            run_app_with_config_impl::<OwnerLocalRoot> as fn(OwnerLocalRoot, AppConfig),
        );
    }

    #[test]
    fn test_config_creation() {
        let config = AppConfig::new().with_title("Test").with_size(800, 600);

        assert_eq!(config.title, "Test");
        assert_eq!(config.size.width, px(800.0));
    }
}
