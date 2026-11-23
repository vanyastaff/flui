//! The semantics of a field: the properties its node publishes and the actions
//! assistive technology edits it through.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_interaction::focus::{FocusAttachment, FocusCause, FocusNode};
use flui_platform::text::store::OwnerCalls;
use flui_semantics::TextSelection;
use flui_view::prelude::*;

use super::CompositionCommit;
use super::edit_observer::EditObserver;
use super::obscuring::source_offset_for_masked_offset;
use crate::text::controller::TextEditingController;
use crate::text::text_store::EditableTextStore;

/// The field's name, placeholder, validity and whether it is required, as its semantics node
/// publishes them. A decorated field fills these from its visible label, hint
/// and error, which then publish no nodes of their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::text) struct FieldSemantics {
    pub(super) label: Option<String>,
    pub(super) hint: Option<String>,
    pub(super) placeholder: Option<String>,
    pub(super) invalid: bool,
    pub(super) required: bool,
    pub(super) identifier: Option<std::sync::Arc<str>>,
}

/// Accessibility edits use the same current document and notification path as
/// keyboard input. The semantics wrapper's owner-local ticket fences delivery;
/// this check also refuses a field whose attachment has detached or been
/// superseded, even when its external node remains in the live focus tree.
#[derive(Clone)]
pub(super) struct FieldSemanticsActions {
    pub(super) focus_node: Rc<RefCell<Rc<FocusNode>>>,
    pub(super) attachment: Rc<RefCell<Option<Rc<FocusAttachment>>>>,
    pub(super) enabled: Rc<Cell<bool>>,
    pub(super) edits: EditObserver,
    pub(super) store: Option<Rc<EditableTextStore>>,
    /// A selection set or replaced from assistive technology commits the
    /// composition first.
    pub(super) commit: CompositionCommit,
}

impl FieldSemanticsActions {
    pub(super) fn live_node(&self) -> Option<Rc<FocusNode>> {
        let attachment = self.attachment.borrow().clone()?;
        let node = Rc::clone(&self.focus_node.borrow());
        (self.enabled.get() && attachment.is_attached()).then_some(node)
    }

    pub(super) fn focus(&self, ctx: &EventContext<'_>) {
        if self.edits.writer.check_context(ctx).is_err() {
            return;
        }
        if let Some(node) = self.live_node().filter(|node| node.can_request_focus()) {
            // Assistive focus is shown, as `Focus` shows it on any control.
            let _ = node.request_focus_with(FocusCause::Assistive);
        }
    }

    pub(super) fn set_text(&self, ctx: &EventContext<'_>, text: &str) {
        self.edit(ctx, false, |controller| controller.set_text(text));
    }

    /// The selection a platform set, in bytes of the published value: the
    /// mask's on an obscured field, mapped back to the source here. The
    /// controller snaps both ends to its grapheme boundaries.
    pub(super) fn set_selection(
        &self,
        ctx: &EventContext<'_>,
        selection: TextSelection,
        obscuring: Option<char>,
    ) {
        self.edit(ctx, true, |controller| {
            let source = |offset: usize| match obscuring {
                Some(mask) => source_offset_for_masked_offset(&controller.text(), offset, mask),
                None => offset,
            };
            controller.set_selection(source(selection.anchor), source(selection.focus));
        });
    }

    /// Replaces the selection with `text`, as a paste does: line breaks
    /// dropped, the caret after the inserted text.
    pub(super) fn replace_selected_text(&self, ctx: &EventContext<'_>, text: &str) {
        let line: String = text
            .chars()
            .filter(|&character| character != '\n' && character != '\r')
            .collect();
        self.edit(ctx, true, |controller| controller.insert_str(&line));
    }

    /// Runs `edit` on the current controller through the keyboard's edit
    /// path: the composition committed first when `commit` asks for it,
    /// accepted platform grants before the edit, `on_changed` around it and
    /// the text store told after it. Each step runs though an earlier one
    /// failed: the platform hears of the edit before the first failure is
    /// resumed.
    pub(super) fn edit(
        &self,
        ctx: &EventContext<'_>,
        commit: bool,
        f: impl FnOnce(&TextEditingController),
    ) {
        if self.edits.writer.check_context(ctx).is_err() || self.live_node().is_none() {
            return;
        }
        let mut calls = OwnerCalls::new();
        // The composition keeps its text and ends before the selection moves
        // from under it, as on a pointer-down or a paste (ADR-0142 item 4).
        if commit {
            self.commit.run(&mut calls);
        }
        // Accepted platform grants precede app edits, just as they precede a
        // key edit. A grant can re-enter and replace or detach the field.
        if let Some(store) = &self.store {
            calls.run(|| store.run_deferred_before_app_edit());
        }
        if self.edits.writer.check_context(ctx).is_ok() && self.live_node().is_some() {
            let controller = self.edits.controller.borrow().clone();
            calls.run(|| self.edits.around(|| f(&controller)));
            if let Some(store) = &self.store {
                calls.run(|| store.controller_changed());
            }
        }
        calls.resume();
    }
}
