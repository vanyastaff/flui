//! When the desktop compositor last refreshed the screen and when it refreshes next, from the
//! counter readings `DwmGetCompositionTimingInfo` reports, as the frame time a frame request
//! carries (ADR-0257).
//!
//! The pure half of the Win32 frame clock: the rules take plain counter values, so every host runs
//! their tests; the window procedure reads the system's structure and the counter (`proc`).

#![forbid(unsafe_code)]
// Elsewhere than on Windows this module is compiled for its tests alone.
#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "off Windows the rules are compiled for their tests"
    )
)]

use std::num::NonZeroU64;
use std::time::Duration;

/// The compositor's refresh around a counter reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RefreshTiming {
    /// How long before the reading the last refresh happened.
    pub(crate) since_refresh: Duration,
    /// How long after the reading the next refresh happens.
    pub(crate) until_refresh: Duration,
}

/// The refresh timing at the counter reading `now`, given the counter of the last refresh
/// (`vblank`) and the refresh period, all in counter ticks at `frequency` per second.
///
/// A refresh the system reports ahead of the reading, or a period of zero, is a report that cannot
/// be placed, and gives `None`: the caller falls back to the reading itself. A refresh several
/// periods back (a compositor that has not ticked, a paused display) places the next refresh on
/// the grid that refresh started, never earlier than the reading.
pub(crate) fn refresh_timing(
    now: u64,
    vblank: u64,
    period: u64,
    frequency: NonZeroU64,
) -> Option<RefreshTiming> {
    if period == 0 || vblank > now {
        return None;
    }
    let elapsed = now - vblank;
    let to_duration = |ticks: u64| {
        let nanos = u128::from(ticks) * 1_000_000_000 / u128::from(frequency.get());
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    };
    Some(RefreshTiming {
        since_refresh: to_duration(elapsed),
        until_refresh: to_duration(period - elapsed % period),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MHZ_10: NonZeroU64 = NonZeroU64::new(10_000_000).unwrap();

    /// The refresh a reading follows is its vsync's age, and the next one is a period after it,
    /// counted from the last report however many periods back that was.
    #[test]
    fn the_next_refresh_follows_the_last_one_on_the_period_grid() {
        // 60 Hz at 10 MHz is 166_667 ticks.
        let period = 166_667;
        let at = |now| refresh_timing(now, 1_000_000, period, MHZ_10).unwrap();

        let just_after = at(1_000_000 + 50_000);
        assert_eq!(just_after.since_refresh, Duration::from_millis(5));
        assert_eq!(
            just_after.until_refresh,
            Duration::from_nanos(11_666_700),
            "the rest of the period"
        );

        let on_the_refresh = at(1_000_000);
        assert_eq!(on_the_refresh.since_refresh, Duration::ZERO);
        assert_eq!(
            on_the_refresh.until_refresh,
            Duration::from_nanos(16_666_700)
        );

        let three_periods_on = at(1_000_000 + 3 * period + 10_000);
        assert_eq!(
            three_periods_on.until_refresh,
            Duration::from_nanos(15_666_700)
        );
    }

    /// A report that cannot be placed is no timing, not a made-up one.
    #[test]
    fn an_unplaceable_refresh_report_gives_no_timing() {
        assert_eq!(
            refresh_timing(100, 200, 166_667, MHZ_10),
            None,
            "refresh in the future"
        );
        assert_eq!(refresh_timing(300, 200, 0, MHZ_10), None, "no period");
    }
}
