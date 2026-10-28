//! [`FrameClock`] — the per-presentation, platform-free physical-time policy
//! state machine (issue #556).
//!
//! # The three-owner split
//!
//! [`UpdateScheduler`](crate::UpdateScheduler) owns *logical* time (phases,
//! callback queues, the priority task queue) and makes no refresh-rate,
//! display, or surface assumption. `FrameClock` owns *physical* time for
//! **one presentation**: demand coalescing, actuator-edge coalescing
//! ([`try_arm_redraw_request`](FrameClock::try_arm_redraw_request)),
//! compositor pacing feedback
//! ([`record_compositor_tick`](FrameClock::record_compositor_tick)),
//! first-frame deferral, visibility gating, an optional produce throttle,
//! and frame timestamps. It answers exactly one question — "does this
//! surface produce a frame now?" — and never touches a phase, a callback,
//! or an element tree, and never reaches out to read an actual display or
//! window: every physical-time fact it reasons about (a compositor tick,
//! a minimum produce interval) is fed in by a caller that owns the real
//! surface. Raster capacity (whether the GPU will accept another frame) is
//! bounded by the surface's configured frame latency, not by this clock.
//!
//! # The driver-loop hybrid
//!
//! Two distinct signals exist, and this clock keeps them separate rather
//! than conflating them. The demand mask ([`DemandMask`]) is the ONLY
//! input [`poll`](FrameClock::poll) ever reads — a tree/animation demand
//! mark (`Dirty`/`Animation`) drives it via
//! [`mark_demand`](FrameClock::mark_demand), exactly as it always has.
//! [`record_compositor_tick`](FrameClock::record_compositor_tick) is a
//! SEPARATE, demand-free channel: it records when a compositor/platform-
//! paced tick arrived (for pacing feedback only — see its own doc for why
//! it does not, and structurally cannot, mark demand from inside this
//! method). What the actuator side gains is
//! [`try_arm_redraw_request`](FrameClock::try_arm_redraw_request): whichever
//! platform mechanism ends up poking `request_redraw()` (a compositor-
//! paced tick where the platform paces one, an immediate self-wake where
//! it does not) consults this method first, so N demand marks that land
//! before the next produce collapse into exactly one poke instead of a
//! storm of redundant ones. See `flui-app`'s own driver wiring and
//! `docs/adr/ADR-0044-driver-loop-hybrid.md`'s per-platform table for
//! which concrete mechanism plays which role on each backend.
//!
//! # No public mode enum
//!
//! There is deliberately no `FrameClock` mode (`OnDemand`/`Continuous`/…).
//! Every produce decision is a pure function of the current
//! [`DemandMask`] plus three orthogonal gates (hidden, deferred, capacity) —
//! "idle" is simply the empty mask, not a state a caller sets.
//!
//! # First-frame deferral withholds the SUBMIT, never the segment
//!
//! While the first frame is deferred, the framework still does all the work
//! to produce frames, but those frames are never sent to the engine and do
//! not appear on screen. Deferral gates the **submit** only. [`FrameClock::poll`]
//! honors this: while deferred, a nonzero demand mask still runs the
//! caller's segment ([`PollDecision::ProduceWithheld`]) — build/layout/paint
//! happen exactly as they would undeferred — and only the separate
//! [`FrameClock::is_deferred`] query, consulted by the caller at its own
//! submit point, withholds the result from the engine.
//!
//! # The deterministic test clock
//!
//! [`ClockSource::Manual`] wraps a [`ManualClock`]:
//! a presentation seam takes a `ClockSource` (`Platform` by default), so an
//! app author's own integration test — not just `flui-testing` — can drive
//! one presentation's clock deterministically while a sibling presentation
//! keeps its own cadence. [`FrameClock::now`] is the ONLY time [`poll`](FrameClock::poll)
//! ever reads; passing a caller-supplied `now` into `poll` (rather than
//! `poll` reading a clock source itself) is what makes the same decision
//! function replayable against a scripted timeline with byte-identical
//! output on every run.
//!
//! # Telemetry: what a produce actually did
//!
//! [`stamp_input_epoch`](FrameClock::stamp_input_epoch)/
//! [`record_frame`](FrameClock::record_frame)/
//! [`frames_since`](FrameClock::frames_since) extend this clock's existing
//! "frame timestamps" responsibility (see the ownership table above) with a
//! fixed-capacity history of what each produce did and which routed inputs
//! it carried — see `crate::frame_telemetry`'s own module doc for the full
//! shape and the zero-allocation argument.
//!
//! # Deferral accounting: a Skip is never a drop
//!
//! [`hidden_deferrals`](FrameClock::hidden_deferrals)/
//! [`backpressure_deferrals`](FrameClock::backpressure_deferrals) count
//! [`poll`](FrameClock::poll) calls that retained demand rather than
//! granting it — `Hidden`/`Backpressure`, never `NoDemand` (there is no
//! demand to defer there). This is deliberately a SEPARATE counter family
//! from a caller's own "frames dropped" accounting (e.g.
//! `PresentationState::frames_dropped` in `flui-app`, incremented only on a
//! real submit failure): a presentation that is merely gated, or waiting on
//! GPU capacity, has not failed to produce a frame — it has deferred one,
//! and the demand is still retained for the next successful poll. Conflating
//! the two would make a healthy, momentarily-gated presentation look like it
//! is failing.

use std::cell::{Cell, RefCell};

use crate::frame::FrameId;
use crate::frame_telemetry::{
    FrameHistory, FramePhaseDurations, FrameSnapshot, InputEpochId, InputEpochs,
    PendingInputEpochs, PresentOutcome,
};
use flui_foundation::{ManualClock, MonotonicClock, PresentationId};
use web_time::{Duration, Instant};

bitflags::bitflags! {
    /// The set of reasons a presentation currently wants a frame.
    ///
    /// Sampled once per [`FrameClock::poll`] call and cleared on a granted
    /// [`PollDecision::Produce`]/[`PollDecision::ProduceWithheld`] — a
    /// persistent demand (a still-running animation) is re-armed by its own
    /// owner every pump via [`FrameClock::mark_demand`], never retained by
    /// the clock itself across a produce.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct DemandMask: u8 {
        /// A pending widget build or a dirty render node.
        const DIRTY = 1 << 0;
        /// A registered, running [`AnimationController`](https://docs.rs/flui-animation)
        /// (or another vsync-ticked consumer) wants the next frame.
        const ANIMATION = 1 << 1;
        /// The host platform asked for a frame directly (a compositor-paced
        /// `RedrawRequested`, or an explicit embedder request) with no
        /// framework-side dirty state of its own.
        const HOST = 1 << 2;
        /// Accepted presentation callbacks need a coherent completion epoch,
        /// even when no widget or render node is dirty.
        const COMPLETION = 1 << 3;
    }
}

/// One reason a presentation wants the next frame — the vocabulary
/// [`FrameClock::mark_demand`]/[`clear_demand`](FrameClock::clear_demand)
/// accept, folded into a [`DemandMask`] bit.
///
/// `#[non_exhaustive]`: this is the demand *vocabulary*, not a mode — adding
/// a media/video-frame kind later is additive, not a breaking match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DemandKind {
    /// A pending widget build or a dirty render node.
    Dirty,
    /// A running animation/ticker consumer.
    Animation,
    /// A direct host/platform request with no framework-side dirty state.
    Host,
    /// Accepted callbacks need a coherent presentation completion epoch.
    Completion,
}

impl From<DemandKind> for DemandMask {
    fn from(kind: DemandKind) -> Self {
        match kind {
            DemandKind::Dirty => DemandMask::DIRTY,
            DemandKind::Animation => DemandMask::ANIMATION,
            DemandKind::Host => DemandMask::HOST,
            DemandKind::Completion => DemandMask::COMPLETION,
        }
    }
}

/// Why [`FrameClock::poll`] returned [`PollDecision::Skip`].
///
/// `#[non_exhaustive]`: a caller matches the reasons it cares about and
/// falls through on the rest; a new reason is additive, not breaking.
///
/// First-frame deferral is deliberately NOT a variant here: it withholds
/// only the submit, never the segment, so it is never a reason to skip
/// running the segment at all — see [`PollDecision::ProduceWithheld`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkipReason {
    /// The demand mask is empty — nobody asked for a frame. The only reason
    /// that does NOT retain any mask bits (there are none to retain).
    NoDemand,
    /// This presentation is hidden ([`FrameClock::set_hidden`]); demand is
    /// retained so an unhide with a nonzero mask produces immediately.
    Hidden,
    /// The caller-configured minimum produce interval
    /// ([`FrameClock::set_min_produce_interval`]) has not yet elapsed since
    /// the last produce. Demand is retained.
    Backpressure,
}

/// [`FrameClock::poll`]'s produce/skip decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PollDecision {
    /// Run the segment now, and the result should reach the engine. The
    /// mask has been cleared and this poll's `now` recorded as the last
    /// produce instant.
    Produce,
    /// Run the segment now (build/layout/paint, exactly as for `Produce` —
    /// the mask clears the same way) but the caller must WITHHOLD the
    /// result from the engine: first-frame deferral
    /// ([`FrameClock::defer`]) is active. See the module doc — deferral
    /// withholds the submit, never the pipeline work.
    /// A caller checks [`FrameClock::is_deferred`] at its own submit point;
    /// `poll` itself does not repeat that check on a later call once the
    /// mask it already cleared here is gone.
    ProduceWithheld,
    /// Do not run anything this poll, for the given reason.
    Skip(SkipReason),
}

impl PollDecision {
    /// Whether the caller should run its segment (build/layout/paint) for
    /// this poll — true for both `Produce` and `ProduceWithheld`.
    #[must_use]
    pub fn should_run_segment(self) -> bool {
        matches!(self, PollDecision::Produce | PollDecision::ProduceWithheld)
    }

    /// Whether this decision is an unconditional `Produce` — the segment
    /// ran AND the result should reach the engine. `false` for
    /// `ProduceWithheld`, whose result must be withheld regardless of
    /// whether the segment succeeded.
    #[must_use]
    pub fn is_produce(self) -> bool {
        matches!(self, PollDecision::Produce)
    }
}

/// Where [`FrameClock::now`] reads physical time from.
///
/// `#[non_exhaustive]`: this is a small, closed choice today (real clock vs.
/// virtual clock), but is not meant to grow into a mode enum — a third
/// variant, if one is ever needed, is still just "a different place to read
/// `now` from", never a produce-policy change.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum ClockSource {
    /// Read the real OS clock (`Instant::now()` / `performance.now()` on
    /// wasm32, via [`web_time`]). The default for every production
    /// presentation.
    #[default]
    Platform,
    /// Read a caller-driven virtual clock. The presentation seam a test
    /// harness (or an app author's own integration test) installs this
    /// through never reads the wall clock: every timestamp
    /// [`FrameClock::now`] returns, and therefore every produce decision
    /// [`FrameClock::poll`] makes, traces back to an explicit
    /// [`FrameClock::advance`] call on `this` clock.
    Manual(ManualClock),
}

/// A clock source is itself a clock: whatever reads time through it (a
/// presentation's [`FrameClock`], a UI runtime's gesture arena and frame-time
/// origin) observes the one timeline it names.
impl MonotonicClock for ClockSource {
    fn now(&self) -> Instant {
        match self {
            Self::Platform => Instant::now(),
            Self::Manual(clock) => clock.now(),
        }
    }

    fn epoch(&self) -> Instant {
        match self {
            Self::Platform => flui_foundation::event_epoch(),
            Self::Manual(clock) => clock.epoch(),
        }
    }
}

/// The per-presentation physical-time policy state machine.
///
/// Pure and platform-free: it owns no window, no GPU handle, no element
/// tree, and runs no callback — it only tracks demand and three produce
/// gates (hidden / deferred / capacity) and answers [`poll`](Self::poll).
///
/// Not `Send`/`Sync`-restricted by construction (every field is a `Cell`),
/// matching the presentation it lives on: `flui-app`'s `PresentationState`
/// is itself owner-thread-confined, so `FrameClock` needs no atomics or
/// locks of its own.
#[derive(Debug)]
pub struct FrameClock {
    source: ClockSource,
    demand: Cell<DemandMask>,
    hidden: Cell<bool>,
    deferred_count: Cell<u32>,
    first_frame_sent: Cell<bool>,
    min_produce_interval: Cell<Option<Duration>>,
    last_produce_at: Cell<Option<Instant>>,
    produced: Cell<u64>,
    /// Set by [`try_arm_redraw_request`](Self::try_arm_redraw_request) the
    /// first time it grants an actuation edge for the current demand mask;
    /// cleared by [`poll`](Self::poll) on a granted produce. See that
    /// method's doc for the full coalescing contract.
    redraw_requested: Cell<bool>,
    /// The instant [`record_compositor_tick`](Self::record_compositor_tick)
    /// last recorded, if any — the pacing-feedback sample this clock's
    /// owner (never this clock itself) feeds from an actual compositor-paced
    /// signal.
    last_compositor_tick: Cell<Option<Instant>>,
    /// The interval between the two most recent
    /// [`record_compositor_tick`](Self::record_compositor_tick) calls —
    /// `None` until at least two ticks have been recorded.
    last_compositor_tick_interval: Cell<Option<Duration>>,
    /// The newest report that a frame of this presentation reached the
    /// screen ([`record_presentation`](Self::record_presentation)).
    last_presentation: Cell<Option<PresentationReport>>,
    /// How many [`poll`](Self::poll) calls returned `Skip(Hidden)` — a
    /// deferral, never a drop. See the module doc's "Deferral accounting"
    /// section.
    deferred_hidden: Cell<u64>,
    /// How many [`poll`](Self::poll) calls returned `Skip(Backpressure)` —
    /// a deferral, never a drop. See the module doc's "Deferral accounting"
    /// section.
    deferred_backpressure: Cell<u64>,
    /// Input epochs stamped since the last [`record_frame`](Self::record_frame)
    /// drained them. `RefCell`, not `Cell`: the buffer is a small inline
    /// array (`InputEpochs`), too large to move in and out of a `Cell` on
    /// every push the way this struct's other fields do.
    pending_input_epochs: RefCell<PendingInputEpochs>,
    /// The fixed-capacity produced-frame ring. `RefCell` for the same
    /// reason as `pending_input_epochs` above.
    history: RefCell<FrameHistory>,
}

/// The platform's report that a frame of a presentation reached the screen,
/// on this clock's timeline: what
/// [`FrameClock::record_presentation`] keeps.
///
/// [`at`](Self::at) is when the display started showing the frame, and
/// [`refresh`](Self::refresh) the display's refresh interval then, where the
/// platform names a fixed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PresentationReport {
    /// When the display started showing the frame.
    pub at: Instant,
    /// The display's refresh interval, where the platform names a fixed one.
    pub refresh: Option<Duration>,
}

impl PresentationReport {
    /// A frame shown at `at`, at an unknown refresh rate.
    #[must_use]
    pub fn new(at: Instant) -> Self {
        Self { at, refresh: None }
    }

    /// The same report with the display's refresh interval; a zero interval
    /// is an unknown one.
    #[must_use]
    pub fn with_refresh(mut self, refresh: Duration) -> Self {
        self.refresh = (!refresh.is_zero()).then_some(refresh);
        self
    }
}

/// How many refreshes past the last presentation report
/// [`FrameClock::next_presentation_after`] extrapolates the refresh grid:
/// about a second at 60 to 240 Hz. Further out, a clock that drifts by a
/// fraction of a percent, or a display that changed its rate without a
/// report, would name an instant that is not a refresh.
pub const PRESENTATION_GRID_HORIZON: u32 = 240;

/// When the stages of one produced frame happened: what
/// [`FrameClock::record_frame`] stores in its [`FrameSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTimes {
    /// The frame time the pump sampled the presentation at.
    pub clock_timestamp: Instant,
    /// When the presentation's segment started.
    pub segment_start: Instant,
    /// When it finished.
    pub segment_end: Instant,
    /// When the raster backend returned from consuming the scene.
    pub submit_at: Instant,
    /// How long the segment's build, layout and paint took.
    pub phases: FramePhaseDurations,
}

impl FrameClock {
    /// A fresh clock reading the real platform clock, with an empty demand
    /// mask, not hidden, not deferred, and no throttle.
    #[must_use]
    pub fn new() -> Self {
        Self::with_source(ClockSource::Platform)
    }

    /// A fresh clock reading `source` instead of the platform default — the
    /// presentation-seam constructor `flui-testing` and an app author's own
    /// integration test use to install a [`ClockSource::Manual`].
    #[must_use]
    pub fn with_source(source: ClockSource) -> Self {
        Self {
            source,
            demand: Cell::new(DemandMask::empty()),
            hidden: Cell::new(false),
            deferred_count: Cell::new(0),
            first_frame_sent: Cell::new(false),
            min_produce_interval: Cell::new(None),
            last_produce_at: Cell::new(None),
            produced: Cell::new(0),
            redraw_requested: Cell::new(false),
            last_compositor_tick: Cell::new(None),
            last_compositor_tick_interval: Cell::new(None),
            last_presentation: Cell::new(None),
            deferred_hidden: Cell::new(0),
            deferred_backpressure: Cell::new(0),
            pending_input_epochs: RefCell::new(PendingInputEpochs::default()),
            history: RefCell::new(FrameHistory::default()),
        }
    }

    /// This clock's current physical time — the real OS clock under
    /// [`ClockSource::Platform`], or the virtual timeline under
    /// [`ClockSource::Manual`]. Callers pass this straight into
    /// [`poll`](Self::poll) so a vsync tick and the produce decision it
    /// gates observe the identical instant.
    #[must_use]
    pub fn now(&self) -> Instant {
        MonotonicClock::now(&self.source)
    }

    /// Move a [`ClockSource::Manual`] clock forward by `dt`.
    ///
    /// # Panics
    ///
    /// Panics under [`ClockSource::Platform`] — advancing the real clock is
    /// not a thing a caller does, and a test harness that tries almost
    /// certainly has a wiring bug (an id installed with the wrong source, or
    /// never installed at all) that must fail loudly rather than silently
    /// do nothing.
    pub fn advance(&self, dt: Duration) {
        match &self.source {
            ClockSource::Platform => {
                panic!(
                    "FrameClock::advance called on a ClockSource::Platform clock -- \
                     advancing the real clock is not a thing a caller does; construct \
                     this FrameClock with ClockSource::Manual if it needs to be driven"
                );
            }
            ClockSource::Manual(clock) => clock.advance(dt),
        }
    }

    /// This clock's source, for a caller that needs to tell manual from
    /// platform apart (e.g. a diagnostic overlay).
    #[must_use]
    pub fn source(&self) -> &ClockSource {
        &self.source
    }

    // ------------------------------------------------------------------
    // Demand
    // ------------------------------------------------------------------

    /// Add `kind` to the current demand mask.
    pub fn mark_demand(&self, kind: DemandKind) {
        self.demand.set(self.demand.get() | DemandMask::from(kind));
    }

    /// Remove `kind` from the current demand mask — used when a demand
    /// source settles (e.g. an `AnimationController` completing clears its
    /// own [`DemandKind::Animation`] bit) without waiting for the next
    /// produce to clear the whole mask.
    ///
    /// If this clears the LAST remaining bit, also clears
    /// [`try_arm_redraw_request`](Self::try_arm_redraw_request)'s armed
    /// latch, not just the mask: a caller that marks demand (arming the
    /// latch), then settles it away via this method before a `poll` ever
    /// runs, has left nothing for a produce to consume and clear the latch
    /// through — without this, the next GENUINELY new mark would find the
    /// latch still armed from the settled-away demand and silently
    /// withhold its own platform-facing poke, stranding the actuator with
    /// no `poll` ever reachable to un-strand it (a `poll` call needs
    /// nonzero demand to grant a produce in the first place). Clearing to
    /// a still-nonzero mask leaves the latch exactly as it was — this is
    /// not a general "settling anything re-arms everything" rule, only
    /// the one case that would otherwise leak.
    pub fn clear_demand(&self, kind: DemandKind) {
        let cleared = self.demand.get() & !DemandMask::from(kind);
        self.demand.set(cleared);
        if cleared.is_empty() {
            self.redraw_requested.set(false);
        }
    }

    /// The current demand mask, unmodified.
    #[must_use]
    pub fn demand_mask(&self) -> DemandMask {
        self.demand.get()
    }

    // ------------------------------------------------------------------
    // Actuator edge — the platform-facing `request_redraw()` poke a caller
    // makes in response to demand, coalesced against this clock's own
    // mask so N marks between two produces collapse into exactly one
    // poke.
    // ------------------------------------------------------------------

    /// Test-and-set: returns `true` at most once per pending demand mask —
    /// the edge a caller should react to by poking the platform's
    /// `request_redraw()` — and `false` on every following call until the
    /// next granted [`poll`](Self::poll) produce (which clears both the
    /// mask and this latch) re-arms it. Also `false` while
    /// [`is_hidden`](Self::is_hidden) or with an empty mask — there is
    /// nothing to poke for.
    ///
    /// This is what turns N repeated demand marks (several ticks landing
    /// before a produce clears the mask — under backpressure, inside a
    /// [`set_min_produce_interval`](Self::set_min_produce_interval) window,
    /// or just several dirty marks in a row) into exactly one
    /// platform-facing request instead of a storm of redundant ones: a
    /// caller marks demand via [`mark_demand`](Self::mark_demand) as
    /// always, then separately consults this method whenever it is about
    /// to decide whether to poke the platform.
    ///
    /// Mutates on a `true` return (arms the latch) — reading it twice in a
    /// row without an intervening `poll` therefore returns `true` then
    /// `false`, never `true` twice.
    ///
    /// **Reaching `poll` is necessary but not sufficient to disarm this
    /// latch — say so honestly rather than claim it is handled.** Only a
    /// GRANTED produce (`poll` returning `Produce`/`ProduceWithheld`)
    /// clears the latch (alongside the mask, in the same call); a `poll`
    /// that returns `Skip(Hidden)` or `Skip(Backpressure)` retains BOTH
    /// the mask and this latch untouched, so a caller stuck on either of
    /// those two reasons stays armed-but-withheld until *something*
    /// causes a produce to actually succeed. This clock has no mechanism
    /// of its own to make that happen — the two edges that close the gap
    /// structurally live in the caller, not here.
    ///
    /// **`Hidden` is now wired and closed:** `flui-app`'s
    /// `UiRuntime::set_presentation_hidden` calls
    /// [`set_hidden`](Self::set_hidden) from production
    /// (`PlatformToUi::WindowVisibility`), and its own unhide branch is the
    /// closing edge — it wakes the platform loop UNCONDITIONALLY when the
    /// retained mask is nonempty, deliberately NOT gated on this latch's own
    /// return value (the latch may already be armed from a mark that
    /// predates the hide, a poke the loop already delivered and wasted
    /// against a `Skip(Hidden)` poll — trusting it here would incorrectly
    /// stay silent and strand the presentation). So a caller stuck on
    /// `Skip(Hidden)` today is bounded: it un-strands at the next unhide,
    /// not "whenever something else happens to wake the loop".
    ///
    /// **`Backpressure` comes only from the throttle**
    /// ([`set_min_produce_interval`](Self::set_min_produce_interval)), which
    /// a later pump past the interval clears. A caller that settles its OWN
    /// demand away without ever reaching a produce (rather than being
    /// blocked by hidden/backpressure) is a different, already-handled
    /// case: see [`clear_demand`](Self::clear_demand)'s doc.
    pub fn try_arm_redraw_request(&self) -> bool {
        if self.hidden.get() || self.demand.get().is_empty() || self.redraw_requested.get() {
            return false;
        }
        self.redraw_requested.set(true);
        true
    }

    // ------------------------------------------------------------------
    // Compositor pacing feedback — a caller that owns an actual display
    // surface feeds this clock the instants a compositor-paced signal (a
    // Wayland frame callback delivering `RedrawRequested`, for instance)
    // arrived. Bookkeeping ONLY: this clock never reaches out to read a
    // display's refresh rate itself, and recording a tick NEVER marks
    // demand — a tick fires on every production pump on the desktop
    // backends (see `record_compositor_tick`'s own doc for why marking
    // `DemandKind::Host` here was tried and reverted), so it cannot be
    // allowed to be sufficient demand on its own without making the
    // demand-mask gate — the entire point of this clock —
    // inert on every platform that calls it.
    // ------------------------------------------------------------------

    /// Records that a compositor/platform-paced tick arrived at `now` —
    /// the pacing-feedback half of the driver-loop hybrid.
    /// Updates [`last_compositor_tick_interval`](Self::last_compositor_tick_interval)
    /// from the previous recorded tick (if any). Marks NO demand: an
    /// earlier version of this method also marked `DemandKind::Host`
    /// ("the host platform asked for a frame directly", per that
    /// variant's own doc) on the theory that the compositor only ever
    /// delivers this tick in response to an earlier `request_redraw()`
    /// call already made for a real reason — which is true for a
    /// self-triggered tick, but FALSE in general: winit also delivers
    /// `RedrawRequested` for OS-driven signals this clock's caller never
    /// asked for (an expose/damage event, a resize-triggered redraw), and
    /// nothing at this call site can tell a self-triggered tick apart
    /// from an unsolicited one. Since this method fires on every
    /// production pump on desktop, marking demand there made `poll`
    /// unable to ever return `Skip(NoDemand)` on that path — the
    /// demand-mask gate this whole clock exists to provide would have
    /// been inert on every backend that calls this method. If a genuinely
    /// unsolicited tick ever needs to become its own demand source, that
    /// requires the caller to distinguish "unsolicited" from
    /// "consequence of our own `request_redraw()`" BEFORE calling this
    /// method (this clock cannot do it from inside `record_compositor_tick`
    /// itself, since by the time this runs the two cases are
    /// indistinguishable) and mark `DemandKind::Host` itself, explicitly,
    /// only in the unsolicited case — not a change made here.
    pub fn record_compositor_tick(&self, now: Instant) {
        if let Some(last) = self.last_compositor_tick.get() {
            self.last_compositor_tick_interval
                .set(Some(now.duration_since(last)));
        }
        self.last_compositor_tick.set(Some(now));
    }

    /// The interval between the two most recent
    /// [`record_compositor_tick`](Self::record_compositor_tick) calls —
    /// `None` until at least two ticks have been recorded. Diagnostic/
    /// telemetry data only; no produce decision reads this.
    #[must_use]
    pub fn last_compositor_tick_interval(&self) -> Option<Duration> {
        self.last_compositor_tick_interval.get()
    }

    // ------------------------------------------------------------------
    // Presentation feedback — when the platform says this presentation's
    // frames reached the screen (DXGI frame statistics, Metal's presented
    // handler, `wp_presentation_time`, Android's frame timelines), fed in by
    // the caller that receives the host's frame requests. It marks no demand
    // and moves no frame time: the frame tick stays the vsync instant
    // (ADR-0257 decision 2), never a presentation time.
    // ------------------------------------------------------------------

    /// Record that the platform reported a frame of this presentation shown
    /// at `report.at`. A report no later than the one kept is a repeat or out
    /// of order and is ignored: the presentation timeline never runs
    /// backwards.
    pub fn record_presentation(&self, report: PresentationReport) {
        if self
            .last_presentation
            .get()
            .is_none_or(|last| report.at > last.at)
        {
            self.last_presentation.set(Some(report));
        }
    }

    /// The newest presentation report, if the platform has made one.
    #[must_use]
    pub fn last_presentation(&self) -> Option<PresentationReport> {
        self.last_presentation.get()
    }

    /// The first refresh strictly after `now` at which a frame can next reach
    /// the screen: on the grid of refreshes the newest report starts, one
    /// refresh interval apart, or the report itself when it lies after `now`.
    ///
    /// `None` without a report, without a refresh interval in it, or past
    /// [`PRESENTATION_GRID_HORIZON`] refreshes from it: the grid is the
    /// display's as last reported, not a prediction to carry indefinitely.
    #[must_use]
    pub fn next_presentation_after(&self, now: Instant) -> Option<Instant> {
        let report = self.last_presentation.get()?;
        if report.at > now {
            return Some(report.at);
        }
        let refresh = report.refresh?.as_nanos();
        if refresh == 0 {
            return None;
        }
        let refreshes = now.duration_since(report.at).as_nanos() / refresh + 1;
        if refreshes > u128::from(PRESENTATION_GRID_HORIZON) {
            return None;
        }
        let offset = u64::try_from(refreshes * refresh).ok()?;
        report.at.checked_add(Duration::from_nanos(offset))
    }

    // ------------------------------------------------------------------
    // Visibility
    // ------------------------------------------------------------------

    /// Mark this presentation hidden or visible. While hidden, every
    /// [`poll`](Self::poll) call returns `Skip(Hidden)` with demand retained
    /// — becoming visible again with a nonzero mask produces on the very
    /// next poll.
    pub fn set_hidden(&self, hidden: bool) {
        self.hidden.set(hidden);
    }

    /// Whether this presentation is currently marked hidden.
    #[must_use]
    pub fn is_hidden(&self) -> bool {
        self.hidden.get()
    }

    // ------------------------------------------------------------------
    // First-frame deferral — withholds the submit, never the segment; see
    // the module doc.
    // ------------------------------------------------------------------

    /// Defer sending a produced frame to the engine until a matching
    /// [`lift`](Self::lift). Stacks: two `defer` calls need two `lift`
    /// calls before a produce is no longer withheld.
    ///
    /// The count is incremented UNCONDITIONALLY, even after the first frame
    /// has already been confirmed sent — this call never becomes a no-op.
    /// What changes is [`is_deferred`](Self::is_deferred)'s observable
    /// answer: once [`has_sent_first_frame`](Self::has_sent_first_frame) is
    /// `true`, it stays masked (`false`) regardless of the retained count,
    /// so a `defer` issued after the first frame has shipped is retained
    /// but has no effect — until
    /// [`reset_first_frame_sent`](Self::reset_first_frame_sent)
    /// deliberately un-masks it, at which
    /// point every `defer` ever called (including ones issued after the
    /// first frame shipped) becomes active again. The deferral count is
    /// masked by the first-frame-sent flag, never cleared, and
    /// `reset_first_frame_sent` exists specifically so a test's later
    /// `defer`/`lift` calls have an effect again.
    ///
    /// Deferring does NOT stop [`poll`](Self::poll) from running the
    /// segment: a nonzero demand mask still returns
    /// [`PollDecision::ProduceWithheld`], never a `Skip`.
    pub fn defer(&self) {
        self.deferred_count.set(self.deferred_count.get() + 1);
    }

    /// Undo one [`defer`](Self::defer) call.
    ///
    /// # Panics
    ///
    /// Panics if called without a matching prior `defer()` — a caller-
    /// contract violation, mirroring the oracle's
    /// `assert(_firstFrameDeferredCount > 0)`.
    pub fn lift(&self) {
        let prev = self.deferred_count.get();
        assert!(prev > 0, "FrameClock::lift called without a matching defer");
        self.deferred_count.set(prev - 1);
    }

    /// Whether a produced frame must currently be withheld from the engine.
    #[must_use]
    pub fn is_deferred(&self) -> bool {
        self.deferred_count.get() > 0 && !self.first_frame_sent.get()
    }

    /// Whether this clock has ever confirmed a frame sent to the engine
    /// while undeferred (the oracle's `_firstFrameSent`). Once `true`,
    /// further [`defer`](Self::defer) calls cannot withhold a produce.
    #[must_use]
    pub fn has_sent_first_frame(&self) -> bool {
        self.first_frame_sent.get()
    }

    /// Test/embedder escape hatch: pretend no frame has been sent yet, so a
    /// fresh `defer`/`lift` pair has an effect again.
    pub fn reset_first_frame_sent(&self) {
        self.first_frame_sent.set(false);
    }

    /// Latch that the first frame has been sent — the caller's job, not
    /// `poll`'s: `poll` grants a produce before the segment it gates has
    /// actually run, so it cannot yet know whether that segment will
    /// succeed, and it cannot know whether the caller actually submitted
    /// (a `ProduceWithheld` result never should be). Call this AFTER a
    /// `Produce` (not `ProduceWithheld`) segment completes without error —
    /// an errored or withheld attempt must not latch, or a later `defer`
    /// could never block a genuine future produce again. Idempotent.
    pub fn mark_first_frame_sent(&self) {
        self.first_frame_sent.set(true);
    }

    // ------------------------------------------------------------------
    // Produce throttle — a self-imposed minimum interval between produces.
    // GPU in-flight capacity is bounded by the surface's configured frame
    // latency, not by this clock.
    // ------------------------------------------------------------------

    /// Configure a minimum interval between produces (a self-imposed
    /// throttle, e.g. a target frame rate lower than the demand cadence).
    /// `None` (the default) imposes no throttle.
    pub fn set_min_produce_interval(&self, interval: Option<Duration>) {
        self.min_produce_interval.set(interval);
    }

    fn has_capacity(&self, now: Instant) -> bool {
        match (self.min_produce_interval.get(), self.last_produce_at.get()) {
            (Some(interval), Some(last)) => now.saturating_duration_since(last) >= interval,
            _ => true,
        }
    }

    // ------------------------------------------------------------------
    // The produce decision
    // ------------------------------------------------------------------

    /// How many times this clock has ever granted a produce (`Produce` OR
    /// `ProduceWithheld` — a pipeline-run count, the same "ran regardless
    /// of whether anything reached the screen" semantics
    /// `PresentationState::flush_count` uses at the app layer). A plain
    /// counter, always available (not a test-only oracle).
    #[must_use]
    pub fn produced_count(&self) -> u64 {
        self.produced.get()
    }

    /// The produce/skip decision for physical instant `now`.
    ///
    /// Checked in this order: the demand mask FIRST, then hidden, then
    /// the throttle. An empty mask is ALWAYS
    /// `Skip(NoDemand)`, regardless of whether hidden or a throttle
    /// happens to be configured — hidden and capacity are
    /// reasons to defer demand that exists, not reasons to invent a demand
    /// that doesn't; checking either before the mask would misreport an
    /// idle clock (no demand at all) as deferred whenever a caller had ALSO
    /// hidden the presentation or configured a throttle, which is wrong on
    /// its own terms and would undermine both the demand-driven-idle
    /// invariant this clock exists to prove (idle must read
    /// `Skip(NoDemand)` on every pump, never `Skip(Hidden)`/
    /// `Skip(Backpressure)`, however hidden/capacity happen to be
    /// configured) AND the deferral-accounting invariant the module doc's
    /// "Deferral accounting" section states: `hidden_deferrals`/
    /// `backpressure_deferrals` count demand that was RETAINED, never an
    /// idle poll that had nothing to retain. A nonzero mask always runs the
    /// segment (clearing the mask and recording `now` as the last produce
    /// instant) — first-frame deferral does NOT change whether the segment
    /// runs; it only changes which of [`PollDecision::Produce`]/
    /// [`PollDecision::ProduceWithheld`] comes back, so the caller knows
    /// whether it may submit the result. Every `Skip` reason retains the
    /// mask untouched, except [`SkipReason::NoDemand`] — there is nothing
    /// to retain there, it IS the empty mask.
    ///
    /// `now` is the caller's own responsibility to obtain from
    /// [`Self::now`] (or a shared instant several clocks/tickers observe
    /// together this same pump) — `poll` itself never reads a clock source,
    /// which is what makes a [`ClockSource::Manual`] clock's decisions
    /// exactly replayable.
    pub fn poll(&self, now: Instant) -> PollDecision {
        if self.demand.get().is_empty() {
            return PollDecision::Skip(SkipReason::NoDemand);
        }
        if self.hidden.get() {
            self.deferred_hidden.set(self.deferred_hidden.get() + 1);
            return PollDecision::Skip(SkipReason::Hidden);
        }
        if !self.has_capacity(now) {
            self.deferred_backpressure
                .set(self.deferred_backpressure.get() + 1);
            return PollDecision::Skip(SkipReason::Backpressure);
        }

        self.demand.set(DemandMask::empty());
        self.redraw_requested.set(false);
        self.last_produce_at.set(Some(now));
        self.produced.set(self.produced.get() + 1);

        if self.is_deferred() {
            PollDecision::ProduceWithheld
        } else {
            PollDecision::Produce
        }
    }

    // ------------------------------------------------------------------
    // Deferral accounting — see the module doc's "Deferral accounting"
    // section for why this is a separate counter family from a caller's
    // own "frames dropped" accounting.
    // ------------------------------------------------------------------

    /// How many `poll` calls returned `Skip(Hidden)` — demand deferred,
    /// never dropped.
    #[must_use]
    pub fn hidden_deferrals(&self) -> u64 {
        self.deferred_hidden.get()
    }

    /// How many `poll` calls returned `Skip(Backpressure)` — demand
    /// deferred, never dropped.
    #[must_use]
    pub fn backpressure_deferrals(&self) -> u64 {
        self.deferred_backpressure.get()
    }

    /// The sum of every deferral reason (never `Skip(NoDemand)`, which is
    /// not a deferral — there was no demand to defer).
    #[must_use]
    pub fn produces_deferred(&self) -> u64 {
        self.deferred_hidden.get() + self.deferred_backpressure.get()
    }

    // ------------------------------------------------------------------
    // Telemetry — see `crate::frame_telemetry`'s module doc for the shape
    // and the zero-allocation argument.
    // ------------------------------------------------------------------

    /// Stamp a routed input event as having arrived at `arrival` (read from
    /// [`Self::now`], never the wall clock directly, so a
    /// [`ClockSource::Manual`] test gets a deterministic latency
    /// computation). Buffered until the next [`record_frame`](Self::record_frame)
    /// drains it into whichever frame actually carries this event's effect.
    pub fn stamp_input_epoch(&self, arrival: Instant) -> InputEpochId {
        self.pending_input_epochs.borrow_mut().stamp(arrival)
    }

    /// Record that this clock's presentation produced and submitted a
    /// frame: drains every pending input epoch into the new
    /// [`FrameSnapshot`], stores it in the fixed-capacity history ring, and
    /// returns it (handy for a caller that wants to trace-emit the exact
    /// value just recorded without a redundant `frames_since` pull).
    ///
    /// `presentation` is stamped onto the returned [`FrameSnapshot`] as-is —
    /// this clock does not know its own owning presentation's identity, so
    /// the caller (the one presentation-scoped instance that actually
    /// produced this frame) must supply it. Passing a DIFFERENT
    /// presentation's id here is exactly the misattribution bug this
    /// parameter exists to make impossible to reach silently: a caller
    /// must name whose clock this is, not default to "whichever
    /// presentation I happened to have a reference to."
    ///
    /// `frame_id` is deliberately derived from [`Self::produced_count`]
    /// (`FrameId::from_count(produced_count())`, valid because `poll` has already
    /// incremented it by the time a caller reaches its own submit point) —
    /// not a second, independent counter minted here, which would drift
    /// from `produced_count` the first time the two disagreed about what
    /// counts as a produce.
    ///
    /// Call this ONLY when a real submit happened (a segment that ran but
    /// was withheld/deferred/produced no damage this pump must NOT call
    /// this — its pending input epochs stay buffered for whichever later
    /// pump does submit, so an event is never attributed to a frame that
    /// never reached the screen).
    ///
    /// # Panics
    ///
    /// Panics if no frame has ever been produced by THIS clock
    /// (`produced_count() == 0`) — the precondition above already states in
    /// prose ("call this ONLY when a real submit happened"): a real submit
    /// only ever follows a `poll()` that returned `Produce`/
    /// `ProduceWithheld`, which increments `produced_count` before the
    /// caller can reach a submit point at all.
    pub fn record_frame(
        &self,
        presentation: PresentationId,
        times: FrameTimes,
        present_outcome: PresentOutcome,
    ) -> FrameSnapshot {
        let input_epochs = self.pending_input_epochs.borrow_mut().drain();
        self.record_frame_with_epochs(presentation, times, present_outcome, input_epochs)
    }

    /// Like [`Self::record_frame`], but for a submit attempt that FAILED on
    /// a path its own caller has already armed a retry for (e.g.
    /// `EngineError::SurfaceLost`) — reads the currently pending input
    /// epochs into the recorded `FrameSnapshot` WITHOUT draining them, so
    /// the eventual retry's own real [`Self::record_frame`] still finds
    /// them and can attribute them to the frame that actually reaches the
    /// screen. Without this, [`Self::record_frame`]'s drain on the failed
    /// attempt would leave the retry with nothing pending, and the inputs
    /// that arrived before the failure would never be attributed to any
    /// frame at all.
    ///
    /// The failed attempt's own recorded snapshot still reports every epoch
    /// pending at this instant (useful for diagnosing what a surface-lost
    /// episode affected); it is the SAME epochs the retry's snapshot will
    /// also report once it actually submits — deliberately, since both
    /// snapshots describe real attempts to deliver those inputs' effects,
    /// only one of which reached the screen.
    ///
    /// Same panic precondition as [`Self::record_frame`].
    pub fn record_frame_retaining_epochs(
        &self,
        presentation: PresentationId,
        times: FrameTimes,
        present_outcome: PresentOutcome,
    ) -> FrameSnapshot {
        let input_epochs = self.pending_input_epochs.borrow().peek();
        self.record_frame_with_epochs(presentation, times, present_outcome, input_epochs)
    }

    /// Shared core of [`Self::record_frame`]/[`Self::record_frame_retaining_epochs`]:
    /// mint this produce's `frame_id`, build the [`FrameSnapshot`], and
    /// store it in the history ring. The only difference between the two
    /// public callers is whether `input_epochs` came from a drain or a
    /// peek — a decision made by the caller, not this shared core.
    ///
    /// # Panics
    ///
    /// Panics if no frame has ever been produced by THIS clock
    /// (`produced_count() == 0`) — see the public callers' own docs for why
    /// that precondition always holds by the time either is reached.
    fn record_frame_with_epochs(
        &self,
        presentation: PresentationId,
        times: FrameTimes,
        present_outcome: PresentOutcome,
        input_epochs: InputEpochs,
    ) -> FrameSnapshot {
        let frame_id = FrameId::from_count(self.produced_count()).expect(
            "BUG: record_frame(_retaining_epochs) called before this clock's poll() ever \
             produced a frame -- only call this after a real submit attempt, whose produce \
             already incremented produced_count",
        );
        let snapshot = FrameSnapshot {
            presentation,
            frame_id,
            clock_timestamp: times.clock_timestamp,
            segment_start: times.segment_start,
            segment_end: times.segment_end,
            submit_at: times.submit_at,
            present_outcome,
            input_epochs,
            phases: times.phases,
        };
        self.history.borrow_mut().record(&snapshot);
        snapshot
    }

    /// Every retained [`FrameSnapshot`] with `frame_id` strictly greater
    /// than `since` (or every retained snapshot, if `since` is `None`),
    /// oldest first, as owned values. `Option<FrameId>` rather than a bare
    /// `FrameId` sentinel: frame ids are 1-based `NonZeroU64`
    /// under the hood, so there is no
    /// representable "before the first frame" `FrameId` to pass instead.
    ///
    /// Allocates a `Vec` — this is the pull side an app/devtools consumer
    /// calls, never the frame-production path itself (see the module doc).
    #[must_use]
    pub fn frames_since(&self, since: Option<FrameId>) -> Vec<FrameSnapshot> {
        self.history.borrow().since(since)
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manual() -> (FrameClock, ManualClock) {
        let clock = ManualClock::new();
        let frame_clock = FrameClock::with_source(ClockSource::Manual(clock.clone()));
        (frame_clock, clock)
    }

    // ----------------------------------------------------------------
    // Anti-vacuous: an empty mask skips, a nonzero one produces, with
    // nothing else gating.
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // Hidden — retains the mask. Ranks below NoDemand (an empty mask is
    // reported as NoDemand even while hidden) and above Backpressure.
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // Throttle — the capacity axis behind Backpressure.
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // First-frame deferral: the segment still runs (`ProduceWithheld`),
    // never a `Skip` -- see the module doc. Lift
    // with retained demand produces exactly once, immediately (kills "lift
    // without re-arm").
    // ----------------------------------------------------------------

    fn deferred_runs_the_segment_withheld_and_lift_with_retained_demand_produces_exactly_once() {
        let (clock, manual) = manual();
        clock.defer();

        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::ProduceWithheld,
            "deferred must still run the segment, just withhold the result"
        );
        manual.advance(Duration::from_millis(16));
        clock.mark_demand(DemandKind::Host);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::ProduceWithheld,
            "still deferred: the segment keeps running on new demand, still withheld"
        );

        clock.lift();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Produce,
            "lifted: the next poll with demand produces unwithheld"
        );
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand),
            "and exactly once -- the produce already cleared the mask"
        );
    }

    // ----------------------------------------------------------------
    // Mid-segment demand lands next pump (no lost frame).
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // Determinism: the same script twice yields identical decisions and
    // timestamps -- kills a stray `Instant::now()` inside poll.
    //
    // A bare `run() == run()` self-comparison is NOT enough for this: a
    // wall-clock read swapped in for the caller-supplied `now` is
    // consistently wrong in BOTH runs (each run's `Instant::now()` calls
    // land at slightly different wall-clock instants than the other run's,
    // but within a single run the decisions it drives are still internally
    // self-consistent), so `first == second` can still hold even though
    // `now` was never actually the value the test advanced the manual clock
    // to. `now` must be made LOAD-BEARING in the decision -- a configured
    // throttle does that -- and the expectation must be an ABSOLUTE,
    // hand-computed sequence, not just "the two runs agree".
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // No policy divergence: the manual-source decision table equals the
    // platform-source table over the same demand/hidden/deferred/capacity
    // matrix -- kills "test clock is a second, laxer produce path".
    // ----------------------------------------------------------------

    fn manual_and_platform_sources_agree_on_every_matrix_cell() {
        for hidden in [false, true] {
            for deferred in [false, true] {
                for demand in [DemandMask::empty(), DemandMask::DIRTY] {
                    let platform = FrameClock::new();
                    let (manual_clock, manual_source) = manual();

                    for clock in [&platform, &manual_clock] {
                        clock.set_hidden(hidden);
                        if deferred {
                            clock.defer();
                        }
                        if !demand.is_empty() {
                            clock.mark_demand(DemandKind::Dirty);
                        }
                    }

                    let platform_decision = platform.poll(platform.now());
                    let manual_decision = manual_clock.poll(manual_source.now());

                    assert_eq!(
                        platform_decision, manual_decision,
                        "hidden={hidden} deferred={deferred} demand={demand:?}: \
                         manual and platform sources must agree"
                    );
                }
            }
        }
    }

    // ----------------------------------------------------------------
    // In-flight vs. throttle both surface as Backpressure -- and are
    // independent axes (one clearing does not clear the other).
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // `is_deferred` / `mark_first_frame_sent` — the caller-driven latch a
    // segment's own success (not `poll`'s produce grant) controls.
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // `try_arm_redraw_request` — the driver-loop hybrid's actuator edge.
    // Anti-vacuous: an empty/hidden clock never arms, one arm per pending
    // mask, re-armed only after a genuine produce.
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // `record_compositor_tick` — pacing feedback.
    // ----------------------------------------------------------------

    /// Criterion 1 at the clock level: two independent clocks fed
    /// compositor ticks at different scripted cadences (60 Hz vs 144 Hz)
    /// over the identical wall-clock span produce proportionally --
    /// exactly the multi-refresh-rate independence a single shared clock
    /// could never reproduce (companion to `flui-testing`'s own
    /// multi-presentation cadence test, at the bare-clock level with no
    /// ui_runtime/vsync machinery at all). Demand is marked explicitly each
    /// iteration (`record_compositor_tick` marks none of its own, per the
    /// fix above) -- this mirrors production, where a compositor tick
    /// never arrives except in response to a `request_redraw()` already
    /// made for a real `Dirty`/`Animation` reason. The produce-count
    /// assertions below are therefore evidence about the demand mark, not
    /// about `record_compositor_tick`; what IS specific to
    /// `record_compositor_tick` -- and is asserted, not just exercised --
    /// is that `last_compositor_tick_interval` ends up matching each
    /// feed's own distinct cadence, proving the two clocks' pacing
    /// feedback stays independent, not just their produce counts.
    fn scripted_60hz_and_144hz_feeds_produce_independently_proportional_counts() {
        let (clock_60, manual_60) = manual();
        let (clock_144, manual_144) = manual();

        let span = Duration::from_secs(1);
        let step_60 = Duration::from_nanos(1_000_000_000 / 60);
        let step_144 = Duration::from_nanos(1_000_000_000 / 144);

        let mut elapsed = Duration::ZERO;
        let mut ticks_60 = 0u32;
        while elapsed + step_60 <= span {
            manual_60.advance(step_60);
            elapsed += step_60;
            clock_60.record_compositor_tick(manual_60.now());
            clock_60.mark_demand(DemandKind::Dirty);
            assert_eq!(clock_60.poll(manual_60.now()), PollDecision::Produce);
            ticks_60 += 1;
        }

        let mut elapsed = Duration::ZERO;
        let mut ticks_144 = 0u32;
        while elapsed + step_144 <= span {
            manual_144.advance(step_144);
            elapsed += step_144;
            clock_144.record_compositor_tick(manual_144.now());
            clock_144.mark_demand(DemandKind::Dirty);
            assert_eq!(clock_144.poll(manual_144.now()), PollDecision::Produce);
            ticks_144 += 1;
        }

        assert_eq!(ticks_60, 60);
        assert_eq!(ticks_144, 144);
        assert_eq!(clock_60.produced_count(), 60);
        assert_eq!(clock_144.produced_count(), 144);
        assert!(
            clock_144.produced_count() > clock_60.produced_count() * 2,
            "144 Hz must produce well over double the 60 Hz count across the identical span \
             (got 60Hz={}, 144Hz={})",
            clock_60.produced_count(),
            clock_144.produced_count()
        );

        // The property `record_compositor_tick` itself actually added:
        // each clock's own pacing feedback matches its own scripted
        // cadence exactly, independent of the other clock's.
        assert_eq!(
            clock_60.last_compositor_tick_interval(),
            Some(step_60),
            "the 60Hz clock's own recorded interval must match its own step exactly"
        );
        assert_eq!(
            clock_144.last_compositor_tick_interval(),
            Some(step_144),
            "the 144Hz clock's own recorded interval must match its own step exactly"
        );
        assert!(
            clock_144.last_compositor_tick_interval() < clock_60.last_compositor_tick_interval(),
            "the 144Hz clock's own recorded interval must be shorter than the 60Hz clock's \
             (144Hz={:?}, 60Hz={:?})",
            clock_144.last_compositor_tick_interval(),
            clock_60.last_compositor_tick_interval()
        );
    }

    #[test]
    fn frame_clock_gate_matrix() {
        crate::table_test::run_table(
            "frame_clock_gate_matrix",
            &[
                ("deferred_runs_the_segment_withheld_and_lift_with_retained_demand_produces_exactly_once", deferred_runs_the_segment_withheld_and_lift_with_retained_demand_produces_exactly_once as fn()),
                ("manual_and_platform_sources_agree_on_every_matrix_cell", manual_and_platform_sources_agree_on_every_matrix_cell as fn()),
                ("scripted_60hz_and_144hz_feeds_produce_independently_proportional_counts", scripted_60hz_and_144hz_feeds_produce_independently_proportional_counts as fn()),
            ],
        );
    }

    // ----------------------------------------------------------------
    // `clear_demand` disarming the actuator latch -- the
    // caller-contract gap `try_arm_redraw_request`'s own doc names: a
    // mark-then-settle-without-a-poll sequence must not permanently
    // strand a later, genuinely new mark.
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // `set_min_produce_interval` as a target-frame-rate throttle (the
    // raster-backpressure slice of this file's own module-header issue): a
    // caller-requested cadence lower than the feed's own rate is enforced
    // entirely through `poll`'s existing capacity check and the caller's
    // own next-wake scheduling — never a sleep or timer this clock owns.
    // A caller converts its target Hz into this knob's `Duration` itself
    // (`Duration::from_secs_f64(1.0 / hz)`); this test proves the cadence a
    // raster-side consumer would observe, with no cross-crate dependency
    // (flui-engine does not depend on flui-scheduler).
    // ----------------------------------------------------------------

    // ----------------------------------------------------------------
    // Telemetry: input->present attribution, coalescing, and deferral
    // accounting (vs. a caller's own "frames dropped" counter).
    // ----------------------------------------------------------------
}
