use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::Vec2;
use flui_interaction::InputSequenceId;
use flui_widgets::{
    FlingHandoff, FlingStep, NestedScroll, NestedScrollConnection, ScrollEndReason, ScrollStep,
    Scrollable, SizedBox,
};

/// Counts what reaches it in `Rc` and `Cell` state, which a `Send` bound on
/// the connection would refuse.
struct Counter {
    consumed: Rc<Cell<f64>>,
    ended: Rc<RefCell<Vec<ScrollEndReason>>>,
}

impl NestedScrollConnection for Counter {
    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
        self.consumed.set(self.consumed.get() + step.available.y);
        Vec2::ZERO
    }

    fn post_fling(&self, _step: &FlingStep) -> FlingHandoff {
        FlingHandoff::Declined
    }

    fn sequence_ended(&self, _sequence: InputSequenceId, reason: ScrollEndReason) {
        self.ended.borrow_mut().push(reason);
    }
}

fn assert_send_sync<T: Send + Sync>() {}

fn main() {
    let counter = Rc::new(Counter {
        consumed: Rc::new(Cell::new(0.0)),
        ended: Rc::new(RefCell::new(Vec::new())),
    });
    let _view = NestedScroll::new(
        counter,
        Scrollable::new().child(SizedBox::new(100.0, 1000.0)),
    );
    assert_send_sync::<ScrollStep>();
    assert_send_sync::<FlingStep>();
}
