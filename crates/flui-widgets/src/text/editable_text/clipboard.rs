//! Copy, cut and paste for one mounted field, and the primary selection it
//! claims and a middle press pastes.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_interaction::ClipboardHandle;
use flui_interaction::focus::{FocusAttachment, FocusNode};
use flui_platform::capability::Abandoned;
use flui_platform::text_store::OwnerCalls;
use flui_view::EventCx;

use super::{CompositionCommit, EditObserver};
use crate::interaction::actions::{
    Action, ActionOutcome, CopySelectionTextIntent, PasteTextIntent,
};
use crate::text::controller::TextEditingController;
use crate::text::primary_selection::PrimarySelectionOwner;

/// Copy, cut and paste for one mounted field — see [`EditableText`](super::EditableText)'s
/// `# Clipboard` section for when each is enabled.
///
/// Reads the controller, node and obscuring flag through the state's shared
/// cells at key time, so a swapped controller or node is the one acted on.
#[derive(Clone)]
pub(super) struct ClipboardTextAction {
    pub(super) attachment: Rc<RefCell<Option<Rc<FocusAttachment>>>>,
    pub(super) enabled: Rc<Cell<bool>>,
    pub(super) controller: Rc<RefCell<TextEditingController>>,
    /// The field's current node; its `can_request_focus` tracks `enabled`.
    pub(super) focus_node: Rc<RefCell<Rc<FocusNode>>>,
    pub(super) obscure: Rc<Cell<bool>>,
    pub(super) clipboard: Option<ClipboardHandle>,
    /// What a middle press pastes.
    pub(super) primary: Rc<PrimarySelectionOwner>,
    /// Cut and paste are user edits.
    pub(super) edits: EditObserver,
    /// A paste commits the composition first.
    pub(super) commit: CompositionCommit,
}

impl ClipboardTextAction {
    fn admits_paste(&self) -> bool {
        let attachment = self.attachment.borrow().clone();
        let node = self.focus_node.borrow().clone();
        self.enabled.get()
            && attachment.is_some_and(|a| a.is_attached())
            && node.has_primary_focus()
            && node.can_request_focus()
    }
    fn enabled(&self) -> bool {
        self.focus_node.borrow().can_request_focus()
    }

    /// The controller, cloned out of its cell so no borrow is held while the
    /// clipboard or the controller's listeners run.
    fn controller(&self) -> TextEditingController {
        self.controller.borrow().clone()
    }
}

impl Action<CopySelectionTextIntent> for ClipboardTextAction {
    fn is_enabled(&self, intent: &CopySelectionTextIntent) -> bool {
        self.clipboard.is_some()
            && !self.obscure.get()
            && (*intent == CopySelectionTextIntent::Copy || self.enabled())
            && self.controller.borrow().has_selection()
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, intent: &CopySelectionTextIntent) -> ActionOutcome {
        let Some(clipboard) = &self.clipboard else {
            return ActionOutcome::NotPerformed;
        };
        let controller = self.controller();
        let selected = controller.selected_text();
        if selected.is_empty() {
            return ActionOutcome::NotPerformed;
        }
        clipboard.write_text(selected);
        if *intent == CopySelectionTextIntent::Cut {
            // Replacing the selection with nothing deletes it and leaves the
            // caret at its start.
            self.edits.around(|| controller.insert_str(""));
        }
        ActionOutcome::Performed
    }
}

impl Action<PasteTextIntent> for ClipboardTextAction {
    fn is_enabled(&self, _intent: &PasteTextIntent) -> bool {
        self.clipboard.is_some() && self.enabled()
    }

    /// Commits an active composition first, keeping its text, so the paste
    /// lands after it (ADR-0142 item 4); the paste runs though the commit's
    /// owner code failed, and that failure is resumed after it.
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &PasteTextIntent) -> ActionOutcome {
        let Some(clipboard) = &self.clipboard else {
            return ActionOutcome::NotPerformed;
        };
        let mut calls = OwnerCalls::new();
        self.commit.run(&mut calls);
        if !self.admits_paste() {
            calls.resume();
            return ActionOutcome::NotPerformed;
        }
        // Settles at a later owner turn, never inside `read_text`; the backend
        // the provider supplied is user code, so it runs under `calls`.
        calls.run(|| clipboard.read_text().on_complete(self.paste_answer(None)));
        calls.resume();
        ActionOutcome::Performed
    }
}

impl ClipboardTextAction {
    /// A middle press at source byte `at`: once the field has taken the
    /// focus the press asked for, the primary selection's text is pasted
    /// there, as a paste chord would paste the clipboard's at the caret.
    pub(super) fn paste_primary(&self, calls: &mut OwnerCalls, at: usize) {
        self.commit.run(calls);
        if !self.admits_paste() {
            return;
        }
        calls.run(|| {
            if let Some(read) = self.primary.read_text() {
                read.on_complete(self.paste_answer(Some(at)));
            }
        });
    }

    /// What a paste does once its text arrives: inserts it at source byte
    /// `at`, or at the caret, if the field still admits a paste and drives
    /// the controller it drove when the paste began.
    fn paste_answer(
        &self,
        at: Option<usize>,
    ) -> impl FnMut(Result<Option<String>, Abandoned>) + 'static {
        let authority = self.clone();
        let controller = self.controller();
        let edits = self.edits.clone();
        move |text| {
            if !authority.admits_paste() || !authority.controller().is_same_controller(&controller)
            {
                return;
            }
            let Ok(Some(text)) = text else {
                return;
            };
            // A single-line field: line breaks are dropped, `\r` included,
            // so a Windows `\r\n` leaves nothing behind.
            let line: String = text
                .chars()
                .filter(|&character| character != '\n' && character != '\r')
                .collect();
            if line.is_empty() {
                return;
            }
            edits.around(|| {
                if let Some(at) = at {
                    controller.set_caret_byte_offset(at);
                }
                controller.insert_str(&line);
            });
        }
    }
}

/// Claim the primary selection for the field's selection, or let it go
/// when the selection is empty or the field obscured. The claim reads the
/// selection when asked, so an obscured field answers nothing even for a
/// claim made before it was obscured.
pub(super) fn claim_primary(
    primary: &PrimarySelectionOwner,
    controller: &Weak<RefCell<TextEditingController>>,
    obscure: &Rc<Cell<bool>>,
) {
    let selected = !obscure.get()
        && controller
            .upgrade()
            .is_some_and(|controller| controller.borrow().has_selection());
    primary.selection_changed(selected, || {
        let controller = Weak::clone(controller);
        let obscure = Rc::clone(obscure);
        move || {
            if obscure.get() {
                return None;
            }
            let controller = controller.upgrade()?.borrow().clone();
            Some(controller.selected_text()).filter(|text| !text.is_empty())
        }
    });
}
