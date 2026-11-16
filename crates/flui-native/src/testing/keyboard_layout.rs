//! The one way in to a host's keyboard layout from outside the crate.
//!
//! The layout is the core host's sealed input position: nothing above the
//! host reads it, so [`Platform`](crate::Platform) does not publish it and no
//! code outside the crate can call it (ADR-0151 §9). The host's integration
//! tests in `tests/` still drive it through real loops, with fixtures that
//! live there (Xvfb and Weston sessions, the Win32 message pump), so they
//! reach it here, behind the `testing` feature.

use crate::host::{Host, Input as _};
use crate::platform::HostPlatform;

/// The current keyboard layout's name as the operating system gives it;
/// empty where the host does not read the layout.
#[must_use]
pub fn current<H: Host>(platform: &HostPlatform<H>) -> String {
    H::Input::keyboard_layout(platform.owner())
}

/// Call `callback` on each keyboard layout change, replacing (and dropping)
/// the callback registered before it.
pub fn on_change<H: Host>(platform: &HostPlatform<H>, callback: Box<dyn FnMut()>) {
    H::Input::on_keyboard_layout_change(platform.owner(), callback);
}
