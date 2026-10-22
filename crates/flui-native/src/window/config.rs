//! What a host's windows open with beyond their `WindowOptions`.
//!
//! Crate-private: nothing outside the core host builds one (ADR-0151 §8), so
//! every window opens with the default.

#![forbid(unsafe_code)]

/// What a host's windows open with beyond their `WindowOptions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowConfiguration {
    /// The virtual key that toggles fullscreen, `None` for no key. The Win32
    /// window reads it as a `VK_*` code (F11 is `0x7A`).
    pub(crate) fullscreen_hotkey: Option<u16>,
}

impl Default for WindowConfiguration {
    fn default() -> Self {
        WindowConfiguration {
            // F11 is the conventional fullscreen hotkey across platforms.
            fullscreen_hotkey: Some(0x7A),
        }
    }
}
