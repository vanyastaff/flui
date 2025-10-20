//! Element lifecycle states
//!
//! Tracks the lifecycle of an element through mounting, updating, and unmounting.

use std::fmt;

/// Element lifecycle state
///
///
/// # States
///
/// - `Initial`: Element created but not mounted
/// - `Active`: Element mounted in tree
/// - `Inactive`: Element temporarily removed (e.g., in KeepAlive)
/// - `Defunct`: Element unmounted and disposed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum Lifecycle {
    /// Element created but not yet mounted
    #[default]
    Initial,

    /// Element mounted and active in tree
    Active,

    /// Element temporarily removed but kept alive
    Inactive,

    /// Element unmounted and disposed
    Defunct,
}

impl Lifecycle {
    /// Check if element is mounted (active or inactive)
    #[inline]
    pub fn is_mounted(self) -> bool {
        matches!(self, Lifecycle::Active | Lifecycle::Inactive)
    }

    /// Check if element is active
    #[inline]
    pub fn is_active(self) -> bool {
        matches!(self, Lifecycle::Active)
    }

    /// Check if element is defunct
    #[inline]
    pub fn is_defunct(self) -> bool {
        matches!(self, Lifecycle::Defunct)
    }

    /// Check if element is in initial state
    #[inline]
    pub fn is_initial(self) -> bool {
        matches!(self, Lifecycle::Initial)
    }
}


impl fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lifecycle::Initial => write!(f, "Initial"),
            Lifecycle::Active => write!(f, "Active"),
            Lifecycle::Inactive => write!(f, "Inactive"),
            Lifecycle::Defunct => write!(f, "Defunct"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_is_mounted() {
        assert!(!Lifecycle::Initial.is_mounted());
        assert!(Lifecycle::Active.is_mounted());
        assert!(Lifecycle::Inactive.is_mounted());
        assert!(!Lifecycle::Defunct.is_mounted());
    }

    #[test]
    fn test_lifecycle_is_active() {
        assert!(!Lifecycle::Initial.is_active());
        assert!(Lifecycle::Active.is_active());
        assert!(!Lifecycle::Inactive.is_active());
        assert!(!Lifecycle::Defunct.is_active());
    }

    #[test]
    fn test_lifecycle_is_defunct() {
        assert!(!Lifecycle::Initial.is_defunct());
        assert!(!Lifecycle::Active.is_defunct());
        assert!(!Lifecycle::Inactive.is_defunct());
        assert!(Lifecycle::Defunct.is_defunct());
    }

    #[test]
    fn test_lifecycle_default() {
        assert_eq!(Lifecycle::default(), Lifecycle::Initial);
    }

    #[test]
    fn test_lifecycle_display() {
        assert_eq!(Lifecycle::Initial.to_string(), "Initial");
        assert_eq!(Lifecycle::Active.to_string(), "Active");
        assert_eq!(Lifecycle::Inactive.to_string(), "Inactive");
        assert_eq!(Lifecycle::Defunct.to_string(), "Defunct");
    }
}
