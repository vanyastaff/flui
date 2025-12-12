//! Concrete arity marker types.
//!
//! This module defines all the concrete arity types:
//! - `Leaf` — 0 children
//! - `Optional` — 0 or 1 child
//! - `Single` — exactly 1 child (alias for `Exact<1>`)
//! - `Exact<N>` — exactly N children
//! - `AtLeast<N>` — N or more children
//! - `Variable` — any number of children
//! - `Range<MIN, MAX>` — bounded range
//! - `Never` — impossible arity

use std::marker::PhantomData;

use super::accessors::{FixedChildren, NeverAccessor, NoChildren, OptionalChild, SliceChildren};
use super::runtime::RuntimeArity;
use super::traits::Arity;

// ============================================================================
// LEAF (0 children)
// ============================================================================

/// Leaf arity marker — 0 children with enhanced type safety.
///
/// For nodes that never have children (e.g., Text, Image, Spacer).
///
/// # Example
///
/// ```rust,ignore
/// fn layout_leaf<T>(children: &[T]) {
///     let accessor = Leaf::from_slice(children);
///     assert!(accessor.is_empty());
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
    const SUPPORTS_SIMD: bool = true;

    #[inline(always)]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Exact(0)
    }

    #[inline(always)]
    fn validate_count(count: usize) -> bool {
        count == 0
    }

    #[inline(always)]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.is_empty(),
            "Leaf expects 0 children, got {}",
            children.len()
        );
        NoChildren(PhantomData)
    }

    #[inline(always)]
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
/// (e.g., SizedBox, Container, ColoredBox).
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

    #[inline(always)]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.len() <= 1,
            "Optional expects 0 or 1 child, got {}",
            children.len()
        );
        OptionalChild { children }
    }

    #[inline(always)]
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

    #[inline(always)]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.len() == N,
            "Exact<{}> expects {} children, got {}",
            N,
            N,
            children.len()
        );
        let array_ref: &[T; N] = children.try_into().expect("slice length mismatch");
        FixedChildren {
            children: array_ref,
        }
    }

    #[inline(always)]
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
pub type Single = Exact<1>;

// ============================================================================
// AT_LEAST<N> (N or more children)
// ============================================================================

/// AtLeast arity marker — N or more children.
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

    #[inline(always)]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.len() >= N,
            "AtLeast<{}> expects >= {} children, got {}",
            N,
            N,
            children.len()
        );
        SliceChildren { children }
    }

    #[inline(always)]
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

    #[inline(always)]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        SliceChildren { children }
    }

    #[inline(always)]
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        children.iter()
    }
}

// ============================================================================
// RANGE<MIN, MAX> (bounded range)
// ============================================================================

/// Range arity marker — bounded number of children with const generics.
///
/// Provides compile-time validation for bounded ranges.
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

    const EXPECTED_SIZE: usize = (MIN + MAX) / 2;
    const INLINE_THRESHOLD: usize = MAX;
    const BATCH_SIZE: usize = MAX;

    #[inline]
    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Range(MIN, MAX)
    }

    #[inline]
    fn validate_count(count: usize) -> bool {
        count >= MIN && count <= MAX
    }

    #[inline(always)]
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T> {
        debug_assert!(
            children.len() >= MIN,
            "Range<{}, {}> expects >= {} children, got {}",
            MIN,
            MAX,
            MIN,
            children.len()
        );
        debug_assert!(
            children.len() <= MAX,
            "Range<{}, {}> expects <= {} children, got {}",
            MIN,
            MAX,
            MAX,
            children.len()
        );
        SliceChildren { children }
    }

    #[inline(always)]
    fn iter_slice<'a, T>(children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        debug_assert!(children.len() >= MIN && children.len() <= MAX);
        children.iter()
    }
}

// ============================================================================
// NEVER (impossible operations)
// ============================================================================

/// Never arity marker for impossible operations.
///
/// Used for operations that should never succeed, providing compile-time
/// safety through the never type.
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
        false
    }

    #[inline(always)]
    fn from_slice<T: Send + Sync>(_children: &[T]) -> Self::Accessor<'_, T> {
        NeverAccessor(PhantomData)
    }

    #[inline(always)]
    fn iter_slice<'a, T>(_children: &'a [T]) -> Self::Iterator<'a, T>
    where
        T: 'a,
    {
        std::iter::empty()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::accessors::ChildrenAccess;

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
    fn test_range_arity() {
        assert_eq!(Range::<2, 5>::runtime_arity(), RuntimeArity::Range(2, 5));
        assert!(!Range::<2, 5>::validate_count(1));
        assert!(Range::<2, 5>::validate_count(2));
        assert!(Range::<2, 5>::validate_count(5));
        assert!(!Range::<2, 5>::validate_count(6));
    }

    #[test]
    fn test_never_arity() {
        assert_eq!(Never::runtime_arity(), RuntimeArity::Never);
        assert!(!Never::validate_count(0));
        assert!(!Never::validate_count(1));
    }

    #[test]
    fn test_try_from_slice() {
        let one: &[u32] = &[42];
        assert!(Single::try_from_slice(one).is_some());
        assert!(Leaf::try_from_slice(one).is_none());
        assert!(Optional::try_from_slice(one).is_some());
        assert!(Variable::try_from_slice(one).is_some());
    }
}
