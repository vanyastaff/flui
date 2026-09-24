//! Session-scoped element handles (`"e12"`).
//!
//! Agents address elements by short handles instead of OS objects. A handle
//! is keyed by the backend's stable identity for the element (UIA's runtime
//! id), so reading the same element twice returns the same handle, and the
//! handle always resolves to the most recently read OS object for it. An
//! identity the OS reuses for a new element after the old one is gone is
//! retired first ([`ElementCache::retire`]), so the old handle never follows
//! it to the new element.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

use crate::error::{HandleKind, ToolError, ToolResult};

/// How many handles stay resolvable. A tree that keeps minting fresh
/// identities (dynamic content, a `wait_for` polling one) must not grow the
/// server without bound; the oldest handles go first, and a caller holding
/// one reads a fresh tree.
pub const CAPACITY: usize = 20_000;

/// Maps stable element identities to handles and handles to OS objects.
#[derive(Debug)]
pub struct ElementCache<K, T> {
    by_handle: HashMap<u64, (K, T, u64)>,
    by_key: HashMap<K, u64>,
    /// `(handle, touch)` in the order handles were last touched, oldest
    /// first. An entry is current only while its touch matches the handle's;
    /// a re-touched handle leaves a stale entry behind, skipped on eviction.
    order: VecDeque<(u64, u64)>,
    capacity: usize,
    /// The next handle number; 64 bits never wrap into a live one.
    next: u64,
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
                    && self.by_key.get(&old_key) == Some(&oldest)
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

    /// The object last recorded under identity `key`.
    pub fn by_identity(&self, key: &K) -> Option<&T> {
        let n = self.by_key.get(key)?;
        self.by_handle.get(n).map(|(_, value, _)| value)
    }

    /// The handle identity `key` has now, if any.
    pub fn handle_of(&self, key: &K) -> Option<String> {
        self.by_key.get(key).map(|n| format!("e{n}"))
    }

    /// Forgets which handle identity `key` has: that handle keeps resolving
    /// to the object it held (gone, so it answers stale), and the next
    /// [`Self::insert`] of `key` issues a fresh handle.
    pub fn retire(&mut self, key: &K) {
        self.by_key.remove(key);
    }

    /// Resolves a handle issued by [`Self::insert`]. One issued and since
    /// evicted answers as gone, not as never issued.
    pub fn get(&self, handle: &str) -> ToolResult<&T> {
        let n = parse_handle(handle)?;
        self.by_handle
            .get(&n)
            .map(|(_, value, _)| value)
            .ok_or_else(|| self.missing(handle, n))
    }

    /// Why a parsed handle does not resolve: never issued, or issued and
    /// dropped since to make room for newer ones.
    fn missing(&self, handle: &str, n: u64) -> ToolError {
        if n < self.next {
            ToolError::Gone {
                handle: handle.to_owned(),
                kind: HandleKind::Element,
                why: format!(
                    "it was dropped to make room for newer handles (at most {} stay resolvable)",
                    self.capacity
                ),
            }
        } else {
            ToolError::UnknownHandle {
                handle: handle.to_owned(),
                kind: HandleKind::Element,
            }
        }
    }

    /// Resolves a handle for an update in place (a held element's state
    /// after an action it survived).
    #[cfg_attr(
        not(target_os = "windows"),
        allow(
            dead_code,
            reason = "used by the UIA backend, the only accessibility backend built yet"
        )
    )]
    pub fn get_mut(&mut self, handle: &str) -> ToolResult<&mut T> {
        let n = parse_handle(handle)?;
        if !self.by_handle.contains_key(&n) {
            return Err(self.missing(handle, n));
        }
        self.by_handle
            .get_mut(&n)
            .map(|(_, value, _)| value)
            .ok_or_else(|| ToolError::UnknownHandle {
                handle: handle.to_owned(),
                kind: HandleKind::Element,
            })
    }

    /// How many handles are live.
    pub fn len(&self) -> usize {
        self.by_handle.len()
    }
}

fn parse_handle(handle: &str) -> ToolResult<u64> {
    // An id is `e` and at most 20 digits; anything longer is not one, and is
    // not echoed back in full.
    if handle.len() > 21 {
        return Err(ToolError::InvalidArgument(
            "that is not an element id; ids look like `e12`".into(),
        ));
    }
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
        assert!(
            matches!(cache.get("e9"), Err(ToolError::UnknownHandle { handle, .. }) if handle == "e9")
        );
        for bad in ["", "12", "x1", "e", "e-1", "eabc"] {
            assert!(
                matches!(cache.get(bad), Err(ToolError::InvalidArgument(_))),
                "`{bad}` should be malformed"
            );
        }
        assert_eq!(cache.get(" e1 ").copied().ok(), Some("a"));
        // Padding counts: the error never echoes a long value in full.
        let padded = format!("{}x{}", " ".repeat(10_000), " ".repeat(10_000));
        assert!(matches!(cache.get(&padded), Err(ToolError::InvalidArgument(m)) if m.len() < 100));
    }

    #[test]
    fn past_the_capacity_the_oldest_handle_goes() {
        let mut cache = ElementCache::with_capacity(2);
        let a = cache.insert("a", 1);
        let b = cache.insert("b", 2);
        let c = cache.insert("c", 3);
        assert_eq!(cache.len(), 2);
        assert!(
            matches!(cache.get(&a), Err(ToolError::Gone { .. })),
            "the oldest was dropped, and says so rather than 'never issued'"
        );
        assert_eq!(cache.get(&b).copied().ok(), Some(2));
        assert_eq!(cache.get(&c).copied().ok(), Some(3));
        assert_eq!(cache.insert("b", 20), b, "a kept identity keeps its handle");
        assert_ne!(
            cache.insert("a", 10),
            a,
            "a dropped identity gets a new one"
        );
    }

    /// An identity reused for a new element after its old one went is
    /// retired: the old handle keeps its old object (and answers stale),
    /// the new element gets its own handle, and evicting the old handle
    /// later does not unmap the new one.
    #[test]
    fn a_retired_identity_gets_a_fresh_handle() {
        let mut cache = ElementCache::with_capacity(3);
        let old = cache.insert("id", "removed button");
        assert_eq!(cache.by_identity(&"id").copied(), Some("removed button"));
        assert_eq!(cache.handle_of(&"id"), Some(old.clone()));
        cache.retire(&"id");
        assert_eq!(cache.handle_of(&"id"), None);
        let new = cache.insert("id", "new control");
        assert_ne!(old, new);
        assert_eq!(cache.get(&old).copied().ok(), Some("removed button"));
        assert_eq!(cache.get(&new).copied().ok(), Some("new control"));
        cache.insert("x", "x");
        cache.insert("y", "y");
        assert!(cache.get(&old).is_err(), "the old handle was evicted");
        assert_eq!(
            cache.insert("id", "new control"),
            new,
            "the new mapping survived"
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
