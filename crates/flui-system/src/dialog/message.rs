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
    /// The answers its buttons give, in the order a box reads them: the
    /// accepting one first, which is the default button unless the request
    /// names another, and Cancel last.
    #[must_use]
    pub const fn answers(self) -> &'static [MessageAnswer] {
        match self {
            Self::Ok => &[MessageAnswer::Ok],
            Self::OkCancel => &[MessageAnswer::Ok, MessageAnswer::Cancel],
            Self::YesNo => &[MessageAnswer::Yes, MessageAnswer::No],
            Self::YesNoCancel => &[MessageAnswer::Yes, MessageAnswer::No, MessageAnswer::Cancel],
        }
    }

    /// Whether a box with these buttons can answer `answer`.
    #[must_use]
    pub const fn admits(self, answer: MessageAnswer) -> bool {
        let answers = self.answers();
        let mut at = 0;
        while at < answers.len() {
            // Compared by discriminant: `PartialEq` is not `const`.
            if answers[at] as u8 == answer as u8 {
                return true;
            }
            at += 1;
        }
        false
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

/// The labels a request gives its buttons, each kept by the answer its
/// button gives, so two requests that label the same buttons alike are
/// equal whatever order they were labelled in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ButtonLabels {
    ok: Option<String>,
    cancel: Option<String>,
    yes: Option<String>,
    no: Option<String>,
}

impl ButtonLabels {
    fn of(&self, answer: MessageAnswer) -> Option<&str> {
        match answer {
            MessageAnswer::Ok => self.ok.as_deref(),
            MessageAnswer::Cancel => self.cancel.as_deref(),
            MessageAnswer::Yes => self.yes.as_deref(),
            MessageAnswer::No => self.no.as_deref(),
        }
    }

    fn set(&mut self, answer: MessageAnswer, label: String) {
        let slot = match answer {
            MessageAnswer::Ok => &mut self.ok,
            MessageAnswer::Cancel => &mut self.cancel,
            MessageAnswer::Yes => &mut self.yes,
            MessageAnswer::No => &mut self.no,
        };
        *slot = Some(label);
    }

    /// Each label given, with its button's answer.
    fn iter(&self) -> impl Iterator<Item = (MessageAnswer, &str)> {
        [
            MessageAnswer::Ok,
            MessageAnswer::Cancel,
            MessageAnswer::Yes,
            MessageAnswer::No,
        ]
        .into_iter()
        .filter_map(|answer| Some((answer, self.of(answer)?)))
    }
}

/// A message box: the message, a title, a detail, how serious it is, and its
/// buttons with their default and their labels.
///
/// What each host shows of it:
///
/// | | Windows (`MessageBoxW`) | macOS (`NSAlert` sheet) | iOS (`UIAlertController`) | Web (`<dialog>`) |
/// |---|---|---|---|---|
/// | no title | an empty caption, never the system's "Error" | no heading: the message is the alert's message text | no title | no heading |
/// | detail | below the message, after an empty line | the informative text, below the title and message | below the message, after an empty line | a paragraph below the message |
/// | default button | `MB_DEFBUTTON` of its position | the button Return presses | the preferred action | the button that takes focus |
/// | button label | refused: [`Unsupported`](flui_platform::Unsupported) with `NotYetOnThisHost` | the button's title | the action's title | the button's text |
///
/// `MessageBoxW` cannot label a button, so a Windows box whose request
/// labels one is refused before anything is shown rather than shown with
/// words other than its author wrote; [`MessageBoxRequest::is_labelled`]
/// says whether a request does.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MessageBoxRequest {
    /// The box's title, if set; a box without one shows none.
    pub title: Option<String>,
    /// The message: not blank, at most [`MAX_TEXT_LEN`](super::MAX_TEXT_LEN)
    /// bytes, no NUL; line breaks are kept.
    pub message: String,
    /// More about the message, if set ([`detail`](Self::detail)).
    pub detail: Option<String>,
    /// How serious it is.
    pub level: MessageLevel,
    /// The buttons it offers.
    pub buttons: MessageButtons,
    /// The button Return presses, by its answer, if set
    /// ([`default_button`](Self::default_button)); otherwise the first of
    /// [`MessageButtons::answers`].
    pub default_button: Option<MessageAnswer>,
    labels: ButtonLabels,
}

impl MessageBoxRequest {
    /// A box showing `message` at [`MessageLevel::Info`] with an OK button.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            title: None,
            message: message.into(),
            detail: None,
            level: MessageLevel::default(),
            buttons: MessageButtons::default(),
            default_button: None,
            labels: ButtonLabels::default(),
        }
    }

    /// The box's title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// More about the message, shown below it in the body's lesser weight
    /// where the host has one: what will happen, what the user can do. Not
    /// blank, at most [`MAX_TEXT_LEN`](super::MAX_TEXT_LEN) bytes, no NUL;
    /// line breaks are kept.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
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

    /// The button Return presses: the one that answers `answer`, which the
    /// box's [`buttons`](Self::buttons) must offer. A confirmation of
    /// something that cannot be undone names the safe answer here
    /// (`MessageAnswer::No`, `MessageAnswer::Cancel`), so a key pressed by
    /// habit destroys nothing.
    #[must_use]
    pub fn default_button(mut self, answer: MessageAnswer) -> Self {
        self.default_button = Some(answer);
        self
    }

    /// Label the button that answers `answer` with `label` in place of the
    /// host's own word for it ("Delete" for the Yes of a confirmation): one
    /// line, not blank, on a button the box's [`buttons`](Self::buttons)
    /// offer, and no two buttons alike. The button still answers `answer`.
    /// Labelling a button again replaces its label.
    ///
    /// Windows' box cannot show a label, and refuses a labelled request with
    /// [`DialogError::Unsupported`] and `NotYetOnThisHost`.
    #[must_use]
    pub fn button_label(mut self, answer: MessageAnswer, label: impl Into<String>) -> Self {
        self.labels.set(answer, label.into());
        self
    }

    /// The label the request gives the button that answers `answer`;
    /// `None` when it gives none and the host's own word stands.
    #[must_use]
    pub fn label(&self, answer: MessageAnswer) -> Option<&str> {
        self.labels.of(answer)
    }

    /// Whether the request labels any of its buttons
    /// ([`button_label`](Self::button_label)): what a host that cannot show
    /// a label refuses it for.
    #[must_use]
    pub fn is_labelled(&self) -> bool {
        self.labels.iter().next().is_some()
    }

    /// The answer of the button Return presses: the one the request named,
    /// or the first of its buttons.
    #[must_use]
    pub fn default_answer(&self) -> MessageAnswer {
        self.default_button
            .unwrap_or_else(|| self.buttons.answers()[0])
    }

    /// The first thing wrong with the request, which no box is shown for.
    fn check(&self) -> Result<(), MalformedRequest> {
        if self.message.trim().is_empty() {
            return Err(MalformedRequest::EmptyMessage);
        }
        let detail_shows = self
            .detail
            .as_deref()
            .is_none_or(|detail| is_body(detail) && !detail.trim().is_empty());
        if !is_body(&self.message) || !self.title.as_deref().is_none_or(is_line) || !detail_shows {
            return Err(MalformedRequest::Text);
        }
        if !self
            .default_button
            .is_none_or(|answer| self.buttons.admits(answer))
        {
            return Err(MalformedRequest::DefaultButton);
        }
        let mut seen: Vec<&str> = Vec::new();
        for (answer, label) in self.labels.iter() {
            let shown = label.trim();
            if !self.buttons.admits(answer)
                || shown.is_empty()
                || !is_line(label)
                || seen.contains(&shown)
            {
                return Err(MalformedRequest::ButtonLabel);
            }
            seen.push(shown);
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
