//! A sheet that is up on a window, and who answers its caller: the sheet's
//! completion handler when the user answers, or the dialog's hold when the
//! caller lets the dialog go or its window closes (ADR-0344), whichever
//! comes first.

use std::cell::Cell;
use std::rc::Rc;

use flui_platform::capability::Resolver;

/// The caller's resolver while a sheet is up, shared by the sheet's
/// completion handler and its hold. Owner-thread: AppKit runs the handler
/// on the main thread.
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

    /// Answer the caller with `value`, unless the sheet was dismissed
    /// first; `value` is made only when someone still waits.
    pub(crate) fn answer(&self, value: impl FnOnce() -> T) {
        if let Some(answer) = self.0.take() {
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(value());
        }
    }

    /// Take the resolver for a dismissal, so the completion handler the
    /// dismissal runs answers nobody: whether the sheet was still up.
    pub(crate) fn dismiss(&self) -> bool {
        self.0.take().is_some()
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
}
