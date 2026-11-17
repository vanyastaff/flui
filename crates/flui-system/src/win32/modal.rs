//! A modal dialog on a thread of its own.
//!
//! A common item dialog or a message box runs a modal loop inside the call
//! that shows it. On the owner thread that loop would stand in for the main
//! loop, which a capability crate cannot pace (the host's modal pacing is
//! the host's), and would run inside the widget's call. So each dialog gets
//! a thread inside a single-threaded apartment, and the acquiring widget's
//! window is its owner: the dialog disables that window and stays above it,
//! while the owner thread's own loop keeps delivering frames and owner
//! turns. A window and its owned dialog on two threads share input state;
//! both threads keep pumping, so neither waits on the other.
//!
//! Before it shows the dialog, the thread makes a message-only window of
//! its own, the dismissal window. Dropping the [`ModalHold`] posts it a
//! message, which the dialog's modal loop dispatches on the dialog's thread,
//! where the dialog's own close runs ([`Dismisser`]): `IFileDialog::Close`,
//! or `EndDialog` on the message box. A hold dropped before the dialog was
//! shown keeps it from being shown; one dropped after it answered does
//! nothing. Whatever ended the dialog, the thread hands the answer on only
//! when nobody dismissed it.

use std::cell::Cell;
use std::sync::{Arc, Mutex, PoisonError};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HWND_MESSAGE,
    PostMessageW, RegisterClassExW, SetWindowLongPtrW, WINDOW_EX_STYLE, WM_APP, WNDCLASSEXW,
    WS_OVERLAPPED,
};
use windows::core::w;

use flui_platform::capability::{Capability, Resolver};
use flui_platform::{Unsupported, UnsupportedReason};

use super::threads::{Apartment, OnceAnswer};
use crate::dialog::{DialogError, DialogHold};

/// The message the dismissal window takes as "dismiss the dialog".
const DISMISS: u32 = WM_APP + 0x51;

/// Show a dialog of capability `C` on its thread with `show`, and answer
/// `answer` with what `meaning` makes of the system's answer. A thread
/// that cannot start answers `Unavailable` at once; a dismissed dialog
/// answers nobody, and the handle has already ended it.
pub(super) fn show_answering<C, A, T>(
    answer: Resolver<Result<T, DialogError>>,
    show: impl FnOnce(&Dismisser) -> A + Send + 'static,
    meaning: impl FnOnce(A) -> Result<T, DialogError> + Send + 'static,
) -> DialogHold
where
    C: Capability,
    A: 'static,
    T: Send + 'static,
{
    let (thread, here) = OnceAnswer::pair(answer);
    let shown = show_on_thread(show, move |system| thread.resolve(|| meaning(system)));
    if let Ok(hold) = shown {
        return DialogHold::new(hold);
    }
    here.resolve(|| {
        Err(DialogError::Unsupported(Unsupported::of::<C>(
            UnsupportedReason::Unavailable,
        )))
    });
    DialogHold::new(())
}

/// What a dialog's thread is doing, as the hold and the thread see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// The thread has not shown the dialog yet.
    Starting,
    /// The dialog is up; its dismissal window is this handle's value.
    Showing(isize),
    /// The hold dropped before the dialog answered: its answer, if any, is
    /// dropped.
    Dismissed,
    /// The dialog answered.
    Answered,
}

/// What the hold and the dialog's thread share.
struct Shared {
    stage: Mutex<Stage>,
}

impl Shared {
    fn stage(&self) -> std::sync::MutexGuard<'_, Stage> {
        // A stage is a plain value; a panic while it was held leaves one.
        self.stage.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What a dialog's backend returns as its hold: dropping it dismisses the
/// dialog.
pub(super) struct ModalHold {
    shared: Arc<Shared>,
}

impl Drop for ModalHold {
    fn drop(&mut self) {
        let mut stage = self.shared.stage();
        match *stage {
            Stage::Starting => *stage = Stage::Dismissed,
            Stage::Showing(window) => {
                *stage = Stage::Dismissed;
                drop(stage);
                // SAFETY: `window` is the dismissal window of a thread still
                // showing its dialog: the thread destroys it only after it
                // left `Showing` under the same lock, which this drop just
                // did. A post to a window destroyed meanwhile fails and
                // changes nothing; nothing else is passed.
                let posted = unsafe {
                    PostMessageW(
                        Some(HWND(window as *mut core::ffi::c_void)),
                        DISMISS,
                        WPARAM(0),
                        LPARAM(0),
                    )
                };
                // A post that failed leaves the dialog up until the user
                // closes it; its answer is dropped either way, since the
                // stage is `Dismissed`.
                drop(posted);
            }
            Stage::Dismissed | Stage::Answered => {}
        }
    }
}

/// How the dialog on this thread is closed: the dialog's own close, which
/// the code that shows it installs before it calls the system.
///
/// Thread-local by construction: it lives on the dialog thread's stack and
/// is reached only by the dismissal window's procedure on that thread.
#[derive(Default)]
pub(super) struct Dismisser {
    close: Cell<Option<Box<dyn FnOnce()>>>,
}

impl Dismisser {
    /// Close the dialog with `close` when it is dismissed.
    pub(super) fn on_dismiss(&self, close: impl FnOnce() + 'static) {
        self.close.set(Some(Box::new(close)));
    }

    fn dismiss(&self) {
        if let Some(close) = self.close.take() {
            close();
        }
    }
}

/// Show a dialog on a new apartment thread: `show` shows it, installing its
/// close on the [`Dismisser`], and returns when it ends; `answer` hears what
/// it returned unless the hold was dropped first.
///
/// # Errors
///
/// The thread could not be started; `show` and `answer` are dropped unrun.
pub(super) fn show_on_thread<R: 'static>(
    show: impl FnOnce(&Dismisser) -> R + Send + 'static,
    answer: impl FnOnce(R) + Send + 'static,
) -> std::io::Result<ModalHold> {
    let shared = Arc::new(Shared {
        stage: Mutex::new(Stage::Starting),
    });
    let thread = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("flui-dialog".to_owned())
        .spawn(move || run_dialog(&thread, show, answer))?;
    Ok(ModalHold { shared })
}

fn run_dialog<R>(shared: &Shared, show: impl FnOnce(&Dismisser) -> R, answer: impl FnOnce(R)) {
    let _apartment = Apartment::enter();
    let dismisser = Dismisser::default();
    let window = DismissalWindow::create(&dismisser);
    {
        let mut stage = shared.stage();
        if *stage == Stage::Dismissed {
            return;
        }
        // Without a dismissal window the dialog cannot be dismissed early:
        // it stays up until the user answers, and the answer is dropped.
        *stage = Stage::Showing(window.as_ref().map_or(0, DismissalWindow::value));
    }
    let result = show(&dismisser);
    let dismissed = {
        let mut stage = shared.stage();
        let dismissed = *stage == Stage::Dismissed;
        *stage = Stage::Answered;
        dismissed
    };
    // Destroyed after the stage left `Showing`, so no hold posts to it after.
    drop(window);
    if !dismissed {
        answer(result);
    }
}

/// The dismissal window of a dialog's thread: message-only, its user data
/// the thread's [`Dismisser`].
struct DismissalWindow(HWND);

impl DismissalWindow {
    /// The window, or `None` when it could not be made.
    fn create(dismisser: &Dismisser) -> Option<Self> {
        let class = w!("FluiSystemDialogDismissal");
        // SAFETY: the class names a procedure of this module and the
        // module's own instance; registering it again from another thread
        // fails with `ERROR_CLASS_ALREADY_EXISTS` and leaves the first,
        // identical registration in place.
        let instance = unsafe { GetModuleHandleW(None) }.ok()?;
        let description = WNDCLASSEXW {
            cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>())
                .expect("BUG: WNDCLASSEXW is a few dozen bytes"),
            lpfnWndProc: Some(dismissal_procedure),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        // SAFETY: as above; `description` outlives the call.
        let _ = unsafe { RegisterClassExW(&raw const description) };
        // SAFETY: a message-only window of the class registered above, on
        // this thread, with no parameter of ours.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                None,
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
        }
        .ok()?;
        // SAFETY: `window` is this thread's window, just made; the pointer
        // stored is the thread's `Dismisser`, which outlives the window: the
        // window is destroyed in `drop` before `run_dialog` returns, and
        // `run_dialog` holds the `Dismisser` on its stack until then.
        unsafe {
            SetWindowLongPtrW(
                window,
                GWLP_USERDATA,
                std::ptr::from_ref(dismisser) as isize,
            )
        };
        Some(Self(window))
    }

    fn value(&self) -> isize {
        self.0.0 as isize
    }
}

impl Drop for DismissalWindow {
    fn drop(&mut self) {
        // SAFETY: the window is this thread's, made by `create`; destroying
        // it ends every later dispatch to its procedure.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

/// The dismissal window's procedure: on [`DISMISS`], the dialog's own close.
/// It runs only the close the dialog's code installed, which calls the
/// system and nothing of the application's.
unsafe extern "system" fn dismissal_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == DISMISS {
        // SAFETY: the user data is null until `create` stores the thread's
        // `Dismisser`, which outlives the window (see `create`); this
        // procedure runs on the window's own thread, the `Dismisser`'s.
        let dismisser = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const Dismisser;
        // SAFETY: as above: null or the live `Dismisser` of this thread.
        if let Some(dismisser) = unsafe { dismisser.as_ref() } {
            dismisser.dismiss();
        }
        return LRESULT(0);
    }
    // SAFETY: every other message goes to the default procedure with the
    // arguments the system passed.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };

    use std::sync::{Arc, Mutex};

    use super::{Dismisser, ModalHold, Shared, Stage, run_dialog, show_on_thread};

    /// Pump this thread's messages until `done` says so: what a dialog's
    /// modal loop does while it is up.
    fn modal_loop(done: impl Fn() -> bool) {
        while !done() {
            let mut message = MSG::default();
            // SAFETY: `message` is a live out-parameter; the loop dispatches
            // only this thread's own messages.
            while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                // SAFETY: as above.
                unsafe {
                    let _ = TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// A dialog that stays up until it is dismissed, closing as the system's
    /// does: the close it installed ends its loop.
    fn dialog_until_dismissed(dismisser: &Dismisser) -> &'static str {
        let closed = std::rc::Rc::new(std::cell::Cell::new(false));
        let closing = std::rc::Rc::clone(&closed);
        dismisser.on_dismiss(move || closing.set(true));
        modal_loop(|| closed.get());
        "cancelled by its close"
    }

    #[test]
    fn a_dropped_hold_closes_the_dialog_on_its_thread_and_drops_its_answer() {
        let (shown, showing) = mpsc::channel::<()>();
        let (ended, ends) = mpsc::channel::<&'static str>();
        let (heard, hearing) = mpsc::channel::<&'static str>();
        let hold = show_on_thread(
            move |dismisser| {
                let _ = shown.send(());
                let result = dialog_until_dismissed(dismisser);
                let _ = ended.send(result);
                result
            },
            move |answer| {
                let _ = heard.send(answer);
            },
        )
        .expect("a thread starts");
        showing.recv().expect("the dialog is shown");
        drop(hold);
        assert_eq!(
            ends.recv_timeout(Duration::from_secs(10)),
            Ok("cancelled by its close"),
            "the dialog's own close ran on its thread"
        );
        assert!(
            hearing.recv_timeout(Duration::from_millis(200)).is_err(),
            "a dismissed dialog's answer reaches nobody"
        );
    }

    #[test]
    fn an_answered_dialog_is_heard_and_a_later_drop_does_nothing() {
        let (heard, hearing) = mpsc::channel::<u32>();
        let hold = show_on_thread(
            |_| 42_u32,
            move |answer| {
                let _ = heard.send(answer);
            },
        )
        .expect("a thread starts");
        assert_eq!(hearing.recv_timeout(Duration::from_secs(10)), Ok(42));
        drop(hold);
    }

    #[test]
    fn a_hold_dropped_before_the_dialog_shows_keeps_it_from_showing() {
        let shared = Shared {
            stage: Mutex::new(Stage::Dismissed),
        };
        run_dialog(
            &shared,
            |_| panic!("a dismissed dialog is not shown"),
            |()| panic!("a dismissed dialog answers nobody"),
        );
        assert_eq!(*shared.stage(), Stage::Dismissed);
    }

    #[test]
    fn a_hold_dropped_while_starting_marks_the_dialog_dismissed() {
        let shared = Arc::new(Shared {
            stage: Mutex::new(Stage::Starting),
        });
        drop(ModalHold {
            shared: Arc::clone(&shared),
        });
        assert_eq!(*shared.stage(), Stage::Dismissed);
    }
}
