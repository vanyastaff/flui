//! Addressed input advances at the production realm frame and lifecycle boundaries.

use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::geometry::Offset;
use flui_foundation::{ManualClock, PresentationId};
use flui_interaction::events::{
    PointerEvent, PointerType, make_down_event, make_move_event,
};
use flui_platform_api::{PlatformInput, WindowExecutionState};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_runtime::testing::ScriptedSink;
use flui_runtime::ui_realm::UiRealm;
use flui_scheduler::AppLifecycleState;
use flui_view::prelude::*;
use flui_widgets::{Align, Listener, MouseRegion, SizedBox};

fn pump(realm: &mut UiRealm) {
    let _ = realm.pump(&mut ManualClock::default(), &mut ScriptedSink::always_presents());
}

fn dispatch(realm: &UiRealm, id: PresentationId, event: PointerEvent) {
    realm.enter(|realm| {
        realm.handle_input_addressed(id, PlatformInput::Pointer(event));
    });
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
    let secondary = realm.install_second_presentation_for_test();
    let moves = Rc::new(Cell::new(0));
    let seen = moves.clone();
    realm.attach_root_widget_to_for_test(
        secondary,
        &Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_move(move |_, _| seen.set(seen.get() + 1))
            .child(SizedBox::new(40.0, 40.0)),
    ).expect("secondary root attaches");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(&realm, secondary, make_down_event(Offset::new(4.0, 6.0), PointerType::Touch));
    dispatch(&realm, secondary, make_move_event(Offset::new(8.0, 9.0), PointerType::Touch));
    assert_eq!(moves.get(), 0, "motion waits for frame cadence");
    pump(&mut realm);
    assert_eq!(moves.get(), 1, "the secondary contact receives its queued motion");
    pump(&mut realm);
    assert_eq!(moves.get(), 1, "a later frame cannot duplicate motion");
}

fn queued_hover_after_transition(paused: bool) {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let hovers = Rc::new(Cell::new(0));
    let seen = hovers.clone();
    realm.attach_root_widget(
        &Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_hover(move |_, _| seen.set(seen.get() + 1))
            .child(SizedBox::new(40.0, 40.0)),
    ).expect("root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(&realm, primary, hover());
    assert_eq!(hovers.get(), 0, "hover waits for frame cadence");
    if paused {
        realm.update_host_lifecycle(AppLifecycleState::Paused);
        realm.update_host_lifecycle(AppLifecycleState::Resumed);
    } else {
        realm.update_window_focus(primary, false);
    }
    pump(&mut realm);
    assert_eq!(hovers.get(), usize::from(!paused), "pause discards pending input; blur preserves hover");
    dispatch(&realm, primary, hover());
    pump(&mut realm);
    assert_eq!(hovers.get(), 1 + usize::from(!paused), "fresh hovering still works after the transition");
}

pub(crate) fn host_pause_discards_a_queued_hover_before_resume() {
    queued_hover_after_transition(true);
}

pub(crate) fn window_blur_keeps_a_queued_hover() {
    queued_hover_after_transition(false);
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
    let secondary = realm.install_second_presentation_for_test();
    let graph = realm.presentation_widgets_for_test(secondary)
        .with_build_owner(|owner| owner.reactive().clone());
    let enters = Rc::new(Cell::new(0));
    let exits = Rc::new(Cell::new(0));
    realm.attach_root_widget_to_for_test(secondary, &ShrinkingHoverRegion {
        width: graph.signal(20.0),
        enters: enters.clone(),
        exits: exits.clone(),
    }).expect("secondary root attaches");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(&realm, secondary, hover());
    // Establish the mouse position independently of frame motion flushing:
    // this row isolates the committed-layout refresh contract.
    realm.enter(|realm| {
        realm.presentation_gestures_for_test(secondary).flush_pending_moves();
    });
    assert_eq!((enters.get(), exits.get()), (1, 0), "the cursor enters before layout shrinks");
    pump(&mut realm);
    assert_eq!((enters.get(), exits.get()), (1, 1), "the committed smaller region releases its stationary cursor");
    pump(&mut realm);
    assert_eq!((enters.get(), exits.get()), (1, 1), "ambient refresh does not duplicate exit");
}
