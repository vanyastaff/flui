//! Sealed trait pattern for View
//!
//! This module prevents external implementations of the View trait.
//! Only types within this crate can implement View.

/// Sealed trait to prevent external implementation of View
///
/// This trait is public but cannot be implemented outside this crate
/// because it's in a private module.
pub trait Sealed {}

// All primitives that can be Views
impl Sealed for () {}
impl Sealed for String {}
impl Sealed for &'static str {}
impl Sealed for i32 {}
impl Sealed for f32 {}
impl Sealed for bool {}

// Tuples (for ViewSequence)
impl<A: Sealed> Sealed for (A,) {}
impl<A: Sealed, B: Sealed> Sealed for (A, B) {}
impl<A: Sealed, B: Sealed, C: Sealed> Sealed for (A, B, C) {}
impl<A: Sealed, B: Sealed, C: Sealed, D: Sealed> Sealed for (A, B, C, D) {}
impl<A: Sealed, B: Sealed, C: Sealed, D: Sealed, E: Sealed> Sealed for (A, B, C, D, E) {}

// Option (conditional views)
impl<T: Sealed> Sealed for Option<T> {}

// Vec (list of views)
impl<T: Sealed> Sealed for Vec<T> {}
