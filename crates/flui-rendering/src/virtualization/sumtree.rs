//! Focused augmented B+-tree over per-item extents.
//!
//! This is the backbone of the [`Virtualizer`](super::Virtualizer): a balanced
//! B-tree whose every node caches a `{ count, total_extent }` summary of its
//! subtree. That summary is what makes the windowing math `O(log n)` in *both*
//! directions and `O(log n)` under structural edits:
//!
//! - **offset → index** (`ExtentTree::seek_offset`): descend the tree, at each
//!   internal node skipping whole children whose summed extent lies before the
//!   target offset. `O(log n)`.
//! - **index → offset** ([`ExtentTree::offset_of`]): descend the tree, at each
//!   internal node adding the summed extent of skipped children. `O(log n)`.
//! - **point update** ([`ExtentTree::set`]): descend to the leaf, split or merge
//!   the run it lands in, repair summaries on the way back up, and split or
//!   rebalance the leaf if that changed its entry count. `O(log n)`.
//! - **bulk reshape** ([`ExtentTree::resize`], [`ExtentTree::invalidate_from`],
//!   [`ExtentTree::rehint_unmeasured`]): walk the runs, transform, rebuild.
//!   `O(runs)` — which is `O(measured)`, because unmeasured items collapse.
//!   These replace the per-item loops the virtualizer used to run, and they are
//!   what makes an unbounded list expressible: growing to, or clamping back
//!   from, `usize::MAX` touches one run rather than `usize::MAX` items.
//!
//! Item-wise structural insert/delete still exists but is `#[cfg(test)]`: it is
//! the exerciser for the split/merge/borrow machinery the operations above rely
//! on, driven by the property test, not a production path.
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
//! arithmetic over `usize` indices and `f64` extents.

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
    total_extent: f64,
}

impl Summary {
    const EMPTY: Self = Self {
        count: 0,
        total_extent: 0.0,
    };

    #[inline]
    fn of_run(run: &Run) -> Self {
        Self {
            count: run.count,
            total_extent: run.total(),
        }
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        Self {
            // Saturating: the tree can hold `usize::MAX` items in one run, and
            // an unbounded list plus anything else must stay at the sentinel
            // rather than wrap to a small count.
            count: self.count.saturating_add(other.count),
            total_extent: finite_or_max(self.total_extent + other.total_extent),
        }
    }
}

/// Clamps a non-finite extent sum to `f64::MAX`.
///
/// An unbounded list's total leaves the range `f64` can name. Infinity there
/// propagates into `max_scroll_extent` and poisons every scroll clamp
/// downstream, so it is pinned to the largest nameable extent instead — still
/// far beyond any reachable scroll position, but arithmetically ordinary.
#[inline]
fn finite_or_max(value: f64) -> f64 {
    if value.is_finite() { value } else { f64::MAX }
}

/// A run of consecutive items sharing one extent — the leaf's unit of storage.
///
/// Unmeasured items are always created in large identical batches: the whole
/// list at construction, the tail after a growth, everything after an
/// invalidation. Collapsing them costs one entry instead of `count`. Measured
/// items are individual, but a lazy sliver only ever measures the band it lays
/// out, so the tree is `O(measured)` rather than `O(item_count)`.
///
/// That is what makes ADR-0053's unbounded `usize::MAX` sentinel representable
/// at all: an endless list is a single entry, not an allocation that overflows.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Run {
    /// How many consecutive items this run covers. Always `>= 1`.
    count: usize,
    /// The extent every item in the run carries.
    extent: ItemExtent,
}

impl Run {
    /// The run's total pixel extent, saturating rather than overflowing.
    ///
    /// No reachable offset loses precision to the saturation: `offset_of`
    /// computes a prefix from within the run it lands in and never adds a
    /// saturated total (see [`Node::offset_of`]).
    #[inline]
    fn total(&self) -> f64 {
        finite_or_max(self.count as f64 * self.extent.extent())
    }
}

/// Appends `run` to `runs`, merging it into the previous entry when the two
/// carry the same extent.
///
/// Coalescing is what keeps the run count bounded: without it, re-hinting or
/// invalidating would leave one entry per original item and the whole point of
/// the representation would be lost on the second edit.
fn push_coalesced(runs: &mut Vec<Run>, run: Run) {
    if run.count == 0 {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.extent == run.extent => {
            last.count = last.count.saturating_add(run.count);
        }
        _ => runs.push(run),
    }
}

/// A B-tree node: either a leaf holding runs of items, or an internal node
/// holding child subtrees plus a parallel array of their cached summaries.
#[derive(Debug, Clone)]
enum Node {
    /// Leaf: runs of items, in index order. Adjacent runs always differ in
    /// extent (see [`push_coalesced`]).
    Leaf { runs: Vec<Run> },
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
        Node::Leaf { runs: Vec::new() }
    }

    /// Number of **entries** directly in this node — runs for a leaf, children
    /// for an internal node.
    ///
    /// This is what the `MIN`/`MAX` balance invariants are stated over, and it
    /// is deliberately not the item count: one run may cover `usize::MAX`
    /// items. [`Self::count`] is the item count.
    #[inline]
    fn len(&self) -> usize {
        match self {
            Node::Leaf { runs } => runs.len(),
            Node::Internal { children, .. } => children.len(),
        }
    }

    /// Computes this node's subtree summary from scratch.
    fn summary(&self) -> Summary {
        match self {
            Node::Leaf { runs } => runs
                .iter()
                .fold(Summary::EMPTY, |acc, r| acc.add(Summary::of_run(r))),
            Node::Internal { summaries, .. } => {
                summaries.iter().fold(Summary::EMPTY, |acc, s| acc.add(*s))
            }
        }
    }

    /// Total item count in this subtree.
    #[inline]
    fn count(&self) -> usize {
        match self {
            Node::Leaf { runs } => runs
                .iter()
                .fold(0usize, |acc, r| acc.saturating_add(r.count)),
            Node::Internal { summaries, .. } => summaries
                .iter()
                .fold(0usize, |acc, s| acc.saturating_add(s.count)),
        }
    }

    // ---- index → offset ---------------------------------------------------

    /// Sum of extents of items in `[0, index)` within this subtree.
    ///
    /// `index` is subtree-local and must satisfy `index <= self.count()`.
    fn offset_of(&self, index: usize) -> f64 {
        match self {
            Node::Leaf { runs } => {
                // Whole runs before `index` contribute their total; the run
                // `index` lands inside contributes only the prefix, computed
                // as `consumed * extent` directly. That direct product is why
                // a saturating run total never costs a reachable offset any
                // precision — the saturated value is only ever added for runs
                // entirely before `index`, which for an unbounded tail cannot
                // happen (nothing follows it).
                let mut acc = 0.0_f64;
                let mut remaining = index;
                for run in runs {
                    if remaining >= run.count {
                        // Saturating, exactly as `Summary::add` is: several
                        // runs can each reach `f64::MAX`, and an unchecked sum
                        // would reach infinity while the cached total stays
                        // finite. A prefix that disagrees with the total leaks
                        // straight into item placement and scroll bounds.
                        acc = finite_or_max(acc + run.total());
                        remaining -= run.count;
                    } else {
                        return finite_or_max(acc + remaining as f64 * run.extent.extent());
                    }
                }
                acc
            }
            Node::Internal {
                children,
                summaries,
            } => {
                let mut acc = 0.0;
                let mut remaining = index;
                for (child, summ) in children.iter().zip(summaries) {
                    if remaining >= summ.count {
                        // The whole child is before `index`: add its total.
                        acc = finite_or_max(acc + summ.total_extent);
                        remaining -= summ.count;
                    } else {
                        // `index` lands inside this child: recurse for the rest.
                        return finite_or_max(acc + child.offset_of(remaining));
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
    ///
    /// Scalar reference for the batched [`seek_sorted`](Self::seek_sorted), kept
    /// test-only (the production windowing path uses `seek_sorted`).
    #[cfg(test)]
    fn seek_offset(&self, offset: f64) -> (usize, f64) {
        match self {
            Node::Leaf { runs } => {
                let mut acc = 0.0_f64;
                let mut base = 0usize;
                for run in runs {
                    let e = run.extent.extent();
                    let run_total = run.total();
                    // Strictly-greater end means a zero-extent run at exactly
                    // `offset` is skipped in favour of the next real item — the
                    // half-open `[start, end)` containment rule, applied to the
                    // whole run at once (every item in it has the same extent,
                    // so a zero-extent run contains no offset).
                    if acc + run_total > offset {
                        let into_run = offset - acc;
                        // `e > 0.0` here: a zero-extent run has `run_total ==
                        // 0.0` and cannot satisfy the test above.
                        let local = ((into_run / e) as usize).min(run.count - 1);
                        return (base + local, into_run - local as f64 * e);
                    }
                    acc = finite_or_max(acc + run_total);
                    base += run.count;
                }
                // `offset` is at or past the end: clamp to the last item.
                let last_run = runs.last().expect("leaf reached by seek is non-empty");
                let last = base - 1;
                (last, offset - (acc - last_run.extent.extent()))
            }
            Node::Internal {
                children,
                summaries,
            } => {
                // Scan every child but the last, descending into the first whose
                // running extent reaches past `offset`. The last child is the
                // unconditional fallback: descending it when no earlier child
                // matched is what absorbs f64 round-off at the final boundary, so
                // the loop needs no per-iteration `is-last` test and the tail is
                // a real descent, not an `unreachable!`. `take(len-1)` and
                // `last()` keep both accesses bounds-check-free (slice indexing
                // would reintroduce a check the `zip` elides).
                let split = children.len() - 1;
                let mut acc = 0.0;
                let mut index_base = 0usize;
                for (child, summ) in children.iter().zip(summaries).take(split) {
                    if acc + summ.total_extent > offset {
                        let (local, into) = child.seek_offset(offset - acc);
                        return (index_base + local, into);
                    }
                    acc = finite_or_max(acc + summ.total_extent);
                    index_base += summ.count;
                }
                let last = children.last().expect("internal node has >= 1 child");
                let (local, into) = last.seek_offset(offset - acc);
                (index_base + local, into)
            }
        }
    }

    // ---- batched offset → index (one shared descent) ----------------------

    /// Seeks a batch of **ascending-sorted** `offsets` in a single descent that
    /// shares its prefix across them, writing `(global_index, offset_into_item)`
    /// to `out[k]` for `offsets[k]`. Equivalent to calling `seek_offset` on
    /// each offset individually, but when the offsets cluster (the common
    /// windowing case: visible + cache band edges) they fall into the same
    /// children and the root-to-leaf prefix is walked once, not once per offset.
    ///
    /// `base_index` / `base_offset` are this subtree's global index/offset origin
    /// (an offset routed here is `>= base_offset`). Every offset must lie within
    /// this subtree's span `[base_offset, base_offset + subtree_extent)` — the
    /// caller's partition guarantees it, so no per-offset clamping happens here
    /// (the `<= 0` / `>= total` clamps live in [`ExtentTree::seek_sorted`]).
    ///
    /// Complexity: `O(log n + k)` when the `k` offsets share a descent (clustered),
    /// degrading to `O(k · log n)` only if every offset lands in a distinct
    /// subtree. `seek_offset` (`ExtentTree::seek_offset`) docs.
    fn seek_sorted(
        &self,
        offsets: &[f64],
        base_index: usize,
        base_offset: f64,
        out: &mut [(usize, f64)],
    ) {
        debug_assert_eq!(offsets.len(), out.len());
        if offsets.is_empty() {
            return;
        }
        match self {
            Node::Leaf { runs } => {
                // One forward scan shared across all offsets (they are sorted,
                // so the run cursor only advances). Within the landing run the
                // item is found by division, not iteration — a run may cover
                // `usize::MAX` items. `last`-clamp mirrors the single-offset
                // leaf rule, though middle offsets never reach it.
                let last_run = runs.len() - 1;
                let mut acc = base_offset;
                let mut r = 0usize;
                let mut base = base_index;
                for (slot, &off) in out.iter_mut().zip(offsets) {
                    while r < last_run && acc + runs[r].total() <= off {
                        acc = finite_or_max(acc + runs[r].total());
                        base = base.saturating_add(runs[r].count);
                        r += 1;
                    }
                    let run = &runs[r];
                    let e = run.extent.extent();
                    let into_run = off - acc;
                    let local = if e > 0.0 {
                        ((into_run / e) as usize).min(run.count - 1)
                    } else {
                        // A zero-extent run spans no pixels; every offset in it
                        // resolves to its first item, matching the scalar rule.
                        0
                    };
                    *slot = (base + local, into_run - local as f64 * e);
                }
            }
            Node::Internal {
                children,
                summaries,
            } => {
                // Walk children once; each child claims the contiguous run of
                // offsets that fall in its span, and is recursed into a single
                // time with that run. The last child is the unconditional sink
                // for the tail (same float-rounding guard as `seek_offset`).
                let split = children.len() - 1;
                let mut acc = base_offset;
                let mut idx = base_index;
                let mut start = 0usize;
                for (child, summ) in children.iter().zip(summaries).take(split) {
                    if start == offsets.len() {
                        return;
                    }
                    let child_end = finite_or_max(acc + summ.total_extent);
                    let mut run = start;
                    while run < offsets.len() && offsets[run] < child_end {
                        run += 1;
                    }
                    if run > start {
                        child.seek_sorted(&offsets[start..run], idx, acc, &mut out[start..run]);
                        start = run;
                    }
                    acc = child_end;
                    idx += summ.count;
                }
                if start < offsets.len() {
                    let last = children.last().expect("internal node has >= 1 child");
                    last.seek_sorted(&offsets[start..], idx, acc, &mut out[start..]);
                }
            }
        }
    }

    // ---- point update -----------------------------------------------------

    /// Replaces the item at subtree-local `index`, returning the *old* item so
    /// the caller can compute deltas. Repairs summaries on the way back up.
    ///
    /// Unlike a flat-item leaf, this can grow the node: writing into the middle
    /// of a run splits it into up to three (before / the new item / after), so
    /// a leaf can overflow `MAX` and must report a [`Mutation`] the way
    /// the structural insert path does.
    fn set(&mut self, index: usize, item: ItemExtent) -> (ItemExtent, Mutation) {
        match self {
            Node::Leaf { runs } => {
                let mut before = 0usize;
                let mut target = None;
                for (i, run) in runs.iter().enumerate() {
                    if index - before < run.count {
                        target = Some((i, index - before, run.extent, run.count));
                        break;
                    }
                    before += run.count;
                }
                let (slot, local, old, run_count) =
                    target.expect("BUG: Node::set index is bounded by the caller's assert");
                if old == item {
                    // Nothing to do, and splitting here would create two
                    // adjacent equal runs — the invariant coalescing exists to
                    // prevent.
                    return (old, Mutation::Done);
                }
                // Rebuild through `push_coalesced` so the write merges with
                // whichever neighbours now match it, in one pass. Bounded by
                // `MAX` entries, so this stays O(1) in the item count.
                let mut rebuilt = Vec::with_capacity(runs.len() + 2);
                for (i, run) in runs.iter().enumerate() {
                    if i != slot {
                        push_coalesced(&mut rebuilt, *run);
                        continue;
                    }
                    push_coalesced(
                        &mut rebuilt,
                        Run {
                            count: local,
                            extent: old,
                        },
                    );
                    push_coalesced(
                        &mut rebuilt,
                        Run {
                            count: 1,
                            extent: item,
                        },
                    );
                    push_coalesced(
                        &mut rebuilt,
                        Run {
                            count: run_count - local - 1,
                            extent: old,
                        },
                    );
                }
                *runs = rebuilt;
                // A point update moves entries in BOTH directions, which a
                // flat-item leaf never did: writing a value that matches one
                // neighbour merges two runs into one, and matching both merges
                // three into one. So this reports underflow as well as
                // overflow, and the parent repairs either.
                let mutation = if runs.len() > MAX {
                    self.split_leaf()
                } else if runs.len() < MIN {
                    Mutation::Underflow
                } else {
                    Mutation::Done
                };
                (old, mutation)
            }
            Node::Internal { .. } => {
                let (child_index, local) = self.locate_child_for_remove(index);
                let (old, child_mutation) = {
                    let Node::Internal { children, .. } = self else {
                        unreachable!()
                    };
                    children[child_index].set(local, item)
                };
                let mutation = self.apply_child_set_mutation(child_index, child_mutation);
                (old, mutation)
            }
        }
    }

    /// Folds a child's `set` mutation back into this node. Unlike insert or
    /// remove, a point update can produce a split *or* an underflow, so this
    /// routes to whichever repair applies.
    fn apply_child_set_mutation(&mut self, child_index: usize, mutation: Mutation) -> Mutation {
        match mutation {
            Mutation::Underflow => self.apply_child_remove_mutation(child_index, mutation),
            split_or_done => self.apply_child_insert_mutation(child_index, split_or_done),
        }
    }

    /// Returns the item at subtree-local `index`.
    fn get(&self, index: usize) -> &ItemExtent {
        match self {
            Node::Leaf { runs } => {
                let mut remaining = index;
                for run in runs {
                    if remaining < run.count {
                        return &run.extent;
                    }
                    remaining -= run.count;
                }
                unreachable!("index out of range in Node::get")
            }
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
        let Node::Leaf { runs } = self else {
            unreachable!("split_leaf on an internal node")
        };
        let mid = runs.len() / 2;
        let right_runs = runs.split_off(mid);
        let right = Node::Leaf { runs: right_runs };
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
        //
        // A borrow can come up short, which a flat-item tree never had to
        // handle: a donated run whose extent equals the head it lands beside
        // merges into it, moving items without adding an entry (equal runs
        // across a leaf boundary are legal — no-adjacent-equal is a per-leaf
        // rule). The helpers therefore donate until the child is legal or the
        // donor is exhausted, and this checks the result rather than assuming
        // one donation sufficed.
        let has_left = child_index > 0;
        let has_right = child_index + 1 < children.len();

        if has_left && children[child_index - 1].len() > MIN {
            Self::borrow_from_left(children, summaries, child_index);
            if children[child_index].len() >= MIN {
                return Mutation::Done;
            }
        } else if has_right && children[child_index + 1].len() > MIN {
            Self::borrow_from_right(children, summaries, child_index);
            if children[child_index].len() >= MIN {
                return Mutation::Done;
            }
        }

        if has_left {
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
        while children[child_index].len() < MIN && children[left_index].len() > MIN {
            Self::donate_left_to_right(children, left_index, child_index);
        }
        summaries[left_index] = children[left_index].summary();
        summaries[child_index] = children[child_index].summary();
    }

    /// Moves one entry from `left_index`'s tail to `child_index`'s head.
    fn donate_left_to_right(children: &mut [Node], left_index: usize, child_index: usize) {
        // Pop the donated entry out of the left sibling first.
        match &mut children[left_index] {
            Node::Leaf { runs } => {
                let donated = runs.pop().expect("left sibling above MIN is non-empty");
                let Node::Leaf { runs: dst } = &mut children[child_index] else {
                    unreachable!("sibling node kinds must match")
                };
                // A donated run may match the head it lands beside; merging
                // keeps the no-adjacent-equal-runs invariant across the move.
                match dst.first_mut() {
                    Some(first) if first.extent == donated.extent => {
                        first.count = first.count.saturating_add(donated.count);
                    }
                    _ => dst.insert(0, donated),
                }
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
    }

    /// Moves the first entry of the right sibling to the back of
    /// `children[child_index]`. Both are leaves or both internal.
    fn borrow_from_right(children: &mut [Node], summaries: &mut [Summary], child_index: usize) {
        let right_index = child_index + 1;
        while children[child_index].len() < MIN && children[right_index].len() > MIN {
            Self::donate_right_to_left(children, child_index, right_index);
        }
        summaries[child_index] = children[child_index].summary();
        summaries[right_index] = children[right_index].summary();
    }

    /// Moves one entry from `right_index`'s head to `child_index`'s tail.
    fn donate_right_to_left(children: &mut [Node], child_index: usize, right_index: usize) {
        match &mut children[right_index] {
            Node::Leaf { runs } => {
                let donated = runs.remove(0);
                let Node::Leaf { runs: dst } = &mut children[child_index] else {
                    unreachable!("sibling node kinds must match")
                };
                push_coalesced(dst, donated);
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
    }

    /// Merges `children[left_index + 1]` into `children[left_index]`, removing
    /// the right child and its summary slot. Both are leaves or both internal.
    fn merge(children: &mut Vec<Node>, summaries: &mut Vec<Summary>, left_index: usize) {
        let right = children.remove(left_index + 1);
        summaries.remove(left_index + 1);
        match (&mut children[left_index], right) {
            (Node::Leaf { runs: left }, Node::Leaf { runs: right }) => {
                for run in right {
                    push_coalesced(left, run);
                }
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
        if let Node::Leaf { runs } = self {
            // A zero-count run is unrepresentable by construction, and two
            // adjacent runs carrying the same extent mean a coalescing site was
            // missed — which is how the representation silently degrades back
            // to one entry per item.
            for (i, run) in runs.iter().enumerate() {
                if run.count == 0 {
                    return Err(format!("run {i} has count 0"));
                }
                if i > 0 && runs[i - 1].extent == run.extent {
                    return Err(format!("runs {} and {i} are adjacent and equal", i - 1));
                }
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
    /// **Test-only, deliberately.** It calls `make` once per item, so it is
    /// `O(count)` and cannot express an unbounded list at all — which is the
    /// defect this representation exists to remove. Production construction
    /// goes through [`Self::uniform`]; keeping this out of non-test builds
    /// stops a per-item constructor from creeping back in.
    ///
    /// Bulk-loads leaves bottom-up, so construction is `O(count)` rather than
    /// `O(count log count)` repeated inserts. Every node it builds is in
    /// `[MIN, MAX]` entries (except a sole root, which may be smaller) — see
    /// [`balanced_chunk_sizes`].
    #[cfg(test)]
    pub(super) fn from_fn(count: usize, mut make: impl FnMut(usize) -> ItemExtent) -> Self {
        let mut runs: Vec<Run> = Vec::new();
        for index in 0..count {
            push_coalesced(
                &mut runs,
                Run {
                    count: 1,
                    extent: make(index),
                },
            );
        }
        Self::from_runs(runs)
    }

    /// Builds a tree of `count` items that all carry `extent`, in `O(1)`.
    ///
    /// This is the constructor a virtualizer actually uses: every item starts
    /// unmeasured with the same estimate, which is one run. It is also the only
    /// way an unbounded list is representable — the test-only per-item
    /// `from_fn` would call its closure `usize::MAX` times.
    pub(super) fn uniform(count: usize, extent: ItemExtent) -> Self {
        Self::from_runs(if count == 0 {
            Vec::new()
        } else {
            vec![Run { count, extent }]
        })
    }

    /// Bulk-loads a tree from `runs` bottom-up, so construction is `O(runs)`
    /// rather than `O(runs log runs)` repeated inserts.
    ///
    /// Every node it builds holds `[MIN, MAX]` entries (except a sole root,
    /// which may be smaller) — see [`balanced_chunk_sizes`]. `runs` must already
    /// be coalesced; the callers all build theirs through [`push_coalesced`].
    fn from_runs(runs: Vec<Run>) -> Self {
        if runs.is_empty() {
            return Self {
                root: Node::new_leaf(),
            };
        }
        let run_count = runs.len();
        let mut iter = runs.into_iter();
        let mut level: Vec<Node> = balanced_chunk_sizes(run_count)
            .into_iter()
            .map(|size| Node::Leaf {
                runs: iter.by_ref().take(size).collect(),
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
                .expect("a non-empty run list always yields at least one node"),
        }
    }

    /// Collects every run in index order, coalescing across leaf boundaries.
    fn collect_runs(node: &Node, out: &mut Vec<Run>) {
        match node {
            Node::Leaf { runs } => {
                for run in runs {
                    push_coalesced(out, *run);
                }
            }
            Node::Internal { children, .. } => {
                for child in children {
                    Self::collect_runs(child, out);
                }
            }
        }
    }

    /// Replaces the estimate carried by every still-unmeasured item, leaving
    /// measured extents alone.
    ///
    /// `O(runs)`, which is `O(measured)` — the unmeasured items are exactly the
    /// ones that collapse. The per-item loop this replaces was `O(n log n)` and
    /// ran from inside the layout pass whenever the measured mean moved.
    pub(super) fn rehint_unmeasured(&mut self, hint: f64) {
        let mut runs = Vec::new();
        Self::collect_runs(&self.root, &mut runs);
        let mut rebuilt = Vec::with_capacity(runs.len());
        for run in runs {
            let extent = match run.extent {
                ItemExtent::Unmeasured { .. } => ItemExtent::Unmeasured { hint },
                measured @ ItemExtent::Measured { .. } => measured,
            };
            push_coalesced(
                &mut rebuilt,
                Run {
                    count: run.count,
                    extent,
                },
            );
        }
        *self = Self::from_runs(rebuilt);
    }

    /// Resizes to `n` items in `O(runs)`, reporting `(measured items dropped,
    /// their total extent)`.
    ///
    /// Growth appends one run regardless of how many items it adds; truncation
    /// drops whole runs and splits at most one. The per-item loop this replaces
    /// could not survive the unbounded sentinel in either direction: a finite
    /// feed discovered to be endless would insert `usize::MAX` items one at a
    /// time, and an endless feed that later reports a real end would remove
    /// them the same way.
    pub(super) fn resize(&mut self, n: usize, hint: f64) -> (usize, f64) {
        let len = self.len();
        if n == len {
            return (0, 0.0);
        }
        let mut runs = Vec::new();
        Self::collect_runs(&self.root, &mut runs);
        let mut rebuilt = Vec::with_capacity(runs.len() + 1);
        let mut dropped_count = 0usize;
        let mut dropped_total = 0.0_f64;

        if n > len {
            for run in runs {
                push_coalesced(&mut rebuilt, run);
            }
            push_coalesced(
                &mut rebuilt,
                Run {
                    count: n - len,
                    extent: ItemExtent::Unmeasured { hint },
                },
            );
        } else {
            let mut before = 0usize;
            for run in runs {
                let kept = n.saturating_sub(before).min(run.count);
                let discarded = run.count - kept;
                if discarded > 0 && run.extent.is_measured() {
                    dropped_count = dropped_count.saturating_add(discarded);
                    dropped_total += discarded as f64 * run.extent.extent();
                }
                push_coalesced(
                    &mut rebuilt,
                    Run {
                        count: kept,
                        extent: run.extent,
                    },
                );
                before += run.count;
            }
        }
        *self = Self::from_runs(rebuilt);
        (dropped_count, dropped_total)
    }

    /// Resets every item from `index` onward to unmeasured with `hint`,
    /// reporting `(how many measured items were dropped, their total extent)`
    /// so the caller can repair its accumulators without a second walk.
    ///
    /// `O(runs)`. The whole invalidated tail becomes a single run, so a list
    /// invalidated from item 0 costs one entry regardless of its length — and
    /// the dropped-measured tally comes out of the same walk, which is what
    /// keeps an unbounded list from being counted item by item.
    pub(super) fn invalidate_from(&mut self, index: usize, hint: f64) -> (usize, f64) {
        let len = self.len();
        if index >= len {
            return (0, 0.0);
        }
        let mut runs = Vec::new();
        Self::collect_runs(&self.root, &mut runs);
        let mut rebuilt = Vec::with_capacity(runs.len() + 1);
        let mut before = 0usize;
        let mut dropped_count = 0usize;
        let mut dropped_total = 0.0_f64;
        for run in runs {
            // The part of this run at or past `index` is being discarded; tally
            // it when it was measured.
            let kept = index.saturating_sub(before).min(run.count);
            let discarded = run.count - kept;
            if discarded > 0 && run.extent.is_measured() {
                dropped_count = dropped_count.saturating_add(discarded);
                dropped_total += discarded as f64 * run.extent.extent();
            }
            push_coalesced(
                &mut rebuilt,
                Run {
                    count: kept,
                    extent: run.extent,
                },
            );
            before += run.count;
        }
        push_coalesced(
            &mut rebuilt,
            Run {
                count: len - index,
                extent: ItemExtent::Unmeasured { hint },
            },
        );
        *self = Self::from_runs(rebuilt);
        (dropped_count, dropped_total)
    }

    /// Number of items in the tree.
    #[inline]
    pub(super) fn len(&self) -> usize {
        self.root.count()
    }

    /// Total extent of all items.
    #[inline]
    pub(super) fn total_extent(&self) -> f64 {
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
    pub(super) fn offset_of(&self, index: usize) -> f64 {
        debug_assert!(index <= self.len(), "offset_of index out of range");
        self.root.offset_of(index)
    }

    /// Maps `offset` to `(index, offset_into_item)`. `offset` is clamped to
    /// `[0, total_extent()]`. Returns `(0, 0.0)` for an empty tree.
    ///
    /// The scalar reference for [`seek_sorted`](Self::seek_sorted): production
    /// windowing goes through the batched `seek_sorted` (one shared descent),
    /// and this simple one-offset version is kept as the independent oracle the
    /// batched path is property-tested against — hence `#[cfg(test)]`.
    #[cfg(test)]
    pub(super) fn seek_offset(&self, offset: f64) -> (usize, f64) {
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

    /// Batched `seek_offset` for **ascending-sorted**
    /// `offsets`, writing each result to `out[k]`. The `[0, total]`-interior
    /// offsets are resolved in **one shared-prefix descent** (see
    /// [`Node::seek_sorted`]); the `<= 0` prefix and `>= total` suffix are
    /// clamped exactly as the scalar `seek_offset` would,
    /// so `seek_sorted(&[o0, o1, ..], out)` is observably identical to calling
    /// `seek_offset(oi)` for each — just cheaper when the offsets cluster.
    ///
    /// `offsets.len()` must equal `out.len()`, and `offsets` must be sorted
    /// ascending (the windowing band edges are sorted by construction).
    ///
    /// Complexity: `O(log n + k)` for `k` clustered offsets (the windowing case),
    /// degrading to `O(k · log n)` only if every offset lands in a distinct leaf.
    pub(super) fn seek_sorted(&self, offsets: &[f64], out: &mut [(usize, f64)]) {
        debug_assert_eq!(offsets.len(), out.len(), "offsets/out length mismatch");
        debug_assert!(
            offsets.windows(2).all(|w| w[0] <= w[1]),
            "seek_sorted requires ascending offsets"
        );
        let count = self.len();
        if count == 0 {
            out.fill((0, 0.0));
            return;
        }
        let total = self.total_extent();
        let last = count - 1;
        // Sorted, so the three regions are contiguous: `<= 0` | interior | `>= total`.
        let lo = offsets.partition_point(|&o| o <= 0.0);
        // `.max(lo)`: when `total == 0.0` (every item has zero extent), the two
        // partition predicates disagree on an offset of exactly `0.0` -- `o <=
        // 0.0` counts it into `lo` but `o < total` (= `o < 0.0`) does not, so
        // `hi` alone could fall below `lo` and underflow the `split_at_mut`
        // below. `total > 0.0` implies `o <= 0.0 ⟹ o < total`, so `lo <= hi`
        // already holds there and this is a no-op; it only clamps the `total
        // == 0.0` degenerate case, collapsing the (empty) interior region.
        let hi = offsets.partition_point(|&o| o < total).max(lo);
        let (head, rest) = out.split_at_mut(lo);
        head.fill((0, 0.0));
        let (mid, tail) = rest.split_at_mut(hi - lo);
        if !tail.is_empty() {
            let last_start = self.offset_of(last);
            for (slot, &o) in tail.iter_mut().zip(&offsets[hi..]) {
                *slot = (last, o - last_start);
            }
        }
        if !mid.is_empty() {
            self.root.seek_sorted(&offsets[lo..hi], 0, 0.0, mid);
        }
    }

    /// Replaces the item at `index`, returning the previous value.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub(super) fn set(&mut self, index: usize, item: ItemExtent) -> ItemExtent {
        assert!(index < self.len(), "set index out of range");
        let (old, mutation) = self.root.set(index, item);
        // A write into the middle of a run splits it, so unlike a flat-item
        // tree this can overflow the root leaf and grow a level; coalescing
        // can equally collapse one, so the root may need shrinking too.
        self.grow_root_if_split(mutation);
        self.shrink_root_if_needed();
        old
    }

    /// Grows a new root level when the old root split. Shared by `insert` and
    /// `set` — with run-length leaves both can overflow.
    fn grow_root_if_split(&mut self, mutation: Mutation) {
        if let Mutation::Split {
            right,
            right_summary,
        } = mutation
        {
            let old_root = std::mem::replace(&mut self.root, Node::new_leaf());
            let left_summary = old_root.summary();
            self.root = Node::Internal {
                children: vec![old_root, right],
                summaries: vec![left_summary, right_summary],
            };
        }
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

    fn measured(e: f64) -> ItemExtent {
        ItemExtent::Measured { extent: e }
    }

    fn build(extents: &[f64]) -> ExtentTree {
        ExtentTree::from_fn(extents.len(), |i| measured(extents[i]))
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
}
