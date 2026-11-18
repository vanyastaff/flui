//! The message box as a `UIAlertController` in the alert style, presented
//! from the requesting window's top controller (`presenter`).
//!
//! The title is the alert's title and the message its message. Each button
//! is an action whose handler answers it. The button that closing the box
//! means (OK of a box with one button, Cancel of a box that has one) is the
//! cancel-style action, which UIKit places apart and gives Escape to on a
//! hardware keyboard; the default button is the preferred action, drawn
//! bold and given Return. A Yes or No box has no cancel action, so nothing
//! closes it but an answer. The labels are English, as on macOS and the
//! web. The level has no form in an alert, which shows no icon.
//!
//! A box is always the alert style, never an action sheet: that style is
//! for a choice about something the user just touched, and an iPad shows it
//! in a popover a tap outside dismisses. An alert is dismissed by no tap
//! and no swipe, so it answers exactly once, by a button, unless its dialog
//! ends first (the hold dismisses it, and no handler runs) or other code
//! dismisses it (UIKit releases it, and its handlers with the caller's
//! resolver, so the dialog ends unanswered rather than waiting for good).

use crate::dialog::{MessageAnswer, MessageButtons};

/// How UIKit draws an action and what it does with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionStyle {
    /// `UIAlertActionStyleDefault`.
    Default,
    /// `UIAlertActionStyleCancel`: placed apart, and given Escape.
    Cancel,
}

/// One button of an alert: its title, its style and the answer it gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Action {
    pub(super) title: &'static str,
    pub(super) style: ActionStyle,
    pub(super) answer: MessageAnswer,
}

/// The actions an alert adds for `buttons`, the default first: the one the
/// alert prefers. At most one is the cancel style, since UIKit raises an
/// exception for a second.
pub(super) fn actions(buttons: MessageButtons) -> &'static [Action] {
    const fn action(title: &'static str, style: ActionStyle, answer: MessageAnswer) -> Action {
        Action {
            title,
            style,
            answer,
        }
    }
    const OK: Action = action("OK", ActionStyle::Default, MessageAnswer::Ok);
    const CLOSING_OK: Action = action("OK", ActionStyle::Cancel, MessageAnswer::Ok);
    const CANCEL: Action = action("Cancel", ActionStyle::Cancel, MessageAnswer::Cancel);
    const YES: Action = action("Yes", ActionStyle::Default, MessageAnswer::Yes);
    const NO: Action = action("No", ActionStyle::Default, MessageAnswer::No);
    match buttons {
        MessageButtons::Ok => &[CLOSING_OK],
        MessageButtons::OkCancel => &[OK, CANCEL],
        MessageButtons::YesNo => &[YES, NO],
        MessageButtons::YesNoCancel => &[YES, NO, CANCEL],
    }
}

#[cfg(target_os = "ios")]
pub(crate) use uikit::IOSMessageBoxes;

#[cfg(target_os = "ios")]
mod uikit {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use flui_platform::capability::{Resolver, WindowRef};
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2::MainThreadMarker;
    use objc2::rc::Weak;
    use objc2_foundation::NSString;
    use objc2_ui_kit::{
        UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle,
    };

    use super::super::presenter::{Refusal, dismiss, present, presenter};
    use super::{ActionStyle, actions};
    use crate::apple::modal::{Pending, WeakPending};
    use crate::dialog::{
        DialogError, DialogHold, MessageBoxBackend, MessageBoxCapability, MessageBoxRequest,
        MessageBoxResult,
    };
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// [`MessageBoxBackend`] over `UIAlertController`.
    pub(crate) struct IOSMessageBoxes {
        mtm: MainThreadMarker,
    }

    impl IOSMessageBoxes {
        pub(crate) fn new(mtm: MainThreadMarker) -> Self {
            Self { mtm }
        }
    }

    /// Dismisses an alert when its dialog ends before the user answered.
    /// UIKit keeps the alert while it is up, and the alert its handlers:
    /// the hold keeps neither.
    struct AlertHold {
        pending: WeakPending<MessageBoxResult>,
        alert: Weak<UIAlertController>,
    }

    impl Drop for AlertHold {
        fn drop(&mut self) {
            if self.pending.dismiss()
                && let Some(alert) = self.alert.load()
            {
                dismiss(&alert);
            }
        }
    }

    fn refused(reason: UnsupportedReason) -> DialogError {
        DialogError::Unsupported(Unsupported::of::<MessageBoxCapability>(reason))
    }

    impl MessageBoxBackend for IOSMessageBoxes {
        fn show(
            &self,
            window: &WindowRef,
            request: MessageBoxRequest,
            answer: Resolver<MessageBoxResult>,
        ) -> DialogHold {
            let from = match presenter(window, self.mtm) {
                Ok(from) => from,
                Err(refusal) => {
                    let error = match refusal {
                        Refusal::NoWindow => refused(UnsupportedReason::NoWindow),
                        Refusal::Busy => DialogError::Busy,
                    };
                    // The owner retiring first leaves nothing waiting.
                    let _ = answer.resolve(Err(error));
                    return DialogHold::new(());
                }
            };
            let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
                request.title.as_deref().map(NSString::from_str).as_deref(),
                Some(&NSString::from_str(&request.message)),
                UIAlertControllerStyle::Alert,
                self.mtm,
            );
            let pending = Pending::new(answer);
            for (index, button) in actions(request.buttons).iter().enumerate() {
                let handler_pending = pending.clone();
                let given = button.answer;
                let handler = RcBlock::new(move |_action: NonNull<UIAlertAction>| {
                    os_entry(
                        "UIAlertAction handler",
                        (),
                        AfterPanic::Answer(|()| ()),
                        || handler_pending.answer(|| Ok(given)),
                    );
                });
                let style = match button.style {
                    ActionStyle::Default => UIAlertActionStyle::Default,
                    ActionStyle::Cancel => UIAlertActionStyle::Cancel,
                };
                let action = UIAlertAction::actionWithTitle_style_handler(
                    Some(&NSString::from_str(button.title)),
                    style,
                    Some(&handler),
                    self.mtm,
                );
                alert.addAction(&action);
                if index == 0 {
                    alert.setPreferredAction(Some(&action));
                }
            }
            if !present(&from, &alert) {
                // UIKit took nothing: no handler will run.
                pending.answer(|| Err(DialogError::Busy));
                return DialogHold::new(());
            }
            DialogHold::new(AlertHold {
                pending: pending.downgrade(),
                alert: Weak::from_retained(&alert),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActionStyle, actions};
    use crate::dialog::{MessageAnswer, MessageButtons};

    #[test]
    fn each_button_is_an_action_and_only_what_closing_means_is_the_cancel_one() {
        let cases = [
            (MessageButtons::Ok, Some(MessageAnswer::Ok)),
            (MessageButtons::OkCancel, Some(MessageAnswer::Cancel)),
            (MessageButtons::YesNo, None),
            (MessageButtons::YesNoCancel, Some(MessageAnswer::Cancel)),
        ];
        for (offered, closing) in cases {
            let added = actions(offered);
            for action in added {
                assert!(offered.admits(action.answer), "{offered:?} {action:?}");
            }
            for (index, action) in added.iter().enumerate() {
                assert!(
                    added[index + 1..]
                        .iter()
                        .all(|later| later.answer != action.answer),
                    "{offered:?}: one action per answer"
                );
            }
            let cancelling = added
                .iter()
                .filter(|action| action.style == ActionStyle::Cancel)
                .map(|action| action.answer)
                .collect::<Vec<MessageAnswer>>();
            assert_eq!(
                cancelling,
                closing.into_iter().collect::<Vec<MessageAnswer>>(),
                "{offered:?}: UIKit allows one cancel action, and it is what closing means"
            );
        }
        assert_eq!(
            actions(MessageButtons::YesNoCancel)[0].answer,
            MessageAnswer::Yes,
            "the default is first, the preferred action"
        );
        assert_eq!(
            actions(MessageButtons::OkCancel)[0].answer,
            MessageAnswer::Ok
        );
    }
}
