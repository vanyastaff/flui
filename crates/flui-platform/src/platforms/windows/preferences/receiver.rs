//! A hidden top-level receiver; message-only HWNDs cannot receive broadcasts.

use std::{cell::Cell, sync::Arc};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
            GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW, UnregisterClassW, WINDOW_STYLE,
            WM_CLOSE, WM_NCCREATE, WM_NCDESTROY, WM_SETTINGCHANGE, WNDCLASSW, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
        },
    },
    core::{PCWSTR, w},
};

use super::Invalidation;
use crate::shared::panic_boundary::contain_owner_callback;

struct Context {
    invalidation: Arc<Invalidation>,
    hwnd: Cell<Option<HWND>>,
}

/// The class registration is private to this receiver. Its allocation-based
/// name stays unique while the context lives, including failed destruction.
pub(super) struct Receiver {
    context: Option<Box<Context>>,
    class: Vec<u16>,
    instance: HINSTANCE,
}

impl Receiver {
    #[cfg(test)]
    pub(super) fn send_setting_change(&self) {
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
                WM_SETTINGCHANGE,
                Some(WPARAM(0)),
                Some(LPARAM(0)),
            );
        }
    }

    pub(super) fn new(invalidation: Arc<Invalidation>) -> windows::core::Result<Self> {
        let context = Box::new(Context {
            invalidation,
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
        Ok(receiver)
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let Some(context) = self.context.take() else {
            return;
        };
        if let Some(hwnd) = context.hwnd.get() {
            // SAFETY: only this owner can destroy its live HWND. WM_NCDESTROY
            // clears the handle before reuse; context stays live through this call.
            if let Err(error) = unsafe { DestroyWindow(hwnd) } {
                // Keep both userdata and its unique class name reserved if the
                // OS still owns the HWND. The already-closed source is inert.
                std::mem::forget(context);
                contain_owner_callback(
                    || tracing::error!(%error, "preference receiver retained after failed destruction"),
                );
                return;
            }
        }
        // SAFETY: no live HWND refers to this private class after destruction.
        if let Err(error) =
            unsafe { UnregisterClassW(PCWSTR(self.class.as_ptr()), Some(self.instance)) }
        {
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
    let mut result = LRESULT(0);
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
                } else if message == WM_SETTINGCHANGE {
                    let invalidation = Arc::clone(&(*context).invalidation);
                    invalidation.notify();
                    return;
                } else if message == WM_CLOSE {
                    // This internal receiver is retired by the host, not by
                    // ordinary window-close routing or last-user-window policy.
                    return;
                }
            }
            result = DefWindowProcW(hwnd, message, wparam, lparam);
        }
    });
    result
}
