//! OS calls outside what xcap, enigo and the accessibility backend cover.
//! Each function has a Windows implementation and a portable fallback that
//! answers "unknown", which the callers treat as a refusal where safety
//! depends on the answer.

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::{KillOnExitJob, init_dpi, move_pointer};

#[cfg(not(target_os = "windows"))]
use crate::error::ToolError;
use crate::error::ToolResult;
use crate::geometry::Rect;

/// What the OS says is under a screen point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Under {
    /// The top-level window there.
    pub id: u32,
    /// Its process.
    pub pid: u32,
    /// The process of the deepest window there, the one that takes a click.
    pub inner_pid: u32,
}

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

/// What is under a screen point, when the OS reports it directly.
pub fn window_at(x: i32, y: i32) -> Option<Under> {
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

/// Window `id`'s bounds, read directly rather than through the window list.
pub fn window_rect(id: u32) -> Option<Rect> {
    #[cfg(target_os = "windows")]
    {
        windows::window_rect(id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        None
    }
}

/// Window `id`'s title, read directly.
pub fn window_title(id: u32) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        windows::window_title(id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        None
    }
}

/// The top-level window `id` belongs to.
#[cfg_attr(
    not(target_os = "windows"),
    expect(dead_code, reason = "used by the UIA backend, Windows-only")
)]
pub fn root_window(id: u32) -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        windows::root_window(id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        None
    }
}

/// The process that owns window `id`.
pub fn window_pid(id: u32) -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        windows::window_pid(id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        None
    }
}

/// When process `pid` started, in an OS-specific unit; with the pid, an
/// identity the OS does not recycle. `None` when the OS cannot say.
pub fn process_started(pid: u32) -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        windows::process_started(pid)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        None
    }
}

/// Process `pid`'s visible top-level windows front to back, popups
/// included; `None` when the OS offers no such list (use the window list).
#[cfg_attr(
    target_os = "windows",
    expect(
        clippy::unnecessary_wraps,
        reason = "`None` is the answer on the OSes without a window enumeration"
    )
)]
pub fn process_windows(pid: u32) -> Option<Vec<u32>> {
    #[cfg(target_os = "windows")]
    {
        Some(windows::process_windows(pid))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
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

/// The virtual key and modifier bits (1 Shift, 2 Ctrl, 4 Alt) that type
/// `c` on the current keyboard layout.
#[cfg(target_os = "windows")]
pub fn char_key(c: char) -> Option<(u16, u8)> {
    windows::char_key(c)
}
