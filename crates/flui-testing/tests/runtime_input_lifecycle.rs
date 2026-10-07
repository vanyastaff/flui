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
use flui_widgets::{Listener, SizedBox};

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
