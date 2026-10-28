//! iOS runner — the native `run_app` path on UIKit.
//!
//! Mirrors the Android runner: `IOSPlatform::run` enters `UIApplicationMain`,
//! the app delegate starts process services once, the scene consumer calls
//! [`bootstrap_ios`] for each fresh session, and every
//! later frame is a `CADisplayLink` tick that reaches the same
//! `dispatch_platform_ui_runtime` frame path the other backends use.
//!
//! # Lifecycle
//!
//! The backend already translates UIKit's transitions into the framework's
//! independent observations (`owner/apple/ios.rs`):
//!
//! - focus and visibility → addressed presentation facts.
//! - execution → a per-presentation suspension cap, independent of host lifecycle.
//! - the window's `surface_changed` → drop/rebuild the wgpu surface, so a
//!   `CAMetalLayer`-backed surface is never alive across a suspension.
//!
//! # Hot-reload
//!
//! iOS drives the application's development reload hook the way desktop
//! does: the loop attaches it once, and `bootstrap_ios` polls it at every
//! frame boundary and reassembles the UI runtime on a patch. A worker hook's
//! `dlopen` is usable in the Simulator (and for a dev-signed build), while a
//! production App Store build installs no hook and the whole capability is
//! inert. See [`super::hot_reload`] for the seam and `docs/hot-reload.md` for
//! the two-layer model.

use flui_native::HostWindow;
use flui_native::{IOSSceneEvent, IOSSceneSessionId};
use flui_view::{StatelessView, View};
use std::rc::Rc;
use std::sync::Arc;

use super::device_recovery::{new_device_recovery_backoff, pump_with_device_recovery};
use super::frame_pacing::{
    BACKGROUNDED_PUMP_PACE, FallbackGate, WakeAction, frame_is_dirty, wake_action,
};
use super::host::{
    APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, runtime_wake_callback,
    with_owner_platform,
};
use super::owner_dispatch::{
    RuntimeEvent, RuntimeTask, close_this_window, dispatch_platform_ui_runtime,
    install_input_wiring, prepare_ui_runtime_alongside, teardown_platform_ui_runtime,
};
use super::surface_lifecycle::{
    SurfaceLifecycleOutcome, SurfaceRecreationRetry, report_surface_settlement,
    retry_surface_recreation, settle_surface_availability,
};
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

struct ControllerLease {
    controller: Option<IOSController>,
    host: super::InstalledHost,
}
impl Drop for ControllerLease {
    fn drop(&mut self) {
        let controller = self.controller.take();
        let retired = APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            if runtime.ios_running
                && runtime.installed_host.same_host(&self.host)
                && runtime.ios_controller.is_none()
            {
                std::mem::replace(&mut runtime.ios_controller, controller)
            } else {
                controller
            }
        });
        contain(|| drop(retired));
        // A nested owner turn may have arrived while the controller was
        // leased. Re-arm after restoration instead of losing that wake.
        if APP_RUNTIME.with(|slot| {
            slot.borrow()
                .ios_controller
                .as_ref()
                .is_some_and(SessionController::has_pending)
        }) {
            let _ = with_owner_platform(|owner| owner.proxy().wake());
        }
    }
}

fn lease_controller() -> Option<ControllerLease> {
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state
            .ios_controller
            .take()
            .map(|controller| ControllerLease {
                controller: Some(controller),
                host: state.installed_host.clone(),
            })
    })
}

fn scene_event(event: IOSSceneEvent) -> Result<(), flui_native::BootstrapError> {
    let mut lease = lease_controller()
        .ok_or_else(|| anyhow::anyhow!("iOS process controller is unavailable"))?;
    let controller = lease
        .controller
        .as_mut()
        .expect("BUG: live controller lease");
    match event {
        IOSSceneEvent::Connected {
            session,
            window,
            reconnect,
            installation,
            ..
        } => {
            controller.connect(session.clone(), window, reconnect, installation)?;
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
    // The owner's turn starts with the commands application code queued for
    // its windows: this wake is the one a command asks for.
    super::owner_dispatch::apply_queued_window_commands(std::thread::current().id());
    super::host::drive_native_text_sizing();
    if let Some(mut lease) = lease_controller() {
        lease
            .controller
            .as_mut()
            .expect("BUG: live controller lease")
            .poll();
    }
    // A continuation exists to make progress on roots already carried across
    // the previous finite owner turn. Generating another Pump for every
    // retained scene here could consume the whole budget before that FIFO is
    // reached and, above the budget, grow the queue on every continuation.
    // The guard's tail spends this callback exclusively on the carried batch.
    // Re-arm one ordinary owner opportunity as well: OwnerSignal coalesces
    // causes, so this physical callback may also represent an async/frame wake
    // whose per-session Pumps must run after the backlog has made progress.
    super::owner_dispatch::drive_fanout_owner_callback(
        || {
            let dispatchers = APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .ios_controller
                    .as_ref()
                    .map(SessionController::dispatchers)
                    .unwrap_or_default()
            });
            for dispatcher in dispatchers {
                let _ = dispatcher.runtime().background();
            }
        },
        || {
            let _ = with_owner_platform(|owner| owner.proxy().wake());
        },
    );
}

fn stop_process() {
    let controller = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        runtime.ios_running = false;
        runtime.ios_controller.take()
    });
    contain(|| drop(controller));
    teardown_platform_ui_runtime();
}

fn run_ios<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use flui_native::{IOSPlatform, Platform};
    let platform = match IOSPlatform::new() {
        Ok(platform) => platform,
        Err(error) => {
            tracing::error!(%error, "iOS initialization failed");
            return;
        }
    };
    // The loop makes a session's window, with its handler, before the
    // session connects; the connection names the session and its window, so
    // the routes made for that session are adopted for that window, whatever
    // order sessions are made and connected in.
    let made = Rc::new(std::cell::RefCell::new(std::collections::HashMap::<
        flui_native::IOSSceneSessionId,
        Rc<super::window_routes::WindowRoutes>,
    >::new()));
    let adopting = Rc::clone(&made);
    let handlers = Box::new(move |session: &flui_native::IOSSceneSessionId| {
        let routes = super::window_routes::WindowRoutes::new();
        made.borrow_mut()
            .insert(session.clone(), Rc::clone(&routes));
        routes as Rc<dyn flui_platform::WindowHandler>
    });
    let events = Box::new(move |event: IOSSceneEvent| {
        if let IOSSceneEvent::Connected {
            session, window, ..
        } = &event
            && let Some(routes) = adopting.borrow_mut().remove(session)
        {
            routes.adopt(window.id());
        }
        if let IOSSceneEvent::Discarded { session } = &event {
            adopting.borrow_mut().remove(session);
        }
        scene_event(event)
    });
    if let Err(error) = platform.on_scene_event(events, handlers) {
        tracing::error!(%error, "iOS scene registration failed");
        return;
    }
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();
    let result = Box::new(platform).run(crate::app::runner::app_handler(move |owner| {
        install_owner_platform(owner)?;
        with_owner_platform(|owner| owner.on_wake(Box::new(drive_owner)))
            .expect("BUG: owner installed above")?;
        super::host::route_quit(stop_process);
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            runtime.ios_running = true;
            if let Some(executors) = config.executors.clone() {
                runtime.install_host_executors(executors);
            }
            runtime.reopen_lifecycles();
            runtime.ensure_execution();
        });
        super::host::install_host_storage(&config);
        for service in &config.services {
            super::host::start_service(service)?;
        }
        let reload = WorkerReload::from_config(&config);
        let watcher = reload.spawn_watcher(runtime_wake_callback());
        let agent = config
            .dev_agent
            .as_ref()
            .and_then(crate::app::dev_agent::DevAgent::attach);
        let installer = Box::new(move |window| {
            bootstrap_ios(root.clone(), config.clone(), reload.clone(), window)
        });
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut().ios_controller = Some(IOSController::new(installer, watcher, agent));
        });
        Ok(())
    }));
    if let Err(error) = result {
        tracing::error!(%error, "iOS platform failed");
    }
}

/// The iOS bootstrap: window, GPU renderer, UI runtime, and callback wiring.
///
/// Runs once per fresh logical scene session, before native publication. A
/// reconnect retains this UI runtime and does not call the installer again.
fn bootstrap_ios<V>(
    root: V,
    config: AppConfig,
    worker_reload: WorkerReload,
    host: Rc<dyn HostWindow>,
) -> anyhow::Result<super::installed_host::Installation>
where
    V: View + StatelessView + Clone + 'static,
{
    use std::sync::Arc;

    use flui_engine::Renderer;
    use parking_lot::Mutex;

    let presentation_window = super::presentation_window(Rc::clone(&host))
        .with_pointer_resampling(config.pointer_resampling);
    let window = Rc::clone(presentation_window.window());

    // 0b. This window's device-recovery backoff, constructed before the
    // wake-deadline hook below so the hook can carry its deadline.
    let device_recovery_backoff = Arc::new(new_device_recovery_backoff());

    // 0b-2. Automatic surface-recreation retry: a genuine rebuild failure
    // leaves the presentation released, and on a window that stays available
    // nothing would re-ask (see `SurfaceRecreationRetry`'s own doc). The retry
    // is deadline-paced exactly like device recovery, and its deadline joins
    // the same wake hook below.
    let surface_recreation_retry = Arc::new(SurfaceRecreationRetry::new());
    // Separate handle for the availability callback below: the frame closure
    // consumes the original by value on capture, so the callback needs its own
    // `Arc` clone to share the same retry state.
    let surface_retry_for_callback = Arc::clone(&surface_recreation_retry);

    // 0c. Wire the wall-clock-wake hook to runtime/native-service deadlines and
    // both recovery backoffs. CADisplayLink drives visible frames; owner wakes
    // also keep native service and recovery deliverable between those frames.
    {
        let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
        let surface_recreation_retry = Arc::clone(&surface_recreation_retry);
        super::host::route_wake_deadline(move || {
            let device = device_recovery_backoff.next_attempt_at();
            let surface = surface_recreation_retry.next_attempt_at();
            super::host::merge_wake_deadlines(
                APP_RUNTIME.with(|slot| slot.borrow().next_wake()),
                super::host::merge_wake_deadlines(device, surface),
            )
        });
    }

    // 2. Create the GPU renderer (Metal on iOS). `Renderer::new` takes
    // ownership of a `WindowTarget` (issue #1043): the window's surface
    // target, never the window (ADR-0254 §5).
    let geometry = super::window_install::SurfaceGeometry::of(window.as_ref());
    let phys_size = geometry.physical;
    let renderer = pollster::block_on(Renderer::with_surface_color(
        window.surface_target(),
        config.surface_color,
    ));
    let mut renderer = match renderer {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GPU init failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("GPU init failed"));
        }
    };
    renderer.resize(phys_size.width as u32, phys_size.height as u32);
    renderer.set_frame_latency(config.frame_latency);

    // 3. Mount the root widget at the LOGICAL size; the paint root's DPR
    // transform maps to physical.
    let scale_factor = geometry.scale;
    let ui_runtime = match super::host::build_ui_runtime(presentation_window, scale_factor) {
        Ok(ui_runtime) => ui_runtime,
        Err(error) => {
            tracing::error!(%error, "UiRuntime construction failed");
            return Err(anyhow::anyhow!(error).context("UiRuntime construction failed"));
        }
    };

    ui_runtime.set_performance_overlay(config.show_performance_overlay);
    ui_runtime.set_motion_override(config.motion_override);
    ui_runtime.set_frame_failure_handler(config.frame_failure_handler.clone());
    ui_runtime.set_frame_failure_detail(config.frame_failure_detail);
    config.install_input_tap(&ui_runtime);
    config.install_error_view_builder(&ui_runtime);

    let logical = geometry.logical;
    let owner = APP_RUNTIME
        .with(|slot| slot.borrow().owner_platform.clone())
        .expect("BUG: UIKit owner installed before scene bootstrap");
    let native_sizing = super::native_text_sizing::NativeTextSizing::prepare(
        flui_foundation::PresentationAddress {
            ui_runtime_id: ui_runtime.id(),
            presentation_id: ui_runtime.presentation_id(),
        },
        host,
        owner,
    );
    ui_runtime
        .install_captured_text_sizing_for(ui_runtime.presentation_id(), native_sizing.source());
    let attach = ui_runtime.enter(|ui_runtime| {
        ui_runtime.attach_root_widget_with_size(&root, logical.width, logical.height)
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
    }
    worker_reload.register_ui_runtime(&ui_runtime);
    // Vended while the ui_runtime is still ours, handed over once the install
    // commits: a failed install drops it with the ui_runtime.
    let agent_window = config
        .dev_agent
        .as_ref()
        .and_then(|agent| agent.vend(&ui_runtime, ui_runtime.presentation_id()));
    let textures = ui_runtime.texture_registry(ui_runtime.presentation_id());
    let mut installation = prepare_ui_runtime_alongside(ui_runtime, Rc::clone(&window));
    installation.text_sizing(native_sizing);
    let owner_dispatch = installation.dispatcher();

    // 4. Adopt the raster mailbox (ADR-0045's inline lane).
    let lane = Arc::new(Mutex::new(
        crate::app::raster_lane::RasterLane::new(
            renderer,
            owner_dispatch.address,
            phys_size.width as u32,
            phys_size.height as u32,
        )
        .with_textures(textures),
    ));

    super::host::track_raster_lane(lane.lock().reclaim_handle());

    // 5. Input -> entered ui_runtime input dispatch.
    install_input_wiring(owner_dispatch, window.as_ref());

    installation.dev_agent(config.dev_agent.zip(agent_window));
    let frame =
        installation.frame_driver(super::frame_driver::FrameDriver::Ios(IosFrameDriver {
            lane: Arc::clone(&lane),
            worker_reload,
            device_recovery_backoff,
            surface_recreation_retry,
        }))?;
    let binding = frame.binding;
    let routes = super::window_routes::WindowRoutes::of(&*window);
    routes.route_frame_requested(move |request| {
        let _ = dispatch_platform_ui_runtime(owner_dispatch, RuntimeTask::Frame(binding, request));
    });

    // 8. Lifecycle.
    //
    // Surface availability: release the wgpu surface before the
    // `CAMetalLayer` behind it is torn down, and rebuild when one returns.
    // The same shape Android uses — the engine's tracker mark runs inside
    // the lane lock with the mint, and only the ui_runtime half is dispatched.
    let lane_surface = Arc::clone(&lane);
    routes.route_surface_changed(move |change| {
        let has_surface = change == flui_platform::SurfaceChange::Rebuild;
        let now = web_time::Instant::now();
        let mut lane = lane_surface.lock();
        let outcome =
            settle_surface_availability(&mut lane, &surface_retry_for_callback, has_surface, now);
        // The guard ends before the ui_runtime dispatch — see Android's matching
        // route for the same-thread hazard that puts the `drop` here.
        drop(lane);
        if matches!(outcome, SurfaceLifecycleOutcome::Recreated) {
            // The ui_runtime half is deferrable, so it goes through the ui_runtime
            // dispatch rather than running inline.
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::PrimarySurfaceRestored),
            );
        }
        report_surface_settlement("iOS", &outcome);
    });

    // The scene's facts — metrics, safe area, lifecycle, appearance and
    // display — reach the presentation through the window's one snapshot
    // subscription; the resize applier installed above touches the renderer.
    // Registered before the seed is read, so native facts changed during
    // renderer bootstrap cannot be replaced by a synthetic Resumed
    // observation.
    installation.observe_window();

    installation.on_close(move || {
        close_this_window(owner_dispatch);
    });

    // 9. Store the window in the redraw-poke slot BEFORE the initial redraw.
    APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

    debug_assert_eq!(
        std::thread::current().id(),
        owner_dispatch.owner_thread,
        "iOS bootstrap must run on the ui_runtime's owner thread"
    );

    Ok(installation.submit())
}

pub(super) struct IosFrameDriver {
    lane: Arc<parking_lot::Mutex<crate::app::raster_lane::RasterLane<flui_engine::Renderer>>>,
    worker_reload: WorkerReload,
    device_recovery_backoff: Arc<super::device_recovery::DeviceRecoveryBackoff>,
    surface_recreation_retry: Arc<SurfaceRecreationRetry>,
}

impl IosFrameDriver {
    pub(super) fn installed() {
        tracing::info!("iOS platform initialized with callbacks");
    }
    pub(super) fn resize(&mut self, metrics: flui_foundation::geometry::WindowMetrics) {
        let resize = self.lane.lock().resize_hook();
        let physical = metrics.physical();
        resize.apply(
            physical.width.unsigned_abs(),
            physical.height.unsigned_abs(),
        );
    }
    pub(super) fn wake(&mut self, ui_runtime: &mut crate::app::ui_runtime::UiRuntime) {
        let lane_frame = &self.lane;
        let worker_reload_frame = &self.worker_reload;
        let device_recovery_backoff = &self.device_recovery_backoff;
        let surface_recreation_retry = &self.surface_recreation_retry;
        // The gate half of the wake runs inside one ui_runtime entry and
        // decides; the pump below enters the ui_runtime itself.
        let (action, now) = ui_runtime.enter(|ui_runtime| {
            let now = web_time::Instant::now();
            // Development reload: applies a queued rebuild at the frame
            // boundary (the `dlopen` stays owner-thread), exactly as the
            // desktop runner does.
            worker_reload_frame.poll_and_apply(ui_runtime);

            let inbox_redraw = ui_runtime.drain_owner_inbox();

            let has_pending = ui_runtime.has_pending_work();
            // The surface-retry deadline joins the device-recovery one in
            // the `dirty` predicate, for the same reason that one must be
            // present: a deadline the platform faithfully actuates still
            // reaches `WakeAction::Skip` and returns before the retry is
            // consulted if it is absent from this gate.
            let retry_deadline = super::host::merge_wake_deadlines(
                device_recovery_backoff.next_attempt_at(),
                surface_recreation_retry.next_attempt_at(),
            );
            let dirty = frame_is_dirty(
                inbox_redraw,
                ui_runtime.needs_redraw(),
                has_pending,
                retry_deadline,
                FallbackGate::default(),
            );
            let scheduler = ui_runtime.scheduler();
            let action = wake_action(
                scheduler.frames_enabled(),
                dirty,
                scheduler.is_frame_scheduled(),
                FallbackGate::default(),
            );
            if action != WakeAction::Render {
                return (action, now);
            }

            // A retry owed by a genuine surface-recreation failure gets
            // its gated attempt here, BEFORE the frame, through the shared
            // helper (its own lane-lock scope, released before the ui_runtime
            // half). This closure already runs as the ui_runtime's Pump task, so
            // the full-repaint mark goes to `ui_runtime` directly and the frame
            // about to run is the one that repaints into the new surface —
            // re-dispatching it as another task would queue it behind
            // this one (the dispatcher is mid-phase) and land it a frame late.
            match retry_surface_recreation(lane_frame, surface_recreation_retry, now) {
                Some(SurfaceLifecycleOutcome::Recreated) => {
                    ui_runtime.mark_primary_needs_full_repaint();
                }
                Some(SurfaceLifecycleOutcome::Failed(source)) => {
                    tracing::warn!(
                        platform = "iOS",
                        ?source,
                        "surface recreation retry failed; the deadline-paced retry \
                             continues"
                    );
                }
                Some(SurfaceLifecycleOutcome::Released) | None => {}
            }
            (action, now)
        });

        match action {
            WakeAction::Skip => return,
            WakeAction::PumpBackground => {
                // Frames disabled: service owner commands, async tasks, and
                // input deadlines, then pace a self-re-arming task.
                ui_runtime.pump_background();
                std::thread::sleep(BACKGROUNDED_PUMP_PACE);
                return;
            }
            WakeAction::Render => {}
        }

        // The frame: `UiRuntime::pump` at `now`, with device-loss
        // recovery around it (`pump_with_device_recovery`).
        if pump_with_device_recovery(ui_runtime, lane_frame, device_recovery_backoff, now).is_none()
        {
            tracing::error!("frame skipped: raster lane busy at frame admission");
        }
    }
}
