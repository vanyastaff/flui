//! Owner-local callbacks capture `Rc` state: no binding, recognizer or router
//! signature asks for `Send` (ADR-0207 §8).

use flui_interaction::gesture::GestureContest;
use std::{cell::Cell, rc::Rc};

use flui_interaction::{
    GestureArenaMember, GestureBinding, PointerId, PointerRouteHandler, TapGestureRecognizer,
};

struct CountingMember(Rc<Cell<u32>>);

impl GestureArenaMember for CountingMember {
    fn accept_gesture(&self, _: GestureContest) {
        self.0.set(self.0.get() + 1);
    }

    fn reject_gesture(&self, _: GestureContest) {}
}

fn main() {
    let binding = Rc::new(GestureBinding::new());
    let count = Rc::new(Cell::new(0_u32));

    let tapped = Rc::clone(&count);
    let _tap = TapGestureRecognizer::builder(binding.arena().clone())
        .on_tap(move |_| tapped.set(tapped.get() + 1))
        .build();

    let routed = Rc::clone(&count);
    let reentrant = Rc::downgrade(&binding);
    let handler: PointerRouteHandler = Rc::new(move |_| {
        routed.set(routed.get() + 1);
        let _same_owner = reentrant.upgrade();
    });
    binding.pointer_router().add_global_handler(handler);

    let member = Rc::new(CountingMember(Rc::clone(&count)));
    let _entry = binding
        .arena()
        .add(PointerId::new(std::num::NonZeroU64::MIN), &member);
}
