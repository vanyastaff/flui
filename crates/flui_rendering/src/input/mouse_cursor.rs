//! Mouse cursor types and management.

use std::fmt::Debug;

/// Represents a mouse cursor.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `MouseCursor` class.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MouseCursor {
    /// Defer to the next region for cursor decision.
    Defer,

    /// Hide the cursor completely.
    None,

    /// A system-defined cursor.
    System(SystemMouseCursor),

    /// A custom cursor identified by a string key.
    Custom(String),
}

impl Default for MouseCursor {
    fn default() -> Self {
        Self::Defer
    }
}

impl MouseCursor {
    /// The basic arrow cursor.
    pub const BASIC: Self = Self::System(SystemMouseCursor::Basic);

    /// A pointing hand cursor for clickable elements.
    pub const CLICK: Self = Self::System(SystemMouseCursor::Click);

    /// A text selection cursor (I-beam).
    pub const TEXT: Self = Self::System(SystemMouseCursor::Text);

    /// A forbidden/not-allowed cursor.
    pub const FORBIDDEN: Self = Self::System(SystemMouseCursor::Forbidden);

    /// A wait/busy cursor.
    pub const WAIT: Self = Self::System(SystemMouseCursor::Wait);

    /// A progress cursor (arrow with hourglass).
    pub const PROGRESS: Self = Self::System(SystemMouseCursor::Progress);

    /// A grab cursor (open hand).
    pub const GRAB: Self = Self::System(SystemMouseCursor::Grab);

    /// A grabbing cursor (closed hand).
    pub const GRABBING: Self = Self::System(SystemMouseCursor::Grabbing);

    /// Returns whether this cursor defers to the next region.
    pub fn is_defer(&self) -> bool {
        matches!(self, Self::Defer)
    }

    /// Returns whether this cursor is hidden.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// System-defined mouse cursors.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SystemMouseCursors` class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemMouseCursor {
    /// The basic arrow cursor.
    Basic,

    /// A pointing hand cursor, typically used for links.
    Click,

    /// A forbidden action cursor.
    Forbidden,

    /// A wait/busy cursor.
    Wait,

    /// A progress cursor (wait with arrow).
    Progress,

    /// A context menu cursor.
    ContextMenu,

    /// A help cursor.
    Help,

    /// A text selection cursor (I-beam).
    Text,

    /// A vertical text selection cursor.
    VerticalText,

    /// A cell selection cursor.
    Cell,

    /// A precise/crosshair cursor.
    Precise,

    /// A move cursor.
    Move,

    /// A grab/open hand cursor.
    Grab,

    /// A grabbing/closed hand cursor.
    Grabbing,

    /// No drop cursor.
    NoDrop,

    /// An alias cursor.
    Alias,

    /// A copy cursor.
    Copy,

    /// A disappearing item cursor.
    Disappearing,

    /// An all-scroll cursor.
    AllScroll,

    /// Resize to the north.
    ResizeDown,

    /// Resize to the south.
    ResizeUp,

    /// Resize to the east.
    ResizeLeft,

    /// Resize to the west.
    ResizeRight,

    /// Resize to the north-south.
    ResizeUpDown,

    /// Resize to the east-west.
    ResizeLeftRight,

    /// Resize to the up-left/down-right diagonal.
    ResizeUpLeftDownRight,

    /// Resize to the up-right/down-left diagonal.
    ResizeUpRightDownLeft,

    /// Resize a column.
    ResizeColumn,

    /// Resize a row.
    ResizeRow,

    /// A zoom-in cursor.
    ZoomIn,

    /// A zoom-out cursor.
    ZoomOut,
}

impl Default for SystemMouseCursor {
    fn default() -> Self {
        Self::Basic
    }
}

impl SystemMouseCursor {
    /// Returns the platform-specific cursor name.
    ///
    /// This can be used to map to platform cursor APIs.
    pub fn platform_name(&self) -> &'static str {
        match self {
            Self::Basic => "default",
            Self::Click => "pointer",
            Self::Forbidden => "not-allowed",
            Self::Wait => "wait",
            Self::Progress => "progress",
            Self::ContextMenu => "context-menu",
            Self::Help => "help",
            Self::Text => "text",
            Self::VerticalText => "vertical-text",
            Self::Cell => "cell",
            Self::Precise => "crosshair",
            Self::Move => "move",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::NoDrop => "no-drop",
            Self::Alias => "alias",
            Self::Copy => "copy",
            Self::Disappearing => "default", // No standard equivalent
            Self::AllScroll => "all-scroll",
            Self::ResizeDown => "s-resize",
            Self::ResizeUp => "n-resize",
            Self::ResizeLeft => "w-resize",
            Self::ResizeRight => "e-resize",
            Self::ResizeUpDown => "ns-resize",
            Self::ResizeLeftRight => "ew-resize",
            Self::ResizeUpLeftDownRight => "nwse-resize",
            Self::ResizeUpRightDownLeft => "nesw-resize",
            Self::ResizeColumn => "col-resize",
            Self::ResizeRow => "row-resize",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
        }
    }
}

/// A session for managing cursor state on a device.
///
/// This tracks the active cursor for a specific pointer device.
#[derive(Debug)]
pub struct MouseCursorSession {
    /// The device ID this session is for.
    device: i32,

    /// The currently active cursor.
    cursor: MouseCursor,
}

impl MouseCursorSession {
    /// Creates a new cursor session for a device.
    pub fn new(device: i32) -> Self {
        Self {
            device,
            cursor: MouseCursor::BASIC,
        }
    }

    /// Returns the device ID.
    pub fn device(&self) -> i32 {
        self.device
    }

    /// Returns the current cursor.
    pub fn cursor(&self) -> &MouseCursor {
        &self.cursor
    }

    /// Activates a new cursor.
    ///
    /// Returns `true` if the cursor changed.
    pub fn activate(&mut self, cursor: MouseCursor) -> bool {
        if self.cursor != cursor {
            self.cursor = cursor;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mouse_cursor_default() {
        let cursor = MouseCursor::default();
        assert!(cursor.is_defer());
    }

    #[test]
    fn test_mouse_cursor_constants() {
        assert_eq!(
            MouseCursor::BASIC,
            MouseCursor::System(SystemMouseCursor::Basic)
        );
        assert_eq!(
            MouseCursor::CLICK,
            MouseCursor::System(SystemMouseCursor::Click)
        );
        assert_eq!(
            MouseCursor::TEXT,
            MouseCursor::System(SystemMouseCursor::Text)
        );
    }

    #[test]
    fn test_mouse_cursor_is_none() {
        assert!(MouseCursor::None.is_none());
        assert!(!MouseCursor::BASIC.is_none());
    }

    #[test]
    fn test_system_cursor_platform_name() {
        assert_eq!(SystemMouseCursor::Basic.platform_name(), "default");
        assert_eq!(SystemMouseCursor::Click.platform_name(), "pointer");
        assert_eq!(SystemMouseCursor::Text.platform_name(), "text");
        assert_eq!(SystemMouseCursor::Wait.platform_name(), "wait");
    }

    #[test]
    fn test_cursor_session_new() {
        let session = MouseCursorSession::new(0);
        assert_eq!(session.device(), 0);
        assert_eq!(session.cursor(), &MouseCursor::BASIC);
    }

    #[test]
    fn test_cursor_session_activate() {
        let mut session = MouseCursorSession::new(0);

        // First change
        assert!(session.activate(MouseCursor::CLICK));
        assert_eq!(session.cursor(), &MouseCursor::CLICK);

        // Same cursor - no change
        assert!(!session.activate(MouseCursor::CLICK));

        // Different cursor
        assert!(session.activate(MouseCursor::TEXT));
        assert_eq!(session.cursor(), &MouseCursor::TEXT);
    }
}
