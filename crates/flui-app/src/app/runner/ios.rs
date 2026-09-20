//! iOS runner — the native `run_app` path on UIKit.
//!
//! Mirrors the Android runner: `IOSPlatform::run` enters `UIApplicationMain`,
//! the app delegate starts process services once, the scene consumer calls
//! [`bootstrap_ios`] for each fresh session, and every
//! later frame is a `CADisplayLink` tick that reaches the same
//! `dispatch_platform_realm` frame path the other backends use.
//!
//! # Lifecycle
//!
//! The backend already translates UIKit's transitions into the framework's
//! independent observations (`platforms/ios/platform.rs`):
//!
//! - focus and visibility → addressed presentation facts.
//! - execution → a per-presentation suspension cap, independent of host lifecycle.
//! - `on_surface_status_change` → drop/rebuild the wgpu surface, so a
//!   `CAMetalLayer`-backed surface is never alive across a suspension.
//!
//! # Hot-reload
//!
//! iOS runs the same host/worker split as desktop and Android: `bootstrap_ios`
//! loads a worker dylib when one is configured, watches the artifact, and
//! reassembles on change. The `dlopen` path is the shared `flui-hot-reload`
//! `DynLib`; on iOS this is usable in the Simulator (and for a dev-signed
//! build), while a production App Store build has no mutable dylib to load and
//! simply runs static — `AppConfig`'s worker field is `None` and the whole
//! capability is inert. See [`super::hot_reload`] for the seam and
//! `docs/hot-reload.md` for the two-layer model.

use flui_platform::PlatformWindow;
use flui_platform::platforms::ios::{IOSSceneEvent, IOSSceneSessionId};
use flui_view::{StatelessView, View};
use std::sync::Arc;

use super::device_recovery::{DeviceRecoveryBackoff, render_frame_with_device_recovery};
use super::frame_pacing::{
    BACKGROUNDED_PUMP_PACE, FallbackGate, WakeAction, frame_is_dirty, wake_action,
};
use super::host::{
    APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, runtime_needs_redraw_handle,
    runtime_wake_callback, with_owner_platform,
};
use super::realm_dispatch::{
    PlatformToUi, RealmDispatcher, RealmTask, close_this_window, dispatch_platform_realm,
    drain_owner_inbox, install_realm_alongside, install_surface_applier, teardown_platform_realm,
};
use super::surface_lifecycle::{SurfaceLifecycleOutcome, ensure_surface};
use crate::app::AppConfig;
use crate::app::hot_reload::WorkerReload;

/// Run a FLUI application on iOS with default configuration.
///
/// Call this from the Rust application entry point. UIKit owns the process loop.
pub fn run_app_ios<V>(root: V)
where
    V: View + StatelessView + Clone + 'static,
{
    run_app_ios_with_config(root, AppConfig::default());
}

/// Run a FLUI application on iOS with custom configuration.
pub fn run_app_ios_with_config<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    let _installation = crate::app::logging::init_managed_logging(&config);
    tracing::info!(title = %config.title, "Starting FLUI application on iOS");
    run_ios(root, config);
}

use super::session_controller::{SessionController, contain};
pub(in crate::app) type IOSController = SessionController<IOSSceneSessionId>;

fn scene_event(event: IOSSceneEvent) -> Result<(), flui_platform::BootstrapError> {
    let controller = APP_RUNTIME
        .with(|slot| slot.borrow_mut().ios_controller.take())
        .ok_or_else(|| anyhow::anyhow!("iOS process controller is unavailable"))?;
    struct Lease(Option<IOSController>);
    impl Drop for Lease {
        fn drop(&mut self) {
            let controller = self.0.take();
            let retired = APP_RUNTIME.with(|slot| {
                let mut runtime = slot.borrow_mut();
                if runtime.ios_running {
                    std::mem::replace(&mut runtime.ios_controller, controller)
                } else {
                    controller
                }
            });
            contain(|| drop(retired));
            // A nested owner turn may have arrived while the controller was
            // leased. Re-arm after restoration instead of losing that wake.
            if APP_RUNTIME.with(|slot| slot.borrow().ios_controller.is_some()) {
                let _ = with_owner_platform(|owner| owner.proxy().wake());
            }
        }
    }
    let mut lease = Lease(Some(controller));
    let controller = lease.0.as_mut().expect("BUG: live controller lease");
    match event {
        IOSSceneEvent::Connected {
            session,
            window,
            reconnect,
            ..
        } => {
            controller.connect(session.clone(), window, reconnect)?;
            if !APP_RUNTIME.with(|slot| slot.borrow().ios_running) {
                controller.discard(&session);
                return Err(anyhow::anyhow!("iOS process stopped during installation").into());
            }
        }
        IOSSceneEvent::Disconnected { .. } => {}
        IOSSceneEvent::Discarded { session }
        | IOSSceneEvent::InstallationAborted { session, .. } => {
            controller.discard(&session);
            let old = APP_RUNTIME.with(|slot| slot.borrow().clear_redraw_window());
            drop(old);
        }
    }
    Ok(())
}

fn drive_owner() {
    let dispatchers = APP_RUNTIME.with(|slot| {
        slot.borrow()
            .ios_controller
            .as_ref()
            .map(SessionController::dispatchers)
            .unwrap_or_default()
    });
    for dispatcher in dispatchers {
        let _ = dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(|realm| {
                drain_owner_inbox(realm);
                let scheduler = realm.scheduler();
                scheduler.finish_async_pump();
                scheduler.drive_async_tasks();
            })),
        );
    }
}

fn stop_process() {
    let controller = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        runtime.ios_running = false;
        runtime.ios_controller.take()
    });
    contain(|| drop(controller));
    teardown_platform_realm();
}

fn run_ios<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use flui_platform::{IOSPlatform, Platform};
    let platform = match IOSPlatform::new() {
        Ok(platform) => platform,
        Err(error) => {
            tracing::error!(%error, "iOS initialization failed");
            return;
        }
    };
    if let Err(error) = platform.on_scene_event(Box::new(scene_event)) {
        tracing::error!(%error, "iOS scene registration failed");
        return;
    }
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();
    let result = Box::new(platform).run(Box::new(move |owner| {
        install_owner_platform(owner)?;
        with_owner_platform(|owner| {
            owner.on_wake(Box::new(drive_owner))?;
            owner.shared().on_quit(Box::new(stop_process));
            Ok::<_, flui_platform::WakeRegistrationError>(())
        })
        .expect("BUG: owner installed above")?;
        let clipboard = with_owner_platform(|owner| owner.shared().clipboard())
            .expect("BUG: owner installed above");
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            runtime.ios_running = true;
            if let Some(executors) = config.executors.clone() {
                runtime.install_host_executors(executors);
            }
            runtime.reopen_lifecycles();
            runtime.ensure_execution();
            runtime.set_platform_clipboard(clipboard);
        });
        for service in &config.services {
            APP_RUNTIME.with(|slot| slot.borrow_mut().start_service(service))?;
        }
        let reload = WorkerReload::from_config(&config);
        let watcher = reload.spawn_watcher(runtime_wake_callback());
        let installer = Box::new(move |window| {
            bootstrap_ios(root.clone(), config.clone(), reload.clone(), window)
        });
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut().ios_controller = Some(IOSController::new(installer, watcher));
        });
        Ok(())
    }));
    if let Err(error) = result {
        tracing::error!(%error, "iOS platform failed");
    }
}

/// The iOS bootstrap: window, GPU renderer, realm, and callback wiring.
///
/// Runs once per fresh logical scene session, before native publication. A
/// reconnect retains this realm and does not call the installer again.
fn bootstrap_ios<V>(
    root: V,
    config: AppConfig,
    worker_reload: WorkerReload,
    window: Arc<dyn PlatformWindow>,
) -> anyhow::Result<RealmDispatcher>
where
    V: View + StatelessView + Clone + 'static,
{
    use std::sync::Arc;

    use flui_engine::Renderer;
    use flui_platform::traits::{DispatchEventResult, PlatformInput};
    use parking_lot::Mutex;

    fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
        with_owner_platform(f).expect("BUG: bootstrap_ios runs only after install_owner_platform")
    }

    // 0b. This window's device-recovery backoff, constructed before the
    // wake-deadline hook below so the hook can carry its deadline.
    let device_recovery_backoff = Arc::new(DeviceRecoveryBackoff::new());

    // 0c. Wire the wall-clock-wake hook to the backoff. Like Android, this
    // does NOT fold in realm-level deadlines — the backend's frame source is
    // the `CADisplayLink`, and this hook exists to carry a recovery deadline.
    owner_platform_installed(|owner| {
        let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
        owner
            .shared()
            .set_wake_deadline_hook(Box::new(move || device_recovery_backoff.next_attempt_at()));
    });

    // 2. Create the GPU renderer (Metal on iOS). `Renderer::new` takes its own
    // strong `Arc` of the window as the surface target (issue #1043).
    let phys_size = window.physical_size();
    let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)));
    let mut renderer = match renderer {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GPU init failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("GPU init failed"));
        }
    };
    renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

    // 3. Mount the root widget at the LOGICAL size; the paint root's DPR
    // transform maps to physical.
    let scale_factor = window.scale_factor() as f32;
    let wake = runtime_wake_callback();
    let ui_realm = match crate::app::ui_realm::UiRealm::new(
        Arc::clone(&wake),
        Arc::clone(&window),
        scale_factor,
        runtime_needs_redraw_handle(),
    ) {
        Ok(realm) => realm,
        Err(error) => {
            tracing::error!(%error, "UiRealm construction failed");
            return Err(anyhow::anyhow!(error).context("UiRealm construction failed"));
        }
    };

    ui_realm.set_performance_overlay(config.show_performance_overlay);
    ui_realm.set_frame_failure_handler(config.frame_failure_handler.clone());
    ui_realm.set_frame_failure_detail(config.frame_failure_detail);

    let logical = window.logical_size();
    let attach = ui_realm.enter(|realm| {
        realm.attach_root_widget_with_size(&root, logical.width.0, logical.height.0)
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
    }
    let hot_reload_sender = ui_realm.command_sender();
    let realm_dispatch = install_realm_alongside(ui_realm, &window)?;
    struct ProvisionalRealm(Option<RealmDispatcher>);
    impl Drop for ProvisionalRealm {
        fn drop(&mut self) {
            if let Some(dispatcher) = self.0.take() {
                contain(|| close_this_window(dispatcher));
            }
        }
    }
    let mut provisional = ProvisionalRealm(Some(realm_dispatch));

    let rebuild_guard: crate::app::hot_reload::RebuildHookGuard =
        worker_reload.register_rebuild_hook(hot_reload_sender);

    // 4. Adopt the raster mailbox (ADR-0045's inline lane).
    let lane = Arc::new(Mutex::new(crate::app::raster_lane::RasterLane::new(
        renderer,
        realm_dispatch.address,
        phys_size.width.0 as u32,
        phys_size.height.0 as u32,
    )));

    {
        let resize_hook = lane.lock().resize_hook();
        install_surface_applier(
            realm_dispatch.address.realm_id,
            move |size, scale_factor| {
                let w = (size.width.0 * scale_factor) as u32;
                let h = (size.height.0 * scale_factor) as u32;
                resize_hook.apply(w, h);
            },
        );
    }

    // 5. Input -> entered realm input dispatch.
    window.on_input(Box::new(move |input: PlatformInput| {
        let _ =
            dispatch_platform_realm(realm_dispatch, RealmTask::Event(PlatformToUi::Input(input)));
        DispatchEventResult::resolved(false, true)
    }));

    // 6. Frame callback — the `CADisplayLink` tick lands here.
    let lane_frame = Arc::clone(&lane);
    let worker_reload_frame = worker_reload.clone();
    window.on_request_frame(Box::new(move || {
        let lane_frame = Arc::clone(&lane_frame);
        let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
        let worker_reload_frame = worker_reload_frame.clone();
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Frame(Box::new(move |realm| {
                // Development reload: applies a queued rebuild at the frame
                // boundary (the `dlopen` stays owner-thread), exactly as the
                // desktop runner does.
                worker_reload_frame.poll_and_apply(realm);

                let inbox_redraw = drain_owner_inbox(realm);

                let has_pending = realm.has_pending_work();
                let dirty = frame_is_dirty(
                    inbox_redraw,
                    realm.needs_redraw(),
                    has_pending,
                    device_recovery_backoff.next_attempt_at(),
                    FallbackGate::default(),
                );
                let scheduler = realm.scheduler();
                match wake_action(
                    scheduler.frames_enabled(),
                    dirty,
                    scheduler.is_frame_scheduled(),
                    FallbackGate::default(),
                ) {
                    WakeAction::Skip => return,
                    WakeAction::PumpAsync => {
                        // Frames disabled (backgrounded): pump only the async
                        // driver, then sleep to bound a self-re-arming task.
                        scheduler.finish_async_pump();
                        scheduler.drive_async_tasks();
                        std::thread::sleep(BACKGROUNDED_PUMP_PACE);
                        return;
                    }
                    WakeAction::Render => {}
                }

                let now = web_time::Instant::now();
                scheduler.drive_frame_with_lane(
                    now,
                    flui_scheduler::IdleDeadline::far_future(now),
                    || {
                        let Some(mut lane) = lane_frame.try_lock() else {
                            tracing::error!(
                                "frame skipped: raster lane already held by an outer frame \
                                 dispatch"
                            );
                            return;
                        };
                        let _ = render_frame_with_device_recovery(
                            realm,
                            &mut *lane,
                            &device_recovery_backoff,
                            now,
                        );
                    },
                    realm.local_post_frame_lane(),
                );
            })),
        );
    }));

    // 7. Resize -> typed Resized event; the applier installed above touches
    // the renderer.
    window.on_resize(Box::new(move |size, scale_factor| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Resized { size, scale_factor }),
        );
    }));

    // Safe-area intrusions: addressed to this window's presentation, which
    // republishes the root `MediaQuery` without touching renderer geometry.
    window.on_safe_area_change(Box::new(move |insets| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::SafeAreaChanged(insets)),
        );
    }));

    // 8. Lifecycle.
    //
    // Surface availability: release the wgpu surface before the
    // `CAMetalLayer` behind it is torn down, and rebuild when one returns.
    // The same shape Android uses — the engine's tracker mark runs inside
    // the lane lock with the mint, and only the realm half is dispatched.
    let lane_surface = Arc::clone(&lane);
    window.on_surface_status_change(Box::new(move |has_surface| {
        let mut lane = lane_surface.lock();
        let outcome = lane.with_backend(|renderer| ensure_surface(renderer, has_surface));
        if matches!(&outcome, SurfaceLifecycleOutcome::Recreated) {
            lane.note_surface_recreated();
        }
        drop(lane);
        match outcome {
            SurfaceLifecycleOutcome::Released => {}
            SurfaceLifecycleOutcome::Recreated => {
                let _ = dispatch_platform_realm(
                    realm_dispatch,
                    RealmTask::Frame(Box::new(|realm| {
                        realm.mark_primary_needs_full_repaint();
                    })),
                );
            }
            SurfaceLifecycleOutcome::Failed(source) => {
                tracing::warn!(
                    source = ?source,
                    "iOS: the wgpu surface could not be rebuilt after the window was \
                     reported available"
                );
            }
        }
    }));

    window.on_active_status_change(Box::new(move |focused| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowFocus(focused)),
        );
    }));
    window.on_visibility_status_change(Box::new(move |visible| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowVisibility(visible)),
        );
    }));
    window.on_execution_state_change(Box::new(move |state| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowExecution(state)),
        );
    }));
    // Register before sampling, so native facts changed during renderer bootstrap
    // cannot be replaced by a synthetic Resumed observation.
    let execution = window.execution_state();
    let focused = window.is_focused();
    let visible = window.is_visible();
    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmTask::Frame(Box::new(move |realm| {
            realm.synchronize_window_snapshot(
                realm_dispatch.address.presentation_id,
                execution,
                focused,
                visible,
            );
        })),
    );

    window.on_close(Box::new(move || {
        contain(|| drop(rebuild_guard));
        close_this_window(realm_dispatch);
    }));

    // 9. Store the window in the redraw-poke slot BEFORE the initial redraw.
    APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

    debug_assert_eq!(
        std::thread::current().id(),
        realm_dispatch.owner_thread,
        "iOS bootstrap must run on the realm's owner thread"
    );

    // 10. Request the initial redraw.
    wake();

    tracing::info!("iOS platform initialized with callbacks");
    provisional.0 = None;
    Ok(realm_dispatch)
}
