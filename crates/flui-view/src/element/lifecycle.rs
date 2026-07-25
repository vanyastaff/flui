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

    /// Returns `true` if the element can be reactivated.
    #[inline]
    pub fn can_activate(self) -> bool {
        matches!(self, Self::Inactive)
    }

    /// Returns `true` if the element can be deactivated.
    #[inline]
    pub fn can_deactivate(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns `true` if `self` may legally become `next`.
    ///
    /// The permitted graph is the one this type documents:
    /// `Initial → Active ⇄ Inactive → Defunct`. **`Defunct` is absorbing** —
    /// its state has been disposed, so reactivating a defunct element would
    /// operate on torn-down state. That is the edge this predicate exists to
    /// catch; the mutators on `ElementCore` assert it in debug builds.
    ///
    /// Re-entering the state you are already in is permitted: mount/activate
    /// are driven by tree walks that can legitimately re-run against an
    /// already-Active element.
    #[must_use]
    #[inline]
    pub fn can_transition_to(self, next: Self) -> bool {
        !self.is_defunct() || next == Self::Defunct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_initial() {
        assert_eq!(Lifecycle::default(), Lifecycle::Initial);
    }

    /// The whole 4x4 transition table, stated once so a future edit to
    /// `can_transition_to` has to argue with every cell rather than with the
    /// two or three a spot-check would cover.
    #[test]
    fn defunct_is_the_only_absorbing_state() {
        use Lifecycle::{Active, Defunct, Inactive, Initial};

        for from in [Initial, Active, Inactive] {
            for to in [Initial, Active, Inactive, Defunct] {
                assert!(
                    from.can_transition_to(to),
                    "{from:?} -> {to:?} should be permitted"
                );
            }
        }

        // The edge this predicate exists for: a defunct element has disposed
        // its state, so nothing may bring it back.
        for to in [Initial, Active, Inactive] {
            assert!(
                !Defunct.can_transition_to(to),
                "Defunct -> {to:?} must be rejected"
            );
        }
        assert!(
            Defunct.can_transition_to(Defunct),
            "unmounting an already-defunct element is idempotent, not illegal"
        );
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
