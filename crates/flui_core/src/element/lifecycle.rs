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
///
/// # Lifecycle Hooks
///
/// Different operations happen at each transition:
///
/// - **Initial → Active**: `build()` called, dependencies registered
/// - **Active → Inactive**: Dependencies unregistered, element hidden
/// - **Inactive → Active**: Dependencies re-registered, element shown
/// - *** → Defunct**: Cleanup hooks called, resources freed
///
/// # Memory Management
///
/// - **Initial/Active/Inactive**: Element state retained in memory
/// - **Defunct**: Element removed from tree, state dropped
///
/// # Example: Typical Lifecycle
///
/// ```rust,ignore
/// use flui_core::ElementLifecycle;
///
/// // 1. Element created
/// let mut state = ElementLifecycle::Initial;
/// assert!(state.is_initial());
///
/// // 2. Element mounted to tree
/// state = ElementLifecycle::Active;
/// assert!(state.is_active());
///
/// // 3. Element temporarily removed (e.g., conditional rendering)
/// state = ElementLifecycle::Inactive;
/// assert!(state.is_inactive());
///
/// // 4. Element re-mounted
/// state = ElementLifecycle::Active;
/// assert!(state.is_active());
///
/// // 5. Element permanently removed
/// state = ElementLifecycle::Defunct;
/// assert!(state.is_defunct());
/// ```
///
/// # Use Cases
///
/// ## Conditional Rendering
///
/// ```rust,ignore
/// if show_widget {
///     // Active: Widget visible
///     widget.activate();
/// } else {
///     // Inactive: Widget hidden but state preserved
///     widget.deactivate();
/// }
/// ```
///
/// ## Animations
///
/// ```rust,ignore
/// // Inactive during exit animation
/// widget.deactivate();
/// animate_out();
///
/// // Defunct after animation completes
/// widget.dispose();
/// ```
///
/// ## Debugging
///
/// ```rust,ignore
/// if !element.lifecycle().is_active() {
///     eprintln!("Warning: Operating on non-active element {:?}", element);
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
    /// Check if element is active
    #[inline]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if element is initial
    #[inline]
    pub fn is_initial(self) -> bool {
        matches!(self, Self::Initial)
    }

    /// Check if element is inactive
    #[inline]
    pub fn is_inactive(self) -> bool {
        matches!(self, Self::Inactive)
    }

    /// Check if element is defunct
    #[inline]
    pub fn is_defunct(self) -> bool {
        matches!(self, Self::Defunct)
    }
}

// Default is now derived with #[default] annotation above

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_states() {
        let initial = ElementLifecycle::Initial;
        assert!(initial.is_initial());
        assert!(!initial.is_active());

        let active = ElementLifecycle::Active;
        assert!(active.is_active());
        assert!(!active.is_defunct());

        let inactive = ElementLifecycle::Inactive;
        assert!(inactive.is_inactive());
        assert!(!inactive.is_active());

        let defunct = ElementLifecycle::Defunct;
        assert!(defunct.is_defunct());
    }

    #[test]
    fn test_default() {
        assert_eq!(ElementLifecycle::default(), ElementLifecycle::Initial);
    }
}
