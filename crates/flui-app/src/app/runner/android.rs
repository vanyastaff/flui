use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};

use super::device_recovery::{DeviceRecoveryBackoff, render_frame_with_device_recovery};
use super::frame_pacing::{NO_PRESENT_FALLBACK_PACE, WakeAction, frame_is_dirty, wake_action};
use super::host::{
    APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, runtime_needs_redraw_handle,
    runtime_wake_callback, with_owner_platform,
};
use super::lifecycle_ladder::emit_lifecycle_transition;
use super::realm_dispatch::{
    PlatformToUi, RealmTask, dispatch_platform_realm, drain_owner_inbox, install_platform_realm,
    install_surface_applier, teardown_platform_realm,
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
    use std::{path::PathBuf, sync::Arc};

    use flui_engine::wgpu::Renderer;
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
    /// backend delivers at the first `Resume` (module doc,
    /// `platforms/android/mod.rs:13`: "Resumed -> on_ready() -> create
    /// surface"). Migrated here from before `run()` (ADR-0039 slice 2):
    /// `on_ready` is `FnOnce` and fires exactly once, matching today's
    /// once-only pre-run bootstrap semantics exactly — no behavior change
    /// is intended or made on the subsequent-Resume/surface-recreation
    /// path, which flows through the backend's existing window/surface
    /// code untouched by this migration. **Unvalidated on-device**: no
    /// device and no CI compile target for `target_os = "android"` verify
    /// this; stated here and in the registry rather than assumed.
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

        // 2. Create GPU renderer (Vulkan backend on Android)
        let phys_size = window.physical_size();
        let renderer = pollster::block_on(Renderer::new(window.as_ref()));
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

        // 4. Wrap renderer for callback sharing
        let renderer = Arc::new(Mutex::new(renderer));

        // Install the registration-lifetime surface applier alongside the
        // realm (cleared together at teardown) — see the desktop bootstrap's
        // matching comment for the take/call/restore protocol this feeds.
        {
            let renderer_resize = Arc::clone(&renderer);
            install_surface_applier(
                realm_dispatch.address.realm_id,
                move |size, scale_factor| {
                    let w = (size.width.0 * scale_factor) as u32;
                    let h = (size.height.0 * scale_factor) as u32;
                    renderer_resize.lock().resize(w, h);
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
        let renderer_frame = Arc::clone(&renderer);
        let hot_reload_frame = hot_reload.clone();
        // Reuses the SAME backoff constructed at step 0b (already wired
        // into the wake-deadline hook above) — not a fresh one.
        window.on_request_frame(Box::new(move || {
            let renderer_frame = Arc::clone(&renderer_frame);
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

                    let mut r = renderer_frame.lock();
                    let (w, h) = r.size();

                    // If a scene plugin is live it owns this presentation frame,
                    // but the callback still executes inside the realm entry
                    // scope. Always `false` in a build without the `hot-reload`
                    // feature.
                    if hot_reload_frame.try_render_frame(&mut *r, w as f32, h as f32) {
                        return;
                    }
                    drop(r);

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
                    );
                    let scheduler = realm.scheduler();
                    match wake_action(
                        scheduler.frames_enabled(),
                        dirty,
                        scheduler.is_frame_scheduled(),
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
                            // `NO_PRESENT_FALLBACK_PACE`'s doc.
                            std::thread::sleep(NO_PRESENT_FALLBACK_PACE);
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
                            let mut r = renderer_frame.lock();
                            let _ = render_frame_with_device_recovery(
                                realm,
                                &mut *r,
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
                    RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Detached)),
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
                    let old = realm.scheduler().lifecycle_state();
                    emit_lifecycle_transition(realm, old, target);
                })),
            );
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
        bootstrap_android(root, config, hot_reload)
    }));
    teardown_platform_realm();

    // `on_ready`'s `Err` propagates straight out of `Platform::run`; surface
    // it the same way `run_desktop` does now that the event loop has
    // exited.
    if let Err(err) = result {
        panic!("android bootstrap failed: {err:?}");
    }
}
