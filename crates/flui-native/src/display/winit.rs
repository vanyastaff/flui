//! The winit host's displays: the native source of the operating system it
//! runs on, never winit's monitor list, which exists only inside a live event
//! loop and keys monitors by labels that are reused (ADR-0236).
//!
//! The winit loop holds its source through its `HostDisplays` hook, which the
//! table fills:
//!
//! - on Windows and macOS with the Win32 and AppKit hosts' own sources, which
//!   implement the hook beside their own loop's (`display::windows`,
//!   `display::apple::macos`);
//! - on Linux with `WinitDisplaySource`, which reads the compositor's or X
//!   server's outputs on a connection of its own (`display::linux`).

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub(crate) use linux::WinitDisplaySource;

use flui_platform::DisplaySnapshot;

use crate::error::PlatformError;
use crate::host;
use crate::owner::winit::WinitLoop;

/// The winit host's displays position: the snapshot of the source its loop
/// holds.
#[derive(Debug)]
pub struct WinitDisplays;

// The answer is the native source of the OS (Windows: `QueryDisplayConfig`,
// macOS: `NSScreen`, Linux: the compositor), kept between calls and read
// again only after an event marked it stale: the host asks on every owner
// wake. It needs no event loop, so it answers before `Platform::run`; on
// Linux that first answer is read once on a connection that then closes,
// and the display thread takes over when the loop resumes.
impl host::Displays<WinitLoop> for WinitDisplays {
    fn snapshot(owner: &WinitLoop) -> Result<DisplaySnapshot, PlatformError> {
        owner.displays.snapshot()
    }
}
