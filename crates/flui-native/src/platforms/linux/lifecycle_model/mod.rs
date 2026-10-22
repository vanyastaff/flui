//! What the Linux window adapters decide, kept apart from the connections they
//! read.
//!
//! Each adapter reads a window manager and hands the raw answer to a model
//! here; the model is pure, so a table of inputs checks it on every host.
//!
//! - [`x11_window_mode`]: the properties a window manager sets on a window
//!   (`_NET_WM_STATE`, `WM_STATE`) to the window's mode.

pub(crate) mod x11_window_mode;
