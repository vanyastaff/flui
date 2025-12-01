//! Compile-time arity system for render nodes
//!
//! This module provides a production-grade, zero-cost abstraction for expressing
//! and validating child counts of render nodes using Rust's type system.
//!
//! # Overview
//! - Compile time: Type-level arity prevents invalid layouts from compiling.
//! - Debug builds: `debug_assert!` guards catch incorrect internal usage (no cost in release).
//! - Runtime (optional): `try_from_slice()` enables dynamic validation paths when needed.
//!
//! # Arity Forms
//! - `Leaf` — 0 children
//! - `Optional` — 0 or 1 child
//! - `Exact<1>` — exactly 1 child
//! - `Exact<2>` / `Exact<3>` — exactly 2 / 3 children
//! - `Exact<N>` — exactly N children
//! - `AtLeast<N>` — N or more children
//! - `Variable` — any number (0..)
//!
//! # Guarantees
//! - Accessors never panic when used through the typed API.
//! - Zero allocations: all accessors are thin views over slices.
//! - Ergonomic helpers for common fixed arities (`single()`, `pair()`, `triple()`).
//!
//! # Example
//! ```rust,ignore
//! impl Render<Exact<1>> for RenderPadding {
//!     fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, Exact<1>>) -> Size {
//!         let child = ctx.children().single(); // Statically guaranteed
//!         // ...
//!     }
//! }
//! ```
//!
//! # Dynamic Interop
//! If arity is only known at runtime, use `A::try_from_slice(&ids)` to get a
//! typed accessor safely.
//!
//! # Debugging Tips
//! - Enable `debug_assertions` in development builds to catch arity mismatches.
//! - Use `runtime_arity()` in error messages to report expected arity.
//! - Leverage the `ChildrenAccess` trait for flexible, arity-agnostic code.

/// Runtime arity information
///
/// Represents the runtime equivalent of compile-time arity types.
/// Used for error messages and dynamic validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeArity {
    /// Exactly N children
    Exact(usize),
    /// 0 or 1 a child
    Optional,
    /// At least N children
    AtLeast(usize),
    /// Any number of children
    Variable,
}

impl RuntimeArity {
    /// Check if the count is valid for this arity
    #[inline(always)]
    pub fn validate(&self, count: usize) -> bool {
        match self {
            Self::Exact(n) => count == *n,
            Self::AtLeast(n) => count >= *n,
            Self::Optional => count <= 1,
            Self::Variable => true,
        }
    }
}

impl std::fmt::Display for RuntimeArity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(0) => write!(f, "Leaf (0 children)"),
            Self::Exact(1) => write!(f, "Exact(1 child)"),
            Self::Exact(2) => write!(f, "Exact(2 children)"),
            Self::Exact(3) => write!(f, "Exact(3 children)"),
            Self::Exact(n) => write!(f, "Exact({} children)", n),
            Self::AtLeast(n) => write!(f, "AtLeast({} children)", n),
            Self::Optional => write!(f, "Optional (0 or 1 child)"),
            Self::Variable => write!(f, "Variable (any number)"),
        }
    }
}

/// Marker trait for compile-time arity.
///
/// Implementations are sealed; use the provided types (`Leaf`, `Optional`,
/// `Single`, `Exact<N>`, `AtLeast<N>`, `Variable`) rather than defining your own.
pub trait Arity: sealed::Sealed + Send + Sync + 'static {
    /// The accessor type for this arity
    ///
    /// Must be Copy to allow returning from trait object methods.
    type Children<'a>: ChildrenAccess + Copy;

    /// Get runtime arity information
    fn runtime_arity() -> RuntimeArity;

    /// Check if count is valid for this arity
    fn validate_count(count: usize) -> bool;

    /// Convert slice to typed accessor
    ///
    /// # Panics (debug only)
    /// Panics in debug builds if count doesn't match arity.
    /// Zero cost in release builds.
    fn from_slice(children: &[std::num::NonZeroUsize]) -> Self::Children<'_>;

    /// Try to convert slice to typed accessor
    fn try_from_slice(children: &[std::num::NonZeroUsize]) -> Option<Self::Children<'_>> {
        if Self::validate_count(children.len()) {
            Some(Self::from_slice(children))
        } else {
            None
        }
    }
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::Leaf {}
    impl Sealed for super::Optional {}
    impl Sealed for super::Variable {}
    impl<const N: usize> Sealed for super::Exact<N> {}
    impl<const N: usize> Sealed for super::AtLeast<N> {}
}

/// Trait for children access
///
/// All children accessors implement this for common operations.
///
/// Note: All children accessors are Copy (they contain only references or small arrays),
/// which ensures they can be safely returned from methods returning `A::Children<'_>`.
pub trait ChildrenAccess: std::fmt::Debug + Copy {
    /// Borrow the underlying slice of child ids.
    fn as_slice(&self) -> &[std::num::NonZeroUsize];

    /// Number of children.
    #[inline]
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// True if there are zero children.
    #[inline]
    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

/// Leaf - 0 children
///
/// For render objects with no children (e.g., Text, Image, Spacer).
#[derive(Debug, Clone, Copy)]
pub struct Leaf;

impl Arity for Leaf {
    type Children<'a> = NoChildren;

    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Exact(0)
    }

    fn validate_count(count: usize) -> bool {
        count == 0
    }

    #[inline(always)]
    fn from_slice(children: &[std::num::NonZeroUsize]) -> Self::Children<'_> {
        debug_assert!(
            children.is_empty(),
            "Leaf expects 0 children, got {}",
            children.len()
        );
        NoChildren
    }
}

/// No children accessor (for Leaf)
#[derive(Debug, Clone, Copy)]
pub struct NoChildren;

impl ChildrenAccess for NoChildren {
    /// Always returns an empty slice (leaf has no children).
    fn as_slice(&self) -> &[std::num::NonZeroUsize] {
        &[]
    }
}

/// Optional - 0 or 1 child
///
/// For render objects that can work with or without a child
/// (e.g., SizedBox, Container, ColoredBox).
#[derive(Debug, Clone, Copy)]
pub struct Optional;

impl Arity for Optional {
    type Children<'a> = OptionalChild<'a>;

    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Optional
    }

    fn validate_count(count: usize) -> bool {
        count <= 1
    }

    #[inline(always)]
    fn from_slice(children: &[std::num::NonZeroUsize]) -> Self::Children<'_> {
        debug_assert!(
            children.len() <= 1,
            "Optional expects 0 or 1 child, got {}",
            children.len()
        );
        OptionalChild { children }
    }
}

/// Optional child accessor (like `Option<T>`)
#[derive(Debug, Clone, Copy)]
pub struct OptionalChild<'a> {
    children: &'a [std::num::NonZeroUsize],
}

impl ChildrenAccess for OptionalChild<'_> {
    /// Returns the slice (length 0 or 1).
    fn as_slice(&self) -> &[std::num::NonZeroUsize] {
        self.children
    }
}

impl<'a> OptionalChild<'a> {
    /// Get the optional child
    #[inline(always)]
    pub fn get(&self) -> Option<std::num::NonZeroUsize> {
        self.children.first().copied()
    }

    /// Check if child exists
    #[inline(always)]
    pub fn is_some(&self) -> bool {
        !self.children.is_empty()
    }

    /// Check if no child
    #[inline(always)]
    pub fn is_none(&self) -> bool {
        self.children.is_empty()
    }

    /// Get child or panic
    #[inline(always)]
    pub fn unwrap(&self) -> std::num::NonZeroUsize {
        self.children
            .first()
            .copied()
            .expect("Optional child is None")
    }

    /// Get child or default
    #[inline(always)]
    pub fn unwrap_or(&self, default: std::num::NonZeroUsize) -> std::num::NonZeroUsize {
        self.children.first().copied().unwrap_or(default)
    }

    /// Map over the child
    #[inline]
    pub fn map<F, T>(&self, f: F) -> Option<T>
    where
        F: FnOnce(std::num::NonZeroUsize) -> T,
    {
        self.children.first().copied().map(f)
    }

    /// Map or return default
    #[inline]
    pub fn map_or<F, T>(&self, default: T, f: F) -> T
    where
        F: FnOnce(std::num::NonZeroUsize) -> T,
    {
        self.children.first().copied().map(f).unwrap_or(default)
    }

    /// Map or compute default
    #[inline]
    pub fn map_or_else<F, D, T>(&self, default: D, f: F) -> T
    where
        F: FnOnce(std::num::NonZeroUsize) -> T,
        D: FnOnce() -> T,
    {
        self.children
            .first()
            .copied()
            .map(f)
            .unwrap_or_else(default)
    }
}

/// Const generic exact arity (exactly `N` children).
#[derive(Debug, Clone, Copy)]
pub struct Exact<const N: usize>;

impl<const N: usize> Arity for Exact<N> {
    type Children<'a> = FixedChildren<'a, N>;

    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Exact(N)
    }

    fn validate_count(count: usize) -> bool {
        count == N
    }

    #[inline(always)]
    fn from_slice(children: &[std::num::NonZeroUsize]) -> Self::Children<'_> {
        debug_assert!(
            children.len() == N,
            "Exact<{}> expects {} children, got {}",
            N,
            N,
            children.len()
        );
        // Safe: we've validated the length
        let arr: &[std::num::NonZeroUsize; N] =
            children.try_into().expect("slice length already validated");
        FixedChildren { children: arr }
    }
}

/// Fixed children accessor (for `Exact<N>`)
#[derive(Debug, Clone, Copy)]
pub struct FixedChildren<'a, const N: usize> {
    children: &'a [std::num::NonZeroUsize; N],
}

impl<'a, const N: usize> ChildrenAccess for FixedChildren<'a, N> {
    /// Returns the fixed-size slice of children.
    fn as_slice(&self) -> &[std::num::NonZeroUsize] {
        self.children
    }
}

impl<'a> FixedChildren<'a, 1> {
    /// Return the single child (guaranteed to exist).
    #[inline(always)]
    pub fn single(&self) -> std::num::NonZeroUsize {
        self.children[0]
    }
}

impl<'a> FixedChildren<'a, 2> {
    /// First child.
    #[inline(always)]
    pub fn first(&self) -> std::num::NonZeroUsize {
        self.children[0]
    }
    /// Second child.
    #[inline(always)]
    pub fn second(&self) -> std::num::NonZeroUsize {
        self.children[1]
    }
    /// Both children as a tuple.
    #[inline(always)]
    pub fn pair(&self) -> (std::num::NonZeroUsize, std::num::NonZeroUsize) {
        (self.children[0], self.children[1])
    }
}

impl<'a> FixedChildren<'a, 3> {
    /// All three children as a tuple.
    #[inline(always)]
    pub fn triple(
        &self,
    ) -> (
        std::num::NonZeroUsize,
        std::num::NonZeroUsize,
        std::num::NonZeroUsize,
    ) {
        (self.children[0], self.children[1], self.children[2])
    }
}

/// Exactly one child (alias for `Exact<1>`).
pub type Single = Exact<1>;

/// At least N children
#[derive(Debug, Clone, Copy)]
pub struct AtLeast<const N: usize>;

impl<const N: usize> Arity for AtLeast<N> {
    type Children<'a> = SliceChildren<'a>;

    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::AtLeast(N)
    }

    fn validate_count(count: usize) -> bool {
        count >= N
    }

    #[inline(always)]
    fn from_slice(children: &[std::num::NonZeroUsize]) -> Self::Children<'_> {
        debug_assert!(
            children.len() >= N,
            "AtLeast<{}> expects >= {} children, got {}",
            N,
            N,
            children.len()
        );
        SliceChildren { children }
    }
}

/// Variable number of children (any count)
#[derive(Debug, Clone, Copy)]
pub struct Variable;

impl Arity for Variable {
    type Children<'a> = SliceChildren<'a>;

    fn runtime_arity() -> RuntimeArity {
        RuntimeArity::Variable
    }

    fn validate_count(_: usize) -> bool {
        true
    }

    #[inline(always)]
    fn from_slice(children: &[std::num::NonZeroUsize]) -> Self::Children<'_> {
        SliceChildren { children }
    }
}

/// Slice children accessor (for Variable and AtLeast)
#[derive(Debug, Clone, Copy)]
pub struct SliceChildren<'a> {
    children: &'a [std::num::NonZeroUsize],
}

impl ChildrenAccess for SliceChildren<'_> {
    /// Returns the backing slice.
    fn as_slice(&self) -> &[std::num::NonZeroUsize] {
        self.children
    }
}

impl<'a> SliceChildren<'a> {
    /// Get child at index or None if out of bounds.
    #[inline(always)]
    pub fn get(&self, index: usize) -> Option<std::num::NonZeroUsize> {
        self.children.get(index).copied()
    }

    /// Iterator over all children (by value).
    #[inline(always)]
    pub fn iter(&self) -> impl Iterator<Item = std::num::NonZeroUsize> + '_ {
        self.children.iter().copied()
    }

    /// First child, if any.
    #[inline(always)]
    pub fn first(&self) -> Option<std::num::NonZeroUsize> {
        self.children.first().copied()
    }

    /// Last child, if any.
    #[inline(always)]
    pub fn last(&self) -> Option<std::num::NonZeroUsize> {
        self.children.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_arity() {
        assert_eq!(Leaf::runtime_arity(), RuntimeArity::Exact(0));
        assert!(Leaf::validate_count(0));
        assert!(!Leaf::validate_count(1));
    }

    #[test]
    fn test_optional_arity() {
        assert_eq!(Optional::runtime_arity(), RuntimeArity::Optional);
        assert!(Optional::validate_count(0));
        assert!(Optional::validate_count(1));
        assert!(!Optional::validate_count(2));
    }

    #[test]
    fn test_exact_one_arity() {
        assert_eq!(Exact::<1>::runtime_arity(), RuntimeArity::Exact(1));
        assert!(!Exact::<1>::validate_count(0));
        assert!(Exact::<1>::validate_count(1));
        assert!(!Exact::<1>::validate_count(2));
    }

    #[test]
    fn test_exact_two_arity() {
        assert_eq!(Exact::<2>::runtime_arity(), RuntimeArity::Exact(2));
        assert!(!Exact::<2>::validate_count(0));
        assert!(!Exact::<2>::validate_count(1));
        assert!(Exact::<2>::validate_count(2));
        assert!(!Exact::<2>::validate_count(3));
    }

    #[test]
    fn test_variable_arity() {
        assert_eq!(Variable::runtime_arity(), RuntimeArity::Variable);
        assert!(Variable::validate_count(0));
        assert!(Variable::validate_count(1));
        assert!(Variable::validate_count(1000));
    }

    #[test]
    fn test_at_least_arity() {
        assert_eq!(AtLeast::<2>::runtime_arity(), RuntimeArity::AtLeast(2));
        assert!(!AtLeast::<2>::validate_count(0));
        assert!(!AtLeast::<2>::validate_count(1));
        assert!(AtLeast::<2>::validate_count(2));
        assert!(AtLeast::<2>::validate_count(100));
    }

    #[test]
    fn test_optional_child_like_option() {
        use std::num::NonZeroUsize;

        let child_id = NonZeroUsize::new(1).unwrap();
        let children = [child_id];
        let optional = Optional::from_slice(&children);

        assert!(optional.is_some());
        assert!(!optional.is_none());
        assert_eq!(optional.get(), Some(child_id));
        assert_eq!(optional.unwrap(), child_id);
    }

    #[test]
    fn test_optional_empty() {
        let children = [];
        let optional = Optional::from_slice(&children);

        assert!(optional.is_none());
        assert!(!optional.is_some());
        assert_eq!(optional.get(), None);
    }

    #[test]
    fn test_fixed_children_single() {
        use std::num::NonZeroUsize;
        let child = NonZeroUsize::new(1).unwrap();
        let children = [child];
        let fixed = Exact::<1>::from_slice(&children);
        assert_eq!(fixed.single(), child);
    }

    #[test]
    fn test_fixed_children_pair() {
        use std::num::NonZeroUsize;
        let a = NonZeroUsize::new(1).unwrap();
        let b = NonZeroUsize::new(2).unwrap();
        let children = [a, b];
        let fixed = Exact::<2>::from_slice(&children);
        assert_eq!(fixed.first(), a);
        assert_eq!(fixed.second(), b);
        assert_eq!(fixed.pair(), (a, b));
    }

    #[test]
    fn test_slice_children() {
        use std::num::NonZeroUsize;

        let ids: Vec<_> = (1..=5).map(|i| NonZeroUsize::new(i).unwrap()).collect();
        let slice = Variable::from_slice(&ids);

        assert_eq!(slice.len(), 5);
        assert_eq!(slice.first(), Some(ids[0]));
        assert_eq!(slice.last(), Some(ids[4]));
        assert_eq!(slice.get(2), Some(ids[2]));
        assert_eq!(slice.get(10), None);

        let collected: Vec<_> = slice.iter().collect();
        assert_eq!(collected, ids);
    }

    // Property-based tests using quickcheck
    #[cfg(test)]
    mod property_tests {
        use super::*;
        use quickcheck::{quickcheck, TestResult};
        use std::num::NonZeroUsize;

        // Helper: create NonZeroUsize vec safely
        fn make_children(count: usize) -> Vec<NonZeroUsize> {
            (1..=count).map(|i| NonZeroUsize::new(i).unwrap()).collect()
        }

        #[test]
        fn prop_leaf_rejects_any_children() {
            fn prop(count: usize) -> TestResult {
                if count > 100 {
                    return TestResult::discard();
                }
                TestResult::from_bool(Leaf::validate_count(count) == (count == 0))
            }
            quickcheck(prop as fn(usize) -> TestResult);
        }

        #[test]
        fn prop_optional_accepts_zero_or_one() {
            fn prop(count: usize) -> TestResult {
                if count > 100 {
                    return TestResult::discard();
                }
                TestResult::from_bool(Optional::validate_count(count) == (count <= 1))
            }
            quickcheck(prop as fn(usize) -> TestResult);
        }

        #[test]
        fn prop_single_accepts_only_one() {
            fn prop(count: usize) -> TestResult {
                if count > 100 {
                    return TestResult::discard();
                }
                TestResult::from_bool(Exact::<1>::validate_count(count) == (count == 1))
            }
            quickcheck(prop as fn(usize) -> TestResult);
        }

        #[test]
        fn prop_variable_accepts_all() {
            fn prop(count: usize) -> TestResult {
                if count > 100 {
                    return TestResult::discard();
                }
                TestResult::from_bool(Variable::validate_count(count))
            }
            quickcheck(prop as fn(usize) -> TestResult);
        }

        #[test]
        fn prop_at_least_n_validates_correctly() {
            fn prop(min: usize, count: usize) -> TestResult {
                if min > 10 || count > 100 {
                    return TestResult::discard();
                }

                let valid = match min {
                    0 => AtLeast::<0>::validate_count(count),
                    1 => AtLeast::<1>::validate_count(count),
                    2 => AtLeast::<2>::validate_count(count),
                    3 => AtLeast::<3>::validate_count(count),
                    _ => return TestResult::discard(),
                };

                TestResult::from_bool(valid == (count >= min))
            }
            quickcheck(prop as fn(usize, usize) -> TestResult);
        }

        #[test]
        fn prop_exact_n_validates_correctly() {
            fn prop(n: usize, count: usize) -> quickcheck::TestResult {
                if n > 10 || count > 100 {
                    return quickcheck::TestResult::discard();
                }
                let valid = match n {
                    0 => Leaf::validate_count(count),
                    1 => Exact::<1>::validate_count(count),
                    2 => Exact::<2>::validate_count(count),
                    3 => Exact::<3>::validate_count(count),
                    _ => return quickcheck::TestResult::discard(),
                };
                quickcheck::TestResult::from_bool(valid == (count == n))
            }
            quickcheck::quickcheck(prop as fn(usize, usize) -> quickcheck::TestResult);
        }

        #[test]
        fn prop_from_slice_preserves_length() {
            fn prop(count: usize) -> TestResult {
                if count > 100 {
                    return TestResult::discard();
                }

                let children = make_children(count);
                let slice = Variable::from_slice(&children);

                TestResult::from_bool(slice.len() == count)
            }
            quickcheck(prop as fn(usize) -> TestResult);
        }

        #[test]
        fn prop_slice_children_iter_matches_vec() {
            fn prop(count: usize) -> TestResult {
                if count > 100 {
                    return TestResult::discard();
                }

                let children = make_children(count);
                let slice = Variable::from_slice(&children);
                let collected: Vec<_> = slice.iter().collect();

                TestResult::from_bool(collected == children)
            }
            quickcheck(prop as fn(usize) -> TestResult);
        }

        #[test]
        fn prop_optional_child_none_when_empty() {
            fn prop() -> bool {
                let children = [];
                let optional = Optional::from_slice(&children);
                optional.is_none() && optional.get().is_none()
            }
            quickcheck(prop as fn() -> bool);
        }

        #[test]
        fn prop_optional_child_some_when_one() {
            fn prop() -> bool {
                let child = NonZeroUsize::new(42).unwrap();
                let children = [child];
                let optional = Optional::from_slice(&children);
                optional.is_some() && optional.get() == Some(child)
            }
            quickcheck(prop as fn() -> bool);
        }
    }
}
