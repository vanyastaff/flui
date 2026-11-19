//! Something modal that is up on a window (an AppKit sheet, a view
//! controller UIKit presents), and who answers its caller: the system's
//! handler when the user answers, or the dialog's hold when the caller lets
//! the dialog go or its window closes (ADR-0344), whichever comes first.
//!
//! AppKit runs a sheet's completion handler however the sheet ends, so a
//! sheet's hold shares the caller's resolver with the handler
//! ([`Pending`]). UIKit tells nobody when a presented controller is
//! dismissed by other code (a controller further down the chain dismissed,
//! its root replaced), so there the controller owns its handlers, and they
//! the resolver, while the hold keeps a `WeakPending`: a controller that
//! goes away unanswered drops the resolver with its handlers, and the
//! caller's completion ends instead of waiting for good.

use std::cell::Cell;
use std::rc::Rc;

use flui_platform::capability::Resolver;

/// The caller's resolver while something modal is up, shared by the
/// system's handlers and, on macOS, the dialog's hold. Owner-thread: AppKit
/// and UIKit run their handlers on the main thread.
pub(crate) struct Pending<T>(Rc<Cell<Option<Resolver<T>>>>);

impl<T> Clone for Pending<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T: 'static> Pending<T> {
    pub(crate) fn new(answer: Resolver<T>) -> Self {
        Self(Rc::new(Cell::new(Some(answer))))
    }

    /// Answer the caller with `value`, unless someone answered or
    /// dismissed first; `value` is made only when someone still waits.
    pub(crate) fn answer(&self, value: impl FnOnce() -> T) {
        if let Some(answer) = self.0.take() {
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(value());
        }
    }

    /// Take the resolver for a dismissal, so the handler the dismissal runs
    /// answers nobody: whether nobody had answered yet. A sheet's hold asks
    /// it; the hold of a presented controller asks its `WeakPending`.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn dismiss(&self) -> bool {
        self.0.take().is_some()
    }

    /// Take the resolver for an answer the user gave that settles later (a
    /// pick still being read), so no other handler (a cancel, a dismissal)
    /// answers in its place; `None` when someone answered first. A dialog
    /// dropped meanwhile drops that answer at the handle (ADR-0344). An iOS
    /// pick and a macOS pick whose bookmarks are still being made take it.
    pub(crate) fn take(&self) -> Option<Resolver<T>> {
        self.0.take()
    }

    /// Whether nobody has answered or dismissed yet: what a provider asks
    /// before it presents something for a dialog that may have ended while
    /// it prepared (a save's file still being written).
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn is_waiting(&self) -> bool {
        let answer = self.0.take();
        let waiting = answer.is_some();
        self.0.set(answer);
        waiting
    }

    /// A reference that does not keep the resolver: what the hold of a
    /// presented controller keeps.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn downgrade(&self) -> WeakPending<T> {
        WeakPending(Rc::downgrade(&self.0))
    }
}

/// A [`Pending`] its holder does not keep alive: the resolver lives as
/// long as the handlers that can answer it do.
#[cfg(any(target_os = "ios", test))]
pub(crate) struct WeakPending<T>(std::rc::Weak<Cell<Option<Resolver<T>>>>);

#[cfg(any(target_os = "ios", test))]
impl<T: 'static> WeakPending<T> {
    /// Take the resolver for a dismissal, as a sheet's `Pending::dismiss` does:
    /// whether a handler was still there and nobody had answered yet.
    pub(crate) fn dismiss(&self) -> bool {
        self.0.upgrade().is_some_and(|slot| slot.take().is_some())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_platform::capability::{Abandoned, Completion};

    use super::Pending;
    use crate::testing::owner::TestOwner;

    /// Where `completion`'s outcome lands.
    fn land(completion: Completion<u8>) -> Rc<RefCell<Option<Result<u8, Abandoned>>>> {
        let landed: Rc<RefCell<Option<Result<u8, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&landed);
        completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        landed
    }

    #[test]
    fn the_first_of_the_answer_and_the_dismissal_takes_the_caller() {
        let owner = TestOwner::new();
        let (answered, answer) = owner.completions().pending::<u8>();
        let answered = land(answered);
        let pending = Pending::new(answer);
        pending.answer(|| 7);
        assert!(!pending.dismiss(), "an answered sheet is not up");
        owner.turn();
        assert_eq!(*answered.borrow(), Some(Ok(7)));

        let (dismissed, answer) = owner.completions().pending::<u8>();
        let dismissed = land(dismissed);
        let pending = Pending::new(answer);
        let handler = pending.clone();
        assert!(pending.dismiss(), "the sheet was up");
        handler.answer(|| panic!("a dismissed sheet's answer is never made"));
        owner.turn();
        assert_eq!(
            *dismissed.borrow(),
            Some(Err(Abandoned::ProviderDropped)),
            "the dismissal answers nothing; the handle has ended the dialog"
        );
    }

    #[test]
    fn a_pick_that_settles_later_keeps_a_cancel_and_a_dismissal_from_answering() {
        let owner = TestOwner::new();
        let (picked, answer) = owner.completions().pending::<u8>();
        let picked = land(picked);
        let pending = Pending::new(answer);
        let hold = pending.downgrade();
        assert!(pending.is_waiting(), "asking leaves the caller waiting");
        assert!(pending.is_waiting());
        let later = pending.take().expect("nobody answered yet");
        assert!(!pending.is_waiting());
        pending.answer(|| panic!("a cancel after the pick is never made"));
        assert!(!hold.dismiss(), "the pick took the caller");
        assert!(pending.take().is_none());
        let _ = later.resolve(3);
        owner.turn();
        assert_eq!(*picked.borrow(), Some(Ok(3)));
    }

    #[test]
    fn a_controller_that_goes_away_unanswered_ends_its_callers_completion() {
        let owner = TestOwner::new();
        let (gone, answer) = owner.completions().pending::<u8>();
        let gone = land(gone);
        // The handlers a presented controller owns, and its dialog's hold.
        let handler = Pending::new(answer);
        let hold = handler.downgrade();
        let handlers = [handler.clone(), handler];
        owner.turn();
        assert_eq!(*gone.borrow(), None, "up, and nobody answered");
        // Other code dismissed the controller: UIKit releases it, and its
        // handlers with it, telling nobody.
        drop(handlers);
        owner.turn();
        assert_eq!(
            *gone.borrow(),
            Some(Err(Abandoned::ProviderDropped)),
            "the caller hears that the dialog is gone"
        );
        assert!(!hold.dismiss(), "nothing is left to dismiss");

        let (held, answer) = owner.completions().pending::<u8>();
        let held = land(held);
        let handler = Pending::new(answer);
        let hold = handler.downgrade();
        assert!(hold.dismiss(), "a controller still up is dismissed once");
        assert!(!hold.dismiss());
        handler.answer(|| panic!("a dismissed controller's answer is never made"));
        owner.turn();
        assert_eq!(*held.borrow(), Some(Err(Abandoned::ProviderDropped)));
    }
}
