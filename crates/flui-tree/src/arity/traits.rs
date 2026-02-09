//! Core arity trait definition.
//!
//! This module defines the `Arity` trait that all arity marker types implement.

use std::fmt::Debug;

use super::accessors::ChildrenAccess;
use super::runtime::{PerformanceHint, RuntimeArity};

// ============================================================================
// ARITY TRAIT
// ============================================================================

/// Advanced marker trait for compile-time arity with enhanced type features.
///
/// This trait defines the relationship between an arity marker type and its
/// corresponding accessor type using advanced Rust type system features:
///
/// - **GAT (Generic Associated Types)** for flexible accessors
/// - **Associated Constants** for performance tuning
/// - **HRTB compatibility** for universal predicates
/// - **Const generics** support for compile-time optimization
/// - **Never type** support for impossible operations
///
/// # Generic Accessor with GAT
///
/// The `Accessor` associated type uses GAT to provide flexible iteration:
///
/// ```
/// use flui_tree::arity::{Arity, Variable, ChildrenAccess};
///
/// // The Arity trait defines an Accessor type via GAT:
/// let children: &[u32] = &[1, 2, 3];
/// let accessor = Variable::from_slice(children);
/// assert_eq!(accessor.len(), 3);
/// ```
///
/// # HRTB Support
///
/// All operations are compatible with Higher-Rank Trait Bounds:
///
/// ```
/// use flui_tree::arity::{Arity, Variable, ChildrenAccess};
///
/// let children: &[u32] = &[10, 20, 30];
/// let accessor = Variable::from_slice(children);
/// // HRTB predicate works with any lifetime
/// let found = accessor.find_where(|x| *x > 15);
/// assert_eq!(found, Some(&20));
/// ```
///
/// # Performance Constants
///
/// Each arity type provides performance tuning constants:
/// - `EXPECTED_SIZE` - Expected number of children
/// - `INLINE_THRESHOLD` - Threshold for stack vs heap allocation
/// - `BATCH_SIZE` - Optimal batch processing size
///
/// # Implementations
///
/// - `Leaf` — 0 children with optimized empty operations
/// - `Optional` — 0 or 1 child with `Option`-like API
/// - `Exact<N>` — exactly N children with const generic validation
/// - `AtLeast<N>` — N or more children with iterator optimization
/// - `Variable` — any number with dynamic sizing strategies
/// - `Range<MIN, MAX>` — bounded range with compile-time limits
/// - `Never` — impossible arity (returns `!` for type safety)
pub trait Arity: sealed::Sealed + Send + Sync + Debug + Copy + Default + 'static {
    /// The accessor type for this arity, generic over element type `T`.
    ///
    /// Uses GAT to provide flexible, zero-cost accessors.
    /// Requires `T: Send + Sync` for thread-safe access.
    type Accessor<'a, T: 'a + Send + Sync>: ChildrenAccess<'a, T>;

    /// Iterator type for this arity using GAT.
    ///
    /// Allows different arity types to return optimized iterator implementations.
    type Iterator<'a, T: 'a>: Iterator<Item = &'a T>
    where
        T: 'a,
        Self: 'a;

    /// Expected number of children for sizing hints.
    const EXPECTED_SIZE: usize = 4;

    /// Threshold for inline vs heap allocation.
    const INLINE_THRESHOLD: usize = 16;

    /// Optimal batch size for bulk operations.
    const BATCH_SIZE: usize = 32;

    /// Whether this arity supports SIMD operations.
    const SUPPORTS_SIMD: bool = false;

    /// Get runtime arity information.
    fn runtime_arity() -> RuntimeArity;

    /// Check if count is valid for this arity.
    fn validate_count(count: usize) -> bool;

    /// Get performance hint for the given count.
    #[must_use]
    fn performance_hint(count: usize) -> PerformanceHint {
        let (_, hint) = Self::runtime_arity().validate_with_hint(count);
        hint
    }

    /// Convert slice to typed accessor.
    ///
    /// # Panics (debug only)
    ///
    /// Panics in debug builds if count doesn't match arity.
    /// Zero cost in release builds.
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T>;

    /// Try to convert slice to typed accessor.
    ///
    /// Returns `None` if the count doesn't match the arity.
    fn try_from_slice<T: Send + Sync>(children: &[T]) -> Option<Self::Accessor<'_, T>> {
        if Self::validate_count(children.len()) {
            Some(Self::from_slice(children))
        } else {
            None
        }
    }

    /// Create iterator from slice using GAT.
    ///
    /// Provides optimized iteration for the specific arity type.
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a;

    /// HRTB-compatible find operation.
    ///
    /// Find first child matching a predicate that works with any lifetime.
    #[allow(clippy::redundant_closure)]
    fn find_in_slice<'a, T, P>(children: &'a [T], predicate: P) -> Option<&'a T>
    where
        T: 'a,
        P: for<'b> Fn(&'b T) -> bool,
    {
        Self::iter_slice(children).find(|item| predicate(item))
    }

    /// HRTB-compatible filter operation.
    ///
    /// Filter children with a predicate that works with any lifetime.
    #[allow(clippy::redundant_closure)]
    fn filter_slice<'a, T, P>(children: &'a [T], predicate: P) -> Vec<&'a T>
    where
        T: 'a,
        P: for<'b> Fn(&'b T) -> bool,
    {
        Self::iter_slice(children)
            .filter(|item| predicate(item))
            .collect()
    }
}

// ============================================================================
// SEALED TRAIT
// ============================================================================

pub(crate) mod sealed {
    use super::super::types::{AtLeast, Exact, Leaf, Never, Optional, Range, Variable};

    pub trait Sealed {}

    impl Sealed for Leaf {}
    impl Sealed for Optional {}
    impl Sealed for Variable {}
    impl Sealed for Never {}
    impl<const N: usize> Sealed for Exact<N> {}
    impl<const N: usize> Sealed for AtLeast<N> {}
    impl<const MIN: usize, const MAX: usize> Sealed for Range<MIN, MAX> {}
}
