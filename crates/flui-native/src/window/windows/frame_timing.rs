//! When the desktop compositor last refreshed the screen and when it refreshes next, from the
//! counter readings `DwmGetCompositionTimingInfo` reports, as the frame time a frame request
//! carries (ADR-0347).
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

use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::time::Duration;

use flui_foundation::{FrameStamp, SurfaceGeneration};
use flui_platform::{DxgiShown, DxgiStatistics, RefreshPeriod};

use crate::input::assembly::counter_nanos;

/// `ticks` of the performance counter at `frequency` as a duration.
fn counter_duration(ticks: u64, frequency: NonZeroU64) -> Duration {
    Duration::from_nanos(counter_nanos(ticks, frequency))
}

/// The compositor's refresh around a counter reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RefreshTiming {
    /// How long before the reading the last refresh happened.
    pub(crate) since_refresh: Duration,
    /// How long after the reading the next refresh happens.
    pub(crate) until_refresh: Duration,
}

/// The refresh timing at the counter reading `now`, given the counter of a refresh the system
/// reported (`vblank`) and the refresh period, all in counter ticks at `frequency` per second.
///
/// DWM's `qpcVBlank` is usually the refresh still to come: on a Windows 11 run at 100 Hz it lay
/// up to a period after the reading in most reads, and before it in the rest. A refresh ahead of
/// the reading places the last one a whole number of periods before it. A refresh several periods
/// back (a compositor that has not ticked, a paused display) is the last refresh, and the next one
/// lies on the grid it started, never earlier than the reading. A period of zero is a report that
/// cannot be placed, and gives `None`: the caller falls back to the reading itself.
pub(crate) fn refresh_timing(
    now: u64,
    vblank: u64,
    period: u64,
    frequency: NonZeroU64,
) -> Option<RefreshTiming> {
    if period == 0 {
        return None;
    }
    let (since, until) = if let Some(elapsed) = now.checked_sub(vblank) {
        (elapsed, period - elapsed % period)
    } else {
        let into_period = (period - (vblank - now) % period) % period;
        (into_period, period - into_period)
    };
    Some(RefreshTiming {
        since_refresh: counter_duration(since, frequency),
        until_refresh: counter_duration(until, frequency),
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

/// Where a present the statistics newly name shown lies on the performance counter, with the
/// counter reading it was read at: both as time since the counter's zero.
///
/// The two are handed on as they are. What a stamp ahead of the reading means is decided where
/// every host's reports pass (`PresentationReports`), not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placed {
    /// When the refresh it was shown at began.
    pub(crate) shown_at: Duration,
    /// When the statistics were read.
    pub(crate) read_at: Duration,
    /// The refresh period the two latest reports measure: the counter time between their
    /// refreshes over the refreshes between them. `None` until two reports name different
    /// refreshes.
    pub(crate) refresh: Option<Duration>,
}

/// A frame the statistics newly name shown: the frame its present carried, where it lies on the
/// counter, and the refresh period, where the measured one is a cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShownPresent {
    /// The frame the shown present carried.
    pub(crate) frame: FrameStamp,
    /// When the refresh it was shown at began.
    pub(crate) shown_at: Duration,
    /// When the statistics were read.
    pub(crate) read_at: Duration,
    /// The measured refresh period, `None` until two reports measure one or where it is no
    /// cadence ([`RefreshPeriod`]).
    pub(crate) refresh: Option<RefreshPeriod>,
}

/// How many presents of a swap chain a window keeps the frame of until its statistics name one
/// shown: DXGI's largest buffer count, so no present a swap chain can hold is forgotten.
const PRESENTS_KEPT: usize = 16;

/// The frames of one surface's presents, by the number DXGI gives each present, from the one
/// about to be made until the statistics name it or a later one shown.
///
/// DXGI numbers a swap chain's presents from its creation and names a shown present by that
/// number (`PresentCount`), so a report goes to the frame its own present carried, not to the
/// newest one made: with presents in flight the shown one is older, and one the display skipped
/// is never named. The numbers are a swap chain's, not a surface's identity: a surface replaced
/// (a restore, a resize, a device rebuild) numbers its presents from the start again, so the
/// frames are kept per surface generation (`FrameStamp::surface_generation`), and a present of
/// another generation forgets every frame of the old one. Within one generation, a present
/// numbered no higher than the newest kept replaces the kept ones from that number on. A count at
/// `u32::MAX` numbers no next present, so that one present is not kept.
#[derive(Debug, Default)]
struct PresentFrames {
    /// The surface generation of the frames kept.
    generation: Option<SurfaceGeneration>,
    /// Presents made with a frame and not yet named shown, oldest first, by rising number.
    in_flight: VecDeque<(u32, FrameStamp)>,
}

impl PresentFrames {
    /// A present of `frame` is about to be made (`None` for a present that shows no frame):
    /// whether the frames kept were of another surface, or of no known one, and are forgotten.
    fn before_present(&mut self, frame: Option<FrameStamp>) -> bool {
        let generation = frame.map(|frame| frame.surface_generation);
        let other = generation.is_none() || self.generation != generation;
        if other {
            self.in_flight.clear();
            self.generation = generation;
        }
        other
    }

    /// The present about to be, numbered after the `made` before it, shows `frame`.
    fn presenting(&mut self, made: u32, frame: FrameStamp) {
        let Some(number) = made.checked_add(1) else {
            return;
        };
        while self
            .in_flight
            .back()
            .is_some_and(|&(kept, _)| kept >= number)
        {
            self.in_flight.pop_back();
        }
        if self.in_flight.len() == PRESENTS_KEPT {
            self.in_flight.pop_front();
        }
        self.in_flight.push_back((number, frame));
    }

    /// The frame of present `number`, named shown; it and the presents before it leave, shown or
    /// skipped. `None` for a present made without a frame, before the oldest kept, or not made.
    fn shown(&mut self, number: u32) -> Option<FrameStamp> {
        let position = self
            .in_flight
            .iter()
            .position(|&(kept, _)| kept == number)?;
        let (_, frame) = self.in_flight.drain(..=position).next_back()?;
        Some(frame)
    }
}

/// What a window's swap chain statistics last reported, so each read names only what is new, and
/// the frames of the presents in flight, so each report names the frame it describes.
///
/// The renderer reads the statistics before every present, and a display that shows one present
/// per refresh answers several reads with the same present. Only a read naming another present
/// and another refresh is a new report: a repeat would replace a report that measured the period
/// with one that measures none. A break in the statistics (`DXGI_ERROR_FRAME_STATISTICS_DISJOINT`,
/// after a mode change or a move to another output) forgets the last refresh, so no period is
/// measured across it; a present of another surface, or of no frame, forgets it too.
#[derive(Debug, Default)]
pub(crate) struct PresentTracker {
    last: Option<SyncPoint>,
    frames: PresentFrames,
}

impl PresentTracker {
    /// What the counters read before the present of `frame` (`None` for a present that shows no
    /// frame) report, with the counter reading `now` they were read at: the frame a newly shown
    /// present carried, or `None` when nothing new is shown, the shown present carried no frame
    /// kept, the statistics broke off, or the present is of another surface than the frames kept
    /// or shows no frame (the statistics may then be of another swap chain). Then the present
    /// about to be is kept with its frame.
    pub(crate) fn observe(
        &mut self,
        read: DxgiStatistics,
        frame: Option<FrameStamp>,
        now: u64,
        frequency: NonZeroU64,
    ) -> Option<ShownPresent> {
        if self.frames.before_present(frame) {
            self.last = None;
        }
        let shown = match read.shown {
            DxgiShown::Present(shown) => self
                .read(
                    shown.present_count,
                    shown.sync_refresh_count,
                    shown.sync_qpc_time,
                    now,
                    frequency,
                )
                .and_then(|placed| {
                    Some(ShownPresent {
                        frame: self.frames.shown(shown.present_count)?,
                        shown_at: placed.shown_at,
                        read_at: placed.read_at,
                        refresh: placed
                            .refresh
                            .and_then(|period| RefreshPeriod::try_from(period).ok()),
                    })
                }),
            DxgiShown::Disjoint => {
                self.disjoint();
                None
            }
            _ => None,
        };
        if let Some(frame) = frame {
            self.frames.presenting(read.last_present_count, frame);
        }
        shown
    }

    /// The present a read names shown (`PresentCount`, `SyncRefreshCount`, `SyncQPCTime`),
    /// with the counter reading `now` it was read at; `None` for a read that names the present or
    /// the refresh the last one did, or names nothing shown yet (a zero or negative counter time).
    fn read(
        &mut self,
        present: u32,
        sync_refresh: u32,
        sync_ticks: i64,
        now: u64,
        frequency: NonZeroU64,
    ) -> Option<Placed> {
        if self
            .last
            .is_some_and(|last| last.present == present || last.refresh == sync_refresh)
        {
            return None;
        }
        let ticks = u64::try_from(sync_ticks).ok().filter(|&ticks| ticks > 0)?;
        let to_duration = |ticks: u64| counter_duration(ticks, frequency);
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
        Some(Placed {
            shown_at: to_duration(ticks),
            read_at: to_duration(now),
            refresh,
        })
    }

    /// The statistics broke off: the next report measures no period from the ones before.
    fn disjoint(&mut self) {
        self.last = None;
    }
}

/// A DXGI adapter as the presentation check reads it: its PCI ids and how many of its outputs the
/// desktop is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DisplayAdapter {
    /// `DXGI_ADAPTER_DESC1::VendorId`.
    pub(crate) vendor_id: u32,
    /// `DXGI_ADAPTER_DESC1::DeviceId`.
    pub(crate) device_id: u32,
    /// The outputs of the adapter whose `AttachedToDesktop` is set.
    pub(crate) desktop_outputs: u32,
}

/// The vendor and device id of the Microsoft Basic Render Driver, Windows' software rasterizer
/// (WARP).
const BASIC_RENDER_DRIVER: (u32, u32) = (0x1414, 0x8c);

/// Why the windows of this machine report no presentation, given its DXGI adapters; `None` where
/// a hardware adapter drives a desktop output, whose swap chains' frame statistics name the
/// refresh each present was shown at.
///
/// Where the desktop's outputs belong to the Basic Render Driver alone (a virtual machine whose
/// display is a display-only device, such as Hyper-V Video), DWM composes on a timer, not on a
/// display's refresh: on a `windows-latest` runner it reported a 64 Hz refresh (15.625 ms, the
/// system timer's tick), and the frame statistics of 300 flip-model presents counted each present
/// but never named a refresh or a counter time it was shown at. No present is ever placed, so the
/// window says it reports none.
pub(crate) fn presentation_absence(
    adapters: &[DisplayAdapter],
) -> Option<flui_platform::CapabilityAbsence> {
    let scanned_out = adapters.iter().any(|adapter| {
        adapter.desktop_outputs > 0 && (adapter.vendor_id, adapter.device_id) != BASIC_RENDER_DRIVER
    });
    (!scanned_out).then_some(flui_platform::CapabilityAbsence::NOT_ON_THIS_DEVICE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shown present keeps its distance from the reading, and two reports a few refreshes
    /// apart measure the period between them.
    #[test]
    fn a_shown_present_is_aged_and_two_reads_measure_the_period() {
        let period = 166_667;
        let mut tracker = PresentTracker::default();
        let first = tracker
            .read(10, 100, 1_000_000, 1_050_000, MHZ_10)
            .expect("a first report");
        assert_eq!(
            first.read_at.saturating_sub(first.shown_at),
            Duration::from_millis(5)
        );
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

    /// The frame of the `epoch`-th frame of one presentation, on its first surface.
    fn frame(epoch: u64) -> FrameStamp {
        use flui_foundation::{
            FrameEpoch, PresentationAddress, PresentationId, SurfaceGeneration, UiRuntimeId,
        };
        FrameStamp::new(
            PresentationAddress {
                ui_runtime_id: UiRuntimeId::new(1),
                presentation_id: PresentationId::new(1),
            },
            FrameEpoch::from_raw(epoch),
            SurfaceGeneration::ZERO.next(),
        )
    }

    /// The counters read before a present: `made` presents before it, and the statistics naming
    /// present `shown` at refresh `refresh`, `refresh` periods of 166_667 ticks after tick 1_000_000.
    fn counters(made: u32, shown: Option<(u32, u32)>) -> DxgiStatistics {
        DxgiStatistics::new(
            made,
            shown.map_or(DxgiShown::Nothing, |(present, refresh)| {
                DxgiShown::Present(flui_platform::DxgiFrameStatistics::new(
                    present,
                    refresh,
                    1_000_000 + i64::from(refresh) * 166_667,
                ))
            }),
        )
    }
    /// Frame `epoch`, drawn for surface generation `surface`.
    fn frame_on(epoch: u64, surface: u64) -> FrameStamp {
        FrameStamp::new(
            frame(epoch).address,
            frame(epoch).epoch,
            SurfaceGeneration::from_raw(surface),
        )
    }

    /// Each report names the frame its own present carried: with two presents in flight the
    /// statistics name the older, a present the display skipped is never reported, and a present
    /// made without a frame reports nothing and forgets what was kept, since its surface is not
    /// known.
    #[test]
    fn a_shown_present_reports_the_frame_its_present_carried() {
        const NOW: u64 = 100_000_000;
        let mut tracker = PresentTracker::default();
        let mut observe = |read, carried: Option<u64>| {
            tracker
                .observe(read, carried.map(frame), NOW, MHZ_10)
                .map(|shown| shown.frame.epoch.get())
        };
        assert_eq!(observe(counters(0, None), Some(1)), None, "present 1");
        assert_eq!(observe(counters(1, None), Some(2)), None, "present 2");
        assert_eq!(
            observe(counters(2, Some((1, 10))), Some(3)),
            Some(1),
            "two presents in flight: the first is shown"
        );
        assert_eq!(
            observe(counters(3, Some((3, 11))), Some(4)),
            Some(3),
            "present 2 skipped: present 3 shown"
        );
        assert_eq!(
            observe(counters(4, Some((2, 12))), Some(5)),
            None,
            "the skipped present is never reported"
        );
        assert_eq!(
            observe(counters(5, Some((4, 13))), None),
            None,
            "a present without a frame: the surface is not known"
        );
        assert_eq!(
            observe(counters(6, Some((5, 14))), Some(7)),
            None,
            "what was kept before it is forgotten"
        );
        assert_eq!(observe(counters(7, Some((7, 15))), Some(8)), Some(7));
    }

    /// A surface replaced before its first present was read: the new swap chain's first present
    /// takes the number the old one's first present had, and its report names the new surface's
    /// frame. Two presents read with one count (the first never made) keep the second.
    #[test]
    fn a_replaced_surface_reports_its_own_frame() {
        const NOW: u64 = 100_000_000;
        let mut tracker = PresentTracker::default();
        let mut observe = |read, carried: FrameStamp| {
            tracker
                .observe(read, Some(carried), NOW, MHZ_10)
                .map(|shown| shown.frame)
        };
        assert_eq!(observe(counters(0, None), frame_on(1, 1)), None);
        // The surface is replaced: the new swap chain has made no present either.
        assert_eq!(observe(counters(0, None), frame_on(2, 2)), None);
        assert_eq!(
            observe(counters(1, Some((1, 10))), frame_on(3, 2)),
            Some(frame_on(2, 2)),
            "the new swap chain's present 1 is the new surface's frame"
        );

        // A resize keeps the swap chain and its numbering but is a new surface generation: the
        // statistics read before the new generation's first present name the old one's present,
        // whose frame is no report of the new surface.
        let mut tracker = PresentTracker::default();
        let mut observe = |read, carried: FrameStamp| {
            tracker
                .observe(read, Some(carried), NOW, MHZ_10)
                .map(|shown| shown.frame)
        };
        assert_eq!(observe(counters(2, None), frame_on(3, 1)), None);
        assert_eq!(
            observe(counters(3, Some((3, 10))), frame_on(4, 2)),
            None,
            "the old generation's present"
        );
        assert_eq!(
            observe(counters(4, Some((4, 11))), frame_on(5, 2)),
            Some(frame_on(4, 2))
        );

        let mut tracker = PresentTracker::default();
        let mut observe = |read, carried: FrameStamp| {
            tracker
                .observe(read, Some(carried), NOW, MHZ_10)
                .map(|shown| shown.frame)
        };
        assert_eq!(observe(counters(0, None), frame_on(1, 1)), None);
        assert_eq!(
            observe(counters(0, None), frame_on(2, 1)),
            None,
            "present 1 again"
        );
        assert_eq!(
            observe(counters(1, Some((1, 10))), frame_on(3, 1)),
            Some(frame_on(2, 1)),
            "the present made with number 1 is the second"
        );
    }

    /// A period the reports measure outside a cadence (an hour between two refreshes counted one
    /// apart) is no refresh period in the report.
    #[test]
    fn a_measured_period_outside_a_cadence_is_no_refresh() {
        const HOUR: u64 = 3_600 * 10_000_000;
        const HOUR_TICKS: i64 = 3_600 * 10_000_000;
        let read = |made, present, refresh, ticks| {
            DxgiStatistics::new(
                made,
                DxgiShown::Present(flui_platform::DxgiFrameStatistics::new(
                    present, refresh, ticks,
                )),
            )
        };
        let mut tracker = PresentTracker::default();
        let _ = tracker.observe(counters(0, None), Some(frame(1)), 1, MHZ_10);
        let _ = tracker.observe(counters(1, None), Some(frame(2)), 1, MHZ_10);
        let first = tracker
            .observe(read(2, 1, 10, 1_000), Some(frame(3)), 2_000, MHZ_10)
            .expect("the first report");
        assert_eq!(first.refresh, None, "one report measures no period");
        let second = tracker
            .observe(
                read(3, 2, 11, 1_000 + HOUR_TICKS),
                Some(frame(4)),
                2_000 + HOUR,
                MHZ_10,
            )
            .expect("the second report");
        assert_eq!(second.frame, frame(2));
        assert_eq!(second.refresh, None, "an hour is no cadence");
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

    /// A refresh after the reading is handed on with its lead: the reports' seam decides what a
    /// stamp ahead means, for every host alike.
    #[test]
    fn a_refresh_after_the_reading_is_handed_on_with_its_lead() {
        let ahead = PresentTracker::default()
            .read(1, 1, 200, 100, MHZ_10)
            .expect("a refresh after the reading");
        assert_eq!(
            ahead.shown_at.saturating_sub(ahead.read_at),
            Duration::from_micros(10),
            "100 ticks at 10 MHz after the reading"
        );
    }

    /// Statistics that name nothing shown place no present.
    #[test]
    fn statistics_that_name_no_shown_present_place_none() {
        let read = |present, refresh, ticks| {
            PresentTracker::default().read(present, refresh, ticks, 100, MHZ_10)
        };
        assert_eq!(read(0, 0, 0), None, "nothing shown yet");
        assert_eq!(read(1, 1, -5), None);
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

    /// A refresh reported ahead of the reading is the next one, and the last lies a period
    /// before it; one further ahead is placed on its grid. The readings are a live Windows 11
    /// run's at 100 Hz.
    #[test]
    fn a_refresh_reported_ahead_of_the_reading_is_the_next_one() {
        let next = refresh_timing(3_068_788_336_492, 3_068_788_408_043, 100_000, MHZ_10)
            .expect("a refresh still to come");
        assert_eq!(next.until_refresh, Duration::from_nanos(7_155_100));
        assert_eq!(next.since_refresh, Duration::from_nanos(2_844_900));

        let two_on = refresh_timing(1_000_000, 1_000_000 + 2 * 100_000 + 30_000, 100_000, MHZ_10)
            .expect("a refresh two periods on");
        assert_eq!(two_on.until_refresh, Duration::from_millis(3));
        assert_eq!(two_on.since_refresh, Duration::from_millis(7));

        let on_a_refresh =
            refresh_timing(1_000_000, 1_100_000, 100_000, MHZ_10).expect("a refresh a period on");
        assert_eq!(on_a_refresh.since_refresh, Duration::ZERO);
        assert_eq!(on_a_refresh.until_refresh, Duration::from_millis(10));
    }

    /// A report that cannot be placed is no timing, not a made-up one.
    #[test]
    fn an_unplaceable_refresh_report_gives_no_timing() {
        assert_eq!(refresh_timing(300, 200, 0, MHZ_10), None, "no period");
    }

    /// A machine reports presentation where a hardware adapter drives a desktop output, and not
    /// where the Basic Render Driver is all that does: an adapter with no desktop output, such as
    /// the software adapter DXGI also lists, counts for neither.
    #[test]
    fn presentation_is_reported_where_hardware_drives_the_desktop() {
        let adapter = |vendor_id, device_id, desktop_outputs| DisplayAdapter {
            vendor_id,
            device_id,
            desktop_outputs,
        };
        let warp = |desktop_outputs| adapter(0x1414, 0x8c, desktop_outputs);
        let absent = Some(flui_platform::CapabilityAbsence::NOT_ON_THIS_DEVICE);
        for (machine, adapters, expected) in [
            ("one GPU", vec![adapter(0x10de, 0x2482, 1), warp(0)], None),
            (
                "a hybrid laptop",
                vec![
                    adapter(0x8086, 0x9a49, 1),
                    adapter(0x10de, 0x2520, 0),
                    warp(0),
                ],
                None,
            ),
            ("a Hyper-V runner", vec![warp(1), warp(0)], absent),
            (
                "a GPU with no display on",
                vec![adapter(0x10de, 0x2482, 0), warp(1)],
                absent,
            ),
            ("no adapter read", vec![], absent),
        ] {
            assert_eq!(presentation_absence(&adapters), expected, "{machine}");
        }
    }
}
