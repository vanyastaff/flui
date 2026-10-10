//! Interruptible, velocity-preserving animation of fixed-width values.
//!
//! Each `AnimatedValue<T>` owns one frame registration for all its components.
//! Its observer implements `Animation<T>`, retaining the last published value
//! without owning or prolonging the run.

mod driver;

pub use driver::{AnimatedValue, AnimatedValueView, MotionUpdate, VsyncPublication, VsyncUpdate};

use flui_foundation::geometry::{EdgeInsets, Offset, Size};
use flui_painting::Alignment;
use flui_painting::styling::{Color, PremultipliedOklab};
use std::time::Duration;

mod sealed {
    pub trait Vector {}
    impl<const N: usize> Vector for [f64; N] {}
    pub trait GeneratedMotion {}
}

/// A fixed array of scalar components. Sealed so sample publication cannot
/// invoke a user-defined conversion while the controller commits its time.
pub trait AnimationVector: sealed::Vector + AsRef<[f64]> + AsMut<[f64]> + Copy + 'static {
    /// Number of scalar components, used to compose nested fixed-width values.
    const COMPONENTS: usize;

    /// A vector with the same width and every component set to zero.
    #[must_use]
    fn zero(self) -> Self;
}

impl<const N: usize> AnimationVector for [f64; N] {
    const COMPONENTS: usize = N;

    fn zero(self) -> Self {
        [0.0; N]
    }
}

/// Only generated component runs can publish vector state inside a sample
/// commit. Staging invokes curves outside controller borrows; publication and
/// settlement only move Copy arrays.
pub(crate) trait ValueMotion:
    sealed::GeneratedMotion + crate::simulation::Simulation
{
    fn stage_sample(&self, time: f64, current: &dyn Fn() -> bool) -> bool;
    fn commit_sample(&self, elapsed: Duration);
    fn settle(&self);
}

/// A value that can be decomposed into, and rebuilt from, a fixed-width vector
/// of scalar components, so each component can be animated by its own spring.
///
/// Implement it for a value whose components have independent motion, or derive
/// it for a nonempty struct whose fields implement this trait and `Lerp`.
/// Derived vectors concatenate fields in declaration order, including nested
/// structs; interpolation preserves each field's `Lerp` contract. Deriving
/// requires concrete component widths, since stable Rust cannot sum
/// generic-dependent widths in an array length.
/// Conversions run outside state borrows.
pub trait TwoWayConverter: Clone {
    /// The scalar-component representation, e.g. `[f64; 4]` for a colour.
    /// `Copy` so it can be used as a scratch buffer; `AsRef`/`AsMut<[f64]>` so
    /// the spring core can iterate components generically.
    type Vector: AnimationVector;

    /// Decompose into scalar components.
    fn to_vector(&self) -> Self::Vector;

    /// Rebuild from scalar components.
    fn from_vector(v: Self::Vector) -> Self;

    /// Positive finite rest distances in each component's own units.
    ///
    /// Spring admission validates these distances before replacing a run.
    /// Speed limits follow the spring's natural rate; curve motion ignores
    /// these thresholds. Derived values concatenate their fields' thresholds.
    /// Geometry uses 0.01 logical pixels; normalized scalar, alignment and
    /// premultiplied Oklab components use 0.001. Custom converters must choose
    /// distances appropriate for their representation rather than transform a
    /// value through `to_vector` (nonlinear conversion does not preserve errors).
    fn rest_thresholds() -> Self::Vector;
}

impl TwoWayConverter for f64 {
    type Vector = [f64; 1];
    fn rest_thresholds() -> Self::Vector {
        [0.001]
    }
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [*self]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        v[0]
    }
}

impl TwoWayConverter for Offset<f64> {
    type Vector = [f64; 2];
    fn rest_thresholds() -> Self::Vector {
        [0.01; 2]
    }
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [self.dx, self.dy]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        Offset::new(v[0], v[1])
    }
}

impl TwoWayConverter for Size<f64> {
    type Vector = [f64; 2];
    fn rest_thresholds() -> Self::Vector {
        [0.01; 2]
    }
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [self.width, self.height]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        Size::new(v[0], v[1])
    }
}

impl TwoWayConverter for EdgeInsets {
    type Vector = [f64; 4];
    fn rest_thresholds() -> Self::Vector {
        [0.01; 4]
    }
    fn to_vector(&self) -> Self::Vector {
        [self.top, self.right, self.bottom, self.left]
    }
    fn from_vector([top, right, bottom, left]: Self::Vector) -> Self {
        Self::new(top, right, bottom, left)
    }
}

impl TwoWayConverter for Alignment {
    type Vector = [f64; 2];
    fn rest_thresholds() -> Self::Vector {
        [0.001; 2]
    }
    fn to_vector(&self) -> Self::Vector {
        [self.x, self.y]
    }
    fn from_vector([x, y]: Self::Vector) -> Self {
        Self::new(x, y)
    }
}

/// Springs a colour in premultiplied Oklab (`L·α`, `a·α`, `b·α`, `α`), the space
/// `Tween<Color>` mixes in (ADR-0149): each component's spring carries its own
/// velocity, and a fade to transparent keeps the opaque end's hue.
impl TwoWayConverter for Color {
    type Vector = [f64; 4];
    fn rest_thresholds() -> Self::Vector {
        [0.001; 4]
    }
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        let p = self.to_premultiplied_oklab();
        [p.l, p.a, p.b, p.alpha].map(f64::from)
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        // f32 is the colour math's precision (ADR-0098 §2); an overshoot past
        // f32's range saturates like any out-of-gamut channel.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spring components narrow to the f32 colour math"
        )]
        let [l, a, b, alpha] = v.map(|c| c as f32);
        Color::from_premultiplied_oklab(PremultipliedOklab { l, a, b, alpha })
    }
}
