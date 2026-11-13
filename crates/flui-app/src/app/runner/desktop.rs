use crate::app::hot_reload::WorkerReload;
use flui_native::HostWindow;
use flui_platform::PlatformWindow;
use flui_runtime::pump::SampledClock;
use flui_scheduler::AppLifecycleState;
use flui_view::View;
use flui_widgets::app_root;
use std::rc::Rc;
use std::sync::Arc;

use super::first_reveal::FirstReveal;
use super::frame_pacing::{
    FallbackWake, WakeAction, frame_is_dirty, keeps_frame_gate_open, wake_action,
};
use super::host::{APP_RUNTIME, desktop_secondary_wake_deadline, merge_wake_deadlines};
use super::owner_dispatch::{
    PresentationDispatcher, RuntimeTask, close_this_window, dispatch_platform_ui_runtime,
    install_input_wiring, prepare_ui_runtime_alongside,
};
use super::raster_host::{RasterSlot, device_retry_at};
use crate::app::AppConfig;
use flui_engine::SlotVisibility;

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
        .reveal(flui_platform::WindowReveal::AfterFirstFrame);
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
    // The reveal deadline belongs to this presentation. The native frame
    // registry retains its metadata even while its driver is leased.

    // 0d. The window's deferred first reveal (see `FirstReveal`): the
    // platform opened it hidden-but-intended-visible, and the frame closure
    // below hands it the reveal on the first presented frame, or at the
    // policy's fallback bound. Its deadline joins the wake hook and the
    // dirty predicate like every other wake-deadline source here.
    let first_reveal = Arc::new(FirstReveal::new());

    let geometry = super::window_install::SurfaceGeometry::of(window.as_ref());
    let phys_size = geometry.physical;

    // 3. Mount root widget at the LOGICAL size; the framework lays out
    // in logical pixels and the paint root's DPR transform maps to the
    // physical surface. `UiRuntime::try_new` applies the DPR to the freshly
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
    config.install_frame_failure_handler(&ui_runtime);
    ui_runtime.set_frame_failure_detail(config.frame_failure_detail);
    config.install_input_tap(&ui_runtime);
    config.install_error_view_builder(&ui_runtime);

    ui_runtime.enter(|ui_runtime| ui_runtime.update_host_lifecycle(host_lifecycle));
    let logical = geometry.logical;
    let attach = ui_runtime.enter(|ui_runtime| {
        ui_runtime.attach_root_widget_with_size(
            &app_root(root.clone()),
            logical.width,
            logical.height,
        )
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return Err(crate::app::AppWindowError::Mount {
            source: Arc::new(e),
        });
    }
    worker_reload.register_ui_runtime(&ui_runtime);
    // The application's request to rebuild a given-up GPU device
    // (`AppHandle::retry_render_device`, ADR-0267) asks this window for a
    // frame, which acts on it.
    let device_retry = APP_RUNTIME.with(|slot| {
        slot.borrow()
            .main_ingress
            .as_ref()
            .map(|ingress| ingress.device_retry().watch(&ui_runtime.wake_handle()))
    });
    // The development agent's window is vended while the ui_runtime is still
    // ours: if the installation below fails, the ui_runtime drops it, and the
    // hooks are handed only a window that finished installing, whatever
    // configuration the application opened it with.
    let agent_window = super::dev_agents::vend(&ui_runtime, ui_runtime.presentation_id());

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

    // 3d. The window's wall-clock wake sources: the owner loop consults them
    // every idle iteration instead of blocking forever, so the pacing
    // fallback, the first reveal and the executor's next device rebuild
    // still wake the loop while nothing else is dirty. Read only while this
    // presentation's frames are enabled.
    let deadlines = Arc::new(DesktopWakeDeadlines {
        fallback: Arc::clone(&fallback),
        first_reveal: Arc::clone(&first_reveal),
    });

    // 4. The window's slot on the loop's raster executor (ADR-0305): the
    // first window to attach places the executor on its surface, which
    // picks the adapter; every later window is a slot on the same device.
    // The slot holds the window's surface target, which the raster side may
    // hold on any thread, never the window itself (ADR-0254 §5). It is
    // unconfigured until its first frame measures the window.
    let host = APP_RUNTIME.with(|slot| slot.borrow().raster_host.clone());
    let display = super::host::display_color_of(window.as_ref());
    let attached = host
        .try_with(|host| {
            host.attach(
                window.as_ref(),
                owner_dispatch.address,
                config,
                textures,
                display,
            )
        })
        // A window opened from code the host's turn runs (a scene's `Drop`,
        // a tracing subscriber) finds it busy: refused, as a GPU that gives
        // no surface is.
        .unwrap_or_else(|| Err(super::raster_host::host_busy()));
    if let Err(error) = attached {
        tracing::error!(%error, "GPU init failed");
        return Err(crate::app::AppWindowError::Renderer {
            source: Arc::new(error),
        });
    }
    let raster = RasterSlot {
        host,
        address: owner_dispatch.address,
        size: (phys_size.width as u32, phys_size.height as u32),
        device: flui_engine::DeviceHealth::Healthy,
    };

    // 5. Register input callback -> entered ui_runtime input dispatch
    install_input_wiring(owner_dispatch, window.as_ref());

    // Install frame resources once; native deliveries carry only their binding.
    installation.dev_agent(agent_window);
    let frame = installation
        .frame_driver(super::frame_driver::FrameDriver::Desktop(
            DesktopFrameDriver {
                raster,
                worker_reload,
                deadlines,
                reveal_window: Rc::downgrade(&window),
                device_retry,
                retry_pending: false,
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
    // bookkeeping hygiene: a native host's close handling
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
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the window-close callback's panic is held; the pin still releases"
        )]
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
        // Dropped outside the TLS borrow: the native window's own drop
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

pub(super) struct DesktopFrameDriver {
    pub(super) deadlines: Arc<DesktopWakeDeadlines>,
    raster: RasterSlot,
    worker_reload: WorkerReload,
    reveal_window: std::rc::Weak<dyn PlatformWindow>,
    device_retry: Option<crate::app::application_control::DeviceRetryWatch>,
    /// The application asked for a rebuild of a given-up device while the
    /// host was turning; sent on the next wake.
    retry_pending: bool,
}

/// Presentation-owned pacing sources, retained even while its driver is leased.
pub(super) struct DesktopWakeDeadlines {
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

    /// The earliest armed deadline: the device's next rebuild, the pacing
    /// fallback and the first reveal. `period` is the presentation's current
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
                device_retry_at(),
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
        self.retry_pending |= self
            .device_retry
            .as_mut()
            .is_some_and(crate::app::application_control::DeviceRetryWatch::take_request);
        let retry_requested = self.retry_pending;
        let worker_reload_frame = &self.worker_reload;
        let fallback = &self.deadlines.fallback;
        let first_reveal = &self.deadlines.first_reveal;
        let reveal_window = &self.reveal_window;
        // The gate half of the wake runs inside one ui_runtime entry and
        // decides; the pump below enters the ui_runtime itself.
        let (action, now, display_color) = ui_runtime.enter(|ui_runtime| {
            worker_reload_frame.poll_and_apply(ui_runtime);

            let scheduler = ui_runtime.scheduler();

            // Every fire of this callback is a platform-delivered frame
            // request (`docs/adr/ADR-0044-driver-loop-hybrid.md`). Recorded
            // before the gate below decides whether anything runs: pacing
            // feedback observes the platform's delivery timing. `now` is read
            // once and is also the pump's frame timestamp, so the sample and
            // the frame agree.
            let now = web_time::Instant::now();
            ui_runtime.record_compositor_tick(now);

            // Owner-inbox drain at the frame boundary, before the dirty gate,
            // so a command-driven redraw is observed by the frame its wake
            // produced. Drained closures never run under the slot's borrow.
            let inbox_redraw = ui_runtime.drain_owner_inbox();
            // A requested rebuild takes a given-up device out of its held
            // state, which asks for the frame the executor rebuilds it in.
            if retry_requested {
                ui_runtime.retry_render_device();
            }

            // The device's next rebuild is a dirty source as well as a wake
            // deadline: a deadline the hook wakes the loop for but this
            // predicate ignores would reach `WakeAction::Skip` and never run
            // the turn that rebuilds. The pacing deferral is read once: `gate`
            // consumes a due deadline.
            let fallback_gate = fallback.gate(now);
            let dirty = frame_is_dirty(
                inbox_redraw,
                ui_runtime.needs_redraw(),
                ui_runtime.has_pending_work(),
                merge_wake_deadlines(device_retry_at(), first_reveal.next_deadline()),
                fallback_gate,
            );
            let action = wake_action(
                scheduler.frames_enabled(),
                dirty,
                scheduler.is_frame_scheduled(),
                fallback_gate,
            );
            (
                action,
                now,
                ui_runtime
                    .window_display(presentation)
                    .and_then(|display| display.color),
            )
        });

        // A window whose frames are off cannot be seen: its slot is not
        // drawn, and draws in full when frames come back (ADR-0305).
        if action != WakeAction::Skip {
            let visibility = if action == WakeAction::Render {
                SlotVisibility::Shown
            } else {
                SlotVisibility::Hidden
            };
            let address = self.raster.address;
            // A host already turning is told on the next wake.
            let _told = self
                .raster
                .host
                .try_with(|host| host.set_visibility(address, visibility));
        }
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

        // Before the frame: the application's rebuild request, a changed
        // display colour profile (HDR switched, a move to another monitor,
        // ADR-0256 §6), and a turn of the executor, which rebuilds a device
        // that is lost and due even when this frame has nothing new to draw.
        // A device that came back repaints the window in full, so this frame
        // submits a fresh scene.
        let address = self.raster.address;
        let retry_pending = self.retry_pending;
        let turned = self.raster.host.try_with(|host| {
            if retry_pending {
                host.retry_device();
            }
            let changed = host.follow_display(address, display_color);
            host.service();
            changed
        });
        // A host already turning is asked again on the next wake.
        if turned.is_some() {
            self.retry_pending = false;
        }
        let display_changed = turned.unwrap_or(false);
        if display_changed | self.raster.settle_device(ui_runtime) {
            ui_runtime.mark_primary_needs_full_repaint();
        }

        // The frame: `UiRuntime::pump` at `now` (apply commands -> begin ->
        // persistent callbacks -> the pipeline and its submit through the
        // window's slot -> post-frame callbacks -> Idle).
        let mut sink = self.raster.frame();
        let presented = ui_runtime
            .pump(&mut SampledClock(now), &mut sink)
            .presented();
        // A panic of the frame's GPU work, held by the host, reaches the
        // application's frame-failure handler.
        self.raster.report_frame_panic(ui_runtime);
        // A device the frame itself lost is reported with the frames it
        // drops; its rebuild is due at the executor's deadline, which the
        // wake hook and the dirty predicate above both read.
        let _ = self.raster.settle_device(ui_runtime);

        // Frame-pacing fallback (ADR-0058). A frame that reached `present()`
        // needs no bound: the present blocked at vsync or the compositor is
        // pacing the redraws. A frame that did not present got neither, so
        // with a ticker still asking for frames nothing would pace the loop;
        // the deferral bounds it to one pipeline pass per display period, as
        // a wake deadline rather than a sleep.
        let keeps_gate_open = keeps_frame_gate_open(
            ui_runtime.needs_redraw(),
            ui_runtime.scheduler().is_frame_scheduled(),
            ui_runtime.has_pending_work(),
        );
        let pace_now = web_time::Instant::now();
        // The deferred first reveal: handed to the window on the first
        // presented frame, or at the fallback bound after a frame that ran
        // and presented nothing (`FirstReveal`).
        if first_reveal.after_frame(presented, pace_now)
            && let Some(window) = reveal_window.upgrade()
        {
            window.reveal_after_first_frame();
        }
        if presented {
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
