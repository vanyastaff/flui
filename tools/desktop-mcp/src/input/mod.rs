//! Real pointer and keyboard input.
//!
//! On Windows and macOS the device is enigo's. Linux is not built in yet:
//! enigo and xcap link libxkbcommon and PipeWire there, which the workspace's
//! Linux builds do not install, so on Linux [`Input`] is uninhabited and
//! [`Input::new`] reports why.

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod device;

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use device::Input;

/// Runs before each emitted input event and refuses it with an error — how
/// the server keeps a multi-event action (a repeated key, a drag, typed text)
/// from reaching a window that took the foreground partway through. It is
/// given the screen point the event lands on, when it has one (each step of a
/// drag), so that point is checked too.
pub type Guard<'a> = dyn FnMut(Option<(i32, i32)>) -> crate::error::ToolResult<()> + 'a;

/// Which mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Primary.
    Left,
    /// Secondary.
    Right,
    /// Wheel button.
    Middle,
}

/// No input device on this OS: the type has no values.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[derive(Debug)]
pub enum Input {}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Input {
    /// Why there is no input device.
    pub fn new() -> Result<Self, String> {
        Err(format!(
            "input is not supported on {} yet (Windows and macOS only)",
            std::env::consts::OS
        ))
    }

    /// Unreachable: no `Input` exists.
    pub fn move_to(&mut self, _: i32, _: i32) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn position(&self) -> Option<(i32, i32)> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn click(
        &mut self,
        _: i32,
        _: i32,
        _: MouseButton,
        _: bool,
        _: &mut Guard<'_>,
    ) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn drag(
        &mut self,
        _: (i32, i32),
        _: (i32, i32),
        _: std::time::Duration,
        _: &mut Guard<'_>,
    ) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn scroll(
        &mut self,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: &mut Guard<'_>,
    ) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn type_text(&mut self, _: &str, _: &mut Guard<'_>) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn key(
        &mut self,
        _: &crate::keys::KeyCombo,
        _: u32,
        _: &mut Guard<'_>,
    ) -> crate::error::ToolResult<()> {
        match *self {}
    }
}
