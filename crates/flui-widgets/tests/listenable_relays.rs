//! Framework relays registered on a controller's `Listenable` borrow the
//! enclosing notification's failure custody, whichever controller forwards it.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::Listenable;
use flui_foundation::geometry::Matrix4;
use flui_rendering::view::ScrollPosition;
use flui_widgets::interaction::TransformationController;
use flui_widgets::text::TextEditingController;
use flui_widgets::widget_state::{WidgetState, WidgetStates, WidgetStatesController};

/// A listener that panics first, then a relay: the relay must observe the
/// failure its enclosing delivery already holds, so outgoing captures it
/// retires stay retained rather than running user destructors after a panic.
fn relay_sees_the_enclosing_failure(listenable: &dyn Listenable, notify: impl FnOnce()) {
    let first = listenable.add_listener(Rc::new(|| panic!("first listener")));
    let seen = Rc::new(Cell::new(None));
    let relay = {
        let seen = Rc::clone(&seen);
        listenable.add_observer(Rc::new(move |recovery| {
            seen.set(Some(recovery.has_failure()));
        }))
    };
    let unwound = catch_unwind(AssertUnwindSafe(notify)).is_err();
    listenable.remove_listener(first);
    listenable.remove_listener(relay);
    assert!(unwound, "the first listener's panic propagates");
    assert_eq!(
        seen.get(),
        Some(true),
        "the relay runs inside the enclosing delivery's recovery scope"
    );
}

pub fn text_editing_controller_relays_keep_failure_custody() {
    let controller = TextEditingController::new();
    relay_sees_the_enclosing_failure(&controller, || controller.set_text("changed"));
}

pub fn widget_states_controller_relays_keep_failure_custody() {
    let controller = WidgetStatesController::new(WidgetStates::empty());
    relay_sees_the_enclosing_failure(&controller, || {
        controller.update(WidgetState::Hovered, true);
    });
}

pub fn transformation_controller_relays_keep_failure_custody() {
    let controller = TransformationController::new();
    let listenable = controller.listenable();
    relay_sees_the_enclosing_failure(&*listenable, || {
        controller.set_value(Matrix4::translation(1.0, 0.0, 0.0));
    });
}

pub fn scroll_position_relays_keep_failure_custody() {
    let position = ScrollPosition::new(0.0);
    let listenable = position.listenable();
    relay_sees_the_enclosing_failure(&*listenable, || position.notify());
}
