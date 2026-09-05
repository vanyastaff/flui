use flui_scheduler::AppLifecycleState;
use flui_view::{StatelessView, View};

use super::device_recovery::{
    DeviceRecoveryBackoff, FrameRecoveryOutcome, render_frame_with_device_recovery,
};
use super::frame_pacing::{
    WakeAction, frame_is_dirty, keeps_frame_gate_open, no_present_fallback_pace, wake_action,
};
use super::host::{
    APP_RUNTIME, OwnerHostClearGuard, desktop_secondary_wake_deadline, install_exit_policy_hook,
    install_owner_platform, install_wake_deadline_hook, runtime_needs_redraw_handle,
    runtime_wake_callback, with_owner_platform,
};
use super::realm_dispatch::{
    PlatformToUi, RealmTask, close_this_window, dispatch_platform_realm, drain_owner_inbox,
    install_platform_realm, install_surface_applier, teardown_platform_realm,
};
use crate::app::AppConfig;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn run_desktop<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    use flui_engine::wgpu::Renderer;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    use crate::app::hot_reload::{RebuildHookGuard, WorkerReload};

    tracing::info!("Starting desktop platform via flui-platform");

    // Development reload, if this build has it: with the `hot-reload` feature
    // off this value is inert and `flui-hot-reload` is not in the graph.
    let worker_reload = WorkerReload::from_config(&config);

    // Platform init is an environment failure (missing display server, unsupported
    // OS, driver problem), not a `BUG:` invariant — no `bootstrap_error_slot` exists
    // yet to route this through (that cell, and the `platform` it needs for
    // `quit()`, only exist once `on_ready` is running), so this is the one desktop
    // failure this function still surfaces via `panic!` directly rather than the
    // deferred-teardown path below. It still gets a full error log and the same
    // "desktop bootstrap failed" wording as that deferred path, instead of a bare
    // `.expect()`'s terse, context-free message.
    let platform = match flui_platform::current_platform() {
        Ok(platform) => platform,
        Err(error) => {
            tracing::error!(%error, "Failed to initialize platform");
            panic!("desktop bootstrap failed: platform initialization error: {error:?}");
        }
    };

    // `rebuild_registration`'s `Drop` detaches the hot-reload hook and must
    // stay alive until the event loop exits — but it (like the window and
    // every callback below) can only be created from inside `on_ready`, so
    // it is threaded back out through this cell instead of a plain local.
    let rebuild_registration: Rc<RefCell<Option<RebuildHookGuard>>> = Rc::new(RefCell::new(None));
    let rebuild_registration_slot = Rc::clone(&rebuild_registration);

    /// The actual desktop bootstrap: opens the window, initializes the GPU
    /// renderer, mounts the widget tree, and wires every platform/window
    /// callback. Runs exactly once, synchronously, inside `on_ready` (see
    /// `Platform::run`'s doc) — never before, since the winit backend can
    /// only create a window from inside a running event loop
    /// (`ActiveEventLoop` is unreachable beforehand, and `open_window` fails
    /// fast rather than deadlock if called too early).
    ///
    /// Returns `Err` on any bootstrap failure (GPU init, `UiRealm`
    /// construction, root widget attach); `on_ready` itself is now fallible,
    /// so this propagates straight out of `Platform::run` instead of
    /// threading the failure out through a
    /// `Rc<RefCell<Option<anyhow::Error>>>` side channel — that pattern is
    /// now redundant here and has been removed. Every backend stops
    /// entering (or promptly exits) its loop on this `Err`, so there is no
    /// need to call `owner.quit()` explicitly on any of the error paths
    /// below.
    ///
    /// Pulled out of the `on_ready` closure into a named fn so rustfmt
    /// actually formats it — rustfmt does not reliably reformat very large
    /// closure literals passed as call arguments.
    fn bootstrap_desktop<V>(
        root: V,
        config: AppConfig,
        worker_reload: WorkerReload,
        rebuild_registration_slot: Rc<RefCell<Option<RebuildHookGuard>>>,
    ) -> anyhow::Result<()>
    where
        V: View + StatelessView + Clone + 'static,
    {
        tracing::info!("Platform ready");

        // No `owner: OwnerPlatform` parameter: the caller already installed
        // it in the loop-scoped host (ADR-0039 §6) before calling this
        // function, so every owner-thread touch below re-crosses the fenced
        // `with_owner_platform` accessor instead of holding a private copy
        // (`OwnerPlatform` isn't `Clone` by design — there is exactly one
        // instance, and the TLS host is its one sanctioned home for the
        // rest of the loop's life).
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
            with_owner_platform(f)
                .expect("BUG: bootstrap_desktop runs only after install_owner_platform")
        }

        // 0. Wire the platform clipboard (ADR-0034) before anything else can
        // observe `AppRuntime::clipboard()`.
        let clipboard = owner_platform_installed(|owner| owner.shared().clipboard());
        APP_RUNTIME.with(|slot| slot.borrow().set_platform_clipboard(clipboard));

        // 0b. Wire the exit-policy hook (issue #555's native-lifecycle
        // wiring): the winit backend's `CloseRequested` handling consults
        // this instead of deciding from its own native window count alone,
        // so a queued "open the main window" install (this same thread's
        // `AppRuntime`, invisible to the platform layer) can veto an exit
        // the backend would otherwise take unconditionally.
        install_exit_policy_hook(config.exit_policy);

        // 0b2. Stash host-injected executors (issue #557) BEFORE the realm
        // install below resolves the loop's execution services — the order
        // that makes `ensure_execution` route background work to the host's
        // pools instead of constructing the default ones.
        if let Some(host) = config.executors.clone() {
            APP_RUNTIME.with(|slot| slot.borrow_mut().install_host_executors(host));
        }

        // 0c. This window's device-recovery backoff, constructed here (not
        // down at step 6 alongside the renderer it paces) so the
        // wake-deadline hook below can be wired to it from the start — one
        // instance for the whole closure's life, `Arc`'d because a fresh
        // clone is threaded into each `RealmTask::Frame` the frame closure
        // builds while the underlying counters/deadline must persist. See
        // `DeviceRecoveryBackoff`'s own doc.
        let device_recovery_backoff = Arc::new(DeviceRecoveryBackoff::new());

        // 1. Open window now that the event loop is running. Window creation is
        // an environment failure (display server hiccup, resource exhaustion),
        // not a `BUG:` invariant, and — unlike platform init above — this DOES
        // run inside `on_ready` with a live owner capability, so a failure here
        // gets the same `Err`-propagates-out-of-`run` handling as the
        // GPU/realm/attach failures below instead of an immediate bare
        // `.expect()` panic mid-`on_ready`. `Ready` is guaranteed here
        // (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let window = match owner_platform_installed(|owner| owner.open_window(options))
            .and_then(flui_platform::WindowOpen::try_ready)
        {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Window creation failed");
                return Err(anyhow::Error::from(error).context("Window creation failed"));
            }
        };

        // 2. Create GPU renderer directly (no DesktopEmbedder)
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
                return Err(anyhow::anyhow!(e).context("UiRealm construction failed"));
            }
        };

        // Debug overlay: `Some` stats IS the enable flag, so this is the
        // single point that turns the frame path's overlay work on.
        ui_realm.set_performance_overlay(config.show_performance_overlay);

        // Typed frame-failure route (issue #561): failures are contained
        // per presentation either way; this only adds the embedder's
        // delivery.
        ui_realm.set_frame_failure_handler(config.frame_failure_handler.clone());

        let logical = window.logical_size();
        let attach = ui_realm.enter(|realm| {
            realm.attach_root_widget_with_size(&root, logical.width.0, logical.height.0)
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
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
        let realm_dispatch = install_platform_realm(ui_realm, &window);

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
        *rebuild_registration_slot.borrow_mut() =
            Some(worker_reload.register_rebuild_hook(hot_reload_sender));

        // 3c2. Start config-declared application services (issue #558) now
        // that the realm install above has resolved the loop's execution
        // services. Started here — not before the install — so a service's
        // spawned tasks land on the same pools (host-injected or default)
        // the rest of the loop uses. A start failure is a bootstrap
        // failure: a declared service is a load-bearing part of the
        // application, not an optional extra to drop silently.
        for service in &config.services {
            if let Err(error) = APP_RUNTIME.with(|slot| slot.borrow_mut().start_service(service)) {
                tracing::error!(service = service.name(), %error, "service start failed");
                return Err(anyhow::Error::from(error).context(format!(
                    "failed to start application service `{}`",
                    service.name()
                )));
            }
        }

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
                desktop_secondary_wake_deadline(
                    device_recovery_backoff.next_attempt_at(),
                    frames_enabled,
                )
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
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Input(input)),
            );
            DispatchEventResult::resolved(false, true)
        }));

        // 6. Register frame callback -> scheduler + UiRealm::render_frame_on_lane()
        let lane_frame = Arc::clone(&lane);
        let worker_reload_frame = worker_reload.clone();
        // Reuses the SAME backoff constructed at step 0c (already wired
        // into the wake-deadline hook above) — not a fresh one.
        window.on_request_frame(Box::new(move || {
            let lane_frame = Arc::clone(&lane_frame);
            let worker_reload_frame = worker_reload_frame.clone();
            let device_recovery_backoff = Arc::clone(&device_recovery_backoff);
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
                    let dirty = frame_is_dirty(
                        inbox_redraw,
                        realm.needs_redraw(),
                        realm.has_pending_work(),
                        device_recovery_backoff.next_attempt_at(),
                    );
                    match wake_action(
                        scheduler.frames_enabled(),
                        dirty,
                        scheduler.is_frame_scheduled(),
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
                            // Reuse the existing no-present throttle: a backgrounded
                            // wake with dirty/pending work re-requesting another
                            // wake every loop tick has the identical busy-spin risk
                            // an un-presented frame with an open gate has, and
                            // nothing else paces it while frames are disabled.
                            let keeps_gate_open = keeps_frame_gate_open(
                                realm.needs_redraw(),
                                scheduler.is_frame_scheduled(),
                                realm.has_pending_work(),
                            );
                            if let Some(pace) = no_present_fallback_pace(false, keeps_gate_open) {
                                std::thread::sleep(pace);
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

                    // No-present fallback throttle. Fifo present (the default, see
                    // `select_present_mode`) blocks every PRESENTED frame at display
                    // cadence — that IS the steady-state pacing, which is why the fixed
                    // frame-budget sleep this replaced is gone. A frame that never
                    // reaches `present()` (no damage, occluded surface, surface lost)
                    // gets none of that blocking, so if nothing else is going to wake
                    // this loop, an unpaced wake is harmless: the loop falls back to
                    // `ControlFlow::Wait` and blocks on the next real event. The
                    // busy-spin this guards against (observed: ~30 000 fps) only
                    // happens when a ticker/animation keeps re-requesting a frame every
                    // wake with nothing pacing it — `no_present_fallback_pace` fires
                    // only in exactly that combination. A still-lost device armed for
                    // a LATER retry does NOT feed this pace: `DeviceRecoveryBackoff`
                    // paces the ATTEMPT itself (a deadline check, never a sleep — see
                    // its own doc), and its deadline reaches the platform's own idle
                    // wait through the wake-deadline hook wired above, not through
                    // this fallback.
                    let keeps_gate_open = keeps_frame_gate_open(
                        realm.needs_redraw(),
                        scheduler.is_frame_scheduled(),
                        realm.has_pending_work(),
                    );
                    if let Some(pace) = no_present_fallback_pace(outcome.presented, keeps_gate_open)
                    {
                        // This runs on the platform event-loop thread, so the sleep
                        // blocks input dispatch for its duration — acceptable here
                        // because this path only fires for an occluded/undamaged
                        // window with a ticker still running, not an interactive one.
                        std::thread::sleep(pace);
                    }
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
        // Detached is dispatched through the realm because shutdown must
        // cancel any pointer sequence whose platform Up/Cancel will never
        // arrive before lifecycle observers run.

        // Platform quit -> Detached (frames disabled, listeners notified).
        // Fallback path, not the primary one any more: the ORDINARY
        // "close the last window -> exit" sequence now delivers Detached
        // earlier, from `close_this_window`'s own `RealmTask::
        // ClosePresentation` handling (the sole-presentation branch), before
        // this realm is even uninstalled -- so by the time `on_quit` fires
        // moments later, this dispatch routinely finds the realm already
        // gone. This callback stays registered for the case that path does
        // NOT cover: a quit requested with no preceding window close at all
        // (an OS-level quit signal, e.g. macOS Cmd+Q, or an embedder calling
        // `owner.quit()` directly).
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
                    // Debug-only, not warn: the ordinary window-close-then-
                    // quit sequence above ALWAYS reaches this dispatch after
                    // the realm is already gone (Detached already delivered,
                    // the realm already uninstalled) -- an error here is the
                    // routine case, not a signal something went wrong.
                    tracing::debug!(
                        ?error,
                        "realm unavailable during Detached lifecycle dispatch (routine when a \
                         window close already delivered it)"
                    );
                }
            }));
        });

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
            close_this_window(realm_dispatch);
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
        window.on_visibility_status_change(Box::new(move |visible| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::WindowVisibility(visible)),
            );
        }));
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
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        // Mark lifecycle as started (Resumed). Routed through the same
        // dispatch every other lifecycle signal uses -- one fact, one place
        // (`emit_lifecycle_transition` reads the realm's own scheduler) --
        // rather than reaching for a process-global one that no longer
        // exists. A fresh realm's scheduler already starts at `Resumed`
        // (`BindingState::lifecycle_state`'s default), so this ladder is
        // empty and the call is a documented no-op, matching prior behavior.
        debug_assert_eq!(
            std::thread::current().id(),
            realm_dispatch.owner_thread,
            "desktop bootstrap must run on the realm's owner thread"
        );
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
        );

        // 10. Request initial redraw, now that the window is stored.
        // `wake` (not a direct `request_redraw()` on the window): it clones
        // the window out from under the redraw-poke slot's lock before
        // calling through, so a backend whose `request_redraw` re-enters
        // this runtime synchronously (headless, in this crate's own tests)
        // cannot deadlock on that same lock — the same clone-then-call
        // discipline used by direct platform capabilities.
        wake();

        tracing::info!("Desktop platform initialized with callbacks");
        Ok(())
    }

    // Window creation, GPU/renderer setup, and callback wiring all run
    // inside `on_ready` rather than before `run()`. The winit backend can
    // only create a window from inside a running event loop (`ActiveEventLoop`
    // is unreachable beforehand); opening it earlier would deadlock forever
    // waiting for a pump that never started. `on_ready` runs exactly once,
    // synchronously, on this same thread — see `Platform::run`'s doc.
    //
    // The owner-host clear guard is armed HERE, before `run(...)`, not
    // inside `on_ready` — so a panic anywhere inside `on_ready` (or later,
    // on backends where `run` keeps running after it) unwinds through the
    // guard and cannot leak the host onto this thread past this call
    // (ADR-0039 §6).
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();
    let result = platform.run(Box::new(move |owner| {
        install_owner_platform(owner);
        // `?` converts `bootstrap_desktop`'s `anyhow::Error` into the
        // callback's opaque `BootstrapError` (anyhow's own `From` impl).
        bootstrap_desktop(root, config, worker_reload, rebuild_registration_slot)?;
        Ok(())
    }));

    // Event loop exited: drop the runtime now (releases the at-most-one
    // claim; outstanding senders turn `OwnerGone`) instead of at thread
    // death.
    drop(rebuild_registration.borrow_mut().take());
    teardown_platform_realm();

    // Surface a fatal bootstrap failure (GPU init, `UiRealm` construction,
    // root widget attach, or window creation) now that the event loop has
    // exited — `on_ready`'s `Err` propagates straight out of `Platform::run`;
    // no side-channel cell is needed to thread it out anymore.
    if let Err(err) = result {
        panic!("desktop bootstrap failed: {err:?}");
    }
}
