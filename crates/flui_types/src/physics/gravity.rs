//! Gravity-based physics simulations
//!
//! This module provides simulations that model motion under gravity.

use super::{Simulation, Tolerance};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GravitySimulation {
    /// The acceleration due to gravity (in pixels per second squared)
    acceleration: f32,

    /// The starting position
    start: f32,

    /// The ending position (where the simulation should stop)
    end: f32,

    /// The initial velocity
    initial_velocity: f32,

    /// The tolerance for this simulation
    tolerance: Tolerance,
}

impl GravitySimulation {
    #[must_use]
    pub fn new(acceleration: f32, start: f32, end: f32, velocity: f32) -> Self {
        Self {
            acceleration,
            start,
            end,
            initial_velocity: velocity,
            tolerance: Tolerance::default(),
        }
    }

    #[must_use]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    #[must_use]
    pub fn acceleration(&self) -> f32 {
        self.acceleration
    }

    #[must_use]
    pub fn start(&self) -> f32 {
        self.start
    }

    #[must_use]
    pub fn end(&self) -> f32 {
        self.end
    }

    #[must_use]
    pub fn initial_velocity(&self) -> f32 {
        self.initial_velocity
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.acceleration.is_finite()
            && self.start.is_finite()
            && self.end.is_finite()
            && self.initial_velocity.is_finite()
            && self.tolerance.is_valid()
    }

    #[must_use]
    pub fn time_at_end(&self) -> Option<f32> {
        let distance = self.end - self.start;

        // Using quadratic formula: position = start + velocity*t + 0.5*acceleration*t^2
        // distance = velocity*t + 0.5*acceleration*t^2
        // 0.5*a*t^2 + v*t - distance = 0

        let a = 0.5 * self.acceleration;
        let b = self.initial_velocity;
        let c = -distance;

        if a.abs() < 1e-6 {
            // Linear motion (no acceleration)
            if b.abs() < 1e-6 {
                return None; // Not moving
            }
            let t = -c / b;
            return if t >= 0.0 { Some(t) } else { None };
        }

        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return None; // No real solution
        }

        let sqrt_discriminant = discriminant.sqrt();
        let t1 = (-b + sqrt_discriminant) / (2.0 * a);
        let t2 = (-b - sqrt_discriminant) / (2.0 * a);

        // Return the smallest positive time
        match (t1 >= 0.0, t2 >= 0.0) {
            (true, true) => Some(t1.min(t2)),
            (true, false) => Some(t1),
            (false, true) => Some(t2),
            (false, false) => None,
        }
    }
}

impl Simulation for GravitySimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        // position = start + velocity*t + 0.5*acceleration*t^2
        self.start + self.initial_velocity * time + 0.5 * self.acceleration * time * time
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        // velocity = initial_velocity + acceleration*t
        self.initial_velocity + self.acceleration * time
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        let pos = self.position(time);

        // Check if we've reached or passed the end position
        if self.acceleration > 0.0 {
            // Moving in positive direction
            pos >= self.end - self.tolerance.distance
        } else if self.acceleration < 0.0 {
            // Moving in negative direction
            pos <= self.end + self.tolerance.distance
        } else {
            // No acceleration - check if we've reached end with initial velocity
            if self.initial_velocity > 0.0 {
                pos >= self.end - self.tolerance.distance
            } else if self.initial_velocity < 0.0 {
                pos <= self.end + self.tolerance.distance
            } else {
                // Not moving at all
                (pos - self.end).abs() < self.tolerance.distance
            }
        }
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

