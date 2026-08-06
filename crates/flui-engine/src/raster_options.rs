//! [`RasterOptions`] — the advanced, opt-in pacing/capacity configuration
//! carried at the [`crate::raster_owner`] boundary (issue #556).
//!
//! Both fields are numbers a caller reads back out to configure a *clock*
//! (`flui_scheduler::FrameClock::set_max_in_flight`/
//! `set_min_produce_interval`), never a setting this module or
//! [`crate::raster_owner::RasterOwner`] acts on directly:
//!
//! - `max_frames_in_flight` is the CLOCK-side counting threshold only. It
//!   is deliberately NOT threaded into wgpu's own
//!   `wgpu::SurfaceConfiguration::desired_maximum_frame_latency`, which
//!   stays pinned at `1` everywhere — see the doc at that literal in
//!   `crate::wgpu::renderer` for the documented anti-resize-jitter
//!   rationale (a latency of 2 lets the present queue hold a stale-size
//!   frame the compositor then stretches). The raster mailbox itself
//!   (`crate::raster_owner`'s internal `RasterMailbox`) also does not
//!   enforce this number — it is always capacity-1, latest-frame-wins,
//!   regardless of how many frames a caller's own clock permits
//!   outstanding.
//! - `target_frame_rate` is a caller-facing cadence request (e.g. "30",
//!   read as Hz). [`RasterOptions::min_produce_interval`] converts it to
//!   the `Duration` a clock's own throttle knob expects; nothing in this
//!   crate sleeps, polls, or times anything against it — the clock's own
//!   `now`-driven `poll` is what turns this number into an actual cadence,
//!   with no timer of its own.
//!
//! This slice lands the typed, tested boundary; wiring a live `FrameClock`
//! to read these numbers from a live `RasterHandle` is later work on the
//! same issue (`RasterOwner`'s in-flight/throttle knobs already exist
//! unwired for exactly this reason — see
//! `flui_scheduler::frame_clock`'s own module doc). `flui-engine` does not
//! depend on `flui-scheduler` (siblings in the workspace's dependency DAG),
//! so this module carries the numbers as plain `std::time`/`std::num`
//! types rather than referencing `FrameClock` directly.

use std::num::{NonZeroU8, NonZeroU32};
use std::time::Duration;

/// The default in-flight capacity before a caller configures anything
/// explicitly. Matches `flui_scheduler::frame_clock`'s own
/// `DEFAULT_MAX_IN_FLIGHT` — kept in sync by hand (documented here) rather
/// than by a shared constant, since the two crates do not depend on each
/// other (see the module doc).
///
/// **Honest scope:** at this value, a clock's `in_flight() >= max` gate is
/// currently INERT against today's synchronous `RasterOwner` — see
/// [`RasterOptions::max_frames_in_flight`]'s own doc for why (the reachable
/// range, and what would give a higher threshold real teeth).
const DEFAULT_MAX_FRAMES_IN_FLIGHT: u8 = 2;

/// Advanced pacing/capacity configuration for the raster boundary.
///
/// `#[non_exhaustive]`: a future field (e.g. a distinct GPU-side in-flight
/// bound, if one is ever wired) is additive, not a breaking change to every
/// existing construction site. Since `#[non_exhaustive]` also blocks
/// functional-update construction (`RasterOptions { .., ..Default::default() }`,
/// `E0639`) from outside this crate — not just a bare struct literal — use
/// [`Self::with_target_frame_rate`]/[`Self::with_max_frames_in_flight`] to
/// build a non-default value from another crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RasterOptions {
    /// A caller-requested produce cadence, in Hz. `None` (the default)
    /// requests no throttle beyond whatever the demand feed and raster
    /// capacity already impose. See [`Self::min_produce_interval`] for the
    /// conversion a clock's throttle knob expects.
    pub target_frame_rate: Option<NonZeroU32>,
    /// The in-flight capacity threshold a clock should configure via
    /// `FrameClock::set_max_in_flight`. Clamped away from zero by
    /// construction (`NonZeroU8`) — a zero threshold would make a clock's
    /// `poll` permanently non-producing with no way to ever recover, the
    /// same hazard `FrameClock::set_max_in_flight`'s own `max(1)` clamp
    /// guards against on that side.
    ///
    /// **Honest scope — the reachable range today, and what widens it:**
    /// this type advertises a `1..=255` range (`NonZeroU8`), but today's
    /// synchronous `RasterOwner` structurally cannot produce an
    /// `in_flight()` reading above 2: a ticket lives in at most two places
    /// at once — the mailbox's own single `pending_frame` slot, and the
    /// one local `pump` is currently rendering — and the mailbox's
    /// supersede-not-queue rule makes a third simultaneously-live ticket
    /// impossible. Because of that, a threshold *at* the ceiling (the
    /// documented default, 2) can never actually turn away a produce
    /// attempt: the system already caps itself there for free, with no
    /// help from any clock. A threshold of `1` is the only value that
    /// meaningfully gates THIS owner today — it makes a clock wait for the
    /// one outstanding frame to retire before producing the next, the real
    /// "don't get ahead of the GPU" policy this knob exists for. Widening
    /// the reachable range past 2 — and giving thresholds above 1 real
    /// gating power — needs the planned threaded raster owner (this
    /// crate's own module docs) to pipeline more than one frame at a time;
    /// until then, configuring anything above 1 here asks a clock to gate
    /// on a ceiling the owner can never reach in a way the gate would ever
    /// observe as "over".
    pub max_frames_in_flight: NonZeroU8,
}

impl RasterOptions {
    /// Builder: sets [`Self::target_frame_rate`]. The primary way an
    /// external crate constructs a non-default [`RasterOptions`] — see the
    /// type's own doc for why `#[non_exhaustive]` rules out functional
    /// update syntax from outside this crate.
    #[must_use]
    pub fn with_target_frame_rate(mut self, target_frame_rate: NonZeroU32) -> Self {
        self.target_frame_rate = Some(target_frame_rate);
        self
    }

    /// Builder: sets [`Self::max_frames_in_flight`]. See that field's own
    /// doc for the reachable range today's synchronous `RasterOwner`
    /// actually honors before picking a value here.
    #[must_use]
    pub fn with_max_frames_in_flight(mut self, max_frames_in_flight: NonZeroU8) -> Self {
        self.max_frames_in_flight = max_frames_in_flight;
        self
    }

    /// Converts [`Self::target_frame_rate`] to the `Duration` a clock's
    /// `set_min_produce_interval` expects. `None` in, `None` out — an
    /// unset target frame rate imposes no throttle.
    #[must_use]
    pub fn min_produce_interval(&self) -> Option<Duration> {
        self.target_frame_rate
            .map(|hz| Duration::from_secs_f64(1.0 / f64::from(hz.get())))
    }
}

impl Default for RasterOptions {
    /// No target frame rate (uncapped); the same in-flight default
    /// [`RasterOwner::new`](crate::raster_owner::RasterOwner::new) has
    /// always implicitly assumed by constructing with these defaults.
    fn default() -> Self {
        Self {
            target_frame_rate: None,
            max_frames_in_flight: NonZeroU8::new(DEFAULT_MAX_FRAMES_IN_FLIGHT)
                .expect("BUG: DEFAULT_MAX_FRAMES_IN_FLIGHT is a nonzero literal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_target_frame_rate_and_the_documented_in_flight_default() {
        let options = RasterOptions::default();
        assert_eq!(options.target_frame_rate, None);
        assert_eq!(
            options.max_frames_in_flight.get(),
            DEFAULT_MAX_FRAMES_IN_FLIGHT
        );
        assert_eq!(
            options.min_produce_interval(),
            None,
            "no target frame rate must convert to no throttle interval"
        );
    }

    // ------------------------------------------------------------------
    // `min_produce_interval` — exact-value, not just "is Some": kills a
    // mutant that inverts the formula (e.g. `hz as f64` instead of
    // `1.0 / hz`) or drops the `f64` conversion and truncates.
    // ------------------------------------------------------------------

    // Every test below builds through `with_target_frame_rate`/
    // `with_max_frames_in_flight` rather than `RasterOptions { .., \
    // ..RasterOptions::default() }` -- the struct-literal-with-update form
    // compiles from inside this crate (`#[non_exhaustive]` has no effect
    // in-crate), so it would silently prove nothing about what a
    // downstream crate can actually write. `#[non_exhaustive]` blocks
    // functional-update syntax across a crate boundary too (`E0639`, not
    // just a bare literal), so the builder methods are the only
    // cross-crate construction path -- these tests exercise exactly that
    // path.

    #[test]
    fn thirty_hz_converts_to_a_thirty_three_point_three_millisecond_interval() {
        let options =
            RasterOptions::default().with_target_frame_rate(NonZeroU32::new(30).expect("nonzero"));
        let interval = options
            .min_produce_interval()
            .expect("a Some target frame rate must convert to Some interval");
        let expected = Duration::from_secs_f64(1.0 / 30.0);
        let delta = interval.abs_diff(expected);
        assert!(
            delta < Duration::from_nanos(1),
            "30 Hz must convert to ~33.333ms; got {interval:?} (expected {expected:?})"
        );
    }

    #[test]
    fn one_hundred_forty_four_hz_converts_to_a_sub_seven_millisecond_interval() {
        let options =
            RasterOptions::default().with_target_frame_rate(NonZeroU32::new(144).expect("nonzero"));
        let interval = options
            .min_produce_interval()
            .expect("a Some target frame rate must convert to Some interval");
        assert!(
            interval > Duration::from_millis(6) && interval < Duration::from_millis(7),
            "144 Hz must convert to ~6.94ms; got {interval:?}"
        );
    }

    #[test]
    fn max_frames_in_flight_is_never_zero_by_construction() {
        // `NonZeroU8` itself is the enforcement -- this test pins that a
        // caller cannot even attempt to build a zero threshold through this
        // type, matching `FrameClock::set_max_in_flight`'s own `max(1)`
        // clamp on the consuming side.
        assert!(NonZeroU8::new(0).is_none());
    }

    #[test]
    fn with_max_frames_in_flight_overrides_the_field_and_chains_with_the_other_builder() {
        let options = RasterOptions::default()
            .with_target_frame_rate(NonZeroU32::new(30).expect("nonzero"))
            .with_max_frames_in_flight(NonZeroU8::new(1).expect("nonzero"));
        assert_eq!(options.max_frames_in_flight.get(), 1);
        assert_eq!(options.target_frame_rate.map(NonZeroU32::get), Some(30));
    }
}
