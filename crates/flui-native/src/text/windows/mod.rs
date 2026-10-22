//! The Windows host's text input: the Text Services Framework reads the
//! focused field's store (ADR-0135).
//!
//! - `geometry`: the pure half, a text range's logical rect as the physical
//!   screen rect an input method places its windows against, checked on every
//!   host;
//! - `tsf`: each window's TSF connection, its documents and their store, the
//!   window's `TextStoreHost`;
//! - [`Tsf`]: the text services' hook on FLUI's window class, which activates
//!   each new window's connection and tells it when the window moved.
//!
//! The window keeps its connection in its context. Two of the window's own
//! messages still reach it from the window's procedure, because their order
//! is the window's: a DPI change tells it the field's screen rectangles moved
//! once the new scale is applied, and `WM_DESTROY` deactivates it between the
//! close callbacks and the context's retirement (ADR-0135 §3).

pub(crate) mod geometry;
#[cfg(windows)]
mod tsf;

#[cfg(windows)]
pub(crate) use tsf::TextServices;

#[cfg(windows)]
use std::rc::Rc;

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{WM_EXITSIZEMOVE, WM_MOVE};

#[cfg(windows)]
use crate::owner::windows::{CreatedWindow, WindowMessages};
#[cfg(windows)]
use crate::platforms::windows::platform::{WindowRef, with_window_context};

/// The text services' hook on FLUI's window class: each window the class
/// creates gets its TSF connection, held in the window's context
/// (ADR-0135 §3).
#[cfg(windows)]
pub(crate) struct Tsf;

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "the window class's hook trait declares its message entry `unsafe`"
)]
impl WindowMessages for Tsf {
    /// Activate TSF for the window just installed, on its owner thread, and
    /// hold the connection in its context: from here on the window offers its
    /// text-store host. TSF's calls read the window's context (its scale),
    /// which is installed before the hooks learn of the window. A failure
    /// leaves the window without a host, and text input takes the `WM_CHAR`
    /// path, as it does with no apartment (the platform's is reachable on its
    /// owner thread only, where windows are opened). A message the activation
    /// dispatched may have destroyed the window; the open then fails.
    fn created(&self, window: &CreatedWindow) {
        let hwnd = window.hwnd;
        let Some(apartment) = window.apartment.clone() else {
            tracing::warn!(
                ?hwnd,
                "no COM apartment for TSF; text input falls back to WM_CHAR"
            );
            return;
        };
        let this = WindowRef::new(hwnd, window.identity);
        #[cfg(test)]
        if let Some(hook) = BEFORE_TSF_ACTIVATION.get() {
            hook(hwnd);
        }
        match TextServices::activate(this, apartment) {
            Ok(services) => {
                let installed = this.with_context("text_services", |context| {
                    context.text_services.replace(Some(Rc::clone(&services)))
                });
                if installed.is_none() {
                    // The window is already gone (destroyed by a message
                    // the activation dispatched): deactivate at once.
                    TextServices::retire_with_window(services);
                }
            }
            Err(error) => {
                tracing::warn!(
                    ?hwnd,
                    ?error,
                    "TSF did not activate for this window; text input falls back to WM_CHAR"
                );
            }
        }
    }

    /// The window moved, or a size-and-move loop ended: the focused field's
    /// text sits somewhere else on the screen, and TSF is told so. The
    /// window's procedure still answers the message, after this hook: TSF
    /// reads the window's position from the system and its scale from the
    /// context, neither of which the procedure's own handling of these
    /// messages changes. A DPI change is told from the window's own handling,
    /// once the new scale is applied, which TSF's synchronous layout query
    /// reads.
    unsafe fn message(
        &self,
        hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        if matches!(msg, WM_MOVE | WM_EXITSIZEMOVE) {
            // Cloned out of the context, so no borrow is held while TSF and
            // the field's store run.
            let services = with_window_context(hwnd, "text_services", |context| {
                context.text_services.borrow().clone()
            })
            .flatten();
            if let Some(services) = services {
                services.window_moved();
            }
        }
        None
    }
}

#[cfg(all(windows, test))]
thread_local! {
    /// Runs as a window's text services are about to activate: lets a test
    /// stand in for a message the activation dispatches.
    static BEFORE_TSF_ACTIVATION: std::cell::Cell<Option<fn(HWND)>> =
        const { std::cell::Cell::new(None) };
}
