use std::cell::Cell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use std::time::Duration;

use flui_animation::AnimationController;
use flui_engine::EngineError;
use flui_interaction::PointerId;
use flui_interaction::events::{
    PointerButtons, PointerType, make_down_event, make_down_event_for_id, make_move_event,
    make_move_event_for_id,
};
use flui_platform::traits::PlatformInput;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox};
use flui_types::{
    Size,
    geometry::{Offset, px},
};
use flui_widgets::SizedBox;

use super::{SegmentPhase, UiRealm};
use crate::app::epoch::{FrameCommitState, TreeRevision};
use crate::app::raster_test_support::TestRasterBackend;

fn with_quiet_panics<R>(f: impl FnOnce() -> R) -> R {
    type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>;

    struct HookRestore(Option<PanicHook>);

    impl Drop for HookRestore {
        fn drop(&mut self) {
            if let Some(hook) = self.0.take() {
                std::panic::set_hook(hook);
            }
        }
    }

    let _restore = HookRestore(Some(std::panic::take_hook()));
    std::panic::set_hook(Box::new(|_| {}));
    f()
}

fn mount_box() -> UiRealm {
    let realm = UiRealm::for_test();
    realm
        .attach_root_widget(&SizedBox::new(20.0, 20.0))
        .expect("root attaches");
    realm
}

fn primary_state(realm: &UiRealm) -> FrameCommitState {
    realm.presentations.primary().frame_commit_state()
}

fn assert_uncommitted_since(realm: &UiRealm, since: TreeRevision) {
    assert_eq!(
        primary_state(realm),
        FrameCommitState::Uncommitted { since }
    );
}

fn arm_one_shot_build_panic(realm: &UiRealm) {
    let is_armed = Rc::new(Cell::new(true));
    let probe_is_armed = Rc::clone(&is_armed);
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(move || {
            assert!(
                !probe_is_armed.replace(false),
                "build probe — intentional one-shot panic"
            );
        })),
    );
}

#[derive(Debug)]
struct HitCountingBox {
    hits: Arc<AtomicU32>,
}

impl flui_foundation::Diagnosticable for HitCountingBox {}

impl RenderBox for HitCountingBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::new(px(100.0), px(100.0))
    }

    fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}

    fn hit_test(
        &self,
        ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Leaf, BoxParentData>,
    ) -> bool {
        self.hits.fetch_add(1, Ordering::Relaxed);
        ctx.is_within_own_size()
    }
}

#[derive(Clone)]
struct HitCountingView {
    hits: Arc<AtomicU32>,
}

impl flui_view::RenderView for HitCountingView {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = HitCountingBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        HitCountingBox {
            hits: Arc::clone(&self.hits),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl flui_view::View for HitCountingView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

pub(super) fn mount_hit_counting_root() -> (UiRealm, Arc<AtomicU32>) {
    let realm = UiRealm::for_test();
    let hits = Arc::new(AtomicU32::new(0));
    realm
        .attach_root_widget(&HitCountingView {
            hits: Arc::clone(&hits),
        })
        .expect("hit-counting root attaches");
    realm.gestures().mouse_tracker().add_device(
        0,
        PointerType::Mouse,
        Offset::new(px(10.0), px(10.0)),
    );
    (realm, hits)
}

#[test]
fn fresh_presentation_starts_at_committed_zero() {
    let realm = UiRealm::for_test();
    assert_eq!(primary_state(&realm), FrameCommitState::Committed);
    assert_eq!(
        realm.presentations.primary().revision_pair(),
        (TreeRevision::ZERO, TreeRevision::ZERO)
    );
}

#[test]
fn presented_and_no_present_painted_frames_commit() {
    for presents in [true, false] {
        let realm = mount_box();
        let mut backend = TestRasterBackend::single_shot(Ok(presents));
        assert_eq!(realm.render_frame_entered(&mut backend), presents);
        assert_eq!(primary_state(&realm), FrameCommitState::Committed);
        let revision = TreeRevision::ZERO.next();
        assert_eq!(
            realm.presentations.primary().revision_pair(),
            (revision, revision)
        );
    }
}

#[test]
fn errored_frame_is_uncommitted() {
    let realm = mount_box();
    arm_one_shot_build_panic(&realm);
    let mut backend = TestRasterBackend::always_presents();

    assert!(!with_quiet_panics(
        || realm.render_frame_entered(&mut backend)
    ));
    assert_uncommitted_since(&realm, TreeRevision::ZERO.next());
    assert_eq!(backend.render_scene_calls, 0);
}

#[test]
fn errored_then_automatic_idle_preserves_the_earliest_absent_revision() {
    let realm = UiRealm::for_test();
    let controller = AnimationController::new(
        Duration::from_secs(1),
        &flui_scheduler::UpdateScheduler::new(),
    );
    realm.vsync().register(controller.clone());
    controller.forward().expect("fresh controller starts");
    realm.set_now_secs_for_test(0.0);
    arm_one_shot_build_panic(&realm);
    realm.request_redraw();
    let mut backend = TestRasterBackend::always_presents();

    assert!(!with_quiet_panics(
        || realm.render_frame_entered(&mut backend)
    ));
    let since = TreeRevision::ZERO.next();
    assert_uncommitted_since(&realm, since);
    let flushes_after_error = realm.presentations.primary().flush_count();

    realm.set_now_secs_for_test(0.01);
    assert!(!realm.render_frame_entered(&mut backend));
    assert_eq!(
        realm.presentations.primary().flush_count(),
        flushes_after_error + 1,
        "the automatic animation/retry pump must run a clean Idle segment"
    );
    assert_uncommitted_since(&realm, since);
    assert_eq!(
        realm.presentations.primary().revision_pair(),
        (since, TreeRevision::ZERO),
        "Idle must advance neither revision and must not acknowledge the error"
    );
    controller.dispose();
}

#[test]
fn repeated_errors_keep_the_first_gap_and_a_later_commit_starts_a_new_gap() {
    let realm = mount_box();
    let remaining_failures = Rc::new(Cell::new(2_u8));
    let probe_failures = Rc::clone(&remaining_failures);
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(move || {
            let remaining = probe_failures.get();
            if remaining > 0 {
                probe_failures.set(remaining - 1);
                panic!("build probe — intentional repeated panic");
            }
        })),
    );
    let mut backend = TestRasterBackend::always_presents();

    assert!(!with_quiet_panics(
        || realm.render_frame_entered(&mut backend)
    ));
    let first_missing = TreeRevision::ZERO.next();
    assert_uncommitted_since(&realm, first_missing);
    assert!(!with_quiet_panics(
        || realm.render_frame_entered(&mut backend)
    ));
    assert_uncommitted_since(&realm, first_missing);

    assert!(realm.render_frame_entered(&mut backend));
    assert_eq!(primary_state(&realm), FrameCommitState::Committed);
    let committed_revision = first_missing.next().next();
    assert_eq!(
        realm.presentations.primary().revision_pair(),
        (committed_revision, committed_revision)
    );

    arm_one_shot_build_panic(&realm);
    realm.request_redraw();
    assert!(!with_quiet_panics(
        || realm.render_frame_entered(&mut backend)
    ));
    assert_uncommitted_since(&realm, committed_revision.next());
}

#[test]
fn submit_failures_leave_the_painted_revision_uncommitted() {
    let outcomes = [
        ("surface-stale", EngineError::SurfaceLost),
        ("device-lost", EngineError::DeviceLost),
        ("failed", EngineError::Timeout),
    ];
    let states: Vec<_> = outcomes
        .into_iter()
        .map(|(label, error)| {
            let realm = mount_box();
            let mut backend = TestRasterBackend::single_shot(Err(error));
            assert!(!realm.render_frame_entered(&mut backend));
            (label, primary_state(&realm))
        })
        .collect();
    let uncommitted = FrameCommitState::Uncommitted {
        since: TreeRevision::ZERO.next(),
    };
    assert_eq!(
        states,
        vec![
            ("surface-stale", uncommitted),
            ("device-lost", uncommitted),
            ("failed", uncommitted),
        ]
    );
}

#[test]
fn deferred_painted_frame_waits_for_the_later_present_to_commit() {
    let realm = mount_box();
    realm.defer_first_frame();
    let mut backend = TestRasterBackend::always_presents();

    assert!(!realm.render_frame_entered(&mut backend));
    assert_uncommitted_since(&realm, TreeRevision::ZERO.next());
    assert_eq!(backend.render_scene_calls, 0);

    realm.allow_first_frame();
    assert!(realm.render_frame_entered(&mut backend));
    assert_eq!(primary_state(&realm), FrameCommitState::Committed);
}

#[test]
fn semantics_only_idle_on_a_committed_tree_changes_no_revision() {
    let realm = mount_box();
    let mut backend = TestRasterBackend::always_presents();
    assert!(realm.render_frame_entered(&mut backend));
    let committed_pair = realm.presentations.primary().revision_pair();
    let flushes = realm.presentations.primary().flush_count();

    realm
        .pipeline_for_test()
        .with_mut(|owner| owner.set_semantics_enabled(true));
    assert!(!realm.render_frame_entered(&mut backend));
    assert_eq!(realm.presentations.primary().flush_count(), flushes + 1);
    assert_eq!(
        realm.presentations.primary().revision_pair(),
        committed_pair
    );
    assert_eq!(primary_state(&realm), FrameCommitState::Committed);
}

#[test]
fn uncommitted_primary_holds_the_previous_hover_derivation() {
    let (realm, hits) = mount_hit_counting_root();
    let mut clean_backend = TestRasterBackend::always_presents();
    assert!(realm.render_frame_entered(&mut clean_backend));
    let clean_hits = hits.load(Ordering::Relaxed);

    realm.pipeline_for_test().with_mut(|owner| {
        let root = owner.root_id().expect("root installed");
        owner.mark_needs_paint(root);
    });
    realm.request_redraw();
    let mut failed_backend = TestRasterBackend::single_shot(Err(EngineError::Timeout));
    assert!(!realm.render_frame_entered(&mut failed_backend));
    assert!(matches!(
        primary_state(&realm),
        FrameCommitState::Uncommitted { .. }
    ));
    assert_eq!(
        hits.load(Ordering::Relaxed),
        clean_hits,
        "an uncommitted painted revision must hold the prior hover derivation"
    );
}

#[test]
fn secondary_failure_does_not_freeze_a_committed_primary_reprobe() {
    let (mut realm, hits) = mount_hit_counting_root();
    let mut backend = TestRasterBackend::always_presents();
    assert!(realm.render_frame_entered(&mut backend));
    let hits_before_secondary_failure = hits.load(Ordering::Relaxed);

    let secondary_id = realm.install_second_presentation_for_test();
    realm
        .attach_root_widget_to_for_test(secondary_id, &SizedBox::new(20.0, 20.0))
        .expect("secondary root attaches");
    arm_one_shot_build_panic_for(&realm, secondary_id);
    assert!(!with_quiet_panics(
        || realm.render_frame_entered(&mut backend)
    ));

    assert_eq!(primary_state(&realm), FrameCommitState::Committed);
    assert!(
        hits.load(Ordering::Relaxed) > hits_before_secondary_failure,
        "a secondary failure must not suppress the committed primary's ambient re-probe"
    );
}

fn arm_one_shot_build_panic_for(realm: &UiRealm, presentation_id: flui_foundation::PresentationId) {
    let is_armed = Rc::new(Cell::new(true));
    let probe_is_armed = Rc::clone(&is_armed);
    realm
        .presentations
        .get(presentation_id)
        .expect("presentation installed")
        .set_segment_probe(
            SegmentPhase::Build,
            Some(Box::new(move || {
                assert!(
                    !probe_is_armed.replace(false),
                    "build probe — intentional one-shot panic"
                );
            })),
        );
}

#[test]
fn primary_failure_suppresses_its_ambient_reprobe() {
    let (realm, hits) = mount_hit_counting_root();
    let mut backend = TestRasterBackend::always_presents();
    assert!(realm.render_frame_entered(&mut backend));
    let clean_hits = hits.load(Ordering::Relaxed);

    arm_one_shot_build_panic(&realm);
    realm.request_redraw();
    assert!(!with_quiet_panics(
        || realm.render_frame_entered(&mut backend)
    ));
    assert!(matches!(
        primary_state(&realm),
        FrameCommitState::Uncommitted { .. }
    ));
    assert_eq!(hits.load(Ordering::Relaxed), clean_hits);
}

#[test]
fn pointer_input_is_held_while_the_target_presentation_is_uncommitted() {
    let (realm, hits) = mount_hit_counting_root();
    let mut backend = TestRasterBackend::always_presents();
    assert!(realm.render_frame_entered(&mut backend));
    let clean_hits = hits.load(Ordering::Relaxed);

    realm.pipeline_for_test().with_mut(|owner| {
        let root = owner.root_id().expect("root installed");
        owner.mark_needs_paint(root);
    });
    realm.request_redraw();
    let mut failed_backend = TestRasterBackend::single_shot(Err(EngineError::Timeout));
    assert!(!realm.render_frame_entered(&mut failed_backend));
    assert!(matches!(
        primary_state(&realm),
        FrameCommitState::Uncommitted { .. }
    ));

    let primary = realm.presentations.primary();
    let down = make_down_event(Offset::new(px(10.0), px(10.0)), PointerType::Mouse);
    realm.handle_input_addressed(primary.id(), PlatformInput::Pointer(down));

    assert_eq!(
        hits.load(Ordering::Relaxed),
        clean_hits,
        "held pointer input must not hit-test against an uncommitted tree"
    );
    assert_eq!(
        primary.held_pointer_input().borrow().len(),
        1,
        "the pointer event must be retained for the later commit replay"
    );
}

#[test]
fn a_nonempty_held_queue_keeps_later_pointer_input_held_after_commit() {
    let (realm, hits) = mount_hit_counting_root();
    let mut backend = TestRasterBackend::always_presents();
    assert!(realm.render_frame_entered(&mut backend));
    let clean_hits = hits.load(Ordering::Relaxed);

    let primary = realm.presentations.primary();
    primary
        .held_pointer_input()
        .borrow_mut()
        .append(make_down_event(
            Offset::new(px(10.0), px(10.0)),
            PointerType::Mouse,
        ));
    assert_eq!(primary.frame_commit_state(), FrameCommitState::Committed);

    let move_event = make_move_event(Offset::new(px(11.0), px(11.0)), PointerType::Mouse);
    realm.handle_input_addressed(primary.id(), PlatformInput::Pointer(move_event));

    assert_eq!(
        hits.load(Ordering::Relaxed),
        clean_hits,
        "a queued replay backlog must keep subsequent pointer input out of live hit-testing"
    );
    assert_eq!(
        primary.held_pointer_input().borrow().len(),
        2,
        "the later pointer event must queue behind the unreplayed backlog"
    );
}

#[test]
fn window_leave_drops_held_hovers_but_retains_held_contact_sequences() {
    let realm = mount_box();
    let primary = realm.presentations.primary();
    primary
        .held_pointer_input()
        .borrow_mut()
        .append(make_down_event(
            Offset::new(px(10.0), px(10.0)),
            PointerType::Mouse,
        ));
    let mut hover = make_move_event(Offset::new(px(11.0), px(11.0)), PointerType::Mouse);
    let flui_interaction::PointerEvent::Move(update) = &mut hover else {
        unreachable!("the move helper always constructs PointerEvent::Move")
    };
    update.current.buttons = PointerButtons::new();
    primary.held_pointer_input().borrow_mut().append(hover);

    realm.handle_window_hover_addressed(primary.id(), false);

    assert_eq!(
        primary.held_pointer_input().borrow().len(),
        1,
        "window leave must only drop held hovers; contact epochs stay queued"
    );
}

#[test]
fn closing_a_presentation_drops_held_pointer_input_without_synthesizing_cancel() {
    let mut realm = UiRealm::for_test();
    let closing_id = realm.install_second_presentation_for_test();
    let closing = realm
        .presentations
        .get(closing_id)
        .expect("secondary presentation installed");
    let pointer = PointerId::new(42).expect("test pointer id is nonzero");
    closing
        .held_pointer_input()
        .borrow_mut()
        .append(make_down_event_for_id(
            pointer,
            Offset::new(px(10.0), px(10.0)),
            PointerType::Touch,
        ));
    let routed_events = Rc::new(Cell::new(0));
    let routed_events_from_cancel = Rc::clone(&routed_events);
    let route: flui_interaction::routing::PointerRouteHandler = Rc::new(move |_| {
        routed_events_from_cancel.set(routed_events_from_cancel.get() + 1);
    });
    closing
        .gestures()
        .pointer_router()
        .add_route(pointer, route);

    assert!(realm.close_presentation_entered(closing_id));

    assert_eq!(
        routed_events.get(),
        0,
        "closing a presentation must drop held input without synthesizing a routed Cancel"
    );
}

#[test]
fn presented_commit_replays_held_pointer_input_after_current_frame_telemetry() {
    let realm = mount_box();
    let primary = realm.presentations.primary();
    let pointer = PointerId::new(42).expect("test pointer id is nonzero");
    primary
        .held_pointer_input()
        .borrow_mut()
        .append(make_down_event_for_id(
            pointer,
            Offset::new(px(10.0), px(10.0)),
            PointerType::Touch,
        ));

    let mut backend = TestRasterBackend::always_presents();
    assert!(realm.render_frame_entered(&mut backend));

    assert!(
        primary.held_pointer_input().borrow().is_empty(),
        "a committed presented frame must drain held pointer input"
    );
    assert!(
        realm.needs_redraw(),
        "replayed pointer input must wake the next pump after mark_rendered cleared this one"
    );
    let snapshots = primary.clock().frames_since(None);
    assert_eq!(snapshots.len(), 1);
    assert_eq!(
        snapshots[0].latencies().count(),
        0,
        "held input replayed after the commit must not be attributed to the frame already presented"
    );
}

#[test]
fn replayed_move_enters_pending_moves_and_flushes_on_the_next_pump() {
    let realm = mount_box();
    let primary = realm.presentations.primary();
    let pointer = PointerId::new(42).expect("test pointer id is nonzero");
    primary
        .held_pointer_input()
        .borrow_mut()
        .append(make_down_event_for_id(
            pointer,
            Offset::new(px(10.0), px(10.0)),
            PointerType::Touch,
        ));
    primary
        .held_pointer_input()
        .borrow_mut()
        .append(make_move_event_for_id(
            pointer,
            Offset::new(px(11.0), px(11.0)),
            PointerType::Touch,
        ));

    let mut backend = TestRasterBackend::always_presents();
    assert!(realm.render_frame_entered(&mut backend));

    assert_eq!(
        primary.gestures().pending_move_count(),
        1,
        "a replayed Move must use the normal GestureBinding coalesced-move path"
    );
    assert!(
        realm.needs_redraw(),
        "the pending replayed Move needs a pump"
    );

    let _ = realm.render_frame_entered(&mut backend);
    assert_eq!(
        primary.gestures().pending_move_count(),
        0,
        "the next pump must flush the replayed pending Move"
    );
}

#[test]
fn no_present_commit_also_replays_held_pointer_input() {
    let realm = mount_box();
    let primary = realm.presentations.primary();
    let pointer = PointerId::new(42).expect("test pointer id is nonzero");
    primary
        .held_pointer_input()
        .borrow_mut()
        .append(make_down_event_for_id(
            pointer,
            Offset::new(px(10.0), px(10.0)),
            PointerType::Touch,
        ));
    primary
        .held_pointer_input()
        .borrow_mut()
        .append(make_move_event_for_id(
            pointer,
            Offset::new(px(11.0), px(11.0)),
            PointerType::Touch,
        ));

    let mut backend = TestRasterBackend::single_shot(Ok(false));
    assert!(!realm.render_frame_entered(&mut backend));

    assert!(
        primary.held_pointer_input().borrow().is_empty(),
        "NoPresent still acknowledges the tree and must drain held input"
    );
    assert_eq!(
        primary.gestures().pending_move_count(),
        1,
        "NoPresent replay must use the same pointer dispatch path as Presented replay"
    );
    assert!(
        realm.needs_redraw(),
        "NoPresent replay must wake a follow-up pump"
    );
}
