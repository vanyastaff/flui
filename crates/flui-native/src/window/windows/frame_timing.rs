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

/// A refresh DXGI reported a present shown at: its number and when it began, in counter ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SyncPoint {
    /// `SyncRefreshCount`.
    pub(crate) refresh: u32,
    /// `SyncQPCTime`.
    pub(crate) ticks: u64,
}

/// The newest present a swap chain's frame statistics report shown, placed against a counter
/// reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShownPresent {
    /// The refresh it was shown at, for the next report to measure the period from.
    pub(crate) sync: SyncPoint,
    /// How long before the reading that refresh began.
    pub(crate) age: Duration,
    /// The refresh period the two latest reports measure: the counter time between their
    /// refreshes over the refreshes between them. `None` until two reports name different
    /// refreshes.
    pub(crate) refresh: Option<Duration>,
}

/// Place the shown present of a DXGI frame statistics read (`SyncRefreshCount`, `SyncQPCTime`)
/// against the counter reading `now`, measuring the refresh period from `previous`, the refresh
/// the last read placed.
///
/// A read that names no shown present (a zero or negative counter time: nothing reached the
/// screen yet) or a refresh after the reading cannot be placed, and gives `None`. A refresh count
/// that did not move, or moved backwards (a swap chain made again), measures no period.
pub(crate) fn shown_present(
    sync_refresh: u32,
    sync_ticks: i64,
    now: u64,
    frequency: NonZeroU64,
    previous: Option<SyncPoint>,
) -> Option<ShownPresent> {
    let ticks = u64::try_from(sync_ticks).ok().filter(|&ticks| ticks > 0)?;
    if ticks > now {
        return None;
    }
    let to_duration = |ticks: u64| {
        let nanos = u128::from(ticks) * 1_000_000_000 / u128::from(frequency.get());
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    };
    let sync = SyncPoint {
        refresh: sync_refresh,
        ticks,
    };
    let refresh = previous.and_then(|previous| {
        let refreshes = sync_refresh
            .checked_sub(previous.refresh)
            .filter(|&n| n > 0)?;
        let elapsed = ticks.checked_sub(previous.ticks).filter(|&t| t > 0)?;
        Some(to_duration(elapsed / u64::from(refreshes))).filter(|period| !period.is_zero())
    });
    Some(ShownPresent {
        sync,
        age: to_duration(now - ticks),
        refresh,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shown present is placed by its age before the reading, and two reads a few refreshes
    /// apart measure the period between them.
    #[test]
    fn a_shown_present_is_aged_and_two_reads_measure_the_period() {
        let period = 166_667;
        let first = shown_present(100, 1_000_000, 1_050_000, MHZ_10, None).unwrap();
        assert_eq!(first.age, Duration::from_millis(5));
        assert_eq!(first.refresh, None, "one read measures no period");

        let second = shown_present(
            103,
            1_000_000 + 3 * period,
            1_600_000,
            MHZ_10,
            Some(first.sync),
        )
        .unwrap();
        assert_eq!(second.refresh, Some(Duration::from_nanos(16_666_700)));

        let same_refresh = shown_present(
            103,
            1_000_000 + 3 * period,
            1_700_000,
            MHZ_10,
            Some(second.sync),
        )
        .unwrap();
        assert_eq!(same_refresh.refresh, None, "no refresh between the reads");

        let remade = shown_present(2, 1_800_000, 1_900_000, MHZ_10, Some(second.sync)).unwrap();
        assert_eq!(remade.refresh, None, "a refresh count that went back");
    }

    /// Statistics that name nothing shown, or a refresh after the reading, place no present.
    #[test]
    fn statistics_that_name_no_shown_present_place_none() {
        assert_eq!(
            shown_present(0, 0, 100, MHZ_10, None),
            None,
            "nothing shown yet"
        );
        assert_eq!(shown_present(1, -5, 100, MHZ_10, None), None);
        assert_eq!(
            shown_present(1, 200, 100, MHZ_10, None),
            None,
            "after the reading"
        );
    }

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
