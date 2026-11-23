//! The errors a headless host's waiting and window-opening calls return.

use flui_runtime::window_system::WindowSystem;

/// The UI runtime still had work after a bounded run of frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the UI runtime had not settled after {frames} frames")]
pub struct NotSettled {
    /// The frames pumped.
    pub frames: usize,
}

/// A window was opened that plays another window system than its host's:
/// the browser where the host's primary window plays a platform's, or the
/// other way round. A host has one window system, as a runner does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "a window playing {window:?} cannot open beside windows playing {host:?}: the window system \
     is the host's, set by its primary window"
)]
#[non_exhaustive]
pub struct WindowSystemMismatch {
    /// The window system the host's windows play.
    pub host: WindowSystem,
    /// The one the refused window plays.
    pub window: WindowSystem,
}
