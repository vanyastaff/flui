//! Element lifecycle states.
//!
//! Defines the lifecycle phases an Element goes through from creation to
//! disposal.

/// Lifecycle state of an Element.
///
/// Elements progress through these states:
/// ```text
/// Initial → Active ⇄ Inactive → Defunct
/// ```
///
/// - `Initial`: Just created, not yet mounted
/// - `Active`: Mounted in tree, participating in builds
/// - `Inactive`: Temporarily removed, may be reactivated
/// - `Defunct`: Permanently removed, will be dropped
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Lifecycle {
    /// Element has been created but not yet mounted.
    #[default]
    Initial,

    /// Element is mounted and active in the tree.
    ///
    /// Active elements:
    /// - Participate in the build phase
    /// - Can be marked dirty
    /// - Have valid parent/child relationships
    Active,

    /// Element has been temporarily removed from the tree.
    ///
    /// Inactive elements:
    /// - May be reactivated within the same frame
    /// - State is preserved
    /// - RenderObject is detached but not disposed
    Inactive,

    /// Element has been permanently removed.
    ///
    /// Defunct elements:
    /// - Cannot be reactivated
    /// - State has been disposed
    /// - Will be dropped
    Defunct,
}

impl Lifecycle {
    /// Returns `true` if the element is active.
    #[inline]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns `true` if the element has been created but not yet mounted.
    ///
    /// Flutter's `Element.mount` asserts `_lifecycleState == initial`; this is
    /// the predicate `ElementCore::mount` checks so the contract matches.
    #[inline]
    pub fn is_initial(self) -> bool {
        matches!(self, Self::Initial)
    }

    /// Returns `true` if the element is inactive.
    #[inline]
    pub fn is_inactive(self) -> bool {
        matches!(self, Self::Inactive)
    }

    /// Returns `true` if the element is defunct.
    #[inline]
    pub fn is_defunct(self) -> bool {
        matches!(self, Self::Defunct)
    }

    /// Returns `true` if the element can be built (is active).
    #[inline]
    pub fn can_build(self) -> bool {
        self.is_active()
    }

    /// Returns `true` if the element can be reactivated — only an `Inactive`
    /// element can.
    ///
    /// In particular `Defunct` cannot: its state has been disposed, so
    /// reviving it would operate on torn-down state. `ElementCore::activate`
    /// asserts this in debug builds.
    #[inline]
    pub fn can_activate(self) -> bool {
        matches!(self, Self::Inactive)
    }

    /// Returns `true` if the element can be deactivated.
    #[inline]
    pub fn can_deactivate(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_initial() {
        assert_eq!(Lifecycle::default(), Lifecycle::Initial);
    }

    /// Both mutator predicates, over every state, stated once so a future edit
    /// has to argue with each cell rather than the two or three a spot-check
    /// would cover.
    #[test]
    fn activate_and_deactivate_admit_exactly_one_state_each() {
        use Lifecycle::{Active, Defunct, Inactive, Initial};

        for state in [Initial, Active, Inactive, Defunct] {
            assert_eq!(
                state.can_activate(),
                state == Inactive,
                "only Inactive may be reactivated; {state:?} disagreed"
            );
            assert_eq!(
                state.can_deactivate(),
                state == Active,
                "only Active may be deactivated; {state:?} disagreed"
            );
        }

        // The edge these guards exist for: a disposed element stays disposed.
        assert!(!Defunct.can_activate(), "Defunct must not be revivable");
        assert!(!Defunct.can_build(), "Defunct must not be buildable");
    }

    #[test]
    fn test_lifecycle_checks() {
        assert!(Lifecycle::Active.is_active());
        assert!(Lifecycle::Active.can_build());
        assert!(Lifecycle::Active.can_deactivate());

        assert!(Lifecycle::Inactive.is_inactive());
        assert!(Lifecycle::Inactive.can_activate());

        assert!(Lifecycle::Defunct.is_defunct());
        assert!(!Lifecycle::Defunct.can_activate());
    }
}
