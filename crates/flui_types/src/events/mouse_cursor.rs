//! Mouse cursor types for Flutter-compatible cursor management
//!
//! This module provides mouse cursor types that mirror Flutter's `MouseCursor` hierarchy:
//!
//! - [`MouseCursor`] - Base enum for all cursor types
//! - [`SystemMouseCursor`] - System-provided cursors (most common)
//! - [`MouseCursorSession`] - Active cursor session for a device
//!
//! # Flutter Architecture
//!
//! ```text
//! MouseCursor (abstract)
//!     ├── SystemMouseCursor (system cursors)
//!     │       └── SystemMouseCursors (static instances)
//!     ├── DeferringMouseCursor (defer to other cursor)
//!     └── NoopMouseCursor (no-op, debugging)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_types::events::{MouseCursor, SystemMouseCursors};
//!
//! // Use a system cursor
//! let cursor = MouseCursor::System(SystemMouseCursors::CLICK);
//!
//! // Check cursor type
//! if cursor.is_clickable() {
//!     println!("This is a clickable cursor!");
//! }
//! ```
//!
//! # Platform Mapping
//!
//! System cursors map to platform-native cursors:
//!
//! | FLUI | Windows | macOS | Linux/X11 |
//! |------|---------|-------|-----------|
//! | Basic | IDC_ARROW | arrowCursor | default |
//! | Click | IDC_HAND | pointingHandCursor | pointer |
//! | Text | IDC_IBEAM | IBeamCursor | text |
//! | Wait | IDC_WAIT | busyButClickableCursor | wait |
//! | Forbidden | IDC_NO | operationNotAllowedCursor | not-allowed |
//!
//! # References
//!
//! - [Flutter MouseCursor](https://api.flutter.dev/flutter/services/MouseCursor-class.html)
//! - [Flutter SystemMouseCursors](https://api.flutter.dev/flutter/services/SystemMouseCursors-class.html)

/// Mouse cursor representation.
///
/// This is the base type for all mouse cursors in FLUI. Use [`SystemMouseCursors`]
/// constants for common system cursors.
///
/// # Flutter Compliance
///
/// | Flutter | FLUI |
/// |---------|------|
/// | `MouseCursor` | `MouseCursor` |
/// | `SystemMouseCursor` | `MouseCursor::System(SystemMouseCursor)` |
/// | `MouseCursor.defer` | `MouseCursor::Defer` |
/// | `MouseCursor.uncontrolled` | `MouseCursor::Uncontrolled` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MouseCursor {
    /// A system mouse cursor.
    ///
    /// System cursors are provided by the operating system and are the most
    /// common type of cursor used in applications.
    System(SystemMouseCursor),

    /// Defer to the next cursor in the hit-test chain.
    ///
    /// When a widget uses this cursor, the cursor is determined by the next
    /// widget in the hit-test result. This is useful for transparent overlays.
    Defer,

    /// An uncontrolled cursor that doesn't participate in cursor management.
    ///
    /// Use this when cursor management is handled elsewhere (e.g., by the platform).
    Uncontrolled,
}

impl MouseCursor {
    /// The basic arrow cursor (default).
    pub const BASIC: Self = Self::System(SystemMouseCursor::Basic);

    /// The click/pointer cursor (hand).
    pub const CLICK: Self = Self::System(SystemMouseCursor::Click);

    /// The text selection cursor (I-beam).
    pub const TEXT: Self = Self::System(SystemMouseCursor::Text);

    /// The forbidden/not-allowed cursor.
    pub const FORBIDDEN: Self = Self::System(SystemMouseCursor::Forbidden);

    /// The wait/busy cursor.
    pub const WAIT: Self = Self::System(SystemMouseCursor::Wait);

    /// The progress cursor (busy but clickable).
    pub const PROGRESS: Self = Self::System(SystemMouseCursor::Progress);

    /// The grab cursor (open hand).
    pub const GRAB: Self = Self::System(SystemMouseCursor::Grab);

    /// The grabbing cursor (closed hand).
    pub const GRABBING: Self = Self::System(SystemMouseCursor::Grabbing);

    /// Returns whether this is a system cursor.
    #[inline]
    pub const fn is_system(&self) -> bool {
        matches!(self, Self::System(_))
    }

    /// Returns whether this cursor defers to another.
    #[inline]
    pub const fn is_defer(&self) -> bool {
        matches!(self, Self::Defer)
    }

    /// Returns whether this is an uncontrolled cursor.
    #[inline]
    pub const fn is_uncontrolled(&self) -> bool {
        matches!(self, Self::Uncontrolled)
    }

    /// Returns the system cursor if this is a system cursor.
    #[inline]
    pub const fn as_system(&self) -> Option<SystemMouseCursor> {
        match self {
            Self::System(cursor) => Some(*cursor),
            _ => None,
        }
    }

    /// Returns whether this is a clickable cursor (pointer/hand).
    #[inline]
    pub const fn is_clickable(&self) -> bool {
        matches!(self, Self::System(SystemMouseCursor::Click))
    }

    /// Returns whether this indicates text selection.
    #[inline]
    pub const fn is_text(&self) -> bool {
        matches!(self, Self::System(SystemMouseCursor::Text))
    }

    /// Returns whether this indicates a resize operation.
    pub const fn is_resize(&self) -> bool {
        matches!(
            self,
            Self::System(
                SystemMouseCursor::ResizeColumn
                    | SystemMouseCursor::ResizeRow
                    | SystemMouseCursor::ResizeUp
                    | SystemMouseCursor::ResizeDown
                    | SystemMouseCursor::ResizeLeft
                    | SystemMouseCursor::ResizeRight
                    | SystemMouseCursor::ResizeUpLeft
                    | SystemMouseCursor::ResizeUpRight
                    | SystemMouseCursor::ResizeDownLeft
                    | SystemMouseCursor::ResizeDownRight
                    | SystemMouseCursor::ResizeUpDown
                    | SystemMouseCursor::ResizeLeftRight
                    | SystemMouseCursor::ResizeUpLeftDownRight
                    | SystemMouseCursor::ResizeUpRightDownLeft
            )
        )
    }

    /// Returns whether this indicates a move operation.
    #[inline]
    pub const fn is_move(&self) -> bool {
        matches!(self, Self::System(SystemMouseCursor::Move))
    }

    /// Returns whether this indicates a drag operation.
    #[inline]
    pub const fn is_drag(&self) -> bool {
        matches!(
            self,
            Self::System(
                SystemMouseCursor::Grab
                    | SystemMouseCursor::Grabbing
                    | SystemMouseCursor::AllScroll
            )
        )
    }

    /// Returns whether this indicates a forbidden/not-allowed action.
    #[inline]
    pub const fn is_forbidden(&self) -> bool {
        matches!(
            self,
            Self::System(SystemMouseCursor::Forbidden | SystemMouseCursor::NoDrop)
        )
    }

    /// Returns whether this indicates a busy/wait state.
    #[inline]
    pub const fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::System(SystemMouseCursor::Wait | SystemMouseCursor::Progress)
        )
    }
}

impl Default for MouseCursor {
    fn default() -> Self {
        Self::BASIC
    }
}

impl From<SystemMouseCursor> for MouseCursor {
    fn from(cursor: SystemMouseCursor) -> Self {
        Self::System(cursor)
    }
}

/// System mouse cursor types.
///
/// These are standard cursors provided by the operating system. They don't
/// require external resources and work across all platforms.
///
/// # Flutter Compliance
///
/// All cursors from Flutter's `SystemMouseCursors` class are represented here.
/// The naming follows Flutter's conventions (use-case based, not appearance based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SystemMouseCursor {
    // ========================================================================
    // Basic cursors
    // ========================================================================
    /// No cursor is displayed.
    ///
    /// Use when the cursor should be hidden.
    None,

    /// The basic arrow cursor.
    ///
    /// The most common cursor, used for general pointing.
    #[default]
    Basic,

    /// A clickable cursor (pointing hand).
    ///
    /// Used for clickable elements like buttons and links.
    Click,

    /// A cursor that indicates the UI is busy.
    ///
    /// The cursor is displayed as a wait indicator, but the application
    /// may still be interactive.
    Wait,

    /// A cursor that indicates the UI is busy but still responsive.
    ///
    /// Unlike `Wait`, this indicates the user can still interact with the UI.
    Progress,

    /// A cursor indicating that the action is not allowed.
    ///
    /// Used when hovering over disabled elements.
    Forbidden,

    // ========================================================================
    // Selection cursors
    // ========================================================================
    /// A cursor indicating text can be selected.
    ///
    /// The I-beam cursor used in text fields.
    Text,

    /// A cursor indicating vertical text can be selected.
    ///
    /// A rotated I-beam for vertical text.
    VerticalText,

    /// A cursor indicating something can be selected.
    ///
    /// A crosshair cursor for precise selection.
    Precise,

    /// A context menu cursor.
    ///
    /// Indicates a context menu is available.
    ContextMenu,

    // ========================================================================
    // Drag cursors
    // ========================================================================
    /// A cursor indicating something can be grabbed.
    ///
    /// An open hand cursor.
    Grab,

    /// A cursor indicating something is being grabbed.
    ///
    /// A closed hand cursor.
    Grabbing,

    /// A cursor for moving in all directions.
    ///
    /// Four-way arrow cursor.
    Move,

    /// A cursor for scrolling in all directions.
    ///
    /// Similar to `Move` but indicates scrolling.
    AllScroll,

    /// A cursor indicating dragging will copy.
    Copy,

    /// A cursor indicating dragging will create an alias/shortcut.
    Alias,

    /// A cursor indicating no drop target is available.
    NoDrop,

    /// A cursor indicating a cell can be selected.
    ///
    /// Used in spreadsheet-like interfaces.
    Cell,

    // ========================================================================
    // Resize cursors (edges)
    // ========================================================================
    /// A cursor indicating upward resize.
    ResizeUp,

    /// A cursor indicating downward resize.
    ResizeDown,

    /// A cursor indicating leftward resize.
    ResizeLeft,

    /// A cursor indicating rightward resize.
    ResizeRight,

    // ========================================================================
    // Resize cursors (corners)
    // ========================================================================
    /// A cursor indicating resize toward upper-left.
    ResizeUpLeft,

    /// A cursor indicating resize toward upper-right.
    ResizeUpRight,

    /// A cursor indicating resize toward lower-left.
    ResizeDownLeft,

    /// A cursor indicating resize toward lower-right.
    ResizeDownRight,

    // ========================================================================
    // Resize cursors (bidirectional)
    // ========================================================================
    /// A cursor indicating vertical resize (up or down).
    ResizeUpDown,

    /// A cursor indicating horizontal resize (left or right).
    ResizeLeftRight,

    /// A cursor indicating resize along the diagonal (NW-SE).
    ResizeUpLeftDownRight,

    /// A cursor indicating resize along the diagonal (NE-SW).
    ResizeUpRightDownLeft,

    /// A cursor indicating column resize (horizontal splitter).
    ResizeColumn,

    /// A cursor indicating row resize (vertical splitter).
    ResizeRow,

    // ========================================================================
    // Zoom cursors
    // ========================================================================
    /// A cursor indicating zoom in.
    ZoomIn,

    /// A cursor indicating zoom out.
    ZoomOut,

    // ========================================================================
    // Help cursor
    // ========================================================================
    /// A cursor indicating help is available.
    ///
    /// Arrow with question mark.
    Help,
}

impl SystemMouseCursor {
    /// Returns the debug name for this cursor.
    pub const fn debug_name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Basic => "basic",
            Self::Click => "click",
            Self::Wait => "wait",
            Self::Progress => "progress",
            Self::Forbidden => "forbidden",
            Self::Text => "text",
            Self::VerticalText => "verticalText",
            Self::Precise => "precise",
            Self::ContextMenu => "contextMenu",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::Move => "move",
            Self::AllScroll => "allScroll",
            Self::Copy => "copy",
            Self::Alias => "alias",
            Self::NoDrop => "noDrop",
            Self::Cell => "cell",
            Self::ResizeUp => "resizeUp",
            Self::ResizeDown => "resizeDown",
            Self::ResizeLeft => "resizeLeft",
            Self::ResizeRight => "resizeRight",
            Self::ResizeUpLeft => "resizeUpLeft",
            Self::ResizeUpRight => "resizeUpRight",
            Self::ResizeDownLeft => "resizeDownLeft",
            Self::ResizeDownRight => "resizeDownRight",
            Self::ResizeUpDown => "resizeUpDown",
            Self::ResizeLeftRight => "resizeLeftRight",
            Self::ResizeUpLeftDownRight => "resizeUpLeftDownRight",
            Self::ResizeUpRightDownLeft => "resizeUpRightDownLeft",
            Self::ResizeColumn => "resizeColumn",
            Self::ResizeRow => "resizeRow",
            Self::ZoomIn => "zoomIn",
            Self::ZoomOut => "zoomOut",
            Self::Help => "help",
        }
    }

    /// Returns the CSS cursor name for web platform.
    pub const fn css_name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Basic => "default",
            Self::Click => "pointer",
            Self::Wait => "wait",
            Self::Progress => "progress",
            Self::Forbidden => "not-allowed",
            Self::Text => "text",
            Self::VerticalText => "vertical-text",
            Self::Precise => "crosshair",
            Self::ContextMenu => "context-menu",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::Move => "move",
            Self::AllScroll => "all-scroll",
            Self::Copy => "copy",
            Self::Alias => "alias",
            Self::NoDrop => "no-drop",
            Self::Cell => "cell",
            Self::ResizeUp => "n-resize",
            Self::ResizeDown => "s-resize",
            Self::ResizeLeft => "w-resize",
            Self::ResizeRight => "e-resize",
            Self::ResizeUpLeft => "nw-resize",
            Self::ResizeUpRight => "ne-resize",
            Self::ResizeDownLeft => "sw-resize",
            Self::ResizeDownRight => "se-resize",
            Self::ResizeUpDown => "ns-resize",
            Self::ResizeLeftRight => "ew-resize",
            Self::ResizeUpLeftDownRight => "nwse-resize",
            Self::ResizeUpRightDownLeft => "nesw-resize",
            Self::ResizeColumn => "col-resize",
            Self::ResizeRow => "row-resize",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::Help => "help",
        }
    }

    /// Returns the Windows cursor ID (IDC_*).
    #[cfg(target_os = "windows")]
    pub const fn windows_cursor_id(&self) -> u32 {
        // Windows cursor IDs from winuser.h
        const IDC_ARROW: u32 = 32512;
        const IDC_IBEAM: u32 = 32513;
        const IDC_WAIT: u32 = 32514;
        const IDC_CROSS: u32 = 32515;
        const IDC_UPARROW: u32 = 32516;
        const IDC_SIZENWSE: u32 = 32642;
        const IDC_SIZENESW: u32 = 32643;
        const IDC_SIZEWE: u32 = 32644;
        const IDC_SIZENS: u32 = 32645;
        const IDC_SIZEALL: u32 = 32646;
        const IDC_NO: u32 = 32648;
        const IDC_HAND: u32 = 32649;
        const IDC_APPSTARTING: u32 = 32650;
        const IDC_HELP: u32 = 32651;

        match self {
            Self::None => 0,
            Self::Basic => IDC_ARROW,
            Self::Click => IDC_HAND,
            Self::Wait => IDC_WAIT,
            Self::Progress => IDC_APPSTARTING,
            Self::Forbidden | Self::NoDrop => IDC_NO,
            Self::Text | Self::VerticalText => IDC_IBEAM,
            Self::Precise => IDC_CROSS,
            Self::ContextMenu => IDC_ARROW,
            Self::Grab | Self::Grabbing => IDC_HAND,
            Self::Move | Self::AllScroll => IDC_SIZEALL,
            Self::Copy | Self::Alias => IDC_ARROW,
            Self::Cell => IDC_CROSS,
            Self::ResizeUp | Self::ResizeDown | Self::ResizeUpDown => IDC_SIZENS,
            Self::ResizeLeft | Self::ResizeRight | Self::ResizeLeftRight | Self::ResizeColumn => {
                IDC_SIZEWE
            }
            Self::ResizeUpLeft | Self::ResizeDownRight | Self::ResizeUpLeftDownRight => {
                IDC_SIZENWSE
            }
            Self::ResizeUpRight | Self::ResizeDownLeft | Self::ResizeUpRightDownLeft => {
                IDC_SIZENESW
            }
            Self::ResizeRow => IDC_SIZENS,
            Self::ZoomIn | Self::ZoomOut => IDC_ARROW,
            Self::Help => IDC_HELP,
        }
    }
}

/// Static cursor constants for convenient access.
///
/// This mirrors Flutter's `SystemMouseCursors` class.
///
/// # Example
///
/// ```rust
/// use flui_types::events::SystemMouseCursors;
///
/// let cursor = SystemMouseCursors::CLICK;
/// assert_eq!(cursor.debug_name(), "click");
/// ```
pub struct SystemMouseCursors;

impl SystemMouseCursors {
    /// No cursor displayed.
    pub const NONE: SystemMouseCursor = SystemMouseCursor::None;

    /// Basic arrow cursor.
    pub const BASIC: SystemMouseCursor = SystemMouseCursor::Basic;

    /// Click/pointer cursor.
    pub const CLICK: SystemMouseCursor = SystemMouseCursor::Click;

    /// Wait/busy cursor.
    pub const WAIT: SystemMouseCursor = SystemMouseCursor::Wait;

    /// Progress cursor.
    pub const PROGRESS: SystemMouseCursor = SystemMouseCursor::Progress;

    /// Forbidden cursor.
    pub const FORBIDDEN: SystemMouseCursor = SystemMouseCursor::Forbidden;

    /// Text cursor.
    pub const TEXT: SystemMouseCursor = SystemMouseCursor::Text;

    /// Vertical text cursor.
    pub const VERTICAL_TEXT: SystemMouseCursor = SystemMouseCursor::VerticalText;

    /// Precise/crosshair cursor.
    pub const PRECISE: SystemMouseCursor = SystemMouseCursor::Precise;

    /// Context menu cursor.
    pub const CONTEXT_MENU: SystemMouseCursor = SystemMouseCursor::ContextMenu;

    /// Grab cursor.
    pub const GRAB: SystemMouseCursor = SystemMouseCursor::Grab;

    /// Grabbing cursor.
    pub const GRABBING: SystemMouseCursor = SystemMouseCursor::Grabbing;

    /// Move cursor.
    pub const MOVE: SystemMouseCursor = SystemMouseCursor::Move;

    /// All-scroll cursor.
    pub const ALL_SCROLL: SystemMouseCursor = SystemMouseCursor::AllScroll;

    /// Copy cursor.
    pub const COPY: SystemMouseCursor = SystemMouseCursor::Copy;

    /// Alias cursor.
    pub const ALIAS: SystemMouseCursor = SystemMouseCursor::Alias;

    /// No-drop cursor.
    pub const NO_DROP: SystemMouseCursor = SystemMouseCursor::NoDrop;

    /// Cell cursor.
    pub const CELL: SystemMouseCursor = SystemMouseCursor::Cell;

    /// Resize up cursor.
    pub const RESIZE_UP: SystemMouseCursor = SystemMouseCursor::ResizeUp;

    /// Resize down cursor.
    pub const RESIZE_DOWN: SystemMouseCursor = SystemMouseCursor::ResizeDown;

    /// Resize left cursor.
    pub const RESIZE_LEFT: SystemMouseCursor = SystemMouseCursor::ResizeLeft;

    /// Resize right cursor.
    pub const RESIZE_RIGHT: SystemMouseCursor = SystemMouseCursor::ResizeRight;

    /// Resize up-left cursor.
    pub const RESIZE_UP_LEFT: SystemMouseCursor = SystemMouseCursor::ResizeUpLeft;

    /// Resize up-right cursor.
    pub const RESIZE_UP_RIGHT: SystemMouseCursor = SystemMouseCursor::ResizeUpRight;

    /// Resize down-left cursor.
    pub const RESIZE_DOWN_LEFT: SystemMouseCursor = SystemMouseCursor::ResizeDownLeft;

    /// Resize down-right cursor.
    pub const RESIZE_DOWN_RIGHT: SystemMouseCursor = SystemMouseCursor::ResizeDownRight;

    /// Resize up-down cursor.
    pub const RESIZE_UP_DOWN: SystemMouseCursor = SystemMouseCursor::ResizeUpDown;

    /// Resize left-right cursor.
    pub const RESIZE_LEFT_RIGHT: SystemMouseCursor = SystemMouseCursor::ResizeLeftRight;

    /// Resize diagonal (NW-SE) cursor.
    pub const RESIZE_UP_LEFT_DOWN_RIGHT: SystemMouseCursor =
        SystemMouseCursor::ResizeUpLeftDownRight;

    /// Resize diagonal (NE-SW) cursor.
    pub const RESIZE_UP_RIGHT_DOWN_LEFT: SystemMouseCursor =
        SystemMouseCursor::ResizeUpRightDownLeft;

    /// Resize column cursor.
    pub const RESIZE_COLUMN: SystemMouseCursor = SystemMouseCursor::ResizeColumn;

    /// Resize row cursor.
    pub const RESIZE_ROW: SystemMouseCursor = SystemMouseCursor::ResizeRow;

    /// Zoom in cursor.
    pub const ZOOM_IN: SystemMouseCursor = SystemMouseCursor::ZoomIn;

    /// Zoom out cursor.
    pub const ZOOM_OUT: SystemMouseCursor = SystemMouseCursor::ZoomOut;

    /// Help cursor.
    pub const HELP: SystemMouseCursor = SystemMouseCursor::Help;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mouse_cursor_default() {
        let cursor = MouseCursor::default();
        assert_eq!(cursor, MouseCursor::BASIC);
        assert!(cursor.is_system());
    }

    #[test]
    fn test_system_mouse_cursor_default() {
        let cursor = SystemMouseCursor::default();
        assert_eq!(cursor, SystemMouseCursor::Basic);
    }

    #[test]
    fn test_mouse_cursor_constants() {
        assert_eq!(
            MouseCursor::CLICK,
            MouseCursor::System(SystemMouseCursor::Click)
        );
        assert_eq!(
            MouseCursor::TEXT,
            MouseCursor::System(SystemMouseCursor::Text)
        );
        assert_eq!(
            MouseCursor::FORBIDDEN,
            MouseCursor::System(SystemMouseCursor::Forbidden)
        );
    }

    #[test]
    fn test_system_mouse_cursors_constants() {
        assert_eq!(SystemMouseCursors::BASIC, SystemMouseCursor::Basic);
        assert_eq!(SystemMouseCursors::CLICK, SystemMouseCursor::Click);
        assert_eq!(SystemMouseCursors::TEXT, SystemMouseCursor::Text);
    }

    #[test]
    fn test_cursor_predicates() {
        assert!(MouseCursor::CLICK.is_clickable());
        assert!(MouseCursor::TEXT.is_text());
        assert!(MouseCursor::FORBIDDEN.is_forbidden());
        assert!(MouseCursor::WAIT.is_busy());
        assert!(MouseCursor::GRAB.is_drag());
        assert!(MouseCursor::System(SystemMouseCursor::ResizeUp).is_resize());
    }

    #[test]
    fn test_cursor_css_names() {
        assert_eq!(SystemMouseCursor::Basic.css_name(), "default");
        assert_eq!(SystemMouseCursor::Click.css_name(), "pointer");
        assert_eq!(SystemMouseCursor::Text.css_name(), "text");
        assert_eq!(SystemMouseCursor::Forbidden.css_name(), "not-allowed");
        assert_eq!(SystemMouseCursor::Grab.css_name(), "grab");
    }

    #[test]
    fn test_cursor_debug_names() {
        assert_eq!(SystemMouseCursor::Basic.debug_name(), "basic");
        assert_eq!(SystemMouseCursor::Click.debug_name(), "click");
        assert_eq!(SystemMouseCursor::ResizeUpDown.debug_name(), "resizeUpDown");
    }

    #[test]
    fn test_defer_cursor() {
        let cursor = MouseCursor::Defer;
        assert!(cursor.is_defer());
        assert!(!cursor.is_system());
        assert!(cursor.as_system().is_none());
    }

    #[test]
    fn test_uncontrolled_cursor() {
        let cursor = MouseCursor::Uncontrolled;
        assert!(cursor.is_uncontrolled());
        assert!(!cursor.is_system());
    }

    #[test]
    fn test_from_system_cursor() {
        let system = SystemMouseCursor::Click;
        let cursor: MouseCursor = system.into();
        assert_eq!(cursor, MouseCursor::System(SystemMouseCursor::Click));
    }

    #[test]
    fn test_as_system() {
        let cursor = MouseCursor::System(SystemMouseCursor::Grab);
        assert_eq!(cursor.as_system(), Some(SystemMouseCursor::Grab));

        let defer = MouseCursor::Defer;
        assert_eq!(defer.as_system(), None);
    }
}
