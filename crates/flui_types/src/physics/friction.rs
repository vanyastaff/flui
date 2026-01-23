//! Friction-based physics simulations
//!
//! This module provides simulations that model friction and deceleration.

use super::{Simulation, Tolerance};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrictionSimulation {
    /// The drag coefficient (higher = more friction)
    drag: f32,

    /// The starting position
    position_at_zero: f32,

    /// The initial velocity in pixels per second
    velocity_at_zero: f32,

    /// The tolerance for this simulation
    tolerance: Tolerance,
}

impl FrictionSimulation {
    #[must_use]
    pub fn new(drag: f32, position: f32, velocity: f32) -> Self {
        Self {
            drag,
            position_at_zero: position,
            velocity_at_zero: velocity,
            tolerance: Tolerance::default(),
        }
    }

    #[must_use]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    #[must_use]
    pub fn drag(&self) -> f32 {
        self.drag
    }

    #[must_use]
    pub fn start_position(&self) -> f32 {
        self.position_at_zero
    }

    #[must_use]
    pub fn initial_velocity(&self) -> f32 {
        self.velocity_at_zero
    }

    #[must_use]
    pub fn final_position(&self) -> f32 {
        self.position_at_zero + self.velocity_at_zero / self.drag
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.drag > 0.0
            && self.drag.is_finite()
            && self.position_at_zero.is_finite()
            && self.velocity_at_zero.is_finite()
            && self.tolerance.is_valid()
    }

    #[must_use]
    pub fn time_to_velocity(&self, target_velocity: f32) -> Option<f32> {
        if self.drag <= 0.0 || target_velocity.abs() > self.velocity_at_zero.abs() {
            return None;
        }

        if target_velocity == 0.0 {
            return Some(f32::INFINITY);
        }

        // v(t) = v₀ * e^(-k*t)
        // t = -ln(v/v₀) / k
        let ratio = target_velocity / self.velocity_at_zero;
        if ratio <= 0.0 || ratio > 1.0 {
            return None;
        }

        Some(-ratio.ln() / self.drag)
    }

    #[must_use]
    pub fn distance_to_velocity(&self, target_velocity: f32) -> f32 {
        if let Some(time) = self.time_to_velocity(target_velocity) {
            self.position(time) - self.position_at_zero
        } else {
            self.final_position() - self.position_at_zero
        }
    }

    #[must_use]
    pub fn deceleration(&self, time: f32) -> f32 {
        -self.drag * self.velocity(time)
    }
}

impl Simulation for FrictionSimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        self.position_at_zero
            + self.velocity_at_zero * (1.0 - (-self.drag * time).exp()) / self.drag
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        self.velocity_at_zero * (-self.drag * time).exp()
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        self.velocity(time).abs() < self.tolerance.velocity
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BoundedFrictionSimulation {
    /// The underlying friction simulation
    friction: FrictionSimulation,

    /// The boundary position
    boundary: f32,

    /// Whether we're going in the positive direction
    positive_direction: bool,
}

impl BoundedFrictionSimulation {
    #[must_use]
    pub fn new(drag: f32, position: f32, velocity: f32, boundary: f32) -> Self {
        Self {
            friction: FrictionSimulation::new(drag, position, velocity),
            boundary,
            positive_direction: velocity > 0.0,
        }
    }

    #[must_use]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.friction = self.friction.with_tolerance(tolerance);
        self
    }

    #[must_use]
    pub fn boundary(&self) -> f32 {
        self.boundary
    }

    #[must_use]
    pub fn inner(&self) -> &FrictionSimulation {
        &self.friction
    }

    #[must_use]
    pub fn will_hit_boundary(&self) -> bool {
        let final_pos = self.friction.final_position();
        if self.positive_direction {
            final_pos >= self.boundary
        } else {
            final_pos <= self.boundary
        }
    }

    #[must_use]
    pub fn is_at_boundary(&self, time: f32) -> bool {
        let pos = self.friction.position(time);
        if self.positive_direction {
            pos >= self.boundary
        } else {
            pos <= self.boundary
        }
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.friction.is_valid() && self.boundary.is_finite()
    }
}

impl Simulation for BoundedFrictionSimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        let pos = self.friction.position(time);
        if self.positive_direction {
            pos.min(self.boundary)
        } else {
            pos.max(self.boundary)
        }
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        if self.is_at_boundary(time) {
            0.0
        } else {
            self.friction.velocity(time)
        }
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        self.is_at_boundary(time) || self.friction.is_done(time)
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.friction.tolerance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

}
