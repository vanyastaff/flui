use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};

use super::device_recovery::{new_device_recovery_backoff, pump_with_device_recovery};
use super::frame_pacing::{
    BACKGROUNDED_PUMP_PACE, FallbackGate, WakeAction, frame_is_dirty, wake_action,
};
use super::host::{
    APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, runtime_wake_callback,
    with_owner_platform,
};
use super::owner_dispatch::{
    RuntimeEvent, RuntimeTask, dispatch_platform_ui_runtime, install_input_wiring,
    install_platform_ui_runtime, install_single_window_terminal_wiring, install_surface_applier,
    teardown_platform_ui_runtime,
};
use super::surface_lifecycle::{
    SurfaceLifecycleOutcome, SurfaceRecreationRetry, report_surface_settlement,
    retry_surface_recreation, settle_surface_availability,
};
use crate::app::AppConfig;

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
    let _installation = crate::app::logging::init_managed_logging(&config);

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
    use std::sync::Arc;

    use flui_engine::Renderer;
    use flui_platform::{AndroidPlatform, Platform, WindowOptions};
    use parking_lot::Mutex;

    use crate::app::hot_reload::ScenePlugin;

    tracing::info!("Starting Android platform via flui-platform");

    // The application's development reload hook may own frames with a scene
    // plugin (`flui run --scene`); inert unless one is installed.
    let hot_reload = ScenePlugin::from_config(&config, app.internal_data_path().as_deref());
    crate::app::dev_agent::log_undriven(&config, "android");

    let platform: Box<dyn Platform> = Box::new(AndroidPlatform::new(app));

    /// The actual Android bootstrap: window, GPU, UI runtime, and callback
    /// wiring. Runs once, synchronously, inside `on_ready` — which this
    /// backend delivers at the first `MainEvent::InitWindow`, not at the
    /// first `Resume` (module doc, `platforms/android/mod.rs`'s
    /// "# Surface Lifecycle": `Resume` arrives before any window exists, so
    /// only `InitWindow` can carry a presentation). Migrated here from
    /// before `run()` (ADR-0039 §1): `on_ready` is `FnOnce` and fires
    /// exactly once, matching the once-only pre-run bootstrap semantics it
    /// replaced. **The surface-recreation path is no longer untouched**: this
    /// bootstrap registers `on_surface_status_change`, which releases the
    /// renderer's surface on `Pause`/`TerminateWindow` and rebuilds it when a
    /// window returns — see that registration below. **Unvalidated
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
        V: View + StatelessView + Clone + 'static,
    {
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
            with_owner_platform(f)
                .expect("BUG: bootstrap_android runs only after install_owner_platform")
        }

        // 0. The platform clipboard (ADR-0038 §9) was installed with the
        // owner platform; the ui_runtime below takes it through `build_ui_runtime`.
        //
        // 0b. This window's device-recovery backoff, constructed here (not
        // down at step 6 alongside the renderer it paces) so the
        // wake-deadline hook below can be wired to it from the start — see
        // `bootstrap_desktop`'s matching comment.
        let device_recovery_backoff = Arc::new(new_device_recovery_backoff());

        // 0b-2. Automatic surface-recreation retry: a genuine rebuild failure
        // leaves the presentation released, and on a window that stays
        // available nothing would re-ask (see `SurfaceRecreationRetry`'s own
        // doc). The retry is deadline-paced exactly like device recovery, and
        // its deadline joins the same wake hook below. One handle for the
        // frame closure, one for the availability callback.
        let surface_recreation_retry = Arc::new(SurfaceRecreationRetry::new());
        let surface_retry_for_callback = Arc::clone(&surface_recreation_retry);

        // 0c. Wire the wall-clock-wake hook to both retries. Unlike
        // `install_wake_deadline_hook` (desktop's `bootstrap_desktop`), this
        // does NOT also fold in `AppRuntime::next_wake()` (ui_runtime-level
        // deadlines: gesture-arena timers, animation continuations) — this
        // backend's `Platform::set_wake_deadline_hook` override
        // (`flui-platform`'s `platforms/android/mod.rs`) was added
        // specifically to carry recovery deadlines; folding in ui_runtime-level
        // deadlines too would change this backend's existing, untested-here
        // wake behavior for gesture/animation timers.
        owner_platform_installed(|owner| {
            let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
            let surface_recreation_retry = Arc::clone(&surface_recreation_retry);
            owner.shared().set_wake_deadline_hook(Box::new(move || {
                super::host::merge_wake_deadlines(
                    device_recovery_backoff.next_attempt_at(),
                    surface_recreation_retry.next_attempt_at(),
                )
            }));
        });

        // 1. Open window (wraps the existing ANativeWindow). `Ready` is
        // guaranteed inside `on_ready` (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let host = match owner_platform_installed(|owner| owner.open_window(options))
            .and_then(flui_platform::WindowOpen::try_ready)
        {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Failed to create Android window");
                return Err(anyhow::Error::from(error).context("Failed to create Android window"));
            }
        };
        let presentation_window = super::presentation_window(host);
        let window = Arc::clone(presentation_window.window());

        // 2. Create GPU renderer (Vulkan backend on Android). `Renderer::new`
        // takes ownership of a `WindowTarget` (issue #1043) — `Arc::clone`
        // gives it its own strong ref rather than a borrow of `window`.
        let phys_size = window.physical_size();
        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)));
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                return Err(anyhow::anyhow!(e).context("GPU init failed"));
            }
        };
        renderer.resize(phys_size.width as u32, phys_size.height as u32);

        // 3. Mount root widget (used when no plugin is active) at the
        // LOGICAL size; the paint root's DPR transform maps to physical.
        // `UiRuntime::new` applies the DPR to the freshly built pipeline
        // before returning.
        let scale_factor = window.scale_factor() as f64;
        let wake = runtime_wake_callback();
        let ui_runtime =
            match super::host::build_ui_runtime(&wake, presentation_window, scale_factor) {
                Ok(ui_runtime) => ui_runtime,
                Err(error) => {
                    tracing::error!(%error, "UiRuntime construction failed");
                    return Err(anyhow::anyhow!(error).context("UiRuntime construction failed"));
                }
            };

        // Debug overlay: `Some` stats IS the enable flag, so this is the
        // single point that turns the frame path's overlay work on.
        ui_runtime.set_performance_overlay(config.show_performance_overlay);

        // Typed frame-failure route (issue #561) — same wiring as the
        // desktop bootstrap.
        ui_runtime.set_frame_failure_handler(config.frame_failure_handler.clone());
        ui_runtime.set_frame_failure_detail(config.frame_failure_detail);

        let logical = window.logical_size();
        let attach = ui_runtime.enter(|ui_runtime| {
            ui_runtime.attach_root_widget_with_size(
                &root,
                logical.width as f64,
                logical.height as f64,
            )
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
        }
        let owner_dispatch = install_platform_ui_runtime(ui_runtime, &window);

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
            if let Err(error) = APP_RUNTIME.with(|slot| slot.borrow_mut().start_service(service)) {
                tracing::error!(service = service.name(), %error, "service start failed");
                return Err(anyhow::Error::from(error).context(format!(
                    "failed to start application service `{}`",
                    service.name()
                )));
            }
        }

        // 4. Adopt the raster mailbox (ADR-0045's inline lane) — the same
        // ownership shape as the desktop bootstrap: the lane's owner solely
        // owns the renderer, the resize path holds only the mailbox handle
        // plus the owner-affine stamp/size state, and every frame submit
        // below crosses the raster boundary as an owned, stamped
        // `SceneSnapshot`.
        let lane = Arc::new(Mutex::new(crate::app::raster_lane::RasterLane::new(
            renderer,
            owner_dispatch.address,
            phys_size.width as u32,
            phys_size.height as u32,
        )));

        // Install the registration-lifetime surface applier alongside the
        // ui_runtime (cleared together at teardown) — see the desktop bootstrap's
        // matching comment for the take/call/restore protocol this feeds.
        {
            let resize_hook = lane.lock().resize_hook();
            install_surface_applier(
                owner_dispatch.address.ui_runtime_id,
                move |size, scale_factor| {
                    let w = (size.width * scale_factor) as u32;
                    let h = (size.height * scale_factor) as u32;
                    resize_hook.apply(w, h);
                },
            );
        }

        // 5. Register input callback -> entered ui_runtime input dispatch
        install_input_wiring(owner_dispatch, window.as_ref());

        let frame = super::frame_driver::install_frame_driver(
            owner_dispatch,
            super::frame_driver::FrameDriver::Android(AndroidFrameDriver {
                lane: Arc::clone(&lane),
                hot_reload,
                device_recovery_backoff,
                surface_recreation_retry,
            }),
        )?;
        let binding = frame.binding;
        window.on_request_frame(Box::new(move || {
            let _owner_callback = super::owner_dispatch::begin_owner_callback();
            let _ = dispatch_platform_ui_runtime(owner_dispatch, RuntimeTask::Frame(binding));
        }));

        // 7. Register resize callback -> typed Resized event; the applier
        // installed above (not this closure) actually touches the renderer.
        window.on_resize(Box::new(move |size, scale_factor| {
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::Resized { size, scale_factor }),
            );
        }));

        // 8. Lifecycle callbacks
        //
        // Detached is ui_runtime-dispatched so interrupted gesture state is drained
        // before lifecycle observers run.

        // Native close and quit both revoke this binding before observers run.
        owner_platform_installed(|owner| {
            install_single_window_terminal_wiring(&window, &owner.shared(), owner_dispatch);
        });

        // Window active status. On Android this one callback conflates real
        // window focus (`MainEvent::GainedFocus`/`LostFocus`) with the app's
        // actual pause/resume signal (`MainEvent::Resume`/`Pause` currently fire
        // the identical `dispatch_active_status_change` — see
        // `flui-platform`'s `platforms/android/mod.rs`); a dedicated
        // `MainEvent` -> lifecycle callback that tells them apart is a named
        // follow-up (ADR-0035), not this PR. Until that split lands, this keeps
        // the existing transport but fixes the mapping: `false` ladders all the
        // way to `Paused` and `true` back to `Resumed` — Android's
        // backgrounding signal needs the deeper ladder the desktop/web
        // `(visible, focused)` derivation (which only ever reaches
        // `Inactive`/`Hidden`) does not produce.
        window.on_active_status_change(Box::new(move |resumed| {
            let target = if resumed {
                AppLifecycleState::Resumed
            } else {
                AppLifecycleState::Paused
            };
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::Lifecycle(target)),
            );
        }));

        // 8b. Surface availability (issue #1146): the release that has to
        // happen before the native window behind the surface dies, and the
        // rebuild for the window that replaces it. `flui-platform` reports
        // both as one bool out of its own event loop — `false` on
        // `Pause`/`TerminateWindow`, `true` on `Resume`/`InitWindow`, with
        // the mapping and its reason in `flui-platform`'s
        // `platforms/android/mod.rs`, "# Surface Lifecycle".
        //
        // The lock below is BLOCKING, unlike the frame closure's `try_lock`
        // above, and the difference is the failure mode: a skipped frame
        // retries on the next wake and heals itself, while a skipped release
        // is exactly the defect this callback exists to fix — the surface has
        // to be gone before `TerminateWindow`'s callback returns, because
        // that callback is the last moment the handle behind it is valid.
        //
        // That hold carries the release's own unbounded cost, so the guard is
        // not free either: dropping a configured `wgpu::Surface` reaches
        // `vkDeviceWaitIdle`, which means the driver's device-idle wait is
        // paid under this guard, on this thread (`SurfaceLifecycle::
        // release_surface`'s doc names it as the price of the verb). Accepted
        // for the same reason the lock is blocking at all: the wait has to
        // finish before the callback returns. **Unverified:** the wait's length
        // is the driver's and there is no Android device here to measure it;
        // the contingent risk is Android's input-dispatch watchdog, since the
        // thread that asked for this transition stays parked until the callback
        // returns, which turns a wait past the watchdog's budget into an ANR
        // rather than a skipped frame.
        //
        // Blocking here is only safe while ONE invariant holds: **nothing
        // off-thread ever holds this lane**. Today that follows from this
        // backend's shape — the lane is taken inside `dispatch_request_frame`,
        // which `AndroidPlatform::run`'s loop calls after `poll_events`
        // returns, on this same thread, and no event arm drives a frame. A
        // second lane consumer on another thread (a worker service, a
        // pipelined presentation) would make this lock a real deadlock, which
        // is why the invariant is named here and not just its local
        // consequences: this is the one path where a wrong assumption about
        // it is not self-healing.
        //
        // The off-thread invariant is not the only hazard here, and it does not
        // cover the same-thread one: `dispatch_platform_ui_runtime` drains the ui_runtime
        // queue inline, so it can reach another owner operation right here, on this
        // thread, where the frame path takes this same lane with `try_lock` and
        // self-skips. Holding the guard across that costs a dropped frame
        // rather than a deadlock, but it is a frame dropped for no reason,
        // because nothing after the mint needs the lane. So the guard's scope
        // ends at the mint and the dispatch runs outside it; the ui_runtime half is
        // documented as deferrable below, so the ordering does not change.
        //
        // Two more invariants this registration rests on, stated where it is
        // made rather than assumed:
        //
        // * The slots are cleared exactly once, on `AndroidPlatform::run`'s
        //   exit path (`flui-platform`'s `platforms/android/mod.rs`, after the
        //   loop and before the quit hook), on the loop's three returning
        //   routes: `MainEvent::Destroy`, a `quit()`, a bootstrap that failed,
        //   plus one named exception, a panic that unwinds out of `run` and
        //   skips the clear by decision (ADR-0063 decision 5). The clear drops
        //   this closure and the frame closure above, which are the only
        //   owners of `lane`, so the renderer and its surface lease go with
        //   them and the window's `Arc` is released.
        // * Nothing registers on the cleared set afterwards. `on_ready` is
        //   `FnOnce`, so this bootstrap runs once per platform, and a recreated
        //   activity is a new `android_main` with a new `AndroidApp`, a new
        //   platform and a new window (`android-activity` 0.6.1,
        //   `native_activity/glue.rs`'s `ANativeActivity_onCreate`); the
        //   once-then-discarded rule `WindowCallbacks::clear` documents is
        //   never reached by this runner.
        let lane_surface = Arc::clone(&lane);
        window.on_surface_status_change(Box::new(move |has_surface| {
            let now = web_time::Instant::now();
            let mut lane = lane_surface.lock();
            // The engine's tracker mark, the generation mint and the retry's
            // classification all happen inside this lane lock scope, through
            // the shared helper — so no stale in-flight work stays addressed
            // to the destroyed surface, and this callback and the frame-path
            // retry cannot disagree about which failure is genuine.
            let outcome = settle_surface_availability(
                &mut lane,
                &surface_retry_for_callback,
                has_surface,
                now,
            );
            // The guard ends before the ui_runtime dispatch — see the same-thread
            // hazard named above, which is what puts the `drop` here.
            drop(lane);
            if matches!(outcome, SurfaceLifecycleOutcome::Recreated) {
                // The ui_runtime half is deferrable, so it goes through the ui_runtime
                // dispatch rather than running inline: unlike the release, its
                // ordering cannot affect completeness (the engine's tracker
                // mark already ran above, and the mint with it), and the
                // dispatcher may queue it when it is mid-phase.
                let _ = dispatch_platform_ui_runtime(
                    owner_dispatch,
                    RuntimeTask::Event(RuntimeEvent::PrimarySurfaceRestored),
                );
            }
            // A release logs nothing here (the engine logs
            // `surface_released_by_owner`). `SurfaceTargetUnavailable` is the
            // expected outcome of the acquire half — `Resume` reaches this
            // callback before any window exists, since the backend writes
            // `Resume` independently of the window — and is traced; a genuine
            // failure warns, and the frame-path retry above now polls it.
            report_surface_settlement("Android", &outcome);
        }));

        // 9. Store the window in AppRuntime's redraw-poke slot — BEFORE
        // marking the lifecycle Resumed or requesting the initial redraw.
        // Both of those can synchronously run the first frame through
        // `dispatch_platform_ui_runtime`; if the slot were still empty at that
        // point, anything resolving it during that frame would silently
        // no-op instead of waking the loop.
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        // Mark lifecycle as started (Resumed). Routed through dispatch --
        // see `run_desktop`'s matching comment for why.
        debug_assert_eq!(
            std::thread::current().id(),
            owner_dispatch.owner_thread,
            "android bootstrap must run on the ui_runtime's owner thread"
        );
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::Lifecycle(AppLifecycleState::Resumed)),
        );

        // 10. Request initial redraw, now that the window is stored.
        wake();

        tracing::info!("Android platform initialized with callbacks");
        Ok(())
    }

    // Owner-host clear guard armed BEFORE `run(...)`, not inside `on_ready`
    // (ADR-0039 §6) — see `run_desktop`'s matching comment.
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();
    let result = platform.run(Box::new(move |owner| {
        install_owner_platform(owner)?;
        APP_RUNTIME.with(|slot| slot.borrow_mut().install_host_storage(&config));
        // `?` converts `bootstrap_android`'s `anyhow::Error` into the
        // callback's opaque `BootstrapError` (anyhow's own `From` impl),
        // exactly as `run_desktop`'s closure does.
        bootstrap_android(root, config, hot_reload)?;
        Ok(())
    }));
    teardown_platform_ui_runtime();

    // `on_ready`'s `Err` propagates straight out of `Platform::run`; surface
    // it the same way `run_desktop` does now that the event loop has
    // exited.
    if let Err(err) = result {
        panic!("android bootstrap failed: {err:?}");
    }
}

pub(super) struct AndroidFrameDriver {
    lane: std::sync::Arc<
        parking_lot::Mutex<crate::app::raster_lane::RasterLane<flui_engine::Renderer>>,
    >,
    hot_reload: crate::app::hot_reload::ScenePlugin,
    device_recovery_backoff: std::sync::Arc<super::device_recovery::DeviceRecoveryBackoff>,
    surface_recreation_retry: std::sync::Arc<SurfaceRecreationRetry>,
}

impl AndroidFrameDriver {
    pub(super) fn wake(&mut self, ui_runtime: &mut crate::app::ui_runtime::UiRuntime) {
        let lane_frame = &self.lane;
        let hot_reload_frame = &self.hot_reload;
        let device_recovery_backoff = &self.device_recovery_backoff;
        let surface_recreation_retry = &self.surface_recreation_retry;
        // The gate half of the wake runs inside one ui_runtime entry
        // and decides; the pump below enters the ui_runtime itself.
        let (action, now) = ui_runtime.enter(|ui_runtime| {
            let now = web_time::Instant::now();
            // Owner-inbox drain: commands and worker results commit HERE,
            // at the frame boundary while the scheduler phase is Idle —
            // never inside the frame transaction below. Runs before
            // everything else in this callback, including the hot-reload
            // plugin scene fast path below, so a command-driven redraw
            // request is observed by the very frame its wake produced
            // regardless of which rendering path this frame takes.
            let inbox_redraw = ui_runtime.drain_owner_inbox();

            // If a scene plugin is live it owns this presentation frame,
            // but the callback still executes inside the ui_runtime entry
            // scope. Always `false` without an installed development
            // reload hook. The plugin renders through the backend directly
            // (its own diagnostic scene, not a ui_runtime-produced frame),
            // so it goes through the lane's scoped backend access —
            // per ADR-0045 decision 6 the plugin path is one of the
            // named inline-only lanes.
            {
                let Some(mut lane) = lane_frame.try_lock() else {
                    tracing::error!(
                        "frame skipped: raster lane already held by an outer \
                                 frame dispatch"
                    );
                    return (WakeAction::Skip, now);
                };
                let plugin_rendered = lane.with_backend(|r| {
                    let (w, h) = r.size();
                    hot_reload_frame.try_render_frame(r, w as f64, h as f64)
                });
                if plugin_rendered {
                    return (WakeAction::Skip, now);
                }
            }

            let has_pending = ui_runtime.has_pending_work();
            // See the desktop closure's matching comment: a wake-
            // deadline source (here, `set_wake_deadline_hook` forces
            // a dispatch once due — `flui-platform`'s
            // `platforms/android/mod.rs`'s own `run` loop) that is
            // absent from `dirty` reaches `WakeAction::Skip` and
            // returns before this closure ever calls
            // `pump_with_device_recovery`, no matter how faithfully the
            // platform actuates the wake. Calls the shared
            // `frame_is_dirty` — see that function's own doc for why
            // this must not be reimplemented locally.
            //
            // The surface-retry deadline joins the device-recovery one
            // here for the same reason that one must be present.
            let retry_deadline = super::host::merge_wake_deadlines(
                device_recovery_backoff.next_attempt_at(),
                surface_recreation_retry.next_attempt_at(),
            );
            let dirty = frame_is_dirty(
                inbox_redraw,
                ui_runtime.needs_redraw(),
                has_pending,
                retry_deadline,
                // Android's wake-deadline hook carries only the retry
                // deadlines, so no fallback deferral exists to consult
                // (ADR-0058): its backgrounded pump sleeps.
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
                        platform = "Android",
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
            WakeAction::PumpAsync => {
                // Frames disabled: pump only the async driver — no
                // frame, no tickers, no pipeline, no present. See
                // `wake_action`'s doc for why this is the only thing
                // keeping a spawned future progressing while
                // backgrounded, and `UiRuntime::pump_background` for
                // the latch-first order it keeps.
                ui_runtime.pump_background();
                // Unconditional throttle: a self-re-arming task has
                // no vsync/present call to bound it here either, and
                // this arm has no gate-open signal to make the pace
                // conditional the way desktop's does — see
                // `BACKGROUNDED_PUMP_PACE`'s doc. Android keeps the
                // sleep the desktop path dropped (ADR-0058): its frame
                // source has no wake-deadline hook to arm instead.
                std::thread::sleep(BACKGROUNDED_PUMP_PACE);
                return;
            }
            WakeAction::Render => {}
        }

        // The frame: `UiRuntime::pump` at `now`, with device-loss
        // recovery around it, same shape as the desktop path — see
        // `pump_with_device_recovery`.
        //
        // No sleep here, unlike an earlier version of this
        // closure: both retries pace their ATTEMPT via a
        // non-blocking deadline check (see `RetryBackoff`'s
        // doc), never by blocking this thread.
        // `AndroidPlatform::run`'s poll loop calls
        // `process_input_events`/`dispatch_request_frame`
        // inline on this SAME thread, so a sleep here — even
        // one bounded to the backoff's own growing interval —
        // would stall input and `MainEvent` lifecycle delivery
        // (Pause/Destroy/Resize) for its duration, which is ANR
        // territory at the backoff's one-second cap. The
        // deadline is carried by the wake hook wired at step 0c
        // instead: this backend has no `ControlFlow::WaitUntil`,
        // so `AndroidPlatform::run`'s own ~16 ms idle poll
        // consults the hook every iteration and forces a
        // dispatch once the deadline is due (`flui-platform`'s
        // `platforms/android/mod.rs`).
        let Some(mut lane) = lane_frame.try_lock() else {
            tracing::error!("frame skipped: raster lane already held by an outer frame dispatch");
            return;
        };
        let _ = pump_with_device_recovery(ui_runtime, &mut *lane, device_recovery_backoff, now);
    }
}
