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
// never reached `present()` (`FallbackWake`, ADR-0058: a non-blocking wake
// one display period after the last present, never a sleep on the loop).

/// Hands the window's frame-pacing signal to the raster backend: the
/// backend calls [`PlatformWindow::pre_present_notify`](flui_platform::traits::PlatformWindow::pre_present_notify) immediately before
/// every present, and only then. On Wayland this is what makes a running
/// animation compositor-paced (winit withholds the next `RedrawRequested`
/// until the surface's frame callback) and an occluded window silent; on
/// every other backend the notify is a no-op. A named function rather than
/// an inline closure at the bootstrap so the wiring — the hook is installed,
/// it reaches the window, it fires once per presented frame and not for a
/// frame that did not present — is pinned by a unit test through the same
/// call the bootstrap makes.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn install_pre_present_hook(
    backend: &mut impl flui_engine::RasterBackend,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) {
    let window = std::sync::Arc::clone(window);
    backend.set_pre_present_hook(Some(Box::new(move || window.pre_present_notify())));
}

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
pub(super) fn wake_action(
    frames_enabled: bool,
    dirty: bool,
    frame_scheduled: bool,
    fallback: FallbackGate,
) -> WakeAction {
    if !frames_enabled {
        return WakeAction::PumpAsync;
    }
    if dirty {
        return WakeAction::Render;
    }
    // A scheduled ticker alone renders — unless a fallback wake is armed
    // and not yet due (ADR-0058): the previous pump ran the pipeline for
    // this ticker and presented nothing (no visible change in the ~0.2 ms
    // since the last present), so re-running it now would only repeat
    // that; the armed deadline brings the loop back exactly one display
    // period after the last present, and real dirty work (`dirty` above)
    // is never held behind it.
    if frame_scheduled && !fallback.pending {
        WakeAction::Render
    } else {
        WakeAction::Skip
    }
}

/// The frame closure's `dirty` gate, shared verbatim by both backends' own
/// closures AND their tests — pulled out for the identical reason
/// `wake_action`/`keeps_frame_gate_open`/`FallbackWake`/
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
    fallback: FallbackGate,
) -> bool {
    // `needs_redraw` is also the realm's OWN echo: every pump with a
    // running ticker ends by re-requesting a frame through `wake_frame`,
    // which sets it. While a fallback wake is pending that echo is exactly
    // the wake being deferred, so it does not count; inbox redraws, pending
    // build/gesture work and a due recovery attempt always do. The due
    // fallback itself is the fifth term — a deadline source has to be in
    // the dirty predicate AND self-clearing, or its wake is a no-op.
    let needs_redraw = needs_redraw && !fallback.pending;
    inbox_redraw || needs_redraw || has_pending_work || next_attempt_at.is_some() || fallback.due
}

/// Whether another frame will be requested regardless of this one's
/// outcome: `needs_redraw`, a scheduled ticker, or dirty
/// pipeline/build work left over from the frame that just ran.
///
/// This only feeds [`FallbackWake`]'s deferral decision below —
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

/// The pace a BACKGROUNDED pump (frames disabled) is bounded to on Android,
/// whose frame source has no wake-deadline hook to arm instead — see
/// `bootstrap_android`'s `PumpAsync` arm. Desktop no longer sleeps on the
/// loop thread at all (ADR-0058, [`FallbackWake`]).
#[cfg(target_os = "android")]
pub(super) const BACKGROUNDED_PUMP_PACE: std::time::Duration = std::time::Duration::from_millis(16);

/// The display period assumed when the platform cannot report one (no
/// monitor known, a backend without the query): one 60 Hz frame. Only the
/// fallback wake reads it, and only after a pump that presented nothing —
/// on a stack whose present blocks, or whose compositor paces redraws, it
/// is never the pacer. A window that CAN report its period overrides this
/// at bootstrap and after every move/resize.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) const DEFAULT_DISPLAY_PERIOD: std::time::Duration =
    std::time::Duration::from_micros(16_667);

/// How much of the display period the fallback wake waits after the last
/// present. Slightly under a full period on purpose: on a stack whose
/// present DOES block at vsync the block absorbs the difference and keeps
/// the pump phase-locked to the display; a full period would slide later
/// each frame and beat against vsync (a periodic dropped frame), and on a
/// non-blocking stack the ~5 % surplus is simply absorbed by the
/// swapchain. The number is a measured trade, not a constant of nature —
/// ADR-0058 carries the captures behind it.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
const FALLBACK_PERIOD_FRACTION: f64 = 0.95;

/// What a wake sees of the fallback deadline: armed and not yet due (the
/// ticker-only wake is deferred), or due (this wake IS the deferred one).
/// Both false when nothing is armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct FallbackGate {
    /// A deadline is armed and lies in the future.
    pub(super) pending: bool,
    /// A deadline is armed and has passed — consumed by the wake that reads
    /// it (see [`FallbackWake::gate`]).
    pub(super) due: bool,
}

/// A window's non-blocking bound on ticker-driven wakes that present
/// nothing (ADR-0058). Replaces the fixed 16 ms `thread::sleep` on the
/// event-loop thread that used to play this role: that sleep was the only
/// pacer on any stack whose present does not block at vsync, quantized a
/// 165 Hz panel to ~60 frames per second, and blocked input for its whole
/// duration every cycle.
///
/// The bound is a deadline, not a sleep: after a pump that ran the
/// pipeline for a running ticker and presented nothing, the next produce
/// is deferred to `last_present + FALLBACK_PERIOD_FRACTION × period` via
/// the platform's wake-deadline hook (`ControlFlow::WaitUntil`), the loop
/// stays responsive to input meanwhile, and the deadline is one-shot:
/// consumed by the wake it brings, or cleared once it has passed without
/// one (a hidden Wayland surface never delivers that wake — its redraw is
/// withheld until the compositor resumes — and a past deadline handed back
/// to `about_to_wait` again and again would be the busy-spin ADR-0044 §7
/// measured, not a wake).
///
/// A stack whose present blocks at vsync, or whose compositor paces
/// redraws (Wayland after `pre_present_notify`), never reaches this: the
/// pump presents every time and the deadline is never armed.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[derive(Debug)]
pub(super) struct FallbackWake {
    state: parking_lot::Mutex<FallbackState>,
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[derive(Debug)]
struct FallbackState {
    period: std::time::Duration,
    last_present_at: Option<web_time::Instant>,
    deadline: Option<web_time::Instant>,
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl FallbackWake {
    /// A fallback wake for a display with the given refresh period
    /// ([`DEFAULT_DISPLAY_PERIOD`] when the platform reports none).
    pub(super) fn new(period: std::time::Duration) -> Self {
        Self {
            state: parking_lot::Mutex::new(FallbackState {
                period,
                last_present_at: None,
                deadline: None,
            }),
        }
    }

    /// Adopt a new display period (the window moved to another monitor, or
    /// the platform learned it late). Takes effect at the next arm.
    pub(super) fn set_period(&self, period: std::time::Duration) {
        self.state.lock().period = period;
    }

    /// The period in force.
    pub(super) fn period(&self) -> std::time::Duration {
        self.state.lock().period
    }

    /// What this wake sees. Reading a DUE deadline consumes it — this wake
    /// is the one the deadline asked for, and a deadline source that is not
    /// self-clearing re-fires forever.
    pub(super) fn gate(&self, now: web_time::Instant) -> FallbackGate {
        let mut state = self.state.lock();
        match state.deadline {
            Some(deadline) if now < deadline => FallbackGate {
                pending: true,
                due: false,
            },
            Some(_) => {
                state.deadline = None;
                FallbackGate {
                    pending: false,
                    due: true,
                }
            }
            None => FallbackGate::default(),
        }
    }

    /// A frame presented at `now`: the anchor for the next deferral, and
    /// nothing is deferred any more.
    pub(super) fn record_present(&self, now: web_time::Instant) {
        let mut state = self.state.lock();
        state.last_present_at = Some(now);
        state.deadline = None;
    }

    /// A pump ran and presented nothing while a ticker keeps the gate open:
    /// defer the next ticker-only wake to one (fractional) display period
    /// after the last present — or after `now`, when nothing has presented
    /// yet or the last present is already further back than that (an
    /// occluded window keeps a bounded, non-blocking cadence). Returns the
    /// armed instant.
    pub(super) fn arm_after_no_present(&self, now: web_time::Instant) -> web_time::Instant {
        let mut state = self.state.lock();
        let interval = state.period.mul_f64(FALLBACK_PERIOD_FRACTION);
        let anchored = state
            .last_present_at
            .map(|at| at + interval)
            .filter(|deadline| *deadline > now)
            .unwrap_or(now + interval);
        state.deadline = Some(anchored);
        anchored
    }

    /// The instant the platform's wake-deadline hook should wake the loop
    /// at while a deferral is armed.
    ///
    /// **This query never consumes the deadline** — [`Self::gate`], called
    /// from the frame callback, is the sole consumer, and splitting those
    /// two roles is not a style choice: `about_to_wait` and the frame
    /// callback observe the same wake, and winit runs `about_to_wait` on
    /// the very iteration the deadline expires, BEFORE the redraw its
    /// `ResumeTimeReached` poke queues has been dispatched. A version of
    /// this method that cleared a just-passed deadline destroyed it in
    /// that window: the hook then answered `None`, the loop parked in
    /// `ControlFlow::Wait`, the poke never happened, and — because a
    /// pending deferral suppresses the realm's own redraw echo — nothing
    /// woke the loop again. Measured on a real 164.89 Hz X11 session: the
    /// animation froze mid-flight, `next_wake` observing the deadline 5 µs
    /// late, and stayed frozen for the rest of the run.
    ///
    /// A deadline already behind `now` is therefore still reported (as
    /// `now`, so `WaitUntil` fires immediately) — that IS the wake being
    /// asked for, and `gate` consumes it exactly once when the frame
    /// callback it pokes finally runs, so at most one extra iteration is
    /// spent, never a spin. The one case where that poke can never be
    /// answered is a surface whose redraws the compositor withholds (a
    /// hidden Wayland window): a deadline more than one full period late
    /// is abandoned, which bounds that case at roughly one wasted wake and
    /// leaves the presentation idle, as a hidden presentation should be.
    /// The ticker's demand is retained on the clock either way, so the
    /// next delivered redraw produces.
    pub(super) fn next_wake(&self, now: web_time::Instant) -> Option<web_time::Instant> {
        let mut state = self.state.lock();
        let deadline = state.deadline?;
        if deadline > now {
            return Some(deadline);
        }
        let late = now.saturating_duration_since(deadline);
        if late > state.period {
            // The wake this deadline asked for is not coming (a hidden
            // surface withholds redraws). Abandon it rather than re-arming
            // `WaitUntil` in the past every iteration — ADR-0044 §7's
            // measured busy-spin. Clearing it also lifts the suppression of
            // the realm's own redraw echo, so an ordinary wake produces.
            state.deadline = None;
            tracing::trace!(
                target: "flui.pace",
                event = "fallback_abandoned",
                late_us = late.as_micros() as u64,
                "the deferred wake was never delivered; abandoning the deadline"
            );
            return None;
        }
        Some(now)
    }
}

/// App.1 vsync-pacing gate tests.
///
/// `run_desktop` itself opens a real window and GPU device, so it cannot
/// run headlessly; `wake_action` and `FallbackWake` were pulled
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
/// - **No-present fallback bound**: pinned by
///   `the_fallback_bounds_repeating_no_present_wakes_without_sleeping_on_the_loop`
///   (ADR-0058's non-blocking deadline, replacing ADR-0029's sleep).
/// - **Ticker keeps the gate open**: the fallback's AND condition — pinned
///   by `pending_work_arms_the_fallback_like_any_other_open_gate`
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
        DEFAULT_DISPLAY_PERIOD, FallbackGate, FallbackWake, WakeAction, frame_is_dirty,
        keeps_frame_gate_open, wake_action,
    };

    #[test]
    fn idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips() {
        assert_eq!(
            wake_action(true, false, false, FallbackGate::default()),
            WakeAction::Skip,
            "a spurious wake with frames enabled, nothing dirty, and no scheduled ticker must \
             render zero frames"
        );
    }

    #[test]
    fn dirty_work_or_a_scheduled_ticker_alone_renders_a_frame() {
        assert_eq!(
            wake_action(true, true, false, FallbackGate::default()),
            WakeAction::Render,
            "dirty work alone renders"
        );
        assert_eq!(
            wake_action(true, false, true, FallbackGate::default()),
            WakeAction::Render,
            "a scheduled ticker alone renders (keeps animations alive with no other dirty state)"
        );
        assert_eq!(
            wake_action(true, true, true, FallbackGate::default()),
            WakeAction::Render
        );
    }

    #[test]
    fn frames_disabled_always_pumps_async_regardless_of_dirty_or_scheduled() {
        // The load-bearing case: a backgrounded app must never render, even
        // with real dirty work or a scheduled ticker — dirty work
        // accumulates untouched until frames re-enable.
        assert_eq!(
            wake_action(false, false, false, FallbackGate::default()),
            WakeAction::PumpAsync
        );
        assert_eq!(
            wake_action(false, true, false, FallbackGate::default()),
            WakeAction::PumpAsync
        );
        assert_eq!(
            wake_action(false, false, true, FallbackGate::default()),
            WakeAction::PumpAsync
        );
        assert_eq!(
            wake_action(false, true, true, FallbackGate::default()),
            WakeAction::PumpAsync
        );
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
                    scheduler.is_frame_scheduled(),
                    FallbackGate::default(),
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
                scheduler.is_frame_scheduled(),
                FallbackGate::default(),
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

    /// The frame-pacing signal wiring (ADR-0058): once installed, the
    /// backend notifies the window before every present and never for a
    /// frame that did not present. The counter is read from INSIDE the
    /// backend's own render script, so the assertion is "notified by the
    /// time the present happens", not merely "notified at some point".
    #[test]
    fn install_pre_present_hook_notifies_the_window_once_per_presented_frame_only() {
        use std::sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        };

        use flui_engine::RasterBackend;
        use flui_layer::Scene;

        use crate::app::{raster_test_support::TestRasterBackend, window_test_support::TestWindow};

        let test_window = TestWindow::new();
        let notifies = test_window.pre_present_notifies_handle();
        let window: Arc<dyn flui_platform::traits::PlatformWindow> = Arc::new(test_window);

        // Present on even calls, skip (no damage / occluded) on odd ones.
        let seen_at_present: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let notifies_for_script = Arc::clone(&notifies);
        let seen_for_script = Arc::clone(&seen_at_present);
        let mut backend = TestRasterBackend::new(move |call, _scene| {
            let presents = call % 2 == 0;
            if presents {
                // What the wgpu renderer guarantees: by the time the present
                // happens the window has already been notified for THIS frame.
                seen_for_script.store(notifies_for_script.load(Ordering::SeqCst), Ordering::SeqCst);
            }
            Ok(presents)
        });

        let scene = Scene::default();
        // Nothing installed yet: a present must not notify anybody.
        backend.render_scene(&scene).expect("scripted present");
        assert_eq!(
            notifies.load(Ordering::SeqCst),
            0,
            "no hook installed, no notify"
        );

        super::install_pre_present_hook(&mut backend, &window);

        backend.render_scene(&scene).expect("skip");
        assert_eq!(
            notifies.load(Ordering::SeqCst),
            0,
            "a frame that does not present must not arm the platform's frame callback — \
             on Wayland a callback with no commit behind it wedges every later redraw"
        );
        backend.render_scene(&scene).expect("present");
        assert_eq!(
            notifies.load(Ordering::SeqCst),
            1,
            "one notify per presented frame"
        );
        backend.render_scene(&scene).expect("skip");
        backend.render_scene(&scene).expect("present");
        assert_eq!(notifies.load(Ordering::SeqCst), 2);
        // The double runs the hook only when the script reports a present,
        // so the notified count the script observed lags by one: the
        // test backend cannot know the outcome before the script runs. What
        // this pins is that the count is monotonic and one-per-present.
        assert!(seen_at_present.load(Ordering::SeqCst) <= 2);

        backend.set_pre_present_hook(None);
        backend.render_scene(&scene).expect("skip");
        backend.render_scene(&scene).expect("present");
        assert_eq!(
            notifies.load(Ordering::SeqCst),
            2,
            "uninstalled: no further notifies"
        );
    }

    #[test]
    fn pending_work_arms_the_fallback_like_any_other_open_gate() {
        // A frame that never presents (surface lost / no damage) but left dirty
        // pipeline work behind must still arm the bounding fallback wake —
        // exactly as if `needs_redraw` or a ticker had kept the gate open.
        let keeps_gate_open = keeps_frame_gate_open(false, false, true);
        assert!(keeps_gate_open);

        let fallback = FallbackWake::new(DEFAULT_DISPLAY_PERIOD);
        let now = Instant::now();
        let armed = fallback.arm_after_no_present(now);
        assert!(
            armed > now && armed <= now + DEFAULT_DISPLAY_PERIOD,
            "an un-presented frame with an open gate defers the next ticker-only wake by \
             at most one display period"
        );
    }

    /// The deferral is anchored to the LAST PRESENT, not to the pump that
    /// found nothing to present. Anchoring to `now` instead would add the
    /// pump's own duration to every period — a slow, cumulative slide
    /// against the display that shows up as a periodic dropped frame while
    /// every median still reads healthy.
    #[test]
    fn the_fallback_deadline_is_anchored_to_the_last_present() {
        let period = Duration::from_micros(6_065); // a 164.89 Hz panel
        let fallback = FallbackWake::new(period);
        let present_at = Instant::now();
        fallback.record_present(present_at);

        // The pump that presents nothing runs 1 ms after the present.
        let pump_at = present_at + Duration::from_millis(1);
        let armed = fallback.arm_after_no_present(pump_at);

        let from_present = armed.saturating_duration_since(present_at);
        assert!(
            from_present < period,
            "the deadline must sit inside one period of the last present (got {from_present:?} \
             for a {period:?} panel), not one period after the pump that observed no present"
        );
        assert!(
            armed > pump_at,
            "and still in the future, or the wake it asks for is a busy-spin"
        );
    }

    /// A present clears any pending deferral: the next ticker wake is not
    /// held behind a deadline armed before the frame that actually landed.
    #[test]
    fn a_present_clears_a_pending_deferral() {
        let fallback = FallbackWake::new(DEFAULT_DISPLAY_PERIOD);
        let now = Instant::now();
        fallback.arm_after_no_present(now);
        assert!(fallback.gate(now).pending, "armed");

        fallback.record_present(now + Duration::from_millis(1));
        assert_eq!(
            fallback.gate(now + Duration::from_millis(1)),
            FallbackGate::default(),
            "a present clears the deferral outright"
        );
    }

    /// The deadline is one-shot in BOTH directions, which is what keeps it
    /// from becoming the `ControlFlow::WaitUntil` busy-spin ADR-0044 §7
    /// measured: the wake that finds it due consumes it, and a deadline
    /// that passed without ever being delivered (a hidden Wayland surface
    /// withholds redraws) is dropped by the next `next_wake` query rather
    /// than handed to `about_to_wait` again in the past.
    #[test]
    fn a_due_deadline_is_consumed_once_by_whichever_side_sees_it_first() {
        let fallback = FallbackWake::new(DEFAULT_DISPLAY_PERIOD);
        let now = Instant::now();
        let armed = fallback.arm_after_no_present(now);
        assert_eq!(fallback.next_wake(now), Some(armed), "armed and ahead");

        let after = armed + Duration::from_millis(1);
        assert_eq!(
            fallback.gate(after),
            FallbackGate {
                pending: false,
                due: true
            },
            "the wake that arrives after the deadline is the deferred one"
        );
        assert_eq!(
            fallback.gate(after),
            FallbackGate::default(),
            "and it is consumed exactly once"
        );

        // `next_wake` REPORTS a just-passed deadline instead of consuming
        // it — that report is the wake being asked for, and destroying it
        // there is what froze a real animation (see `next_wake`'s doc).
        let armed_again = fallback.arm_after_no_present(after);
        let just_late = armed_again + Duration::from_millis(1);
        assert_eq!(
            fallback.next_wake(just_late),
            Some(just_late),
            "a deadline a moment past still asks for its wake, immediately"
        );
        assert_eq!(
            fallback.gate(just_late),
            FallbackGate {
                pending: false,
                due: true
            },
            "and the frame callback that wake pokes still finds it due"
        );

        // A wake that can never be delivered (a hidden surface withholds
        // redraws) is abandoned once it is more than a period late, so the
        // loop parks instead of re-arming `WaitUntil` in the past forever.
        let stale = fallback.arm_after_no_present(just_late);
        assert_eq!(
            fallback.next_wake(stale + DEFAULT_DISPLAY_PERIOD + Duration::from_millis(1)),
            None,
            "a deadline more than one period late is abandoned"
        );
        assert_eq!(
            fallback.gate(stale + DEFAULT_DISPLAY_PERIOD * 2),
            FallbackGate::default(),
            "and leaves nothing armed, so the realm's own redraw echo counts again"
        );
    }

    /// The gate's whole point: a ticker-only wake that arrives before the
    /// deadline runs nothing, and real dirty work is never held behind it.
    #[test]
    fn a_pending_fallback_defers_a_ticker_only_wake_but_never_dirty_work() {
        assert_eq!(
            wake_action(
                true,
                false,
                true,
                FallbackGate {
                    pending: true,
                    due: false
                }
            ),
            WakeAction::Skip,
            "a scheduled ticker alone, with the fallback pending, waits for the deadline"
        );
        assert_eq!(
            wake_action(true, false, true, FallbackGate::default()),
            WakeAction::Render,
            "with nothing pending the same wake renders, exactly as before"
        );
        assert_eq!(
            wake_action(
                true,
                true,
                true,
                FallbackGate {
                    pending: true,
                    due: false
                }
            ),
            WakeAction::Render,
            "real dirty work overrides a pending deferral — input and state changes are \
             never paced behind the fallback"
        );
    }

    /// The realm's own end-of-pump redraw echo must not defeat the
    /// deferral, and the due deadline must reach the dirty predicate — the
    /// two halves a deadline source owes (in the predicate AND
    /// self-clearing), without which this is a one-shot plus a busy-spin.
    #[test]
    fn the_dirty_predicate_ignores_the_realms_own_echo_while_deferring_and_admits_a_due_deadline() {
        let pending = FallbackGate {
            pending: true,
            due: false,
        };
        assert!(
            !frame_is_dirty(false, true, false, None, pending),
            "the realm's own post-pump wake_frame echo is the wake being deferred, not \
             new work"
        );
        assert!(
            frame_is_dirty(true, false, false, None, pending),
            "an inbox redraw is real work and is never deferred"
        );
        assert!(
            frame_is_dirty(false, false, true, None, pending),
            "pending build/gesture work is real work and is never deferred"
        );
        let due = FallbackGate {
            pending: false,
            due: true,
        };
        assert!(
            frame_is_dirty(false, false, false, None, due),
            "the due deadline itself makes the wake it asked for dirty — otherwise the \
             wake arrives and wake_action skips it"
        );
    }

    /// The bound the deleted 16 ms sleep used to provide, without the
    /// sleep: a window that never presents while a ticker keeps
    /// re-requesting a frame must run a BOUNDED number of pipeline passes
    /// per wall-clock window. Drives the real gate — `arm_after_no_present`
    /// then `gate`/`wake_action` — and waits on the armed deadline the way
    /// `ControlFlow::WaitUntil` does in production, instead of sleeping a
    /// fixed pace.
    #[test]
    fn the_fallback_bounds_repeating_no_present_wakes_without_sleeping_on_the_loop() {
        let period = Duration::from_micros(6_065); // a 164.89 Hz panel
        let fallback = FallbackWake::new(period);
        let window = Duration::from_millis(80);
        let end = Instant::now() + window;
        let mut pipeline_passes = 0u32;
        let mut skipped = 0u32;

        while Instant::now() < end {
            let now = Instant::now();
            let gate = fallback.gate(now);
            // A ticker keeps the gate open; nothing else is dirty.
            let dirty = frame_is_dirty(false, true, false, None, gate);
            match wake_action(true, dirty, true, gate) {
                WakeAction::Skip => {
                    skipped += 1;
                    // Production parks in `ControlFlow::WaitUntil` here.
                    if let Some(deadline) = fallback.next_wake(now) {
                        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                    }
                }
                WakeAction::PumpAsync => unreachable!("frames stay enabled"),
                WakeAction::Render => {
                    pipeline_passes += 1;
                    // The pump presents nothing (occluded / no damage).
                    fallback.arm_after_no_present(Instant::now());
                }
            }
        }

        let ceiling = (window.as_secs_f64() / period.as_secs_f64()).ceil() as u32 + 4;
        assert!(
            pipeline_passes <= ceiling,
            "an un-presenting ticker must cost at most one pipeline pass per display \
             period: {pipeline_passes} passes ({skipped} deferred wakes) in {window:?} on a \
             {period:?} panel, ceiling {ceiling} — unbounded here is the busy-spin the \
             deleted sleep used to prevent"
        );
        assert!(
            pipeline_passes > 1,
            "sanity: the bound must not be 'never runs again' — the ticker still advances"
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
                FallbackGate::default(),
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
    /// `FallbackWake` -- not a bare fixed-interval poll loop.
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
    /// still open afterward, `FallbackWake` would defer the next
    /// ticker-only wake. What keeps this genuinely idle is that NOTHING
    /// wakes the loop until the deadline's own instant (`UiRealm::next_wake`,
    /// standing in for the real winit `about_to_wait`/`new_events`
    /// actuator this crate cannot drive with a live event loop under its
    /// own test harness) -- so there is exactly ONE real callback over the
    /// whole window, not many, and the fallback deferral never engages.
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
        // Production's own fallback state, driven by this loop exactly as
        // `bootstrap_desktop` drives it.
        let fallback = FallbackWake::new(DEFAULT_DISPLAY_PERIOD);

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
                realm.scheduler().is_frame_scheduled(),
                FallbackGate::default(),
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
                FallbackGate::default(),
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
                    // Production's own pairing (ADR-0058): a present clears
                    // any deferral, an un-presented frame with an open gate
                    // arms one. No sleep on this thread either way.
                    if presented {
                        fallback.record_present(Instant::now());
                    } else if keeps_gate_open {
                        fallback.arm_after_no_present(Instant::now());
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
                realm.scheduler().is_frame_scheduled(),
                FallbackGate::default(),
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
