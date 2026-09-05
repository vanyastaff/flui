//! Keep-alive holds for lazy sliver children.
//!
//! A lazy sliver evicts every child outside its cache band
//! (`SparseChildren::retain_band`), which costs the child its state: a half-typed field, a playing video, a nested
//! scroll offset. Keep-alive is the opt-out — while anything holds a child, the
//! band eviction skips it.
//!
//! # Why a lease and not a flag
//!
//! The decision is usually a *descendant's*: the text field deep inside a row
//! knows it has unsaved input; the row itself does not. Several descendants may
//! each want a hold independently, so a boolean resolves last-writer-wins and
//! loses one of them.
//!
//! Flutter routes this as `KeepAliveNotification(handle)` bubbling to a
//! per-item `AutomaticKeepAlive`, which keeps a `Map<Listenable, VoidCallback>`
//! and writes a `KeepAlive` parent-data widget while any handle is held. FLUI
//! keeps the decision where the decision is made — the element side, next to
//! the eviction it modifies — and replaces the handle with an RAII lease:
//!
//! - **`Drop` is the release.** Flutter fuses release into
//!   `KeepAliveHandle.dispose()` precisely because a separate `release()` gets
//!   forgotten, and its own documentation concedes the failure mode ("the
//!   subtree will continue to be kept alive until the list itself is
//!   disposed"). A `#[must_use]` guard makes that unrepresentable.
//! - **N holders are a refcount**, which is what Flutter's handle map emulates
//!   by hand.
//! - **Identity is the lease**, not an index. A held child whose logical index
//!   changes under reconcile keeps its hold, because nothing is keyed by index.
//!
//! # Where the parked child goes: nowhere
//!
//! Flutter moves a kept-alive child out of the render child list into
//! `_keepAliveBucket`, re-adopting it on revival. FLUI leaves it attached and
//! simply does not lay it out — the band walk only visits `cache_first
//! ..cache_last`. Every phase that could observe it consults the
//! placed-generation stamp, so an unlaid child is skipped by paint, both
//! hit-test walks, and semantics alike. That makes the stamp load-bearing here
//! rather than defence-in-depth, which is recorded in the ADR.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use flui_foundation::ElementId;
use smallvec::SmallVec;

/// Holders of one child. Two inline slots: a row with more than a couple of
/// independently-keeping descendants is rare.
type HolderIds = SmallVec<[ElementId; 2]>;

#[derive(Debug, Default)]
struct Inner {
    /// Held sparse child -> the holders currently keeping it alive.
    holders: HashMap<ElementId, HolderIds>,
}

/// The keep-alive table owned by one element tree.
///
/// Cheap to clone (one `Rc`), so the split-borrow `ElementOwner` and the
/// leases can all reach the same table without threading a lifetime through
/// every seam.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeepAliveHolds {
    inner: Rc<RefCell<Inner>>,
}

impl KeepAliveHolds {
    /// Registers `holder` as keeping `held` alive, returning the lease whose
    /// drop releases it.
    ///
    /// A holder may hold at most one child — it lives inside exactly one — so a
    /// repeat acquisition by the same holder is deduplicated rather than
    /// counted twice. That matters because `init_state` is the sanctioned
    /// acquisition site but a re-activated element runs it again.
    pub(crate) fn acquire(&self, holder: ElementId, held: ElementId) -> KeepAliveLease {
        {
            let mut inner = self.inner.borrow_mut();
            let holders = inner.holders.entry(held).or_default();
            if !holders.contains(&holder) {
                holders.push(holder);
            }
        }
        KeepAliveLease {
            inner: Rc::downgrade(&self.inner),
            holder,
            held,
        }
    }

    /// Whether anything is currently keeping `child` alive.
    ///
    /// This is the whole question band eviction asks.
    pub(crate) fn is_held(&self, child: ElementId) -> bool {
        self.inner.borrow().holders.contains_key(&child)
    }

    /// Drops every hold `holder` had, whichever child it was holding.
    ///
    /// Called when an element unmounts. A lease's own `Drop` normally does
    /// this, but an element can be torn down without its state being dropped
    /// in the same step, and a stranded hold would pin a child forever.
    pub(crate) fn forget_holder(&self, holder: ElementId) {
        let mut inner = self.inner.borrow_mut();
        inner.holders.retain(|_, holders| {
            holders.retain(|candidate| *candidate != holder);
            !holders.is_empty()
        });
    }

    /// Drops every hold on `child`.
    ///
    /// Called when the child is genuinely destroyed — a data-source removal,
    /// which destroys regardless of holds — so the table cannot retain an id
    /// the tree no longer has.
    pub(crate) fn forget_held(&self, child: ElementId) {
        self.inner.borrow_mut().holders.remove(&child);
    }

    /// How many children are currently held. Diagnostics and tests.
    pub(crate) fn held_count(&self) -> usize {
        self.inner.borrow().holders.len()
    }
}

/// A live keep-alive hold. Dropping it releases the hold.
///
/// Acquire one from
/// [`BuildContext::keep_alive_lease`](crate::context::BuildContext::keep_alive_lease)
/// in `init_state` (never during `build`, `perform_layout` or `paint` — see the
/// frame-capability scope rule) and store it in your `ViewState`. While it
/// lives, the lazy sliver child containing it survives scrolling out of the
/// cache band; when the state drops, so does the hold.
///
/// A lease is inert if the element was not inside a lazy sliver child, or if
/// the element tree it came from is gone.
///
/// # Example
///
/// ```rust,ignore
/// struct EditorState {
///     // Held for as long as there is unsaved text.
///     keep_alive: Option<KeepAliveLease>,
/// }
///
/// impl ViewState<Editor> for EditorState {
///     fn init_state(&mut self, ctx: &dyn BuildContext) {
///         self.keep_alive = ctx.keep_alive_lease();
///     }
/// }
///
/// // Release it when the draft is saved:
/// self.keep_alive = None;
/// ```
#[must_use = "dropping the lease immediately releases the keep-alive hold; \
              store it in your ViewState for as long as the child must survive"]
#[derive(Debug)]
pub struct KeepAliveLease {
    /// Weak, so a lease outliving its element tree is inert rather than
    /// keeping the table alive.
    inner: Weak<RefCell<Inner>>,
    holder: ElementId,
    held: ElementId,
}

impl KeepAliveLease {
    /// The lazy sliver child this lease keeps alive.
    #[must_use]
    pub fn held(&self) -> ElementId {
        self.held
    }
}

impl Drop for KeepAliveLease {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        let Some(holders) = inner.holders.get_mut(&self.held) else {
            return;
        };
        holders.retain(|candidate| *candidate != self.holder);
        if holders.is_empty() {
            inner.holders.remove(&self.held);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: usize) -> ElementId {
        ElementId::new(raw)
    }

    #[test]
    fn a_lease_holds_until_it_is_dropped() {
        let holds = KeepAliveHolds::default();
        let child = id(1);
        assert!(!holds.is_held(child));

        let lease = holds.acquire(id(2), child);
        assert!(holds.is_held(child));

        drop(lease);
        assert!(
            !holds.is_held(child),
            "dropping the lease releases the hold"
        );
    }

    /// The multi-client law: releasing one holder must not release the others.
    /// A boolean flag would fail exactly here.
    #[test]
    fn the_last_holder_releases_not_the_first() {
        let holds = KeepAliveHolds::default();
        let child = id(1);
        let first = holds.acquire(id(2), child);
        let second = holds.acquire(id(3), child);

        drop(first);
        assert!(holds.is_held(child), "one holder released, another remains");

        drop(second);
        assert!(!holds.is_held(child));
    }

    /// `init_state` runs again on a re-activated element, so a repeat
    /// acquisition must not push a second entry that outlives the first lease.
    #[test]
    fn a_repeat_acquisition_by_one_holder_is_not_counted_twice() {
        let holds = KeepAliveHolds::default();
        let child = id(1);
        let holder = id(2);

        let first = holds.acquire(holder, child);
        let second = holds.acquire(holder, child);
        drop(first);
        drop(second);

        assert!(
            !holds.is_held(child),
            "a deduplicated holder must fully release"
        );
    }

    /// An element can be torn down without its state dropping in the same step;
    /// the hold must not survive the holder.
    #[test]
    fn forgetting_a_holder_releases_its_hold() {
        let holds = KeepAliveHolds::default();
        let child = id(1);
        let lease = holds.acquire(id(2), child);

        holds.forget_holder(id(2));
        assert!(!holds.is_held(child));

        // The lease's own drop must then be harmless, not a double-release.
        drop(lease);
        assert_eq!(holds.held_count(), 0);
    }

    /// A data-source removal destroys the child regardless of holds, so the
    /// table must not retain an id the tree no longer has.
    #[test]
    fn forgetting_a_held_child_clears_every_holder() {
        let holds = KeepAliveHolds::default();
        let child = id(1);
        let a = holds.acquire(id(2), child);
        let b = holds.acquire(id(3), child);

        holds.forget_held(child);
        assert_eq!(holds.held_count(), 0);

        drop(a);
        drop(b);
        assert_eq!(holds.held_count(), 0);
    }

    /// A lease that outlives its tree is inert, not a dangling write.
    #[test]
    fn a_lease_outliving_its_table_drops_harmlessly() {
        let lease = {
            let holds = KeepAliveHolds::default();
            holds.acquire(id(2), id(1))
        };
        drop(lease);
    }
}
