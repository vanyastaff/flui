use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};

use super::frame_pacing::{FallbackGate, WakeAction, wake_action};
use super::host::{
    APP_RUNTIME, install_owner_platform, runtime_wake_callback, with_owner_platform,
};
use super::owner_dispatch::{
    RuntimeEvent, RuntimeTask, dispatch_platform_ui_runtime, install_input_wiring,
    prepare_platform_ui_runtime,
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
        let presentation_window =
            super::presentation_window(host).with_pointer_resampling(config.pointer_resampling);
        let window = Arc::clone(presentation_window.window());

        // 2. Shared renderer slot — starts as None, filled async once the WebGPU
        //    adapter is available. `Option` lets the frame callback skip frames that
        //    arrive before the renderer is ready.
        let renderer: Arc<Mutex<Option<Renderer>>> = Arc::new(Mutex::new(None));

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
        let mut installation = prepare_platform_ui_runtime(ui_runtime, Arc::clone(&window));
        let owner_dispatch = installation.dispatcher();

        // 4. Register input callback
        install_input_wiring(owner_dispatch, window.as_ref());

        let frame =
            installation.frame_driver(super::frame_driver::FrameDriver::Web(WebFrameDriver {
                renderer: Arc::clone(&renderer),
                window: Arc::clone(&window),
            }))?;
        let binding = frame.binding;
        window.on_request_frame(Box::new(move || {
            super::owner_dispatch::with_owner_callback(|_| {
                let _ = dispatch_platform_ui_runtime(owner_dispatch, RuntimeTask::Frame(binding));
            });
        }));

        window.on_resize(Box::new(move |size, scale_factor| {
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::Resized { size, scale_factor }),
            );
        }));

        // 6. Terminal callbacks close the binding before lifecycle observers.
        owner_platform_installed(|owner| {
            installation.terminal_callbacks(&owner.shared());
        });

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
        installation.observe(flui_runtime::owner::WindowObservation::Metrics {
            size: window.logical_size(),
            scale_factor: window.scale_factor(),
        });
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        debug_assert_eq!(
            std::thread::current().id(),
            owner_dispatch.owner_thread,
            "web bootstrap must run on the ui_runtime's owner thread"
        );
        // Routed through dispatch -- see `run_desktop`'s matching comment.
        installation.lifecycle(AppLifecycleState::Resumed);
        installation
            .submit()
            .outcome()
            .expect("BUG: the fresh web host completes its initial installation synchronously")?;

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

pub(super) struct WebFrameDriver {
    renderer: std::sync::Arc<parking_lot::Mutex<Option<flui_engine::Renderer>>>,
    window: std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
}

impl WebFrameDriver {
    pub(super) fn installed(&mut self, authority: super::frame_driver::FrameLiveness) {
        use std::sync::Arc;
        let renderer_slot = Arc::clone(&self.renderer);
        let window = Arc::clone(&self.window);
        wasm_bindgen_futures::spawn_local(async move {
            if !authority.is_live() {
                return;
            }
            let mut renderer = match flui_engine::Renderer::new(Arc::clone(&window)).await {
                Ok(renderer) => renderer,
                Err(error) => {
                    tracing::error!(%error, "GPU initialization failed");
                    return;
                }
            };
            if !authority.is_live() {
                return;
            }
            // Adapter initialization may span many native resizes.
            let size = window.physical_size();
            renderer.resize(size.width as u32, size.height as u32);
            match authority.publish(&renderer_slot, renderer) {
                Ok(previous) => drop(previous),
                Err(renderer) => {
                    drop(renderer);
                    return;
                }
            }
            window.request_redraw();
            tracing::info!("WebGPU renderer initialized");
        });
    }
    pub(super) fn resize(&mut self, size: flui_foundation::geometry::Size<f64>, scale_factor: f64) {
        if let Some(renderer) = self.renderer.lock().as_mut() {
            // Match the browser canvas backing store at fractional DPR.
            renderer.resize(
                (size.width * scale_factor).round() as u32,
                (size.height * scale_factor).round() as u32,
            );
        }
    }
    pub(super) fn wake(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
        liveness: super::frame_driver::FrameLiveness,
    ) {
        use crate::app::raster_lane::DirectSink;
        use flui_runtime::pump::SampledClock;
        use std::sync::Arc;
        let renderer_frame = &self.renderer;
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
                let renderer_recover = Arc::clone(renderer_frame);
                // A cloned, `'static` wake handle: the spawned
                // future outlives this callback's `&UiRuntime`
                // borrow, so it cannot capture `ui_runtime` itself.
                let wake = ui_runtime.wake_handle();
                let recovery_authority = liveness;
                wasm_bindgen_futures::spawn_local(async move {
                    if !recovery_authority.is_live() {
                        return;
                    }
                    // Never hold the renderer mutex across `.await`.
                    let Some(mut renderer) = renderer_recover.lock().take() else {
                        return;
                    };
                    let result = renderer.recover().await;
                    if !recovery_authority.is_live() {
                        return;
                    }
                    match recovery_authority.publish(&renderer_recover, renderer) {
                        Ok(previous) => drop(previous),
                        Err(renderer) => {
                            drop(renderer);
                            return;
                        }
                    }
                    match result {
                        Ok(()) => {
                            tracing::warn!("GPU device lost — recovered successfully");
                            wake();
                        }
                        Err(e @ flui_engine::EngineError::SurfaceTargetUnavailable { .. }) => {
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
    }
}
