//! What a field asks of the window's text input through its
//! `TextInputHandle` while it holds the input: to end its composition before
//! it loses the input, and to show the soft keyboard on its own tap.

use std::cell::RefCell;
use std::rc::Rc;

use flui_interaction::text_input::{ClientToken, TextInputHandle};
use flui_platform::text::store::OwnerCalls;

/// Commits a field's composition, keeping its text, before the field loses
/// its input: a blur, a pointer-down on it and a paste (ADR-0142 item 4).
/// The presentation asks the platform to end its composition (a pull host)
/// and commits in place when it cannot, or does that directly (a push
/// platform); inside a frame the request is queued with the field's store
/// and reaches it at the anchor, even after a detach that follows.
#[derive(Clone)]
pub(super) struct CompositionCommit {
    pub(super) handle: Option<TextInputHandle>,
    /// The client this field attached, while it holds the input.
    pub(super) token: Rc<RefCell<Option<ClientToken>>>,
}

impl CompositionCommit {
    /// Commit the attached client's composition inside `calls`, which keeps
    /// a failure of the owner code the commit reaches (its `on_changed`, a
    /// host) for the caller to resume once its own operation is complete.
    /// A field with no client attached has no composition to commit.
    pub(super) fn run(&self, calls: &mut OwnerCalls) {
        let Some(handle) = &self.handle else {
            return;
        };
        // Read in a statement of its own: the commit runs owner code, which
        // may detach this field and so take the token.
        let token = *self.token.borrow();
        if let Some(token) = token
            && let Some(Err(error)) = calls.run(|| handle.complete_composition(token))
        {
            calls.run(|| {
                tracing::trace!(
                    ?error,
                    "a composition commit reached a presentation that was already closing"
                );
            });
        }
    }
}

/// What a tap the field's gesture arena gave it asks of the platform while
/// the field holds the input: its soft keyboard, so one the user put away
/// returns, as on the platform's own text field (`TextStoreHost::show_keyboard`).
#[derive(Clone)]
pub(super) struct KeyboardRequest {
    pub(super) handle: Option<TextInputHandle>,
    /// The client this field attached, while it holds the input.
    pub(super) token: Rc<RefCell<Option<ClientToken>>>,
}

impl KeyboardRequest {
    /// Ask for the keyboard; a field holding no input asks nothing. The host
    /// call reaches platform code, so a failure it raises is resumed here,
    /// after the request.
    pub(super) fn run(&self) {
        let Some(handle) = &self.handle else {
            return;
        };
        let token = *self.token.borrow();
        let Some(token) = token else {
            return;
        };
        let mut calls = OwnerCalls::new();
        if let Some(Err(error)) = calls.run(|| handle.show_keyboard(token)) {
            calls.run(|| {
                tracing::trace!(
                    ?error,
                    "a keyboard request reached a presentation that was already closing"
                );
            });
        }
        calls.resume();
    }
}
