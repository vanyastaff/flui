//! [`FrameClock`] — the per-presentation, platform-free physical-time policy
//! state machine (issue #556, ADR-0044).
//!
//! # The three-owner split
//!
//! [`UpdateScheduler`](crate::UpdateScheduler) owns *logical* time (phases,
//! callback queues, the priority task queue) and makes no refresh-rate,
//! display, or surface assumption. `FrameClock` owns *physical* time for
//! **one presentation**: demand coalescing, first-frame deferral, visibility
//! gating, a produce-capacity threshold, and frame timestamps. It answers
//! exactly one question — "does this surface produce a frame now?" — and
//! never touches a phase, a callback, or an element tree. Raster capacity
//! (whether the GPU will accept another frame) is a third, still-separate
//! owner; `FrameClock`'s in-flight/throttle knobs exist so that owner has
//! somewhere to report backpressure, not so this clock reaches into a GPU
//! queue itself.
//!
//! # No public mode enum
//!
//! There is deliberately no `FrameClock` mode (`OnDemand`/`Continuous`/…).
//! Every produce decision is a pure function of the current
//! [`DemandMask`] plus three orthogonal gates (hidden, deferred, capacity) —
//! "idle" is simply the empty mask, not a state a caller sets.
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

use std::cell::Cell;

use flui_foundation::{ManualClock, MonotonicClock};
use web_time::{Duration, Instant};

bitflags::bitflags! {
    /// The set of reasons a presentation currently wants a frame.
    ///
    /// Sampled once per [`FrameClock::poll`] call and cleared on a granted
    /// [`PollDecision::Produce`] — a persistent demand (a still-running
    /// animation) is re-armed by its own owner every pump via
    /// [`FrameClock::mark_demand`], never retained by the clock itself across
    /// a produce.
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
        // Bit 3 is reserved for a future `Media` demand kind (§1d) —
        // deliberately unassigned until a consumer needs it.
    }
}

/// One reason a presentation wants the next frame — the vocabulary
/// [`FrameClock::mark_demand`]/[`clear_demand`](FrameClock::clear_demand)
/// accept, folded into a [`DemandMask`] bit.
///
/// `#[non_exhaustive]`: this is the demand *vocabulary*, not a mode — adding
/// `Media` later is additive, not a breaking match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DemandKind {
    /// A pending widget build or a dirty render node.
    Dirty,
    /// A running animation/ticker consumer.
    Animation,
    /// A direct host/platform request with no framework-side dirty state.
    Host,
}

impl From<DemandKind> for DemandMask {
    fn from(kind: DemandKind) -> Self {
        match kind {
            DemandKind::Dirty => DemandMask::DIRTY,
            DemandKind::Animation => DemandMask::ANIMATION,
            DemandKind::Host => DemandMask::HOST,
        }
    }
}

/// Why [`FrameClock::poll`] returned [`PollDecision::Skip`].
///
/// `#[non_exhaustive]`: a caller matches the reasons it cares about and
/// falls through on the rest; a new reason is additive, not breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkipReason {
    /// The demand mask is empty — nobody asked for a frame. The only reason
    /// that does NOT retain any mask bits (there are none to retain).
    NoDemand,
    /// This presentation is hidden ([`FrameClock::set_hidden`]); demand is
    /// retained so an unhide with a nonzero mask produces immediately.
    Hidden,
    /// Produce capacity is unavailable right now — either the in-flight
    /// count has reached [`FrameClock::set_max_in_flight`]'s threshold, or a
    /// caller-configured minimum produce interval
    /// ([`FrameClock::set_min_produce_interval`]) has not yet elapsed.
    /// Demand is retained.
    Backpressure,
    /// The first-frame deferral gate is active
    /// ([`FrameClock::defer`]/[`FrameClock::lift`]); demand is retained, so
    /// lifting the gate with a nonzero mask produces on the very next poll
    /// with no separate re-arm call.
    Deferred,
}

/// [`FrameClock::poll`]'s produce/skip decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PollDecision {
    /// Produce a frame now. The mask has been cleared and this poll's `now`
    /// recorded as the last produce instant.
    Produce,
    /// Do not produce this poll, for the given reason.
    Skip(SkipReason),
}

impl PollDecision {
    /// Shorthand for `matches!(self, PollDecision::Produce)`.
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

/// The per-presentation physical-time policy state machine (issue #556).
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
    max_in_flight: Cell<u8>,
    in_flight: Cell<u8>,
    min_produce_interval: Cell<Option<Duration>>,
    last_produce_at: Cell<Option<Instant>>,
    produced: Cell<u64>,
}

/// The default in-flight capacity before any raster-owner wiring configures
/// it explicitly — matches `RasterOptions::max_frames_in_flight`'s own
/// documented default (§8), so a `FrameClock` constructed with no explicit
/// call to [`FrameClock::set_max_in_flight`] already agrees with it.
const DEFAULT_MAX_IN_FLIGHT: u8 = 2;

impl FrameClock {
    /// A fresh clock reading the real platform clock, with an empty demand
    /// mask, not hidden, not deferred, and the default in-flight capacity.
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
            max_in_flight: Cell::new(DEFAULT_MAX_IN_FLIGHT),
            in_flight: Cell::new(0),
            min_produce_interval: Cell::new(None),
            last_produce_at: Cell::new(None),
            produced: Cell::new(0),
        }
    }

    /// This clock's current physical time — the real OS clock under
    /// [`ClockSource::Platform`], or the virtual timeline under
    /// [`ClockSource::Manual`]. Callers pass this straight into
    /// [`poll`](Self::poll) so a vsync tick and the produce decision it
    /// gates observe the identical instant.
    #[must_use]
    pub fn now(&self) -> Instant {
        match &self.source {
            ClockSource::Platform => Instant::now(),
            ClockSource::Manual(clock) => clock.now(),
        }
    }

    /// Move a [`ClockSource::Manual`] clock forward by `dt`. A no-op (traced
    /// at `debug`) under [`ClockSource::Platform`] — advancing the real
    /// clock is not a thing a caller does.
    pub fn advance(&self, dt: Duration) {
        match &self.source {
            ClockSource::Platform => {
                tracing::debug!(
                    "FrameClock::advance called on a ClockSource::Platform clock; no-op"
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
    pub fn clear_demand(&self, kind: DemandKind) {
        self.demand.set(self.demand.get() & !DemandMask::from(kind));
    }

    /// The current demand mask, unmodified.
    #[must_use]
    pub fn demand_mask(&self) -> DemandMask {
        self.demand.get()
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
    // First-frame deferral (R2 — folds `send_frames_to_engine`'s old
    // counter into this clock's own produce-gate vocabulary)
    // ------------------------------------------------------------------

    /// Defer producing until a matching [`lift`](Self::lift). Stacks: two
    /// `defer` calls need two `lift` calls before demand can produce again.
    /// A no-op count-wise once the first frame has ever produced — mirrors
    /// the oracle's "first-frame" naming: this gate cannot re-arm after a
    /// frame has shipped.
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

    /// Whether the first-frame deferral gate currently blocks a produce.
    #[must_use]
    pub fn is_deferred(&self) -> bool {
        self.deferred_count.get() > 0 && !self.first_frame_sent.get()
    }

    /// Whether this clock has ever produced a frame while undeferred (the
    /// oracle's `_firstFrameSent`). Once `true`, further [`defer`](Self::defer)
    /// calls cannot block a produce.
    #[must_use]
    pub fn has_sent_first_frame(&self) -> bool {
        self.first_frame_sent.get()
    }

    /// Test/embedder escape hatch: pretend no frame has been sent yet, so a
    /// fresh `defer`/`lift` pair has an effect again. Mirrors
    /// `RenderingFlutterBinding::reset_first_frame_sent`.
    pub fn reset_first_frame_sent(&self) {
        self.first_frame_sent.set(false);
    }

    /// Latch that the first frame has been sent — the caller's job, not
    /// `poll`'s: `poll` grants `Produce` before the segment it gates has
    /// actually run, so it cannot yet know whether that segment will
    /// succeed. Call this AFTER a granted produce's segment completes
    /// without error (mirroring `RenderingFlutterBinding::mark_first_frame_sent`'s
    /// old `!errored` guard) — an errored first attempt must not latch, or a
    /// later `defer` could never block it again. Idempotent.
    pub fn mark_first_frame_sent(&self) {
        self.first_frame_sent.set(true);
    }

    /// Whether a presentation should hand its produced frame to the engine
    /// right now — `true` once the first frame has ever shipped, or while no
    /// deferral is active. Exactly `!is_deferred()`; kept as its own named
    /// query because it answers a different question a caller asks at a
    /// different point (before deciding whether to submit, not before
    /// deciding whether to produce).
    #[must_use]
    pub fn should_send_to_engine(&self) -> bool {
        !self.is_deferred()
    }

    // ------------------------------------------------------------------
    // Produce capacity (in-flight threshold + optional throttle) — the
    // knobs a raster owner's backpressure (§8, PR-E) reports through;
    // this clock never reaches into a GPU queue itself.
    // ------------------------------------------------------------------

    /// Configure the in-flight capacity threshold (`RasterOptions.
    /// max_frames_in_flight`'s clock-side counterpart, §8). Clamped to at
    /// least 1 — a zero threshold would make `poll` permanently
    /// non-producing with no way to ever recover.
    pub fn set_max_in_flight(&self, max: u8) {
        self.max_in_flight.set(max.max(1));
    }

    /// Record that a frame was submitted (increments the in-flight count).
    pub fn record_submit(&self) {
        self.in_flight.set(self.in_flight.get().saturating_add(1));
    }

    /// Record that a submitted frame retired — presented, errored, or
    /// dropped at shutdown/device-loss (§8's three decrement sites, all
    /// funneled through one RAII ticket upstream of this call).
    pub fn record_retire(&self) {
        self.in_flight.set(self.in_flight.get().saturating_sub(1));
    }

    /// The current in-flight count.
    #[must_use]
    pub fn in_flight(&self) -> u8 {
        self.in_flight.get()
    }

    /// Configure a minimum interval between produces (a self-imposed
    /// throttle, independent of GPU in-flight capacity — e.g. a
    /// `target_frame_rate` pace lower than the demand cadence, §8/PR-E).
    /// `None` (the default) imposes no throttle.
    pub fn set_min_produce_interval(&self, interval: Option<Duration>) {
        self.min_produce_interval.set(interval);
    }

    fn has_capacity(&self, now: Instant) -> bool {
        if self.in_flight.get() >= self.max_in_flight.get() {
            return false;
        }
        if let Some(interval) = self.min_produce_interval.get()
            && let Some(last) = self.last_produce_at.get()
            && now.duration_since(last) < interval
        {
            return false;
        }
        true
    }

    // ------------------------------------------------------------------
    // The produce decision
    // ------------------------------------------------------------------

    /// How many frames this clock has ever granted `Produce` for. A plain
    /// counter, always available (not a test-only oracle) — the same
    /// per-presentation accounting `PresentationState::frames_rendered`
    /// keeps at the render-result layer, kept here at the produce-decision
    /// layer instead.
    #[must_use]
    pub fn produced_count(&self) -> u64 {
        self.produced.get()
    }

    /// The produce/skip decision for physical instant `now`.
    ///
    /// Checked in this order: hidden, then capacity (in-flight/throttle),
    /// then first-frame deferral, then the demand mask. A granted
    /// [`PollDecision::Produce`] clears the mask and records `now` as the
    /// last produce instant — it does NOT latch
    /// [`has_sent_first_frame`](Self::has_sent_first_frame); see
    /// [`mark_first_frame_sent`](Self::mark_first_frame_sent)'s doc for why
    /// that is the caller's job. Every `Skip` reason except
    /// [`SkipReason::NoDemand`] retains the mask untouched (there is
    /// nothing to retain for `NoDemand` — it IS the empty mask).
    ///
    /// `now` is the caller's own responsibility to obtain from
    /// [`Self::now`] (or a shared instant several clocks/tickers observe
    /// together this same pump) — `poll` itself never reads a clock source,
    /// which is what makes a [`ClockSource::Manual`] clock's decisions
    /// exactly replayable.
    pub fn poll(&self, now: Instant) -> PollDecision {
        if self.hidden.get() {
            return PollDecision::Skip(SkipReason::Hidden);
        }
        if !self.has_capacity(now) {
            return PollDecision::Skip(SkipReason::Backpressure);
        }
        if self.is_deferred() {
            return PollDecision::Skip(SkipReason::Deferred);
        }
        if self.demand.get().is_empty() {
            return PollDecision::Skip(SkipReason::NoDemand);
        }

        self.demand.set(DemandMask::empty());
        self.last_produce_at.set(Some(now));
        self.produced.set(self.produced.get() + 1);
        PollDecision::Produce
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

    #[test]
    fn empty_mask_skips_with_no_demand_reason() {
        let (clock, manual) = manual();
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand)
        );
    }

    #[test]
    fn nonzero_mask_produces_and_clears_itself() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        assert!(
            clock.demand_mask().is_empty(),
            "a granted produce clears the mask"
        );
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand),
            "with nothing re-marked, the very next poll finds nothing to do"
        );
    }

    #[test]
    fn each_demand_kind_alone_is_sufficient_to_produce() {
        for kind in [DemandKind::Dirty, DemandKind::Animation, DemandKind::Host] {
            let (clock, manual) = manual();
            clock.mark_demand(kind);
            assert_eq!(
                clock.poll(manual.now()),
                PollDecision::Produce,
                "{kind:?} alone must be sufficient demand"
            );
        }
    }

    #[test]
    fn clearing_the_only_demand_kind_restores_no_demand() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Animation);
        clock.clear_demand(DemandKind::Animation);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand)
        );
    }

    // ----------------------------------------------------------------
    // Hidden — retains the mask, outranks everything else.
    // ----------------------------------------------------------------

    #[test]
    fn hidden_skips_even_with_demand_and_retains_it() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        clock.set_hidden(true);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Hidden)
        );
        assert!(
            !clock.demand_mask().is_empty(),
            "hidden must retain demand, not discard it"
        );

        clock.set_hidden(false);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Produce,
            "unhiding with retained demand must produce immediately"
        );
    }

    // ----------------------------------------------------------------
    // Backpressure (in-flight) — retains the mask.
    // ----------------------------------------------------------------

    #[test]
    fn in_flight_at_capacity_skips_with_backpressure_and_retains_demand() {
        let (clock, manual) = manual();
        clock.set_max_in_flight(1);
        clock.record_submit();
        clock.mark_demand(DemandKind::Dirty);

        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Backpressure)
        );
        assert!(!clock.demand_mask().is_empty());

        clock.record_retire();
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Produce,
            "retiring the in-flight frame must reopen capacity with demand already latched"
        );
    }

    // ----------------------------------------------------------------
    // Throttle — a second, independent capacity axis, also Backpressure.
    // ----------------------------------------------------------------

    #[test]
    fn throttled_interval_skips_with_backpressure_reason() {
        let (clock, manual) = manual();
        clock.set_min_produce_interval(Some(Duration::from_millis(33)));
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);

        manual.advance(Duration::from_millis(10));
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Backpressure),
            "10ms into a 33ms throttle window must not produce yet"
        );

        manual.advance(Duration::from_millis(30));
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Produce,
            "40ms after the last produce, past the 33ms window, must produce"
        );
    }

    // ----------------------------------------------------------------
    // Deferred (first-frame) — retains the mask; lift with retained mask
    // produces exactly once, immediately (kills "lift without re-arm").
    // ----------------------------------------------------------------

    #[test]
    fn deferred_skips_and_lift_with_retained_demand_produces_exactly_once() {
        let (clock, manual) = manual();
        clock.defer();

        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Deferred)
        );
        manual.advance(Duration::from_millis(16));
        clock.mark_demand(DemandKind::Host);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Deferred),
            "still deferred: dirty marks accumulate but never produce"
        );

        clock.lift();
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Produce,
            "lift with retained demand must produce on the very next poll, no re-arm call"
        );
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand),
            "and exactly once -- the produce already cleared the mask"
        );
    }

    #[test]
    #[should_panic(expected = "matching defer")]
    fn lift_without_a_matching_defer_panics() {
        let (clock, _manual) = manual();
        clock.lift();
    }

    #[test]
    fn stacked_defers_need_matching_lifts() {
        let (clock, manual) = manual();
        clock.defer();
        clock.defer();
        clock.mark_demand(DemandKind::Dirty);

        clock.lift();
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Deferred),
            "one lift against two defers must still block"
        );

        clock.lift();
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
    }

    #[test]
    fn defer_after_the_first_frame_has_no_effect() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        // `poll` itself never latches -- that is the caller's job, done only
        // once the segment it gated is known to have succeeded (see
        // `mark_first_frame_sent`'s doc).
        assert!(!clock.has_sent_first_frame());
        clock.mark_first_frame_sent();
        assert!(clock.has_sent_first_frame());

        clock.defer();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Produce,
            "a defer() issued after the first frame already shipped cannot re-arm the gate"
        );
    }

    // ----------------------------------------------------------------
    // Mid-segment demand lands next pump (no lost frame).
    // ----------------------------------------------------------------

    #[test]
    fn demand_marked_after_a_produce_lands_on_the_next_poll_not_the_last() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);

        // "Mid-segment" demand: something dirties the presentation again
        // right after the segment that just ran.
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Produce,
            "no lost frame: demand marked after a produce is observed on the very next poll"
        );
    }

    // ----------------------------------------------------------------
    // Determinism: the same script twice yields identical decisions and
    // timestamps -- kills a stray `Instant::now()` inside poll.
    // ----------------------------------------------------------------

    #[test]
    fn the_same_script_replayed_twice_is_byte_identical() {
        // Compared by ELAPSED duration, not the absolute `Instant`: two
        // separate `ManualClock`s each anchor `base` to the real wall clock
        // at construction (a nanosecond apart between the two runs), so the
        // raw `Instant` values themselves legitimately differ even though
        // the clock is deterministic -- what must be identical is how far
        // each poll's `now` sits from this run's own start, and the decision
        // made there.
        fn run() -> Vec<(PollDecision, Duration)> {
            let (clock, manual) = manual();
            let mut trace = Vec::new();
            for step in 0..5 {
                if step % 2 == 0 {
                    clock.mark_demand(DemandKind::Dirty);
                }
                manual.advance(Duration::from_millis(16));
                let now = clock.now();
                trace.push((clock.poll(now), manual.elapsed()));
            }
            trace
        }

        let first = run();
        let second = run();
        assert_eq!(
            first, second,
            "identical scripts must produce identical traces"
        );
    }

    // ----------------------------------------------------------------
    // No policy divergence: the manual-source decision table equals the
    // platform-source table over the same demand/hidden/deferred/capacity
    // matrix -- kills "test clock is a second, laxer produce path".
    // ----------------------------------------------------------------

    #[test]
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

    #[test]
    fn in_flight_and_throttle_are_independent_backpressure_sources() {
        let (clock, manual) = manual();
        clock.set_max_in_flight(1);
        clock.set_min_produce_interval(Some(Duration::from_millis(100)));
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);

        // Still within the throttle window AND nothing retired: both axes
        // would refuse independently.
        clock.record_submit();
        manual.advance(Duration::from_millis(10));
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Backpressure)
        );

        // Retire the in-flight frame but stay inside the throttle window:
        // still blocked, now purely by the throttle.
        clock.record_retire();
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Backpressure),
            "in-flight cleared, but the throttle window has not elapsed"
        );

        manual.advance(Duration::from_millis(100));
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
    }

    #[test]
    fn produced_count_tracks_granted_produces_only() {
        let (clock, manual) = manual();
        assert_eq!(clock.produced_count(), 0);

        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        assert_eq!(clock.produced_count(), 1);

        // A skip must not move the counter.
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand)
        );
        assert_eq!(clock.produced_count(), 1);
    }

    #[test]
    fn advance_on_a_platform_source_is_a_harmless_no_op() {
        let clock = FrameClock::new();
        // Must not panic; must not affect anything observable.
        clock.advance(Duration::from_millis(16));
        assert!(matches!(clock.source(), ClockSource::Platform));
    }

    // ----------------------------------------------------------------
    // `should_send_to_engine` / `mark_first_frame_sent` — the caller-driven
    // latch a segment's own success (not `poll`'s produce grant) controls.
    // ----------------------------------------------------------------

    #[test]
    fn an_unconfirmed_produce_does_not_latch_should_send_to_engine() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        // The caller's segment has not yet reported success -- a defer
        // issued right now must still take effect, exactly as it would if
        // this were genuinely the first attempt.
        clock.defer();
        assert!(
            !clock.should_send_to_engine(),
            "an unconfirmed produce must not have latched first_frame_sent"
        );
    }

    #[test]
    fn mark_first_frame_sent_makes_a_later_defer_inert() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        clock.mark_first_frame_sent();
        assert!(clock.should_send_to_engine());

        clock.defer();
        assert!(
            clock.should_send_to_engine(),
            "a defer issued after the confirmed first frame must not re-close the gate"
        );
    }

    #[test]
    fn should_send_to_engine_is_true_by_default_with_no_deferral_ever_registered() {
        let (clock, _manual) = manual();
        assert!(clock.should_send_to_engine());
    }
}
