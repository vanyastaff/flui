//! Component lifecycle states and transitions
//!
//! This module defines the unified lifecycle for View and Element trees.
//! RenderLifecycle remains separate as it has different states (layout/paint phases).
//!
//! # Design Rationale
//!
//! ViewLifecycle and ElementLifecycle were identical duplicates (DRY violation).
//! Both represent the same state machine: Initial → Active → Inactive → Defunct.
//!
//! ComponentLifecycle consolidates these into a single source of truth.
//!
//! # State Diagram
//!
//! ```text
//!     ┌─────────┐
//!     │ Initial │  (Created but not mounted)
//!     └────┬────┘
//!          │ mount()
//!          ▼
//!     ┌────────┐
//!  ┌─▶│ Active │◀─┐  (Mounted in tree, can build)
//!  │  └───┬────┘  │
//!  │      │       │ activate()
//!  │      │ deactivate()
//!  │      ▼       │
//!  │  ┌──────────┐│
//!  └──│ Inactive ├┘  (Unmounted but state preserved)
//!     └────┬─────┘
//!          │ dispose()
//!          ▼
//!     ┌─────────┐
//!     │ Defunct │  (Permanently removed)
//!     └─────────┘
//! ```

use std::fmt;

/// Component lifecycle states (unified for View and Element trees).
///
/// This lifecycle applies to components (views and elements) but NOT to render objects.
/// Render objects use RenderLifecycle which includes layout/paint states.
///
/// # Valid State Transitions
///
/// | From     | To       | Trigger        | Description                          |
/// |----------|----------|----------------|--------------------------------------|
/// | Initial  | Active   | `mount()`      | Component mounted to tree            |
/// | Active   | Inactive | `deactivate()` | Component removed but state preserved|
/// | Inactive | Active   | `activate()`   | Component re-mounted to tree         |
/// | Active   | Defunct  | `dispose()`    | Component permanently destroyed      |
/// | Inactive | Defunct  | `dispose()`    | Inactive component permanently removed|
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum ComponentLifecycle {
    /// Component created but not yet mounted
    #[default]
    Initial = 0,

    /// Component is active in the tree
    Active = 1,

    /// Component removed from tree but might be reinserted
    Inactive = 2,

    /// Component permanently removed
    Defunct = 3,
}

impl ComponentLifecycle {
    /// Check if component is in initial state
    #[inline]
    #[must_use]
    pub const fn is_initial(self) -> bool {
        matches!(self, Self::Initial)
    }

    /// Check if component is active
    #[inline]
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if component is inactive
    #[inline]
    #[must_use]
    pub const fn is_inactive(self) -> bool {
        matches!(self, Self::Inactive)
    }

    /// Check if component is defunct
    #[inline]
    #[must_use]
    pub const fn is_defunct(self) -> bool {
        matches!(self, Self::Defunct)
    }

    /// Check if component can participate in builds
    #[inline]
    #[must_use]
    pub const fn can_build(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if component is still alive (not defunct)
    #[inline]
    #[must_use]
    pub const fn is_alive(self) -> bool {
        !matches!(self, Self::Defunct)
    }

    /// Check if component is mounted
    #[inline]
    #[must_use]
    pub const fn is_mounted(self) -> bool {
        matches!(self, Self::Active)
    }

    // ========== LIFECYCLE TRANSITIONS ==========

    /// Transition to Active state (mount).
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
    pub fn can_transition_to(&self, next: ComponentLifecycle) -> bool {
        use ComponentLifecycle::*;

        match (*self, next) {
            (Initial, Active) => true,
            (Active, Inactive) | (Active, Defunct) => true,
            (Inactive, Active) | (Inactive, Defunct) => true,
            (Defunct, _) => false,
            (s, n) if s == n => true,
            _ => false,
        }
    }
}

impl fmt::Display for ComponentLifecycle {
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
        let initial = ComponentLifecycle::Initial;
        assert!(initial.is_initial());
        assert!(!initial.is_active());
        assert!(initial.is_alive());

        let active = ComponentLifecycle::Active;
        assert!(active.is_active());
        assert!(active.can_build());
        assert!(active.is_alive());
        assert!(active.is_mounted());

        let inactive = ComponentLifecycle::Inactive;
        assert!(inactive.is_inactive());
        assert!(!inactive.can_build());
        assert!(inactive.is_alive());

        let defunct = ComponentLifecycle::Defunct;
        assert!(defunct.is_defunct());
        assert!(!defunct.is_alive());
    }

    #[test]
    fn test_lifecycle_transitions() {
        let mut lifecycle = ComponentLifecycle::Initial;

        lifecycle.mount();
        assert_eq!(lifecycle, ComponentLifecycle::Active);

        lifecycle.deactivate();
        assert_eq!(lifecycle, ComponentLifecycle::Inactive);

        lifecycle.activate();
        assert_eq!(lifecycle, ComponentLifecycle::Active);

        lifecycle.unmount();
        assert_eq!(lifecycle, ComponentLifecycle::Defunct);
    }

    #[test]
    fn test_default() {
        assert_eq!(ComponentLifecycle::default(), ComponentLifecycle::Initial);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ComponentLifecycle::Initial), "Initial");
        assert_eq!(format!("{}", ComponentLifecycle::Active), "Active");
        assert_eq!(format!("{}", ComponentLifecycle::Inactive), "Inactive");
        assert_eq!(format!("{}", ComponentLifecycle::Defunct), "Defunct");
    }

    #[test]
    fn test_can_transition_to() {
        use ComponentLifecycle::*;

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
