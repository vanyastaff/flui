//! The browser's displays: the screens the page reports and its pixel ratio and
//! animation-frame cadence, with the capabilities they need, detected once
//! ([`source`]), and the listeners that keep them current (`events`).
//!
//! The rules behind them are the pure half, which every host compiles and
//! tests: the proof a capability exists ([`capability`]), the pixel-ratio query
//! ([`dppx_query`]), the refresh estimate from animation-frame timestamps
//! ([`refresh_estimate`]), screen identity ([`screen_identity`]) and the
//! colour the media queries describe ([`color_query`]). `source`
//! and `events` call the browser and exist only on wasm32.

#[cfg(target_arch = "wasm32")]
pub(crate) use source::WebDisplays;

#[cfg(any(target_arch = "wasm32", test))]
#[forbid(unsafe_code)]
pub(crate) mod capability {
    //! Browser capabilities, detected by what the page can call.
    //!
    //! A browser's version says nothing reliable about what it offers: a feature is
    //! behind a flag, withdrawn, or available only in secure contexts. The platform
    //! therefore asks for the capability itself, once, and keeps the answer as a
    //! [`WebFeature`]: a zero-sized proof that the named function exists. Code that
    //! calls the function takes the proof as a parameter, so it cannot be written
    //! without the check having happened, and an absent feature gives no handle at
    //! all rather than a `false` to forget.

    use std::marker::PhantomData;

    /// A browser function a page may or may not have, named by its path from the
    /// global object.
    pub(crate) trait Feature {
        /// The path of the function, dot separated: `matchMedia`,
        /// `navigator.permissions.query`.
        const PATH: &'static str;
        /// What the path must resolve to.
        const KIND: FeatureKind;
    }

    /// What a feature path resolves to.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum FeatureKind {
        /// A callable function.
        Function,
        /// A property that exists, even when its value is `null` (an event handler
        /// slot such as `onchange` is `null` until set).
        Property,
    }

    /// `window.matchMedia`: media queries and their change events.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct MatchMedia;

    impl Feature for MatchMedia {
        const PATH: &'static str = "matchMedia";
        const KIND: FeatureKind = FeatureKind::Function;
    }

    /// `window.getScreenDetails`: every screen of the machine (Window Management).
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct GetScreenDetails;

    impl Feature for GetScreenDetails {
        const PATH: &'static str = "getScreenDetails";
        const KIND: FeatureKind = FeatureKind::Function;
    }

    /// `navigator.permissions.query`: read a permission without asking for it.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct PermissionsQuery;

    impl Feature for PermissionsQuery {
        const PATH: &'static str = "navigator.permissions.query";
        const KIND: FeatureKind = FeatureKind::Function;
    }

    /// `window.screen.onchange`: the screen object raises `change` when its size,
    /// pixel ratio or position changes (Chromium; Window Management not needed).
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct ScreenChange;

    impl Feature for ScreenChange {
        const PATH: &'static str = "screen.onchange";
        const KIND: FeatureKind = FeatureKind::Property;
    }

    /// Where a capability is looked up: the browser's global object, or a fake in a
    /// test.
    pub(crate) trait FeatureProbe {
        /// Whether `path` resolves to something of `kind`.
        fn has(&self, path: &str, kind: FeatureKind) -> bool;
    }

    /// Proof that the browser has feature `F`.
    ///
    /// Obtainable only from [`detect`](Self::detect), which found the function.
    #[derive(Debug)]
    pub(crate) struct WebFeature<F> {
        feature: PhantomData<fn() -> F>,
    }

    impl<F> Clone for WebFeature<F> {
        fn clone(&self) -> Self {
            *self
        }
    }

    impl<F> Copy for WebFeature<F> {}

    impl<F: Feature> WebFeature<F> {
        /// The proof, or `None` when the browser lacks the feature.
        pub(crate) fn detect(probe: &impl FeatureProbe) -> Option<Self> {
            probe.has(F::PATH, F::KIND).then_some(Self {
                feature: PhantomData,
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            Feature, FeatureKind, FeatureProbe, GetScreenDetails, MatchMedia, PermissionsQuery,
            ScreenChange, WebFeature,
        };

        /// A browser that offers exactly these functions, and has these properties
        /// (present even when their value is `null`).
        struct Browser(&'static [&'static str], &'static [&'static str]);

        impl FeatureProbe for Browser {
            fn has(&self, path: &str, kind: FeatureKind) -> bool {
                match kind {
                    FeatureKind::Function => self.0.contains(&path),
                    FeatureKind::Property => self.1.contains(&path),
                }
            }
        }

        #[test]
        fn web_feature_absent_gives_no_handle() {
            let desktop_chrome = Browser(
                &[
                    MatchMedia::PATH,
                    GetScreenDetails::PATH,
                    PermissionsQuery::PATH,
                ],
                &[],
            );
            assert!(WebFeature::<GetScreenDetails>::detect(&desktop_chrome).is_some());
            assert!(WebFeature::<PermissionsQuery>::detect(&desktop_chrome).is_some());

            // Firefox and Safari have no Window Management API; the detection does
            // not depend on any version string, only on the function.
            let without_window_management =
                Browser(&[MatchMedia::PATH, PermissionsQuery::PATH], &[]);
            assert!(WebFeature::<GetScreenDetails>::detect(&without_window_management).is_none());
            assert!(WebFeature::<MatchMedia>::detect(&without_window_management).is_some());

            let bare = Browser(&[], &[]);
            assert!(WebFeature::<MatchMedia>::detect(&bare).is_none());
            assert!(WebFeature::<PermissionsQuery>::detect(&bare).is_none());
        }

        #[test]
        fn a_feature_is_found_by_its_whole_path() {
            // `navigator.permissions` without `query` is not the capability.
            let partial = Browser(&["navigator.permissions"], &[]);
            assert!(WebFeature::<PermissionsQuery>::detect(&partial).is_none());
        }

        #[test]
        fn a_null_handler_slot_is_a_present_property() {
            // `screen.onchange` is `null` where `change` is supported: the property
            // exists, no function does. A function feature is not satisfied by a
            // property of the same name.
            let chromium = Browser(&[], &[ScreenChange::PATH]);
            assert!(WebFeature::<ScreenChange>::detect(&chromium).is_some());
            let without = Browser(&[ScreenChange::PATH], &[]);
            assert!(WebFeature::<ScreenChange>::detect(&without).is_none());
            let property_only = Browser(&[], &[MatchMedia::PATH]);
            assert!(WebFeature::<MatchMedia>::detect(&property_only).is_none());
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[forbid(unsafe_code)]
pub(crate) mod dppx_query {
    //! The media query a page arms to learn that its `devicePixelRatio` changed.
    //!
    //! The browser has no event for a changed pixel ratio, only for a media query
    //! whose match flips. The query `(resolution: Xdppx)` matches while the ratio
    //! is exactly `X`, so it fires `change` the moment the ratio becomes anything
    //! else: page zoom, a window dragged to a display with another scale, an OS
    //! scale change. A fired query is spent (it describes the old ratio), so the
    //! next one is armed for the new ratio before anything else happens. This
    //! module is that rule; the platform owns the `MediaQueryList` objects.

    /// The largest ratio a query is armed for. Real displays and page zoom stay far
    /// below it; a larger value is not a pixel ratio.
    const MAX_DPPX: f64 = 64.0;

    /// What re-arming for the ratio now in effect asks of the platform.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct Rearm {
        /// The query to listen to from now on; `None` when the ratio is not one a
        /// query can describe (not finite, not positive, or implausibly large), in
        /// which case nothing is armed until a later change.
        pub(crate) query: Option<String>,
        /// Whether the ratio differs from the one the fired query described. A
        /// spurious fire re-arms without reporting a change.
        pub(crate) changed: bool,
    }

    /// The ratio the currently armed query describes.
    #[derive(Debug, Default)]
    pub(crate) struct DppxWatch {
        armed: Option<f64>,
    }

    impl DppxWatch {
        /// A watch that has armed nothing.
        pub(crate) fn new() -> Self {
            Self::default()
        }

        /// Arm for `current`, the ratio read just now. Used first at start-up and
        /// again each time the armed query fires.
        pub(crate) fn arm(&mut self, current: f64) -> Rearm {
            let described =
                (current.is_finite() && current > 0.0 && current <= MAX_DPPX).then_some(current);
            let changed = self.armed != described;
            self.armed = described;
            Rearm {
                query: described.map(|dppx| format!("(resolution: {dppx}dppx)")),
                changed,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{DppxWatch, Rearm};

        #[test]
        fn dpr_change_arms_the_query_for_the_new_ratio() {
            let mut watch = DppxWatch::new();
            assert_eq!(
                watch.arm(2.0),
                Rearm {
                    query: Some("(resolution: 2dppx)".into()),
                    changed: true
                }
            );
            // Zoom to 125% on that display: 2 * 1.25.
            assert_eq!(
                watch.arm(2.5),
                Rearm {
                    query: Some("(resolution: 2.5dppx)".into()),
                    changed: true
                }
            );
            // Fractional ratios keep their full precision: a query rounded to fewer
            // digits would never match and would fire at once.
            assert_eq!(
                watch.arm(1.100_000_023_841_858).query.as_deref(),
                Some("(resolution: 1.100000023841858dppx)")
            );
        }

        #[test]
        fn an_unchanged_ratio_rearms_without_reporting_a_change() {
            let mut watch = DppxWatch::new();
            watch.arm(1.5);
            let again = watch.arm(1.5);
            assert!(!again.changed);
            assert_eq!(again.query.as_deref(), Some("(resolution: 1.5dppx)"));
        }

        #[test]
        fn a_ratio_no_query_can_describe_arms_nothing() {
            for ratio in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
                let mut watch = DppxWatch::new();
                watch.arm(2.0);
                let rearm = watch.arm(ratio);
                assert_eq!(rearm.query, None, "{ratio}");
                assert!(rearm.changed, "{ratio} replaces the 2 dppx the watch held");
            }
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[forbid(unsafe_code)]
pub(crate) mod refresh_estimate {
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
    //! the rules run on every host. The estimate becomes a period by the
    //! contract's `RefreshPeriod::from_secs_f64`, and is published by the display
    //! model's one publisher, whose band rule also judges agreement and stability.

    use flui_platform::RefreshPeriod;

    use crate::display::source::{MeasuredPeriod, outside_band};

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
        published: MeasuredPeriod,
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
            self.published.forget();
        }

        /// The period published so far; `None` until enough frames have run.
        pub(crate) fn published(&self) -> Option<RefreshPeriod> {
            self.published.published()
        }

        /// The trimmed mean of the recent intervals without background
        /// outliers; `None` while there are too few of them.
        fn candidate(&self) -> Option<RefreshPeriod> {
            let count = self.intervals.len;
            // The first estimate waits for a full window, so what is published
            // first is the best the window can say; a replacement needs fewer
            // because it is judged against what is already published.
            let needed = if self.published().is_none() {
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
            RefreshPeriod::from_secs_f64(millis / 1000.0).ok()
        }

        /// Publish `candidate` if it has left the hysteresis band for
        /// [`PERSISTENCE`] consecutive frames while the latest intervals agree with
        /// it; `Some` when the published value changed. The first estimate is
        /// published at once.
        fn publish(&mut self, candidate: RefreshPeriod) -> Option<RefreshPeriod> {
            let candidate_ms = candidate.get().as_secs_f64() * 1000.0;
            if self.published().is_some() {
                if !self.published.departs(candidate, HYSTERESIS) {
                    self.pending = None;
                    return None;
                }
                let agrees = self
                    .intervals
                    .recent_mean(RECENT)
                    .is_some_and(|recent| !outside_band(recent, candidate_ms, 2.0 * HYSTERESIS));
                if !agrees {
                    self.pending = None;
                    return None;
                }
                // Stable: the candidate is no longer moving. While the window holds
                // two rates it glides from one to the other, a step at a time.
                let pending = match self.pending {
                    Some(pending) if !outside_band(candidate_ms, pending.anchor_ms, STABILITY) => {
                        Pending {
                            anchor_ms: pending.anchor_ms,
                            frames: pending.frames + 1,
                        }
                    }
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
            self.published.offer(candidate, HYSTERESIS)
        }
    }

    #[cfg(test)]
    mod tests {
        use std::time::Duration;

        use flui_platform::RefreshPeriod;

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
}

#[cfg(any(target_arch = "wasm32", test))]
#[forbid(unsafe_code)]
pub(crate) mod screen_identity {
    //! Display identity for browser screens that carry no stable key.
    //!
    //! The Window Management API lists the screens as JS objects. The browser
    //! hands back the same object for the same screen across `screenschange`, and
    //! a label is no identity (two identical monitors share one). A screen is
    //! therefore known by the object itself, compared by identity, in a list the
    //! platform retains: a screen found in the list keeps its [`DisplayId`], and
    //! an object that matches nothing is a new display with a fresh id. An id
    //! leaves the list with its screen and is never issued again, so a monitor that
    //! is unplugged and plugged back in is a new display.

    use flui_platform::DisplayId;

    /// The screens of the last listing, each with the id it was given.
    #[derive(Debug)]
    pub(crate) struct RetainedScreens<T> {
        retained: Vec<(T, DisplayId)>,
    }

    impl<T> RetainedScreens<T> {
        /// An empty list.
        pub(crate) fn new() -> Self {
            Self {
                retained: Vec::new(),
            }
        }

        /// Whether no screen is retained.
        pub(crate) fn is_empty(&self) -> bool {
            self.retained.is_empty()
        }

        /// Retain `screen` under an id issued before it was listed: the display a
        /// page saw as its only one is this screen.
        pub(crate) fn adopt(&mut self, screen: T, id: DisplayId) {
            self.retained.push((screen, id));
        }

        /// Replace the list with `current` and answer the id of each entry, in
        /// order: the retained id for an entry `same` identifies with a retained
        /// one, otherwise the id `mint` issues. `None` where `mint` has none left;
        /// such a screen is not retained and has no display.
        pub(crate) fn reconcile(
            &mut self,
            current: Vec<T>,
            same: impl Fn(&T, &T) -> bool,
            mut mint: impl FnMut() -> Option<DisplayId>,
        ) -> Vec<Option<DisplayId>> {
            let mut previous = std::mem::take(&mut self.retained);
            let mut ids = Vec::with_capacity(current.len());
            for screen in current {
                let known = previous
                    .iter()
                    .position(|(retained, _)| same(retained, &screen))
                    .map(|index| previous.swap_remove(index).1);
                let id = known.or_else(&mut mint);
                if let Some(id) = id {
                    self.retained.push((screen, id));
                }
                ids.push(id);
            }
            ids
        }

        /// The id of the retained screen `same` identifies with `screen`.
        pub(crate) fn id_of(&self, screen: &T, same: impl Fn(&T, &T) -> bool) -> Option<DisplayId> {
            self.retained
                .iter()
                .find(|(retained, _)| same(retained, screen))
                .map(|(_, id)| *id)
        }
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::{DisplayId, DisplayKey, DisplayRegistry};

        use super::RetainedScreens;

        /// A screen object: the object is its identity, the label is not.
        #[derive(Debug, Clone, Copy)]
        struct Screen {
            object: u32,
            label: &'static str,
        }

        fn same(a: &Screen, b: &Screen) -> bool {
            a.object == b.object
        }

        fn mint_from(registry: &DisplayRegistry) -> impl FnMut() -> Option<DisplayId> + '_ {
            || registry.mint(&DisplayKey::Unkeyed).ok()
        }

        const LEFT: Screen = Screen {
            object: 1,
            label: "DELL U2720Q",
        };
        const RIGHT: Screen = Screen {
            object: 2,
            label: "DELL U2720Q",
        };

        #[test]
        fn retained_screen_keeps_its_id() {
            let registry = DisplayRegistry::new();
            let mut screens = RetainedScreens::new();
            let first = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
            // The listing comes back in another order, as `screenschange` may.
            let second = screens.reconcile(vec![RIGHT, LEFT], same, mint_from(&registry));
            assert_eq!(second, vec![first[1], first[0]]);
        }

        #[test]
        fn a_label_shared_by_two_screens_is_not_one_display() {
            let registry = DisplayRegistry::new();
            let mut screens = RetainedScreens::new();
            let ids = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
            assert_ne!(ids[0], ids[1]);
        }

        #[test]
        fn new_screen_object_gets_a_fresh_id() {
            let registry = DisplayRegistry::new();
            let mut screens = RetainedScreens::new();
            let first = screens.reconcile(vec![LEFT], same, mint_from(&registry));
            // The same label on a different object is a different screen.
            let second = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
            assert_eq!(second[0], first[0]);
            assert_ne!(second[1], first[0]);
        }

        #[test]
        fn removed_screen_id_is_not_reissued() {
            let registry = DisplayRegistry::new();
            let mut screens = RetainedScreens::new();
            let first = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
            screens.reconcile(vec![LEFT], same, mint_from(&registry));
            // The right monitor is plugged back in: a new object, a new display.
            let back = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
            assert_eq!(back[0], first[0]);
            assert_ne!(back[1], first[1]);
        }

        #[test]
        fn a_screen_with_no_id_left_is_not_retained() {
            let registry = DisplayRegistry::new();
            let mut screens = RetainedScreens::new();
            let ids = screens.reconcile(vec![LEFT, RIGHT], same, || None);
            assert_eq!(ids, vec![None, None]);
            assert_eq!(screens.id_of(&LEFT, same), None);
            // Ids that become available later are issued then.
            let later = screens.reconcile(vec![LEFT], same, mint_from(&registry));
            assert!(later[0].is_some());
        }

        #[test]
        fn an_adopted_screen_keeps_the_id_it_was_given() {
            // The single screen a page saw before screen access is the same
            // physical screen as the one the listing calls current.
            let registry = DisplayRegistry::new();
            let before = registry
                .mint(&DisplayKey::Web("primary".into()))
                .expect("id");
            let mut screens = RetainedScreens::new();
            assert!(screens.is_empty());
            screens.adopt(RIGHT, before);
            assert!(!screens.is_empty());
            let ids = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
            assert_eq!(ids[1], Some(before), "the adopted screen keeps its id");
            assert_ne!(ids[0], Some(before), "another screen gets a fresh one");
        }

        #[test]
        fn id_of_finds_the_current_screen_by_identity() {
            let registry = DisplayRegistry::new();
            let mut screens = RetainedScreens::new();
            let ids = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
            assert_eq!(screens.id_of(&RIGHT, same), ids[1]);
            let stranger = Screen {
                object: 3,
                label: "DELL U2720Q",
            };
            assert_eq!(screens.id_of(&stranger, same), None);
            assert_eq!(LEFT.label, stranger.label);
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[forbid(unsafe_code)]
pub(crate) mod color_query {
    //! The colour of the screen a page is on, from the media queries a browser
    //! answers: `dynamic-range` and `color-gamut`.
    //!
    //! `(dynamic-range: high)` says the screen can show high range, not that it
    //! does now, and a browser exposes no headroom, so a high-range screen's
    //! headroom is absent on this platform. A page learns a change of either
    //! answer from its query's `change` event.

    use flui_platform::{CapabilityAbsence, ColorGamut, DisplayColor};

    /// Whether the screen can show high range.
    const HIGH_RANGE: &str = "(dynamic-range: high)";
    /// Whether the screen covers the BT.2020 gamut.
    const REC2020: &str = "(color-gamut: rec2020)";
    /// Whether the screen covers the Display P3 gamut.
    const P3: &str = "(color-gamut: p3)";

    /// Every query the colour is read from, which a page listens to for its
    /// change.
    #[cfg(target_arch = "wasm32")]
    pub(crate) const QUERIES: [&str; 3] = [HIGH_RANGE, REC2020, P3];

    /// The display colour the queries `matches` answers describe: the widest
    /// gamut that matches, high range with no headroom to read when
    /// `dynamic-range` matches.
    pub(crate) fn display_color(matches: impl Fn(&str) -> bool) -> DisplayColor {
        let gamut = if matches(REC2020) {
            ColorGamut::Rec2020
        } else if matches(P3) {
            ColorGamut::DisplayP3
        } else {
            ColorGamut::Srgb
        };
        if matches(HIGH_RANGE) {
            DisplayColor::high(gamut, Err(CapabilityAbsence::NotOnThisPlatform))
        } else {
            DisplayColor::standard(gamut)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Each answer the colour queries give, to the display colour it means.
        #[test]
        fn browser_colour_queries_map_to_the_display_colour() {
            let matching = |high_range: bool, rec2020: bool, p3: bool| {
                display_color(|query| match query {
                    HIGH_RANGE => high_range,
                    REC2020 => rec2020,
                    P3 => p3,
                    _ => panic!("{query} is no colour query"),
                })
            };
            assert_eq!(
                matching(false, false, false),
                DisplayColor::standard(ColorGamut::Srgb)
            );
            assert_eq!(
                matching(false, false, true),
                DisplayColor::standard(ColorGamut::DisplayP3)
            );
            assert_eq!(
                matching(true, true, true),
                DisplayColor::high(
                    ColorGamut::Rec2020,
                    Err(CapabilityAbsence::NotOnThisPlatform)
                ),
                "the widest gamut wins, and the headroom is never readable"
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) mod source {
    //! The Web display source.
    //!
    //! # What a page can know
    //!
    //! A browser tab sees one screen, the one `window.screen` describes, and learns
    //! nothing about its refresh rate. Both are filled in here:
    //!
    //! - **The refresh period is an estimate**, taken from the timestamps of the
    //!   page's own `requestAnimationFrame` callbacks (see
    //!   [`super::refresh_estimate`] for the rule and its limits). It is
    //!   unknown (`None`) until enough frames have run, follows the cadence the
    //!   browser presents at rather than the panel's capability, and is estimated
    //!   again after the tab becomes visible and after a pixel-ratio change.
    //! - **Several screens** are listed only when the Window Management API is
    //!   available and its permission is already granted: the platform never
    //!   requests the permission, because a prompt is the application's explicit
    //!   act. Without it the snapshot holds the one screen. Each listed screen is
    //!   known by its JS object, compared by identity in a retained list
    //!   ([`super::screen_identity`]); labels repeat and are not keys.
    //!   The estimate belongs to the screen the window is on, the others stay
    //!   unknown.
    //!
    //! # Sampling
    //!
    //! `Platform::display_snapshot` runs on every owner wake, so the snapshot is
    //! cached and read from the browser again only after an event that can change
    //! it: a pixel-ratio change, a new published period, a `change` of `window.screen`
    //! (size, pixel ratio or position; Chromium raises it without any permission),
    //! a change of the screen list, of a listed screen, or of the screen the window
    //! is on. A browser whose `screen` has no `onchange` raises none of the screen
    //! events, so there a move between two screens of equal pixel ratio but
    //! different size is not seen until the next other event.
    //!
    //! # Scale, bounds and page zoom
    //!
    //! `scale_factor` of the screen the window is on is `window.devicePixelRatio`
    //! in both modes, which includes page zoom; the other listed screens report
    //! their own `ScreenDetailed.devicePixelRatio`. Bounds describe the physical
    //! screen and must not move with zoom, so they use a ratio without it:
    //!
    //! - **With screen access** the screen's own `ScreenDetailed.devicePixelRatio`,
    //!   which is the display's scale alone.
    //! - **Without it** the page's pixel ratio when `window.screen` was first
    //!   seen, taken as the display's own scale with the page at 100%. Chrome
    //!   reports `screen.width` in device-independent pixels that do not change
    //!   with zoom, and `devicePixelRatio` is the OS scale times the zoom, so the
    //!   zoom cannot be read back out of the two later; a page that is zoomed when
    //!   it starts reports bounds inflated by that zoom until the screen changes.
    //!   A `change` of `window.screen` (another display) takes the ratio again.
    //!   Firefox reports `screen.width` in zoomed CSS pixels, where this and the
    //!   product with the current ratio agree only at 100%.
    //!
    //! # Identity across screen access
    //!
    //! When a listing first appears, its `currentScreen` takes the id of the
    //! single-screen display the page had, so the window sees that display gain a
    //! name rather than change; the other screens get fresh ids.

    use std::sync::Arc;

    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::{
        CapabilityAbsence, DisplayColor, DisplayId, DisplayInfo, DisplayKey, DisplaySnapshot,
        RefreshPeriod,
    };
    use parking_lot::Mutex;
    use wasm_bindgen::{JsCast, JsValue};

    use super::capability::{
        FeatureKind, FeatureProbe, GetScreenDetails, MatchMedia, PermissionsQuery, ScreenChange,
        WebFeature,
    };
    use super::color_query::display_color;
    use super::refresh_estimate::RefreshEstimator;
    use super::screen_identity::RetainedScreens;
    use crate::display::source::DisplaySource;
    use crate::error::PlatformError;

    /// The Web platform's display state: identities, the estimator and the cached
    /// snapshot. Shared by the platform, its window and the browser listeners.
    pub struct WebDisplays {
        source: DisplaySource,
        pub(super) features: BrowserFeatures,
        state: Mutex<State>,
    }

    impl std::fmt::Debug for WebDisplays {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebDisplays").finish_non_exhaustive()
        }
    }

    struct State {
        /// The display the window is on, as of the last sample.
        current: Option<DisplayId>,
        estimator: RefreshEstimator,
        /// The Window Management `ScreenDetails` object, while access is granted.
        details: Option<JsValue>,
        screens: RetainedScreens<JsValue>,
        /// The ratio that turns `window.screen`'s size into device pixels while no
        /// screen list is available: the page's pixel ratio when the screen was
        /// first seen, taken as the display's own scale with the page at 100%.
        /// `None` until the first sample and after the screen changed.
        screen_ratio: Option<f64>,
    }

    impl crate::owner::web::HostDisplays for WebDisplays {
        /// The cached snapshot, sampled from the browser when an event invalidated
        /// it.
        fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
            self.source.snapshot_with(|| {
                let mut state = self.state.lock();
                self.sample(&mut state)
            })
        }

        /// The id is a product of sampling: it follows the cached snapshot.
        fn current_display(&self) -> Option<DisplayId> {
            crate::owner::web::HostDisplays::snapshot(self).ok()?;
            self.state.lock().current
        }

        /// A new published period marks the snapshot stale.
        fn frame(&self, timestamp_ms: f64) -> bool {
            let mut state = self.state.lock();
            if state.estimator.frame(timestamp_ms).is_none() {
                return false;
            }
            self.source.invalidate();
            true
        }

        fn observe(
            self: Arc<Self>,
            signal: &Arc<crate::owner::signal::OwnerSignal>,
        ) -> crate::owner::watch::Watch {
            crate::owner::watch::Watch::new(super::events::DisplayObservers::start(&self, signal))
        }

        fn watch_pixel_ratio(
            &self,
            active: &crate::owner::web::Accepting,
            on_change: Box<dyn Fn()>,
        ) -> Option<Box<dyn crate::owner::web::PixelRatioWatch>> {
            let watch = super::events::PixelRatioWatch::for_layout(active, on_change)?;
            Some(Box::new(watch))
        }
    }

    impl WebDisplays {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self {
                source: DisplaySource::new(),
                features: BrowserFeatures::detect(),
                state: Mutex::new(State {
                    current: None,
                    estimator: RefreshEstimator::new(),
                    details: None,
                    screens: RetainedScreens::new(),
                    screen_ratio: None,
                }),
            })
        }

        /// The cadence may have changed (the tab became visible, the page changed
        /// scale or screen): measure again.
        pub(super) fn restart_estimate(&self) {
            self.state.lock().estimator.restart();
        }

        /// The window is on another screen: the published period described the
        /// previous one, so none is reported until the new screen is measured.
        pub(super) fn forget_estimate(&self) {
            let mut state = self.state.lock();
            state.estimator.forget();
            self.source.invalidate();
        }

        /// `window.screen` changed (another size, another display): sample again,
        /// and take the page's pixel ratio then as the display's own scale.
        pub(super) fn screen_changed(&self) {
            let mut state = self.state.lock();
            state.screen_ratio = None;
            // Without screen access this event is the only sign of a move to
            // another monitor, and it cannot be told from a change of the same
            // screen's metrics: the old period may describe another display. With
            // a `ScreenDetails` the current-screen identity decides that instead.
            if state.details.is_none() {
                state.estimator.forget();
            }
            self.source.invalidate();
        }

        /// Something the snapshot reports changed: sample the browser again on the
        /// next read.
        pub(super) fn invalidate(&self) {
            self.source.invalidate();
        }

        /// Replace the `ScreenDetails` object, or drop it when access ended.
        /// Returns the previous one so the caller retires it outside the lock.
        pub(super) fn set_details(&self, details: Option<JsValue>) -> Option<JsValue> {
            let mut state = self.state.lock();
            if details.is_none() && state.details.is_none() {
                // Nothing was granted and nothing is: not an event.
                return None;
            }
            self.source.invalidate();
            state.estimator.restart();
            std::mem::replace(&mut state.details, details)
        }

        fn sample(&self, state: &mut State) -> Result<Vec<DisplayInfo>, PlatformError> {
            let window = web_sys::window().ok_or_else(|| PlatformError::Displays {
                message: "no global window".into(),
            })?;
            let period = state.estimator.published();
            let listed = match state.details.clone() {
                Some(details) => self.listed_screens(state, &window, &details, period),
                None => None,
            };
            let (displays, current) = match listed {
                Some(listed) => listed,
                None => self.single_screen(state, &window, period)?,
            };
            state.current = current;
            Ok(displays)
        }

        /// The colour of the screen the page is on, by its media queries; absent
        /// on a browser without `matchMedia`.
        fn page_color(&self, window: &web_sys::Window) -> Result<DisplayColor, CapabilityAbsence> {
            let media = self
                .features
                .match_media
                .ok_or(CapabilityAbsence::NotOnThisDevice)?;
            Ok(display_color(|query| {
                media
                    .query(window, query)
                    .ok()
                    .flatten()
                    .is_some_and(|list| list.matches())
            }))
        }

        /// The one screen `window.screen` describes.
        fn single_screen(
            &self,
            state: &mut State,
            window: &web_sys::Window,
            period: Option<RefreshPeriod>,
        ) -> Result<(Vec<DisplayInfo>, Option<DisplayId>), PlatformError> {
            let screen = window.screen().map_err(|_| PlatformError::Displays {
                message: "the window has no screen".into(),
            })?;
            let scale_factor = positive(window.device_pixel_ratio()).unwrap_or(1.0);
            let device_scale = *state.screen_ratio.get_or_insert(scale_factor);
            let bounds = Bounds::new(
                Point::new(0, 0),
                Size::new(
                    device_pixels(f64::from(screen.width().unwrap_or(1920)), device_scale),
                    device_pixels(f64::from(screen.height().unwrap_or(1080)), device_scale),
                ),
            );
            let id = self.source.mint(&DisplayKey::Web("primary".into()));
            let displays = id
                .map(|id| {
                    vec![with_period(
                        DisplayInfo::new(id, "Browser Screen", bounds, scale_factor)
                            .with_primary(true)
                            .with_color(self.page_color(window)),
                        period,
                    )]
                })
                .unwrap_or_default();
            Ok((displays, id))
        }

        /// The screens `ScreenDetails` lists, with the display the window is on;
        /// `None` when the object does not list any screen, which reads as the
        /// single-screen case.
        fn listed_screens(
            &self,
            state: &mut State,
            window: &web_sys::Window,
            details: &JsValue,
            period: Option<RefreshPeriod>,
        ) -> Option<(Vec<DisplayInfo>, Option<DisplayId>)> {
            let screens: Vec<JsValue> = js_sys::Array::from(&get(details, "screens")?).to_vec();
            if screens.is_empty() {
                return None;
            }
            // The screen the page saw as its only one is the listing's current
            // screen: the first time a listing appears it takes that display's id,
            // so the window sees the display gain a name, not change.
            if state.screens.is_empty()
                && let Some(current) = get(details, "currentScreen")
                && let Some(id) = self.source.mint(&DisplayKey::Web("primary".into()))
            {
                state.screens.adopt(current, id);
            }
            let ids = state
                .screens
                .reconcile(screens.clone(), js_sys::Object::is, || {
                    self.source.mint(&DisplayKey::Unkeyed)
                });
            let listed: Vec<(JsValue, DisplayId)> = screens
                .into_iter()
                .zip(ids)
                .filter_map(|(screen, id)| Some((screen, id?)))
                .collect();
            let current = get(details, "currentScreen")
                .and_then(|screen| state.screens.id_of(&screen, js_sys::Object::is))
                // A listing whose current screen is missing is still a listing: the
                // window is then on the primary screen.
                .or_else(|| {
                    listed
                        .iter()
                        .find(|(screen, _)| is_primary(screen))
                        .map(|(_, id)| *id)
                });
            let window_scale = positive(window.device_pixel_ratio());
            let displays = listed
                .iter()
                .map(|(screen, id)| {
                    if Some(*id) == current {
                        // The screen the window is on reports the window's own pixel
                        // ratio, which includes page zoom, so the scale behaves the
                        // same with and without screen access. Its estimate belongs
                        // to it alone.
                        with_period(screen_info(screen, *id, window_scale), period)
                            .with_color(self.page_color(window))
                    } else {
                        // A page's media queries describe the screen it is on;
                        // the browser says nothing of the others' colour.
                        screen_info(screen, *id, None)
                            .with_color(Err(CapabilityAbsence::NotOnThisPlatform))
                    }
                })
                .collect();
            Some((displays, current))
        }
    }

    fn is_primary(screen: &JsValue) -> bool {
        get(screen, "isPrimary").is_some_and(|primary| primary.is_truthy())
    }

    /// One `ScreenDetailed` as a display. Its metrics are CSS pixels of the
    /// screen, converted to device pixels with the screen's own
    /// `devicePixelRatio`, which is the display's scale and does not include page
    /// zoom: bounds describe the physical screen and stay put while the page is
    /// zoomed. The reported scale is `scale_override` (the window's pixel ratio, zoom
    /// included, for the screen the window is on) or else that same ratio. Origins
    /// are converted the same way: the coordinate space of a mixed-ratio layout is
    /// the browser's, and the origin is therefore the browser's value in that
    /// screen's device pixels, not a global device layout.
    fn screen_info(screen: &JsValue, id: DisplayId, scale_override: Option<f64>) -> DisplayInfo {
        let own_ratio = get_number(screen, "devicePixelRatio")
            .and_then(positive)
            .unwrap_or(1.0);
        let scale_factor = scale_override.unwrap_or(own_ratio);
        let number = |name: &str, fallback: f64| get_number(screen, name).unwrap_or(fallback);
        let device = |css: f64| device_pixels(css, own_ratio);
        let bounds = Bounds::new(
            Point::new(device(number("left", 0.0)), device(number("top", 0.0))),
            Size::new(
                device(number("width", 1920.0)),
                device(number("height", 1080.0)),
            ),
        );
        let usable = Bounds::new(
            Point::new(
                device(number("availLeft", number("left", 0.0))),
                device(number("availTop", number("top", 0.0))),
            ),
            Size::new(
                device(number("availWidth", number("width", 1920.0))),
                device(number("availHeight", number("height", 1080.0))),
            ),
        );
        let label = get(screen, "label")
            .and_then(|label| label.as_string())
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| "Browser Screen".to_string());
        let is_primary = is_primary(screen);
        DisplayInfo::new(id, label, bounds, scale_factor)
            .with_usable_bounds(usable)
            .with_primary(is_primary)
    }

    fn with_period(info: DisplayInfo, period: Option<RefreshPeriod>) -> DisplayInfo {
        match period {
            Some(period) => info.with_refresh_period(period.get()),
            None => info,
        }
    }

    /// A property that is present, neither `undefined` nor `null`.
    pub(super) fn get(target: &JsValue, name: &str) -> Option<JsValue> {
        js_sys::Reflect::get(target, &JsValue::from_str(name))
            .ok()
            .filter(|value| !value.is_undefined() && !value.is_null())
    }

    fn get_number(target: &JsValue, name: &str) -> Option<f64> {
        get(target, name)?
            .as_f64()
            .filter(|value| value.is_finite())
    }

    fn positive(value: f64) -> Option<f64> {
        (value.is_finite() && value > 0.0).then_some(value)
    }

    /// CSS pixels as the device pixels a snapshot carries.
    fn device_pixels(css: f64, scale_factor: f64) -> i32 {
        // `as` saturates at the `i32` range, which is the clamp wanted here.
        (css * scale_factor).round() as i32
    }

    /// Whether `value` is a JS object that can receive events.
    pub(super) fn event_target(value: &JsValue) -> Option<web_sys::EventTarget> {
        value.dyn_ref::<web_sys::EventTarget>().cloned()
    }

    /// Looks a feature up by its dotted path from the global object.
    struct GlobalProbe;

    impl FeatureProbe for GlobalProbe {
        fn has(&self, path: &str, kind: FeatureKind) -> bool {
            let mut segments = path.split('.').peekable();
            let mut value: JsValue = js_sys::global().into();
            while let Some(segment) = segments.next() {
                let name = JsValue::from_str(segment);
                if segments.peek().is_none() && kind == FeatureKind::Property {
                    // A slot that holds `null` is still there.
                    return js_sys::Reflect::has(&value, &name).unwrap_or(false);
                }
                match js_sys::Reflect::get(&value, &name) {
                    Ok(next) if !next.is_undefined() && !next.is_null() => value = next,
                    _ => return false,
                }
            }
            value.is_instance_of::<js_sys::Function>()
        }
    }

    /// What this browser offers the display source, detected once when the
    /// platform is built.
    ///
    /// Detection asks for the function itself (see [`super::capability`]); no
    /// browser version is read. The answers are kept as proofs, and the calls into
    /// the browser that need a capability are methods on its proof: `matchMedia`,
    /// the Permissions API, `getScreenDetails` and `screen.onchange` cannot be
    /// reached without the check having happened.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct BrowserFeatures {
        /// Media queries and their change events: the pixel-ratio watch.
        pub(super) match_media: Option<WebFeature<MatchMedia>>,
        /// Read a permission without prompting for it.
        pub(super) permissions: Option<WebFeature<PermissionsQuery>>,
        /// Every screen of the machine (Window Management).
        pub(super) screen_details: Option<WebFeature<GetScreenDetails>>,
        /// The screen object's own `change` event.
        pub(super) screen_change: Option<WebFeature<ScreenChange>>,
    }

    impl BrowserFeatures {
        /// Probe the running browser.
        pub(super) fn detect() -> Self {
            Self {
                match_media: WebFeature::detect(&GlobalProbe),
                permissions: WebFeature::detect(&GlobalProbe),
                screen_details: WebFeature::detect(&GlobalProbe),
                screen_change: WebFeature::detect(&GlobalProbe),
            }
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "the proof is the capability: the call cannot be written without it"
    )]
    impl WebFeature<MatchMedia> {
        /// `window.matchMedia(query)`.
        pub(super) fn query(
            &self,
            window: &web_sys::Window,
            query: &str,
        ) -> Result<Option<web_sys::MediaQueryList>, JsValue> {
            window.match_media(query)
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "the proof is the capability: the call cannot be written without it"
    )]
    impl WebFeature<PermissionsQuery> {
        /// `navigator.permissions.query({ name })`: the promise of a
        /// `PermissionStatus`. Reading a permission never prompts. `None` when the
        /// call is refused (an unknown permission name throws in some browsers).
        pub(super) fn query(
            &self,
            window: &web_sys::Window,
            name: &str,
        ) -> Option<js_sys::Promise> {
            let navigator = window.navigator();
            let permissions = get(navigator.as_ref(), "permissions")?;
            let query: js_sys::Function = get(&permissions, "query")?.dyn_into().ok()?;
            let descriptor = js_sys::Object::new();
            js_sys::Reflect::set(&descriptor, &"name".into(), &name.into()).ok()?;
            query.call1(&permissions, &descriptor).ok()?.dyn_into().ok()
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "the proof is the capability: the call cannot be written without it"
    )]
    impl WebFeature<GetScreenDetails> {
        /// `window.getScreenDetails()`: the promise of a `ScreenDetails`. It
        /// prompts when the permission is neither granted nor denied, so callers
        /// ask only after reading the permission as granted.
        pub(super) fn request(&self, window: &web_sys::Window) -> Option<js_sys::Promise> {
            let function: js_sys::Function =
                get(window.as_ref(), "getScreenDetails")?.dyn_into().ok()?;
            function.call0(window.as_ref()).ok()?.dyn_into().ok()
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "the proof is the capability: the call cannot be written without it"
    )]
    impl WebFeature<ScreenChange> {
        /// The `window.screen` object, which raises `change`.
        pub(super) fn target(&self, window: &web_sys::Window) -> Option<web_sys::EventTarget> {
            let screen = window.screen().ok()?;
            screen.dyn_into().ok()
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod events {
    //! The browser events that change what the display snapshot reports.
    //!
    //! Every observer here only records that something changed and wakes the owner
    //! through the shared [`OwnerSignal`]: the owner's next turn reads
    //! `display_snapshot`, which samples the browser again. None of them runs
    //! owner code, and none reads or writes frame state. The one exception is a
    //! window's own pixel-ratio watch ([`PixelRatioWatch::for_layout`]), which
    //! brings that window's backing store to the new ratio and runs its callbacks.
    //!
    //! - the page's pixel ratio, through a `(resolution: Xdppx)` media query that
    //!   is armed again after each change ([`super::dppx_query`]);
    //! - the tab becoming visible, after which the refresh period is measured
    //!   again;
    //! - Window Management access: when its permission is already granted the
    //!   screen list and its changes are followed. The permission is only read,
    //!   never requested;
    //! - the `dynamic-range` and `color-gamut` queries, whose change is a change
    //!   of the screen's colour.

    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::{Arc, Weak};

    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    use web_sys::{Event, MediaQueryList};

    use super::capability::{GetScreenDetails, MatchMedia, PermissionsQuery, WebFeature};
    use super::color_query::QUERIES;
    use super::dppx_query::DppxWatch;
    use super::source::{WebDisplays, event_target, get};
    use crate::owner::signal::OwnerSignal;
    use crate::owner::web::listener::{Listener, warn_failure};

    /// The listeners that keep the display snapshot current; dropping it removes
    /// them.
    pub(crate) struct DisplayObservers {
        /// Cleared first on drop, so a callback already queued in the browser is
        /// inert.
        active: Rc<Cell<bool>>,
        _pixel_ratio: Option<PixelRatioWatch>,
        _visibility: Option<Listener>,
        _screen_change: Option<Listener>,
        _resize: Option<Listener>,
        _screens: Option<Rc<ScreenAccess>>,
        _color: Vec<Listener>,
    }

    impl DisplayObservers {
        /// Start observing for `displays`; every change wakes `signal`.
        pub(crate) fn start(displays: &Arc<WebDisplays>, signal: &Arc<OwnerSignal>) -> Self {
            let active = Rc::new(Cell::new(true));
            let pixel_ratio = displays.features.match_media.map(|proof| {
                let displays = Arc::downgrade(displays);
                let signal = Arc::downgrade(signal);
                // A new scale is a new snapshot and a new cadence to measure.
                PixelRatioWatch::start(
                    proof,
                    &crate::owner::web::Accepting::reading(&active),
                    move || {
                        if let Some(displays) = displays.upgrade() {
                            displays.invalidate();
                            displays.restart_estimate();
                            wake(&signal);
                        }
                    },
                )
            });
            let visibility = visibility_listener(displays, &active);
            let ratio = pixel_ratio.as_ref().map(PixelRatioWatch::handle);
            let resize = ratio
                .clone()
                .and_then(|ratio| resize_listener(ratio, &active));
            let screen_change = screen_change_listener(displays, signal, &active, ratio);
            let screens = displays
                .features
                .permissions
                .zip(displays.features.screen_details)
                .map(|(permissions, screen_details)| {
                    ScreenAccess::start(
                        permissions,
                        screen_details,
                        Arc::downgrade(displays),
                        Arc::downgrade(signal),
                        &active,
                    )
                });
            let color = color_listeners(displays, signal, &active);
            Self {
                active,
                _pixel_ratio: pixel_ratio,
                _visibility: visibility,
                _screen_change: screen_change,
                _resize: resize,
                _screens: screens,
                _color: color,
            }
        }
    }

    // ==================== Colour ====================

    /// The colour queries raise `change` when the screen's dynamic range or
    /// gamut answer changes: the page moved to another screen, or the system
    /// switched HDR. Each marks the snapshot stale and wakes the owner.
    fn color_listeners(
        displays: &Arc<WebDisplays>,
        signal: &Arc<OwnerSignal>,
        active: &Rc<Cell<bool>>,
    ) -> Vec<Listener> {
        let (Some(media), Some(window)) = (displays.features.match_media, web_sys::window()) else {
            return Vec::new();
        };
        QUERIES
            .into_iter()
            .filter_map(|query| {
                let list = media.query(&window, query).ok().flatten()?;
                let displays = Arc::downgrade(displays);
                let signal = Arc::downgrade(signal);
                let active = Rc::clone(active);
                Listener::add(list.as_ref(), "change", move |_| {
                    if active.get()
                        && let Some(displays) = displays.upgrade()
                    {
                        displays.invalidate();
                        wake(&signal);
                    }
                })
                .inspect_err(|error| warn_failure("colour observation unavailable", error))
                .ok()
            })
            .collect()
    }

    impl Drop for DisplayObservers {
        fn drop(&mut self) {
            self.active.set(false);
        }
    }

    fn wake(signal: &Weak<OwnerSignal>) {
        if let Some(signal) = signal.upgrade() {
            // A failed wake leaves the signal's retry debt for the next turn.
            let _ = signal.wake();
        }
    }

    // ==================== Visibility ====================

    fn visibility_listener(
        displays: &Arc<WebDisplays>,
        active: &Rc<Cell<bool>>,
    ) -> Option<Listener> {
        let document = web_sys::window()?.document()?;
        let displays = Arc::downgrade(displays);
        let active = Rc::clone(active);
        let watched = document.clone();
        let registered = Listener::add(document.as_ref(), "visibilitychange", move |_| {
            if active.get()
                && !watched.hidden()
                && let Some(displays) = displays.upgrade()
            {
                // Frames did not run while the tab was hidden: the cadence is
                // measured again from the first one after it returns.
                displays.restart_estimate();
            }
        });
        match registered {
            Ok(listener) => Some(listener),
            Err(error) => {
                warn_failure("visibility observation unavailable", &error);
                None
            }
        }
    }

    // ==================== The screen object ====================

    /// `window.screen` raises `change` when its size, pixel ratio or position
    /// changes, without any permission, where the browser has the event.
    fn screen_change_listener(
        displays: &Arc<WebDisplays>,
        signal: &Arc<OwnerSignal>,
        active: &Rc<Cell<bool>>,
        ratio: Option<std::rc::Weak<PixelRatioShared>>,
    ) -> Option<Listener> {
        let target = displays
            .features
            .screen_change?
            .target(&web_sys::window()?)?;
        let displays = Arc::downgrade(displays);
        let signal = Arc::downgrade(signal);
        let active = Rc::clone(active);
        match Listener::add(&target, "change", move |_| {
            if active.get()
                && let Some(displays) = displays.upgrade()
            {
                displays.screen_changed();
                // A scale change that arrived while no query was armed shows up
                // with the screen's: arm now rather than at the next retry.
                if let Some(ratio) = ratio.as_ref().and_then(std::rc::Weak::upgrade) {
                    ratio.arm_if_unarmed();
                }
                wake(&signal);
            }
        }) {
            Ok(listener) => Some(listener),
            Err(error) => {
                warn_failure("screen change observation unavailable", &error);
                None
            }
        }
    }

    /// Page zoom fires `resize` on the window: a pixel-ratio watch that is not
    /// armed arms itself from it.
    fn resize_listener(
        ratio: std::rc::Weak<PixelRatioShared>,
        active: &Rc<Cell<bool>>,
    ) -> Option<Listener> {
        let window = web_sys::window()?;
        let active = Rc::clone(active);
        Listener::add(window.as_ref(), "resize", move |_| {
            if active.get()
                && let Some(ratio) = ratio.upgrade()
            {
                ratio.arm_if_unarmed();
            }
        })
        .inspect_err(|error| warn_failure("resize observation unavailable", error))
        .ok()
    }

    // ==================== Pixel ratio ====================

    /// A closure called with an animation frame timestamp.
    type FrameCallback = Closure<dyn FnMut(f64)>;

    /// The `(resolution: Xdppx)` query for the ratio now in effect.
    pub(crate) struct PixelRatioWatch {
        shared: Rc<PixelRatioShared>,
        /// The one JS function every query listens with; it is never replaced, so
        /// the callback that is running can arm its successor without freeing
        /// itself.
        callback: Option<Closure<dyn FnMut(Event)>>,
    }

    struct PixelRatioShared {
        proof: WebFeature<MatchMedia>,
        rule: RefCell<DppxWatch>,
        query: RefCell<Option<MediaQueryList>>,
        function: RefCell<Option<js_sys::Function>>,
        /// What a changed ratio means to whoever watches it.
        on_change: Box<dyn Fn()>,
        active: crate::owner::web::Accepting,
        /// The callback that arms again on a frame after an arming that failed; it
        /// is owned here and never replaced while a frame may call it.
        retry: RefCell<Option<FrameCallback>>,
        /// The pending animation frame of that callback.
        retry_frame: Cell<Option<i32>>,
        /// Retries since the last successful arming; each waits longer than the
        /// one before, up to [`MAX_RETRY_WAIT`] frames, and they never stop: a
        /// watch that is not armed misses every zoom and scale change.
        retries: Cell<u32>,
        /// Frames the pending retry callback still lets pass before it tries.
        wait: Cell<u32>,
    }

    /// How many times arming reads the ratio again when it changed between the
    /// read and the query listening: a query that does not match the ratio now
    /// would never fire for a change that already happened.
    const ARM_ATTEMPTS: usize = 4;

    /// The longest wait, in frames, between two attempts to arm after the browser
    /// refused the query.
    const MAX_RETRY_WAIT: u32 = 120;

    impl PixelRatioShared {
        /// Listen to the query for the ratio in effect now, instead of the spent
        /// one. `true` when the ratio differs from the one the spent query
        /// described, or when no query could be armed (a change may have gone
        /// unseen).
        fn rearm(&self) -> bool {
            let Some(window) = web_sys::window() else {
                return false;
            };
            let Some(function) = self.function.borrow().clone() else {
                return false;
            };
            let spent = self.query.borrow_mut().take();
            if let Some(spent) = spent
                && let Err(error) = spent.remove_event_listener_with_callback("change", &function)
            {
                warn_failure("pixel ratio query removal failed", &error);
            }
            let mut changed = false;
            for _ in 0..ARM_ATTEMPTS {
                let rearm = self.rule.borrow_mut().arm(window.device_pixel_ratio());
                changed |= rearm.changed;
                let Some(text) = rearm.query else { break };
                let query = match self.proof.query(&window, &text) {
                    Ok(Some(query)) => query,
                    Ok(None) => break,
                    Err(error) => {
                        warn_failure("pixel ratio query failed", &error);
                        break;
                    }
                };
                if let Err(error) = query.add_event_listener_with_callback("change", &function) {
                    warn_failure("pixel ratio query registration failed", &error);
                    break;
                }
                if query.matches() {
                    *self.query.borrow_mut() = Some(query);
                    self.retries.set(0);
                    return changed;
                }
                // The ratio moved between reading it and listening: this query
                // describes a ratio that is already gone. Read it again.
                changed = true;
                if let Err(error) = query.remove_event_listener_with_callback("change", &function) {
                    warn_failure("pixel ratio query removal failed", &error);
                }
            }
            self.retry_on_next_frame(&window);
            true
        }

        /// Arm again after a growing wait (one frame, then two, four, and so on up
        /// to [`MAX_RETRY_WAIT`]), for as long as the browser refuses. A retry is
        /// already pending: nothing changes.
        fn retry_on_next_frame(&self, window: &web_sys::Window) {
            if !self.active.get() || self.retry_frame.get().is_some() {
                return;
            }
            let retries = self.retries.get();
            self.retries.set(retries.saturating_add(1));
            let delay = (1_u32 << retries.min(7)).min(MAX_RETRY_WAIT);
            self.wait.set(delay - 1);
            self.request_retry_frame(window);
        }

        fn request_retry_frame(&self, window: &web_sys::Window) {
            let retry = self.retry.borrow();
            let Some(retry) = retry.as_ref() else { return };
            match window.request_animation_frame(retry.as_ref().unchecked_ref()) {
                Ok(frame) => self.retry_frame.set(Some(frame)),
                Err(error) => warn_failure("pixel ratio retry unavailable", &error),
            }
        }

        /// A zoom or a screen change that is already known to the page: arm now if
        /// the watch is not armed, without waiting for a retry frame.
        fn arm_if_unarmed(&self) {
            if self.active.get() && self.query.borrow().is_none() && self.rearm() {
                self.announce_change();
            }
        }

        fn announce_change(&self) {
            (self.on_change)();
        }
    }

    impl PixelRatioWatch {
        /// Watch the ratio for a window's layout: `on_change` runs each time it
        /// changes while `active` holds. A window dragged to a display of another
        /// scale keeps its CSS box, so neither a `ResizeObserver` of that box nor
        /// the window's `resize` event reports it, and one of its device-pixel
        /// box misses it while the rounded size stays. `None` in a browser
        /// without media queries.
        pub(crate) fn for_layout(
            active: &crate::owner::web::Accepting,
            on_change: impl Fn() + 'static,
        ) -> Option<Self> {
            let proof = super::source::BrowserFeatures::detect().match_media?;
            Some(Self::start(proof, active, on_change))
        }

        fn handle(&self) -> std::rc::Weak<PixelRatioShared> {
            Rc::downgrade(&self.shared)
        }

        fn start(
            proof: WebFeature<MatchMedia>,
            active: &crate::owner::web::Accepting,
            on_change: impl Fn() + 'static,
        ) -> Self {
            let shared = Rc::new(PixelRatioShared {
                proof,
                rule: RefCell::new(DppxWatch::new()),
                query: RefCell::new(None),
                function: RefCell::new(None),
                on_change: Box::new(on_change),
                active: active.clone(),
                retry: RefCell::new(None),
                retry_frame: Cell::new(None),
                retries: Cell::new(0),
                wait: Cell::new(0),
            });
            let weak = Rc::downgrade(&shared);
            let callback = Closure::<dyn FnMut(Event)>::new(move |_| {
                if let Some(shared) = weak.upgrade()
                    && shared.active.get()
                    && shared.rearm()
                {
                    shared.announce_change();
                }
            });
            *shared.function.borrow_mut() = Some(
                callback
                    .as_ref()
                    .unchecked_ref::<js_sys::Function>()
                    .clone(),
            );
            let weak = Rc::downgrade(&shared);
            *shared.retry.borrow_mut() = Some(Closure::new(move |_: f64| {
                let Some(shared) = weak.upgrade() else { return };
                shared.retry_frame.set(None);
                if !shared.active.get() || shared.query.borrow().is_some() {
                    return;
                }
                let waiting = shared.wait.get();
                if waiting > 0 {
                    shared.wait.set(waiting - 1);
                    if let Some(window) = web_sys::window() {
                        shared.request_retry_frame(&window);
                    }
                    return;
                }
                shared.arm_if_unarmed();
            }));
            shared.rearm();
            Self {
                shared,
                callback: Some(callback),
            }
        }
    }

    impl crate::owner::web::PixelRatioWatch for PixelRatioWatch {
        /// Arm now if no query is armed (the browser refused the last one), and
        /// report a change that went unseen meanwhile.
        fn arm_if_unarmed(&self) {
            self.shared.arm_if_unarmed();
        }
    }

    impl Drop for PixelRatioWatch {
        fn drop(&mut self) {
            if let Some(frame) = self.shared.retry_frame.take()
                && let Some(window) = web_sys::window()
            {
                window.cancel_animation_frame(frame).ok();
            }
            let query = self.shared.query.borrow_mut().take();
            let function = self.shared.function.borrow_mut().take();
            let Some(callback) = self.callback.take() else {
                return;
            };
            if let (Some(query), Some(function)) = (query, function)
                && let Err(error) = query.remove_event_listener_with_callback("change", &function)
            {
                callback.forget();
                warn_failure("pixel ratio query removal failed", &error);
            }
        }
    }

    // ==================== Window Management ====================

    /// Follows the screen list while the Window Management permission is granted.
    struct ScreenAccess {
        permissions: WebFeature<PermissionsQuery>,
        screen_details: WebFeature<GetScreenDetails>,
        displays: Weak<WebDisplays>,
        signal: Weak<OwnerSignal>,
        active: Rc<Cell<bool>>,
        /// The `PermissionStatus` and its `change` listener, when one could be
        /// registered; without it the state is still read, but a later change is
        /// missed.
        permission: RefCell<Option<(JsValue, Option<Listener>)>>,
        /// The `ScreenDetails` object being followed and the screen it named as
        /// current when last looked at.
        followed: RefCell<Option<Followed>>,
        /// The listeners on the `ScreenDetails` object.
        details: RefCell<Vec<Listener>>,
        /// The listeners on each listed screen: its size or pixel ratio changed.
        screens: RefCell<Vec<Listener>>,
        /// Counts refreshes; an older one that finishes late discards its result.
        generation: Cell<u64>,
    }

    /// The permission names Chrome has used for Window Management, current first.
    const PERMISSION_NAMES: [&str; 2] = ["window-management", "window-placement"];

    /// A `ScreenDetails` object and the `currentScreen` it last named.
    struct Followed {
        details: JsValue,
        current: Option<JsValue>,
    }

    impl ScreenAccess {
        fn start(
            permissions: WebFeature<PermissionsQuery>,
            screen_details: WebFeature<GetScreenDetails>,
            displays: Weak<WebDisplays>,
            signal: Weak<OwnerSignal>,
            active: &Rc<Cell<bool>>,
        ) -> Rc<Self> {
            let access = Rc::new(Self {
                permissions,
                screen_details,
                displays,
                signal,
                active: Rc::clone(active),
                permission: RefCell::new(None),
                followed: RefCell::new(None),
                details: RefCell::new(Vec::new()),
                screens: RefCell::new(Vec::new()),
                generation: Cell::new(0),
            });
            Self::refresh(&access);
            access
        }

        /// Read the permission and bring the screen list in line with it.
        fn refresh(this: &Rc<Self>) {
            let generation = this.generation.get() + 1;
            this.generation.set(generation);
            let access = Rc::downgrade(this);
            wasm_bindgen_futures::spawn_local(async move {
                let Some(access) = access.upgrade() else {
                    return;
                };
                access.refresh_async(&access, generation).await;
            });
        }

        fn current(&self, generation: u64) -> bool {
            self.active.get() && self.generation.get() == generation
        }

        async fn refresh_async(&self, this: &Rc<Self>, generation: u64) {
            if self.permission.borrow().is_none() {
                let Some(status) = self.query_permission().await else {
                    return;
                };
                if !self.current(generation) {
                    return;
                }
                self.watch_permission(this, status);
            }
            let granted = self
                .permission
                .borrow()
                .as_ref()
                .is_some_and(|(status, _)| permission_granted(status));
            if !granted {
                self.set_details(None);
                return;
            }
            if self.followed.borrow().is_none() {
                let Some(details) = self.get_screen_details().await else {
                    return;
                };
                if !self.current(generation) {
                    return;
                }
                self.watch_details(this, &details);
                self.set_details(Some(details));
            }
        }

        /// The `PermissionStatus` for Window Management, read without prompting.
        async fn query_permission(&self) -> Option<JsValue> {
            let window = web_sys::window()?;
            for name in PERMISSION_NAMES {
                // An unknown name throws or rejects; the older name is tried next.
                let Some(promise) = self.permissions.query(&window, name) else {
                    continue;
                };
                if let Ok(status) = wasm_bindgen_futures::JsFuture::from(promise).await {
                    return Some(status);
                }
            }
            None
        }

        /// `getScreenDetails()`. Called only after the permission read `granted`,
        /// when it does not prompt.
        async fn get_screen_details(&self) -> Option<JsValue> {
            let window = web_sys::window()?;
            let promise = self.screen_details.request(&window)?;
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(details) => Some(details),
                Err(error) => {
                    warn_failure("screen details unavailable", &error);
                    None
                }
            }
        }

        /// Re-read the permission whenever the user changes it.
        fn watch_permission(&self, this: &Rc<Self>, status: JsValue) {
            let access = Rc::downgrade(this);
            let listener = event_target(&status).and_then(|target| {
                let listener = Listener::add(&target, "change", move |_| {
                    if let Some(access) = access.upgrade()
                        && access.active.get()
                    {
                        Self::refresh(&access);
                    }
                });
                // Without the listener the state is still readable; a later
                // change is missed.
                listener
                    .inspect_err(|error| warn_failure("permission observation unavailable", error))
                    .ok()
            });
            *self.permission.borrow_mut() = Some((status, listener));
        }

        /// Follow the screen list and the screen the window is on.
        fn watch_details(&self, this: &Rc<Self>, details: &JsValue) {
            *self.followed.borrow_mut() = Some(Followed {
                details: details.clone(),
                current: get(details, "currentScreen"),
            });
            self.watch_screens();
            let Some(target) = event_target(details) else {
                return;
            };
            let mut listeners = Vec::new();
            for event in ["screenschange", "currentscreenchange"] {
                let access = Rc::downgrade(this);
                let registered = Listener::add(&target, event, move |_| {
                    if let Some(access) = access.upgrade()
                        && access.active.get()
                    {
                        if event == "screenschange" {
                            access.screens_changed();
                        } else {
                            access.current_screen_changed();
                        }
                    }
                });
                match registered {
                    Ok(listener) => listeners.push(listener),
                    Err(error) => warn_failure("screen change observation unavailable", &error),
                }
            }
            *self.details.borrow_mut() = listeners;
        }

        /// Listen to `change` on every listed screen: one that changes size or
        /// pixel ratio without the list changing is otherwise never seen.
        fn watch_screens(&self) {
            let listed = self
                .followed
                .borrow()
                .as_ref()
                .and_then(|followed| get(&followed.details, "screens"))
                .map(|screens| js_sys::Array::from(&screens).to_vec())
                .unwrap_or_default();
            let listeners: Vec<Listener> = listed
                .iter()
                .filter_map(event_target)
                .filter_map(|target| {
                    let displays = self.displays.clone();
                    let signal = self.signal.clone();
                    let active = Rc::clone(&self.active);
                    Listener::add(&target, "change", move |_| {
                        if active.get()
                            && let Some(displays) = displays.upgrade()
                        {
                            displays.invalidate();
                            wake(&signal);
                        }
                    })
                    .inspect_err(|error| warn_failure("screen observation unavailable", error))
                    .ok()
                })
                .collect();
            // Replacing drops the old listeners outside the borrow.
            let old = std::mem::replace(&mut *self.screens.borrow_mut(), listeners);
            drop(old);
        }

        /// The list of screens changed: follow the new ones and sample again.
        fn screens_changed(&self) {
            self.watch_screens();
            if let Some(displays) = self.displays.upgrade() {
                displays.invalidate();
                wake(&self.signal);
            }
        }

        /// `currentscreenchange`: the window's screen changed only if the object
        /// `currentScreen` names is another one; otherwise the same screen's
        /// metrics changed and the cadence it was measured at still holds.
        fn current_screen_changed(&self) {
            let moved = {
                let mut followed = self.followed.borrow_mut();
                let Some(followed) = followed.as_mut() else {
                    return;
                };
                let now = get(&followed.details, "currentScreen");
                let moved = match (&followed.current, &now) {
                    (Some(before), Some(now)) => !js_sys::Object::is(before, now),
                    (None, None) => false,
                    _ => true,
                };
                followed.current = now;
                moved
            };
            if let Some(displays) = self.displays.upgrade() {
                if moved {
                    // Another screen has another cadence to measure.
                    displays.forget_estimate();
                } else {
                    displays.invalidate();
                }
                wake(&self.signal);
            }
        }

        /// Install or drop the `ScreenDetails` object and wake the owner.
        fn set_details(&self, details: Option<JsValue>) {
            let Some(displays) = self.displays.upgrade() else {
                return;
            };
            let dropping = details.is_none();
            if dropping {
                let old = (
                    std::mem::take(&mut *self.details.borrow_mut()),
                    std::mem::take(&mut *self.screens.borrow_mut()),
                    self.followed.borrow_mut().take(),
                );
                drop(old);
            }
            let previous = displays.set_details(details);
            let unchanged = dropping && previous.is_none();
            drop(previous);
            if !unchanged {
                wake(&self.signal);
            }
        }
    }

    fn permission_granted(status: &JsValue) -> bool {
        get(status, "state")
            .and_then(|state| state.as_string())
            .as_deref()
            == Some("granted")
    }
}
