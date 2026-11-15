//! The one rule for a scale an operating system reports: the number a host
//! reads for a display or a window (a DPI over the base DPI, a backing scale,
//! a density, a pixel ratio) becomes a `DevicePixelRatio` here, once, where
//! it enters.
//!
//! A ratio that is finite and positive is taken. One that is not (a DPI of
//! zero for a handle the system could not measure, a NaN from a broken
//! driver) reads as the base ratio, one device pixel per logical pixel: the
//! display or window the system describes badly still exists and still needs
//! a scale, and leaving it out would lose it. The first refusal of a streak is
//! handed back to the caller as a [`RefusedScale`], which it reports with
//! nothing borrowed, since a `tracing` subscriber is the application's code; a
//! good reading ends the streak.

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

/// A reading with no earlier one to continue: a refused number is always
/// the first of its streak.
#[cfg(any(windows, x11_host, test))]
pub(crate) fn read_scale(
    reported: f64,
    what: &'static str,
) -> (DevicePixelRatio, Option<RefusedScale>) {
    ScaleStreak::default().read(reported, what)
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
            "the operating system reported a scale that is not a finite positive ratio; \
             reading it as one device pixel per logical pixel"
        );
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::DevicePixelRatio;

    use super::ScaleStreak;

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
                "negative infinity starts another",
                f64::NEG_INFINITY,
                1.0,
                true,
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
