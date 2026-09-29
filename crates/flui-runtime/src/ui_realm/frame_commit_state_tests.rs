use std::cell::Cell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use std::time::Duration;

use flui_animation::AnimationController;
use flui_foundation::geometry::{Offset, Size};
use flui_interaction::PointerId;
use flui_interaction::events::{
    PointerButtons, PointerType, make_down_event, make_down_event_for_id, make_move_event,
    make_up_event_for_id,
};
use flui_platform_api::PlatformInput;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox};
use flui_widgets::SizedBox;

use super::{SegmentPhase, UiRealm};
use crate::epoch::{FrameCommitState, TreeRevision};
use crate::sink::SubmitVerdict;
use crate::testing::ScriptedSink;

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
        Size::new(100.0, 100.0)
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
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.hits = Arc::clone(&self.hits);
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
    realm
        .gestures()
        .mouse_tracker()
        .add_device(0, PointerType::Mouse, Offset::new(10.0, 10.0));
    (realm, hits)
}

#[test]
fn presented_and_no_present_painted_frames_commit() {
    for presents in [true, false] {
        let realm = mount_box();
        let outcome = if presents {
            SubmitVerdict::Presented
        } else {
            SubmitVerdict::NoPresent
        };
        let mut backend = ScriptedSink::single_shot(outcome);
        assert_eq!(realm.render_frame(&mut backend), presents);
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
    let mut backend = ScriptedSink::always_presents();

    assert!(!with_quiet_panics(|| realm.render_frame(&mut backend)));
    assert_uncommitted_since(&realm, TreeRevision::ZERO.next());
    assert_eq!(backend.submit_calls, 0);
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
    let mut backend = ScriptedSink::always_presents();

    assert!(!with_quiet_panics(|| realm.render_frame(&mut backend)));
    let since = TreeRevision::ZERO.next();
    assert_uncommitted_since(&realm, since);
    let flushes_after_error = realm.presentations.primary().flush_count();

    realm.set_now_secs_for_test(0.01);
    assert!(!realm.render_frame(&mut backend));
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
    let mut backend = ScriptedSink::always_presents();

    assert!(!with_quiet_panics(|| realm.render_frame(&mut backend)));
    let first_missing = TreeRevision::ZERO.next();
    assert_uncommitted_since(&realm, first_missing);
    assert!(!with_quiet_panics(|| realm.render_frame(&mut backend)));
    assert_uncommitted_since(&realm, first_missing);

    assert!(realm.render_frame(&mut backend));
    assert_eq!(primary_state(&realm), FrameCommitState::Committed);
    let committed_revision = first_missing.next().next();
    assert_eq!(
        realm.presentations.primary().revision_pair(),
        (committed_revision, committed_revision)
    );

    arm_one_shot_build_panic(&realm);
    realm.request_redraw();
    assert!(!with_quiet_panics(|| realm.render_frame(&mut backend)));
    assert_uncommitted_since(&realm, committed_revision.next());
}

#[test]
fn submit_failures_leave_the_painted_revision_uncommitted() {
    let outcomes = [
        ("surface-stale", SubmitVerdict::SurfaceStale),
        ("device-lost", SubmitVerdict::DeviceLost),
        ("failed", SubmitVerdict::Failed),
    ];
    let states: Vec<_> = outcomes
        .into_iter()
        .map(|(label, verdict)| {
            let realm = mount_box();
            let mut backend = ScriptedSink::single_shot(verdict);
            assert!(!realm.render_frame(&mut backend));
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
    let mut backend = ScriptedSink::always_presents();

    assert!(!realm.render_frame(&mut backend));
    assert_uncommitted_since(&realm, TreeRevision::ZERO.next());
    assert_eq!(backend.submit_calls, 0);

    realm.allow_first_frame();
    assert!(realm.render_frame(&mut backend));
    assert_eq!(primary_state(&realm), FrameCommitState::Committed);
}

#[test]
fn secondary_failure_does_not_freeze_a_committed_primary_reprobe() {
    let (mut realm, hits) = mount_hit_counting_root();
    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));
    let hits_before_secondary_failure = hits.load(Ordering::Relaxed);

    let secondary_id = realm.install_second_presentation_for_test();
    realm
        .attach_root_widget_to_for_test(secondary_id, &SizedBox::new(20.0, 20.0))
        .expect("secondary root attaches");
    arm_one_shot_build_panic_for(&realm, secondary_id);
    assert!(!with_quiet_panics(|| realm.render_frame(&mut backend)));

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
    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));
    let clean_hits = hits.load(Ordering::Relaxed);

    arm_one_shot_build_panic(&realm);
    realm.request_redraw();
    assert!(!with_quiet_panics(|| realm.render_frame(&mut backend)));
    assert!(matches!(
        primary_state(&realm),
        FrameCommitState::Uncommitted { .. }
    ));
    assert_eq!(hits.load(Ordering::Relaxed), clean_hits);
}

#[test]
fn pointer_input_is_held_while_the_target_presentation_is_uncommitted() {
    let (realm, hits) = mount_hit_counting_root();
    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));
    let clean_hits = hits.load(Ordering::Relaxed);

    realm.pipeline_for_test().with_mut(|owner| {
        let root = owner.root_id().expect("root installed");
        owner.mark_needs_paint(root);
    });
    realm.request_redraw();
    let mut failed_backend = ScriptedSink::single_shot(SubmitVerdict::Failed);
    assert!(!realm.render_frame(&mut failed_backend));
    assert!(matches!(
        primary_state(&realm),
        FrameCommitState::Uncommitted { .. }
    ));

    let primary = realm.presentations.primary();
    let down = make_down_event(Offset::new(10.0, 10.0), PointerType::Mouse);
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
fn held_terminal_event_for_an_already_active_pointer_releases_the_cached_route_after_commit() {
    let (realm, hits) = mount_hit_counting_root();
    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));
    let primary = realm.presentations.primary();
    let pointer = PointerId::new(101).expect("test pointer id is nonzero");

    realm.handle_input_addressed(
        primary.id(),
        PlatformInput::Pointer(make_down_event_for_id(
            pointer,
            Offset::new(10.0, 10.0),
            PointerType::Touch,
        )),
    );
    assert_eq!(primary.gestures().active_pointer_count(), 1);
    let hits_after_down = hits.load(Ordering::Relaxed);

    realm.pipeline_for_test().with_mut(|owner| {
        let root = owner.root_id().expect("root installed");
        owner.mark_needs_paint(root);
    });
    realm.request_redraw();
    let mut failed_backend = ScriptedSink::single_shot(SubmitVerdict::Failed);
    assert!(!realm.render_frame(&mut failed_backend));
    assert!(matches!(
        primary.frame_commit_state(),
        FrameCommitState::Uncommitted { .. }
    ));

    realm.handle_input_addressed(
        primary.id(),
        PlatformInput::Pointer(make_up_event_for_id(
            pointer,
            Offset::new(10.0, 10.0),
            PointerType::Touch,
        )),
    );

    assert_eq!(
        hits.load(Ordering::Relaxed),
        hits_after_down,
        "the held terminal event must not re-hit-test while the frame is uncommitted"
    );
    assert_eq!(
        primary.gestures().active_pointer_count(),
        1,
        "the active route must stay live until the held terminal event replays"
    );
    assert_eq!(primary.held_pointer_input().borrow().len(), 1);

    realm.pipeline_for_test().with_mut(|owner| {
        let root = owner.root_id().expect("root installed");
        owner.mark_needs_paint(root);
    });
    realm.request_redraw();
    assert!(realm.render_frame(&mut backend));
    assert_eq!(primary.held_pointer_input().borrow().len(), 0);
    assert_eq!(
        primary.gestures().active_pointer_count(),
        0,
        "the replayed Up must run through the normal terminal route and release the cached hit path"
    );
}

#[test]
fn a_nonempty_held_queue_keeps_later_pointer_input_held_after_commit() {
    let (realm, hits) = mount_hit_counting_root();
    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));
    let clean_hits = hits.load(Ordering::Relaxed);

    let primary = realm.presentations.primary();
    primary
        .held_pointer_input()
        .borrow_mut()
        .append(make_down_event(Offset::new(10.0, 10.0), PointerType::Mouse));
    assert_eq!(primary.frame_commit_state(), FrameCommitState::Committed);

    let move_event = make_move_event(Offset::new(11.0, 11.0), PointerType::Mouse);
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
        .append(make_down_event(Offset::new(10.0, 10.0), PointerType::Mouse));
    let mut hover = make_move_event(Offset::new(11.0, 11.0), PointerType::Mouse);
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
            Offset::new(10.0, 10.0),
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
            Offset::new(10.0, 10.0),
            PointerType::Touch,
        ));

    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));

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

/// A frame the backend owed content for and could not put on screen must be
/// RETAINED: the loop has to come back for it, and the retry has to find real
/// work when it does.
///
/// Both halves are asserted separately because only the second one is easy to
/// get wrong. Waking alone re-opens `draw_frame_entered`'s segment gate but
/// leaves `PipelineOwner` with nothing dirty — the frame that could not be
/// shown already consumed the state that produced its scene — so a retry that
/// only wakes produces `Idle`, never reaches `render_scene`, and parks
/// (the shape issue #637 records for the submit-failure arms). Asserting on
/// the backend being ASKED AGAIN is what pins that, where asserting on the
/// wake bit alone would pass with the repaint missing.
#[test]
fn a_frame_the_surface_never_showed_is_retained_and_repainted() {
    let realm = mount_box();
    let mut backend = ScriptedSink::new(|call, _scene| {
        if call == 0 {
            SubmitVerdict::NotShown
        } else {
            SubmitVerdict::Presented
        }
    });

    assert!(
        !realm.render_frame(&mut backend),
        "a frame whose surface was unavailable did not present"
    );
    assert_eq!(backend.submit_calls, 1);
    assert_eq!(
        primary_state(&realm),
        FrameCommitState::Committed,
        "the pipeline DID produce this frame, so its tree revision commits -- \
         what failed was showing it, not making it"
    );

    let _ = realm.render_frame(&mut backend);
    assert_eq!(
        backend.submit_calls, 2,
        "the retained frame must be handed to the backend again, not merely \
         have its wake bit set"
    );
}

/// The withheld retry is BOUNDED: a drawable that never comes back stops
/// being retried instead of spinning at the fallback pace forever.
///
/// This is the one rule separating "ride out a transient" from "loop
/// forever", and AppKit's occlusion gate does not provide it — that gate keys
/// off `occlusionState`, which the cold-start trace behind this arm has
/// reporting the window VISIBLE for the whole ~132 ms the drawable was
/// unavailable. Retrying is itself what re-dirties the presentation, so with
/// no cap a surface that stays withdrawn produces one frame per fallback
/// period indefinitely.
#[test]
fn the_withheld_retry_is_bounded_and_then_parks() {
    let budget = super::MAX_NOT_SHOWN_RETRIES;
    let realm = mount_box();
    // Every attempt is withheld, including the one that exhausts the budget,
    // so what the test measures is WHERE the loop stopped rather than that it
    // stopped only because a frame finally succeeded.
    let mut backend = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);

    for _ in 0..=budget {
        assert!(
            !realm.render_frame(&mut backend),
            "a withheld frame never reports a present"
        );
    }
    assert_eq!(
        backend.submit_calls,
        budget + 1,
        "the budget is spent by RE-ARMING: {budget} retained attempts, then \
         the attempt that exhausts it and runs without arming another"
    );

    for _ in 0..8 {
        let _ = realm.render_frame(&mut backend);
    }
    assert_eq!(
        backend.submit_calls,
        budget + 1,
        "once the budget is spent and nothing else is dirty the loop must \
         park: no further frame may reach the backend"
    );
}
