//! The message box as an `AlertDialog` on Java's main thread.
//!
//! The library asks from the owner thread and builds and shows the dialog
//! in a job on the main thread; the dialog's three listeners are one proxy
//! (`FluiCallback`), and the box answers once, with whichever comes first:
//! a button, the user leaving (a back press, a touch outside), or a box
//! that could not be shown. Leaving answers what closing means for the
//! box's buttons, and a Yes or No box cannot be left. The buttons are
//! labelled in English, as on the other hosts: Android's own `yes` and `no`
//! strings read "OK" and "Cancel".
//!
//! The box ends with its window and with its caller's dialog (ADR-0344):
//! the hold's drop takes the caller's answer away first, so nothing answers
//! a dialog nobody holds, and dismisses the box on the main thread, or
//! keeps it from showing. A box Android took away without an answer (its
//! Activity went) answers nobody: its caller hears that the window closed,
//! or went with the runtime.

use flui_platform::{Unsupported, UnsupportedReason};

use crate::dialog::{
    DialogError, MessageAnswer, MessageBoxCapability, MessageBoxResult, MessageButtons,
};

/// How a box ended, as its listeners heard it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoxEvent {
    /// The positive button.
    Positive,
    /// The negative button.
    Negative,
    /// The neutral button.
    Neutral,
    /// The user left the box: a back press, or a touch outside it.
    Left,
    /// The box went without an answer: the library dismissed it, or Android
    /// did.
    Dismissed,
    /// The box could not be shown.
    NotShown,
}

/// `DialogInterface.BUTTON_POSITIVE`, `BUTTON_NEGATIVE` and
/// `BUTTON_NEUTRAL`: what `OnClickListener.onClick` names a button by.
const BUTTON_POSITIVE: i32 = -1;
const BUTTON_NEGATIVE: i32 = -2;
const BUTTON_NEUTRAL: i32 = -3;

/// The event a call of the listener's `method` with `which` is: a click by
/// its button, `onCancel` the user leaving, `onDismiss` the box going.
/// `None` for a call that is none of them.
pub(crate) fn event_of(method: &str, which: i32) -> Option<BoxEvent> {
    match (method, which) {
        ("onClick", BUTTON_POSITIVE) => Some(BoxEvent::Positive),
        ("onClick", BUTTON_NEGATIVE) => Some(BoxEvent::Negative),
        ("onClick", BUTTON_NEUTRAL) => Some(BoxEvent::Neutral),
        ("onCancel", _) => Some(BoxEvent::Left),
        ("onDismiss", _) => Some(BoxEvent::Dismissed),
        _ => None,
    }
}

/// One button of a box: its label and the answer it gives.
type Button = (&'static str, MessageAnswer);

/// How a box's buttons sit in an `AlertDialog`, and what leaving it means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BoxLayout {
    positive: Button,
    negative: Option<Button>,
    neutral: Option<Button>,
    /// What a back press or a touch outside answers; `None` for a box that
    /// cannot be left.
    left: Option<MessageAnswer>,
}

impl BoxLayout {
    /// The layout of a box offering `buttons`: the accepting button is the
    /// positive one, the refusing one the negative, and Cancel beside a Yes
    /// and a No the neutral.
    pub(crate) const fn of(buttons: MessageButtons) -> Self {
        const OK: Button = ("OK", MessageAnswer::Ok);
        const CANCEL: Button = ("Cancel", MessageAnswer::Cancel);
        const YES: Button = ("Yes", MessageAnswer::Yes);
        const NO: Button = ("No", MessageAnswer::No);
        match buttons {
            MessageButtons::Ok => Self {
                positive: OK,
                negative: None,
                neutral: None,
                left: Some(MessageAnswer::Ok),
            },
            MessageButtons::OkCancel => Self {
                positive: OK,
                negative: Some(CANCEL),
                neutral: None,
                left: Some(MessageAnswer::Cancel),
            },
            MessageButtons::YesNo => Self {
                positive: YES,
                negative: Some(NO),
                neutral: None,
                left: None,
            },
            MessageButtons::YesNoCancel => Self {
                positive: YES,
                negative: Some(NO),
                neutral: Some(CANCEL),
                left: Some(MessageAnswer::Cancel),
            },
        }
    }

    /// The labels of the positive, negative and neutral buttons, `None` for
    /// a button the box does not have.
    pub(crate) fn labels(&self) -> [Option<&'static str>; 3] {
        [
            Some(self.positive.0),
            self.negative.map(|(label, _)| label),
            self.neutral.map(|(label, _)| label),
        ]
    }

    /// Whether a back press or a touch outside leaves the box.
    pub(crate) const fn is_leavable(&self) -> bool {
        self.left.is_some()
    }

    /// What the box answers its caller for `event`; `None` for a box that
    /// went without an answer, which answers nobody.
    pub(crate) fn answer(&self, event: BoxEvent) -> Option<MessageBoxResult> {
        let malformed = Err(DialogError::Failed(
            flui_platform::SystemFailure::MalformedAnswer,
        ));
        let button = |button: Option<Button>| button.map_or(malformed, |(_, answer)| Ok(answer));
        match event {
            BoxEvent::Positive => Some(Ok(self.positive.1)),
            BoxEvent::Negative => Some(button(self.negative)),
            BoxEvent::Neutral => Some(button(self.neutral)),
            BoxEvent::Left => Some(self.left.map_or(malformed, Ok)),
            BoxEvent::Dismissed => None,
            BoxEvent::NotShown => Some(Err(not_shown())),
        }
    }
}

/// The answer of a box Android did not show: its window is not there to
/// attach to just now.
pub(crate) fn not_shown() -> DialogError {
    DialogError::Unsupported(Unsupported::of::<MessageBoxCapability>(
        UnsupportedReason::Unavailable,
    ))
}

#[cfg(target_os = "android")]
pub(crate) use provider::AndroidMessageBoxes;

#[cfg(target_os = "android")]
mod provider {
    use std::sync::Arc;

    use flui_platform::capability::{Activity, Resolver, WindowRef};

    use super::super::java::{BoxOnScreen, Java, Waiting};
    use super::{BoxEvent, BoxLayout, not_shown};
    use crate::dialog::{DialogHold, MessageBoxBackend, MessageBoxRequest, MessageBoxResult};

    /// [`MessageBoxBackend`] over `AlertDialog`.
    pub(crate) struct AndroidMessageBoxes {
        activity: Activity,
        java: Arc<Java>,
    }

    impl AndroidMessageBoxes {
        pub(crate) fn new(activity: Activity, java: Arc<Java>) -> Self {
            Self { activity, java }
        }
    }

    /// Dismisses a box when its dialog ends before the user answered.
    struct BoxHold {
        activity: Activity,
        java: Arc<Java>,
        waiting: Waiting<MessageBoxResult>,
        on_screen: Arc<BoxOnScreen>,
    }

    impl Drop for BoxHold {
        fn drop(&mut self) {
            // Taken first: the dismissal's own event, and a button the user
            // reaches before it, find nobody to answer.
            drop(self.waiting.take());
            self.java.dismiss_box(&self.activity, &self.on_screen);
        }
    }

    impl MessageBoxBackend for AndroidMessageBoxes {
        fn show(
            &self,
            _window: &WindowRef,
            request: MessageBoxRequest,
            answer: Resolver<MessageBoxResult>,
        ) -> DialogHold {
            let layout = BoxLayout::of(request.buttons);
            let waiting = Waiting::new(answer);
            let heard = waiting.clone();
            let on_screen = Arc::new(BoxOnScreen::default());
            let asked = self.java.show_box(
                &self.activity,
                request,
                layout,
                &on_screen,
                move |event: BoxEvent| {
                    // The first event answers; a box that went answers
                    // nobody, and leaves the answer to its hold.
                    if let Some(result) = layout.answer(event) {
                        heard.answer(result);
                    }
                },
            );
            if !asked {
                waiting.answer(Err(not_shown()));
                return DialogHold::new(());
            }
            DialogHold::new(BoxHold {
                activity: self.activity.clone(),
                java: Arc::clone(&self.java),
                waiting,
                on_screen,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{SystemFailure, UnsupportedReason};

    use super::{BoxEvent, BoxLayout, event_of};
    use crate::dialog::{DialogError, MessageAnswer, MessageButtons};

    /// Each button answers its answer, leaving answers what closing means
    /// for the buttons, a box that went answers nobody, and no event
    /// answers a button the box does not offer.
    #[test]
    fn a_box_answers_its_button_what_leaving_means_or_nobody() {
        let malformed = Err(DialogError::Failed(SystemFailure::MalformedAnswer));
        let rows = [
            (
                MessageButtons::Ok,
                [Some("OK"), None, None],
                [Ok(MessageAnswer::Ok), malformed, malformed],
                Some(MessageAnswer::Ok),
            ),
            (
                MessageButtons::OkCancel,
                [Some("OK"), Some("Cancel"), None],
                [Ok(MessageAnswer::Ok), Ok(MessageAnswer::Cancel), malformed],
                Some(MessageAnswer::Cancel),
            ),
            (
                MessageButtons::YesNo,
                [Some("Yes"), Some("No"), None],
                [Ok(MessageAnswer::Yes), Ok(MessageAnswer::No), malformed],
                None,
            ),
            (
                MessageButtons::YesNoCancel,
                [Some("Yes"), Some("No"), Some("Cancel")],
                [
                    Ok(MessageAnswer::Yes),
                    Ok(MessageAnswer::No),
                    Ok(MessageAnswer::Cancel),
                ],
                Some(MessageAnswer::Cancel),
            ),
        ];
        for (buttons, labels, answers, left) in rows {
            let layout = BoxLayout::of(buttons);
            assert_eq!(layout.labels(), labels, "{buttons:?}");
            let clicks = [BoxEvent::Positive, BoxEvent::Negative, BoxEvent::Neutral];
            for (event, expected) in clicks.into_iter().zip(answers) {
                assert_eq!(
                    layout.answer(event),
                    Some(expected),
                    "{buttons:?} {event:?}"
                );
                if let Ok(answer) = expected {
                    assert!(buttons.admits(answer), "{buttons:?} offers {answer:?}");
                }
            }
            assert_eq!(layout.is_leavable(), left.is_some(), "{buttons:?}");
            assert_eq!(
                layout.answer(BoxEvent::Left),
                Some(left.map_or(malformed, Ok)),
                "{buttons:?}"
            );
            assert_eq!(layout.answer(BoxEvent::Dismissed), None, "{buttons:?}");
            let not_shown = layout
                .answer(BoxEvent::NotShown)
                .expect("a box not shown answers")
                .expect_err("with a refusal");
            assert_eq!(
                not_shown
                    .unsupported()
                    .map(|unsupported| unsupported.reason),
                Some(UnsupportedReason::Unavailable)
            );
        }
    }

    /// A listener's call is a click by its button, the user leaving, or the
    /// box going; any other call is none of them.
    #[test]
    fn a_listeners_call_is_a_button_the_user_leaving_or_the_box_going() {
        for (method, which, expected) in [
            ("onClick", -1, Some(BoxEvent::Positive)),
            ("onClick", -2, Some(BoxEvent::Negative)),
            ("onClick", -3, Some(BoxEvent::Neutral)),
            ("onClick", 0, None),
            ("onClick", 2, None),
            ("onCancel", 0, Some(BoxEvent::Left)),
            ("onDismiss", 0, Some(BoxEvent::Dismissed)),
            ("run", 0, None),
        ] {
            assert_eq!(event_of(method, which), expected, "{method} {which}");
        }
    }
}
