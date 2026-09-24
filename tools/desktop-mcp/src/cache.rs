//! Session-scoped element handles (`"e12"`).
//!
//! Agents address elements by short handles instead of OS objects. A handle
//! is keyed by the backend's stable identity for the element (UIA's runtime
//! id), so reading the same element twice returns the same handle, and the
//! handle always resolves to the most recently read OS object for it.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

use crate::error::{ToolError, ToolResult};

/// How many handles stay resolvable. A tree that keeps minting fresh
/// identities (dynamic content, a `wait_for` polling one) must not grow the
/// server without bound; the oldest handles go first, and a caller holding
/// one reads a fresh tree.
pub const CAPACITY: usize = 20_000;

/// Maps stable element identities to handles and handles to OS objects.
#[derive(Debug)]
pub struct ElementCache<K, T> {
    by_handle: HashMap<u32, (K, T, u64)>,
    by_key: HashMap<K, u32>,
    /// `(handle, touch)` in the order handles were last touched, oldest
    /// first. An entry is current only while its touch matches the handle's;
    /// a re-touched handle leaves a stale entry behind, skipped on eviction.
    order: VecDeque<(u32, u64)>,
    capacity: usize,
    next: u32,
    touches: u64,
}

impl<K: Eq + Hash + Clone, T> Default for ElementCache<K, T> {
    fn default() -> Self {
        Self::with_capacity(CAPACITY)
    }
}

impl<K: Eq + Hash + Clone, T> ElementCache<K, T> {
    /// A cache holding at most `capacity` handles.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            by_handle: HashMap::new(),
            by_key: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
            next: 1,
            touches: 0,
        }
    }

    /// Records `value` under its identity `key`, returning the handle — the
    /// existing one when the identity was seen before. Past the capacity the
    /// handle touched longest ago is dropped, so the handles of the read in
    /// progress (all touched last) stay resolvable.
    pub fn insert(&mut self, key: K, value: T) -> String {
        self.touches += 1;
        let touch = self.touches;
        let n = if let Some(&n) = self.by_key.get(&key) {
            n
        } else {
            while self.by_handle.len() >= self.capacity {
                let Some((oldest, seen)) = self.order.pop_front() else {
                    break;
                };
                if self
                    .by_handle
                    .get(&oldest)
                    .is_some_and(|(_, _, t)| *t == seen)
                    && let Some((old_key, _, _)) = self.by_handle.remove(&oldest)
                {
                    self.by_key.remove(&old_key);
                }
            }
            let n = self.next;
            self.next += 1;
            self.by_key.insert(key.clone(), n);
            n
        };
        self.by_handle.insert(n, (key, value, touch));
        self.order.push_back((n, touch));
        // Stale entries accumulate as handles are re-touched; drop them once
        // they outnumber the live ones.
        if self.order.len() > self.capacity * 2 {
            let live = &self.by_handle;
            self.order
                .retain(|(handle, seen)| live.get(handle).is_some_and(|(_, _, t)| t == seen));
        }
        format!("e{n}")
    }

    /// Resolves a handle issued by [`Self::insert`].
    pub fn get(&self, handle: &str) -> ToolResult<&T> {
        let n = parse_handle(handle)?;
        self.by_handle
            .get(&n)
            .map(|(_, value, _)| value)
            .ok_or_else(|| ToolError::UnknownElement(handle.to_owned()))
    }

    /// How many handles are live.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.by_handle.len()
    }
}

fn parse_handle(handle: &str) -> ToolResult<u32> {
    handle
        .trim()
        .strip_prefix('e')
        .and_then(|digits| digits.parse().ok())
        .ok_or_else(|| {
            ToolError::InvalidArgument(format!(
                "`{handle}` is not an element id; ids look like `e12`"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_short_and_sequential() {
        let mut cache = ElementCache::default();
        assert_eq!(cache.insert(vec![1, 2], "a"), "e1");
        assert_eq!(cache.insert(vec![1, 3], "b"), "e2");
        assert_eq!(cache.get("e1").copied().ok(), Some("a"));
        assert_eq!(cache.get("e2").copied().ok(), Some("b"));
    }

    #[test]
    fn same_identity_keeps_its_handle_and_refreshes_the_object() {
        let mut cache = ElementCache::default();
        let first = cache.insert(vec![42], "old");
        let again = cache.insert(vec![42], "new");
        assert_eq!(first, again);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&first).copied().ok(), Some("new"));
    }

    #[test]
    fn unknown_and_malformed_handles_are_distinct_errors() {
        let mut cache = ElementCache::<Vec<i32>, &str>::default();
        cache.insert(vec![1], "a");
        assert!(matches!(cache.get("e9"), Err(ToolError::UnknownElement(h)) if h == "e9"));
        for bad in ["", "12", "x1", "e", "e-1", "eabc"] {
            assert!(
                matches!(cache.get(bad), Err(ToolError::InvalidArgument(_))),
                "`{bad}` should be malformed"
            );
        }
        assert_eq!(cache.get(" e1 ").copied().ok(), Some("a"));
    }

    #[test]
    fn past_the_capacity_the_oldest_handle_goes() {
        let mut cache = ElementCache::with_capacity(2);
        let a = cache.insert("a", 1);
        let b = cache.insert("b", 2);
        let c = cache.insert("c", 3);
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&a).is_err(), "the oldest was dropped");
        assert_eq!(cache.get(&b).copied().ok(), Some(2));
        assert_eq!(cache.get(&c).copied().ok(), Some(3));
        assert_eq!(cache.insert("b", 20), b, "a kept identity keeps its handle");
        assert_ne!(
            cache.insert("a", 10),
            a,
            "a dropped identity gets a new one"
        );
    }

    /// A handle read again is the newest, so eviction takes one not seen
    /// since: a response never carries a handle its own read evicted.
    #[test]
    fn a_re_read_handle_outlives_older_ones() {
        let mut cache = ElementCache::with_capacity(2);
        let a = cache.insert("a", 1);
        let b = cache.insert("b", 2);
        assert_eq!(cache.insert("a", 10), a, "a re-read keeps its handle");
        let c = cache.insert("c", 3);
        assert!(cache.get(&b).is_err(), "b, touched longest ago, went");
        assert_eq!(cache.get(&a).copied().ok(), Some(10));
        assert_eq!(cache.get(&c).copied().ok(), Some(3));
    }
}
