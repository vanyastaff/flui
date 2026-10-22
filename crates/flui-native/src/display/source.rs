//! The display state every native backend keeps: the registry that issues its
//! [`DisplayId`]s, the snapshot it last published, and the rules for when that
//! snapshot is read from the OS again; and the refresh period's one
//! constructor ([`Refresh`]) and one publisher of a measured period
//! (`MeasuredPeriod`, on the platforms that measure one).

use flui_platform::{
    DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, RefreshPeriod,
};
use parking_lot::Mutex;

use crate::error::PlatformError;

/// A display's refresh as a platform reports it, in the platform's unit.
///
/// [`period`](Self::period) is the one way a reported value becomes a
/// [`RefreshPeriod`]: every display source hands its report here, and a
/// value that describes no cadence is unknown, never a guess.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "each platform reports its refresh in one of these units, so a target builds only its own platforms' variants"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Refresh {
    /// A rate in hertz: Android's `Display.Mode.getRefreshRate`,
    /// CoreGraphics' `CGDisplayModeGetRefreshRate`.
    Hertz(f64),
    /// A rate in hertz as `numerator / denominator`, kept exact:
    /// `QueryDisplayConfig`'s `DISPLAYCONFIG_RATIONAL` (60000/1001 stays
    /// 16.683 ms), and a whole rate over one.
    HertzRatio { numerator: u32, denominator: u32 },
    /// A rate in millihertz: Wayland's `wl_output.mode`, a RandR mode's dot
    /// clock over its totals.
    Millihertz(i64),
    /// The time between two frames, in seconds: a display link's tick, the
    /// interval a browser's frames are estimated at.
    Interval(f64),
}

impl Refresh {
    /// The period this describes; `None` for a value that is not finite or
    /// not above zero (what a platform reports when it does not know:
    /// CoreGraphics' 0, Wayland's 0 or less, a rational with a zero term)
    /// and for a period outside 1 ms to 1 s.
    pub(crate) fn period(self) -> Option<RefreshPeriod> {
        if !self.is_finite_and_positive() {
            return None;
        }
        match self {
            Self::Hertz(hertz) => RefreshPeriod::from_hz(hertz).ok(),
            Self::HertzRatio {
                numerator,
                denominator,
            } => RefreshPeriod::from_ratio(numerator, denominator).ok(),
            Self::Millihertz(millihertz) => RefreshPeriod::from_millihertz(millihertz).ok(),
            Self::Interval(seconds) => std::time::Duration::try_from_secs_f64(seconds)
                .ok()
                .and_then(|interval| RefreshPeriod::try_from(interval).ok()),
        }
    }

    /// The one check every report passes: a finite value above zero.
    fn is_finite_and_positive(self) -> bool {
        match self {
            Self::Hertz(value) | Self::Interval(value) => value.is_finite() && value > 0.0,
            Self::HertzRatio {
                numerator,
                denominator,
            } => numerator > 0 && denominator > 0,
            Self::Millihertz(millihertz) => millihertz > 0,
        }
    }
}

/// The refresh period a display publishes from a cadence it measures (a
/// display link's ticks, a browser's frames), and the one rule for
/// publishing a new one: a measurement replaces the published period only
/// when it lies more than its band (a fraction of the published period) away
/// from it, so jitter inside the band publishes nothing; the first one is
/// always published.
#[cfg(any(target_os = "ios", target_arch = "wasm32", test))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MeasuredPeriod {
    published: Option<RefreshPeriod>,
}

#[cfg(any(target_os = "ios", target_arch = "wasm32", test))]
impl MeasuredPeriod {
    /// The period published so far; `None` until a measurement is. The
    /// browser's estimate reads it back; a display link only publishes.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn published(self) -> Option<RefreshPeriod> {
        self.published
    }

    /// Whether `measured` lies outside `band` around the published period,
    /// and so would replace it; always, while none is published.
    pub(crate) fn departs(self, measured: RefreshPeriod, band: f64) -> bool {
        self.published.is_none_or(|published| {
            let published = published.get().as_secs_f64();
            (measured.get().as_secs_f64() - published).abs() > published * band
        })
    }

    /// Publish `measured`; answers it. A measurer that holds back what
    /// departs from the band (the browser's, until its frames agree) asks
    /// [`departs`](Self::departs) first.
    pub(crate) fn publish(&mut self, measured: RefreshPeriod) -> RefreshPeriod {
        self.published = Some(measured);
        measured
    }

    /// Forget the published period: the cadence is unknown until the next
    /// measurement.
    pub(crate) fn forget(&mut self) {
        self.published = None;
    }
}

/// One platform instance's display identities and last published snapshot.
///
/// A backend mints ids for the outputs it sampled, hands the resulting
/// [`DisplayInfo`] list to [`publish`](Self::publish) and returns what that
/// answers from `Platform::display_snapshot`. An unchanged sample answers the
/// previous snapshot with its revision, so a consumer that compares revisions
/// sees no change.
///
/// `display_snapshot` runs on every owner wake, so a backend that has events to
/// learn of a change reads the OS through
/// `snapshot_with` instead: the last snapshot is
/// answered until `invalidate` marks it stale.
pub(crate) struct DisplaySource {
    registry: DisplayRegistry,
    last: Mutex<DisplaySnapshot>,
    /// Set from any thread when the displays may differ from `last`; cleared
    /// before a sample reads the OS, so an invalidation that lands during the
    /// read survives it.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    stale: std::sync::atomic::AtomicBool,
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        test
    ))]
    misses: Mutex<MissLedger>,
}

#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_arch = "wasm32",
    test
))]
impl crate::owner::notices::StaleOnWake for DisplaySource {
    fn mark_stale(&self) {
        self.invalidate();
    }
}

impl DisplaySource {
    /// Whether the next read samples the operating system again.
    #[cfg(test)]
    pub(crate) fn is_stale(&self) -> bool {
        self.stale.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn new() -> Self {
        Self {
            registry: DisplayRegistry::new(),
            last: Mutex::new(DisplaySnapshot::empty()),
            #[cfg(any(
                windows,
                target_os = "linux",
                target_os = "macos",
                target_os = "ios",
                target_os = "android",
                target_arch = "wasm32",
                test
            ))]
            stale: std::sync::atomic::AtomicBool::new(true),
            #[cfg(any(
                windows,
                target_os = "linux",
                target_os = "macos",
                target_os = "ios",
                target_os = "android",
                test
            ))]
            misses: Mutex::new(MissLedger::default()),
        }
    }

    /// The id for `key`, or `None` once the id space is used up: that output is
    /// then left out of the snapshot and its window reports no display.
    pub(crate) fn mint(&self, key: &DisplayKey) -> Option<DisplayId> {
        match self.registry.mint(key) {
            Ok(id) => Some(id),
            Err(error) => {
                tracing::warn!(%error, "an output is left out of the display snapshot");
                None
            }
        }
    }

    /// Publish a fresh sample: the previous snapshot when nothing differs,
    /// otherwise the next revision.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the revision space is used up; the last
    /// snapshot stays published.
    pub(crate) fn publish(
        &self,
        displays: Vec<DisplayInfo>,
    ) -> Result<DisplaySnapshot, PlatformError> {
        let mut last = self.last.lock();
        let next = last
            .successor(displays)
            .map_err(|error| PlatformError::Displays {
                message: error.to_string(),
            })?;
        *last = next.clone();
        Ok(next)
    }

    /// Record that the displays may have changed. One atomic store, so a
    /// window procedure, a notification block or a foreign thread may call it
    /// at any point; the next `snapshot_with` samples
    /// again.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn invalidate(&self) {
        self.stale.store(true, std::sync::atomic::Ordering::Release);
    }

    /// The last published snapshot, without reading the OS.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn published(&self) -> DisplaySnapshot {
        self.last.lock().clone()
    }

    /// The snapshot: the last published one while nothing has invalidated it,
    /// otherwise the result of `sample` published.
    ///
    /// `sample` runs with no lock held, so it may call the OS and mint ids. A
    /// sample that fails, or whose result cannot be published, leaves the
    /// source stale and the last snapshot published.
    ///
    /// # Errors
    /// What `sample` fails with, or [`PlatformError::Displays`] when the
    /// revision space is used up.
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn snapshot_with(
        &self,
        sample: impl FnOnce() -> Result<Vec<DisplayInfo>, PlatformError>,
    ) -> Result<DisplaySnapshot, PlatformError> {
        self.snapshot_with_aux(|| sample().map(|displays| (displays, ())), |()| {})
    }

    /// As `snapshot_with`, for a sample that also yields
    /// data a reader resolves against the snapshot (Windows' monitor handles).
    /// `commit` receives that data only after the snapshot it belongs to is
    /// published, so a reader on another thread never sees data for displays
    /// the published snapshot lacks. A sample that fails or cannot be published
    /// commits nothing.
    ///
    /// # Errors
    /// As `snapshot_with`.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn snapshot_with_aux<T>(
        &self,
        sample: impl FnOnce() -> Result<(Vec<DisplayInfo>, T), PlatformError>,
        commit: impl FnOnce(T),
    ) -> Result<DisplaySnapshot, PlatformError> {
        use std::sync::atomic::Ordering;
        if !self.stale.load(Ordering::Acquire) {
            return Ok(self.published());
        }
        self.stale.store(false, Ordering::Release);
        let published = sample().and_then(|(displays, aux)| {
            let snapshot = self.publish(displays)?;
            commit(aux);
            Ok(snapshot)
        });
        if published.is_err() {
            self.invalidate();
        }
        published
    }

    /// Note that a display the published snapshot lacks was asked for (a window
    /// on a monitor plugged in since the last sample). The first miss of
    /// `handle` in a snapshot revision invalidates the snapshot; the same miss
    /// again, however often it repeats, costs nothing until the revision
    /// changes.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        test
    ))]
    pub(crate) fn note_miss(&self, handle: u64) {
        self.note_miss_with(handle, || {});
    }

    /// As [`note_miss`](Self::note_miss), and `wake` runs when this miss is the
    /// first of its handle in the revision, after the source was invalidated.
    /// A backend whose owner would otherwise sleep until something else wakes
    /// it passes the call that asks for an owner turn, so the sample the miss
    /// owes is not left waiting; a backend with OS change notifications that
    /// already wake the owner uses `note_miss`.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        test
    ))]
    pub(crate) fn note_miss_with(&self, handle: u64, wake: impl FnOnce()) {
        let revision = self.last.lock().revision().get();
        let first = self.misses.lock().first_miss(handle, revision);
        if first {
            self.invalidate();
            wake();
        }
    }
}

/// Remembers which display handles missed in the current snapshot revision, so
/// windows on displays the snapshot omits ask for one fresh sample per handle
/// and revision rather than one per owner wake, however many such displays
/// there are.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
#[derive(Debug, Default)]
struct MissLedger {
    revision: u64,
    missed: Vec<u64>,
}

#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
impl MissLedger {
    /// Whether this miss is the first of `handle` in snapshot `revision`.
    fn first_miss(&mut self, handle: u64, revision: u64) -> bool {
        if revision != self.revision {
            self.revision = revision;
            self.missed.clear();
        }
        if self.missed.contains(&handle) {
            return false;
        }
        self.missed.push(handle);
        true
    }
}

/// The table of the refresh period's one constructor and publisher, which
/// every display source is held to: one display's cadence, as each platform
/// reports it, is the row's period, and a value that describes no cadence is
/// no period anywhere.
#[cfg(test)]
mod mapping {
    use std::time::Duration;

    use flui_platform::{DisplayKey, DisplayRegistry, RefreshPeriod};

    use super::Refresh;
    use crate::display::android::sample::RawDisplay;
    use crate::display::apple::link_rate::LinkPeriod;
    use crate::display::web::refresh_estimate::RefreshEstimator;
    use crate::display::windows::sample::display_period;

    /// One display's cadence as each platform reports it.
    struct Cadence {
        case: &'static str,
        /// Hertz: Android's `Display.Mode`, CoreGraphics' mode rate.
        hertz: f64,
        /// Windows' `DISPLAYCONFIG_RATIONAL`; `None` where no `u32` pair says it.
        ratio: Option<(u32, u32)>,
        /// Millihertz: Wayland's `wl_output.mode`, a RandR mode.
        millihertz: Option<i64>,
        /// Seconds between frames: a display link's tick, a browser's frames.
        interval: f64,
        /// The period, in nanoseconds; `None` for no cadence.
        period: Option<u64>,
    }

    const CADENCES: [Cadence; 11] = [
        Cadence {
            case: "60 Hz",
            hertz: 60.0,
            ratio: Some((60, 1)),
            millihertz: Some(60_000),
            interval: 1.0 / 60.0,
            period: Some(16_666_667),
        },
        Cadence {
            case: "59.94 Hz",
            hertz: 59.94,
            ratio: Some((60_000, 1001)),
            millihertz: Some(59_940),
            interval: 1001.0 / 60_000.0,
            period: Some(16_683_333),
        },
        Cadence {
            case: "120 Hz",
            hertz: 120.0,
            ratio: Some((120, 1)),
            millihertz: Some(120_000),
            interval: 1.0 / 120.0,
            period: Some(8_333_333),
        },
        Cadence {
            case: "1 kHz, the fastest cadence",
            hertz: 1000.0,
            ratio: Some((1000, 1)),
            millihertz: Some(1_000_000),
            interval: 0.001,
            period: Some(1_000_000),
        },
        Cadence {
            case: "1 Hz, the slowest cadence",
            hertz: 1.0,
            ratio: Some((1, 1)),
            millihertz: Some(1000),
            interval: 1.0,
            period: Some(1_000_000_000),
        },
        Cadence {
            case: "faster than 1 kHz",
            hertz: 5000.0,
            ratio: Some((5000, 1)),
            millihertz: Some(5_000_000),
            interval: 0.0002,
            period: None,
        },
        Cadence {
            case: "slower than 1 Hz",
            hertz: 0.5,
            ratio: Some((1, 2)),
            millihertz: Some(500),
            interval: 2.0,
            period: None,
        },
        Cadence {
            case: "nothing reported",
            hertz: 0.0,
            ratio: Some((0, 0)),
            millihertz: Some(0),
            interval: 0.0,
            period: None,
        },
        Cadence {
            case: "negative",
            hertz: -60.0,
            ratio: None,
            millihertz: Some(-60_000),
            interval: -1.0 / 60.0,
            period: None,
        },
        Cadence {
            case: "not a number",
            hertz: f64::NAN,
            ratio: None,
            millihertz: None,
            interval: f64::NAN,
            period: None,
        },
        Cadence {
            case: "infinite",
            hertz: f64::INFINITY,
            ratio: None,
            millihertz: None,
            interval: f64::INFINITY,
            period: None,
        },
    ];

    /// The period an Android display sample reporting `hertz` publishes.
    fn android(hertz: f64) -> Option<RefreshPeriod> {
        let id = DisplayRegistry::new()
            .mint(&DisplayKey::AndroidId(0))
            .expect("BUG: a fresh registry mints an id");
        let display = RawDisplay {
            display_id: 0,
            name: "Built-in".into(),
            width: 1080,
            height: 2400,
            density_dpi: 420,
            refresh_hz: hertz as f32,
            max_refresh_hz: None,
        };
        display
            .info(id)
            .expect("BUG: the sample has a size and a density")
            .refresh_period
    }

    /// The period a display link measuring ticks `interval` apart publishes.
    fn display_link(interval: f64) -> Option<RefreshPeriod> {
        LinkPeriod::default().observe(interval)
    }

    /// The period a browser estimates from frames `interval` apart, after a
    /// window of them.
    fn browser(interval: f64) -> Option<RefreshPeriod> {
        let mut estimator = RefreshEstimator::new();
        let millis = interval * 1000.0;
        for frame in 0..=30_u32 {
            let _ = estimator.frame(1000.0 + f64::from(frame) * millis);
        }
        estimator.published()
    }

    #[test]
    fn every_display_source_reads_a_cadence_alike() {
        for cadence in CADENCES {
            let case = cadence.case;
            let periods = [
                ("Android", Some(android(cadence.hertz))),
                ("CoreGraphics", Some(Refresh::Hertz(cadence.hertz).period())),
                (
                    "Windows",
                    cadence.ratio.map(|ratio| display_period(Some(ratio), || 0)),
                ),
                (
                    "Wayland and RandR",
                    cadence
                        .millihertz
                        .map(|millihertz| Refresh::Millihertz(millihertz).period()),
                ),
                ("display link", Some(display_link(cadence.interval))),
                ("browser", Some(browser(cadence.interval))),
            ];
            for (source, period) in periods {
                // A source that cannot say this value has no row to hold.
                let Some(period) = period else {
                    continue;
                };
                match (period.map(RefreshPeriod::get), cadence.period) {
                    (None, None) => {}
                    (Some(period), Some(nanos)) => assert!(
                        period.abs_diff(Duration::from_nanos(nanos)) <= Duration::from_micros(1),
                        "{source}: {case} is {period:?}"
                    ),
                    (period, nanos) => {
                        panic!("{source}: {case} is {period:?}, the row says {nanos:?} ns")
                    }
                }
            }
        }
    }

    /// The period of every common panel rate CoreGraphics reports stays the
    /// one the AppKit backend reported before the display snapshot
    /// (`1 / hz`), to the nanosecond.
    #[test]
    fn macos_shim_keeps_the_period_it_reports_today() {
        for millihertz in [
            24_000, 30_000, 48_000, 50_000, 59_940, 60_000, 75_000, 90_000, 100_000, 119_880,
            120_000, 144_000, 165_000, 240_000, 360_000,
        ] {
            let hz = f64::from(millihertz) / 1000.0;
            let period = Refresh::Hertz(hz).period().map(RefreshPeriod::get);
            let before = Duration::from_secs_f64(1.0 / hz);
            assert!(
                period.is_some_and(|period| period.abs_diff(before) <= Duration::from_nanos(1)),
                "macOS {hz} Hz: {period:?}, before the snapshot {before:?}"
            );
        }
    }

    /// The publisher both measurers share: the first measurement publishes, a
    /// later one only outside the band around what is published, and a
    /// forgotten period is unknown again.
    #[test]
    fn a_measured_period_is_published_outside_its_band() {
        let hz = |hertz| {
            Refresh::Hertz(hertz)
                .period()
                .expect("BUG: a test rate describes a cadence")
        };
        let mut measured = super::MeasuredPeriod::default();
        assert!(measured.departs(hz(60.0), 0.05), "nothing is published yet");
        assert_eq!(measured.publish(hz(60.0)), hz(60.0));
        // (measured rate, band, whether it departs from 60 Hz)
        for (hertz, band, departs) in [
            (60.0, 0.05, false),
            (61.0, 0.05, false),
            (58.0, 0.05, false),
            (64.0, 0.05, true),
            (61.0, 0.01, true),
            (120.0, 0.03, true),
        ] {
            assert_eq!(
                measured.departs(hz(hertz), band),
                departs,
                "{hertz} Hz within {band} of 60 Hz"
            );
        }
        assert_eq!(measured.published(), Some(hz(60.0)));
        measured.forget();
        assert_eq!(measured.published(), None);
        assert!(
            measured.departs(hz(60.0), 0.05),
            "a forgotten period departs"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use flui_foundation::geometry::Bounds;
    use flui_platform::{DisplayInfo, DisplayKey};

    use super::DisplaySource;
    use crate::error::PlatformError;

    fn one_display(source: &DisplaySource, scale: f64) -> Vec<DisplayInfo> {
        let id = source
            .mint(&DisplayKey::Injected(1))
            .expect("ids are not exhausted");
        vec![DisplayInfo::new(id, "Test", Bounds::default(), scale)]
    }

    /// A source with a counted sample that reads one display.
    fn counted(source: &DisplaySource, samples: &Cell<usize>) {
        source
            .snapshot_with(|| {
                samples.set(samples.get() + 1);
                Ok(one_display(source, 1.0))
            })
            .expect("the sample publishes");
    }

    #[test]
    fn invalidation_during_a_sample_stays_stale() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        source
            .snapshot_with(|| {
                samples.set(samples.get() + 1);
                // The OS changes while the screens are being read.
                source.invalidate();
                Ok(one_display(&source, 1.0))
            })
            .expect("the sample publishes");
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "the next call reads again");
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "and then the snapshot is kept");
    }

    #[test]
    fn a_monitor_resolves_only_once_its_snapshot_is_published() {
        let source = DisplaySource::new();
        let resolvable = std::sync::Mutex::new(Vec::<u32>::new());
        let seen_during_sample = Cell::new(None);
        source
            .snapshot_with_aux(
                || {
                    // A reader on another thread, between the OS read and the
                    // publish, must not find the new monitor.
                    seen_during_sample.set(Some(resolvable.lock().expect("lock").clone()));
                    Ok((one_display(&source, 1.0), vec![42]))
                },
                |monitors| {
                    assert_eq!(
                        source.published().displays().len(),
                        1,
                        "the snapshot is published before its monitors resolve"
                    );
                    *resolvable.lock().expect("lock") = monitors;
                },
            )
            .expect("the sample publishes");
        assert_eq!(seen_during_sample.take(), Some(vec![]));
        assert_eq!(*resolvable.lock().expect("lock"), vec![42]);

        // A sample that cannot be published commits nothing.
        source.invalidate();
        let failed = source.snapshot_with_aux(
            || {
                Err::<(Vec<DisplayInfo>, Vec<u32>), _>(PlatformError::Displays {
                    message: "the OS refused".into(),
                })
            },
            |monitors| *resolvable.lock().expect("lock") = monitors,
        );
        assert!(failed.is_err());
        assert_eq!(*resolvable.lock().expect("lock"), vec![42]);
    }

    #[test]
    fn a_failed_sample_stays_stale() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        counted(&source, &samples);
        source.invalidate();
        let failed = source.snapshot_with(|| {
            Err(PlatformError::Displays {
                message: "the OS refused".into(),
            })
        });
        assert!(failed.is_err());
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "the failure left the source stale");
        counted(&source, &samples);
        assert_eq!(samples.get(), 2);
    }

    #[test]
    fn a_missed_display_resamples_once_per_revision() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        counted(&source, &samples);
        source.note_miss(7);
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "the first miss asks for a sample");
        // The display is still absent from the identical sample: asking again
        // in the same revision must not keep the source stale.
        for _ in 0..5 {
            source.note_miss(7);
            counted(&source, &samples);
        }
        assert_eq!(samples.get(), 2, "a repeated miss costs nothing");
        // A snapshot that changed is a new revision, so the miss may retry.
        source.invalidate();
        source
            .snapshot_with(|| Ok(one_display(&source, 2.0)))
            .expect("the sample publishes");
        source.note_miss(7);
        counted(&source, &samples);
        assert_eq!(samples.get(), 3, "a new revision allows one retry");
    }

    #[test]
    fn a_first_miss_asks_for_an_owner_turn_and_a_repeat_does_not() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        counted(&source, &samples);
        let wakes = Cell::new(0);
        let miss = |handle| source.note_miss_with(handle, || wakes.set(wakes.get() + 1));
        miss(7);
        assert_eq!(wakes.get(), 1, "an idle owner must be woken to sample");
        miss(7);
        assert_eq!(wakes.get(), 1, "the same miss in the same revision is free");
        miss(8);
        assert_eq!(wakes.get(), 2, "another handle owes its own sample");
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "the sample the wake is for");
    }

    #[test]
    fn alternating_misses_resample_once_each() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        counted(&source, &samples);
        for handle in [7, 8, 7, 8, 7, 8] {
            source.note_miss(handle);
            counted(&source, &samples);
        }
        assert_eq!(samples.get(), 3, "one initial sample and one per handle");
    }
}
