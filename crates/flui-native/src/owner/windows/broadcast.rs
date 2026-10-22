//! A hidden top-level receiver; message-only HWNDs cannot receive broadcasts.
//!
//! The system broadcasts its setting, display, session, power and memory
//! messages to top-level windows. The preferences, the displays and the
//! lifecycle each hear part of them, so the receiver is the owner's and names
//! none of them: it tells the [`Subscriber`]s its host names, in order, until
//! one answers.

use std::{cell::Cell, rc::Rc};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
            GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW, UnregisterClassW, WINDOW_STYLE,
            WM_CLOSE, WM_NCCREATE, WM_NCDESTROY, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        },
    },
    core::{PCWSTR, w},
};

use crate::owner::panic_boundary::{AfterPanic, contain_owner_callback, os_entry};

/// What one feature does with the messages the receiver hears. Runs on the
/// owner thread, inside the receiver's window procedure.
pub(crate) trait Subscriber {
    /// The receiver's window exists: `receiver` is where the system sends
    /// what the subscriber hears.
    fn attach(&self, _receiver: HWND) {}

    /// The receiver's window is about to be destroyed.
    fn detach(&self, _receiver: HWND) {}

    /// What `message` is answered when a subscriber panics while the receiver
    /// dispatches it: the panic is contained and an answer still goes back to
    /// Windows. `None` leaves it to the subscribers after this one; with none
    /// it is 0.
    fn answer_after_panic(&self, _message: u32) -> Option<LRESULT> {
        None
    }

    /// Hears one message of the receiver's window; `Some` answers it, and no
    /// subscriber after this one hears it.
    fn message(&self, hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT>;
}

/// Sends a pointer-free message to the receiver window `receiver` and returns
/// its procedure's answer.
#[cfg(test)]
pub(crate) fn send_for_test(receiver: HWND, message: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: the message carries no pointer, and a test sends it only to a
    // receiver whose context its owner keeps alive.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            receiver,
            message,
            Some(WPARAM(wparam)),
            Some(LPARAM(lparam)),
        )
        .0
    }
}

struct Context {
    /// Who hears the receiver's messages, in order. Shared, so a message's
    /// dispatch holds them while a subscriber's user code may free this
    /// context.
    subscribers: Rc<[Rc<dyn Subscriber>]>,
    hwnd: Cell<Option<HWND>>,
}

/// The class registration is private to this receiver. Its allocation-based
/// name stays unique while the context lives, including failed destruction.
pub(crate) struct Receiver {
    context: Option<Box<Context>>,
    class: Vec<u16>,
    instance: HINSTANCE,
}

impl Receiver {
    /// Send a pointer-free message to this receiver and return the window
    /// procedure's result.
    #[cfg(test)]
    pub(crate) fn send_with_for_test(&self, message: u32, wparam: usize, lparam: isize) -> isize {
        send_for_test(self.hwnd_for_test(), message, wparam, lparam)
    }

    /// The receiver's window, while it lives.
    #[cfg(test)]
    pub(crate) fn hwnd_for_test(&self) -> HWND {
        self.context
            .as_ref()
            .and_then(|context| context.hwnd.get())
            .expect("live receiver")
    }

    /// A receiver whose messages `subscribers` hear, in this order.
    pub(crate) fn new(
        subscribers: impl IntoIterator<Item = Rc<dyn Subscriber>>,
    ) -> windows::core::Result<Self> {
        let context = Box::new(Context {
            subscribers: subscribers.into_iter().collect(),
            hwnd: Cell::new(None),
        });
        let class: Vec<_> = format!("FLUIBroadcasts_{:p}\0", &*context)
            .encode_utf16()
            .collect();
        // SAFETY: the executable module remains loaded for the process lifetime.
        let instance = unsafe { GetModuleHandleW(None)? }.into();
        let definition = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // SAFETY: definition/name are valid; procedure implements this class's ABI.
        if unsafe { RegisterClassW(&raw const definition) } == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let receiver = Self {
            context: Some(context),
            class,
            instance,
        };
        let context = receiver
            .context
            .as_deref()
            .expect("BUG: new receiver owns context");
        // SAFETY: the stable context allocation lives through creation and
        // synchronous destruction. No WS_VISIBLE, parent, or user-window entry.
        unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                PCWSTR(receiver.class.as_ptr()),
                w!("FLUI system broadcasts"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                Some(std::ptr::from_ref(context).cast()),
            )?;
        }
        if let Some(hwnd) = context.hwnd.get() {
            for subscriber in context.subscribers.iter() {
                subscriber.attach(hwnd);
            }
        }
        Ok(receiver)
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.retire(
            |hwnd| {
                // SAFETY: only this owner can destroy its live HWND. WM_NCDESTROY
                // clears the handle before reuse; context stays live through this call.
                unsafe { DestroyWindow(hwnd) }
            },
            |class, instance| {
                // SAFETY: `retire` asks only once no live HWND refers to this
                // private class.
                unsafe { UnregisterClassW(class, Some(instance)) }
            },
        );
    }
}

impl Receiver {
    #[cfg(test)]
    pub(crate) fn class_for_test(&self) -> (Vec<u16>, HINSTANCE) {
        (self.class.clone(), self.instance)
    }

    /// Retire with `destroy` and `unregister` standing in for `DestroyWindow`
    /// and `UnregisterClassW`.
    #[cfg(test)]
    pub(crate) fn retire_for_test(
        mut self,
        destroy: impl FnOnce(HWND) -> windows::core::Result<()>,
        unregister: impl FnOnce(PCWSTR, HINSTANCE) -> windows::core::Result<()>,
    ) {
        self.retire(destroy, unregister);
    }

    /// Detach the subscribers, destroy the window with `destroy`, then retire the
    /// class with `unregister`. A window the OS still owns keeps its context
    /// and unique class name reserved, so its procedure still finds a live,
    /// already-closed source.
    fn retire(
        &mut self,
        destroy: impl FnOnce(HWND) -> windows::core::Result<()>,
        unregister: impl FnOnce(PCWSTR, HINSTANCE) -> windows::core::Result<()>,
    ) {
        let Some(context) = self.context.take() else {
            return;
        };
        if let Some(hwnd) = context.hwnd.get() {
            for subscriber in context.subscribers.iter() {
                subscriber.detach(hwnd);
            }
            if let Err(error) = destroy(hwnd) {
                // Keep both userdata and its unique class name reserved if the
                // OS still owns the HWND. The already-closed source is inert.
                std::mem::forget(context);
                contain_owner_callback(
                    || tracing::error!(%error, "broadcast receiver retained after failed destruction"),
                );
                return;
            }
        }
        if let Err(error) = unregister(PCWSTR(self.class.as_ptr()), self.instance) {
            contain_owner_callback(
                || tracing::warn!(%error, "broadcast receiver class retirement failed"),
            );
        }
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    os_entry(
        "the broadcast receiver's procedure",
        AfterPanic::Answer(|| LRESULT(0)),
        || {
            // A subscriber's contained panic answers what the subscribers set below,
            // 0 otherwise.
            let result = Cell::new(LRESULT(0));
            contain_owner_callback(|| {
                // SAFETY: this class receives only its own boxed Context during native
                // creation. Only this owner reaches userdata. It is detached before
                // destruction returns, and no pointer borrow crosses a user callback.
                unsafe {
                    if message == WM_NCCREATE {
                        let create = &*(lparam.0 as *const CREATESTRUCTW);
                        let context = create.lpCreateParams.cast::<Context>();
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, context as isize);
                        (*context).hwnd.set(Some(hwnd));
                    }
                    let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Context;
                    if !context.is_null() {
                        if message == WM_NCDESTROY {
                            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                            (*context).hwnd.set(None);
                        } else if message == WM_CLOSE {
                            // This internal receiver is retired by the host, not by
                            // ordinary window-close routing or last-user-window policy.
                            return;
                        } else {
                            // Cloned out of the context: a subscriber may drop the
                            // platform, and nothing borrowed from userdata survives
                            // user code.
                            let subscribers = Rc::clone(&(*context).subscribers);
                            result.set(
                                subscribers
                                    .iter()
                                    .find_map(|subscriber| subscriber.answer_after_panic(message))
                                    .unwrap_or(LRESULT(0)),
                            );
                            for subscriber in subscribers.iter() {
                                if let Some(answer) =
                                    subscriber.message(hwnd, message, wparam, lparam)
                                {
                                    result.set(answer);
                                    return;
                                }
                            }
                        }
                    }
                    result.set(DefWindowProcW(hwnd, message, wparam, lparam));
                }
            });
            result.get()
        },
    )
}
