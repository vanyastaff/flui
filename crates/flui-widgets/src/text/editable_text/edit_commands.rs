//! The edits a field performs on a command that is not a key: select all,
//! and the [`EditCommand`]s a platform's own edit menu sends through the
//! field's store (iOS's `UITextInteraction`), performed by the same actions
//! the field's chords and menu invoke, under the same rules.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use flui_interaction::focus::FocusNode;
use flui_painting::text_boundaries::{WordSegment, word_segments_from};
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
    /// copies, cuts and selects no word, a cut and a paste need the field to
    /// take an edit, every clipboard command needs a clipboard, and a word is
    /// selected from a collapsed caret only.
    pub(in crate::text) fn is_enabled(&self, command: EditCommand) -> bool {
        match command {
            EditCommand::Cut => self.clipboard.is_enabled(&CopySelectionTextIntent::Cut),
            EditCommand::Copy => self.clipboard.is_enabled(&CopySelectionTextIntent::Copy),
            EditCommand::Paste => self.clipboard.is_enabled(&PasteTextIntent),
            EditCommand::SelectWord => {
                self.select_all.is_enabled(&SelectAllTextIntent) && self.word_at_caret().is_some()
            }
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
            EditCommand::SelectWord => self.select_word(),
            EditCommand::SelectAll => self.select_all.select_all(),
            _ => ActionOutcome::NotPerformed,
        };
        outcome == ActionOutcome::Performed
    }

    /// The word at a collapsed caret: the word segment that holds the caret
    /// or ends at it, else the one that starts at it, as UIKit's `select:`
    /// takes it. None for a selection, for a caret with no word beside it,
    /// and in an obscured field, which shows no words.
    fn word_at_caret(&self) -> Option<Range<usize>> {
        if self.clipboard.obscure.get() {
            return None;
        }
        let controller = self.select_all.controller.borrow().clone();
        if controller.has_selection() {
            return None;
        }
        let caret = controller.caret_byte_offset();
        let text = controller.text();
        // The line's words up to the caret, in order: the one that holds the
        // caret comes before the one that starts at it, and is preferred.
        word_segments_from(&text, caret)
            .take_while(|segment| segment.start <= caret)
            .filter(|segment| !segment.whitespace && caret <= segment.end)
            .map(|segment: WordSegment| segment.start..segment.end)
            .next()
    }

    fn select_word(&self) -> ActionOutcome {
        let Some(word) = self.word_at_caret() else {
            return ActionOutcome::NotPerformed;
        };
        let controller = self.select_all.controller.borrow().clone();
        controller.set_selection(word.start, word.end);
        ActionOutcome::Performed
    }
}
