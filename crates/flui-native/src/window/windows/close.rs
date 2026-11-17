//! How a Win32 window ends: its retirement, which lets go of the window's
//! handler and the context in its `GWLP_USERDATA` slot, and what happens to
//! the native window after.
//!
//! A close the window granted (a `WM_CLOSE` its handler allowed, a posted
//! [`CLOSE_WINDOW`](super::proc::CLOSE_WINDOW), a `WindowCommand::Close` on
//! the owner thread, the last wrapper's drop) hides the window, retires it,
//! and settles the native window after the handler let go: destroyed, or kept
//! hidden for the life of the process when a keep was recorded for it
//! (`SurfaceTarget::keep_past_close`, ADR-0305 §9, which may come while the
//! window is still open), because a raster thread that did not answer may
//! still present over a swap chain made for the `HWND`. A `WM_DESTROY` the OS
//! sends on its own (the session ends, code that is not FLUI's destroys the
//! window) retires it while the OS destroys it, and nothing is kept.
//!
//! A retired window that is still alive carries the [`RETIRED`] property:
//! its slot is cleared like any retired window's, so its procedure reaches no
//! context and every hook already forgot it, it takes input for nobody, the
//! platform's window list no longer has it, and a `WM_CLOSE` that reaches it
//! leaves it to its retirement.

use std::rc::Rc;

use flui_foundation::panic::FirstPanic;
use windows::Win32::{
    Foundation::{HANDLE, HWND},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        DestroyWindow, GWLP_USERDATA, GetPropW, RemovePropW, SW_HIDE, SetPropW, SetWindowLongPtrW,
        ShowWindow,
    },
};
use windows::core::{PCWSTR, w};

use super::proc::WindowContext;
use crate::owner::hwnd_affinity::HandleNames;
use crate::owner::window_delivery::NativeFate;
use crate::owner::windows::{WindowRef, WindowStep, window_destroyed};

/// The window property a granted close puts on the window it retires, until
/// the window is destroyed: what tells a retired FLUI window from one whose
/// context was never installed, as both have a cleared slot.
const RETIRED: PCWSTR = w!("flui.window.retired");

/// How a window's retirement ([`WindowContext::retire`]) ends its native
/// window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeEnd {
    /// The OS is destroying it: a `WM_DESTROY` no close of the window's own
    /// started.
    Destroying,
    /// The window granted its close: it is hidden first, and destroyed or
    /// kept once its handler let go.
    Granted,
}

/// Whether `hwnd` is a window a granted close retired and has not destroyed:
/// kept past its close, or waiting for its handler's release.
pub(crate) fn is_retired(hwnd: HWND) -> bool {
    // SAFETY: a property read of a window handle, with a string the call
    // reads for its duration; a handle that names no window answers null.
    !unsafe { GetPropW(hwnd, RETIRED) }.is_invalid()
}

impl WindowContext {
    /// Close this window, `hwnd`, without asking its handler: retire it
    /// ([`Self::retire`]), then destroy the native window, or keep it hidden
    /// when a keep was recorded for it.
    ///
    /// # Safety
    ///
    /// `hwnd` is the window whose `GWLP_USERDATA` slot holds this context, and
    /// the call runs on its owner thread, under a borrow of this context the
    /// window procedure or a context gate holds.
    pub(crate) unsafe fn close_granted(&self, hwnd: HWND) {
        // SAFETY: this function's contract is `retire`'s.
        unsafe { self.retire(hwnd, NativeEnd::Granted) };
    }

    /// Retire this window, `hwnd`: what the input, the drag and the session
    /// hold of it ends, its handler hears `closed` and is let go of, its slot
    /// is cleared so no new borrow of this context is minted, and it leaves
    /// the platform's window list. `end` says what becomes of the native
    /// window.
    ///
    /// A granted close hides the window first, with its context still
    /// installed, as `DestroyWindow` hides a window before its `WM_DESTROY`:
    /// the activation and focus loss the hide sends reach the handler before
    /// `closed`. The handler is let go of last (`WindowDelivery::clear_then`),
    /// after the slot is cleared: the native window is destroyed in the
    /// release's completion, and the `WM_DESTROY` that sends finds no
    /// context. Asked from inside one of the handler's own methods, the
    /// release, and the destruction with it, waits until that method returns.
    ///
    /// Every step runs whichever user code before it panics (a hook,
    /// `closed`, a tracing subscriber), so a retirement once begun always
    /// finishes; the first panic resumes at the end. The messages the hide
    /// sends are handled in a nested window procedure, whose OS boundary
    /// aborts the process on a panic (`AfterPanic::Abort`): no panic of
    /// theirs reaches this function.
    ///
    /// # Safety
    ///
    /// As [`Self::close_granted`].
    pub(crate) unsafe fn retire(&self, hwnd: HWND, end: NativeEnd) {
        // The callbacks below may close the window again (`closed` closing
        // its own window, the OS destroying it under a hook): the retirement
        // under way finishes it.
        if self.retiring.replace(true) {
            return;
        }
        let mut failure = FirstPanic::new();
        // The cursor clip is the desktop's: it must not outlive the window,
        // whether the window is destroyed now or hidden past a wedged raster
        // thread; nor does a retired window stay kept above or below.
        failure.contain(|| self.let_pointer_go(hwnd));
        self.kept_below.set(false);
        if end == NativeEnd::Granted {
            // The visible effect waits for nothing, a handler that is slow
            // to answer `closed` included.
            // SAFETY: `hwnd` is a live window of this thread (this function's
            // contract); no pointer is passed.
            let _was_visible = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
        // A modal loop the window ran ends with it, though its end message
        // never comes: its timer stops and the thread's code no longer asks
        // it again.
        failure.contain(|| self.modal_pacing.abandon());
        // While the window and its callbacks still exist: what the input
        // holds of the window ends as cancelled and its touchpad viewport is
        // released, a drag over it leaves it and OLE no longer offers it one,
        // and the session no longer reports to it.
        let hooks = Rc::clone(&self.hooks);
        failure.contain(|| window_destroyed(&hooks, WindowRef::new(hwnd, self.identity)));

        failure.contain(|| self.delivery.dispatch_close());

        let retirement = self.delivery.retirement();
        if end == NativeEnd::Destroying {
            // The window is going whatever was asked for it.
            let _gone = retirement.settle();
        }

        // The hooks hear that the close callbacks ran, while the HWND is
        // still valid and before the context retires: the text input
        // deactivates TSF for the window now that the presentation is closed
        // and its field unfocused (ADR-0135 §3).
        failure.contain(|| self.step(hwnd, WindowStep::Closed));

        if end == NativeEnd::Granted {
            let flag = HANDLE(std::ptr::dangling_mut());
            // SAFETY: `hwnd` is a live window of this thread; `RETIRED` is a
            // string the call reads for its duration, and the data is a flag,
            // never dereferenced.
            let marked = unsafe { SetPropW(hwnd, RETIRED, Some(flag)) };
            if let Err(error) = marked {
                // A subscriber is user code.
                failure.contain(|| {
                    tracing::warn!(handle = ?hwnd, %error, "a retired window could not be marked retired");
                });
            }
        }

        // Retire the context: clear the slot FIRST, so no new borrow can be
        // minted (`with_window_context` and the window procedure's entry
        // both refuse a null slot), then mark the ledger. The allocation is
        // NOT freed here: the caller's borrow still holds it, and so may
        // outer frames when this runs reentrantly (a framework callback
        // closing the window from inside another message's dispatch). The
        // free happens exactly once, at the outermost `ContextGuard` release.
        // The window procedure never runs concurrently with itself for the
        // same `hwnd` (Win32 dispatches on the one owning thread), so slot
        // clear and retire need no further ordering.
        // SAFETY: `hwnd` is this context's window on its owner thread (this
        // function's contract); the call writes an integer into its slot.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        self.ledger.borrow_mut().retire();

        // Leave the tracked windows only after the slot: the removed `Arc`
        // may be the last wrapper, whose drop must find the slot cleared and
        // so cannot destroy this window again.
        let last_window_closed = failure
            .contain(|| {
                let removed = self
                    .windows
                    .upgrade()
                    .and_then(|windows| windows.remove(hwnd, self.identity));
                removed.as_ref().is_some_and(|(_, empty)| *empty)
            })
            .unwrap_or(false);
        // The last tracked window is gone: the owner decides, on its own
        // turn, whether the loop ends. Not here: this may run nested inside
        // embedder code that closed the window, and the hook re-enters the
        // embedder.
        if last_window_closed {
            self.exit_policy.post();
        }

        // Let go of the handler while the HWND is still valid: in
        // `flui-app` it owns what the window keeps, and the surface made
        // over the window's surface target goes before the window it draws
        // into. The destruction trace rides on the release, ordered after it
        // however the close arrived.
        let window_id = self.window_id.get();
        let raw = hwnd.0 as isize;
        failure.contain(|| {
            self.delivery.clear_then(move || {
                let fate = match end {
                    NativeEnd::Destroying => NativeFate::Destroy,
                    NativeEnd::Granted => {
                        let fate = retirement.settle();
                        end_granted(HWND(raw as *mut std::ffi::c_void), fate);
                        fate
                    }
                };
                match fate {
                    NativeFate::Destroy => tracing::debug!(
                        target: "flui.platform",
                        event = "native_window_destroyed",
                        window_id,
                    ),
                    NativeFate::Keep => tracing::debug!(
                        target: "flui.platform",
                        event = "native_window_kept",
                        window_id,
                    ),
                }
            });
        });
        failure.resume();
    }
}

/// Destroy, or keep hidden, the native window a granted close retired, on
/// the owner thread, once its handler let go.
fn end_granted(hwnd: HWND, fate: NativeFate) {
    // The release may have waited behind a running method, in which the OS
    // could destroy the window and give its handle to another: only a live
    // FLUI window of this thread whose slot is cleared, the retired one, is
    // touched: the retired window is the one whose cleared slot (context
    // `0`) names it, on the thread that runs this.
    // SAFETY: no arguments; the calling thread's id.
    let this_thread = unsafe { GetCurrentThreadId() };
    let retired = super::proc::InstalledWindow {
        thread: this_thread,
        context: 0,
    };
    if super::proc::handle_names(hwnd, retired) != HandleNames::ItsWindow {
        return;
    }
    match fate {
        NativeFate::Destroy => {
            // SAFETY: `handle_names` just established a live window of this thread
            // (the only one Win32 permits the call from) whose slot is
            // cleared, so its `WM_DESTROY` finds no context. The mark goes
            // before the window, as Win32 asks of a property it was given;
            // `RETIRED` is read for the call's duration.
            let destroyed = unsafe {
                let _mark = RemovePropW(hwnd, RETIRED);
                DestroyWindow(hwnd)
            };
            if let Err(error) = destroyed {
                tracing::warn!(handle = ?hwnd, ?error, "DestroyWindow failed on a granted close");
            }
        }
        NativeFate::Keep => {
            // Hidden since the close began; hidden again in case a show
            // reached it meanwhile. The window, and the mark that leaves it
            // to its retirement, live until the process exits.
            // SAFETY: as above; no pointer is passed.
            let _was_visible = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
    }
}
