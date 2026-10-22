//! The Windows host's text input: the Text Services Framework reads the
//! focused field's store (ADR-0135).
//!
//! - `geometry`: the pure half, a text range's logical rect as the physical
//!   screen rect an input method places its windows against, checked on every
//!   host;
//! - `tsf`: each window's TSF connection, its documents and their store, the
//!   window's `TextStoreHost`;
//! - [`Tsf`]: the text services' hook on FLUI's window class, which activates
//!   each new window's connection.
//!
//! The window keeps its connection in its context, so the window's own
//! messages reach it there: a move, a size-and-move loop's end and a DPI
//! change tell it the field's screen rectangles moved, and `WM_DESTROY`
//! deactivates it between the close callbacks and the context's retirement
//! (ADR-0135 §3).

pub(crate) mod geometry;
#[cfg(windows)]
mod tsf;

#[cfg(windows)]
pub(crate) use tsf::TextServices;

#[cfg(windows)]
use std::rc::Rc;

#[cfg(all(windows, test))]
use windows::Win32::Foundation::HWND;

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
        let Some(this) = with_window_context(hwnd, "text_services", |context| {
            WindowRef::new(hwnd, context.identity)
        }) else {
            return;
        };
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
}

#[cfg(all(windows, test))]
thread_local! {
    /// Runs as a window's text services are about to activate: lets a test
    /// stand in for a message the activation dispatches.
    static BEFORE_TSF_ACTIVATION: std::cell::Cell<Option<fn(HWND)>> =
        const { std::cell::Cell::new(None) };
}
