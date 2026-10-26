use flui_view::{StatelessView, View};

use super::frame_pacing::{FallbackGate, WakeAction, wake_action};
use super::gpu_loss::GpuLossLedger;
use super::host::{install_owner_platform, with_owner_platform};
use super::owner_dispatch::prepare_platform_ui_runtime;
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

    tracing::info!("Starting web platform via flui-native");

    // Platform init is an environment failure (unsupported browser, missing
    // wasm feature, driver problem), not a `BUG:` invariant — see the
    // matching comment in `run_desktop` above for why this is a `match` +
    // `panic!` with a full error log instead of a bare `.expect()`.
    let platform = match flui_native::current_platform() {
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
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_native::OwnerPlatform) -> R) -> R {
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
            .and_then(flui_native::WindowOpen::try_ready)
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
        ui_runtime.set_frame_failure_handler(config.frame_failure_handler.clone());
        ui_runtime.set_frame_failure_detail(config.frame_failure_detail);
        config.install_input_tap(&ui_runtime);
        config.install_error_view_builder(&ui_runtime);

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
        // Vended while the ui_runtime is still ours, as on the desktop: an
        // installation that fails drops it, and the hook is handed only a
        // window that finished installing.
        let agent_window = config
            .dev_agent
            .as_ref()
            .and_then(|agent| agent.vend(&ui_runtime, ui_runtime.presentation_id()));
        // The browser takes a canvas's WebGL context away whenever it reclaims
        // GPU memory, often from a tab nobody sees, where no animation frame
        // would notice. The loss wakes the owner, whose next turn rebuilds the
        // renderer; the same ledger takes a lost WebGPU device (the renderer
        // reports it).
        let gpu_loss = Arc::new(GpuLossLedger::default());
        let wake = ui_runtime.wake_handle();
        {
            let (gpu_loss, wake) = (Arc::clone(&gpu_loss), Arc::clone(&wake));
            window.on_surface_status_change(Box::new(move |available| {
                if available {
                    gpu_loss.record_context_restore();
                } else {
                    gpu_loss.record_context_loss();
                }
                wake();
            }));
        }
        let mut installation = prepare_platform_ui_runtime(ui_runtime, Arc::clone(&window));
        installation.dev_agent(config.dev_agent.zip(agent_window));
        owner_platform_installed(|owner| {
            installation.submit_web(
                super::frame_driver::FrameDriver::Web(WebFrameDriver {
                    renderer,
                    window,
                    gpu_loss,
                    owner_wake: wake,
                    surface_color: config.surface_color,
                    frame_latency: config.frame_latency,
                }),
                &owner.shared(),
            )
        })?;

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
        super::host::install_configured_executors(&config);
        super::host::install_host_storage(&config);
        // Attached before the page's one window opens, as a desktop loop
        // attaches before its first. A failed bootstrap drops the attachment,
        // which detaches the hook.
        let agent = config
            .dev_agent
            .as_ref()
            .and_then(crate::app::dev_agent::DevAgent::attach);
        bootstrap_web(root, config)?;
        // The loop this attachment spans is the page's, which outlives this
        // callback and never ends while the page lives (the host stays
        // resident for the same reason, above): the hook stays attached.
        std::mem::forget(agent);
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
    window: std::sync::Arc<dyn flui_platform::PlatformWindow>,
    /// What the browser or the renderer reported lost, and not yet rebuilt.
    gpu_loss: std::sync::Arc<GpuLossLedger>,
    /// Wakes the owner: the next turn rebuilds without waiting for a frame.
    owner_wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    /// The colour capability the window asked of its surface.
    surface_color: flui_engine::SurfaceColor,
    /// The swapchain latency a first-built renderer is given; a rebuilt one keeps its own.
    frame_latency: flui_engine::FrameLatency,
}

impl WebFrameDriver {
    pub(super) fn installed(&mut self, authority: super::frame_driver::FrameLiveness) {
        // The first build is held like a rebuild, so a loss while the adapter
        // is being asked for neither starts a second one nor is forgotten: a
        // failed first build is owed again and made at the next wake.
        let claim = self.gpu_loss.claim_initialization();
        self.spawn_build(None, claim, authority);
    }
    pub(super) fn resize(&mut self, _metrics: flui_foundation::geometry::WindowMetrics) {
        // A lost context cannot be configured; the rebuild reads the window's
        // size when it finishes.
        if !self.gpu_loss.can_draw() {
            return;
        }
        if let Some(renderer) = self.renderer.lock().as_mut() {
            // The canvas's backing store, which the browser may have sized
            // from its device-pixel layout rather than `size × scale`.
            let size = self.window.physical_size();
            renderer.resize(size.width as u32, size.height as u32);
        }
    }
    pub(super) fn wake(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
        liveness: super::frame_driver::FrameLiveness,
    ) {
        use crate::app::raster_lane::DirectSink;
        use flui_runtime::pump::SampledClock;
        // Before the dirty gate: a lost device or context needs no frame to be
        // noticed, and a hidden tab runs none.
        self.start_rebuild_if_owed(&liveness);
        let gpu_loss = &self.gpu_loss;
        let renderer_frame = &self.renderer;
        let action = ui_runtime.enter(|ui_runtime| {
            // A rebuilt renderer draws on a canvas that starts empty.
            if gpu_loss.take_repaint_owed() {
                ui_runtime.mark_primary_needs_full_repaint();
            }
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
        // The WebGL context is gone: nothing can draw until the browser
        // restores it, and the restoration owes a full repaint. The wake still
        // services commands, tasks and input without a frame.
        let action = match action {
            WakeAction::Render if !gpu_loss.can_draw() => WakeAction::PumpBackground,
            other => other,
        };
        match action {
            WakeAction::Skip => return,
            WakeAction::PumpBackground => {
                // Frames disabled: service owner commands, async tasks, and
                // input deadlines without a visual frame. See
                // `UiRuntime::pump_background` for the latch-first order.
                //
                // No `NO_PRESENT_FALLBACK_PACE` sleep here, unlike
                // desktop/Android: this callback is driven by the
                // browser's `requestAnimationFrame` loop
                // (`start_raf_loop`, `flui-native`'s web backend),
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

            let textures = ui_runtime.texture_registry(ui_runtime.presentation_id());
            let mut sink = DirectSink::new(r).with_textures(textures);
            let _ = ui_runtime.pump(&mut SampledClock(now), &mut sink);

            // A device lost during the frame is rebuilt now rather than at
            // the next wake.
            if r.is_device_lost() {
                gpu_loss.record_device_loss();
            }
        }
        self.start_rebuild_if_owed(&liveness);
    }

    /// Rebuild the renderer when something was lost and the rebuild can
    /// succeed; build it from scratch when there is none, because its first
    /// build failed.
    ///
    /// A failed build is retried by the next wake — the next animation frame
    /// of a visible tab, or the context's restoration — and not at once: the
    /// driver may still be resetting, and an immediate retry would ask it again
    /// in a loop.
    fn start_rebuild_if_owed(&self, liveness: &super::frame_driver::FrameLiveness) {
        if self
            .renderer
            .lock()
            .as_ref()
            .is_some_and(flui_engine::Renderer::is_device_lost)
        {
            self.gpu_loss.record_device_loss();
        }
        let Some(claim) = self.gpu_loss.claim_rebuild() else {
            return;
        };
        // Taken now, not when the future first runs: a frame between the two
        // would draw on the lost renderer, and its own loss report would owe a
        // second rebuild.
        let lost = self.renderer.lock().take();
        self.spawn_build(lost, claim, liveness.clone());
    }

    /// Build the renderer on the browser's executor: `lost` is rebuilt, and
    /// `None` is built for the first time. Success publishes it and wakes the
    /// owner; failure puts a rebuilt renderer back and leaves the claim to owe
    /// the build again.
    fn spawn_build(
        &self,
        lost: Option<flui_engine::Renderer>,
        claim: super::gpu_loss::RebuildClaim,
        authority: super::frame_driver::FrameLiveness,
    ) {
        use std::sync::Arc;
        let renderer_slot = Arc::clone(&self.renderer);
        let surface_color = self.surface_color;
        let window = Arc::clone(&self.window);
        let (gpu_loss, wake) = (Arc::clone(&self.gpu_loss), Arc::clone(&self.owner_wake));
        let frame_latency = self.frame_latency;
        wasm_bindgen_futures::spawn_local(async move {
            if !authority.is_live() {
                return;
            }
            let first_build = lost.is_none();
            let (renderer, built) = match lost {
                Some(mut renderer) => {
                    let built = renderer.recover().await;
                    (Some(renderer), built)
                }
                None => match flui_engine::Renderer::with_surface_color(
                    Arc::clone(&window),
                    surface_color,
                )
                .await
                {
                    Ok(mut renderer) => {
                        renderer.set_frame_latency(frame_latency);
                        (Some(renderer), Ok(()))
                    }
                    Err(error) => (None, Err(error)),
                },
            };
            if !authority.is_live() {
                return;
            }
            let Some(mut renderer) = renderer else {
                if let Err(error) = built {
                    tracing::error!(%error, "GPU initialization failed; retried at the next wake");
                }
                return;
            };
            if let Err(error) = built {
                // The renderer goes back either way; the dropped claim owes
                // the rebuild again.
                if let Ok(previous) = authority.publish(&renderer_slot, renderer) {
                    drop(previous);
                }
                if matches!(
                    error,
                    flui_engine::EngineError::SurfaceTargetUnavailable { .. }
                ) {
                    // The window owner reports its native handle is gone or
                    // suspended. A backgrounded tab's canvas can report this
                    // transiently, so the log is a warning.
                    tracing::warn!(
                        ?error,
                        "GPU rebuild failed — window target unavailable; retried at the next wake"
                    );
                } else {
                    tracing::error!(?error, "GPU rebuild failed; retried at the next wake");
                }
                return;
            }
            // A lost WebGPU device is reported by the browser, not by a canvas
            // event: the renderer says so, and the owner starts rebuilding at
            // once. A loss before the hook was installed is not replayed, so
            // ask once.
            let (hook_loss, hook_wake) = (Arc::clone(&gpu_loss), Arc::clone(&wake));
            renderer.on_device_lost(move || {
                hook_loss.record_device_loss();
                hook_wake();
            });
            if renderer.is_device_lost() {
                gpu_loss.record_device_loss();
            }
            // The adapter and device may take many native resizes to arrive.
            let size = window.physical_size();
            renderer.resize(size.width as u32, size.height as u32);
            match authority.publish(&renderer_slot, renderer) {
                Ok(previous) => drop(previous),
                Err(renderer) => {
                    drop(renderer);
                    return;
                }
            }
            claim.complete();
            if first_build {
                window.request_redraw();
                tracing::info!("WebGPU renderer initialized");
            } else {
                tracing::warn!("GPU lost — renderer rebuilt");
                wake();
            }
        });
    }
}
