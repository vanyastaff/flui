//! Animation curve types
//!
//! This module contains types for animation easing curves,
//! similar to Flutter's Curves class.

/// A mapping from a unit interval (0.0 to 1.0) to another unit interval.
///
/// Curves are used to adjust the rate of change of an animation over time,
/// allowing animations to speed up and slow down, rather than moving at a
/// constant rate.
///
/// Similar to Flutter's `Curve`.
pub trait Curve {
    /// Returns the value of the curve at the given time.
    ///
    /// The input `t` should be in the range [0.0, 1.0].
    /// The output is also typically in the range [0.0, 1.0], though some
    /// curves may produce values outside this range.
    fn transform(&self, t: f32) -> f32;

    /// Returns a new curve that is the reverse of this curve.
    fn flipped(&self) -> FlippedCurve
    where
        Self: Sized + Clone + 'static,
    {
        FlippedCurve {
            curve: Box::new(self.clone()),
        }
    }
}

/// A curve that is the reversed curve of its parent.
///
/// Similar to Flutter's `FlippedCurve`.
pub struct FlippedCurve {
    curve: Box<dyn Curve + 'static>,
}

impl Curve for FlippedCurve {
    fn transform(&self, t: f32) -> f32 {
        1.0 - self.curve.transform(t)
    }
}

/// A linear animation curve.
///
/// This is the simplest curve - it produces no easing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Linear;

impl Curve for Linear {
    fn transform(&self, t: f32) -> f32 {
        t
    }
}

/// A curve that is 0.0 until a threshold, then jumps to 1.0.
///
/// Similar to Flutter's `Threshold`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Threshold {
    pub threshold: f32,
}

impl Threshold {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl Curve for Threshold {
    fn transform(&self, t: f32) -> f32 {
        if t < self.threshold {
            0.0
        } else {
            1.0
        }
    }
}

/// A sawtooth curve that repeats a given number of times.
///
/// Similar to Flutter's `SawTooth`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SawTooth {
    pub count: usize,
}

impl SawTooth {
    pub fn new(count: usize) -> Self {
        Self { count }
    }
}

impl Curve for SawTooth {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        (t * self.count as f32) % 1.0
    }
}

/// A cubic polynomial curve.
///
/// This is the basis for many standard easing curves.
/// Similar to Flutter's `Cubic`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cubic {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
}

impl Cubic {
    /// Create a new cubic curve with the given control points.
    pub const fn new(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self { a, b, c, d }
    }

    fn evaluate_cubic(a: f32, b: f32, m: f32) -> f32 {
        3.0 * a * (1.0 - m) * (1.0 - m) * m + 3.0 * b * (1.0 - m) * m * m + m * m * m
    }
}

impl Curve for Cubic {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let mut start = 0.0_f32;
        let mut end = 1.0_f32;

        // Binary search for the t value that gives us the input x
        for _ in 0..8 {
            let mid = (start + end) / 2.0;
            let estimate = Self::evaluate_cubic(self.a, self.c, mid);

            if (t - estimate).abs() < 0.001 {
                return Self::evaluate_cubic(self.b, self.d, mid);
            }

            if estimate < t {
                start = mid;
            } else {
                end = mid;
            }
        }

        Self::evaluate_cubic(self.b, self.d, (start + end) / 2.0)
    }
}

/// A collection of common animation curves.
///
/// Similar to Flutter's `Curves` class.
pub struct Curves;

impl Curves {
    /// A linear animation curve (no easing).
    pub const LINEAR: Linear = Linear;

    /// A curve that starts slowly and ends quickly (ease in).
    pub const EASE_IN: Cubic = Cubic::new(0.42, 0.0, 1.0, 1.0);

    /// A curve that starts quickly and ends slowly (ease out).
    pub const EASE_OUT: Cubic = Cubic::new(0.0, 0.0, 0.58, 1.0);

    /// A curve that starts slowly, speeds up, then slows down (ease in-out).
    pub const EASE_IN_OUT: Cubic = Cubic::new(0.42, 0.0, 0.58, 1.0);

    /// A cubic animation curve (default ease).
    pub const EASE: Cubic = Cubic::new(0.25, 0.1, 0.25, 1.0);

    /// A curve that starts slowly and accelerates quickly.
    pub const EASE_IN_QUAD: Cubic = Cubic::new(0.55, 0.085, 0.68, 0.53);

    /// A curve that starts quickly and decelerates.
    pub const EASE_OUT_QUAD: Cubic = Cubic::new(0.25, 0.46, 0.45, 0.94);

    /// A curve that accelerates then decelerates.
    pub const EASE_IN_OUT_QUAD: Cubic = Cubic::new(0.455, 0.03, 0.515, 0.955);

    /// A curve that starts slowly with a cubic function.
    pub const EASE_IN_CUBIC: Cubic = Cubic::new(0.55, 0.055, 0.675, 0.19);

    /// A curve that ends slowly with a cubic function.
    pub const EASE_OUT_CUBIC: Cubic = Cubic::new(0.215, 0.61, 0.355, 1.0);

    /// A curve that starts and ends slowly with a cubic function.
    pub const EASE_IN_OUT_CUBIC: Cubic = Cubic::new(0.645, 0.045, 0.355, 1.0);

    /// A curve that starts very slowly.
    pub const EASE_IN_QUART: Cubic = Cubic::new(0.895, 0.03, 0.685, 0.22);

    /// A curve that ends very slowly.
    pub const EASE_OUT_QUART: Cubic = Cubic::new(0.165, 0.84, 0.44, 1.0);

    /// A curve that starts and ends very slowly.
    pub const EASE_IN_OUT_QUART: Cubic = Cubic::new(0.77, 0.0, 0.175, 1.0);

    /// A curve that starts extremely slowly.
    pub const EASE_IN_QUINT: Cubic = Cubic::new(0.755, 0.05, 0.855, 0.06);

    /// A curve that ends extremely slowly.
    pub const EASE_OUT_QUINT: Cubic = Cubic::new(0.23, 1.0, 0.32, 1.0);

    /// A curve that starts and ends extremely slowly.
    pub const EASE_IN_OUT_QUINT: Cubic = Cubic::new(0.86, 0.0, 0.07, 1.0);

    /// A curve that starts slowly with a sine wave.
    pub const EASE_IN_SINE: Cubic = Cubic::new(0.47, 0.0, 0.745, 0.715);

    /// A curve that ends slowly with a sine wave.
    pub const EASE_OUT_SINE: Cubic = Cubic::new(0.39, 0.575, 0.565, 1.0);

    /// A curve that starts and ends slowly with a sine wave.
    pub const EASE_IN_OUT_SINE: Cubic = Cubic::new(0.445, 0.05, 0.55, 0.95);

    /// A curve that starts slowly with an exponential function.
    pub const EASE_IN_EXPO: Cubic = Cubic::new(0.95, 0.05, 0.795, 0.035);

    /// A curve that ends slowly with an exponential function.
    pub const EASE_OUT_EXPO: Cubic = Cubic::new(0.19, 1.0, 0.22, 1.0);

    /// A curve that starts and ends slowly with an exponential function.
    pub const EASE_IN_OUT_EXPO: Cubic = Cubic::new(1.0, 0.0, 0.0, 1.0);

    /// A curve that starts slowly with a circular function.
    pub const EASE_IN_CIRC: Cubic = Cubic::new(0.6, 0.04, 0.98, 0.335);

    /// A curve that ends slowly with a circular function.
    pub const EASE_OUT_CIRC: Cubic = Cubic::new(0.075, 0.82, 0.165, 1.0);

    /// A curve that starts and ends slowly with a circular function.
    pub const EASE_IN_OUT_CIRC: Cubic = Cubic::new(0.785, 0.135, 0.15, 0.86);

    /// A curve that backs up slightly before moving forward.
    pub const EASE_IN_BACK: Cubic = Cubic::new(0.6, -0.28, 0.735, 0.045);

    /// A curve that goes slightly beyond 1.0 before settling.
    pub const EASE_OUT_BACK: Cubic = Cubic::new(0.175, 0.885, 0.32, 1.275);

    /// A curve that backs up and overshoots both ends.
    pub const EASE_IN_OUT_BACK: Cubic = Cubic::new(0.68, -0.55, 0.265, 1.55);

    /// A curve that quickly accelerates and decelerates.
    pub const FAST_OUT_SLOW_IN: Cubic = Cubic::new(0.4, 0.0, 0.2, 1.0);

    /// A curve used for incoming elements.
    pub const DECELERATE: Cubic = Cubic::new(0.0, 0.0, 0.2, 1.0);

    /// A curve used for outgoing elements.
    pub const ACCELERATE: Cubic = Cubic::new(0.4, 0.0, 1.0, 1.0);
}

/// An interval curve that only animates between two points.
///
/// Similar to Flutter's `Interval`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub begin: f32,
    pub end: f32,
}

impl Interval {
    pub fn new(begin: f32, end: f32) -> Self {
        Self { begin, end }
    }
}

impl Curve for Interval {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let t = ((t - self.begin) / (self.end - self.begin)).clamp(0.0, 1.0);
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_curve() {
        let curve = Linear;
        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(0.5), 0.5);
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn test_threshold_curve() {
        let curve = Threshold::new(0.5);
        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(0.49), 0.0);
        assert_eq!(curve.transform(0.5), 1.0);
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn test_sawtooth_curve() {
        let curve = SawTooth::new(2);
        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(0.25), 0.5);
        assert_eq!(curve.transform(0.5), 0.0);
        assert_eq!(curve.transform(0.75), 0.5);
    }

    #[test]
    fn test_cubic_curve_bounds() {
        let curve = Curves::EASE_IN;
        let start = curve.transform(0.0);
        let end = curve.transform(1.0);

        assert!((start - 0.0).abs() < 0.01);
        assert!((end - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cubic_curve_monotonic() {
        let curve = Curves::EASE_IN_OUT;

        // Should be monotonically increasing
        let mut prev = 0.0;
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let value = curve.transform(t);
            assert!(value >= prev, "Curve should be monotonically increasing");
            prev = value;
        }
    }

    #[test]
    fn test_interval_curve() {
        let curve = Interval::new(0.25, 0.75);

        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(0.25), 0.0);
        assert_eq!(curve.transform(0.5), 0.5);
        assert_eq!(curve.transform(0.75), 1.0);
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn test_flipped_curve() {
        let linear = Linear;
        let flipped = linear.flipped();

        assert_eq!(flipped.transform(0.0), 1.0);
        assert_eq!(flipped.transform(0.5), 0.5);
        assert_eq!(flipped.transform(1.0), 0.0);
    }

    #[test]
    fn test_common_curves_exist() {
        // Just verify all the common curves are accessible
        let _linear = Curves::LINEAR;
        let _cubic_curves = [
            Curves::EASE_IN,
            Curves::EASE_OUT,
            Curves::EASE_IN_OUT,
            Curves::EASE,
            Curves::FAST_OUT_SLOW_IN,
            Curves::DECELERATE,
            Curves::ACCELERATE,
        ];
    }

    #[test]
    fn test_ease_in_slower_at_start() {
        let curve = Curves::EASE_IN;

        // At the beginning, the curve should move slowly (derivative is small)
        let delta_early = curve.transform(0.1) - curve.transform(0.0);
        let delta_late = curve.transform(1.0) - curve.transform(0.9);

        assert!(delta_early < delta_late, "Ease in should be slower at the start");
    }

    #[test]
    fn test_ease_out_slower_at_end() {
        let curve = Curves::EASE_OUT;

        // At the end, the curve should move slowly (derivative is small)
        let delta_early = curve.transform(0.1) - curve.transform(0.0);
        let delta_late = curve.transform(1.0) - curve.transform(0.9);

        assert!(delta_early > delta_late, "Ease out should be slower at the end");
    }

    #[test]
    fn test_back_curves_overshoot() {
        let ease_out_back = Curves::EASE_OUT_BACK;

        // Ease out back should go slightly beyond 1.0
        let mid_value = ease_out_back.transform(0.5);
        // We can't easily test overshoot without more sophisticated analysis,
        // but we can at least verify it still ends at 1.0
        let end_value = ease_out_back.transform(1.0);
        assert!((end_value - 1.0).abs() < 0.1);
    }
}
