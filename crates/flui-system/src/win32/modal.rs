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

use std::cell::{Cell, RefCell};
use std::num::NonZeroIsize;
use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::WindowId;
use flui_platform::capability::{Capability, Resolver};
use flui_platform::{Unsupported, UnsupportedReason};
use rustc_hash::FxHashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HWND_MESSAGE,
    PostMessageW, RegisterClassExW, SetWindowLongPtrW, WINDOW_EX_STYLE, WM_APP, WNDCLASSEXW,
    WS_OVERLAPPED,
};
use windows::core::w;

use super::threads::{Apartment, OnceAnswer};
use crate::dialog::{DialogError, DialogHold};
use crate::owner::panic_boundary::{AfterPanic, os_entry};

/// The message the dismissal window takes as "dismiss the dialog".
const DISMISS: u32 = WM_APP + 0x51;

/// The dialog threads of one UI runtime's windows, by window: a window
/// holds one dialog thread at a time, until that thread has left its
/// dialog. The handle frees a window as soon as its dialog is dismissed,
/// while the dialog closes on its own thread a little later and only then
/// re-enables its owner; a second dialog shown before that would be left
/// under a re-enabled window. Owner-local.
#[derive(Default)]
pub(crate) struct ModalWindows(RefCell<FxHashMap<WindowId, Arc<Shared>>>);

impl ModalWindows {
    /// Whether `window` has a dialog thread that has not left its dialog.
    fn busy(&self, window: WindowId) -> bool {
        self.0
            .borrow()
            .get(&window)
            .is_some_and(|shared| *shared.stage() != Stage::Ended)
    }

    fn hold(&self, window: WindowId, shared: Arc<Shared>) {
        let replaced = self.0.borrow_mut().insert(window, shared);
        drop(replaced);
    }
}

/// Show a dialog of capability `C`, modal to `window`, on its thread with
/// `show`, and answer `answer` with what `meaning` makes of the system's
/// answer.
///
/// A window whose previous dialog thread has not left its dialog answers
/// [`DialogError::Busy`]; a thread that cannot start, or cannot make the
/// window its dialog is dismissed through, answers `Unavailable` without
/// showing anything; a dismissed dialog answers nobody, since the handle
/// has already ended it.
pub(super) fn show_answering<C, A, T>(
    windows: &ModalWindows,
    window: WindowId,
    answer: Resolver<Result<T, DialogError>>,
    show: impl FnOnce(&Dismisser) -> A + Send + 'static,
    meaning: impl FnOnce(A) -> Result<T, DialogError> + Send + 'static,
) -> DialogHold
where
    C: Capability,
    A: 'static,
    T: Send + 'static,
{
    let unavailable = || {
        Err(DialogError::Unsupported(Unsupported::of::<C>(
            UnsupportedReason::Unavailable,
        )))
    };
    if windows.busy(window) {
        let _ = answer.resolve(Err(DialogError::Busy));
        return DialogHold::new(());
    }
    let (thread, here) = OnceAnswer::pair(answer);
    let shown = show_on_thread(show, move |outcome| match outcome {
        Outcome::Answered(system) => thread.resolve(|| meaning(system)),
        Outcome::Unshown => thread.resolve(unavailable),
    });
    let Ok(hold) = shown else {
        here.resolve(unavailable);
        return DialogHold::new(());
    };
    windows.hold(window, Arc::clone(&hold.shared));
    DialogHold::new(hold)
}

/// What a dialog's thread ended with, when someone still waits for it.
pub(super) enum Outcome<R> {
    /// The dialog was shown and answered `R`.
    Answered(R),
    /// The thread could not make its dismissal window, so it showed
    /// nothing: a dialog that cannot be dismissed is not shown.
    Unshown,
}

/// What a dialog's thread is doing, as the hold and the thread see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// The thread has not shown the dialog yet.
    Starting,
    /// The dialog is up; its dismissal window is this handle's value.
    Showing(NonZeroIsize),
    /// The hold dropped before the thread left its dialog: whatever the
    /// dialog answers is dropped.
    Dismissed,
    /// The thread has left its dialog, or never showed one: on every exit,
    /// a panic in the dialog's code included, and always before its
    /// dismissal window is destroyed.
    Ended,
}

/// What the hold and the dialog's thread share.
pub(crate) struct Shared {
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
                // SAFETY: `window` is the dismissal window of a thread that
                // was in its dialog under the lock this drop just released:
                // the thread moves the stage to `Ended` under the same lock
                // before it destroys the window, on every exit, so the window
                // was alive when the stage was read. One destroyed since makes
                // the post fail, and a handle Windows reused names a window of
                // another thread only after that destruction, which the stage
                // read rules out; nothing but the message is passed.
                let posted = unsafe {
                    PostMessageW(
                        Some(HWND(window.get() as *mut core::ffi::c_void)),
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
            Stage::Dismissed | Stage::Ended => {}
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
/// close on the [`Dismisser`], and returns when it ends; `answer` hears how
/// it ended unless the hold was dropped first.
///
/// # Errors
///
/// The thread could not be started; `show` and `answer` are dropped unrun.
pub(super) fn show_on_thread<R: 'static>(
    show: impl FnOnce(&Dismisser) -> R + Send + 'static,
    answer: impl FnOnce(Outcome<R>) + Send + 'static,
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

/// Moves the stage to `Ended` when dropped: on every exit of the thread's
/// dialog, before its dismissal window is destroyed.
struct EndOnExit<'a>(&'a Shared);

impl EndOnExit<'_> {
    /// End now; whether the hold was dropped while the dialog was up.
    fn end(self) -> bool {
        let mut stage = self.0.stage();
        let dismissed = *stage == Stage::Dismissed;
        *stage = Stage::Ended;
        dismissed
    }
}

impl Drop for EndOnExit<'_> {
    fn drop(&mut self) {
        *self.0.stage() = Stage::Ended;
    }
}

fn run_dialog<R>(
    shared: &Shared,
    show: impl FnOnce(&Dismisser) -> R,
    answer: impl FnOnce(Outcome<R>),
) {
    let _apartment = Apartment::enter();
    let dismisser = Dismisser::default();
    let window = DismissalWindow::create(&dismisser);
    // Declared after the window, so dropped before it: the stage leaves
    // `Showing` before the window is destroyed, a panic in `show` included.
    let ending = EndOnExit(shared);
    {
        let mut stage = shared.stage();
        if *stage == Stage::Dismissed {
            return;
        }
        let Some(window) = &window else {
            drop(stage);
            drop(ending);
            answer(Outcome::Unshown);
            return;
        };
        *stage = Stage::Showing(window.value());
    }
    let result = show(&dismisser);
    if !ending.end() {
        answer(Outcome::Answered(result));
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

    fn value(&self) -> NonZeroIsize {
        NonZeroIsize::new(self.0.0 as isize)
            .expect("BUG: a window `CreateWindowExW` made is not null")
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
/// system and nothing of the application's; a panic in it is contained,
/// and the message answered 0.
unsafe extern "system" fn dismissal_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    os_entry(
        "the dialog's dismissal window",
        (window, message, wparam, lparam),
        AfterPanic::Answer(|_| LRESULT(0)),
        || dismissal_message(window, message, wparam, lparam),
    )
}

fn dismissal_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };

    use super::{Dismisser, ModalHold, Outcome, Shared, Stage, run_dialog, show_on_thread};

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

    fn starting() -> Arc<Shared> {
        Arc::new(Shared {
            stage: Mutex::new(Stage::Starting),
        })
    }

    #[test]
    fn a_dropped_hold_closes_the_dialog_on_its_thread_and_drops_its_answer() {
        let (shown, showing) = mpsc::channel::<()>();
        let (ended, ends) = mpsc::channel::<&'static str>();
        let (heard, hearing) = mpsc::channel::<()>();
        let hold = show_on_thread(
            move |dismisser| {
                let _ = shown.send(());
                let result = dialog_until_dismissed(dismisser);
                let _ = ended.send(result);
                result
            },
            move |_| {
                let _ = heard.send(());
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
        let (heard, hearing) = mpsc::channel::<Option<u32>>();
        let hold = show_on_thread(
            |_| 42_u32,
            move |outcome| {
                let _ = heard.send(match outcome {
                    Outcome::Answered(answer) => Some(answer),
                    Outcome::Unshown => None,
                });
            },
        )
        .expect("a thread starts");
        assert_eq!(hearing.recv_timeout(Duration::from_secs(10)), Ok(Some(42)));
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
            |_: Outcome<()>| panic!("a dismissed dialog answers nobody"),
        );
        assert_eq!(
            *shared.stage(),
            Stage::Ended,
            "the thread left: the window is free for its next dialog"
        );
    }

    #[test]
    fn a_dialog_whose_code_panics_ends_before_its_window_goes_and_a_later_drop_posts_nothing() {
        let shared = starting();
        let thread = Arc::clone(&shared);
        let unwound = std::thread::spawn(move || {
            run_dialog(
                &thread,
                |_| -> () { panic!("the dialog's code fails") },
                |_| {},
            );
        })
        .join();
        assert!(unwound.is_err(), "the dialog's code panicked");
        assert_eq!(
            *shared.stage(),
            Stage::Ended,
            "the stage left `Showing` before the dismissal window was destroyed"
        );
        drop(ModalHold {
            shared: Arc::clone(&shared),
        });
        assert_eq!(
            *shared.stage(),
            Stage::Ended,
            "nothing is posted to the gone window"
        );
    }
}
