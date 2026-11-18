//! A hash map whose every lookup, insertion, removal and walk counts, in a
//! test build, the entries it touches, so a test can bound the work of the
//! code above it whatever that code does: no access reaches the table
//! without passing a count.

use std::hash::Hash;

use rustc_hash::FxHashMap;

/// An [`FxHashMap`] reached only through methods that count the entries
/// they touch.
#[derive(Debug)]
pub(crate) struct TalliedMap<K, V> {
    map: FxHashMap<K, V>,
    /// The entries reached since [`Self::take_visits`] last answered.
    #[cfg(test)]
    visits: u64,
}

impl<K: Hash + Eq, V> TalliedMap<K, V> {
    pub(crate) fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            #[cfg(test)]
            visits: 0,
        }
    }

    #[cfg_attr(
        not(test),
        expect(clippy::unused_self, reason = "only a test build counts the visit")
    )]
    fn visit(&mut self, entries: usize) {
        #[cfg(test)]
        {
            self.visits += entries as u64;
        }
        #[cfg(not(test))]
        let _ = entries;
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        self.visit(1);
        self.map.get(key)
    }

    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.visit(1);
        self.map.get_mut(key)
    }

    /// The value at `key`, a default one put there first when there is none.
    pub(crate) fn get_or_default(&mut self, key: K) -> &mut V
    where
        V: Default,
    {
        self.visit(1);
        self.map.entry(key).or_default()
    }

    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        self.visit(1);
        self.map.remove(key)
    }

    /// Every value: a walk that reaches every entry, counted as such.
    #[cfg(test)]
    pub(crate) fn values(&mut self) -> impl Iterator<Item = &V> {
        let entries = self.map.len();
        self.visit(entries);
        self.map.values()
    }

    /// Lets every entry go, and the table's room with them.
    pub(crate) fn release(&mut self) {
        let entries = self.map.len();
        self.visit(entries);
        self.map = FxHashMap::default();
    }

    /// The entries the table has room for.
    #[cfg(test)]
    pub(crate) fn room(&self) -> usize {
        self.map.capacity()
    }

    /// The entries reached since this last answered, which starts the
    /// count again.
    #[cfg(test)]
    pub(crate) fn take_visits(&mut self) -> u64 {
        std::mem::take(&mut self.visits)
    }
}
