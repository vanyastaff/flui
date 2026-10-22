//! The Win32 window, in its halves (ADR-0151 §10):
//!
//! - [`limits`]: the pure half, the size limits the system asks for as rules
//!   over plain values, compiled and tested on every host;
//! - `window`: [`WindowsWindow`], the handle a host hands out, how it opens
//!   and what its commands ask of the OS;
//! - `set`: [`WindowSet`], the windows of a platform, which the loop holds as
//!   `owner::windows::ClassWindows`, the window position and where a window
//!   opens;
//! - `proc`: the window's context in its `GWLP_USERDATA` slot and the
//!   window's own messages, the hook `WindowProc` the host registers on FLUI's
//!   window class;
//! - `backdrop`: what the desktop window manager draws around and behind the
//!   window, and the extension trait that sets it.

// The halves under `cfg(windows)` are one of the workspace's sanctioned
// `unsafe` FFI islands — direct Win32 calls have no safe wrapper. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`); every `unsafe`
// block still carries its own `// SAFETY:` comment stating the invariant
// that makes it sound. The pure half forbids it.
#![cfg_attr(windows, expect(unsafe_code))]

#[cfg(windows)]
pub(crate) mod backdrop;
pub(crate) mod limits;
#[cfg(windows)]
pub(crate) mod proc;
#[cfg(windows)]
mod set;
#[cfg(windows)]
mod window;

#[cfg(windows)]
pub(crate) use set::WindowSet;
#[cfg(windows)]
pub use window::WindowsWindow;
