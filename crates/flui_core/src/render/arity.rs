//! Arity - child count specification for render objects
//!
//! This module provides the `Arity` enum which specifies how many children
//! a render object expects. Used for runtime validation during element mounting.

/// Arity specification for render objects
///
/// Specifies the expected number of children for a render object.
/// Used for runtime validation to catch bugs early.
///
/// # Variants
///
/// - `Exact(n)`: Exactly n children required
/// - `Variable`: Any number of children allowed
///
/// # Examples
///
/// ```rust,ignore
/// // Leaf render - no children
/// fn arity(&self) -> Arity {
///     Arity::Exact(0)
/// }
///
/// // Single child render
/// fn arity(&self) -> Arity {
///     Arity::Exact(1)
/// }
///
/// // Multi-child render
/// fn arity(&self) -> Arity {
///     Arity::Variable
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arity {
    /// Exact number of children required
    ///
    /// Use `Exact(0)` for leaf nodes (no children).
    /// Use `Exact(1)` for single-child wrappers.
    /// Use `Exact(n)` for fixed-arity layouts (e.g., `Exact(2)` for a Split pane).
    Exact(usize),

    /// Variable number of children allowed
    ///
    /// Use this for multi-child containers like Flex, Stack, Wrap, etc.
    /// Allows 0, 1, 2, or any number of children.
    Variable,
}

impl Arity {
    /// Validate child count against arity specification
    ///
    /// Returns `Ok(())` if the child count matches the arity,
    /// `Err(message)` if validation fails.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let arity = Arity::Exact(1);
    /// assert!(arity.validate(1).is_ok());
    /// assert!(arity.validate(0).is_err());
    /// assert!(arity.validate(2).is_err());
    ///
    /// let arity = Arity::Variable;
    /// assert!(arity.validate(0).is_ok());
    /// assert!(arity.validate(1).is_ok());
    /// assert!(arity.validate(100).is_ok());
    /// ```
    pub fn validate(&self, actual: usize) -> Result<(), String> {
        match self {
            Arity::Exact(expected) => {
                if actual == *expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Arity mismatch: expected exactly {} children, got {}",
                        expected, actual
                    ))
                }
            }
            Arity::Variable => Ok(()),
        }
    }

    /// Check if arity matches the given count
    ///
    /// Returns `true` if the count is valid for this arity.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let arity = Arity::Exact(2);
    /// assert!(!arity.matches(0));
    /// assert!(!arity.matches(1));
    /// assert!(arity.matches(2));
    /// assert!(!arity.matches(3));
    ///
    /// let arity = Arity::Variable;
    /// assert!(arity.matches(0));
    /// assert!(arity.matches(1));
    /// assert!(arity.matches(100));
    /// ```
    #[inline]
    pub fn matches(&self, actual: usize) -> bool {
        self.validate(actual).is_ok()
    }

    /// Check if this arity allows no children
    ///
    /// Returns `true` for `Exact(0)` or `Variable`.
    #[inline]
    pub fn allows_zero(&self) -> bool {
        matches!(self, Arity::Exact(0) | Arity::Variable)
    }

    /// Check if this arity requires exactly one child
    #[inline]
    pub fn is_single(&self) -> bool {
        matches!(self, Arity::Exact(1))
    }

    /// Check if this arity is variable
    #[inline]
    pub fn is_variable(&self) -> bool {
        matches!(self, Arity::Variable)
    }

    /// Check if this arity is exact
    #[inline]
    pub fn is_exact(&self) -> bool {
        matches!(self, Arity::Exact(_))
    }

    /// Get the exact count if this is an exact arity
    ///
    /// Returns `Some(n)` for `Exact(n)`, `None` for `Variable`.
    #[inline]
    pub fn exact_count(&self) -> Option<usize> {
        match self {
            Arity::Exact(n) => Some(*n),
            Arity::Variable => None,
        }
    }
}

impl Default for Arity {
    /// Default arity is Variable (allows any number of children)
    fn default() -> Self {
        Arity::Variable
    }
}

impl std::fmt::Display for Arity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arity::Exact(n) => write!(f, "Exact({})", n),
            Arity::Variable => write!(f, "Variable"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_zero() {
        let arity = Arity::Exact(0);
        assert!(arity.validate(0).is_ok());
        assert!(arity.validate(1).is_err());
        assert!(arity.allows_zero());
        assert!(!arity.is_single());
        assert!(arity.is_exact());
        assert_eq!(arity.exact_count(), Some(0));
    }

    #[test]
    fn test_exact_one() {
        let arity = Arity::Exact(1);
        assert!(arity.validate(1).is_ok());
        assert!(arity.validate(0).is_err());
        assert!(arity.validate(2).is_err());
        assert!(!arity.allows_zero());
        assert!(arity.is_single());
        assert!(arity.is_exact());
        assert_eq!(arity.exact_count(), Some(1));
    }

    #[test]
    fn test_exact_two() {
        let arity = Arity::Exact(2);
        assert!(arity.validate(2).is_ok());
        assert!(arity.validate(0).is_err());
        assert!(arity.validate(1).is_err());
        assert!(arity.validate(3).is_err());
        assert!(!arity.allows_zero());
        assert!(!arity.is_single());
        assert!(arity.is_exact());
        assert_eq!(arity.exact_count(), Some(2));
    }

    #[test]
    fn test_variable() {
        let arity = Arity::Variable;
        assert!(arity.validate(0).is_ok());
        assert!(arity.validate(1).is_ok());
        assert!(arity.validate(100).is_ok());
        assert!(arity.allows_zero());
        assert!(!arity.is_single());
        assert!(!arity.is_exact());
        assert!(arity.is_variable());
        assert_eq!(arity.exact_count(), None);
    }

    #[test]
    fn test_matches() {
        let arity = Arity::Exact(3);
        assert!(!arity.matches(0));
        assert!(!arity.matches(2));
        assert!(arity.matches(3));
        assert!(!arity.matches(4));

        let arity = Arity::Variable;
        assert!(arity.matches(0));
        assert!(arity.matches(10));
        assert!(arity.matches(1000));
    }

    #[test]
    fn test_default() {
        let arity = Arity::default();
        assert!(matches!(arity, Arity::Variable));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Arity::Exact(0)), "Exact(0)");
        assert_eq!(format!("{}", Arity::Exact(1)), "Exact(1)");
        assert_eq!(format!("{}", Arity::Exact(5)), "Exact(5)");
        assert_eq!(format!("{}", Arity::Variable), "Variable");
    }
}
