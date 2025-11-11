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
/// - `Optional`: 0 or 1 child allowed
/// - `Range(min, max)`: Between min and max children (inclusive)
/// - `AtLeast(n)`: At least n children required
/// - `Variable`: Any number of children allowed (0..=∞)
///
/// # Examples
///
/// ```rust,ignore
/// // Leaf render - no children
/// fn arity(&self) -> Arity {
///     Arity::Exact(0)
/// }
///
/// // Single child render (required)
/// fn arity(&self) -> Arity {
///     Arity::Exact(1)
/// }
///
/// // Optional single child (0 or 1)
/// fn arity(&self) -> Arity {
///     Arity::Optional
/// }
///
/// // Range of children (2-4 inclusive)
/// fn arity(&self) -> Arity {
///     Arity::Range(2, 4)
/// }
///
/// // At least 1 child
/// fn arity(&self) -> Arity {
///     Arity::AtLeast(1)
/// }
///
/// // Multi-child render (any count)
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
    ///
    /// # Examples
    /// - `Exact(0)`: RenderText, RenderImage (leaf nodes)
    /// - `Exact(1)`: RenderPadding, RenderOpacity (single-child wrappers)
    /// - `Exact(2)`: RenderSplitPane (two-child layouts)
    Exact(usize),

    /// Optional single child (0 or 1)
    ///
    /// Use for widgets that can work with or without a child.
    ///
    /// # Examples
    /// - `Optional`: RenderSizedBox, RenderAlign, RenderContainer
    ///
    /// ```rust,ignore
    /// fn arity(&self) -> Arity {
    ///     Arity::Optional  // 0 or 1 child
    /// }
    /// ```
    Optional,

    /// Range of children (min..=max inclusive)
    ///
    /// Use for widgets with specific min/max requirements.
    ///
    /// # Examples
    /// - `Range(2, 10)`: Row/Column with min 2, max 10 children
    /// - `Range(1, 3)`: Custom layout with 1-3 children
    ///
    /// ```rust,ignore
    /// fn arity(&self) -> Arity {
    ///     Arity::Range(2, 10)  // Between 2 and 10 children
    /// }
    /// ```
    Range(usize, usize),

    /// At least n children required (no upper bound)
    ///
    /// Use for widgets that need a minimum number of children but have no maximum.
    ///
    /// # Examples
    /// - `AtLeast(1)`: ListView, GridView (at least 1 child)
    /// - `AtLeast(2)`: Comparison widget (needs at least 2 to compare)
    ///
    /// ```rust,ignore
    /// fn arity(&self) -> Arity {
    ///     Arity::AtLeast(1)  // At least 1 child
    /// }
    /// ```
    AtLeast(usize),

    /// Variable number of children allowed (0..=∞)
    ///
    /// Use this for flexible multi-child containers with no constraints.
    ///
    /// # Examples
    /// - `Variable`: Flex, Stack, Wrap, Column, Row
    ///
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
    /// let arity = Arity::Optional;
    /// assert!(arity.validate(0).is_ok());
    /// assert!(arity.validate(1).is_ok());
    /// assert!(arity.validate(2).is_err());
    ///
    /// let arity = Arity::Range(2, 4);
    /// assert!(arity.validate(1).is_err());
    /// assert!(arity.validate(2).is_ok());
    /// assert!(arity.validate(3).is_ok());
    /// assert!(arity.validate(4).is_ok());
    /// assert!(arity.validate(5).is_err());
    ///
    /// let arity = Arity::AtLeast(2);
    /// assert!(arity.validate(1).is_err());
    /// assert!(arity.validate(2).is_ok());
    /// assert!(arity.validate(100).is_ok());
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
            Arity::Optional => {
                if actual <= 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "Arity mismatch: expected 0 or 1 child, got {}",
                        actual
                    ))
                }
            }
            Arity::Range(min, max) => {
                if actual < *min {
                    Err(format!(
                        "Arity mismatch: expected at least {} children, got {}",
                        min, actual
                    ))
                } else if actual > *max {
                    Err(format!(
                        "Arity mismatch: expected at most {} children, got {}",
                        max, actual
                    ))
                } else {
                    Ok(())
                }
            }
            Arity::AtLeast(min) => {
                if actual >= *min {
                    Ok(())
                } else {
                    Err(format!(
                        "Arity mismatch: expected at least {} children, got {}",
                        min, actual
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

    /// Check if this arity allows zero children
    ///
    /// Returns `true` for `Exact(0)`, `Optional`, `Range(0, _)`, `AtLeast(0)`, or `Variable`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// assert!(Arity::Exact(0).allows_zero());
    /// assert!(Arity::Optional.allows_zero());
    /// assert!(Arity::Range(0, 5).allows_zero());
    /// assert!(Arity::AtLeast(0).allows_zero());
    /// assert!(Arity::Variable.allows_zero());
    /// assert!(!Arity::Exact(1).allows_zero());
    /// assert!(!Arity::Range(1, 5).allows_zero());
    /// ```
    #[inline]
    pub fn allows_zero(&self) -> bool {
        match self {
            Arity::Exact(0) | Arity::Optional | Arity::Variable => true,
            Arity::Range(min, _) | Arity::AtLeast(min) => *min == 0,
            _ => false,
        }
    }

    /// Check if this arity allows exactly one child
    ///
    /// Returns `true` if the arity allows (but doesn't necessarily require) exactly 1 child.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// assert!(Arity::Exact(1).allows_one());
    /// assert!(Arity::Optional.allows_one());
    /// assert!(Arity::Range(1, 5).allows_one());
    /// assert!(Arity::AtLeast(1).allows_one());
    /// assert!(Arity::Variable.allows_one());
    /// assert!(!Arity::Exact(0).allows_one());
    /// assert!(!Arity::Exact(2).allows_one());
    /// ```
    #[inline]
    pub fn allows_one(&self) -> bool {
        match self {
            Arity::Exact(1) | Arity::Optional | Arity::Variable => true,
            Arity::Range(min, max) => *min <= 1 && *max >= 1,
            Arity::AtLeast(min) => *min <= 1,
            _ => false,
        }
    }

    /// Check if this arity requires exactly one child
    ///
    /// Returns `true` only for `Exact(1)`.
    #[inline]
    pub fn is_single(&self) -> bool {
        matches!(self, Arity::Exact(1))
    }

    /// Check if this arity is optional (0 or 1)
    ///
    /// Returns `true` only for `Optional` variant.
    #[inline]
    pub fn is_optional(&self) -> bool {
        matches!(self, Arity::Optional)
    }

    /// Check if this arity is variable (any count)
    ///
    /// Returns `true` only for `Variable` variant.
    #[inline]
    pub fn is_variable(&self) -> bool {
        matches!(self, Arity::Variable)
    }

    /// Check if this arity is exact
    ///
    /// Returns `true` only for `Exact(_)` variant.
    #[inline]
    pub fn is_exact(&self) -> bool {
        matches!(self, Arity::Exact(_))
    }

    /// Check if this arity is a range
    ///
    /// Returns `true` only for `Range(_, _)` variant.
    #[inline]
    pub fn is_range(&self) -> bool {
        matches!(self, Arity::Range(_, _))
    }

    /// Check if this arity has a minimum requirement
    ///
    /// Returns `true` for `AtLeast(_)` variant.
    #[inline]
    pub fn is_at_least(&self) -> bool {
        matches!(self, Arity::AtLeast(_))
    }

    /// Get the exact count if this is an exact arity
    ///
    /// Returns `Some(n)` for `Exact(n)`, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// assert_eq!(Arity::Exact(5).exact_count(), Some(5));
    /// assert_eq!(Arity::Optional.exact_count(), None);
    /// assert_eq!(Arity::Variable.exact_count(), None);
    /// ```
    #[inline]
    pub fn exact_count(&self) -> Option<usize> {
        match self {
            Arity::Exact(n) => Some(*n),
            _ => None,
        }
    }

    /// Get the minimum child count required
    ///
    /// Returns the minimum number of children required by this arity.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// assert_eq!(Arity::Exact(3).min_count(), 3);
    /// assert_eq!(Arity::Optional.min_count(), 0);
    /// assert_eq!(Arity::Range(2, 5).min_count(), 2);
    /// assert_eq!(Arity::AtLeast(3).min_count(), 3);
    /// assert_eq!(Arity::Variable.min_count(), 0);
    /// ```
    #[inline]
    pub fn min_count(&self) -> usize {
        match self {
            Arity::Exact(n) => *n,
            Arity::Optional => 0,
            Arity::Range(min, _) => *min,
            Arity::AtLeast(min) => *min,
            Arity::Variable => 0,
        }
    }

    /// Get the maximum child count allowed
    ///
    /// Returns `Some(max)` if there's a maximum, `None` for unbounded arities.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// assert_eq!(Arity::Exact(3).max_count(), Some(3));
    /// assert_eq!(Arity::Optional.max_count(), Some(1));
    /// assert_eq!(Arity::Range(2, 5).max_count(), Some(5));
    /// assert_eq!(Arity::AtLeast(3).max_count(), None);
    /// assert_eq!(Arity::Variable.max_count(), None);
    /// ```
    #[inline]
    pub fn max_count(&self) -> Option<usize> {
        match self {
            Arity::Exact(n) => Some(*n),
            Arity::Optional => Some(1),
            Arity::Range(_, max) => Some(*max),
            Arity::AtLeast(_) | Arity::Variable => None,
        }
    }

    /// Check if this arity has an upper bound
    ///
    /// Returns `true` if there's a maximum child count.
    #[inline]
    pub fn is_bounded(&self) -> bool {
        self.max_count().is_some()
    }

    /// Check if this arity is unbounded (no maximum)
    ///
    /// Returns `true` for `AtLeast(_)` and `Variable`.
    #[inline]
    pub fn is_unbounded(&self) -> bool {
        matches!(self, Arity::AtLeast(_) | Arity::Variable)
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
            Arity::Optional => write!(f, "Optional"),
            Arity::Range(min, max) => write!(f, "Range({}, {})", min, max),
            Arity::AtLeast(min) => write!(f, "AtLeast({})", min),
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
        assert_eq!(arity.min_count(), 0);
        assert_eq!(arity.max_count(), Some(0));
        assert!(arity.is_bounded());
    }

    #[test]
    fn test_exact_one() {
        let arity = Arity::Exact(1);
        assert!(arity.validate(1).is_ok());
        assert!(arity.validate(0).is_err());
        assert!(arity.validate(2).is_err());
        assert!(!arity.allows_zero());
        assert!(arity.allows_one());
        assert!(arity.is_single());
        assert!(arity.is_exact());
        assert_eq!(arity.exact_count(), Some(1));
        assert_eq!(arity.min_count(), 1);
        assert_eq!(arity.max_count(), Some(1));
    }

    #[test]
    fn test_exact_two() {
        let arity = Arity::Exact(2);
        assert!(arity.validate(2).is_ok());
        assert!(arity.validate(0).is_err());
        assert!(arity.validate(1).is_err());
        assert!(arity.validate(3).is_err());
        assert!(!arity.allows_zero());
        assert!(!arity.allows_one());
        assert!(!arity.is_single());
        assert!(arity.is_exact());
        assert_eq!(arity.exact_count(), Some(2));
        assert_eq!(arity.min_count(), 2);
        assert_eq!(arity.max_count(), Some(2));
    }

    #[test]
    fn test_optional() {
        let arity = Arity::Optional;
        assert!(arity.validate(0).is_ok());
        assert!(arity.validate(1).is_ok());
        assert!(arity.validate(2).is_err());
        assert!(arity.allows_zero());
        assert!(arity.allows_one());
        assert!(!arity.is_single());
        assert!(arity.is_optional());
        assert_eq!(arity.min_count(), 0);
        assert_eq!(arity.max_count(), Some(1));
        assert!(arity.is_bounded());
    }

    #[test]
    fn test_range() {
        let arity = Arity::Range(2, 5);
        assert!(arity.validate(0).is_err());
        assert!(arity.validate(1).is_err());
        assert!(arity.validate(2).is_ok());
        assert!(arity.validate(3).is_ok());
        assert!(arity.validate(4).is_ok());
        assert!(arity.validate(5).is_ok());
        assert!(arity.validate(6).is_err());
        assert!(!arity.allows_zero());
        assert!(!arity.allows_one());
        assert!(arity.is_range());
        assert_eq!(arity.min_count(), 2);
        assert_eq!(arity.max_count(), Some(5));
        assert!(arity.is_bounded());
    }

    #[test]
    fn test_range_allows_one() {
        let arity = Arity::Range(1, 3);
        assert!(arity.allows_one());
        assert!(!arity.allows_zero());
    }

    #[test]
    fn test_at_least() {
        let arity = Arity::AtLeast(2);
        assert!(arity.validate(0).is_err());
        assert!(arity.validate(1).is_err());
        assert!(arity.validate(2).is_ok());
        assert!(arity.validate(100).is_ok());
        assert!(!arity.allows_zero());
        assert!(!arity.allows_one());
        assert!(arity.is_at_least());
        assert_eq!(arity.min_count(), 2);
        assert_eq!(arity.max_count(), None);
        assert!(!arity.is_bounded());
        assert!(arity.is_unbounded());
    }

    #[test]
    fn test_at_least_one() {
        let arity = Arity::AtLeast(1);
        assert!(arity.allows_one());
        assert!(!arity.allows_zero());
        assert!(arity.validate(1).is_ok());
        assert!(arity.validate(100).is_ok());
    }

    #[test]
    fn test_variable() {
        let arity = Arity::Variable;
        assert!(arity.validate(0).is_ok());
        assert!(arity.validate(1).is_ok());
        assert!(arity.validate(100).is_ok());
        assert!(arity.allows_zero());
        assert!(arity.allows_one());
        assert!(!arity.is_single());
        assert!(!arity.is_exact());
        assert!(arity.is_variable());
        assert_eq!(arity.exact_count(), None);
        assert_eq!(arity.min_count(), 0);
        assert_eq!(arity.max_count(), None);
        assert!(!arity.is_bounded());
        assert!(arity.is_unbounded());
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
        assert_eq!(format!("{}", Arity::Optional), "Optional");
        assert_eq!(format!("{}", Arity::Range(2, 5)), "Range(2, 5)");
        assert_eq!(format!("{}", Arity::AtLeast(3)), "AtLeast(3)");
        assert_eq!(format!("{}", Arity::Variable), "Variable");
    }
}
