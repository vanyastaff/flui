//! The message box as an `NSAlert` sheet on the acquiring widget's window.
//!
//! An alert has two texts and no title bar. The title is its message text,
//! with the message and then the detail as its informative text; with no
//! title the message is the message text and the detail the informative
//! text, so a box without a title shows no heading of its own.
//!
//! The buttons are added in the order [`MessageButtons::answers`] gives,
//! which AppKit lays out from the right, each titled by the request's label
//! for it or by its English word. AppKit would give Return to the first
//! button added and Escape to one titled "Cancel"; the provider sets every
//! button's key itself, so the rule holds under any label: Return for the
//! request's default button, Escape for the one that answers Cancel when it
//! is not the default, and no key for the rest.
//!
//! The alert ends with its window and with its caller's dialog: the hold
//! ends the sheet, and the completion handler that ending runs answers
//! nobody.

use flui_platform::SystemFailure;

use crate::dialog::{
    DialogError, MessageAnswer, MessageBoxRequest, MessageBoxResult, MessageButtons, MessageLevel,
};

/// The key that presses an alert's button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ButtonKey {
    /// Return: the default button.
    Return,
    /// Escape: the button that cancels.
    Escape,
    /// No key: the button is clicked.
    Unbound,
}

impl ButtonKey {
    /// The button's `keyEquivalent`.
    pub(super) fn equivalent(self) -> &'static str {
        match self {
            Self::Return => "\r",
            Self::Escape => "\u{1b}",
            Self::Unbound => "",
        }
    }
}

/// One button of an alert, in the order it is added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AlertButton {
    pub(super) title: String,
    pub(super) answer: MessageAnswer,
    pub(super) key: ButtonKey,
}

/// The word an alert shows on the button that answers `answer` when the
/// request labels it with none. English: AppKit localizes only the OK it
/// adds itself, and no public API gives its Cancel, Yes or No.
fn standard_title(answer: MessageAnswer) -> &'static str {
    match answer {
        MessageAnswer::Ok => "OK",
        MessageAnswer::Cancel => "Cancel",
        MessageAnswer::Yes => "Yes",
        MessageAnswer::No => "No",
    }
}

/// The buttons an alert adds for `request`, in the order it adds them.
pub(super) fn buttons(request: &MessageBoxRequest) -> Vec<AlertButton> {
    let default = request.default_answer();
    request
        .buttons
        .answers()
        .iter()
        .map(|answer| AlertButton {
            title: request
                .label(*answer)
                .unwrap_or_else(|| standard_title(*answer))
                .to_owned(),
            answer: *answer,
            key: if *answer == default {
                ButtonKey::Return
            } else if *answer == MessageAnswer::Cancel {
                ButtonKey::Escape
            } else {
                ButtonKey::Unbound
            },
        })
        .collect::<Vec<AlertButton>>()
}

/// The alert's message text and its informative text for `request`: the
/// title over the message and the detail, or, with no title, the message
/// over the detail.
pub(super) fn texts(request: &MessageBoxRequest) -> (String, Option<String>) {
    match (&request.title, &request.detail) {
        (Some(title), Some(detail)) => (
            title.clone(),
            Some(format!("{}\n\n{detail}", request.message)),
        ),
        (Some(title), None) => (title.clone(), Some(request.message.clone())),
        (None, detail) => (request.message.clone(), detail.clone()),
    }
}

/// `NSAlertFirstButtonReturn`: the response of the first button added; the
/// next ones count up from it.
const FIRST_BUTTON: isize = 1000;

/// What an alert with `offered` buttons answers with `response`: the
/// answer of the button it names, or a malformed answer for one it does not
/// offer.
pub(super) fn answer(offered: MessageButtons, response: isize) -> MessageBoxResult {
    response
        .checked_sub(FIRST_BUTTON)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| offered.answers().get(index))
        .copied()
        .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))
}

/// The `NSAlertStyle` of `level`: informational, warning or critical.
pub(super) fn style(level: MessageLevel) -> usize {
    match level {
        MessageLevel::Info => 1,
        MessageLevel::Warning => 0,
        MessageLevel::Error => 2,
    }
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacMessageBoxes;

#[cfg(target_os = "macos")]
mod appkit {
    use block2::RcBlock;
    use flui_platform::capability::{Resolver, WindowRef};
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSAlert, NSAlertStyle, NSModalResponse, NSWindow};
    use objc2_foundation::NSString;

    use super::super::appkit::ns_window;
    use super::super::sheet::Pending;
    use super::{answer, buttons, style, texts};
    use crate::dialog::{
        DialogError, DialogHold, MessageBoxBackend, MessageBoxCapability, MessageBoxRequest,
        MessageBoxResult,
    };
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// `NSModalResponseAbort`: what a sheet the hold ends answers.
    const ABORTED: NSModalResponse = -1001;

    /// [`MessageBoxBackend`] over `NSAlert` sheets.
    pub(crate) struct MacMessageBoxes {
        mtm: MainThreadMarker,
    }

    impl MacMessageBoxes {
        pub(crate) fn new(mtm: MainThreadMarker) -> Self {
            Self { mtm }
        }
    }

    /// Ends an alert's sheet when the dialog ends before the user answered.
    struct AlertHold {
        pending: Pending<MessageBoxResult>,
        alert: Retained<NSAlert>,
        parent: Retained<NSWindow>,
    }

    impl Drop for AlertHold {
        fn drop(&mut self) {
            if self.pending.dismiss() {
                self.parent
                    .endSheet_returnCode(&self.alert.window(), ABORTED);
            }
        }
    }

    impl MessageBoxBackend for MacMessageBoxes {
        fn show(
            &self,
            window: &WindowRef,
            request: MessageBoxRequest,
            answer_to: Resolver<MessageBoxResult>,
        ) -> DialogHold {
            let Some(parent) = ns_window(window, self.mtm) else {
                // The owner retiring first leaves nothing waiting.
                let _ = answer_to.resolve(Err(DialogError::Unsupported(Unsupported::of::<
                    MessageBoxCapability,
                >(
                    UnsupportedReason::NoWindow,
                ))));
                return DialogHold::new(());
            };
            let alert = NSAlert::new(self.mtm);
            let (message, informative) = texts(&request);
            alert.setMessageText(&NSString::from_str(&message));
            if let Some(informative) = informative {
                alert.setInformativeText(&NSString::from_str(&informative));
            }
            alert.setAlertStyle(NSAlertStyle(style(request.level)));
            for button in buttons(&request) {
                let added = alert.addButtonWithTitle(&NSString::from_str(&button.title));
                added.setKeyEquivalent(&NSString::from_str(button.key.equivalent()));
            }
            let pending = Pending::new(answer_to);
            let handler_pending = pending.clone();
            let offered = request.buttons;
            let handler = RcBlock::new(move |response: NSModalResponse| {
                os_entry(
                    "NSAlert completion handler",
                    response,
                    AfterPanic::Answer(|_| ()),
                    || handler_pending.answer(|| answer(offered, response)),
                );
            });
            alert.beginSheetModalForWindow_completionHandler(&parent, Some(&handler));
            DialogHold::new(AlertHold {
                pending,
                alert,
                parent,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::SystemFailure;

    use super::{AlertButton, ButtonKey, answer, buttons, style, texts};
    use crate::dialog::{
        DialogError, MessageAnswer, MessageBoxRequest, MessageButtons, MessageLevel,
    };

    #[test]
    fn each_button_answers_its_answer_and_no_other_response_is_one() {
        let every = [
            MessageButtons::Ok,
            MessageButtons::OkCancel,
            MessageButtons::YesNo,
            MessageButtons::YesNoCancel,
        ];
        for offered in every {
            let added = buttons(&MessageBoxRequest::new("Saved").buttons(offered));
            for (index, button) in added.iter().enumerate() {
                let response = 1000 + isize::try_from(index).expect("a few buttons");
                assert_eq!(answer(offered, response), Ok(button.answer), "{offered:?}");
                assert!(offered.admits(button.answer), "{offered:?} offers it");
            }
            let past = 1000 + isize::try_from(added.len()).expect("a few buttons");
            for response in [past, 999, 0, -1001] {
                assert_eq!(
                    answer(offered, response),
                    Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
                    "{offered:?} {response}"
                );
            }
        }
    }

    #[test]
    fn return_presses_the_default_button_and_escape_the_one_that_cancels_under_any_label() {
        let button = |title: &str, answer, key| AlertButton {
            title: title.to_owned(),
            answer,
            key,
        };
        assert_eq!(
            buttons(&MessageBoxRequest::new("Save?").buttons(MessageButtons::YesNoCancel)),
            [
                button("Yes", MessageAnswer::Yes, ButtonKey::Return),
                button("No", MessageAnswer::No, ButtonKey::Unbound),
                button("Cancel", MessageAnswer::Cancel, ButtonKey::Escape),
            ],
            "the first button is the default unless the request names another"
        );
        assert_eq!(
            buttons(
                &MessageBoxRequest::new("Delete the note?")
                    .buttons(MessageButtons::YesNoCancel)
                    .default_button(MessageAnswer::No)
                    .button_label(MessageAnswer::Yes, "Delete")
                    .button_label(MessageAnswer::Cancel, "Keep editing")
            ),
            [
                // No Return left on the first button, which AppKit gives it.
                button("Delete", MessageAnswer::Yes, ButtonKey::Unbound),
                button("No", MessageAnswer::No, ButtonKey::Return),
                // Escape by its answer: AppKit reads it from the title.
                button("Keep editing", MessageAnswer::Cancel, ButtonKey::Escape),
            ]
        );
        assert_eq!(
            buttons(
                &MessageBoxRequest::new("Replace it?")
                    .buttons(MessageButtons::OkCancel)
                    .default_button(MessageAnswer::Cancel)
            ),
            [
                button("OK", MessageAnswer::Ok, ButtonKey::Unbound),
                button("Cancel", MessageAnswer::Cancel, ButtonKey::Return),
            ],
            "a button has one key: the default's"
        );
        assert_eq!(ButtonKey::Return.equivalent(), "\r");
        assert_eq!(ButtonKey::Escape.equivalent(), "\u{1b}");
        assert_eq!(ButtonKey::Unbound.equivalent(), "");
    }

    #[test]
    fn an_alert_shows_the_title_over_the_message_and_the_detail_or_the_message_over_the_detail() {
        let request = || MessageBoxRequest::new("The note was deleted.");
        assert_eq!(
            texts(&request()),
            ("The note was deleted.".to_owned(), None),
            "no title: no heading but the message"
        );
        assert_eq!(
            texts(&request().detail("It is in the trash for 30 days.")),
            (
                "The note was deleted.".to_owned(),
                Some("It is in the trash for 30 days.".to_owned())
            )
        );
        assert_eq!(
            texts(&request().title("Notes")),
            ("Notes".to_owned(), Some("The note was deleted.".to_owned()))
        );
        assert_eq!(
            texts(
                &request()
                    .title("Notes")
                    .detail("It is in the trash for 30 days.")
            ),
            (
                "Notes".to_owned(),
                Some("The note was deleted.\n\nIt is in the trash for 30 days.".to_owned())
            )
        );
    }

    #[test]
    fn a_level_is_the_alerts_style() {
        assert_eq!(style(MessageLevel::Info), 1);
        assert_eq!(style(MessageLevel::Warning), 0);
        assert_eq!(style(MessageLevel::Error), 2);
    }
}
