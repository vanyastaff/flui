use std::cell::Cell;
use std::sync::atomic::{AtomicBool as StdAtomicBool, AtomicUsize};

use flui_engine::EngineError;
use flui_types::geometry::px;

use super::*;

/// Minimal leaf view/element so a headless `attach_root_widget` has
/// something to mount without pulling in a widget crate.
#[derive(Clone)]
struct LeafView;

impl flui_view::RenderView for LeafView {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        flui_objects::RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_size(
            Some(flui_types::Pixels::ZERO),
            Some(flui_types::Pixels::ZERO),
        )
    }
}

impl flui_view::View for LeafView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

fn test_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
}

#[derive(Clone)]
struct PanicsOnFirstBuild {
    should_panic: Rc<Cell<bool>>,
}

impl flui_view::StatelessView for PanicsOnFirstBuild {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        assert!(
            !self.should_panic.replace(false),
            "intentional first-frame build panic"
        );
        LeafView
    }
}

impl flui_view::View for PanicsOnFirstBuild {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// Recovered-panic expiry belongs to every produced presentation
/// frame, not only frames whose widget tree is dirty. Before the
/// unconditional widget-frame entry, the second pipeline-only frame
/// skipped `WidgetsBinding::draw_frame` and left the first frame's
/// contained build-panic record drainable indefinitely.
#[test]
fn a_pipeline_only_frame_discards_the_prior_frames_undrained_recovered_panics() {
    let realm = UiRealm::for_test();
    let should_panic = Rc::new(Cell::new(true));
    realm
        .attach_root_widget(&PanicsOnFirstBuild {
            should_panic: Rc::clone(&should_panic),
        })
        .expect("root attaches");

    let _ = realm.draw_frame(test_constraints());
    assert!(
        !realm.widgets().has_pending_builds(),
        "the next frame must have no widget build work"
    );
    assert!(
        !should_panic.get(),
        "the first frame exercised the contained build-panic producer"
    );

    realm.pipeline_for_test().with_mut(|owner| {
        let root_id = owner.root_id().expect("attached render root");
        owner.mark_needs_paint(root_id);
    });
    assert!(
        !realm.widgets().has_pending_builds(),
        "render dirtiness must not manufacture widget work"
    );

    let _ = realm.draw_frame(test_constraints());
    assert!(
        realm.widgets().take_recovered_panics().is_empty(),
        "the pipeline-only frame entry must discard the stale batch"
    );
}

#[test]
fn dirty_mark_fires_wake_via_notifier() {
    let realm = UiRealm::for_test();
    let pipeline = realm.pipeline_for_test();

    let id = pipeline.with_mut(|owner| {
        owner.insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >)
    });
    pipeline.with_mut(PipelineOwner::clear_all_dirty_nodes);
    realm.mark_rendered();
    // The insert above already dirtied the pipeline and, through the
    // unified carrier, flipped the scheduler's `frame_scheduled` latch
    // (its false->true edge is what fires the wake hook). Clear that
    // latch so the dirty mark below is a genuine false->true edge, the
    // only edge the wake hook fires on.
    realm.scheduler().finish_async_pump();

    pipeline.with_mut(|owner| owner.mark_needs_layout(id));
    assert!(
        realm.needs_redraw(),
        "an owner dirty mark must wake the realm via the visual-update \
         notifier wired in UiRealm::construct",
    );
}

#[test]
fn cross_thread_dirty_handle_wakes_owner_binding_not_worker_tls() {
    let realm = UiRealm::for_test();
    realm.mark_rendered();

    let pipeline = realm.pipeline_for_test();
    let id = pipeline.with_mut(|owner| {
        owner.insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >)
    });
    let handle = pipeline
        .with(|owner| owner.render_invalidation_handle(id))
        .expect("fresh render node is attached");
    std::thread::spawn(move || {
        handle
            .mark_needs_paint()
            .expect("dirty request should enqueue");
    })
    .join()
    .expect("worker thread should not panic");

    assert!(
        realm.needs_redraw(),
        "cross-thread node invalidation must wake the owner realm captured \
         during UiRealm construction, not resolve a worker-local TLS realm"
    );
}

#[test]
fn test_needs_redraw() {
    let realm = UiRealm::for_test();

    realm.mark_rendered();
    assert!(!realm.needs_redraw());

    realm.request_redraw();
    assert!(realm.needs_redraw());

    realm.mark_rendered();
    assert!(!realm.needs_redraw());
}

#[test]
fn test_renderer_initialized() {
    let realm = UiRealm::for_test();
    // Verify the renderer sub-binding is accessible (created during
    // UiRealm::construct).
    let _renderer = realm.renderer();
}

/// E2/E3 regression: `UiRealm` hands its shared `PipelineOwner` to the
/// `WidgetsBinding` it owns, so `attach_root_widget` actually
/// bootstraps the root render tree.
#[test]
fn attach_root_widget_bootstraps_shared_render_tree() {
    let realm = UiRealm::for_test();
    realm
        .enter(|realm| realm.attach_root_widget(&LeafView))
        .expect("attach succeeds");
    assert!(
        realm
            .pipeline_for_test()
            .with(|owner| owner.root_id().is_some()),
        "UiRealm must pass its PipelineOwner to the widgets binding so the \
         root render tree bootstraps; without it the window renders nothing",
    );
}

/// Root-hop parent-link regression: after a standard bootstrap
/// (`attach_root_widget` + a build/layout/paint `draw_frame`), the
/// mounted leaf's render node must have a working parent link back
/// to the root, not just the root's child-list entry.
#[test]
fn transform_to_resolves_through_the_root_hop_after_standard_bootstrap() {
    let realm = UiRealm::for_test();
    realm
        .enter(|realm| realm.attach_root_widget(&LeafView))
        .expect("attach succeeds");
    let _ = realm.draw_frame(test_constraints());

    realm.pipeline_for_test().with(|owner| {
        let root_id = owner.root_id().expect("root id set by attach_root_widget");
        let root_node = owner
            .render_tree()
            .get(root_id)
            .expect("root render node resolves");
        let leaf_id = *root_node
            .children()
            .first()
            .expect("LeafView must have mounted one render child under the root");

        assert_eq!(
            owner
                .render_tree()
                .get(leaf_id)
                .and_then(flui_rendering::storage::RenderNode::parent),
            Some(root_id),
            "the leaf's render node must carry a parent link back to the root"
        );

        let transform = owner.transform_to(leaf_id, root_id);
        assert!(
            transform.is_some(),
            "transform_to(leaf, root) must resolve through the root hop; None means the \
             ancestor walk broke at the very first step"
        );
        assert_eq!(
            transform,
            Some(flui_types::Matrix4::IDENTITY),
            "LeafView (RenderSizedBox::shrink(), zero offset) composes to the identity \
             transform into root space"
        );
    });
}

/// Wiring test: `draw_frame` must invoke
/// `WidgetsBinding::service_child_requests`, which drains the
/// pipeline's `pending_child_requests` buffer.
#[test]
fn draw_frame_invokes_service_child_requests() {
    let realm = UiRealm::for_test();
    let pipeline = realm.pipeline_for_test();

    let sliver_id = pipeline.with_mut(|owner| {
        owner.insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >)
    });
    pipeline.with_mut(|owner| owner.push_pending_child_request_for_test(sliver_id, 0));
    pipeline.with_mut(|owner| {
        let drained = owner.take_pending_child_requests();
        assert_eq!(drained.len(), 1, "seed must be present before draw_frame");
        owner.push_pending_child_request_for_test(sliver_id, 0);
    });

    let _ = realm.draw_frame(test_constraints());

    let remaining = pipeline.with_mut(PipelineOwner::take_pending_child_requests);
    assert!(
        remaining.is_empty(),
        "draw_frame must drain pending_child_requests via service_child_requests; \
         {} request(s) remained undrained — wiring is absent",
        remaining.len(),
    );
}

/// The production frame path polls the async driver **exactly once**,
/// on the realm's own scheduler, in the mid-frame slot — and the
/// pipeline runs afterwards, in the persistent slot.
#[test]
fn the_production_frame_polls_the_realms_async_driver_once_before_the_pipeline() {
    let realm = UiRealm::for_test();
    let scheduler = realm.scheduler();

    let polls = Arc::new(AtomicUsize::new(0));
    let polls_for_task = Arc::clone(&polls);
    let _token = scheduler.spawn_local(Box::pin(async move {
        polls_for_task.fetch_add(1, Ordering::Release);
    }));
    assert_eq!(
        polls.load(Ordering::Acquire),
        0,
        "spawn must not poll inline"
    );

    let polled_before_pipeline = Arc::new(StdAtomicBool::new(false));
    let flag = Arc::clone(&polled_before_pipeline);
    let polls_probe = Arc::clone(&polls);

    scheduler.drive_frame_with_lane(
        flui_scheduler::Instant::now(),
        flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
        || {
            flag.store(polls_probe.load(Ordering::Acquire) == 1, Ordering::Release);
            let _ = realm.draw_frame(test_constraints());
        },
        realm.local_post_frame_lane(),
    );

    assert!(
        polled_before_pipeline.load(Ordering::Acquire),
        "the async driver must be polled before the pipeline runs"
    );
    assert_eq!(
        polls.load(Ordering::Acquire),
        1,
        "exactly one driver poll per frame"
    );
}

/// `draw_frame` no longer polls the driver itself.
#[test]
fn draw_frame_does_not_poll_the_async_driver_itself() {
    let realm = UiRealm::for_test();
    let ran = Arc::new(StdAtomicBool::new(false));
    let ran_for_task = Arc::clone(&ran);
    let _token = realm.scheduler().spawn_local(Box::pin(async move {
        ran_for_task.store(true, Ordering::Release);
    }));

    let _ = realm.draw_frame(test_constraints());

    assert!(
        !ran.load(Ordering::Acquire),
        "the driver step belongs to UpdateScheduler::handle_begin_frame, not to the pipeline"
    );
}

/// **The production-path acceptance test.** A post-frame callback on
/// the realm's own scheduler observes the geometry this realm's real
/// pipeline committed **in the same frame**.
#[test]
fn production_post_frame_callback_observes_this_frames_committed_layout() {
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
        ) -> flui_types::Size {
            flui_types::Size::new(px(40.0), px(24.0))
        }
        fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
    }

    let realm = UiRealm::for_test();
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

    let scheduler = realm.scheduler();
    // `PipelineCell` is `!Send`, so this callback cannot go through
    // `add_post_frame_callback` (its `Send` bound is for cross-thread
    // wake). `LocalPostFrameHandle::schedule_local` accepts it
    // instead, matching the `editable_text.rs` IME cursor-loop
    // pattern.
    let post_frame_handle = realm
        .widgets()
        .with_build_owner(|owner| owner.local_post_frame_handle().cloned())
        .expect("owner-local post-frame handle installed by UiRealm::construct");
    // The handle addresses its lane directly (a `Weak` pointer), so
    // registration needs no `enter()` at all — only the drive itself
    // needs the lane, passed explicitly to `drive_frame_with_lane`
    // (drain-by-parameter, not an ambient "active lane" lookup).
    post_frame_handle
        .schedule_local(move |_timing| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            // PORT-CHECK-OK-LOCK: plain data: Option<Size>, no Drop
            *observed_cb.write() = pipeline_cb.with(|owner| owner.box_size(root));
        })
        .expect("the realm's local post-frame lane outlives this call");

    realm.enter(|realm| {
        scheduler.drive_frame_with_lane(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {
                let _ =
                    realm.draw_frame(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)));
            },
            &realm.local_post_frame,
        );
    });

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *observed.read(),
        Some(flui_types::Size::new(px(40.0), px(24.0))),
        "the production post-frame callback must observe THIS frame's layout"
    );
}

/// Wiring test: `draw_frame` must run the shared layout<->build
/// fixpoint (`BuildOwner::run_frame_with_layout_builders`), not a bare
/// `PipelineOwner::run_frame`.
#[test]
fn draw_frame_invokes_the_layout_builder_seam() {
    let realm = UiRealm::for_test();

    realm.widgets().with_build_owner_mut(|owner| {
        let _cell = owner.register_layout_builder_for_test(
            flui_foundation::RenderId::new(1),
            flui_foundation::ElementId::new(1),
        );
        assert_eq!(owner.layout_builder_count(), 1);
    });

    let _ = realm.draw_frame(test_constraints());

    realm.widgets().with_build_owner_mut(|owner| {
        assert_eq!(
            owner.layout_builder_count(),
            0,
            "draw_frame must run service_layout_builders (via the shared \
             run_frame_with_layout_builders helper), which prunes the stale entry"
        );
    });
}

/// Wake-gate contract: after a frame marks a render node dirty,
/// `has_pending_work()` must return `true` so the runner gate
/// schedules the settling frame; once no nodes are dirty,
/// `has_pending_work()` is `false` and the app can go idle.
#[test]
fn wake_gate_schedules_settling_frame_after_dirty_mark() {
    let realm = UiRealm::for_test();
    let pipeline = realm.pipeline_for_test();

    realm.mark_rendered();
    assert!(!realm.needs_redraw(), "precondition: needs_redraw clear");

    let node_id = pipeline.with_mut(|owner| {
        owner.insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >)
    });
    pipeline.with_mut(PipelineOwner::clear_all_dirty_nodes);
    assert!(
        !realm.has_pending_work(),
        "baseline: no pending work after clearing dirty nodes",
    );

    pipeline.with_mut(|owner| owner.mark_needs_layout(node_id));
    assert!(
        realm.has_pending_work(),
        "a dirty layout node must make has_pending_work() true so the runner \
         schedules the settling frame",
    );

    pipeline.with_mut(PipelineOwner::clear_all_dirty_nodes);
    assert!(
        !realm.has_pending_work(),
        "after clearing dirty nodes has_pending_work() must be false so a \
         settled lazy-list app does not loop forever",
    );
}

// ---- Input lifecycle gate --------------------------------------------

/// `Suspended` drops pointer input only — gesture-arena protection —
/// while keyboard/IME continue to flow. A flaky or absent occlusion
/// signal (the web backend wires none) must never become a
/// keystroke blackout.
///
/// If reverted: remove the `Suspended` pointer-only arm from
/// `input_dropped_by_lifecycle` and the pointer assertions below
/// fail (the arena receives the down and `needs_redraw` is armed
/// even though the presentation is suspended).
#[test]
fn pointer_input_is_dropped_while_suspended_but_keyboard_flows() {
    use flui_interaction::events::{PointerType, make_down_event};

    let realm = UiRealm::for_test();
    realm.synchronize_window_lifecycle();
    realm.mark_rendered();
    realm.update_host_lifecycle(AppLifecycleState::Hidden);

    let position = flui_types::Offset::new(px(50.0), px(50.0));
    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
            position,
            PointerType::Mouse,
        )));
    });
    assert_eq!(
        realm.gestures().active_pointer_count(),
        0,
        "a pointer down while suspended must never reach the gesture arena"
    );
    assert!(
        !realm.needs_redraw(),
        "a dropped pointer event must never arm a redraw"
    );

    // Keyboard/IME keep flowing while suspended: falling through to
    // the ordinary dispatch path is observable because that path
    // (unlike the pointer early-return above) always requests a
    // redraw.
    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Ime(flui_types::ImeEvent::Commit(
            "suspended-ime".to_string(),
        )));
    });
    assert!(
        realm.needs_redraw(),
        "IME input must keep flowing while the presentation is only suspended"
    );

    // Resume: pointer flows again.
    realm.mark_rendered();
    realm.update_host_lifecycle(AppLifecycleState::Resumed);
    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
            position,
            PointerType::Mouse,
        )));
    });
    assert_eq!(
        realm.gestures().active_pointer_count(),
        1,
        "pointer input must reach the arena again once resumed"
    );
}

/// `Closing`/`Closed` is a hard gate: every input kind is dropped,
/// not just pointer.
///
/// If reverted: remove the `Closing | Closed` hard-gate arm from
/// `input_dropped_by_lifecycle` and the IME assertion below fails (a
/// "closed" presentation still dispatches and arms a redraw).
#[test]
fn all_input_dropped_after_close() {
    use flui_interaction::events::{PointerType, make_down_event};

    let realm = UiRealm::for_test();
    realm.stop_presentations();

    let position = flui_types::Offset::new(px(50.0), px(50.0));
    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
            position,
            PointerType::Mouse,
        )));
    });
    assert_eq!(
        realm.gestures().active_pointer_count(),
        0,
        "pointer input must never reach the arena once closed"
    );
    assert!(
        !realm.needs_redraw(),
        "no input at all may reach dispatch once closed"
    );

    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Ime(flui_types::ImeEvent::Commit(
            "closed-ime".to_string(),
        )));
    });
    assert!(
        !realm.needs_redraw(),
        "IME input must also be dropped once closed — the hard gate covers every kind"
    );
}

/// Exhaustively pins [`input_dropped_by_lifecycle`]'s policy over
/// every `(lifecycle, input kind)` pair, including `DragDrop`.
///
/// If reverted: restore a `matches!(input, PlatformInput::Pointer(_))`
/// check in place of the exhaustive match and the `Suspended` +
/// `DragDrop` case fails (`assert!` sees `false` instead of `true`).
#[test]
fn input_lifecycle_gate_is_exhaustive_and_explicit() {
    use flui_interaction::events::{PointerType, make_down_event};
    use flui_interaction::testing::input::KeyEventBuilder;
    use flui_platform::traits::DragDropEvent;

    use super::super::super::presentation::PresentationLifecycle;

    let pointer = PlatformInput::Pointer(make_down_event(
        flui_types::Offset::new(px(0.0), px(0.0)),
        PointerType::Mouse,
    ));
    let keyboard =
        PlatformInput::Keyboard(KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build());
    let ime = PlatformInput::Ime(flui_types::ImeEvent::Commit("x".to_string()));
    let drag_drop = PlatformInput::DragDrop(DragDropEvent::Exited {
        id: flui_foundation::DataTransferId::new(1),
    });
    let every_kind = [&pointer, &keyboard, &ime, &drag_drop];

    for lifecycle in [
        PresentationLifecycle::Created,
        PresentationLifecycle::Closing,
        PresentationLifecycle::Closed,
    ] {
        for input in every_kind {
            assert!(
                input_dropped_by_lifecycle(lifecycle, input),
                "{lifecycle:?} must reject every input kind, including {input:?}"
            );
        }
    }

    assert!(input_dropped_by_lifecycle(
        PresentationLifecycle::Suspended,
        &pointer
    ));
    assert!(
        input_dropped_by_lifecycle(PresentationLifecycle::Suspended, &drag_drop),
        "DragDrop must be dropped while suspended, the same as Pointer"
    );
    assert!(!input_dropped_by_lifecycle(
        PresentationLifecycle::Suspended,
        &keyboard
    ));
    assert!(!input_dropped_by_lifecycle(
        PresentationLifecycle::Suspended,
        &ime
    ));

    for input in every_kind {
        assert!(!input_dropped_by_lifecycle(
            PresentationLifecycle::SurfaceAttached,
            input
        ));
    }
}

// ---- Gesture-arena / pointer dispatch --------------------------------

/// Shell auto-wrap regression: the `GestureArenaScope` `attach_root_widget*`
/// installs around the root must put every `GestureDetector` in the
/// gesture binding's ONE shared arena, so two nested detectors on the
/// same hit-test path resolve to exactly one winner (Flutter parity:
/// front member wins, loser is rejected). Without the shell wrap each
/// detector falls back to a private arena it closes itself, and the
/// same tap fires BOTH callbacks.
///
/// Drives the exact production path, with no manually mounted scope:
/// `attach_root_widget_with_size` (what every runner's bootstrap
/// calls) for the mount, `draw_frame` for layout, and
/// `handle_input_entered` (what every platform input callback calls
/// after entering a realm) for the pointer stream.
#[test]
fn shell_installed_arena_resolves_nested_tap_detectors_to_one_winner() {
    use flui_interaction::events::{PointerType, make_down_event, make_up_event};
    use flui_types::Color;
    use flui_widgets::{ColoredBox, GestureDetector};

    let realm = UiRealm::for_test();

    let inner_taps = Arc::new(AtomicUsize::new(0));
    let outer_taps = Arc::new(AtomicUsize::new(0));
    let inner = Arc::clone(&inner_taps);
    let outer = Arc::clone(&outer_taps);

    let root = GestureDetector::new()
        .on_tap(move || {
            outer.fetch_add(1, Ordering::SeqCst);
        })
        .child(
            GestureDetector::new()
                .on_tap(move || {
                    inner.fetch_add(1, Ordering::SeqCst);
                })
                .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        );

    realm
        .enter(|realm| realm.attach_root_widget_with_size(&root, 100.0, 100.0))
        .expect("attach succeeds");
    let _ = realm.draw_frame(test_constraints());
    realm.presentations.primary().commit_tree_revision();
    assert_eq!(
        realm.presentations.primary().frame_commit_state(),
        FrameCommitState::Committed
    );
    assert!(
        realm
            .presentations
            .primary()
            .held_pointer_input()
            .borrow()
            .is_empty()
    );

    // Production input arrives inside the realm (runner.rs's
    // PlatformToUi dispatch enters it before calling handle_input),
    // so the synthetic tap does the same.
    let position = flui_types::Offset::new(px(50.0), px(50.0));
    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
            position,
            PointerType::Mouse,
        )));
        realm.handle_input_entered(PlatformInput::Pointer(make_up_event(
            position,
            PointerType::Mouse,
        )));
    });

    assert_eq!(
        inner_taps.load(Ordering::SeqCst),
        1,
        "the inner tap recognizer is the arena's front member and wins",
    );
    assert_eq!(
        outer_taps.load(Ordering::SeqCst),
        0,
        "the outer tap recognizer shares the arena and must be rejected — \
         if both fire, each detector built its own private arena (no shell \
         GestureArenaScope above the root)",
    );
}

/// Same auto-wrap invariant as
/// `shell_installed_arena_resolves_nested_tap_detectors_to_one_winner`,
/// through `attach_root_widget` (the unsized variant) and asserting
/// on the arena's `SweepModel` directly.
#[test]
fn root_gesture_scope_arbitrates_overlapping_detectors_once() {
    use flui_interaction::arena::SweepModel;
    use flui_interaction::events::{PointerType, make_down_event, make_up_event};
    use flui_types::geometry::{Offset, Pixels};
    use flui_widgets::{GestureDetector, HitTestBehavior, SizedBox};

    let realm = UiRealm::for_test();
    let outer_taps = Rc::new(Cell::new(0));
    let inner_taps = Rc::new(Cell::new(0));

    let inner_count = Rc::clone(&inner_taps);
    let inner = GestureDetector::new()
        .on_tap(move || inner_count.set(inner_count.get() + 1))
        .behavior(HitTestBehavior::Opaque)
        .child(SizedBox::new(100.0, 100.0));
    let outer_count = Rc::clone(&outer_taps);
    let root = GestureDetector::new()
        .on_tap(move || outer_count.set(outer_count.get() + 1))
        .behavior(HitTestBehavior::Opaque)
        .child(inner);

    realm
        .attach_root_widget(&root)
        .expect("a fresh realm must attach the detector tree");
    let _ = realm.draw_frame(test_constraints());
    realm.presentations.primary().commit_tree_revision();
    assert_eq!(
        realm.presentations.primary().frame_commit_state(),
        FrameCommitState::Committed
    );
    assert!(
        realm
            .presentations
            .primary()
            .held_pointer_input()
            .borrow()
            .is_empty()
    );

    let position = Offset::new(Pixels(10.0), Pixels(10.0));
    let down = make_down_event(position, PointerType::Touch);
    let up = make_up_event(position, PointerType::Touch);
    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(down));
        realm.handle_input_entered(PlatformInput::Pointer(up));
    });

    assert_eq!(
        outer_taps.get() + inner_taps.get(),
        1,
        "overlapping detectors must compete in one arena; private arenas let both taps fire"
    );
    assert_eq!(
        realm.gestures().arena().sweep_model(),
        SweepModel::BindingDriven,
        "the root scope must expose the production binding-owned arena"
    );
    assert_eq!(realm.gestures().active_pointer_count(), 0);
}

/// Two independently constructed realms must never observe each
/// other's gesture-router registrations or arena state — the
/// per-realm isolation `UiRealm::for_test()` (a fully independent
/// pipeline/presentation/gesture-binding triple) is supposed to give.
#[test]
fn realm_input_dispatch_keeps_gesture_state_isolated() {
    use flui_interaction::PointerId;
    use flui_interaction::events::{PointerType, make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::routing::PointerRouteHandler;
    use flui_types::geometry::{Offset, Pixels};

    let realm_a = UiRealm::for_test();
    let realm_b = UiRealm::for_test();
    let pointer = PointerId::new(9001).expect("nonzero pointer id");
    let position = Offset::new(Pixels(10.0), Pixels(10.0));

    let fired = Rc::new(Cell::new(0));
    let fired_by_route = Rc::clone(&fired);
    let handler: PointerRouteHandler = Rc::new(move |_| {
        fired_by_route.set(fired_by_route.get() + 1);
    });
    realm_a
        .gestures()
        .pointer_router()
        .add_route(pointer, handler);

    let down_b = make_down_event_for_id(pointer, position, PointerType::Touch);
    realm_b.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(down_b));
    });

    assert_eq!(
        fired.get(),
        0,
        "a route registered in realm A must not observe realm B input"
    );
    assert_eq!(realm_a.gestures().active_pointer_count(), 0);
    assert_eq!(realm_b.gestures().active_pointer_count(), 1);

    let down_a = make_down_event_for_id(pointer, position, PointerType::Touch);
    realm_a.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(down_a));
    });

    assert_eq!(
        fired.get(),
        1,
        "realm A must dispatch through its own router"
    );
    assert_eq!(realm_a.gestures().active_pointer_count(), 1);
    assert_eq!(realm_b.gestures().active_pointer_count(), 1);

    let up_b = make_up_event_for_id(pointer, position, PointerType::Touch);
    realm_b.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(up_b));
    });
    let up_a = make_up_event_for_id(pointer, position, PointerType::Touch);
    realm_a.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(up_a));
    });

    realm_a
        .gestures()
        .pointer_router()
        .remove_all_routes(pointer);
    assert_eq!(realm_a.gestures().active_pointer_count(), 0);
    assert_eq!(realm_b.gestures().active_pointer_count(), 0);
}

struct CountingArenaAcceptance(Arc<AtomicU64>);

impl flui_interaction::sealed::CustomGestureRecognizer for CountingArenaAcceptance {
    fn on_arena_accept(&self, _pointer: flui_interaction::PointerId) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn on_arena_reject(&self, _pointer: flui_interaction::PointerId) {}
}

/// A lone arena member that accepts on `Down` must be swept and
/// drained by the SAME `handle_input_entered` call, leaving the
/// arena empty — no deferred second pass required for the
/// one-member case.
#[test]
fn pointer_input_boundary_drains_a_lone_deferred_winner() {
    use flui_interaction::events::{PointerType, make_down_event_for_id};
    use flui_interaction::routing::PointerRouteHandler;
    use flui_types::geometry::{Offset, Pixels};

    let realm = UiRealm::for_test();
    let pointer = flui_interaction::PointerId::new(9002).expect("nonzero pointer id");
    let accepted = Arc::new(AtomicU64::new(0));
    let arena = realm.gestures().arena().clone();
    let accepted_by_member = Arc::clone(&accepted);
    let handler: PointerRouteHandler = Rc::new(move |event| {
        if matches!(event, flui_interaction::PointerEvent::Down(_)) {
            arena.add(
                pointer,
                Arc::new(CountingArenaAcceptance(Arc::clone(&accepted_by_member))),
            );
        }
    });
    realm
        .gestures()
        .pointer_router()
        .add_route(pointer, Rc::clone(&handler));

    let down = make_down_event_for_id(
        pointer,
        Offset::new(Pixels(10.0), Pixels(10.0)),
        PointerType::Touch,
    );
    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(down));
    });

    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    assert!(realm.gestures().arena().is_empty());
    realm
        .gestures()
        .pointer_router()
        .remove_route(pointer, &handler);
}

/// Deadline keep-alive regression: a long-press armed by a pointer
/// down must fire at its 500ms deadline even when NO further pointer
/// events or redraw requests occur — the frame loop must keep
/// producing frames while a recognizer deadline is pending (each
/// frame's deadline tick re-requests the next), instead of going
/// idle once the down event's own frames drain. Without the
/// keep-alive the deadline tick never runs at the deadline and the
/// gesture never fires.
///
/// Simulates the runner's render loop against the real
/// `SystemClock`-driven gesture arena — consume `needs_redraw`, draw
/// a frame, repeat — after mounting through the production shell
/// path (`attach_root_widget_with_size`) and delivering the down
/// through `handle_input_entered`. The assertion is "fires at all",
/// never "fires on time", so a loaded CI machine cannot flake it.
///
/// If reverted: remove the `self.gestures().tick_deadlines()` call
/// from `draw_frame_entered` and this fails — the frame loop below
/// spins on `needs_redraw()`/`draw_frame()` forever with no deadline
/// tick ever advancing the recognizer, hitting the 5s timeout.
#[test]
fn long_press_fires_at_its_deadline_with_no_further_input() {
    use std::time::{Duration, Instant as StdInstant};

    use flui_interaction::events::{PointerType, make_down_event};
    use flui_types::Color;
    use flui_widgets::{ColoredBox, GestureDetector};

    let realm = UiRealm::for_test();

    let presses = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&presses);
    let root = GestureDetector::new()
        .on_long_press(move || {
            in_cb.fetch_add(1, Ordering::SeqCst);
        })
        .child(ColoredBox::new(Color::rgb(10, 20, 30)));

    realm
        .enter(|realm| realm.attach_root_widget_with_size(&root, 100.0, 100.0))
        .expect("attach succeeds");
    let constraints = test_constraints();
    let _ = realm.draw_frame(constraints);
    realm.presentations.primary().commit_tree_revision();
    assert_eq!(
        realm.presentations.primary().frame_commit_state(),
        FrameCommitState::Committed
    );
    assert!(
        realm
            .presentations
            .primary()
            .held_pointer_input()
            .borrow()
            .is_empty()
    );

    // Contact down — and nothing else, ever after. Production input
    // arrives inside the realm (runner.rs's RealmEvent dispatch
    // enters it before calling handle_input), so the synthetic down
    // does the same.
    let position = flui_types::Offset::new(px(50.0), px(50.0));
    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
            position,
            PointerType::Mouse,
        )));
    });

    // Simulated runner loop: the ONLY frame source is
    // `needs_redraw`, consumed the way the runner consumes it
    // (observe, render). The default long-press timeout is 500ms;
    // the cap is generous so the pass/fail signal is purely "did the
    // deadline ever fire".
    let deadline = StdInstant::now() + Duration::from_secs(5);
    while presses.load(Ordering::SeqCst) == 0 {
        assert!(
            StdInstant::now() < deadline,
            "long-press never fired: the frame loop went idle with an armed \
             recognizer deadline (no keep-alive frame was requested)",
        );
        if realm.needs_redraw() {
            realm.mark_rendered();
            let _ = realm.draw_frame(constraints);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "the held long-press fires exactly once",
    );
}

/// Resampled (manually clocked) pointer motion that has not yet been
/// assigned a frame timestamp must keep `has_pending_work()` true —
/// the wake-gate half of the same deadline/motion keep-alive
/// contract `long_press_fires_at_its_deadline_with_no_further_input`
/// exercises for deadlines.
#[test]
fn resampled_contact_motion_keeps_the_frame_wake_gate_open() {
    use std::time::Duration;

    use flui_interaction::{
        events::{PointerType, make_down_event, make_move_event, make_up_event},
        processing::SamplingClock,
        routing::HitTestResult,
    };
    use flui_types::geometry::{Offset, Pixels};

    let realm = UiRealm::for_test();
    realm.mark_rendered();
    realm
        .pipeline_for_test()
        .with_mut(PipelineOwner::clear_all_dirty_nodes);

    realm.enter(|realm| {
        realm
            .gestures()
            .set_resampling_enabled(true)
            .expect("test configures sampling before Down");
        realm.gestures().set_sampling_clock(SamplingClock::Manual {
            period: Duration::from_millis(8),
        });

        let position = Offset::new(Pixels(8.0), Pixels(13.0));
        realm
            .gestures()
            .handle_pointer_event(&make_down_event(position, PointerType::Touch), |_| {
                HitTestResult::new()
            });
        realm
            .gestures()
            .handle_pointer_event(&make_move_event(position, PointerType::Touch), |_| {
                HitTestResult::new()
            });

        assert_eq!(
            realm.gestures().flush_pending_moves(),
            0,
            "manual sampling has no implicit frame timestamp"
        );
        assert!(realm.gestures().has_pending_motion());
        assert!(
            realm.has_pending_work(),
            "a sequence-owned sample waiting for frame time must keep the runner awake"
        );

        realm
            .gestures()
            .handle_pointer_event(&make_up_event(position, PointerType::Touch), |_| {
                HitTestResult::new()
            });
        assert!(!realm.gestures().has_pending_motion());
    });
}

// ---- Vsync wiring (production frame continuation) -------------------

fn make_controller(duration_ms: u64) -> flui_animation::AnimationController {
    use std::time::Duration;
    flui_animation::AnimationController::new(
        Duration::from_millis(duration_ms),
        &flui_scheduler::UpdateScheduler::new(),
    )
}

/// V1 — Frame continuation (the key test): a running controller
/// registered in the realm's Vsync must keep the runner gate
/// schedulable across every mid-animation frame, and the gate must
/// go idle once the controller completes.
///
/// Drives `render_frame_entered` (a fresh single-shot
/// `TestRasterBackend` per call), not the bare `draw_frame`
/// this test used before — the continuation wake this test pins is
/// now raised in `render_frame_entered`, AFTER `mark_rendered()`
/// runs, not inside `draw_frame_entered` (a reviewer
/// probe found that raising it before `mark_rendered()` let the
/// SAME callback's own `mark_rendered()` clobber it, silently
/// stalling a controller with no other tree-visible effect on the
/// real desktop path — see `render_frame_entered`'s own comment).
/// `draw_frame` alone no longer implies a continuation wake by
/// design; only `render_frame_entered` does, matching what
/// production always actually calls.
#[test]
fn vsync_continuation_keeps_gate_open_while_running_and_closes_on_settle() {
    use flui_animation::{Animation, AnimationStatus};

    let realm = UiRealm::for_test();
    let vsync = realm.vsync();

    let controller = make_controller(100);
    vsync.register(controller.clone());
    controller.forward().expect("fresh controller forwards");

    realm.set_now_secs_for_test(0.0);
    realm.mark_rendered();
    let mut backend = TestRasterBackend::single_shot(Ok(PresentDisposition::Presented));
    let _ = realm.render_frame_entered(&mut backend);
    // `needs_redraw()` is what actually carries this assertion in
    // this test, not `has_pending_work()`: no widget is attached
    // (a pure-animation scenario, matching this test's own point —
    // the controller alone keeps the gate open), so nothing ever
    // dirties the pipeline and `has_pending_work()` reads `false`
    // for the entire test (checked directly, not assumed).
    // `needs_redraw()` is `true` here because `render_frame_entered`'s
    // post-`mark_rendered()` continuation-wake step (see that
    // method's own comment) called `wake_frame()` for this still-
    // running controller. The `||` stays as the real production
    // predicate this test also exercises through the FrameClock
    // segment gate (`has_pending_work()` DOES carry other tests,
    // e.g. any widget-driven dirty state) — kept here for parity
    // with that predicate, not because this specific scenario
    // needs it.
    assert!(
        realm.needs_redraw() || realm.has_pending_work(),
        "V1: the runner gate must be open after an anchor frame",
    );

    realm.set_now_secs_for_test(0.05);
    realm.mark_rendered();
    let mut backend = TestRasterBackend::single_shot(Ok(PresentDisposition::Presented));
    let _ = realm.render_frame_entered(&mut backend);
    assert!(
        realm.needs_redraw() || realm.has_pending_work(),
        "V1: runner gate must remain open at t=0.05s",
    );
    let mid_value = controller.value();
    assert!(
        mid_value > 0.1 && mid_value < 0.95,
        "V1: sanity — controller is mid-run at t=50ms (value={mid_value})",
    );

    realm.set_now_secs_for_test(0.20);
    realm.mark_rendered();
    let mut backend = TestRasterBackend::single_shot(Ok(PresentDisposition::Presented));
    let _ = realm.render_frame_entered(&mut backend);
    assert_eq!(controller.status(), AnimationStatus::Completed);

    assert!(
        !realm.needs_redraw(),
        "V1: the runner gate must be CLOSED after settle",
    );
    assert!(!realm.has_vsync_running());

    controller.dispose();
}

/// V2 — Value advances across injected-time frames.
#[test]
fn vsync_value_advances_across_frames() {
    use flui_animation::Animation;

    let realm = UiRealm::for_test();
    let vsync = realm.vsync();
    let controller = make_controller(200);
    vsync.register(controller.clone());
    controller.forward().expect("fresh controller forwards");

    let constraints = test_constraints();

    realm.set_now_secs_for_test(0.0);
    let _ = realm.draw_frame(constraints);
    let v0 = controller.value();

    realm.set_now_secs_for_test(0.10);
    let _ = realm.draw_frame(constraints);
    let v1 = controller.value();

    assert!(
        v1 > v0,
        "V2: controller value must increase (v0={v0}, v1={v1})"
    );
    assert!(
        (v1 - 0.5).abs() < 0.05,
        "V2: at t=100ms/200ms run ~0.5 (got {v1})"
    );

    controller.dispose();
}

/// V3 — Exactly-once-per-frame (no double-advance).
#[test]
fn vsync_tick_exactly_once_per_frame() {
    use flui_animation::{Animation, AnimationStatus};

    let realm = UiRealm::for_test();
    let vsync = realm.vsync();
    let controller = make_controller(100);
    vsync.register(controller.clone());
    controller.forward().expect("fresh controller forwards");

    let constraints = test_constraints();

    realm.set_now_secs_for_test(0.0);
    let _ = realm.draw_frame(constraints);

    realm.set_now_secs_for_test(0.05);
    let _ = realm.draw_frame(constraints);
    assert_ne!(
        controller.status(),
        AnimationStatus::Completed,
        "V3: must NOT be complete at t=50ms (100ms duration)",
    );

    realm.set_now_secs_for_test(0.15);
    let _ = realm.draw_frame(constraints);
    assert_eq!(controller.status(), AnimationStatus::Completed);

    controller.dispose();
}

use flui_view::{IntoView, StatefulView, ViewState};

/// Test-local view that captures the auto-injected `VsyncScope` in
/// `init_state`, registers a caller-supplied controller, and starts
/// it running.
#[derive(Clone)]
struct VsyncProbeView {
    controller_to_register: flui_animation::AnimationController,
}

struct VsyncProbeState {
    controller: flui_animation::AnimationController,
}

impl StatefulView for VsyncProbeView {
    type State = VsyncProbeState;

    fn create_state(&self) -> Self::State {
        VsyncProbeState {
            controller: self.controller_to_register.clone(),
        }
    }
}

impl ViewState<VsyncProbeView> for VsyncProbeState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        use flui_view::BuildContextExt as _;
        if let Some(vsync) = ctx.get::<flui_widgets::VsyncScope, _>(|scope| scope.vsync().clone()) {
            vsync.register(self.controller.clone());
            self.controller.forward().ok();
        }
    }

    fn build(&self, _view: &VsyncProbeView, _ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        LeafView
    }
}

impl flui_view::View for VsyncProbeView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

fn make_vsync_probe() -> (VsyncProbeView, flui_animation::AnimationController) {
    use std::time::Duration;
    let controller = flui_animation::AnimationController::new(
        Duration::from_millis(200),
        &flui_scheduler::UpdateScheduler::new(),
    );
    let view = VsyncProbeView {
        controller_to_register: controller.clone(),
    };
    (view, controller)
}

/// A1 — Auto-wrap causes registration after the first build pass.
#[test]
fn a1_autowrap_causes_registration_after_build_pass() {
    let realm = UiRealm::for_test();
    let (probe, controller) = make_vsync_probe();

    realm
        .attach_root_widget(&probe)
        .expect("a fresh UiRealm must accept its first root widget");

    assert!(
        realm.vsync().is_empty(),
        "A1 precondition: controller must not be registered before the first build pass",
    );

    let _ = realm.draw_frame(test_constraints());

    assert!(
        !realm.vsync().is_empty(),
        "A1: after a build pass the controller registered in init_state must appear \
         in realm.vsync()",
    );

    controller.dispose();
}

/// A2 — End-to-end tick: auto-wrap -> register -> tick -> value advances.
#[test]
fn a2_autowrap_end_to_end_tick_advances_controller_value() {
    use flui_animation::Animation as _;

    let realm = UiRealm::for_test();
    let (probe, controller) = make_vsync_probe();
    realm
        .attach_root_widget(&probe)
        .expect("a fresh UiRealm must accept its first root widget");

    realm.set_now_secs_for_test(0.0);
    let _ = realm.draw_frame(test_constraints());
    assert!(!realm.vsync().is_empty());

    realm.set_now_secs_for_test(0.1);
    let _ = realm.draw_frame(test_constraints());
    let value_after_anchor = controller.value();

    realm.set_now_secs_for_test(0.2);
    let _ = realm.draw_frame(test_constraints());
    let value_at_50_percent = controller.value();

    assert!(
        value_at_50_percent > value_after_anchor,
        "A2: controller value must advance from the anchor frame to t=0.2s \
         (anchor={value_after_anchor}, t=200ms={value_at_50_percent})",
    );

    controller.dispose();
}

/// A3 — No-animation root: auto-wrap registers nothing itself.
#[test]
fn a3_no_animation_root_vsync_stays_empty_after_build_pass() {
    let realm = UiRealm::for_test();
    realm
        .attach_root_widget(&LeafView)
        .expect("a fresh UiRealm must accept its first root widget");

    let _ = realm.draw_frame(test_constraints());

    assert!(
        realm.vsync().is_empty(),
        "A3: a root with no implicitly-animated widgets must not register anything",
    );
}

/// V4 — No-animation app idles cheaply.
#[test]
fn vsync_empty_does_not_keep_gate_open() {
    let realm = UiRealm::for_test();
    assert!(realm.vsync().is_empty(), "precondition: Vsync is empty");

    let constraints = test_constraints();
    realm.set_now_secs_for_test(1.0);
    realm.mark_rendered();

    let _ = realm.draw_frame(constraints);

    assert!(
        !realm.has_vsync_running(),
        "V4: has_vsync_running() must be false when no controllers are registered",
    );
}

// ---- render_frame_entered retry / first-frame-deferral semantics ----

fn mount_root() -> UiRealm {
    let realm = UiRealm::for_test();
    realm
        .enter(|realm| realm.attach_root_widget(&LeafView))
        .expect("attach succeeds");
    realm
}

#[test]
fn surface_lost_keeps_needs_redraw_armed_for_a_retry() {
    let realm = mount_root();
    let mut backend = TestRasterBackend::single_shot(Err(EngineError::SurfaceLost));

    realm.mark_rendered();
    let presented = realm.render_frame_entered(&mut backend);

    assert!(!presented, "a SurfaceLost frame never reaches present()");
    assert_eq!(
        backend.render_scene_calls, 1,
        "precondition: the mounted scene actually reached render_scene"
    );
    assert!(
        realm.needs_redraw(),
        "a dropped SurfaceLost frame must re-arm needs_redraw so the next wake \
         actually retries"
    );
}

#[test]
fn device_lost_keeps_needs_redraw_armed_for_a_retry() {
    let realm = mount_root();
    let mut backend = TestRasterBackend::single_shot(Err(EngineError::DeviceLost));

    realm.mark_rendered();
    let presented = realm.render_frame_entered(&mut backend);

    assert!(!presented, "a DeviceLost frame never reaches present()");
    assert_eq!(
        backend.render_scene_calls, 1,
        "precondition: the mounted scene actually reached render_scene"
    );
    assert!(
        realm.needs_redraw(),
        "a dropped DeviceLost frame must re-arm needs_redraw: rebuilding the \
         device is the renderer owner's job, but on a quiescent loop nothing \
         else would ever wake the owner to retry the recovery"
    );
}

#[test]
fn surface_validation_keeps_needs_redraw_armed_for_a_retry() {
    let realm = mount_root();
    let mut backend = TestRasterBackend::single_shot(Err(EngineError::SurfaceValidation));

    realm.mark_rendered();
    let presented = realm.render_frame_entered(&mut backend);

    assert!(
        !presented,
        "a SurfaceValidation frame never reaches present()"
    );
    assert_eq!(
        backend.render_scene_calls, 1,
        "precondition: the mounted scene actually reached render_scene"
    );
    assert!(
        realm.needs_redraw(),
        "a dropped SurfaceValidation frame must re-arm needs_redraw so a later \
         wake gets another acquire attempt instead of parking the misconfigured \
         surface forever"
    );
}

#[test]
fn a_successful_frame_still_clears_needs_redraw() {
    let realm = mount_root();
    let mut backend = TestRasterBackend::single_shot(Ok(PresentDisposition::Presented));

    realm.request_redraw();
    let presented = realm.render_frame_entered(&mut backend);

    assert!(presented, "Ok(true) means render_scene reached present()");
    assert!(
        !realm.needs_redraw(),
        "a successfully presented frame must clear needs_redraw"
    );
}

#[test]
fn deferred_first_frame_runs_the_pipeline_but_withholds_the_scene() {
    let realm = mount_root();
    let mut backend = TestRasterBackend::always_presents();

    realm.defer_first_frame();
    realm.mark_rendered();

    let presented = realm.render_frame_entered(&mut backend);

    assert!(!presented, "a deferred first frame must never present");
    assert_eq!(
        backend.render_scene_calls, 0,
        "the scene must never reach render_scene while deferred"
    );
    assert_eq!(realm.frames_rendered(), 0);
    assert!(
        !realm.needs_redraw(),
        "deferred is not errored: it must not spam a retry wake"
    );
    // The property this test's own NAME claims, actually asserted:
    // the pipeline (build/layout/paint) really did run -- Flutter
    // parity, `.flutter/packages/flutter/lib/src/rendering/binding.dart:582-599`
    // (`RendererBinding.deferFirstFrame`'s own doc: "the framework
    // will still do all the work to produce frames"). Without this
    // assertion a regression that skips the WHOLE segment while
    // deferred (not just the submit) would still pass every check
    // above -- `render_scene_calls == 0` and `frames_rendered ==
    // 0` are also exactly what a fully-skipped segment produces.
    assert_eq!(
        realm.presentations.primary().flush_count(),
        1,
        "the segment must have actually run while deferred, not been skipped outright"
    );
}

#[test]
fn allow_first_frame_alone_presents_the_previously_withheld_content() {
    let realm = mount_root();
    let mut backend = TestRasterBackend::always_presents();

    realm.defer_first_frame();
    let withheld = realm.render_frame_entered(&mut backend);
    assert!(
        !withheld,
        "precondition: the first frame is withheld while deferred"
    );
    assert_eq!(backend.render_scene_calls, 0);

    realm.allow_first_frame();

    let presented = realm.render_frame_entered(&mut backend);

    assert!(
        presented,
        "allow_first_frame alone (no external re-dirty) must make the withheld \
         content reach present() on the next pumped frame"
    );
    assert_eq!(backend.render_scene_calls, 1);
    assert_eq!(realm.frames_rendered(), 1);
}

#[test]
fn nested_defer_allow_only_presents_after_the_last_allow() {
    let realm = mount_root();
    let mut backend = TestRasterBackend::always_presents();

    realm.defer_first_frame();
    realm.defer_first_frame();

    assert!(!realm.render_frame_entered(&mut backend));
    assert_eq!(backend.render_scene_calls, 0);

    realm.allow_first_frame();
    assert!(
        !realm.render_frame_entered(&mut backend),
        "one matching allow of two nested defers must not yet open the gate"
    );
    assert_eq!(backend.render_scene_calls, 0);

    realm.allow_first_frame();
    assert!(
        realm.render_frame_entered(&mut backend),
        "the last matching allow must open the gate"
    );
    assert_eq!(backend.render_scene_calls, 1);
}

#[test]
// The panic message changed with the migration from
// `RenderingFlutterBinding`'s own counter to `FrameClock::lift` --
// same caller-contract panic, new mechanism's own wording.
#[should_panic(expected = "FrameClock::lift called without a matching defer")]
fn allow_first_frame_without_matching_defer_panics() {
    let realm = UiRealm::for_test();
    realm.allow_first_frame();
}

/// Root `RenderBox` whose layout panics — the exact catch_unwind path
/// any third-party panic in production widget code reaches.
#[derive(Debug)]
struct PanicOnLayoutBox;

impl flui_foundation::Diagnosticable for PanicOnLayoutBox {}

impl flui_rendering::traits::RenderBox for PanicOnLayoutBox {
    type Arity = flui_rendering::prelude::Leaf;
    type ParentData = flui_rendering::prelude::BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_types::Size {
        panic!("PanicOnLayoutBox::perform_layout -- intentional test panic");
    }
}

fn mount_panicking_root() -> UiRealm {
    let realm = UiRealm::for_test();
    realm.pipeline_for_test().with_mut(|owner| {
        let root_id = owner.insert(Box::new(PanicOnLayoutBox)
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);
        owner.set_root_id(Some(root_id));
    });
    realm
}

#[test]
fn errored_first_frame_does_not_latch_first_frame_sent() {
    let realm = mount_panicking_root();
    let mut backend = TestRasterBackend::always_presents();

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let presented = realm.render_frame_entered(&mut backend);
    std::panic::set_hook(prev_hook);

    assert!(!presented, "an errored frame must never present");
    assert_eq!(backend.render_scene_calls, 0);

    assert!(realm.send_frames_to_engine());

    realm.defer_first_frame();
    assert!(
        !realm.send_frames_to_engine(),
        "an errored first frame must not latch first_frame_sent"
    );
}

#[test]
fn first_frame_sent_latch_short_circuits_later_defers() {
    let realm = mount_root();
    let mut backend = TestRasterBackend::always_presents();

    let presented = realm.render_frame_entered(&mut backend);
    assert!(
        presented,
        "precondition: the first frame presents with no active deferral"
    );
    assert!(realm.send_frames_to_engine());

    realm.defer_first_frame();
    assert!(
        realm.send_frames_to_engine(),
        "a defer registered AFTER the first frame was sent must not re-close the gate"
    );
}
