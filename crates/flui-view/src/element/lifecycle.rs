//! View lifecycle states and transitions.
//!
//! This module defines the lifecycle states for ViewElement, tracking
//! the phases from creation through mounting and building.
//!
//! # Architecture
//!
//! ViewLifecycle is simpler than RenderLifecycle because views don't
//! participate in layout/paint - they only build children.
//!
//! ```text
//! ┌─────────┐
//! │ Initial │  (Created but not mounted)
//! └────┬────┘
//!      │ mount()
//!      ▼
//! ┌────────┐
//! │ Active │◀─┐  (Mounted in tree, can build)
//! └───┬────┘  │
//!     │       │ activate()
//!     │ deactivate()
//!     ▼       │
//! ┌──────────┐│
//! │ Inactive ├┘  (Unmounted but state preserved)
//! └────┬─────┘
//!      │ dispose()
//!      ▼
//! ┌─────────┐
//! │ Defunct │  (Permanently removed)
//! └─────────┘
//! ```

use std::fmt;

/// View lifecycle states.
///
/// Views transition through these states as they're created, mounted,
/// unmounted, and destroyed in the element tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum ViewLifecycle {
    /// View created but not yet mounted.
    #[default]
    Initial = 0,

    /// View is active in the tree and can build.
    Active = 1,

    /// View removed from tree but state preserved for potential reinsertion.
    Inactive = 2,

    /// View permanently removed, resources should be cleaned up.
    Defunct = 3,
}

impl ViewLifecycle {
    /// Check if view is in initial state.
    #[inline]
    #[must_use]
    pub const fn is_initial(self) -> bool {
        matches!(self, Self::Initial)
    }

    /// Check if view is active.
    #[inline]
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if view is inactive.
    #[inline]
    #[must_use]
    pub const fn is_inactive(self) -> bool {
        matches!(self, Self::Inactive)
    }

    /// Check if view is defunct.
    #[inline]
    #[must_use]
    pub const fn is_defunct(self) -> bool {
        matches!(self, Self::Defunct)
    }

    /// Check if view can participate in builds.
    ///
    /// Returns `true` for Active views only.
    #[inline]
    #[must_use]
    pub const fn can_build(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if view is still alive (not defunct).
    #[inline]
    #[must_use]
    pub const fn is_alive(self) -> bool {
        !matches!(self, Self::Defunct)
    }

    /// Check if view is mounted (active or was active).
    #[inline]
    #[must_use]
    pub const fn is_mounted(self) -> bool {
        matches!(self, Self::Active)
    }
}

// ============================================================================
// LIFECYCLE TRANSITIONS
// ============================================================================

impl ViewLifecycle {
    /// Transition to Active state (mount).
    ///
    /// # Panics
    ///
    /// Panics in debug mode if transition is invalid.
    #[inline]
    pub fn mount(&mut self) {
        debug_assert!(
            matches!(self, Self::Initial | Self::Inactive),
            "Cannot mount: invalid state {:?}",
            self
        );
        *self = Self::Active;
    }

    /// Transition to Inactive state (deactivate).
    ///
    /// # Panics
    ///
    /// Panics in debug mode if not active.
    #[inline]
    pub fn deactivate(&mut self) {
        debug_assert!(
            self.is_active(),
            "Cannot deactivate: not active (state: {:?})",
            self
        );
        *self = Self::Inactive;
    }

    /// Transition back to Active state (activate).
    ///
    /// # Panics
    ///
    /// Panics in debug mode if not inactive.
    #[inline]
    pub fn activate(&mut self) {
        debug_assert!(
            self.is_inactive(),
            "Cannot activate: not inactive (state: {:?})",
            self
        );
        *self = Self::Active;
    }

    /// Transition to Defunct state (unmount/dispose).
    #[inline]
    pub fn unmount(&mut self) {
        *self = Self::Defunct;
    }

    /// Checks if transition to given state is valid.
    #[must_use]
    pub fn can_transition_to(&self, next: ViewLifecycle) -> bool {
        use ViewLifecycle::*;

        match (*self, next) {
            // Initial can only go to Active
            (Initial, Active) => true,
            // Active can go to Inactive or Defunct
            (Active, Inactive) | (Active, Defunct) => true,
            // Inactive can go to Active or Defunct
            (Inactive, Active) | (Inactive, Defunct) => true,
            // Defunct is terminal
            (Defunct, _) => false,
            // Same state is always valid
            (s, n) if s == n => true,
            _ => false,
        }
    }
}

impl fmt::Display for ViewLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initial => write!(f, "Initial"),
            Self::Active => write!(f, "Active"),
            Self::Inactive => write!(f, "Inactive"),
            Self::Defunct => write!(f, "Defunct"),
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
    fn test_lifecycle_states() {
        let initial = ViewLifecycle::Initial;
        assert!(initial.is_initial());
        assert!(!initial.is_active());
        assert!(initial.is_alive());

        let active = ViewLifecycle::Active;
        assert!(active.is_active());
        assert!(active.can_build());
        assert!(active.is_alive());
        assert!(active.is_mounted());

        let inactive = ViewLifecycle::Inactive;
        assert!(inactive.is_inactive());
        assert!(!inactive.can_build());
        assert!(inactive.is_alive());

        let defunct = ViewLifecycle::Defunct;
        assert!(defunct.is_defunct());
        assert!(!defunct.is_alive());
    }

    #[test]
    fn test_lifecycle_transitions() {
        let mut lifecycle = ViewLifecycle::Initial;

        lifecycle.mount();
        assert_eq!(lifecycle, ViewLifecycle::Active);

        lifecycle.deactivate();
        assert_eq!(lifecycle, ViewLifecycle::Inactive);

        lifecycle.activate();
        assert_eq!(lifecycle, ViewLifecycle::Active);

        lifecycle.unmount();
        assert_eq!(lifecycle, ViewLifecycle::Defunct);
    }

    #[test]
    fn test_default() {
        assert_eq!(ViewLifecycle::default(), ViewLifecycle::Initial);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ViewLifecycle::Initial), "Initial");
        assert_eq!(format!("{}", ViewLifecycle::Active), "Active");
        assert_eq!(format!("{}", ViewLifecycle::Inactive), "Inactive");
        assert_eq!(format!("{}", ViewLifecycle::Defunct), "Defunct");
    }

    #[test]
    fn test_can_transition_to() {
        use ViewLifecycle::*;

        assert!(Initial.can_transition_to(Active));
        assert!(!Initial.can_transition_to(Inactive));

        assert!(Active.can_transition_to(Inactive));
        assert!(Active.can_transition_to(Defunct));

        assert!(Inactive.can_transition_to(Active));
        assert!(Inactive.can_transition_to(Defunct));

        assert!(!Defunct.can_transition_to(Active));
        assert!(!Defunct.can_transition_to(Initial));
    }
}
