//! Spring-based physics simulations
//!
//! This module provides simulations that model spring physics with
//! mass, stiffness, and damping.

use super::{Simulation, Tolerance};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SpringType {
    /// Critical damping - returns to rest as quickly as possible without oscillating
    Critical,

    /// Under-damped - oscillates before coming to rest
    Underdamped,

    /// Over-damped - slowly returns to rest without oscillating
    Overdamped,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpringDescription {
    /// The mass of the spring (must be positive)
    pub mass: f32,

    /// The stiffness constant (must be positive)
    pub stiffness: f32,

    /// The damping coefficient (must be non-negative)
    pub damping: f32,
}

impl SpringDescription {
    #[must_use]
    pub const fn new(mass: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    #[must_use]
    pub fn with_critical_damping(mass: f32, stiffness: f32) -> Self {
        let damping = 2.0 * (mass * stiffness).sqrt();
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    #[must_use]
    pub fn bouncy() -> Self {
        // Mass=1, Stiffness=300, Damping=10 gives ~0.4 damping ratio
        Self::new(1.0, 300.0, 10.0)
    }

    #[must_use]
    pub fn stiff() -> Self {
        Self::with_critical_damping(1.0, 500.0)
    }

    #[must_use]
    pub fn soft() -> Self {
        // Mass=1, Stiffness=100, Damping=30 gives ~1.5 damping ratio
        Self::new(1.0, 100.0, 30.0)
    }

    #[must_use]
    pub fn spring_type(&self) -> SpringType {
        let critical_damping = 2.0 * (self.mass * self.stiffness).sqrt();
        let damping_ratio = self.damping / critical_damping;

        if (damping_ratio - 1.0).abs() < 0.001 {
            SpringType::Critical
        } else if damping_ratio < 1.0 {
            SpringType::Underdamped
        } else {
            SpringType::Overdamped
        }
    }

    #[must_use]
    pub fn damping_ratio(&self) -> f32 {
        let critical_damping = 2.0 * (self.mass * self.stiffness).sqrt();
        self.damping / critical_damping
    }

    #[must_use]
    pub fn natural_frequency(&self) -> f32 {
        (self.stiffness / self.mass).sqrt()
    }

    #[must_use]
    pub fn damped_frequency(&self) -> f32 {
        let w0 = self.natural_frequency();
        let zeta = self.damping_ratio();
        w0 * (1.0 - zeta * zeta).max(0.0).sqrt()
    }

    #[must_use]
    pub fn period(&self) -> Option<f32> {
        let wd = self.damped_frequency();
        if wd > 0.0 {
            Some(2.0 * std::f32::consts::PI / wd)
        } else {
            None
        }
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.mass > 0.0
            && self.stiffness > 0.0
            && self.damping >= 0.0
            && self.mass.is_finite()
            && self.stiffness.is_finite()
            && self.damping.is_finite()
    }

    #[must_use]
    pub fn critical_damping(&self) -> f32 {
        2.0 * (self.mass * self.stiffness).sqrt()
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpringSimulation {
    /// The spring description
    spring: SpringDescription,

    /// The starting position
    start: f32,

    /// The ending position (equilibrium point)
    end: f32,

    /// The initial velocity
    initial_velocity: f32,

    /// The tolerance for this simulation
    tolerance: Tolerance,
}

impl SpringSimulation {
    #[must_use]
    pub fn new(spring: SpringDescription, start: f32, end: f32, velocity: f32) -> Self {
        Self {
            spring,
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
    pub fn spring(&self) -> &SpringDescription {
        &self.spring
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
        self.spring.is_valid()
            && self.start.is_finite()
            && self.end.is_finite()
            && self.initial_velocity.is_finite()
            && self.tolerance.is_valid()
    }

    /// Calculate position for an underdamped spring
    fn position_underdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let wd = w0 * (1.0 - zeta * zeta).sqrt();

        let a = self.start - self.end;
        let b = (self.initial_velocity + zeta * w0 * a) / wd;

        let envelope = (-zeta * w0 * time).exp();
        self.end + envelope * (a * (wd * time).cos() + b * (wd * time).sin())
    }

    /// Calculate velocity for an underdamped spring
    fn velocity_underdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let wd = w0 * (1.0 - zeta * zeta).sqrt();

        let a = self.start - self.end;
        let b = (self.initial_velocity + zeta * w0 * a) / wd;

        let envelope = (-zeta * w0 * time).exp();
        let envelope_derivative = -zeta * w0 * envelope;

        envelope_derivative * (a * (wd * time).cos() + b * (wd * time).sin())
            + envelope * (-a * wd * (wd * time).sin() + b * wd * (wd * time).cos())
    }

    /// Calculate position for a critically damped spring
    fn position_critical(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let w0 = (k / m).sqrt();

        let a = self.start - self.end;
        let b = self.initial_velocity + w0 * a;

        let envelope = (-w0 * time).exp();
        self.end + envelope * (a + b * time)
    }

    /// Calculate velocity for a critically damped spring
    fn velocity_critical(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let w0 = (k / m).sqrt();

        let a = self.start - self.end;
        let b = self.initial_velocity + w0 * a;

        let envelope = (-w0 * time).exp();
        -w0 * envelope * (a + b * time) + envelope * b
    }

    /// Calculate position for an overdamped spring
    fn position_overdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let r = -w0 * (zeta - (zeta * zeta - 1.0).sqrt());
        let s = -w0 * (zeta + (zeta * zeta - 1.0).sqrt());

        let a = self.start - self.end;
        let b = (
            (s * a - self.initial_velocity) / (s - r),
            (self.initial_velocity - r * a) / (s - r),
        );

        self.end + b.0 * (r * time).exp() + b.1 * (s * time).exp()
    }

    /// Calculate velocity for an overdamped spring
    fn velocity_overdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let r = -w0 * (zeta - (zeta * zeta - 1.0).sqrt());
        let s = -w0 * (zeta + (zeta * zeta - 1.0).sqrt());

        let a = self.start - self.end;
        let b = (
            (s * a - self.initial_velocity) / (s - r),
            (self.initial_velocity - r * a) / (s - r),
        );

        b.0 * r * (r * time).exp() + b.1 * s * (s * time).exp()
    }
}

impl Simulation for SpringSimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        match self.spring.spring_type() {
            SpringType::Critical => self.position_critical(time),
            SpringType::Underdamped => self.position_underdamped(time),
            SpringType::Overdamped => self.position_overdamped(time),
        }
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        match self.spring.spring_type() {
            SpringType::Critical => self.velocity_critical(time),
            SpringType::Underdamped => self.velocity_underdamped(time),
            SpringType::Overdamped => self.velocity_overdamped(time),
        }
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        let pos = self.position(time);
        let vel = self.velocity(time);

        (pos - self.end).abs() < self.tolerance.distance && vel.abs() < self.tolerance.velocity
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

