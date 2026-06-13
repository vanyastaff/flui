//! Focused augmented B+-tree over per-item extents.
//!
//! This is the backbone of the [`Virtualizer`](super::Virtualizer): a balanced
//! B-tree whose every node caches a `{ count, total_extent }` summary of its
//! subtree. That summary is what makes the windowing math `O(log n)` in *both*
//! directions and `O(log n)` under structural edits:
//!
//! - **offset → index** ([`ExtentTree::seek_offset`]): descend the tree, at each
//!   internal node skipping whole children whose summed extent lies before the
//!   target offset. `O(log n)`.
//! - **index → offset** ([`ExtentTree::offset_of`]): descend the tree, at each
//!   internal node adding the summed extent of skipped children. `O(log n)`.
//! - **point update** ([`ExtentTree::set`]): descend to the leaf, replace the
//!   item, repair summaries on the way back up. `O(log n)`.
//! - **structural insert/delete** ([`ExtentTree::insert`] / [`ExtentTree::remove`]):
//!   descend to the leaf, splice, then split overflowing / rebalance
//!   underflowing nodes up the spine. `O(log n)` — *not* the `O(n)` index shift a
//!   flat-array Fenwick/BIT would pay. This is the whole reason for a tree.
//!
//! # Why a mutable B-tree (not a generic `SumTree<T, Summary>`)
//!
//! GPUI/Zed's `SumTree` is a fully generic, copy-on-write augmented B+-tree. This
//! is a *focused* version: the item type is fixed ([`ItemExtent`]) and the summary
//! is fixed (`{ count, total_extent }`). Keeping it focused keeps the internals a
//! small, auditable, allocation-light deep module; generality lives at the
//! [`Virtualizer`](super::Virtualizer) public boundary, not in maximal internal
//! genericity. The tree owns its children inline in a `Vec` (a mutable B-tree —
//! the `Vec`'s heap buffer breaks the recursive-type size cycle, so no per-child
//! `Box` is needed), so every operation is plain, safe Rust — there is no
//! `unsafe`, no parent pointers, and balance is guaranteed by construction
//! (split-on-overflow, merge-on-underflow), not by rotations that have to be
//! reasoned about separately.
//!
//! # Agnostic
//!
//! Nothing here names a render, sliver, or protocol type. The tree is pure
//! arithmetic over `usize` indices and `f32` extents.

use super::ItemExtent;

/// Branching factor. Each non-root node holds between `B` and `2 * B` entries;
/// the root holds between `1` and `2 * B`. A leaf's entries are items; an
/// internal node's entries are child subtrees.
///
/// `B = 6` keeps nodes small enough to be cache-friendly while giving a shallow
/// tree (depth `≈ log_6 n`: ~10k items fit in 5 levels, ~1M in 8).
const B: usize = 6;

/// Maximum entries per node before it must split.
const MAX: usize = 2 * B;

/// Minimum entries a non-root node may hold before it must rebalance.
const MIN: usize = B;

/// Cached summary of a subtree: how many items it contains and their total
/// extent. Carried on every node so seeks can skip whole subtrees.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Summary {
    /// Number of leaf items in the subtree.
    count: usize,
    /// Sum of every leaf item's extent in the subtree.
    total_extent: f32,
}

impl Summary {
    const EMPTY: Self = Self {
        count: 0,
        total_extent: 0.0,
    };

    #[inline]
    fn of_item(item: &ItemExtent) -> Self {
        Self {
            count: 1,
            total_extent: item.extent(),
        }
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        Self {
            count: self.count + other.count,
            total_extent: self.total_extent + other.total_extent,
        }
    }
}

/// A B-tree node: either a leaf holding items, or an internal node holding
/// child subtrees plus a parallel array of their cached summaries.
#[derive(Debug, Clone)]
enum Node {
    /// Leaf: the actual items, in index order.
    Leaf { items: Vec<ItemExtent> },
    /// Internal: child subtrees in index order, with `summaries[i]` caching
    /// `children[i]`'s subtree summary. `children.len() == summaries.len()`.
    ///
    /// Children are stored inline in the `Vec` (no `Box`): the `Vec`'s own heap
    /// buffer already breaks the recursive-type size cycle, so an extra
    /// per-child box would only add a pointer indirection.
    Internal {
        children: Vec<Node>,
        summaries: Vec<Summary>,
    },
}

/// What an insert/remove produced for the parent to act on after recursing into
/// a child: nothing, an overflow split (the right half to adopt as a new
/// sibling), or an underflow (the child fell below `MIN` and needs rebalancing).
enum Mutation {
    /// The child handled the edit internally; nothing structural to propagate.
    Done,
    /// The child overflowed and split; adopt `right` as a new sibling immediately
    /// after the recursed child. `right`'s summary is `right_summary`.
    Split { right: Node, right_summary: Summary },
    /// The child fell below `MIN` entries and must be rebalanced by its parent.
    Underflow,
}

impl Node {
    #[inline]
    fn new_leaf() -> Self {
        Node::Leaf { items: Vec::new() }
    }

    /// Number of entries directly in this node (items for a leaf, children for
    /// an internal node).
    #[inline]
    fn len(&self) -> usize {
        match self {
            Node::Leaf { items } => items.len(),
            Node::Internal { children, .. } => children.len(),
        }
    }

    /// Computes this node's subtree summary from scratch.
    fn summary(&self) -> Summary {
        match self {
            Node::Leaf { items } => items
                .iter()
                .fold(Summary::EMPTY, |acc, it| acc.add(Summary::of_item(it))),
            Node::Internal { summaries, .. } => {
                summaries.iter().fold(Summary::EMPTY, |acc, s| acc.add(*s))
            }
        }
    }

    /// Total item count in this subtree.
    #[inline]
    fn count(&self) -> usize {
        match self {
            Node::Leaf { items } => items.len(),
            Node::Internal { summaries, .. } => summaries.iter().map(|s| s.count).sum(),
        }
    }

    // ---- index → offset ---------------------------------------------------

    /// Sum of extents of items in `[0, index)` within this subtree.
    ///
    /// `index` is subtree-local and must satisfy `index <= self.count()`.
    fn offset_of(&self, index: usize) -> f32 {
        match self {
            Node::Leaf { items } => items
                .iter()
                .take(index)
                .map(ItemExtent::extent)
                .sum::<f32>(),
            Node::Internal {
                children,
                summaries,
            } => {
                let mut acc = 0.0;
                let mut remaining = index;
                for (child, summ) in children.iter().zip(summaries) {
                    if remaining >= summ.count {
                        // The whole child is before `index`: add its total.
                        acc += summ.total_extent;
                        remaining -= summ.count;
                    } else {
                        // `index` lands inside this child: recurse for the rest.
                        acc += child.offset_of(remaining);
                        return acc;
                    }
                }
                acc
            }
        }
    }

    // ---- offset → index ---------------------------------------------------

    /// Finds the item containing `offset` within this subtree.
    ///
    /// Returns `(local_index, offset_into_item)` where `local_index` is the
    /// subtree-local index of the item whose half-open extent span
    /// `[start, start + extent)` contains `offset`, and `offset_into_item` is
    /// `offset - start`.
    ///
    /// Boundary rule: an offset exactly at an item's start belongs to that item
    /// (the first item whose *end* is strictly greater than `offset`). `offset`
    /// is clamped to `[0, total]` by the caller; here it is assumed in range.
    fn seek_offset(&self, offset: f32) -> (usize, f32) {
        match self {
            Node::Leaf { items } => {
                let mut acc = 0.0;
                for (i, it) in items.iter().enumerate() {
                    let e = it.extent();
                    // Strictly-greater end means a zero-extent item at exactly
                    // `offset` is skipped in favour of the next real item — the
                    // half-open `[start, end)` containment rule.
                    if acc + e > offset {
                        return (i, offset - acc);
                    }
                    acc += e;
                }
                // `offset` is at or past the end: clamp to the last item.
                let last = items.len() - 1;
                (last, offset - (acc - items[last].extent()))
            }
            Node::Internal {
                children,
                summaries,
            } => {
                let mut acc = 0.0;
                let mut index_base = 0usize;
                let last = children.len() - 1;
                for (i, (child, summ)) in children.iter().zip(summaries).enumerate() {
                    if i == last || acc + summ.total_extent > offset {
                        let (local, into) = child.seek_offset(offset - acc);
                        return (index_base + local, into);
                    }
                    acc += summ.total_extent;
                    index_base += summ.count;
                }
                unreachable!("internal node always has at least one child")
            }
        }
    }

    // ---- point update -----------------------------------------------------

    /// Replaces the item at subtree-local `index`, returning the *old* item so
    /// the caller can compute deltas. Repairs summaries on the way back up.
    fn set(&mut self, index: usize, item: ItemExtent) -> ItemExtent {
        match self {
            Node::Leaf { items } => std::mem::replace(&mut items[index], item),
            Node::Internal {
                children,
                summaries,
            } => {
                let mut remaining = index;
                for (child, summ) in children.iter_mut().zip(summaries.iter_mut()) {
                    if remaining < summ.count {
                        let old = child.set(remaining, item);
                        *summ = child.summary();
                        return old;
                    }
                    remaining -= summ.count;
                }
                unreachable!("index out of range in Node::set")
            }
        }
    }

    /// Returns the item at subtree-local `index`.
    fn get(&self, index: usize) -> &ItemExtent {
        match self {
            Node::Leaf { items } => &items[index],
            Node::Internal {
                children,
                summaries,
            } => {
                let mut remaining = index;
                for (child, summ) in children.iter().zip(summaries) {
                    if remaining < summ.count {
                        return child.get(remaining);
                    }
                    remaining -= summ.count;
                }
                unreachable!("index out of range in Node::get")
            }
        }
    }

    // ---- structural insert ------------------------------------------------

    /// Inserts `item` at subtree-local `index` (`index <= self.count()`).
    ///
    /// Returns a [`Mutation`] telling the parent whether this node split.
    fn insert(&mut self, index: usize, item: ItemExtent) -> Mutation {
        match self {
            Node::Leaf { items } => {
                items.insert(index, item);
                if items.len() > MAX {
                    self.split_leaf()
                } else {
                    Mutation::Done
                }
            }
            Node::Internal { .. } => {
                let child_pos = self.locate_child_for_insert(index);
                let (child_index, local) = child_pos;
                let mutation = {
                    let Node::Internal { children, .. } = self else {
                        unreachable!()
                    };
                    children[child_index].insert(local, item)
                };
                self.apply_child_insert_mutation(child_index, mutation)
            }
        }
    }

    /// For an internal node, picks which child an insert at subtree-local
    /// `index` belongs to, returning `(child_index, index_within_child)`.
    ///
    /// An insert at a child boundary goes to the *left* child's tail (so
    /// appending at `count()` lands in the last child) — except an insert at
    /// index 0 of a non-empty child stays at that child's head.
    fn locate_child_for_insert(&self, index: usize) -> (usize, usize) {
        let Node::Internal { summaries, .. } = self else {
            unreachable!("locate_child_for_insert on a leaf")
        };
        let mut remaining = index;
        let last = summaries.len() - 1;
        for (i, summ) in summaries.iter().enumerate() {
            // `<=` lets an insert at the child's end stay in this child; the
            // `i == last` guard makes append (index == count) land in the tail.
            if i == last || remaining <= summ.count {
                return (i, remaining);
            }
            remaining -= summ.count;
        }
        unreachable!("internal node always has at least one child")
    }

    /// After recursing an insert into `children[child_index]`, fold the child's
    /// reported mutation back into this node (adopt a split sibling, refresh the
    /// cached summary), and report whether *this* node now overflows.
    fn apply_child_insert_mutation(&mut self, child_index: usize, mutation: Mutation) -> Mutation {
        let Node::Internal {
            children,
            summaries,
        } = self
        else {
            unreachable!()
        };
        match mutation {
            Mutation::Done => {
                summaries[child_index] = children[child_index].summary();
                Mutation::Done
            }
            Mutation::Split {
                right,
                right_summary,
            } => {
                summaries[child_index] = children[child_index].summary();
                children.insert(child_index + 1, right);
                summaries.insert(child_index + 1, right_summary);
                if children.len() > MAX {
                    self.split_internal()
                } else {
                    Mutation::Done
                }
            }
            Mutation::Underflow => unreachable!("insert never underflows a child"),
        }
    }

    /// Splits an over-full leaf in half, keeping the left half in `self` and
    /// returning the right half as a new sibling.
    fn split_leaf(&mut self) -> Mutation {
        let Node::Leaf { items } = self else {
            unreachable!("split_leaf on an internal node")
        };
        let mid = items.len() / 2;
        let right_items = items.split_off(mid);
        let right = Node::Leaf { items: right_items };
        let right_summary = right.summary();
        Mutation::Split {
            right,
            right_summary,
        }
    }

    /// Splits an over-full internal node in half.
    fn split_internal(&mut self) -> Mutation {
        let Node::Internal {
            children,
            summaries,
        } = self
        else {
            unreachable!("split_internal on a leaf")
        };
        let mid = children.len() / 2;
        let right_children = children.split_off(mid);
        let right_summaries = summaries.split_off(mid);
        let right = Node::Internal {
            children: right_children,
            summaries: right_summaries,
        };
        let right_summary = right.summary();
        Mutation::Split {
            right,
            right_summary,
        }
    }

    // ---- structural remove ------------------------------------------------

    /// Removes the item at subtree-local `index`, returning it. Reports via
    /// [`Mutation`] whether this node underflowed (`< MIN`) so the parent can
    /// rebalance.
    fn remove(&mut self, index: usize) -> (ItemExtent, Mutation) {
        match self {
            Node::Leaf { items } => {
                let removed = items.remove(index);
                let mutation = if items.len() < MIN {
                    Mutation::Underflow
                } else {
                    Mutation::Done
                };
                (removed, mutation)
            }
            Node::Internal { .. } => {
                let (child_index, local) = self.locate_child_for_remove(index);
                let (removed, child_mutation) = {
                    let Node::Internal { children, .. } = self else {
                        unreachable!()
                    };
                    children[child_index].remove(local)
                };
                let mutation = self.apply_child_remove_mutation(child_index, child_mutation);
                (removed, mutation)
            }
        }
    }

    /// For an internal node, picks which child holds subtree-local `index`,
    /// returning `(child_index, index_within_child)`.
    fn locate_child_for_remove(&self, index: usize) -> (usize, usize) {
        let Node::Internal { summaries, .. } = self else {
            unreachable!("locate_child_for_remove on a leaf")
        };
        let mut remaining = index;
        for (i, summ) in summaries.iter().enumerate() {
            if remaining < summ.count {
                return (i, remaining);
            }
            remaining -= summ.count;
        }
        unreachable!("index out of range in locate_child_for_remove")
    }

    /// After recursing a remove into `children[child_index]`, refresh the cached
    /// summary, rebalance the child if it underflowed, and report whether *this*
    /// node now underflows.
    fn apply_child_remove_mutation(&mut self, child_index: usize, mutation: Mutation) -> Mutation {
        match mutation {
            Mutation::Done => {
                let Node::Internal {
                    children,
                    summaries,
                } = self
                else {
                    unreachable!()
                };
                summaries[child_index] = children[child_index].summary();
                Mutation::Done
            }
            Mutation::Underflow => self.rebalance_child(child_index),
            Mutation::Split { .. } => unreachable!("remove never splits a child"),
        }
    }

    /// Restores the `>= MIN` invariant for `children[child_index]`, which has
    /// just dropped below it, by borrowing one entry from a sibling or merging
    /// with one. Refreshes affected summaries. Returns whether *this* node
    /// underflowed as a result (it can only do so via a merge, which removes one
    /// of its children).
    fn rebalance_child(&mut self, child_index: usize) -> Mutation {
        let Node::Internal {
            children,
            summaries,
        } = self
        else {
            unreachable!("rebalance_child on a leaf")
        };

        // Prefer borrowing from the left sibling, then the right; fall back to a
        // merge. A borrow keeps both siblings `>= MIN`; a merge collapses two
        // children into one and may underflow this node.
        let has_left = child_index > 0;
        let has_right = child_index + 1 < children.len();

        if has_left && children[child_index - 1].len() > MIN {
            Self::borrow_from_left(children, summaries, child_index);
            Mutation::Done
        } else if has_right && children[child_index + 1].len() > MIN {
            Self::borrow_from_right(children, summaries, child_index);
            Mutation::Done
        } else if has_left {
            // Merge the underflowed child into its left sibling.
            Self::merge(children, summaries, child_index - 1);
            Self::underflow_or_done(children)
        } else if has_right {
            // Merge the right sibling into the underflowed child.
            Self::merge(children, summaries, child_index);
            Self::underflow_or_done(children)
        } else {
            // No sibling: this node has a single child. The empty-leaf / lone-
            // child collapse is handled at the tree root in `ExtentTree`.
            Mutation::Done
        }
    }

    /// Reports whether this internal node (post-merge) is below `MIN` children.
    fn underflow_or_done(children: &[Node]) -> Mutation {
        if children.len() < MIN {
            Mutation::Underflow
        } else {
            Mutation::Done
        }
    }

    /// Moves the last entry of the left sibling to the front of
    /// `children[child_index]`. Both are leaves or both internal.
    fn borrow_from_left(children: &mut [Node], summaries: &mut [Summary], child_index: usize) {
        let left_index = child_index - 1;
        // Pop the donated entry out of the left sibling first.
        match &mut children[left_index] {
            Node::Leaf { items } => {
                let donated = items.pop().expect("left sibling above MIN is non-empty");
                let Node::Leaf { items: dst } = &mut children[child_index] else {
                    unreachable!("sibling node kinds must match")
                };
                dst.insert(0, donated);
            }
            Node::Internal {
                children: lc,
                summaries: ls,
            } => {
                let donated_child = lc.pop().expect("left sibling above MIN is non-empty");
                let donated_summary = ls.pop().expect("parallel arrays stay in lockstep");
                let Node::Internal {
                    children: dc,
                    summaries: ds,
                } = &mut children[child_index]
                else {
                    unreachable!("sibling node kinds must match")
                };
                dc.insert(0, donated_child);
                ds.insert(0, donated_summary);
            }
        }
        summaries[left_index] = children[left_index].summary();
        summaries[child_index] = children[child_index].summary();
    }

    /// Moves the first entry of the right sibling to the back of
    /// `children[child_index]`. Both are leaves or both internal.
    fn borrow_from_right(children: &mut [Node], summaries: &mut [Summary], child_index: usize) {
        let right_index = child_index + 1;
        match &mut children[right_index] {
            Node::Leaf { items } => {
                let donated = items.remove(0);
                let Node::Leaf { items: dst } = &mut children[child_index] else {
                    unreachable!("sibling node kinds must match")
                };
                dst.push(donated);
            }
            Node::Internal {
                children: rc,
                summaries: rs,
            } => {
                let donated_child = rc.remove(0);
                let donated_summary = rs.remove(0);
                let Node::Internal {
                    children: dc,
                    summaries: ds,
                } = &mut children[child_index]
                else {
                    unreachable!("sibling node kinds must match")
                };
                dc.push(donated_child);
                ds.push(donated_summary);
            }
        }
        summaries[child_index] = children[child_index].summary();
        summaries[right_index] = children[right_index].summary();
    }

    /// Merges `children[left_index + 1]` into `children[left_index]`, removing
    /// the right child and its summary slot. Both are leaves or both internal.
    fn merge(children: &mut Vec<Node>, summaries: &mut Vec<Summary>, left_index: usize) {
        let right = children.remove(left_index + 1);
        summaries.remove(left_index + 1);
        match (&mut children[left_index], right) {
            (Node::Leaf { items: left }, Node::Leaf { items: mut right }) => {
                left.append(&mut right);
            }
            (
                Node::Internal {
                    children: lc,
                    summaries: ls,
                },
                Node::Internal {
                    children: mut rc,
                    summaries: mut rs,
                },
            ) => {
                lc.append(&mut rc);
                ls.append(&mut rs);
            }
            _ => unreachable!("merged node kinds must match"),
        }
        summaries[left_index] = children[left_index].summary();
    }

    // ---- debug / invariant helpers ----------------------------------------

    /// Depth of this subtree (a lone leaf has depth 1).
    #[cfg(test)]
    fn depth(&self) -> usize {
        match self {
            Node::Leaf { .. } => 1,
            Node::Internal { children, .. } => 1 + children[0].depth(),
        }
    }

    /// Recursively checks structural invariants. `is_root` relaxes the lower
    /// bound (the root may hold fewer than `MIN` entries). Returns `Err` with a
    /// human-readable reason on the first violation.
    #[cfg(test)]
    fn check_invariants(&self, is_root: bool) -> Result<(), String> {
        let len = self.len();
        if len > MAX {
            return Err(format!("node over MAX: len={len} MAX={MAX}"));
        }
        if !is_root && len < MIN {
            return Err(format!("non-root node under MIN: len={len} MIN={MIN}"));
        }
        if is_root && len == 0 {
            // An empty root is only legal as a single empty leaf.
            if !matches!(self, Node::Leaf { .. }) {
                return Err("empty root must be a leaf".to_string());
            }
        }
        if let Node::Internal {
            children,
            summaries,
        } = self
        {
            if children.len() != summaries.len() {
                return Err("children/summaries length mismatch".to_string());
            }
            if children.is_empty() {
                return Err("internal node with no children".to_string());
            }
            for (child, cached) in children.iter().zip(summaries) {
                let actual = child.summary();
                if actual.count != cached.count {
                    return Err(format!(
                        "cached count {} != actual {}",
                        cached.count, actual.count
                    ));
                }
                if (actual.total_extent - cached.total_extent).abs() > 1e-3 {
                    return Err(format!(
                        "cached extent {} != actual {}",
                        cached.total_extent, actual.total_extent
                    ));
                }
                child.check_invariants(false)?;
            }
        }
        Ok(())
    }
}

/// Splits `n` entries into consecutive chunk sizes for one bulk-load level,
/// where every chunk is in `[MIN, MAX]` — except when there is a single chunk
/// (`n <= MAX`), which may be smaller and becomes the relaxed root.
///
/// All but possibly the last chunk are `MAX`. If the final remainder `r` is a
/// nonzero amount below `MIN`, the last full `MAX` chunk and the remainder are
/// re-split into two halves: their combined size is `MAX + r`, which is at least
/// `MAX + 1 = 2·MIN + 1`, so each half is in `[MIN, MAX]`. This is what keeps
/// `from_fn` from emitting an under-`MIN` tail node (the bug a naive
/// `chunks(MAX)` would create when `n % MAX` is small but nonzero).
fn balanced_chunk_sizes(n: usize) -> Vec<usize> {
    if n <= MAX {
        return vec![n];
    }
    let mut sizes = Vec::with_capacity(n.div_ceil(MAX));
    let mut remaining = n;
    while remaining > MAX {
        // Hold back `MIN` so the loop never strands a sub-`MIN` final chunk: the
        // last `MAX`-chunk plus the tail are split evenly once `remaining` is in
        // `(MAX, MAX + MIN]`.
        if remaining <= MAX + MIN {
            let left = remaining / 2;
            sizes.push(left);
            sizes.push(remaining - left);
            return sizes;
        }
        sizes.push(MAX);
        remaining -= MAX;
    }
    // `remaining` is in `[MIN, MAX]` here (the `<= MAX + MIN` branch handled the
    // small-tail case), so it is a legal final chunk.
    sizes.push(remaining);
    sizes
}

/// A balanced, augmented B+-tree over per-item extents.
///
/// Owns a single root [`Node`]. All public operations are `O(log n)` (worst
/// case as well as average — the tree is balanced by construction, so there is
/// no degenerate-shape worst case the way an unbalanced BST would have).
///
/// The tree is the deleted `FenwickExtents`' replacement: where a flat-array
/// Fenwick/BIT paid `O(n)` to insert or delete mid-list (every later index
/// shifts), this pays `O(log n)`.
#[derive(Debug, Clone)]
pub(super) struct ExtentTree {
    root: Node,
}

impl ExtentTree {
    /// Builds a tree from `count` items produced by `make`, each at `index`.
    ///
    /// Bulk-loads leaves bottom-up, so construction is `O(count)` rather than
    /// `O(count log count)` repeated inserts. Every node it builds is in
    /// `[MIN, MAX]` entries (except a sole root, which may be smaller) — see
    /// [`balanced_chunk_sizes`].
    pub(super) fn from_fn(count: usize, mut make: impl FnMut(usize) -> ItemExtent) -> Self {
        if count == 0 {
            return Self {
                root: Node::new_leaf(),
            };
        }

        // Leaf level: split items into legal-sized chunks (never below MIN
        // unless it is the only chunk), each a leaf.
        let mut items = (0..count).map(&mut make);
        let mut level: Vec<Node> = balanced_chunk_sizes(count)
            .into_iter()
            .map(|size| {
                let chunk: Vec<ItemExtent> = items.by_ref().take(size).collect();
                Node::Leaf { items: chunk }
            })
            .collect();

        // Group each level into internal parents until a single root remains,
        // using the same legal-chunking so internal nodes are never below MIN.
        while level.len() > 1 {
            let mut nodes = level.into_iter();
            level = balanced_chunk_sizes(nodes.len())
                .into_iter()
                .map(|size| {
                    let children: Vec<Node> = nodes.by_ref().take(size).collect();
                    let summaries: Vec<Summary> = children.iter().map(Node::summary).collect();
                    Node::Internal {
                        children,
                        summaries,
                    }
                })
                .collect();
        }

        Self {
            root: level
                .pop()
                .expect("count > 0 always yields at least one leaf node"),
        }
    }

    /// Number of items in the tree.
    #[inline]
    pub(super) fn len(&self) -> usize {
        self.root.count()
    }

    /// Total extent of all items.
    #[inline]
    pub(super) fn total_extent(&self) -> f32 {
        self.root.summary().total_extent
    }

    /// Returns the item at `index`.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    #[inline]
    pub(super) fn get(&self, index: usize) -> &ItemExtent {
        debug_assert!(index < self.len(), "get index out of range");
        self.root.get(index)
    }

    /// Sum of extents of items in `[0, index)`. `offset_of(0) == 0.0`,
    /// `offset_of(len()) == total_extent()`.
    ///
    /// # Panics
    /// Panics if `index > len()`.
    #[inline]
    pub(super) fn offset_of(&self, index: usize) -> f32 {
        debug_assert!(index <= self.len(), "offset_of index out of range");
        self.root.offset_of(index)
    }

    /// Maps `offset` to `(index, offset_into_item)`. `offset` is clamped to
    /// `[0, total_extent()]`. Returns `(0, 0.0)` for an empty tree.
    pub(super) fn seek_offset(&self, offset: f32) -> (usize, f32) {
        let count = self.len();
        if count == 0 {
            return (0, 0.0);
        }
        let total = self.total_extent();
        if offset <= 0.0 {
            return (0, 0.0);
        }
        if offset >= total {
            // At or past the end: the last item, with the overflow folded into
            // `offset_into_item` (matches the leaf clamp).
            let last = count - 1;
            return (last, offset - self.offset_of(last));
        }
        self.root.seek_offset(offset)
    }

    /// Replaces the item at `index`, returning the previous value.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub(super) fn set(&mut self, index: usize, item: ItemExtent) -> ItemExtent {
        assert!(index < self.len(), "set index out of range");
        self.root.set(index, item)
    }

    /// Inserts `item` so it becomes the new item at `index`, shifting later
    /// items up by one. `index == len()` appends.
    ///
    /// # Panics
    /// Panics if `index > len()`.
    pub(super) fn insert(&mut self, index: usize, item: ItemExtent) {
        assert!(index <= self.len(), "insert index out of range");
        let mutation = self.root.insert(index, item);
        if let Mutation::Split {
            right,
            right_summary,
        } = mutation
        {
            // Root split: grow a new level.
            let old_root = std::mem::replace(&mut self.root, Node::new_leaf());
            let left_summary = old_root.summary();
            self.root = Node::Internal {
                children: vec![old_root, right],
                summaries: vec![left_summary, right_summary],
            };
        }
    }

    /// Removes and returns the item at `index`, shifting later items down by one.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub(super) fn remove(&mut self, index: usize) -> ItemExtent {
        assert!(index < self.len(), "remove index out of range");
        let (removed, _) = self.root.remove(index);
        self.shrink_root_if_needed();
        removed
    }

    /// Collapses a lone-child internal root into that child (the only place the
    /// tree height shrinks). Keeps the root a leaf when empty.
    fn shrink_root_if_needed(&mut self) {
        while let Node::Internal { children, .. } = &mut self.root {
            if children.len() == 1 {
                let only = children.pop().expect("len()==1 has exactly one child");
                self.root = only;
            } else {
                break;
            }
        }
    }

    /// Depth of the tree (a fresh empty tree has depth 1).
    #[cfg(test)]
    pub(super) fn depth(&self) -> usize {
        self.root.depth()
    }

    /// Checks every structural invariant (balance, summary correctness, the
    /// `MIN`/`MAX` bounds). Used by property tests.
    #[cfg(test)]
    pub(super) fn check_invariants(&self) -> Result<(), String> {
        self.root.check_invariants(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(e: f32) -> ItemExtent {
        ItemExtent::Measured { extent: e }
    }

    fn build(extents: &[f32]) -> ExtentTree {
        ExtentTree::from_fn(extents.len(), |i| measured(extents[i]))
    }

    #[test]
    fn empty_tree() {
        let t = ExtentTree::from_fn(0, |_| measured(0.0));
        assert_eq!(t.len(), 0);
        assert_eq!(t.total_extent(), 0.0);
        assert_eq!(t.seek_offset(10.0), (0, 0.0));
        assert_eq!(t.offset_of(0), 0.0);
        assert_eq!(t.depth(), 1);
        t.check_invariants().unwrap();
    }

    #[test]
    fn single_item() {
        let t = build(&[42.0]);
        assert_eq!(t.len(), 1);
        assert_eq!(t.total_extent(), 42.0);
        assert_eq!(t.offset_of(0), 0.0);
        assert_eq!(t.offset_of(1), 42.0);
        assert_eq!(t.seek_offset(0.0), (0, 0.0));
        assert_eq!(t.seek_offset(21.0), (0, 21.0));
        assert_eq!(t.seek_offset(42.0), (0, 42.0));
        t.check_invariants().unwrap();
    }

    #[test]
    fn uniform_offsets_and_seeks() {
        let t = build(&[10.0; 4]);
        assert_eq!(t.total_extent(), 40.0);
        for i in 0..=4 {
            assert_eq!(t.offset_of(i), (i as f32) * 10.0);
        }
        assert_eq!(t.seek_offset(0.0), (0, 0.0));
        assert_eq!(t.seek_offset(5.0), (0, 5.0));
        assert_eq!(t.seek_offset(10.0), (1, 0.0));
        assert_eq!(t.seek_offset(15.0), (1, 5.0));
        assert_eq!(t.seek_offset(25.0), (2, 5.0));
    }

    #[test]
    fn variable_offsets_and_seeks() {
        let t = build(&[20.0, 30.0, 10.0, 40.0]);
        assert_eq!(t.total_extent(), 100.0);
        assert_eq!(t.offset_of(1), 20.0);
        assert_eq!(t.offset_of(2), 50.0);
        assert_eq!(t.offset_of(3), 60.0);
        assert_eq!(t.seek_offset(25.0), (1, 5.0));
        assert_eq!(t.seek_offset(55.0), (2, 5.0));
        assert_eq!(t.seek_offset(70.0), (3, 10.0));
    }

    #[test]
    fn point_update_repairs_sums() {
        let mut t = build(&[10.0, 10.0, 10.0]);
        let old = t.set(1, measured(20.0));
        assert_eq!(old.extent(), 10.0);
        assert_eq!(t.total_extent(), 40.0);
        assert_eq!(t.offset_of(2), 30.0);
        assert_eq!(t.offset_of(3), 40.0);
    }

    #[test]
    fn grows_balanced_under_sequential_insert() {
        // Enough to force several splits and at least 3 levels.
        let mut t = ExtentTree::from_fn(0, |_| measured(0.0));
        let n = 500usize;
        for i in 0..n {
            t.insert(i, measured((i % 5 + 1) as f32));
            t.check_invariants()
                .unwrap_or_else(|e| panic!("invariant broke after insert {i}: {e}"));
        }
        assert_eq!(t.len(), n);
        // log_6(500) ≈ 3.5; a balanced tree must be shallow.
        assert!(t.depth() <= 5, "depth {} too deep for {n} items", t.depth());
        // Prefix sums must match a naive scan.
        let expected: f32 = (0..n).map(|i| (i % 5 + 1) as f32).sum();
        assert!((t.total_extent() - expected).abs() < 1e-2);
    }

    #[test]
    fn mid_list_insert_preserves_order() {
        let mut t = build(&[1.0, 2.0, 4.0, 5.0]);
        t.insert(2, measured(3.0)); // -> 1,2,3,4,5
        assert_eq!(t.len(), 5);
        for (i, &e) in [1.0, 2.0, 3.0, 4.0, 5.0].iter().enumerate() {
            assert_eq!(t.get(i).extent(), e, "item {i}");
        }
        assert_eq!(t.offset_of(3), 6.0); // 1+2+3
        t.check_invariants().unwrap();
    }

    #[test]
    fn mid_list_remove_preserves_order_and_rebalances() {
        let mut t = ExtentTree::from_fn(200, |i| measured((i % 4 + 1) as f32));
        // Remove from the middle repeatedly; invariants must hold each time.
        for _ in 0..150 {
            let mid = t.len() / 2;
            t.remove(mid);
            t.check_invariants().unwrap();
        }
        assert_eq!(t.len(), 50);
    }

    #[test]
    fn remove_down_to_empty() {
        let mut t = ExtentTree::from_fn(40, |i| measured((i + 1) as f32));
        while t.len() > 0 {
            t.remove(0);
            t.check_invariants().unwrap();
        }
        assert_eq!(t.len(), 0);
        assert_eq!(t.depth(), 1);
        assert_eq!(t.total_extent(), 0.0);
    }

    #[test]
    fn zero_extent_items_seek_to_next_real_item() {
        // [10, 0, 0, 20]: offset 10 should land on the first item whose span
        // actually contains it — item 3 (items 1,2 are collapsed at offset 10).
        let t = build(&[10.0, 0.0, 0.0, 20.0]);
        assert_eq!(t.total_extent(), 30.0);
        assert_eq!(t.seek_offset(5.0), (0, 5.0));
        assert_eq!(t.seek_offset(10.0), (3, 0.0));
        assert_eq!(t.seek_offset(15.0), (3, 5.0));
    }
}
