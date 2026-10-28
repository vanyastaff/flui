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

/// A present DXGI reported shown: its number, and the refresh it began on, in counter ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncPoint {
    /// `PresentCount`.
    present: u32,
    /// `SyncRefreshCount`.
    refresh: u32,
    /// `SyncQPCTime`.
    ticks: u64,
}

/// A present a swap chain's frame statistics newly report shown, placed against a counter
/// reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShownPresent {
    /// How long before the reading the refresh it was shown at began.
    pub(crate) age: Duration,
    /// The refresh period the two latest reports measure: the counter time between their
    /// refreshes over the refreshes between them. `None` until two reports name different
    /// refreshes.
    pub(crate) refresh: Option<Duration>,
}

/// What a window's swap chain statistics last reported, so each read names only what is new.
///
/// The renderer reads the statistics before every present, and a display that shows one present
/// per refresh answers several reads with the same present. Only a read naming another present
/// and another refresh is a new report: a repeat would replace a report that measured the period
/// with one that measures none. A break in the statistics (`DXGI_ERROR_FRAME_STATISTICS_DISJOINT`,
/// after a mode change or a move to another output) forgets the last refresh, so no period is
/// measured across it.
#[derive(Debug, Default)]
pub(crate) struct PresentTracker {
    last: Option<SyncPoint>,
}

impl PresentTracker {
    /// The present a read names shown (`PresentCount`, `SyncRefreshCount`, `SyncQPCTime`),
    /// placed against the counter reading `now`; `None` for a read that names the present or the
    /// refresh the last one did, names nothing shown yet (a zero or negative counter time), or a
    /// refresh after the reading.
    pub(crate) fn read(
        &mut self,
        present: u32,
        sync_refresh: u32,
        sync_ticks: i64,
        now: u64,
        frequency: NonZeroU64,
    ) -> Option<ShownPresent> {
        if self
            .last
            .is_some_and(|last| last.present == present || last.refresh == sync_refresh)
        {
            return None;
        }
        let ticks = u64::try_from(sync_ticks).ok().filter(|&ticks| ticks > 0)?;
        if ticks > now {
            return None;
        }
        let to_duration = |ticks: u64| {
            let nanos = u128::from(ticks) * 1_000_000_000 / u128::from(frequency.get());
            Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
        };
        let refresh = self.last.and_then(|previous| {
            let refreshes = sync_refresh
                .checked_sub(previous.refresh)
                .filter(|&n| n > 0)?;
            let elapsed = ticks.checked_sub(previous.ticks).filter(|&t| t > 0)?;
            Some(to_duration(elapsed / u64::from(refreshes))).filter(|period| !period.is_zero())
        });
        self.last = Some(SyncPoint {
            present,
            refresh: sync_refresh,
            ticks,
        });
        Some(ShownPresent {
            age: to_duration(now - ticks),
            refresh,
        })
    }

    /// The statistics broke off: the next report measures no period from the ones before.
    pub(crate) fn disjoint(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shown present is placed by its age before the reading, and two reports a few refreshes
    /// apart measure the period between them.
    #[test]
    fn a_shown_present_is_aged_and_two_reads_measure_the_period() {
        let period = 166_667;
        let mut tracker = PresentTracker::default();
        let first = tracker
            .read(10, 100, 1_000_000, 1_050_000, MHZ_10)
            .expect("a first report");
        assert_eq!(first.age, Duration::from_millis(5));
        assert_eq!(first.refresh, None, "one report measures no period");

        let second = tracker
            .read(13, 103, 1_000_000 + 3 * period, 1_600_000, MHZ_10)
            .expect("a later present");
        assert_eq!(second.refresh, Some(Duration::from_nanos(16_666_700)));

        let remade = tracker
            .read(2, 2, 1_800_000, 1_900_000, MHZ_10)
            .expect("a swap chain made again");
        assert_eq!(remade.refresh, None, "a refresh count that went back");
    }

    /// A read naming the present or the refresh the last report named is no report: the one
    /// that measured the period stands.
    #[test]
    fn a_repeated_read_is_reported_once() {
        let mut tracker = PresentTracker::default();
        let read = |tracker: &mut PresentTracker, present, refresh, ticks| {
            tracker.read(present, refresh, ticks, 2_000_000, MHZ_10)
        };
        assert!(read(&mut tracker, 10, 100, 1_000_000).is_some());
        assert_eq!(
            read(&mut tracker, 10, 100, 1_000_000),
            None,
            "the same statistics"
        );
        assert_eq!(
            read(&mut tracker, 11, 100, 1_000_000),
            None,
            "two presents in one refresh"
        );
        let next = read(&mut tracker, 12, 101, 1_166_667).expect("the next refresh");
        assert_eq!(next.refresh, Some(Duration::from_nanos(16_666_700)));
    }

    /// After a break in the statistics the next report measures no period across it.
    #[test]
    fn a_disjoint_read_measures_no_period_across_it() {
        let mut tracker = PresentTracker::default();
        assert!(
            tracker
                .read(10, 100, 1_000_000, 2_000_000, MHZ_10)
                .is_some()
        );
        tracker.disjoint();
        let after = tracker
            .read(11, 105, 1_400_000, 2_000_000, MHZ_10)
            .expect("a report after the break");
        assert_eq!(after.refresh, None);
    }

    /// Statistics that name nothing shown, or a refresh after the reading, place no present.
    #[test]
    fn statistics_that_name_no_shown_present_place_none() {
        let read = |present, refresh, ticks| {
            PresentTracker::default().read(present, refresh, ticks, 100, MHZ_10)
        };
        assert_eq!(read(0, 0, 0), None, "nothing shown yet");
        assert_eq!(read(1, 1, -5), None);
        assert_eq!(read(1, 1, 200), None, "after the reading");
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
