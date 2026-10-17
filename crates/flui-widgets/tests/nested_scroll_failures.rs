//! Callout failure custody through the mounted binding and gesture producer.

use super::*;
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};

struct Failing {
    armed: Rc<Cell<bool>>,
    post: bool,
    fail_end: bool,
    name: &'static str,
    log: Rc<RefCell<Vec<&'static str>>>,
}

impl NestedScrollConnection for Failing {
    fn pre_scroll(&self, _: &ScrollStep) -> Vec2 {
        assert!(!self.armed.get() || self.post, "{}", self.name);
        Vec2::ZERO
    }
    fn post_scroll(&self, _: &ScrollStep, _: Vec2) -> Vec2 {
        assert!(!self.armed.get() || !self.post, "{}", self.name);
        Vec2::ZERO
    }
    fn sequence_ended(&self, _: InputSequenceId, _: ScrollEndReason) {
        self.log.borrow_mut().push(self.name);
        assert!(!self.fail_end, "competing end failure");
    }
}

pub(super) fn participant_failure_closes_exact_links_and_a_new_sequence_recovers() {
    for post in [false, true] {
        for competing in [false, true] {
            let armed = Rc::new(Cell::new(false));
            let log = Rc::new(RefCell::new(Vec::new()));
            let connection = |name, fail_end| {
                Rc::new(Failing {
                    armed: armed.clone(),
                    post,
                    fail_end,
                    name,
                    log: log.clone(),
                })
            };
            let controller = ScrollController::new();
            let laid = lay_out(
                NestedScroll::new(connection("outer failure", competing)).child(
                    NestedScroll::new(connection("inner failure", competing)).child(
                        Scrollable::new()
                            .controller(controller.clone())
                            .child(SizedBox::new(100.0, 600.0)),
                    ),
                ),
                tight(100.0, 100.0),
            );
            laid.dispatch_pointer_down(50.0, 80.0);
            armed.set(true);
            let failure = catch_unwind(AssertUnwindSafe(|| laid.dispatch_pointer_move(50.0, 40.0)))
                .expect_err("the original participant failure reaches the public source boundary");
            let expected = if post {
                "inner failure"
            } else {
                "outer failure"
            };
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some(expected)
            );
            assert_eq!(
                controller.pixels(),
                if post { 40.0 } else { 0.0 },
                "only motion accepted before the failure survives"
            );
            let expected_ends = if post {
                ["inner failure", "outer failure"]
            } else {
                ["outer failure", "inner failure"]
            };
            assert_eq!(
                *log.borrow(),
                expected_ends,
                "all withdrawn links complete despite competing end failures"
            );
            laid.dispatch_pointer_cancel();
            let before = controller.pixels();
            laid.dispatch_pointer_down(50.0, 80.0);
            laid.dispatch_pointer_move(50.0, 40.0);
            assert_eq!(
                controller.pixels(),
                before + 40.0,
                "a new source skips closed links and makes progress"
            );
            laid.dispatch_pointer_cancel();
            assert_eq!(log.borrow().len(), 2, "no withdrawn callback is replayed");
        }
    }
}
