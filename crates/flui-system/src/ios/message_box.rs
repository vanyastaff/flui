//! The message box as a `UIAlertController` in the alert style, presented
//! from the requesting window's top controller (`presenter`).
//!
//! The title is the alert's title and the message its message, with the
//! detail below it after an empty line (an alert has one message). Each
//! button is an action whose handler answers it, titled by the request's
//! label or its English word, as on macOS and the web. The button that
//! closing the box means (OK of a box with one button, Cancel of a box that
//! has one) is the cancel-style action, which UIKit places apart and gives
//! Escape to on a hardware keyboard; the request's default button is the
//! preferred action, drawn bold and given Return. A Yes or No box has no
//! cancel action, so nothing closes it but an answer. The level has no form
//! in an alert, which shows no icon.
//!
//! A box is always the alert style, never an action sheet: that style is
//! for a choice about something the user just touched, and an iPad shows it
//! in a popover a tap outside dismisses. An alert is dismissed by no tap
//! and no swipe, so it answers exactly once, by a button, unless its dialog
//! ends first (the hold dismisses it, and no handler runs) or other code
//! dismisses it (UIKit releases it, and its handlers with the caller's
//! resolver, so the dialog ends unanswered rather than waiting for good).
//! A box asked for while another controller of its window is still leaving
//! is shown once that one has left.

use crate::dialog::{MessageAnswer, MessageBoxRequest, MessageButtons};

/// The alert's message: the request's message, and its detail below it
/// after an empty line.
pub(super) fn alert_message(request: &MessageBoxRequest) -> String {
    match &request.detail {
        Some(detail) => format!("{}\n\n{detail}", request.message),
        None => request.message.clone(),
    }
}

/// The title of `action` in an alert of `request`: the request's label for
/// its button, or its English word.
pub(super) fn action_title<'a>(request: &'a MessageBoxRequest, action: &Action) -> &'a str {
    request.label(action.answer).unwrap_or(action.title)
}

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
    use std::cell::Cell;
    use std::ptr::NonNull;
    use std::rc::Rc;

    use block2::RcBlock;
    use flui_platform::capability::{Resolver, WindowRef};
    use objc2::MainThreadMarker;
    use objc2::rc::{Retained, Weak};
    use objc2_foundation::NSString;
    use objc2_ui_kit::{
        UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle,
    };

    use super::super::presenter::{dismiss, guarded, present, when_presentable};
    use super::{ActionStyle, action_title, actions, alert_message};
    use crate::apple::modal::{Pending, WeakPending};
    use crate::dialog::{
        DialogHold, MessageBoxBackend, MessageBoxCapability, MessageBoxRequest, MessageBoxResult,
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

    /// The alert a box shows, once it shows one: a box asked for while a
    /// controller is leaving shows it later. UIKit keeps the alert while it
    /// is up, and the alert its handlers; this keeps neither.
    type ShownAlert = Rc<Cell<Option<Weak<UIAlertController>>>>;

    /// Dismisses an alert when its dialog ends before the user answered.
    struct AlertHold {
        pending: WeakPending<MessageBoxResult>,
        shown: ShownAlert,
    }

    impl Drop for AlertHold {
        fn drop(&mut self) {
            if self.pending.dismiss()
                && let Some(alert) = self.shown.take().and_then(|alert| alert.load())
            {
                dismiss(&alert);
            }
        }
    }

    /// The alert of `request`, each of its buttons an action that answers
    /// `pending`.
    fn build_alert(
        mtm: MainThreadMarker,
        request: &MessageBoxRequest,
        pending: &Pending<MessageBoxResult>,
    ) -> Retained<UIAlertController> {
        let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
            request.title.as_deref().map(NSString::from_str).as_deref(),
            Some(&NSString::from_str(&alert_message(request))),
            UIAlertControllerStyle::Alert,
            mtm,
        );
        let preferred = request.default_answer();
        for button in actions(request.buttons) {
            // Taken by the handler when it runs, so what it holds of the
            // caller is dropped inside the boundary.
            let answering = Cell::new(Some(pending.clone()));
            let given = button.answer;
            let handler = RcBlock::new(move |_action: NonNull<UIAlertAction>| {
                os_entry(
                    "UIAlertAction handler",
                    (),
                    AfterPanic::Answer(|()| ()),
                    || {
                        if let Some(pending) = answering.take() {
                            pending.answer(|| Ok(given));
                        }
                    },
                );
            });
            let style = match button.style {
                ActionStyle::Default => UIAlertActionStyle::Default,
                ActionStyle::Cancel => UIAlertActionStyle::Cancel,
            };
            let action = UIAlertAction::actionWithTitle_style_handler(
                Some(&NSString::from_str(action_title(request, button))),
                style,
                Some(&handler),
                mtm,
            );
            alert.addAction(&action);
            if button.answer == preferred {
                alert.setPreferredAction(Some(&action));
            }
        }
        alert
    }

    impl MessageBoxBackend for IOSMessageBoxes {
        fn show(
            &self,
            window: &WindowRef,
            request: MessageBoxRequest,
            answer: Resolver<MessageBoxResult>,
        ) -> DialogHold {
            let pending = Pending::new(answer);
            let hold = AlertHold {
                pending: pending.downgrade(),
                shown: ShownAlert::default(),
            };
            let shown = Rc::clone(&hold.shown);
            let mtm = self.mtm;
            // Until the alert is up, this closure is what keeps the
            // caller's resolver; then the alert's handlers are.
            when_presentable(window, mtm, move |from| {
                if !pending.is_waiting() {
                    // The dialog ended while a controller was leaving.
                    return;
                }
                let presented = from.and_then(|from| {
                    let alert =
                        guarded("making an alert", || build_alert(mtm, &request, &pending))?;
                    present(&from, &alert)?;
                    Ok(alert)
                });
                match presented {
                    Ok(alert) => shown.set(Some(Weak::from_retained(&alert))),
                    // UIKit shows nothing: no handler will run.
                    Err(refusal) => {
                        pending.answer(|| Err(refusal.dialog_error::<MessageBoxCapability>()));
                    }
                }
            });
            DialogHold::new(hold)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActionStyle, action_title, actions, alert_message};
    use crate::dialog::{MessageAnswer, MessageBoxRequest, MessageButtons};

    #[test]
    fn an_alert_shows_the_detail_below_the_message_and_each_buttons_label() {
        let request = MessageBoxRequest::new("Delete the note?")
            .detail("It cannot be brought back.")
            .buttons(MessageButtons::OkCancel)
            .button_label(MessageAnswer::Ok, "Delete");
        assert_eq!(
            alert_message(&request),
            "Delete the note?\n\nIt cannot be brought back."
        );
        assert_eq!(alert_message(&MessageBoxRequest::new("Saved")), "Saved");
        let titles = actions(request.buttons)
            .iter()
            .map(|action| action_title(&request, action))
            .collect::<Vec<&str>>();
        assert_eq!(titles, ["Delete", "Cancel"], "a label, or the English word");
    }

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
