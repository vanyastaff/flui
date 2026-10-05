//! The intra-tree `GlobalKey` registry — **identity is the identity, the
//! hash is only an index**.
//!
//! # Why this exists
//!
//! The registry used to be a bare `HashMap<u64, ElementId>` keyed on
//! [`ViewKey::key_hash`]. That makes a hash the *identity* of a key, which
//! is a category error with two consequences:
//!
//! - two genuinely distinct keys that happen to hash alike are
//!   indistinguishable, so one silently evicts (and can even be *re-taken
//!   by*) the other;
//! - the framework cannot tell a real duplicate-`GlobalKey` bug — the thing
//!   the caller wants reported — from an accidental collision it should
//!   simply route around.
//!
//! This type keeps the hash as a pure *accelerator*: it buckets by
//! `key_hash()` and then decides membership with [`ViewKey::key_eq`]. Two
//! colliding-but-distinct keys land in one bucket and stay two entries; the
//! same key looked up through any clone resolves to the one entry it owns.
//! `Box<dyn ViewKey>` has no blanket `Hash + Eq`, so identity semantics are
//! explicit: hash to a bucket, then `key_eq` within it.
//!
//! # Not the uniqueness authority
//!
//! This map answers "which element in *this* owner's tree holds this key?".
//! Cross-owner uniqueness is [`GlobalKeyScope`](super::GlobalKeyScope)'s
//! job, and per-frame duplicate-declaration reporting is
//! [`global_key_reservations`](super::global_key_reservations)' job. The
//! three never merge — see `global_key_scope`'s "Split authority" section.

use std::{collections::HashMap, ops::Deref};

use flui_foundation::{ElementId, ViewKey};

use super::global_key_scope::ClaimIdentity;

/// One independently owned key envelope. Ordinary destruction remains observable;
/// an existing unwind retains this envelope before its user destructor can run.
pub(super) struct OwnedGlobalKey(Option<Box<dyn ViewKey>>);

impl OwnedGlobalKey {
    pub(super) fn new(key: Box<dyn ViewKey>) -> Self {
        Self(Some(key))
    }

    pub(super) fn as_ref(&self) -> &dyn ViewKey {
        self.0
            .as_deref()
            .expect("BUG: live key envelope is occupied")
    }
}

impl Deref for OwnedGlobalKey {
    type Target = dyn ViewKey;
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl std::fmt::Debug for OwnedGlobalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_ref().debug_fmt(f)
    }
}

impl Drop for OwnedGlobalKey {
    fn drop(&mut self) {
        let key = self.0.take();
        if std::thread::panicking() {
            std::mem::forget(key);
        } else {
            drop(key);
        }
    }
}

/// One live registration: the key that owns the entry, plus the element it
/// resolves to.
///
/// The key is stored **by value** (`clone_key`) rather than by hash so the
/// entry can be identity-compared later, after the view that declared it is
/// long gone.
struct Entry {
    key: OwnedGlobalKey,
    element: ElementId,
    claim: ClaimIdentity,
}

/// `GlobalKey` → `ElementId` for one [`BuildOwner`](super::BuildOwner)'s own
/// tree, keyed by key **identity** with the hash used only to pick a bucket.
///
/// Buckets are `Vec`s because a collision is rare enough that a linear
/// `key_eq` scan over one or two entries beats any cleverer structure, and
/// explicit enough that the identity check cannot be optimised away by
/// accident.
#[derive(Default)]
pub(crate) struct GlobalKeyRegistry {
    buckets: HashMap<u64, Vec<Entry>>,
}

pub(super) struct PreparedRegistration {
    hash: u64,
    key: Option<OwnedGlobalKey>,
}

impl GlobalKeyRegistry {
    /// An empty registry.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The element currently registered under `key`, by identity.
    pub(crate) fn get(&self, key: &dyn ViewKey) -> Option<ElementId> {
        self.buckets
            .get(&key.key_hash())?
            .iter()
            .find(|entry| entry.key.key_eq(key))
            .map(|entry| entry.element)
    }

    /// Prepare every user-owned clone before taking scope authority.
    pub(super) fn prepare(&self, key: &dyn ViewKey) -> PreparedRegistration {
        let hash = key.key_hash();
        let present = self
            .buckets
            .get(&hash)
            .is_some_and(|bucket| bucket.iter().any(|entry| entry.key.key_eq(key)));
        PreparedRegistration {
            hash,
            key: (!present).then(|| OwnedGlobalKey::new(key.clone_key())),
        }
    }

    /// Publish prepared ownership; the caller guards scope rollback if an
    /// identity comparison fails before local admission.
    pub(super) fn insert_prepared(
        &mut self,
        key: &dyn ViewKey,
        element: ElementId,
        mut prepared: PreparedRegistration,
        claim: ClaimIdentity,
    ) -> Option<ElementId> {
        let bucket = self.buckets.entry(prepared.hash).or_default();
        if let Some(entry) = bucket.iter_mut().find(|entry| entry.key.key_eq(key)) {
            entry.claim = claim;
            return Some(std::mem::replace(&mut entry.element, element));
        }
        bucket.push(Entry {
            key: prepared
                .key
                .take()
                .expect("BUG: new registration has a prepared key"),
            element,
            claim,
        });
        None
    }

    /// Remove `key`'s registration, returning the element it held.
    pub(crate) fn remove(&mut self, key: &dyn ViewKey) -> Option<ElementId> {
        self.take_registration(key)
            .map(|(element, _key, _claim)| element)
    }

    /// Commit local withdrawal without running a user destructor.
    pub(super) fn take_registration(
        &mut self,
        key: &dyn ViewKey,
    ) -> Option<(ElementId, OwnedGlobalKey, ClaimIdentity)> {
        let hash = key.key_hash();
        let bucket = self.buckets.get_mut(&hash)?;
        let position = bucket.iter().position(|entry| entry.key.key_eq(key))?;
        let removed = bucket.swap_remove(position);
        if bucket.is_empty() {
            self.buckets.remove(&hash);
        }
        Some((removed.element, removed.key, removed.claim))
    }

    /// Number of registered keys.
    ///
    /// Summed over the buckets rather than cached: this is a diagnostic and
    /// test surface (production resolves one key at a time), and an
    /// incrementally-maintained counter would be one more invariant to keep
    /// true for no gain. An empty bucket is dropped on removal, so the sum
    /// never counts corpses.
    pub(crate) fn len(&self) -> usize {
        self.buckets.values().map(Vec::len).sum()
    }

    /// Whether any key is registered.
    pub(crate) fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }
}

impl std::fmt::Debug for GlobalKeyRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalKeyRegistry")
            .field("len", &self.len())
            .field("buckets", &self.buckets.len())
            .finish()
    }
}
