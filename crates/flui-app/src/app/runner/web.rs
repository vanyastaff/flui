use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};

use super::frame_pacing::{FallbackGate, WakeAction, wake_action};
use super::host::{
    APP_RUNTIME, install_owner_platform, runtime_wake_callback, with_owner_platform,
};
use super::owner_dispatch::{
    RuntimeEvent, RuntimeTask, dispatch_platform_ui_runtime, install_input_wiring,
    install_platform_ui_runtime, install_surface_applier,
};
use crate::app::AppConfig;

// ============================================================================
// Web Implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
pub(super) fn run_web<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::sync::Arc;

    use flui_engine::Renderer;
    use flui_platform::WindowOptions;
    use parking_lot::Mutex;

    use flui_runtime::pump::SampledClock;

    use crate::app::raster_lane::DirectSink;

    tracing::info!("Starting web platform via flui-platform");
    crate::app::dev_agent::log_undriven(&config, "web");

    // Platform init is an environment failure (unsupported browser, missing
    // wasm feature, driver problem), not a `BUG:` invariant — see the
    // matching comment in `run_desktop` above for why this is a `match` +
    // `panic!` with a full error log instead of a bare `.expect()`.
    let platform = match flui_platform::current_platform() {
        Ok(platform) => platform,
        Err(error) => {
            tracing::error!(%error, "Failed to initialize platform");
            panic!("web bootstrap failed: platform initialization error: {error:?}");
        }
    };

    /// The actual web bootstrap: canvas window, renderer, UI runtime, and
    /// callback wiring. Runs once, synchronously, inside `on_ready` —
    /// `WebPlatform::run` invokes it before starting the RAF loop
    /// (ADR-0039 §1; behavior-preserving, since `on_ready`
    /// already runs synchronously on this thread before `run` returns).
    ///
    /// Returns `Err` on bootstrap failure — `on_ready` itself is fallible
    /// now, so `WebPlatform::run` does not install the RAF loop over a
    /// half-built page.
    fn bootstrap_web<V>(root: V, config: AppConfig) -> anyhow::Result<()>
    where
        V: View + StatelessView + Clone + 'static,
    {
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
            with_owner_platform(f)
                .expect("BUG: bootstrap_web runs only after install_owner_platform")
        }

        // 0. The platform clipboard (ADR-0038 §9) was installed with the
        // owner platform; the ui_runtime below takes it through `build_ui_runtime`.
        //
        // 1. Open window (creates canvas). `Ready` is guaranteed inside
        // `on_ready` (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let host = match owner_platform_installed(|owner| owner.open_window(options))
            .and_then(flui_platform::WindowOpen::try_ready)
        {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Failed to create canvas window");
                return Err(anyhow::Error::from(error).context("Failed to create canvas window"));
            }
        };
        let presentation_window = super::presentation_window(host);
        let window = Arc::clone(presentation_window.window());

        // 2. Shared renderer slot — starts as None, filled async once the WebGPU
        //    adapter is available. `Option` lets the frame callback skip frames that
        //    arrive before the renderer is ready.
        let renderer: Arc<Mutex<Option<Renderer>>> = Arc::new(Mutex::new(None));

        let phys_size = window.physical_size();
        let renderer_init = Arc::clone(&renderer);
        let renderer_window = Arc::clone(&window);

        // The future owns a strong window reference. This is required because the
        // browser platform installs RAF and returns immediately, and startup can
        // also return early before the window reaches AppRuntime's redraw-poke slot.
        wasm_bindgen_futures::spawn_local(async move {
            // `Renderer::new` takes ownership of a `WindowTarget` (issue
            // #1043) — `Arc::clone` gives it its own strong ref rather than
            // a borrow of `renderer_window` (which the future already owns,
            // per the comment above).
            let mut r = match Renderer::new(Arc::clone(&renderer_window)).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("GPU init failed: {:?}", e);
                    return;
                }
            };
            r.resize(phys_size.width as u32, phys_size.height as u32);
            tracing::info!("WebGPU renderer initialized");
            let _prev = renderer_init.lock().replace(r);
        });

        // 3. Mount root widget at the LOGICAL size; the paint root's DPR
        // transform maps to the physical canvas. `UiRuntime::new` applies the
        // DPR to the freshly built pipeline before returning.
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

        // Install the registration-lifetime surface applier alongside the
        // ui_runtime (cleared together at teardown) — see the desktop bootstrap's
        // matching comment for the take/call/restore protocol this feeds.
        {
            let renderer_resize = Arc::clone(&renderer);
            install_surface_applier(
                owner_dispatch.address.ui_runtime_id,
                move |size, scale_factor| {
                    if let Some(renderer) = renderer_resize.lock().as_mut() {
                        // Rounded like the backend's `physical_size` and
                        // the canvas backing store it sets, so all three
                        // agree at fractional device pixel ratios.
                        let width = (size.width * scale_factor).round() as u32;
                        let height = (size.height * scale_factor).round() as u32;
                        renderer.resize(width, height);
                    }
                },
            );
        }

        // 4. Register input callback
        install_input_wiring(owner_dispatch, window.as_ref());

        // 5. Register frame callback
        let renderer_frame = Arc::clone(&renderer);
        window.on_request_frame(Box::new(move || {
            let _owner_callback = super::owner_dispatch::begin_owner_callback();
            let renderer_frame = Arc::clone(&renderer_frame);
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Pump(Box::new(move |ui_runtime| {
                    let action = ui_runtime.enter(|ui_runtime| {
                        // Owner-inbox drain: commands and worker results commit
                        // HERE, at the frame boundary while the scheduler phase
                        // is Idle — never inside the frame transaction below.
                        // Runs before the dirty gate so a command-driven redraw
                        // request is observed by the very frame its wake
                        // produced.
                        let inbox_redraw = ui_runtime.drain_owner_inbox();

                        let has_pending = ui_runtime.has_pending_work();
                        let dirty = inbox_redraw || ui_runtime.needs_redraw() || has_pending;
                        let scheduler = ui_runtime.scheduler();
                        wake_action(
                            scheduler.frames_enabled(),
                            dirty,
                            scheduler.is_frame_scheduled(),
                            // No deferral on web: this callback is driven by the
                            // browser's own `requestAnimationFrame` loop, which
                            // already paces at the display's rate — the exact job
                            // ADR-0058's deadline does for the native backends.
                            FallbackGate::default(),
                        )
                    });
                    match action {
                        WakeAction::Skip => return,
                        WakeAction::PumpAsync => {
                            // Frames disabled: pump only the async driver — see
                            // `wake_action`'s doc for why this is the only thing
                            // keeping a spawned future progressing while
                            // backgrounded, and `UiRuntime::pump_background` for
                            // the latch-first order it keeps.
                            //
                            // No `NO_PRESENT_FALLBACK_PACE` sleep here, unlike
                            // desktop/Android: this callback is driven by the
                            // browser's `requestAnimationFrame` loop
                            // (`start_raf_loop`, `flui-platform`'s web backend),
                            // which fires unconditionally once per animation
                            // frame regardless of whether a redraw was
                            // requested — the browser's own vsync-paced RAF
                            // cadence already bounds this arm's re-wake rate, so
                            // an additional sleep would be redundant. It would
                            // also be unsound here: `wasm32-unknown-unknown` has
                            // no real OS threads, and blocking the single JS
                            // thread with `std::thread::sleep` would hang the
                            // page rather than pace it.
                            ui_runtime.pump_background();
                            return;
                        }
                        WakeAction::Render => {}
                    }

                    let now = web_time::Instant::now();
                    {
                        // No frame runs before the renderer exists: the ui_runtime
                        // stays dirty, and the first animation frame after the
                        // renderer arrives renders.
                        let mut slot = renderer_frame.lock();
                        let Some(r) = slot.as_mut() else {
                            return;
                        };

                        let _ = ui_runtime.pump(&mut SampledClock(now), &mut DirectSink::new(r));

                        if r.is_device_lost() {
                            drop(slot);
                            let renderer_recover = Arc::clone(&renderer_frame);
                            // A cloned, `'static` wake handle: the spawned
                            // future outlives this callback's `&UiRuntime`
                            // borrow, so it cannot capture `ui_runtime` itself.
                            let wake = ui_runtime.wake_handle();
                            wasm_bindgen_futures::spawn_local(async move {
                                // Never hold the renderer mutex across `.await`.
                                let Some(mut renderer) = renderer_recover.lock().take() else {
                                    return;
                                };
                                let result = renderer.recover().await;
                                let _prev = renderer_recover.lock().replace(renderer);
                                match result {
                                    Ok(()) => {
                                        tracing::warn!("GPU device lost — recovered successfully");
                                        wake();
                                    }
                                    Err(e @ flui_engine::EngineError::SurfaceTargetUnavailable {
                                        ..
                                    }) => {
                                        // The window owner reports its native
                                        // handle is gone or suspended (issue
                                        // #1043). `HandleError::Unavailable` is
                                        // `Recoverability::Recoverable` — a
                                        // backgrounded tab's canvas can report
                                        // this transiently — but
                                        // `HandleError::NotSupported` is
                                        // `Fatal` (the owner can never answer
                                        // this handle kind). This arm does not
                                        // branch on that classification: the
                                        // retry wake below fires either way,
                                        // same as any other failure; only the
                                        // log severity is lower, since the
                                        // common case is expected to clear on
                                        // its own.
                                        tracing::warn!(
                                            error = ?e,
                                            "GPU device recovery failed — window target unavailable; retry armed for the next wake regardless"
                                        );
                                        wake();
                                    }
                                    Err(e) => {
                                        // Driver may still be resetting. Arm
                                        // the retry wake in the failure arm
                                        // too — RAF alone re-pumps an ACTIVE
                                        // tab, but a backgrounded tab's RAF
                                        // is suspended, and without this wake
                                        // the recovery is never retried once
                                        // the tab comes back to the front.
                                        //
                                        // No `DeviceRecoveryBackoff` here —
                                        // web stays un-unified with the
                                        // desktop/Android `DeviceRecovery`
                                        // seam (its `recover()` is async,
                                        // driven through `spawn_local`, not
                                        // a synchronous call that trait
                                        // could wrap) — and needs no backoff
                                        // of its own either: the renderer
                                        // slot stays `None` for the
                                        // duration of this `.await`, so the
                                        // outer closure's own `let Some(r)
                                        // = slot.as_mut() else { return; }`
                                        // above already refuses to spawn a
                                        // second recovery while one is in
                                        // flight, and once it returns the
                                        // browser's own `requestAnimationFrame`
                                        // cadence bounds how often a new one
                                        // can start (~16ms, the same order
                                        // as desktop/Android's base backoff
                                        // interval) — see the `PumpAsync`
                                        // arm's own comment above for why
                                        // RAF is a sufficient pacer here.
                                        tracing::error!(
                                            error = ?e,
                                            "GPU device recovery failed; retry armed for the next wake"
                                        );
                                        wake();
                                    }
                                }
                            });
                        }
                    }
                })),
            );
        }));

        window.on_resize(Box::new(move |size, scale_factor| {
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::Resized { size, scale_factor }),
            );
        }));

        // 6. Lifecycle callbacks
        //
        // Detached is ui_runtime-dispatched so interrupted gesture state is drained
        // before lifecycle observers run.
        owner_platform_installed(|owner| {
            owner.shared().on_quit(Box::new(move || {
                tracing::info!("Web platform quit");
                debug_assert_eq!(
                    std::thread::current().id(),
                    owner_dispatch.owner_thread,
                    "platform on_quit must fire on the ui_runtime's owner thread"
                );
                if let Err(error) = dispatch_platform_ui_runtime(
                    owner_dispatch,
                    RuntimeTask::Event(RuntimeEvent::Shutdown),
                ) {
                    // Trace-only: the scheduler died WITH the ui_runtime now (each
                    // ui_runtime owns its own), so there is no process-global
                    // scheduler left to notify as a fallback.
                    tracing::warn!(
                        ?error,
                        "ui_runtime unavailable during Detached lifecycle dispatch"
                    );
                }
            }));
        });

        window.on_close(Box::new(move || {
            tracing::info!("Canvas window closed");
            // On web, no explicit quit mechanism needed
        }));

        // No `on_visibility_status_change` registration on web (yet): there is
        // no occlusion signal wired for this backend in this PR (winit's
        // `Occluded` is desktop-only) — a DOM `visibilitychange` listener is a
        // future follow-up, not this PR's scope.
        window.on_active_status_change(Box::new(move |focused| {
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::WindowFocus(focused)),
            );
        }));
        // The web translation already emits hover-status changes for DOM
        // pointerenter/pointerleave; route them like the desktop bootstrap
        // does so a cursor leaving the canvas sweeps hover state.
        window.on_hover_status_change(Box::new(move |is_hovered| {
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::WindowHover(is_hovered)),
            );
        }));

        // 7. Store the window in AppRuntime's redraw-poke slot — BEFORE
        // marking the lifecycle Resumed, which can synchronously run the
        // first frame through `dispatch_platform_ui_runtime`; anything resolving
        // the slot during that frame must not see it empty.
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        debug_assert_eq!(
            std::thread::current().id(),
            owner_dispatch.owner_thread,
            "web bootstrap must run on the ui_runtime's owner thread"
        );
        // Routed through dispatch -- see `run_desktop`'s matching comment.
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::Lifecycle(AppLifecycleState::Resumed)),
        );

        tracing::info!("Web platform initialized with callbacks");
        Ok(())
    }

    // Run the event loop (takes ownership of the platform). No
    // `OwnerHostClearGuard` here — deliberately: `WebPlatform::run` installs
    // the RAF callback and returns immediately, and tearing down the ui_runtime
    // (or the owner host) at that point would destroy it before the first
    // frame. The host stays owner-TLS resident for the page's lifetime
    // (ADR-0039 §6/§7 "wasm posture"). An explicit web detach/quit
    // ownership hook is deferred until the platform exposes a callback
    // whose lifetime encloses the RAF registration.
    let result = platform.run(Box::new(move |owner| {
        install_owner_platform(owner)?;
        APP_RUNTIME.with(|slot| slot.borrow_mut().install_host_storage(&config));
        bootstrap_web(root, config)?;
        tracing::info!("Web platform ready");
        Ok(())
    }));

    // `on_ready`'s `Err` propagates straight out of `Platform::run`:
    // `WebPlatform::run` does not install the RAF loop over a half-built
    // page in that case.
    if let Err(err) = result {
        panic!("web bootstrap failed: {err:?}");
    }
}
