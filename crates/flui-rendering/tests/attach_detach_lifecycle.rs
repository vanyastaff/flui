//! ADR-0013 Slice B milestone — the `attach`/`detach` tree-lifecycle hook.
//!
//! Proves the pipeline actually fires `RenderObject::attach`/`detach` at
//! insert/remove, that the handed-over `RepaintHandle` is bound to the
//! right node and drives a REAL re-layout on the very next frame, and
//! that a handle captured before removal degrades to a silent no-op
//! afterward — the generational-staleness guarantee `RepaintHandle`
//! documents. Reparenting in this codebase has no dedicated API (no
//! `move_child`/`adopt_child`); it is remove-then-insert, so that case is
//! exercised the same way a real reparent would hit it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use flui_rendering::pipeline::{PipelineOwner, RepaintHandle};
use flui_rendering::prelude::*;
use flui_rendering::traits::RenderSliver;
use flui_tree::Leaf;
use flui_types::geometry::px;

type BoxedRenderObject = Box<dyn flui_rendering::traits::RenderObject<BoxProtocol>>;
type BoxedSliverObject = Box<dyn flui_rendering::traits::RenderObject<SliverProtocol>>;

// ────────────────────────────────────────────────────────────────────────
// Probe: a leaf RenderBox that records attach/detach/perform_layout calls
// ────────────────────────────────────────────────────────────────────────

/// Shared bookkeeping a [`LifecycleProbe`] writes into on
/// `attach`/`detach`/`perform_layout`, read back by the test after the
/// pipeline call returns.
#[derive(Clone, Default, Debug)]
struct LifecycleLog {
    attach_count: Arc<AtomicUsize>,
    detach_count: Arc<AtomicUsize>,
    layout_count: Arc<AtomicUsize>,
    captured_handle: Arc<Mutex<Option<RepaintHandle>>>,
}

impl LifecycleLog {
    fn attach_count(&self) -> usize {
        self.attach_count.load(Ordering::SeqCst)
    }

    fn detach_count(&self) -> usize {
        self.detach_count.load(Ordering::SeqCst)
    }

    fn layout_count(&self) -> usize {
        self.layout_count.load(Ordering::SeqCst)
    }

    /// The most recently captured handle. Panics if `attach` never fired —
    /// every test here calls it only after asserting `attach_count() > 0`.
    fn captured_handle(&self) -> RepaintHandle {
        self.captured_handle
            .lock()
            .expect("lock poisoned")
            .clone()
            .expect("attach must have captured a handle before this call")
    }
}

/// A leaf `RenderBox` whose only job is to prove the tree-lifecycle hook
/// fires and hands over a working handle — real render objects (e.g. the
/// future `RenderAnimatedSize`) hold a `Listenable` here instead of a log.
#[derive(Debug)]
struct LifecycleProbe {
    log: LifecycleLog,
    size: Size,
}

impl flui_foundation::Diagnosticable for LifecycleProbe {}

impl RenderBox for LifecycleProbe {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        self.log.layout_count.fetch_add(1, Ordering::SeqCst);
        self.size
    }

    fn attach(&mut self, handle: RepaintHandle) {
        self.log.attach_count.fetch_add(1, Ordering::SeqCst);
        *self.log.captured_handle.lock().expect("lock poisoned") = Some(handle);
    }

    fn detach(&mut self) {
        self.log.detach_count.fetch_add(1, Ordering::SeqCst);
    }
}

fn probe(log: LifecycleLog) -> BoxedRenderObject {
    Box::new(LifecycleProbe {
        log,
        size: Size::new(px(40.0), px(40.0)),
    }) as BoxedRenderObject
}

// ────────────────────────────────────────────────────────────────────────
// Probe: a leaf RenderSliver that records attach/detach/perform_layout
// calls — the Sliver-protocol counterpart of `LifecycleProbe`, proving the
// ADR-0013 lifecycle hook fires for Sliver children too (it did not,
// before this fix: no insertion path called `attach` for a Sliver child).
// ────────────────────────────────────────────────────────────────────────

/// A leaf `RenderSliver` whose only job is to prove the tree-lifecycle hook
/// fires and hands over a working handle — mirrors [`LifecycleProbe`] but
/// returns [`SliverGeometry`] instead of [`Size`].
#[derive(Debug)]
struct LifecycleProbeSliver {
    log: LifecycleLog,
}

impl flui_foundation::Diagnosticable for LifecycleProbeSliver {}

impl RenderSliver for LifecycleProbeSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.log.layout_count.fetch_add(1, Ordering::SeqCst);
        let paint = 20.0_f32.min(ctx.constraints().remaining_paint_extent);
        SliverGeometry {
            scroll_extent: 20.0,
            paint_extent: paint,
            layout_extent: paint,
            max_paint_extent: 20.0,
            hit_test_extent: paint,
            visible: paint > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    fn attach(&mut self, handle: RepaintHandle) {
        self.log.attach_count.fetch_add(1, Ordering::SeqCst);
        *self.log.captured_handle.lock().expect("lock poisoned") = Some(handle);
    }

    fn detach(&mut self) {
        self.log.detach_count.fetch_add(1, Ordering::SeqCst);
    }
}

fn sliver_probe(log: LifecycleLog) -> BoxedSliverObject {
    Box::new(LifecycleProbeSliver { log }) as BoxedSliverObject
}

/// Mounts a probe as the pipeline's root with tight constraints, ready to
/// drive `run_frame`.
fn rooted_fixture() -> (PipelineOwner, flui_foundation::RenderId, LifecycleLog) {
    let mut owner = PipelineOwner::new();
    let log = LifecycleLog::default();
    let id = owner.insert(probe(log.clone()));
    owner.set_root_id(Some(id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(40.0), px(40.0)))));
    (owner, id, log)
}

fn frame(owner: PipelineOwner) -> PipelineOwner {
    let (owner, result) = owner.run_frame();
    result.expect("frame must not error");
    owner
}

// ────────────────────────────────────────────────────────────────────────
// attach on insert
// ────────────────────────────────────────────────────────────────────────

#[test]
fn insert_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id() {
    let mut owner = PipelineOwner::new();
    let log = LifecycleLog::default();

    let id = owner.insert(probe(log.clone()));

    assert_eq!(
        log.attach_count(),
        1,
        "insert must call attach exactly once"
    );
    let handle = log.captured_handle();
    assert_eq!(
        handle.id(),
        id,
        "the handed-over handle must be bound to the freshly-inserted node"
    );
}

// ────────────────────────────────────────────────────────────────────────
// mark_needs_layout from the captured handle reaches perform_layout
// ────────────────────────────────────────────────────────────────────────

#[test]
fn captured_handle_mark_needs_layout_relayouts_that_node_next_frame() {
    let (owner, _id, log) = rooted_fixture();

    let owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        1,
        "the first frame lays the node out once"
    );

    let owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        1,
        "a clean tree must not re-layout on an idle frame"
    );

    let handle = log.captured_handle();
    handle
        .mark_needs_layout()
        .expect("owner is alive; the send must succeed");

    let owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        2,
        "mark_needs_layout on the captured handle must reach perform_layout \
         on the very next frame"
    );

    let _ = frame(owner);
    assert_eq!(
        log.layout_count(),
        2,
        "one request produces one relayout, then the tree idles again"
    );
}

// ────────────────────────────────────────────────────────────────────────
// detach on remove
// ────────────────────────────────────────────────────────────────────────

#[test]
fn remove_fires_exactly_one_detach() {
    let (owner, id, log) = rooted_fixture();
    let mut owner = frame(owner);
    assert_eq!(log.detach_count(), 0, "detach must not fire before removal");

    owner.remove_render_object(id);

    assert_eq!(
        log.detach_count(),
        1,
        "remove must call detach exactly once"
    );
}

// ────────────────────────────────────────────────────────────────────────
// Generational staleness: a handle captured before removal goes silent
// ────────────────────────────────────────────────────────────────────────

#[test]
fn mark_needs_layout_on_a_handle_from_a_removed_node_is_a_silent_noop() {
    let (owner, id, log) = rooted_fixture();
    let owner = frame(owner);
    let handle = log.captured_handle();

    let mut owner = owner;
    owner.remove_render_object(id);
    owner.set_root_id(None);

    // The channel cannot know the node died — the send itself still
    // succeeds. The generational guarantee is that drain_pending_dirty
    // drops it silently rather than replaying it against a reused slot.
    handle
        .mark_needs_layout()
        .expect("channel send succeeds even for a dead generation; Ok, not an error");

    let before = log.layout_count();
    let owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        before,
        "a request for a removed node's dead generation must be dropped at \
         drain, never replayed as a real layout"
    );
    drop(owner);
}

// ────────────────────────────────────────────────────────────────────────
// Reparent = remove + insert (no dedicated API in this codebase)
// ────────────────────────────────────────────────────────────────────────

#[test]
fn reparent_via_remove_then_insert_detaches_old_and_attaches_a_fresh_handle() {
    let mut owner = PipelineOwner::new();
    let log = LifecycleLog::default();

    let first_id = owner.insert(probe(log.clone()));
    assert_eq!(log.attach_count(), 1);
    let first_handle = log.captured_handle();
    assert_eq!(first_handle.id(), first_id);

    owner.remove_render_object(first_id);
    assert_eq!(
        log.detach_count(),
        1,
        "the old node's detach must fire before the new node is inserted"
    );

    let second_id = owner.insert(probe(log.clone()));
    assert_eq!(
        log.attach_count(),
        2,
        "reparent (remove + insert) must fire detach then attach with a fresh handle"
    );
    let second_handle = log.captured_handle();
    assert_eq!(
        second_handle.id(),
        second_id,
        "the fresh handle must be bound to the NEW node"
    );
    assert_ne!(
        second_handle.id(),
        first_handle.id(),
        "the fresh handle must not be bound to the stale node"
    );

    // The stale handle from the removed node stays a silent no-op — it
    // must never be confused for the new node's handle.
    first_handle
        .mark_needs_layout()
        .expect("stale handle send still succeeds; drain drops it silently");
}

// ────────────────────────────────────────────────────────────────────────
// Sliver-protocol coverage: no insertion path called `attach` for a Sliver
// child before this fix. `insert_child_render_object` was hard-coded to
// `BoxProtocol`, and `apply_deferred_mutation`'s lazy-child-building path
// (the `Insert` arm in `pipeline/owner/layout.rs`) called the raw
// `RenderTree::insert_sliver_child`/`insert_box_child` directly, bypassing
// `attach_inserted_node` for BOTH protocols. These tests exercise the two
// fixed call sites: the new `insert_sliver_child_render_object` (the
// Sliver-protocol counterpart of `insert_child_render_object`) and
// `apply_deferred_mutation`'s `Insert` arm for each `DeferredRenderObject`
// variant.
// ────────────────────────────────────────────────────────────────────────

#[test]
fn insert_sliver_child_render_object_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(probe(LifecycleLog::default()));

    let log = LifecycleLog::default();
    let child_id = owner
        .insert_sliver_child_render_object(root_id, sliver_probe(log.clone()))
        .expect("root_id was just inserted and is valid");

    assert_eq!(
        log.attach_count(),
        1,
        "insert_sliver_child_render_object must call attach exactly once"
    );
    let handle = log.captured_handle();
    assert_eq!(
        handle.id(),
        child_id,
        "the handed-over handle must be bound to the freshly-inserted sliver child"
    );
}

/// Mounts `parent_id` as a laid-out-ready root, ready to run a layout pass
/// that drains whatever gets deferred onto it.
fn rooted_layout_pipeline() -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let parent_id = owner.insert(probe(LifecycleLog::default()));
    owner.set_root_id(Some(parent_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(40.0), px(40.0)))));
    (owner, parent_id)
}

#[test]
fn deferred_sliver_insert_fires_attach_via_apply_deferred_mutation() {
    let (mut owner, parent_id) = rooted_layout_pipeline();

    let log = LifecycleLog::default();
    owner.defer_insert_sliver(parent_id, sliver_probe(log.clone()), None, None, None);

    let mut layout_owner = owner.into_layout();
    layout_owner.run_layout().expect(
        "layout must not error: parent_id is the root with constraints set, \
         and the deferred insert only needs parent_id to exist",
    );

    assert_eq!(
        log.attach_count(),
        1,
        "apply_deferred_mutation's DeferredRenderObject::Sliver arm \
         (pipeline/owner/layout.rs, the lazy-sliver-child-building path) \
         must call attach exactly once"
    );
}

#[test]
fn deferred_box_insert_fires_attach_via_apply_deferred_mutation() {
    // Collateral fix at the same call site: `apply_deferred_mutation`'s
    // `Insert` arm handles `DeferredRenderObject::Box` and `::Sliver`
    // through one shared code path that calls `attach_inserted_node` once
    // after either variant's tree insertion — so the Box side, which was
    // equally starved of `attach` before this fix, is proven here too.
    let (mut owner, parent_id) = rooted_layout_pipeline();

    let log = LifecycleLog::default();
    owner.defer_insert_box(parent_id, probe(log.clone()), None, None, None);

    let mut layout_owner = owner.into_layout();
    layout_owner
        .run_layout()
        .expect("layout must not error: parent_id is the root with constraints set");

    assert_eq!(
        log.attach_count(),
        1,
        "apply_deferred_mutation's DeferredRenderObject::Box arm must call \
         attach exactly once"
    );
}
