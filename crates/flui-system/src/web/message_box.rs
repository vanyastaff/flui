//! The message box of a page: a modal `<dialog>` beside the window's canvas.
//!
//! A browser's own boxes (`alert`, `confirm`) block the page's one thread
//! while they are up, cannot be closed by the page, and offer no Yes, No and
//! Cancel; a `<dialog>` shown with `showModal` is modal to the page, in the
//! top layer above the canvas, read by assistive technology as a dialog, and
//! closed by the page when the dialog's caller lets it go.
//!
//! The box answers the button the user pressed. The Escape key closes it
//! with what closing means for its buttons; a box with no way out but an
//! answer (Yes or No) keeps Escape from closing it, and is shown again if
//! the browser closes it anyway. The buttons are labelled in English, since
//! a page has no system labels to borrow.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_platform::capability::{Resolver, WindowRef};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;
use web_sys::{Element, Event, HtmlDialogElement, HtmlElement};

use super::page::{Page, exception};
use super::rules;
use crate::dialog::{
    DialogError, DialogHold, MessageBoxBackend, MessageBoxRequest, MessageBoxResult,
    MessageButtons, MessageLevel,
};

/// The box's own look: the page's fonts and colour scheme, a readable
/// measure, and the message's line breaks kept.
const BOX_STYLE: &str = "color-scheme: light dark; font: menu; max-width: min(32rem, 90vw); \
    padding: 1rem 1.25rem; border-radius: 0.5rem;";
const MESSAGE_STYLE: &str = "white-space: pre-wrap; overflow-wrap: anywhere; margin: 0 0 1rem;";
const BUTTONS_STYLE: &str = "display: flex; gap: 0.5rem; justify-content: flex-end; margin: 0;";

/// A page's message boxes.
pub(crate) struct WebMessageBoxes {
    page: Rc<Page>,
}

impl WebMessageBoxes {
    pub(crate) fn new(page: Rc<Page>) -> Self {
        Self { page }
    }
}

impl MessageBoxBackend for WebMessageBoxes {
    fn show(
        &self,
        _: &WindowRef,
        request: MessageBoxRequest,
        answer: Resolver<MessageBoxResult>,
    ) -> DialogHold {
        match ShownBox::show(&self.page, &request, answer) {
            Ok(shown) => DialogHold::new(BoxHold(shown)),
            Err((answer, error)) => {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(Err(error));
                DialogHold::new(())
            }
        }
    }
}

/// A box on the page, until the user answers it or its caller lets it go.
struct ShownBox {
    dialog: HtmlDialogElement,
    buttons: MessageButtons,
    answer: Cell<Option<Resolver<MessageBoxResult>>>,
    /// The `close` and `cancel` listeners, kept while the box is up.
    listeners: RefCell<Option<[Closure<dyn FnMut(Event)>; 2]>>,
}

/// A box that could not be shown: the answer to give, and why.
type NotShown = (Resolver<MessageBoxResult>, DialogError);

impl ShownBox {
    fn show(
        page: &Page,
        request: &MessageBoxRequest,
        answer: Resolver<MessageBoxResult>,
    ) -> Result<Rc<Self>, NotShown> {
        let dialog = match build(page, request) {
            Ok(dialog) => dialog,
            Err(error) => return Err((answer, error)),
        };
        let Some(host) = page.host() else {
            return Err((
                answer,
                DialogError::Unsupported(flui_platform::Unsupported::of::<
                    crate::dialog::MessageBoxCapability,
                >(
                    flui_platform::UnsupportedReason::Unavailable
                )),
            ));
        };
        if let Err(thrown) = host.append_child(&dialog) {
            return Err((answer, DialogError::Failed(exception(&thrown).into())));
        }
        let shown = Rc::new(Self {
            dialog,
            buttons: request.buttons,
            answer: Cell::new(Some(answer)),
            listeners: RefCell::new(None),
        });
        let closing = Rc::downgrade(&shown);
        let close = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            if let Some(shown) = closing.upgrade() {
                shown.closed();
            }
        });
        let buttons = request.buttons;
        let cancel = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            if rules::box_holds_escape(buttons) {
                event.prevent_default();
            }
        });
        let _ = shown
            .dialog
            .add_event_listener_with_callback("close", close.as_ref().unchecked_ref());
        let _ = shown
            .dialog
            .add_event_listener_with_callback("cancel", cancel.as_ref().unchecked_ref());
        *shown.listeners.borrow_mut() = Some([close, cancel]);
        if let Err(thrown) = shown.dialog.show_modal() {
            let answer = shown.answer.take();
            shown.take_down();
            if let Some(answer) = answer {
                return Err((answer, DialogError::Failed(exception(&thrown).into())));
            }
        }
        Ok(shown)
    }

    /// The dialog's `close`: answer the button it closed with, or show it
    /// again when closing is no answer for its buttons.
    fn closed(&self) {
        let value = self.dialog.return_value();
        match rules::box_answer(self.buttons, &value) {
            Some(chosen) => {
                if let Some(answer) = self.answer.take() {
                    self.take_down();
                    // A retired owner has nobody left to answer.
                    let _ = answer.resolve(Ok(chosen));
                }
            }
            None => {
                self.dialog.set_return_value("");
                if self.dialog.show_modal().is_err()
                    && let Some(answer) = self.answer.take()
                {
                    self.take_down();
                    // The box could not be shown again, and no answer it
                    // admits was given: the system failed.
                    let _ = answer.resolve(Err(DialogError::Failed(
                        flui_platform::SystemFailure::MalformedAnswer,
                    )));
                }
            }
        }
    }

    /// Stop listening, close the dialog if it is open and take it out of
    /// the page. The listeners stay alive until the box drops, since one
    /// may be running.
    fn take_down(&self) {
        if let Some([close, cancel]) = self.listeners.borrow().as_ref() {
            let _ = self
                .dialog
                .remove_event_listener_with_callback("close", close.as_ref().unchecked_ref());
            let _ = self
                .dialog
                .remove_event_listener_with_callback("cancel", cancel.as_ref().unchecked_ref());
        }
        if self.dialog.open() {
            self.dialog.close();
        }
        self.dialog.remove();
    }
}

/// What the handle holds for a box: dropping it closes the box and drops
/// the answer.
struct BoxHold(Rc<ShownBox>);

impl Drop for BoxHold {
    fn drop(&mut self) {
        if self.0.answer.take().is_some() {
            self.0.take_down();
        }
    }
}

/// The `<dialog>` for `request`: its title, its message and a form of its
/// buttons, the first of which takes focus.
fn build(page: &Page, request: &MessageBoxRequest) -> Result<HtmlDialogElement, DialogError> {
    let document = page.document();
    let element = |tag: &str| {
        document
            .create_element(tag)
            .map_err(|thrown| DialogError::Failed(exception(&thrown).into()))
    };
    let dialog = element("dialog")?.unchecked_into::<HtmlDialogElement>();
    let role = match request.level {
        MessageLevel::Info => "dialog",
        MessageLevel::Warning | MessageLevel::Error => "alertdialog",
    };
    let _ = dialog.set_attribute("role", role);
    let _ = dialog.set_attribute("style", BOX_STYLE);
    let _ = dialog.set_attribute("data-level", level_name(request.level));
    if let Some(title) = &request.title {
        let heading = element("h2")?;
        heading.set_text_content(Some(title));
        let _ = heading.set_attribute("style", "font-size: 1.1em; margin: 0 0 0.5rem;");
        let _ = dialog.set_attribute("aria-label", title);
        let _ = dialog.append_child(&heading);
    }
    let message = element("p")?;
    message.set_text_content(Some(&request.message));
    let _ = message.set_attribute("style", MESSAGE_STYLE);
    let _ = dialog.append_child(&message);
    let form: Element = element("form")?;
    let _ = form.set_attribute("method", "dialog");
    let _ = form.set_attribute("style", BUTTONS_STYLE);
    for (at, button) in rules::box_buttons(request.buttons).iter().enumerate() {
        let element = element("button")?;
        let _ = element.set_attribute("value", button.value);
        element.set_text_content(Some(button.label));
        if at == 0 {
            let _ = element.unchecked_ref::<HtmlElement>().set_autofocus(true);
        }
        let _ = form.append_child(&element);
    }
    let _ = dialog.append_child(&form);
    Ok(dialog)
}

/// The level's name, on the box for a page's own styles to read.
fn level_name(level: MessageLevel) -> &'static str {
    match level {
        MessageLevel::Info => "info",
        MessageLevel::Warning => "warning",
        MessageLevel::Error => "error",
    }
}
