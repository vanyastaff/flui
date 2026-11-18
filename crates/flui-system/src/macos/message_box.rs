//! The message box as an `NSAlert` sheet on the acquiring widget's window.
//!
//! The title is the alert's message text and the message its informative
//! text; with no title the message is the message text. The buttons are
//! added in the order AppKit places them from the right, the default first,
//! so `OkCancel` shows Cancel left of OK and Return answers OK; AppKit gives
//! Escape to the button titled Cancel. The alert ends with its window and
//! with its caller's dialog: the hold ends the sheet, and the completion
//! handler that ending runs answers nobody.

use flui_platform::SystemFailure;

use crate::dialog::{DialogError, MessageAnswer, MessageBoxResult, MessageButtons, MessageLevel};

/// The buttons an alert adds for `buttons`, in the order it adds them (the
/// first is the default, rightmost), with the answer each gives.
pub(super) fn buttons(buttons: MessageButtons) -> &'static [(&'static str, MessageAnswer)] {
    match buttons {
        MessageButtons::Ok => &[("OK", MessageAnswer::Ok)],
        MessageButtons::OkCancel => &[("OK", MessageAnswer::Ok), ("Cancel", MessageAnswer::Cancel)],
        MessageButtons::YesNo => &[("Yes", MessageAnswer::Yes), ("No", MessageAnswer::No)],
        MessageButtons::YesNoCancel => &[
            ("Yes", MessageAnswer::Yes),
            ("No", MessageAnswer::No),
            ("Cancel", MessageAnswer::Cancel),
        ],
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
        .and_then(|index| buttons(offered).get(index))
        .map(|(_, answer)| *answer)
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
    use super::{answer, buttons, style};
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
            match &request.title {
                Some(title) => {
                    alert.setMessageText(&NSString::from_str(title));
                    alert.setInformativeText(&NSString::from_str(&request.message));
                }
                None => alert.setMessageText(&NSString::from_str(&request.message)),
            }
            alert.setAlertStyle(NSAlertStyle(style(request.level)));
            for (title, _) in buttons(request.buttons) {
                let _ = alert.addButtonWithTitle(&NSString::from_str(title));
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

    use super::{answer, buttons, style};
    use crate::dialog::{DialogError, MessageAnswer, MessageButtons, MessageLevel};

    #[test]
    fn each_button_answers_its_answer_and_no_other_response_is_one() {
        let every = [
            MessageButtons::Ok,
            MessageButtons::OkCancel,
            MessageButtons::YesNo,
            MessageButtons::YesNoCancel,
        ];
        for offered in every {
            for (index, (_, expected)) in buttons(offered).iter().enumerate() {
                let response = 1000 + isize::try_from(index).expect("a few buttons");
                assert_eq!(answer(offered, response), Ok(*expected), "{offered:?}");
                assert!(offered.admits(*expected), "{offered:?} offers {expected:?}");
            }
            let past = 1000 + isize::try_from(buttons(offered).len()).expect("a few buttons");
            for response in [past, 999, 0, -1001] {
                assert_eq!(
                    answer(offered, response),
                    Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
                    "{offered:?} {response}"
                );
            }
        }
        assert_eq!(
            buttons(MessageButtons::OkCancel)[0],
            ("OK", MessageAnswer::Ok),
            "the default is added first"
        );
    }

    #[test]
    fn a_level_is_the_alerts_style() {
        assert_eq!(style(MessageLevel::Info), 1);
        assert_eq!(style(MessageLevel::Warning), 0);
        assert_eq!(style(MessageLevel::Error), 2);
    }
}
