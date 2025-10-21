//! State lifecycle management
//!
//! This module defines the StateLifecycle enum that tracks the lifecycle
//! progression of State objects from creation to disposal.

use std::fmt;
use std::str::FromStr;

/// State lifecycle progression
///
/// Tracks the lifecycle state of a State object from creation to disposal.
/// This enforces correct lifecycle ordering and prevents invalid operations.
///
/// # Lifecycle Progression
///
/// ```text
/// Created → Initialized → Ready → Defunct
///    ↓           ↓          ↓        ↓
///  new()    initState()  build()  dispose()
/// ```
///
/// # Ordering
///
/// The lifecycle has a natural ordering:
/// `Created < Initialized < Ready < Defunct`
///
/// This allows comparisons like:
/// ```rust,ignore
/// if lifecycle >= StateLifecycle::Ready {
///     // Safe to build
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::StateLifecycle;
///
/// let lifecycle = StateLifecycle::default();
/// assert_eq!(lifecycle, StateLifecycle::Created);
/// assert!(!lifecycle.is_mounted());
/// assert!(!lifecycle.can_build());
///
/// let lifecycle = StateLifecycle::Ready;
/// assert!(lifecycle.is_mounted());
/// assert!(lifecycle.can_build());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StateLifecycle {
    /// State object created but initState() not yet called
    Created = 0,
    /// initState() called, ready to build
    Initialized = 1,
    /// State is active and can build/rebuild
    Ready = 2,
    /// dispose() called, state is defunct and cannot be used
    Defunct = 3,
}

impl StateLifecycle {
    /// Check if state is mounted (can call setState)
    ///
    /// Returns `true` for `Initialized` and `Ready` states.
    /// Returns `false` for `Created` and `Defunct` states.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert!(!StateLifecycle::Created.is_mounted());
    /// assert!(StateLifecycle::Initialized.is_mounted());
    /// assert!(StateLifecycle::Ready.is_mounted());
    /// assert!(!StateLifecycle::Defunct.is_mounted());
    /// ```
    #[must_use]
    #[inline]
    pub const fn is_mounted(&self) -> bool {
        matches!(self, Self::Initialized | Self::Ready)
    }

    /// Check if state can build
    ///
    /// Returns `true` only for `Ready` state.
    /// Returns `false` for all other states.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert!(!StateLifecycle::Created.can_build());
    /// assert!(!StateLifecycle::Initialized.can_build());
    /// assert!(StateLifecycle::Ready.can_build());
    /// assert!(!StateLifecycle::Defunct.can_build());
    /// ```
    #[must_use]
    #[inline]
    pub const fn can_build(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Check if state is defunct (disposed)
    ///
    /// Returns `true` only for `Defunct` state.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert!(!StateLifecycle::Ready.is_defunct());
    /// assert!(StateLifecycle::Defunct.is_defunct());
    /// ```
    #[must_use]
    #[inline]
    pub const fn is_defunct(&self) -> bool {
        matches!(self, Self::Defunct)
    }

    /// Get a human-readable name
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "Created",
            Self::Initialized => "Initialized",
            Self::Ready => "Ready",
            Self::Defunct => "Defunct",
        }
    }

    /// Check if this lifecycle state can transition to another
    ///
    /// Valid transitions:
    /// - Created → Initialized (init_state called)
    /// - Initialized → Ready (ready to build)
    /// - Ready → Defunct (dispose called)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::StateLifecycle;
    ///
    /// // Valid transitions
    /// assert!(StateLifecycle::Created.can_transition_to(StateLifecycle::Initialized));
    /// assert!(StateLifecycle::Initialized.can_transition_to(StateLifecycle::Ready));
    /// assert!(StateLifecycle::Ready.can_transition_to(StateLifecycle::Defunct));
    ///
    /// // Invalid transitions
    /// assert!(!StateLifecycle::Defunct.can_transition_to(StateLifecycle::Ready));
    /// assert!(!StateLifecycle::Created.can_transition_to(StateLifecycle::Ready));
    /// ```
    #[must_use]
    #[inline]
    pub const fn can_transition_to(&self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Created, Self::Initialized)
                | (Self::Initialized, Self::Ready)
                | (Self::Ready, Self::Defunct)
        )
    }

    /// Check if this is the Created state
    #[must_use]
    #[inline]
    pub const fn is_created(&self) -> bool {
        matches!(self, Self::Created)
    }

    /// Check if this is the Initialized state
    #[must_use]
    #[inline]
    pub const fn is_initialized(&self) -> bool {
        matches!(self, Self::Initialized)
    }

    /// Check if this is the Ready state
    #[must_use]
    #[inline]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl Default for StateLifecycle {
    /// Default lifecycle state is `Created`
    ///
    /// This represents a newly created state object before `init_state()` is called.
    fn default() -> Self {
        Self::Created
    }
}

impl fmt::Display for StateLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error type for parsing StateLifecycle from string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseStateLifecycleError {
    invalid_value: String,
}

impl fmt::Display for ParseStateLifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid state lifecycle '{}', expected one of: Created, Initialized, Ready, Defunct (case-insensitive)",
            self.invalid_value
        )
    }
}

impl std::error::Error for ParseStateLifecycleError {}

impl FromStr for StateLifecycle {
    type Err = ParseStateLifecycleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "created" => Ok(Self::Created),
            "initialized" => Ok(Self::Initialized),
            "ready" => Ok(Self::Ready),
            "defunct" => Ok(Self::Defunct),
            _ => Err(ParseStateLifecycleError {
                invalid_value: s.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_ordering() {
        assert!(StateLifecycle::Created < StateLifecycle::Initialized);
        assert!(StateLifecycle::Initialized < StateLifecycle::Ready);
        assert!(StateLifecycle::Ready < StateLifecycle::Defunct);
    }

    #[test]
    fn test_lifecycle_comparison() {
        let lifecycle = StateLifecycle::Ready;
        assert!(lifecycle >= StateLifecycle::Initialized);
        assert!(lifecycle < StateLifecycle::Defunct);
    }

    #[test]
    fn test_is_mounted() {
        assert!(!StateLifecycle::Created.is_mounted());
        assert!(StateLifecycle::Initialized.is_mounted());
        assert!(StateLifecycle::Ready.is_mounted());
        assert!(!StateLifecycle::Defunct.is_mounted());
    }

    #[test]
    fn test_can_build() {
        assert!(!StateLifecycle::Created.can_build());
        assert!(!StateLifecycle::Initialized.can_build());
        assert!(StateLifecycle::Ready.can_build());
        assert!(!StateLifecycle::Defunct.can_build());
    }

    #[test]
    fn test_is_defunct() {
        assert!(!StateLifecycle::Created.is_defunct());
        assert!(!StateLifecycle::Ready.is_defunct());
        assert!(StateLifecycle::Defunct.is_defunct());
    }

    #[test]
    fn test_default() {
        assert_eq!(StateLifecycle::default(), StateLifecycle::Created);
    }

    #[test]
    fn test_display() {
        assert_eq!(StateLifecycle::Created.to_string(), "Created");
        assert_eq!(StateLifecycle::Ready.to_string(), "Ready");
        assert_eq!(StateLifecycle::Defunct.to_string(), "Defunct");
    }

    #[test]
    fn test_as_str() {
        assert_eq!(StateLifecycle::Created.as_str(), "Created");
        assert_eq!(StateLifecycle::Initialized.as_str(), "Initialized");
        assert_eq!(StateLifecycle::Ready.as_str(), "Ready");
        assert_eq!(StateLifecycle::Defunct.as_str(), "Defunct");
    }

    #[test]
    fn test_clone_copy() {
        let lifecycle = StateLifecycle::Ready;
        let cloned = lifecycle;
        assert_eq!(lifecycle, cloned);
    }

    #[test]
    fn test_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(StateLifecycle::Created);
        set.insert(StateLifecycle::Ready);

        assert!(set.contains(&StateLifecycle::Created));
        assert!(set.contains(&StateLifecycle::Ready));
        assert!(!set.contains(&StateLifecycle::Initialized));
    }
}