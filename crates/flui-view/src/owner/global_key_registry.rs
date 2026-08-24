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
//!
//! Flutter parity: `BuildOwner._globalKeyRegistry`
//! (`framework.dart:3165`) is a `Map<GlobalKey, Element>` keyed on the key
//! object itself — Dart's `Map` gives identity semantics for free because
//! `GlobalKey` uses default (reference) equality. `Box<dyn ViewKey>` has no
//! blanket `Hash + Eq`, so we get the same semantics explicitly: hash to a
//! bucket, then `key_eq` within it.
//!
//! # Not the uniqueness authority
//!
//! This map answers "which element in *this* owner's tree holds this key?".
//! Cross-owner uniqueness is [`GlobalKeyScope`](super::GlobalKeyScope)'s
//! job, and per-frame duplicate-declaration reporting is
//! [`global_key_reservations`](super::global_key_reservations)' job. The
//! three never merge — see `global_key_scope`'s "Split authority" section.

use std::collections::HashMap;

use flui_foundation::{ElementId, ViewKey};

/// One live registration: the key that owns the entry, plus the element it
/// resolves to.
///
/// The key is stored **by value** (`clone_key`) rather than by hash so the
/// entry can be identity-compared later, after the view that declared it is
/// long gone.
struct Entry {
    key: Box<dyn ViewKey>,
    element: ElementId,
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

    /// Register `key -> element`, returning the element it displaced (the
    /// same-key last-write-wins case) if there was one.
    ///
    /// A *different* key that merely collides on hash is never displaced: it
    /// keeps its own entry in the same bucket.
    pub(crate) fn insert(&mut self, key: &dyn ViewKey, element: ElementId) -> Option<ElementId> {
        let bucket = self.buckets.entry(key.key_hash()).or_default();
        if let Some(entry) = bucket.iter_mut().find(|entry| entry.key.key_eq(key)) {
            return Some(std::mem::replace(&mut entry.element, element));
        }
        bucket.push(Entry {
            key: key.clone_key(),
            element,
        });
        None
    }

    /// Remove `key`'s registration, returning the element it held.
    pub(crate) fn remove(&mut self, key: &dyn ViewKey) -> Option<ElementId> {
        let hash = key.key_hash();
        let bucket = self.buckets.get_mut(&hash)?;
        let position = bucket.iter().position(|entry| entry.key.key_eq(key))?;
        let removed = bucket.swap_remove(position);
        if bucket.is_empty() {
            self.buckets.remove(&hash);
        }
        Some(removed.element)
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

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::fmt;

    use super::*;

    /// A key whose hash is chosen by the test, so a collision between two
    /// *distinct* keys can be built on purpose. `is_global_key` is true
    /// because that is the population this registry holds.
    #[derive(Clone, PartialEq, Eq)]
    struct StubKey {
        identity: u32,
        hash: u64,
    }

    impl ViewKey for StubKey {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn key_eq(&self, other: &dyn ViewKey) -> bool {
            other
                .as_any()
                .downcast_ref::<Self>()
                .is_some_and(|other| self.identity == other.identity)
        }

        fn key_hash(&self) -> u64 {
            self.hash
        }

        fn clone_key(&self) -> Box<dyn ViewKey> {
            Box::new(self.clone())
        }

        fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "StubKey({}#{})", self.identity, self.hash)
        }

        fn is_global_key(&self) -> bool {
            true
        }
    }

    fn eid(n: usize) -> ElementId {
        ElementId::new(n)
    }

    #[test]
    fn a_fresh_registry_is_empty() {
        let registry = GlobalKeyRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn insert_then_get_resolves_by_identity_not_by_the_borrow_that_inserted_it() {
        let mut registry = GlobalKeyRegistry::new();
        let key = StubKey {
            identity: 1,
            hash: 7,
        };
        assert_eq!(registry.insert(&key, eid(1)), None);

        // A *different* value with the same identity — the ordinary case of a
        // `GlobalKey` cloned into a second view.
        let clone = StubKey {
            identity: 1,
            hash: 7,
        };
        assert_eq!(registry.get(&clone), Some(eid(1)));
        assert_eq!(registry.len(), 1);
    }

    /// The property the old hash-keyed map could not hold: two distinct keys
    /// that collide stay two registrations, each resolving to its own
    /// element.
    #[test]
    fn two_distinct_keys_sharing_one_hash_stay_distinct() {
        let mut registry = GlobalKeyRegistry::new();
        let first = StubKey {
            identity: 1,
            hash: 42,
        };
        let second = StubKey {
            identity: 2,
            hash: 42,
        };

        assert_eq!(registry.insert(&first, eid(1)), None);
        assert_eq!(
            registry.insert(&second, eid(2)),
            None,
            "a colliding but distinct key displaces nothing",
        );

        assert_eq!(registry.len(), 2, "both registrations are live");
        assert_eq!(registry.get(&first), Some(eid(1)));
        assert_eq!(registry.get(&second), Some(eid(2)));
    }

    #[test]
    fn re_registering_the_same_key_reports_the_element_it_displaced() {
        let mut registry = GlobalKeyRegistry::new();
        let key = StubKey {
            identity: 1,
            hash: 7,
        };

        assert_eq!(registry.insert(&key, eid(1)), None);
        assert_eq!(registry.insert(&key, eid(2)), Some(eid(1)));
        assert_eq!(registry.get(&key), Some(eid(2)));
        assert_eq!(registry.len(), 1, "last-write-wins, not a second entry");
    }

    #[test]
    fn removing_one_of_two_colliding_keys_leaves_the_other_resolvable() {
        let mut registry = GlobalKeyRegistry::new();
        let first = StubKey {
            identity: 1,
            hash: 42,
        };
        let second = StubKey {
            identity: 2,
            hash: 42,
        };
        registry.insert(&first, eid(1));
        registry.insert(&second, eid(2));

        assert_eq!(registry.remove(&first), Some(eid(1)));
        assert_eq!(registry.get(&first), None);
        assert_eq!(
            registry.get(&second),
            Some(eid(2)),
            "the surviving collision partner is untouched",
        );
        assert_eq!(registry.len(), 1);

        assert_eq!(registry.remove(&second), Some(eid(2)));
        assert!(registry.is_empty());
    }

    #[test]
    fn removing_an_unregistered_key_is_a_none_not_a_panic() {
        let mut registry = GlobalKeyRegistry::new();
        let key = StubKey {
            identity: 1,
            hash: 7,
        };
        assert_eq!(registry.remove(&key), None);

        // Same hash, nothing registered under this identity.
        registry.insert(
            &StubKey {
                identity: 2,
                hash: 7,
            },
            eid(2),
        );
        assert_eq!(registry.remove(&key), None);
        assert_eq!(registry.len(), 1);
    }
}
