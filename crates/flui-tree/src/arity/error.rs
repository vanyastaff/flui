//! Arity error types.
//!
//! This module provides error types for arity constraint violations.

use super::RuntimeArity;

// ============================================================================
// ARITY ERROR
// ============================================================================

/// Error type for arity violations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArityError {
    /// Too many children for the arity constraint.
    TooManyChildren {
        /// The arity constraint.
        arity: RuntimeArity,
        /// The attempted child count.
        attempted: usize,
    },
    /// Too few children for the arity constraint.
    TooFewChildren {
        /// The arity constraint.
        arity: RuntimeArity,
        /// The attempted child count.
        attempted: usize,
    },
    /// Invalid child count for the arity constraint.
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
    pub const fn too_many(arity: RuntimeArity, attempted: usize) -> Self {
        Self::TooManyChildren { arity, attempted }
    }

    /// Create a "too few children" error.
    #[inline]
    pub const fn too_few(arity: RuntimeArity, attempted: usize) -> Self {
        Self::TooFewChildren { arity, attempted }
    }

    /// Create an "invalid child count" error.
    #[inline]
    pub const fn invalid(arity: RuntimeArity, actual: usize) -> Self {
        Self::InvalidChildCount { arity, actual }
    }

    /// Get the arity constraint that was violated.
    #[inline]
    pub const fn arity(&self) -> &RuntimeArity {
        match self {
            Self::TooManyChildren { arity, .. } => arity,
            Self::TooFewChildren { arity, .. } => arity,
            Self::InvalidChildCount { arity, .. } => arity,
        }
    }

    /// Get the count that caused the error.
    #[inline]
    pub const fn count(&self) -> usize {
        match self {
            Self::TooManyChildren { attempted, .. } => *attempted,
            Self::TooFewChildren { attempted, .. } => *attempted,
            Self::InvalidChildCount { actual, .. } => *actual,
        }
    }
}

impl std::fmt::Display for ArityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyChildren { arity, attempted } => {
                write!(
                    f,
                    "Too many children: {arity} does not allow {attempted} children"
                )
            }
            Self::TooFewChildren { arity, attempted } => {
                write!(
                    f,
                    "Too few children: {arity} requires more than {attempted} children"
                )
            }
            Self::InvalidChildCount { arity, actual } => {
                write!(
                    f,
                    "Invalid child count: {arity} does not allow {actual} children"
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
        assert!(msg.contains("Too many"));
        assert!(msg.contains("Optional"));

        let err = ArityError::too_few(RuntimeArity::AtLeast(3), 1);
        let msg = format!("{}", err);
        assert!(msg.contains("Too few"));

        let err = ArityError::invalid(RuntimeArity::Exact(5), 3);
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid"));
    }
}
