//! Reverse ownership index for inherited dependencies.
//!
//! An [`InheritedBehavior`](crate::element::InheritedBehavior) owns the
//! notification-facing `provider -> dependents` map. Teardown needs the inverse
//! question: which providers retain this dependent? Keeping that sparse index
//! on [`BuildOwner`](super::BuildOwner) avoids adding a collection to every
//! element node while making registration and lifecycle cleanup symmetric.

use std::collections::{HashMap, HashSet};

use flui_foundation::ElementId;
use smallvec::SmallVec;

/// The usual element reads one inherited value; two inline slots avoid a heap
/// allocation for the common theme-plus-media-query case as well.
pub(crate) type ProviderIds = SmallVec<[ElementId; 2]>;

/// Sparse reverse index owned by one element tree.
#[derive(Debug, Default)]
pub(crate) struct InheritedDependencies {
    /// Active dependent -> providers currently retaining it.
    active: HashMap<ElementId, ProviderIds>,
    /// Deactivated elements that had dependencies.
    ///
    /// Flutter removes provider registrations during `deactivate`, then calls
    /// `didChangeDependencies` if that element is reactivated. This sparse
    /// marker preserves the lifecycle fact without retaining stale provider
    /// ids or adding state to every element node.
    inactive_with_dependencies: HashSet<ElementId>,
}

impl InheritedDependencies {
    /// Record one active dependency, deduplicating repeated reads.
    pub(crate) fn register(&mut self, dependent: ElementId, provider: ElementId) {
        let providers = self.active.entry(dependent).or_default();
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }

    /// Remove an active dependency from the reverse index.
    ///
    /// Used when a provider itself unmounts and releases its forward map.
    pub(crate) fn unregister(&mut self, dependent: ElementId, provider: ElementId) {
        let Some(providers) = self.active.get_mut(&dependent) else {
            return;
        };
        providers.retain(|candidate| *candidate != provider);
        if providers.is_empty() {
            self.active.remove(&dependent);
        }
    }

    /// Detach an element from every provider while retaining the lifecycle fact
    /// that a reactivation must run `didChangeDependencies`.
    pub(crate) fn deactivate(&mut self, dependent: ElementId) -> ProviderIds {
        let providers = self.active.remove(&dependent).unwrap_or_default();
        if !providers.is_empty() {
            self.inactive_with_dependencies.insert(dependent);
        }
        providers
    }

    /// Complete permanent teardown and return any still-active providers.
    pub(crate) fn unmount(&mut self, dependent: ElementId) -> ProviderIds {
        self.inactive_with_dependencies.remove(&dependent);
        self.active.remove(&dependent).unwrap_or_default()
    }

    /// Consume the reactivation marker.
    pub(crate) fn activate(&mut self, dependent: ElementId) -> bool {
        self.inactive_with_dependencies.remove(&dependent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_and_tracks_deactivate_activate_lifecycle() {
        let dependent = ElementId::new(1);
        let first = ElementId::new(2);
        let second = ElementId::new(3);
        let mut dependencies = InheritedDependencies::default();

        dependencies.register(dependent, first);
        dependencies.register(dependent, first);
        dependencies.register(dependent, second);

        assert_eq!(
            dependencies.deactivate(dependent).as_slice(),
            &[first, second]
        );
        assert!(dependencies.activate(dependent));
        assert!(!dependencies.activate(dependent));
        assert!(dependencies.unmount(dependent).is_empty());
    }

    #[test]
    fn provider_unregistration_removes_only_its_reverse_edge() {
        let dependent = ElementId::new(1);
        let first = ElementId::new(2);
        let second = ElementId::new(3);
        let mut dependencies = InheritedDependencies::default();

        dependencies.register(dependent, first);
        dependencies.register(dependent, second);
        dependencies.unregister(dependent, first);

        assert_eq!(dependencies.unmount(dependent).as_slice(), &[second]);
        assert!(!dependencies.activate(dependent));
    }
}
