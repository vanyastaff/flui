// ============================================================================
// Desktop frame-pacing gate
// ============================================================================
//
// Extracted as free functions — pure, no ui_runtime/window/GPU state — so the
// decisions each platform's frame callback makes each wake are unit
// testable without a live event loop. See the frame-pacing ADR for the
// full design: a PRESENTED frame is paced at display cadence by the
// backend's own pacing signal (the blocking Fifo present on X11 and Win32,
// the compositor's frame callback on Wayland,
// the display-pass cadence on native AppKit); these functions cover what
// happens on
// the frames that path never blocks: a spurious wake with nothing to do or
// a backgrounded app (`wake_action`), and a frame that ran the pipeline but
// never reached `present()` (`FallbackWake`, ADR-0353: a non-blocking wake
// one display period after the last present, never a sleep on the loop).

/// What a platform wake should do: run the full frame pipeline, service
/// background owner work, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WakeAction {
    /// Run the full frame pipeline — the pre-existing path, unchanged:
    /// frames are enabled and there is real work or a requested frame.
    Render,
    /// Frames are disabled (`AppLifecycleState::Hidden`/`Paused`/
    /// `Detached`): drain owner commands, poll ready async tasks, and service
    /// input deadlines through `UiRuntime::pump_background`. Visual work stays
    /// pending until frames re-enable; animation, build, layout, paint, and
    /// presentation do not run.
    PumpBackground,
    /// A spurious wake while frames are enabled: nothing dirty, no
    /// requested frame. No render, no pump, no sleep.
    Skip,
}

/// Decides what a platform wake should do, given the scheduler's
/// [`UpdateScheduler::frames_enabled`](flui_scheduler::UpdateScheduler::frames_enabled) fact (ADR-0035) alongside the pre-existing
/// dirty/frame-requested signals.
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
/// the global `UpdateScheduler` has a requested frame it has not begun yet:
/// any `UpdateScheduler::request_frame` or `FrameWaker::request_frame` sets it
/// (a task's waker, for one) and beginning the frame clears it. An animation
/// does not set it by itself: controllers tick through their presentation's
/// `Vsync`, and a pump that leaves one running re-requests a frame through
/// `wake_frame`, which sets `needs_redraw`.
pub(super) fn wake_action(
    frames_enabled: bool,
    dirty: bool,
    frame_scheduled: bool,
    fallback: FallbackGate,
) -> WakeAction {
    // A requested frame alone renders — unless a fallback wake is armed
    // and not yet due (ADR-0353): the previous pump ran the pipeline for
    // this request and presented nothing (no visible change in the ~0.2 ms
    // since the last present), so re-running it now would only repeat
    // that; the armed deadline brings the loop back exactly one display
    // period after the last present, and what `frame_is_dirty` counts as
    // dirty (`dirty`) is never held behind it.
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
/// Its sources, all required: `inbox_redraw` (a command drained this frame
/// boundary asked for a redraw), `needs_redraw` and `pending` (the UI runtime's
/// own dirty state, its tree, input and due work), the fallback, and `next_attempt_at.is_some()` —
/// an armed raster deadline of the window's own (`RasterSlot::frame_deadline`:
/// a device rebuild, or a restore of this window's surface the raster host
/// paces; the loop wakes for it while the window's frames run,
/// `RasterHost::wake_deadline`). Dropping that last term loses the
/// deadline: wired into the wake-deadline hook but invisible to this gate,
/// it reaches `WakeAction::Skip` and returns before the raster host's turn
/// (`RasterSlot::pump`) is ever reached, no matter how faithfully the
/// platform actuates the wake.
// Absent on wasm: its production callers are the desktop, Android and iOS
// frame closures, none of which exists there, and the web runner has no
// raster host deadline to fold in.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn frame_is_dirty(
    inbox_redraw: bool,
    needs_redraw: bool,
    pending: PendingWork,
    next_attempt_at: Option<web_time::Instant>,
    fallback: FallbackGate,
) -> bool {
    // A pending fallback means the last frame presented nothing: no present
    // blocked at vsync and no compositor paces the next one, so the next
    // frame waits for the fallback, about one display period away. That holds
    // for what every frame continues: the ui_runtime's OWN echo
    // (`needs_redraw`, which every pump with a running animation sets again
    // through `wake_frame`) and its input work (pointer motion to resample, a
    // gesture deadline still armed). It holds for tree work only when the
    // frame that armed the fallback left some (`holds_work`): a tree that
    // dirties itself each frame would otherwise run its frames at CPU speed
    // into a sink that shows none of them (a dead raster executor, an
    // occluded window), while tree work that arrives after a frame that left
    // none (a link opened, a tap's state change) runs at once. Due work (a
    // timer, a gesture deadline that came, a dropped frame's retry) runs on
    // the wake its deadline brings, as do inbox redraws and an armed recovery
    // attempt. The due fallback itself is the last term — a deadline source
    // has to be in the dirty predicate AND self-clearing, or its wake is a
    // no-op.
    let needs_redraw = needs_redraw && !fallback.pending;
    let input = pending.input && !fallback.pending;
    let tree = pending.tree && !(fallback.pending && fallback.holds_work);
    inbox_redraw
        || needs_redraw
        || input
        || tree
        || pending.due
        || next_attempt_at.is_some()
        || fallback.due
}

/// A UI runtime's pending work as the frame gate weighs it: its tree work
/// (`UiRuntime::has_pending_tree_work`), which a frame that showed nothing
/// and left some paces; its input work (`UiRuntime::has_pending_input_work`),
/// which every frame continues and none owes before the next period; and its
/// due work (`UiRuntime::has_pending_due_work`), which never waits behind a
/// pacing deferral.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct PendingWork {
    pub(super) tree: bool,
    pub(super) input: bool,
    pub(super) due: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl PendingWork {
    /// What `ui_runtime` has pending now. Input work is read before due work,
    /// so a gesture deadline that comes between the reads is counted at least
    /// once.
    pub(super) fn of(ui_runtime: &crate::app::ui_runtime::UiRuntime) -> Self {
        Self {
            tree: ui_runtime.has_pending_tree_work(),
            input: ui_runtime.has_pending_input_work(),
            due: ui_runtime.has_pending_due_work(),
        }
    }
}

/// Whether another frame will be requested regardless of this one's
/// outcome: `needs_redraw`, a requested frame, or dirty
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

/// What a desktop window's pacing does after its pump.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AfterPump {
    /// The frame presented: the first reveals the window, and the fallback
    /// stands down.
    Presented,
    /// A raster thread holds the frame: its settlement wakes the window and
    /// paces it, so neither the first reveal's fallback nor the pacing
    /// fallback arms, and a slow first render reveals nothing blank.
    Held,
    /// Nothing presented: the first reveal counts the frame toward its
    /// fallback, and the pacing fallback arms when `defer`, the gate staying
    /// open.
    NotPresented { defer: bool },
}

/// The pacing after a pump that `presented`, with a raster thread `holding`
/// the window's frames and the gate kept open (`keeps_gate_open`).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn after_pump(presented: bool, holding: bool, keeps_gate_open: bool) -> AfterPump {
    if presented {
        AfterPump::Presented
    } else if holding {
        AfterPump::Held
    } else {
        AfterPump::NotPresented {
            defer: keeps_gate_open,
        }
    }
}

/// The pace a BACKGROUNDED pump (frames disabled) is bounded to on Android,
/// armed as a deadline ([`BackgroundPace`]). iOS needs none: its display link
/// runs the pump at most once per refresh and not at all while held
/// (ADR-0268). No runner sleeps on its loop thread (ADR-0353,
/// [`FallbackWake`]).
#[cfg(any(target_os = "android", test))]
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
/// ADR-0353 carries the captures behind it.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
const FALLBACK_PERIOD_FRACTION: f64 = 0.95;

/// What a wake sees of the fallback deadline: armed and not yet due (the
/// wake with no dirty work is deferred), or due (this wake IS the deferred one).
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
    /// A pending deadline was armed by a frame that left its tree dirty
    /// (`UiRuntime::has_pending_tree_work`): that tree work waits for the deadline too,
    /// since a tree that dirties itself every frame would otherwise run its
    /// frames at CPU speed into a sink that shows none of them.
    pub(super) holds_work: bool,
}

/// A window's non-blocking bound on animation-driven wakes that present
/// nothing (ADR-0353). Replaces the fixed 16 ms `thread::sleep` on the
/// event-loop thread that used to play this role: that sleep was the only
/// pacer on any stack whose present does not block at vsync, quantized a
/// 165 Hz panel to ~60 frames per second, and blocked input for its whole
/// duration every cycle.
///
/// The bound is a deadline, not a sleep: after a pump that ran the
/// pipeline for a requested frame and presented nothing, the next produce
/// is deferred to `last_present + FALLBACK_PERIOD_FRACTION × period` via
/// the platform's wake-deadline hook (`ControlFlow::WaitUntil`), the loop
/// stays responsive to input meanwhile, and the deadline is one-shot:
/// consumed by the wake it brings, or cleared once it has passed without
/// one (a hidden Wayland surface never delivers that wake — its redraw is
/// withheld until the compositor resumes — and a past deadline handed back
/// to the loop's wake query again and again would be the busy-spin ADR-0044 §7
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
    /// Whether the frame that armed the deadline left work dirty behind it
    /// ([`FallbackGate::holds_work`]).
    holds_work: bool,
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
                holds_work: false,
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
                holds_work: state.holds_work,
            },
            Some(_) => {
                state.deadline = None;
                state.delivered_at = None;
                state.holds_work = false;
                FallbackGate {
                    pending: false,
                    due: true,
                    holds_work: false,
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
        state.holds_work = false;
    }

    /// A pump ran and presented nothing while a requested frame keeps the gate open:
    /// defer the next wake with no dirty work to one (fractional) display period
    /// after the last present — or after `now`, when nothing has presented
    /// yet or the last present is already further back than that (an
    /// occluded window keeps a bounded, non-blocking cadence). Returns the
    /// armed instant. `period` is the presentation's current refresh interval;
    /// `left_work` is whether the pump left its tree dirty, whose work then
    /// waits for the deadline too.
    pub(super) fn arm_after_no_present(
        &self,
        now: web_time::Instant,
        period: std::time::Duration,
        left_work: bool,
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
        state.holds_work = left_work;
        anchored
    }

    /// A pump ran, presented nothing and left nothing pending (the gate is
    /// closed, so no deadline is armed for it): a pending deadline no longer
    /// holds tree work. The deadline stays, and tree work that arrives
    /// after this frame is new and runs at once.
    pub(super) fn release_held_work(&self) {
        self.state.lock().holds_work = false;
    }

    /// The instant the platform's wake-deadline hook should wake the loop
    /// at while a deferral is armed. `period` is the presentation's current
    /// refresh interval, which bounds how late a deadline may be and still be
    /// answered.
    ///
    /// **This query never consumes the deadline** — [`Self::gate`], called
    /// from the frame callback, is the sole consumer, and splitting those
    /// two roles is not a style choice: the loop's wake query and the frame
    /// callback observe the same wake, and the loop queries the deadline on
    /// the very iteration it expires, BEFORE the redraw its poke queues has
    /// been dispatched. A version of
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
    /// never handed that wake to answer. The frame request's demand is retained on
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
/// - **A requested frame keeps the gate open**, and a pending fallback defers a
///   wake that carries no dirty work, but never what the dirty predicate counts: pinned by
///   `a_pending_fallback_defers_a_wake_with_no_dirty_work_but_never_dirty_work`.
/// - **No-present fallback deadline** (ADR-0353's non-blocking deadline):
///   `the_fallback_deadline_is_anchored_to_the_last_present` pins where
///   the deadline sits, and
///   `leftover_work_after_a_frame_that_presented_nothing_waits_for_the_fallback`
///   that the build and layout work such a frame leaves waits for it too.
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
            "a spurious wake with frames enabled, nothing dirty, and no requested frame must \
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
        let armed = fallback.arm_after_no_present(pump_at, period, false);

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
        let deadline = fallback.arm_after_no_present(armed_at, Duration::from_millis(16), false);

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
        let deadline = fallback.arm_after_no_present(Instant::now(), period, false);
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

    /// The gate's whole point: a wake with no dirty work that arrives before the
    /// deadline runs nothing, and what the dirty predicate counts is never held behind it.
    fn a_pending_fallback_defers_a_wake_with_no_dirty_work_but_never_dirty_work() {
        assert_eq!(
            wake_action(
                true,
                false,
                true,
                FallbackGate {
                    pending: true,
                    due: false,
                    holds_work: false,
                }
            ),
            WakeAction::Skip,
            "a requested frame alone, with the fallback pending, waits for the deadline"
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
                    due: false,
                    holds_work: false,
                }
            ),
            WakeAction::Render,
            "work the dirty predicate counts (an inbox redraw, a due raster attempt) overrides \
             a pending deferral"
        );
    }

    /// A frame a raster thread holds is paced by its settlement: no fallback
    /// of either kind arms for it, whatever the gate, while a frame that
    /// presented or presented nothing paces as before.
    ///
    /// Red-check: answer `NotPresented` while held and the first reveal arms
    /// its one-second fallback under a render still on the thread.
    fn a_held_frame_arms_no_fallback() {
        use super::{AfterPump, after_pump};
        for (presented, holding, open, expected) in [
            (true, false, true, AfterPump::Presented),
            (true, true, true, AfterPump::Presented),
            (false, true, true, AfterPump::Held),
            (false, true, false, AfterPump::Held),
            (false, false, true, AfterPump::NotPresented { defer: true }),
            (
                false,
                false,
                false,
                AfterPump::NotPresented { defer: false },
            ),
        ] {
            assert_eq!(
                after_pump(presented, holding, open),
                expected,
                "presented {presented}, holding {holding}, open {open}"
            );
        }
    }

    /// A frame that presented nothing and left its tree dirty paces the next
    /// one at the fallback: a tree that dirties itself every frame into a
    /// sink that shows nothing (a dead raster executor, an occluded window)
    /// runs one frame per fallback, not one per wake. Input work (pointer
    /// motion, an armed gesture deadline) waits for any pending fallback. Due
    /// work (a timer, a gesture deadline that came, a dropped frame's retry)
    /// runs on its own wake; tree work that arrives after a frame that left
    /// none runs at once, as do an inbox redraw and an armed raster attempt;
    /// and the due fallback runs the frame.
    ///
    /// Red-check: count tree work past a pending fallback that holds work and
    /// the leftover work is dirty on every wake; count input work and a held
    /// press renders on every wake; mask due work and a timer waits.
    fn leftover_work_after_a_frame_that_presented_nothing_waits_for_the_fallback() {
        use super::{PendingWork, frame_is_dirty};
        let tree = PendingWork {
            tree: true,
            ..PendingWork::default()
        };
        let input = PendingWork {
            input: true,
            ..PendingWork::default()
        };
        let due_work = PendingWork {
            due: true,
            ..PendingWork::default()
        };
        let none = PendingWork::default();
        let fallback = FallbackWake::new();
        let period = Duration::from_millis(16);
        let now = Instant::now();
        let _armed = fallback.arm_after_no_present(now, period, true);
        let holding = fallback.gate(now);
        assert!(
            holding.holds_work,
            "the arm records the tree the frame left"
        );
        assert!(
            !frame_is_dirty(false, true, tree, None, holding),
            "the echo and the leftover tree work wait for the fallback"
        );
        assert!(
            frame_is_dirty(false, false, due_work, None, holding),
            "due work runs on its own wake"
        );
        assert!(
            frame_is_dirty(true, false, none, None, holding),
            "an inbox redraw does not wait"
        );
        assert!(
            frame_is_dirty(false, false, none, Some(now), holding),
            "nor does an armed raster attempt"
        );
        fallback.release_held_work();
        let released = fallback.gate(now);
        assert!(
            released.pending && !released.holds_work,
            "a frame that left nothing pending releases the hold; the deadline stays"
        );
        assert!(
            frame_is_dirty(false, false, tree, None, released),
            "and tree work that arrives after it runs at once"
        );
        let due = fallback.gate(now + period);
        assert!(
            frame_is_dirty(false, false, tree, None, due),
            "the due fallback runs the frame"
        );
        let _armed = fallback.arm_after_no_present(now + period, period, false);
        let pressed = fallback.gate(now + period);
        assert!(
            !frame_is_dirty(false, true, input, None, pressed),
            "a held press waits for the fallback even when the frame left no tree work"
        );
        assert!(
            frame_is_dirty(false, true, tree, None, pressed),
            "tree work that arrives after a frame that left none runs at once"
        );
        assert!(
            frame_is_dirty(false, false, tree, None, FallbackGate::default()),
            "with no fallback armed, tree work runs at once"
        );
        assert!(
            frame_is_dirty(false, false, input, None, FallbackGate::default()),
            "and so does input work"
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
                    "a_pending_fallback_defers_a_wake_with_no_dirty_work_but_never_dirty_work",
                    a_pending_fallback_defers_a_wake_with_no_dirty_work_but_never_dirty_work
                        as fn(),
                ),
                (
                    "a_held_frame_arms_no_fallback",
                    a_held_frame_arms_no_fallback as fn(),
                ),
                (
                    "leftover_work_after_a_frame_that_presented_nothing_waits_for_the_fallback",
                    leftover_work_after_a_frame_that_presented_nothing_waits_for_the_fallback
                        as fn(),
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
