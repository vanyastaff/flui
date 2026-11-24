//! The message box over `MessageBoxW`, on its own apartment thread and owned
//! by the acquiring widget's window ([`super::modal`]).
//!
//! `MessageBoxW` is in every Windows (`TaskDialogIndirect` needs the
//! version 6 common controls, which an application without that manifest
//! entry does not load). Dismissing ends the box with `EndDialog` from its
//! own thread; the answer is then dropped.
//!
//! What `MessageBoxW` has of a request: its message, with the detail below
//! it after an empty line, since the box has one text; its title as the
//! caption, an empty one for a request without a title, since a null
//! caption shows the system's "Error" whatever the level; its default
//! button, by its place among the buttons. It has no way to label a button,
//! so a request that labels one is refused with `NotYetOnThisHost` before
//! anything is shown, rather than shown with the system's own words where
//! its author wrote others. The box is owned by a window, which is what makes
//! it modal to it: a window whose handle names no Win32 window answers
//! `NoWindow`, and nothing is shown.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    EndDialog, EnumThreadWindows, GetClassNameW, IDCANCEL, IDNO, IDOK, IDYES, MB_DEFBUTTON1,
    MB_DEFBUTTON2, MB_DEFBUTTON3, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK,
    MB_OKCANCEL, MB_SETFOREGROUND, MB_YESNO, MB_YESNOCANCEL, MESSAGEBOX_RESULT, MESSAGEBOX_STYLE,
    MessageBoxW,
};
use windows::core::{BOOL, PCWSTR};

use super::modal::{Dismisser, ModalWindows, show_answering};
use super::{Owner, wide};
use crate::dialog::{
    DialogError, DialogHold, MessageAnswer, MessageBoxBackend, MessageBoxCapability,
    MessageBoxRequest, MessageBoxResult, MessageButtons, MessageLevel,
};
use flui_foundation::panic::{AfterPanic, os_entry};

/// The message box as the provider reaches it, on the box's thread: show
/// `text` under `caption` with `style`, owned by `owner`, installing its
/// close on `dismisser`, and return the button chosen, or the code
/// `MessageBoxW` failed with.
pub(super) trait MessageBoxSystem: Send + Sync + 'static {
    fn show(
        &self,
        owner: Owner,
        text: &str,
        caption: &str,
        style: MESSAGEBOX_STYLE,
        dismisser: &Dismisser,
    ) -> Result<MESSAGEBOX_RESULT, i32>;
}

/// The style a request asks for: its buttons, its icon and its default
/// button, which `MessageBoxW` names by its place among the buttons, the
/// order [`MessageButtons::answers`] lists them in.
pub(super) fn style(request: &MessageBoxRequest) -> MESSAGEBOX_STYLE {
    let buttons = match request.buttons {
        MessageButtons::OkCancel => MB_OKCANCEL,
        MessageButtons::YesNo => MB_YESNO,
        MessageButtons::YesNoCancel => MB_YESNOCANCEL,
        MessageButtons::Ok => MB_OK,
    };
    let icon = match request.level {
        MessageLevel::Warning => MB_ICONWARNING,
        MessageLevel::Error => MB_ICONERROR,
        MessageLevel::Info => MB_ICONINFORMATION,
    };
    let default = request.default_answer();
    let place = request
        .buttons
        .answers()
        .iter()
        .position(|answer| *answer == default);
    let default_button = match place {
        Some(1) => MB_DEFBUTTON2,
        Some(2) => MB_DEFBUTTON3,
        // The first button, and a default the seam would have refused.
        Some(_) | None => MB_DEFBUTTON1,
    };
    buttons | icon | default_button | MB_SETFOREGROUND
}

/// The box's one text: the message, and the detail below it after an empty
/// line.
pub(super) fn text(request: &MessageBoxRequest) -> String {
    match &request.detail {
        Some(detail) => format!("{}\n\n{detail}", request.message),
        None => request.message.clone(),
    }
}

/// The box's caption: the title, or none. Never a null caption, which
/// `MessageBoxW` reads as "Error" whatever the request's level.
pub(super) fn caption(request: &MessageBoxRequest) -> &str {
    request.title.as_deref().unwrap_or_default()
}

/// What the button `chosen` answers; a value the box cannot return is
/// outside its contract.
pub(super) fn answer(chosen: MESSAGEBOX_RESULT) -> MessageBoxResult {
    match chosen {
        IDOK => Ok(MessageAnswer::Ok),
        IDCANCEL => Ok(MessageAnswer::Cancel),
        IDYES => Ok(MessageAnswer::Yes),
        IDNO => Ok(MessageAnswer::No),
        _ => Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
    }
}

/// [`MessageBoxBackend`] over `MessageBoxW`.
pub(crate) struct WindowsMessageBoxes {
    system: Arc<dyn MessageBoxSystem>,
    windows: Rc<ModalWindows>,
}

impl WindowsMessageBoxes {
    pub(crate) fn new(windows: Rc<ModalWindows>) -> Self {
        Self::over(Arc::new(Win32MessageBox), windows)
    }

    pub(super) fn over(system: Arc<dyn MessageBoxSystem>, windows: Rc<ModalWindows>) -> Self {
        Self { system, windows }
    }
}

impl MessageBoxBackend for WindowsMessageBoxes {
    fn show(
        &self,
        window: &WindowRef,
        request: MessageBoxRequest,
        answer_to: Resolver<MessageBoxResult>,
    ) -> DialogHold {
        let refusal =
            |reason| DialogError::Unsupported(Unsupported::of::<MessageBoxCapability>(reason));
        if request.is_labelled() {
            // `MessageBoxW` would show other words than the request's. The
            // owner retiring first leaves nothing waiting.
            let _ = answer_to.resolve(Err(refusal(UnsupportedReason::NotYetOnThisHost)));
            return DialogHold::new(());
        }
        let Some(owner) = Owner::of(window) else {
            // A box with no owner would be modal to nothing.
            let _ = answer_to.resolve(Err(refusal(UnsupportedReason::NoWindow)));
            return DialogHold::new(());
        };
        let style = style(&request);
        let system = Arc::clone(&self.system);
        show_answering::<MessageBoxCapability, _, _>(
            &self.windows,
            window.id(),
            answer_to,
            move |dismisser| {
                system.show(owner, &text(&request), caption(&request), style, dismisser)
            },
            |chosen| {
                chosen
                    .map_err(|code| DialogError::Failed(SystemFailure::Os(code)))
                    .and_then(answer)
            },
        )
    }
}

/// The real `MessageBoxW`.
struct Win32MessageBox;

impl MessageBoxSystem for Win32MessageBox {
    fn show(
        &self,
        owner: Owner,
        text: &str,
        caption: &str,
        style: MESSAGEBOX_STYLE,
        dismisser: &Dismisser,
    ) -> Result<MESSAGEBOX_RESULT, i32> {
        dismisser.on_dismiss(|| {
            // SAFETY: called on the box's own thread from the dismissal
            // window's procedure, while `MessageBoxW` runs its modal loop;
            // the enumeration visits this thread's windows only.
            let _ = unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(end_box), LPARAM(0)) };
        });
        let text = wide(text);
        let caption = wide(caption);
        // SAFETY: `text` and `caption` are NUL-terminated buffers that
        // outlive the call; `owner` is a window of another thread, which a
        // message box may be owned by.
        let chosen = unsafe {
            MessageBoxW(
                Some(owner.hwnd()),
                PCWSTR(text.as_ptr()),
                PCWSTR(caption.as_ptr()),
                style,
            )
        };
        if chosen.0 == 0 {
            Err(super::os_code(windows::core::Error::from_thread().code()))
        } else {
            Ok(chosen)
        }
    }
}

/// Ends the message box among this thread's windows: the dialog class
/// `#32770`, which is what `MessageBoxW` makes. A panic in it is contained
/// and ends the enumeration.
unsafe extern "system" fn end_box(window: HWND, lparam: LPARAM) -> BOOL {
    os_entry(
        "the message box's dismissal",
        (window, lparam),
        AfterPanic::Answer(|_| BOOL(0)),
        || end_box_window(window),
    )
}

fn end_box_window(window: HWND) -> BOOL {
    let mut class = [0_u16; 16];
    // SAFETY: `class` is a live buffer of the length the call is given.
    let length = unsafe { GetClassNameW(window, &mut class) };
    let length = usize::try_from(length).unwrap_or(0);
    if class.get(..length) == Some("#32770".encode_utf16().collect::<Vec<u16>>().as_slice()) {
        // SAFETY: `window` is a dialog of this thread, the message box; it
        // ends with `IDCANCEL`, which the dismissed box's answer drops.
        let _ = unsafe { EndDialog(window, IDCANCEL.0 as isize) };
        return BOOL(0);
    }
    BOOL(1)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use flui_platform::capability::{Abandoned, WindowRef};
    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
    use windows::Win32::UI::WindowsAndMessaging::{
        IDABORT, IDCANCEL, IDNO, IDOK, IDYES, MB_DEFBUTTON1, MB_DEFBUTTON2, MB_DEFBUTTON3,
        MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_OKCANCEL, MB_SETFOREGROUND,
        MB_YESNO, MB_YESNOCANCEL, MESSAGEBOX_RESULT, MESSAGEBOX_STYLE,
    };

    use super::{MessageBoxSystem, WindowsMessageBoxes, answer, style};
    use crate::dialog::{
        DialogError, MessageAnswer, MessageBoxCapability, MessageBoxRequest, MessageBoxResult,
        MessageBoxes, MessageButtons, MessageLevel,
    };
    use crate::testing::win32::{TestOwner, win32_window, window};
    use crate::win32::Owner;
    use crate::win32::modal::Dismisser;

    #[test]
    fn a_request_becomes_its_buttons_and_icon_and_a_button_its_answer() {
        let cases = [
            (
                MessageButtons::Ok,
                MessageLevel::Info,
                MB_OK | MB_ICONINFORMATION,
            ),
            (
                MessageButtons::OkCancel,
                MessageLevel::Warning,
                MB_OKCANCEL | MB_ICONWARNING,
            ),
            (
                MessageButtons::YesNo,
                MessageLevel::Error,
                MB_YESNO | MB_ICONERROR,
            ),
            (
                MessageButtons::YesNoCancel,
                MessageLevel::Info,
                MB_YESNOCANCEL | MB_ICONINFORMATION,
            ),
        ];
        for (buttons, level, expected) in cases {
            let request = MessageBoxRequest::new("Saved")
                .buttons(buttons)
                .level(level);
            assert_eq!(
                style(&request),
                expected | MB_SETFOREGROUND,
                "{buttons:?} {level:?}"
            );
        }
        assert_eq!(answer(IDOK), Ok(MessageAnswer::Ok));
        assert_eq!(answer(IDCANCEL), Ok(MessageAnswer::Cancel));
        assert_eq!(answer(IDYES), Ok(MessageAnswer::Yes));
        assert_eq!(answer(IDNO), Ok(MessageAnswer::No));
        assert_eq!(
            answer(IDABORT),
            Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
            "a button the request did not offer"
        );
    }

    #[test]
    fn the_default_button_is_named_by_its_place_among_the_buttons() {
        let cases = [
            (MessageButtons::Ok, None, MB_DEFBUTTON1),
            (MessageButtons::OkCancel, None, MB_DEFBUTTON1),
            (
                MessageButtons::OkCancel,
                Some(MessageAnswer::Ok),
                MB_DEFBUTTON1,
            ),
            (
                MessageButtons::OkCancel,
                Some(MessageAnswer::Cancel),
                MB_DEFBUTTON2,
            ),
            (
                MessageButtons::YesNo,
                Some(MessageAnswer::No),
                MB_DEFBUTTON2,
            ),
            (
                MessageButtons::YesNoCancel,
                Some(MessageAnswer::Yes),
                MB_DEFBUTTON1,
            ),
            (
                MessageButtons::YesNoCancel,
                Some(MessageAnswer::No),
                MB_DEFBUTTON2,
            ),
            (
                MessageButtons::YesNoCancel,
                Some(MessageAnswer::Cancel),
                MB_DEFBUTTON3,
            ),
        ];
        // The bits `MessageBoxW` reads the default button from.
        let default_bits = MB_DEFBUTTON2 | MB_DEFBUTTON3;
        for (buttons, default, expected) in cases {
            let mut request = MessageBoxRequest::new("Delete the note?").buttons(buttons);
            request.default_button = default;
            assert_eq!(
                style(&request) & default_bits,
                expected,
                "{buttons:?} defaulting to {default:?}"
            );
        }
    }

    /// What a box was shown with.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Shown {
        owner: Owner,
        text: String,
        caption: String,
        style: MESSAGEBOX_STYLE,
    }

    /// A message box the user answers at once with the button it was given,
    /// recording what it was shown with.
    struct AnsweringBox {
        button: MESSAGEBOX_RESULT,
        shown: Mutex<Vec<Shown>>,
    }

    impl AnsweringBox {
        fn answering(button: MESSAGEBOX_RESULT) -> Arc<Self> {
            Arc::new(Self {
                button,
                shown: Mutex::new(Vec::new()),
            })
        }

        fn shown(&self) -> Vec<Shown> {
            self.shown.lock().expect("BUG: the test's lock").clone()
        }
    }

    impl MessageBoxSystem for AnsweringBox {
        fn show(
            &self,
            owner: Owner,
            text: &str,
            caption: &str,
            style: MESSAGEBOX_STYLE,
            _: &Dismisser,
        ) -> Result<MESSAGEBOX_RESULT, i32> {
            self.shown
                .lock()
                .expect("BUG: the test's lock")
                .push(Shown {
                    owner,
                    text: text.to_owned(),
                    caption: caption.to_owned(),
                    style,
                });
            Ok(self.button)
        }
    }

    /// The value of the Win32 window the tests' boxes are owned by, which no
    /// stand-in hands to the system.
    const OWNER: isize = 0x0F1E;

    /// Show `request` in `window` over `system` and turn the owner until it
    /// answers.
    fn ask(
        owner: &TestOwner,
        system: &Arc<AnsweringBox>,
        window: WindowRef,
        request: MessageBoxRequest,
    ) -> MessageBoxResult {
        let system: Arc<dyn MessageBoxSystem> = Arc::<AnsweringBox>::clone(system);
        let boxes = MessageBoxes::try_new(
            WindowsMessageBoxes::over(system, Rc::default()),
            &owner.ctx(Some(window)),
        )
        .expect("a window");
        let landed: Rc<RefCell<Option<Result<MessageBoxResult, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&landed);
        boxes
            .show(request)
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn_until(|| landed.borrow().is_some());
        landed
            .borrow_mut()
            .take()
            .expect("landed")
            .expect("not abandoned")
    }

    #[test]
    fn a_box_answers_from_its_thread_and_a_button_it_did_not_offer_is_refused() {
        let owner = TestOwner::new();
        let asked = |button, buttons| {
            let (window, _closer) = win32_window(OWNER);
            ask(
                &owner,
                &AnsweringBox::answering(button),
                window,
                MessageBoxRequest::new("Delete?").buttons(buttons),
            )
        };
        assert_eq!(asked(IDYES, MessageButtons::YesNo), Ok(MessageAnswer::Yes));
        assert_eq!(
            asked(IDCANCEL, MessageButtons::YesNo),
            Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
            "a yes-or-no box has no cancel"
        );
    }

    #[test]
    fn a_box_shows_its_title_or_none_its_detail_below_its_message_and_its_default_button() {
        let owner = TestOwner::new();
        let system = AnsweringBox::answering(IDNO);
        let (window, _closer) = win32_window(OWNER);
        let untitled = MessageBoxRequest::new("Delete the note?")
            .detail("It cannot be brought back.")
            .level(MessageLevel::Warning)
            .buttons(MessageButtons::YesNo)
            .default_button(MessageAnswer::No);
        assert_eq!(
            ask(&owner, &system, window, untitled),
            Ok(MessageAnswer::No)
        );
        let (window, _closer) = win32_window(OWNER);
        let titled = MessageBoxRequest::new("Saved").title("Notes");
        assert_eq!(
            ask(&owner, &system, window, titled),
            Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
            "the stand-in answers No, which an OK box does not offer"
        );

        let owned_by = Owner(std::num::NonZeroIsize::new(OWNER).expect("not zero"));
        assert_eq!(
            system.shown(),
            [
                Shown {
                    owner: owned_by,
                    text: "Delete the note?\n\nIt cannot be brought back.".to_owned(),
                    // Not a null caption, which the system shows as "Error".
                    caption: String::new(),
                    style: MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_SETFOREGROUND,
                },
                Shown {
                    owner: owned_by,
                    text: "Saved".to_owned(),
                    caption: "Notes".to_owned(),
                    style: MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
                },
            ]
        );
    }

    #[test]
    fn a_window_that_is_no_win32_window_is_refused_and_shows_no_box() {
        let owner = TestOwner::new();
        let system = AnsweringBox::answering(IDOK);
        let (window, _closer) = window();
        assert_eq!(
            ask(&owner, &system, window, MessageBoxRequest::new("Saved")),
            Err(DialogError::Unsupported(Unsupported::of::<
                MessageBoxCapability,
            >(
                UnsupportedReason::NoWindow
            ))),
            "a box is modal to a window"
        );
        assert_eq!(system.shown(), [], "no box was shown without an owner");
    }

    #[test]
    fn a_box_whose_request_labels_a_button_is_refused_and_shows_nothing() {
        let owner = TestOwner::new();
        let system = AnsweringBox::answering(IDYES);
        let (window, _closer) = win32_window(OWNER);
        let labelled = MessageBoxRequest::new("Delete the note?")
            .buttons(MessageButtons::YesNo)
            .button_label(MessageAnswer::Yes, "Delete");
        assert_eq!(
            ask(&owner, &system, window, labelled),
            Err(DialogError::Unsupported(Unsupported::of::<
                MessageBoxCapability,
            >(
                UnsupportedReason::NotYetOnThisHost
            ))),
            "MessageBoxW cannot show the label the request gives"
        );
        assert_eq!(system.shown(), [], "no box was shown with other words");
    }
}
