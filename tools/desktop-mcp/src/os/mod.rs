//! OS calls outside what xcap, enigo and the accessibility backend cover.
//! Each function has a Windows implementation and a portable fallback.

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::{KillOnExitJob, init_dpi, move_pointer};

#[cfg(not(target_os = "windows"))]
use crate::error::ToolError;
use crate::error::ToolResult;

/// The foreground window's id and process, when the OS reports it directly.
/// `None` means "ask the window list" (xcap's `is_focused`).
pub fn foreground() -> Option<(u32, u32)> {
    #[cfg(target_os = "windows")]
    {
        windows::foreground()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// The top-level window under a screen point and its process, when the OS
/// reports it directly.
pub fn window_at(x: i32, y: i32) -> Option<(u32, u32)> {
    #[cfg(target_os = "windows")]
    {
        windows::window_at(x, y)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (x, y);
        None
    }
}

/// Asks the OS to put window `id` in front.
pub fn bring_to_front(id: u32) -> ToolResult<()> {
    #[cfg(target_os = "windows")]
    {
        windows::bring_to_front(id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        Err(ToolError::NotSupported(format!(
            "activate_window is not supported on {} yet",
            std::env::consts::OS
        )))
    }
}

/// The pointer position as the OS reports it, if it can.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn cursor() -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        windows::cursor()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}
