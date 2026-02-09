//! Arity error types.
//!
//! This module provides error types for arity constraint violations.

use thiserror::Error;

use super::RuntimeArity;

// ============================================================================
// ARITY ERROR
// ============================================================================

/// Error type for arity violations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
#[non_exhaustive]
pub enum ArityError {
    /// Too many children for the arity constraint.
    #[error("too many children: {arity} does not allow {attempted} children")]
    TooManyChildren {
        /// The arity constraint.
        arity: RuntimeArity,
        /// The attempted child count.
        attempted: usize,
    },
    /// Too few children for the arity constraint.
    #[error("too few children: {arity} requires more than {attempted} children")]
    TooFewChildren {
        /// The arity constraint.
        arity: RuntimeArity,
        /// The attempted child count.
        attempted: usize,
    },
    /// Invalid child count for the arity constraint.
    #[error("invalid child count: {arity} does not allow {actual} children")]
    InvalidChildCount {
        /// The arity constraint.
        arity: RuntimeArity,
        /// The actual child count.
        actual: usize,
    },
}

impl ArityError {
    /// Create a "too many children" error.
    #[inline]
    #[must_use]
    pub const fn too_many(arity: RuntimeArity, attempted: usize) -> Self {
        Self::TooManyChildren { arity, attempted }
    }

    /// Create a "too few children" error.
    #[inline]
    #[must_use]
    pub const fn too_few(arity: RuntimeArity, attempted: usize) -> Self {
        Self::TooFewChildren { arity, attempted }
    }

    /// Create an "invalid child count" error.
    #[inline]
    #[must_use]
    pub const fn invalid(arity: RuntimeArity, actual: usize) -> Self {
        Self::InvalidChildCount { arity, actual }
    }

    /// Get the arity constraint that was violated.
    #[inline]
    #[must_use]
    pub const fn arity(&self) -> &RuntimeArity {
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
        let err = ArityError::too_many(RuntimeArity::Optional, 2);
        assert_eq!(err.count(), 2);
        assert_eq!(*err.arity(), RuntimeArity::Optional);

        let err = ArityError::too_few(RuntimeArity::AtLeast(3), 1);
        assert_eq!(err.count(), 1);

        let err = ArityError::invalid(RuntimeArity::Exact(5), 3);
        assert_eq!(err.count(), 3);
    }

    #[test]
    fn test_error_display() {
        let err = ArityError::too_many(RuntimeArity::Optional, 2);
        let msg = format!("{}", err);
        assert!(msg.contains("too many"));
        assert!(msg.contains("Optional"));

        let err = ArityError::too_few(RuntimeArity::AtLeast(3), 1);
        let msg = format!("{}", err);
        assert!(msg.contains("too few"));

        let err = ArityError::invalid(RuntimeArity::Exact(5), 3);
        let msg = format!("{}", err);
        assert!(msg.contains("invalid child count"));
    }
}
