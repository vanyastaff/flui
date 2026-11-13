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
use flui_view::View;
use flui_widgets::app_root;
use std::rc::Rc;

use super::frame_pacing::{FallbackGate, WakeAction, frame_is_dirty, wake_action};
use super::host::{
    APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, runtime_wake_callback,
    with_owner_platform,
};
use super::owner_dispatch::{
    RuntimeEvent, RuntimeTask, close_this_window, dispatch_platform_ui_runtime,
    install_input_wiring, prepare_ui_runtime_alongside, teardown_platform_ui_runtime,
};
use super::raster_host::{RasterSlot, raster_wake_at};
use crate::app::hot_reload::WorkerReload;
use crate::app::{AppConfig, LaunchError};

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
                    .map_or_default(SessionController::dispatchers)
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
    // The loop's hooks detach first, as on the desktop: an agent stops
    // before the scenes it serves are retired.
    super::dev_agents::end();
    let controller = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        runtime.ios_running = false;
        runtime.ios_controller.take()
    });
    contain(|| drop(controller));
    teardown_platform_ui_runtime();
}

pub(super) fn run_ios<V>(root: V, config: AppConfig) -> Result<(), LaunchError>
where
    V: View + Clone + 'static,
{
    use flui_native::{IOSPlatform, Platform};
    // Before the platform, whose registries report only to a capture already installed.
    super::begin_native_journal(flui_native::journal::codec::Backend::UiKit);
    let platform = IOSPlatform::try_new().map_err(|error| {
        tracing::error!(%error, "iOS initialization failed");
        LaunchError::from_platform(error)
    })?;
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
    platform.on_scene_event(events, handlers).map_err(|error| {
        tracing::error!(%error, "iOS scene registration failed");
        LaunchError::Platform {
            source: std::sync::Arc::new(error),
        }
    })?;
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
        // The driver agent (ADR-0239), beside the application's own hooks, when
        // the simulator launch named an endpoint. A driver ends the app with
        // `simctl terminate`, so a disconnect asks no quit here.
        let mut config = config;
        let served = crate::app::agent::install_from_launch(
            &mut config,
            |key| with_owner_platform(|owner| owner.launch_value(key)).flatten(),
            &std::sync::Arc::default(),
            super::journals_native(),
        );
        super::keep_native_journal(served);
        let reload = WorkerReload::from_config(&config);
        let watcher = reload.spawn_watcher(runtime_wake_callback());
        // The loop takes the hooks and owns them until the process stops
        // (`stop_process`).
        super::dev_agents::start(&config);
        let installer = Box::new(move |window| {
            bootstrap_ios(root.clone(), config.clone(), reload.clone(), window)
        });
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut().ios_controller = Some(IOSController::new(installer, watcher));
        });
        Ok(())
    }));
    result.map_err(|error| {
        tracing::error!(%error, "iOS platform failed");
        LaunchError::Platform {
            source: std::sync::Arc::new(error),
        }
    })
}

/// The iOS bootstrap: window, GPU slot, UI runtime, and callback wiring.
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
    V: View + Clone + 'static,
{
    let presentation_window = super::presentation_window(Rc::clone(&host))
        .with_pointer_resampling(config.pointer_resampling);
    let window = Rc::clone(presentation_window.window());

    // 0. Wire the wall-clock-wake hook to the runtime's deadlines and the
    // raster host's own (a lost device's next rebuild, a surface's next
    // attempt). CADisplayLink drives visible frames; owner wakes also keep
    // native service and the host's work deliverable between those frames.
    super::host::route_wake_deadline(move || {
        super::host::merge_wake_deadlines(
            APP_RUNTIME.with(|slot| slot.borrow().next_wake()),
            raster_wake_at(),
        )
    });

    // 1. The window's measured geometry; its GPU slot is attached once the UI
    // runtime is installed (step 4).
    let geometry = super::window_install::SurfaceGeometry::of(window.as_ref());
    let phys_size = geometry.physical;

    // 2. Mount the root widget at the LOGICAL size; the paint root's DPR
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
    config.install_frame_failure_handler(&ui_runtime);
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
        ui_runtime.attach_root_widget_with_size(
            &app_root(root.clone()),
            logical.width,
            logical.height,
        )
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
    }
    worker_reload.register_ui_runtime(&ui_runtime);
    // Vended while the ui_runtime is still ours, handed over once the install
    // commits: a failed install drops it with the ui_runtime.
    let agent_window = super::dev_agents::vend(&ui_runtime, ui_runtime.presentation_id());
    let textures = ui_runtime.texture_registry(ui_runtime.presentation_id());
    let mut installation = prepare_ui_runtime_alongside(ui_runtime, Rc::clone(&window));
    installation.text_sizing(native_sizing);
    let owner_dispatch = installation.dispatcher();

    // 4. The scene's slot on the loop's raster executor (ADR-0305): the
    // first scene places the executor on its surface (Metal on iOS), and
    // every later scene is a slot on the same device. The slot holds the
    // window's surface target, never the window (ADR-0254 §5).
    let raster_host = APP_RUNTIME.with(|slot| slot.borrow().raster_host.clone());
    let display = super::host::display_color_of(window.as_ref());
    let attached = raster_host
        .try_with(|raster_host| {
            raster_host.attach(
                window.as_ref(),
                owner_dispatch.address,
                &config,
                textures,
                display,
            )
        })
        .unwrap_or_else(|| Err(super::raster_host::host_busy()));
    if let Err(error) = attached {
        // UIKit owns the process by now, so a refusal cannot reach
        // `run_app`'s caller: it is logged as one, and the scene fails.
        if let Some(unsupported) = crate::app::UnsupportedSystem::from_renderer(&error) {
            tracing::error!(%unsupported, "this system cannot run FLUI");
        } else {
            tracing::error!(%error, "GPU init failed");
        }
        return Err(anyhow::Error::from(error).context("GPU init failed"));
    }
    let raster = RasterSlot {
        host: raster_host,
        address: owner_dispatch.address,
        size: (phys_size.width as u32, phys_size.height as u32),
        device: flui_engine::DeviceHealth::Healthy,
    };
    let surface_host = raster.host.clone();

    // 5. Input -> entered ui_runtime input dispatch.
    install_input_wiring(owner_dispatch, window.as_ref());

    installation.dev_agent(agent_window);
    let frame =
        installation.frame_driver(super::frame_driver::FrameDriver::Ios(IosFrameDriver {
            raster,
            worker_reload,
        }))?;
    let binding = frame.binding;
    let routes = super::window_routes::WindowRoutes::of(&*window);
    routes.route_frame_requested(move |request| {
        let _ = dispatch_platform_ui_runtime(owner_dispatch, RuntimeTask::Frame(binding, request));
    });

    // 8. Lifecycle.
    //
    // Surface availability: the slot's surface is released before the
    // `CAMetalLayer` behind it is torn down, before this callback returns,
    // and made again when one returns (`RasterHost::set_surface`), as on
    // Android. Only the ui_runtime half, the full repaint, is dispatched.
    let surface_address = owner_dispatch.address;
    routes.route_surface_changed(move |change| {
        let available = change == flui_platform::SurfaceChange::Rebuild;
        if surface_host.set_surface(surface_address, available) {
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::PrimarySurfaceRestored),
            );
        }
    });

    // The scene's facts — metrics, safe area, lifecycle, appearance and
    // display — reach the presentation through the window's one snapshot
    // subscription; the resize applier installed above sizes the slot.
    // Registered before the seed is read, so native facts changed during
    // bootstrap cannot be replaced by a synthetic Resumed observation.
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
    raster: RasterSlot,
    worker_reload: WorkerReload,
}

impl IosFrameDriver {
    pub(super) fn installed() {
        tracing::info!("iOS platform initialized with callbacks");
    }

    /// The platform's new size, which layout reads and the next frame
    /// configures the slot for.
    pub(super) fn resize(&mut self, metrics: flui_foundation::geometry::WindowMetrics) {
        let physical = metrics.physical();
        self.raster.size = (
            physical.width.unsigned_abs(),
            physical.height.unsigned_abs(),
        );
    }

    pub(super) fn wake(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
        presentation: flui_foundation::PresentationId,
    ) {
        let worker_reload_frame = &self.worker_reload;
        // The gate half of the wake runs inside one ui_runtime entry and
        // decides; the pump below enters the ui_runtime itself.
        let (action, now, display_color) = ui_runtime.enter(|ui_runtime| {
            let now = web_time::Instant::now();
            // Development reload: applies a queued rebuild at the frame
            // boundary (the `dlopen` stays owner-thread), exactly as the
            // desktop runner does.
            worker_reload_frame.poll_and_apply(ui_runtime);

            let inbox_redraw = ui_runtime.drain_owner_inbox();

            // The host's own deadlines (a lost device's next rebuild, a
            // surface's next attempt) are a dirty source as well as a wake
            // deadline: one the platform faithfully actuates still reaches
            // `WakeAction::Skip` if it is absent from this gate.
            let dirty = frame_is_dirty(
                inbox_redraw,
                ui_runtime.needs_redraw(),
                ui_runtime.has_pending_work(),
                raster_wake_at(),
                FallbackGate::default(),
            );
            let scheduler = ui_runtime.scheduler();
            let action = wake_action(
                scheduler.frames_enabled(),
                dirty,
                scheduler.is_frame_scheduled(),
                FallbackGate::default(),
            );
            (
                action,
                now,
                ui_runtime
                    .window_display(presentation)
                    .and_then(|display| display.color),
            )
        });

        // A presentation whose frames are off cannot be seen: its slot is not
        // drawn, and draws in full when frames come back (ADR-0305).
        if action != WakeAction::Skip {
            self.raster.show(action == WakeAction::Render);
        }
        match action {
            WakeAction::Skip => return,
            WakeAction::PumpBackground => {
                // Frames disabled: service owner commands, async tasks, and
                // input deadlines. A task that asks for another wake is paced
                // by the display link, which ticks at most once per refresh and
                // not at all while backgrounded (ADR-0268).
                ui_runtime.pump_background();
                // A backgrounded scene reports what its earlier frames left.
                self.raster.report_frame_panic(ui_runtime);
                return;
            }
            WakeAction::Render => {}
        }

        // The frame, through the scene's slot (`RasterSlot::pump`).
        let _presented = self.raster.pump(ui_runtime, now, display_color);
    }
}
