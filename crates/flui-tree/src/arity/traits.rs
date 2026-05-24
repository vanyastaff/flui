//! Core arity trait definition.
//!
//! Cycle 3 T-7: simplified from the pre-cycle "Arity + accessor + GAT
//! iterator + slice conversion" surface (~160 LOC, ~12 associated
//! items + methods) to a pure compile-time marker trait. The
//! pre-cycle machinery depended on `ChildrenAccess`, `RuntimeArity`,
//! `PerformanceHint` — all deleted alongside `accessors.rs`,
//! `runtime.rs`, `arity_storage.rs`, `storage.rs` as zombie surface.
//!
//! Concrete render objects attach the marker to their child storage
//! (plain `Option<C>` / `Vec<C>` / fixed array) at the type level
//! and rely on the marker for documentation + sealed-trait
//! constraint. No runtime dispatch.

use std::fmt::Debug;

// ============================================================================
// ARITY TRAIT
// ============================================================================

/// Compile-time marker trait for tree-node arity (child-count
/// constraint).
///
/// Implemented by the seven canonical markers:
/// - [`super::Leaf`] — 0 children
/// - [`super::Optional`] — 0 or 1 child
/// - [`super::Exact<N>`] — exactly N children (`Single = Exact<1>`)
/// - [`super::AtLeast<N>`] — N or more children
/// - [`super::Variable`] — any number
/// - [`super::Range<MIN, MAX>`] — bounded range
/// - [`super::Never`] — uninhabited (type-system bottom)
///
/// The trait is sealed — only the canonical markers in
/// `arity::types` implement it. The `Send + Sync + Debug + Copy +
/// Default + 'static` super-bounds let arity markers cross thread
/// boundaries, derive Debug, and be plugged into generic code that
/// expects a zero-sized type.
pub trait Arity: sealed::Sealed + Send + Sync + Debug + Copy + Default + 'static {
    /// Static description of this arity (e.g. `"Leaf"`, `"Single"`,
    /// `"Exact<3>"`). Used by `ArityError` messages so consumers
    /// don't have to import a separate runtime enum.
    const DESCRIPTION: &'static str;

    /// Check if a given child count is valid for this arity.
    ///
    /// `Leaf::validate_count(0) → true`, `Leaf::validate_count(1) → false`.
    /// `Single::validate_count(1) → true`, etc.
    fn validate_count(count: usize) -> bool;
}

// ============================================================================
// SEALED TRAIT
// ============================================================================

pub(crate) mod sealed {
    use super::super::types::{AtLeast, Exact, Leaf, Never, Optional, Range, Variable};

    pub trait Sealed {} // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked

    impl Sealed for Leaf {}
    impl Sealed for Optional {}
    impl<const N: usize> Sealed for Exact<N> {}
    impl<const N: usize> Sealed for AtLeast<N> {}
    impl Sealed for Variable {}
    impl<const MIN: usize, const MAX: usize> Sealed for Range<MIN, MAX> {}
    impl Sealed for Never {}
}
