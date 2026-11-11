//! Side tables addressed by a generational key's slot: membership and
//! position without hashing, cleared in the time of what they hold.
//!
//! A per-frame set over tree nodes (the dirty queues, the elements built this
//! frame) is filled from a handful of nodes and cleared every frame. A hash
//! table is the wrong shape for that: its `clear` walks every bucket it ever
//! grew, so after a mount that dirtied the whole tree, each later frame pays
//! for the whole tree again. These types index a dense `Vec` by the key's
//! slot instead, and their `clear` touches only the entries they hold, so a
//! frame that marks four nodes costs four nodes on a tree of any size.
//!
//! The slot is only the address: every lookup compares the full key, slot
//! and generation, so a stale key whose slot was reused is not a member
//! (ADR-0313). The table grows to the highest slot it has seen and keeps
//! that capacity, one `u32` per slot.

use std::marker::PhantomData;

use crate::id::{ElementId, GenId, Marker};

/// A generational key whose slot index can address a dense side table.
///
/// The slot alone is not an identity: a side table keyed by it stores the
/// full key beside the slot and compares it on every lookup.
pub trait SlotKey: Copy + Eq {
    /// The 0-based slot this key's arena stores it in.
    fn slot(self) -> usize;
}

impl<T: Marker> SlotKey for GenId<T> {
    #[inline]
    fn slot(self) -> usize {
        self.index() as usize
    }
}

impl SlotKey for ElementId {
    #[inline]
    fn slot(self) -> usize {
        self.index() as usize
    }
}

/// Where each key sits in an ordered list its owner keeps: slot → position.
///
/// The owner holds the entries (and so the full keys); this index holds only
/// positions and is consulted through [`Self::find`], which confirms the
/// entry at the stored position carries the asked key. Every reorder or
/// removal in the owner's list is followed by [`Self::reindex`].
#[derive(Debug)]
pub struct SlotIndex<K> {
    /// `position + 1` per slot; 0 is "not listed".
    positions: Vec<u32>,
    _key: PhantomData<fn() -> K>,
}

impl<K> Default for SlotIndex<K> {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            _key: PhantomData,
        }
    }
}

impl<K: SlotKey> SlotIndex<K> {
    /// An empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The position of `key` in `entries`, or `None` when it is not listed.
    ///
    /// `key_of` reads an entry's key; the entry at the stored position must
    /// carry `key` itself, so a key of the same slot and an older or newer
    /// generation is not found.
    #[inline]
    pub fn find<E>(&self, entries: &[E], key: K, key_of: impl Fn(&E) -> K) -> Option<usize> {
        let stored = *self.positions.get(key.slot())?;
        let position = usize::try_from(stored).ok()?.checked_sub(1)?;
        entries
            .get(position)
            .filter(|entry| key_of(entry) == key)
            .map(|_| position)
    }

    /// Records that `key` sits at `position`, replacing whatever its slot
    /// recorded.
    ///
    /// # Panics
    ///
    /// Panics when `position` does not fit the index's `u32` positions: a
    /// list of more than `u32::MAX - 1` entries.
    #[inline]
    pub fn insert(&mut self, key: K, position: usize) {
        let slot = key.slot();
        if slot >= self.positions.len() {
            self.positions.resize(slot + 1, 0);
        }
        self.positions[slot] = position
            .checked_add(1)
            .and_then(|stored| u32::try_from(stored).ok())
            .expect("BUG: a slot-indexed list holds fewer than u32::MAX entries");
    }

    /// Forgets the position recorded for `key`'s slot.
    #[inline]
    pub fn remove(&mut self, key: K) {
        if let Some(stored) = self.positions.get_mut(key.slot()) {
            *stored = 0;
        }
    }

    /// Forgets every key in `keys`: the owner's list is about to be cleared.
    ///
    /// Costs the length of `keys`, not the index's capacity.
    #[inline]
    pub fn remove_all(&mut self, keys: impl IntoIterator<Item = K>) {
        for key in keys {
            self.remove(key);
        }
    }

    /// Records the position of every key in `keys`, in order: the owner's
    /// list was reordered or shortened. The caller first passes the keys it
    /// removed to [`Self::remove_all`].
    #[inline]
    pub fn reindex(&mut self, keys: impl IntoIterator<Item = K>) {
        for (position, key) in keys.into_iter().enumerate() {
            self.insert(key, position);
        }
    }
}

/// An insertion-ordered set of keys: O(1) insert and membership, and a
/// `clear` that costs the keys it holds.
#[derive(Debug)]
pub struct SlotSet<K> {
    keys: Vec<K>,
    index: SlotIndex<K>,
}

impl<K> Default for SlotSet<K> {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            index: SlotIndex::default(),
        }
    }
}

impl<K: SlotKey> SlotSet<K> {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `key`; `true` when it was not a member.
    #[inline]
    pub fn insert(&mut self, key: K) -> bool {
        if self.contains(key) {
            return false;
        }
        self.index.insert(key, self.keys.len());
        self.keys.push(key);
        true
    }

    /// Whether `key` is a member.
    #[inline]
    #[must_use]
    pub fn contains(&self, key: K) -> bool {
        self.index.find(&self.keys, key, |k| *k).is_some()
    }

    /// Removes `key`; `true` when it was a member.
    pub fn remove(&mut self, key: K) -> bool {
        let Some(position) = self.index.find(&self.keys, key, |k| *k) else {
            return false;
        };
        self.index.remove(key);
        self.keys.swap_remove(position);
        if let Some(&moved) = self.keys.get(position) {
            self.index.insert(moved, position);
        }
        true
    }

    /// The number of members.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the set is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The members, in insertion order (a `remove` moves the last member
    /// into the removed one's place).
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[K] {
        &self.keys
    }

    /// Removes every member, keeping capacity. Costs the members it held.
    #[inline]
    pub fn clear(&mut self) {
        self.index.remove_all(self.keys.iter().copied());
        self.keys.clear();
    }
}
