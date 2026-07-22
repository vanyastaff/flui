//! `RepaintHandle` (D4): cross-thread repaint capability with wake.
//!
//! The production story this pins: an async producer (image decode,
//! arriving asset) finishes while the app idles; its `mark_needs_paint`
//! must (a) wake the platform and (b) land as a real paint in the next
//! frame — and a handle whose node died must degrade to a silent no-op
//! (generational id), never repaint a reused slot.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_objects::RenderColoredBox;
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::{DirtyKind, DirtySendError, PipelineOwner},
};
use flui_types::{Size, geometry::px};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

fn fixture() -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let node = owner.insert(Box::new(RenderColoredBox::red(40.0, 40.0)) as BoxedRenderObject);
    owner.set_root_id(Some(node));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
    (owner, node)
}

fn frame(owner: PipelineOwner) -> (PipelineOwner, bool) {
    let (owner, result) = owner.run_frame();
    let painted = result.expect("frame must not error").is_some();
    (owner, painted)
}

#[test]
fn cross_thread_repaint_lands_in_the_next_frame() {
    let (owner, node) = fixture();
    let (mut owner, painted) = frame(owner);
    assert!(painted, "initial frame paints");
    let (next, painted) = frame(owner);
    owner = next;
    assert!(!painted, "clean tree idles");

    let handle = owner.repaint_handle(node).expect("live node");
    std::thread::spawn(move || {
        handle.mark_needs_paint().expect("owner alive");
    })
    .join()
    .expect("producer thread");

    let (owner, painted) = frame(owner);
    assert!(
        painted,
        "a repaint requested from another thread must be observed by \
         the very next frame"
    );
    let (_owner, painted) = frame(owner);
    assert!(!painted, "one request produces one repaint, then idle");
}

#[test]
fn request_fires_the_visual_update_wake() {
    let (mut owner, node) = fixture();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    owner.set_on_need_visual_update(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });

    let handle = owner.repaint_handle(node).expect("live node");
    let before = wakes.load(Ordering::Relaxed);
    handle.mark_needs_paint().expect("owner alive");
    assert!(
        wakes.load(Ordering::Relaxed) > before,
        "enqueue without a wake is the GIF-frozen-until-you-scroll bug \
         — an idle event loop would never drain the request"
    );
}

#[test]
fn stale_handle_is_a_silent_noop() {
    let (owner, node) = fixture();
    let (mut owner, _) = frame(owner);

    let handle = owner.repaint_handle(node).expect("live node");
    owner.remove_render_object(node);
    owner.set_root_id(None);

    // The node is gone; its generation died with it. The send still
    // succeeds (the channel cannot know), but the drain must drop it.
    handle.mark_needs_paint().expect("channel alive");
    let (_owner, painted) = frame(owner);
    assert!(
        !painted,
        "a request for a dead generation must be dropped at drain, not \
         replayed into the paint queue"
    );
}

#[test]
fn layout_request_routes_through_the_boundary_walk() {
    let (owner, node) = fixture();
    let (owner, _) = frame(owner);

    // Raw pipeline handle, Layout kind: the drain must route through
    // mark_needs_layout (boundary walk + dedup), not push a raw queue
    // entry. The requested depth is deliberately wrong — the live
    // node's state is authoritative.
    let pipeline_handle = owner.handle();
    pipeline_handle
        .request_mark_dirty(node, 999, DirtyKind::Layout)
        .expect("owner alive");

    let (owner, painted) = frame(owner);
    assert!(painted, "the relayout reaches paint through run_layout");
    let (_owner, painted) = frame(owner);
    assert!(!painted, "no residue in the dirty queues");
}

#[test]
fn dropped_owner_reports_owner_gone() {
    let (owner, node) = fixture();
    let handle = owner.repaint_handle(node).expect("live node");
    drop(owner);
    assert!(
        matches!(handle.mark_needs_paint(), Err(DirtySendError::OwnerGone)),
        "producers must learn the pipeline is gone and stop sending"
    );
}
