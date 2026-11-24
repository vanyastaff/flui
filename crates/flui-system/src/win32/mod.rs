//! The Windows providers: the shell's COM objects and Win32 calls.
//!
//! Each service runs where its documentation puts it, and no file system
//! call runs on the owner thread (ADR-0360 §2):
//!
//! - The file dialogs and the message box run on a thread of their own
//!   inside a single-threaded apartment ([`modal`]), owned by the acquiring
//!   widget's window, so the dialog is modal to it while the owner thread's
//!   own loop keeps running frames; a dialog ends through the thread's
//!   dismissal window when its caller lets it go or its window closes.
//! - The launcher, autostart, the jump list and the user's folders run each
//!   request on a short-lived apartment thread, at most
//!   [`crate::off_owner::MAX_IN_FLIGHT`] at once per UI runtime ([`threads`]).
//! - The share sheet, the global shortcuts, the taskbar entry and the
//!   system information run on the owner thread, whose apartment the host
//!   entered: they call the window's own thread's objects and read no file
//!   there.
//!
//! Every OS call sits behind a trait of its file, so the provider's rules
//! (what a request becomes, what an answer means, who hears it on every
//! exit) are tested with the OS's answers as inputs; the real calls are the
//! trait's one implementation.

// Each module below calls Win32 or COM: the workspace's `unsafe_code` lint is
// expected per module, and each block names its invariant.
#[expect(unsafe_code, reason = "the Run key is read and written through Win32")]
mod autostart;
#[expect(unsafe_code, reason = "the common item dialogs are COM objects")]
mod dialogs;
mod grants;
#[expect(unsafe_code, reason = "the jump list is a COM object")]
mod jump_list;
#[expect(unsafe_code, reason = "the shell's launch, reveal and trash calls")]
pub(crate) mod launcher;
#[expect(unsafe_code, reason = "MessageBoxW and the box's own end")]
mod message_box;
#[expect(unsafe_code, reason = "a dialog thread's dismissal window")]
mod modal;
#[expect(unsafe_code, reason = "the version and system information calls")]
mod os;
#[expect(unsafe_code, reason = "the share sheet's interop call")]
mod share;
#[expect(unsafe_code, reason = "RegisterHotKey and its message-only window")]
pub(crate) mod shortcuts;
#[expect(
    unsafe_code,
    reason = "ITaskbarList3, the button's subclass and its icon"
)]
pub(crate) mod taskbar;
#[expect(unsafe_code, reason = "the apartment each shell thread enters")]
mod threads;
#[expect(unsafe_code, reason = "the known folders' lookup")]
mod user_dirs;
mod verbatim;

use std::ffi::OsStr;
use std::num::NonZeroIsize;
use std::os::windows::ffi::OsStrExt as _;

use flui_platform::capability::{ProviderContext, WindowRef};
use flui_platform::launch::Launcher;
use flui_platform::{SystemFailure, Unsupported};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;

use crate::autostart::Autostart;
use crate::dialog::{FileDialogs, MessageBoxes};
use crate::grant::Grants;
use crate::jump_list::JumpList;
use crate::os::Os;
use crate::providers::{Providers, Provision};
use crate::share::Share;
use crate::shortcuts::GlobalShortcuts;
use crate::taskbar::TaskbarEntry;
use crate::user_dirs::UserDirs;

pub(crate) use threads::ShellThreads;

/// A window as another thread names it: its `HWND`'s value, which moves
/// between threads where the handle type does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner(NonZeroIsize);

impl Owner {
    /// The Win32 window `window` is, when its handle names one: `None` for a
    /// window of another kind (a headless window).
    pub(crate) fn of(window: &WindowRef) -> Option<Self> {
        match window.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => Some(Self(handle.hwnd)),
            _ => None,
        }
    }

    /// The window's handle, for a call on whatever thread holds this.
    pub(crate) fn hwnd(self) -> HWND {
        HWND(self.0.get() as *mut core::ffi::c_void)
    }
}

/// `text` as a NUL-terminated UTF-16 string for a Win32 call. A request the
/// seam admitted holds no NUL, so nothing is cut short.
pub(crate) fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>()
}

/// The code an OS failure reports: the Win32 error of an `HRESULT` that
/// wraps one (`0x8007xxxx`), the `HRESULT` itself otherwise.
pub(crate) fn os_code(hresult: windows::core::HRESULT) -> i32 {
    let value = hresult.0;
    // FACILITY_WIN32 with the failure bit: the low word is a Win32 error.
    if (value as u32) & 0xFFFF_0000 == 0x8007_0000 {
        value & 0xFFFF
    } else {
        value
    }
}

/// The [`SystemFailure`] of a failed Windows call.
pub(crate) fn failure(error: &windows::core::Error) -> SystemFailure {
    SystemFailure::Os(os_code(error.code()))
}

/// The Windows providers of every service.
pub(crate) fn providers() -> Providers {
    Providers {
        launcher: Provision::provided(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Launcher::new(
                launcher::WindowsLauncher::new(ctx.runtime_local::<ShellThreads>(), ctx.window()),
                ctx,
            ))
        }),
        file_dialogs: Provision::provided(|ctx: &ProviderContext<'_>| {
            FileDialogs::try_new(
                dialogs::WindowsDialogs::new(ctx.runtime_local::<modal::ModalWindows>()),
                ctx,
            )
        }),
        message_boxes: Provision::provided(|ctx: &ProviderContext<'_>| {
            MessageBoxes::try_new(
                message_box::WindowsMessageBoxes::new(ctx.runtime_local::<modal::ModalWindows>()),
                ctx,
            )
        }),
        share: Provision::provided(|ctx: &ProviderContext<'_>| {
            Share::try_new(share::WindowsShare::new(ctx), ctx)
        }),
        autostart: Provision::provided(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Autostart::new(
                autostart::WindowsAutostart::new(ctx.runtime_local::<ShellThreads>()),
                ctx,
            ))
        }),
        global_shortcuts: Provision::provided(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(GlobalShortcuts::new(
                shortcuts::WindowsShortcuts::new(ctx.runtime_local::<shortcuts::HotkeyWindow>()),
                ctx,
            ))
        }),
        taskbar: Provision::provided(|ctx: &ProviderContext<'_>| {
            let window = ctx.window();
            Ok::<_, Unsupported>(TaskbarEntry::new(
                taskbar::WindowsTaskbar::new(
                    ctx.runtime_local::<taskbar::TaskbarButtons>(),
                    window.as_ref(),
                ),
                ctx,
            ))
        }),
        user_dirs: Provision::provided(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(UserDirs::new(
                user_dirs::WindowsUserDirs::new(ctx.runtime_local::<ShellThreads>()),
                ctx,
            ))
        }),
        jump_list: Provision::provided(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(JumpList::new(
                jump_list::WindowsJumpList::new(ctx.runtime_local::<ShellThreads>()),
                ctx,
            ))
        }),
        os: Provision::provided(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Os::new(os::WindowsOs, ctx))
        }),
        grants: Provision::provided(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Grants::new(
                grants::WindowsGrants::new(ctx.runtime_local::<ShellThreads>()),
                ctx,
            ))
        }),
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::{E_FAIL, ERROR_ACCESS_DENIED};

    use super::os_code;

    #[test]
    fn a_wrapped_win32_error_reports_its_code_and_any_other_hresult_itself() {
        assert_eq!(os_code(ERROR_ACCESS_DENIED.to_hresult()), 5);
        assert_eq!(os_code(E_FAIL), E_FAIL.0);
    }
}
