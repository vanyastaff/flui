use crate::app::hot_reload::WorkerReload;
use flui_engine::Renderer;
use flui_native::HostWindow;
use flui_platform::PlatformWindow;
use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};
use parking_lot::Mutex;
use std::rc::Rc;
use std::sync::Arc;

use super::device_recovery::{
    FrameRecoveryOutcome, new_device_recovery_backoff, pump_with_device_recovery,
};
use super::first_reveal::FirstReveal;
use super::frame_pacing::{
    FallbackWake, WakeAction, frame_is_dirty, install_pre_present_hook, keeps_frame_gate_open,
    wake_action,
};
use super::host::{APP_RUNTIME, desktop_secondary_wake_deadline, merge_wake_deadlines};
use super::owner_dispatch::{
    PresentationDispatcher, RuntimeTask, close_this_window, dispatch_platform_ui_runtime,
    install_input_wiring, prepare_ui_runtime_alongside,
};
use crate::app::AppConfig;

pub(super) struct RenderedMain {
    pub(super) window: Rc<dyn PlatformWindow>,
    pub(super) address: flui_foundation::PresentationAddress,
    pub(super) installation: super::installed_host::Installation,
}

struct InstallRollback {
    window: Rc<dyn PlatformWindow>,
    dispatcher: Option<PresentationDispatcher>,
    committed: bool,
}
impl Drop for InstallRollback {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Some(dispatcher) = self.dispatcher {
            crate::app::application_control::contain(|| close_this_window(dispatcher));
        }
        crate::app::application_control::contain(|| {
            super::discard_window(self.window.as_ref(), "a rolled-back main window");
        });
    }
}

/// The window options for a window this module's runner will drive:
/// [`From<&AppConfig>`]'s options with the reveal deferred to the first
/// presented frame. The deferral is a promise to call
/// `PlatformWindow::reveal_after_first_frame`, and [`install_desktop_window`]
/// is what keeps it (the [`FirstReveal`] policy wired into its frame
/// closure), so only the two open sites that install it — the main window
/// and a content-bearing secondary window — ask for it. `run_direct` and the
/// bare `open_secondary_window` convert `AppConfig` directly and reveal at
/// open, because nothing in them would ever perform the reveal.
pub(super) fn rendered_window_options(config: &AppConfig) -> flui_platform::WindowOptions {
    let mut options = flui_platform::WindowOptions::from(config)
        .with_reveal(flui_platform::WindowReveal::AfterFirstFrame);
    // Read before the window opens, so its first reveal is already in place.
    super::placement::restore(&mut options, config);
    options
}

pub(super) fn install_desktop_window<V>(
    root: V,
    config: &AppConfig,
    worker_reload: WorkerReload,
    host: Rc<dyn HostWindow>,
    host_lifecycle: AppLifecycleState,
) -> Result<RenderedMain, crate::app::AppWindowError>
where
    V: View + Clone + 'static,
{
    let presentation_window =
        super::presentation_window(host).with_pointer_resampling(config.pointer_resampling);
    let window = Rc::clone(presentation_window.window());
    let mut rollback = InstallRollback {
        window: Rc::clone(&window),
        dispatcher: None,
        committed: false,
    };
    // Recovery and reveal deadlines belong to this presentation. The native
    // frame registry retains their metadata even while its driver is leased.
    let device_recovery_backoff = Arc::new(new_device_recovery_backoff());

    // 0d. The window's deferred first reveal (see `FirstReveal`): the
    // platform opened it hidden-but-intended-visible, and the frame closure
    // below hands it the reveal on the first presented frame, or at the
    // policy's fallback bound. Its deadline joins the wake hook and the
    // dirty predicate like every other wake-deadline source here.
    let first_reveal = Arc::new(FirstReveal::new());

    // 2. Create GPU renderer directly (no DesktopEmbedder). `Renderer::new`
    // takes ownership of a `WindowTarget`: the window's
    // surface target, which the raster side may hold on any thread, never
    // the window itself (ADR-0254 §5).
    let geometry = super::window_install::SurfaceGeometry::of(window.as_ref());
    let phys_size = geometry.physical;
    let surface = window.surface_target();
    let renderer = pollster::block_on(Renderer::with_surface_color(
        Arc::clone(&surface),
        config.surface_color,
    ));
    let mut renderer = match renderer {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GPU init failed: {:?}", e);
            return Err(crate::app::AppWindowError::Renderer {
                source: Arc::new(e),
            });
        }
    };
    renderer.resize(phys_size.width as u32, phys_size.height as u32);
    renderer.set_frame_latency(config.frame_latency);
    // The platform's frame-pacing signal (Wayland frame callbacks) is
    // armed by the renderer right before each present — see
    // `install_pre_present_hook`'s own doc and ADR-0058.
    install_pre_present_hook(&mut renderer, surface);

    // 3. Mount root widget at the LOGICAL size; the framework lays out
    // in logical pixels and the paint root's DPR transform maps to the
    // physical surface. `UiRuntime::new` applies the DPR to the freshly
    // built pipeline before returning, so the RenderView configuration
    // and the first frame agree on the scale from construction.
    let scale_factor = geometry.scale;
    let ui_runtime = match super::host::build_ui_runtime(presentation_window, scale_factor) {
        Ok(ui_runtime) => ui_runtime,
        Err(e) => {
            tracing::error!(error = %e, "UiRuntime construction failed");
            return Err(crate::app::AppWindowError::Mount {
                source: Arc::new(e),
            });
        }
    };

    // Debug overlay: `Some` stats IS the enable flag, so this is the
    // single point that turns the frame path's overlay work on.
    ui_runtime.set_performance_overlay(config.show_performance_overlay);
    ui_runtime.set_motion_override(config.motion_override);

    // Typed frame-failure route (issue #561): failures are contained
    // per presentation either way; this only adds the embedder's
    // delivery.
    ui_runtime.set_frame_failure_handler(config.frame_failure_handler.clone());
    ui_runtime.set_frame_failure_detail(config.frame_failure_detail);
    config.install_input_tap(&ui_runtime);
    config.install_error_view_builder(&ui_runtime);

    ui_runtime.enter(|ui_runtime| ui_runtime.update_host_lifecycle(host_lifecycle));
    let logical = geometry.logical;
    let attach = ui_runtime.enter(|ui_runtime| {
        ui_runtime.attach_root_widget_with_size(&root, logical.width, logical.height)
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return Err(crate::app::AppWindowError::Mount {
            source: Arc::new(e),
        });
    }
    worker_reload.register_ui_runtime(&ui_runtime);
    // The development agent's window is vended while the ui_runtime is still
    // ours: if the installation below fails, the ui_runtime drops it, and the
    // hook is handed only a window that finished installing.
    let agent_window = config
        .dev_agent
        .as_ref()
        .and_then(|agent| agent.vend(&ui_runtime, ui_runtime.presentation_id()));

    // 3c. Construct the per-window owner and its bounded command inbox.
    // The wake is the existing chain: `wake_frame` sets
    // `needs_redraw` and queues a `RedrawRequested`, so a command sent to an
    // idle loop produces the frame whose drain observes it.
    //
    tracing::info!(
        { flui_foundation::diagnostics::PRESENTATION_ID } = ui_runtime.presentation_id().as_u64(),
        inbox_capacity = ui_runtime.command_sender().capacity(),
        "UiRuntime constructed"
    );
    let textures = ui_runtime.texture_registry(ui_runtime.presentation_id());
    let mut installation = prepare_ui_runtime_alongside(ui_runtime, Rc::clone(&window));
    let owner_dispatch = installation.dispatcher();
    rollback.dispatcher = Some(owner_dispatch);

    // 3c1. Wire this presentation into the close-request seam (issue
    // #558): the application's own answer to "may this window close?",
    // plus the entry that makes the window closable programmatically
    // afterwards. One shared implementation with
    // `open_secondary_window`'s window — see that function's own doc.
    installation.close_requests(config.close_request_handler.clone());

    // 3c2. The frame-pacing fallback (ADR-0058): a non-blocking bound on
    // ticker-driven wakes that present nothing, anchored to this
    // window's own display period. Replaces the fixed 16 ms
    // `thread::sleep` on this very thread — which was the only pacer on
    // any stack whose present does not block at vsync, and which
    // quantized every animation to ~60 Hz and blocked input for its
    // duration. It keeps no period of its own: every arm reads the
    // presentation's current refresh interval, which follows the display the
    // window is on (a mode switch, a move to another monitor, a display that
    // reports nothing) with or without a resize.
    let fallback = Arc::new(FallbackWake::new());

    // 3d. Wire the wall-clock-wake hook, now that `owner_dispatch`
    // exists — the winit backend's `about_to_wait` consults this every
    // idle iteration instead of blocking forever, so a pending gesture-
    // arena deadline (a long-press hold, a double-tap give-up) or an
    // armed device-recovery retry still wakes the loop at the right
    // instant even while nothing else is dirty and no animation is
    // running. Moved here from directly after step 0c (this window's
    // `device_recovery_backoff` construction) specifically so this
    // closure can capture `owner_dispatch.address.ui_runtime_id` and look up
    // THIS ui_runtime's `frames_enabled` state each time it runs — see the
    // closure body's own comment for why that lookup, not just
    // `next_attempt_at()`, is required.
    let deadlines = Arc::new(DesktopWakeDeadlines {
        device_recovery_backoff: Arc::clone(&device_recovery_backoff),
        fallback: Arc::clone(&fallback),
        first_reveal: Arc::clone(&first_reveal),
    });

    // 4. Adopt the raster mailbox (ADR-0045's inline lane). The lane's
    // owner SOLELY owns the renderer from here on: the platform resize
    // path holds only the lane's mailbox handle plus the owner-affine
    // stamp/size state, and the frame closure below drives every
    // submit through the mailbox pump — an owned, stamped
    // `SceneSnapshot` per frame, checked against the lane's single
    // `SurfaceGeneration` counter. `Arc<Mutex<RasterLane>>` mirrors the
    // `Arc<Mutex<Renderer>>` it replaces (the platform's callback
    // registrations require `Send` closures even though they only ever
    // fire on this owner thread); a reentrant frame dispatch — already
    // skipped upstream by the empty-slot drain protection — degrades to
    // a skipped frame at the `try_lock` below instead of a same-thread
    // lock deadlock.
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

    // 5. Register input callback -> entered ui_runtime input dispatch
    install_input_wiring(owner_dispatch, window.as_ref());

    // Install frame resources once; native deliveries carry only their binding.
    installation.dev_agent(config.dev_agent.clone().zip(agent_window));
    let frame = installation
        .frame_driver(super::frame_driver::FrameDriver::Desktop(
            DesktopFrameDriver {
                lane: Arc::clone(&lane),
                worker_reload,
                deadlines,
                reveal_window: Rc::downgrade(&window),
            },
        ))
        .map_err(|error| crate::app::AppWindowError::Mount {
            source: Arc::new(error),
        })?;
    let binding = frame.binding;
    super::window_routes::WindowRoutes::of(&*window).route_frame_requested(move |request| {
        let _ = dispatch_platform_ui_runtime(owner_dispatch, RuntimeTask::Frame(binding, request));
    });

    // 8. Lifecycle callbacks
    //
    // Detached is dispatched through the ui_runtime because shutdown must
    // cancel any pointer sequence whose platform Up/Cancel will never
    // arrive before lifecycle observers run.

    // Explicit quit also reaches ui_runtimes whose windows never closed. The
    // loop-owned callback visits surviving ui_runtimes rather than capturing
    // this primary dispatch address, which may already be uninstalled.

    // Window close -> close THIS window's own presentation before the
    // platform decides whether to exit. Load-bearing ordering, not just
    // bookkeeping hygiene: the winit backend's `CloseRequested` handling
    // delivers a window's `closed` (this route) BEFORE it consults the
    // exit-policy hook `install_exit_policy_hook` installs above
    // (`AppRuntime::should_exit`, which decides purely from
    // `AppRuntime`'s own ui_runtime registry, never this backend's native
    // window count). Without this call, closing this app's only window
    // would leave the ui_runtime registry non-empty forever — nothing else
    // ever removes it — and the exit-policy hook would report "don't
    // exit" on every subsequent window close, including the very last
    // one, silently hanging the app open with no window left at all.
    // `close_this_window` (not `request_ui_runtime_uninstall` directly): the
    // primary window closing while a `WindowPolicy::Shared` sibling
    // survives must remove only THIS presentation, never the whole
    // ui_runtime out from under that sibling; `close_this_window` reduces to
    // the same full-ui_runtime-uninstall effect exactly when this is the
    // ui_runtime's sole presentation (today's single-window desktop shape).
    // The tail `teardown_platform_ui_runtime()` call still runs once
    // `Platform::run` actually returns, for the clipboard/redraw-window
    // cleanup no per-window close performs.
    let closing_window_id = window.id();
    super::placement::keep(config, owner_dispatch.address);
    installation.on_close(move || {
        tracing::info!("Window closed");
        super::placement::finish(owner_dispatch.address);
        super::main_window::main_window_closing(owner_dispatch.address);
        let closed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            close_this_window(owner_dispatch);
        }));
        // Release the redraw-poke slot's pin on this window NOW, while
        // the platform event loop is still alive — this slot was the
        // one `Arc` that survived `Platform::run`, deferring the
        // window's native teardown (and, before the platform's own
        // callback clear existed, the GPU surface teardown chained
        // behind it) to after the loop was gone: the Wayland post-quit
        // SIGSEGV of issue #713. Keyed by id, not unconditional, so
        // closing a `WindowPolicy::Shared` sibling never unpins the primary.
        // Dropped outside the TLS borrow: the winit window's own drop
        // may re-enter platform code.
        let released =
            APP_RUNTIME.with(|slot| slot.borrow().release_redraw_window_for(closing_window_id));
        drop(released);
        super::main_window::main_window_closed(owner_dispatch.address);
        if let Err(payload) = closed {
            std::panic::resume_unwind(payload);
        }
    });

    // No `WindowHandler::close_requested` registration here: step 3c1 above installed
    // it, together with the router entry it consults, so the two can
    // never be wired apart.

    // 7. The window's facts — metrics, focus, visibility, execution, hover,
    // appearance, safe area and display — reach the presentation from its
    // one snapshot subscription, seeded from the snapshot read right after
    // it; the resize applier installed above (not the subscription) touches
    // the renderer.
    installation.observe_window();

    // 9. Store the window in AppRuntime's redraw-poke slot — BEFORE
    // marking the lifecycle Resumed or requesting the initial redraw.
    // Both of those can synchronously run the first frame through
    // `dispatch_platform_ui_runtime`; if the slot were still empty at that
    // point, anything resolving it during that frame would silently
    // no-op instead of waking the loop.
    APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(Rc::clone(&window)));

    // Start the host through the shared lifecycle dispatch. Each
    // presentation derives its initial state from observed native facts,
    // so a hidden or unfocused window starts Hidden or Inactive even
    // though the host itself is running.
    debug_assert_eq!(
        std::thread::current().id(),
        owner_dispatch.owner_thread,
        "desktop bootstrap must run on the ui_runtime's owner thread"
    );
    installation.lifecycle(host_lifecycle);

    // Publication applies the initial observations before waking or announcing
    // the window. Reentrant callers retain the pending completion receipt.
    let installation = installation.submit();
    rollback.committed = true;
    Ok(RenderedMain {
        window,
        address: owner_dispatch.address,
        installation,
    })
}

pub(super) fn run_desktop<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    if let Err(error) = crate::app::Application::new(move |_| root.clone())
        .with_config(config)
        .run()
    {
        panic!("desktop bootstrap failed: {error}");
    }
}

pub(super) struct DesktopFrameDriver {
    pub(super) deadlines: Arc<DesktopWakeDeadlines>,
    lane: Arc<Mutex<crate::app::raster_lane::RasterLane<Renderer>>>,
    worker_reload: WorkerReload,
    reveal_window: std::rc::Weak<dyn PlatformWindow>,
}

/// Presentation-owned pacing sources, retained even while its driver is leased.
pub(super) struct DesktopWakeDeadlines {
    pub(super) device_recovery_backoff: Arc<super::device_recovery::DeviceRecoveryBackoff>,
    pub(super) fallback: Arc<FallbackWake>,
    pub(super) first_reveal: Arc<FirstReveal>,
}

impl DesktopWakeDeadlines {
    /// Defer the next ticker-only wake of `presentation` after a pump that
    /// presented nothing, to a fraction of the refresh interval it is paced by
    /// right now: read from the presentation here, at the arm, so a display
    /// that changed mode, or a window that moved to another display, is
    /// followed without waiting for a resize. Returns the armed instant.
    pub(super) fn defer_after_no_present(
        &self,
        ui_runtime: &crate::app::ui_runtime::UiRuntime,
        presentation: flui_foundation::PresentationId,
        now: web_time::Instant,
    ) -> web_time::Instant {
        self.fallback
            .arm_after_no_present(now, ui_runtime.refresh_interval(presentation))
    }

    /// The earliest armed deadline. `period` is the presentation's current
    /// refresh interval, which the pacing fallback needs to tell a late wake
    /// from one that is never coming.
    pub(super) fn next_wake(
        &self,
        frames_enabled: bool,
        period: std::time::Duration,
    ) -> Option<web_time::Instant> {
        if !frames_enabled {
            return None;
        }
        let deadline = merge_wake_deadlines(
            merge_wake_deadlines(
                self.device_recovery_backoff.next_attempt_at(),
                self.fallback.next_wake(web_time::Instant::now(), period),
            ),
            self.first_reveal.next_deadline(),
        );
        desktop_secondary_wake_deadline(deadline, frames_enabled)
    }
}

impl DesktopFrameDriver {
    pub(super) fn installed() {
        super::host::with_owner_platform(|owner| owner.activate(false));
        tracing::info!("Desktop platform initialized with callbacks");
    }
    pub(super) fn resize(&mut self, metrics: flui_foundation::geometry::WindowMetrics) {
        let resize = self.lane.lock().resize_hook();
        let physical = metrics.physical();
        resize.apply(
            physical.width.unsigned_abs(),
            physical.height.unsigned_abs(),
        );
    }
    pub(super) fn wake(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
        presentation: flui_foundation::PresentationId,
    ) {
        let lane_frame = &self.lane;
        let worker_reload_frame = &self.worker_reload;
        let device_recovery_backoff = &self.deadlines.device_recovery_backoff;
        let fallback = &self.deadlines.fallback;
        let first_reveal = &self.deadlines.first_reveal;
        let reveal_window = &self.reveal_window;
        // The gate half of the wake runs inside one ui_runtime entry and
        // decides; the pump below enters the ui_runtime itself.
        let (action, now) = ui_runtime.enter(|ui_runtime| {
            worker_reload_frame.poll_and_apply(ui_runtime);

            let scheduler = ui_runtime.scheduler();

            // Every fire of this callback is a genuine platform-delivered
            // frame-request signal on this backend (`WinitWindowEvent::
            // RedrawRequested` -> `dispatch_request_frame` -> here; see
            // `docs/adr/ADR-0044-driver-loop-hybrid.md`'s per-platform table for which backends
            // pace this via the compositor vs. deliver it immediately).
            // Recorded unconditionally, before the dirty/wake_action gate below
            // decides whether anything actually runs this pump: pacing
            // feedback is about observing the PLATFORM's own delivery timing,
            // independent of whether this particular delivery ends up idle.
            // `now` is read ONCE here and is also the pump's frame
            // timestamp below (`pump_with_device_recovery` hands it to
            // `UiRuntime::pump` as a `SampledClock`) — this wake's
            // pacing-feedback sample and its frame's timestamp must agree.
            let now = web_time::Instant::now();
            ui_runtime.record_compositor_tick(now);

            // Owner-inbox drain: commands and worker results
            // commit HERE, at the frame boundary while the scheduler phase is
            // Idle — never inside the frame transaction below. Runs before the
            // dirty gate so a command-driven redraw request is observed by the
            // very frame its wake produced.
            //
            // The runtime is TAKEN out of the slot for the drain (and restored
            // after) so drained user closures never run under the RefCell
            // borrow: a command that re-enters this frame callback through a
            // nested platform pump then finds an empty slot and skips the
            // drain, instead of panicking the borrow.
            let inbox_redraw = ui_runtime.drain_owner_inbox();

            // `device_recovery_backoff.next_attempt_at().is_some()` is a
            // REQUIRED fourth dirty source, not an optional extra: a
            // deadline wired into the wake-deadline hook (installed
            // below, after this ui_runtime exists) but absent from THIS
            // predicate reaches `WakeAction::Skip` and returns before
            // `pump_with_device_recovery` is ever called, no
            // matter how faithfully the platform actuates the wake —
            // see `DeviceRecoveryBackoff`'s own doc for the two paired
            // obligations a wake-deadline source carries and the
            // dropped-attempt trace that motivated this line. Calls
            // the shared `frame_is_dirty` (not a local reimplementation)
            // for the same reason `wake_action` itself is a named
            // function here and not inlined: this closure's own
            // `dirty` computation and the tests that pin it must run
            // the identical code, or a regression in one is invisible
            // to the other.
            // The frame-pacing deferral (ADR-0058), read ONCE per wake:
            // `gate` consumes a due deadline, so this is both the
            // "is a deferral pending" question the gate below asks and
            // the "this wake IS the deferred one" answer that makes it
            // dirty. Reading it twice would consume it in the first read
            // and skip the very wake it asked for.
            let fallback_gate = fallback.gate(now);
            let dirty = frame_is_dirty(
                inbox_redraw,
                ui_runtime.needs_redraw(),
                ui_runtime.has_pending_work(),
                // The reveal fallback joins the device-recovery deadline
                // here for the same reason that one must be present.
                merge_wake_deadlines(
                    device_recovery_backoff.next_attempt_at(),
                    first_reveal.next_deadline(),
                ),
                fallback_gate,
            );
            let action = wake_action(
                scheduler.frames_enabled(),
                dirty,
                scheduler.is_frame_scheduled(),
                fallback_gate,
            );
            (action, now)
        });

        match action {
            WakeAction::Skip => return,
            WakeAction::PumpBackground => {
                // Frames disabled (Hidden/Paused/Detached): service owner
                // commands, async tasks, and input deadlines without a visual
                // frame. `pump_background` clears the frame latch before
                // polling so accepted work can request another owner wake.
                ui_runtime.pump_background();
                return;
            }
            WakeAction::Render => {}
        }

        // The frame: `UiRuntime::pump` at `now` (apply commands ->
        // begin -> persistent callbacks -> the pipeline and submit ->
        // post-frame callbacks -> Idle, as the text-store transaction
        // with its commit anchor after it), with a lost GPU device
        // rebuilt around it: BEFORE the pump when the loss predates
        // the frame (the frame runs anyway, see
        // `pump_with_device_recovery`'s doc for why), and AFTER when
        // the wgpu device-lost callback fired mid-frame.
        let outcome = if let Some(outcome) =
            pump_with_device_recovery(ui_runtime, lane_frame, device_recovery_backoff, now)
        {
            outcome
        } else {
            // The busy lane refused admission without consuming frame work.
            // The recovery helper retains demand; this tail paces its retry.
            tracing::error!("frame skipped: raster lane busy at frame admission");
            FrameRecoveryOutcome {
                presented: false,
                just_failed: false,
                next_attempt_at: None,
            }
        };

        // Frame-pacing fallback (ADR-0058), replacing the fixed
        // 16 ms sleep this thread used to take here. A frame that
        // reached `present()` needs no bound: the present either
        // blocked at vsync or the compositor is pacing our redraws
        // (Wayland, after `pre_present_notify`). A frame that did
        // NOT present — no damage, occluded surface, surface lost —
        // got neither, so with a ticker still re-requesting a frame
        // every wake, nothing would pace the loop; the deferral
        // below bounds it to one pipeline pass per display period,
        // as a wake deadline rather than a sleep, so input stays
        // responsive throughout. A still-lost device armed for a
        // LATER retry does not feed this: `DeviceRecoveryBackoff`
        // paces the ATTEMPT itself and reaches the loop through the
        // same wake-deadline hook.
        let keeps_gate_open = keeps_frame_gate_open(
            ui_runtime.needs_redraw(),
            ui_runtime.scheduler().is_frame_scheduled(),
            ui_runtime.has_pending_work(),
        );
        let pace_now = web_time::Instant::now();
        // The deferred first reveal: handed to the window on the
        // first presented frame, or at the fallback bound after a
        // frame that ran and presented nothing (`FirstReveal`).
        if first_reveal.after_frame(outcome.presented, pace_now)
            && let Some(window) = reveal_window.upgrade()
        {
            window.reveal_after_first_frame();
        }
        if outcome.presented {
            fallback.record_present(pace_now);
        } else if keeps_gate_open {
            let deadline =
                self.deadlines
                    .defer_after_no_present(ui_runtime, presentation, pace_now);
            tracing::trace!(
                target: "flui.pace",
                event = "fallback_armed",
                in_us = deadline.saturating_duration_since(pace_now).as_micros() as u64,
                "frame presented nothing with the gate open; deferring the next \
                 ticker wake"
            );
        }
    }
}
