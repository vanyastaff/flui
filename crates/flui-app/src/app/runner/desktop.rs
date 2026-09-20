use crate::app::hot_reload::{RebuildHookGuard, WorkerReload};
use flui_engine::Renderer;
use flui_platform::traits::{DispatchEventResult, PlatformInput, PlatformWindow};
use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};
use parking_lot::Mutex;
use std::sync::Arc;

use super::device_recovery::{
    DeviceRecoveryBackoff, FrameRecoveryOutcome, render_frame_with_device_recovery,
};
use super::frame_pacing::{
    DEFAULT_DISPLAY_PERIOD, FallbackWake, WakeAction, frame_is_dirty, install_pre_present_hook,
    keeps_frame_gate_open, wake_action,
};
use super::host::{
    APP_RUNTIME, desktop_secondary_wake_deadline, install_wake_deadline_hook, merge_wake_deadlines,
    runtime_needs_redraw_handle, runtime_wake_callback,
};
use super::realm_dispatch::{
    PlatformToUi, RealmDispatcher, RealmTask, close_this_window, dispatch_platform_realm,
    drain_owner_inbox, install_realm_alongside, install_surface_applier,
};
use crate::app::AppConfig;

pub(super) struct RenderedMain {
    pub(super) window: Arc<dyn PlatformWindow>,
    pub(super) address: flui_foundation::PresentationAddress,
    pub(super) _rebuild_registration: RebuildHookGuard,
}

struct InstallRollback {
    window: Arc<dyn PlatformWindow>,
    dispatcher: Option<RealmDispatcher>,
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

pub(super) fn install_desktop_window<V>(
    root: V,
    config: &AppConfig,
    worker_reload: WorkerReload,
    window: Arc<dyn PlatformWindow>,
    host_lifecycle: AppLifecycleState,
) -> Result<RenderedMain, crate::app::AppWindowError>
where
    V: View + Clone + 'static,
{
    let mut rollback = InstallRollback {
        window: Arc::clone(&window),
        dispatcher: None,
        committed: false,
    };
    // 0c. This window's device-recovery backoff, constructed here (not
    // down at step 6 alongside the renderer it paces) so the
    // wake-deadline hook below can be wired to it from the start — one
    // instance for the whole closure's life, `Arc`'d because a fresh
    // clone is threaded into each `RealmTask::Frame` the frame closure
    // builds while the underlying counters/deadline must persist. See
    // `DeviceRecoveryBackoff`'s own doc.
    let device_recovery_backoff = Arc::new(DeviceRecoveryBackoff::new());

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
    renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);
    // The platform's frame-pacing signal (Wayland frame callbacks) is
    // armed by the renderer right before each present — see
    // `install_pre_present_hook`'s own doc and ADR-0058.
    install_pre_present_hook(&mut renderer, &window);

    // 3. Mount root widget at the LOGICAL size; the framework lays out
    // in logical pixels and the paint root's DPR transform maps to the
    // physical surface. `UiRealm::new` applies the DPR to the freshly
    // built pipeline before returning, so the RenderView configuration
    // and the first frame agree on the scale from construction.
    let scale_factor = window.scale_factor() as f32;
    let wake = runtime_wake_callback();
    let ui_realm = match crate::app::ui_realm::UiRealm::new(
        Arc::clone(&wake),
        Arc::clone(&window),
        scale_factor,
        runtime_needs_redraw_handle(),
    ) {
        Ok(realm) => realm,
        Err(e) => {
            tracing::error!(error = %e, "UiRealm construction failed");
            return Err(crate::app::AppWindowError::Mount {
                source: Arc::new(e),
            });
        }
    };

    // Debug overlay: `Some` stats IS the enable flag, so this is the
    // single point that turns the frame path's overlay work on.
    ui_realm.set_performance_overlay(config.show_performance_overlay);

    // Typed frame-failure route (issue #561): failures are contained
    // per presentation either way; this only adds the embedder's
    // delivery.
    ui_realm.set_frame_failure_handler(config.frame_failure_handler.clone());
    ui_realm.set_frame_failure_detail(config.frame_failure_detail);

    ui_realm.enter(|realm| realm.update_host_lifecycle(host_lifecycle));
    let logical = window.logical_size();
    let attach = ui_realm.enter(|realm| {
        realm.attach_root_widget_with_size(&root, logical.width.0, logical.height.0)
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return Err(crate::app::AppWindowError::Mount {
            source: Arc::new(e),
        });
    }

    // 3b. Wire the wake chain (E0a).
    //
    // `on_need_frame` fires whenever `handle_build_scheduled` determines a new
    // frame is required (e.g. after setState).  The closure calls `wake`
    // which sets `needs_redraw` atomically AND calls `PlatformWindow::
    // request_redraw()` so the winit event loop wakes from idle.
    //
    // Deadlock analysis:
    // * `wake` acquires only the loop-scoped redraw-window leaf Mutex.
    // * The closure is called from `handle_build_scheduled`, which holds no
    //   `inner`/`widgets` lock (see `WidgetsBinding::handle_build_scheduled`
    //   doc).
    // * `on_need_frame` itself is a separate `RwLock` on `WidgetsBinding`,
    //   never held across any `inner` critical section.
    // Therefore: no lock ordering conflict.
    {
        let widgets = ui_realm.widgets();
        let wake = Arc::clone(&wake);
        widgets.set_on_need_frame(move || wake());
    }

    // Wire `on_build_scheduled` on the BuildOwner so a dirty-element
    // registration (e.g. from setState inside an element build) wakes the
    // platform loop. The callback fires from inside `schedule_build_for`,
    // which runs during a build while the realm's `widgets` write lock is
    // held — so it must NOT re-lock `widgets`. It calls `wake`
    // directly (the same effect as the `on_need_frame` callback above),
    // which touches only the loop-scoped redraw-window leaf lock. The
    // callback must not re-enter widget state while `BuildOwner` is
    // scheduling; realm entry is reserved for the outer event/frame
    // dispatch boundary.
    {
        let widgets = ui_realm.widgets();
        widgets.with_build_owner_mut(|build_owner| {
            let wake = Arc::clone(&wake);
            build_owner.set_on_build_scheduled(move || wake());
        });
    }

    // 3c. Construct the per-window owner and its bounded command inbox.
    // The wake is the existing chain: `wake_frame` sets
    // `needs_redraw` and queues a `RedrawRequested`, so a command sent to an
    // idle loop produces the frame whose drain observes it.
    //
    tracing::info!(
        { flui_foundation::diagnostics::PRESENTATION_ID } = ui_realm.presentation_id().as_u64(),
        inbox_capacity = ui_realm.command_sender().capacity(),
        "UiRealm constructed"
    );
    let hot_reload_sender = ui_realm.command_sender();
    let realm_dispatch = install_realm_alongside(ui_realm, &window).map_err(|error| {
        crate::app::AppWindowError::Mount {
            source: Arc::new(error),
        }
    })?;
    rollback.dispatcher = Some(realm_dispatch);

    // 3c1. Wire this presentation into the close-request seam (issue
    // #558): the application's own answer to "may this window close?",
    // plus the entry that makes the window closable programmatically
    // afterwards. One shared implementation with
    // `open_secondary_window`'s window — see that function's own doc.
    crate::app::runner::install_close_request_wiring(
        realm_dispatch.address,
        &window,
        config.close_request_handler.clone(),
    );
    let rebuild_registration = worker_reload.register_rebuild_hook(hot_reload_sender);

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

    // 3d. Wire the wall-clock-wake hook, now that `realm_dispatch`
    // exists — the winit backend's `about_to_wait` consults this every
    // idle iteration instead of blocking forever, so a pending gesture-
    // arena deadline (a long-press hold, a double-tap give-up) or an
    // armed device-recovery retry still wakes the loop at the right
    // instant even while nothing else is dirty and no animation is
    // running. Moved here from directly after step 0c (this window's
    // `device_recovery_backoff` construction) specifically so this
    // closure can capture `realm_dispatch.address.realm_id` and look up
    // THIS realm's `frames_enabled` state each time it runs — see the
    // closure body's own comment for why that lookup, not just
    // `next_attempt_at()`, is required.
    install_wake_deadline_hook({
        let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
        let fallback = Arc::clone(&fallback);
        let realm_id = realm_dispatch.address.realm_id;
        move || {
            // The frames-enabled gate itself is `desktop_secondary_
            // wake_deadline` — a pure, unit-tested function (see its own
            // doc for why an unconditional report here would reproduce
            // `WinitApp::new_events`'s named busy-spin one layer up).
            // Only the `frames_enabled` LOOKUP is inline here, since it
            // needs live `APP_RUNTIME` state no pure function can carry.
            let frames_enabled = APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .realms
                    .get(&realm_id)
                    .and_then(|realm_slot| realm_slot.realm.as_ref())
                    .is_some_and(|realm| realm.scheduler().frames_enabled())
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
                device_recovery_backoff.next_attempt_at(),
                fallback.next_wake(web_time::Instant::now()),
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
        realm_dispatch.address,
        phys_size.width.0 as u32,
        phys_size.height.0 as u32,
    )));

    // Install the registration-lifetime surface applier alongside the
    // realm (cleared together at teardown): a `Resized` event takes it
    // out of the TLS slot, calls it, and restores it (see
    // `PlatformToUi::run`'s `Resized` arm) rather than capturing the
    // lane inside the event payload itself. The hook mints the frame
    // stamp's next `SurfaceGeneration` and records the platform's new
    // size as layout's authority; the backend surface itself is
    // reconfigured by the lane's next pump, before the next render.
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
        let _ =
            dispatch_platform_realm(realm_dispatch, RealmTask::Event(PlatformToUi::Input(input)));
        DispatchEventResult::resolved(false, true)
    }));

    // 6. Register frame callback -> scheduler + UiRealm::render_frame_on_lane()
    let lane_frame = Arc::clone(&lane);
    let worker_reload_frame = worker_reload.clone();
    // Reuses the SAME backoff constructed at step 0c (already wired
    // into the wake-deadline hook above) — not a fresh one.
    let frame_fallback = Arc::clone(&fallback);
    window.on_request_frame(Box::new(move || {
        let lane_frame = Arc::clone(&lane_frame);
        let worker_reload_frame = worker_reload_frame.clone();
        let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
        let fallback = Arc::clone(&frame_fallback);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Frame(Box::new(move |realm| {
                worker_reload_frame.poll_and_apply(realm);

                let scheduler = realm.scheduler();

                // Every fire of this callback is a genuine platform-delivered
                // frame-request signal on this backend (`WinitWindowEvent::
                // RedrawRequested` -> `dispatch_request_frame` -> here; see
                // `docs/adr/ADR-0044-driver-loop-hybrid.md`'s per-platform table for which backends
                // pace this via the compositor vs. deliver it immediately).
                // Recorded unconditionally, before the dirty/wake_action gate below
                // decides whether anything actually runs this pump: pacing
                // feedback is about observing the PLATFORM's own delivery timing,
                // independent of whether this particular delivery ends up idle.
                // `now` is read ONCE here and reused ~60 lines below at this
                // closure's `drive_frame_with_lane(now, ...)` call, past the
                // `wake_action` match below (that later call site's own comment
                // points back to this one) — this pump's pacing-feedback sample
                // and its own frame-drive instant must agree, the same
                // single-`now`-per-pump discipline every other call site in this
                // closure already follows.
                let now = web_time::Instant::now();
                realm.record_compositor_tick(now);

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
                let inbox_redraw = drain_owner_inbox(realm);

                // `device_recovery_backoff.next_attempt_at().is_some()` is a
                // REQUIRED fourth dirty source, not an optional extra: a
                // deadline wired into the wake-deadline hook (installed
                // below, after this realm exists) but absent from THIS
                // predicate reaches `WakeAction::Skip` and returns before
                // `render_frame_with_device_recovery` is ever called, no
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
                    realm.needs_redraw(),
                    realm.has_pending_work(),
                    device_recovery_backoff.next_attempt_at(),
                    fallback_gate,
                );
                match wake_action(
                    scheduler.frames_enabled(),
                    dirty,
                    scheduler.is_frame_scheduled(),
                    fallback_gate,
                ) {
                    WakeAction::Skip => return,
                    WakeAction::PumpAsync => {
                        // Frames disabled (Hidden/Paused/Detached): the mid-frame
                        // `drive_async_tasks` poll inside `handle_begin_frame`
                        // never runs because no frame runs at all — this
                        // explicit call is the ONLY thing keeping a spawned
                        // future progressing while backgrounded. No begin/draw
                        // frame, no tickers, no pipeline, no present.
                        //
                        // `finish_async_pump` MUST run first, not after: nothing
                        // else ever clears the scheduler's `frame_scheduled`
                        // latch on this path (only `handle_begin_frame` does,
                        // and it never runs here), so without this call a LATER,
                        // independent wake (a network response's `Waker::wake`,
                        // arriving after this pump cycle returns) would find the
                        // latch already set, never re-fire `on_frame_scheduled`,
                        // and never wake this loop again — see
                        // `UpdateScheduler::finish_async_pump`'s doc for the full
                        // starvation hazard and why the ordering matters.
                        scheduler.finish_async_pump();
                        scheduler.drive_async_tasks();
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
                            realm.needs_redraw(),
                            scheduler.is_frame_scheduled(),
                            realm.has_pending_work(),
                        );
                        if keeps_gate_open {
                            fallback.arm_after_no_present(web_time::Instant::now());
                        }
                        return;
                    }
                    WakeAction::Render => {}
                }

                // The `now` used below is the SAME instant bound near the top of
                // this closure and already recorded into `record_compositor_tick`
                // there (see the comment at that earlier `let now` binding) --
                // reused here, not re-read fresh.
                // UpdateScheduler callbacks (animations). NOTE: the global `UpdateScheduler` is driven
                // off this per-frame `Instant::now()`, while the tree-bound `Vsync`
                // (`UiRealm::draw_frame`) ticks off the realm's own `start` origin —
                // two separate clocks ON PURPOSE: the controller sets are disjoint (implicit
                // animations register with `Vsync`; plain controllers carry a private
                // `UpdateScheduler` ticker, never the global one), so the origins never need to
                // agree and no controller is advanced twice.
                // The ONE shared frame ordering — begin (transient +
                // microtasks + the single async-driver poll) -> persistent callbacks ->
                // the pipeline below -> post-frame callbacks -> Idle. `HeadlessBinding`
                // drives the same helper on its binding-local scheduler.
                let outcome = scheduler.drive_frame_with_lane(
                    now,
                    flui_scheduler::IdleDeadline::far_future(now),
                    || {
                        // Render frame via the realm, rebuilding a lost GPU device
                        // around it: BEFORE the frame build when the loss predates
                        // the frame (a dead device never pays extra for it — the
                        // frame builds anyway, see this function's own doc for why),
                        // and AFTER when the wgpu device-lost callback fired
                        // mid-frame — see `render_frame_with_device_recovery`.
                        let Some(mut lane) = lane_frame.try_lock() else {
                            // A reentrant frame dispatch that slipped past the
                            // empty-slot drain protection upstream: skip this
                            // nested frame rather than deadlock mid-pump; the
                            // outer dispatch still completes its own.
                            tracing::error!(
                                "frame skipped: raster lane already held by an \
                             outer frame dispatch"
                            );
                            return FrameRecoveryOutcome {
                                presented: false,
                                just_failed: false,
                                next_attempt_at: None,
                            };
                        };
                        render_frame_with_device_recovery(
                            realm,
                            &mut *lane,
                            &device_recovery_backoff,
                            now,
                        )
                    },
                    realm.local_post_frame_lane(),
                );

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
                    realm.needs_redraw(),
                    scheduler.is_frame_scheduled(),
                    realm.has_pending_work(),
                );
                let pace_now = web_time::Instant::now();
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
            })),
        );
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
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Resized { size, scale_factor }),
        );
    }));

    // 8. Lifecycle callbacks
    //
    // Detached is dispatched through the realm because shutdown must
    // cancel any pointer sequence whose platform Up/Cancel will never
    // arrive before lifecycle observers run.

    // Explicit quit also reaches realms whose windows never closed. The
    // loop-owned callback visits surviving realms rather than capturing
    // this primary dispatch address, which may already be uninstalled.

    // Window close -> close THIS window's own presentation before the
    // platform decides whether to exit. Load-bearing ordering, not just
    // bookkeeping hygiene: the winit backend's `CloseRequested` handling
    // calls a window's `on_close` (this callback) BEFORE it consults the
    // exit-policy hook `install_exit_policy_hook` installs above
    // (`AppRuntime::should_exit`, which decides purely from
    // `AppRuntime`'s own realm registry, never this backend's native
    // window count). Without this call, closing this app's only window
    // would leave the realm registry non-empty forever — nothing else
    // ever removes it — and the exit-policy hook would report "don't
    // exit" on every subsequent window close, including the very last
    // one, silently hanging the app open with no window left at all.
    // `close_this_window` (not `uninstall_platform_realm` directly): the
    // primary window closing while a `WindowPolicy::SharedRealm` sibling
    // survives must remove only THIS presentation, never the whole
    // realm out from under that sibling; `close_this_window` reduces to
    // the same full-realm-uninstall effect exactly when this is the
    // realm's sole presentation (today's single-window desktop shape).
    // The tail `teardown_platform_realm()` call still runs once
    // `Platform::run` actually returns, for the clipboard/redraw-window
    // cleanup no per-window close performs.
    let closing_window_id = window.id();
    window.on_close(Box::new(move || {
        tracing::info!("Window closed");
        super::main_window::main_window_closing(realm_dispatch.address);
        let closed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            close_this_window(realm_dispatch);
        }));
        // Release the redraw-poke slot's pin on this window NOW, while
        // the platform event loop is still alive — this slot was the
        // one `Arc` that survived `Platform::run`, deferring the
        // window's native teardown (and, before the platform's own
        // callback clear existed, the GPU surface teardown chained
        // behind it) to after the loop was gone: the Wayland post-quit
        // SIGSEGV of issue #713. Keyed by id, not unconditional, so
        // closing a `SharedRealm` sibling never unpins the primary.
        // Dropped outside the TLS borrow: the winit window's own drop
        // may re-enter platform code.
        let released =
            APP_RUNTIME.with(|slot| slot.borrow().release_redraw_window_for(closing_window_id));
        drop(released);
        super::main_window::main_window_closed(realm_dispatch.address);
        if let Err(payload) = closed {
            std::panic::resume_unwind(payload);
        }
    }));

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
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowFocus(focused)),
        );
    }));
    window.on_execution_state_change(Box::new(move |state| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowExecution(state)),
        );
    }));
    window.on_visibility_status_change(Box::new(move |visible| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowVisibility(visible)),
        );
    }));
    let execution = window.execution_state();
    let focused = window.is_focused();
    let visible = window.is_visible();
    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmTask::Frame(Box::new(move |realm| {
            realm.synchronize_window_snapshot(
                realm_dispatch.address.presentation_id,
                execution,
                focused,
                visible,
            );
        })),
    );

    window.on_hover_status_change(Box::new(move |is_hovered| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowHover(is_hovered)),
        );
    }));
    // The platform callback carries no payload; query the window's
    // current appearance at dispatch time. Weak: the callback lives
    // inside the window's own handler table, and a strong capture
    // would cycle it alive past close.
    let appearance_window = Arc::downgrade(&window);
    window.on_appearance_changed(Box::new(move || {
        if let Some(win) = appearance_window.upgrade() {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::AppearanceChanged(win.appearance())),
            );
        }
    }));
    // Seed the initial brightness — a user on a dark desktop must not
    // start light until the first live theme flip.
    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmTask::Event(PlatformToUi::AppearanceChanged(window.appearance())),
    );
    // Seed the initial size and device-pixel ratio the same way: the
    // source must not sit on defaults until the first live resize —
    // on the web backend no resize observer exists, so a default
    // would be permanent there.
    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmTask::Event(PlatformToUi::Resized {
            size: window.logical_size(),
            scale_factor: window.scale_factor() as f32,
        }),
    );

    // 9. Store the window in AppRuntime's redraw-poke slot — BEFORE
    // marking the lifecycle Resumed or requesting the initial redraw.
    // Both of those can synchronously run the first frame through
    // `dispatch_platform_realm`; if the slot were still empty at that
    // point, anything resolving it during that frame would silently
    // no-op instead of waking the loop.
    APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(Arc::clone(&window)));

    // Start the host through the shared lifecycle dispatch. Each
    // presentation derives its initial state from observed native facts,
    // so a hidden or unfocused window starts Hidden or Inactive even
    // though the host itself is running.
    debug_assert_eq!(
        std::thread::current().id(),
        realm_dispatch.owner_thread,
        "desktop bootstrap must run on the realm's owner thread"
    );
    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmTask::Event(PlatformToUi::Lifecycle(host_lifecycle)),
    );

    // 10. Request initial redraw, now that the window is stored.
    // `wake` (not a direct `request_redraw()` on the window): it clones
    // the window out from under the redraw-poke slot's lock before
    // calling through, so a backend whose `request_redraw` re-enters
    // this runtime synchronously (headless, in this crate's own tests)
    // cannot deadlock on that same lock — the same clone-then-call
    // discipline used by direct platform capabilities.
    wake();

    super::host::with_owner_platform(|owner| owner.activate(false));
    tracing::info!("Desktop platform initialized with callbacks");
    rollback.committed = true;
    Ok(RenderedMain {
        window,
        address: realm_dispatch.address,
        _rebuild_registration: rebuild_registration,
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
