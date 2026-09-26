//! Compile-time arity markers for tree nodes.
//!
//! The arity system expresses a node's child-count constraint as a
//! zero-sized marker type. A render object names its marker in its type,
//! and the protocol code uses the marker to pick the child storage and
//! accessors it offers:
//!
//! ```ignore
//! use flui_foundation::{Leaf, Single, Variable};
//!
//! struct RenderText { /* … */ }             // marker: Leaf
//! struct RenderPadding<C> { child: C }      // marker: Single
//! struct RenderFlex<C> { children: Vec<C> } // marker: Variable
//! ```
//!
//! | Marker | Description | Use case |
//! |--------|-------------|----------|
//! | [`Leaf`] | 0 children | `RenderText`, `RenderColoredBox` |
//! | [`Optional`] | 0 or 1 child | `RenderSizedBox` |
//! | [`Single`] | exactly 1 child | `RenderPadding`, `RenderTransform` |
//! | [`Exact<N>`] | exactly N children | Custom layouts |
//! | [`AtLeast<N>`] | N or more children | Min-child layouts |
//! | [`Variable`] | any number | `RenderFlex`, `RenderStack` |
//! | [`Range<MIN, MAX>`] | bounded range | Constrained layouts |
//! | [`Never`] | uninhabited | Type-system bottom |
//!
//! [`Range`] is not in the crate prelude: a glob import would shadow
//! `std::ops::Range`.

use std::fmt::Debug;

// ============================================================================
// ARITY TRAIT
// ============================================================================

/// Compile-time marker trait for tree-node arity (child-count
/// constraint).
///
/// Implemented by the canonical markers:
/// - [`Leaf`] — 0 children
/// - [`Optional`] — 0 or 1 child
/// - [`Exact<N>`] — exactly N children (`Single = Exact<1>`)
/// - [`AtLeast<N>`] — N or more children
/// - [`Variable`] — any number
/// - [`Range<MIN, MAX>`] — bounded range
/// - [`Never`] — uninhabited (type-system bottom)
///
/// The trait is sealed — only the markers in this module implement it. The
/// `Send + Sync + Debug + Copy + Default + 'static` super-bounds let arity
/// markers cross thread boundaries, derive `Debug`, and be plugged into
/// generic code that expects a zero-sized type.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an arity marker",
    label = "expected `Leaf`, `Optional`, `Variable`, `Exact<N>`, `AtLeast<N>`, `Range<MIN, MAX>`, or `Never`",
    note = "`Arity` is sealed: only the markers in `flui_foundation::arity` implement it, so a node's child count is checked at compile time rather than at layout"
)]
pub trait Arity: sealed::Sealed + Send + Sync + Debug + Copy + Default + 'static {
    /// Static description of this arity (e.g. `"Leaf"`, `"Exact<N>"`), for
    /// diagnostics.
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

mod sealed {
    use super::{AtLeast, Exact, Leaf, Never, Optional, Range, Variable};

    pub trait Sealed {}

    impl Sealed for Leaf {}
    impl Sealed for Optional {}
    impl<const N: usize> Sealed for Exact<N> {}
    impl<const N: usize> Sealed for AtLeast<N> {}
    impl Sealed for Variable {}
    impl<const MIN: usize, const MAX: usize> Sealed for Range<MIN, MAX> {}
    impl Sealed for Never {}
}

// ============================================================================
// LEAF (0 children)
// ============================================================================

/// Leaf arity — 0 children. For nodes that never have children
/// (e.g. `Text`, `Image`, `ColoredBox`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Leaf;

impl Arity for Leaf {
    const DESCRIPTION: &'static str = "Leaf";

    #[inline]
    fn validate_count(count: usize) -> bool {
        count == 0
    }
}

// ============================================================================
// OPTIONAL (0 or 1 child)
// ============================================================================

/// Optional arity — 0 or 1 child.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Optional;

impl Arity for Optional {
    const DESCRIPTION: &'static str = "Optional";

    #[inline]
    fn validate_count(count: usize) -> bool {
        count <= 1
    }
}

// ============================================================================
// EXACT<N> (exactly N children)
// ============================================================================

/// Exact arity — exactly N children at compile time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Exact<const N: usize>;

impl<const N: usize> Arity for Exact<N> {
    const DESCRIPTION: &'static str = "Exact<N>";

    #[inline]
    fn validate_count(count: usize) -> bool {
        count == N
    }
}

/// Single arity — exactly 1 child. Type alias for `Exact<1>` (most
/// common case: `Padding`, `Transform`, etc.).
pub type Single = Exact<1>;

// ============================================================================
// AT_LEAST<N> (N or more children)
// ============================================================================

/// `AtLeast` arity — N or more children at compile time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AtLeast<const N: usize>;

impl<const N: usize> Arity for AtLeast<N> {
    const DESCRIPTION: &'static str = "AtLeast<N>";

    #[inline]
    fn validate_count(count: usize) -> bool {
        count >= N
    }
}

// ============================================================================
// VARIABLE (any number)
// ============================================================================

/// Variable arity — any number of children (Flex, Stack, Column).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Variable;

impl Arity for Variable {
    const DESCRIPTION: &'static str = "Variable";

    #[inline]
    fn validate_count(_count: usize) -> bool {
        true
    }
}

// ============================================================================
// RANGE<MIN, MAX> (bounded range)
// ============================================================================

/// Range arity — bounded range at compile time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Range<const MIN: usize, const MAX: usize>;

impl<const MIN: usize, const MAX: usize> Arity for Range<MIN, MAX> {
    const DESCRIPTION: &'static str = "Range<MIN, MAX>";

    #[inline]
    fn validate_count(count: usize) -> bool {
        count >= MIN && count <= MAX
    }
}

// ============================================================================
// NEVER (uninhabited)
// ============================================================================

/// Never arity — type-system bottom. No node can have this arity at
/// runtime; useful as a generic-parameter placeholder for "this
/// branch is impossible."
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Never;

impl Arity for Never {
    const DESCRIPTION: &'static str = "Never";

    #[inline]
    fn validate_count(_count: usize) -> bool {
        false
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_validates_zero() {
        assert!(Leaf::validate_count(0));
        assert!(!Leaf::validate_count(1));
    }

    #[test]
    fn optional_validates_zero_or_one() {
        assert!(Optional::validate_count(0));
        assert!(Optional::validate_count(1));
        assert!(!Optional::validate_count(2));
    }

    #[test]
    fn single_validates_exactly_one() {
        assert!(!Single::validate_count(0));
        assert!(Single::validate_count(1));
        assert!(!Single::validate_count(2));
    }

    #[test]
    fn exact_n_validates_n() {
        assert!(Exact::<3>::validate_count(3));
        assert!(!Exact::<3>::validate_count(2));
        assert!(!Exact::<3>::validate_count(4));
    }

    #[test]
    fn at_least_n_validates_at_least_n() {
        assert!(!AtLeast::<2>::validate_count(1));
        assert!(AtLeast::<2>::validate_count(2));
        assert!(AtLeast::<2>::validate_count(100));
    }

    #[test]
    fn variable_validates_any() {
        assert!(Variable::validate_count(0));
        assert!(Variable::validate_count(100));
    }

    #[test]
    fn range_validates_bounds() {
        assert!(!Range::<2, 5>::validate_count(1));
        assert!(Range::<2, 5>::validate_count(2));
        assert!(Range::<2, 5>::validate_count(5));
        assert!(!Range::<2, 5>::validate_count(6));
    }

    #[test]
    fn never_validates_nothing() {
        assert!(!Never::validate_count(0));
        assert!(!Never::validate_count(1));
    }

    #[test]
    fn descriptions_are_meaningful() {
        assert_eq!(Leaf::DESCRIPTION, "Leaf");
        assert_eq!(Single::DESCRIPTION, "Exact<N>");
        assert_eq!(Variable::DESCRIPTION, "Variable");
        assert_eq!(Never::DESCRIPTION, "Never");
    }
}
