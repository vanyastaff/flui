//! Numeric tolerance shared by layout and geometry comparisons.

/// Precision tolerance for floating-point comparisons (f64).
///
/// When comparing floating-point numbers for equality, use this tolerance
/// to account for precision errors. Two values `a` and `b` are considered
/// equal if `(a - b).abs() < EPSILON`.
pub const EPSILON: f64 = 1e-10;
