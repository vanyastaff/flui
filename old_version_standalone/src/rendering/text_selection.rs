//! Text selection and cursor management
//!
//! This module provides types for managing text selection,
//! similar to Flutter's TextSelection.

use crate::types::core::{Offset, Rect};
use serde::{Deserialize, Serialize};

/// Represents a selection of text.
///
/// Similar to Flutter's `TextSelection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSelection {
    /// The offset of the base (starting point) of the selection.
    pub base_offset: usize,

    /// The offset of the extent (ending point) of the selection.
    pub extent_offset: usize,

    /// The affinity of the selection.
    pub affinity: TextAffinity,

    /// Whether this selection is directional.
    pub is_directional: bool,
}

/// The affinity of a text selection.
///
/// Similar to Flutter's `TextAffinity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAffinity {
    /// The cursor has affinity for the upstream characters.
    Upstream,
    /// The cursor has affinity for the downstream characters.
    Downstream,
}

impl Default for TextAffinity {
    fn default() -> Self {
        TextAffinity::Downstream
    }
}

impl TextSelection {
    /// Create a new collapsed selection (cursor) at the given offset.
    pub fn collapsed(offset: usize) -> Self {
        Self {
            base_offset: offset,
            extent_offset: offset,
            affinity: TextAffinity::default(),
            is_directional: false,
        }
    }

    /// Create a new selection from base to extent.
    pub fn new(base_offset: usize, extent_offset: usize) -> Self {
        Self {
            base_offset,
            extent_offset,
            affinity: TextAffinity::default(),
            is_directional: true,
        }
    }

    /// Create a selection that spans the entire text.
    pub fn all_text(text_length: usize) -> Self {
        Self::new(0, text_length)
    }

    /// Check if this selection is collapsed (cursor, not a range).
    pub fn is_collapsed(&self) -> bool {
        self.base_offset == self.extent_offset
    }

    /// Check if this selection is not collapsed.
    pub fn is_valid(&self) -> bool {
        !self.is_collapsed()
    }

    /// Get the start offset of the selection (minimum of base and extent).
    pub fn start(&self) -> usize {
        self.base_offset.min(self.extent_offset)
    }

    /// Get the end offset of the selection (maximum of base and extent).
    pub fn end(&self) -> usize {
        self.base_offset.max(self.extent_offset)
    }

    /// Get the length of the selected text.
    pub fn length(&self) -> usize {
        self.end() - self.start()
    }

    /// Check if the selection is reversed (base > extent).
    pub fn is_reversed(&self) -> bool {
        self.base_offset > self.extent_offset
    }

    /// Create a copy of this selection with a different base offset.
    pub fn with_base_offset(mut self, offset: usize) -> Self {
        self.base_offset = offset;
        self
    }

    /// Create a copy of this selection with a different extent offset.
    pub fn with_extent_offset(mut self, offset: usize) -> Self {
        self.extent_offset = offset;
        self
    }

    /// Create a copy of this selection with a different affinity.
    pub fn with_affinity(mut self, affinity: TextAffinity) -> Self {
        self.affinity = affinity;
        self
    }

    /// Expand the selection by the given amount.
    pub fn expand_by(mut self, delta: isize) -> Self {
        if delta > 0 {
            self.extent_offset = self.extent_offset.saturating_add(delta as usize);
        } else {
            self.extent_offset = self.extent_offset.saturating_sub((-delta) as usize);
        }
        self
    }

    /// Move the selection by the given amount.
    pub fn move_by(mut self, delta: isize) -> Self {
        if delta > 0 {
            let d = delta as usize;
            self.base_offset = self.base_offset.saturating_add(d);
            self.extent_offset = self.extent_offset.saturating_add(d);
        } else {
            let d = (-delta) as usize;
            self.base_offset = self.base_offset.saturating_sub(d);
            self.extent_offset = self.extent_offset.saturating_sub(d);
        }
        self
    }

    /// Copy this selection collapsed at the current base offset.
    pub fn collapse_to_base(self) -> Self {
        Self::collapsed(self.base_offset)
    }

    /// Copy this selection collapsed at the current extent offset.
    pub fn collapse_to_extent(self) -> Self {
        Self::collapsed(self.extent_offset)
    }

    /// Copy this selection collapsed at the start.
    pub fn collapse_to_start(self) -> Self {
        Self::collapsed(self.start())
    }

    /// Copy this selection collapsed at the end.
    pub fn collapse_to_end(self) -> Self {
        Self::collapsed(self.end())
    }

    /// Extract the selected text from the given text.
    pub fn extract_from<'a>(&self, text: &'a str) -> &'a str {
        let bytes = text.as_bytes();
        let start = self.start().min(bytes.len());
        let end = self.end().min(bytes.len());

        // Ensure we don't split UTF-8 characters
        let start = Self::round_to_char_boundary(text, start);
        let end = Self::round_to_char_boundary(text, end);

        &text[start..end]
    }

    /// Round an offset to the nearest character boundary.
    fn round_to_char_boundary(text: &str, offset: usize) -> usize {
        if offset >= text.len() {
            return text.len();
        }

        let mut result = offset;
        while result > 0 && !text.is_char_boundary(result) {
            result -= 1;
        }
        result
    }
}

impl Default for TextSelection {
    fn default() -> Self {
        Self::collapsed(0)
    }
}

/// The type of text selection handle.
///
/// Similar to Flutter's `TextSelectionHandleType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSelectionHandleType {
    /// The handle at the start of the selection.
    Left,
    /// The handle at the end of the selection.
    Right,
    /// A single collapsed handle (cursor).
    Collapsed,
}

/// Information about a text selection handle position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSelectionHandlePosition {
    /// The type of handle.
    pub handle_type: TextSelectionHandleType,

    /// The position of the handle.
    pub position: Offset,

    /// The height of the line at this position.
    pub line_height: f32,
}

/// A point in a text selection with associated metadata.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSelectionPoint {
    /// The offset in the text.
    pub offset: usize,

    /// The visual position of this point.
    pub point: Offset,

    /// The bounding box of the character at this offset.
    pub bounds: Option<Rect>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_selection_collapsed() {
        let sel = TextSelection::collapsed(5);
        assert!(sel.is_collapsed());
        assert!(!sel.is_valid());
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 5);
        assert_eq!(sel.length(), 0);
    }

    #[test]
    fn test_text_selection_range() {
        let sel = TextSelection::new(5, 10);
        assert!(!sel.is_collapsed());
        assert!(sel.is_valid());
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 10);
        assert_eq!(sel.length(), 5);
        assert!(!sel.is_reversed());
    }

    #[test]
    fn test_text_selection_reversed() {
        let sel = TextSelection::new(10, 5);
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 10);
        assert!(sel.is_reversed());
    }

    #[test]
    fn test_text_selection_all_text() {
        let sel = TextSelection::all_text(20);
        assert_eq!(sel.start(), 0);
        assert_eq!(sel.end(), 20);
        assert_eq!(sel.length(), 20);
    }

    #[test]
    fn test_text_selection_move() {
        let sel = TextSelection::new(5, 10);

        let moved = sel.move_by(3);
        assert_eq!(moved.start(), 8);
        assert_eq!(moved.end(), 13);

        let moved_back = sel.move_by(-2);
        assert_eq!(moved_back.start(), 3);
        assert_eq!(moved_back.end(), 8);
    }

    #[test]
    fn test_text_selection_expand() {
        let sel = TextSelection::new(5, 10);

        let expanded = sel.expand_by(3);
        assert_eq!(expanded.base_offset, 5);
        assert_eq!(expanded.extent_offset, 13);

        let shrunk = sel.expand_by(-2);
        assert_eq!(shrunk.base_offset, 5);
        assert_eq!(shrunk.extent_offset, 8);
    }

    #[test]
    fn test_text_selection_collapse() {
        let sel = TextSelection::new(5, 10);

        let at_start = sel.collapse_to_start();
        assert_eq!(at_start.base_offset, 5);
        assert_eq!(at_start.extent_offset, 5);

        let at_end = sel.collapse_to_end();
        assert_eq!(at_end.base_offset, 10);
        assert_eq!(at_end.extent_offset, 10);
    }

    #[test]
    fn test_text_selection_extract() {
        let text = "Hello, world!";
        let sel = TextSelection::new(0, 5);

        assert_eq!(sel.extract_from(text), "Hello");

        let sel2 = TextSelection::new(7, 12);
        assert_eq!(sel2.extract_from(text), "world");
    }

    #[test]
    fn test_text_selection_extract_utf8() {
        let text = "Hello 🌍 world!";
        let sel = TextSelection::new(6, 10); // Should include emoji

        let extracted = sel.extract_from(text);
        assert!(extracted.contains('🌍'));
    }

    #[test]
    fn test_text_affinity() {
        assert_eq!(TextAffinity::default(), TextAffinity::Downstream);
    }
}
