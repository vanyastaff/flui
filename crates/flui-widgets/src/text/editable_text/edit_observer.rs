//! Reports a user edit through [`EditableText::on_changed`].

#[cfg(doc)]
use super::{EditableText, EditableTextState};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_platform::text::store::OwnerCalls;
use flui_view::prelude::*;

use super::TextChanged;
use crate::text::controller::TextEditingController;

/// Reports a user edit through [`EditableText::on_changed`]: compares the
/// committed text (the text without the IME composition) before and after
/// the edit, and calls the callback with the new committed text, with no
/// borrow held, when they differ, inside a write `writer` opens.
#[derive(Clone)]
pub(in crate::text) struct EditObserver {
    pub(super) controller: Rc<RefCell<TextEditingController>>,
    pub(super) on_changed: Rc<RefCell<Option<TextChanged>>>,
    /// The field's [`EditableTextState::reports_edits`].
    pub(super) reports: Rc<Cell<bool>>,
    pub(super) writer: WriterSource,
}

impl EditObserver {
    pub(super) fn around<R>(&self, edit: impl FnOnce() -> R) -> R {
        // The callback this edit is owed to is the one installed when it is
        // accepted: the edit notifies the controller's listeners, which may
        // rebuild the field and remove or replace `on_changed` before the
        // owner hears of the change.
        let Some(on_changed) = self.accept() else {
            return edit();
        };
        // So is the controller it edits: a listener's rebuild may hand the
        // field another controller, whose text is not this edit's result.
        let controller = self.controller.borrow().clone();
        let before = controller.committed_text();
        // The listeners' retirement can fail after the text changed: the
        // owner still hears of the change, and the first failure is resumed
        // after it.
        let mut calls = OwnerCalls::new();
        let result = calls.run(edit);
        let after = controller.committed_text();
        if after == before {
            calls.retire(on_changed);
        } else {
            self.deliver(on_changed, &after, &mut calls);
        }
        calls.resume();
        result.expect("BUG: an edit that failed resumed its failure above")
    }

    /// The `on_changed` an edit accepted now is owed to: a snapshot of the
    /// installed callback, which [`Self::deliver`] calls though the field
    /// removed or replaced it meanwhile. `None` when none is installed or the
    /// field's dispose has begun: the edit then owes nothing.
    pub(in crate::text) fn accept(&self) -> Option<TextChanged> {
        if !self.reports.get() {
            return None;
        }
        self.on_changed.borrow().clone()
    }

    /// Retire this observer's handles to the controller and `on_changed`
    /// inside `calls`, for an owner (a store outliving its field) going
    /// away: a last owner's captures are other code, destroyed under
    /// containment after a failure too (ADR-0281).
    pub(in crate::text) fn retire(&mut self, calls: &mut OwnerCalls) {
        let controller = std::mem::replace(
            &mut self.controller,
            Rc::new(RefCell::new(TextEditingController::new())),
        );
        let on_changed = std::mem::replace(&mut self.on_changed, Rc::new(RefCell::new(None)));
        calls.retire(on_changed);
        calls.retire(controller);
    }

    /// Call `on_changed`, the snapshot [`Self::accept`] took when the edit
    /// was accepted, with `committed`, inside `calls`. A field that replaced
    /// or removed the callback since (a rebuild) left the snapshot its last
    /// owner, which retires inside `calls`.
    pub(in crate::text) fn deliver(
        &self,
        on_changed: TextChanged,
        committed: &str,
        calls: &mut OwnerCalls,
    ) {
        calls.run(|| self.writer.write(|ctx| on_changed(ctx, committed)));
        calls.retire(on_changed);
    }
}
