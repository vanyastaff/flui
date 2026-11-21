//! The `GlobalKey` registry as the owner exposes it: registering, looking up, reparenting and the duplicate diagnostics.

use super::BuildOwner;
use crate::owner::{DuplicateGlobalKey, global_key_scope};
use flui_foundation::{ElementId, ViewKey};

impl BuildOwner {
    /// Register a GlobalKey for an element.
    ///
    /// GlobalKeys allow elements to be found and reparented across the tree.
    /// If a [`GlobalKeyScope`](crate::owner::GlobalKeyScope) is installed (or, absent that, this owner's
    /// lazily self-owned private one — see [`Self::set_global_key_scope`]),
    /// this first claims `key` in that scope: a different owner already
    /// holding the same hash there is a cross-owner collision, traced then
    /// panicked (ADR-0043) rather than silently recorded here.
    ///
    /// # Panics
    ///
    /// Panics if `key` is already claimed, in the installed
    /// [`GlobalKeyScope`](crate::owner::GlobalKeyScope), by a *different* owner (see
    /// [`GlobalKeyScope`](crate::owner::GlobalKeyScope)'s contract). Re-registering a key this same owner
    /// already holds never panics. This panic is fatal, not recoverable: it
    /// fires from a post-mount registration site, the same timing the
    /// pre-existing intra-tree duplicate-key panic already has, so this
    /// owner's tree is left with an incomplete mount. A host must not catch
    /// it and keep using this owner's tree.
    pub fn register_global_key(&mut self, key: &dyn ViewKey, element: ElementId) {
        global_key_scope::claim_and_register(
            &mut self.global_key_scope,
            self.owner_tag,
            key,
            element,
            &mut self.global_keys,
        );
    }

    /// Unregister a GlobalKey.
    ///
    /// Also releases this owner's scope claim on `key`, if one is
    /// installed — tag-checked, so this is a no-op against a claim a
    /// different owner has since taken (ADR-0043).
    pub fn unregister_global_key(&mut self, key: &dyn ViewKey) {
        global_key_scope::release_and_unregister(
            self.global_key_scope.as_ref(),
            self.owner_tag,
            key,
            &mut self.global_keys,
        );
    }

    /// Look up an element by GlobalKey.
    ///
    /// Resolution is by key identity — a different key that merely hashes
    /// alike never answers this lookup.
    pub fn element_for_global_key(&self, key: &dyn ViewKey) -> Option<ElementId> {
        self.global_keys.get(key)
    }

    /// Atomically remove and return the element registered under
    /// `key` for a reparent operation.
    ///
    /// Closes the race window that a
    /// two-call sequence (`element_for_global_key` followed by
    /// `unregister_global_key`) would leave open if any other code
    /// path mutates the registry between the two calls — a real
    /// risk in Phase 2 testing infrastructure where multiple
    /// parents may rebuild concurrently in test fixtures.
    ///
    /// The caller (the keyed reconciler's middle-walk) consults
    /// this method on an unmatched-by-position keyed view whose
    /// `is_global_key()` is true; on `Some`, it claims the element
    /// for the new parent. Returning `Some` AND removing the entry
    /// in one operation guarantees a second concurrent claim of
    /// the same key sees `None`, not a stale id.
    ///
    /// Re-registering at the new parent is the caller's
    /// responsibility — typically through the standard
    /// [`Self::register_global_key`] path after the element is
    /// re-attached to its new slot.
    pub fn take_global_key_for_reparent(&mut self, key: &dyn ViewKey) -> Option<ElementId> {
        self.global_keys.remove(key)
    }

    /// Drain the duplicate-`GlobalKey` reports the frame boundaries since
    /// the last drain produced.
    ///
    /// A duplicate `GlobalKey` is caller-controlled input, so
    /// [`Self::finalize_tree`] reports it as data rather than panicking:
    /// the offending frame still completes, with the losing parent's
    /// dangling child edge already repaired. Hosts that want a hard failure
    /// can drain this and escalate; tests assert on it directly. The verdict
    /// is reached in every build profile.
    pub fn take_global_key_diagnostics(&mut self) -> Vec<DuplicateGlobalKey> {
        std::mem::take(&mut self.global_key_diagnostics)
    }

    /// The duplicate-`GlobalKey` reports currently pending, without
    /// draining them.
    pub fn global_key_diagnostics(&self) -> &[DuplicateGlobalKey] {
        &self.global_key_diagnostics
    }

    /// Number of `GlobalKey`s currently registered.
    ///
    /// Test surface — production code reads
    /// [`BuildOwner::element_for_global_key`] on a single key rather
    /// than scanning size. Tests use this to confirm the registry
    /// stays at the expected size across mount / unmount cycles.
    pub fn global_keys_len(&self) -> usize {
        self.global_keys.len()
    }
}
