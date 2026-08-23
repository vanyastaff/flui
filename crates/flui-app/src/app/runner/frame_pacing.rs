// ============================================================================
// Desktop frame-pacing gate (App.1 vsync pacing)
// ============================================================================
//
// Extracted as free functions — pure, no realm/window/GPU state — so the
// decisions each platform's frame callback makes each wake are unit
// testable without a live event loop. See the frame-pacing ADR for the
// full design: Fifo present blocks every PRESENTED frame at display
// cadence (the steady-state pacing); these functions cover what happens on
// the frames that path never blocks: a spurious wake with nothing to do or
// a backgrounded app (`wake_action`), and a frame that ran the pipeline but
// never reached `present()` (`no_present_fallback_pace`).

/// What a platform wake should do: run the full frame pipeline, pump only
/// the async driver, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WakeAction {
    /// Run the full frame pipeline — the pre-existing path, unchanged:
    /// frames are enabled and there is real work or a scheduled ticker.
    Render,
    /// Frames are disabled (`AppLifecycleState::Hidden`/`Paused`/
    /// `Detached`): poll only [`UpdateScheduler::drive_async_tasks`](flui_scheduler::UpdateScheduler::drive_async_tasks) — never
    /// begin/draw a frame, tick, run the pipeline, or present. Dirty work
    /// is left untouched; it accumulates until frames re-enable.
    PumpAsync,
    /// A spurious wake while frames are enabled: nothing dirty, no
    /// scheduled ticker. No render, no pump, no sleep.
    Skip,
}

/// Decides what a platform wake should do, given the scheduler's
/// [`UpdateScheduler::frames_enabled`](flui_scheduler::UpdateScheduler::frames_enabled) fact (ADR-0035) alongside the pre-existing
/// dirty/scheduled-ticker signals.
///
/// `frames_enabled == false` takes priority over everything else — even
/// with `dirty` work pending, a backgrounded app pumps only the async
/// driver; the dirty work is left alone (it accumulates untouched) rather
/// than running a full frame nobody can see. This is the ONLY thing that
/// keeps a spawned future progressing while the app is backgrounded: the
/// mid-frame `drive_async_tasks` poll inside `handle_begin_frame` never
/// runs in `PumpAsync` mode (no frame runs at all), so this explicit call
/// is the only pump.
///
/// `dirty` is true when there is real work (an inbox redraw request,
/// `needs_redraw`, or dirty pipeline nodes); `frame_scheduled` is true when
/// the global `UpdateScheduler` has a pending ticker callback (a running
/// `AnimationController` with no other dirty state).
pub(super) fn wake_action(frames_enabled: bool, dirty: bool, frame_scheduled: bool) -> WakeAction {
    if !frames_enabled {
        return WakeAction::PumpAsync;
    }
    if dirty || frame_scheduled {
        WakeAction::Render
    } else {
        WakeAction::Skip
    }
}

/// The frame closure's `dirty` gate, shared verbatim by both backends' own
/// closures AND their tests — pulled out for the identical reason
/// `wake_action`/`keeps_frame_gate_open`/`no_present_fallback_pace`/
/// `merge_wake_deadlines` already are one level up: a test that reimplements
/// a one-line predicate in its own body instead of calling the production
/// code silently stops pinning it. That happened here once already — both
/// `the_real_closure_gate_...` tests below used to compute this boolean
/// inline, so reverting the REAL `next_attempt_at().is_some()` term in
/// either closure below left the tests green (they were asserting against
/// their own copy of the old logic, not the production line) even though
/// the fix it was meant to pin was gone.
///
/// Four sources, all required: `inbox_redraw` (a command drained this frame
/// boundary asked for a redraw), `needs_redraw`/`has_pending_work` (the
/// realm's own pre-existing dirty state), and `next_attempt_at.is_some()` —
/// an armed device-recovery retry deadline. Dropping that last term is
/// exactly the bug `DeviceRecoveryBackoff`'s own doc describes: a deadline
/// wired into the wake-deadline hook but invisible to this gate reaches
/// `WakeAction::Skip` and returns before `render_frame_with_device_recovery`
/// is ever called, no matter how faithfully the platform actuates the wake.
// Absent on wasm: its two production callers are the desktop and Android
// frame closures, neither of which exists there, and the web runner has
// no `DeviceRecoveryBackoff` deadline term to fold in.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn frame_is_dirty(
    inbox_redraw: bool,
    needs_redraw: bool,
    has_pending_work: bool,
    next_attempt_at: Option<web_time::Instant>,
) -> bool {
    inbox_redraw || needs_redraw || has_pending_work || next_attempt_at.is_some()
}

/// Whether another frame will be requested regardless of this one's
/// outcome: `needs_redraw`, a scheduled ticker, or dirty
/// pipeline/build work left over from the frame that just ran.
///
/// This only feeds [`no_present_fallback_pace`]'s THROTTLE decision below —
/// it cannot itself wake anything. A `ControlFlow::Wait` loop only wakes on
/// an actual `wake_frame()`/platform `request_redraw()` call or external
/// input; a dropped/errored frame's retry wake comes from
/// `render_frame_entered`'s `retry_needed` path, not from this function.
///
/// The pending-work leg matters when a frame that left dirty pipeline/build
/// nodes behind is ALSO being re-invoked by some other wake source without
/// ever reaching `present()`: without this leg, such a frame would read
/// `keeps_gate_open == false`, skip the fallback sleep, and the loop could
/// spin at full CPU speed re-processing the same leftover work on every
/// rapid re-wake instead of being bounded like any other no-present,
/// gate-open frame.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn keeps_frame_gate_open(
    needs_redraw: bool,
    frame_scheduled: bool,
    has_pending_work: bool,
) -> bool {
    needs_redraw || frame_scheduled || has_pending_work
}

/// Coarse fallback pace for a frame that ran the pipeline but never reached
/// `present()`, applied only while a ticker keeps re-requesting a frame.
///
/// This throttles; it does not pace. An un-presented frame carries no vsync
/// signal (Fifo's blocking present never engaged), so this is a fixed CPU-time
/// bound, not frame-accurate cadence — good enough to keep a repeating
/// controller behind a minimized/occluded window (or a `SurfaceLost` retry
/// loop) from busy-spinning at CPU speed (observed pre-fix: ~30 000 fps).
///
/// Not `cfg`-gated to desktop-only: Android's `PumpAsync` arm (`run_android`)
/// reuses this same bound unconditionally — a self-re-arming task
/// `UpdateScheduler::finish_async_pump` lets keep waking the loop has no
/// vsync/present call to bound it there either, and that arm has no
/// `keeps_frame_gate_open`-style signal desktop's conditional
/// `no_present_fallback_pace` uses — so its throttle is unconditional
/// instead of gate-open-dependent. Web's `PumpAsync` arm does NOT use this
/// (see its call site's comment: the browser's own `requestAnimationFrame`
/// cadence already bounds it, and `wasm32-unknown-unknown` has no real
/// `std::thread::sleep`) — excluded via `cfg` so it isn't flagged unused
/// there.
#[cfg(not(target_arch = "wasm32"))]
pub(super) const NO_PRESENT_FALLBACK_PACE: std::time::Duration =
    std::time::Duration::from_millis(16);

/// Decides whether [`NO_PRESENT_FALLBACK_PACE`] applies this frame.
///
/// `presented` is `false` when `render_frame_entered`'s scene never reached
/// `present()` — no damage, an occluded surface, or a lost surface.
/// `keeps_gate_open` is `true` when another frame will be requested
/// regardless (`UiRealm::needs_redraw` or the scheduler still has a
/// ticker scheduled). The fallback is needed only when both hold: no vsync
/// block happened AND something is about to wake this loop again anyway —
/// that combination is the only busy-spin risk left once the fixed
/// frame-budget sleep is gone. A presented frame needs no fallback (Fifo
/// already paced it); an un-presented frame with nothing re-requesting a
/// wake needs no fallback either (the loop just goes idle).
///
/// Occlusion semantics differ by platform: on Wayland, frame callbacks stop
/// while a window is hidden, so no redraws arrive and tickers freeze (this
/// fallback never fires); on Windows/X11, redraw requests keep arriving for a
/// minimized window and this fallback bounds them. Timeout-shaped animations
/// (e.g. the snack-bar auto-dismiss controller) do not progress while frozen —
/// a future platform Timer service is the correctness seam for those.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn no_present_fallback_pace(
    presented: bool,
    keeps_gate_open: bool,
) -> Option<std::time::Duration> {
    (!presented && keeps_gate_open).then_some(NO_PRESENT_FALLBACK_PACE)
}

/// App.1 vsync-pacing gate tests.
///
/// `run_desktop` itself opens a real window and GPU device, so it cannot
/// run headlessly; `wake_action` and `no_present_fallback_pace` were pulled
/// out specifically so the decisions the frame callback makes each wake are
/// covered here without one. Coverage map for the four invariants the
/// frame-pacing ADR calls out:
///
/// - **Wake coalescing** (N `wake_frame` calls -> one draw): a
///   PRE-EXISTING invariant, unchanged by this diff — pinned by
///   `ui_realm::tests::redraw_requests_coalesce_to_one_flag_and_one_wake`.
/// - **Idle = zero frames**: a PRE-EXISTING invariant (the dirty gate
///   itself predates this diff; only its migration onto `wake_action` is
///   new) — pinned by
///   `idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips`
///   below.
/// - **No-present fallback bound**: the actual delta the frame-pacing ADR
///   introduces — pinned by `no_present_fallback_bounds_repeating_no_present_wakes`.
/// - **Ticker keeps the gate open**: the fallback's AND condition — pinned
///   by `no_present_fallback_pace_requires_both_no_present_and_an_open_gate`
///   (this module) and, at the binding layer, by
///   `binding::tests::vsync_continuation_keeps_gate_open_while_running_and_closes_on_settle`.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod desktop_pacing_tests {
    use std::time::{Duration, Instant};

    use super::{
        NO_PRESENT_FALLBACK_PACE, WakeAction, keeps_frame_gate_open, no_present_fallback_pace,
        wake_action,
    };

    #[test]
    fn idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips() {
        assert_eq!(
            wake_action(true, false, false),
            WakeAction::Skip,
            "a spurious wake with frames enabled, nothing dirty, and no scheduled ticker must \
             render zero frames"
        );
    }

    #[test]
    fn dirty_work_or_a_scheduled_ticker_alone_renders_a_frame() {
        assert_eq!(
            wake_action(true, true, false),
            WakeAction::Render,
            "dirty work alone renders"
        );
        assert_eq!(
            wake_action(true, false, true),
            WakeAction::Render,
            "a scheduled ticker alone renders (keeps animations alive with no other dirty state)"
        );
        assert_eq!(wake_action(true, true, true), WakeAction::Render);
    }

    #[test]
    fn frames_disabled_always_pumps_async_regardless_of_dirty_or_scheduled() {
        // The load-bearing case: a backgrounded app must never render, even
        // with real dirty work or a scheduled ticker — dirty work
        // accumulates untouched until frames re-enable.
        assert_eq!(wake_action(false, false, false), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, true, false), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, false, true), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, true, true), WakeAction::PumpAsync);
    }

    /// A spawned future must keep progressing through `PumpAsync`'s
    /// `UpdateScheduler::drive_async_tasks` call while frames are disabled, with
    /// no frame ever advancing — and a `Resumed` transition afterward must
    /// produce exactly one frame.
    ///
    /// Standalone `UpdateScheduler::new()`, not the process singleton: this test
    /// mirrors what `run_desktop`'s frame callback does on a `PumpAsync`
    /// wake, without needing a live window/event loop.
    ///
    /// If reverted: gate the pump too (only call `drive_async_tasks` when
    /// `wake_action` returns `Render` — a mistaken "no work while
    /// backgrounded" fix) and this fails: the future never completes (RUN
    /// IT — see the test module doc for how this is verified).
    #[test]
    fn frames_disabled_pump_async_keeps_futures_running_without_advancing_frames() {
        use std::sync::atomic::{AtomicBool, AtomicUsize};

        use flui_scheduler::{AppLifecycleState, UpdateScheduler};

        let scheduler = UpdateScheduler::new();
        let polls = std::sync::Arc::new(AtomicUsize::new(0));
        let completed = std::sync::Arc::new(AtomicBool::new(false));
        let polls_for_task = std::sync::Arc::clone(&polls);
        let completed_for_task = std::sync::Arc::clone(&completed);
        // Needs two polls to complete, so the loop below observes both the
        // Pending and the Ready poll — proving `drive_async_tasks` is what
        // actually advances it, not a single incidental call.
        let _token = scheduler.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            let n = polls_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if n < 2 {
                // Re-arm itself for the next `drive_async_tasks` call —
                // without this, `poll_ready` would only ever poll it once
                // (nothing else wakes it), and the second loop iteration
                // below would silently poll zero tasks.
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            } else {
                completed_for_task.store(true, std::sync::atomic::Ordering::SeqCst);
                std::task::Poll::Ready(())
            }
        })));

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        assert!(!scheduler.frames_enabled());

        let frame_count_before = scheduler.frame_count();
        for _ in 0..2 {
            assert_eq!(
                wake_action(
                    scheduler.frames_enabled(),
                    true,
                    scheduler.is_frame_scheduled()
                ),
                WakeAction::PumpAsync,
                "frames disabled must always pump, even with dirty work"
            );
            scheduler.drive_async_tasks();
        }

        assert!(
            completed.load(std::sync::atomic::Ordering::SeqCst),
            "the future must complete via PumpAsync's drive_async_tasks calls alone"
        );
        assert_eq!(
            scheduler.frame_count(),
            frame_count_before,
            "no frame may run while the app is backgrounded"
        );

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        assert_eq!(
            wake_action(
                scheduler.frames_enabled(),
                true,
                scheduler.is_frame_scheduled()
            ),
            WakeAction::Render
        );
        let now = web_time::Instant::now();
        scheduler.drive_frame(now, flui_scheduler::IdleDeadline::far_future(now), || {});
        assert_eq!(
            scheduler.frame_count(),
            frame_count_before + 1,
            "resuming must produce exactly one frame"
        );
    }

    #[test]
    fn pending_work_alone_keeps_the_gate_open() {
        assert!(
            keeps_frame_gate_open(false, false, true),
            "a frame that left dirty pipeline/build nodes behind must keep the fallback-pace \
             gate open (so the busy-spin throttle still applies on a rapid re-wake) even with \
             no `needs_redraw` and no scheduled ticker"
        );
    }

    #[test]
    fn needs_redraw_or_scheduled_ticker_alone_keeps_the_gate_open() {
        assert!(keeps_frame_gate_open(true, false, false));
        assert!(keeps_frame_gate_open(false, true, false));
    }

    #[test]
    fn no_signal_at_all_closes_the_gate() {
        assert!(
            !keeps_frame_gate_open(false, false, false),
            "with no redraw request, no scheduled ticker, and no pending work, the gate \
             must close so the loop can go idle"
        );
    }

    #[test]
    fn pending_work_drives_the_no_present_fallback_pace_like_any_other_open_gate() {
        // A frame that never presents (surface lost / no damage) but left dirty
        // pipeline work behind must still get the busy-spin-bounding fallback pace —
        // exactly as if `needs_redraw` or a ticker had kept the gate open.
        let keeps_gate_open = keeps_frame_gate_open(false, false, true);
        assert_eq!(
            no_present_fallback_pace(false, keeps_gate_open),
            Some(NO_PRESENT_FALLBACK_PACE)
        );
    }

    #[test]
    fn no_present_fallback_pace_requires_both_no_present_and_an_open_gate() {
        assert_eq!(
            no_present_fallback_pace(true, true),
            None,
            "a presented frame needs no fallback — Fifo present already paced it"
        );
        assert_eq!(
            no_present_fallback_pace(true, false),
            None,
            "a presented frame with a closing gate needs no fallback either"
        );
        assert_eq!(
            no_present_fallback_pace(false, false),
            None,
            "an un-presented frame with nothing re-requesting a wake needs no fallback \
             — the loop simply goes idle, no busy-spin risk"
        );
        assert_eq!(
            no_present_fallback_pace(false, true),
            Some(NO_PRESENT_FALLBACK_PACE),
            "the only busy-spin risk: no present AND a ticker keeps re-requesting a frame"
        );
    }

    /// Mutation-run target for the no-present fallback bound: simulates the shape of
    /// `run_desktop`'s frame callback for a window that never presents
    /// (e.g. minimized/occluded) while a repeating ticker keeps
    /// re-requesting a frame every wake — the exact scenario that used to
    /// busy-spin at CPU speed (observed pre-fix: ~30 000 fps) once the
    /// fixed frame-budget sleep this diff removes was the only thing
    /// bounding it.
    ///
    /// This cannot drive the real winit closure (it requires a live event
    /// loop), so it exercises the same predicate + `thread::sleep` pairing
    /// `run_desktop` calls, in a tight loop bounded by wall-clock time.
    /// Deleting the `sleep` (or the `if let Some` guard around it) turns
    /// this from ~5 iterations in the test window into whatever the CPU
    /// can spin through in that time — comfortably over the assertion's
    /// generous ceiling.
    #[test]
    fn no_present_fallback_bounds_repeating_no_present_wakes() {
        let window = Duration::from_millis(80);
        let deadline = Instant::now() + window;
        let mut iterations = 0u32;

        while Instant::now() < deadline {
            iterations += 1;
            let presented = false; // simulated: no damage / occluded / surface lost
            let keeps_gate_open = true; // simulated: a repeating AnimationController
            if let Some(pace) = no_present_fallback_pace(presented, keeps_gate_open) {
                std::thread::sleep(pace);
            }
        }

        assert!(
            iterations < 50,
            "no-present fallback failed to bound the loop: {iterations} iterations in \
             {window:?} (expected roughly window / NO_PRESENT_FALLBACK_PACE, generously \
             capped) — a busy-spin without it would rack up orders of magnitude more",
        );
    }

    /// Reviewer probe (issue #556): does a running controller with no other
    /// tree-visible effect survive `on_request_frame`'s OWN dirty gate
    /// across many platform callbacks, or does it stall after its anchor
    /// frame?
    ///
    /// The question this settles: `draw_frame_entered`'s vsync-continuation
    /// loop calls `wake_frame()` — which sets `needs_redraw = true` — BEFORE
    /// the segment that renders the frame; `render_frame_entered`'s own
    /// tail then calls `mark_rendered()` — which sets `needs_redraw =
    /// false` — once that render succeeds with no retry needed. Both run
    /// inside the SAME production callback, so `needs_redraw` reads `false`
    /// again by the time this callback returns. The open question was
    /// whether `wake_action`'s dirty check on the NEXT callback (computed
    /// from `needs_redraw()`/`has_pending_work()`, sampled BEFORE
    /// `draw_frame_entered` ever runs, i.e. before the controller gets a
    /// chance to tick again and re-mark demand) would read `Skip` and never
    /// call `draw_frame_entered` at all — silently stalling the controller
    /// after one frame, forever, in production.
    ///
    /// This is deliberately the exact sequence `bootstrap_desktop`'s
    /// `on_request_frame` closure runs, minus the owner-inbox drain (this
    /// probe has nothing in the inbox) and the actual GPU/platform calls
    /// (`record_compositor_tick` -> the `dirty`/`wake_action` gate ->
    /// `render_frame_entered` -> repeat), not `draw_frame_entered` called
    /// directly the way the segment-gate unit tests do — a direct call
    /// bypasses the exact gate this probe exists to settle.
    #[test]
    fn a_running_controller_with_no_other_dirty_state_keeps_producing_across_the_real_wake_action_gate()
     {
        use flui_animation::{Animation, AnimationController, AnimationStatus};

        use crate::app::raster_test_support::TestRasterBackend;

        let realm = crate::app::ui_realm::UiRealm::for_test();
        let controller = AnimationController::new(
            Duration::from_millis(100),
            &flui_scheduler::UpdateScheduler::new(),
        );
        realm.vsync().register(controller.clone());
        controller.forward().expect("fresh controller forwards");
        // The anchor: starting a controller does not itself wake anything
        // (`AnimationController::forward`/`Vsync::register` are pure
        // state mutations with no coupling to a realm's wake capability —
        // confirmed by reading both, neither calls anything wake-shaped).
        // In production SOMETHING external always establishes the first
        // wake before a freshly-started controller's own continuation
        // logic can keep the loop going (the gesture/build code path that
        // called `.forward()` in the first place almost always also
        // dirties the tree or explicitly redraws) — simulated here as one
        // `request_redraw()` call, the same flag `wake_action`'s dirty
        // check reads, without needing a real platform window to poke.
        realm.request_redraw();

        let mut backend = TestRasterBackend::always_presents();
        let mut now = Instant::now();
        let mut render_calls = 0u32;
        let mut skip_calls = 0u32;

        // 40 simulated platform callbacks at ~16ms apart -- comfortably
        // past the controller's 100ms duration, so a genuine stall shows
        // up as `status() != Completed` at the end, not just a slow test.
        for callback in 0..40u32 {
            now += Duration::from_millis(16);
            realm.set_now_secs_for_test(f64::from(callback) * 0.016);

            // Exactly what `bootstrap_desktop`'s closure does, in order:
            // record the compositor tick first (pacing feedback only, marks
            // no demand -- see `FrameClock::record_compositor_tick`'s own
            // doc), THEN read the dirty gate `wake_action` decides from.
            realm.record_compositor_tick(now);
            let dirty = realm.needs_redraw() || realm.has_pending_work();
            match wake_action(
                realm.scheduler().frames_enabled(),
                dirty,
                realm.scheduler().is_frame_scheduled(),
            ) {
                WakeAction::Skip => {
                    skip_calls += 1;
                    continue;
                }
                WakeAction::PumpAsync => panic!(
                    "frames must stay enabled for this probe -- a PumpAsync wake would mean \
                     something else broke lifecycle state, not the property under test"
                ),
                WakeAction::Render => {}
            }
            let _presented = realm.render_frame_entered(&mut backend);
            render_calls += 1;
        }

        assert_eq!(
            controller.status(),
            AnimationStatus::Completed,
            "a running controller with no other dirty state must reach completion across \
             real platform callbacks gated by wake_action -- {render_calls} renders, \
             {skip_calls} skips out of 40 callbacks, final value={}",
            controller.value()
        );
        assert!(
            render_calls > 1,
            "sanity: completion in exactly one render would not distinguish this from a \
             single-anchor-frame stall followed by the loop simply running out of callbacks"
        );

        controller.dispose();
    }

    /// The idle/instant-response headline (adaptive on-demand pacing),
    /// driven through the SAME real dirty-gate every other production wake
    /// goes through -- `wake_action`, `keeps_frame_gate_open`, and
    /// `no_present_fallback_pace` -- not a bare fixed-interval poll loop.
    ///
    /// This distinction is load-bearing, not stylistic: `has_pending_work`
    /// includes `gestures().has_pending_deadlines()`, so a pending
    /// gesture-arena deadline keeps `dirty == true` for the ENTIRE time it
    /// is armed. An earlier version of this test called
    /// `drive_frame_with_lane` on a fixed 5ms interval regardless of this
    /// gate, which made "zero produces" true but hid a real cost: driven
    /// through the real gate, ANY wake while the deadline is
    /// armed-but-not-due reads `dirty == true` -> `WakeAction::Render` ->
    /// a full (unpainting) pump -> `presented == false`, and with the gate
    /// still open afterward, `no_present_fallback_pace` would sleep on the
    /// calling thread. What keeps this genuinely idle is that NOTHING
    /// wakes the loop until the deadline's own instant (`UiRealm::next_wake`,
    /// standing in for the real winit `about_to_wait`/`new_events`
    /// actuator this crate cannot drive with a live event loop under its
    /// own test harness) -- so there is exactly ONE real callback over the
    /// whole window, not many, and the fallback sleep never engages.
    #[test]
    fn idle_presentation_over_a_real_wall_clock_window_produces_zero_then_responds_instantly() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use flui_interaction::{
            GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
        };

        use crate::app::raster_test_support::TestRasterBackend;

        let realm = crate::app::ui_realm::UiRealm::for_test();
        realm
            .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
            .expect("attach succeeds");
        let mut backend = TestRasterBackend::always_presents();

        // Settle the attach's own first paint, through the real gate --
        // so the "instant response" check at the end has real content to
        // actually submit, not a vacuous zero-widget tree.
        let now = Instant::now();
        realm.record_compositor_tick(now);
        let dirty = realm.needs_redraw() || realm.has_pending_work();
        assert_eq!(
            wake_action(
                realm.scheduler().frames_enabled(),
                dirty,
                realm.scheduler().is_frame_scheduled()
            ),
            WakeAction::Render,
            "precondition: the attach's own pending build must render"
        );
        realm.scheduler().drive_frame_with_lane(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {
                let _ = realm.render_frame_entered(&mut backend);
            },
            realm.local_post_frame_lane(),
        );

        // A live async task, polled through the scheduler's real mid-frame
        // slot whenever a pump actually runs -- it never touches
        // demand/dirty state.
        let async_polls = Arc::new(AtomicUsize::new(0));
        let async_polls_for_task = Arc::clone(&async_polls);
        let _token = realm.scheduler().spawn_local(Box::pin(async move {
            async_polls_for_task.fetch_add(1, Ordering::Release);
        }));

        // A real, in-flight gesture-arena deadline, due 100ms from now.
        let arena = realm.gestures().arena().clone();
        let long_press_fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_for_callback = Arc::clone(&long_press_fired);
        let recognizer = LongPressGestureRecognizer::with_settings(
            arena,
            GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(100)),
        )
        .with_on_long_press(move || fired_for_callback.store(true, Ordering::SeqCst));
        let pointer = PointerId::new(2).expect("nonzero pointer id");
        recognizer.add_pointer(
            pointer,
            flui_types::Offset::new(
                flui_types::geometry::px(10.0),
                flui_types::geometry::px(10.0),
            ),
        );

        let produced_before = realm.primary_produced_count_for_test();
        let submits_before = backend.render_scene_calls;

        // Drive the loop waking ONLY at `next_wake`'s own computed instant
        // -- no artificial fixed-interval polling.
        let window_end = Instant::now() + Duration::from_millis(300);
        let mut render_calls = 0u32;
        let mut skip_calls = 0u32;
        // `next_wake() == None` means production would genuinely fall back
        // to `ControlFlow::Wait` here (nothing left to wait for) -- once the
        // one callback below resolves the deadline, that is the expected,
        // successful end of the simulated window, not a bug, so the loop
        // condition itself ends it.
        while let Some(next_wake) = realm.next_wake() {
            if next_wake >= window_end {
                break;
            }
            let wait = next_wake.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            let now = Instant::now();
            realm.record_compositor_tick(now);
            let dirty = realm.needs_redraw() || realm.has_pending_work();
            match wake_action(
                realm.scheduler().frames_enabled(),
                dirty,
                realm.scheduler().is_frame_scheduled(),
            ) {
                WakeAction::Skip => {
                    skip_calls += 1;
                    continue;
                }
                WakeAction::PumpAsync => panic!(
                    "frames must stay enabled for this probe -- a PumpAsync wake would mean \
                     something else broke lifecycle state, not the property under test"
                ),
                WakeAction::Render => {}
            }
            // Wrapped in `drive_frame_with_lane`, exactly like
            // `bootstrap_desktop`'s `on_request_frame` closure -- this is
            // the ONLY call site that actually polls the async driver's
            // mid-frame slot; `render_frame_entered` alone does not.
            realm.scheduler().drive_frame_with_lane(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {
                    let presented = realm.render_frame_entered(&mut backend);
                    let keeps_gate_open = keeps_frame_gate_open(
                        realm.needs_redraw(),
                        realm.scheduler().is_frame_scheduled(),
                        realm.has_pending_work(),
                    );
                    if let Some(pace) = no_present_fallback_pace(presented, keeps_gate_open) {
                        std::thread::sleep(pace);
                    }
                },
                realm.local_post_frame_lane(),
            );
            render_calls += 1;
        }

        assert_eq!(
            realm.primary_produced_count_for_test(),
            produced_before,
            "an idle presentation, driven through the real wake_action gate, must produce \
             exactly zero additional frames over the wall-clock window -- {render_calls} \
             render callbacks, {skip_calls} skips"
        );
        assert_eq!(
            backend.render_scene_calls, submits_before,
            "and submit exactly zero additional frames to the raster backend"
        );
        assert_eq!(
            render_calls, 1,
            "exactly ONE real callback over the whole idle window -- the deadline's own wake, \
             nothing polled it more often than that (render_calls={render_calls}, \
             skip_calls={skip_calls})"
        );
        assert_eq!(
            async_polls.load(Ordering::Acquire),
            1,
            "the async driver's mid-frame poll must have genuinely run the spawned task, at \
             the one real callback that occurred"
        );
        assert!(
            long_press_fired.load(Ordering::SeqCst),
            "the deadline must have resolved by the end of the window"
        );

        // Instant response: dirty the tree now and drive exactly one more
        // real callback through the same gate -- must produce.
        realm.request_redraw();
        let dirty = realm.needs_redraw() || realm.has_pending_work();
        assert_eq!(
            wake_action(
                realm.scheduler().frames_enabled(),
                dirty,
                realm.scheduler().is_frame_scheduled()
            ),
            WakeAction::Render,
            "precondition: the fresh redraw request must render"
        );
        let _ = realm.render_frame_entered(&mut backend);

        assert_eq!(
            realm.primary_produced_count_for_test(),
            produced_before + 1,
            "the first dirty mark after the idle window must produce within the very next \
             real callback"
        );
        assert_eq!(
            backend.render_scene_calls, 1,
            "and that callback must actually reach the raster backend"
        );
    }
}
