//! Sparse, on-demand child storage for lazy slivers — the FLUI analogue of the
//! child bookkeeping in Flutter's `SliverMultiBoxAdaptorElement`.
//!
//! A normal multi-child element keeps a *dense* `Vec<ElementId>` reconciled
//! top-down. A lazy sliver instead builds only the children whose logical
//! indices fall inside the viewport's visible-plus-cache band, in arbitrary
//! order, and disposes them when they scroll off. [`SparseChildren`] is that
//! bookkeeping: a `logical index -> ElementId` map plus mount/evict operations
//! that reuse [`ElementTree::insert`]/[`ElementTree::remove`] and stamp each
//! freshly-built child's render node with its [`SliverMultiBoxAdaptorParentData`](flui_rendering::parent_data::SliverMultiBoxAdaptorParentData)
//! index. Stamping is what lets the lazy sliver recover `logical -> dense slot`
//! from parent-data alone (ADR-0003), so children may be attached in any order —
//! FLUI has no equivalent of Flutter's `_currentBeforeChild` insertion cursor.

use std::any::{Any, TypeId};
#[cfg(test)]
use std::collections::btree_map::Keys;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::AssertUnwindSafe;

use flui_foundation::{ElementId, RenderId, SaltedKey, ViewKey};
use flui_rendering::pipeline::PipelineCell;

use crate::BoxedView;
use crate::ElementOwner;
use crate::owner::{LifecycleHook, RecoveredAt, RecoveredPanic};
use crate::tree::ElementNode;
use crate::tree::ElementTree;
use crate::tree::ProvisionalOrder;
use crate::tree::SubtreeRemoval;
use crate::view::View;
use crate::view::recovery_view_for;

/// Bookkeeping for a lazy sliver's on-demand children.
///
/// Children are keyed by *logical index* (their position in the data source),
/// not by dense slot — the map is sparse because only the visible-plus-cache
/// band is built. Ordered (`BTreeMap`) so band eviction sweeps in index order.
///
/// # Invariant: host `child_ids` stays empty
///
/// The adaptor element that owns a `SparseChildren` must **never** append its
/// lazy children to the host's `ElementNode::child_ids` list. If it did, a
/// dense reconcile of the host (e.g. on a rebuild triggered by an unrelated
/// state change) would call `reconcile(host, [])` and delete all lazy children
/// via the normal dense teardown path before `SparseChildren` can evict them
/// gracefully. `RenderSliverList` indexes children by their
/// `SliverMultiBoxAdaptorParentData.index` field (stamped at `ensure` time),
/// not by dense slot order, so the empty `child_ids` is safe and intentional.
#[derive(Debug, Default)]
pub(crate) struct SparseChildren {
    by_logical_index: BTreeMap<usize, ElementId>,
}

impl SparseChildren {
    /// An empty manager — no children built yet.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Number of currently-built children.
    pub(crate) fn len(&self) -> usize {
        self.by_logical_index.len()
    }

    /// Whether no child is currently built.
    ///
    /// Used in tests; suppressed in release builds to avoid the dead-code lint
    /// until a production caller lands.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.by_logical_index.is_empty()
    }

    /// The `ElementId` of the child built at `logical_index`, if any.
    pub(crate) fn get(&self, logical_index: usize) -> Option<ElementId> {
        self.by_logical_index.get(&logical_index).copied()
    }

    /// The logical indices of all currently-built children, ascending.
    ///
    /// Used in tests; suppressed in release builds to avoid the dead-code lint
    /// until a production caller lands.
    #[cfg(test)]
    pub(crate) fn logical_indices(&self) -> Keys<'_, usize, ElementId> {
        self.by_logical_index.keys()
    }

    /// Iterate over all currently-built `(logical_index, ElementId)` pairs.
    ///
    /// Used by the adaptor element's `on_unmount` to find and subtree-remove
    /// every lazy child: since the host's `child_ids` stays empty by
    /// invariant, the generic tree-walk that covers dense children cannot
    /// reach them.
    pub(crate) fn iter_built(&self) -> impl Iterator<Item = (usize, ElementId)> + '_ {
        self.by_logical_index
            .iter()
            .map(|(&logical_index, &id)| (logical_index, id))
    }

    /// Ensure a child exists at `logical_index`, building it from `view` under
    /// `host` if absent. Returns the child's `ElementId` (existing or freshly
    /// mounted). A freshly-mounted child has its render node stamped with
    /// `SliverMultiBoxAdaptorParentData { index: logical_index }` so the lazy
    /// sliver can map it back to a dense slot regardless of attach order.
    ///
    /// Idempotent: a second call for an already-built index returns the existing
    /// id and does **not** rebuild (reconciling a changed `view` is a later
    /// concern — Flutter's `updateChild`).
    pub(crate) fn ensure(
        &mut self,
        logical_index: usize,
        view: &dyn View,
        host: ElementId,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
        pipeline: &PipelineCell,
    ) -> ElementId {
        if let Some(&existing) = self.by_logical_index.get(&logical_index) {
            return existing;
        }
        let child = mount_sparse_child(logical_index, view, host, tree, owner, pipeline);
        self.by_logical_index.insert(logical_index, child);
        child
    }

    /// Drop the bookkeeping for `child` without touching the tree — the
    /// child was grafted to another parent by a `GlobalKey` retake and is
    /// no longer this sliver's to evict or refresh (Flutter's
    /// `SliverMultiBoxAdaptorElement.forgetChild`). Returns the logical
    /// index it held, if it was resident.
    pub(crate) fn forget(&mut self, child: ElementId) -> Option<usize> {
        let index = self
            .by_logical_index
            .iter()
            .find_map(|(&index, &id)| (id == child).then_some(index))?;
        self.by_logical_index.remove(&index);
        tracing::trace!(
            logical_index = index,
            ?child,
            "SparseChildren forgot a grafted child"
        );
        Some(index)
    }

    /// Evict the child at `logical_index`, unmounting its element subtree (and
    /// thus its render nodes). Returns whether a child was removed; a `false`
    /// means no child was built at that index.
    pub(crate) fn evict(
        &mut self,
        logical_index: usize,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
    ) -> bool {
        let Some(child) = self.by_logical_index.remove(&logical_index) else {
            return false;
        };
        // Use `remove_subtree` so the child's entire descendant subtree is
        // freed.  A single-node `tree.remove` only removes the top-level element
        // and leaks every descendant (e.g. the Padding and Text inside a
        // Container child stay as orphaned slab entries and dangling render nodes).
        tree.remove_subtree(child, owner, SubtreeRemoval::DeactivateKeyed);
        tracing::trace!(logical_index, ?child, "SparseChildren evicted lazy child");
        true
    }

    /// Evict every child whose logical index falls outside the half-open band
    /// `[first, last)` — the children that have scrolled out of the cache band.
    /// `O(K)` in the currently-built child count `K` (bounded by the band).
    ///
    /// Returns `true` if at least one child was evicted, `false` if all built
    /// children were already inside the band (no work done). Callers use this
    /// to decide whether to mark the sliver dirty for re-layout.
    pub(crate) fn retain_band(
        &mut self,
        first: usize,
        last: usize,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
    ) -> bool {
        // Keep-alive is honoured *here and only here*. Band eviction is the
        // "this scrolled away" path, which is exactly what a hold opts out of.
        //
        // The reconcile path must NOT consult holds: a resident the data source
        // stopped producing is destroyed regardless, because a held child
        // squatting on index 3 while a keyed resident relocates onto 3 would
        // leave two attached children stamped 3 and trip the uniqueness
        // assertion in the band walk. Flutter draws the same line — a hold is
        // consulted by `collectGarbage`, while a removal goes through
        // `removeChild` and destroys unconditionally.
        // Resolved once for the pass, not per candidate: each holder costs one
        // walk to its nearest sparse host, and resolving it *now* rather than
        // caching it at acquisition is what makes a holder grafted between two
        // lists re-target instead of pinning the row it left.
        let held = owner.keep_alive.held_children(tree);
        let out_of_band: Vec<usize> = self
            .by_logical_index
            .iter()
            .filter(|(logical_index, child)| {
                (**logical_index < first || **logical_index >= last) && !held.contains(child)
            })
            .map(|(logical_index, _)| *logical_index)
            .collect();
        let any_evicted = !out_of_band.is_empty();
        for logical_index in out_of_band {
            self.evict(logical_index, tree, owner);
        }
        // The parked set is unbounded and user-controlled, and nothing else
        // reports it: a leaked hold shows up as a list that quietly stops
        // reclaiming memory. Emitting the count here is the one place that
        // sees both halves.
        let holders = owner.keep_alive.holder_count();
        if holders > 0 {
            tracing::trace!(
                band = ?(first, last),
                resident = self.by_logical_index.len(),
                holders,
                held = held.len(),
                "SparseChildren retained a band with keep-alive holds"
            );
        }
        any_evicted
    }

    /// Reconcile the resident children against a (possibly changed) data
    /// source — the sparse counterpart of Flutter's
    /// `SliverMultiBoxAdaptorElement.performRebuild`
    /// (`widgets/sliver.dart`, tag `3.44.0`), and the mechanism behind
    /// `SliverChildBuilderDelegate.shouldRebuild => true`: a new delegate
    /// re-consults the builder for every resident index, not only the
    /// newly-visible ones.
    ///
    /// Two-phase, into a fresh map, so that a shift or swap of several keyed
    /// residents can never overwrite one of them (Flutter's separate
    /// `newChildren` map is load-bearing for the same reason):
    ///
    /// 1. **Snapshot and build.** Record every resident `(index, element,
    ///    key)`. The indices to build are the resident ones plus, for every
    ///    keyed resident, the index `find_index_by_key` reports for its key —
    ///    that is how a keyed child whose data moved *out of the resident
    ///    band* is still found (Flutter's `findChildIndexCallback`; a
    ///    `SliverChildListDelegate` derives the map from its children). Every
    ///    index is built through [`build_item_or_error`], so a panicking
    ///    builder yields an error child at that index and nothing else.
    /// 2. **Match and apply.** A built view with a key claims the first
    ///    unclaimed resident carrying an equal key (first wins on duplicate
    ///    keys, as the dense reconciler does) — wherever that resident sat,
    ///    so a keyed child moving *within* the band needs no callback at all;
    ///    a keyless view claims the resident at its own index when the types
    ///    agree. A claimed resident is updated in place, relocated first if
    ///    its index changed ([`ElementTree::relocate_sparse_child`] re-slots
    ///    it and re-derives the `sliver_slot` chain, then its render
    ///    descendants are re-stamped); an unclaimed resident is evicted; an
    ///    unclaimed view is mounted fresh. Keys are compared as the
    ///    residents carry them (a per-item wrapper carries the item's key
    ///    salted, and so does the freshly built wrapper); the callback sees
    ///    the item's own key through [`SaltedKey::unsalt`].
    ///
    /// `item_count` bounds the indices worth building. A builder answering
    /// `None` below it means the data source shrank: the index is left
    /// empty and reported in [`ReconcileOutcome::end_reached_at`] so the
    /// caller can clamp the render object's count.
    ///
    /// `retain_band` bounds the work: a keyless resident outside the band the
    /// layout pass retained is neither built nor evicted here — it is carried
    /// over for the band eviction that follows, so a scroll that rebuilds the
    /// host never calls the builder for an item it is about to drop (Flutter
    /// rebuilds every resident and collects the garbage afterwards).
    /// A **kept-alive** resident is the exception, and reconciles like an
    /// in-band one: it is not about to be dropped, so carrying it over would
    /// freeze its content at whatever the data said when it left the band —
    /// see [ADR-0056](../../../../docs/adr/ADR-0056-keep-alive-is-an-element-side-lease.md). A view
    /// built for an out-of-band index that claims no resident is dropped, not
    /// mounted.
    ///
    /// `host` is the adaptor element's own id (the parent for fresh mounts).
    pub(crate) fn reconcile(
        &mut self,
        source: ReconcileSource<'_>,
        host: ElementId,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
        pipeline: &PipelineCell,
    ) -> ReconcileOutcome {
        let ReconcileSource {
            builder,
            find_index_by_key,
            item_count,
            retain_band: (band_first, band_last),
        } = source;
        let in_band = |index: usize| index >= band_first && index < band_last;
        // The identity a lazy-delegate panic (the item builder or
        // `find_index_by_key`) is recorded against: there is no item view to
        // name yet when either panics, so the host that owns the delegate is
        // the closest identity available (`RecoveredAt::LazyDelegate`'s
        // doc). Resolved once for the whole reconcile rather than per call —
        // it cannot change mid-pass.
        let host_view_type_id = tree
            .get(host)
            .map(|node| node.element().view_type_id())
            .expect("BUG: host must be live while reconciling its own lazy children");
        // ── Phase 1: snapshot the residents, decide what to build, build ──
        struct Resident {
            index: usize,
            id: ElementId,
            key: Option<Box<dyn ViewKey>>,
            claimed: bool,
            /// Keyless and outside the band: left to the band eviction.
            carried_over: bool,
        }
        // Same one-walk-per-holder resolution the band eviction uses.
        let held = owner.keep_alive.held_children(tree);
        let mut residents: Vec<Resident> = self
            .by_logical_index
            .iter()
            .map(|(&index, &id)| {
                let key = tree
                    .get(id)
                    .and_then(|node| node.key().map(ViewKey::clone_key));
                // Carrying over is an optimisation for a resident that is
                // *about to be dropped* by the band eviction that follows, so
                // rebuilding it would call the builder for an item on its way
                // out. A held child is the exact opposite: it persists
                // indefinitely, so skipping it would leave it showing whatever
                // its data said when it left the band, and would deny it the
                // update that might release the hold. Held children therefore
                // reconcile like in-band residents.
                let carried_over = key.is_none() && !in_band(index) && !held.contains(&id);
                Resident {
                    index,
                    id,
                    key,
                    claimed: false,
                    carried_over,
                }
            })
            .collect();
        let mut targets: BTreeSet<usize> = residents
            .iter()
            .filter(|r| !r.carried_over)
            .map(|r| r.index)
            .collect();
        if let Some(find) = find_index_by_key {
            for resident in &residents {
                if let Some(key) = &resident.key
                    && let Some(new_index) = find_index_or_none(
                        find,
                        SaltedKey::unsalt(&**key),
                        host,
                        host_view_type_id,
                        owner,
                    )
                    && new_index < item_count
                {
                    targets.insert(new_index);
                }
            }
        }
        let mut end_reached_at: Option<usize> = None;
        let built: Vec<(usize, Option<BoxedView>)> = targets
            .into_iter()
            .map(|index| {
                let view = if index < item_count {
                    build_item_or_report(builder, index, host, host_view_type_id, owner)
                } else {
                    None
                };
                if view.is_none() && index < item_count {
                    end_reached_at = Some(end_reached_at.map_or(index, |end| end.min(index)));
                }
                (index, view)
            })
            .collect();

        // ── Phase 2: match built views to residents ──
        // `matched[k] = Some(resident position)` for built entry `k`. Keyed
        // residents are bucketed by key hash (decided by `key_eq` inside the
        // bucket, as the dense reconciler does) and keyless ones by index, so
        // each built view costs O(1) expected rather than a scan of the band.
        let mut keyed_by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
        let mut keyless_by_index: HashMap<usize, usize> = HashMap::new();
        for (pos, resident) in residents.iter().enumerate() {
            match &resident.key {
                Some(key) => keyed_by_hash.entry(key.key_hash()).or_default().push(pos),
                None => {
                    keyless_by_index.insert(resident.index, pos);
                }
            }
        }
        let mut matched: Vec<Option<usize>> = vec![None; built.len()];
        for (k, (index, view)) in built.iter().enumerate() {
            let Some(view) = view else {
                continue;
            };
            let view: &dyn View = view.0.as_ref();
            let candidate = if let Some(key) = view.key() {
                keyed_by_hash.get(&key.key_hash()).and_then(|bucket| {
                    bucket.iter().copied().find(|&pos| {
                        let resident = &residents[pos];
                        !resident.claimed
                            && resident.key.as_deref().is_some_and(|rk| rk.key_eq(key))
                    })
                })
            } else {
                keyless_by_index
                    .get(index)
                    .copied()
                    .filter(|&pos| !residents[pos].claimed)
            };
            if let Some(pos) = candidate
                && resident_type_matches(tree, residents[pos].id, view)
            {
                residents[pos].claimed = true;
                matched[k] = Some(pos);
            }
        }

        // ── Apply into a fresh map: evict, relocate, update, mount ──
        let mut any_work = false;
        let mut next: BTreeMap<usize, ElementId> = BTreeMap::new();
        for resident in residents.iter().filter(|r| r.carried_over) {
            next.insert(resident.index, resident.id);
        }
        for resident in residents.iter().filter(|r| !r.claimed && !r.carried_over) {
            tree.remove_subtree(resident.id, owner, SubtreeRemoval::DeactivateKeyed);
            tracing::trace!(
                logical_index = resident.index,
                child = ?resident.id,
                "SparseChildren evicted an unclaimed resident"
            );
            any_work = true;
        }
        for (k, (index, view)) in built.iter().enumerate() {
            let Some(view) = view else {
                continue;
            };
            let view: &dyn View = view.0.as_ref();
            if let Some(pos) = matched[k] {
                {
                    let resident = &residents[pos];
                    if resident.index != *index {
                        tree.relocate_sparse_child(resident.id, *index);
                        tracing::trace!(
                            from = resident.index,
                            to = *index,
                            child = ?resident.id,
                            "SparseChildren relocated a keyed resident"
                        );
                        any_work = true;
                    }
                    // Stamped on EVERY reconcile, not only on a move. The host
                    // mints the slot from its current mapping, and that mapping
                    // changes without any child changing index — a list
                    // shrinking from 100 items to 40 leaves every resident
                    // where it is, and a resident that kept its old stamp goes
                    // on announcing "of 100".
                    stamp_logical_index(tree, pipeline, host, resident.id, *index);
                    let live = match update_or_replace_resident(
                        resident.id,
                        *index,
                        view,
                        host,
                        tree,
                        owner,
                        pipeline,
                    ) {
                        Ok(()) => resident.id,
                        Err(replacement) => {
                            any_work = true;
                            replacement
                        }
                    };
                    // Mirrors the dense reconciler's post-update scheduling
                    // (`tree/id_reconcile.rs`): an update that left the child
                    // clean (its own `should_skip_rebuild` memoization fired)
                    // must not be pushed onto the build heap.
                    if let Some(node) = tree.get(live)
                        && node.element().is_dirty()
                    {
                        let depth = node.depth();
                        owner.schedule_build_for(live, depth, crate::RebuildReason::ParentUpdate);
                        any_work = true;
                    }
                    next.insert(*index, live);
                }
            } else if in_band(*index) {
                let child = mount_sparse_child(*index, view, host, tree, owner, pipeline);
                next.insert(*index, child);
                any_work = true;
            }
            // An unclaimed view outside the band would be evicted by the
            // band before it was ever laid out: not mounted.
        }
        self.by_logical_index = next;
        ReconcileOutcome {
            did_work: any_work,
            end_reached_at,
        }
    }
}

/// A borrowed key → index callback (Flutter's `findChildIndexCallback`).
pub(crate) type FindIndexByKeyRef<'a> = &'a dyn Fn(&dyn ViewKey) -> Option<usize>;

/// The data source [`SparseChildren::reconcile`] reconciles against.
pub(crate) struct ReconcileSource<'a> {
    /// The delegate's item builder.
    pub(crate) builder: &'a dyn Fn(usize) -> Option<BoxedView>,
    /// The delegate's key → index callback, if it has one.
    pub(crate) find_index_by_key: Option<FindIndexByKeyRef<'a>>,
    /// The data source length as the render object currently knows it.
    pub(crate) item_count: usize,
    /// The `[first, last)` band the layout pass retained. A keyless resident
    /// outside it is about to be evicted by the band and is carried over
    /// untouched rather than rebuilt; nothing is mounted fresh outside it.
    /// Keyed residents are always reconciled — their data may have moved
    /// into the band.
    pub(crate) retain_band: (usize, usize),
}

/// What [`SparseChildren::reconcile`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReconcileOutcome {
    /// Whether any child was evicted, relocated, remounted, or left dirty —
    /// callers mark the sliver for re-layout on `true`.
    pub(crate) did_work: bool,
    /// The lowest index below `item_count` for which the builder answered
    /// `None`: the data source shrank and the render object's count should
    /// be clamped to it.
    pub(crate) end_reached_at: Option<usize>,
}

/// Mount `view` at `logical_index`, substituting the registered `ErrorView`
/// when [`ElementTree::mount_or_substitute`]'s containment window catches a
/// panic — the boundary one level up from [`build_item_or_error`], which
/// bounds only the *builder*.
///
/// The boundary is [`ElementTree::mount_or_substitute`]'s permanent window
/// invariant: `View::create_element()` through the end of `mount`, or — for
/// a `GlobalKey` retake — two separate windows around `activate_subtree` and
/// the retake's own `update`. The sliver-index stamp and the
/// `schedule_build_for` tail below are OUTSIDE those windows: a panic there is a `BUG:` in this module's own
/// bookkeeping, not user code, and propagates rather than being recovered.
/// Framework code the windows do not cover — the debug-only eager
/// duplicate-GlobalKey rejection, the retake preflight — also propagates,
/// exactly as it does through a dense parent.
fn mount_sparse_child(
    logical_index: usize,
    view: &dyn View,
    host: ElementId,
    tree: &mut ElementTree,
    owner: &mut ElementOwner<'_>,
    pipeline: &PipelineCell,
) -> ElementId {
    // Declare `host` as the parent being reconciled for the duration of
    // this insert, including its substitute if the first attempt panics.
    // `ElementTree::insert` refuses to relocate an active GlobalKey onto a
    // parent that is not the one currently reconciling, and its rejection
    // arm panics — so without this, a keyed item scrolling from one lazy
    // list into another aborted the process instead of moving.
    // `service_child_requests` calls `service` (and so this) outside any
    // other reconcile, before its own `build_scope`, so this never nests
    // inside the guard `reconcile_children_by_id` installs — which
    // `begin_reconcile` asserts against.
    let _reconcile_guard = tree.begin_reconcile(host);
    let child = tree.mount_or_substitute(
        view,
        host,
        logical_index,
        owner,
        ProvisionalOrder::NONE,
        format!("mounting lazy sliver child {logical_index}"),
    );
    stamp_logical_index(tree, pipeline, host, child, logical_index);

    // `ElementTree::insert` (via `ElementCore::mount`) sets the child's
    // `dirty = true` but does NOT push it onto the build heap — only
    // `id_reconcile.rs` does that through `schedule_build_for`.  Without
    // this explicit push the follow-up `build_scope` in
    // `BuildOwner::service_child_requests` drains an empty heap and the
    // child's own subtree (e.g. Padding(Text)) never expands.
    let child_depth = tree.get(child).map_or(0, ElementNode::depth);
    owner.schedule_build_for(child, child_depth, crate::RebuildReason::ChildListChange);

    tracing::trace!(
        logical_index,
        ?child,
        ?host,
        "SparseChildren mounted lazy child"
    );
    child
}

/// Apply `view` to the resident at `resident_id`, substituting the registered
/// `ErrorView` when [`ElementTree::update_or_substitute`]'s containment
/// window (the resident's own `update`) catches a panic.
///
/// Returns `Err(replacement)` with the id of the freshly mounted error child
/// when it had to substitute; the caller re-points its residency map at it.
///
/// The panic this bounds is a user `View::update_render_object`, which runs
/// inside `RenderBehavior::on_update`'s `with_mut` — half-applied, and with
/// `apply_render_update_impact` never reached, so the render object can be
/// left carrying part of a new configuration that nothing marked dirty. That
/// is why the recovery removes the resident outright rather than keeping it:
/// a silently stale subtree is the one outcome worse than a visible error.
fn update_or_replace_resident(
    resident_id: ElementId,
    logical_index: usize,
    view: &dyn View,
    host: ElementId,
    tree: &mut ElementTree,
    owner: &mut ElementOwner<'_>,
    pipeline: &PipelineCell,
) -> Result<(), ElementId> {
    let now = tree.update_or_substitute(
        resident_id,
        view,
        owner,
        format!("updating lazy sliver child {logical_index}"),
    );
    if now == resident_id {
        Ok(())
    } else {
        stamp_logical_index(tree, pipeline, host, now, logical_index);
        // Same tail `mount_or_substitute`'s own fresh-mount path runs — a
        // substitute is a fresh child too, and needs the same explicit push
        // onto the build heap (see `mount_sparse_child`'s doc).
        let depth = tree.get(now).map_or(0, ElementNode::depth);
        owner.schedule_build_for(now, depth, crate::RebuildReason::ChildListChange);
        Err(now)
    }
}

/// Call `builder(index)` under a panic boundary, with no recovery or
/// reporting of its own — the raw window every caller of the item builder
/// shares. `Err` carries the unconverted panic payload so each caller builds
/// its own `FlutterError`/`RecoveredPanic` exactly once, instead of the
/// payload being converted here and thrown away by a caller that does not
/// want it (the count probes).
fn try_build_item(
    builder: &dyn Fn(usize) -> Option<BoxedView>,
    index: usize,
) -> Result<Option<BoxedView>, Box<dyn Any + Send>> {
    std::panic::catch_unwind(AssertUnwindSafe(|| builder(index)))
}

/// Build the item at `index` through `builder`, substituting the registered
/// `ErrorView` when the builder panics, **without reporting** — for
/// `sliver_adaptor.rs`'s two COUNT-PROBE callers (`regrown_item_count` and
/// `probe_item_count`), which build and discard the view purely to learn
/// whether an index exists.
///
/// A probe's panic is unreported by design, not by oversight: an index a
/// probe hits either goes on to be built again through the mount path
/// ([`build_item_or_report`], from `reconcile` or the adaptor's own mount
/// caller) — which DOES report it — or it is never mounted at all (an index
/// past the retained band), in which case there is no production content
/// this panic ever affected. Reporting it here too would count one failing
/// builder as two failures for the indices that go on to be mounted.
///
pub(crate) fn build_item_or_error(
    builder: &dyn Fn(usize) -> Option<BoxedView>,
    index: usize,
) -> Option<BoxedView> {
    match try_build_item(builder, index) {
        Ok(view) => view,
        Err(payload) => Some(recovery_view_for(&crate::view::FlutterError::from_panic(
            payload.as_ref(),
            format!("probing lazy sliver child {index}"),
        ))),
    }
}

/// Build the item at `index` through `builder`, substituting the registered
/// `ErrorView` and recording a [`RecoveredPanic`] when the builder panics —
/// the lazy-sliver counterpart of
/// [`build_or_recover`](super::behavior_commons::build_or_recover), and the
/// port of `SliverChildBuilderDelegate.build`'s `try { builder(context,
/// index) } catch { _createErrorWidget(...) }`.
///
/// For the two MOUNT-path callers ([`SparseChildren::reconcile`] and
/// `SliverAdaptorManager::service`, `sliver_adaptor.rs`): the index a
/// panicking builder yields an error view for is either resident already or
/// about to be mounted, so — unlike [`build_item_or_error`]'s probe-only
/// callers — this IS production content, and the panic is reported through
/// `owner` exactly once, at [`LifecycleHook::Build`].
///
/// Recovery is per item, as in Flutter: the error child takes exactly the
/// panicking index, is unkeyed (so it can never be mistaken for a user's
/// keyed item by `find_index_by_key`), and updates in place while the panic
/// persists. Everything the caller had already done for other indices
/// stands.
pub(crate) fn build_item_or_report(
    builder: &dyn Fn(usize) -> Option<BoxedView>,
    index: usize,
    host: ElementId,
    host_view_type_id: TypeId,
    owner: &mut ElementOwner<'_>,
) -> Option<BoxedView> {
    match try_build_item(builder, index) {
        Ok(view) => view,
        Err(payload) => {
            let panic = RecoveredPanic::from_payload(
                RecoveredAt::LazyDelegate {
                    host,
                    index: Some(index),
                },
                host_view_type_id,
                LifecycleHook::Build,
                payload.as_ref(),
                format!("building lazy sliver child {index}"),
            );
            let substitute = recovery_view_for(&panic.error);
            owner.push_recovered_panic(panic);
            Some(substitute)
        }
    }
}

/// Consult a user `find_index_by_key` callback under the same panic boundary
/// as the builder; a panicking callback declines the move (answers `None`,
/// same as a genuine "not found") and is recorded once, at
/// [`LifecycleHook::LazyIndexLookup`], through `owner`.
fn find_index_or_none(
    find: &dyn Fn(&dyn ViewKey) -> Option<usize>,
    key: &dyn ViewKey,
    host: ElementId,
    host_view_type_id: TypeId,
    owner: &mut ElementOwner<'_>,
) -> Option<usize> {
    match std::panic::catch_unwind(AssertUnwindSafe(|| find(key))) {
        Ok(index) => index,
        Err(payload) => {
            owner.push_recovered_panic(RecoveredPanic::from_payload(
                RecoveredAt::LazyDelegate { host, index: None },
                host_view_type_id,
                LifecycleHook::LazyIndexLookup,
                payload.as_ref(),
                "resolving a lazy sliver child index by key",
            ));
            None
        }
    }
}

/// Whether `existing`'s live element can be updated in place by `new`.
///
/// Delegates to `tree/id_reconcile.rs`'s `can_update_by_id` — the same
/// type-then-key predicate the dense reconciler uses, so a keyed lazy child
/// (an item wrapper carrying the item's salted key, or an item answering
/// `View::key` itself) remounts on a key mismatch exactly as Flutter's
/// `Widget.canUpdate` demands, and a keyless one reconciles by type alone.
///
/// [`ViewKey`]: flui_foundation::ViewKey
fn resident_type_matches(tree: &ElementTree, existing: ElementId, new: &dyn View) -> bool {
    crate::tree::id_reconcile::can_update_by_id(tree, existing, new)
}

// Called from `SparseChildren::ensure` via the lazy-sliver adaptor element.
/// Stamp the render node(s) that carry `child`'s sliver logical index — the
/// first render descendants reachable from `child` without crossing another
/// render element — so the lazy sliver can map `logical -> dense slot` from
/// parent-data alone.
///
/// This is the *relocation* half of the stamp. A freshly-mounted render
/// object is stamped at adoption by `RenderBehavior::on_mount`, reading the
/// `sliver_slot` the slab seeded on insert (Flutter's `didAdoptChild`); a
/// subtree that arrives through GlobalKey relocation never re-mounts, so its
/// already-built render descendants are found here and stamped explicitly.
/// A fresh composite child (a bare `Text`, a `StatefulView`) has no render
/// descendant yet at `ensure` time — the walk stamps nothing, and the
/// adopt-time path covers it when the follow-up build expands the subtree.
fn stamp_logical_index(
    tree: &ElementTree,
    pipeline: &PipelineCell,
    host: ElementId,
    child: ElementId,
    logical_index: usize,
) {
    let render_ids = first_render_descendants(tree, child);
    if render_ids.is_empty() {
        return;
    }
    // Minted by the HOST's mapping, not assembled here, so the relocation path
    // and the mount path cannot disagree about a child's semantic position.
    //
    // `sliver_slot_for_child` rather than `child_sliver_slot`: the latter also
    // encodes the INHERITANCE rules, under which a render element resets the
    // slot to `None` for its children. This function is only ever reached from
    // a sparse host, which is itself a render element, so asking the
    // inheritance question here would answer about the wrong relationship.
    let slot = tree.get(host).map_or_else(
        || flui_rendering::parent_data::SliverSlot::identity(logical_index),
        |node| node.element().sliver_slot_for_child(logical_index),
    );
    pipeline.with_mut(|owner| {
        for render_id in render_ids {
            crate::element::behavior_commons::stamp_sliver_slot(owner, render_id, slot);
        }
    });
}

/// The render ids of the first render elements reachable from `root`
/// (inclusive) walking down through composite elements only. A render
/// element stops the walk on its branch: its own children attach under it,
/// not under the sliver.
fn first_render_descendants(tree: &ElementTree, root: ElementId) -> Vec<RenderId> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(node) = tree.get(id) else {
            continue;
        };
        if let Some(render_id) = node.element().render_id() {
            found.push(render_id);
        } else {
            stack.extend(node.child_ids().iter().copied());
        }
    }
    found
}

#[cfg(test)]
#[path = "sparse_children/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "sparse_children/reconcile_tests.rs"]
mod reconcile_tests;
