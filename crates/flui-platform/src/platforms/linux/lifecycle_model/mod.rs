//! What the Linux window and host adapters decide, kept apart from the
//! connections they read.
//!
//! Each adapter reads a window manager, the kernel or the login manager, and
//! hands the raw answer to a model here; the model is pure, so a table of
//! inputs checks it on every host.
//!
//! - [`x11_window_mode`]: the properties a window manager sets on a window
//!   (`_NET_WM_STATE`, `WM_STATE`) to the window's mode.
//! - [`memory_pressure`]: `/proc/pressure/memory` lines and the stall
//!   triggers that turn the kernel's pressure into a level.
//! - [`logind`]: the login manager's signals to sleep, shutdown and lock
//!   events, and what to do with the delay inhibitor after each.

// The adapters that read these models come with the window commands; until
// they land the models have no caller outside their tests.
#![allow(
    dead_code,
    reason = "the Linux window and host adapters that read these models land with the window commands"
)]

pub(crate) mod logind;
pub(crate) mod memory_pressure;
pub(crate) mod x11_window_mode;
