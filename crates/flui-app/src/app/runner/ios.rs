//! iOS runner — the native `run_app` path on UIKit.
//!
//! Mirrors the Android runner: `IOSPlatform::run` enters `UIApplicationMain`,
//! the app delegate runs [`bootstrap_ios`] at `didFinishLaunching`, and every
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

use flui_view::{StatelessView, View};
use std::cell::RefCell;
use std::rc::Rc;

use super::device_recovery::{DeviceRecoveryBackoff, render_frame_with_device_recovery};
use super::frame_pacing::{
    BACKGROUNDED_PUMP_PACE, FallbackGate, WakeAction, frame_is_dirty, wake_action,
};
use super::host::{
    APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, runtime_needs_redraw_handle,
    runtime_wake_callback, with_owner_platform,
};
use super::realm_dispatch::{
    PlatformToUi, RealmTask, dispatch_platform_realm, drain_owner_inbox, install_platform_realm,
    install_surface_applier, teardown_platform_realm,
};
use super::surface_lifecycle::{SurfaceLifecycleOutcome, ensure_surface};
use crate::app::AppConfig;
use crate::app::hot_reload::{RebuildHookGuard, WorkerReload, WorkerWatcherGuard};

/// Run a FLUI application on iOS with default configuration.
///
/// Call this from the app's Swift/ObjC entry point (the `FluiAppDelegate`
/// installs it via `flui_ios_main`).
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

fn run_ios<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_platform::{IOSPlatform, Platform};

    let platform: Box<dyn Platform> = match IOSPlatform::new() {
        Ok(platform) => Box::new(platform),
        Err(error) => {
            tracing::error!(%error, "Failed to initialize iOS platform");
            return;
        }
    };

    // Development reload, if this build has it and a worker is configured.
    // With the `hot-reload` feature off this value is inert and
    // `flui-hot-reload` is not in the graph.
    let worker_reload = WorkerReload::from_config(&config);

    // The reload guards can only be created from inside `on_ready` (they need
    // the realm's `wake`), so — exactly as `run_desktop` does — they are
    // threaded back out through cells that outlive `platform.run`, which keeps
    // the rebuild hook attached and the watcher thread alive for the loop's
    // whole life.
    let rebuild_registration: Rc<RefCell<Option<RebuildHookGuard>>> = Rc::new(RefCell::new(None));
    let rebuild_registration_slot = Rc::clone(&rebuild_registration);
    let worker_watcher: Rc<RefCell<Option<WorkerWatcherGuard>>> = Rc::new(RefCell::new(None));
    let worker_watcher_slot = Rc::clone(&worker_watcher);

    // Armed BEFORE `run(...)`, matching the Android and desktop runners: the
    // guard clears the loop-scoped owner host on the way out, including on
    // unwind.
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();

    let result = platform.run(Box::new(move |owner| {
        install_owner_platform(owner)?;
        // `on_ready` returns `Result<(), BootstrapError>` (an opaque boxed
        // error), so the bootstrap's `anyhow::Error` crosses via anyhow's own
        // `From` impl — the same conversion the Android runner's closure
        // relies on.
        bootstrap_ios(
            root,
            config,
            worker_reload,
            rebuild_registration_slot,
            worker_watcher_slot,
        )
        .map_err(Into::into)
    }));

    // Loop exited: detach the hook and stop the watcher before teardown. Take
    // each value out under its borrow, then drop it AFTER the guard falls — the
    // watcher's `Drop` joins a thread, and dropping that under the borrow is
    // the `LockDiscipline/StatementDrop` shape port-check refuses.
    let hook_guard = rebuild_registration.borrow_mut().take();
    let watcher_guard = worker_watcher.borrow_mut().take();
    drop(hook_guard);
    drop(watcher_guard);

    if let Err(error) = result {
        tracing::error!(%error, "iOS platform run returned an error");
    }
}

/// The iOS bootstrap: window, GPU renderer, realm, and callback wiring.
///
/// Runs once, synchronously, inside `on_ready` — which the delegate delivers
/// at `didFinishLaunching`, the first point UIKit permits a window.
fn bootstrap_ios<V>(
    root: V,
    config: AppConfig,
    worker_reload: WorkerReload,
    rebuild_registration_slot: Rc<RefCell<Option<RebuildHookGuard>>>,
    worker_watcher_slot: Rc<RefCell<Option<WorkerWatcherGuard>>>,
) -> anyhow::Result<()>
where
    V: View + StatelessView + Clone + 'static,
{
    use std::sync::Arc;

    use flui_engine::Renderer;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
        with_owner_platform(f).expect("BUG: bootstrap_ios runs only after install_owner_platform")
    }

    // 0. Wire the platform clipboard (ADR-0034).
    let clipboard = owner_platform_installed(|owner| owner.shared().clipboard());
    APP_RUNTIME.with(|slot| slot.borrow().set_platform_clipboard(clipboard));

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

    // 1. Open the window. `Ready` is guaranteed inside `on_ready`.
    let options: WindowOptions = (&config).into();
    let window = match owner_platform_installed(|owner| owner.open_window(options))
        .and_then(flui_platform::WindowOpen::try_ready)
    {
        Ok(window) => window,
        Err(error) => {
            tracing::error!(%error, "Failed to create iOS window");
            return Err(anyhow::Error::from(error).context("Failed to create iOS window"));
        }
    };

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
        realm.attach_root_widget_with_size(&root, logical.width.0 as f32, logical.height.0 as f32)
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
    }
    let hot_reload_sender = ui_realm.command_sender();
    let realm_dispatch = install_platform_realm(ui_realm, &window);

    // 3b. Wire development reload, when a worker is configured. The hook turns
    // a worker-side rebuild request into a queued `HotReload` command; the
    // watcher wakes the loop when the artifact changes, so an idle app (no
    // frames arriving) still reloads. Both are inert when no worker is set.
    {
        let hook_guard = worker_reload.register_rebuild_hook(hot_reload_sender);
        let displaced = {
            let mut slot = rebuild_registration_slot.borrow_mut();
            slot.replace(hook_guard)
        };
        drop(displaced);

        let watcher_guard = worker_reload.spawn_watcher(Arc::clone(&wake));
        let displaced = {
            let mut slot = worker_watcher_slot.borrow_mut();
            std::mem::replace(&mut *slot, watcher_guard)
        };
        drop(displaced);
    }

    // 3b. Start config-declared application services (issue #558).
    for service in &config.services {
        if let Err(error) = APP_RUNTIME.with(|slot| slot.borrow_mut().start_service(service)) {
            tracing::error!(service = service.name(), %error, "service start failed");
            return Err(anyhow::Error::from(error).context(format!(
                "failed to start application service `{}`",
                service.name()
            )));
        }
    }

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

    // Platform quit -> Detached (frames disabled, listeners notified).
    //
    // `UIApplicationMain` never returns, so there is no "after `run`" on this
    // backend: the loop-exit signal arrives as `on_quit`, which the delegate
    // fires from `applicationWillTerminate:`. That makes this handler the
    // place the full teardown runs — the same `teardown_platform_realm` the
    // desktop and Android runners call after their loops exit.
    owner_platform_installed(|owner| {
        owner.shared().on_quit(Box::new(move || {
            let _ =
                dispatch_platform_realm(realm_dispatch, RealmTask::Event(PlatformToUi::Shutdown));
            teardown_platform_realm();
        }));
    });

    window.on_close(Box::new(move || {
        tracing::info!("iOS window closed");
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
    Ok(())
}
