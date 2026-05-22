//! Concrete arity marker types.
//!
//! Cycle 3 T-7: rewritten as pure zero-sized markers after the
//! accessor/runtime machinery was deleted. Each marker is a unit
//! struct (or zero-sized generic) implementing the simplified
//! [`super::Arity`] trait with `DESCRIPTION` + `validate_count`.
//!
//! Concrete render objects use plain `Option<C>` / `Vec<C>` / fixed
//! array storage attached to the marker — no runtime dispatch, no
//! `ChildrenAccess` indirection.

use super::traits::Arity;

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
