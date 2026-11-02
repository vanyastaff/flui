//! The Android keyboard's motion as `FluiView`'s insets animation callback
//! reports it (ADR-0277 §4): its bottom edge before and after, when it
//! started, how long it lasts, and three values of its interpolator, which
//! name the [`InsetCurve`] it runs along.

use std::time::Duration;

use flui_foundation::geometry::EdgeInsets;
use flui_platform::{EventTime, InsetCurve, InsetMotion, KeyboardInset};

/// The fractions of time at which `FluiView` reads the interpolator, in the
/// order its values arrive (`KeyboardMotion.PROBES` in `FluiView.java`).
const PROBES: [f64; 3] = [0.25, 0.5, 0.75];

/// The interpolators Android's insets controller runs an animation along
/// (`InsetsController`): the input method's own while the app observes it, the
/// two it uses when no app does, and the system bars'. A custom interpolator
/// is taken as the nearest of these or linear.
const CURVES: [InsetCurve; 5] = [
    InsetCurve::CubicBezier {
        x1: 0.2,
        y1: 0.0,
        x2: 0.0,
        y2: 1.0,
    },
    InsetCurve::CubicBezier {
        x1: 0.0,
        y1: 0.0,
        x2: 0.2,
        y2: 1.0,
    },
    InsetCurve::CubicBezier {
        x1: 0.4,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    },
    InsetCurve::CubicBezier {
        x1: 0.4,
        y1: 0.0,
        x2: 0.2,
        y2: 1.0,
    },
    InsetCurve::Linear,
];

/// The curve whose values at [`PROBES`] are nearest to `values`; linear for
/// values that are no number.
fn curve_of(values: [f32; 3]) -> InsetCurve {
    if !values.iter().all(|value| value.is_finite()) {
        return InsetCurve::Linear;
    }
    let distance = |curve: InsetCurve| {
        PROBES
            .iter()
            .zip(values)
            .map(|(&time, value)| (curve.progress(time) - f64::from(value)).abs())
            .fold(0.0, f64::max)
    };
    CURVES
        .into_iter()
        .map(|curve| (distance(curve), curve))
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
        .map_or(InsetCurve::Linear, |(_, curve)| curve)
}

/// One keyboard motion in device pixels, as reported on the Java main thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct KeyboardMotion {
    /// The keyboard's bottom edge when the motion started.
    from: i32,
    /// The keyboard's bottom edge once it ends.
    to: i32,
    start: EventTime,
    duration: Duration,
    curve: InsetCurve,
}

impl KeyboardMotion {
    /// started at `start` on the event timeline and lasts `duration_millis`,
    /// along the interpolator that took `values` at [`PROBES`]. `None` for a
    /// motion with no start or no length of time, which a user drives frame
    /// by frame.
    /// drives frame by frame, or a start before the clock began.
    pub(crate) fn try_from_report(
        from: i32,
        to: i32,
        start: Option<EventTime>,
        duration_millis: i64,
        values: [f32; 3],
    ) -> Option<Self> {
        let start = start?;
        let duration = Duration::from_millis(u64::try_from(duration_millis).ok()?);
        if duration.is_zero() {
            return None;
        }
        Some(Self {
            from: from.max(0),
            to: to.max(0),
            start,
            duration,
            curve: curve_of(values),
        })
    }

    /// The keyboard occlusion for a sample whose keyboard is `target` in
    /// logical pixels, with a bottom edge of `bottom` device pixels, at
    /// `scale`: moving toward `target` when the sample is this motion's end,
    /// `None` when it is some other occlusion (taken before the motion, or
    /// after another replaced it).
    pub(crate) fn keyboard(
        self,
        target: EdgeInsets,
        bottom: i32,
        scale: f64,
    ) -> Option<KeyboardInset> {
        if bottom != self.to || !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let from = EdgeInsets::new(
            target.top,
            target.right,
            f64::from(self.from) / scale,
            target.left,
        );
        let motion = InsetMotion::try_new(from, self.start, self.duration, self.curve).ok()?;
        KeyboardInset::try_moving(target, motion).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(curve: InsetCurve) -> [f32; 3] {
        PROBES.map(|time| curve.progress(time) as f32)
    }

    #[test]
    fn an_interpolator_is_named_by_its_values() {
        for curve in CURVES {
            assert_eq!(curve_of(values(curve)), curve, "{curve:?}");
        }
        // A custom interpolator near the input method's own.
        let [a, b, c] = values(CURVES[0]);
        assert_eq!(curve_of([a + 0.02, b - 0.01, c]), CURVES[0]);
        // Values that are no number fall back to linear.
        assert_eq!(curve_of([f32::NAN; 3]), InsetCurve::Linear);
    }

    #[test]
    fn a_motion_moves_the_sample_that_is_its_end() {
        let motion = KeyboardMotion::try_from_report(
            0,
            900,
            Some(EventTime::from_nanos(1_000_000)),
            285,
            values(CURVES[0]),
        )
        .expect("a motion with a duration");
        let target = EdgeInsets::new(0.0, 0.0, 300.0, 0.0);
        let keyboard = motion
            .keyboard(target, 900, 3.0)
            .expect("the sample is the motion's end");
        assert_eq!(keyboard.target(), target);
        let moving = keyboard.motion().expect("a keyboard in motion");
        assert_eq!(moving.from(), EdgeInsets::ZERO);
        assert_eq!(moving.start(), EventTime::from_nanos(1_000_000));
        assert_eq!(moving.duration(), Duration::from_millis(285));
        assert_eq!(moving.curve(), CURVES[0]);
        // Halfway along the curve, the keyboard is most of the way up.
        let half = keyboard.at(EventTime::from_nanos(1_000_000 + 142_500_000));
        assert!(half.bottom > 150.0 && half.bottom < 300.0, "{half:?}");
    }

    #[test]
    fn a_motion_leaves_other_samples_and_lengthless_reports_alone() {
        let motion = KeyboardMotion::try_from_report(
            900,
            0,
            Some(EventTime::from_nanos(0)),
            340,
            values(CURVES[4]),
        )
        .expect("a motion with a duration");
        // Sampled before the keyboard began to hide.
        assert_eq!(
            motion.keyboard(EdgeInsets::new(0.0, 0.0, 300.0, 0.0), 900, 3.0),
            None
        );
        assert_eq!(motion.keyboard(EdgeInsets::ZERO, 0, f64::NAN), None);
        let hiding = motion
            .keyboard(EdgeInsets::ZERO, 0, 3.0)
            .and_then(|keyboard| keyboard.motion())
            .expect("the hidden keyboard is the motion's end");
        assert_eq!(hiding.from(), EdgeInsets::new(0.0, 0.0, 300.0, 0.0));
        // A user-driven motion (`getDurationMillis` is -1), a zero one and one
        // with no start on the event timeline.
        assert_eq!(
            KeyboardMotion::try_from_report(0, 900, Some(EventTime::from_nanos(0)), -1, [0.5; 3]),
            None
        );
        assert_eq!(
            KeyboardMotion::try_from_report(0, 900, Some(EventTime::from_nanos(0)), 0, [0.5; 3]),
            None
        );
        assert_eq!(
            KeyboardMotion::try_from_report(0, 900, None, 285, [0.5; 3]),
            None
        );
    }
}
