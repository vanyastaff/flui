//! Work a window of the owner thread posts to itself, with one way back when
//! the thread's message queue refuses the post.

use crate::owner::signal::OwnerSignal;
use std::sync::{Arc, Weak};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, SendMessageW},
};

/// The owner thread's message queue, as the windows that thread created post
/// their work to it from any thread: a frame of a window nothing paints, the
/// deliveries another thread raised, a close the handler granted, the exit
/// policy, a system notice a pool thread heard.
///
/// The queue refuses a post once it holds the thread's posted-message quota
/// (`ERROR_NOT_ENOUGH_QUOTA`, ten thousand messages by default). Such a post
/// is not lost: the owner signal, whose own wake falls back on a kernel event
/// when its post is refused too, runs an owner turn that sends the message
/// instead. An owner turn runs on the thread that created the window, so the
/// send calls the window procedure directly, past the full queue, and the
/// procedure answers it as it answers the posted one. A message therefore
/// names what it acts on in its parameters (the window's identity), never by
/// pointer. Once the owner stopped accepting work, neither way runs it.
///
/// Nothing is logged where a post is asked for: the asker may hold borrows a
/// `tracing` subscriber takes again.
#[derive(Clone)]
pub(crate) struct OwnerQueue {
    signal: Weak<OwnerSignal>,
}

impl OwnerQueue {
    /// The queue of the owner thread behind `signal`.
    pub(crate) fn new(signal: Weak<OwnerSignal>) -> Self {
        Self { signal }
    }

    /// Post `message` with `wparam` and `lparam` to `hwnd`, a window the owner
    /// thread created; refused, send it from the owner's next turn.
    pub(crate) fn post(&self, hwnd: HWND, message: u32, wparam: usize, lparam: isize) {
        // SAFETY: `PostMessageW` takes a window handle and plain integers by
        // value, from any thread, and dereferences nothing; a handle that
        // closed meanwhile only makes it fail.
        if unsafe { PostMessageW(Some(hwnd), message, WPARAM(wparam), LPARAM(lparam)) }.is_ok() {
            return;
        }
        let Some(signal) = self.signal.upgrade() else {
            return;
        };
        // The handle as an integer: only sent to, never dereferenced.
        let raw = hwnd.0 as isize;
        signal.run_for_window(Arc::new(move || {
            // SAFETY: an owner turn runs on the thread that created the
            // window, so the send runs its procedure on that thread, with
            // plain integers and no pointer. A handle that closed since makes
            // the call fail, and the parameters name the window, so a handle
            // the system gave another window is left alone by the procedure.
            unsafe {
                SendMessageW(
                    HWND(raw as *mut _),
                    message,
                    Some(WPARAM(wparam)),
                    Some(LPARAM(lparam)),
                )
            };
        }));
    }
}

/// The owner thread's queue as a test fills it.
#[cfg(test)]
pub(crate) mod testing {
    use windows::Win32::{
        Foundation::{ERROR_NOT_ENOUGH_QUOTA, HWND, LPARAM, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW, PostThreadMessageW, WM_NULL},
    };

    /// This thread's posted-message queue, filled to its quota: while it
    /// lives, every post to a window of the thread is refused, as the system
    /// refuses it to a thread that fell behind. Dropped, it takes back the
    /// messages it posted.
    pub(crate) struct FullQueue(());

    impl FullQueue {
        pub(crate) fn fill() -> Self {
            // SAFETY: a query of the calling thread's id.
            let thread = unsafe { GetCurrentThreadId() };
            let mut posted = 0_usize;
            let refusal = loop {
                // SAFETY: a pointer-free thread message to this thread, whose
                // queue exists: the caller created windows on it.
                match unsafe { PostThreadMessageW(thread, WM_NULL, WPARAM(0), LPARAM(0)) } {
                    Ok(()) => posted += 1,
                    Err(error) => break error,
                }
                assert!(
                    posted < 1_000_000,
                    "the queue took a million posts and refused none"
                );
            };
            assert_eq!(
                refusal.code(),
                ERROR_NOT_ENOUGH_QUOTA.to_hresult(),
                "the queue refused a post for another reason"
            );
            Self(())
        }
    }

    impl Drop for FullQueue {
        fn drop(&mut self) {
            let mut message = MSG::default();
            // SAFETY: `message` is a live local the call writes; a handle of
            // -1 takes only this thread's own thread messages, the ones
            // `fill` posted.
            while unsafe {
                PeekMessageW(
                    &raw mut message,
                    Some(HWND(-1_isize as *mut _)),
                    WM_NULL,
                    WM_NULL,
                    PM_REMOVE,
                )
            }
            .as_bool()
            {}
        }
    }
}
