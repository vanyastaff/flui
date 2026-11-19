//! The edits a field performs on a command that is not a key: select all,
//! and the [`EditCommand`]s a platform's own edit menu sends through the
//! field's store (iOS's `UITextInteraction`), performed by the same actions
//! the field's chords and menu invoke, under the same rules.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use flui_interaction::focus::FocusNode;
use flui_painting::text_boundaries::{WordSegment, word_segments_from};
use flui_platform::text::store::EditCommand;
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

    /// The word at a collapsed caret, as UIKit's `select:` takes it: the word
    /// that holds the caret, else the one that ends at it, else the one that
    /// starts at it, else the nearest word before it on its line past
    /// punctuation and spaces, else the nearest after on its line. A word is
    /// a segment with a letter or a digit, so a caret just past "hello,"
    /// selects "hello", never the comma; a line break bounds the search on
    /// both sides. None for a selection, for a line with no word, and in an
    /// obscured field, which shows no words.
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
        let is_word = |segment: &WordSegment| {
            !segment.whitespace
                && text[segment.start..segment.end]
                    .chars()
                    .any(char::is_alphanumeric)
        };
        // Each line break is a segment of its own; the segments start at the
        // caret's line, past its last line feed, and a break before the
        // caret that is not a line feed (a carriage return, a paragraph
        // separator) starts the line again.
        let is_line_break = |segment: &WordSegment| {
            text[segment.start..segment.end].contains([
                '\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
            ])
        };
        // The line's words in order: the last that ends at or before the
        // caret, and the first that starts at or after it.
        let mut before: Option<WordSegment> = None;
        for segment in word_segments_from(&text, caret) {
            if is_line_break(&segment) {
                if segment.end > caret {
                    break;
                }
                before = None;
                continue;
            }
            if !is_word(&segment) {
                continue;
            }
            if segment.start < caret && caret < segment.end {
                return Some(segment.start..segment.end);
            }
            if segment.end <= caret {
                before = Some(segment);
                continue;
            }
            let touching_before = before.filter(|word| word.end == caret);
            let after_touches = segment.start == caret;
            let chosen = match (touching_before, after_touches) {
                (Some(word), _) => word,
                (None, true) => segment,
                (None, false) => before.unwrap_or(segment),
            };
            return Some(chosen.start..chosen.end);
        }
        before.map(|word| word.start..word.end)
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
