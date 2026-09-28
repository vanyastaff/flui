//! One Euro Filter — adaptive low-pass filtering for noisy pointer input.
//!
//! Implements Casiez, Roussel & Vogel, *"1€ Filter: A Simple Speed-based
//! Low-pass Filter for Noisy Input in Interactive Systems"*, CHI 2012
//! (<https://gery.casiez.net/1euro/>). The filter adapts its cutoff to the
//! signal's speed:
//!
//! ```text
//! α(fc, Te) = 2π·fc·Te / (1 + 2π·fc·Te)
//! fc        = mincutoff + β·|dx̂|
//! ```
//!
//! - at low speeds a low cutoff suppresses jitter (stylus hover, slow drag);
//! - at high speeds the cutoff rises with the filtered derivative, so fast
//!   strokes track with minimal lag.
//!
//! This is the standard smoothing filter for stylus/ink, AR/VR pointing, and
//! cursor stabilization. Neither Flutter nor any Rust UI framework ships
//! one in its input pipeline.
//!
//! # Tuning (from the paper)
//!
//! 1. Set `beta = 0`, adjust `min_cutoff` until slow-motion jitter is gone.
//! 2. Increase `beta` until fast motion lags acceptably little.
//!
//! Defaults here (`min_cutoff = 1.0 Hz`, `beta = 0.007`, `d_cutoff = 1.0 Hz`)
//! are the paper's recommended starting point for 60–120 Hz pointer input.

use web_time::Instant;

use flui_types::geometry::Offset;

/// Smoothing factor for a first-order low-pass at cutoff `fc` (Hz) and
/// sampling period `te` (seconds).
#[inline]
fn smoothing_alpha(fc: f64, te: f64) -> f64 {
    let r = 2.0 * core::f64::consts::PI * fc * te;
    r / (1.0 + r)
}

/// One-dimensional 1€ filter.
///
/// Instantiate one per axis (the filter is scalar); see [`OneEuroFilter2D`]
/// for the pointer-position convenience wrapper.
#[derive(Debug, Clone, Copy)]
pub struct OneEuroFilter {
    /// Minimum cutoff frequency (Hz). Lower = less jitter, more lag at rest.
    pub min_cutoff: f64,
    /// Speed coefficient. Higher = less lag during fast motion.
    pub beta: f64,
    /// Cutoff for the derivative low-pass (Hz). Rarely tuned; 1 Hz default.
    pub d_cutoff: f64,
    /// Last filtered value (`x̂`).
    x_prev: Option<f64>,
    /// Last filtered derivative (`dx̂`), units/second.
    dx_prev: f64,
}

impl Default for OneEuroFilter {
    fn default() -> Self {
        Self::new(1.0, 0.007, 1.0)
    }
}

impl OneEuroFilter {
    /// Create a filter with explicit parameters (see module docs for tuning).
    #[must_use]
    pub fn new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
            x_prev: None,
            dx_prev: 0.0,
        }
    }

    /// Reset the filter state (e.g. on pointer-down of a new stroke).
    pub fn reset(&mut self) {
        self.x_prev = None;
        self.dx_prev = 0.0;
    }

    /// Filter a sample taken `te` seconds after the previous one.
    ///
    /// The first sample initializes the filter and is returned unchanged.
    /// Non-positive `te` (duplicate timestamp) returns the previous filtered
    /// value without state corruption.
    pub fn filter(&mut self, x: f64, te: f64) -> f64 {
        let Some(x_prev) = self.x_prev else {
            self.x_prev = Some(x);
            self.dx_prev = 0.0;
            return x;
        };
        if te <= 0.0 {
            return x_prev;
        }

        // Filtered derivative: raw slope against the PREVIOUS FILTERED value
        // (per the paper — using the raw previous sample would re-amplify
        // the very noise being removed).
        let dx = (x - x_prev) / te;
        let a_d = smoothing_alpha(self.d_cutoff, te);
        let dx_hat = a_d * dx + (1.0 - a_d) * self.dx_prev;

        // Speed-adaptive cutoff.
        let fc = self.min_cutoff + self.beta * dx_hat.abs();
        let a = smoothing_alpha(fc, te);
        let x_hat = a * x + (1.0 - a) * x_prev;

        self.x_prev = Some(x_hat);
        self.dx_prev = dx_hat;
        x_hat
    }
}

/// Two-dimensional 1€ filter for pointer positions.
///
/// Owns an X and a Y [`OneEuroFilter`] plus the previous timestamp, so the
/// caller only feeds `(time, position)` pairs:
///
/// ```
/// use std::time::{Duration, Instant};
///
/// use flui_interaction::processing::OneEuroFilter2D;
/// use flui_types::geometry::Offset;
///
/// let mut filter = OneEuroFilter2D::default();
/// let t0 = Instant::now();
/// let p0 = filter.filter(t0, Offset::new(10.0, 10.0));
/// assert_eq!(p0.dx, 10.0); // first sample passes through
/// let _p1 = filter.filter(
///     t0 + Duration::from_millis(8),
///     Offset::new(10.4, 9.8), // sensor jitter
/// );
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct OneEuroFilter2D {
    x: OneEuroFilter,
    y: OneEuroFilter,
    last_time: Option<Instant>,
}

impl OneEuroFilter2D {
    /// Create with explicit parameters applied to both axes.
    #[must_use]
    pub fn new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Self {
        Self {
            x: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            y: OneEuroFilter::new(min_cutoff, beta, d_cutoff),
            last_time: None,
        }
    }

    /// Reset both axes (new stroke).
    pub fn reset(&mut self) {
        self.x.reset();
        self.y.reset();
        self.last_time = None;
    }

    /// Filter a timestamped position sample.
    pub fn filter(&mut self, time: Instant, position: Offset<f64>) -> Offset<f64> {
        let te = self.last_time.map_or(0.0, |last| {
            time.saturating_duration_since(last).as_secs_f64()
        });
        self.last_time = Some(time);
        Offset::new(
            self.x.filter(position.dx, te),
            self.y.filter(position.dy, te),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn first_sample_passes_through() {
        let mut f = OneEuroFilter::default();
        assert_eq!(f.filter(42.0, 0.008), 42.0);
    }

    #[test]
    fn static_jitter_is_suppressed() {
        // Small oscillation around 100: the filtered output's wobble must be
        // much smaller than the raw ±1 px jitter.
        let mut f = OneEuroFilter::default();
        let te = 1.0 / 120.0;
        let _ = f.filter(100.0, te);
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        for i in 0..240 {
            let noise = if i % 2 == 0 { 1.0 } else { -1.0 };
            let out = f.filter(100.0 + noise, te);
            if i > 60 {
                min = min.min(out);
                max = max.max(out);
            }
        }
        let wobble = max - min;
        assert!(
            wobble < 0.4,
            "±1 px alternating jitter must be attenuated, residual wobble {wobble}"
        );
    }

    #[test]
    fn fast_motion_tracks_with_bounded_lag() {
        // A fast linear ramp (2000 px/s): the speed-adaptive cutoff must keep
        // lag small relative to motion per-frame.
        let mut f = OneEuroFilter::default();
        let te = 1.0 / 120.0;
        let mut x = 0.0_f64;
        let mut out = 0.0_f64;
        for _ in 0..120 {
            x += 2000.0 * te;
            out = f.filter(x, te);
        }
        let lag = x - out;
        assert!(
            lag < 2000.0 * te * 4.0,
            "lag {lag} px must stay within a few frames of motion at speed"
        );
    }

    #[test]
    fn duplicate_timestamp_returns_previous() {
        let mut f = OneEuroFilter::default();
        let a = f.filter(10.0, 0.008);
        let b = f.filter(999.0, 0.0);
        assert_eq!(a, b, "zero-dt sample must not corrupt state");
    }

    #[test]
    fn two_d_wrapper_filters_both_axes() {
        let mut f = OneEuroFilter2D::default();
        let t0 = Instant::now();
        let p0 = f.filter(t0, Offset::new(0.0, 0.0));
        assert_eq!(p0, Offset::new(0.0, 0.0));

        // Jittery samples around (50, 50) settle near (50, 50).
        let mut last = p0;
        for i in 1..120 {
            let jitter = if i % 2 == 0 { 0.8 } else { -0.8 };
            last = f.filter(
                t0 + Duration::from_millis(8 * i),
                Offset::new(50.0 + jitter, 50.0 - jitter),
            );
        }
        assert!((last.dx - 50.0).abs() < 1.0);
        assert!((last.dy - 50.0).abs() < 1.0);
    }
}
