//! Wake-deadline actuation for the AppKit backend (ADR-0058 decision 2).
//!
//! # Why this module exists
//!
//! `install_wake_deadline_hook` publishes the earliest wall-clock instant the
//! event loop should come back at. `winit` hands that to
//! `ControlFlow::WaitUntil` and the Android backend checks it once per
//! iteration of its own loop. AppKit has neither: `[NSApplication run]` is an
//! opaque, unbounded run loop, so its default `set_wake_deadline_hook` (a
//! no-op) dropped the deadline on the floor and the process parked in
//! `_nextEventMatchingEventMask:untilDate:` with the hook never consulted.
//! This module is that deadline's landing place.
//!
//! # The two things that make the shape what it is
//!
//! 1. **The hook re-enters `flui-app` and `APP_RUNTIME` is thread-local**, so
//!    it may only be consulted on the owner (main) thread — which rules out a
//!    background deadline thread and picks the main queue as the actuator.
//!    [`owner_queue`] is that queue and `exec_after` schedules onto it without
//!    an Objective-C target class, so `[NSApp run]` keeps draining it
//!    unchanged.
//! 2. **A frame arms its fallback deadline *after* the redraw request that
//!    triggered it**, so a consult made at the request itself is always too
//!    early to see the deadline that frame is about to arm. Hence
//!    [`WakePump::arm`] schedules a second look ([`FOLLOW_UP`]) when the first
//!    consult finds nothing — that second look is what picks the deadline up.
//!
//! The chain is self-limiting, which is what keeps it from being a busy poll:
//! a firing that finds no deadline *and* requested no frame stops, and an idle
//! app sends no redraw requests, so nothing is ever scheduled for it.

use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;

use super::owner_lane::owner_queue;
use crate::shared::PlatformHandlers;

/// How long after a redraw request the pump looks at the hook a second time
/// when the first look found no deadline.
///
/// One pacing period is the natural scale: the frame a redraw request
/// triggers runs within the pass AppKit schedules for it, and the fallback
/// gate's own deadline is shorter than a period on every display measured so
/// far (9.5 ms against a 100 Hz panel). A fixed delay rather than
/// `refresh_period()` because the pump is per-*platform* while the period is
/// per-*window*, and the deadline it reads back is absolute — so being a few
/// milliseconds off costs a re-schedule, never a missed or an early wake.
const FOLLOW_UP: Duration = Duration::from_millis(8);

/// The AppKit landing place for the platform's wake deadline.
///
/// Held as `Arc` by [`super::MacOSPlatform`] and as `Weak` by each
/// [`super::MacOSWindow`], which arms it from `request_redraw`.
pub(crate) struct WakePump {
    /// Consulted through [`Self::consult`], which clones the hook out of the
    /// lock and calls it with the lock released — the discipline
    /// [`PlatformHandlers::wake_deadline`] documents for this field.
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// Asks this platform's windows for a frame. Set once at construction;
    /// never read under a lock (see [`Self::fire`]).
    wake: Box<dyn Fn() + Send + Sync>,

    /// Supersedes stale ticks. An `arm` or a firing takes the next
    /// generation, and a tick whose generation is no longer current returns
    /// without acting.
    generation: AtomicU64,

    /// The instant the scheduled tick will run at, as microseconds since
    /// [`base`], or `0` when none is scheduled. Read by [`Self::arm`] to
    /// refuse to move a tick LATER — the rule that keeps a redraw burst from
    /// starving the pump (see `arm`).
    ///
    /// A plain atomic rather than a `Mutex<Option<Instant>>`: the value is
    /// read on every redraw request and written on every schedule, and an
    /// atomic keeps both off the lock-discipline surface
    /// (no guard whose drop ordering has to be gotten right) for a value
    /// that carries no
    /// invariant beyond itself.
    pending_at: AtomicU64,
}

/// The epoch [`WakePump::pending_at`] counts from.
///
/// `web_time::Instant` has no representation as an integer and no
/// `from_micros`, so the deadline is stored as an offset from a process-wide
/// base fixed at first use. Nothing ever compares these offsets across
/// processes, so the base only has to be stable within one.
fn base() -> web_time::Instant {
    static BASE: std::sync::OnceLock<web_time::Instant> = std::sync::OnceLock::new();
    *BASE.get_or_init(web_time::Instant::now)
}

/// Microseconds from [`base`] to `instant`, for [`WakePump::pending_at`].
fn micros_since_base(instant: web_time::Instant) -> u64 {
    instant.saturating_duration_since(base()).as_micros() as u64
}

impl WakePump {
    /// Build the pump. `wake` is invoked on the owner thread when a deadline
    /// has come due; it must be safe to call from there (see
    /// [`super::MacOSPlatform`]'s construction site).
    pub(crate) fn new(
        handlers: Arc<Mutex<PlatformHandlers>>,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            handlers,
            wake,
            generation: AtomicU64::new(0),
            pending_at: AtomicU64::new(0),
        })
    }

    /// Ask the pump to make the loop come back. Called from `request_redraw`
    /// (and once from `set_wake_deadline_hook`), i.e. whenever a frame is
    /// about to happen.
    pub(crate) fn arm(self: &Arc<Self>) {
        let now = web_time::Instant::now();
        // No deadline yet is the common case at a redraw request and is NOT
        // "nothing to do": the frame this request triggers has not run yet,
        // so the deadline it will arm is not visible here. Take the second
        // look; a firing that still finds nothing stops the chain.
        let target = self.consult().unwrap_or(now + FOLLOW_UP);

        // Only ever move the tick EARLIER. Re-scheduling unconditionally
        // looks harmless — the answer is absolute, so a second look at the
        // same deadline computes the same instant — but it is not: while a
        // frame is being asked for over and over (the redraw burst a cold
        // start produces), each request would take a new generation and
        // cancel the tick the previous one scheduled, so no tick would ever
        // run and the pump would be starved by exactly the traffic it exists
        // to survive. Measured: 241 arms, 0 firings, 238 superseded — the
        // deadline armed mid-burst was never consulted at all. Leaving an
        // already-earlier tick alone is what makes the burst harmless.
        let pending = self.pending_at.load(Ordering::Acquire);
        if pending != 0 && base() + Duration::from_micros(pending) <= target {
            return;
        }

        tracing::trace!(
            target: "flui.pace",
            event = "wake_pump_armed",
            delay_us = target.saturating_duration_since(now).as_micros() as u64,
            "AppKit wake pump armed"
        );
        self.schedule(target);
    }

    /// Consult the installed hook, with the platform's handler lock released.
    ///
    /// The `Arc` is cloned out first and called after the guard drops, because
    /// the hook walks every hosted realm and takes gesture-arena locks — the
    /// exact re-entrancy [`PlatformHandlers::wake_deadline`] warns about.
    fn consult(&self) -> Option<web_time::Instant> {
        let hook = self.handlers.lock().wake_deadline.clone();
        hook.and_then(|hook| hook())
    }

    /// Schedule one tick for `target`, superseding any tick before it.
    fn schedule(self: &Arc<Self>, target: web_time::Instant) {
        self.pending_at
            .store(micros_since_base(target), Ordering::Release);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let delay = target.saturating_duration_since(web_time::Instant::now());
        let pump = Arc::clone(self);
        owner_queue().exec_after(delay, move || pump.fire(generation));
    }

    /// A scheduled tick: honor the deadline if it is due, otherwise wait for it.
    fn fire(self: &Arc<Self>, generation: u64) {
        if self.generation.load(Ordering::Acquire) != generation {
            // A later arm superseded this tick; it owns the deadline now.
            return;
        }
        match tick_plan(self.consult(), web_time::Instant::now()) {
            TickPlan::Stop => {
                // Nothing pending and this tick requested no frame, so there
                // is nothing left to come back for. Stopping HERE is what
                // keeps an idle app from ticking: it sends no redraw
                // requests, so this is the only place the chain could
                // restart itself.
                //
                // Clear `pending_at` with the stop. This tick has run, so no
                // tick is queued any more; leaving the (now-expired) instant
                // behind would make every later `arm()` see a pending tick
                // earlier than its own target and return without scheduling —
                // `arm` reads a nonzero `pending_at` as "a tick is still
                // queued", which is no longer true. The next real deadline
                // (no-present fallback, device recovery) would then never
                // wake the loop. The store must happen before the trace so a
                // reader of the field after this arm never sees the stopped
                // tick as live.
                self.pending_at.store(0, Ordering::Release);
                tracing::trace!(
                    target: "flui.pace",
                    event = "wake_pump_idle",
                    "AppKit wake pump found no deadline; chain stopped"
                );
            }
            TickPlan::WakeThenLook { late_us } => {
                tracing::trace!(
                    target: "flui.pace",
                    event = "wake_pump_fired",
                    late_us,
                    "AppKit wake pump firing a frame for a due deadline"
                );
                (self.wake)();
                // The frame just requested runs after this block returns
                // (AppKit coalesces it into a display pass), so it may arm a
                // deadline this consult could not have seen — look again
                // shortly, the same reason `arm` does.
                self.schedule(web_time::Instant::now() + FOLLOW_UP);
            }
            TickPlan::Wait(deadline) => self.schedule(deadline),
        }
    }
}

/// Type-erased weak handle a window stores (see `MacOSWindow::install_wake_pump`).
pub(crate) type WeakWakePump = Weak<WakePump>;

/// What a consult says the pump should do next.
///
/// Split out of [`WakePump::fire`] so the decision is unit-testable without a
/// running run loop: `fire` is three lines of dispatch over this, and the GCD
/// half (`schedule`/`exec_after`) never sees an `Instant`.
#[derive(Debug, PartialEq, Eq)]
enum TickPlan {
    /// No deadline and no frame requested — the chain ends here.
    Stop,
    /// A deadline is still ahead: wait for it (carrying the instant itself,
    /// which is absolute — see [`WakePump::arm`] for why that matters).
    Wait(web_time::Instant),
    /// The deadline is due: ask for a frame, then look again shortly
    /// (carrying how late the tick was, for the trace).
    WakeThenLook { late_us: u64 },
}

/// Decide what to do at `now`, given what the hook answered.
fn tick_plan(answer: Option<web_time::Instant>, now: web_time::Instant) -> TickPlan {
    match answer {
        None => TickPlan::Stop,
        Some(deadline) if deadline <= now => TickPlan::WakeThenLook {
            late_us: now.saturating_duration_since(deadline).as_micros() as u64,
        },
        Some(deadline) => TickPlan::Wait(deadline),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> web_time::Instant {
        // A fixed base so the tests never depend on the wall clock; the only
        // thing under test is the comparison between two instants.
        web_time::Instant::now() + Duration::from_millis(ms)
    }

    #[test]
    fn no_deadline_stops_the_chain() {
        // The property that keeps the pump off an idle app: nothing pending
        // and no frame requested must end the chain, never re-schedule.
        assert_eq!(tick_plan(None, at(0)), TickPlan::Stop);
    }

    #[test]
    fn a_future_deadline_waits_exactly_for_it() {
        let now = at(0);
        let deadline = now + Duration::from_millis(500);
        assert_eq!(tick_plan(Some(deadline), now), TickPlan::Wait(deadline));
    }

    #[test]
    fn a_deadline_at_now_is_due() {
        // The boundary is inclusive (`<=`, not `<`): a deadline that has
        // arrived must fire a frame, not wait another period for it.
        let now = at(0);
        assert_eq!(
            tick_plan(Some(now), now),
            TickPlan::WakeThenLook { late_us: 0 },
        );
    }

    #[test]
    fn a_past_deadline_fires_and_reports_its_lateness() {
        let now = at(0);
        let overdue = now
            .checked_sub(Duration::from_millis(7))
            .expect("a base 7 ms ahead of now can be subtracted from");
        assert_eq!(
            tick_plan(Some(overdue), now),
            TickPlan::WakeThenLook { late_us: 7_000 },
        );
    }

    fn silent_pump() -> Arc<WakePump> {
        WakePump::new(
            Arc::new(Mutex::new(PlatformHandlers::default())),
            Box::new(|| panic!("this pump must not request a frame")),
        )
    }

    #[test]
    fn an_arm_never_moves_a_pending_tick_later() {
        // The anti-starvation rule. Without it a redraw burst cancels its own
        // ticks: every request takes a generation, so the tick the previous
        // one scheduled never runs — measured as 241 arms, 0 firings, 238
        // superseded, with the deadline armed in the middle of the burst
        // never consulted at all.
        let pump = silent_pump();
        // A tick already scheduled a millisecond out. The hook answers
        // nothing here, so this arm's own target is FOLLOW_UP (8 ms) — later
        // than the pending tick, which must therefore be left alone.
        pump.pending_at.store(
            micros_since_base(web_time::Instant::now() + Duration::from_millis(1)),
            Ordering::Release,
        );
        let before = pump.generation.load(Ordering::Acquire);
        pump.arm();
        assert_eq!(
            pump.generation.load(Ordering::Acquire),
            before,
            "an arm later than the pending tick must not re-schedule it"
        );
    }

    #[test]
    fn an_arm_earlier_than_the_pending_tick_replaces_it() {
        // The other side of the rule: moving the tick EARLIER must still
        // work, or a deadline armed after a long wait would never be honored
        // on time.
        let pump = silent_pump();
        pump.pending_at.store(
            micros_since_base(web_time::Instant::now() + Duration::from_secs(30)),
            Ordering::Release,
        );
        let before = pump.generation.load(Ordering::Acquire);
        pump.arm();
        assert_ne!(
            pump.generation.load(Ordering::Acquire),
            before,
            "an arm earlier than the pending tick must replace it"
        );
    }

    #[test]
    fn a_stopped_chain_lets_the_next_arm_schedule_again() {
        // Regression: `Stop` used to leave `pending_at` pointing at the
        // stopped (now-expired) tick, so every later `arm` read a "pending"
        // tick earlier than its own target and returned without scheduling.
        // After the first idle cycle the pump was effectively dead: the next
        // no-present fallback or device-recovery deadline could never wake
        // the loop.
        let pump = silent_pump();
        // A tick that has already run, whose instant was never cleared.
        pump.pending_at.store(
            micros_since_base(web_time::Instant::now()),
            Ordering::Release,
        );

        // Drive the stop through `fire` so the clearing under test runs.
        let generation = pump.generation.load(Ordering::Acquire);
        pump.fire(generation);
        assert_eq!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "a stopped chain must clear its pending marker, or the next arm \
             sees a phantom tick and schedules nothing"
        );

        // With the marker clear, a fresh arm must actually schedule.
        let before = pump.generation.load(Ordering::Acquire);
        pump.arm();
        assert_ne!(
            pump.generation.load(Ordering::Acquire),
            before,
            "the first arm after an idle stop must schedule a new tick"
        );
    }

    #[test]
    fn a_superseded_tick_returns_without_acting() {
        // The generation that makes `arm` safe to call on every redraw
        // request: a tick whose generation is stale must not consult at all.
        let pump = silent_pump();
        let stale = pump.generation.fetch_add(1, Ordering::AcqRel);
        // A later arm takes the next generation, superseding `stale`.
        let _ = pump.generation.fetch_add(1, Ordering::AcqRel);
        pump.fire(stale);
    }
}
