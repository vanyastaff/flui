use flui_scheduler::AppLifecycleState;
use flui_view::View;
use flui_widgets::app_root;

use super::frame_pacing::{
    BackgroundPace, FallbackGate, WakeAction, frame_is_dirty, keeps_frame_gate_open, wake_action,
};
use super::host::{APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, with_owner_platform};
use super::owner_dispatch::{
    ExitWatchdog, RuntimeEvent, RuntimeTask, dispatch_platform_ui_runtime, install_input_wiring,
    prepare_platform_ui_runtime, teardown_platform_ui_runtime_logged,
};
use super::raster_host::{RasterSlot, raster_wake_at};
use crate::app::AppConfig;

// ============================================================================
// Android Implementation
// ============================================================================

#[cfg(target_os = "android")]
/// Run `root` in the Activity's window `app` names, with `config`.
pub(super) fn run_android<V>(
    root: V,
    config: AppConfig,
    app: android_activity::AndroidApp,
) -> Result<(), crate::app::LaunchError>
where
    V: View + Clone + 'static,
{
    use std::sync::Arc;

    use flui_native::Platform;
    use flui_platform::WindowOptions;

    use crate::app::hot_reload::ScenePlugin;

    tracing::info!("Starting Android platform via flui-native");

    // The application's development reload hook may own frames with a scene
    // plugin (`flui run --scene`); inert unless one is installed.
    let hot_reload = ScenePlugin::from_config(&config, app.internal_data_path().as_deref());
    // Before the platform, whose registries report only to a capture already installed.
    super::begin_native_journal(flui_native::journal::codec::Backend::Android);
    let platform: Box<dyn Platform> =
        flui_native::android_platform(app).map_err(crate::app::LaunchError::from_platform)?;

    // The driver agent (ADR-0239), beside the application's own hooks, when
    // the launching intent named an endpoint (ADR-0288 §7). A driver that goes
    // under `EXIT_ON_DISCONNECT` quits the loop, and the process exits once
    // the teardown ran.
    let disconnect = Arc::new(crate::app::agent::Disconnect::default());
    let mut config = config;
    let served = crate::app::agent::install_from_launch(
        &mut config,
        |key| platform.launch_value(key),
        &disconnect,
        super::journals_native(),
    );
    super::keep_native_journal(served);
    // The loop takes the hooks and owns them until `run` returns. The window
    // is handed over once it is installed.
    super::dev_agents::start(&config);
    let agents = super::dev_agents::EndOnDrop;

    /// The actual Android bootstrap: window, GPU, UI runtime, and callback
    /// wiring. Runs once, synchronously, inside `on_ready` — which this
    /// backend delivers at the first `MainEvent::InitWindow`, not at the
    /// first `Resume` (module doc, `owner/android/looper.rs`'s
    /// "# Surface Lifecycle": `Resume` arrives before any window exists, so
    /// only `InitWindow` can carry a presentation). Migrated here from
    /// before `run()` (ADR-0039 §1): `on_ready` is `FnOnce` and fires
    /// exactly once, matching the once-only pre-run bootstrap semantics it
    /// replaced. **The surface-recreation path is no longer untouched**: this
    /// bootstrap routes the window's `surface_changed`, which releases its
    /// slot's surface on `Pause`/`TerminateWindow` and rebuilds it when a
    /// window returns — see that route below. **Unvalidated
    /// on-device**: no device and no CI compile target for
    /// `target_os = "android"` verify this; stated here and in the registry
    /// rather than assumed.
    ///
    /// Returns `Err` on bootstrap failure — `on_ready` itself is fallible
    /// now, so the Android backend's `run` loop stops (and propagates the
    /// error out) instead of continuing to pump input/frame
    /// dispatch for an app that never finished bootstrapping.
    fn bootstrap_android<V>(
        root: V,
        config: AppConfig,
        hot_reload: ScenePlugin,
    ) -> anyhow::Result<()>
    where
        V: View + Clone + 'static,
    {
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_native::OwnerPlatform) -> R) -> R {
            with_owner_platform(f)
                .expect("BUG: bootstrap_android runs only after install_owner_platform")
        }

        // 0. The platform clipboard (ADR-0312 §6) was installed with the
        // owner platform; the ui_runtime below takes it through `build_ui_runtime`.
        //
        // 0b. The bound on backgrounded pumps: a deadline the wake hook
        // reports, not a sleep on the loop thread.
        let background_pace = Arc::new(BackgroundPace::default());

        // The existing owner wake carries runtime deadlines as well as the
        // raster host's own (a lost device's next rebuild, a surface's next
        // attempt) and the background pace. Geometry refresh is owner work
        // even while a hidden presentation cannot produce frames.
        {
            let background_pace = Arc::clone(&background_pace);
            super::host::route_wake_deadline(move || {
                super::host::merge_wake_deadlines(
                    super::host::merge_wake_deadlines(
                        APP_RUNTIME.with(|slot| slot.borrow().next_wake()),
                        raster_wake_at(),
                    ),
                    background_pace.next_wake(),
                )
            });
        }

        // 1. Open window (wraps the existing ANativeWindow). `Ready` is
        // guaranteed inside `on_ready` (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let routes = super::window_routes::WindowRoutes::new();
        let host =
            match owner_platform_installed(|owner| owner.open_window(options, routes.clone()))
                .and_then(flui_native::WindowOpen::try_ready)
            {
                Ok(window) => window,
                Err(error) => {
                    tracing::error!(%error, "Failed to create Android window");
                    return Err(
                        anyhow::Error::from(error).context("Failed to create Android window")
                    );
                }
            };
        routes.adopt(host.id());
        let presentation_window = super::presentation_window(std::rc::Rc::clone(&host))
            .with_pointer_resampling(config.pointer_resampling);
        let window = std::rc::Rc::clone(presentation_window.window());

        // 2. The window's measured geometry; its GPU slot is attached once
        // the UI runtime is installed (step 4).
        let geometry = super::window_install::SurfaceGeometry::of(window.as_ref());
        let phys_size = geometry.physical;

        // 3. Mount root widget (used when no plugin is active) at the
        // LOGICAL size; the paint root's DPR transform maps to physical.
        // `UiRuntime::try_new` applies the DPR to the freshly built pipeline
        // before returning.
        let scale_factor = geometry.scale;
        let ui_runtime = match super::host::build_ui_runtime(presentation_window, scale_factor) {
            Ok(ui_runtime) => ui_runtime,
            Err(error) => {
                tracing::error!(%error, "UiRuntime construction failed");
                return Err(anyhow::anyhow!(error).context("UiRuntime construction failed"));
            }
        };

        // Debug overlay: `Some` stats IS the enable flag, so this is the
        // single point that turns the frame path's overlay work on.
        ui_runtime.set_performance_overlay(config.show_performance_overlay);
        ui_runtime.set_motion_override(config.motion_override);

        // Typed frame-failure route (issue #561) — same wiring as the
        // desktop bootstrap.
        config.install_frame_failure_handler(&ui_runtime);
        ui_runtime.set_frame_failure_detail(config.frame_failure_detail);
        config.install_input_tap(&ui_runtime);
        config.install_error_view_builder(&ui_runtime);

        let logical = geometry.logical;
        let owner = APP_RUNTIME
            .with(|slot| slot.borrow().owner_platform.clone())
            .expect("BUG: Android owner installed before bootstrap");
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
                logical.width as f64,
                logical.height as f64,
            )
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
        }
        // Vended while the ui_runtime is still ours, handed over once the
        // install commits: a failed install drops it with the ui_runtime.
        let agent_window = super::dev_agents::vend(&ui_runtime, ui_runtime.presentation_id());
        let textures = ui_runtime.texture_registry(ui_runtime.presentation_id());
        let mut installation = prepare_platform_ui_runtime(ui_runtime, std::rc::Rc::clone(&window));
        installation.text_sizing(native_sizing);
        installation.dev_agent(agent_window);
        let owner_dispatch = installation.dispatcher();

        // 3b. Start config-declared application services (issue #558) —
        // same wiring and same failure contract as the desktop bootstrap:
        // the ui_runtime install above resolved the loop's execution services,
        // and a declared service failing to start fails the bootstrap
        // rather than being silently ignored. Exit-policy consultation is
        // NOT wired on this backend (its platform installs no exit-policy
        // hook — see `install_exit_policy_hook`'s doc), so
        // `ServiceLifetime` currently has no observable effect on Android
        // process lifetime; the services themselves still run, spawn, and
        // get the staged cancel-then-join teardown.
        for service in &config.services {
            if let Err(error) = super::host::start_service(service) {
                tracing::error!(service = service.name(), %error, "service start failed");
                return Err(anyhow::Error::from(error).context(format!(
                    "failed to start application service `{}`",
                    service.name()
                )));
            }
        }

        // 4. The window's slot on the loop's raster executor (ADR-0305), as
        // on the desktop: the window places the executor on its surface,
        // which picks the adapter (Vulkan on Android). The slot holds the
        // window's surface target, never the window (ADR-0254 §5), and every
        // frame crosses to it as an owned, stamped `SceneSnapshot`.
        let host = APP_RUNTIME.with(|slot| slot.borrow().raster_host.clone());
        let display = super::host::display_color_of(window.as_ref());
        let attached = host
            .try_with(|host| {
                host.attach(
                    window.as_ref(),
                    owner_dispatch.address,
                    &config,
                    textures,
                    display,
                )
            })
            .unwrap_or_else(|| Err(super::raster_host::host_busy()));
        if let Err(error) = attached {
            tracing::error!(%error, "GPU init failed");
            return Err(anyhow::Error::from(error).context("GPU init failed"));
        }
        let raster = RasterSlot {
            host,
            address: owner_dispatch.address,
            size: (phys_size.width as u32, phys_size.height as u32),
            device: flui_engine::DeviceHealth::Healthy,
        };
        let surface_host = raster.host.clone();

        // 5. Register input callback -> entered ui_runtime input dispatch
        install_input_wiring(owner_dispatch, window.as_ref());

        let frame = installation.frame_driver(super::frame_driver::FrameDriver::Android(
            AndroidFrameDriver {
                raster,
                hot_reload,
                background_pace,
            },
        ))?;
        let binding = frame.binding;
        routes.route_frame_requested(move |request| {
            // The frame is the turn a window command's wake asks for: it starts
            // with the queued commands, before the callback guard would leave
            // them waiting.
            super::owner_dispatch::apply_queued_window_commands(std::thread::current().id());
            super::owner_dispatch::with_owner_callback(|_| {
                let _ = dispatch_platform_ui_runtime(
                    owner_dispatch,
                    RuntimeTask::Frame(binding, request),
                );
            });
        });

        // 8. Lifecycle callbacks
        //
        // Detached is ui_runtime-dispatched so interrupted gesture state is drained
        // before lifecycle observers run.

        // Native close and quit both revoke this binding before observers run.
        installation.terminal_callbacks();

        // 8b. Surface availability (issue #1146): the release that has to
        // happen before the native window behind the surface dies, and the
        // rebuild for the window that replaces it. `flui-native` reports
        // both out of its own event loop: `Release` on
        // `Pause`/`TerminateWindow`, `Rebuild` on `Resume`/`InitWindow`, with
        // the mapping and its reason in `flui-native`'s
        // `owner/android/looper.rs`, "# Surface Lifecycle".
        //
        // The release is applied before this callback returns, since the
        // callback is the last moment the handle behind the surface is valid:
        // the host sends `RasterCommand::Release` and turns the executor at
        // once (`RasterHost::set_surface`). Dropping a configured
        // `wgpu::Surface` reaches `vkDeviceWaitIdle`, so the driver's
        // device-idle wait is paid here, on this thread. **Unverified:** its
        // length is the driver's and there is no Android device here to
        // measure it; the contingent risk is Android's input-dispatch
        // watchdog, which turns a wait past its budget into an ANR.
        //
        // The host is owner-local, not behind a lock: nothing off this thread
        // reaches it. Only a callback re-entering from the host's own GPU work
        // finds it turning, and then the release lands when that turn ends.
        //
        // The window lets go of its handler exactly once, on the Android
        // platform's `run` exit path (`flui-native`'s `owner/android/looper.rs`,
        // after the loop and before the quit hook); one window, one handler:
        // `on_ready` is `FnOnce`, and a recreated activity is a new
        // `android_main` with a new platform and window (`android-activity`
        // 0.6.1, `native_activity/glue.rs`'s `ANativeActivity_onCreate`).
        let surface_address = owner_dispatch.address;
        routes.route_surface_changed(move |change| {
            let available = change == flui_platform::SurfaceChange::Rebuild;
            // A surface on its way repaints the window in full. That half is
            // deferrable, so it goes through the ui_runtime dispatch, which
            // may queue it while the dispatcher is mid-phase.
            if surface_host.set_surface(surface_address, available) {
                let _ = dispatch_platform_ui_runtime(
                    owner_dispatch,
                    RuntimeTask::Event(RuntimeEvent::PrimarySurfaceRestored),
                );
            }
        });

        // 9. Store the window in AppRuntime's redraw-poke slot — BEFORE
        // marking the lifecycle Resumed or requesting the initial redraw.
        // Both of those can synchronously run the first frame through
        // `dispatch_platform_ui_runtime`; if the slot were still empty at that
        // point, anything resolving it during that frame would silently
        // no-op instead of waking the loop.
        //
        // The Activity's three lifecycle facts reach the runtime as they are,
        // and `derive_lifecycle_state` builds the application state from them:
        // on screen (`onStart`/`onStop`), allowed to run (`onResume`/`onPause`)
        // and holding input focus. Focus alone must not stand for the other two
        // (a dialog or the notification shade takes focus from a running
        // Activity). They arrive with the metrics, the insets, the night mode
        // and the display through the window's one snapshot subscription,
        // seeded from the snapshot read right after it, so a fact that changed
        // during bootstrap is not lost.
        installation.observe_window();
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        // Seed the host's own lifecycle as Resumed. It does not decide what a
        // presentation is: the window's visible, running and focused facts,
        // sampled above, do. Routed through dispatch -- see `run_desktop`'s
        // matching comment for why.
        debug_assert_eq!(
            std::thread::current().id(),
            owner_dispatch.owner_thread,
            "android bootstrap must run on the ui_runtime's owner thread"
        );
        installation.lifecycle(AppLifecycleState::Resumed);
        installation.submit().outcome().expect(
            "BUG: the fresh Android host completes its initial installation synchronously",
        )?;

        tracing::info!("Android platform initialized with callbacks");
        Ok(())
    }

    // Owner-host clear guard armed BEFORE `run(...)`, not inside `on_ready`
    // (ADR-0039 §6) — see `run_desktop`'s matching comment.
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();
    // A bootstrap that found no GPU backend leaves its refusal here, typed,
    // for the launch's result (ADR-0244 §3).
    let refused: std::rc::Rc<std::cell::Cell<Option<crate::app::UnsupportedSystem>>> =
        std::rc::Rc::default();
    let refusal = std::rc::Rc::clone(&refused);
    let ended = Arc::clone(&disconnect);
    let result = platform.run(crate::app::runner::app_handler(move |owner| {
        // A driver that goes under `EXIT_ON_DISCONNECT` quits the loop.
        let quit = owner.proxy();
        ended.arm(move || {
            let _ = quit.request_quit();
        });
        install_owner_platform(owner)?;
        with_owner_platform(|owner| owner.on_wake(Box::new(super::host::drive_native_text_sizing)))
            .expect("BUG: Android owner was just installed")?;
        super::host::install_configured_executors(&config);
        super::host::install_host_storage(&config);
        // `?` converts `bootstrap_android`'s `anyhow::Error` into the
        // callback's opaque `BootstrapError` (anyhow's own `From` impl),
        // exactly as `run_desktop`'s closure does.
        bootstrap_android(root, config, hot_reload).inspect_err(|error| {
            refusal.set(
                error
                    .downcast_ref::<flui_engine::EngineError>()
                    .and_then(|error| crate::app::UnsupportedSystem::from_renderer(error)),
            );
        })?;
        Ok(())
    }));
    // The exit is bounded end to end, as on the desktop (ADR-0258 §7).
    let watchdog = ExitWatchdog::start();
    // The loop's hooks detach first, as on the desktop: an agent stops before
    // the windows it serves are retired.
    drop(agents);
    teardown_platform_ui_runtime_logged(&mut |step| watchdog.reached(step));
    drop(watchdog);
    // The exit's outcome: a terminal failure resumes once everything above
    // ran (ADR-0258 §3).
    if let Some(payload) = super::terminal::take_terminal() {
        std::panic::resume_unwind(payload);
    }

    // `on_ready`'s `Err` propagates straight out of `Platform::run`; surface
    // it the same way `run_desktop` does now that the event loop has
    // exited.
    if let Some(unsupported) = refused.take() {
        tracing::error!(%unsupported, "FLUI cannot run on this system");
        return Err(crate::app::LaunchError::UnsupportedSystem(unsupported));
    }
    if disconnect.happened() {
        // The teardown above wrote what the application persisted. Android
        // reports no exit code, so the driver that launched the app sees the
        // process go; ending it keeps a cached process from carrying this
        // run's state into the next launch.
        std::process::exit(flui_protocol::launch::EXIT_DISCONNECTED);
    }
    if let Err(err) = result {
        panic!("android bootstrap failed: {err:?}");
    }
    Ok(())
}

pub(super) struct AndroidFrameDriver {
    raster: RasterSlot,
    hot_reload: crate::app::hot_reload::ScenePlugin,
    background_pace: std::sync::Arc<BackgroundPace>,
}

impl AndroidFrameDriver {
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
        let raster = &self.raster;
        let hot_reload = &self.hot_reload;
        // The gate half of the wake runs inside one ui_runtime entry
        // and decides; the pump below enters the ui_runtime itself.
        let (action, now, display_color) = ui_runtime.enter(|ui_runtime| {
            let now = web_time::Instant::now();
            // Owner-inbox drain: commands and worker results commit HERE,
            // at the frame boundary while the scheduler phase is Idle,
            // never inside the frame transaction below, and before the
            // scene plugin's fast path, so a command-driven redraw request is
            // observed by the very frame its wake produced whichever path
            // this frame takes.
            let inbox_redraw = ui_runtime.drain_owner_inbox();

            // A live scene plugin owns this presentation's frame: its scene
            // (not a ui_runtime-produced one) is lent to the window's slot
            // for one render. Always `false` without an installed
            // development reload hook.
            let (address, size) = (raster.address, raster.size);
            let plugin_rendered = hot_reload.try_render_frame(
                f64::from(size.0),
                f64::from(size.1),
                |scene, reset_fonts| {
                    raster
                        .host
                        .try_with(|host| host.render_lent(address, size, scene, reset_fonts))
                        .unwrap_or(false)
                },
            );
            if plugin_rendered {
                return (WakeAction::Skip, now, None);
            }

            // The host's own deadlines (a lost device's next rebuild, a
            // surface's next attempt) are a dirty source as well as a wake
            // deadline: one the wake hook actuates but this predicate ignores
            // reaches `WakeAction::Skip` and never runs the turn that serves
            // it (`frame_is_dirty`'s doc).
            let dirty = frame_is_dirty(
                inbox_redraw,
                ui_runtime.needs_redraw(),
                ui_runtime.has_pending_work(),
                raster_wake_at(),
                // Android paces only its backgrounded pump
                // (`BackgroundPace`): a rendering wake has no fallback
                // deferral to consult (ADR-0058).
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

        if action != WakeAction::PumpBackground {
            self.background_pace.clear();
        }
        // A presentation whose frames are off cannot be seen: its slot is not
        // drawn, and draws in full when frames come back (ADR-0305).
        if action != WakeAction::Skip {
            self.raster.show(action == WakeAction::Render);
        }
        match action {
            WakeAction::Skip => return,
            WakeAction::PumpBackground => {
                // A wake before the pace deadline does not pump; the
                // deadline, reported through the wake hook, brings the next.
                if !self.background_pace.admit(now) {
                    return;
                }
                // Frames disabled: service owner commands, async tasks, and
                // input deadlines without animation or a visual frame. See
                // `UiRuntime::pump_background` for the latch-first order.
                ui_runtime.pump_background();
                // A self-re-arming task has no vsync or present to bound it
                // here, so work that asks for another wake defers the next
                // pump by a pace (ADR-0058). A deadline, not a sleep: the
                // loop thread keeps delivering input and lifecycle events.
                if keeps_frame_gate_open(
                    ui_runtime.needs_redraw(),
                    ui_runtime.scheduler().is_frame_scheduled(),
                    ui_runtime.has_pending_work(),
                ) {
                    self.background_pace.defer(web_time::Instant::now());
                }
                // A backgrounded presentation reports what its earlier frames
                // left.
                self.raster.report_frame_panic(ui_runtime);
                return;
            }
            WakeAction::Render => {}
        }

        // The frame, through the window's slot (`RasterSlot::pump`): the
        // executor rebuilds a lost device on its own deadline, which the wake
        // hook carries; no sleep runs on this thread, which also delivers
        // input and `MainEvent` lifecycle.
        let _presented = self.raster.pump(ui_runtime, now, display_color);
    }
}
