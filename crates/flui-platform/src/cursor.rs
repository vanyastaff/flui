//! Cursor style types for platform cursor management

/// Platform-independent cursor styles
///
/// Maps to platform-native cursors:
/// - Windows: `LoadCursorW` with IDC_* constants
/// - macOS: `NSCursor` methods
/// - Linux: X11/Wayland cursor names
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorStyle {
    /// Default pointer arrow
    #[default]
    Arrow,
    /// Text insertion cursor (I-beam)
    IBeam,
    /// Crosshair cursor
    Crosshair,
    /// Closed hand (dragging)
    ClosedHand,
    /// Open hand (grab)
    OpenHand,
    /// Pointing hand (links)
    PointingHand,
    /// Resize left edge
    ResizeLeft,
    /// Resize right edge
    ResizeRight,
    /// Resize left-right (horizontal)
    ResizeLeftRight,
    /// Resize top edge
    ResizeUp,
    /// Resize bottom edge
    ResizeDown,
    /// Resize up-down (vertical)
    ResizeUpDown,
    /// Resize diagonal (top-left to bottom-right)
    ResizeUpLeftDownRight,
    /// Resize diagonal (top-right to bottom-left)
    ResizeUpRightDownLeft,
    /// Resize column
    ResizeColumn,
    /// Resize row
    ResizeRow,
    /// Operation not allowed
    OperationNotAllowed,
    /// Drag link
    DragLink,
    /// Drag copy
    DragCopy,
    /// Contextual menu
    ContextualMenu,
    /// Hidden cursor
    None,
}
