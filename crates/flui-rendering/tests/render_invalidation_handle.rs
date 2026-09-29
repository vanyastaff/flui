//! `RenderInvalidationHandle` (D4): cross-thread repaint capability with wake.
//!
//! The production story this pins: an async producer (image decode,
//! arriving asset) finishes while the app idles; its `mark_needs_paint`
//! must (a) wake the platform and (b) land as a real paint in the next
//! frame — and a handle whose node died must degrade to a silent no-op
//! (generational id), never repaint a reused slot.

use flui_foundation::geometry::Size;
use flui_objects::RenderColoredBox;
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};

use crate::common::BoxedRenderObject;

fn fixture() -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let node = owner.insert(Box::new(RenderColoredBox::red(40.0, 40.0)) as BoxedRenderObject);
    owner.set_root_id(Some(node));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 100.0))));
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

    let handle = owner.render_invalidation_handle(node).expect("live node");
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
fn stale_handle_is_a_silent_noop() {
    let (owner, node) = fixture();
    let (mut owner, _) = frame(owner);

    let handle = owner.render_invalidation_handle(node).expect("live node");
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
