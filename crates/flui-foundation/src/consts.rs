//! Numeric tolerances shared by layout and sliver geometry.

/// The shared tolerance for layout and sliver comparisons that use Flutter's
/// `precisionErrorTolerance` scale.
///
/// This is not a universal floating-point equality threshold: each geometric
/// operation documents the tolerance and range policy appropriate to its units.
pub const EPSILON: f64 = 1e-10;
