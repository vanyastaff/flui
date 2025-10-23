//! Element lifecycle states

use std::fmt;
use std::str::FromStr;

/// Element lifecycle state (Initial → Active → Inactive → Defunct)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementLifecycle {
    /// Element created but not yet mounted
    Initial,
    /// Element is actively mounted in the tree
    Active,
    /// Element removed but can be reactivated (GlobalKey reparenting)
    Inactive,
    /// Element permanently unmounted and defunct
    Defunct,
}

impl ElementLifecycle {
    /// Check if an element is active
    #[inline]
    pub const fn is_active(self) -> bool {
        matches!(self, ElementLifecycle::Active)
    }

    /// Check if an element can be reactivated
    #[inline]
    pub const fn can_reactivate(self) -> bool {
        matches!(self, ElementLifecycle::Inactive)
    }

    /// Check if an element is mounted (Active or Inactive)
    #[inline]
    pub const fn is_mounted(self) -> bool {
        matches!(self, ElementLifecycle::Active | ElementLifecycle::Inactive)
    }

    /// Get human-readable string representation
    #[must_use]
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Defunct => "defunct",
        }
    }

    /// Check if this lifecycle state can transition to another
    ///
    /// Valid transitions:
    /// - Initial → Active (mounting)
    /// - Active → Inactive (deactivation)
    /// - Active → Defunct (unmounting)
    /// - Inactive → Active (reactivation)
    /// - Inactive → Defunct (cleanup)
    #[must_use]
    #[inline]
    pub const fn can_transition_to(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Initial, Self::Active)
                | (Self::Active, Self::Inactive)
                | (Self::Active, Self::Defunct)
                | (Self::Inactive, Self::Active)
                | (Self::Inactive, Self::Defunct)
        )
    }

    /// Returns true if this is the initial state
    #[must_use]
    #[inline]
    pub const fn is_initial(self) -> bool {
        matches!(self, Self::Initial)
    }

    /// Returns true if this is the defunct state
    #[must_use]
    #[inline]
    pub const fn is_defunct(self) -> bool {
        matches!(self, Self::Defunct)
    }

    /// Returns true if this is the inactive state
    #[must_use]
    #[inline]
    pub const fn is_inactive(self) -> bool {
        matches!(self, Self::Inactive)
    }
}

impl Default for ElementLifecycle {
    fn default() -> Self {
        Self::Initial
    }
}

impl fmt::Display for ElementLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Error type for parsing ElementLifecycle from string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLifecycleError {
    invalid_value: String,
}

impl fmt::Display for ParseLifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid lifecycle state '{}', expected one of: initial, active, inactive, defunct",
            self.invalid_value
        )
    }
}

impl std::error::Error for ParseLifecycleError {}

impl FromStr for ElementLifecycle {
    type Err = ParseLifecycleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "initial" => Ok(Self::Initial),
            "active" => Ok(Self::Active),
            "inactive" => Ok(Self::Inactive),
            "defunct" => Ok(Self::Defunct),
            _ => Err(ParseLifecycleError {
                invalid_value: s.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_lifecycle_states() {
        assert!(!ElementLifecycle::Initial.is_active());
        assert!(ElementLifecycle::Active.is_active());
        assert!(!ElementLifecycle::Inactive.is_active());
        assert!(!ElementLifecycle::Defunct.is_active());
    }

    #[test]
    fn test_element_lifecycle_can_reactivate() {
        assert!(!ElementLifecycle::Initial.can_reactivate());
        assert!(!ElementLifecycle::Active.can_reactivate());
        assert!(ElementLifecycle::Inactive.can_reactivate());
        assert!(!ElementLifecycle::Defunct.can_reactivate());
    }

    #[test]
    fn test_element_lifecycle_is_mounted() {
        assert!(!ElementLifecycle::Initial.is_mounted());
        assert!(ElementLifecycle::Active.is_mounted());
        assert!(ElementLifecycle::Inactive.is_mounted());
        assert!(!ElementLifecycle::Defunct.is_mounted());
    }

    #[test]
    fn test_element_lifecycle_default() {
        assert_eq!(ElementLifecycle::default(), ElementLifecycle::Initial);
    }

    #[test]
    fn test_element_lifecycle_as_str() {
        assert_eq!(ElementLifecycle::Initial.as_str(), "initial");
        assert_eq!(ElementLifecycle::Active.as_str(), "active");
        assert_eq!(ElementLifecycle::Inactive.as_str(), "inactive");
        assert_eq!(ElementLifecycle::Defunct.as_str(), "defunct");
    }

    #[test]
    fn test_element_lifecycle_display() {
        assert_eq!(ElementLifecycle::Initial.to_string(), "initial");
        assert_eq!(ElementLifecycle::Active.to_string(), "active");
        assert_eq!(ElementLifecycle::Inactive.to_string(), "inactive");
        assert_eq!(ElementLifecycle::Defunct.to_string(), "defunct");
    }

    #[test]
    fn test_element_lifecycle_from_str() {
        assert_eq!("initial".parse::<ElementLifecycle>().unwrap(), ElementLifecycle::Initial);
        assert_eq!("active".parse::<ElementLifecycle>().unwrap(), ElementLifecycle::Active);
        assert_eq!("inactive".parse::<ElementLifecycle>().unwrap(), ElementLifecycle::Inactive);
        assert_eq!("defunct".parse::<ElementLifecycle>().unwrap(), ElementLifecycle::Defunct);

        // Case insensitive
        assert_eq!("ACTIVE".parse::<ElementLifecycle>().unwrap(), ElementLifecycle::Active);
        assert_eq!("Active".parse::<ElementLifecycle>().unwrap(), ElementLifecycle::Active);
    }

    #[test]
    fn test_element_lifecycle_from_str_invalid() {
        let result = "invalid".parse::<ElementLifecycle>();
        assert!(result.is_err());
    }

    #[test]
    fn test_element_lifecycle_can_transition_to() {
        // Valid transitions
        assert!(ElementLifecycle::Initial.can_transition_to(ElementLifecycle::Active));
        assert!(ElementLifecycle::Active.can_transition_to(ElementLifecycle::Inactive));
        assert!(ElementLifecycle::Active.can_transition_to(ElementLifecycle::Defunct));
        assert!(ElementLifecycle::Inactive.can_transition_to(ElementLifecycle::Active));
        assert!(ElementLifecycle::Inactive.can_transition_to(ElementLifecycle::Defunct));

        // Invalid transitions
        assert!(!ElementLifecycle::Initial.can_transition_to(ElementLifecycle::Inactive));
        assert!(!ElementLifecycle::Initial.can_transition_to(ElementLifecycle::Defunct));
        assert!(!ElementLifecycle::Defunct.can_transition_to(ElementLifecycle::Active));
        assert!(!ElementLifecycle::Defunct.can_transition_to(ElementLifecycle::Inactive));
        assert!(!ElementLifecycle::Active.can_transition_to(ElementLifecycle::Initial));
    }

    #[test]
    fn test_element_lifecycle_predicates() {
        assert!(ElementLifecycle::Initial.is_initial());
        assert!(!ElementLifecycle::Active.is_initial());

        assert!(ElementLifecycle::Active.is_active());
        assert!(!ElementLifecycle::Initial.is_active());

        assert!(ElementLifecycle::Inactive.is_inactive());
        assert!(!ElementLifecycle::Active.is_inactive());

        assert!(ElementLifecycle::Defunct.is_defunct());
        assert!(!ElementLifecycle::Active.is_defunct());
    }
}
