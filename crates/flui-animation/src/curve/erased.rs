//! Curve modifiers and erasure: `Flipped`, `ArcCurve` and shared pointers.

use std::fmt;
use std::sync::Arc;

use super::{Builtin, BuiltinCurve, Curve};
/// A curve that is the flipped version of another curve: the 180° rotation
/// `1.0 - curve.transform(1.0 - t)`, which turns an ease-in into an ease-out
/// while still mapping `0.0 → 0.0` and `1.0 → 1.0`.
///
/// It is the 180° rotation, not the vertical mirror `1.0 - curve.transform(t)`
/// — a mirror would invert every consumer expecting a rotation (a
/// `CurvedAnimation` reverse-curve default, most visibly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Flipped<C: Curve> {
    /// The curve to flip.
    pub curve: C,
}

impl<C: Curve> Flipped<C> {
    /// Creates a new flipped curve.
    #[inline]
    #[must_use]
    pub const fn new(curve: C) -> Self {
        Self { curve }
    }
}

impl<C: Curve> Curve for Flipped<C> {
    #[inline]
    fn transform(&self, t: f64) -> f64 {
        1.0 - self.curve.transform(1.0 - t)
    }

    fn slope(&self, t: f64) -> f64 {
        // d/dt [1 − c(1 − t)] = c'(1 − t); NaN and the outside-[0, 1] zero
        // carry through.
        self.curve.slope(1.0 - t)
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        Some(BuiltinCurve::new(Builtin::Flipped(self.curve.builtin()?)))
    }
}

/// A reference-counted, type-erased curve handle.
///
/// Wraps any `impl Curve + Send + Sync + 'static` so that a single, stable
/// concrete type can be stored in widgets and animation controllers,
/// regardless of which specific curve is used.
///
/// `ArcCurve` implements `Curve + Clone + Send + Sync + Debug`, with value
/// comparison for built-ins and identity comparison for custom curves.
///
/// # Equality
///
/// A curve this crate defines compares **by value**: two `ArcCurve`s wrapping
/// equal [`Cubic`](super::Cubic)s, [`ThreePointCubic`](super::ThreePointCubic)s, elastic, bounce or linear curves,
/// or an [`Interval`](super::Interval) or [`Flipped`] of those, are equal however they
/// were built. A parent that rebuilds every frame with
/// `.curve(curves::EASE_IN_OUT)` therefore hands an implicit animation an
/// *unchanged* curve, and the animation keeps its run. Any other curve —
/// yours, a [`Split`](super::Split), or a combinator over one — compares by identity
/// (`Arc::ptr_eq`), because [`Curve`] has no equality of its own: reuse the
/// same handle (clone it) to keep it equal across rebuilds.
///
/// # Examples
///
/// ```
/// use flui_animation::{ArcCurve, Curve, curves, Elastic};
///
/// let curve = ArcCurve::new(Elastic::default());
/// assert_eq!(curve.transform(0.0), 0.0);
/// assert_eq!(curve.transform(1.0), 1.0);
///
/// // Built-in curves compare by value.
/// assert_eq!(ArcCurve::new(curves::EASE_IN), ArcCurve::new(curves::EASE_IN));
/// assert_ne!(ArcCurve::new(curves::EASE_IN), ArcCurve::new(curves::EASE_OUT));
/// ```
///
/// [`CurvedAnimation`]: crate::CurvedAnimation
#[derive(Clone)]
pub struct ArcCurve(Erased);

/// What an [`ArcCurve`] holds: a built-in curve's value, or an opaque curve.
#[derive(Clone)]
enum Erased {
    Builtin(BuiltinCurve),
    Custom(Arc<dyn Curve + Send + Sync>),
}

impl PartialEq for ArcCurve {
    /// Value equality for built-in curves, identity otherwise — see the type
    /// doc.
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Erased::Builtin(a), Erased::Builtin(b)) => a == b,
            (Erased::Custom(a), Erased::Custom(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

// Built-in curve parameters are validated finite, so value equality is
// reflexive.
impl Eq for ArcCurve {}

impl ArcCurve {
    /// Wrap `curve` in a reference-counted erased handle.
    ///
    /// A built-in curve is stored by value (and compares by value); any
    /// other curve is moved behind an `Arc`. Cloning is a reference-count
    /// bump either way.
    pub fn new(curve: impl Curve + Send + Sync + 'static) -> Self {
        Self(match curve.builtin() {
            Some(builtin) => Erased::Builtin(builtin),
            None => Erased::Custom(Arc::new(curve)),
        })
    }
}

impl Curve for ArcCurve {
    fn transform(&self, t: f64) -> f64 {
        match &self.0 {
            Erased::Builtin(curve) => curve.transform(t),
            Erased::Custom(curve) => curve.transform(t),
        }
    }

    fn slope(&self, t: f64) -> f64 {
        match &self.0 {
            Erased::Builtin(curve) => curve.slope(t),
            Erased::Custom(curve) => curve.slope(t),
        }
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        match &self.0 {
            Erased::Builtin(curve) => Some(curve.clone()),
            Erased::Custom(_) => None,
        }
    }
}

impl fmt::Debug for ArcCurve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Erased::Builtin(curve) => f.debug_tuple("ArcCurve").field(curve).finish(),
            // `Curve` does not require `Debug`.
            Erased::Custom(_) => f.debug_struct("ArcCurve").finish_non_exhaustive(),
        }
    }
}

/// Blanket impl so `Arc<dyn Curve + Send + Sync>` can itself be used as a
/// `Curve` where object-safety is all that matters. Use [`ArcCurve`] when a
/// stable concrete handle with configuration equality is needed.
impl<C: Curve + ?Sized> Curve for Arc<C> {
    fn transform(&self, t: f64) -> f64 {
        (**self).transform(t)
    }

    fn slope(&self, t: f64) -> f64 {
        (**self).slope(t)
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        (**self).builtin()
    }
}

impl<C: Curve + ?Sized> Curve for std::rc::Rc<C> {
    fn transform(&self, t: f64) -> f64 {
        (**self).transform(t)
    }

    fn slope(&self, t: f64) -> f64 {
        (**self).slope(t)
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        (**self).builtin()
    }
}
