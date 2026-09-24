//! OS calls outside what xcap, enigo and the accessibility backend cover.
//! Each function has a Windows implementation and a portable fallback that
//! answers "unknown", which the callers treat as a refusal where safety
//! depends on the answer.

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::{
    KillOnExitJob, init_dpi, move_pointer, release_unicode, runtime_id, send_unicode,
    uia_with_timeouts,
};

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

/// Where keyboard input goes inside the foreground window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "only the Windows lookup reports a focus; elsewhere it is Unknown"
    )
)]
pub enum Focus {
    /// No window in it has focus: keys reach the foreground window itself.
    Foreground,
    /// A window of this process holds keyboard focus.
    Pid(u32),
    /// The OS cannot say, so keyboard input with a safety target is refused.
    Unknown,
}

/// Where keyboard input goes inside foreground window `fg` (the one just
/// checked); an error when the foreground has changed since.
#[cfg_attr(
    not(target_os = "windows"),
    expect(clippy::unnecessary_wraps, reason = "only the Windows lookup can fail")
)]
pub fn focus(fg: u32) -> ToolResult<Focus> {
    #[cfg(target_os = "windows")]
    {
        windows::focus(fg)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = fg;
        Ok(Focus::Unknown)
    }
}

/// Modifier keys down on the keyboard now, the user's included (1 Shift,
/// 2 Ctrl, 4 Alt, 8 Windows).
#[cfg(target_os = "windows")]
pub fn modifiers_down() -> u8 {
    windows::modifiers_down()
}

/// Whether any mouse button is down now, the user's included.
#[cfg(target_os = "windows")]
pub fn mouse_button_down() -> bool {
    windows::mouse_button_down()
}

/// Whether the primary and secondary mouse buttons are swapped.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn buttons_swapped() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::buttons_swapped()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
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

/// A fingerprint of window `id`'s class, where the OS reports it: with the
/// owner's process identity, what tells a window from a later one that got
/// the same id.
pub fn window_class(id: u32) -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        windows::window_class(id)
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
    not(target_os = "windows"),
    expect(
        clippy::unnecessary_wraps,
        reason = "only the Windows enumeration can fail"
    )
)]
pub fn process_windows(pid: u32) -> ToolResult<Option<Vec<u32>>> {
    #[cfg(target_os = "windows")]
    {
        windows::process_windows(pid).map(Some)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        Ok(None)
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
/// `c` on the current keyboard layout; with `command` (a shortcut), the key
/// itself, Caps Lock left out. Also the [`keyboard_owner`] whose layout that
/// was, for the caller to check it still is.
#[cfg(target_os = "windows")]
pub fn char_key(c: char, command: bool) -> Option<(u16, u8, KeyboardOwner)> {
    windows::char_key(c, command)
}

#[cfg(target_os = "windows")]
pub use windows::KeyboardOwner;

/// The [`KeyboardOwner`] now.
#[cfg(target_os = "windows")]
pub fn keyboard_owner() -> KeyboardOwner {
    windows::keyboard_owner()
}
