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
//! first-frame deferral, visibility gating, a produce-capacity threshold,
//! and frame timestamps. It answers exactly one question — "does this
//! surface produce a frame now?" — and never touches a phase, a callback,
//! or an element tree, and never reaches out to read an actual display or
//! window: every physical-time fact it reasons about (a compositor tick,
//! a produce capacity signal) is fed in by a caller that owns the real
//! surface. Raster capacity (whether the GPU will accept another frame)
//! is a third, still-separate owner; `FrameClock`'s in-flight/throttle
//! knobs exist so that owner has somewhere to report backpressure, not so
//! this clock reaches into a GPU queue itself.
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
//! `.flutter/packages/flutter/lib/src/rendering/binding.dart`'s
//! `RendererBinding.deferFirstFrame` is explicit: "the framework will still
//! do all the work to produce frames, but those frames are never sent to the
//! engine and will not appear on screen" (binding.dart:582-599's
//! `sendFramesToEngine` doc: "Whether frames produced by `drawFrame` are sent
//! to the engine"). Deferral gates the **submit** only. [`FrameClock::poll`]
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
    FrameHistory, FrameSnapshot, InputEpochId, InputEpochs, PendingInputEpochs, PresentOutcome,
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
        // Bit 3 is reserved for a future media/video-frame demand kind —
        // deliberately unassigned until a consumer needs it.
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
    /// Produce capacity is unavailable right now — either the in-flight
    /// count has reached [`FrameClock::set_max_in_flight`]'s threshold, or a
    /// caller-configured minimum produce interval
    /// ([`FrameClock::set_min_produce_interval`]) has not yet elapsed.
    /// Demand is retained.
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
    /// ([`FrameClock::defer`]) is active. See the module doc's `.flutter/`
    /// citation — deferral withholds the submit, never the pipeline work.
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
    max_in_flight: Cell<u8>,
    in_flight: Cell<u8>,
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

/// The default in-flight capacity before any raster-owner wiring configures
/// it explicitly — matches the raster owner's own documented default, so a
/// `FrameClock` constructed with no explicit call to
/// [`FrameClock::set_max_in_flight`] already agrees with it.
const DEFAULT_MAX_IN_FLIGHT: u8 = 2;

/// The four [`FrameSnapshot`] timing fields [`FrameClock::record_frame`]/
/// [`FrameClock::record_frame_retaining_epochs`] pass through unchanged to
/// their shared private core — bundled purely to keep that core's own
/// arity under clippy's `too_many_arguments` bound; not part of either
/// public method's own signature.
struct FrameTiming {
    clock_timestamp: Instant,
    segment_start: Instant,
    segment_end: Instant,
    submit_at: Instant,
}

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
            redraw_requested: Cell::new(false),
            last_compositor_tick: Cell::new(None),
            last_compositor_tick_interval: Cell::new(None),
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
        match &self.source {
            ClockSource::Platform => Instant::now(),
            ClockSource::Manual(clock) => clock.now(),
        }
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
    /// `UiRealm::set_presentation_hidden` calls
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
    /// **`Backpressure` remains open** — no raster owner exists yet, so
    /// nothing calls [`set_max_in_flight`](Self::set_max_in_flight)/
    /// [`record_submit`](Self::record_submit) against a real presentation's
    /// clock, and `poll` never actually returns `Skip(Backpressure)` in
    /// production. Wiring either knob into production ahead of a real
    /// retire→wake edge (the raster-owner work this issue's later slice
    /// owns) would turn this dormant half into a real stall, the same shape
    /// `Hidden` had before it was closed. A caller that settles its OWN
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
    // the module doc's `.flutter/` citation.
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
    /// first frame shipped) becomes active again. This is exactly the
    /// oracle's own contract, not a simplification of it:
    /// `.flutter/packages/flutter/lib/src/rendering/binding.dart` —
    /// `deferFirstFrame` (:603-606) increments `_firstFrameDeferredCount`
    /// with no check on `_firstFrameSent` at all; `sendFramesToEngine`
    /// (:591) is `_firstFrameSent || _firstFrameDeferredCount == 0` (the
    /// count is masked, not cleared); `resetFirstFrameSent` (:627-634)
    /// exists specifically so a test's later `deferFirstFrame`/
    /// `allowFirstFrame` calls have an effect again.
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
    /// fresh `defer`/`lift` pair has an effect again. Mirrors
    /// `RenderingFlutterBinding::reset_first_frame_sent`.
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
    // Produce capacity (in-flight threshold + optional throttle) — the
    // knobs a raster owner's backpressure reports through; this clock
    // never reaches into a GPU queue itself.
    // ------------------------------------------------------------------

    /// Configure the in-flight capacity threshold (the raster owner's
    /// clock-side counterpart). Clamped to at least 1 — a zero threshold
    /// would make `poll` permanently non-producing with no way to ever
    /// recover.
    pub fn set_max_in_flight(&self, max: u8) {
        self.max_in_flight.set(max.max(1));
    }

    /// Record that a frame was submitted (increments the in-flight count).
    pub fn record_submit(&self) {
        self.in_flight.set(self.in_flight.get().saturating_add(1));
    }

    /// Record that a submitted frame retired — presented, errored, or
    /// dropped at shutdown/device-loss, all funneled through one RAII
    /// ticket upstream of this call.
    pub fn record_retire(&self) {
        self.in_flight.set(self.in_flight.get().saturating_sub(1));
    }

    /// The current in-flight count.
    #[must_use]
    pub fn in_flight(&self) -> u8 {
        self.in_flight.get()
    }

    /// Configure a minimum interval between produces (a self-imposed
    /// throttle, independent of GPU in-flight capacity — e.g. a target
    /// frame rate lower than the demand cadence). `None` (the default)
    /// imposes no throttle.
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
    /// capacity (in-flight/throttle). An empty mask is ALWAYS
    /// `Skip(NoDemand)`, regardless of whether hidden/a throttle/an
    /// in-flight limit happens to be configured — hidden and capacity are
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
    /// (`FrameId::zip(produced_count())`, valid because `poll` has already
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
        clock_timestamp: Instant,
        segment_start: Instant,
        segment_end: Instant,
        submit_at: Instant,
        present_outcome: PresentOutcome,
    ) -> FrameSnapshot {
        let input_epochs = self.pending_input_epochs.borrow_mut().drain();
        self.record_frame_with_epochs(
            presentation,
            FrameTiming {
                clock_timestamp,
                segment_start,
                segment_end,
                submit_at,
            },
            present_outcome,
            input_epochs,
        )
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
        clock_timestamp: Instant,
        segment_start: Instant,
        segment_end: Instant,
        submit_at: Instant,
        present_outcome: PresentOutcome,
    ) -> FrameSnapshot {
        let input_epochs = self.pending_input_epochs.borrow().peek();
        self.record_frame_with_epochs(
            presentation,
            FrameTiming {
                clock_timestamp,
                segment_start,
                segment_end,
                submit_at,
            },
            present_outcome,
            input_epochs,
        )
    }

    /// Shared core of [`Self::record_frame`]/[`Self::record_frame_retaining_epochs`]:
    /// mint this produce's `frame_id`, build the [`FrameSnapshot`], and
    /// store it in the history ring. The only difference between the two
    /// public callers is whether `input_epochs` came from a drain or a
    /// peek — a decision made by the caller, not this shared core. `timing`
    /// bundles the four `Instant` fields both callers pass through
    /// unchanged, keeping this function's own arity within clippy's
    /// `too_many_arguments` bound without changing either public caller's
    /// own (pre-existing, six-`Instant`-plus-enum) signature.
    ///
    /// # Panics
    ///
    /// Panics if no frame has ever been produced by THIS clock
    /// (`produced_count() == 0`) — see the public callers' own docs for why
    /// that precondition always holds by the time either is reached.
    fn record_frame_with_epochs(
        &self,
        presentation: PresentationId,
        timing: FrameTiming,
        present_outcome: PresentOutcome,
        input_epochs: InputEpochs,
    ) -> FrameSnapshot {
        let produced_count = self.produced_count();
        let frame_id = (produced_count > 0)
            .then(|| FrameId::zip(produced_count as usize))
            .expect(
                "BUG: record_frame(_retaining_epochs) called before this clock's poll() ever \
             produced a frame -- only call this after a real submit attempt, whose produce \
             already incremented produced_count",
            );
        let snapshot = FrameSnapshot {
            presentation,
            frame_id,
            clock_timestamp: timing.clock_timestamp,
            segment_start: timing.segment_start,
            segment_end: timing.segment_end,
            submit_at: timing.submit_at,
            present_outcome,
            input_epochs,
        };
        self.history.borrow_mut().record(&snapshot);
        snapshot
    }

    /// Every retained [`FrameSnapshot`] with `frame_id` strictly greater
    /// than `since` (or every retained snapshot, if `since` is `None`),
    /// oldest first, as owned values. `Option<FrameId>` rather than a bare
    /// `FrameId` sentinel: this workspace's IDs are 1-based `NonZeroUsize`
    /// under the hood (see `AGENTS.md`'s ID offset pattern), so there is no
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

    /// The unit-matrix cell that distinguishes checking demand before vs.
    /// after capacity: an EMPTY mask must read `Skip(NoDemand)`, never
    /// `Skip(Backpressure)`, regardless of whether a throttle or in-flight
    /// limit happens to be configured. Capacity is a reason to defer
    /// demand that exists, not a reason to invent demand that doesn't --
    /// and this is exactly the cell the demand-driven-idle invariant
    /// depends on (an idle clock must read `NoDemand` on every pump no
    /// matter how capacity is configured).
    #[test]
    fn empty_mask_with_capacity_configured_is_still_no_demand_not_backpressure() {
        // Throttle sub-case: a throttle with no prior produce is a no-op
        // capacity-wise either way (there is nothing to measure the window
        // against yet), so establish a real produce FIRST to arm the
        // window, then poll again with an EMPTY mask while still inside
        // it -- only THIS shape actually distinguishes checking demand
        // before vs. after capacity.
        let (throttled, throttled_clock) = manual();
        throttled.set_min_produce_interval(Some(Duration::from_millis(33)));
        throttled.mark_demand(DemandKind::Dirty);
        assert_eq!(throttled.poll(throttled_clock.now()), PollDecision::Produce);
        throttled_clock.advance(Duration::from_millis(10)); // still inside the 33ms window
        assert_eq!(
            throttled.poll(throttled_clock.now()),
            PollDecision::Skip(SkipReason::NoDemand),
            "an empty mask inside an active throttle window must still report NoDemand, \
             not Backpressure"
        );

        let (at_capacity, at_capacity_clock) = manual();
        at_capacity.set_max_in_flight(1);
        at_capacity.record_submit();
        assert_eq!(
            at_capacity.poll(at_capacity_clock.now()),
            PollDecision::Skip(SkipReason::NoDemand),
            "an empty mask with the in-flight limit already reached must still report NoDemand"
        );
    }

    // ----------------------------------------------------------------
    // First-frame deferral: the segment still runs (`ProduceWithheld`),
    // never a `Skip` -- see the module doc's `.flutter/` citation. Lift
    // with retained demand produces exactly once, immediately (kills "lift
    // without re-arm").
    // ----------------------------------------------------------------

    #[test]
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

    #[test]
    fn lift_with_no_new_demand_marked_finds_nothing_to_do() {
        // A `ProduceWithheld` poll already clears the mask (the segment DID
        // run) -- lifting afterward with nothing freshly marked must not
        // conjure a produce out of nothing.
        let (clock, manual) = manual();
        clock.defer();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::ProduceWithheld);

        clock.lift();
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand),
            "the mask was already cleared by the ProduceWithheld poll above"
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
            PollDecision::ProduceWithheld,
            "one lift against two defers must still withhold"
        );

        clock.lift();
        clock.mark_demand(DemandKind::Dirty);
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
    //
    // A bare `run() == run()` self-comparison is NOT enough for this: a
    // wall-clock read swapped in for the caller-supplied `now` is
    // consistently wrong in BOTH runs (each run's `Instant::now()` calls
    // land at slightly different wall-clock instants than the other run's,
    // but within a single run the decisions it drives are still internally
    // self-consistent), so `first == second` can still hold even though
    // `now` was never actually the value the test advanced the manual clock
    // to. `now` must be made LOAD-BEARING in the decision -- a configured
    // throttle is what `in_flight_and_throttle_are_independent_backpressure_
    // sources`/`throttled_interval_skips_with_backpressure_reason` above
    // already use for exactly this reason -- and the expectation must be an
    // ABSOLUTE, hand-computed sequence, not just "the two runs agree".
    // ----------------------------------------------------------------

    #[test]
    fn the_same_script_replayed_twice_matches_an_absolute_throttled_sequence() {
        fn run() -> Vec<PollDecision> {
            let (clock, manual) = manual();
            clock.set_min_produce_interval(Some(Duration::from_millis(33)));
            let mut trace = Vec::new();
            for _ in 0..5 {
                clock.mark_demand(DemandKind::Dirty);
                manual.advance(Duration::from_millis(16));
                trace.push(clock.poll(clock.now()));
            }
            trace
        }

        // Hand-computed against a 33ms throttle and a fixed 16ms step, from
        // a `None` `last_produce_at` (first poll always has capacity):
        //   t=16ms: no prior produce                    -> Produce (last=16)
        //   t=32ms: 32-16=16ms  < 33ms                   -> Skip(Backpressure)
        //   t=48ms: 48-16=32ms  < 33ms                   -> Skip(Backpressure)
        //   t=64ms: 64-16=48ms >= 33ms                    -> Produce (last=64)
        //   t=80ms: 80-64=16ms  < 33ms                   -> Skip(Backpressure)
        let expected = vec![
            PollDecision::Produce,
            PollDecision::Skip(SkipReason::Backpressure),
            PollDecision::Skip(SkipReason::Backpressure),
            PollDecision::Produce,
            PollDecision::Skip(SkipReason::Backpressure),
        ];

        let first = run();
        assert_eq!(
            first, expected,
            "the exact produce/skip sequence must match the scripted throttle timing -- \
             a clock reading real wall-clock time instead of the caller-supplied `now` \
             would not reliably reproduce this exact pattern"
        );

        let second = run();
        assert_eq!(
            first, second,
            "identical scripts must also produce identical traces run to run"
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
    fn produced_count_tracks_granted_produces_including_withheld_ones() {
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

        // A withheld produce still counts -- the segment genuinely ran.
        clock.defer();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::ProduceWithheld);
        assert_eq!(clock.produced_count(), 2);
    }

    #[test]
    #[should_panic(expected = "ClockSource::Platform")]
    fn advance_on_a_platform_source_panics() {
        let clock = FrameClock::new();
        clock.advance(Duration::from_millis(16));
    }

    // ----------------------------------------------------------------
    // `is_deferred` / `mark_first_frame_sent` — the caller-driven latch a
    // segment's own success (not `poll`'s produce grant) controls.
    // ----------------------------------------------------------------

    #[test]
    fn an_unconfirmed_produce_does_not_latch_is_deferred_open() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        // The caller's segment has not yet reported success -- a defer
        // issued right now must still take effect, exactly as it would if
        // this were genuinely the first attempt.
        clock.defer();
        assert!(
            clock.is_deferred(),
            "an unconfirmed produce must not have latched first_frame_sent"
        );
    }

    #[test]
    fn mark_first_frame_sent_makes_a_later_defer_inert() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        clock.mark_first_frame_sent();
        assert!(!clock.is_deferred());

        clock.defer();
        assert!(
            !clock.is_deferred(),
            "a defer issued after the confirmed first frame must not re-close the gate"
        );
    }

    /// The retained-not-cleared half of the same contract
    /// (`.flutter/packages/flutter/lib/src/rendering/binding.dart:627-634`'s
    /// `resetFirstFrameSent`, deliberately for tests that want a later
    /// `defer`/`lift` pair to matter again): a `defer` issued after the
    /// first frame shipped is retained, not discarded, so once
    /// `reset_first_frame_sent` un-masks it, the deferral becomes active.
    #[test]
    fn a_defer_issued_after_sent_is_retained_and_becomes_active_again_after_reset() {
        let (clock, manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        clock.mark_first_frame_sent();

        // Masked, not discarded: `defer` here is retained even though it
        // has no observable effect yet.
        clock.defer();
        assert!(
            !clock.is_deferred(),
            "masked while first_frame_sent holds -- not yet observable"
        );

        clock.reset_first_frame_sent();
        assert!(
            clock.is_deferred(),
            "the earlier defer() must have been retained, not discarded -- reset un-masks it"
        );

        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::ProduceWithheld,
            "the now-active retained deferral must withhold this produce"
        );
    }

    #[test]
    fn is_deferred_is_false_by_default_with_no_deferral_ever_registered() {
        let (clock, _manual) = manual();
        assert!(!clock.is_deferred());
    }

    // ----------------------------------------------------------------
    // `try_arm_redraw_request` — the driver-loop hybrid's actuator edge.
    // Anti-vacuous: an empty/hidden clock never arms, one arm per pending
    // mask, re-armed only after a genuine produce.
    // ----------------------------------------------------------------

    #[test]
    fn an_empty_mask_never_arms_a_redraw_request() {
        let (clock, _manual) = manual();
        assert!(!clock.try_arm_redraw_request());
    }

    #[test]
    fn a_hidden_clock_never_arms_a_redraw_request_even_with_demand() {
        let (clock, _manual) = manual();
        clock.set_hidden(true);
        clock.mark_demand(DemandKind::Dirty);
        assert!(
            !clock.try_arm_redraw_request(),
            "no reason to poke the platform for a surface that cannot produce"
        );
    }

    #[test]
    fn one_mark_arms_exactly_once_and_reads_false_after() {
        let (clock, _manual) = manual();
        clock.mark_demand(DemandKind::Dirty);
        assert!(clock.try_arm_redraw_request(), "the first call is the edge");
        assert!(
            !clock.try_arm_redraw_request(),
            "the SAME pending mask must not arm a second poke"
        );
        assert!(
            !clock.try_arm_redraw_request(),
            "reading it again changes nothing further -- it is not a toggle"
        );
    }

    /// The named exploit: N demand marks landing before a produce collapse
    /// into exactly one armed edge; a produce re-arms it. Scripted via
    /// backpressure so the mask genuinely survives several `poll` calls
    /// without clearing -- the realistic shape a caller sees under GPU
    /// backpressure or a throttle window, not just "call mark_demand
    /// twice with no poll in between".
    #[test]
    fn n_demands_before_a_produce_collapse_into_exactly_one_armed_request_then_rearm() {
        let (clock, manual) = manual();
        clock.set_max_in_flight(1);
        clock.record_submit(); // at capacity from the start -- every poll below skips

        // N=5 marks, each followed by a poll that skips (Backpressure) and
        // retains the mask -- only the FIRST arms.
        let mut armed_count = 0;
        for _ in 0..5 {
            clock.mark_demand(DemandKind::Dirty);
            if clock.try_arm_redraw_request() {
                armed_count += 1;
            }
            assert_eq!(
                clock.poll(manual.now()),
                PollDecision::Skip(SkipReason::Backpressure),
                "capacity never freed in this loop -- every poll must skip"
            );
        }
        assert_eq!(
            armed_count, 1,
            "exactly one of the five marks may arm a redraw request"
        );

        // Free capacity: the next poll produces, clearing both the mask
        // and the armed latch.
        clock.record_retire();
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);

        // Re-armed: a fresh mark after the produce arms again.
        clock.mark_demand(DemandKind::Dirty);
        assert!(
            clock.try_arm_redraw_request(),
            "a produce must re-arm the next genuinely new demand"
        );
    }

    #[test]
    fn skip_no_demand_never_arms_and_never_needed_to() {
        let (clock, manual) = manual();
        assert!(!clock.try_arm_redraw_request());
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand)
        );
        assert!(!clock.try_arm_redraw_request());
    }

    // ----------------------------------------------------------------
    // `record_compositor_tick` — pacing feedback.
    // ----------------------------------------------------------------

    /// The demand-mask gate's own load-bearing regression test: a
    /// compositor tick, alone, with no other demand ever marked, must
    /// leave `poll` reading `Skip(NoDemand)` -- proving
    /// `record_compositor_tick` marks no demand of its own. An earlier
    /// version of this method marked `DemandKind::Host` unconditionally,
    /// which -- because this method fires on every production pump on
    /// every desktop backend -- made `poll` permanently unable to return
    /// `Skip(NoDemand)` there, i.e. made the entire demand-mask gate
    /// inert on the one path that calls it. Repeats the tick 5 times
    /// (not just once) to rule out a one-shot exception.
    #[test]
    fn record_compositor_tick_alone_marks_no_demand_and_never_produces() {
        let (clock, manual) = manual();
        for _ in 0..5 {
            manual.advance(Duration::from_millis(16));
            clock.record_compositor_tick(manual.now());
            assert_eq!(
                clock.poll(manual.now()),
                PollDecision::Skip(SkipReason::NoDemand),
                "a compositor tick alone must never be sufficient demand on its own"
            );
        }
        assert_eq!(clock.produced_count(), 0);
    }

    #[test]
    fn compositor_tick_interval_is_none_until_a_second_tick_then_tracks_the_gap() {
        let (clock, manual) = manual();
        assert_eq!(clock.last_compositor_tick_interval(), None);

        clock.record_compositor_tick(manual.now());
        assert_eq!(
            clock.last_compositor_tick_interval(),
            None,
            "one tick alone has no interval to report yet"
        );

        manual.advance(Duration::from_millis(16));
        clock.record_compositor_tick(manual.now());
        assert_eq!(
            clock.last_compositor_tick_interval(),
            Some(Duration::from_millis(16))
        );

        manual.advance(Duration::from_millis(7));
        clock.record_compositor_tick(manual.now());
        assert_eq!(
            clock.last_compositor_tick_interval(),
            Some(Duration::from_millis(7)),
            "the interval tracks only the two most recent ticks, not a running average"
        );
    }

    /// Criterion 1 at the clock level: two independent clocks fed
    /// compositor ticks at different scripted cadences (60 Hz vs 144 Hz)
    /// over the identical wall-clock span produce proportionally --
    /// exactly the multi-refresh-rate independence a single shared clock
    /// could never reproduce (companion to `flui-testing`'s own
    /// multi-presentation cadence test, at the bare-clock level with no
    /// realm/vsync machinery at all). Demand is marked explicitly each
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
    #[test]
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

    /// Compositor-freeze loyalty: a scripted feed that stops delivering
    /// ticks must stop producing -- this clock has no background thread
    /// and no timer of its own, so "pausing the feed" and "pausing
    /// production" are the same fact, but this pins it so a future
    /// regression (e.g. a stray internal re-arm) would be caught rather
    /// than assumed.
    #[test]
    fn a_paused_compositor_feed_pauses_production_and_resuming_resumes_it() {
        let (clock, manual) = manual();

        // Demand is marked alongside the tick each iteration -- see the
        // 60/144Hz test's own doc for why `record_compositor_tick` alone
        // is never sufficient.
        for _ in 0..10 {
            manual.advance(Duration::from_millis(16));
            clock.record_compositor_tick(manual.now());
            clock.mark_demand(DemandKind::Dirty);
            assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        }
        assert_eq!(clock.produced_count(), 10);

        // The feed "freezes" -- nothing calls record_compositor_tick,
        // mark_demand, or poll for a long virtual stretch. Advancing the
        // manual clock alone (no tick, no mark, no poll) must not move
        // produced_count.
        manual.advance(Duration::from_secs(5));
        assert_eq!(
            clock.produced_count(),
            10,
            "advancing wall-clock time alone, with no delivered tick and no poll, must not \
             produce a frame"
        );

        // Resume: the feed starts delivering ticks again.
        for _ in 0..3 {
            manual.advance(Duration::from_millis(16));
            clock.record_compositor_tick(manual.now());
            clock.mark_demand(DemandKind::Dirty);
            assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
        }
        assert_eq!(clock.produced_count(), 13);
    }

    // ----------------------------------------------------------------
    // `clear_demand` disarming the actuator latch -- the
    // caller-contract gap `try_arm_redraw_request`'s own doc names: a
    // mark-then-settle-without-a-poll sequence must not permanently
    // strand a later, genuinely new mark.
    // ----------------------------------------------------------------

    #[test]
    fn clear_demand_emptying_the_mask_disarms_the_latch() {
        let (clock, _manual) = manual();
        clock.mark_demand(DemandKind::Animation);
        assert!(
            clock.try_arm_redraw_request(),
            "the first mark arms the latch"
        );

        // Settled away with no intervening poll -- the only path that
        // could otherwise leave the latch stranded.
        clock.clear_demand(DemandKind::Animation);
        assert!(
            clock.demand_mask().is_empty(),
            "sanity: the mask is genuinely empty after clearing the only marked kind"
        );

        // A later, genuinely new demand must be able to arm a fresh
        // request -- this is exactly what would fail if `clear_demand`
        // left the latch armed from the settled-away demand.
        clock.mark_demand(DemandKind::Dirty);
        assert!(
            clock.try_arm_redraw_request(),
            "a fresh mark after the mask emptied via clear_demand (not via poll) must still \
             be able to arm a new request -- clear_demand must disarm the stale latch"
        );
    }

    #[test]
    fn clear_demand_leaving_the_mask_nonempty_does_not_disturb_the_latch() {
        let (clock, _manual) = manual();
        clock.mark_demand(DemandKind::Animation);
        clock.mark_demand(DemandKind::Dirty);
        assert!(clock.try_arm_redraw_request());

        // Clears only ONE of the two marked kinds -- the mask stays
        // nonempty, so the already-armed latch must stay armed (a pending
        // request is still genuinely pending).
        clock.clear_demand(DemandKind::Animation);
        assert!(!clock.demand_mask().is_empty());
        assert!(
            !clock.try_arm_redraw_request(),
            "clearing one of two demand kinds, with the mask still nonempty, must not \
             re-arm an already-pending request"
        );
    }

    // ----------------------------------------------------------------
    // `set_min_produce_interval` as a target-frame-rate throttle (the
    // raster-backpressure slice of this file's own module-header issue): a
    // caller-requested cadence lower than the feed's own rate is enforced
    // entirely through `poll`'s existing capacity check and the caller's
    // own next-wake scheduling — never a sleep or timer this clock owns.
    // `RasterOptions::target_frame_rate` (`flui-engine`) feeds this knob via
    // `RasterOptions::min_produce_interval`; this test proves the cadence
    // the two crates cannot otherwise prove together, since flui-engine
    // does not depend on flui-scheduler (see `RasterOptions`'s own doc).
    // ----------------------------------------------------------------

    /// A 144 Hz demand feed throttled to a 30 Hz target frame rate produces
    /// at ~30 Hz, not 144 Hz — with NO sleep anywhere in this clock: the
    /// whole 1-second virtual span below is driven by a `ManualClock`, and
    /// the wall-clock guard asserts the real time this test itself took
    /// stays far below the 1 virtual second it simulated, which could not
    /// hold if `poll`/`set_min_produce_interval` blocked or slept
    /// internally to enforce the cadence.
    #[test]
    fn target_frame_rate_thirty_throttles_a_one_hundred_forty_four_hertz_feed_with_no_sleep_in_the_clock()
     {
        let (clock, manual) = manual();
        // The exact conversion `RasterOptions { target_frame_rate:
        // Some(30), .. }.min_produce_interval()` performs (flui-engine
        // does not depend on flui-scheduler, so this is the identical
        // arithmetic inlined rather than a cross-crate call — see that
        // method's own doc and its dedicated unit tests in flui-engine).
        let target_frame_rate_hz = 30u32;
        clock.set_min_produce_interval(Some(Duration::from_secs_f64(
            1.0 / f64::from(target_frame_rate_hz),
        )));

        let step_144 = Duration::from_nanos(1_000_000_000 / 144);
        let span = Duration::from_secs(1);

        let wall_clock_start = Instant::now();

        let mut elapsed = Duration::ZERO;
        let mut ticks = 0u32;
        let mut produces = 0u32;
        while elapsed + step_144 <= span {
            manual.advance(step_144);
            elapsed += step_144;
            ticks += 1;
            clock.record_compositor_tick(manual.now());
            clock.mark_demand(DemandKind::Dirty);
            if clock.poll(manual.now()).is_produce() {
                produces += 1;
            }
        }

        let wall_clock_elapsed = wall_clock_start.elapsed();

        assert_eq!(
            ticks, 144,
            "sanity: the scripted feed itself ran at 144 Hz for 1s"
        );
        // Hand-verifiable via the same throttle-boundary arithmetic
        // `the_same_script_replayed_twice_matches_an_absolute_throttled_sequence`
        // above already established: a produce needs
        // `now.duration_since(last) >= interval`, so at a fixed 1/144s
        // step and a 1/30s interval it takes `ceil((1/30) / (1/144)) =
        // ceil(4.8) = 5` ticks to re-arm capacity after each produce (5 *
        // 1/144s ≈ 34.72ms ≥ 33.33ms) — 144 ticks at 5 ticks/produce (the
        // FIRST produce needs only 1 tick, since `last_produce_at` starts
        // `None`) gives `1 + (144 - 1) / 5 = 29` (integer division)
        // produces over the span. Pinned to the exact number, not just "far
        // less than 144", so a mutant that shifts the throttle-boundary
        // comparison by one step is caught.
        assert_eq!(
            produces, 29,
            "a 30 Hz target frame rate against a 144 Hz feed must throttle to the \
             hand-verified 29 produces over this exact 1-second, 144-tick script (see \
             this test's own doc comment for the arithmetic) -- not 144"
        );
        assert!(
            wall_clock_elapsed < Duration::from_millis(200),
            "simulating 1 virtual second of a 144 Hz feed took {wall_clock_elapsed:?} of REAL \
             time -- this clock has no sleep or timer of its own, so driving it through a \
             ManualClock must complete near-instantly regardless of how much virtual time is \
             simulated; a real sleep hiding in `poll`/`set_min_produce_interval` would blow \
             this budget"
        );
    }

    // ----------------------------------------------------------------
    // Telemetry: input->present attribution, coalescing, and deferral
    // accounting (vs. a caller's own "frames dropped" counter).
    // ----------------------------------------------------------------

    /// Two inputs stamped before one produce -> both survive into the
    /// recorded frame, with the OLDER arrival carrying the LARGER latency
    /// -- kills "last-input-wins" attribution and "epoch field exists but
    /// is zero/None".
    #[test]
    fn two_inputs_before_one_produce_both_survive_with_distinct_latencies_older_larger() {
        let (clock, manual) = manual();

        let older = clock.stamp_input_epoch(manual.now());
        manual.advance(Duration::from_millis(10));
        let newer = clock.stamp_input_epoch(manual.now());

        manual.advance(Duration::from_millis(5));
        clock.mark_demand(DemandKind::Dirty);
        let segment_start = manual.now();
        assert_eq!(clock.poll(segment_start), PollDecision::Produce);
        manual.advance(Duration::from_millis(2));
        let segment_end = manual.now();
        manual.advance(Duration::from_millis(3));
        let submit_at = manual.now();

        let snapshot = clock.record_frame(
            PresentationId::new(1),
            segment_start,
            segment_start,
            segment_end,
            submit_at,
            PresentOutcome::Presented,
        );

        let latencies: std::collections::HashMap<InputEpochId, Duration> =
            snapshot.latencies().collect();
        assert_eq!(
            latencies.len(),
            2,
            "both inputs must survive into the record"
        );
        let older_latency = latencies[&older];
        let newer_latency = latencies[&newer];
        assert!(
            older_latency > newer_latency,
            "the older arrival must show the larger latency: older={older_latency:?} \
             newer={newer_latency:?}"
        );
        // Exact values, not just an ordering -- kills a mutant that always
        // reports the same (wrong) latency for every epoch. `older` arrived
        // at t=0, `newer` at t=10ms, submit at t=20ms (10 + 5 + 2 + 3).
        assert_eq!(older_latency, Duration::from_millis(20));
        assert_eq!(newer_latency, Duration::from_millis(10));
    }

    /// `record_frame_retaining_epochs` must leave every pending epoch in
    /// place for a SUBSEQUENT real `record_frame` — the property a failed
    /// submit that its caller has already armed a retry for depends on: the
    /// epoch that arrived before the failure must still reach the frame
    /// that eventually presents it, not vanish into the failed attempt's
    /// own snapshot alone.
    #[test]
    fn record_frame_retaining_epochs_leaves_pending_epochs_for_the_next_real_record_frame() {
        let (clock, manual) = manual();

        let arrival = manual.now();
        let epoch_id = clock.stamp_input_epoch(arrival);

        clock.mark_demand(DemandKind::Dirty);
        let attempt_1 = manual.now();
        assert_eq!(clock.poll(attempt_1), PollDecision::Produce);
        let failed_snapshot = clock.record_frame_retaining_epochs(
            PresentationId::new(1),
            attempt_1,
            attempt_1,
            attempt_1,
            attempt_1,
            PresentOutcome::Errored,
        );
        assert_eq!(
            failed_snapshot.latencies().count(),
            1,
            "the failed attempt's own snapshot still reports what was pending"
        );

        // Retry: a later real produce must still find the SAME epoch
        // pending, not an empty buffer.
        manual.advance(Duration::from_millis(5));
        clock.mark_demand(DemandKind::Dirty);
        let attempt_2 = manual.now();
        assert_eq!(clock.poll(attempt_2), PollDecision::Produce);
        let presented_snapshot = clock.record_frame(
            PresentationId::new(1),
            attempt_2,
            attempt_2,
            attempt_2,
            attempt_2,
            PresentOutcome::Presented,
        );

        let latencies: Vec<_> = presented_snapshot.latencies().collect();
        assert_eq!(
            latencies.len(),
            1,
            "the retry's own presented frame must carry the ORIGINAL epoch -- retaining it \
             across the failed attempt must not have lost it"
        );
        assert_eq!(latencies[0].0, epoch_id);

        // And it really is drained now: a third produce finds nothing left.
        clock.mark_demand(DemandKind::Dirty);
        let attempt_3 = manual.now();
        assert_eq!(clock.poll(attempt_3), PollDecision::Produce);
        let third_snapshot = clock.record_frame(
            PresentationId::new(1),
            attempt_3,
            attempt_3,
            attempt_3,
            attempt_3,
            PresentOutcome::Presented,
        );
        assert_eq!(
            third_snapshot.latencies().count(),
            0,
            "the epoch must not be attributed a SECOND time to a later, unrelated frame"
        );
    }

    /// A synthetic input at a known scripted time, one produced frame -> the
    /// exported record's latency equals (submit - arrival) exactly, within
    /// zero tolerance under a `ManualClock` (no wall-clock jitter to admit
    /// any tolerance at all).
    #[test]
    fn end_to_end_attribution_latency_equals_submit_minus_arrival_exactly() {
        let (clock, manual) = manual();

        let arrival = manual.now();
        let epoch_id = clock.stamp_input_epoch(arrival);

        manual.advance(Duration::from_millis(7));
        clock.mark_demand(DemandKind::Dirty);
        let now = manual.now();
        assert_eq!(clock.poll(now), PollDecision::Produce);

        manual.advance(Duration::from_millis(4));
        let submit_at = manual.now();
        let snapshot = clock.record_frame(
            PresentationId::new(1),
            now,
            now,
            now,
            submit_at,
            PresentOutcome::Presented,
        );

        let latencies: Vec<(InputEpochId, Duration)> = snapshot.latencies().collect();
        assert_eq!(latencies.len(), 1);
        assert_eq!(latencies[0].0, epoch_id);
        assert_eq!(latencies[0].1, Duration::from_millis(11));
    }

    /// `frames_since` returns the recorded snapshot, and a second `None`
    /// pull after a fresh `record_frame` with no new input excludes the
    /// already-seen one when queried with `since` -- proves the pull API
    /// is a real filter, not a constant "return everything" stub.
    #[test]
    fn frames_since_filters_by_frame_id() {
        let (clock, manual) = manual();

        clock.mark_demand(DemandKind::Dirty);
        let now = manual.now();
        assert_eq!(clock.poll(now), PollDecision::Produce);
        let first = clock.record_frame(
            PresentationId::new(1),
            now,
            now,
            now,
            now,
            PresentOutcome::Presented,
        );

        manual.advance(Duration::from_millis(16));
        clock.mark_demand(DemandKind::Dirty);
        let now2 = manual.now();
        assert_eq!(clock.poll(now2), PollDecision::Produce);
        let second = clock.record_frame(
            PresentationId::new(1),
            now2,
            now2,
            now2,
            now2,
            PresentOutcome::Presented,
        );

        assert_eq!(clock.frames_since(None).len(), 2);
        let recent = clock.frames_since(Some(first.frame_id));
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].frame_id, second.frame_id);
    }

    /// A backpressure episode retains demand (never dropped) and is counted
    /// on the clock's own deferral stat -- NOT on a caller's separate
    /// "frames dropped" counter, which this clock does not own and never
    /// touches. Pins `Skip(Backpressure)` vs "dropped" semantics at the
    /// level this clock actually controls.
    #[test]
    fn backpressure_deferral_is_counted_separately_and_never_looks_like_a_dropped_frame() {
        let (clock, manual) = manual();
        clock.set_max_in_flight(1);
        clock.record_submit(); // saturate capacity

        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Backpressure)
        );
        assert_eq!(clock.backpressure_deferrals(), 1);
        assert_eq!(clock.hidden_deferrals(), 0);
        assert_eq!(clock.produces_deferred(), 1);
        assert!(
            !clock.demand_mask().is_empty(),
            "backpressure must retain demand, never silently drop it"
        );

        // Capacity frees; the SAME retained demand now produces -- proving
        // this was a deferral (demand survived), not a drop (which would
        // have nothing left to produce from).
        clock.record_retire();
        assert_eq!(clock.poll(manual.now()), PollDecision::Produce);
    }

    /// `Skip(NoDemand)` is not a deferral -- an idle clock's poll must
    /// leave every deferral counter at zero. Kills a mutant that counts
    /// every Skip reason as a deferral.
    #[test]
    fn no_demand_skip_is_not_counted_as_a_deferral() {
        let (clock, manual) = manual();
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand)
        );
        assert_eq!(clock.produces_deferred(), 0);
        assert_eq!(clock.hidden_deferrals(), 0);
        assert_eq!(clock.backpressure_deferrals(), 0);
    }

    /// `Skip(Hidden)` is counted on `hidden_deferrals` specifically, not
    /// folded into `backpressure_deferrals`.
    #[test]
    fn hidden_deferral_is_counted_on_its_own_reason() {
        let (clock, manual) = manual();
        clock.set_hidden(true);
        clock.mark_demand(DemandKind::Dirty);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::Hidden)
        );
        assert_eq!(clock.hidden_deferrals(), 1);
        assert_eq!(clock.backpressure_deferrals(), 0);
    }

    /// A hidden clock with NO demand marked must read `Skip(NoDemand)`, not
    /// `Skip(Hidden)` — hidden alone, with nothing to retain, is not a
    /// deferral. Kills the ordering bug where `poll` checked `hidden`
    /// before the demand mask: `hidden_deferrals` would then increment on
    /// every idle pump of a hidden (but otherwise settled) presentation,
    /// contradicting this module's own "Deferral accounting" doc (a
    /// deferral tracks RETAINED demand, and an empty mask retains nothing).
    #[test]
    fn hidden_and_idle_is_no_demand_not_a_hidden_deferral() {
        let (clock, manual) = manual();
        clock.set_hidden(true);
        assert_eq!(
            clock.poll(manual.now()),
            PollDecision::Skip(SkipReason::NoDemand),
            "hidden with nothing dirty must report NoDemand, never Hidden"
        );
        assert_eq!(
            clock.hidden_deferrals(),
            0,
            "an idle poll must never be counted as a hidden deferral, even while hidden"
        );
        assert_eq!(clock.produces_deferred(), 0);
    }
}
