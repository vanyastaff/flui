//! Frame-rate-independent smoothing primitives.
//!
//! Two building blocks:
//!
//! - [`exp_decay`] / [`Smoothed`]: exponential decay toward a moving target,
//!   parameterized by **half-life**. The classic `lerp(a, b, f)`-per-frame
//!   pattern converges at a rate that depends on the frame count, not on
//!   elapsed time — a 120 Hz device animates twice as fast as a 60 Hz one.
//!   `b + (a - b) * exp(-λ·dt)` is the closed-form fix (Freya Holmér,
//!   "lerp smoothing is broken", <https://www.youtube.com/watch?v=LSNQuFEDOyQ>).
//! - [`SmoothDamp`]: critically-damped spring approximation with a maximum
//!   speed clamp and overshoot guard, after Unity's `Mathf.SmoothDamp`
//!   (Game Programming Gems 4, ch. 1.10). The workhorse for "camera follows
//!   target" / "handle follows finger" motion that must never oscillate.
//!
//! Both are pure functions of `(state, target, dt)` — no allocation, no
//! locking, usable from any tick path.

/// Exponential decay from `current` toward `target` over `dt` seconds.
///
/// `lambda` is the decay rate in 1/seconds: the remaining distance shrinks by
/// `e^-lambda` every second. Prefer [`exp_decay_half_life`] for a tunable in
/// human units.
///
/// Frame-rate independent: two devices stepping with different `dt` sequences
/// that sum to the same elapsed time land on the same value.
#[inline]
#[must_use]
pub fn exp_decay(current: f64, target: f64, lambda: f64, dt: f64) -> f64 {
    target + (current - target) * (-lambda * dt).exp()
}

/// [`exp_decay`] parameterized by **half-life**: the time (seconds) for the
/// remaining distance to the target to halve.
///
/// `half_life <= 0` snaps to the target (interpreted as "no smoothing").
#[inline]
#[must_use]
pub fn exp_decay_half_life(current: f64, target: f64, half_life: f64, dt: f64) -> f64 {
    if half_life <= 0.0 {
        return target;
    }
    exp_decay(current, target, core::f64::consts::LN_2 / half_life, dt)
}

/// A value that follows a (possibly moving) target with exponential decay.
///
/// Thin stateful wrapper over [`exp_decay_half_life`] for the common
/// "smoothed cursor / smoothed scroll indicator" pattern:
///
/// ```
/// use flui_animation::smoothing::Smoothed;
///
/// let mut s = Smoothed::new(0.0, 0.1); // 100 ms half-life
/// s.set_target(10.0);
/// s.tick(0.1);
/// assert!((s.value() - 5.0).abs() < 1e-4); // one half-life -> half the gap
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Smoothed {
    value: f64,
    target: f64,
    half_life: f64,
}

impl Smoothed {
    /// Create at `value`, targeting itself (at rest), with the given
    /// half-life in seconds.
    #[must_use]
    pub fn new(value: f64, half_life: f64) -> Self {
        Self {
            value,
            target: value,
            half_life,
        }
    }

    /// Current smoothed value.
    #[inline]
    #[must_use]
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Current target.
    #[inline]
    #[must_use]
    pub fn target(&self) -> f64 {
        self.target
    }

    /// Retarget without disturbing the current value (the decay follows).
    #[inline]
    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    /// Change the half-life (seconds).
    #[inline]
    pub fn set_half_life(&mut self, half_life: f64) {
        self.half_life = half_life;
    }

    /// Snap to a value and stop (target = value).
    #[inline]
    pub fn snap_to(&mut self, value: f64) {
        self.value = value;
        self.target = value;
    }

    /// Advance by `dt` seconds; returns the new value.
    #[inline]
    pub fn tick(&mut self, dt: f64) -> f64 {
        self.value = exp_decay_half_life(self.value, self.target, self.half_life, dt);
        self.value
    }

    /// Whether the value is within `tolerance` of the target.
    #[inline]
    #[must_use]
    pub fn is_settled(&self, tolerance: f64) -> bool {
        (self.value - self.target).abs() <= tolerance
    }
}

/// Critically-damped spring follower with a maximum-speed clamp.
///
/// Port of Unity's `Mathf.SmoothDamp` (Game Programming Gems 4, ch. 1.10):
/// reaches the target smoothly without overshoot, in roughly `smooth_time`
/// seconds, never moving faster than `max_speed`. Unlike [`exp_decay`] it
/// carries velocity state, so a retarget mid-flight is C¹-continuous (no
/// kink in the motion).
///
/// The inner `e^-x` uses the GPG4 Padé-style polynomial
/// `1/(1 + x + 0.48x² + 0.235x³)` — accurate for `x = 2·dt/smooth_time` in
/// `[0, ~3]`; the max-speed clamp and overshoot guard bound the error
/// outside that range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmoothDamp {
    velocity: f64,
    /// Approximate time (seconds) to reach the target. Smaller = stiffer.
    pub smooth_time: f64,
    /// Maximum speed in value-units/second. `f64::INFINITY` disables the clamp.
    pub max_speed: f64,
}

impl SmoothDamp {
    /// Create with the given smooth-time (seconds) and no speed limit.
    #[must_use]
    pub fn new(smooth_time: f64) -> Self {
        Self {
            velocity: 0.0,
            smooth_time,
            max_speed: f64::INFINITY,
        }
    }

    /// Builder: cap the follow speed (value-units per second).
    #[must_use]
    pub fn with_max_speed(mut self, max_speed: f64) -> Self {
        self.max_speed = max_speed;
        self
    }

    /// Current velocity (value-units per second).
    #[inline]
    #[must_use]
    pub fn velocity(&self) -> f64 {
        self.velocity
    }

    /// Reset velocity to zero (e.g. after a hard snap).
    #[inline]
    pub fn reset(&mut self) {
        self.velocity = 0.0;
    }

    /// Advance `current` toward `target` by `dt` seconds; returns the new
    /// position. Velocity state is updated in place.
    #[must_use]
    pub fn step(&mut self, current: f64, target: f64, dt: f64) -> f64 {
        // Guard degenerate inputs: a zero/negative smooth_time means "snap".
        let smooth_time = self.smooth_time.max(0.0001);
        let omega = 2.0 / smooth_time;
        let x = omega * dt;
        // GPG4 polynomial approximation of e^-x (see type docs).
        let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);

        let mut change = current - target;
        let original_target = target;

        // Clamp maximum follow speed.
        let max_change = self.max_speed * smooth_time;
        if max_change.is_finite() {
            change = change.clamp(-max_change, max_change);
        }
        let target = current - change;

        let temp = (self.velocity + omega * change) * dt;
        self.velocity = (self.velocity - omega * temp) * exp;
        let mut output = target + (change + temp) * exp;

        // Overshoot guard: the polynomial approximation can cross the target
        // for large x; clamp to the target and zero the outward velocity.
        if (original_target - current > 0.0) == (output > original_target) {
            output = original_target;
            self.velocity = 0.0;
        }
        output
    }
}
