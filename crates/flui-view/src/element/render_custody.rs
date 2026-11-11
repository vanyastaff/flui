//! An element's custody of one render object: its creation and adoption at
//! mount, the configuration pushed into it at each rebuild, and its removal.
//!
//! Every behavior that owns a render object goes through these three
//! functions, so the adoption order, the panic containment and the removal
//! are written once. What differs between behaviors is only who answers the
//! hooks: a [`RenderView`](crate::RenderView) itself, or the state of a
//! [`StatefulRenderView`](crate::StatefulRenderView).

use std::any::{Any, TypeId};
use std::panic::AssertUnwindSafe;

use flui_foundation::RenderId;
use flui_rendering::{
    RenderUpdateImpact, pipeline::PipelineOwner, protocol::Protocol,
    traits::RenderObject as RenderObjectTrait,
};

use super::{arity::ElementArity, generic::ElementCore};
use crate::{RenderObjectContext, owner::LifecycleHook};

fn insert_render_object<R, P>(render_object: R, owner: &mut PipelineOwner) -> RenderId
where
    R: RenderObjectTrait<P>,
    P: Protocol,
    flui_rendering::storage::RenderNode: From<Box<dyn RenderObjectTrait<P>>>,
{
    owner.insert(Box::new(render_object))
}

/// Creates the element's render object with `create`, inserts it into the
/// render tree and adopts it under the element's render parent.
///
/// `slot` is the behavior's render id. It is written before adoption and the
/// sliver stamp can panic, so the undo path of a panicking mount
/// (`ElementTree::discard_unannounced`, then the behavior's `on_unmount`)
/// finds and removes the node it already inserted.
///
/// An element mounted with no `PipelineOwner` in scope (a render-less unit
/// test tree) creates nothing; [`push`] and [`unmount`] tolerate that state.
pub(crate) fn mount<V, A, R, P>(
    slot: &mut Option<RenderId>,
    core: &ElementCore<V, A>,
    owner: &mut crate::ElementOwner<'_>,
    create: impl FnOnce(&RenderObjectContext<'_>) -> R,
) where
    V: Clone + 'static,
    A: ElementArity,
    R: RenderObjectTrait<P>,
    P: Protocol,
    flui_rendering::storage::RenderNode: From<Box<dyn RenderObjectTrait<P>>>,
{
    let Some(pipeline_owner) = core.pipeline_owner() else {
        tracing::warn!("a render element mounted without a PipelineOwner");
        return;
    };
    let ctx = RenderObjectContext::new(owner.interaction_dispatch.as_ref(), Some(owner.reactive));
    let render_object = create(&ctx);

    let sliver_slot = core.sliver_slot();
    pipeline_owner.with_mut(|pipeline_owner| {
        let render_id = insert_render_object(render_object, pipeline_owner);
        *slot = Some(render_id);

        // The node reports where its view was created when an error names it.
        if let Some(node) = pipeline_owner.render_tree_mut().get_mut(render_id) {
            node.set_creation_site(core.creation_site());
        }

        // `adopt_child` writes the child's parent link and the parent's
        // child-list entry in one call, so the two directions are never
        // written independently (see `RenderTree::adopt_child`).
        if let Some(parent_id) = core.parent_render_id() {
            pipeline_owner.adopt_render_child(parent_id, render_id);
        } else if let Some(element_parent) = core.element_parent() {
            // A render element under an element-tree parent with an active
            // PipelineOwner must have a render parent in the chain. Without
            // one the render object sits in the render tree with no parent
            // link: present in render space, invisible to layout and paint.
            // The legal bare mount (the element-tree root) has no element
            // parent and stays silent; `mount_bootstrap.rs` pins it.
            tracing::error!(
                element_id = ?core.self_id(),
                ?element_parent,
                ?render_id,
                "BUG: a render element mounted under an element-tree parent \
                 with an active PipelineOwner has no render parent — \
                 adoption would orphan the render object"
            );
            debug_assert!(
                core.parent_render_id().is_some(),
                "BUG: render element {:?} mounted under element-tree parent \
                 {:?} with an active PipelineOwner but no render ancestor in \
                 the chain — a render element mounted under an active \
                 PipelineOwner must have a render parent; adoption would \
                 orphan the render object",
                core.self_id(),
                element_parent,
            );
        }

        // The inherited sliver slot reaches this render object through
        // however many composite elements sit between it and the sparse
        // sliver host, so the first render descendant of an item carries its
        // logical index the moment it attaches; the sliver rebuilds its
        // `logical -> dense slot` map from this parent data on every layout.
        if let Some(sliver_slot) = sliver_slot {
            super::behavior_commons::stamp_sliver_slot(pipeline_owner, render_id, sliver_slot);
        }
    });
    tracing::debug!(render_id = ?*slot, "render element created its render object");
}

/// Hands the live render object to `update` and applies the impact it
/// returns.
///
/// Inside a `build_scope` drain the context carries the borrowed tree, so
/// `RenderObjectContext::depend_on` resolves and records; outside one it is
/// tree-less.
///
/// `update` is user code. It runs under `catch_unwind` inside the `with_mut`
/// borrow that lends it the render object, and the unwind resumes only after
/// that borrow is released. A caught panic is staged through the armed
/// lifecycle handoff, so the `build_scope` seam replaces the element with the
/// registered `ErrorView` (ADR-0287) and the half-applied render object goes
/// with it; the impact of a failed update is not applied. With no handoff
/// armed the unwind reaches the caller's own containment window.
///
/// With neither a render id nor a `PipelineOwner` there is nothing to update
/// (see [`mount`]). Every other half-state is a broken invariant.
pub(crate) fn push<V, A, R>(
    render_id: Option<RenderId>,
    core: &ElementCore<V, A>,
    owner: &mut crate::ElementOwner<'_>,
    update: impl FnOnce(&RenderObjectContext<'_>, &mut R) -> RenderUpdateImpact,
) where
    V: Clone + 'static,
    A: ElementArity,
    R: 'static,
{
    let (render_id, pipeline_owner) = match (render_id, core.pipeline_owner()) {
        (Some(render_id), Some(owner)) => (render_id, owner),
        (None, None) => {
            tracing::trace!(
                "render configuration push skipped: mounted without a PipelineOwner, \
                 no render object to update"
            );
            return;
        }
        (None, Some(_)) => {
            panic!("BUG: a render element with a PipelineOwner must own a RenderId during update")
        }
        (Some(_), None) => {
            panic!("BUG: an active render element must have a PipelineOwner during update")
        }
    };
    let ctx = match (owner.build_view, core.self_id()) {
        (Some(handle), Some(element_id)) => RenderObjectContext::for_rebuild(
            owner.interaction_dispatch.as_ref(),
            Some(owner.reactive),
            handle.tree,
            element_id,
            core.depth(),
            handle.dep_sink,
        ),
        _ => RenderObjectContext::new(owner.interaction_dispatch.as_ref(), Some(owner.reactive)),
    };
    let outcome = pipeline_owner.with_mut(|pipeline_owner| {
        let node = pipeline_owner
            .render_tree_mut()
            .get_mut(render_id)
            .expect("BUG: a render element's RenderId must resolve to a live node during update");
        let render_object = node
            .downcast_render_object_mut::<R>()
            .expect("BUG: a render element's live node must hold its render object type");
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: an update_render_object panic replaces the element with an ErrorView"
        )]
        std::panic::catch_unwind(AssertUnwindSafe(|| update(&ctx, render_object)))
    });
    let impact = match outcome {
        Ok(impact) => impact,
        Err(payload) => {
            owner.record_armed_lifecycle_panic(
                core.self_id(),
                core.creation_site(),
                TypeId::of::<V>(),
                LifecycleHook::Update,
                payload.as_ref(),
                "updating the render object of RenderElement",
            );
            std::panic::resume_unwind(payload);
        }
    };
    pipeline_owner.with_mut(|pipeline_owner| {
        pipeline_owner.apply_render_update_impact(render_id, impact);
    });
}

/// Runs `release` on the render object, then removes it and its subtree from
/// the render tree and clears `slot`.
///
/// `release` is user code and runs contained: the catch sits inside the
/// `with_mut` closure, because the render object it lends exists only behind
/// that borrow, and the panic is recorded after the borrow is released
/// (`PipelineCell::with_mut` is not re-entrant). The removal runs whether or
/// not `release` panicked.
pub(crate) fn unmount<V, A, R>(
    slot: &mut Option<RenderId>,
    core: &ElementCore<V, A>,
    owner: &mut crate::ElementOwner<'_>,
    behavior_name: &'static str,
    release: impl FnOnce(&RenderObjectContext<'_>, &mut R),
) where
    V: Clone + 'static,
    A: ElementArity,
    R: 'static,
{
    let mut recovered: Option<Box<dyn Any + Send>> = None;
    if let Some(render_id) = *slot
        && let Some(pipeline_owner) = core.pipeline_owner()
    {
        let ctx =
            RenderObjectContext::new(owner.interaction_dispatch.as_ref(), Some(owner.reactive));
        pipeline_owner.with_mut(|pipeline_owner| {
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: the unmount hook runs contained; the element still unmounts"
            )]
            if let Some(render_object) = pipeline_owner
                .render_tree_mut()
                .get_mut(render_id)
                .and_then(|node| node.downcast_render_object_mut::<R>())
                && let Err(payload) =
                    std::panic::catch_unwind(AssertUnwindSafe(|| release(&ctx, render_object)))
            {
                recovered = Some(payload);
            }
        });
    }
    if let Some(payload) = recovered {
        owner.record_hook_panic(
            core.self_id(),
            core.creation_site(),
            TypeId::of::<V>(),
            LifecycleHook::UnmountRenderObject,
            payload.as_ref(),
            "unmounting the render object of RenderElement",
        );
        flui_foundation::panic::discard_payload(payload);
    }
    super::behavior_commons::remove_render_object_from_tree(core, *slot, behavior_name);
    *slot = None;
}
