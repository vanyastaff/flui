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
//! The filters smooth pointer coordinates an application consumes directly.
//! Recognizers apply the same filter after recognition when
//! [`GestureSettings::position_smoothing`](crate::gesture::GestureSettings::position_smoothing)
//! selects a tuning: slop, arena and velocity decisions keep reading the raw
//! samples, so smoothing never delays or biases recognition, and each accepted
//! gesture starts its smoothing afresh from an unsmoothed position.
//!
//! Smoothed after acceptance: drag (every axis) and multi-drag update
//! positions and deltas, tap-and-drag drag updates, long-press move updates,
//! force-press update and peak positions, and the scale focal point (restarted
//! whenever its contact set changes). Not smoothed: start positions (the raw
//! acceptance sample), end positions and every velocity, which stay the
//! observed values, and native trackpad pan-zoom, which the platform reports
//! already filtered.
//!
//! # Tuning (from the paper)
//!
//! 1. Set `beta = 0`, adjust `min_cutoff` until slow-motion jitter is gone.
//! 2. Increase `beta` until fast motion lags acceptably little.
//!
//! [`OneEuroTuning::default`] is `min_cutoff = 1.0 Hz`, `beta = 0.007` and
//! `d_cutoff = 1.0 Hz`. Tune them for the source units and required response;
//! these values are application defaults, not a device-independent guarantee,
//! which is why no platform preset selects smoothing (ADR-0172, ADR-0212 §4).
//!
//! Tuning is validated once, by [`OneEuroTuning::try_new`], and cannot be mutated
//! into an invalid state. Filtering rejects non-finite input and
//! unrepresentable arithmetic without changing the last accepted sample or
//! derivative. This is preferable to silently saturating a slope or changing
//! the smoothing policy. Duplicate samples hold the previous result; the 2D
//! wrapper also holds its clock anchor for regressed timestamps. No prediction
//! is performed.

use web_time::Instant;

use flui_foundation::geometry::Offset;

/// Smoothing factor for a first-order low-pass at cutoff `fc` (Hz) and
/// sampling period `te` (seconds).
#[inline]
fn smoothing_alpha(fc: f64, te: f64) -> f64 {
    // Apply TAU to the smaller factor first: neither a huge cutoff paired
    // with a short interval nor a subnormal cutoff multiplied by a fraction
    // loses its representable contribution. A genuinely overflowing positive
    // product has a smoothing-factor limit of one.
    let (smaller, larger) = if fc < te { (fc, te) } else { (te, fc) };
    let r = (core::f64::consts::TAU * smaller) * larger;
    if r.is_infinite() { 1.0 } else { r / (1.0 + r) }
}

/// Preserve constant signals and endpoint limits without overflowing the
/// difference between opposite-sign extremes.
fn low_pass(previous: f64, next: f64, alpha: f64) -> f64 {
    if alpha == 0.0 {
        previous
    } else if alpha == 1.0 {
        next
    } else if previous.is_sign_negative() == next.is_sign_negative() {
        alpha.mul_add(next - previous, previous)
    } else {
        alpha * next + (1.0 - alpha) * previous
    }
}

/// Invalid tuning, input, or arithmetic rejected without changing filter state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OneEuroFilterError {
    /// The minimum cutoff must be finite and strictly positive.
    #[error("minimum cutoff must be finite and strictly positive")]
    InvalidMinCutoff,
    /// The speed coefficient must be finite and nonnegative.
    #[error("speed coefficient must be finite and nonnegative")]
    InvalidBeta,
    /// The derivative cutoff must be finite and strictly positive.
    #[error("derivative cutoff must be finite and strictly positive")]
    InvalidDerivativeCutoff,
    /// Every scalar or position component must be finite.
    #[error("sample must be finite")]
    NonFiniteSample,
    /// An explicit sampling interval must be finite and nonnegative.
    #[error("sampling interval must be finite and nonnegative")]
    InvalidInterval,
    /// A slope, filtered derivative, adaptive cutoff, or output is unrepresentable.
    #[error("filter arithmetic exceeds the finite representable range")]
    ArithmeticOverflow,
}

/// Validated 1€ parameters: a plain value a filter or a
/// [`GestureSettings`](crate::gesture::GestureSettings) profile carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OneEuroTuning {
    /// Minimum cutoff frequency (Hz). Lower = less jitter, more lag at rest.
    min_cutoff: f64,
    /// Speed coefficient. Higher = less lag during fast motion.
    beta: f64,
    /// Cutoff for the derivative low-pass (Hz). Rarely tuned; 1 Hz default.
    d_cutoff: f64,
}

impl Default for OneEuroTuning {
    fn default() -> Self {
        Self {
            min_cutoff: 1.0,
            beta: 0.007,
            d_cutoff: 1.0,
        }
    }
}

impl OneEuroTuning {
    /// Explicit parameters (see the module docs for tuning).
    ///
    /// # Errors
    ///
    /// Cutoffs must be finite and strictly positive; `beta` must be finite
    /// and nonnegative.
    pub fn try_new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Result<Self, OneEuroFilterError> {
        if !min_cutoff.is_finite() || min_cutoff <= 0.0 {
            return Err(OneEuroFilterError::InvalidMinCutoff);
        }
        if !beta.is_finite() || beta < 0.0 {
            return Err(OneEuroFilterError::InvalidBeta);
        }
        if !d_cutoff.is_finite() || d_cutoff <= 0.0 {
            return Err(OneEuroFilterError::InvalidDerivativeCutoff);
        }
        Ok(Self {
            min_cutoff,
            beta,
            d_cutoff,
        })
    }

    /// Minimum cutoff frequency in hertz.
    #[must_use]
    pub const fn min_cutoff(&self) -> f64 {
        self.min_cutoff
    }

    /// Speed coefficient.
    #[must_use]
    pub const fn beta(&self) -> f64 {
        self.beta
    }

    /// Derivative cutoff frequency in hertz.
    #[must_use]
    pub const fn d_cutoff(&self) -> f64 {
        self.d_cutoff
    }
}

/// One-dimensional 1€ filter.
///
/// Instantiate one per axis (the filter is scalar); see [`OneEuroFilter2D`]
/// for the pointer-position convenience wrapper.
#[derive(Debug, Clone, Copy, Default)]
pub struct OneEuroFilter {
    tuning: OneEuroTuning,
    /// Last filtered value (`x̂`).
    x_prev: Option<f64>,
    /// Last filtered derivative (`dx̂`), units/second.
    dx_prev: f64,
}

impl OneEuroFilter {
    /// Create a filter with validated parameters. Construct a new filter to
    /// change tuning.
    #[must_use]
    pub const fn new(tuning: OneEuroTuning) -> Self {
        Self {
            tuning,
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
    /// Zero `te` (duplicate timestamp) returns the previous filtered
    /// value without state corruption.
    ///
    /// # Errors
    ///
    /// Rejects non-finite samples, non-finite or negative intervals, and
    /// unrepresentable slopes, derivatives, cutoffs or results. An error
    /// leaves all accepted history unchanged, including before initialization.
    pub fn filter(&mut self, x: f64, te: f64) -> Result<f64, OneEuroFilterError> {
        if !x.is_finite() {
            return Err(OneEuroFilterError::NonFiniteSample);
        }
        if !te.is_finite() || te < 0.0 {
            return Err(OneEuroFilterError::InvalidInterval);
        }
        let Some(x_prev) = self.x_prev else {
            self.x_prev = Some(x);
            self.dx_prev = 0.0;
            return Ok(x);
        };
        if te == 0.0 {
            return Ok(x_prev);
        }

        // Filtered derivative: raw slope against the PREVIOUS FILTERED value
        // (per the paper — using the raw previous sample would re-amplify
        // the very noise being removed).
        let difference = x - x_prev;
        // An opposite-sign difference may overflow while the slope remains
        // representable after division by a long interval. Scale each operand
        // first on that path; an actually unrepresentable slope is still refused.
        let dx = if difference.is_finite() {
            difference / te
        } else {
            x / te - x_prev / te
        };
        if !dx.is_finite() {
            return Err(OneEuroFilterError::ArithmeticOverflow);
        }
        let a_d = smoothing_alpha(self.tuning.d_cutoff, te);
        let dx_hat = low_pass(self.dx_prev, dx, a_d);
        if !dx_hat.is_finite() {
            return Err(OneEuroFilterError::ArithmeticOverflow);
        }

        // Speed-adaptive cutoff.
        let fc = self.tuning.min_cutoff + self.tuning.beta * dx_hat.abs();
        if !fc.is_finite() {
            return Err(OneEuroFilterError::ArithmeticOverflow);
        }
        let a = smoothing_alpha(fc, te);
        let x_hat = low_pass(x_prev, x, a);
        if !x_hat.is_finite() {
            return Err(OneEuroFilterError::ArithmeticOverflow);
        }

        self.x_prev = Some(x_hat);
        self.dx_prev = dx_hat;
        Ok(x_hat)
    }
}

/// Two-dimensional 1€ filter for pointer positions.
///
/// Owns an X and a Y [`OneEuroFilter`] plus the previous timestamp, so the
/// caller only feeds `(time, position)` pairs:
///
/// ```
/// use std::time::Duration;
///
/// use flui_interaction::gesture::OneEuroFilter2D;
/// use flui_interaction::{MonotonicClock, SystemClock};
/// use flui_foundation::geometry::Offset;
///
/// let mut filter = OneEuroFilter2D::default();
/// let t0 = SystemClock.now();
/// let p0 = filter.filter(t0, Offset::new(10.0, 10.0)).expect("finite sample");
/// assert_eq!(p0.dx, 10.0); // first sample passes through
/// let _p1 = filter.filter(
///     t0 + Duration::from_millis(8),
///     Offset::new(10.4, 9.8), // sensor jitter
/// ).expect("finite jitter sample");
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct OneEuroFilter2D {
    x: OneEuroFilter,
    y: OneEuroFilter,
    last_time: Option<Instant>,
}

impl OneEuroFilter2D {
    /// Create with validated parameters applied to both axes.
    #[must_use]
    pub const fn new(tuning: OneEuroTuning) -> Self {
        Self {
            x: OneEuroFilter::new(tuning),
            y: OneEuroFilter::new(tuning),
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
    ///
    /// Duplicate or regressed timestamps hold the previous position and keep
    /// the previous accepted timestamp. Reset starts a new independent timeline.
    ///
    /// # Errors
    ///
    /// Invalid components or arithmetic on either axis leave both axes and
    /// the accepted timestamp unchanged, as in [`OneEuroFilter::filter`].
    pub fn filter(
        &mut self,
        time: Instant,
        position: Offset<f64>,
    ) -> Result<Offset<f64>, OneEuroFilterError> {
        if !position.dx.is_finite() || !position.dy.is_finite() {
            return Err(OneEuroFilterError::NonFiniteSample);
        }
        if self.last_time.is_some_and(|last| time <= last) {
            return Ok(Offset::new(
                self.x
                    .x_prev
                    .expect("BUG: accepted timestamp has an X sample"),
                self.y
                    .x_prev
                    .expect("BUG: accepted timestamp has a Y sample"),
            ));
        }
        let te = self.last_time.map_or(0.0, |last| {
            time.saturating_duration_since(last).as_secs_f64()
        });
        let mut x = self.x;
        let mut y = self.y;
        let filtered = Offset::new(x.filter(position.dx, te)?, y.filter(position.dy, te)?);
        self.x = x;
        self.y = y;
        self.last_time = Some(time);
        Ok(filtered)
    }
}

/// The local position one contact last published, and the post-recognition
/// smoothing that shapes what it publishes next.
///
/// Before acceptance the contact [`follow`](Self::follow)s its raw samples;
/// [`begin`](Self::begin) starts smoothing from the acceptance sample, which
/// passes through unsmoothed; [`publish`](Self::publish) then yields each
/// position to publish and its movement since the previous one, so published
/// deltas always add up to the published position.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PublishedPosition {
    smoother: Option<PositionSmoother>,
    local: Offset<f64>,
}

impl PublishedPosition {
    pub(crate) fn new(smoothing: Option<OneEuroTuning>, local: Offset<f64>) -> Self {
        Self {
            smoother: smoothing.map(PositionSmoother::new),
            local,
        }
    }

    /// Track a raw sample the contact does not publish.
    pub(crate) fn follow(&mut self, local: Offset<f64>) {
        self.local = local;
    }

    /// Start smoothing from the acceptance sample.
    pub(crate) fn begin(&mut self, time: Instant, local: Offset<f64>, global: Offset<f64>) {
        if let Some(smoother) = self.smoother.as_mut() {
            smoother.reset();
            smoother.smooth(time, local, global);
        }
        self.local = local;
    }

    /// The `(local, global)` position to publish for an accepted sample and
    /// its local movement since the previously published position. Raw
    /// without smoothing.
    pub(crate) fn publish(
        &mut self,
        time: Instant,
        local: Offset<f64>,
        global: Offset<f64>,
    ) -> (Offset<f64>, Offset<f64>, Offset<f64>) {
        let (local, global) = match self.smoother.as_mut() {
            Some(smoother) => smoother.smooth(time, local, global),
            None => (local, global),
        };
        let delta = local - std::mem::replace(&mut self.local, local);
        (local, global, delta)
    }
}

/// Post-recognition smoothing of the position one accepted gesture reports,
/// in the receiving node's and the root's coordinates.
///
/// A recognizer feeds it only the samples it publishes after acceptance; its
/// slop, arena and velocity decisions keep the raw samples.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PositionSmoother {
    filters: PositionFilters,
    /// The latest sample's time and the filters as they were before it: a
    /// later sample at the same time (another contact of one multi-touch
    /// frame) replaces that sample instead of being held as a duplicate.
    revisable: Option<(Instant, PositionFilters)>,
}

#[derive(Debug, Clone, Copy)]
struct PositionFilters {
    local: OneEuroFilter2D,
    global: OneEuroFilter2D,
}

impl PositionSmoother {
    pub(crate) const fn new(tuning: OneEuroTuning) -> Self {
        Self {
            filters: PositionFilters {
                local: OneEuroFilter2D::new(tuning),
                global: OneEuroFilter2D::new(tuning),
            },
            revisable: None,
        }
    }

    /// Forget the history, so the next sample passes through unchanged.
    pub(crate) fn reset(&mut self) {
        self.filters.local.reset();
        self.filters.global.reset();
        self.revisable = None;
    }

    /// Smooth a `(local, global)` sample. The first sample after construction
    /// or [`reset`](Self::reset) passes through unchanged, and the last sample
    /// at one timestamp is the one that counts. A sample the filter refuses
    /// restarts smoothing from it: the raw position is published, never a
    /// non-finite one.
    pub(crate) fn smooth(
        &mut self,
        time: Instant,
        local: Offset<f64>,
        global: Offset<f64>,
    ) -> (Offset<f64>, Offset<f64>) {
        let before = match self.revisable {
            Some((latest, before)) if latest == time => before,
            _ => self.filters,
        };
        let mut next = before;
        if let (Ok(smoothed_local), Ok(smoothed_global)) = (
            next.local.filter(time, local),
            next.global.filter(time, global),
        ) {
            self.filters = next;
            self.revisable = Some((time, before));
            return (smoothed_local, smoothed_global);
        }
        self.reset();
        let before = self.filters;
        // A reset filter admits any finite sample as its first.
        let _ = self.filters.local.filter(time, local);
        let _ = self.filters.global.filter(time, global);
        self.revisable = Some((time, before));
        (local, global)
    }
}
