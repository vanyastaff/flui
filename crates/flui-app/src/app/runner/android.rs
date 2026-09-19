use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};

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
    use std::{path::PathBuf, sync::Arc};

    use flui_engine::Renderer;
    use flui_platform::{
        AndroidPlatform, Platform, WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    use crate::app::hot_reload::ScenePlugin;

    tracing::info!("Starting Android platform via flui-platform");

    // Hot-reload: build plugin path from app's internal data directory
    let plugin_path: PathBuf = app
        .internal_data_path()
        .map(|p| p.join("libflui_scene.so"))
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/libflui_scene.so"));

    // Inert unless this build carries the `hot-reload` feature.
    let hot_reload = ScenePlugin::new(&plugin_path);

    let platform: Box<dyn Platform> = Box::new(AndroidPlatform::new(app));

    /// The actual Android bootstrap: window, GPU, realm, and callback
    /// wiring. Runs once, synchronously, inside `on_ready` — which this
    /// backend delivers at the first `MainEvent::InitWindow`, not at the
    /// first `Resume` (module doc, `platforms/android/mod.rs`'s
    /// "# Surface Lifecycle": `Resume` arrives before any window exists, so
    /// only `InitWindow` can carry a presentation). Migrated here from
    /// before `run()` (ADR-0039 slice 2): `on_ready` is `FnOnce` and fires
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

        // 0. Wire the platform clipboard (ADR-0034).
        let clipboard = owner_platform_installed(|owner| owner.shared().clipboard());
        APP_RUNTIME.with(|slot| slot.borrow().set_platform_clipboard(clipboard));

        // 0b. This window's device-recovery backoff, constructed here (not
        // down at step 6 alongside the renderer it paces) so the
        // wake-deadline hook below can be wired to it from the start — see
        // `bootstrap_desktop`'s matching comment.
        let device_recovery_backoff = Arc::new(DeviceRecoveryBackoff::new());

        // 0c. Wire the wall-clock-wake hook. Unlike `install_wake_deadline_
        // hook` (desktop's `bootstrap_desktop`), this does NOT also fold in
        // `AppRuntime::next_wake()` (realm-level deadlines: gesture-arena
        // timers, animation continuations) — this backend's `Platform::
        // set_wake_deadline_hook` override is new in this same change
        // (`flui-platform`'s `platforms/android/mod.rs`), added
        // specifically to carry the device-recovery deadline; folding in
        // realm-level deadlines too would change this backend's existing,
        // untested-here wake behavior for gesture/animation timers, which
        // is out of this fix's scope.
        owner_platform_installed(|owner| {
            let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
            owner.shared().set_wake_deadline_hook(Box::new(move || {
                device_recovery_backoff.next_attempt_at()
            }));
        });

        // 1. Open window (wraps the existing ANativeWindow). `Ready` is
        // guaranteed inside `on_ready` (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let window = match owner_platform_installed(|owner| owner.open_window(options))
            .and_then(flui_platform::WindowOpen::try_ready)
        {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Failed to create Android window");
                return Err(anyhow::Error::from(error).context("Failed to create Android window"));
            }
        };

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
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

        // 3. Mount root widget (used when no plugin is active) at the
        // LOGICAL size; the paint root's DPR transform maps to physical.
        // `UiRealm::new` applies the DPR to the freshly built pipeline
        // before returning.
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

        // Debug overlay: `Some` stats IS the enable flag, so this is the
        // single point that turns the frame path's overlay work on.
        ui_realm.set_performance_overlay(config.show_performance_overlay);

        // Typed frame-failure route (issue #561) — same wiring as the
        // desktop bootstrap.
        ui_realm.set_frame_failure_handler(config.frame_failure_handler.clone());
        ui_realm.set_frame_failure_detail(config.frame_failure_detail);

        let logical = window.logical_size();
        let attach = ui_realm.enter(|realm| {
            realm.attach_root_widget_with_size(
                &root,
                logical.width.0 as f32,
                logical.height.0 as f32,
            )
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
        }
        let realm_dispatch = install_platform_realm(ui_realm, &window);

        // 3b. Start config-declared application services (issue #558) —
        // same wiring and same failure contract as the desktop bootstrap:
        // the realm install above resolved the loop's execution services,
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
            realm_dispatch.address,
            phys_size.width.0 as u32,
            phys_size.height.0 as u32,
        )));

        // Install the registration-lifetime surface applier alongside the
        // realm (cleared together at teardown) — see the desktop bootstrap's
        // matching comment for the take/call/restore protocol this feeds.
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

        // 5. Register input callback -> entered realm input dispatch
        window.on_input(Box::new(move |input: PlatformInput| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Input(input)),
            );
            DispatchEventResult::resolved(false, true)
        }));

        // 6. Register frame callback -- with hot-reload plugin override
        let lane_frame = Arc::clone(&lane);
        let hot_reload_frame = hot_reload.clone();
        // Reuses the SAME backoff constructed at step 0b (already wired
        // into the wake-deadline hook above) — not a fresh one.
        window.on_request_frame(Box::new(move || {
            let lane_frame = Arc::clone(&lane_frame);
            let hot_reload_frame = hot_reload_frame.clone();
            let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Frame(Box::new(move |realm| {
                    // Owner-inbox drain: commands and worker results commit HERE,
                    // at the frame boundary while the scheduler phase is Idle —
                    // never inside the frame transaction below. Runs before
                    // everything else in this callback, including the hot-reload
                    // plugin scene fast path below, so a command-driven redraw
                    // request is observed by the very frame its wake produced
                    // regardless of which rendering path this frame takes.
                    let inbox_redraw = drain_owner_inbox(realm);

                    // If a scene plugin is live it owns this presentation frame,
                    // but the callback still executes inside the realm entry
                    // scope. Always `false` in a build without the `hot-reload`
                    // feature. The plugin renders through the backend directly
                    // (its own diagnostic scene, not a realm-produced frame),
                    // so it goes through the lane's scoped backend access —
                    // per ADR-0045 decision 6 the plugin path is one of the
                    // named inline-only lanes.
                    {
                        let Some(mut lane) = lane_frame.try_lock() else {
                            tracing::error!(
                                "frame skipped: raster lane already held by an outer \
                                 frame dispatch"
                            );
                            return;
                        };
                        let plugin_rendered = lane.with_backend(|r| {
                            let (w, h) = r.size();
                            hot_reload_frame.try_render_frame(r, w as f32, h as f32)
                        });
                        if plugin_rendered {
                            return;
                        }
                    }

                    let has_pending = realm.has_pending_work();
                    // See the desktop closure's matching comment: a wake-
                    // deadline source (here, `set_wake_deadline_hook` forces
                    // a dispatch once due — `flui-platform`'s
                    // `platforms/android/mod.rs`'s own `run` loop) that is
                    // absent from `dirty` reaches `WakeAction::Skip` and
                    // returns before this closure ever calls `render_frame_
                    // with_device_recovery`, no matter how faithfully the
                    // platform actuates the wake. Calls the shared
                    // `frame_is_dirty` — see that function's own doc for why
                    // this must not be reimplemented locally.
                    let dirty = frame_is_dirty(
                        inbox_redraw,
                        realm.needs_redraw(),
                        has_pending,
                        device_recovery_backoff.next_attempt_at(),
                        // Android has no wake-deadline hook, so no fallback deferral
                        // exists to consult (ADR-0058): its backgrounded pump sleeps.
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
                            // Frames disabled: pump only the async driver — no
                            // begin/draw frame, no tickers, no pipeline, no
                            // present. See `wake_action`'s doc for why this is
                            // the only thing keeping a spawned future
                            // progressing while backgrounded.
                            //
                            // `finish_async_pump` MUST run first, not after —
                            // see `UpdateScheduler::finish_async_pump`'s doc for the
                            // starvation hazard this ordering avoids.
                            scheduler.finish_async_pump();
                            scheduler.drive_async_tasks();
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

                    let now = web_time::Instant::now();
                    // UpdateScheduler callbacks and rendering share ONE `UiRealm::enter`
                    // dynamic extent; callbacks may legally resolve realm-local
                    // capabilities throughout the complete frame transaction.
                    //
                    // No sleep here, unlike an earlier version of this
                    // closure: `DeviceRecoveryBackoff` paces the recovery
                    // ATTEMPT itself via a non-blocking deadline check (see
                    // its own doc), never by blocking this thread.
                    // `AndroidPlatform::run`'s poll loop calls
                    // `process_input_events`/`dispatch_request_frame`
                    // inline on this SAME thread, so a sleep here — even
                    // one bounded to the backoff's own growing interval —
                    // would stall input and `MainEvent` lifecycle delivery
                    // (Pause/Destroy/Resize) for its duration, which is ANR
                    // territory at the backoff's one-second cap. Unlike
                    // desktop, Android has no non-blocking wait-until
                    // primitive to carry the backoff's deadline instead
                    // (`Platform::set_wake_deadline_hook`'s own doc names
                    // Android explicitly as not overriding it) — so while a
                    // FAILED recovery attempt still wakes this loop once
                    // (`render_frame_with_device_recovery`'s own
                    // `wake_frame()` call, on failure only), a merely
                    // DEFERRED attempt (backoff not yet elapsed) wakes
                    // nothing here, and this platform's retry cadence
                    // degrades to "the next externally-caused wake"
                    // (input, resize, a lifecycle event) rather than a
                    // strict wall-clock cadence — strictly better than the
                    // original bug (no retry, ever, ANDROID included) and
                    // never worse than every other quiescent-wake path
                    // this codebase already has on this backend (gesture-
                    // arena deadlines, animation ticks: none of them have a
                    // platform timer here either).
                    scheduler.drive_frame_with_lane(
                        now,
                        flui_scheduler::IdleDeadline::far_future(now),
                        || {
                            // Device-loss recovery around the frame, same
                            // shape as the desktop path — see
                            // `render_frame_with_device_recovery`.
                            let Some(mut lane) = lane_frame.try_lock() else {
                                tracing::error!(
                                    "frame skipped: raster lane already held by an \
                                     outer frame dispatch"
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

        // 7. Register resize callback -> typed Resized event; the applier
        // installed above (not this closure) actually touches the renderer.
        window.on_resize(Box::new(move |size, scale_factor| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Resized { size, scale_factor }),
            );
        }));

        // 8. Lifecycle callbacks
        //
        // Detached is realm-dispatched so interrupted gesture state is drained
        // before lifecycle observers run.

        // Platform quit -> Detached (frames disabled, listeners notified).
        owner_platform_installed(|owner| {
            owner.shared().on_quit(Box::new(move || {
                tracing::info!("Platform quit");
                debug_assert_eq!(
                    std::thread::current().id(),
                    realm_dispatch.owner_thread,
                    "platform on_quit must fire on the realm's owner thread"
                );
                if let Err(error) = dispatch_platform_realm(
                    realm_dispatch,
                    RealmTask::Event(PlatformToUi::Shutdown),
                ) {
                    // Trace-only: the scheduler died WITH the realm now (each
                    // realm owns its own), so there is no process-global
                    // scheduler left to notify as a fallback.
                    tracing::warn!(
                        ?error,
                        "realm unavailable during Detached lifecycle dispatch"
                    );
                }
            }));
        });

        // Window close (fired by Android Destroy event)
        window.on_close(Box::new(move || {
            tracing::info!("Window closed");
        }));

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
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Frame(Box::new(move |realm| {
                    realm.update_host_lifecycle(target);
                })),
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
        // cover the same-thread one: `dispatch_platform_realm` drains the realm
        // queue inline, so it can run a `RealmTask::Frame` right here, on this
        // thread, where the frame path takes this same lane with `try_lock` and
        // self-skips. Holding the guard across that costs a dropped frame
        // rather than a deadlock, but it is a frame dropped for no reason,
        // because nothing after the mint needs the lane. So the guard's scope
        // ends at the mint and the dispatch runs outside it; the realm half is
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
            let mut lane = lane_surface.lock();
            let outcome = lane.with_backend(|renderer| ensure_surface(renderer, has_surface));
            if matches!(&outcome, SurfaceLifecycleOutcome::Recreated) {
                // A fresh surface's contents are undefined while the
                // damage tracker is incremental, so the first frame after
                // the rebuild owes a full repaint. The engine marks its own
                // tracker inside `recreate_surface`; this is the realm
                // half. The mint happens HERE, inside the same lane lock
                // scope, so no stale in-flight work stays addressed to the
                // destroyed surface — the same act device-loss recovery
                // performs, through the same mailbox counter.
                lane.note_surface_recreated();
            }
            // The guard ends before the realm dispatch — see the same-thread
            // hazard named above, which is what puts the `drop` here.
            drop(lane);
            match outcome {
                // Nothing to do: the engine already logs the release
                // (`surface_released_by_owner`) and a released renderer skips
                // its frames while its authoritative size keeps advancing.
                SurfaceLifecycleOutcome::Released => {}
                SurfaceLifecycleOutcome::Recreated => {
                    // The realm half is deferrable, so it goes through the
                    // realm dispatch rather than running inline: unlike the
                    // release, its ordering cannot affect completeness (the
                    // engine's tracker mark already ran above, and the mint
                    // with it), and the dispatcher may queue it when it is
                    // mid-phase.
                    let _ = dispatch_platform_realm(
                        realm_dispatch,
                        RealmTask::Frame(Box::new(|realm| {
                            realm.mark_primary_needs_full_repaint();
                        })),
                    );
                }
                SurfaceLifecycleOutcome::Failed(source) => {
                    // This one classification is decided by the error, not by
                    // which event produced the request — the callback only
                    // sees the bool. `SurfaceTargetUnavailable` is the
                    // expected outcome of the acquire half: `Resume` reaches
                    // this callback before any window exists (the backend
                    // writes `Resume` independently of the window), so there is
                    // nothing to build from and the probe classifies the target
                    // as suspended. Logged at `trace`, not `debug`, because it
                    // happens on every cycle — a line at `debug` there is
                    // guaranteed noise on a path working as designed.
                    if matches!(
                        source,
                        flui_engine::EngineError::SurfaceTargetUnavailable { .. }
                    ) {
                        tracing::trace!(
                            ?source,
                            "Android: no window to rebuild the surface from yet; the next \
                             InitWindow brings one"
                        );
                    } else {
                        // Unexpected: the window was reported available and the
                        // rebuild failed for another reason. The engine logs
                        // its own act; this is the app-side record of the
                        // cause.
                        //
                        // No retry, and the residual that leaves is named here
                        // rather than left to read as self-healing: the
                        // presentation stays released, every frame after this
                        // is a skipped one, and nothing re-asks until another
                        // `true` arrives — and the only `true` emitters on this
                        // backend are `Resume` and `InitWindow`. So the window
                        // stays blank until the next lifecycle event. A retry
                        // is declined deliberately: the signal that got us here
                        // is delivered with the window already set (`InitWindow`
                        // sets it before the callback runs), so a failure at
                        // this point is genuine rather than a timing race, and
                        // a poll is the wrong answer to a genuine failure —
                        // it would mean re-arming the wake hook and a backoff.
                        // The class that does recover on its own is the
                        // device-loss one, and it is a different path:
                        // `device_recovery` sees the renderer's device-lost
                        // flag, retries under its backoff and wakes the loop.
                        // The seam's statelessness covers a *missed signal*,
                        // not a reported failure.
                        tracing::warn!(
                            source = ?source,
                            "Android: the wgpu surface could not be rebuilt after a window was \
                             reported available"
                        );
                    }
                }
            }
        }));

        // 9. Store the window in AppRuntime's redraw-poke slot — BEFORE
        // marking the lifecycle Resumed or requesting the initial redraw.
        // Both of those can synchronously run the first frame through
        // `dispatch_platform_realm`; if the slot were still empty at that
        // point, anything resolving it during that frame would silently
        // no-op instead of waking the loop.
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        // Mark lifecycle as started (Resumed). Routed through dispatch --
        // see `run_desktop`'s matching comment for why.
        debug_assert_eq!(
            std::thread::current().id(),
            realm_dispatch.owner_thread,
            "android bootstrap must run on the realm's owner thread"
        );
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
        );

        // 10. Request initial redraw, now that the window is stored.
        wake();

        tracing::info!("Android platform initialized with callbacks (hot-reload enabled)");
        Ok(())
    }

    // Owner-host clear guard armed BEFORE `run(...)`, not inside `on_ready`
    // (ADR-0039 §6) — see `run_desktop`'s matching comment.
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();
    let result = platform.run(Box::new(move |owner| {
        install_owner_platform(owner);
        // `?` converts `bootstrap_android`'s `anyhow::Error` into the
        // callback's opaque `BootstrapError` (anyhow's own `From` impl),
        // exactly as `run_desktop`'s closure does.
        bootstrap_android(root, config, hot_reload)?;
        Ok(())
    }));
    teardown_platform_realm();

    // `on_ready`'s `Err` propagates straight out of `Platform::run`; surface
    // it the same way `run_desktop` does now that the event loop has
    // exited.
    if let Err(err) = result {
        panic!("android bootstrap failed: {err:?}");
    }
}
