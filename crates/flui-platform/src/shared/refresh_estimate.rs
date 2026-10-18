//! The refresh period a browser tab can measure for itself.
//!
//! A page cannot ask the browser for its display's refresh rate; it can only
//! watch the timestamps `requestAnimationFrame` hands its callbacks, which are
//! aligned to the display's vertical blank. [`RefreshEstimator`] turns that
//! stream into a [`RefreshPeriod`].
//!
//! The value is an **estimate**, not a report from the display: it follows the
//! cadence the browser is presenting at, which a power saver, a throttled
//! frame, a compositor that halves the rate or a variable-refresh panel can
//! lower below what the panel is capable of, and it is unknown until enough
//! frames have run. The estimate is the median of the most recent frame
//! intervals, so a few dropped frames do not move it; an interval longer than
//! three medians (a background tab, a debugger pause) is not a frame time and
//! is left out. A new median is published only when it leaves a band around
//! the published value, so jitter does not republish on every frame.
//!
//! Pure: the platform feeds in timestamps and reads the published value, so
//! the rules run on every host.

use std::collections::VecDeque;
use std::time::Duration;

use flui_platform_api::RefreshPeriod;

/// How many of the most recent frame intervals the median is taken over.
const WINDOW: usize = 30;

/// How many usable intervals an estimate needs before it is published.
const MIN_INTERVALS: usize = 10;

/// An interval longer than this many medians is not a frame time.
const OUTLIER_FACTOR: f64 = 3.0;

/// A new median is published only when it differs from the published period by
/// more than this fraction of it. 3% keeps 59.94 and 60 Hz as one value and
/// still separates every pair of common panel rates (60, 75, 90, 120, 144).
const HYSTERESIS: f64 = 0.03;

/// Estimates a display's refresh period from `requestAnimationFrame`
/// timestamps and publishes it through hysteresis.
#[derive(Debug, Default)]
pub(crate) struct RefreshEstimator {
    /// The previous callback's timestamp in milliseconds; `None` at the start
    /// and after [`restart`](Self::restart), when the gap since the last frame
    /// is not a frame interval.
    previous: Option<f64>,
    /// The most recent frame intervals in milliseconds, oldest first.
    intervals: VecDeque<f64>,
    published: Option<RefreshPeriod>,
}

impl RefreshEstimator {
    /// An estimator that has seen no frame.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record one `requestAnimationFrame` callback at `timestamp_ms` (the
    /// `DOMHighResTimeStamp` the callback receives). Returns the new published
    /// period when this frame changed it.
    ///
    /// A timestamp that is not finite is ignored; one that does not advance
    /// restarts the interval but adds none.
    pub(crate) fn frame(&mut self, timestamp_ms: f64) -> Option<RefreshPeriod> {
        if !timestamp_ms.is_finite() {
            return None;
        }
        if let Some(previous) = self.previous.replace(timestamp_ms) {
            let interval = timestamp_ms - previous;
            if interval > 0.0 {
                if self.intervals.len() == WINDOW {
                    self.intervals.pop_front();
                }
                self.intervals.push_back(interval);
            }
        }
        let candidate = self.median()?;
        self.publish(candidate)
    }

    /// Forget the collected intervals and the last timestamp: the cadence may
    /// have changed (the tab became visible, the page moved to a display with
    /// another rate) and the time since the last frame is not an interval. The
    /// published period stays until a fresh estimate leaves its band.
    pub(crate) fn restart(&mut self) {
        self.previous = None;
        self.intervals.clear();
    }

    /// [`restart`](Self::restart) and also forget the published period: the
    /// page is on another display, so the old value describes none it is on.
    pub(crate) fn forget(&mut self) {
        self.restart();
        self.published = None;
    }

    /// The period published so far; `None` until enough frames have run.
    pub(crate) fn published(&self) -> Option<RefreshPeriod> {
        self.published
    }

    /// The median of the recent intervals without background outliers; `None`
    /// while there are too few of them.
    fn median(&self) -> Option<RefreshPeriod> {
        if self.intervals.len() < MIN_INTERVALS {
            return None;
        }
        let all = median_of(self.intervals.iter().copied().collect());
        let kept: Vec<f64> = self
            .intervals
            .iter()
            .copied()
            .filter(|interval| *interval <= all * OUTLIER_FACTOR)
            .collect();
        if kept.len() < MIN_INTERVALS {
            return None;
        }
        let millis = median_of(kept);
        let period = Duration::try_from_secs_f64(millis / 1000.0).ok()?;
        RefreshPeriod::try_from(period).ok()
    }

    /// Publish `candidate` if it leaves the hysteresis band; `Some` when the
    /// published value changed.
    fn publish(&mut self, candidate: RefreshPeriod) -> Option<RefreshPeriod> {
        let outside_band = match self.published {
            None => true,
            Some(published) => {
                let published = published.get().as_secs_f64();
                (candidate.get().as_secs_f64() - published).abs() > published * HYSTERESIS
            }
        };
        if !outside_band {
            return None;
        }
        self.published = Some(candidate);
        Some(candidate)
    }
}

/// The median of a non-empty list of finite values; the mean of the two middle
/// values for an even count.
fn median_of(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        f64::midpoint(values[middle - 1], values[middle])
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_platform_api::RefreshPeriod;

    use super::{MIN_INTERVALS, RefreshEstimator, WINDOW};

    /// Runs every row even after one fails, then panics listing the failing
    /// rows.
    fn run_table(table: &str, rows: &[(&str, fn())]) {
        let failed: Vec<&str> = rows
            .iter()
            .filter(|(_, row)| std::panic::catch_unwind(*row).is_err())
            .map(|(name, _)| *name)
            .collect();
        assert!(failed.is_empty(), "{table}: failing rows: {failed:?}");
    }

    /// Feed `intervals` (milliseconds) as consecutive frames starting at
    /// `start`; the last timestamp and the changes published on the way.
    fn feed(
        estimator: &mut RefreshEstimator,
        start: f64,
        intervals: &[f64],
    ) -> (f64, Vec<RefreshPeriod>) {
        let mut now = start;
        let mut published = Vec::new();
        published.extend(estimator.frame(now));
        for interval in intervals {
            now += interval;
            published.extend(estimator.frame(now));
        }
        (now, published)
    }

    fn assert_millis(period: Option<RefreshPeriod>, expected: f64) {
        let period = period.expect("a published period");
        let expected = Duration::from_secs_f64(expected / 1000.0);
        assert!(
            period.get().abs_diff(expected) <= Duration::from_micros(1),
            "{period:?} is not {expected:?}"
        );
    }

    fn median_of_thirty_samples() {
        // Thirty intervals in three values, ten of each, interleaved: the
        // median is the middle value, not the mean (16.73).
        let mut estimator = RefreshEstimator::new();
        let intervals: Vec<f64> = [16.0, 16.7, 17.5].repeat(WINDOW / 3);
        feed(&mut estimator, 1000.0, &intervals);
        assert_millis(estimator.published(), 16.7);
    }

    fn only_the_last_thirty_intervals_count() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.0; WINDOW]);
        // Thirty intervals at 8 ms push every 16 ms interval out of the window.
        feed(&mut estimator, now, &[8.0; WINDOW]);
        assert_millis(estimator.published(), 8.0);
    }

    fn background_outlier_is_dropped() {
        // A backgrounded tab resumes with one very long interval and a few
        // slow ones; none of them is a frame time, and the estimate stays.
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 20]);
        let (_, published) = feed(&mut estimator, now, &[4000.0, 16.7, 16.7, 16.7, 16.7]);
        assert!(published.is_empty(), "the outlier republished: {published:?}");
        assert_millis(estimator.published(), 16.7);
    }

    fn too_few_usable_intervals_publish_nothing() {
        // Enough intervals, but once the long gaps are left out fewer than the
        // minimum remain.
        let mut estimator = RefreshEstimator::new();
        let mut gaps = vec![16.7; MIN_INTERVALS - 1];
        gaps.extend([2000.0; MIN_INTERVALS - 3]);
        feed(&mut estimator, 0.0, &gaps);
        assert_eq!(estimator.published(), None);
    }

    fn too_few_frames_publish_nothing() {
        let mut estimator = RefreshEstimator::new();
        let (_, published) = feed(&mut estimator, 0.0, &[16.7; MIN_INTERVALS - 1]);
        assert_eq!(published.len(), 0);
        assert_eq!(estimator.published(), None);
    }

    fn re_estimates_after_visibility() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 20]);
        assert_millis(estimator.published(), 16.7);
        // The tab was hidden for a minute and came back on a 120 Hz display.
        // Twelve fresh intervals are enough only because the twenty old ones
        // were forgotten; with them still in the window the median stays 16.7.
        estimator.restart();
        let (_, published) = feed(&mut estimator, now + 60_000.0, &[8.33; 12]);
        assert_eq!(published.len(), 1, "one republication: {published:?}");
        assert_millis(estimator.published(), 8.33);
    }

    fn the_old_period_stays_until_a_new_estimate_exists() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 20]);
        estimator.restart();
        let (_, published) = feed(&mut estimator, now + 60_000.0, &[8.33; MIN_INTERVALS - 1]);
        assert!(published.is_empty(), "{published:?}");
        assert_millis(estimator.published(), 16.7);
    }

    fn forgetting_leaves_no_period_until_a_new_estimate() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 20]);
        estimator.forget();
        assert_eq!(estimator.published(), None);
        // The same rate on the new display is published again, not suppressed
        // by the band around the forgotten value.
        let (_, published) = feed(&mut estimator, now + 1000.0, &[16.7; 12]);
        assert_eq!(published.len(), 1);
        assert_millis(estimator.published(), 16.7);
    }

    fn jitter_inside_the_band_publishes_once() {
        let mut estimator = RefreshEstimator::new();
        // 60 Hz with ±1 ms timestamp jitter over 120 frames.
        let jittery: Vec<f64> = (0..120)
            .map(|frame| 16.667 + [0.0, 0.9, -0.9, 0.4, -0.4][frame % 5])
            .collect();
        let (_, published) = feed(&mut estimator, 0.0, &jittery);
        assert_eq!(published.len(), 1, "republished on jitter: {published:?}");
    }

    fn a_rate_outside_the_band_republishes() {
        let mut estimator = RefreshEstimator::new();
        let (now, first) = feed(&mut estimator, 0.0, &[16.667; 40]);
        assert_eq!(first.len(), 1);
        // 59.94 Hz is the same published rate: no republication.
        let (now, same) = feed(&mut estimator, now, &[16.683; 40]);
        assert!(same.is_empty(), "{same:?}");
        // 75 Hz is a different one. While the window holds both rates the
        // median passes through their midpoint, which can publish once on the
        // way; the settled value is the new rate.
        let (_, changed) = feed(&mut estimator, now, &[13.333; 40]);
        assert!(!changed.is_empty() && changed.len() <= 2, "{changed:?}");
        assert_millis(estimator.published(), 13.333);
    }

    fn timestamps_that_do_not_advance_add_no_interval() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 5]);
        // A repeated timestamp and two that are not numbers.
        for timestamp in [now, f64::NAN, f64::INFINITY] {
            assert_eq!(estimator.frame(timestamp), None);
        }
        let (_, published) = feed(&mut estimator, now, &[16.7; 5]);
        assert_eq!(published.len(), 1, "ten usable intervals: {published:?}");
        assert_millis(estimator.published(), 16.7);
    }

    fn a_period_outside_one_millisecond_to_one_second_is_unknown() {
        for interval in [0.4, 1500.0] {
            let mut estimator = RefreshEstimator::new();
            feed(&mut estimator, 0.0, &[interval; 40]);
            assert_eq!(estimator.published(), None, "{interval} ms");
        }
    }

    #[test]
    fn web_refresh_estimate_table() {
        run_table(
            "web_refresh_estimate_table",
            &[
                ("median_of_thirty_samples", median_of_thirty_samples),
                (
                    "only_the_last_thirty_intervals_count",
                    only_the_last_thirty_intervals_count,
                ),
                ("background_outlier_is_dropped", background_outlier_is_dropped),
                (
                    "too_few_usable_intervals_publish_nothing",
                    too_few_usable_intervals_publish_nothing,
                ),
                ("too_few_frames_publish_nothing", too_few_frames_publish_nothing),
                ("re_estimates_after_visibility", re_estimates_after_visibility),
                (
                    "the_old_period_stays_until_a_new_estimate_exists",
                    the_old_period_stays_until_a_new_estimate_exists,
                ),
                (
                    "forgetting_leaves_no_period_until_a_new_estimate",
                    forgetting_leaves_no_period_until_a_new_estimate,
                ),
                (
                    "jitter_inside_the_band_publishes_once",
                    jitter_inside_the_band_publishes_once,
                ),
                (
                    "a_rate_outside_the_band_republishes",
                    a_rate_outside_the_band_republishes,
                ),
                (
                    "timestamps_that_do_not_advance_add_no_interval",
                    timestamps_that_do_not_advance_add_no_interval,
                ),
                (
                    "a_period_outside_one_millisecond_to_one_second_is_unknown",
                    a_period_outside_one_millisecond_to_one_second_is_unknown,
                ),
            ],
        );
    }
}
