//! Interruptible, velocity-preserving spring animation.
//!
//! [`AnimatedValue<T>`] animates any [`TwoWayConverter`] type with an
//! independent spring per scalar component. Re-targeting mid-flight
//! ([`AnimatedValue::animate_to`]) re-seeds each component spring from its
//! *current position and velocity*, so motion is continuous — no snap, no
//! restart. FLUI's spring simulation returns the analytic velocity `dx(t)`
//! exactly, so this hand-off is O(1) and exact, unlike libraries that
//! numerically sample velocity.
//!
//! This is the engine primitive. It advances on an externally supplied `dt`
//! (the widget layer drives it from a ticker); it owns no ticker or listeners,
//! which keeps it trivially testable and composable.

use crate::simulation::{Simulation, SpringDescription, SpringSimulation};
use flui_types::geometry::{Offset, Pixels, Size, px};
use flui_types::styling::Color;
use smallvec::SmallVec;

/// A value that can be decomposed into, and rebuilt from, a fixed-width vector
/// of scalar components, so each component can be animated by its own spring.
///
/// Mirrors the role of Jetpack Compose's `TwoWayConverter`. Implement it (or, in
/// future, derive it) for any type you want to spring-animate.
pub trait TwoWayConverter: Clone {
    /// The scalar-component representation, e.g. `[f32; 4]` for an RGBA color.
    /// `Copy` so it can be used as a scratch buffer; `AsRef`/`AsMut<[f32]>` so
    /// the spring core can iterate components generically.
    type Vector: AsRef<[f32]> + AsMut<[f32]> + Copy;

    /// Decompose into scalar components.
    fn to_vector(&self) -> Self::Vector;

    /// Rebuild from scalar components.
    fn from_vector(v: Self::Vector) -> Self;
}

impl TwoWayConverter for f32 {
    type Vector = [f32; 1];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [*self]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        v[0]
    }
}

impl TwoWayConverter for Offset<Pixels> {
    type Vector = [f32; 2];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [self.dx.get(), self.dy.get()]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        Offset::new(px(v[0]), px(v[1]))
    }
}

impl TwoWayConverter for Size<Pixels> {
    type Vector = [f32; 2];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [self.width.get(), self.height.get()]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        Size::new(px(v[0]), px(v[1]))
    }
}

impl TwoWayConverter for Color {
    type Vector = [f32; 4];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [
            f32::from(self.r),
            f32::from(self.g),
            f32::from(self.b),
            f32::from(self.a),
        ]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        // The `clamp(0.0, 255.0).round()` pins the value into the exact u8 range
        // before the cast, so the truncation/sign-loss lints do not apply.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let to_u8 = |c: f32| c.clamp(0.0, 255.0).round() as u8;
        Color::rgba(to_u8(v[0]), to_u8(v[1]), to_u8(v[2]), to_u8(v[3]))
    }
}

/// An interruptible spring-animated value. See the module docs.
///
/// Advance it with [`advance`](Self::advance), read it with [`value`](Self::value),
/// retarget it with [`animate_to`](Self::animate_to), and check for rest with
/// [`is_settled`](Self::is_settled).
#[derive(Debug, Clone)]
pub struct AnimatedValue<T: TwoWayConverter> {
    spring: SpringDescription,
    /// One spring per scalar component (inline for the common 1–4 component case).
    components: SmallVec<[SpringSimulation; 4]>,
    /// The current target value (also serves as a correctly-sized scratch buffer).
    target: T,
    /// Seconds elapsed since the most recent (re)target.
    elapsed: f32,
}

impl<T: TwoWayConverter> AnimatedValue<T> {
    /// Create a value resting at `initial`, animated by `spring`.
    #[must_use]
    pub fn new(initial: T, spring: SpringDescription) -> Self {
        let vector = initial.to_vector();
        let components = vector
            .as_ref()
            .iter()
            .map(|&c| SpringSimulation::new(spring, c, c, 0.0).with_snap_to_end(true))
            .collect();
        Self {
            spring,
            components,
            target: initial,
            elapsed: 0.0,
        }
    }

    /// Retarget toward `target`, preserving each component's current velocity.
    ///
    /// Each component spring is re-seeded from its analytic position and
    /// velocity at the current time, so an in-flight animation flows into the
    /// new one without snapping or losing momentum.
    pub fn animate_to(&mut self, target: T) {
        let goal = target.to_vector();
        for (sim, &goal_c) in self.components.iter_mut().zip(goal.as_ref()) {
            let position = sim.x(self.elapsed);
            let velocity = sim.dx(self.elapsed);
            *sim = SpringSimulation::new(self.spring, position, goal_c, velocity)
                .with_snap_to_end(true);
        }
        self.elapsed = 0.0;
        self.target = target;
    }

    /// Jump immediately to `value`, cancelling any motion (zero velocity).
    pub fn set_value(&mut self, value: T) {
        let v = value.to_vector();
        for (sim, &c) in self.components.iter_mut().zip(v.as_ref()) {
            *sim = SpringSimulation::new(self.spring, c, c, 0.0).with_snap_to_end(true);
        }
        self.elapsed = 0.0;
        self.target = value;
    }

    /// Advance time by `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        self.elapsed += dt;
    }

    /// The current animated value.
    #[must_use]
    pub fn value(&self) -> T {
        let mut buffer = self.target.to_vector();
        for (slot, sim) in buffer.as_mut().iter_mut().zip(&self.components) {
            *slot = sim.x(self.elapsed);
        }
        T::from_vector(buffer)
    }

    /// The current target value.
    #[must_use]
    pub fn target(&self) -> &T {
        &self.target
    }

    /// Whether every component spring has come to rest at its target.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.components.iter().all(|sim| sim.is_done(self.elapsed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spring() -> SpringDescription {
        SpringDescription::with_response_and_damping(0.3, 1.0)
    }

    #[test]
    fn settles_at_target() {
        let mut v = AnimatedValue::new(0.0_f32, spring());
        v.animate_to(100.0);
        // Advance well past the response period.
        for _ in 0..600 {
            v.advance(1.0 / 60.0);
        }
        assert!(v.is_settled(), "spring should settle");
        assert!((v.value() - 100.0).abs() < 0.5, "value={}", v.value());
    }

    #[test]
    fn retarget_preserves_velocity() {
        // Animate toward 100; midway (moving fast) retarget to 0. With velocity
        // preserved the value must briefly continue PAST its position toward 100
        // before the new spring pulls it back — momentum is not discarded.
        let mut v = AnimatedValue::new(0.0_f32, spring());
        v.animate_to(100.0);
        for _ in 0..6 {
            v.advance(1.0 / 60.0);
        }
        let position = v.value();
        assert!(
            position > 0.0 && position < 100.0,
            "mid-flight pos={position}"
        );

        v.animate_to(0.0);
        let v_after = {
            v.advance(1.0 / 60.0);
            v.value()
        };
        // Momentum carried it further from 0 than where it was when retargeted.
        assert!(
            v_after > position,
            "velocity not preserved: {v_after} should overshoot past {position}"
        );
    }

    #[test]
    fn per_component_color_spring() {
        let mut v = AnimatedValue::new(Color::rgba(0, 0, 0, 255), spring());
        v.animate_to(Color::rgba(255, 128, 0, 255));
        for _ in 0..600 {
            v.advance(1.0 / 60.0);
        }
        let c = v.value();
        assert!((i32::from(c.r) - 255).abs() <= 1);
        assert!((i32::from(c.g) - 128).abs() <= 1);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn apple_presets_build() {
        // Smoke: presets produce sensible, distinct springs.
        assert!(SpringDescription::smooth().damping_ratio() >= 1.0 - f32::EPSILON);
        assert!(SpringDescription::bouncy().damping_ratio() < 1.0);
        assert!(SpringDescription::snappy().damping_ratio() < 1.0);
    }
}
