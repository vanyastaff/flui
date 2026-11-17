//! Work a window of the owner thread posts to itself, with one way back when
//! the thread's message queue refuses the post.

use crate::owner::signal::OwnerSignal;
use crate::owner::windows::WindowIdentity;
use crate::owner::windows::modal_loop::ModalWake;
use std::sync::{Arc, Weak};
use windows::Win32::{
    Foundation::{ERROR_NOT_ENOUGH_QUOTA, HWND, LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{GetWindowThreadProcessId, PostMessageW, SendMessageW},
};

/// The owner thread's message queue, as the windows that thread created post
/// their work to it from any thread: a frame of a window nothing paints, the
/// deliveries another thread raised, a close, the exit
/// policy, a system notice a pool thread heard.
///
/// The queue refuses a post once it holds the thread's posted-message quota
/// (`ERROR_NOT_ENOUGH_QUOTA`, ten thousand messages by default). Such a post
/// is not lost: the owner signal, whose own wake falls back on a kernel event
/// when its post is refused too, runs an owner turn that sends the message
/// instead, once the turn released its state
/// ([`OwnerSignal::run_outside_turn`]). An owner turn runs on the thread that
/// created the window, so the send calls the window procedure directly, past
/// the full queue, and the procedure answers it as it answers the posted one.
/// Every message names its window by identity ([`WindowIdentity::halves`]),
/// so a handle the system gave another window is left alone. Once the owner
/// stopped accepting work, neither way runs it.
///
/// A caller coalesces its own posts (one until the window takes it), so a
/// refused burst is one item for the owner's turn.
///
/// Nothing is logged where a post is asked for: the asker may hold borrows a
/// `tracing` subscriber takes again.
///
/// A post that went through knocks at the thread's modal loops
/// ([`ModalWake`]): a drag loop resting in OLE dispatches no posted message,
/// and its pacing dispatches them once knocked.
#[derive(Clone)]
pub(crate) struct OwnerQueue {
    signal: Weak<OwnerSignal>,
    /// The owner thread's Win32 id: only its windows are posted to.
    thread: u32,
    modal: ModalWake,
}

/// Why a post reached no window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PostRefusal {
    /// The handle names no window of the owner thread: the window closed, or
    /// the system gave its handle to a window of another thread.
    NotOwnerWindow,
    /// The queue was full and the owner no longer takes work.
    OwnerGone,
}

impl OwnerQueue {
    /// The queue of the owner thread behind `signal`, whose modal loops
    /// `modal` knocks at; made on that thread.
    pub(crate) fn new(signal: Weak<OwnerSignal>, modal: ModalWake) -> Self {
        Self {
            signal,
            // SAFETY: a query of the calling thread's id.
            thread: unsafe { GetCurrentThreadId() },
            modal,
        }
    }

    /// Work for the owner thread was handed over by another way than a post
    /// (an invalidation the system folds into a paint): knock at its modal
    /// loops.
    pub(crate) fn wake_modal(&self) {
        self.modal.wake();
    }

    /// Post `message`, naming `identity`, to `hwnd`, a window of the owner
    /// thread; when the queue is full, send it from the owner's next turn.
    ///
    /// # Errors
    ///
    /// [`PostRefusal::NotOwnerWindow`] when `hwnd` is no window of the owner
    /// thread, [`PostRefusal::OwnerGone`] when the queue is full and the owner
    /// has stopped.
    pub(crate) fn post(
        &self,
        hwnd: HWND,
        message: u32,
        identity: WindowIdentity,
    ) -> Result<(), PostRefusal> {
        if !owned_by(hwnd, self.thread) {
            return Err(PostRefusal::NotOwnerWindow);
        }
        let (wparam, lparam) = identity.halves();
        // SAFETY: `PostMessageW` takes a window handle and plain integers by
        // value, from any thread, and dereferences nothing; a handle that
        // closed meanwhile only makes it fail.
        let refusal =
            match unsafe { PostMessageW(Some(hwnd), message, WPARAM(wparam), LPARAM(lparam)) } {
                Ok(()) => {
                    self.modal.wake();
                    return Ok(());
                }
                Err(refusal) => refusal,
            };
        if refusal.code() != ERROR_NOT_ENOUGH_QUOTA.to_hresult() {
            return Err(PostRefusal::NotOwnerWindow);
        }
        let signal = self.signal.upgrade().ok_or(PostRefusal::OwnerGone)?;
        if !signal.accepting() {
            return Err(PostRefusal::OwnerGone);
        }
        // The handle as an integer: only sent to, never dereferenced.
        let (raw, thread) = (hwnd.0 as isize, self.thread);
        signal.run_outside_turn(Arc::new(move || {
            let hwnd = HWND(raw as *mut _);
            // A handle the system gave a window of another thread since would
            // make the send wait on that thread: it is left alone.
            if owned_by(hwnd, thread) {
                // SAFETY: the window is of this, the owner, thread, so the
                // send runs its procedure here, with plain integers and no
                // pointer; the parameters name the window.
                unsafe { SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(lparam))) };
            }
        }));
        Ok(())
    }
}

/// Whether `hwnd` is a live window of the thread `thread`.
fn owned_by(hwnd: HWND, thread: u32) -> bool {
    // SAFETY: a query of a window handle by value, with no out-pointer; a
    // handle that names no window answers 0.
    let owner = unsafe { GetWindowThreadProcessId(hwnd, None) };
    owner != 0 && owner == thread
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
