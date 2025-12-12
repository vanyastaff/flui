//! Runtime arity information and performance hints.
//!
//! This module provides runtime representations of compile-time arity types,
//! useful for error messages, debugging, dynamic validation, and performance optimization.

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
    #[inline(always)]
    pub const fn validate(&self, count: usize) -> bool {
        match self {
            Self::Exact(n) => count == *n,
            Self::Optional => count <= 1,
            Self::AtLeast(n) => count >= *n,
            Self::Variable => true,
            Self::Range(min, max) => count >= *min && count <= *max,
            Self::Never => false,
        }
    }

    /// Check if this arity is impossible (Never type).
    #[inline(always)]
    pub const fn is_impossible(&self) -> bool {
        matches!(self, Self::Never)
    }

    /// Get the minimum valid count for this arity.
    #[inline(always)]
    pub const fn min_count(&self) -> usize {
        match self {
            Self::Exact(n) => *n,
            Self::Optional => 0,
            Self::AtLeast(n) => *n,
            Self::Variable => 0,
            Self::Range(min, _) => *min,
            Self::Never => usize::MAX,
        }
    }

    /// Get the maximum valid count for this arity (None = unbounded).
    #[inline(always)]
    pub const fn max_count(&self) -> Option<usize> {
        match self {
            Self::Exact(n) => Some(*n),
            Self::Optional => Some(1),
            Self::AtLeast(_) => None,
            Self::Variable => None,
            Self::Range(_, max) => Some(*max),
            Self::Never => Some(0),
        }
    }

    /// Check if this arity allows the given count with performance hint.
    pub const fn validate_with_hint(&self, count: usize) -> (bool, PerformanceHint) {
        let valid = self.validate(count);
        let hint = match self {
            Self::Exact(_) | Self::Optional => PerformanceHint::FixedSize,
            Self::AtLeast(_) if count < 32 => PerformanceHint::SmallDynamic,
            Self::AtLeast(_) => PerformanceHint::LargeDynamic,
            Self::Variable if count < 16 => PerformanceHint::SmallDynamic,
            Self::Variable => PerformanceHint::LargeDynamic,
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
            Self::Exact(n) => write!(f, "Exact({} children)", n),
            Self::Optional => write!(f, "Optional (0 or 1 child)"),
            Self::AtLeast(n) => write!(f, "AtLeast({} children)", n),
            Self::Variable => write!(f, "Variable (any number)"),
            Self::Range(min, max) => write!(f, "Range({}-{} children)", min, max),
            Self::Never => write!(f, "Never (impossible)"),
        }
    }
}

// ============================================================================
// PERFORMANCE HINT
// ============================================================================

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
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_arity_validate() {
        assert!(RuntimeArity::Exact(3).validate(3));
        assert!(!RuntimeArity::Exact(3).validate(2));

        assert!(RuntimeArity::Optional.validate(0));
        assert!(RuntimeArity::Optional.validate(1));
        assert!(!RuntimeArity::Optional.validate(2));

        assert!(RuntimeArity::AtLeast(2).validate(2));
        assert!(RuntimeArity::AtLeast(2).validate(100));
        assert!(!RuntimeArity::AtLeast(2).validate(1));

        assert!(RuntimeArity::Variable.validate(0));
        assert!(RuntimeArity::Variable.validate(1000));

        assert!(RuntimeArity::Range(2, 5).validate(3));
        assert!(!RuntimeArity::Range(2, 5).validate(1));
        assert!(!RuntimeArity::Range(2, 5).validate(6));

        assert!(!RuntimeArity::Never.validate(0));
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
        assert_eq!(
            format!("{}", RuntimeArity::Range(2, 5)),
            "Range(2-5 children)"
        );
        assert_eq!(format!("{}", RuntimeArity::Never), "Never (impossible)");
    }

    #[test]
    fn test_min_max_count() {
        assert_eq!(RuntimeArity::Exact(3).min_count(), 3);
        assert_eq!(RuntimeArity::Exact(3).max_count(), Some(3));

        assert_eq!(RuntimeArity::Optional.min_count(), 0);
        assert_eq!(RuntimeArity::Optional.max_count(), Some(1));

        assert_eq!(RuntimeArity::AtLeast(2).min_count(), 2);
        assert_eq!(RuntimeArity::AtLeast(2).max_count(), None);

        assert_eq!(RuntimeArity::Variable.min_count(), 0);
        assert_eq!(RuntimeArity::Variable.max_count(), None);

        assert_eq!(RuntimeArity::Range(2, 5).min_count(), 2);
        assert_eq!(RuntimeArity::Range(2, 5).max_count(), Some(5));
    }

    #[test]
    fn test_is_impossible() {
        assert!(!RuntimeArity::Exact(0).is_impossible());
        assert!(!RuntimeArity::Variable.is_impossible());
        assert!(RuntimeArity::Never.is_impossible());
    }
}
