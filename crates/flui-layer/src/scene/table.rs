//! The persistent table under a scene version (ADR-0304 §1).
//!
//! A trie of fanout 32 over a `u64` key: each level consumes five bits, the
//! leaves hold the values, and every node sits under an [`Arc`]. A write
//! copies the path from the root to its leaf ([`Arc::make_mut`] on each
//! level) and shares every other subtree with the table it was cloned from,
//! so cloning a table is one reference-count increment and a write costs its
//! height in copied nodes. Writes that land in one leaf share that path: the
//! first copies it, the rest find it already unshared.
//!
//! The height is the fewest levels whose capacity holds the largest key
//! written; it grows by one new root per level when a key passes the
//! capacity and never shrinks. A lookup takes `height` steps.

use std::sync::Arc;

/// Bits of the key each level consumes.
const BITS: u32 = 5;
/// Children per node.
const FANOUT: usize = 1 << BITS;
/// The key bits one level indexes by.
const MASK: u64 = (FANOUT as u64) - 1;
/// Levels that index a full `u64` key: `ceil(64 / 5)`.
const MAX_HEIGHT: u32 = u64::BITS.div_ceil(BITS);

/// What writes cost a table: the nodes copied because another version still
/// shared them, and the nodes allocated new.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TableCost {
    /// Nodes cloned out of a shared subtree.
    pub(crate) copies: u64,
    /// Nodes allocated for a path that did not exist, or a new root.
    pub(crate) created: u64,
}

/// A shared node: a branch above the leaf level, a leaf at it.
#[derive(Clone)]
enum Node<T> {
    Branch(Arc<Branch<T>>),
    Leaf(Arc<Leaf<T>>),
}

#[derive(Clone)]
struct Branch<T>([Option<Node<T>>; FANOUT]);

#[derive(Clone)]
struct Leaf<T>([Option<T>; FANOUT]);

impl<T> Node<T> {
    /// An empty node for `level` (0 is the leaf level).
    fn empty(level: u32) -> Self {
        if level == 0 {
            Self::Leaf(Arc::new(Leaf(std::array::from_fn(|_| None))))
        } else {
            Self::Branch(Arc::new(Branch(std::array::from_fn(|_| None))))
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Branch(branch) => branch.0.iter().all(Option::is_none),
            Self::Leaf(leaf) => leaf.0.iter().all(Option::is_none),
        }
    }
}

/// A persistent map from `u64` to `T`, ordered by key.
#[derive(Clone)]
pub(crate) struct Table<T> {
    root: Option<Node<T>>,
    /// Levels under `root`, 1 for a lone leaf; 0 while `root` is `None`.
    height: u32,
    len: usize,
}

impl<T> std::fmt::Debug for Table<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Table")
            .field("len", &self.len)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            root: None,
            height: 0,
            len: 0,
        }
    }
}

/// The fewest levels whose capacity holds `key`.
fn height_for(key: u64) -> u32 {
    key.bit_width().div_ceil(BITS).max(1)
}

/// The child slot `key` takes at `level` (0 is the leaf level).
fn digit(key: u64, level: u32) -> usize {
    // `level < MAX_HEIGHT`, so the shift is at most 60, and the masked value
    // is below FANOUT.
    ((key >> (BITS * level)) & MASK) as usize
}

/// `node` made unshared, counting a clone when another version holds it.
fn unshare<'a, N: Clone>(node: &'a mut Arc<N>, cost: &mut TableCost) -> &'a mut N {
    if Arc::get_mut(node).is_none() {
        cost.copies += 1;
    }
    Arc::make_mut(node)
}

impl<T: Clone> Table<T> {
    /// How many keys hold a value.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Levels a lookup walks.
    #[cfg(test)]
    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// The value at `key`.
    pub(crate) fn get(&self, key: u64) -> Option<&T> {
        if height_for(key) > self.height {
            return None;
        }
        let mut node = self.root.as_ref()?;
        let mut level = self.height - 1;
        loop {
            match node {
                Node::Branch(branch) => {
                    node = branch.0[digit(key, level)].as_ref()?;
                    level = level.checked_sub(1)?;
                }
                Node::Leaf(leaf) => return leaf.0[digit(key, level)].as_ref(),
            }
        }
    }

    /// The value at `key`, inserted as `T::default()` when absent, unshared
    /// for writing in place.
    pub(crate) fn get_or_default(&mut self, key: u64, cost: &mut TableCost) -> &mut T
    where
        T: Default,
    {
        if self.get(key).is_none() {
            self.insert(key, T::default(), cost);
        }
        self.get_mut(key, cost)
            .expect("BUG: a value just inserted is in the table")
    }

    /// The value at `key`, unshared for writing in place.
    pub(crate) fn get_mut(&mut self, key: u64, cost: &mut TableCost) -> Option<&mut T> {
        // Look first, so a miss copies nothing.
        self.get(key)?;
        let mut node = self.root.as_mut()?;
        let mut level = self.height - 1;
        loop {
            match node {
                Node::Branch(branch) => {
                    node = unshare(branch, cost).0[digit(key, level)].as_mut()?;
                    level = level.checked_sub(1)?;
                }
                Node::Leaf(leaf) => return unshare(leaf, cost).0[digit(key, level)].as_mut(),
            }
        }
    }

    /// Stores `value` at `key` and returns the value it replaced.
    pub(crate) fn insert(&mut self, key: u64, value: T, cost: &mut TableCost) -> Option<T> {
        self.grow_to(height_for(key), cost);
        let mut slot = &mut self.root;
        let mut level = self.height - 1;
        let old = loop {
            let node = slot.get_or_insert_with(|| {
                cost.created += 1;
                Node::empty(level)
            });
            match node {
                Node::Branch(branch) => {
                    slot = &mut unshare(branch, cost).0[digit(key, level)];
                    level -= 1;
                }
                Node::Leaf(leaf) => break unshare(leaf, cost).0[digit(key, level)].replace(value),
            }
        };
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    /// Removes the value at `key`, pruning the nodes it leaves empty.
    pub(crate) fn remove(&mut self, key: u64, cost: &mut TableCost) -> Option<T> {
        // Look first, so a miss copies nothing.
        self.get(key)?;
        let root = self.root.as_mut()?;
        let removed = remove_in(root, key, self.height - 1, cost);
        if root.is_empty() {
            self.root = None;
            self.height = 0;
        }
        if removed.is_some() {
            self.len -= 1;
        }
        removed
    }

    /// Every `(key, value)` in key order.
    pub(crate) fn iter(&self) -> Iter<'_, T> {
        let mut stack = Vec::with_capacity(self.height as usize);
        if let Some(root) = self.root.as_ref() {
            stack.push((root, 0usize, 0u64));
        }
        Iter { stack }
    }

    /// Adds levels above the root until the table holds `height`; each is
    /// one new node whose first child is the old root.
    fn grow_to(&mut self, height: u32, cost: &mut TableCost) {
        debug_assert!(
            height <= MAX_HEIGHT,
            "BUG: a u64 key needs at most MAX_HEIGHT levels"
        );
        if self.root.is_none() {
            self.height = height;
            return;
        }
        while self.height < height {
            let mut children: [Option<Node<T>>; FANOUT] = std::array::from_fn(|_| None);
            children[0] = self.root.take();
            cost.created += 1;
            self.root = Some(Node::Branch(Arc::new(Branch(children))));
            self.height += 1;
        }
    }
}

fn remove_in<T: Clone>(
    node: &mut Node<T>,
    key: u64,
    level: u32,
    cost: &mut TableCost,
) -> Option<T> {
    match node {
        Node::Branch(branch) => {
            let slot = &mut unshare(branch, cost).0[digit(key, level)];
            let child = slot.as_mut()?;
            let removed = remove_in(child, key, level.checked_sub(1)?, cost);
            if child.is_empty() {
                *slot = None;
            }
            removed
        }
        Node::Leaf(leaf) => unshare(leaf, cost).0[digit(key, level)].take(),
    }
}

/// The ordered walk of a [`Table`].
pub(crate) struct Iter<'a, T> {
    /// Per level from the root: the node, the next child to visit and the key
    /// prefix the node covers.
    stack: Vec<(&'a Node<T>, usize, u64)>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (u64, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (node, next, prefix) = self.stack.last_mut()?;
            if *next == FANOUT {
                self.stack.pop();
                continue;
            }
            let at = *next;
            *next += 1;
            // `at < FANOUT`, so the widening cast stays in range.
            let key = (*prefix << BITS) | at as u64;
            match *node {
                Node::Branch(branch) => {
                    if let Some(child) = branch.0[at].as_ref() {
                        self.stack.push((child, 0, key));
                    }
                }
                Node::Leaf(leaf) => {
                    if let Some(value) = leaf.0[at].as_ref() {
                        return Some((key, value));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;

    #[derive(Debug, Clone)]
    enum Op {
        Insert(u64, u32),
        Remove(u64),
    }

    fn key() -> impl Strategy<Value = u64> {
        prop_oneof![0..64u64, 0..5_000u64, any::<u64>()]
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (key(), any::<u32>()).prop_map(|(k, v)| Op::Insert(k, v)),
            key().prop_map(Op::Remove),
        ]
    }

    /// Whether both are the same shared node.
    fn same<T>(a: &Node<T>, b: &Node<T>) -> bool {
        match (a, b) {
            (Node::Branch(a), Node::Branch(b)) => Arc::ptr_eq(a, b),
            (Node::Leaf(a), Node::Leaf(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    fn assert_matches(table: &Table<u32>, model: &BTreeMap<u64, u32>) {
        assert_eq!(table.len(), model.len());
        let walked: Vec<(u64, u32)> = table.iter().map(|(k, v)| (k, *v)).collect();
        let expected: Vec<(u64, u32)> = model.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(walked, expected);
        for (k, v) in model {
            assert_eq!(table.get(*k), Some(v));
        }
    }

    proptest! {
        /// Random inserts and removes agree with a `BTreeMap`, and the table
        /// cloned before each write still reads as it did.
        #[test]
        fn the_table_behaves_as_an_ordered_map(ops in prop::collection::vec(op(), 0..200)) {
            let mut table = Table::<u32>::default();
            let mut model = BTreeMap::new();
            let mut cost = TableCost::default();
            for op in ops {
                let before = table.clone();
                let before_model = model.clone();
                match op {
                    Op::Insert(k, v) => {
                        prop_assert_eq!(table.insert(k, v, &mut cost), model.insert(k, v));
                    }
                    Op::Remove(k) => {
                        prop_assert_eq!(table.remove(k, &mut cost), model.remove(&k));
                    }
                }
                assert_matches(&before, &before_model);
                assert_matches(&table, &model);
            }
        }
    }

    /// A write copies the path to its leaf and shares every other subtree
    /// with the table before it.
    #[test]
    fn a_write_shares_every_untouched_subtree() {
        let mut table = Table::<u32>::default();
        let mut cost = TableCost::default();
        for k in 0..2_000u64 {
            table.insert(k, 0, &mut cost);
        }
        let before = table.clone();
        let mut cost = TableCost::default();
        table.insert(5, 1, &mut cost);
        assert_eq!(cost.copies, u64::from(table.height()));

        let (Some(Node::Branch(old)), Some(Node::Branch(new))) =
            (before.root.as_ref(), table.root.as_ref())
        else {
            panic!("2 000 keys need more than one level");
        };
        assert!(!same(
            old.0[0].as_ref().expect("written"),
            new.0[0].as_ref().expect("written")
        ));
        for (old, new) in old.0.iter().zip(new.0.iter()).skip(1) {
            match (old, new) {
                (Some(old), Some(new)) => assert!(same(old, new)),
                (None, None) => {}
                _ => panic!("an untouched subtree changed shape"),
            }
        }
        assert_eq!(before.get(5), Some(&0));
        assert_eq!(table.get(5), Some(&1));
    }

    /// Writes into one leaf copy its path once.
    #[test]
    fn writes_into_one_leaf_share_its_path() {
        let mut table = Table::<u32>::default();
        let mut cost = TableCost::default();
        for k in 0..30_000u64 {
            table.insert(k, 0, &mut cost);
        }
        let _shared = table.clone();
        let mut cost = TableCost::default();
        for k in 64..96u64 {
            table.insert(k, 1, &mut cost);
        }
        assert_eq!(cost.copies, u64::from(table.height()));
    }

    /// Passing the capacity adds one root per level and copies nothing.
    #[test]
    fn growing_past_the_capacity_adds_one_node_per_level() {
        let mut table = Table::<u32>::default();
        let mut cost = TableCost::default();
        for k in 0..1_024u64 {
            table.insert(k, 0, &mut cost);
        }
        assert_eq!(table.height(), 2);
        let shared = table.clone();
        let mut cost = TableCost::default();
        table.insert(1_024, 0, &mut cost);
        assert_eq!(table.height(), 3);
        // The new root, a branch for the new second level and the new leaf.
        assert_eq!(
            cost,
            TableCost {
                copies: 0,
                created: 3
            }
        );
        let Some(Node::Branch(children)) = table.root.as_ref() else {
            panic!("a grown table has a branch root");
        };
        assert!(same(
            children.0[0].as_ref().expect("the old root"),
            shared.root.as_ref().expect("1 024 keys"),
        ));
    }

    /// Removing the last key of a subtree prunes it, and a table emptied by
    /// removal reads as empty.
    #[test]
    fn removal_prunes_empty_nodes() {
        let mut table = Table::<u32>::default();
        let mut cost = TableCost::default();
        table.insert(3, 0, &mut cost);
        table.insert(40_000, 0, &mut cost);
        assert_eq!(table.remove(40_000, &mut cost), Some(0));
        assert_eq!(table.iter().count(), 1);
        assert_eq!(table.remove(3, &mut cost), Some(0));
        assert_eq!(table.len(), 0);
        assert!(table.root.is_none());
        assert_eq!(table.remove(3, &mut cost), None);
    }

    /// The largest key fits.
    #[test]
    fn the_largest_key_round_trips() {
        let mut table = Table::<u32>::default();
        let mut cost = TableCost::default();
        table.insert(u64::MAX, 7, &mut cost);
        table.insert(0, 1, &mut cost);
        assert_eq!(table.height(), MAX_HEIGHT);
        assert_eq!(table.get(u64::MAX), Some(&7));
        let keys: Vec<u64> = table.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec![0, u64::MAX]);
    }
}
