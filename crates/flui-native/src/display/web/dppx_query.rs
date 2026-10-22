//! The media query a page arms to learn that its `devicePixelRatio` changed.
//!
//! The browser has no event for a changed pixel ratio, only for a media query
//! whose match flips. The query `(resolution: Xdppx)` matches while the ratio
//! is exactly `X`, so it fires `change` the moment the ratio becomes anything
//! else: page zoom, a window dragged to a display with another scale, an OS
//! scale change. A fired query is spent (it describes the old ratio), so the
//! next one is armed for the new ratio before anything else happens. This
//! module is that rule; the platform owns the `MediaQueryList` objects.

#![forbid(unsafe_code)]

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
