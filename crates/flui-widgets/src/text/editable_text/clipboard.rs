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
use flui_platform::{ClipboardRead, ClipboardWritten};
use flui_view::EventContext;

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
    /// Whether the field takes an edit from the clipboard now: it is enabled,
    /// mounted (its focus attachment is attached) and holds the primary
    /// focus. Every path to a field's cut and paste gives it the focus first
    /// (the chord reaches the focused node; the selection menu, assistive
    /// technology's included, focuses the field it opens on), so an edit
    /// that arrives at a field without it is refused, at its start and at
    /// its answer alike.
    fn admits_edit(&self) -> bool {
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

    /// Whether an answer that arrives after its request still applies: the
    /// field admits an edit and drives `controller`, the controller it drove
    /// when the request began. Paste and cut answer under this one rule.
    fn still_edits(&self, controller: &TextEditingController) -> bool {
        self.admits_edit() && self.controller().is_same_controller(controller)
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

    fn invoke(
        &self,
        _ctx: &mut EventContext<'_>,
        intent: &CopySelectionTextIntent,
    ) -> ActionOutcome {
        let Some(clipboard) = &self.clipboard else {
            return ActionOutcome::NotPerformed;
        };
        // A cut starts under the rule its answer re-checks, as a paste does:
        // a field that does not take an edit now is not written from.
        if *intent == CopySelectionTextIntent::Cut && !self.admits_edit() {
            return ActionOutcome::NotPerformed;
        }
        let controller = self.controller();
        let selected = controller.selected_text();
        if selected.is_empty() {
            return ActionOutcome::NotPerformed;
        }
        let written = clipboard.write_text(selected);
        match intent {
            // A copy has nothing to undo when the write fails.
            CopySelectionTextIntent::Copy => drop(written),
            // A cut deletes only what the clipboard took (ADR-0337).
            CopySelectionTextIntent::Cut => written.on_complete(self.cut_answer(controller)),
        }
        ActionOutcome::Performed
    }
}

impl ClipboardTextAction {
    /// What a cut does once the clipboard answers its write: deletes the
    /// selection it wrote, if the clipboard took it, the field still edits
    /// (mounted, focused, enabled, on the same controller) and its text and
    /// selection are the ones the cut wrote. A refused or abandoned write
    /// deletes nothing, so a cut the clipboard could not take loses no text.
    fn cut_answer(
        &self,
        controller: TextEditingController,
    ) -> impl FnMut(Result<ClipboardWritten, Abandoned>) + 'static {
        let authority = self.clone();
        let edits = self.edits.clone();
        let text = controller.text();
        let selection = controller.selection();
        move |written| {
            if written != Ok(Ok(())) {
                return;
            }
            let unchanged = authority.still_edits(&controller)
                && controller.selection() == selection
                && controller.text() == text;
            if unchanged {
                // Replacing the selection with nothing deletes it and leaves
                // the caret at its start.
                edits.around(|| controller.insert_str(""));
            }
        }
    }
}

impl Action<PasteTextIntent> for ClipboardTextAction {
    fn is_enabled(&self, _intent: &PasteTextIntent) -> bool {
        self.clipboard.is_some() && self.enabled()
    }

    /// Commits an active composition first, keeping its text, so the paste
    /// lands after it (ADR-0142 item 4); the paste runs though the commit's
    /// owner code failed, and that failure is resumed after it.
    fn invoke(&self, _ctx: &mut EventContext<'_>, _intent: &PasteTextIntent) -> ActionOutcome {
        let Some(clipboard) = &self.clipboard else {
            return ActionOutcome::NotPerformed;
        };
        let mut calls = OwnerCalls::new();
        self.commit.run(&mut calls);
        if !self.admits_edit() {
            calls.resume();
            return ActionOutcome::NotPerformed;
        }
        // Settles at a later owner turn, never inside `read_text`; the backend
        // the provider supplied is user code, so it runs under `calls`. The
        // answer takes the controller before the backend runs: a backend that
        // swaps the controller invalidates the paste rather than redirecting
        // it.
        let answer = self.paste_answer(None);
        calls.run(move || clipboard.read_text().on_complete(answer));
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
        if !self.admits_edit() {
            return;
        }
        // The answer takes the controller before the backend runs, as the
        // paste chord's does.
        let answer = self.paste_answer(Some(at));
        calls.run(move || {
            if let Some(read) = self.primary.read_text() {
                read.on_complete(answer);
            }
        });
    }

    /// What a paste does once its text arrives: inserts it at source byte
    /// `at`, or at the caret, if the field still admits a paste and drives
    /// the controller it drove when the paste began.
    fn paste_answer(
        &self,
        at: Option<usize>,
    ) -> impl FnMut(Result<ClipboardRead, Abandoned>) + 'static {
        let authority = self.clone();
        let controller = self.controller();
        let edits = self.edits.clone();
        move |text| {
            if !authority.still_edits(&controller) {
                return;
            }
            // No text, a failed read and an abandoned one paste nothing.
            let Ok(Ok(Some(text))) = text else {
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
            // The caret move and the insertion are one edit, so a listener
            // that panics at its notification cannot lose the paste.
            edits.around(|| match at {
                Some(at) => controller.insert_str_at(at, &line),
                None => controller.insert_str(&line),
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
