//! The message box of a page: a modal `<dialog>` beside the window's canvas.
//!
//! A browser's own boxes (`alert`, `confirm`) block the page's one thread
//! while they are up, cannot be closed by the page, and offer no Yes, No and
//! Cancel; a `<dialog>` shown with `showModal` is in the top layer above the
//! canvas, read by assistive technology as a dialog named by its title (its
//! message when it has none) and described by its message, and closed by the
//! page when the dialog's caller lets it go. `showModal` makes the rest of
//! the document inert, so the box is modal to the page, every canvas window
//! of it, where the contract says modal to its window, and two windows'
//! boxes stack.
//!
//! The box answers the button the user pressed. The Escape key closes it
//! with what closing means for its buttons; a box with no way out but an
//! answer (Yes or No) keeps Escape from closing it, and is shown again if
//! the browser closes it anyway. The buttons are labelled in English, since
//! a page has no system labels to borrow. Its look is set through each
//! element's style declarations, which a content security policy without
//! `'unsafe-inline'` styles leaves in force, as it would not a `style`
//! attribute.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{Unsupported, UnsupportedReason};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;
use web_sys::{Element, Event, HtmlDialogElement, HtmlElement};

use super::page::{Page, exception};
use super::rules;
use crate::dialog::{
    DialogError, DialogHold, MessageBoxBackend, MessageBoxCapability, MessageBoxRequest,
    MessageBoxResult, MessageButtons, MessageLevel,
};

/// The box's own look: the page's fonts and colour scheme and a readable
/// measure.
const BOX_STYLE: [(&str, &str); 5] = [
    ("color-scheme", "light dark"),
    ("font", "menu"),
    ("max-width", "min(32rem, 90vw)"),
    ("padding", "1rem 1.25rem"),
    ("border-radius", "0.5rem"),
];
const TITLE_STYLE: [(&str, &str); 2] = [("font-size", "1.1em"), ("margin", "0 0 0.5rem")];
/// The message keeps its line breaks, which the contract keeps.
const MESSAGE_STYLE: [(&str, &str); 3] = [
    ("white-space", "pre-wrap"),
    ("overflow-wrap", "anywhere"),
    ("margin", "0 0 1rem"),
];
const BUTTONS_STYLE: [(&str, &str); 4] = [
    ("display", "flex"),
    ("gap", "0.5rem"),
    ("justify-content", "flex-end"),
    ("margin", "0"),
];

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
        window: &WindowRef,
        request: MessageBoxRequest,
        answer: Resolver<MessageBoxResult>,
    ) -> DialogHold {
        match ShownBox::show(&self.page, window, &request, answer) {
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
    /// Set once the box is shown; taken by whichever end comes first.
    answer: Cell<Option<Resolver<MessageBoxResult>>>,
    /// Alive until the box itself drops, since one may be running.
    listeners: BoxListeners,
}

/// The listeners a box keeps while it is up: its `close`, and its `cancel`,
/// which keeps Escape from a box with no way out but an answer.
struct BoxListeners {
    close: Closure<dyn FnMut(Event)>,
    cancel: Closure<dyn FnMut(Event)>,
}

/// A box that could not be shown: the answer to give, and why.
type NotShown = (Resolver<MessageBoxResult>, DialogError);

/// The browser's failure `thrown` as a dialog's.
fn failed(thrown: &wasm_bindgen::JsValue) -> DialogError {
    DialogError::Failed(exception(thrown).into())
}

impl ShownBox {
    fn show(
        page: &Page,
        window: &WindowRef,
        request: &MessageBoxRequest,
        answer: Resolver<MessageBoxResult>,
    ) -> Result<Rc<Self>, NotShown> {
        let dialog = match build(page, window, request) {
            Ok(dialog) => dialog,
            Err(error) => return Err((answer, error)),
        };
        let Some(host) = page.host() else {
            return Err((
                answer,
                DialogError::Unsupported(Unsupported::of::<MessageBoxCapability>(
                    UnsupportedReason::Unavailable,
                )),
            ));
        };
        if let Err(thrown) = host.append_child(&dialog) {
            return Err((answer, failed(&thrown)));
        }
        let buttons = request.buttons;
        let shown = Rc::new_cyclic(|this: &Weak<Self>| {
            let closing = Weak::clone(this);
            Self {
                dialog,
                buttons,
                answer: Cell::new(None),
                listeners: BoxListeners {
                    close: Closure::new(move |_: Event| {
                        if let Some(shown) = closing.upgrade() {
                            shown.closed();
                        }
                    }),
                    cancel: Closure::new(move |event: Event| {
                        if rules::box_holds_escape(buttons) {
                            event.prevent_default();
                        }
                    }),
                },
            }
        });
        let _ = shown.dialog.add_event_listener_with_callback(
            "close",
            shown.listeners.close.as_ref().unchecked_ref(),
        );
        let _ = shown.dialog.add_event_listener_with_callback(
            "cancel",
            shown.listeners.cancel.as_ref().unchecked_ref(),
        );
        if let Err(thrown) = shown.dialog.show_modal() {
            shown.take_down();
            return Err((answer, failed(&thrown)));
        }
        shown.answer.set(Some(answer));
        Ok(shown)
    }

    /// The dialog's `close`: answer the button it closed with, or show it
    /// again when closing is no answer for its buttons.
    fn closed(&self) {
        let value = self.dialog.return_value();
        if let Some(chosen) = rules::box_answer(self.buttons, &value) {
            if let Some(answer) = self.answer.take() {
                self.take_down();
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(Ok(chosen));
            }
            return;
        }
        self.dialog.set_return_value("");
        if let Err(thrown) = self.dialog.show_modal()
            && let Some(answer) = self.answer.take()
        {
            self.take_down();
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(Err(failed(&thrown)));
        }
    }

    /// Stop listening, close the dialog if it is open and take it out of
    /// the page. The listeners stay alive until the box drops, since one
    /// may be running.
    fn take_down(&self) {
        let _ = self.dialog.remove_event_listener_with_callback(
            "close",
            self.listeners.close.as_ref().unchecked_ref(),
        );
        let _ = self.dialog.remove_event_listener_with_callback(
            "cancel",
            self.listeners.cancel.as_ref().unchecked_ref(),
        );
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

/// Give `element` the style `declarations`, through its style object.
fn style(element: &Element, declarations: &[(&str, &str)]) {
    let style = element.unchecked_ref::<HtmlElement>().style();
    for (property, value) in declarations {
        // A property the browser does not know is dropped, as a style sheet
        // drops it.
        let _ = style.set_property(property, value);
    }
}

/// The `<dialog>` for `request` in `window`: its title, its message and a
/// form of its buttons, the first of which takes focus. Its title and
/// message carry ids made of the window's id, which shows one box at a
/// time, so the dialog names and describes itself by them.
fn build(
    page: &Page,
    window: &WindowRef,
    request: &MessageBoxRequest,
) -> Result<HtmlDialogElement, DialogError> {
    let document = page.document();
    let element = |tag: &str| {
        document
            .create_element(tag)
            .map_err(|thrown| failed(&thrown))
    };
    let ids = format!("flui-message-box-{}", window.id().get());
    let (title_id, message_id) = (format!("{ids}-title"), format!("{ids}-message"));
    let dialog = element("dialog")?.unchecked_into::<HtmlDialogElement>();
    let role = match request.level {
        MessageLevel::Info => "dialog",
        MessageLevel::Warning | MessageLevel::Error => "alertdialog",
    };
    let _ = dialog.set_attribute("role", role);
    let _ = dialog.set_attribute("data-level", level_name(request.level));
    style(&dialog, &BOX_STYLE);
    let mut name = &message_id;
    if let Some(title) = &request.title {
        let heading = element("h2")?;
        heading.set_id(&title_id);
        heading.set_text_content(Some(title));
        style(&heading, &TITLE_STYLE);
        let _ = dialog.append_child(&heading);
        name = &title_id;
    }
    let _ = dialog.set_attribute("aria-labelledby", name);
    let _ = dialog.set_attribute("aria-describedby", &message_id);
    let message = element("p")?;
    message.set_id(&message_id);
    message.set_text_content(Some(&request.message));
    style(&message, &MESSAGE_STYLE);
    let _ = dialog.append_child(&message);
    let form = element("form")?;
    let _ = form.set_attribute("method", "dialog");
    style(&form, &BUTTONS_STYLE);
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
