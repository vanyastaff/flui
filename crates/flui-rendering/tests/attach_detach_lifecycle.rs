//! ADR-0013 milestone — the `attach`/`detach` tree-lifecycle hook.
//!
//! Proves the pipeline actually fires `RenderObject::attach`/`detach` at
//! insert/remove, that the handed-over `LocalRenderInvalidationHandle` is bound to the
//! right node and drives a REAL re-layout on the very next frame, and
//! that a handle captured before removal refuses further invalidation
//! afterward — the attachment-lifetime guarantee `LocalRenderInvalidationHandle`
//! documents. Reparenting in this codebase has no dedicated API (no
//! `move_child`/`adopt_child`); it is remove-then-insert, so that case is
//! exercised the same way a real reparent would hit it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::Leaf;
use flui_rendering::pipeline::{LocalRenderInvalidationHandle, PipelineOwner};
use flui_rendering::prelude::*;

use crate::common::BoxedRenderObject;

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
    paint_count: Arc<AtomicUsize>,
    captured_handle: Rc<RefCell<Option<LocalRenderInvalidationHandle>>>,
    peer_drop: Option<Rc<PeerDropLog>>,
}

#[derive(Default, Debug)]
struct PeerDropLog {
    handles: RefCell<Vec<LocalRenderInvalidationHandle>>,
    attempts: Cell<usize>,
    unexpected: Cell<usize>,
}

impl LifecycleLog {
    fn attach_count(&self) -> usize {
        self.attach_count.load(Ordering::SeqCst)
    }

    /// The most recently captured handle. Panics if `attach` never fired —
    /// every test here calls it only after asserting `attach_count() > 0`.
    fn captured_handle(&self) -> LocalRenderInvalidationHandle {
        self.captured_handle
            .borrow()
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

impl Drop for LifecycleProbe {
    fn drop(&mut self) {
        let Some(peers) = &self.log.peer_drop else {
            return;
        };
        let handles = peers.handles.borrow().clone();
        for handle in handles {
            peers.attempts.set(peers.attempts.get() + 1);
            if !matches!(
                handle.mark_needs_paint(),
                Err(flui_rendering::pipeline::DirtySendError::OwnerGone)
            ) {
                peers.unexpected.set(peers.unexpected.get() + 1);
            }
        }
    }
}

impl flui_foundation::Diagnosticable for LifecycleProbe {}

impl RenderBox for LifecycleProbe {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok({
            self.log.layout_count.fetch_add(1, Ordering::SeqCst);
            self.size
        })
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.log.attach_count.fetch_add(1, Ordering::SeqCst);
        let _prev = self.log.captured_handle.borrow_mut().replace(handle);
    }

    fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, Leaf>) {
        self.log.paint_count.fetch_add(1, Ordering::SeqCst);
    }

    fn detach(&mut self) {
        self.log.detach_count.fetch_add(1, Ordering::SeqCst);
    }
}

fn probe(log: LifecycleLog) -> BoxedRenderObject {
    Box::new(LifecycleProbe {
        log,
        size: Size::new(40.0, 40.0),
    }) as BoxedRenderObject
}

// ────────────────────────────────────────────────────────────────────────
// Probe: a leaf RenderSliver that records attach/detach/perform_layout
// calls — the Sliver-protocol counterpart of `LifecycleProbe`, proving the
// ADR-0013 lifecycle hook fires for Sliver children too (it did not,
// before this fix: no insertion path called `attach` for a Sliver child).
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// attach on insert
// ────────────────────────────────────────────────────────────────────────

pub(crate) fn insert_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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

pub(crate) fn local_marks_survive_a_failed_wake_and_coalesce() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let log = LifecycleLog::default();
    owner.insert(probe(log.clone()));
    let handle = log.captured_handle();
    owner.set_on_need_visual_update(|| panic!("local wake failure"));
    let impact = flui_rendering::RenderUpdateImpact::PAINT
        | flui_rendering::RenderUpdateImpact::COMPOSITING_BITS;
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle.request_update(impact).expect("live attachment");
        }))
        .is_err()
    );

    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    owner.set_on_need_visual_update(move || {
        observed.fetch_add(1, Ordering::SeqCst);
    });
    assert!(
        wakes.load(Ordering::SeqCst) > 0,
        "replacement wakes retained work"
    );
    for _ in 0..100 {
        handle.request_update(impact).expect("live attachment");
    }
    assert_eq!(
        owner.drain_pending_dirty(),
        2,
        "one delivery per requested phase"
    );
    assert_eq!(
        owner.drain_pending_dirty(),
        0,
        "accepted batch was delivered"
    );
    handle
        .mark_needs_layout()
        .expect("next operation remains usable");
    assert_eq!(owner.drain_pending_dirty(), 1);
}

pub(crate) fn local_marks_wait_for_a_hook_and_end_with_the_attachment() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let log = LifecycleLog::default();
    let id = owner.insert(probe(log.clone()));
    let handle = log.captured_handle();
    handle
        .mark_needs_paint()
        .expect("missing hook retains work");
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    owner.set_on_need_visual_update(move || {
        observed.fetch_add(1, Ordering::SeqCst);
    });
    assert!(wakes.load(Ordering::SeqCst) > 0);
    owner.remove_render_object(id);
    assert!(matches!(
        handle.mark_needs_layout(),
        Err(flui_rendering::pipeline::DirtySendError::OwnerGone)
    ));
    assert_eq!(
        owner.drain_pending_dirty(),
        0,
        "removed attachment owes no delivery"
    );

    let next = LifecycleLog::default();
    owner.insert(probe(next.clone()));
    let next_handle = next.captured_handle();
    next_handle
        .mark_needs_layout()
        .expect("replacement attachment is live");
    assert_eq!(owner.drain_pending_dirty(), 1);
    drop(owner);
    assert!(matches!(
        next_handle.mark_needs_paint(),
        Err(flui_rendering::pipeline::DirtySendError::OwnerGone)
    ));
}

pub(crate) fn owner_release_withdraws_peer_handles_before_user_drop() {
    thread_local! {
        static OWNER: RefCell<Option<PipelineOwner>> = const { RefCell::new(None) };
    }

    let mut outcomes = Vec::new();
    for release_from_wake in [false, true] {
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let peers = Rc::new(PeerDropLog::default());
        let first = LifecycleLog {
            peer_drop: Some(Rc::clone(&peers)),
            ..LifecycleLog::default()
        };
        let second = LifecycleLog {
            peer_drop: Some(Rc::clone(&peers)),
            ..LifecycleLog::default()
        };
        owner.insert(probe(first.clone()));
        owner.insert(probe(second.clone()));
        peers
            .handles
            .borrow_mut()
            .extend([first.captured_handle(), second.captured_handle()]);
        if release_from_wake {
            owner.set_on_need_visual_update(|| {
                let outgoing = OWNER.with(|saved| saved.borrow_mut().take());
                drop(outgoing);
            });
            let outgoing = OWNER.with(|saved| saved.replace(Some(owner)));
            assert!(
                outgoing.is_none(),
                "no owner remains from the preceding row"
            );
            first
                .captured_handle()
                .mark_needs_paint()
                .expect("live admission before its wake");
        } else {
            drop(owner);
        }
        outcomes.push((
            release_from_wake,
            peers.attempts.get(),
            peers.unexpected.get(),
        ));
    }
    assert_eq!(
        outcomes,
        [(false, 4, 0), (true, 4, 0)],
        "every saved handle refuses new work before the first user Drop, including release inside an active wake"
    );
}

pub(crate) fn local_drain_failure_preserves_the_phase_tail_and_next_frame() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let log = LifecycleLog::default();
    let id = owner.insert(probe(log.clone()));
    owner.set_root_id(Some(id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(40.0, 40.0))));
    let (mut owner, frame) = owner.run_frame();
    frame.expect("initial layout and paint");
    let layouts = log.layout_count.load(Ordering::SeqCst);
    let paints = log.paint_count.load(Ordering::SeqCst);
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    owner.set_on_need_visual_update(move || match observed.fetch_add(1, Ordering::SeqCst) {
        0 => {}
        1 => panic!("local drain first failure"),
        _ => panic!("local drain competing failure"),
    });
    log.captured_handle()
        .request_update(flui_rendering::RenderUpdateImpact::LAYOUT)
        .expect("admit layout and paint before their frame");
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.drain_pending_dirty();
    }))
    .expect_err("canonical layout wake fails");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"local drain first failure")
    );
    assert!(
        wakes.load(Ordering::SeqCst) >= 3,
        "the paint tail runs despite the earlier layout wake failure"
    );
    owner.set_on_need_visual_update(|| {});
    let (owner, frame) = owner.run_frame();
    frame.expect("the accepted tail remains runnable");
    assert_eq!(log.layout_count.load(Ordering::SeqCst), layouts + 1);
    assert_eq!(log.paint_count.load(Ordering::SeqCst), paints + 1);
    log.captured_handle()
        .mark_needs_layout()
        .expect("fresh invalidation");
    let (_, frame) = owner.run_frame();
    frame.expect("fresh operation progresses after containment");
    assert_eq!(log.layout_count.load(Ordering::SeqCst), layouts + 2);
}

// ────────────────────────────────────────────────────────────────────────
// mark_needs_layout from the captured handle reaches perform_layout
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// detach on remove
// ────────────────────────────────────────────────────────────────────────

// The token is a linear capability: duplicating it would let one detached
// batch be attached and released, or released twice.
static_assertions::assert_not_impl_any!(
    flui_rendering::pipeline::DetachedRenderSubtrees: Clone, Copy
);

// ────────────────────────────────────────────────────────────────────────
// Generational staleness: a handle captured before removal goes silent
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// Reparent = remove + insert (no dedicated API in this codebase)
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// Sliver-protocol coverage: no insertion path called `attach` for a Sliver
// child before this fix. `insert_child_render_object` was hard-coded to
// `BoxProtocol`. This test exercises the fixed call site: the new
// `insert_sliver_child_render_object` (the Sliver-protocol counterpart of
// `insert_child_render_object`).
// ────────────────────────────────────────────────────────────────────────
