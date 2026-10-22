//! A hidden top-level receiver; message-only HWNDs cannot receive broadcasts.
//!
//! The system broadcasts its setting, display, session, power and memory
//! messages to top-level windows. The preferences, the displays and the
//! lifecycle each hear part of them, so the receiver is the owner's and names
//! none of them: it tells its [`Subscriber`]s, in order, until one answers.

use std::{cell::Cell, rc::Rc, sync::Arc};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
            GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW, UnregisterClassW, WINDOW_STYLE,
            WM_CLOSE, WM_NCCREATE, WM_NCDESTROY, WM_QUERYENDSESSION, WNDCLASSW, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
        },
    },
    core::{PCWSTR, w},
};

use crate::owner::panic_boundary::contain_owner_callback;

/// What one feature does with the messages the receiver hears. Runs on the
/// owner thread, inside the receiver's window procedure.
pub(crate) trait Subscriber {
    /// The receiver's window exists: `receiver` is where the system sends
    /// what the subscriber hears.
    fn attach(&self, _receiver: HWND) {}

    /// The receiver's window is about to be destroyed.
    fn detach(&self, _receiver: HWND) {}

    /// Hears one message of the receiver's window; `Some` answers it, and no
    /// subscriber after this one hears it.
    fn message(&self, hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT>;
}

impl<S: Subscriber + ?Sized> Subscriber for Arc<S> {
    fn attach(&self, receiver: HWND) {
        S::attach(self, receiver);
    }

    fn detach(&self, receiver: HWND) {
        S::detach(self, receiver);
    }

    fn message(&self, hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        S::message(self, hwnd, message, wparam, lparam)
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
    #[cfg(test)]
    pub(crate) fn send_for_test(&self, message: u32, wparam: usize) {
        self.send_with_for_test(message, wparam, 0);
    }

    /// Send a pointer-free message to this receiver and return the window
    /// procedure's result.
    #[cfg(test)]
    pub(crate) fn send_with_for_test(&self, message: u32, wparam: usize, lparam: isize) -> isize {
        // SAFETY: this test sends a pointer-free notification only to this
        // receiver, whose context is retained by the borrowed owner.
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                self.hwnd_for_test(),
                message,
                Some(WPARAM(wparam)),
                Some(LPARAM(lparam)),
            )
            .0
        }
    }

    #[cfg(test)]
    pub(crate) fn hwnd_for_test(&self) -> HWND {
        self.context
            .as_ref()
            .and_then(|context| context.hwnd.get())
            .expect("live receiver")
    }

    #[cfg(test)]
    pub(crate) fn send_setting_change(&self) {
        let hwnd = self
            .context
            .as_ref()
            .and_then(|context| context.hwnd.get())
            .expect("live receiver");
        // SAFETY: this test sends a pointer-free generic settings notification
        // only to this receiver, whose context is retained by the borrowed owner.
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE,
                Some(WPARAM(0)),
                Some(LPARAM(0)),
            );
        }
    }

    /// A receiver whose messages `first` hears, then `then` when there is one.
    pub(crate) fn new(
        first: impl Subscriber + 'static,
        then: Option<Rc<dyn Subscriber>>,
    ) -> windows::core::Result<Self> {
        let first: Rc<dyn Subscriber> = Rc::new(first);
        let context = Box::new(Context {
            subscribers: std::iter::once(first).chain(then).collect(),
            hwnd: Cell::new(None),
        });
        let class: Vec<_> = format!("FLUIPreferences_{:p}\0", &*context)
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
                w!("FLUI system preferences"),
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
                    || tracing::error!(%error, "preference receiver retained after failed destruction"),
                );
                return;
            }
        }
        if let Err(error) = unregister(PCWSTR(self.class.as_ptr()), self.instance) {
            contain_owner_callback(
                || tracing::warn!(%error, "preference receiver class retirement failed"),
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
    // A contained panic must not answer a session-end query with a refusal.
    let mut result = LRESULT(isize::from(message == WM_QUERYENDSESSION));
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
                    for subscriber in subscribers.iter() {
                        if let Some(answer) = subscriber.message(hwnd, message, wparam, lparam) {
                            result = answer;
                            return;
                        }
                    }
                }
            }
            result = DefWindowProcW(hwnd, message, wparam, lparam);
        }
    });
    result
}
