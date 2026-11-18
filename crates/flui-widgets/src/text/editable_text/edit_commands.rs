//! The edits a field performs on a command that is not a key: select all,
//! and the [`EditCommand`]s a platform's own edit menu sends through the
//! field's store (iOS's `UITextInteraction`), performed by the same actions
//! the field's chords and menu invoke, under the same rules.

use std::cell::RefCell;
use std::rc::Rc;

use flui_interaction::focus::FocusNode;
use flui_platform::text_store::EditCommand;
use flui_view::EventContext;

use super::clipboard::ClipboardTextAction;
use crate::interaction::actions::{
    Action, ActionOutcome, CopySelectionTextIntent, PasteTextIntent, SelectAllTextIntent,
};
use crate::text::controller::TextEditingController;

/// Selection does not need a clipboard and never reports a text edit.
#[derive(Clone)]
pub(super) struct SelectAllTextAction {
    pub(super) controller: Rc<RefCell<TextEditingController>>,
    pub(super) focus_node: Rc<RefCell<Rc<FocusNode>>>,
}

impl SelectAllTextAction {
    fn select_all(&self) -> ActionOutcome {
        let controller = self.controller.borrow().clone();
        controller.set_selection(0, controller.text().len());
        ActionOutcome::Performed
    }
}

impl Action<SelectAllTextIntent> for SelectAllTextAction {
    fn is_enabled(&self, _intent: &SelectAllTextIntent) -> bool {
        self.focus_node.borrow().can_request_focus() && !self.controller.borrow().is_composing()
    }

    fn invoke(&self, _ctx: &mut EventContext<'_>, _intent: &SelectAllTextIntent) -> ActionOutcome {
        self.select_all()
    }
}

/// What a field's store performs of a platform's [`EditCommand`]: the copy,
/// cut, paste and select-all actions the field records for its chords, each
/// asked whether it is enabled first, as a `Shortcuts` asks.
#[derive(Clone)]
pub(in crate::text) struct FieldCommands {
    pub(super) clipboard: ClipboardTextAction,
    pub(super) select_all: SelectAllTextAction,
}

impl FieldCommands {
    /// Whether the field would perform `command` now: a protected field
    /// copies and cuts nothing, a cut and a paste need the field to take an
    /// edit, and every clipboard command needs a clipboard.
    pub(in crate::text) fn is_enabled(&self, command: EditCommand) -> bool {
        match command {
            EditCommand::Cut => self.clipboard.is_enabled(&CopySelectionTextIntent::Cut),
            EditCommand::Copy => self.clipboard.is_enabled(&CopySelectionTextIntent::Copy),
            EditCommand::Paste => self.clipboard.is_enabled(&PasteTextIntent),
            EditCommand::SelectAll => self.select_all.is_enabled(&SelectAllTextIntent),
            // A command added later that no field action performs yet.
            _ => false,
        }
    }

    /// Perform `command` if it is enabled; whether the field took it.
    pub(in crate::text) fn perform(&self, command: EditCommand) -> bool {
        if !self.is_enabled(command) {
            return false;
        }
        let outcome = match command {
            EditCommand::Cut => self.clipboard.copy(CopySelectionTextIntent::Cut),
            EditCommand::Copy => self.clipboard.copy(CopySelectionTextIntent::Copy),
            EditCommand::Paste => self.clipboard.paste(),
            EditCommand::SelectAll => self.select_all.select_all(),
            _ => ActionOutcome::NotPerformed,
        };
        outcome == ActionOutcome::Performed
    }
}
