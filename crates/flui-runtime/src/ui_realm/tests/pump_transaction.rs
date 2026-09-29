//! `UiRealm::pump`, the frame transaction (ADR-0083 §1): apply commands, begin
//! frame, draw frame, end frame, at the one timestamp the pump's clock
//! returns — and `pump_background`, the frames-disabled wake.
//!
//! Each test drives the pump itself, never a hand-assembled
//! `drive_frame_with_lane` around `draw_frame`, so each fails against a pump
//! that skips or reorders the phase it names.

use super::*;
use flui_foundation::ManualClock;

/// A post-frame callback scheduled on the realm's owner-local lane runs once,
/// after the pipeline, and sees the layout this same pump committed.
///
/// Fails against a pump that skips end frame (no call), that drives the
/// pipeline after end frame (the callback sees no layout), or that ends the
/// frame without the realm's local lane (the lane is never drained).
pub(crate) fn pump_post_frame_callback_observes_this_frames_committed_layout() {
    use flui_rendering::prelude::Leaf;
    use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, PaintCx, RenderBox};

    #[derive(Debug, Default)]
    struct FixedBox;
    impl flui_foundation::Diagnosticable for FixedBox {}
    impl RenderBox for FixedBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;
        fn perform_layout(
            &mut self,
            _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
        ) -> flui_foundation::geometry::Size {
            flui_foundation::geometry::Size::new(40.0, 24.0)
        }
        fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
    }

    let mut realm = UiRealm::for_test();
    let pipeline = realm.pipeline_for_test();
    let root = pipeline.with_mut(|owner| {
        let root = owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(FixedBox));
        owner.set_root_id(Some(root));
        root
    });
    assert_eq!(
        pipeline.with(|owner| owner.box_size(root)),
        None,
        "nothing is laid out before the first frame"
    );

    let observed = Arc::new(RwLock::new(None));
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_cb = Arc::clone(&observed);
    let calls_cb = Arc::clone(&calls);
    let pipeline_cb = pipeline.clone();
    // `PipelineCell` is `!Send`, so the callback goes on the owner-local
    // lane, which only a pump that ends its frame with that lane drains.
    realm
        .widgets()
        .with_build_owner(|owner| owner.local_post_frame_handle().cloned())
        .expect("owner-local post-frame handle installed by UiRealm::construct")
        .schedule_local(move |_timing| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            *observed_cb.write() = pipeline_cb.with(|owner| owner.box_size(root));
        })
        .expect("the realm's local post-frame lane outlives this call");

    // The pump lays the root out tight to the sink's surface (at the test
    // realm's device pixel ratio of 1), so the surface is the box's own size.
    let _ = realm.pump(
        &mut ManualClock::new(),
        &mut ScriptedSink::always_presents().with_size(40, 24),
    );

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the pump's end frame drains the realm's local post-frame lane exactly once"
    );
    assert_eq!(
        *observed.read(),
        Some(flui_foundation::geometry::Size::new(40.0, 24.0)),
        "the post-frame callback must observe THIS pump's committed layout"
    );
}
