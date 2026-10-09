//! Stable routing for mounted listeners whose recognizer owner can change.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use flui_interaction::{
    CancelOutcome, GestureArenaMember, GestureRecognizer, PointerDispatch, PointerId,
};

pub(crate) struct RecognizerAttachment<R> {
    target: RefCell<Weak<R>>,
}

impl<R> Default for RecognizerAttachment<R> {
    fn default() -> Self {
        Self {
            target: RefCell::new(Weak::new()),
        }
    }
}

impl<R> RecognizerAttachment<R> {
    pub(crate) fn attach(&self, owner: &Rc<R>) {
        *self.target.borrow_mut() = Rc::downgrade(owner);
    }

    pub(crate) fn clear(&self) {
        *self.target.borrow_mut() = Weak::new();
    }

    pub(crate) fn owner(&self) -> Option<Rc<R>> {
        self.target.borrow().upgrade()
    }
}

impl<R: GestureRecognizer> GestureArenaMember for RecognizerAttachment<R> {
    // Only the current owner joins the arena for a contact.
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, _: PointerId) {}
}

impl<R: GestureRecognizer> GestureRecognizer for RecognizerAttachment<R> {
    fn add_pointer(&self, dispatch: PointerDispatch<'_>) {
        if let Some(owner) = self.owner() {
            owner.add_pointer(dispatch);
        }
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        if let Some(owner) = self.owner() {
            owner.handle_event(dispatch);
        }
    }

    fn cancel(&self) -> CancelOutcome {
        self.owner()
            .map_or(CancelOutcome::Idle, |owner| owner.cancel())
    }
}
