// ============================================================================
// Desktop frame-pacing gate
// ============================================================================
//
// Extracted as free functions — pure, no ui_runtime/window/GPU state — so the
// decisions each platform's frame callback makes each wake are unit
// testable without a live event loop. See the frame-pacing ADR for the
// full design: a PRESENTED frame is paced at display cadence by the
// backend's own present path (the blocking Fifo present on Vulkan/Wayland,
// the display-pass cadence on native AppKit); these functions cover what
// happens on
// the frames that path never blocks: a spurious wake with nothing to do or
// a backgrounded app (`wake_action`), and a frame that ran the pipeline but
// never reached `present()` (`FallbackWake`, ADR-0058: a non-blocking wake
// one display period after the last present, never a sleep on the loop).

/// Hands the window's frame-pacing signal to the raster backend: the
/// backend calls the surface target's
/// [`pre_present_notify`](flui_platform::SurfaceTarget::pre_present_notify)
/// immediately before every present, and only then. On Wayland this is what makes a running
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
    surface: std::sync::Arc<dyn flui_platform::SurfaceTarget>,
) {
    backend.set_pre_present_hook(Some(Box::new(move || surface.pre_present_notify())));
}

/// What a platform wake should do: run the full frame pipeline, service
/// background owner work, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WakeAction {
    /// Run the full frame pipeline — the pre-existing path, unchanged:
    /// frames are enabled and there is real work or a scheduled ticker.
    Render,
    /// Frames are disabled (`AppLifecycleState::Hidden`/`Paused`/
    /// `Detached`): drain owner commands, poll ready async tasks, and service
    /// input deadlines through `UiRuntime::pump_background`. Visual work stays
    /// pending until frames re-enable; animation, build, layout, paint, and
    /// presentation do not run.
    PumpBackground,
    /// A spurious wake while frames are enabled: nothing dirty, no
    /// scheduled ticker. No render, no pump, no sleep.
    Skip,
}

/// Decides what a platform wake should do, given the scheduler's
/// [`UpdateScheduler::frames_enabled`](flui_scheduler::UpdateScheduler::frames_enabled) fact (ADR-0035) alongside the pre-existing
/// dirty/scheduled-ticker signals.
///
/// `frames_enabled == false` takes priority over everything else — even
/// with visual `dirty` work pending, a backgrounded app services owner commands,
/// async tasks, and input deadlines without running a visual frame. The
/// mid-frame owner-task poll inside `handle_begin_frame` does not run in
/// `PumpBackground` mode, so these wakes keep background futures and due input
/// work progressing.
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
    // A scheduled ticker alone renders — unless a fallback wake is armed
    // and not yet due (ADR-0058): the previous pump ran the pipeline for
    // this ticker and presented nothing (no visible change in the ~0.2 ms
    // since the last present), so re-running it now would only repeat
    // that; the armed deadline brings the loop back exactly one display
    // period after the last present, and real dirty work (`dirty`) is
    // never held behind it.
    let action = if !frames_enabled {
        WakeAction::PumpBackground
    } else if dirty || (frame_scheduled && !fallback.pending) {
        WakeAction::Render
    } else {
        WakeAction::Skip
    };

    // Every platform wake resolves here, and `Render` is the only action
    // that leaves any other trace of itself — so without this line a loop
    // that stopped and a platform that stopped delivering wakes are
    // indistinguishable in the logs, and the two inputs that decide `Skip`
    // are exactly what tells them apart. `trace`, not `debug`: this fires
    // once per platform wake, which on a healthy loop is the display rate.
    tracing::trace!(
        target: "flui.pace",
        event = "wake_resolved",
        action = ?action,
        frames_enabled,
        dirty,
        frame_scheduled,
        fallback_pending = fallback.pending,
        "platform wake resolved"
    );

    action
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
/// UI runtime's own pre-existing dirty state), and `next_attempt_at.is_some()` —
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
    // `needs_redraw` is also the ui_runtime's OWN echo: every pump with a
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
#[cfg(not(any(target_os = "ios", target_arch = "wasm32")))]
pub(super) fn keeps_frame_gate_open(
    needs_redraw: bool,
    frame_scheduled: bool,
    has_pending_work: bool,
) -> bool {
    needs_redraw || frame_scheduled || has_pending_work
}

/// The pace a BACKGROUNDED pump (frames disabled) is bounded to on the mobile
/// backends. Android arms it as a deadline ([`BackgroundPace`]); iOS, whose
/// frame source has no wake-deadline hook yet, still sleeps it in
/// `bootstrap_ios`'s `PumpBackground` arm. Desktop does not sleep on the loop
/// thread at all (ADR-0058, [`FallbackWake`]).
#[cfg(any(target_os = "android", target_os = "ios", test))]
pub(super) const BACKGROUNDED_PUMP_PACE: std::time::Duration = std::time::Duration::from_millis(16);

/// The bound on backgrounded pumps (frames disabled) as a deadline instead of
/// a sleep on the loop thread, for a backend whose wake-deadline hook brings
/// the loop back: a pump that leaves work asking for another wake defers the
/// next pump by [`BACKGROUNDED_PUMP_PACE`], and the loop keeps delivering
/// input and lifecycle events meanwhile.
///
/// One-shot, like [`FallbackWake`]: the wake the deadline brings consumes it,
/// so a deadline source that is never served cannot keep reporting a past
/// instant.
#[cfg(any(target_os = "android", test))]
#[derive(Debug, Default)]
pub(super) struct BackgroundPace {
    deadline: parking_lot::Mutex<Option<web_time::Instant>>,
}

#[cfg(any(target_os = "android", test))]
impl BackgroundPace {
    /// Whether a wake at `now` may pump: not while an armed deadline lies
    /// ahead. A deadline that has passed is consumed by the wake reading it.
    pub(super) fn admit(&self, now: web_time::Instant) -> bool {
        let mut deadline = self.deadline.lock();
        match *deadline {
            Some(at) if now < at => false,
            Some(_) => {
                *deadline = None;
                true
            }
            None => true,
        }
    }

    /// A pump at `now` left work that asks for another wake: the next pump
    /// waits until one pace later.
    pub(super) fn defer(&self, now: web_time::Instant) {
        *self.deadline.lock() = Some(now + BACKGROUNDED_PUMP_PACE);
    }

    /// Frames are enabled again: no pump is deferred any more, and a deadline
    /// left armed would be reported as due on every wake with nothing to
    /// consume it.
    pub(super) fn clear(&self) {
        *self.deadline.lock() = None;
    }

    /// The armed deadline, for the wake-deadline hook.
    pub(super) fn next_wake(&self) -> Option<web_time::Instant> {
        *self.deadline.lock()
    }
}

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
///
/// Not `cfg`-gated, unlike `FallbackWake` itself: every backend's
/// `wake_action` call passes one, and the web runner passes
/// `FallbackGate::default()` because the browser's `requestAnimationFrame`
/// loop already paces it. A gated type here would make the shared predicate
/// take a different shape per target — the thing that let a signature change
/// break the wasm build while the native gate stayed green.
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
    last_present_at: Option<web_time::Instant>,
    deadline: Option<web_time::Instant>,
    /// When the loop was first told the armed deadline is due (`next_wake`
    /// answered `Some(now)` for it), so the wake can be given up on only once
    /// the platform has had a period to answer it.
    delivered_at: Option<web_time::Instant>,
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl FallbackWake {
    /// A fallback wake with nothing armed.
    ///
    /// It keeps no display period of its own: every arm and every deadline
    /// query is handed the presentation's current refresh interval, so a window
    /// that changes display mode, moves to another display, or lands on one that
    /// reports nothing paces by the new value from the next arm, not only
    /// after a resize.
    pub(super) fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(FallbackState {
                last_present_at: None,
                deadline: None,
                delivered_at: None,
            }),
        }
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
                state.delivered_at = None;
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
        state.delivered_at = None;
    }

    /// A pump ran and presented nothing while a ticker keeps the gate open:
    /// defer the next ticker-only wake to one (fractional) display period
    /// after the last present — or after `now`, when nothing has presented
    /// yet or the last present is already further back than that (an
    /// occluded window keeps a bounded, non-blocking cadence). Returns the
    /// armed instant. `period` is the presentation's current refresh interval.
    pub(super) fn arm_after_no_present(
        &self,
        now: web_time::Instant,
        period: std::time::Duration,
    ) -> web_time::Instant {
        let mut state = self.state.lock();
        let interval = period.mul_f64(FALLBACK_PERIOD_FRACTION);
        let anchored = state
            .last_present_at
            .map(|at| at + interval)
            .filter(|deadline| *deadline > now)
            .unwrap_or(now + interval);
        state.deadline = Some(anchored);
        state.delivered_at = None;
        anchored
    }

    /// The instant the platform's wake-deadline hook should wake the loop
    /// at while a deferral is armed. `period` is the presentation's current
    /// refresh interval, which bounds how late a deadline may be and still be
    /// answered.
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
    /// pending deferral suppresses the UI runtime's own redraw echo — nothing
    /// woke the loop again. Measured on a real 164.89 Hz X11 session: the
    /// animation froze mid-flight, `next_wake` observing the deadline 5 µs
    /// late, and stayed frozen for the rest of the run.
    ///
    /// A deadline already behind `now` is therefore still reported, the first
    /// time as `now` (so `WaitUntil` fires immediately) — that IS the wake
    /// being asked for, and `gate` consumes it exactly once when the frame
    /// callback it pokes finally runs. Until it does, each further query
    /// reports the instant a full period after that first report, a future
    /// one, so a poke that is slow to dispatch costs one wake and not a spin.
    /// The one case where the poke can never be answered is a surface whose
    /// redraws the compositor withholds (a hidden Wayland window): once that
    /// period passes the deadline is abandoned, which bounds the case at one
    /// wake and leaves the presentation idle, as a hidden presentation should
    /// be. The period runs from the first report, not from the deadline: a
    /// loop that was busy elsewhere (another window blocked in `present`)
    /// reads a deadline late the first time it looks, and the platform was
    /// never handed that wake to answer. The ticker's demand is retained on
    /// the clock either way, so the next delivered redraw produces.
    pub(super) fn next_wake(
        &self,
        now: web_time::Instant,
        period: std::time::Duration,
    ) -> Option<web_time::Instant> {
        let mut state = self.state.lock();
        let deadline = state.deadline?;
        if deadline > now {
            return Some(deadline);
        }
        let Some(delivered_at) = state.delivered_at else {
            state.delivered_at = Some(now);
            return Some(now);
        };
        // Reported already: the platform has a period to answer it, and the
        // loop waits for that period, not for `now` over and over.
        let expiry = delivered_at + period;
        if expiry > now {
            return Some(expiry);
        }
        // The wake this deadline asked for is not coming (a hidden surface
        // withholds redraws). Abandon it rather than re-arming `WaitUntil`
        // in the past every iteration — ADR-0044 §7's measured busy-spin.
        // Clearing it also lifts the suppression of the ui_runtime's own
        // redraw echo, so an ordinary wake produces.
        let unanswered = now.saturating_duration_since(delivered_at);
        state.deadline = None;
        state.delivered_at = None;
        tracing::trace!(
            target: "flui.pace",
            event = "fallback_abandoned",
            unanswered_us = unanswered.as_micros() as u64,
            "the deferred wake was never delivered; abandoning the deadline"
        );
        None
    }
}

/// Frame-pacing gate tests.
///
/// `run_desktop` itself opens a real window and GPU device, so it cannot
/// run headlessly; `wake_action` and `FallbackWake` were pulled
/// out specifically so the decisions the frame callback makes each wake are
/// covered here without one. Coverage map for the four invariants the
/// frame-pacing ADR calls out:
///
/// - **Idle = zero frames**: pinned by
///   `idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips`
///   below.
/// - **Ticker keeps the gate open**, and a pending fallback defers a
///   ticker-only wake but never dirty work: pinned by
///   `a_pending_fallback_defers_a_ticker_only_wake_but_never_dirty_work`.
/// - **No-present fallback deadline** (ADR-0058's non-blocking deadline):
///   `the_fallback_deadline_is_anchored_to_the_last_present` pins where
///   the deadline sits; no test bounds the pipeline passes over repeated
///   no-present wakes.
/// - **Wake coalescing** (N `wake_frame` calls -> one draw) and the
///   pending-work leg of `keeps_frame_gate_open` have no test here.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod desktop_pacing_tests {

    use std::time::{Duration, Instant};

    use super::{FallbackGate, FallbackWake, WakeAction, wake_action};

    fn idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips() {
        assert_eq!(
            wake_action(true, false, false, FallbackGate::default()),
            WakeAction::Skip,
            "a spurious wake with frames enabled, nothing dirty, and no scheduled ticker must \
             render zero frames"
        );
    }

    /// The deferral is anchored to the LAST PRESENT, not to the pump that
    /// found nothing to present. Anchoring to `now` instead would add the
    /// pump's own duration to every period — a slow, cumulative slide
    /// against the display that shows up as a periodic dropped frame while
    /// every median still reads healthy.
    fn the_fallback_deadline_is_anchored_to_the_last_present() {
        let period = Duration::from_micros(6_065); // a 164.89 Hz panel
        let fallback = FallbackWake::new();
        let present_at = Instant::now();
        fallback.record_present(present_at);

        // The pump that presents nothing runs 1 ms after the present.
        let pump_at = present_at + Duration::from_millis(1);
        let armed = fallback.arm_after_no_present(pump_at, period);

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

    /// A deadline that came due is abandoned only once the loop has been told
    /// so for a full period without the wake arriving, where the period is the
    /// one the presentation has when the loop asks: a display that got faster
    /// since the deadline was armed makes the same wait a lost wake.
    fn an_unanswered_deadline_is_abandoned_by_the_period_in_force_when_asked() {
        let fallback = FallbackWake::new();
        let armed_at = Instant::now();
        let deadline = fallback.arm_after_no_present(armed_at, Duration::from_millis(16));

        assert_eq!(
            fallback.next_wake(deadline, Duration::from_millis(16)),
            Some(deadline),
            "reported due"
        );
        let ten_ms_later = deadline + Duration::from_millis(10);
        assert_eq!(
            fallback.next_wake(ten_ms_later, Duration::from_millis(16)),
            Some(deadline + Duration::from_millis(16)),
            "10 ms unanswered on a 16 ms display is a wake still worth waiting for, until a \
             future instant and not a spin on `now`"
        );
        assert_eq!(
            fallback.next_wake(ten_ms_later, Duration::from_millis(8)),
            None,
            "10 ms unanswered on an 8 ms display is a wake that is not coming"
        );
    }

    /// A loop that was busy elsewhere (another window blocked in `present`)
    /// reads the deadline more than a period after it fell due. The platform
    /// was never handed that wake, so the first report is still a wake to
    /// answer: abandoning it parks a window whose animation then freezes.
    fn a_deadline_first_read_late_is_still_reported_due() {
        let fallback = FallbackWake::new();
        let period = Duration::from_millis(7);
        let deadline = fallback.arm_after_no_present(Instant::now(), period);
        let two_periods_late = deadline + period * 2;

        assert_eq!(
            fallback.next_wake(two_periods_late, period),
            Some(two_periods_late),
            "the first report of a due deadline is answered however late the loop looked"
        );
        assert_eq!(
            fallback.next_wake(two_periods_late + period / 2, period),
            Some(two_periods_late + period),
            "a further query waits out the period; it does not report `now` again"
        );
        assert_eq!(
            fallback.next_wake(two_periods_late + period * 2, period),
            None,
            "and abandoned only when that report went unanswered for a period"
        );
    }

    /// The gate's whole point: a ticker-only wake that arrives before the
    /// deadline runs nothing, and real dirty work is never held behind it.
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

    #[test]
    fn desktop_pacing_matrix() {
        crate::table_test::run_table(
            "desktop_pacing_matrix",
            &[
                (
                    "idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips",
                    idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips as fn(),
                ),
                (
                    "the_fallback_deadline_is_anchored_to_the_last_present",
                    the_fallback_deadline_is_anchored_to_the_last_present as fn(),
                ),
                (
                    "an_unanswered_deadline_is_abandoned_by_the_period_in_force_when_asked",
                    an_unanswered_deadline_is_abandoned_by_the_period_in_force_when_asked as fn(),
                ),
                (
                    "a_deadline_first_read_late_is_still_reported_due",
                    a_deadline_first_read_late_is_still_reported_due as fn(),
                ),
                (
                    "a_pending_fallback_defers_a_ticker_only_wake_but_never_dirty_work",
                    a_pending_fallback_defers_a_ticker_only_wake_but_never_dirty_work as fn(),
                ),
            ],
        );
    }
}

#[cfg(test)]
mod background_pace_tests {
    use web_time::Instant;

    use super::{BACKGROUNDED_PUMP_PACE, BackgroundPace};

    /// A backgrounded pump that leaves work defers the next one by a pace
    /// instead of sleeping, and the wake the deadline brings consumes it.
    #[test]
    fn a_background_pump_defers_the_next_by_one_pace() {
        let pace = BackgroundPace::default();
        let start = Instant::now();
        assert!(pace.admit(start), "nothing armed: the first pump runs");
        assert_eq!(pace.next_wake(), None);

        pace.defer(start);
        let deadline = start + BACKGROUNDED_PUMP_PACE;
        assert_eq!(
            pace.next_wake(),
            Some(deadline),
            "the hook reports the pace"
        );
        assert!(
            !pace.admit(start + BACKGROUNDED_PUMP_PACE / 2),
            "a wake before the pace does not pump"
        );
        assert_eq!(
            pace.next_wake(),
            Some(deadline),
            "and leaves the deadline armed"
        );

        assert!(pace.admit(deadline), "the wake the deadline brings pumps");
        assert_eq!(
            pace.next_wake(),
            None,
            "and consumes the deadline, so a past instant is never reported again"
        );

        pace.defer(deadline);
        pace.clear();
        assert_eq!(pace.next_wake(), None, "frames enabled again drop the pace");
        assert!(pace.admit(deadline));
    }
}
