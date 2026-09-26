//! The plain-text clipboard capability.
//!
//! A backend hands out its clipboard through `flui_platform::Platform::clipboard`
//! (ADR-0038 §9); the rich, typed transport is [`crate::data_transfer`].

/// Clipboard operations
pub trait Clipboard: Send + Sync {
    /// Read text from clipboard
    fn read_text(&self) -> Option<String>;

    /// Write text to clipboard
    fn write_text(&self, text: String);

    /// Check if clipboard has text
    fn has_text(&self) -> bool {
        self.read_text().is_some()
    }
}

/// Rich clipboard item with text content and optional metadata
///
/// Wraps clipboard content for cross-platform exchange. Currently supports
/// plain text; future versions will add images and custom MIME types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardItem {
    /// Plain text content
    text: Option<String>,
    /// Optional metadata (e.g., source application, MIME type hints)
    metadata: Option<String>,
}

impl ClipboardItem {
    /// Create a clipboard item from plain text
    pub fn text(content: String) -> Self {
        Self {
            text: Some(content),
            metadata: None,
        }
    }

    /// Create a clipboard item with text and metadata
    pub fn with_metadata(content: String, metadata: String) -> Self {
        Self {
            text: Some(content),
            metadata: Some(metadata),
        }
    }

    /// Get the text content, if any
    pub fn text_content(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Get the metadata, if any
    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }
}
