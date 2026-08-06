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
const DEFAULT_MAX_FRAMES_IN_FLIGHT: u8 = 2;

/// Advanced pacing/capacity configuration for the raster boundary.
///
/// `#[non_exhaustive]`: a future field (e.g. a distinct GPU-side in-flight
/// bound, if one is ever wired) is additive, not a breaking change to every
/// existing construction site.
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
    pub max_frames_in_flight: NonZeroU8,
}

impl RasterOptions {
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

    #[test]
    fn thirty_hz_converts_to_a_thirty_three_point_three_millisecond_interval() {
        let options = RasterOptions {
            target_frame_rate: Some(NonZeroU32::new(30).expect("nonzero")),
            ..RasterOptions::default()
        };
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
        let options = RasterOptions {
            target_frame_rate: Some(NonZeroU32::new(144).expect("nonzero")),
            ..RasterOptions::default()
        };
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
}
