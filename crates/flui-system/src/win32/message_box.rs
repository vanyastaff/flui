//! The message box over `MessageBoxW`, on its own apartment thread and owned
//! by the acquiring widget's window ([`super::modal`]).
//!
//! `MessageBoxW` is in every Windows (`TaskDialogIndirect` needs the
//! version 6 common controls, which an application without that manifest
//! entry does not load). Dismissing ends the box with `EndDialog` from its
//! own thread; the answer is then dropped.

use std::sync::Arc;

use flui_platform::SystemFailure;
use flui_platform::capability::{Resolver, WindowRef};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    EndDialog, EnumThreadWindows, GetClassNameW, IDCANCEL, IDNO, IDOK, IDYES, MB_ICONERROR,
    MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_OKCANCEL, MB_SETFOREGROUND, MB_YESNO,
    MB_YESNOCANCEL, MESSAGEBOX_RESULT, MESSAGEBOX_STYLE, MessageBoxW,
};
use windows::core::{BOOL, PCWSTR};

use super::modal::{Dismisser, show_answering};
use super::{Owner, wide};
use crate::dialog::{
    DialogError, DialogHold, MessageAnswer, MessageBoxBackend, MessageBoxCapability,
    MessageBoxRequest, MessageBoxResult, MessageButtons, MessageLevel,
};

/// The message box as the provider reaches it, on the box's thread: show
/// it with `style`, installing its close on `dismisser`, and return the
/// button chosen, or the code `MessageBoxW` failed with.
pub(super) trait MessageBoxSystem: Send + Sync + 'static {
    fn show(
        &self,
        owner: Option<Owner>,
        text: &str,
        caption: Option<&str>,
        style: MESSAGEBOX_STYLE,
        dismisser: &Dismisser,
    ) -> Result<MESSAGEBOX_RESULT, i32>;
}

/// The style a request asks for: its buttons and its icon.
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
    buttons | icon | MB_SETFOREGROUND
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
}

impl WindowsMessageBoxes {
    pub(crate) fn new() -> Self {
        Self {
            system: Arc::new(Win32MessageBox),
        }
    }
}

impl MessageBoxBackend for WindowsMessageBoxes {
    fn show(
        &self,
        window: &WindowRef,
        request: MessageBoxRequest,
        answer_to: Resolver<MessageBoxResult>,
    ) -> DialogHold {
        let owner = Owner::of(window);
        let style = style(&request);
        let system = Arc::clone(&self.system);
        show_answering::<MessageBoxCapability, _, _>(
            answer_to,
            move |dismisser| {
                system.show(
                    owner,
                    &request.message,
                    request.title.as_deref(),
                    style,
                    dismisser,
                )
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
        owner: Option<Owner>,
        text: &str,
        caption: Option<&str>,
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
        let caption = caption.map(wide);
        // SAFETY: `text` and `caption` are NUL-terminated buffers that
        // outlive the call; `owner` is a window of another thread, which a
        // message box may be owned by.
        let chosen = unsafe {
            MessageBoxW(
                owner.map(Owner::hwnd),
                PCWSTR(text.as_ptr()),
                caption
                    .as_ref()
                    .map_or(PCWSTR::null(), |caption| PCWSTR(caption.as_ptr())),
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
/// `#32770`, which is what `MessageBoxW` makes.
unsafe extern "system" fn end_box(window: HWND, _: LPARAM) -> BOOL {
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
    use flui_platform::SystemFailure;
    use windows::Win32::UI::WindowsAndMessaging::{
        IDABORT, IDCANCEL, IDNO, IDOK, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING,
        MB_OK, MB_OKCANCEL, MB_SETFOREGROUND, MB_YESNO, MB_YESNOCANCEL,
    };

    use super::{answer, style};
    use crate::dialog::{
        DialogError, MessageAnswer, MessageBoxRequest, MessageButtons, MessageLevel,
    };

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
}
