//! The one rule for a scale an operating system reports: the number a host
//! reads for a display or a window (a DPI over the base DPI, a backing scale,
//! a density, a pixel ratio) becomes a `DevicePixelRatio` here, once, where
//! it enters.
//!
//! A ratio a screen can have (`DevicePixelRatio::MIN..=MAX`) is taken. One
//! that is not (a DPI of zero for a handle the system could not measure, a
//! NaN from a broken driver, a number far past any screen) reads as the base
//! ratio, one device pixel per logical pixel: the display or window the
//! system describes badly still exists and still needs a scale, and leaving
//! it out would lose it. The first refusal of a streak is handed back to the
//! caller as a [`RefusedScale`], which it reports with nothing borrowed, since
//! a `tracing` subscriber is the application's code; a good reading ends the
//! streak. A streak is kept per source: one [`ScaleStreak`] where a host reads
//! one (a window, the system DPI, `Xft.dpi`), `ScaleStreaks` by display
//! where it reads several in one sample (only those hosts compile it).

#![forbid(unsafe_code)]

use flui_foundation::geometry::DevicePixelRatio;

/// Whether the last reading of one source of a scale was refused, so a run of
/// refusals is reported once.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScaleStreak {
    refusing: bool,
}

impl ScaleStreak {
    /// The ratio `reported` gives: itself when it is finite and positive,
    /// else the base ratio, with the refusal to report when it starts a
    /// streak. `what` names the source in the report.
    pub(crate) fn read(
        &mut self,
        reported: f64,
        what: &'static str,
    ) -> (DevicePixelRatio, Option<RefusedScale>) {
        if let Some(ratio) = DevicePixelRatio::try_new(reported) {
            self.refusing = false;
            return (ratio, None);
        }
        let first = !std::mem::replace(&mut self.refusing, true);
        (
            DevicePixelRatio::ONE,
            first.then_some(RefusedScale { reported, what }),
        )
    }
}

/// One [`ScaleStreak`] per source of a scale a host reads in one sample (a
/// display, by its typed identity), so a run of refusals is reported once per
/// source and a second source's failure is reported apart from the first's.
/// It holds no more sources than the last sample read: a source that sample
/// did not read (a display that went) is dropped, and starts afresh if it
/// returns.
#[cfg(any(
    windows,
    target_os = "android",
    target_os = "ios",
    target_os = "macos",
    target_arch = "wasm32",
    test
))]
#[derive(Debug)]
pub(crate) struct ScaleStreaks<K> {
    refusing: rustc_hash::FxHashSet<K>,
}

#[cfg(any(
    windows,
    target_os = "android",
    target_os = "ios",
    target_os = "macos",
    target_arch = "wasm32",
    test
))]
impl<K> Default for ScaleStreaks<K> {
    fn default() -> Self {
        Self {
            refusing: rustc_hash::FxHashSet::default(),
        }
    }
}

#[cfg(any(
    windows,
    target_os = "android",
    target_os = "ios",
    target_os = "macos",
    target_arch = "wasm32",
    test
))]
impl<K: Eq + std::hash::Hash + Copy> ScaleStreaks<K> {
    /// [`ScaleStreak::read`] for the source `source`.
    pub(crate) fn read(
        &mut self,
        source: K,
        reported: f64,
        what: &'static str,
    ) -> (DevicePixelRatio, Option<RefusedScale>) {
        let mut streak = ScaleStreak {
            refusing: self.refusing.contains(&source),
        };
        let read = streak.read(reported, what);
        if streak.refusing {
            self.refusing.insert(source);
        } else {
            self.refusing.remove(&source);
        }
        read
    }

    /// Keep only the sources in `read`, the ones the sample just read.
    pub(crate) fn keep_only(&mut self, read: &[K]) {
        self.refusing.retain(|source| read.contains(source));
    }
}

/// A scale the operating system reported that is not one, still to be
/// reported.
#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use = "a refused scale is reported, with nothing borrowed"]
pub(crate) struct RefusedScale {
    reported: f64,
    what: &'static str,
}

impl RefusedScale {
    /// Report the refusal. Call it with no borrow or lock held.
    pub(crate) fn report(self) {
        tracing::warn!(
            source = self.what,
            reported = self.reported,
            "the operating system reported a scale outside the range a screen can have; \
             reading it as one device pixel per logical pixel"
        );
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::DevicePixelRatio;

    use super::{ScaleStreak, ScaleStreaks};

    /// Each display keeps its own streak: alternating good and bad sources
    /// report each bad one once, a second bad source is reported apart from
    /// the first, a source that recovers and fails again is reported again,
    /// and a display that went starts afresh when it returns.
    #[test]
    fn each_source_keeps_its_own_streak() {
        let mut streaks = ScaleStreaks::<u8>::default();
        let mut reported = |source: u8, scale: f64| streaks.read(source, scale, "test").1.is_some();
        // (case, source, reported scale, reported as refused)
        let readings = [
            ("a good source", 1, 2.0, false),
            ("a bad source starts its streak", 2, 0.0, true),
            ("the good source again", 1, 2.0, false),
            ("the bad source continues its streak", 2, 0.0, false),
            ("a second bad source is its own", 3, f64::NAN, true),
            ("the first bad source continues", 2, -1.0, false),
            ("the first bad source recovers", 2, 1.5, false),
            ("and fails again", 2, 0.0, true),
        ];
        for (case, source, scale, refused) in readings {
            assert_eq!(reported(source, scale), refused, "{case}");
        }
        // Display 2 goes: its streak goes with it, and it starts afresh.
        streaks.keep_only(&[1, 3]);
        assert!(streaks.read(3, 0.0, "test").1.is_none(), "3 still refusing");
        assert!(
            streaks.read(2, 0.0, "test").1.is_some(),
            "a display that returns is reported again"
        );
    }

    /// A good number is taken; a bad one reads as the base ratio and is
    /// reported once per streak, which a good number ends.
    #[test]
    fn a_reported_scale_is_taken_or_read_as_the_base_ratio_once_reported() {
        let mut streak = ScaleStreak::default();
        // (case, reported, ratio, reported as refused)
        let readings = [
            ("a scale", 1.5, 1.5, false),
            ("zero starts a streak", 0.0, 1.0, true),
            ("a negative number continues it", -2.0, 1.0, false),
            ("not a number continues it", f64::NAN, 1.0, false),
            ("infinity continues it", f64::INFINITY, 1.0, false),
            ("a scale ends it", 2.0, 2.0, false),
            (
                "a negative sliver starts another",
                -f64::MIN_POSITIVE,
                1.0,
                true,
            ),
            ("the base ratio is a scale", 1.0, 1.0, false),
            (
                "just past the largest ratio starts another",
                DevicePixelRatio::MAX.get() * 1.001,
                1.0,
                true,
            ),
            ("the largest ratio is a scale", 32.0, 32.0, false),
            ("below the smallest ratio starts another", 1e-310, 1.0, true),
            (
                "negative infinity continues it",
                f64::NEG_INFINITY,
                1.0,
                false,
            ),
        ];
        for (case, reported, ratio, refused) in readings {
            let (read, refusal) = streak.read(reported, "test");
            assert_eq!(
                read,
                DevicePixelRatio::try_new(ratio).expect("a ratio"),
                "{case}"
            );
            assert_eq!(refusal.is_some(), refused, "{case}");
        }
    }
}
