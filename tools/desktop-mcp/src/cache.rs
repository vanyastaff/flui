//! Session-scoped element handles (`"e12"`).
//!
//! Agents address elements by short handles instead of OS objects. A handle
//! is keyed by the backend's stable identity for the element (UIA's runtime
//! id), so reading the same element twice returns the same handle, and the
//! handle always resolves to the most recently read OS object for it.

use std::collections::HashMap;
use std::hash::Hash;

use crate::error::{ToolError, ToolResult};

/// Maps stable element identities to handles and handles to OS objects.
#[derive(Debug)]
pub struct ElementCache<K, T> {
    by_handle: HashMap<u32, T>,
    by_key: HashMap<K, u32>,
    next: u32,
}

impl<K: Eq + Hash, T> Default for ElementCache<K, T> {
    fn default() -> Self {
        Self {
            by_handle: HashMap::new(),
            by_key: HashMap::new(),
            next: 1,
        }
    }
}

impl<K: Eq + Hash, T> ElementCache<K, T> {
    /// Records `value` under its identity `key`, returning the handle — the
    /// existing one when the identity was seen before.
    pub fn insert(&mut self, key: K, value: T) -> String {
        let n = if let Some(&n) = self.by_key.get(&key) {
            n
        } else {
            let n = self.next;
            self.next += 1;
            self.by_key.insert(key, n);
            n
        };
        self.by_handle.insert(n, value);
        format!("e{n}")
    }

    /// Resolves a handle issued by [`Self::insert`].
    pub fn get(&self, handle: &str) -> ToolResult<&T> {
        let n = parse_handle(handle)?;
        self.by_handle
            .get(&n)
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
}
