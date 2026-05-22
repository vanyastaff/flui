//! Arity error types.
//!
//! This module provides the error type for arity constraint violations.
//!
//! Cycle 3 T-7: simplified to use `&'static str` arity descriptions
//! instead of the `RuntimeArity` enum, which was part of the zombie
//! storage machinery the cycle deleted. The error is now self-
//! describing without requiring the consumer to import a runtime
//! arity wrapper.

use thiserror::Error;

// ============================================================================
// ARITY ERROR
// ============================================================================

/// Error type for arity constraint violations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
#[non_exhaustive]
pub enum ArityError {
    /// Too many children for the arity constraint.
    #[error("too many children: arity {arity} does not allow {attempted} children")]
    TooManyChildren {
        /// The arity constraint description (e.g. "Leaf", "Single", "Optional", "Exact<3>").
        arity: &'static str,
        /// The attempted child count.
        attempted: usize,
    },
    /// Too few children for the arity constraint.
    #[error("too few children: arity {arity} requires more than {attempted} children")]
    TooFewChildren {
        /// The arity constraint description.
        arity: &'static str,
        /// The attempted child count.
        attempted: usize,
    },
    /// Invalid child count for the arity constraint.
    #[error("invalid child count: arity {arity} does not allow {actual} children")]
    InvalidChildCount {
        /// The arity constraint description.
        arity: &'static str,
        /// The actual child count.
        actual: usize,
    },
}

impl ArityError {
    /// Create a "too many children" error.
    #[inline]
    #[must_use]
    pub const fn too_many(arity: &'static str, attempted: usize) -> Self {
        Self::TooManyChildren { arity, attempted }
    }

    /// Create a "too few children" error.
    #[inline]
    #[must_use]
    pub const fn too_few(arity: &'static str, attempted: usize) -> Self {
        Self::TooFewChildren { arity, attempted }
    }

    /// Create an "invalid child count" error.
    #[inline]
    #[must_use]
    pub const fn invalid(arity: &'static str, actual: usize) -> Self {
        Self::InvalidChildCount { arity, actual }
    }

    /// Get the arity description.
    #[inline]
    #[must_use]
    pub const fn arity(&self) -> &'static str {
        match self {
            Self::TooManyChildren { arity, .. }
            | Self::TooFewChildren { arity, .. }
            | Self::InvalidChildCount { arity, .. } => arity,
        }
    }

    /// Get the count that caused the error.
    #[inline]
    #[must_use]
    pub const fn count(&self) -> usize {
        match self {
            Self::TooManyChildren { attempted, .. } | Self::TooFewChildren { attempted, .. } => {
                *attempted
            }
            Self::InvalidChildCount { actual, .. } => *actual,
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_constructors() {
        let err = ArityError::too_many("Optional", 2);
        assert_eq!(err.count(), 2);
        assert_eq!(err.arity(), "Optional");

        let err = ArityError::too_few("AtLeast<3>", 1);
        assert_eq!(err.count(), 1);

        let err = ArityError::invalid("Exact<5>", 3);
        assert_eq!(err.count(), 3);
    }

    #[test]
    fn test_error_display() {
        let err = ArityError::too_many("Optional", 2);
        let msg = format!("{err}");
        assert!(msg.contains("too many"));
        assert!(msg.contains("Optional"));

        let err = ArityError::too_few("AtLeast<3>", 1);
        let msg = format!("{err}");
        assert!(msg.contains("too few"));

        let err = ArityError::invalid("Exact<5>", 3);
        let msg = format!("{err}");
        assert!(msg.contains("invalid child count"));
    }
}
