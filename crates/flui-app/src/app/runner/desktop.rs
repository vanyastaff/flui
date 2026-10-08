use crate::app::hot_reload::WorkerReload;
use flui_engine::Renderer;
use flui_platform::traits::{HostWindow, PlatformWindow};
use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};
use parking_lot::Mutex;
use std::sync::Arc;

use super::device_recovery::{
    FrameRecoveryOutcome, new_device_recovery_backoff, pump_with_device_recovery,
};
use super::first_reveal::FirstReveal;
use super::frame_pacing::{
    DEFAULT_DISPLAY_PERIOD, FallbackWake, WakeAction, frame_is_dirty, install_pre_present_hook,
    keeps_frame_gate_open, wake_action,
};
use super::host::{
    APP_RUNTIME, desktop_secondary_wake_deadline, install_wake_deadline_hook, merge_wake_deadlines,
    runtime_wake_callback,
};
use super::owner_dispatch::{
    PresentationDispatcher, RuntimeEvent, RuntimeTask, close_this_window,
    dispatch_platform_ui_runtime, install_input_wiring, prepare_ui_runtime_alongside,
};
use crate::app::AppConfig;

pub(super) struct RenderedMain {
    pub(super) window: Arc<dyn PlatformWindow>,
    pub(super) address: flui_foundation::PresentationAddress,
    pub(super) installation: super::installed_host::Installation,
}

struct InstallRollback {
    window: Arc<dyn PlatformWindow>,
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
        crate::app::application_control::contain(|| self.window.close());
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
    flui_platform::WindowOptions {
        reveal: flui_platform::WindowReveal::AfterFirstFrame,
        ..config.into()
    }
}

pub(super) fn install_desktop_window<V>(
    root: V,
    config: &AppConfig,
    worker_reload: WorkerReload,
    host: Arc<dyn HostWindow>,
    host_lifecycle: AppLifecycleState,
) -> Result<RenderedMain, crate::app::AppWindowError>
where
    V: View + Clone + 'static,
{
    let presentation_window = super::presentation_window(host);
    let window = Arc::clone(presentation_window.window());
    let mut rollback = InstallRollback {
        window: Arc::clone(&window),
        dispatcher: None,
        committed: false,
    };
    // 0c. This window's device-recovery backoff, constructed here (not
    // down at step 6 alongside the renderer it paces) so the
    // wake-deadline hook below can be wired to it from the start — one
    // instance shared by the installed frame driver and deadline hook,
    // so the underlying counters/deadline persist across deliveries. See
    // `DeviceRecoveryBackoff`'s own doc.
    let device_recovery_backoff = Arc::new(new_device_recovery_backoff());

    // 0d. The window's deferred first reveal (see `FirstReveal`): the
    // platform opened it hidden-but-intended-visible, and the frame closure
    // below hands it the reveal on the first presented frame, or at the
    // policy's fallback bound. Its deadline joins the wake hook and the
    // dirty predicate like every other wake-deadline source here.
    let first_reveal = Arc::new(FirstReveal::new());

    // 2. Create GPU renderer directly (no DesktopEmbedder). `Renderer::new`
    // takes ownership of a `WindowTarget` (issue #1043) — `Arc::clone`
    // gives it its own strong ref rather than a borrow of `window`.
    let phys_size = window.physical_size();
    let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)));
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
    // The platform's frame-pacing signal (Wayland frame callbacks) is
    // armed by the renderer right before each present — see
    // `install_pre_present_hook`'s own doc and ADR-0058.
    install_pre_present_hook(&mut renderer, &window);

    // 3. Mount root widget at the LOGICAL size; the framework lays out
    // in logical pixels and the paint root's DPR transform maps to the
    // physical surface. `UiRuntime::new` applies the DPR to the freshly
    // built pipeline before returning, so the RenderView configuration
    // and the first frame agree on the scale from construction.
    let scale_factor = window.scale_factor();
    let wake = runtime_wake_callback();
    let ui_runtime = match super::host::build_ui_runtime(&wake, presentation_window, scale_factor) {
        Ok(ui_runtime) => ui_runtime,
        Err(e) => {
            tracing::error!(error = %e, "UiRuntime construction failed");
            return Err(e);
        }
    };

    // Debug overlay: `Some` stats IS the enable flag, so this is the
    // single point that turns the frame path's overlay work on.
    ui_runtime.set_performance_overlay(config.show_performance_overlay);

    // Typed frame-failure route (issue #561): failures are contained
    // per presentation either way; this only adds the embedder's
    // delivery.
    ui_runtime.set_frame_failure_handler(config.frame_failure_handler.clone());
    ui_runtime.set_frame_failure_detail(config.frame_failure_detail);

    ui_runtime.enter(|ui_runtime| ui_runtime.update_host_lifecycle(host_lifecycle));
    let logical = window.logical_size();
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
    let mut installation = prepare_ui_runtime_alongside(ui_runtime, Arc::clone(&window));
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
    // duration. The period is re-read on resize (a window can change
    // monitors); `DEFAULT_DISPLAY_PERIOD` covers a backend that cannot
    // report one.
    let fallback = Arc::new(FallbackWake::new(
        window.refresh_period().unwrap_or(DEFAULT_DISPLAY_PERIOD),
    ));
    tracing::info!(
        period_us = fallback.period().as_micros() as u64,
        reported = window.refresh_period().is_some(),
        "frame-pacing fallback period"
    );

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
    install_wake_deadline_hook({
        let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
        let fallback = Arc::clone(&fallback);
        let first_reveal = Arc::clone(&first_reveal);
        let ui_runtime_id = owner_dispatch.address.ui_runtime_id;
        move || {
            // The frames-enabled gate itself is `desktop_secondary_
            // wake_deadline` — a pure, unit-tested function (see its own
            // doc for why an unconditional report here would reproduce
            // `WinitApp::new_events`'s named busy-spin one layer up).
            // Only the `frames_enabled` LOOKUP is inline here, since it
            // needs live `APP_RUNTIME` state no pure function can carry.
            let frames_enabled = APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .installed_host
                    .logical()
                    .runtime_status(ui_runtime_id)
                    .is_ok_and(|status| status.frames_enabled)
            });
            // Both secondary sources fold through the same
            // frames-enabled gate: a deferred ticker wake is worth
            // nothing to a backgrounded app, and an armed deadline
            // reported while frames are disabled is the busy-spin
            // `desktop_secondary_wake_deadline`'s own doc names.
            // `FallbackWake::next_wake` self-clears a deadline that has
            // already passed, so a wake this hook asks for and never
            // gets (a hidden Wayland surface withholds redraws) cannot
            // be re-reported in the past forever.
            let deadlines = merge_wake_deadlines(
                merge_wake_deadlines(
                    device_recovery_backoff.next_attempt_at(),
                    fallback.next_wake(web_time::Instant::now()),
                ),
                first_reveal.next_deadline(),
            );
            desktop_secondary_wake_deadline(deadlines, frames_enabled)
        }
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
    let lane = Arc::new(Mutex::new(crate::app::raster_lane::RasterLane::new(
        renderer,
        owner_dispatch.address,
        phys_size.width as u32,
        phys_size.height as u32,
    )));

    // 5. Register input callback -> entered ui_runtime input dispatch
    install_input_wiring(owner_dispatch, window.as_ref());

    // Install frame resources once; native deliveries carry only their binding.
    installation.dev_agent(config.dev_agent.clone().zip(agent_window));
    let frame = installation
        .frame_driver(super::frame_driver::FrameDriver::Desktop(
            DesktopFrameDriver {
                lane: Arc::clone(&lane),
                worker_reload,
                device_recovery_backoff,
                fallback: Arc::clone(&fallback),
                first_reveal: Arc::clone(&first_reveal),
                reveal_window: Arc::downgrade(&window),
            },
        ))
        .map_err(|error| crate::app::AppWindowError::Mount {
            source: Arc::new(error),
        })?;
    let binding = frame.binding;
    window.on_request_frame(Box::new(move || {
        let _ = dispatch_platform_ui_runtime(owner_dispatch, RuntimeTask::Frame(binding));
    }));

    // 7. Register resize callback -> typed Resized event; the applier
    // installed above (not this closure) actually touches the renderer.
    let resize_window: Arc<dyn flui_platform::traits::PlatformWindow> = Arc::clone(&window);
    let resize_fallback = Arc::clone(&fallback);
    window.on_resize(Box::new(move |size, scale_factor| {
        // A resize can also be a move to another monitor; re-read the
        // period so the pacing fallback follows the display the window
        // is actually on (ADR-0058).
        if let Some(period) = resize_window.refresh_period() {
            resize_fallback.set_period(period);
        }
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::Resized { size, scale_factor }),
        );
    }));

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
    // calls a window's `on_close` (this callback) BEFORE it consults the
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
    installation.on_close(move || {
        tracing::info!("Window closed");
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

    // No `on_should_close` registration here: step 3c1 above installed
    // it, together with the router entry it consults, so the two can
    // never be wired apart.

    // Window focus/visibility -> the `(visible, focused)`
    // `AppLifecycleState` derivation. `on_visibility_status_change`
    // rides winit's `Occluded` event, which winit 0.30 only emits on
    // X11/macOS/iOS/Web (see that callback's own doc, verified against
    // winit's source) — winit has NO Wayland emitter for this event at
    // all, so on this workspace's own Wayland desktop reference the
    // window is always treated as visible (the same as before this
    // callback existed).
    window.on_active_status_change(Box::new(move |focused| {
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::WindowFocus(focused)),
        );
    }));
    window.on_execution_state_change(Box::new(move |state| {
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::WindowExecution(state)),
        );
    }));
    window.on_visibility_status_change(Box::new(move |visible| {
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::WindowVisibility(visible)),
        );
    }));
    let execution = window.execution_state();
    let focused = window.is_focused();
    let visible = window.is_visible();
    installation.observe(flui_runtime::owner::WindowObservation::Snapshot {
        execution,
        focused,
        visible,
    });

    window.on_hover_status_change(Box::new(move |is_hovered| {
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::WindowHover(is_hovered)),
        );
    }));
    // The platform callback carries no payload; query the window's
    // current appearance at dispatch time. Weak: the callback lives
    // inside the window's own handler table, and a strong capture
    // would cycle it alive past close.
    let appearance_window = Arc::downgrade(&window);
    window.on_appearance_changed(Box::new(move || {
        if let Some(win) = appearance_window.upgrade() {
            let _ = dispatch_platform_ui_runtime(
                owner_dispatch,
                RuntimeTask::Event(RuntimeEvent::AppearanceChanged(win.appearance())),
            );
        }
    }));
    // Seed the initial brightness — a user on a dark desktop must not
    // start light until the first live theme flip.
    installation.observe(flui_runtime::owner::WindowObservation::Brightness(
        match window.appearance() {
            flui_platform::WindowAppearance::Dark
            | flui_platform::WindowAppearance::VibrantDark => flui_platform_api::Brightness::Dark,
            flui_platform::WindowAppearance::Light
            | flui_platform::WindowAppearance::VibrantLight => flui_platform_api::Brightness::Light,
        },
    ));
    // Seed the initial size and device-pixel ratio the same way: the
    // source must not sit on defaults until the first live resize —
    // on the web backend no resize observer exists, so a default
    // would be permanent there.
    installation.observe(flui_runtime::owner::WindowObservation::Metrics {
        size: window.logical_size(),
        scale_factor: window.scale_factor(),
    });

    // 9. Store the window in AppRuntime's redraw-poke slot — BEFORE
    // marking the lifecycle Resumed or requesting the initial redraw.
    // Both of those can synchronously run the first frame through
    // `dispatch_platform_ui_runtime`; if the slot were still empty at that
    // point, anything resolving it during that frame would silently
    // no-op instead of waking the loop.
    APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(Arc::clone(&window)));

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
    lane: Arc<Mutex<crate::app::raster_lane::RasterLane<Renderer>>>,
    worker_reload: WorkerReload,
    device_recovery_backoff: Arc<super::device_recovery::DeviceRecoveryBackoff>,
    fallback: Arc<FallbackWake>,
    first_reveal: Arc<FirstReveal>,
    reveal_window: std::sync::Weak<dyn PlatformWindow>,
}

impl DesktopFrameDriver {
    pub(super) fn installed() {
        super::host::with_owner_platform(|owner| owner.activate(false));
        tracing::info!("Desktop platform initialized with callbacks");
    }
    pub(super) fn resize(&mut self, size: flui_foundation::geometry::Size<f64>, scale_factor: f64) {
        let resize = self.lane.lock().resize_hook();
        resize.apply(
            (size.width * scale_factor) as u32,
            (size.height * scale_factor) as u32,
        );
    }
    pub(super) fn wake(&mut self, ui_runtime: &mut crate::app::ui_runtime::UiRuntime) {
        let lane_frame = &self.lane;
        let worker_reload_frame = &self.worker_reload;
        let device_recovery_backoff = &self.device_recovery_backoff;
        let fallback = &self.fallback;
        let first_reveal = &self.first_reveal;
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
            WakeAction::PumpAsync => {
                // Frames disabled (Hidden/Paused/Detached): no frame
                // runs, so the mid-frame async poll never does either
                // — this background pump is the ONLY thing keeping a
                // spawned future progressing while backgrounded. It
                // clears the frame latch before polling; see
                // `UiRuntime::pump_background` for why that order is
                // load-bearing.
                ui_runtime.pump_background();
                // A backgrounded wake with dirty/pending work
                // re-requesting another wake every loop tick has the
                // identical busy-spin risk an un-presented frame with an
                // open gate has, and nothing else paces it while frames
                // are disabled — so it takes the same bound (ADR-0058),
                // as a deferral rather than a sleep. The wake-deadline
                // hook gates this source on `frames_enabled`, so the
                // deadline is not reported while backgrounded; what
                // bounds this arm is the deferral being consulted by the
                // dirty gate above on the next wake, which is where a
                // backgrounded self-re-arming task would otherwise spin.
                let keeps_gate_open = keeps_frame_gate_open(
                    ui_runtime.needs_redraw(),
                    ui_runtime.scheduler().is_frame_scheduled(),
                    ui_runtime.has_pending_work(),
                );
                if keeps_gate_open {
                    fallback.arm_after_no_present(web_time::Instant::now());
                }
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
        let outcome = if let Some(mut lane) = lane_frame.try_lock() {
            pump_with_device_recovery(ui_runtime, &mut *lane, device_recovery_backoff, now)
        } else {
            // A reentrant frame dispatch that slipped past the
            // empty-slot drain protection upstream: skip this nested
            // frame rather than deadlock mid-pump; the outer dispatch
            // still completes its own. The pacing tail below still
            // arms its fallback for it.
            tracing::error!("frame skipped: raster lane already held by an outer frame dispatch");
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
            let deadline = fallback.arm_after_no_present(pace_now);
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
