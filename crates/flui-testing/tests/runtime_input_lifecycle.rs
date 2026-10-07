//! Addressed input advances at the production realm frame and lifecycle boundaries.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Offset;
use flui_foundation::{ManualClock, PresentationId};
use flui_interaction::events::{
    PointerEvent, PointerType, make_down_event, make_move_event, make_move_event_for_id,
    make_up_event,
};
use flui_interaction::{GestureArenaMember, PointerId};
use flui_platform_api::{PlatformInput, PlatformWindow, WindowExecutionState};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_runtime::testing::{ScriptedSink, TestWindow};
use flui_runtime::ui_realm::UiRealm;
use flui_scheduler::AppLifecycleState;
use flui_view::prelude::*;
use flui_widgets::{Align, Listener, MouseRegion, SizedBox};

fn pump(realm: &mut UiRealm) {
    let _ = realm.pump(
        &mut ManualClock::default(),
        &mut ScriptedSink::always_presents(),
    );
}

fn dispatch(realm: &UiRealm, id: PresentationId, event: PointerEvent) {
    realm.enter(|realm| {
        realm.handle_input_addressed(id, PlatformInput::Pointer(event));
    });
}

fn install_secondary(realm: &mut UiRealm) -> PresentationId {
    let window: Arc<dyn PlatformWindow> = Arc::new(TestWindow::new().focused(false));
    let presentation = realm.assemble_presentation(window);
    realm.install_presentation(presentation)
}

fn hover() -> PointerEvent {
    let mut event = make_move_event(Offset::new(8.0, 9.0), PointerType::Mouse);
    if let PointerEvent::Move(update) = &mut event {
        update.current.buttons = Default::default();
    }
    event
}

pub(crate) fn a_secondary_contact_move_is_delivered_by_the_next_frame() {
    let mut realm = UiRealm::for_test();
    let secondary = install_secondary(&mut realm);
    let moves = Rc::new(Cell::new(0));
    let seen = moves.clone();
    realm
        .attach_root_widget_to_for_test(
            secondary,
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_move(move |_, _| seen.set(seen.get() + 1))
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("secondary root attaches");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(
        &realm,
        secondary,
        make_down_event(Offset::new(4.0, 6.0), PointerType::Touch),
    );
    dispatch(
        &realm,
        secondary,
        make_move_event(Offset::new(8.0, 9.0), PointerType::Touch),
    );
    assert_eq!(moves.get(), 0, "motion waits for frame cadence");
    pump(&mut realm);
    assert_eq!(
        moves.get(),
        1,
        "the secondary contact receives its queued motion"
    );
    pump(&mut realm);
    assert_eq!(moves.get(), 1, "a later frame cannot duplicate motion");
}

struct AcceptLog(Rc<Cell<usize>>);

impl GestureArenaMember for AcceptLog {
    fn accept_gesture(&self, _: PointerId) {
        self.0.set(self.0.get() + 1);
    }

    fn reject_gesture(&self, _: PointerId) {}
}

pub(crate) fn a_secondary_deferred_arena_verdict_is_delivered_by_the_next_frame() {
    let mut realm = UiRealm::for_test();
    let secondary = install_secondary(&mut realm);
    let accepted = Rc::new(Cell::new(0));
    let member = Rc::new(AcceptLog(accepted.clone()));
    let arena = realm.presentation_gestures_for_test(secondary).arena();
    let _entry = arena.add(PointerId::PRIMARY, &member);
    arena.close(PointerId::PRIMARY);
    assert_eq!(
        accepted.get(),
        0,
        "the lone verdict waits for an owner boundary"
    );
    pump(&mut realm);
    assert_eq!(
        accepted.get(),
        1,
        "the frame drains the secondary arena's accepted work"
    );
    pump(&mut realm);
    assert_eq!(accepted.get(), 1, "the verdict is delivered exactly once");
}

fn queued_hover_after_transition(paused: bool, held: bool) {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let hovers = Rc::new(Cell::new(0));
    let seen = hovers.clone();
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_hover(move |_, _| seen.set(seen.get() + 1))
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    if held {
        realm.defer_first_frame();
    }
    pump(&mut realm);
    dispatch(&realm, primary, hover());
    assert_eq!(hovers.get(), 0, "hover waits for frame cadence");
    if paused {
        realm.update_host_lifecycle(AppLifecycleState::Paused);
        realm.update_host_lifecycle(AppLifecycleState::Resumed);
    } else {
        realm.update_window_focus(primary, false);
    }
    if held {
        realm.allow_first_frame();
    }
    pump(&mut realm);
    assert_eq!(
        hovers.get(),
        usize::from(!paused),
        "pause discards pending input; blur preserves hover"
    );
    dispatch(&realm, primary, hover());
    pump(&mut realm);
    assert_eq!(
        hovers.get(),
        1 + usize::from(!paused),
        "fresh hovering still works after the transition"
    );
}

pub(crate) fn host_pause_discards_a_queued_hover_before_resume() {
    queued_hover_after_transition(true, false);
}

pub(crate) fn window_blur_keeps_a_queued_hover() {
    queued_hover_after_transition(false, false);
}

pub(crate) fn host_pause_discards_a_hover_held_before_the_first_commit() {
    queued_hover_after_transition(true, true);
}

pub(crate) fn host_pause_keeps_a_completed_held_tap_for_the_first_commit() {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let downs = Rc::new(Cell::new(0));
    let ups = Rc::new(Cell::new(0));
    let down = downs.clone();
    let up = ups.clone();
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| down.set(down.get() + 1))
                .on_pointer_up(move |_, _| up.set(up.get() + 1))
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    realm.defer_first_frame();
    pump(&mut realm);
    dispatch(
        &realm,
        primary,
        make_down_event(Offset::new(4.0, 6.0), PointerType::Touch),
    );
    dispatch(
        &realm,
        primary,
        make_up_event(Offset::new(4.0, 6.0), PointerType::Touch),
    );
    assert_eq!(
        (downs.get(), ups.get()),
        (0, 0),
        "the completed tap waits for commit"
    );
    realm.update_host_lifecycle(AppLifecycleState::Paused);
    realm.update_host_lifecycle(AppLifecycleState::Resumed);
    realm.allow_first_frame();
    pump(&mut realm);
    assert_eq!(
        (downs.get(), ups.get()),
        (1, 1),
        "a completed tap remains accepted work across suspension"
    );
    pump(&mut realm);
    assert_eq!(
        (downs.get(), ups.get()),
        (1, 1),
        "the completed tap replays exactly once"
    );
}

fn motion_probe(count: Rc<Cell<usize>>, fail: Rc<Cell<bool>>, message: &'static str) -> Listener {
    Listener::new()
        .behavior(HitTestBehavior::Opaque)
        .on_pointer_move(move |_, _| {
            count.set(count.get() + 1);
            if fail.get() {
                panic_any(message);
            }
        })
        .child(SizedBox::new(40.0, 40.0))
}

fn failing_frame_motion_still_delivers_the_sibling(secondary_panics: bool) {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let counts = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let failures = [Rc::new(Cell::new(false)), Rc::new(Cell::new(false))];
    realm
        .attach_root_widget(&motion_probe(
            counts[0].clone(),
            failures[0].clone(),
            "primary motion failed",
        ))
        .expect("primary root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    let secondary = install_secondary(&mut realm);
    realm
        .attach_root_widget_to_for_test(
            secondary,
            &motion_probe(
                counts[1].clone(),
                failures[1].clone(),
                "secondary motion failed",
            ),
        )
        .expect("secondary root attaches");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, false, true);
    pump(&mut realm);
    failures[0].set(true);
    failures[1].set(secondary_panics);
    for id in [primary, secondary] {
        dispatch(
            &realm,
            id,
            make_down_event(Offset::new(4.0, 6.0), PointerType::Touch),
        );
        dispatch(
            &realm,
            id,
            make_move_event(Offset::new(8.0, 9.0), PointerType::Touch),
        );
    }
    let failure = catch_unwind(AssertUnwindSafe(|| pump(&mut realm)))
        .expect_err("the frame propagates its first input failure");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"primary motion failed")
    );
    assert_eq!(
        (counts[0].get(), counts[1].get()),
        (1, 1),
        "both accepted motions complete before the first failure resumes"
    );
    for fail in failures {
        fail.set(false);
    }
    for id in [primary, secondary] {
        dispatch(
            &realm,
            id,
            make_move_event(Offset::new(12.0, 13.0), PointerType::Touch),
        );
    }
    pump(&mut realm);
    assert_eq!(
        (counts[0].get(), counts[1].get()),
        (2, 2),
        "both contact routes recover after containment"
    );
    pump(&mut realm);
    assert_eq!(
        (counts[0].get(), counts[1].get()),
        (2, 2),
        "recovery cannot replay old motion"
    );
}

pub(crate) fn a_panicking_primary_motion_does_not_erase_the_secondary_motion() {
    failing_frame_motion_still_delivers_the_sibling(false);
}

pub(crate) fn competing_frame_motion_failures_preserve_the_first_and_recover() {
    failing_frame_motion_still_delivers_the_sibling(true);
}

struct PauseDiagnosticPanic(Arc<AtomicUsize>);

impl tracing::Subscriber for PauseDiagnosticPanic {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target().ends_with("::binding") && *metadata.level() == tracing::Level::DEBUG
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}").contains(
                        "GestureBinding draining interrupted pointer state on lifecycle pause",
                    );
                }
            }
        }
        let mut message = Message(false);
        event.record(&mut message);
        if message.0 {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic_any("pause drain diagnostic failed");
        }
    }
}

fn pause_diagnostic_failure_still_drains_motion(cancel_panics: bool) {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let hovers = Rc::new(Cell::new(0));
    let cancels = Rc::new(Cell::new(0));
    let seen_hover = hovers.clone();
    let seen_cancel = cancels.clone();
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_hover(move |_, _| seen_hover.set(seen_hover.get() + 1))
                .on_pointer_cancel(move |_, _| {
                    seen_cancel.set(seen_cancel.get() + 1);
                    if cancel_panics {
                        panic_any("pointer cancel failed");
                    }
                })
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(
        &realm,
        primary,
        make_down_event(Offset::new(4.0, 6.0), PointerType::Touch),
    );
    let mut motion =
        make_move_event_for_id(PointerId::new(2), Offset::new(8.0, 9.0), PointerType::Mouse);
    if let PointerEvent::Move(update) = &mut motion {
        update.current.buttons = Default::default();
    }
    dispatch(&realm, primary, motion.clone());
    assert_eq!(hovers.get(), 0, "the mouse move is still queued");
    let diagnostics = Arc::new(AtomicUsize::new(0));
    let failed = catch_unwind(AssertUnwindSafe(|| {
        tracing::subscriber::with_default(PauseDiagnosticPanic(diagnostics.clone()), || {
            realm.update_host_lifecycle(AppLifecycleState::Paused);
        });
    }))
    .expect_err("pause retains its first callback or diagnostic failure");
    let first = if cancel_panics {
        "pointer cancel failed"
    } else {
        "pause drain diagnostic failed"
    };
    assert_eq!(failed.downcast_ref::<&str>(), Some(&first));
    assert!(
        diagnostics.load(Ordering::SeqCst) > 0,
        "the real pause diagnostic reached the subscriber"
    );
    assert_eq!(
        cancels.get(),
        1,
        "the accepted contact receives its terminal callback"
    );
    realm.update_host_lifecycle(AppLifecycleState::Resumed);
    pump(&mut realm);
    assert_eq!(
        hovers.get(),
        0,
        "a failed diagnostic cannot preserve stale motion across pause"
    );
    dispatch(&realm, primary, motion);
    pump(&mut realm);
    assert_eq!(
        hovers.get(),
        1,
        "new input remains deliverable after containment"
    );
}

pub(crate) fn a_panicking_pause_diagnostic_cannot_skip_motion_drain() {
    pause_diagnostic_failure_still_drains_motion(false);
}

pub(crate) fn a_cancel_failure_precedes_a_pause_diagnostic_failure_and_recovers() {
    pause_diagnostic_failure_still_drains_motion(true);
}

#[derive(Clone, StatelessView)]
struct ShrinkingHoverRegion {
    width: Signal<f64>,
    enters: Rc<Cell<usize>>,
    exits: Rc<Cell<usize>>,
}

impl StatelessView for ShrinkingHoverRegion {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let width = self.width.get(ctx);
        let signal = self.width;
        let enters = self.enters.clone();
        let exits = self.exits.clone();
        Align::new(flui_painting::Alignment::TOP_LEFT).child(
            MouseRegion::new()
                .on_enter(move |_, _, _| enters.set(enters.get() + 1))
                .on_exit(move |_, _, _| exits.set(exits.get() + 1))
                .on_hover(move |cx, _, _| {
                    let _ = signal.set(cx, 5.0);
                })
                .child(SizedBox::new(width, 20.0)),
        )
    }
}

pub(crate) fn a_secondary_layout_refreshes_its_stationary_hover() {
    let mut realm = UiRealm::for_test();
    let secondary = install_secondary(&mut realm);
    let graph = realm
        .presentation_widgets_for_test(secondary)
        .with_build_owner(|owner| owner.reactive().clone());
    let enters = Rc::new(Cell::new(0));
    let exits = Rc::new(Cell::new(0));
    realm
        .attach_root_widget_to_for_test(
            secondary,
            &ShrinkingHoverRegion {
                width: graph.signal(20.0),
                enters: enters.clone(),
                exits: exits.clone(),
            },
        )
        .expect("secondary root attaches");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(&realm, secondary, hover());
    // Establish the mouse position independently of frame motion flushing:
    // this row isolates the committed-layout refresh contract.
    realm.enter(|realm| {
        realm
            .presentation_gestures_for_test(secondary)
            .flush_pending_moves();
    });
    assert_eq!(
        (enters.get(), exits.get()),
        (1, 0),
        "the cursor enters before layout shrinks"
    );
    pump(&mut realm);
    assert_eq!(
        (enters.get(), exits.get()),
        (1, 1),
        "the committed smaller region releases its stationary cursor"
    );
    pump(&mut realm);
    assert_eq!(
        (enters.get(), exits.get()),
        (1, 1),
        "ambient refresh does not duplicate exit"
    );
}
