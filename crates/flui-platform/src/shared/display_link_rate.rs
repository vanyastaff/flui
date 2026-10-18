//! The two pure rules behind iOS frame pacing: which frame-rate request a
//! `CADisplayLink` gets on a given iOS version, and when the cadence the link
//! measures becomes a new published refresh period.

use std::time::Duration;

use flui_platform_api::RefreshPeriod;

use super::apple_os_version::{OsVersion, Since, VersionedFeature};

/// The floor of the requested range: an adaptive ProMotion panel may drop to
/// 60 Hz when nothing moves, never lower while a frame source is running.
const RANGE_FLOOR: u16 = 60;

/// What a screen is assumed to sustain when it reports no maximum.
const ASSUMED_MAXIMUM: u16 = 60;

/// The highest rate a screen is believed to report; matches the largest
/// cadence a [`RefreshPeriod`] describes.
const MAXIMUM_CEILING: u16 = 1000;

/// `CADisplayLink.preferredFrameRateRange`, which iOS 15 introduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameRateRangeProperty;

impl VersionedFeature for FrameRateRangeProperty {
    const SINCE: OsVersion = OsVersion::new(15, 0, 0);
}

/// A frame-rate range in frames per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrameRateRange {
    pub(crate) minimum: f32,
    pub(crate) maximum: f32,
    pub(crate) preferred: f32,
}

/// How a `CADisplayLink` is told to run at the screen's best rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FrameRatePreference {
    /// iOS 15 and later: a range, carrying the proof the property exists.
    Range(Since<FrameRateRangeProperty>, FrameRateRange),
    /// Before iOS 15: a single `preferredFramesPerSecond`.
    FramesPerSecond(u16),
}

/// The request that runs a link at `maximum_frames_per_second` (the screen's
/// `maximumFramesPerSecond`) on `version`.
///
/// A maximum below one is a screen that reports nothing and reads as 60; one
/// above 1000 is held at 1000. The range's minimum is 60, or the maximum when
/// that is lower, so the range is never inverted.
pub(crate) fn frame_rate_preference(
    version: OsVersion,
    maximum_frames_per_second: i64,
) -> FrameRatePreference {
    let maximum = if maximum_frames_per_second < 1 {
        ASSUMED_MAXIMUM
    } else {
        u16::try_from(maximum_frames_per_second)
            .unwrap_or(MAXIMUM_CEILING)
            .min(MAXIMUM_CEILING)
    };
    match version.since::<FrameRateRangeProperty>() {
        Some(proof) => FrameRatePreference::Range(
            proof,
            FrameRateRange {
                minimum: f32::from(RANGE_FLOOR.min(maximum)),
                maximum: f32::from(maximum),
                preferred: f32::from(maximum),
            },
        ),
        None => FrameRatePreference::FramesPerSecond(maximum),
    }
}

/// How far a measured period may sit from the published one, as a fraction of
/// the published period, before it is published. An adaptive ProMotion panel
/// jitters inside the band every tick; a real rate switch (60 to 120 Hz is a
/// 50% step) is far outside it.
const PUBLISH_BAND: f64 = 0.05;

/// The refresh period a display link publishes, from the interval between its
/// ticks.
#[derive(Debug, Default)]
pub(crate) struct LinkPeriod {
    published: Option<RefreshPeriod>,
}

impl LinkPeriod {
    /// Take the interval one tick reports (`targetTimestamp - timestamp`, in
    /// seconds). Answers the new period when it leaves the band around the
    /// published one (the first valid interval always does), and `None` when
    /// nothing changes. An interval that describes no cadence (negative, not
    /// finite, outside 1 ms to 1 s) is ignored.
    pub(crate) fn observe(&mut self, interval_seconds: f64) -> Option<RefreshPeriod> {
        let measured = Duration::try_from_secs_f64(interval_seconds)
            .ok()
            .and_then(|interval| RefreshPeriod::try_from(interval).ok())?;
        if let Some(published) = self.published {
            let published = published.get().as_secs_f64();
            let drift = (measured.get().as_secs_f64() - published).abs();
            if drift <= published * PUBLISH_BAND {
                return None;
            }
        }
        self.published = Some(measured);
        Some(measured)
    }

    /// Forget the published period: the link moved to another screen, or was
    /// rebuilt, and its cadence is unknown until the next tick.
    pub(crate) fn forget(&mut self) {
        self.published = None;
    }
}

/// The periods display links published, by screen and the window whose link
/// measured them. A screen can host several windows, each with its own link, so
/// an entry belongs to its window: one window's link going away takes that
/// window's entry and leaves the screen's other entries.
#[derive(Debug)]
pub(crate) struct ScreenPeriods<W> {
    /// `(screen number, window, period)` in the order windows first reported.
    entries: Vec<(u64, W, RefreshPeriod)>,
}

impl<W: Copy + PartialEq> ScreenPeriods<W> {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Record the period `window`'s link measured on `screen`.
    pub(crate) fn report(&mut self, screen: u64, window: W, period: RefreshPeriod) {
        match self
            .entries
            .iter_mut()
            .find(|(held, owner, _)| *held == screen && *owner == window)
        {
            Some(entry) => entry.2 = period,
            None => self.entries.push((screen, window, period)),
        }
    }

    /// Drop `window`'s entry for `screen`; whether there was one.
    pub(crate) fn forget(&mut self, screen: u64, window: W) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|(held, owner, _)| !(*held == screen && *owner == window));
        self.entries.len() != before
    }

    /// The period of `screen`: that of the window that reported first among
    /// those still live, or `None` when no link on it is measuring.
    pub(crate) fn period_of(&self, screen: u64) -> Option<RefreshPeriod> {
        self.entries
            .iter()
            .find(|(held, _, _)| *held == screen)
            .map(|(_, _, period)| *period)
    }

    /// Keep only the entries of the screens in `live`.
    pub(crate) fn retain_screens(&mut self, live: &[u64]) {
        self.entries.retain(|(held, _, _)| live.contains(held));
    }
}

#[cfg(test)]
mod tests {
    use flui_platform_api::RefreshPeriod;

    use super::{
        FrameRatePreference, FrameRateRange, LinkPeriod, OsVersion, ScreenPeriods,
        frame_rate_preference,
    };

    fn period_hz(hz: f64) -> RefreshPeriod {
        RefreshPeriod::from_hz(hz).expect("BUG: a test rate describes a cadence")
    }

    fn range(preference: FrameRatePreference) -> Option<FrameRateRange> {
        match preference {
            FrameRatePreference::Range(_, range) => Some(range),
            FrameRatePreference::FramesPerSecond(_) => None,
        }
    }

    #[test]
    fn ios_14_uses_preferred_frames_per_second() {
        for (version, maximum, expected) in [
            (OsVersion::new(13, 0, 0), 60, 60),
            (OsVersion::new(14, 8, 1), 120, 120),
            (OsVersion::new(14, 0, 0), 0, 60),
            (OsVersion::new(14, 0, 0), -4, 60),
            (OsVersion::new(14, 0, 0), 1_000_000, 1000),
        ] {
            assert_eq!(
                frame_rate_preference(version, maximum),
                FrameRatePreference::FramesPerSecond(expected),
                "{version:?} max {maximum}"
            );
        }
    }

    #[test]
    fn ios_15_uses_the_frame_rate_range() {
        for (version, maximum, minimum, ceiling) in [
            (OsVersion::new(15, 0, 0), 120, 60.0, 120.0),
            (OsVersion::new(17, 4, 1), 60, 60.0, 60.0),
            (OsVersion::new(18, 0, 0), 240, 60.0, 240.0),
            // A maximum below the floor never inverts the range.
            (OsVersion::new(16, 0, 0), 30, 30.0, 30.0),
            // A screen reporting nothing reads as 60.
            (OsVersion::new(16, 0, 0), 0, 60.0, 60.0),
        ] {
            let range = range(frame_rate_preference(version, maximum))
                .unwrap_or_else(|| panic!("{version:?} has the range property"));
            assert_eq!(
                range,
                FrameRateRange {
                    minimum,
                    maximum: ceiling,
                    preferred: ceiling,
                },
                "{version:?} max {maximum}"
            );
        }
    }

    #[test]
    fn ios_period_follows_link_ticks() {
        let mut link = LinkPeriod::default();
        // The first valid tick publishes; before it nothing is known.
        assert_eq!(link.observe(1.0 / 60.0), Some(period_hz(60.0)));
        // A rate switch to 120 Hz and back publishes each step once.
        assert_eq!(link.observe(1.0 / 120.0), Some(period_hz(120.0)));
        assert_eq!(link.observe(1.0 / 120.0), None);
        assert_eq!(link.observe(1.0 / 60.0), Some(period_hz(60.0)));
        // Low power mode halves the rate.
        assert_eq!(link.observe(1.0 / 30.0), Some(period_hz(30.0)));
        // Intervals that describe no cadence change nothing.
        for interval in [-0.016, f64::NAN, f64::INFINITY, 0.0, 0.0001, 5.0] {
            assert_eq!(link.observe(interval), None, "{interval}");
        }
        assert_eq!(link.observe(1.0 / 30.0), None);
        // A link that was rebuilt starts again from unknown.
        link.forget();
        assert_eq!(link.observe(1.0 / 30.0), Some(period_hz(30.0)));
    }

    #[test]
    fn closing_one_window_keeps_the_screen_period_of_another() {
        let mut periods = ScreenPeriods::new();
        periods.report(1, 10u64, period_hz(120.0));
        periods.report(1, 11, period_hz(60.0));
        periods.report(2, 10, period_hz(60.0));
        // The first window's link goes away: the second still measures screen 1.
        assert!(periods.forget(1, 10));
        assert_eq!(periods.period_of(1), Some(period_hz(60.0)));
        // Screen 2 had only that window's entry for 10, which is untouched.
        assert_eq!(periods.period_of(2), Some(period_hz(60.0)));
        // Forgetting an entry that is not there changes nothing.
        assert!(!periods.forget(1, 10));
        // The last link on a screen takes the screen's period with it.
        assert!(periods.forget(1, 11));
        assert_eq!(periods.period_of(1), None);
        // A window re-reporting replaces its own entry.
        periods.report(2, 10, period_hz(120.0));
        assert_eq!(periods.period_of(2), Some(period_hz(120.0)));
        periods.retain_screens(&[]);
        assert_eq!(periods.period_of(2), None);
    }

    #[test]
    fn ios_adaptive_ticks_inside_the_band_publish_nothing() {
        let mut link = LinkPeriod::default();
        let base = 1.0 / 120.0;
        assert_eq!(link.observe(base), Some(period_hz(120.0)));
        // Jitter of up to 5% either way is silent and does not move the band:
        // drift is judged against the published value, not the last tick.
        for factor in [1.0, 1.02, 0.98, 1.049, 0.951, 1.049, 0.951, 1.0] {
            assert_eq!(link.observe(base * factor), None, "x{factor}");
        }
        // Just outside the band publishes the measured value.
        let outside = base * 1.06;
        let published = link.observe(outside).expect("outside the band publishes");
        assert!((published.get().as_secs_f64() - outside).abs() < 1e-9);
        // Slow drift never accumulates past the band without publishing.
        let mut seen = 0;
        for step in 0..40 {
            if link
                .observe(outside * (1.0 + 0.01 * f64::from(step)))
                .is_some()
            {
                seen += 1;
            }
        }
        assert!(seen >= 1, "a long drift must publish eventually");
    }
}
