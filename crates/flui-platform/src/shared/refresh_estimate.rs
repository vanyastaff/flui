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
//! frames have run.
//!
//! The estimate is the mean of the sorted recent frame intervals without the
//! slowest and fastest tenth (a trimmed mean), so a few dropped frames do not
//! move it and, unlike a median, it lands between the two interval lengths a clock
//! quantised to whole milliseconds reports for a rate such as 120 Hz (8 and 9
//! ms, never 8.33). An interval longer than three times the window's lower
//! quartile (a background tab, a debugger pause) is not a frame time and is
//! left out.
//!
//! A new estimate is published only when it leaves a band around the published
//! value, so jitter does not republish, and only after it has done so for
//! several consecutive frames while the latest intervals agree with it. While
//! the window holds two rates the estimate sits between them, at a value the
//! display never ran at; the latest intervals disagree with it, so it is never
//! published.
//!
//! Pure: the platform feeds in timestamps and reads the published value, so
//! the rules run on every host.

use std::time::Duration;

use flui_platform_api::RefreshPeriod;

/// How many of the most recent frame intervals the estimate is taken over.
const WINDOW: usize = 30;

/// How many of the latest intervals must agree with a candidate before it
/// replaces the published period.
const RECENT: usize = 10;

/// How many consecutive frames a candidate must stay outside the band, with
/// the latest intervals agreeing, before it replaces the published period.
const PERSISTENCE: u8 = 5;

/// How far (as a fraction) a candidate may move from where its run began and
/// still count as stable. A glide between two rates moves faster than this per
/// frame; steady jitter moves far less.
const STABILITY: f64 = 0.01;

/// How many usable intervals an estimate needs before it is published.
const MIN_INTERVALS: usize = 10;

/// An interval longer than this many times the window's lower quartile is not
/// a frame time.
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
    intervals: Intervals,
    published: Option<RefreshPeriod>,
    /// The candidate that has stayed outside the published band, with the
    /// latest intervals agreeing, and for how many consecutive frames.
    pending: Option<Pending>,
}

/// A candidate period waiting to replace the published one.
#[derive(Debug, Clone, Copy)]
struct Pending {
    /// The candidate when its run of stable frames began.
    anchor_ms: f64,
    frames: u8,
}

/// The last [`WINDOW`] frame intervals in milliseconds, as a ring.
#[derive(Debug)]
struct Intervals {
    values: [f64; WINDOW],
    len: usize,
    next: usize,
}

impl Default for Intervals {
    fn default() -> Self {
        Self {
            values: [0.0; WINDOW],
            len: 0,
            next: 0,
        }
    }
}

impl Intervals {
    fn push(&mut self, interval: f64) {
        self.values[self.next] = interval;
        self.next = (self.next + 1) % WINDOW;
        self.len = (self.len + 1).min(WINDOW);
    }

    fn clear(&mut self) {
        self.len = 0;
        self.next = 0;
    }

    /// The mean of the latest `count` intervals; `None` while fewer exist.
    fn recent_mean(&self, count: usize) -> Option<f64> {
        if self.len < count {
            return None;
        }
        let sum: f64 = (1..=count)
            .map(|back| self.values[(self.next + WINDOW - back) % WINDOW])
            .sum();
        Some(sum / count as f64)
    }
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
                self.intervals.push(interval);
            }
        }
        let candidate = self.candidate()?;
        self.publish(candidate)
    }

    /// Forget the collected intervals and the last timestamp: the cadence may
    /// have changed (the tab became visible, the page moved to a display with
    /// another rate) and the time since the last frame is not an interval. The
    /// published period stays until a fresh estimate leaves its band.
    pub(crate) fn restart(&mut self) {
        self.previous = None;
        self.intervals.clear();
        self.pending = None;
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

    /// The trimmed mean of the recent intervals without background
    /// outliers; `None` while there are too few of them.
    fn candidate(&self) -> Option<RefreshPeriod> {
        let count = self.intervals.len;
        // The first estimate waits for a full window, so what is published
        // first is the best the window can say; a replacement needs fewer
        // because it is judged against what is already published.
        let needed = if self.published.is_none() {
            WINDOW
        } else {
            MIN_INTERVALS
        };
        if count < needed {
            return None;
        }
        let mut storage = self.intervals.values;
        let sorted = &mut storage[..count];
        sorted.sort_by(f64::total_cmp);
        // The baseline is the lower quartile, not the median: a window that is
        // mostly background frames has a slow median, and only a baseline from
        // the fast end still recognises those frames as outliers. What is not
        // an outlier is a prefix of the sorted intervals.
        let baseline = sorted[count / 4];
        let kept = &sorted[..sorted.partition_point(|v| *v <= baseline * OUTLIER_FACTOR)];
        if kept.len() < MIN_INTERVALS {
            return None;
        }
        let trim = kept.len() / 10;
        let middle = &kept[trim..kept.len() - trim];
        let millis = middle.iter().sum::<f64>() / middle.len() as f64;
        let period = Duration::try_from_secs_f64(millis / 1000.0).ok()?;
        RefreshPeriod::try_from(period).ok()
    }

    /// Publish `candidate` if it has left the hysteresis band for
    /// [`PERSISTENCE`] consecutive frames while the latest intervals agree with
    /// it; `Some` when the published value changed. The first estimate is
    /// published at once.
    fn publish(&mut self, candidate: RefreshPeriod) -> Option<RefreshPeriod> {
        let candidate_ms = candidate.get().as_secs_f64() * 1000.0;
        if let Some(published) = self.published {
            let published_ms = published.get().as_secs_f64() * 1000.0;
            if !differs(candidate_ms, published_ms, HYSTERESIS) {
                self.pending = None;
                return None;
            }
            let agrees = self
                .intervals
                .recent_mean(RECENT)
                .is_some_and(|recent| !differs(recent, candidate_ms, 2.0 * HYSTERESIS));
            if !agrees {
                self.pending = None;
                return None;
            }
            // Stable: the candidate is no longer moving. While the window holds
            // two rates it glides from one to the other, a step at a time.
            let pending = match self.pending {
                Some(pending) if !differs(candidate_ms, pending.anchor_ms, STABILITY) => Pending {
                    anchor_ms: pending.anchor_ms,
                    frames: pending.frames + 1,
                },
                _ => Pending {
                    anchor_ms: candidate_ms,
                    frames: 1,
                },
            };
            self.pending = Some(pending);
            if pending.frames < PERSISTENCE {
                return None;
            }
        }
        self.pending = None;
        self.published = Some(candidate);
        Some(candidate)
    }
}

/// Whether `value` is more than `fraction` of `reference` away from it.
fn differs(value: f64, reference: f64, fraction: f64) -> bool {
    (value - reference).abs() > reference * fraction
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
        // Thirty intervals in three values, ten of each, interleaved. The
        // estimate is the mean of the sorted intervals without the three
        // fastest and three slowest (7 x 16.0, 10 x 16.7, 7 x 17.5).
        let mut estimator = RefreshEstimator::new();
        let intervals: Vec<f64> = [16.0, 16.7, 17.5].repeat(WINDOW / 3);
        feed(&mut estimator, 1000.0, &intervals);
        let published = estimator.published().expect("a published period");
        let millis = published.get().as_secs_f64() * 1000.0;
        assert!((millis - 16.729_17).abs() < 0.001, "{millis}");
    }

    fn only_the_last_thirty_intervals_count() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.0; WINDOW]);
        // Thirty intervals at 8 ms push every 16 ms interval out of the window.
        feed(&mut estimator, now, &[8.0; WINDOW + 10]);
        assert_millis(estimator.published(), 8.0);
    }

    fn background_outlier_is_dropped() {
        // A backgrounded tab resumes with one very long interval and a few
        // slow ones; none of them is a frame time, and the estimate stays.
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 30]);
        let (now, published) = feed(&mut estimator, now, &[4000.0, 16.7, 16.7, 16.7, 16.7]);
        assert_eq!(published.len(), 0, "the outlier republished: {published:?}");
        assert_millis(estimator.published(), 16.7);
        // Throttled frames can even outnumber the real ones in the window: the
        // median is then a throttled interval, which is still not a frame time.
        let (_, published) = feed(&mut estimator, now, &[250.0; 18]);
        assert_eq!(
            published.len(),
            0,
            "throttled frames republished: {published:?}"
        );
        assert_millis(estimator.published(), 16.7);
    }

    fn too_few_usable_intervals_publish_nothing() {
        // Enough intervals, but once the long gaps are left out fewer than the
        // minimum remain.
        let mut estimator = RefreshEstimator::new();
        let mut gaps = vec![16.7; MIN_INTERVALS - 1];
        gaps.extend([2000.0; WINDOW - (MIN_INTERVALS - 1)]);
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
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 30]);
        assert_millis(estimator.published(), 16.7);
        // The tab was hidden for a minute and came back on a 120 Hz display.
        // Sixteen fresh intervals are enough only because the twenty old ones
        // were forgotten; with them still in the window the median stays 16.7.
        estimator.restart();
        let (_, published) = feed(&mut estimator, now + 60_000.0, &[8.33; 16]);
        assert_eq!(published.len(), 1, "one republication: {published:?}");
        assert_millis(estimator.published(), 8.33);
    }

    fn the_old_period_stays_until_a_new_estimate_exists() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 30]);
        estimator.restart();
        let (_, published) = feed(&mut estimator, now + 60_000.0, &[8.33; MIN_INTERVALS - 1]);
        assert!(published.is_empty(), "{published:?}");
        assert_millis(estimator.published(), 16.7);
    }

    fn forgetting_leaves_no_period_until_a_new_estimate() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 30]);
        estimator.forget();
        assert_eq!(estimator.published(), None);
        // The same rate on the new display is published again, not suppressed
        // by the band around the forgotten value.
        let (_, published) = feed(&mut estimator, now + 1000.0, &[16.7; 32]);
        assert_eq!(published.len(), 1);
        assert_millis(estimator.published(), 16.7);
    }

    fn jitter_inside_the_band_publishes_once() {
        let mut estimator = RefreshEstimator::new();
        // 60 Hz with timestamps scattered over 16.1 to 17.1 ms (a fixed
        // pseudo-random sequence) for 400 frames: the median of the window
        // wanders by a few tenths of a millisecond, inside the band.
        let mut state = 0x2545_f491_u32;
        let jittery: Vec<f64> = (0..400)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                16.1 + f64::from(state >> 16) / 65_536.0
            })
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
        // estimate passes through values between them, which the display never
        // ran at and which must not be published: exactly one change, to the
        // settled rate.
        let (_, changed) = feed(&mut estimator, now, &[13.333; 40]);
        assert_eq!(changed.len(), 1, "{changed:?}");
        // Published as soon as the estimate has stayed out of the old band and
        // agreed with the latest intervals: within the new rate's own band.
        let published = changed[0].get().as_secs_f64() * 1000.0;
        assert!((published - 13.333).abs() < 13.333 * 0.03, "{published}");
    }

    fn a_144_to_100_step_publishes_once_near_100() {
        // Real timestamps scatter by a few tenths of a millisecond; a fixed
        // pseudo-random sequence stands in for them.
        let mut state = 0x1234_5678_u32;
        let mut noisy = |base: f64, count: usize| -> Vec<f64> {
            (0..count)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    base + (f64::from(state >> 16) / 65_536.0 - 0.5) * 0.7
                })
                .collect()
        };
        let mut estimator = RefreshEstimator::new();
        let (now, first) = feed(&mut estimator, 0.0, &noisy(1000.0 / 144.0, 40));
        assert_eq!(first.len(), 1);
        // The estimate glides from 6.94 to 10 ms while the window holds both
        // rates; a value on the way (95 Hz was seen live) is not a rate the
        // display ran at, and the step publishes once, at the settled value.
        let (_, changed) = feed(&mut estimator, now, &noisy(10.0, 60));
        assert_eq!(changed.len(), 1, "{changed:?}");
        let published = changed[0].get().as_secs_f64() * 1000.0;
        assert!((published - 10.0).abs() < 0.1, "{published}");
    }

    /// Timestamps of `frames` frames at `period` milliseconds apart, rounded to
    /// whole milliseconds as a browser with a clamped timer reports them.
    fn clamped_timestamps(frames: usize, period: f64) -> Vec<f64> {
        (0..frames)
            .map(|frame| (frame as f64 * period).round())
            .collect()
    }

    fn a_one_millisecond_clock_still_reads_the_real_rate() {
        // 120 Hz reads as 8 and 9 ms intervals (two to one); the median of those
        // is 8, the mean 8.33.
        for (period, expected) in [(1000.0 / 120.0, 8.333), (1000.0 / 60.0, 16.667)] {
            let mut estimator = RefreshEstimator::new();
            let mut changes = 0;
            for stamp in clamped_timestamps(120, period) {
                changes += usize::from(estimator.frame(stamp).is_some());
            }
            let published = estimator.published().expect("a published period");
            assert!(
                (published.get().as_secs_f64() * 1000.0 - expected).abs() < 0.12,
                "{period}: {published:?}"
            );
            assert_eq!(changes, 1, "{period} republished");
        }
    }

    fn timestamps_that_do_not_advance_add_no_interval() {
        let mut estimator = RefreshEstimator::new();
        let (now, _) = feed(&mut estimator, 0.0, &[16.7; 5]);
        // A repeated timestamp and two that are not numbers.
        for timestamp in [now, f64::NAN, f64::INFINITY] {
            assert_eq!(estimator.frame(timestamp), None);
        }
        let (_, published) = feed(&mut estimator, now, &[16.7; 25]);
        assert_eq!(published.len(), 1, "thirty usable intervals: {published:?}");
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
                (
                    "background_outlier_is_dropped",
                    background_outlier_is_dropped,
                ),
                (
                    "too_few_usable_intervals_publish_nothing",
                    too_few_usable_intervals_publish_nothing,
                ),
                (
                    "too_few_frames_publish_nothing",
                    too_few_frames_publish_nothing,
                ),
                (
                    "re_estimates_after_visibility",
                    re_estimates_after_visibility,
                ),
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
                    "a_144_to_100_step_publishes_once_near_100",
                    a_144_to_100_step_publishes_once_near_100,
                ),
                (
                    "a_one_millisecond_clock_still_reads_the_real_rate",
                    a_one_millisecond_clock_still_reads_the_real_rate,
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
