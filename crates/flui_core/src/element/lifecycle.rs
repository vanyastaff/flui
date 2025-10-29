//! Element lifecycle states

/// Element lifecycle states
///
/// Elements transition through these states as they're mounted/unmounted:
///
/// ```text
/// Initial → Active → Inactive → Defunct
///           ↑_____|
/// ```
///
/// # State Transitions
///
/// - **Initial → Active**: Element mounted via `mount()`
/// - **Active → Inactive**: Element unmounted but might be reinserted via `deactivate()`
/// - **Inactive → Active**: Element reinserted via `activate()`
/// - **Active/Inactive → Defunct**: Element permanently removed
///
/// # Examples
///
/// ```rust
/// use flui_core::ElementLifecycle;
///
/// let state = ElementLifecycle::Initial;
/// assert!(!state.is_active());
///
/// let state = ElementLifecycle::Active;
/// assert!(state.is_active());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementLifecycle {
    /// Element created but not yet mounted
    ///
    /// This is the initial state when an element is first created.
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

impl Default for ElementLifecycle {
    fn default() -> Self {
        Self::Initial
    }
}

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
