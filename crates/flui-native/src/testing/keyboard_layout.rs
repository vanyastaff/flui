//! The keyboard layout a host reads and the callback its change reaches.
//!
//! Nothing above the host reads the layout yet, so [`Platform`](crate::Platform)
//! does not publish it (ADR-0151 §9); a host's tests reach its input position
//! here.

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
