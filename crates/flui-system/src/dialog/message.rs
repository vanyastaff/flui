//! The message box: a short message the user acknowledges or answers with
//! one of a fixed set of buttons.

use std::fmt;
use std::rc::Rc;

use flui_platform::capability::{Capability, ProviderContext, Resolver, WindowRef};
use flui_platform::{SystemFailure, Unsupported};

use super::request::{is_body, is_line};
use super::{Dialog, DialogError, DialogHold, DialogScope, MalformedRequest};

/// The message box capability: `ctx.capability::<MessageBoxCapability>()`
/// hands out [`MessageBoxes`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum MessageBoxCapability {}

impl Capability for MessageBoxCapability {
    type Handle = MessageBoxes;
    const NAME: &'static str = "message-box";
}

/// How serious a message is: the icon the box shows and the sound the
/// system plays with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MessageLevel {
    /// Something the user should know.
    #[default]
    Info,
    /// Something that may go wrong, or a choice with consequences.
    Warning,
    /// Something that went wrong.
    Error,
}

/// The buttons a message box offers, and so the answers it can give.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MessageButtons {
    /// One button that acknowledges the message: answers
    /// [`MessageAnswer::Ok`], closing the box included.
    #[default]
    Ok,
    /// Accept or cancel: answers [`MessageAnswer::Ok`] or
    /// [`MessageAnswer::Cancel`], which closing the box answers.
    OkCancel,
    /// A question with no way out but an answer: [`MessageAnswer::Yes`] or
    /// [`MessageAnswer::No`].
    YesNo,
    /// A question that may be left: [`MessageAnswer::Yes`],
    /// [`MessageAnswer::No`] or [`MessageAnswer::Cancel`], which closing the
    /// box answers.
    YesNoCancel,
}

impl MessageButtons {
    /// Whether a box with these buttons can answer `answer`.
    #[must_use]
    pub const fn admits(self, answer: MessageAnswer) -> bool {
        matches!(
            (self, answer),
            (Self::Ok, MessageAnswer::Ok)
                | (Self::OkCancel, MessageAnswer::Ok | MessageAnswer::Cancel)
                | (Self::YesNo, MessageAnswer::Yes | MessageAnswer::No)
                | (
                    Self::YesNoCancel,
                    MessageAnswer::Yes | MessageAnswer::No | MessageAnswer::Cancel
                )
        )
    }
}

/// The button the user chose, or what closing the box means for its
/// [`MessageButtons`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MessageAnswer {
    /// The message was acknowledged or accepted.
    Ok,
    /// The user cancelled.
    Cancel,
    /// The user answered yes.
    Yes,
    /// The user answered no.
    No,
}

/// A message box: the message, a title, how serious it is, and its buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MessageBoxRequest {
    /// The box's title, if set; otherwise the system's (Windows shows
    /// "Error").
    pub title: Option<String>,
    /// The message: not blank, at most [`MAX_TEXT_LEN`](super::MAX_TEXT_LEN)
    /// bytes, no NUL; line breaks are kept.
    pub message: String,
    /// How serious it is.
    pub level: MessageLevel,
    /// The buttons it offers.
    pub buttons: MessageButtons,
}

impl MessageBoxRequest {
    /// A box showing `message` at [`MessageLevel::Info`] with an OK button.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            title: None,
            message: message.into(),
            level: MessageLevel::default(),
            buttons: MessageButtons::default(),
        }
    }

    /// The box's title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// How serious the message is.
    #[must_use]
    pub fn level(mut self, level: MessageLevel) -> Self {
        self.level = level;
        self
    }

    /// The buttons the box offers.
    #[must_use]
    pub fn buttons(mut self, buttons: MessageButtons) -> Self {
        self.buttons = buttons;
        self
    }

    /// The first thing wrong with the request, which no box is shown for.
    fn check(&self) -> Result<(), MalformedRequest> {
        if self.message.trim().is_empty() {
            return Err(MalformedRequest::EmptyMessage);
        }
        if !is_body(&self.message) || !self.title.as_deref().is_none_or(is_line) {
            return Err(MalformedRequest::Text);
        }
        Ok(())
    }
}

/// What a message box answers: the button the user chose.
pub type MessageBoxResult = Result<MessageAnswer, DialogError>;

/// The system's message box as [`MessageBoxes`] reaches it.
///
/// Owner-local, with the contract of a
/// [`FileDialogBackend`](super::FileDialogBackend): a request it receives is
/// well formed, its window is open and shows no other dialog; it answers
/// through the [`Resolver`] and returns the [`DialogHold`] the box lasts by.
pub trait MessageBoxBackend: 'static {
    /// Show a message box modal to `window`.
    fn show(
        &self,
        window: &WindowRef,
        request: MessageBoxRequest,
        answer: Resolver<MessageBoxResult>,
    ) -> DialogHold;
}

/// Widget-facing message boxes for one window.
///
/// Owner-thread (`!Send`), cheap to clone. A box shares its window's
/// one-dialog rule with the [`FileDialogs`](super::FileDialogs) of that
/// window, ends with the window, and answers through its [`Dialog`], which
/// settles at an owner turn after the user answers.
#[derive(Clone)]
pub struct MessageBoxes {
    backend: Rc<dyn MessageBoxBackend>,
    scope: DialogScope,
}

impl MessageBoxes {
    /// A handle over `backend` for the acquisition `ctx`, bound to the
    /// acquiring widget's window.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] with
    /// [`UnsupportedReason::NoWindow`](flui_platform::UnsupportedReason::NoWindow)
    /// when the widget is in no window: a box is modal to one.
    pub fn try_new(
        backend: impl MessageBoxBackend,
        ctx: &ProviderContext<'_>,
    ) -> Result<Self, Unsupported> {
        Ok(Self {
            backend: Rc::new(backend),
            scope: DialogScope::try_new::<MessageBoxCapability>(ctx)?,
        })
    }

    /// Show `request`.
    ///
    /// A backend that answers a button the request does not offer answers
    /// [`SystemFailure::MalformedAnswer`].
    pub fn show(&self, request: MessageBoxRequest) -> Dialog<MessageBoxResult> {
        let checked = request.check();
        let buttons = request.buttons;
        self.scope.show(
            checked,
            |window, answer| self.backend.show(window, request, answer),
            move |result| match result {
                Ok(answer) if !buttons.admits(answer) => Err(SystemFailure::MalformedAnswer.into()),
                result => result,
            },
        )
    }
}

impl fmt::Debug for MessageBoxes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageBoxes")
            .field("window", &self.scope.window.id())
            .finish_non_exhaustive()
    }
}
