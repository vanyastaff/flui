//! Wake-deadline actuation for the AppKit backend (ADR-0353 decision 2).
//!
//! # Why this module exists
//!
//! The application's handler answers the earliest wall-clock instant the
//! event loop should come back at. The Linux loops hand that to their poll
//! timeout and the Android backend checks it once per
//! iteration of its own loop. AppKit has neither: `[NSApplication run]` is an
//! opaque, unbounded run loop, so a deadline nothing actuated was dropped
//! on the floor and the process parked in
//! `_nextEventMatchingEventMask:untilDate:` with the handler never asked.
//! This module is that deadline's landing place.
//!
//! # The two things that make the shape what it is
//!
//! 1. **The hook re-enters `flui-app` and `APP_RUNTIME` is thread-local**, so
//!    it may only be consulted on the owner (main) thread — which rules out a
//!    background deadline thread and picks the main queue as the actuator.
//!    [`owner_queue`] is that queue and `exec_after_on_owner` schedules onto it without
//!    an Objective-C target class, so `[NSApp run]` keeps draining it
//!    unchanged.
//! 2. **A redraw request is not an idle point.** `request_redraw` runs inside
//!    the owner's dispatch, while `flui-app` has a UI runtime checked out, and
//!    the hook's contract is one consult per idle iteration. So
//!    [`WakePump::arm`] never consults: it schedules a look for the next
//!    owner-queue turn.
//!
//! A look is scheduled at a redraw request, after each run of a window's
//! delivery in which a frame ran ([`WakePump::window_delivery`]; a run of
//! input alone asks for none), and after an
//! owner turn that leaves a deadline (`LoopControl`,
//! [`WakePump::after_owner_turn`]). The run of a frame is what follows a
//! delivery up: the frame AppKit runs for a request, whenever its display
//! pass comes, may leave the deadline due again or arm a new one, and no owner
//! turn need follow it. An owner turn can leave a deadline no redraw request
//! starts the pump for, such as the frame a covered window owes its surface
//! (ADR-0357), whose window asks for no frame while it cannot be seen.
//!
//! Each look arms and takes the deadline by the rule every native host shares
//! (`WakeDeadline::turn`): a due instant is delivered once, as a frame request
//! to every window, and armed again only after a frame ran, which the windows
//! count into the pump's count. The chain is self-limiting, which is what
//! keeps it from being a busy poll: a look that finds nothing armed, or
//! delivers what it found, schedules nothing more, so a deadline whose frames
//! the handler declines costs one delivery, and an idle app sends no redraw
//! requests and runs no frames, so nothing is ever scheduled for it.

use std::{
    rc::Rc,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use flui_platform::WindowHandler;
use flui_platform::input_capabilities::InputCapabilities;

use super::owner_lane::{exec_after_on_owner, owner_queue};

use crate::owner::app_delivery::AppDelivery;
use crate::owner::apple::pump_delivery::{self, FollowedDelivery};
use crate::owner::slot::OwnerCell;
use crate::owner::wake_deadline::{FrameCount, WakeDeadline, WakeTurn};

/// The AppKit landing place for the platform's wake deadline.
///
/// Held as `Arc` by [`super::MacOSLoop`] and as `Weak` by each AppKit
/// window, which arms it from `request_redraw`.
pub(crate) struct WakePump {
    /// The loop's delivery, whose handler [`Self::consult`] asks for its wake
    /// deadline, with no lock held.
    app: Arc<AppDelivery>,

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

    /// The deadline armed and the last one delivered, the thread's that made
    /// the pump (the main thread, where every tick and owner turn runs).
    deadline: OwnerCell<WakeDeadline>,

    /// The frames this platform's windows' handlers ran, which tell a
    /// deadline a frame left due again from one nothing serviced.
    frames: FrameCount,
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
///
/// Never `0`, which `pending_at` reserves for "no tick queued": a tick
/// scheduled for the instant the base was fixed (the first `arm`) would
/// otherwise read as none and be superseded by the next arm.
fn micros_since_base(instant: web_time::Instant) -> u64 {
    u64::try_from(instant.saturating_duration_since(base()).as_micros())
        .unwrap_or(u64::MAX)
        .max(1)
}

impl WakePump {
    /// Build the pump. `wake` is invoked on the owner thread when a deadline
    /// has come due; it must be safe to call from there (see
    /// [`super::MacOSLoop`]'s construction site).
    pub(crate) fn new(app: Arc<AppDelivery>, wake: Box<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            app,
            wake,
            generation: AtomicU64::new(0),
            pending_at: AtomicU64::new(0),
            deadline: OwnerCell::new(),
            frames: FrameCount::default(),
        })
    }

    /// The delivery of a window opened with `handler`, whose input devices
    /// give it `capabilities`: each frame its handler runs raises the pump's
    /// count, and each run of its FIFO in which a frame ran ends in a look
    /// ([`pump_delivery::window_delivery`]).
    pub(crate) fn window_delivery(
        self: &Arc<Self>,
        handler: Rc<dyn WindowHandler>,
        capabilities: InputCapabilities,
    ) -> FollowedDelivery {
        let pump = Arc::downgrade(self);
        pump_delivery::window_delivery(handler, capabilities, self.frames.counter(), move || {
            if let Some(pump) = pump.upgrade() {
                pump.look_soon();
            }
        })
    }

    /// Ask the pump to make the loop come back. Called from `request_redraw`
    /// (and once as the run binds the application's handler), i.e. whenever a frame is
    /// about to happen.
    pub(crate) fn arm(self: &Arc<Self>) {
        if self.look_soon() {
            tracing::trace!(
                target: "flui.pace",
                event = "wake_pump_armed",
                "AppKit wake pump armed"
            );
        }
    }

    /// Schedule a look for the next owner-queue turn, unless a tick is
    /// queued no later than now; `true` when one was scheduled. Runs no user
    /// code, a `tracing` event included, so a window's run may call it while
    /// a panic unwinds.
    fn look_soon(self: &Arc<Self>) -> bool {
        // The hook is not consulted here. The caller is inside the owner's
        // dispatch (a runtime asking its own window for a frame, a window's
        // run ending in a display pass), where the hook finds that runtime
        // checked out; the look runs on the next owner-queue turn instead,
        // once the dispatch has returned.
        let target = web_time::Instant::now();

        // Only ever move the tick EARLIER. Re-scheduling unconditionally
        // looks harmless, but it is not: while a frame is being asked for
        // over and over (the redraw burst a cold start produces), each
        // request would take a new generation and cancel the tick the
        // previous one scheduled, so no tick would ever run and the pump
        // would be starved by exactly the traffic it exists to survive.
        // Measured: 241 arms, 0 firings, 238 superseded — the deadline armed
        // mid-burst was never consulted at all. Leaving an already-due tick
        // alone is what makes the burst harmless.
        let pending = self.pending_at.load(Ordering::Acquire);
        if pending != 0 && base() + Duration::from_micros(pending) <= target {
            return false;
        }
        self.schedule(target);
        true
    }

    /// An owner turn ended, an idle point: look at the wake deadline once and
    /// schedule a tick for it only when one is armed earlier than the tick
    /// queued, so a turn that leaves nothing due, or only an instant already
    /// delivered that no frame has run for since, costs no wake. A turn can
    /// leave a deadline no redraw request starts the pump for: the frame a
    /// covered window owes its surface, whose window asks for no frame while
    /// it cannot be seen (ADR-0357).
    pub(crate) fn after_owner_turn(self: &Arc<Self>) {
        let answer = self.consult();
        let frames = self.frames.get();
        // Armed here, taken by the tick at its instant, as the loops that
        // wait between the two do.
        let Some(deadline) = self
            .deadline
            .with(|deadline| deadline.arm(answer, frames))
            .flatten()
        else {
            return;
        };
        let pending = self.pending_at.load(Ordering::Acquire);
        if pending != 0 && base() + Duration::from_micros(pending) <= deadline {
            return;
        }
        self.schedule(deadline);
    }

    /// Ask the application's handler for its wake deadline through the call
    /// point, with no lock held, because the handler walks every hosted UI
    /// runtime and takes gesture-arena locks.
    fn consult(&self) -> Option<web_time::Instant> {
        self.app.wake_deadline().or_resume(None)
    }

    /// Schedule one tick for `target`, superseding any tick before it.
    fn schedule(self: &Arc<Self>, target: web_time::Instant) {
        self.pending_at
            .store(micros_since_base(target), Ordering::Release);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let delay = target.saturating_duration_since(web_time::Instant::now());
        let pump = Arc::clone(self);
        exec_after_on_owner(owner_queue(), delay, move || {
            pump.fire(generation);
        });
    }

    /// A scheduled tick: deliver the deadline if it is due, otherwise wait
    /// for it.
    ///
    /// The tick leaves no tick queued unless it scheduled one (`TickEnd`),
    /// whether it returned or its hook or wake panicked; a newer tick
    /// scheduled meanwhile stays queued.
    fn fire(self: &Arc<Self>, generation: u64) {
        if self.generation.load(Ordering::Acquire) != generation {
            // A later arm superseded this tick; it owns the deadline now.
            return;
        }
        let _end = TickEnd {
            pump: self,
            generation,
            pending_at: self.pending_at.load(Ordering::Acquire),
        };
        self.run_tick();
    }

    /// What a current tick does.
    fn run_tick(self: &Arc<Self>) {
        let answer = self.consult();
        let now = web_time::Instant::now();
        let frames = self.frames.get();
        // Off the thread that made the pump, or with a cell the exhausted
        // owner slots left empty, nothing is armed.
        let turn = self
            .deadline
            .with(|deadline| deadline.turn(answer, now, frames))
            .unwrap_or(WakeTurn::Idle);
        match turn {
            // Nothing armed, so there is nothing left to come back for: the
            // next look comes with a redraw request, a window's run or an
            // owner turn. Stopping HERE is what keeps an idle app from
            // ticking.
            WakeTurn::Idle => tracing::trace!(
                target: "flui.pace",
                event = "wake_pump_idle",
                "AppKit wake pump found no deadline armed; chain stopped"
            ),
            // The frames asked for run in display passes AppKit schedules,
            // and the run of each ends in a look (`window_delivery`): that
            // look sees whatever the frame left, so this tick schedules none.
            WakeTurn::Due(at) => {
                tracing::trace!(
                    target: "flui.pace",
                    event = "wake_pump_fired",
                    late_us = u64::try_from(now.saturating_duration_since(at).as_micros())
                        .unwrap_or(u64::MAX),
                    "AppKit wake pump firing a frame for a due deadline"
                );
                (self.wake)();
            }
            WakeTurn::Wait(at) => self.schedule(at),
        }
    }
}

/// Clears [`WakePump::pending_at`] as a tick ends, by return or by unwinding,
/// unless a newer tick was scheduled meanwhile (a new generation and
/// instant), which stays. This tick has run, so it is no longer queued:
/// leaving its expired instant behind would make every later `arm` read a
/// tick still queued earlier than its own target and schedule nothing, and
/// the next real deadline (no-present fallback, device recovery) would never
/// wake the loop.
struct TickEnd<'a> {
    pump: &'a WakePump,
    generation: u64,
    pending_at: u64,
}

impl Drop for TickEnd<'_> {
    fn drop(&mut self) {
        if self.pump.generation.load(Ordering::Acquire) == self.generation {
            let _ = self.pump.pending_at.compare_exchange(
                self.pending_at,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}

/// Weak handle a window's redraw requester stores (see `MacOSWindow::install_wake_pump`).
pub(crate) type WeakWakePump = Weak<WakePump>;

#[cfg(test)]
mod tests {
    use super::*;

    use crate::owner::app_delivery::testing::{AppLog, Bound, bind};
    use std::rc::Rc;

    /// A pump whose loop's delivery is bound to a recording handler, which
    /// answers no deadline until a test says otherwise.
    fn silent_pump() -> (Arc<WakePump>, Rc<AppLog>, Bound) {
        let app = Arc::new(AppDelivery::new());
        let pump = WakePump::new(
            Arc::clone(&app),
            Box::new(|| panic!("this pump must not request a frame")),
        );
        let handler = AppLog::new();
        let run = bind(app, handler.clone());
        (pump, handler, run)
    }

    #[test]
    fn an_arm_never_moves_a_pending_tick_later() {
        // The anti-starvation rule. Without it a redraw burst cancels its own
        // ticks: every request takes a generation, so the tick the previous
        // one scheduled never runs — measured as 241 arms, 0 firings, 238
        // superseded, with the deadline armed in the middle of the burst
        // never consulted at all.
        let (pump, _app, _run) = silent_pump();
        // A tick already due: an earlier request of the burst scheduled it
        // for its own turn. This arm's target is now, which is no earlier, so
        // the pending tick must be left alone.
        let due = micros_since_base(web_time::Instant::now());
        pump.pending_at.store(due, Ordering::Release);
        // `pending_at` is whole microseconds from the base and never 0, so
        // the tick stored can lie up to a microsecond ahead of now: an arm
        // inside that microsecond is earlier, and moves it. Let it come due.
        while base().elapsed() <= Duration::from_micros(due) {
            std::hint::spin_loop();
        }
        let before = pump.generation.load(Ordering::Acquire);
        pump.arm();
        assert_eq!(
            pump.generation.load(Ordering::Acquire),
            before,
            "an arm later than the pending tick must not re-schedule it"
        );
    }

    /// A redraw request runs inside the owner's dispatch, where `flui-app`
    /// has the asking runtime checked out and its hook answers `Busy`; the
    /// counter app aborted on start that way. The arm schedules the look
    /// instead of taking it. Red-check: consult in `arm` again.
    #[test]
    fn an_arm_schedules_the_look_instead_of_consulting() {
        use std::sync::atomic::AtomicUsize;

        let (pump, app, _run) = silent_pump();
        let consults = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&consults);
        app.answer_deadline_with(move || -> Option<web_time::Instant> {
            counted.fetch_add(1, Ordering::AcqRel);
            None
        });
        let before = pump.generation.load(Ordering::Acquire);

        pump.arm();

        assert_eq!(
            consults.load(Ordering::Acquire),
            0,
            "the arm consulted the hook inside the caller's dispatch"
        );
        assert_ne!(
            pump.generation.load(Ordering::Acquire),
            before,
            "the arm scheduled no look"
        );
        assert_ne!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "the scheduled look does not read as queued"
        );
    }

    /// A look that finds nothing armed stops the chain: the next look comes
    /// with a redraw request, a window's run or an owner turn. Red-check:
    /// leave the tick's instant queued.
    #[test]
    fn an_empty_look_stops_the_chain() {
        let (pump, app, _run) = silent_pump();
        app.answer_deadline_with(|| -> Option<web_time::Instant> { None });
        pump.arm();
        let generation = pump.generation.load(Ordering::Acquire);

        pump.fire(generation);

        assert_eq!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "an empty look kept the chain going"
        );
    }

    /// A due deadline is delivered once; a frame that runs in a window the
    /// pump's delivery built, however late, raises the pump's count and asks
    /// for the look that delivers the same instant again. Red-check: build
    /// the window's delivery with `WindowDelivery::new`.
    #[test]
    fn a_frame_of_a_pump_window_lets_the_deadline_be_delivered_again() {
        use std::sync::atomic::AtomicUsize;

        use flui_platform::{EventTime, FrameRequest};
        use flui_testing::window_handler::RecordingWindowHandler;

        let app = Arc::new(AppDelivery::new());
        let wakes = Arc::new(AtomicUsize::new(0));
        let woken = Arc::clone(&wakes);
        let pump = WakePump::new(
            Arc::clone(&app),
            Box::new(move || {
                woken.fetch_add(1, Ordering::Relaxed);
            }),
        );
        let log = AppLog::new();
        let _run = bind(app, log.clone());
        let due = web_time::Instant::now();
        log.answer_deadline_with(move || -> Option<web_time::Instant> { Some(due) });
        let window = RecordingWindowHandler::new();
        window.react_to_frame(|_| {});
        let delivery = pump
            .window_delivery(window.clone(), InputCapabilities::UNAVAILABLE)
            .into_inner();
        let fire = || pump.fire(pump.generation.load(Ordering::Acquire));

        fire();
        assert_eq!(wakes.load(Ordering::Relaxed), 1, "delivered at its instant");
        fire();
        assert_eq!(
            wakes.load(Ordering::Relaxed),
            1,
            "and once while no frame ran"
        );

        delivery.dispatch_frame(FrameRequest::new(EventTime::from_nanos(1)));
        assert_ne!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "the frame's run asked for no look"
        );
        fire();
        assert_eq!(
            wakes.load(Ordering::Relaxed),
            2,
            "the frame that ran lets the instant be delivered again"
        );
    }

    /// A tick whose hook panics is contained by the GCD boundary, and must not
    /// leave the expired tick looking queued: `arm` would then refuse every
    /// later deadline. Red-check: drop the `TickEnd` guard.
    #[test]
    fn a_tick_whose_hook_panics_leaves_no_tick_queued() {
        let (pump, app, _run) = silent_pump();
        app.answer_deadline_with(|| -> Option<web_time::Instant> { panic!("a failing wake hook") });
        // The tick being fired: a nonzero instant, whatever the base.
        pump.pending_at.store(
            micros_since_base(web_time::Instant::now() + Duration::from_millis(5)),
            Ordering::Release,
        );
        let generation = pump.generation.load(Ordering::Acquire);

        let fired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pump.fire(generation);
        }));

        assert!(
            fired.is_err(),
            "the hook's panic reaches the tick's boundary"
        );
        assert_eq!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "the failed tick still reads as queued"
        );
    }

    /// A hook that scheduled a newer tick before it panicked keeps that tick:
    /// only the failed tick stops reading as queued. Red-check: clear
    /// `pending_at` without comparing the generation and instant.
    #[test]
    fn a_tick_whose_hook_rearms_then_panics_keeps_the_newer_tick() {
        use std::sync::atomic::AtomicBool;

        let (pump, app, _run) = silent_pump();
        let weak = Arc::downgrade(&pump);
        let rearmed = AtomicBool::new(false);
        app.answer_deadline_with(move || -> Option<web_time::Instant> {
            if !rearmed.swap(true, Ordering::AcqRel)
                && let Some(pump) = weak.upgrade()
            {
                pump.schedule(web_time::Instant::now() + Duration::from_millis(50));
            }
            panic!("a wake hook that re-armed, then failed")
        });
        pump.pending_at.store(
            micros_since_base(web_time::Instant::now() + Duration::from_millis(5)),
            Ordering::Release,
        );
        let generation = pump.generation.load(Ordering::Acquire);

        let fired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pump.fire(generation);
        }));

        assert!(
            fired.is_err(),
            "the hook's panic reaches the tick's boundary"
        );
        assert_ne!(
            pump.pending_at.load(Ordering::Acquire),
            0,
            "the newer tick the hook scheduled no longer reads as queued"
        );
    }
}
