//! Value types contract — enums and structs for the platform API surface.
//!
//! Design contract for the implementation phase.

// --- Cursor ---

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorStyle {
    #[default]
    Arrow,
    IBeam,
    Crosshair,
    ClosedHand,
    OpenHand,
    PointingHand,
    ResizeLeft,
    ResizeRight,
    ResizeLeftRight,
    ResizeUp,
    ResizeDown,
    ResizeUpDown,
    ResizeUpLeftDownRight,
    ResizeUpRightDownLeft,
    ResizeColumn,
    ResizeRow,
    OperationNotAllowed,
    DragLink,
    DragCopy,
    ContextualMenu,
    None,
}

// --- Window Appearance ---

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowAppearance {
    #[default]
    Light,
    Dark,
    VibrantLight,
    VibrantDark,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowBackgroundAppearance {
    #[default]
    Opaque,
    Transparent,
    Blurred,
    MicaBackdrop,
    MicaAltBackdrop,
}

// --- Window Bounds ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowBounds {
    Windowed(Bounds<Pixels>),
    Maximized(Bounds<Pixels>),
    Fullscreen(Bounds<Pixels>),
}

// --- Event Result ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchEventResult {
    pub propagate: bool,
    pub default_prevented: bool,
}

impl Default for DispatchEventResult {
    fn default() -> Self {
        Self {
            propagate: true,
            default_prevented: false,
        }
    }
}

// --- Clipboard ---

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardItem {
    pub entries: Vec<ClipboardEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardEntry {
    String(ClipboardString),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardString {
    pub text: String,
    pub metadata: Option<String>,
}

impl ClipboardItem {
    /// Create a simple text clipboard item.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            entries: vec![ClipboardEntry::String(ClipboardString {
                text: text.into(),
                metadata: None,
            })],
        }
    }

    /// Get the first text entry, if any.
    pub fn text_content(&self) -> Option<&str> {
        self.entries.iter().find_map(|e| match e {
            ClipboardEntry::String(s) => Some(s.text.as_str()),
        })
    }
}

// --- File Dialogs ---

#[derive(Clone, Debug)]
pub struct PathPromptOptions {
    pub files: bool,
    pub directories: bool,
    pub multiple: bool,
}

// --- Priority ---

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Priority {
    High,
    #[default]
    Medium,
    Low,
}
