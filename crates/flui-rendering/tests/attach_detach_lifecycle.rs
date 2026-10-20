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
    /// When set, `paint` marks the probe for layout through its handle.
    relayout_from_paint: Rc<Cell<bool>>,
    peer_drop: Option<Rc<PeerDropLog>>,
    peer_detach: Option<Rc<PeerDropLog>>,
}

#[derive(Default, Debug)]
struct PeerDropLog {
    handles: RefCell<Vec<LocalRenderInvalidationHandle>>,
    attempts: Cell<usize>,
    unexpected: Cell<usize>,
}

impl PeerDropLog {
    fn check_withdrawn(&self) {
        let handles = self.handles.borrow().clone();
        for handle in handles {
            self.attempts.set(self.attempts.get() + 1);
            if !matches!(
                handle.mark_needs_paint(),
                Err(flui_rendering::pipeline::DirtySendError::OwnerGone)
            ) {
                self.unexpected.set(self.unexpected.get() + 1);
            }
        }
    }
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
        peers.check_withdrawn();
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
        if self.log.relayout_from_paint.get() {
            self.log
                .captured_handle()
                .mark_needs_layout()
                .expect("an attached probe paints");
        }
    }

    fn detach(&mut self) {
        self.log.detach_count.fetch_add(1, Ordering::SeqCst);
        if let Some(peers) = &self.log.peer_detach {
            peers.check_withdrawn();
        }
    }
}

#[derive(Debug)]
struct LifecycleContainer(LifecycleProbe);

impl flui_foundation::Diagnosticable for LifecycleContainer {}

impl RenderBox for LifecycleContainer {
    type Arity = flui_foundation::Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Self::Arity, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(self.0.size)
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        RenderBox::attach(&mut self.0, handle);
    }

    fn detach(&mut self) {
        RenderBox::detach(&mut self.0);
    }
}

#[derive(Clone, Debug)]
struct RetirementParentData {
    peers: Rc<PeerDropLog>,
    detaches: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
}

impl flui_rendering::parent_data::ParentData for RetirementParentData {
    fn detach(&mut self) {
        self.detaches.set(self.detaches.get() + 1);
        self.peers.check_withdrawn();
    }
}

impl Drop for RetirementParentData {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        self.peers.check_withdrawn();
    }
}

pub(crate) fn subtree_cleanup_withdraws_all_selected_handles_before_user_code() {
    // The public wake hook is Send + Sync; its owner-local observations stay
    // on the invoking thread rather than weakening the capability's type.
    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static PEERS: RefCell<Option<Rc<PeerDropLog>>> = const { RefCell::new(None) };
    }
    let mut outcomes = Vec::new();
    for relocate in [false, true] {
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let peers = Rc::new(PeerDropLog::default());
        let root_log = LifecycleLog {
            peer_detach: Some(Rc::clone(&peers)),
            ..LifecycleLog::default()
        };
        let child_log = LifecycleLog {
            peer_detach: Some(Rc::clone(&peers)),
            ..LifecycleLog::default()
        };
        let parent = owner.insert(Box::new(LifecycleContainer(LifecycleProbe {
            log: LifecycleLog::default(),
            size: Size::new(40.0, 40.0),
        })) as BoxedRenderObject);
        let root = owner
            .insert_child_render_object(
                parent,
                Box::new(LifecycleContainer(LifecycleProbe {
                    log: root_log.clone(),
                    size: Size::new(40.0, 40.0),
                })) as BoxedRenderObject,
            )
            .expect("live parent");
        let child = owner
            .insert_child_render_object(root, probe(child_log.clone()))
            .expect("live container");
        let survivor_log = LifecycleLog::default();
        owner
            .insert_child_render_object(parent, probe(survivor_log.clone()))
            .expect("live parent");
        let old_handles = [root_log.captured_handle(), child_log.captured_handle()];
        peers
            .handles
            .borrow_mut()
            .extend(old_handles.iter().cloned());
        let detaches = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        owner
            .render_tree_mut()
            .get_mut(root)
            .expect("live root")
            .set_parent_data(Box::new(RetirementParentData {
                peers: Rc::clone(&peers),
                detaches: Rc::clone(&detaches),
                drops: Rc::clone(&drops),
            }));
        owner.set_on_need_visual_update(|| {
            let observed = PEERS.with(|saved| saved.borrow_mut().take());
            if let Some(observed) = observed {
                observed.check_withdrawn();
                PEERS.with(|saved| *saved.borrow_mut() = Some(observed));
            }
        });
        PEERS.with(|saved| *saved.borrow_mut() = Some(Rc::clone(&peers)));
        let token = if relocate {
            Some(
                owner
                    .detach_render_subtrees(&[root])
                    .expect("attached subtree"),
            )
        } else {
            assert_eq!(owner.remove_render_object(root), 2);
            None
        };
        owner.set_on_need_visual_update(|| {});
        let outgoing = PEERS.with(|saved| saved.borrow_mut().take());
        drop(outgoing);
        assert!(
            peers.attempts.get() >= 8,
            "parent-data detach and Drop and both render hooks ran"
        );
        outcomes.push((relocate, peers.unexpected.get()));
        assert_eq!((detaches.get(), drops.get()), (1, 1));
        assert_eq!(root_log.detach_count.load(Ordering::SeqCst), 1);
        assert_eq!(child_log.detach_count.load(Ordering::SeqCst), 1);
        survivor_log
            .captured_handle()
            .mark_needs_paint()
            .expect("outside subtree stays attached");
        if let Some(token) = token {
            owner.adopt_render_child(parent, root);
            owner
                .attach_render_subtrees(token)
                .expect("same subtree, fresh intervals");
            for handle in [root_log.captured_handle(), child_log.captured_handle()] {
                handle
                    .mark_needs_paint()
                    .expect("new attachment accepts work");
            }
            for handle in old_handles {
                assert!(matches!(
                    handle.mark_needs_paint(),
                    Err(flui_rendering::pipeline::DirtySendError::OwnerGone)
                ));
            }
            assert!(owner.render_tree().get(child).is_some());
        }
    }
    assert_eq!(
        outcomes,
        [(false, 0), (true, 0)],
        "all selected handles must refuse before any cleanup or membership wake"
    );
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
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
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

/// Paint must not change geometry: a layout mark made while painting is
/// asserted against in debug builds and otherwise applies next frame.
pub(crate) fn a_layout_mark_during_paint_is_diagnosed() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let log = LifecycleLog::default();
    let id = owner.insert(probe(log.clone()));
    owner.set_root_id(Some(id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(40.0, 40.0))));
    let (owner, frame) = owner.run_frame();
    frame.expect("initial layout and paint");
    let layouts = log.layout_count.load(Ordering::SeqCst);

    log.relayout_from_paint.set(true);
    log.captured_handle()
        .mark_needs_paint()
        .expect("live attachment");
    let painted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || owner.run_frame()));

    if cfg!(debug_assertions) {
        let Err(failure) = painted else {
            panic!("a debug build asserts against a layout mark during paint");
        };
        let message = failure.downcast_ref::<String>().map_or("", String::as_str);
        assert!(
            message.contains("requested layout during paint"),
            "unexpected panic: {message}"
        );
    } else {
        let (owner, frame) = painted.expect("a release build only logs the mark");
        frame.expect("the paint pass completes");
        log.relayout_from_paint.set(false);
        let (_, frame) = owner.run_frame();
        frame.expect("the mark applies next frame");
        assert_eq!(log.layout_count.load(Ordering::SeqCst), layouts + 1);
    }
}

/// A relocated subtree takes the depth of its new position, also when the
/// destination edge is written while the batch is detached (as the element
/// tree's edge synchronization does).
pub(crate) fn a_relocated_subtree_takes_its_new_depth() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let insert = |owner: &mut PipelineOwner, parent| {
        owner
            .insert_child_render_object(parent, probe(LifecycleLog::default()))
            .expect("live parent")
    };
    let root = owner.set_root_render_object(probe(LifecycleLog::default()));
    let shallow = insert(&mut owner, root);
    let moved = insert(&mut owner, shallow);
    let moved_child = insert(&mut owner, moved);
    let between = insert(&mut owner, root);
    let deep = insert(&mut owner, between);

    let token = owner
        .detach_render_subtrees(&[moved])
        .expect("attached subtree");
    owner.render_tree_mut().adopt_child(deep, moved);
    owner
        .attach_render_subtrees(token)
        .expect("same subtree, new position");

    let depths: Vec<_> = [deep, moved, moved_child]
        .iter()
        .map(|&id| owner.render_tree().depth(id))
        .collect();
    assert_eq!(depths, [Some(2), Some(3), Some(4)]);
}
