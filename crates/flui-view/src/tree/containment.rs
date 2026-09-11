//! Per-child containment: undo a fresh mount / `GlobalKey` retake / update
//! that a user lifecycle-hook panic interrupted, and substitute the
//! registered `ErrorView` in its place (issue #561).
//!
//! [`ChildHookPanic`] is the typed-error handoff between the fallible cores
//! in `element_tree.rs` (`ElementTree::try_insert_with_provisional_order`,
//! `ElementTree::try_update`, and the two `GlobalKey` retakes) and the two
//! substituting primitives defined here —
//! [`ElementTree::mount_or_substitute`] / [`ElementTree::update_or_substitute`].
//! Neither `ChildHookPanic` nor its containment window ever crosses this
//! crate's boundary: the two primitives consume it, and the infallible
//! public `ElementTree::insert` / `insert_during_reconcile` / `update`
//! resume the unwind from its payload instead — unchanged behavior for
//! every caller that does not opt into substitution.

use std::any::Any;

use flui_foundation::ElementId;

use crate::owner::{LifecycleHook, RecoveredAt, RecoveredPanic};
use crate::view::View;
use crate::view::recovery_view_for;

use super::element_tree::{ElementTree, InsertedChild};
use super::{ProvisionalOrder, SubtreeRemoval};

/// A lifecycle-hook panic caught inside a per-child containment window —
/// the typed-error half of [`ElementTree::mount_or_substitute`] /
/// [`ElementTree::update_or_substitute`]'s undo-then-substitute machinery.
///
/// # The region a containment window covers
///
/// A fresh mount's window runs from `View::create_element()` through the
/// end of `mount` — every call in between can reach user code
/// (`View::create_element`, `RenderView::create_render_object`; `ViewState::
/// init_state` is NOT inside this window — it runs later, in the
/// `build_scope` drain). A `GlobalKey` retake's window is split in
/// two: `activate_subtree` alone, then (separately) the retake's own
/// `update`. Everything ELSE — the debug-only eager duplicate-`GlobalKey`
/// rejection, the retake preflight, and (between a retake's two windows)
/// the relocation bookkeeping (`recompute_subtree_ancestry`,
/// `synchronize_destination_render_children_for_relocation`,
/// `reset_ancestor_parent_data`, `attach_render_relocation`) — is framework
/// code, not user code, and propagates exactly as it always has. This is
/// what ADR-0050's eager duplicate-`GlobalKey` rejection relies on: it
/// panics from OUTSIDE any window, so it is never swallowed by a
/// containment catch.
pub(crate) struct ChildHookPanic {
    /// What the window had committed by the time the panic unwound through
    /// it, beyond the id the primitive already has, so the primitive knows
    /// what to undo before mounting the substitute. `None` in two cases: a
    /// fresh mount's `View::create_element()` itself panicked, before
    /// anything existed to undo; or the panic came from
    /// `ElementTree::update`'s window, whose caller already names the live
    /// element directly and has no separate insert disposition to consult.
    pub(crate) inserted: Option<InsertedChild>,
    /// Which lifecycle hook was running when the panic happened.
    pub(crate) hook: LifecycleHook,
    /// The caught panic payload, unconverted — the primitive builds the
    /// `FlutterError`/`RecoveredPanic` from it exactly once.
    pub(crate) payload: Box<dyn Any + Send>,
}

impl ElementTree {
    /// Mount `view` at `(parent, slot)`, substituting the registered
    /// `ErrorView` when a user lifecycle hook inside the containment window
    /// panics (issue #561 — see [`ChildHookPanic`]).
    ///
    /// On success this behaves exactly like [`insert`](Self::insert). On a
    /// caught panic it undoes whatever the window had committed (discards
    /// an unannounced mint via [`discard_unannounced`](Self::discard_unannounced),
    /// finalizes a reactivated retake via [`remove_subtree`](Self::remove_subtree)),
    /// records at most one [`RecoveredPanic`] through `owner` — skipped
    /// when `owner.take_hook_panic_recorded()` reports an inner seam
    /// already recorded this same panic under its own, more accurate
    /// identity — and mounts the substitute unbounded at the same
    /// `(parent, slot)` through the same insert path — a panicking
    /// substitute factory is deliberately left to propagate, naming a
    /// broken factory instead of hiding it.
    ///
    /// The pushed record's subject is a post-mortem identity: by the time
    /// a later drain reads it, the element it names has already been
    /// discarded or removed. For a panic before any element existed
    /// (`create_element` itself), the substitute mounted here is the only
    /// live handle a consumer can look at, and the record names it
    /// instead.
    ///
    /// Never writes `parent`'s `child_ids` — same contract as
    /// [`insert`](Self::insert); the caller (a sparse host today, the dense
    /// reconciler in a later change) owns that.
    ///
    /// `context` becomes the `FlutterError`/`RecoveredPanic` breadcrumb
    /// (e.g. `"mounting lazy sliver child 3"`) — never user data.
    #[must_use = "a substitute mount re-points whatever id the caller was tracking"]
    pub(crate) fn mount_or_substitute(
        &mut self,
        view: &dyn View,
        parent: ElementId,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
        order: ProvisionalOrder<'_>,
        context: impl Into<String>,
    ) -> ElementId {
        match self.try_insert_with_provisional_order(view, parent, slot, owner, order) {
            Ok(id) => id,
            Err(ChildHookPanic {
                inserted,
                hook,
                payload,
            }) => {
                let stranded = match inserted {
                    Some(InsertedChild::Minted(id)) => {
                        self.discard_unannounced(id, owner);
                        Some(id)
                    }
                    Some(InsertedChild::Retaken(id)) => {
                        self.remove_subtree(id, owner, SubtreeRemoval::Finalize);
                        Some(id)
                    }
                    None => None,
                };

                // Build the FlutterError ONCE: it renders the substitute
                // view below, and — when this window turns out to be the
                // one recording the panic — is reused for the pushed
                // record too (`RecoveredPanic::with_error`), never
                // re-derived from the payload a second time.
                let error = crate::view::FlutterError::from_panic(payload.as_ref(), context);
                let substitute_view = recovery_view_for(&error);
                // Deliberately unbounded: if the registered error-view
                // factory panics on mount too, there is nothing left to
                // substitute, and the crash names a broken factory instead
                // of hiding it.
                let substitute_id = self
                    .try_insert_with_provisional_order(
                        substitute_view.0.as_ref(),
                        parent,
                        slot,
                        owner,
                        order,
                    )
                    .unwrap_or_else(|substitute_panic| {
                        std::panic::resume_unwind(substitute_panic.payload)
                    });

                // An inner seam (e.g. `StatefulBehavior::on_activate`)
                // already recorded this exact panic under its own,
                // accurate identity and re-raised it — this window's job
                // is only the undo-and-substitute above, not a second,
                // coarser record.
                if !owner.take_hook_panic_recorded() {
                    let panic = RecoveredPanic::with_error(
                        RecoveredAt::Substituted {
                            element: stranded,
                            substitute: substitute_id,
                            parent,
                            slot,
                        },
                        view.view_type_id(),
                        hook,
                        payload.as_ref(),
                        error,
                    );
                    owner.push_recovered_panic(panic);
                }
                substitute_id
            }
        }
    }

    /// Update the element at `id` with `view`, substituting the registered
    /// `ErrorView` at its current `(parent, slot)` when the update's
    /// containment window (`try_update`) catches a panic (issue #561 — see
    /// [`ChildHookPanic`]).
    ///
    /// Returns `id` unchanged on success, or the substitute's id when it had
    /// to recover — the caller (a sparse host today, the dense reconciler in
    /// a later change) re-points whatever tracked `id` at the return value.
    ///
    /// On a caught panic the element is removed outright via
    /// [`remove_subtree`](Self::remove_subtree) under [`SubtreeRemoval::Finalize`]
    /// rather than kept: a half-applied `update_render_object` can leave the
    /// render object carrying part of a new configuration that nothing
    /// marked dirty, so a silently stale subtree is the one outcome worse
    /// than a visible error. At most one [`RecoveredPanic`] is pushed
    /// through `owner` (see [`Self::mount_or_substitute`]'s doc on the
    /// already-recorded skip); `context` becomes its `FlutterError`
    /// breadcrumb.
    #[must_use = "a substitute mount re-points whatever id the caller was tracking"]
    pub(crate) fn update_or_substitute(
        &mut self,
        id: ElementId,
        view: &dyn View,
        owner: &mut crate::ElementOwner<'_>,
        context: impl Into<String>,
    ) -> ElementId {
        match self.try_update(id, view, owner) {
            Ok(()) => id,
            Err(ChildHookPanic { hook, payload, .. }) => {
                // Read the current parent/slot BEFORE `remove_subtree`
                // wipes the node — there is nowhere else left to ask once
                // it is gone.
                let (parent, slot) = self
                    .get(id)
                    .map(|node| (node.parent(), node.slot()))
                    .expect(
                        "BUG: a node try_update just touched must still resolve before removal",
                    );
                let parent = parent.expect(
                    "BUG: update_or_substitute's target must be a parented child; the root \
                     never calls this primitive",
                );

                self.remove_subtree(id, owner, SubtreeRemoval::Finalize);

                let error = crate::view::FlutterError::from_panic(payload.as_ref(), context);
                let substitute_view = recovery_view_for(&error);
                let substitute_id = self
                    .try_insert_with_provisional_order(
                        substitute_view.0.as_ref(),
                        parent,
                        slot,
                        owner,
                        ProvisionalOrder::NONE,
                    )
                    .unwrap_or_else(|substitute_panic| {
                        std::panic::resume_unwind(substitute_panic.payload)
                    });

                if !owner.take_hook_panic_recorded() {
                    let panic = RecoveredPanic::with_error(
                        RecoveredAt::Substituted {
                            element: Some(id),
                            substitute: substitute_id,
                            parent,
                            slot,
                        },
                        view.view_type_id(),
                        hook,
                        payload.as_ref(),
                        error,
                    );
                    owner.push_recovered_panic(panic);
                }
                substitute_id
            }
        }
    }
}
