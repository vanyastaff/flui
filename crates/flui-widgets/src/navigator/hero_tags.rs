//! Owner-local Hero matching. Hashes accelerate lookup; allocation identities
//! authorize release. Authored key operations and retirement run outside borrows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use super::hero::HeroTag;
use super::lifecycle::{RetiredValues, Terminal};

struct Entry<T> {
    tag: Terminal<HeroTag>,
    value: Terminal<T>,
}

impl<T> Drop for Entry<T> {
    fn drop(&mut self) {
        let tag = self.tag.withdraw();
        let value = self.value.withdraw();
        drop((tag, value));
    }
}

/// Passive release authority. Weak ownership preserves allocation identity
/// without keeping a withdrawn key or value alive.
pub(super) struct TagSeat<T> {
    hash: u64,
    entry: Weak<Entry<T>>,
}

impl<T> Clone for TagSeat<T> {
    fn clone(&self) -> Self {
        Self {
            hash: self.hash,
            entry: self.entry.clone(),
        }
    }
}

pub(super) struct HeroTags<T> {
    buckets: RefCell<HashMap<u64, Vec<Rc<Entry<T>>>>>,
}

impl<T> Drop for HeroTags<T> {
    fn drop(&mut self) {
        let mut buckets = Terminal::new(std::mem::take(self.buckets.get_mut()).into_values());
        for bucket in buckets.by_ref() {
            drop(RetiredValues(bucket));
        }
    }
}

impl<T> Default for HeroTags<T> {
    fn default() -> Self {
        Self {
            buckets: RefCell::new(HashMap::new()),
        }
    }
}

impl<T> HeroTags<T> {
    fn snapshot(&self, hash: u64) -> RetiredValues<Rc<Entry<T>>> {
        RetiredValues(
            self.buckets
                .borrow()
                .get(&hash)
                .cloned()
                .unwrap_or_default(),
        )
    }

    fn matches(&self, hash: u64, snapshot: &[Rc<Entry<T>>]) -> bool {
        self.buckets
            .borrow()
            .get(&hash)
            .map_or(snapshot.is_empty(), |bucket| {
                bucket.len() == snapshot.len()
                    && bucket
                        .iter()
                        .zip(snapshot)
                        .all(|(current, saved)| Rc::ptr_eq(current, saved))
            })
    }

    pub(super) fn len(&self) -> usize {
        self.buckets.borrow().values().map(Vec::len).sum()
    }

    /// First wins. Rollback exists before outgoing pins retire; a failed
    /// retirement cannot leave admission with no local release authority.
    pub(super) fn insert_first(&self, tag: HeroTag, value: T) -> Option<TagAdmission<'_, T>> {
        let mut tag = Terminal::new(tag);
        let mut value = Terminal::new(value);
        let hash = tag.key_hash();
        for attempt in 0..2 {
            let snapshot = self.snapshot(hash);
            let occupied = snapshot.0.iter().any(|entry| *entry.tag == *tag);
            if !self.matches(hash, &snapshot.0) {
                assert!(
                    attempt == 0,
                    "Hero tag comparison changed registry repeatedly"
                );
                drop(snapshot);
                continue;
            }
            if occupied {
                return None;
            }
            let entry = Rc::new(Entry {
                tag: Terminal::new(tag.take_value()),
                value: Terminal::new(value.take_value()),
            });
            let seat = TagSeat {
                hash,
                entry: Rc::downgrade(&entry),
            };
            self.buckets
                .borrow_mut()
                .entry(hash)
                .or_default()
                .push(entry);
            let admission = TagAdmission {
                tags: self,
                seat: Some(seat),
            };
            drop(snapshot);
            return Some(admission);
        }
        unreachable!("BUG: a second invalidated comparison refuses admission")
    }

    /// Removes only this allocation; no authored hashing, equality or cloning.
    pub(super) fn remove(&self, seat: &TagSeat<T>) -> Option<RemovedTag<T>> {
        let removed = {
            let mut buckets = self.buckets.borrow_mut();
            let bucket = buckets.get_mut(&seat.hash)?;
            let index = bucket
                .iter()
                .position(|entry| std::ptr::eq(Rc::as_ptr(entry), seat.entry.as_ptr()))?;
            let removed = bucket.remove(index);
            if bucket.is_empty() {
                buckets.remove(&seat.hash);
            }
            removed
        };
        Some(RemovedTag(Terminal::new(removed)))
    }
}

impl<T: Clone> HeroTags<T> {
    pub(super) fn get(&self, tag: &HeroTag) -> Option<T> {
        let hash = tag.key_hash();
        for attempt in 0..2 {
            let snapshot = self.snapshot(hash);
            let matching = snapshot.0.iter().find(|entry| *entry.tag == *tag);
            if !self.matches(hash, &snapshot.0) {
                assert!(
                    attempt == 0,
                    "Hero tag comparison changed registry repeatedly"
                );
                drop(snapshot);
                continue;
            }
            let mut value = matching.map(|entry| Terminal::new((*entry.value).clone()));
            drop(snapshot);
            return value.as_mut().map(Terminal::take_value);
        }
        unreachable!("BUG: a second invalidated comparison refuses lookup")
    }
}

impl<T> HeroTags<T> {
    /// Keep owning pins through the caller's matching or cancellation round.
    pub(super) fn snapshot_all(&self) -> TagSnapshot<T> {
        TagSnapshot(RetiredValues(
            self.buckets
                .borrow()
                .values()
                .flat_map(|bucket| bucket.iter().cloned())
                .collect(),
        ))
    }
}

pub(super) struct TagSnapshot<T>(RetiredValues<Rc<Entry<T>>>);

impl<T> TagSnapshot<T> {
    pub(super) fn iter(&self) -> impl Iterator<Item = (&HeroTag, &T)> {
        self.0.0.iter().map(|entry| (&*entry.tag, &*entry.value))
    }
}

pub(super) struct TagAdmission<'a, T> {
    tags: &'a HeroTags<T>,
    seat: Option<TagSeat<T>>,
}

impl<T> TagAdmission<'_, T> {
    pub(super) fn commit(mut self) -> TagSeat<T> {
        self.seat.take().expect("BUG: admission committed once")
    }
}

impl<T> Drop for TagAdmission<'_, T> {
    fn drop(&mut self) {
        if let Some(seat) = self.seat.take() {
            drop(self.tags.remove(&seat));
        }
    }
}

pub(super) struct RemovedTag<T>(Terminal<Rc<Entry<T>>>);

impl<T> RemovedTag<T> {
    pub(super) fn value(&self) -> &T {
        &self.0.value
    }
}
