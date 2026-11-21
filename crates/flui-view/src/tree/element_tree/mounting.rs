//! Mounting: the root, child insertion and replacement, and the ancestor parent data a new child inherits.

use flui_foundation::{ElementId, ViewKey};
use flui_rendering::pipeline::PipelineCell;

use crate::owner::LifecycleHook;
use crate::view::{BoxedView, View};

use super::super::containment::{ChildHookPanic, contain_window};
use super::global_key_retake::{
    GlobalKeyRetake, RetakeDestination, global_key_of, register_global_key_with_collision_check,
    try_retake_global_key,
};
use super::{ElementNode, ElementTree, InheritedScope, SubtreeRemoval, compute_inherited_scope};

/// What an insert did with the child it was asked to produce, reported
/// *before* any user code runs so a caller that bounds mount panics knows
/// what there is to undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InsertedChild {
    /// A fresh node entered the slab at this id. Until `mount` returns it is
    /// announced nowhere — no `Mount` event, no `GlobalKey` registration, no
    /// entry in the parent's `child_ids` — so this report is the only handle
    /// to it.
    Minted(ElementId),
    /// An existing element was relocated here by a `GlobalKey` retake. It was
    /// already announced (as a `Reparent`), carries user state, and its own
    /// `update` — user `update_render_object` included — runs inside the
    /// retake.
    Retaken(ElementId),
}

/// The partial child order a reconcile has committed so far, which the
/// `GlobalKey` retake preflight consults to decide whether a candidate can be
/// relocated into this parent. Empty for an insert outside a reconcile.
#[derive(Clone, Copy)]
pub(crate) struct ProvisionalOrder<'a> {
    reconciled_prefix: &'a [ElementId],
    unclaimed_old_slots: &'a [Option<ElementId>],
}

impl<'a> ProvisionalOrder<'a> {
    /// No order committed yet — a fresh insert with nothing to preflight.
    pub(crate) const NONE: Self = Self {
        reconciled_prefix: &[],
        unclaimed_old_slots: &[],
    };

    /// The order committed so far by dense reconciliation.
    pub(in crate::tree) const fn during_reconcile(
        reconciled_prefix: &'a [ElementId],
        unclaimed_old_slots: &'a [Option<ElementId>],
    ) -> Self {
        Self {
            reconciled_prefix,
            unclaimed_old_slots,
        }
    }
}

impl ElementTree {
    /// Mount a View as the root of the tree.
    ///
    /// Returns the ElementId of the root element, which keeps `view`'s shared
    /// allocation as its configuration.
    ///
    /// Note: This method does NOT pass PipelineOwner to the element.
    /// For RenderObjectElements that need PipelineOwner, use
    /// `mount_root_with_pipeline_owner` instead.
    pub fn mount_root(
        &mut self,
        view: &BoxedView,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        self.mount_root_with_pipeline_owner(view, None, owner)
    }

    /// Mount a View as the element-tree slab root, optionally threading a
    /// [`PipelineCell`] into the root element before `mount`.
    ///
    /// This is the **low-level element-tree attach** primitive. It does **not**
    /// install [`PipelineOwner::root_id`](flui_rendering::PipelineOwner::root_id)
    /// for an ordinary render-producing view — only
    /// [`crate::view::RootRenderView`] / `RootRenderElement` owns that invariant.
    /// The UI runtime therefore wraps the user view in
    /// `RootRenderView` before calling this (`WidgetsBinding::attach_root_widget`).
    ///
    /// Callers that need a live render pipeline must use that
    /// root-bootstrap APIs (or an equivalent `RootRenderView` wrap). Direct
    /// use here remains appropriate for element-tree unit tests that never
    /// drive layout/paint from `root_id`.
    ///
    /// # Arguments
    ///
    /// * `view` - The root View to mount; its element keeps this shared
    ///   allocation as its configuration
    /// * `pipeline_owner` - Optional PipelineOwner for render tree management
    /// * `owner` - Split-borrow handle into the `BuildOwner`
    ///   ([`ElementOwner`](crate::ElementOwner)) threaded into the
    ///   element's `mount` call so `GlobalKey` registration / dirty
    ///   scheduling can take effect during initial mount.
    ///
    /// Returns the ElementId of the root element.
    pub fn mount_root_with_pipeline_owner(
        &mut self,
        view: &BoxedView,
        pipeline_owner: Option<PipelineCell>,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        let mut element = view.create_shared_element();

        // Pass the pipeline cell to the element BEFORE mounting.
        // This is critical for RenderObjectElements to create their RenderObjects
        if let Some(ref pipeline) = pipeline_owner {
            element.element_mut().set_pipeline_owner(pipeline.clone());
            tracing::debug!(
                "ElementTree::mount_root_with_pipeline_owner: passed PipelineCell to root element"
            );
        }

        let mut node = ElementNode::new(element, None, 0);
        // FR-022: store the cloned `View::key()` on the
        // node so the keyed reconciler can index by it without
        // crossing the typed-`V` boundary.
        node.set_key(view.key().map(ViewKey::clone_key));

        let slab_index = self.nodes.insert(node);
        // Mint the generational id from the slot's current generation
        // (fresh slot → gen 1; reused slot → the bumped post-free value).
        let id = self.alloc_id(slab_index);

        // Stamp the element with its own ElementId BEFORE
        // `mount` so the Variable-arity reconciler can read it back
        // when emitting ReconcileEvent's `parent` field.
        self.nodes[slab_index].element_mut().set_self_id(id);

        // Root inherited scope: empty parent map, plus self if the root is
        // itself a provider. Set before `mount` (see `insert`).
        self.nodes[slab_index].inherited = {
            let empty = InheritedScope::EMPTY;
            let node = &self.nodes[slab_index];
            compute_inherited_scope(&empty, node.element(), id)
        };

        // Mount the element (now it has PipelineOwner set)
        self.nodes[slab_index].element_mut().mount(None, 0, owner);

        // Register GlobalKey on mount. The root element's view is
        // queried here because the dispatch boundary at `Element::mount`
        // can't read the typed `View::key()` (V isn't bounded by View
        // there). Doing the check here keeps the wiring at the level
        // where both `view: &dyn View` and `id` are simultaneously in
        // scope.
        //
        // No reservation is recorded here: a reservation names the PARENT
        // that declared the key, and a root has none.
        if let Some(key) = global_key_of(view) {
            register_global_key_with_collision_check(owner, key, id);
            self.nodes[slab_index].registered_global_key = Some(key.clone_key());
        }

        // ADR-0040: a fresh root was minted and mounted.
        crate::owner::emit_observation(owner.tree_observer, |o| {
            o.element_mounted(&flui_foundation::observe::ElementMounted::new(
                id,
                None,
                0,
                view.view_type_id(),
            ));
        });

        self.root = Some(id);
        id
    }

    /// Insert a new element as a child of the given parent.
    ///
    /// Returns the ElementId of the new element, which keeps `view`'s shared
    /// allocation as its configuration.
    ///
    /// The split-borrow `owner` handle is threaded into the new
    /// element's `mount` call.
    ///
    /// # GlobalKey state migration
    ///
    /// If `view` carries a `GlobalKey` that is already registered
    /// to an element AND that element is currently in the inactive
    /// queue (from a prior soft-remove this frame), the inactive
    /// element is pulled back to the new parent/slot instead of a
    /// fresh element being created. Its `ElementId` and persistent
    /// state survive.
    pub fn insert(
        &mut self,
        view: &BoxedView,
        parent: ElementId,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        self.try_insert_with_provisional_order(view, parent, slot, owner, ProvisionalOrder::NONE)
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic.payload))
    }

    /// Finalize one child subtree and mount a keyless replacement in its exact slot.
    ///
    /// Validation completes before mutation. The old parent-child
    /// `GlobalKey` reservation is then withdrawn, the old subtree is finalized,
    /// and the replacement is inserted through the unbounded insertion path.
    /// Only after insertion succeeds is the parent's child-id slot committed,
    /// render reordering armed and notified, and the replacement scheduled for
    /// its initial build.
    ///
    /// A panic from replacement creation or mounting propagates after the old
    /// subtree has been removed but before the child-id slot, render reorder, or
    /// build schedule is committed. That failure is fatal and has no rollback;
    /// callers must not continue using the partially advanced tree.
    ///
    /// # Panics
    ///
    /// Panics with a `BUG:` invariant failure before mutation if the replacement
    /// is keyed, the parent or old child is missing, the slot does not exist, or
    /// the old child's reverse parent/slot edge disagrees. Also propagates any
    /// panic raised while creating or mounting the replacement, with the
    /// post-removal failure state described above.
    pub(crate) fn replace_child_with(
        &mut self,
        parent: ElementId,
        slot: usize,
        view: &BoxedView,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        assert!(
            view.key().is_none(),
            "BUG: a failure replacement must be keyless",
        );
        let (old_child, parent_depth, render_notification) = {
            let parent_node = self
                .get(parent)
                .expect("BUG: a failure replacement parent must remain live");
            let old_child = *parent_node
                .child_ids()
                .get(slot)
                .expect("BUG: a failure replacement slot must exist on its parent");
            let parent_element = parent_node.element();
            let render_notification = parent_element.render_id().map(|render_id| {
                let pipeline = parent_element
                    .pipeline_owner()
                    .expect("BUG: an active render replacement parent must have a PipelineOwner");
                (pipeline, render_id)
            });
            (old_child, parent_node.depth(), render_notification)
        };
        let old_node = self
            .get(old_child)
            .expect("BUG: a failure replacement slot must name a live child");
        assert!(
            old_node.parent() == Some(parent) && old_node.slot() == slot,
            "BUG: a replacement child's reverse edge must match its containing parent and slot",
        );

        owner.forget_global_key_reservation(parent, old_child);
        self.remove_subtree(old_child, owner, SubtreeRemoval::Finalize);
        let replacement = self.insert(view, parent, slot, owner);
        self.get_mut(parent)
            .expect("BUG: a replacement parent must remain live through child mount")
            .commit_child_replacement(slot, old_child, replacement);
        self.mark_render_reorder_needed();
        if let Some((pipeline, render_id)) = render_notification {
            pipeline.with_mut(|owner| owner.note_render_children_reordered(render_id));
        }
        owner.schedule_build_for(
            replacement,
            parent_depth + 1,
            crate::RebuildReason::InitialMount,
        );
        replacement
    }

    /// [`insert`](Self::insert)'s fallible core: a caught panic comes back as `Err` instead of
    /// unwinding through this call, so a bounding caller ([`Self::mount_or_substitute`])
    /// can undo whatever the containment window committed before deciding
    /// what to do next.
    ///
    /// # The containment window
    ///
    /// A fresh mount's window runs from `View::create_element()` through the
    /// end of `mount` — every call in between can reach user code
    /// (`View::create_element`, `RenderView::create_render_object`).
    /// `ViewState::init_state` is NOT inside this window: it runs later, in
    /// the `build_scope` drain, not during `mount`. A `GlobalKey`
    /// retake's window is reported by `retake_inactive_global_key` /
    /// `retake_active_global_key` themselves, which run `activate_subtree`
    /// and the retake's own `update` under their own catch and hand back a
    /// `Result` here. Framework code before `create_element` (the retake
    /// preflight below) and after `mount` returns (`GlobalKey` registration,
    /// the `Mount` event, the observer, ancestor parent-data, the
    /// render-reorder flag) stays outside every window and propagates
    /// exactly as it always has — see [`ChildHookPanic`]'s doc for the full
    /// rule.
    pub(in crate::tree) fn try_insert_with_provisional_order(
        &mut self,
        view: &BoxedView,
        parent: ElementId,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
        order: ProvisionalOrder<'_>,
    ) -> Result<ElementId, ChildHookPanic> {
        let ProvisionalOrder {
            reconciled_prefix,
            unclaimed_old_slots,
        } = order;
        // ADV-1 state migration. Before creating a fresh element,
        // check whether `view` has a `GlobalKey` that points at an
        // existing element. If it is inactive, pull it back; if it is still
        // active under a different parent, forget it from that parent and move
        // it here. In both cases the `ElementId` and state survive.
        if let Some(key) = global_key_of(view) {
            let destination = RetakeDestination {
                parent,
                slot,
                reconciled_prefix,
                unclaimed_old_slots,
            };
            match try_retake_global_key(self, owner, key, view, destination) {
                GlobalKeyRetake::Absent => {}
                GlobalKeyRetake::Retaken(retaken_id) => {
                    // A parent declaring a keyed child has made a claim on
                    // that key for this frame, whether the element was
                    // grafted or freshly mounted. The frame boundary reads
                    // the claims back to tell a legal reparent (one
                    // claimant) from a duplicate (two).
                    owner.reserve_global_key(parent, retaken_id, key);
                    return Ok(retaken_id);
                }
                GlobalKeyRetake::RetakePanicked(panic) => {
                    // The retake's own containment window already committed
                    // the relocation and reported it — the caller undoes
                    // THAT element, not a fresh mint this call never made.
                    return Err(panic);
                }
                GlobalKeyRetake::Rejected => {
                    // The candidate exists but could not be relocated (it is
                    // not idle, has no render frontier to move, or lives in
                    // another pipeline). Fall through to a fresh mount rather
                    // than panicking here: a second live element for one key
                    // is exactly the condition the reservation ledger below
                    // records, and the frame boundary reports it with the
                    // claimants named. Panicking at this point would turn a
                    // reportable duplicate into a crash and would lose that
                    // diagnostic entirely.
                    tracing::debug!(
                        key = ?key,
                        ?parent,
                        "GlobalKey candidate failed the relocation preflight; mounting fresh and \
                         leaving the duplicate to the frame boundary"
                    );
                }
            }
        }

        // Fresh mount. `inserted` is written the instant the node exists in
        // the slab, before `mount` runs, so a panic anywhere in the window
        // below still leaves the caller a report of what to undo — and it
        // is a plain local, not an out-parameter: a write to it survives the
        // unwind that follows, the same way the out-parameter it replaces
        // always did.
        let mut inserted: Option<InsertedChild> = None;
        let mount_result = contain_window(|| -> ElementId {
            let mut element = view.create_shared_element();

            // Read the parent's render-tree propagation context in ONE fresh,
            // immediately-dropped `&ElementTree` borrow, then apply it to the
            // freshly-created child BEFORE mount.
            //
            // E3 (atomic box→arena swap): the old box graph propagated the
            // `PipelineOwner` + parent `RenderId` to children inside
            // `update_or_create_child(ren)`, *before* `mount_children`, because
            // `RenderBehavior::on_mount` creates its `RenderObject` only when a
            // `PipelineOwner` is already in scope. Children are now
            // slab-resident, so that propagate-before-mount ordering moves
            // here: read `pipeline_owner()` / `child_render_id()` off the
            // parent node, hand them to the child, then mount.
            let (
                parent_depth,
                parent_owner,
                child_parent_render_id,
                child_sliver_slot,
                parent_inherited,
            ) = match self.get(parent) {
                Some(node) => (
                    node.depth(),
                    node.element().pipeline_owner(),
                    node.element().child_render_id(),
                    node.element().child_sliver_slot(slot),
                    node.inherited.clone(),
                ),
                None => (0, None, None, None, InheritedScope::EMPTY),
            };

            if let Some(pipeline_owner) = parent_owner {
                element.element_mut().set_pipeline_owner(pipeline_owner);
            }
            element
                .element_mut()
                .set_parent_render_id(child_parent_render_id);
            element.element_mut().set_sliver_slot(child_sliver_slot);

            let mut node = ElementNode::new(element, Some(parent), slot);
            node.set_depth(parent_depth + 1);
            // FR-022.
            node.set_key(view.key().map(ViewKey::clone_key));

            let slab_index = self.nodes.insert(node);
            let id = self.alloc_id(slab_index);

            // Same self-id stamping as mount_root.
            self.nodes[slab_index].element_mut().set_self_id(id);

            // Report before `mount` — everything from here on can panic through
            // user code (`View::create_render_object`), and this is the only
            // handle to the node that would otherwise be stranded.
            inserted = Some(InsertedChild::Minted(id));

            // Resolve this child's inherited scope from the parent's now that the
            // element knows whether it is itself a provider (`as_inherited`) and
            // its `view_type_id`. Computed before `mount` so an
            // `InheritedBehavior::on_mount` (or any mount-time lookup) already sees
            // its own scope.
            self.nodes[slab_index].inherited = {
                let node = &self.nodes[slab_index];
                compute_inherited_scope(&parent_inherited, node.element(), id)
            };

            // Mount the element (PipelineOwner + parent RenderId already set,
            // so `RenderBehavior::on_mount` can create its RenderObject).
            self.nodes[slab_index]
                .element_mut()
                .mount(Some(parent), slot, owner);

            id
        });

        let id = match mount_result {
            Ok(id) => id,
            Err(payload) => {
                return Err(ChildHookPanic {
                    inserted,
                    hook: LifecycleHook::Mount,
                    staged: None,
                    payload,
                });
            }
        };

        // Register the GlobalKey → id mapping, and record the declaration
        // for the frame boundary's duplicate check.
        if let Some(key) = global_key_of(view) {
            register_global_key_with_collision_check(owner, key, id);
            if let Some(node) = self.get_mut(id) {
                node.registered_global_key = Some(key.clone_key());
            }
            owner.reserve_global_key(parent, id, key);
        }

        // A fresh child node appeared at (parent, slot). Emit `Mount` HERE —
        // `insert` is the single site that mints child nodes, and the
        // GlobalKey-retake path above already emitted `Reparent` and returned
        // early, so the two dispositions can never double-fire for one
        // new-side view (the reconciler must NOT also emit `Mount`).
        super::super::reconcile_event::emit(&super::super::reconcile_event::ReconcileEvent::mount(
            parent,
            slot,
            view.view_type_id(),
            view.key().map(ViewKey::key_hash),
        ));

        // ADR-0040: same single-mint funnel as the tracing Mount above — the
        // GlobalKey-retake early-return can never reach this point, so a
        // retake never double-fires as a mount.
        crate::owner::emit_observation(owner.tree_observer, |o| {
            o.element_mounted(&flui_foundation::observe::ElementMounted::new(
                id,
                Some(parent),
                slot,
                view.view_type_id(),
            ));
        });

        // A freshly-attached render child reads the parent-data its nearest
        // ancestor `ParentDataView` (`Expanded`, `Positioned`) contributes.
        self.apply_ancestor_parent_data(id);

        // A render-bearing child appended itself to its render parent in
        // *attach* order, which only matches element-slot order when no
        // component ancestor deferred its build. Flag a post-build reorder so
        // the render children settle into slot order regardless.
        if self
            .get(id)
            .is_some_and(|node| node.element().render_id().is_some())
        {
            self.needs_render_reorder = true;
        }

        Ok(id)
    }

    /// Install or update the nearest ancestor `ParentDataView`'s typed
    /// configuration on `child_id`'s render node.
    ///
    /// No-op unless `child_id` owns a render node. Walks strictly upward from
    /// the child, taking the *nearest* `parent_data_config()` and stopping the
    /// search at the first ancestor render object (the render parent that reads
    /// the data during layout). The nearest config wins.
    /// Existing data is mutated through the typed hook so layout-owned fields
    /// survive; its returned impact controls the render-parent dirty work.
    ///
    /// Before writing, the provider's parent-data [`TypeId`](std::any::TypeId) is
    /// checked against the render parent's
    /// [`child_parent_data_type_id`](flui_rendering::RenderObject::child_parent_data_type_id).
    /// A mismatch panics with a composition diagnostic (offending
    /// `ParentDataView`, typical ancestor family, actual render parent, element
    /// ancestry) — the same failure in debug and release — rather than reaching
    /// layout's `RawBoxLayoutContext::from_erased` TypeId assert.
    ///
    /// Average case O(1) — for a plain render child the very first ancestor is
    /// the render parent, so the walk stops in one hop and never touches the
    /// pipeline owner. Worst case O(proxy-nesting depth) between the render
    /// child and its render parent.
    ///
    /// The element tree and render tree are disjoint stores. The typed element
    /// hook reads widget configuration while the pipeline checkout mutates only
    /// the render node.
    pub(super) fn apply_ancestor_parent_data(&mut self, child_id: ElementId) {
        let Some(child) = self.get(child_id) else {
            return;
        };
        let Some(child_render_id) = child.element().render_id() else {
            return;
        };
        let pipeline_owner = child.element().pipeline_owner();
        let mut cursor = child.parent();

        let mut nearest_parent_data_element: Option<ElementId> = None;
        let mut parent_render_id: Option<flui_foundation::RenderId> = None;
        while let Some(ancestor_id) = cursor {
            let Some(node) = self.get(ancestor_id) else {
                break;
            };
            if let Some(render_id) = node.element().render_id() {
                parent_render_id = Some(render_id);
                break;
            }
            if nearest_parent_data_element.is_none()
                && node.element().parent_data_type_id().is_some()
            {
                nearest_parent_data_element = Some(ancestor_id);
            }
            cursor = node.parent();
        }

        // Nothing to apply unless a ParentDataView sits between this render
        // child and its render parent.
        let Some(parent_data_element_id) = nearest_parent_data_element else {
            return;
        };
        let Some(pipeline_owner) = pipeline_owner else {
            return;
        };

        let (provided_type_id, provider_name, typical_ancestor, provided_pd_name) = {
            let element = self
                .get(parent_data_element_id)
                .expect("BUG: located ParentDataView element must remain live")
                .element();
            (
                element
                    .parent_data_type_id()
                    .expect("BUG: located ParentDataView must report a parent-data TypeId"),
                element
                    .parent_data_debug_type_name()
                    .unwrap_or("ParentDataView"),
                element
                    .parent_data_typical_ancestor_description()
                    .unwrap_or("a render parent whose children use this parent-data type"),
                element
                    .parent_data_storage_type_name()
                    .unwrap_or("unknown parent-data type"),
            )
        };

        // The element-tree borrow and render-tree checkout are disjoint.
        // Ancestry is formatted only inside the assert failure path — not on
        // every successful ParentDataView attach/update.
        pipeline_owner.with_mut(|owner| {
            if let Some(parent_render_id) = parent_render_id
                && let Some(parent_node) = owner.render_tree().get(parent_render_id)
            {
                assert!(
                    provided_type_id == parent_node.child_parent_data_type_id(),
                    "Incorrect use of ParentDataView `{provider_name}`: it contributes \
                     `{provided_pd_name}` but its nearest render parent is `{}` \
                     (incompatible child parent-data TypeId). `{provider_name}` must be \
                     placed under {typical_ancestor}. Element ancestry \
                     (child → … → root): {}",
                    parent_node.kind(),
                    self.format_element_ancestry(child_id),
                );
            }

            let parent_data_element = self
                .get(parent_data_element_id)
                .expect("BUG: located ParentDataView element must remain live");
            // The owner marks the child's render parent, the node that reads
            // its parent data, with the impact the update reports.
            let _ = owner.update_parent_data(child_render_id, |slot| {
                if let Some(parent_data) = slot.get_mut() {
                    parent_data_element
                        .element()
                        .apply_parent_data_config(parent_data)
                } else {
                    let config = parent_data_element
                        .element()
                        .parent_data_config()
                        .expect("BUG: located ParentDataView must create parent data");
                    slot.set(config);
                    flui_rendering::RenderUpdateImpact::NONE
                }
            });
        });
    }

    /// Child → … → root labels for parent-data mismatch diagnostics.
    fn format_element_ancestry(&self, from: ElementId) -> String {
        let mut parts = Vec::new();
        let mut cursor = Some(from);
        while let Some(id) = cursor {
            let Some(node) = self.get(id) else {
                break;
            };
            let label = node
                .element()
                .parent_data_debug_type_name()
                .unwrap_or("Element");
            parts.push(format!("{label}#{id}"));
            cursor = node.parent();
        }
        parts.join(" → ")
    }

    pub(super) fn reset_ancestor_parent_data(&mut self, child_id: ElementId) {
        let Some(child) = self.get(child_id) else {
            return;
        };
        let Some(render_id) = child.element().render_id() else {
            return;
        };
        if let Some(pipeline) = child.element().pipeline_owner() {
            pipeline.with_mut(|owner| {
                let _ = owner.update_parent_data(render_id, |slot| {
                    let _ = slot.clear();
                    flui_rendering::RenderUpdateImpact::NONE
                });
            });
        }
        self.apply_ancestor_parent_data(child_id);
    }
}
