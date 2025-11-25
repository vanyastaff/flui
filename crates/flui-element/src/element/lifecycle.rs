//! Element lifecycle states and transitions
//!
//! This module defines the lifecycle states that elements go through from
//! creation to destruction, and the valid transitions between them.

/// Element lifecycle states
///
/// Elements transition through these states as they're created, mounted,
/// unmounted, and destroyed in the element tree.
///
/// # State Diagram
///
/// ```text
///     ┌─────────┐
///     │ Initial │  (Created but not mounted)
///     └────┬────┘
///          │ mount()
///          ▼
///     ┌────────┐
///  ┌─▶│ Active │◀─┐  (Mounted in tree)
///  │  └───┬────┘  │
///  │      │       │ activate()
///  │      │ deactivate()
///  │      ▼       │
///  │  ┌──────────┐│
///  └──│ Inactive ├┘  (Unmounted but state preserved)
///     └────┬─────┘
///          │ dispose()
///          ▼
///     ┌─────────┐
///     │ Defunct │  (Permanently removed)
///     └─────────┘
/// ```
///
/// # Valid State Transitions
///
/// | From     | To       | Trigger        | Description                          |
/// |----------|----------|----------------|--------------------------------------|
/// | Initial  | Active   | `mount()`      | Element mounted to tree              |
/// | Active   | Inactive | `deactivate()` | Element removed but state preserved  |
/// | Inactive | Active   | `activate()`   | Element re-mounted to tree           |
/// | Active   | Defunct  | `dispose()`    | Element permanently destroyed        |
/// | Inactive | Defunct  | `dispose()`    | Inactive element permanently removed |
///
/// # Invalid Transitions
///
/// These transitions are **not allowed** and indicate bugs:
///
/// - ❌ Initial → Inactive (must mount first)
/// - ❌ Initial → Defunct (must mount then dispose)
/// - ❌ Defunct → * (cannot resurrect defunct elements)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ElementLifecycle {
    /// Element created but not yet mounted
    ///
    /// This is the initial state when an element is first created.
    #[default]
    Initial,

    /// Element is active in the tree
    ///
    /// The element is mounted and participating in builds/layouts.
    Active,

    /// Element removed from tree but might be reinserted
    ///
    /// The element was unmounted but state is preserved for potential reinsertion.
    Inactive,

    /// Element permanently removed
    ///
    /// The element is dead and will never be reinserted. Resources should be cleaned up.
    Defunct,
}

impl ElementLifecycle {
    /// Check if element is in initial state
    #[inline]
    #[must_use]
    pub const fn is_initial(self) -> bool {
        matches!(self, Self::Initial)
    }

    /// Check if element is active
    #[inline]
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if element is inactive
    #[inline]
    #[must_use]
    pub const fn is_inactive(self) -> bool {
        matches!(self, Self::Inactive)
    }

    /// Check if element is defunct
    #[inline]
    #[must_use]
    pub const fn is_defunct(self) -> bool {
        matches!(self, Self::Defunct)
    }

    /// Check if element can participate in builds
    ///
    /// Returns `true` for Active elements only.
    #[inline]
    #[must_use]
    pub const fn can_build(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if element is still alive (not defunct)
    #[inline]
    #[must_use]
    pub const fn is_alive(self) -> bool {
        !matches!(self, Self::Defunct)
    }
}

impl std::fmt::Display for ElementLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        let initial = ElementLifecycle::Initial;
        assert!(initial.is_initial());
        assert!(!initial.is_active());
        assert!(initial.is_alive());

        let active = ElementLifecycle::Active;
        assert!(active.is_active());
        assert!(active.can_build());
        assert!(active.is_alive());

        let inactive = ElementLifecycle::Inactive;
        assert!(inactive.is_inactive());
        assert!(!inactive.can_build());
        assert!(inactive.is_alive());

        let defunct = ElementLifecycle::Defunct;
        assert!(defunct.is_defunct());
        assert!(!defunct.is_alive());
    }

    #[test]
    fn test_default() {
        assert_eq!(ElementLifecycle::default(), ElementLifecycle::Initial);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ElementLifecycle::Initial), "Initial");
        assert_eq!(format!("{}", ElementLifecycle::Active), "Active");
        assert_eq!(format!("{}", ElementLifecycle::Inactive), "Inactive");
        assert_eq!(format!("{}", ElementLifecycle::Defunct), "Defunct");
    }
}
