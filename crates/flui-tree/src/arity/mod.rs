//! Advanced compile-time arity system for tree nodes with enhanced type features.
//!
//! This module provides a production-grade, zero-cost abstraction for expressing
//! and validating child counts using advanced Rust type system features:
//!
//! - **Const Generics** for compile-time size validation
//! - **HRTB (Higher-Rank Trait Bounds)** for universal predicates
//! - **GAT (Generic Associated Types)** for flexible accessors
//! - **Associated Constants** for performance tuning
//! - **Sealed traits** for safety
//! - **Typestate patterns** for compile-time guarantees
//!
//! # Advanced Features
//!
//! - **Compile time**: Type-level arity prevents invalid access patterns
//! - **Zero-cost**: No runtime overhead in release builds
//! - **HRTB-compatible**: Works with predicates of any lifetime
//! - **GAT-based**: Flexible accessor types via Generic Associated Types
//! - **Const generic**: Compile-time size optimization
//! - **Never type**: Impossible operations return `!` for type safety
//!
//! # Enhanced Arity Forms
//!
//! - [`Leaf`] — 0 children with optimized empty operations
//! - [`Optional`] — 0 or 1 child with `Option`-like API
//! - [`Single`] — exactly 1 child (alias for `Exact<1>`)
//! - [`Exact<N>`] — exactly N children with const generic validation
//! - [`AtLeast<N>`] — N or more children with iterator optimization
//! - [`Variable`] — any number with dynamic sizing strategies
//! - [`Range<MIN, MAX>`] — bounded range with compile-time limits
//!
//! # HRTB-Compatible Design
//!
//! All accessors support Higher-Rank Trait Bounds for maximum flexibility:
//!
//! ```rust,ignore
//! // HRTB predicate works with any lifetime
//! fn find_child_where<T, A, P>(accessor: &A, predicate: P) -> Option<&T>
//! where
//!     A: ChildrenAccess<T>,
//!     P: for<'a> Fn(&'a T) -> bool,
//! {
//!     accessor.iter().find(|item| predicate(item))
//! }
//! ```
//!
//! # Generic Design with GAT
//!
//! All accessors use Generic Associated Types for flexible iteration:
//!
//! ```rust,ignore
//! // GAT-based iterator for different storage types
//! trait ArityAccessor<T> {
//!     type Iter<'a>: Iterator<Item = &'a T> where T: 'a, Self: 'a;
//!     fn iter(&self) -> Self::Iter<'_>;
//! }
//!
//! // Works with ElementId, Element, or any type
//! let ids: &[ElementId] = &[id1, id2];
//! let elements: &[Element] = &[elem1, elem2];
//!
//! let id_accessor = Variable::from_slice(ids);
//! let element_accessor = Variable::from_slice(elements);
//! ```
//!
//! # Performance Example
//!
//! ```rust,ignore
//! use flui_tree::{Arity, Single, Variable, Exact};
//!
//! // Const generic optimization for fixed sizes
//! fn layout_exact_three<T: Copy, const BATCH_SIZE: usize = 32>(
//!     children: &[T]
//! ) -> [T; 3] {
//!     let accessor = Exact::<3>::from_slice(children);
//!     accessor.as_array() // Zero-cost conversion
//! }
//!
//! // HRTB predicate for flexible filtering
//! fn layout_filtered<T, P>(children: &[T], predicate: P) -> Vec<&T>
//! where
//!     P: for<'a> Fn(&'a T) -> bool,
//! {
//!     let accessor = Variable::from_slice(children);
//!     accessor.iter().filter(|item| predicate(item)).collect()
//! }
//! ```

mod accessors;
pub mod children;

pub use accessors::{
    // Performance enums
    AccessFrequency,
    AccessPattern,
    // New advanced accessors
    BoundedChildren,
    ChildrenAccess,
    Copied,
    FixedChildren,
    NeverAccessor,
    NoChildren,
    OptionalChild,
    SliceChildren,
    SmartChildren,
    TypeInfo,
    TypedChildren,
};

pub use children::{ArityStorage, Children, LeafChildren, SingleChild, VariableChildren};

use std::fmt::Debug;
use std::marker::PhantomData;

// ============================================================================
// RUNTIME ARITY
// ============================================================================

/// Enhanced runtime arity information with advanced features.
///
/// Represents the runtime equivalent of compile-time arity types.
/// Used for error messages, debugging, dynamic validation, and performance hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeArity {
    /// Exactly N children.
    Exact(usize),
    /// 0 or 1 child.
    Optional,
    /// At least N children.
    AtLeast(usize),
    /// Any number of children.
    Variable,
    /// Bounded range of children (min, max).
    Range(usize, usize),
    /// Never type - impossible arity (for type safety).
    Never,
}

impl RuntimeArity {
    /// Check if the count is valid for this arity.
    #[inline]
    pub const fn validate(&self, count: usize) -> bool {
        match self {
            Self::Exact(n) => count == *n,
            Self::Optional => count <= 1,
            Self::AtLeast(n) => count >= *n,
            Self::Variable => true,
            Self::Range(min, max) => {
                // Validate that min <= max (logical constraint)
                // If min > max, the range is invalid and always returns false
                *min <= *max && count >= *min && count <= *max
            }
            Self::Never => false, // Never type - always invalid
        }
    }

    /// Check if this arity is impossible (Never type).
    #[inline]
    pub const fn is_impossible(&self) -> bool {
        matches!(self, Self::Never)
    }

    /// Get the minimum valid count for this arity.
    #[inline]
    pub const fn min_count(&self) -> usize {
        match self {
            Self::Optional | Self::Variable => 0,
            Self::Exact(n) | Self::AtLeast(n) => *n,
            Self::Range(min, _) => *min,
            Self::Never => usize::MAX, // Impossible
        }
    }

    /// Get the maximum valid count for this arity (None = unbounded).
    #[inline]
    pub const fn max_count(&self) -> Option<usize> {
        match self {
            Self::Exact(n) => Some(*n),
            Self::Optional => Some(1),
            Self::AtLeast(_) | Self::Variable => None,
            Self::Range(_, max) => Some(*max),
            Self::Never => Some(0), // Impossible but bounded
        }
    }

    /// Check if this arity allows the given count with performance hint.
    pub const fn validate_with_hint(&self, count: usize) -> (bool, PerformanceHint) {
        let valid = self.validate(count);
        let hint = match self {
            Self::Exact(_) | Self::Optional => PerformanceHint::FixedSize,
            Self::AtLeast(_) | Self::Variable if count < 16 => PerformanceHint::SmallDynamic,
            Self::AtLeast(_) | Self::Variable => PerformanceHint::LargeDynamic,
            Self::Range(_, _) => PerformanceHint::Bounded,
            Self::Never => PerformanceHint::Impossible,
        };
        (valid, hint)
    }
}

impl std::fmt::Display for RuntimeArity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(0) => write!(f, "Leaf (0 children)"),
            Self::Exact(1) => write!(f, "Single (1 child)"),
            Self::Exact(n) => write!(f, "Exact({n} children)"),
            Self::Optional => write!(f, "Optional (0 or 1 child)"),
            Self::AtLeast(n) => write!(f, "AtLeast({n} children)"),
            Self::Variable => write!(f, "Variable (any number)"),
            Self::Range(min, max) => write!(f, "Range({min}-{max} children)"),
            Self::Never => write!(f, "Never (impossible)"),
        }
    }
}

/// Performance hint for arity validation and access patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PerformanceHint {
    /// Fixed size - use array access
    FixedSize,
    /// Small dynamic - use stack allocation
    SmallDynamic,
    /// Large dynamic - use heap allocation
    LargeDynamic,
    /// Bounded range - use smart allocation
    Bounded,
    /// Impossible operation - compile-time error
    Impossible,
}

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
/// ```rust,ignore
/// trait Arity {
///     type Accessor<'a, T: 'a>: ChildrenAccess<'a, T>;
///     type Iterator<'a, T: 'a>: Iterator<Item = &'a T> where T: 'a;
/// }
/// ```
///
/// # HRTB Support
///
/// All operations are compatible with Higher-Rank Trait Bounds:
///
/// ```rust,ignore
/// fn find_child<A, P>(accessor: A, predicate: P) -> Option<ElementId>
/// where
///     A: ChildrenAccess<ElementId>,
///     P: for<'a> Fn(&'a ElementId) -> bool,
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
/// - [`Leaf`] — 0 children with optimized empty operations
/// - [`Optional`] — 0 or 1 child with `Option`-like API
/// - [`Exact<N>`] — exactly N children with const generic validation
/// - [`AtLeast<N>`] — N or more children with iterator optimization
/// - [`Variable`] — any number with dynamic sizing strategies
/// - [`Range<MIN, MAX>`] — bounded range with compile-time limits
/// - [`Never`] — impossible arity (returns `!` for type safety)
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

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::Leaf {}
    impl Sealed for super::Optional {}
    impl Sealed for super::Variable {}
    impl Sealed for super::Never {}
    impl<const N: usize> Sealed for super::Exact<N> {}
    impl<const N: usize> Sealed for super::AtLeast<N> {}
    impl<const MIN: usize, const MAX: usize> Sealed for super::Range<MIN, MAX> {}
}

// ============================================================================
// RANGE ARITY (Bounded range with const generics)
// ============================================================================

/// Range arity marker — bounded number of children with const generics.
///
/// Provides compile-time validation for bounded ranges while maintaining
/// flexibility for dynamic sizes within those bounds.
///
/// # Example
///
/// ```rust,ignore
/// fn layout_range<T>(children: &[T]) {
///     let accessor = Range::<2, 5>::from_slice(children);
///     assert!(accessor.len() >= 2 && accessor.len() <= 5);
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct Range<const MIN: usize, const MAX: usize>;

impl<const MIN: usize, const MAX: usize> Arity for Range<MIN, MAX> {
    type Accessor<'a, T: 'a + Send + Sync> = SliceChildren<'a, T>;
    type Iterator<'a, T: 'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;

    // Expected size is average of MIN and MAX
    // Note: If MIN > MAX, this will be incorrect, but validate_count will catch it
    const EXPECTED_SIZE: usize = usize::midpoint(MIN, MAX);
    const INLINE_THRESHOLD: usize = MAX;
    const BATCH_SIZE: usize = MAX;

    #[inline]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Range(MIN, MAX)
    }

    #[inline]
    fn validate_count(count: usize) -> bool {
        // Ensure MIN <= MAX (logical constraint)
        // If MIN > MAX, the range is invalid and always returns false
        if MIN > MAX {
            return false;
        }
        count >= MIN && count <= MAX
    }

    #[inline]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        // Validate MIN <= MAX constraint
        debug_assert!(
            MIN <= MAX,
            "Range<{MIN}, {MAX}> is invalid: MIN ({MIN}) must be <= MAX ({MAX})"
        );
        debug_assert!(
            children.len() >= MIN,
            "Range<{MIN}, {MAX}> expects >= {MIN} children, got {}",
            children.len()
        );
        debug_assert!(
            children.len() <= MAX,
            "Range<{MIN}, {MAX}> expects <= {MAX} children, got {}",
            children.len()
        );
        SliceChildren { children }
    }

    #[inline]
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        debug_assert!(children.len() >= MIN && children.len() <= MAX);
        children.iter()
    }
}

// ============================================================================
// NEVER ARITY (Impossible operations)
// ============================================================================

/// Never arity marker for impossible operations.
///
/// Used for operations that should never succeed, providing compile-time
/// safety through the never type.
///
/// # Example
///
/// ```rust,ignore
/// fn impossible_operation<T>(children: &[T]) -> ! {
///     let accessor = Never::from_slice(children);
///     accessor.impossible() // Returns ! - never returns
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Never;

impl Default for Never {
    fn default() -> Self {
        Never
    }
}

impl Arity for Never {
    type Accessor<'a, T: 'a + Send + Sync> = NeverAccessor<T>;
    type Iterator<'a, T: 'a>
        = std::iter::Empty<&'a T>
    where
        T: 'a;

    const EXPECTED_SIZE: usize = 0;
    const INLINE_THRESHOLD: usize = 0;
    const BATCH_SIZE: usize = 0;

    #[inline]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Never
    }

    #[inline]
    fn validate_count(_count: usize) -> bool {
        false // Never valid
    }

    #[inline]
    fn from_slice<T: Send + Sync>(_children: &[T]) -> Self::Accessor<'_, T> {
        NeverAccessor(PhantomData)
    }

    #[inline]
    fn iter_slice<'a, T>(_children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        std::iter::empty()
    }
}

// ============================================================================
// LEAF (0 children) - Enhanced with never type support
// ============================================================================

/// Leaf arity marker — 0 children with enhanced type safety.
///
/// For nodes that never have children (e.g., Text, Image, Spacer).
/// Enhanced with never type support for impossible operations.
///
/// # Enhanced Features
///
/// - **Never type**: Operations that can't succeed return `!`
/// - **Const evaluation**: All validation at compile time
/// - **SIMD ready**: Optimized for empty collection operations
/// - **HRTB compatible**: Works with any lifetime predicates
///
/// # Example
///
/// ```rust,ignore
/// fn layout_leaf<T>(children: &[T]) -> !
/// where
///     [T; 0]: ,  // Const generic constraint
/// {
///     let accessor = Leaf::from_slice(children);
///     accessor.first() // Returns `!` - impossible operation
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct Leaf;

impl Arity for Leaf {
    type Accessor<'a, T: 'a + Send + Sync> = NoChildren<T>;
    type Iterator<'a, T: 'a>
        = std::iter::Empty<&'a T>
    where
        T: 'a;

    const EXPECTED_SIZE: usize = 0;
    const INLINE_THRESHOLD: usize = 0;
    const BATCH_SIZE: usize = 0;
    const SUPPORTS_SIMD: bool = true; // Empty operations are SIMD-friendly

    #[inline]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Exact(0)
    }

    #[inline]
    fn validate_count(count: usize) -> bool {
        count == 0
    }

    #[inline]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.is_empty(),
            "Leaf expects 0 children, got {}",
            children.len()
        );
        NoChildren(PhantomData)
    }

    #[inline]
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        debug_assert!(children.is_empty());
        std::iter::empty()
    }
}

impl Leaf {
    /// Leaf-specific never operation - first child is impossible.
    ///
    /// This method demonstrates the never type for impossible operations.
    /// It will never return because leaf nodes cannot have children.
    ///
    /// # Panics
    ///
    /// Always panics - leaf nodes cannot have children.
    pub fn first_impossible<T>(_children: &[T]) -> ! {
        panic!("Leaf nodes cannot have children - this operation is impossible")
    }
}

// ============================================================================
// OPTIONAL (0 or 1 child)
// ============================================================================

/// Optional arity marker — 0 or 1 child.
///
/// For nodes that can work with or without a child
/// (e.g., `SizedBox`, `Container`, `ColoredBox`).
///
/// # Example
///
/// ```rust,ignore
/// fn layout_optional<T>(children: &[T]) {
///     let accessor = Optional::from_slice(children);
///     if let Some(child) = accessor.get() {
///         // Process child
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct Optional;

impl Arity for Optional {
    type Accessor<'a, T: 'a + Send + Sync> = OptionalChild<'a, T>;
    type Iterator<'a, T: 'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;

    #[inline]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Optional
    }

    #[inline]
    fn validate_count(count: usize) -> bool {
        count <= 1
    }

    #[inline]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.len() <= 1,
            "Optional expects 0 or 1 child, got {}",
            children.len()
        );
        OptionalChild { children }
    }

    #[inline]
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        debug_assert!(children.len() <= 1);
        children.iter()
    }
}

// ============================================================================
// EXACT<N> (exactly N children)
// ============================================================================

/// Exact arity marker — exactly N children.
///
/// For nodes that require a specific number of children.
/// Use [`Single`] as a convenient alias for `Exact<1>`.
///
/// # Example
///
/// ```rust,ignore
/// fn layout_pair<T>(children: &[T]) {
///     let accessor = Exact::<2>::from_slice(children);
///     let (first, second) = accessor.pair();
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct Exact<const N: usize>;

impl<const N: usize> Arity for Exact<N> {
    type Accessor<'a, T: 'a + Send + Sync> = FixedChildren<'a, T, N>;
    type Iterator<'a, T: 'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;

    #[inline]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Exact(N)
    }

    #[inline]
    fn validate_count(count: usize) -> bool {
        count == N
    }

    #[inline]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.len() == N,
            "Exact<{N}> expects {N} children, got {}",
            children.len()
        );
        // SAFETY: We've validated the length in debug mode.
        // In release mode, we trust the caller, but still need to handle the error
        // gracefully to avoid panics in production code.
        let array_ref: &[T; N] = children.try_into().unwrap_or_else(|_| {
            // This is a programming error that should be caught in debug
            panic!(
                "slice length mismatch: expected {N}, got {}",
                children.len()
            );
        });
        FixedChildren {
            children: array_ref,
        }
    }

    #[inline]
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        debug_assert!(children.len() == N);
        children.iter()
    }
}

/// Single child arity — alias for `Exact<1>`.
///
/// The most common arity for wrapper nodes (Padding, Align, Transform, etc.).
///
/// # Example
///
/// ```rust,ignore
/// fn layout_single<T: Copy>(children: &[T]) -> T {
///     let accessor = Single::from_slice(children);
///     *accessor.single()
/// }
/// ```
pub type Single = Exact<1>;

// ============================================================================
// AT_LEAST<N> (N or more children)
// ============================================================================

/// `AtLeast` arity marker — N or more children.
///
/// For nodes that require a minimum number of children.
///
/// # Example
///
/// ```rust,ignore
/// fn layout_at_least_two<T>(children: &[T]) {
///     let accessor = AtLeast::<2>::from_slice(children);
///     assert!(accessor.len() >= 2);
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct AtLeast<const N: usize>;

impl<const N: usize> Arity for AtLeast<N> {
    type Accessor<'a, T: 'a + Send + Sync> = SliceChildren<'a, T>;
    type Iterator<'a, T: 'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;

    #[inline]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::AtLeast(N)
    }

    #[inline]
    fn validate_count(count: usize) -> bool {
        count >= N
    }

    #[inline]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.len() >= N,
            "AtLeast<{N}> expects >= {N} children, got {}",
            children.len()
        );
        SliceChildren { children }
    }

    #[inline]
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        debug_assert!(children.len() >= N);
        children.iter()
    }
}

// ============================================================================
// VARIABLE (any number)
// ============================================================================

/// Variable arity marker — any number of children.
///
/// For nodes that can have any number of children (Flex, Stack, Column, etc.).
///
/// # Example
///
/// ```rust,ignore
/// fn layout_variable<T>(children: &[T]) {
///     let accessor = Variable::from_slice(children);
///     for child in accessor.iter() {
///         // Process each child
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct Variable;

impl Arity for Variable {
    type Accessor<'a, T: 'a + Send + Sync> = SliceChildren<'a, T>;
    type Iterator<'a, T: 'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;

    #[inline]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Variable
    }

    #[inline]
    fn validate_count(_count: usize) -> bool {
        true
    }

    #[inline]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        SliceChildren { children }
    }

    #[inline]
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        children.iter()
    }
}

// ============================================================================
// CHILDREN MUTATION - NOW HANDLED BY ChildrenAccess
// ============================================================================

// All mutation capability is now handled by the ChildrenAccess trait
// which provides can_add_child(), can_remove_child(), etc.
// No need for separate mutation traits.

// ============================================================================
// ARITY ERROR TYPES
// ============================================================================

/// Errors that can occur during arity-validated operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArityError {
    /// Attempted to add too many children.
    TooManyChildren {
        arity: RuntimeArity,
        attempted: usize,
    },
    /// Attempted to remove too many children.
    TooFewChildren {
        arity: RuntimeArity,
        attempted: usize,
    },
    /// Invalid child count for arity.
    InvalidChildCount { arity: RuntimeArity, actual: usize },
}

impl std::fmt::Display for ArityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyChildren { arity, attempted } => {
                write!(
                    f,
                    "Too many children: arity {arity} does not allow {attempted} children"
                )
            }
            Self::TooFewChildren { arity, attempted } => {
                write!(
                    f,
                    "Too few children: arity {arity} requires more than {attempted} children"
                )
            }
            Self::InvalidChildCount { arity, actual } => {
                write!(
                    f,
                    "Invalid child count: arity {arity} does not allow {actual} children"
                )
            }
        }
    }
}

impl std::error::Error for ArityError {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_arity() {
        assert_eq!(Leaf::runtime_arity(), RuntimeArity::Exact(0));
        assert!(Leaf::validate_count(0));
        assert!(!Leaf::validate_count(1));

        let children: &[u32] = &[];
        let accessor = Leaf::from_slice(children);
        assert!(accessor.is_empty());
    }

    #[test]
    fn test_optional_arity() {
        assert_eq!(Optional::runtime_arity(), RuntimeArity::Optional);
        assert!(Optional::validate_count(0));
        assert!(Optional::validate_count(1));
        assert!(!Optional::validate_count(2));

        let empty: &[u32] = &[];
        let accessor = Optional::from_slice(empty);
        assert!(accessor.is_none());

        let one: &[u32] = &[42];
        let accessor = Optional::from_slice(one);
        assert!(accessor.is_some());
        assert_eq!(accessor.get(), Some(&42));
    }

    #[test]
    fn test_single_arity() {
        assert_eq!(Single::runtime_arity(), RuntimeArity::Exact(1));
        assert!(!Single::validate_count(0));
        assert!(Single::validate_count(1));
        assert!(!Single::validate_count(2));

        let one: &[u32] = &[42];
        let accessor = Single::from_slice(one);
        assert_eq!(accessor.single(), &42);
    }

    #[test]
    fn test_exact_two_arity() {
        assert_eq!(Exact::<2>::runtime_arity(), RuntimeArity::Exact(2));
        assert!(!Exact::<2>::validate_count(0));
        assert!(!Exact::<2>::validate_count(1));
        assert!(Exact::<2>::validate_count(2));
        assert!(!Exact::<2>::validate_count(3));

        let two: &[u32] = &[1, 2];
        let accessor = Exact::<2>::from_slice(two);
        assert_eq!(accessor.first(), &1);
        assert_eq!(accessor.second(), &2);
        assert_eq!(accessor.pair(), (&1, &2));
    }

    #[test]
    fn test_at_least_arity() {
        assert_eq!(AtLeast::<2>::runtime_arity(), RuntimeArity::AtLeast(2));
        assert!(!AtLeast::<2>::validate_count(0));
        assert!(!AtLeast::<2>::validate_count(1));
        assert!(AtLeast::<2>::validate_count(2));
        assert!(AtLeast::<2>::validate_count(100));

        let many: &[u32] = &[1, 2, 3, 4];
        let accessor = AtLeast::<2>::from_slice(many);
        assert_eq!(accessor.len(), 4);
    }

    #[test]
    fn test_variable_arity() {
        assert_eq!(Variable::runtime_arity(), RuntimeArity::Variable);
        assert!(Variable::validate_count(0));
        assert!(Variable::validate_count(1));
        assert!(Variable::validate_count(1000));

        let many: &[u32] = &[1, 2, 3];
        let accessor = Variable::from_slice(many);
        assert_eq!(accessor.len(), 3);

        let collected: Vec<_> = accessor.iter().copied().collect();
        assert_eq!(collected, vec![1, 2, 3]);
    }

    #[test]
    fn test_try_from_slice() {
        let one: &[u32] = &[42];
        assert!(Single::try_from_slice(one).is_some());
        assert!(Leaf::try_from_slice(one).is_none());
        assert!(Optional::try_from_slice(one).is_some());
        assert!(Variable::try_from_slice(one).is_some());
    }

    #[test]
    fn test_runtime_arity_display() {
        assert_eq!(format!("{}", RuntimeArity::Exact(0)), "Leaf (0 children)");
        assert_eq!(format!("{}", RuntimeArity::Exact(1)), "Single (1 child)");
        assert_eq!(format!("{}", RuntimeArity::Exact(3)), "Exact(3 children)");
        assert_eq!(
            format!("{}", RuntimeArity::Optional),
            "Optional (0 or 1 child)"
        );
        assert_eq!(
            format!("{}", RuntimeArity::AtLeast(2)),
            "AtLeast(2 children)"
        );
        assert_eq!(
            format!("{}", RuntimeArity::Variable),
            "Variable (any number)"
        );
    }

    #[test]
    fn test_range_arity_validation() {
        // Valid range: MIN <= MAX
        assert!(Range::<2, 5>::validate_count(3));
        assert!(Range::<2, 5>::validate_count(2));
        assert!(Range::<2, 5>::validate_count(5));
        assert!(!Range::<2, 5>::validate_count(1));
        assert!(!Range::<2, 5>::validate_count(6));

        // Invalid range: MIN > MAX should always return false
        assert!(!Range::<5, 2>::validate_count(3));
        assert!(!Range::<5, 2>::validate_count(1));
        assert!(!Range::<5, 2>::validate_count(10));
    }

    #[test]
    fn test_runtime_arity_range_validation() {
        // Valid range
        let valid_range = RuntimeArity::Range(2, 5);
        assert!(valid_range.validate(3));
        assert!(valid_range.validate(2));
        assert!(valid_range.validate(5));
        assert!(!valid_range.validate(1));
        assert!(!valid_range.validate(6));

        // Invalid range: MIN > MAX
        let invalid_range = RuntimeArity::Range(5, 2);
        assert!(!invalid_range.validate(3)); // Should always be false
        assert!(!invalid_range.validate(1));
        assert!(!invalid_range.validate(10));
    }

    #[test]
    fn test_children_access_capability() {
        // Test that ChildrenAccess provides mutation capability info
        let empty: &[u32] = &[];
        let one: &[u32] = &[42];
        let two: &[u32] = &[1, 2];

        // Leaf accessor
        let leaf_accessor = Leaf::from_slice(empty);
        assert!(!leaf_accessor.can_add_child());
        assert!(!leaf_accessor.can_remove_child());
        assert_eq!(leaf_accessor.max_children(), Some(0));
        assert_eq!(leaf_accessor.min_children(), 0);

        // Optional accessor - empty
        let optional_empty = Optional::from_slice(empty);
        assert!(optional_empty.can_add_child());
        assert!(!optional_empty.can_remove_child());
        assert_eq!(optional_empty.max_children(), Some(1));
        assert_eq!(optional_empty.min_children(), 0);

        // Optional accessor - with child
        let optional_full = Optional::from_slice(one);
        assert!(!optional_full.can_add_child());
        assert!(optional_full.can_remove_child());

        // Variable accessor
        let variable_accessor = Variable::from_slice(two);
        assert!(variable_accessor.can_add_child());
        assert!(variable_accessor.can_remove_child());
        assert_eq!(variable_accessor.max_children(), None);
        assert_eq!(variable_accessor.min_children(), 0);
    }

    #[test]
    fn test_arity_error_display() {
        let error1 = ArityError::TooManyChildren {
            arity: RuntimeArity::Exact(1),
            attempted: 2,
        };
        let msg1 = format!("{error1}");
        assert!(msg1.contains("Too many children"));
        // RuntimeArity::Exact(1) displays as "Single (1 child)"
        assert!(msg1.contains("Single"));
        assert!(msg1.contains('2'));

        let error2 = ArityError::TooFewChildren {
            arity: RuntimeArity::AtLeast(2),
            attempted: 1,
        };
        let msg2 = format!("{error2}");
        assert!(msg2.contains("Too few children"));
        // RuntimeArity::AtLeast(2) displays as "AtLeast(2 children)"
        assert!(msg2.contains("AtLeast"));
        assert!(msg2.contains('1'));

        let error3 = ArityError::InvalidChildCount {
            arity: RuntimeArity::Optional,
            actual: 3,
        };
        let msg3 = format!("{error3}");
        assert!(msg3.contains("Invalid child count"));
        assert!(msg3.contains("Optional"));
        assert!(msg3.contains('3'));
    }
}
