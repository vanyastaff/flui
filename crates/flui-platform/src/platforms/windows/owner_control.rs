//! Dedicated message-only HWND: its userdata is never a visible WindowContext.
//!
//! Its `GWLP_USERDATA` slot holds the [`OwnerControlContext`]: the platform's
//! owner-thread state. Only the owner thread can reach it (through
//! [`OwnerControl::shares`], or the window procedure), and it is freed by
//! `WM_NCDESTROY` on that thread, so what it owns is never run or dropped
//! anywhere else.
use crate::{
    PlatformError,
    shared::{
        PlatformHandlers,
        hwnd_affinity::{UserDataRefusal, UserDataVerdict, classify_user_data_access},
        owner_signal::OwnerSignal,
        panic_boundary::contain_owner_callback,
    },
};
use parking_lot::Mutex;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, OnceLock},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClassNameW,
            GetWindowLongPtrW, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW,
            PostQuitMessage, RegisterClassW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE,
            WM_APP, WM_NCDESTROY, WNDCLASSW,
        },
    },
    core::{PCWSTR, w},
};
const WAKE: u32 = WM_APP + 19;
const OWNER_CLASS: PCWSTR = w!("FLUIOwnerSignalWindow");
static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

/// The platform's owner-thread state, boxed into the owner window's
/// `GWLP_USERDATA` slot. `!Send`: it is created, used and freed on the owner
/// thread only.
pub(super) struct OwnerControlContext {
    signal: Arc<OwnerSignal>,
    handlers: Rc<RefCell<PlatformHandlers>>,
}

/// Owner-thread handles cloned out of the [`OwnerControlContext`]. Holding
/// clones instead of a borrow means no reference into the context survives
/// user code, which may tear the owner window down.
pub(super) struct OwnerShares {
    /// The platform-level handlers (`on_quit`, `on_window_event`, ...),
    /// shared with every window context this platform opens.
    pub(super) handlers: Rc<RefCell<PlatformHandlers>>,
}

pub(super) struct OwnerControl {
    pub(super) signal: Arc<OwnerSignal>,
    address: Arc<Mutex<Option<isize>>>,
}
impl OwnerControl {
    pub(super) fn new() -> Result<Self, PlatformError> {
        let registration = REGISTERED.get_or_init(|| {
            // SAFETY: class name lives forever; this exact procedure implements its ABI.
            unsafe {
                let instance = GetModuleHandleW(None).map_err(|error| error.to_string())?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance.into(),
                    lpszClassName: OWNER_CLASS,
                    ..Default::default()
                };
                if RegisterClassW(&raw const class) == 0 {
                    Err(windows::core::Error::from_thread().to_string())
                } else {
                    Ok(())
                }
            }
        });
        registration
            .as_ref()
            .map_err(|message| PlatformError::Init {
                message: message.clone(),
            })?;
        let address = Arc::new(Mutex::new(None::<isize>));
        let target = Arc::clone(&address);
        let signal = OwnerSignal::new(Arc::new(move || {
            let address = target.lock();
            let raw = address.ok_or_else(|| PlatformError::EventLoop {
                message: "owner message window is closed".into(),
            })?;
            // SAFETY: admission is closed before this address is retired. Posting
            // carries no pointer or user closure and never synchronously invokes the procedure.
            unsafe { PostMessageW(Some(HWND(raw as *mut _)), WAKE, WPARAM(0), LPARAM(0)) }.map_err(
                |error| PlatformError::EventLoop {
                    message: error.to_string(),
                },
            )
        }));
        let context = Box::new(OwnerControlContext {
            signal: Arc::clone(&signal),
            handlers: Rc::new(RefCell::new(PlatformHandlers::default())),
        });
        // SAFETY: dedicated registered class, created on this (the owner) thread;
        // userdata is the boxed context, reclaimed exactly once by WM_NCDESTROY.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).map_err(|error| PlatformError::Init {
                message: error.to_string(),
            })?;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                OWNER_CLASS,
                w!("FLUI owner"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|error| PlatformError::Init {
                message: error.to_string(),
            })?;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(context) as isize);
            hwnd
        };
        {
            let mut address_slot = address.lock();
            *address_slot = Some(hwnd.0 as isize);
        }
        Ok(Self { signal, address })
    }

    /// Clones the owner-thread handles out of the owner context, or says why
    /// this thread may not have them: [`UserDataRefusal::ForeignThread`] off
    /// the owner, [`UserDataRefusal::WindowGone`] or
    /// [`UserDataRefusal::EmptySlot`] once the platform has shut down.
    pub(super) fn shares(&self, op: &'static str) -> Result<OwnerShares, UserDataRefusal> {
        let raw = *self.address.lock();
        let Some(raw) = raw else {
            tracing::debug!(op, "refusing owner state: the owner window is closed");
            return Err(UserDataRefusal::WindowGone);
        };
        let hwnd = HWND(raw as *mut _);
        // SAFETY: thread and slot queries take the handle by value and
        // dereference nothing; none of them pumps this thread's queue.
        let (owner_thread, current_thread, slot) = unsafe {
            (
                GetWindowThreadProcessId(hwnd, None),
                GetCurrentThreadId(),
                GetWindowLongPtrW(hwnd, GWLP_USERDATA),
            )
        };
        match classify_user_data_access(owner_thread, current_thread, names_owner_class(hwnd), slot)
        {
            UserDataVerdict::Deref => {
                // SAFETY: the verdict established that this is the owner
                // thread and the live window is of the owner class, whose
                // non-null userdata is the `Box<OwnerControlContext>` `new`
                // installed. Only `WM_NCDESTROY`, dispatched on this same
                // thread, frees it, and nothing between the slot read and
                // these clones dispatches a message; no reference outlives
                // this block.
                let context = unsafe { &*(slot as *const OwnerControlContext) };
                Ok(OwnerShares {
                    handlers: Rc::clone(&context.handlers),
                })
            }
            UserDataVerdict::Refuse(reason) => {
                tracing::debug!(op, ?reason, "refusing owner state");
                Err(reason)
            }
        }
    }

    pub(super) fn close(&self) {
        self.signal.close();
        let address = self.address.lock().take();
        if let Some(address) = address {
            // SAFETY: native destruction occurs on the recorded owner, outside
            // the address/state locks. WM_NCDESTROY owns userdata reclamation.
            if self.signal.owner() == std::thread::current().id() {
                if let Err(error) = unsafe { DestroyWindow(HWND(address as *mut _)) } {
                    tracing::error!(%error, "owner message window destruction failed");
                }
            } else {
                tracing::error!("owner message window dropped off owner; native resource retained");
            }
        }
    }
}
impl Drop for OwnerControl {
    fn drop(&mut self) {
        self.close();
    }
}

/// Whether the live window behind `hwnd` is of the owner class, the only
/// class whose userdata is an [`OwnerControlContext`].
fn names_owner_class(hwnd: HWND) -> bool {
    let mut name = [0_u16; 256];
    // SAFETY: `GetClassNameW` writes at most `name.len()` units into the
    // live stack buffer and returns the count excluding the terminator (0
    // for a dead handle); `OWNER_CLASS` is a NUL-terminated static literal
    // measured without its terminator, the convention `class_name_matches`
    // expects.
    let (copied, expected) = unsafe { (GetClassNameW(hwnd, &mut name), OWNER_CLASS.as_wide()) };
    crate::shared::hwnd_affinity::class_name_matches(&name, copied, expected)
}

// SAFETY: Win32 invokes this exact signature for the dedicated registered class.
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let mut result = LRESULT(0);
    contain_owner_callback(|| {
        // SAFETY: only this class stores `OwnerControlContext` userdata, and
        // Win32 runs this procedure on the owner thread, the only thread
        // that frees it (`WM_NCDESTROY`, below). A wake turn clones the
        // signal out before running callbacks, so a callback that tears the
        // window down cannot free what the turn still uses.
        unsafe {
            let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OwnerControlContext;
            if message == WM_NCDESTROY && !pointer.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(pointer));
            } else if message == WAKE && !pointer.is_null() {
                let signal = Arc::clone(&(*pointer).signal);
                if signal.drive() {
                    signal.close();
                    PostQuitMessage(0);
                }
                return;
            }
            result = DefWindowProcW(hwnd, message, wparam, lparam);
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use super::OwnerControlContext;

    // The owner context holds owner-only state; it must never become
    // `Send` or `Sync` through a field change.
    static_assertions::assert_not_impl_any!(OwnerControlContext: Send, Sync);
}
