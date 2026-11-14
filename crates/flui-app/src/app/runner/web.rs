use flui_view::View;
use flui_widgets::app_root;

use super::frame_pacing::{FallbackGate, WakeAction, wake_action};
use super::gpu_loss::GpuLossLedger;
use super::host::{install_owner_platform, with_owner_platform};
use super::owner_dispatch::prepare_platform_ui_runtime;
use super::raster_host::RasterSlot;
use crate::app::AppConfig;

// ============================================================================
// Web Implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
pub(super) fn run_web<V>(
    root: V,
    config: AppConfig,
    opening: super::OpenSettings,
) -> Result<(), crate::app::LaunchError>
where
    V: View + Clone + 'static,
{
    use std::sync::Arc;

    use flui_platform::WindowOptions;

    tracing::info!("Starting web platform via flui-native");

    // Platform init is an environment failure (unsupported browser, missing
    // wasm feature, driver problem), not a `BUG:` invariant: the caller gets
    // it as a `LaunchError`.
    let platform = flui_native::current_platform().map_err(|error| {
        tracing::error!(%error, "Failed to initialize platform");
        crate::app::LaunchError::from_platform(error)
    })?;

    // The driver agent (ADR-0239), beside the application's own hooks, when
    // the page's launch object named an export (ADR-0288 §7), which the
    // platform took off the page as it was made. A driver that goes under
    // `EXIT_ON_DISCONNECT` quits the loop; the driver closes the tab.
    let disconnect = Arc::new(crate::app::agent::Disconnect::default());
    let mut config = config;
    crate::app::agent::install_from_launch(
        &mut config,
        |key| platform.launch_value(key),
        &disconnect,
        // A page records the input layer only.
        false,
    );

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
        V: View + Clone + 'static,
    {
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_native::OwnerPlatform) -> R) -> R {
            with_owner_platform(f)
                .expect("BUG: bootstrap_web runs only after install_owner_platform")
        }

        // 0. The platform clipboard (ADR-0312 §6) was installed with the
        // owner platform; the ui_runtime below takes it through `build_ui_runtime`.
        //
        // 1. Open window (creates canvas). `Ready` is guaranteed inside
        // `on_ready` (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let routes = super::window_routes::WindowRoutes::new();
        let host =
            match owner_platform_installed(|owner| owner.open_window(options, routes.clone()))
                .and_then(flui_native::WindowOpen::try_ready)
            {
                Ok(window) => window,
                Err(error) => {
                    tracing::error!(%error, "Failed to create canvas window");
                    return Err(
                        anyhow::Error::from(error).context("Failed to create canvas window")
                    );
                }
            };
        routes.adopt(host.id());
        let presentation_window =
            super::presentation_window(host).with_pointer_resampling(config.pointer_resampling);
        let window = std::rc::Rc::clone(presentation_window.window());

        // 3. Mount root widget at the LOGICAL size; the paint root's DPR
        // transform maps to the physical canvas. `UiRuntime::try_new` applies the
        // DPR to the freshly built pipeline before returning.
        let geometry = super::window_install::SurfaceGeometry::of(window.as_ref());
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
        // Vended while the ui_runtime is still ours, as on the desktop: an
        // installation that fails drops it, and the hooks are handed only a
        // window that finished installing.
        let agent_window = super::dev_agents::vend(&ui_runtime, ui_runtime.presentation_id());
        // The browser takes a canvas's WebGL context away whenever it reclaims
        // GPU memory, often from a tab nobody sees, where no animation frame
        // would notice. The loss wakes the owner, whose next turn retires the
        // GPU context and asks for another; the same ledger takes a lost WebGPU
        // device, whose driver report wakes the owner too (`on_device_lost`).
        let gpu_loss = Arc::new(GpuLossLedger::default());
        let wake = ui_runtime.wake_handle();
        {
            let (gpu_loss, wake) = (Arc::clone(&gpu_loss), Arc::clone(&wake));
            routes.route_surface_changed(move |change| {
                if change == flui_platform::SurfaceChange::Rebuild {
                    gpu_loss.record_context_restore();
                } else {
                    gpu_loss.record_context_loss();
                }
                wake();
            });
        }
        // The textures the page's widgets register ride its frames to the
        // canvas's slot.
        let textures = ui_runtime.texture_registry(ui_runtime.presentation_id());
        let size = geometry.physical;
        let mut installation = prepare_platform_ui_runtime(ui_runtime, std::rc::Rc::clone(&window));
        installation.dev_agent(agent_window);
        // The canvas is a slot of the loop's raster executor (ADR-0305), placed
        // once the page's GPU context arrives: the owner thread cannot wait for
        // it, so the driver awaits it and attaches then.
        let raster = RasterSlot {
            host: super::host::APP_RUNTIME.with(|slot| slot.borrow().raster_host.clone()),
            address: installation.dispatcher().address,
            size: (size.width as u32, size.height as u32),
            device: flui_engine::DeviceHealth::Healthy,
        };
        installation.submit_web(super::frame_driver::FrameDriver::Web(WebFrameDriver {
            raster,
            attached: false,
            built: false,
            window,
            textures,
            gpu_loss,
            owner_wake: wake,
            frame_latency: config.frame_latency,
            surface_color: config.surface_color,
        }))?;

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
    let result = platform.run(crate::app::runner::opening_app_handler(
        opening,
        move |owner| {
            let quit = owner.proxy();
            disconnect.arm(move || {
                let _ = quit.request_quit();
            });
            install_owner_platform(owner)?;
            super::host::install_configured_executors(&config);
            super::host::install_host_storage(&config);
            // Started before the page's one window opens, as a desktop loop starts
            // before its first; a failed bootstrap ends them, which detaches the
            // hooks. The loop they serve is the page's, which ends only with a
            // quit (a driver's going under `EXIT_ON_DISCONNECT`): the quit ends
            // them, so a served page's export goes and its connections close.
            super::host::route_quit(super::dev_agents::end);
            super::dev_agents::start(&config);
            let bootstrapped = bootstrap_web(root, config);
            if bootstrapped.is_err() {
                super::dev_agents::end();
            }
            bootstrapped?;
            tracing::info!("Web platform ready");
            Ok(())
        },
    ));

    // `on_ready`'s `Err` propagates straight out of `Platform::run`:
    // `WebPlatform::run` does not install the RAF loop over a half-built
    // page in that case.
    if let Err(err) = result {
        panic!("web bootstrap failed: {err:?}");
    }
    Ok(())
}

pub(super) struct WebFrameDriver {
    /// The canvas's slot on the loop's raster executor, once the page's GPU
    /// context has arrived.
    raster: RasterSlot,
    /// Whether the slot was attached at the last wake: a slot attached since
    /// draws in full on a canvas that starts empty.
    attached: bool,
    /// Whether a context ever reached the canvas: a build before that is the
    /// first, whose arrival asks for the first frame.
    built: bool,
    window: std::rc::Rc<dyn flui_platform::PlatformWindow>,
    /// The external textures the page's widgets register.
    textures: Option<flui_layer::TextureRegistry>,
    /// What the browser or the executor reported lost, and not yet rebuilt.
    gpu_loss: std::sync::Arc<GpuLossLedger>,
    /// Wakes the owner: the next turn draws on a rebuilt context without
    /// waiting for a frame.
    owner_wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    /// The swapchain latency the canvas's slot is given.
    frame_latency: flui_engine::FrameLatency,
    /// The colour capability the window asked of its surface.
    surface_color: flui_engine::SurfaceColor,
}

impl WebFrameDriver {
    pub(super) fn installed(&mut self, authority: super::frame_driver::FrameLiveness) {
        // The first build is held like a rebuild, so a loss while the adapter
        // is being asked for neither starts a second one nor is forgotten: a
        // failed first build is owed again and made at the next wake.
        let claim = self.gpu_loss.claim_initialization();
        self.spawn_build(claim, authority, true);
    }

    pub(super) fn resize(&mut self, _metrics: flui_foundation::geometry::WindowMetrics) {
        // The canvas's backing store, which the browser may have sized from
        // its device-pixel layout rather than `size × scale`; the next frame
        // configures the slot at it.
        let size = super::window_install::SurfaceGeometry::of(self.window.as_ref()).physical;
        self.raster.size = (size.width as u32, size.height as u32);
    }

    pub(super) fn wake(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
        liveness: super::frame_driver::FrameLiveness,
    ) {
        // Before the dirty gate: a lost device or context needs no animation
        // frame of new content to be acted on.
        self.start_rebuild_if_owed(&liveness);
        let gpu_loss = &self.gpu_loss;
        let action = ui_runtime.enter(|ui_runtime| {
            // A rebuilt context draws on a canvas that starts empty.
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
            other @ (WakeAction::Render | WakeAction::PumpBackground | WakeAction::Skip) => other,
        };
        match action {
            WakeAction::Skip => return,
            WakeAction::PumpBackground => {
                // Frames disabled: service owner commands, async tasks, and
                // input deadlines without a visual frame. See
                // `UiRuntime::pump_background` for the latch-first order.
                //
                // No `BackgroundPace` deferral here, unlike Android: this callback
                // is driven by the browser's `requestAnimationFrame` loop
                // (`start_raf_loop`, `flui-native`'s web backend),
                // which fires unconditionally once per animation
                // frame regardless of whether a redraw was
                // requested — the browser's own vsync-paced RAF
                // cadence already bounds this arm's re-wake rate, so
                // an additional deferral would be redundant. A sleep would
                // be unsound here: `wasm32-unknown-unknown` has
                // no real OS threads, and blocking the single JS
                // thread with `std::thread::sleep` would hang the
                // page rather than pace it.
                ui_runtime.pump_background();
                return;
            }
            WakeAction::Render => {}
        }

        // No frame runs before the canvas has a slot: the ui_runtime stays
        // dirty, and the first animation frame after the context arrives
        // renders.
        let address = self.raster.address;
        let attached = self
            .raster
            .host
            .try_read(|host| host.has_slot(address))
            .unwrap_or(false);
        let newly_attached = attached && !self.attached;
        self.attached = attached;
        if !attached {
            return;
        }
        if newly_attached {
            self.built = true;
            // A context bootstrapped again starts healthy, and its canvas
            // empty: whatever the last one reported of its device is over.
            self.raster.device = flui_engine::DeviceHealth::Healthy;
            ui_runtime.note_primary_device_recovery(None);
            ui_runtime.mark_primary_needs_full_repaint();
        }
        // A changed colour of the page's screen sets the canvas surface up
        // again before this frame paints (ADR-0256 §6).
        let display = ui_runtime
            .window_display(ui_runtime.presentation_id())
            .and_then(|display| display.color);
        let now = web_time::Instant::now();
        let _presented = self.raster.pump(ui_runtime, now, display);
        // A device lost during the frame is bootstrapped again now rather
        // than at the next wake: the web's executor cannot rebuild it in
        // place (ADR-0305 §8).
        self.start_rebuild_if_owed(&liveness);
    }

    /// Bootstrap the page's GPU context again when something was lost and
    /// the rebuild can succeed, or for the first time when the first build
    /// failed.
    ///
    /// A lost context is retired before the next one is asked for: its slot
    /// and its surface go, so the canvas is never presented to by two
    /// contexts. A failed build asks for the next animation frame and is
    /// retried there, or at the context's restoration, and not at once: the
    /// driver may still be resetting, and an immediate retry would ask it
    /// again in a loop. A hidden tab runs no frames, so the retry waits for
    /// it to be shown.
    fn start_rebuild_if_owed(&mut self, liveness: &super::frame_driver::FrameLiveness) {
        if self
            .raster
            .host
            .try_read(|host| host.device_health().is_lost())
            .unwrap_or(false)
        {
            self.gpu_loss.record_device_loss();
        }
        let Some(claim) = self.gpu_loss.claim_rebuild() else {
            return;
        };
        // Retired now, not when the future first runs: a frame between the
        // two would draw on the lost context, and its own loss report would
        // owe a second rebuild.
        if self
            .raster
            .host
            .try_with(super::raster_host::RasterHost::retire_executor)
            .is_none()
        {
            // The host is turning up the stack: the claim goes back, and the
            // next wake retires and rebuilds.
            drop(claim);
            return;
        }
        self.attached = false;
        let first_build = !self.built;
        self.spawn_build(claim, liveness.clone(), first_build);
    }

    /// Bootstrap the page's GPU context over the canvas on the browser's
    /// executor and attach the canvas's slot on it. Success settles `claim`
    /// and wakes the owner; failure drops it, which owes the build again.
    fn spawn_build(
        &self,
        claim: super::gpu_loss::RebuildClaim,
        authority: super::frame_driver::FrameLiveness,
        first_build: bool,
    ) {
        use std::sync::Arc;
        let Some(display_connection) =
            super::host::with_owner_platform(flui_native::OwnerPlatform::display_connection)
        else {
            tracing::error!("BUG: the web frame driver runs with no owner platform");
            return;
        };
        let host = self.raster.host.clone();
        let address = self.raster.address;
        let window = std::rc::Rc::clone(&self.window);
        let textures = self.textures.clone();
        let (frame_latency, surface_color) = (self.frame_latency, self.surface_color);
        let wake = Arc::clone(&self.owner_wake);
        wasm_bindgen_futures::spawn_local(async move {
            if !authority.is_live() {
                return;
            }
            let built = flui_engine::GpuContext::bootstrap(
                display_connection,
                Some(window.surface_target()),
            )
            .await;
            if !authority.is_live() {
                return;
            }
            let mut bootstrap = match built {
                Ok(bootstrap) => bootstrap,
                Err(error) => {
                    if let Some(unsupported) = crate::app::UnsupportedSystem::from_renderer(&error)
                    {
                        // Neither WebGPU nor WebGL2 gave a device: the page is
                        // below FLUI's floor, and asking again finds the same
                        // (ADR-0244 §3). The claim is settled, so no rebuild is
                        // owed and, with no slot, no frame runs.
                        tracing::error!(%unsupported, "FLUI cannot run on this system");
                        claim.complete();
                        return;
                    }
                    log_failed_build(&error);

                    // No frame comes unasked, and the retry runs in one.
                    window.request_redraw();
                    return;
                }
            };
            // A lost WebGPU device is acted on without waiting for new content:
            // the wake asks for the next animation frame, which retires the
            // context; a visible tab runs it at once, a hidden one when shown.
            bootstrap.context.on_device_lost(Arc::clone(&wake));
            // Read now: the page's screen may have changed while the device
            // was asked for.
            let display = super::host::display_color_of(window.as_ref());
            let config = flui_engine::SlotConfig::new()
                .color(surface_color, display)
                .latency(frame_latency);
            let attached = host
                .try_with(|host| {
                    host.attach_bootstrapped(
                        bootstrap,
                        window.as_ref(),
                        address,
                        config,
                        textures,
                        display,
                    )
                })
                .unwrap_or_else(|| Err(super::raster_host::host_busy()));
            if let Err(error) = attached {
                log_failed_build(&error);

                window.request_redraw();
                return;
            }
            claim.complete();
            if first_build {
                window.request_redraw();
                tracing::info!("GPU context initialized for the page");
            } else {
                tracing::warn!("GPU lost — context bootstrapped again");
                wake();
            }
        });
    }
}

/// Log a GPU context build that failed and is retried at the next frame.
fn log_failed_build(error: &flui_engine::EngineError) {
    if matches!(
        error,
        flui_engine::EngineError::SurfaceTargetUnavailable { .. }
    ) {
        // A backgrounded tab's canvas can report its target gone
        // transiently, so the log is a warning.
        tracing::warn!(%error, "GPU build failed — canvas unavailable; retried at the next frame");
    } else {
        tracing::error!(%error, "GPU build failed; retried at the next frame");
    }
}
