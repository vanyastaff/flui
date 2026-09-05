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
//! - **Nothing is cached but the holder.** A lease records the element that took
//!   it; the child it keeps alive is resolved from the tree when eviction asks.
//!   So a held child whose logical index changes under reconcile keeps its hold,
//!   and so does one whose *host* changes — a `GlobalKey` graft from one list
//!   into another re-targets with no bookkeeping, where a cached target would
//!   pin the row the holder left (forever, since nothing would release it) while
//!   leaving the row it moved to evictable.
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

use crate::tree::ElementTree;
use smallvec::SmallVec;

/// Holders of one child. Two inline slots: a row with more than a couple of
/// independently-keeping descendants is rare.
type HolderIds = SmallVec<[ElementId; 2]>;

#[derive(Default)]
struct Inner {
    /// Live holder -> how many leases it currently holds.
    ///
    /// Deliberately **not** `held -> holders`. Caching the target at
    /// acquisition pinned the sparse child the holder happened to live in
    /// then, which a `GlobalKey` graft between two lists makes wrong in both
    /// directions at once: the row the holder left stays pinned forever, and
    /// the row it moved to is evictable despite something asking to keep it.
    /// Resolving the target when eviction asks makes relocation free the same
    /// way keying by element rather than index does.
    holders: HashMap<ElementId, usize>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("holders", &self.holders)
            .finish_non_exhaustive()
    }
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
    /// A retained handle for `holder`, so holds can be taken after
    /// `init_state` — see [`KeepAliveHandle`].
    pub(crate) fn handle(&self, holder: ElementId) -> KeepAliveHandle {
        KeepAliveHandle {
            inner: Rc::downgrade(&self.inner),
            holder,
        }
    }

    /// The sparse children currently kept alive, resolved through `tree`.
    ///
    /// Called once per band eviction rather than per candidate child: the
    /// holder set is small (it is the items a user asked to keep), and each
    /// resolution is one walk to the nearest sparse host. A holder whose
    /// element is gone from the tree resolves to nothing and is skipped, so a
    /// stale entry cannot pin a row.
    pub(crate) fn held_children(&self, tree: &ElementTree) -> HolderIds {
        let inner = self.inner.borrow();
        let mut held = HolderIds::new();
        for holder in inner.holders.keys() {
            if let Some(child) = crate::tree::enclosing_sparse_child(tree, *holder)
                && !held.contains(&child)
            {
                held.push(child);
            }
        }
        held
    }

    /// Drops every lease `holder` had.
    ///
    /// Called when an element unmounts. A lease's own `Drop` normally does
    /// this, but an element can be torn down without its state being dropped
    /// in the same step, and a stranded holder would keep resolving to
    /// whatever sparse child it last sat under.
    pub(crate) fn forget_holder(&self, holder: ElementId) {
        self.inner.borrow_mut().holders.remove(&holder);
    }

    /// How many holders are live. Diagnostics and tests.
    pub(crate) fn holder_count(&self) -> usize {
        self.inner.borrow().holders.len()
    }
}

/// A retained capability to take keep-alive holds, obtained once and used
/// later.
///
/// This is the acquisition path for the case that matters most and that a
/// one-shot lease cannot serve: a state that becomes keep-worthy *after*
/// `init_state` — an editor that becomes dirty, a video that starts playing —
/// or one that releases a hold and later needs another. There is no second
/// lifecycle hook to ask from: `did_update_view` and `activate` receive no
/// context, `did_change_dependencies` is not guaranteed to run, and acquiring
/// from `build` is forbidden by the frame-capability rule. So the *capability*
/// is acquired once, in `init_state`, and the holds are taken from it whenever
/// the answer changes — the same shape `RebuildHandle` uses for the same
/// reason.
///
/// ```rust,ignore
/// struct EditorState {
///     keep_alive: KeepAliveHandle,
///     lease: Option<KeepAliveLease>,
/// }
///
/// fn init_state(&mut self, ctx: &dyn BuildContext) {
///     self.keep_alive = ctx.keep_alive_handle();
/// }
///
/// fn on_text_changed(&mut self, dirty: bool) {
///     // false -> true and true -> false, any number of times.
///     self.lease = dirty.then(|| self.keep_alive.hold());
/// }
/// ```
#[derive(Clone, Debug)]
pub struct KeepAliveHandle {
    inner: Weak<RefCell<Inner>>,
    holder: ElementId,
}

impl KeepAliveHandle {
    /// Take a hold. Drop the returned lease to release it.
    ///
    /// Repeatable: each call is counted independently, so `hold()` before
    /// dropping a previous lease keeps the child alive across the swap.
    pub fn hold(&self) -> KeepAliveLease {
        if let Some(inner) = self.inner.upgrade() {
            *inner.borrow_mut().holders.entry(self.holder).or_insert(0) += 1;
        }
        KeepAliveLease {
            inner: self.inner.clone(),
            holder: self.holder,
        }
    }

    /// The element this handle takes holds for.
    #[must_use]
    pub fn holder(&self) -> ElementId {
        self.holder
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
/// A lease is always issued, even to an element not currently inside a lazy
/// sliver: it names its *holder*, and the child is resolved when eviction
/// asks. So it simply holds nothing there, and begins holding if the element is
/// later grafted into a list — which a `GlobalKey` state moved between subtrees
/// does. Refusing would make that refusal permanent, since `init_state` is the
/// only guaranteed acquisition point.
///
/// A lease outliving the element tree it came from is inert.
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
    /// The element that took the lease. The child it keeps alive is resolved
    /// from this when eviction asks, never cached — see [`Inner`].
    holder: ElementId,
}

impl KeepAliveLease {
    /// The element holding this lease.
    #[must_use]
    pub fn holder(&self) -> ElementId {
        self.holder
    }
}

impl Drop for KeepAliveLease {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        // Decrement by one — this lease's own. Clearing the holder outright
        // would release a replacement taken before this one dropped, which is
        // the reacquisition case `acquire` documents.
        if let Some(count) = inner.holders.get_mut(&self.holder) {
            *count -= 1;
            if *count == 0 {
                inner.holders.remove(&self.holder);
            }
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
        let holder = id(2);
        assert_eq!(holds.holder_count(), 0);

        let lease = holds.handle(holder).hold();
        assert_eq!(holds.holder_count(), 1);

        drop(lease);
        assert_eq!(holds.holder_count(), 0, "dropping the lease releases it");
    }

    /// The multi-client law: releasing one holder must not release the others.
    /// A boolean flag would fail exactly here.
    #[test]
    fn the_last_holder_releases_not_the_first() {
        let holds = KeepAliveHolds::default();
        let first = holds.handle(id(2)).hold();
        let second = holds.handle(id(3)).hold();

        drop(first);
        assert_eq!(holds.holder_count(), 1, "one released, another remains");

        drop(second);
        assert_eq!(holds.holder_count(), 0);
    }

    /// The reacquisition idiom must not drop the hold.
    ///
    /// `self.lease = ctx.keep_alive_lease()` builds the new lease before
    /// dropping the old one. If acquisitions by one holder were deduplicated,
    /// the new lease would add nothing and the old one's `Drop` would remove
    /// the only entry — leaving a live lease over an unheld child, evicted on
    /// the next band move. This is that ordering, spelled out.
    #[test]
    fn reacquiring_before_dropping_the_old_lease_keeps_the_hold() {
        let holds = KeepAliveHolds::default();
        let holder = id(2);

        let mut lease = Some(holds.handle(holder).hold());
        // The replacement is built while the old one is still alive, and the
        // old one drops only after it lands.
        let replacement = holds.handle(holder).hold();
        drop(lease.replace(replacement));
        assert_eq!(holds.holder_count(), 1, "the hold survives the swap");

        drop(lease.take());
        assert_eq!(holds.holder_count(), 0, "and the last lease releases it");
    }

    /// An element can be torn down without its state dropping in the same
    /// step; the holder must not survive it.
    #[test]
    fn forgetting_a_holder_releases_every_lease_it_had() {
        let holds = KeepAliveHolds::default();
        let holder = id(2);
        let first = holds.handle(holder).hold();
        let second = holds.handle(holder).hold();

        holds.forget_holder(holder);
        assert_eq!(holds.holder_count(), 0);

        // The leases' own drops must then be harmless, not an underflow.
        drop(first);
        drop(second);
        assert_eq!(holds.holder_count(), 0);
    }

    /// A lease that outlives its tree is inert, not a dangling write.
    #[test]
    fn a_lease_outliving_its_table_drops_harmlessly() {
        let lease = {
            let holds = KeepAliveHolds::default();
            holds.handle(id(2)).hold()
        };
        drop(lease);
    }
}
