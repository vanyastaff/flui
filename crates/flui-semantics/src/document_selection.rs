//! A selection that spans several texts of one document (ADR-0214): the
//! selection a `SelectionArea` holds over the paragraphs it registered.

use crate::SemanticsAnchor;

/// A place in one of a document's texts: the text, named by the anchor its
/// node declares, and a UTF-8 byte offset into that text.
///
/// The byte is not checked here. Publication snaps it down to a character
/// boundary inside the text, and a position whose anchor names no current
/// text is dropped with the selection it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentPosition {
    text: SemanticsAnchor,
    byte: usize,
}

impl DocumentPosition {
    /// The position `byte` bytes into the text `text` names.
    #[must_use]
    pub fn new(text: SemanticsAnchor, byte: usize) -> Self {
        Self { text, byte }
    }

    /// The anchor of the text this position is in.
    #[must_use]
    pub fn text(&self) -> &SemanticsAnchor {
        &self.text
    }

    /// The byte offset into that text.
    #[must_use]
    pub fn byte(&self) -> usize {
        self.byte
    }
}

/// A selection across a document's texts: it starts at `anchor`, which stays
/// put as it grows, and ends at `focus`. Every text between the two ends, in
/// the document's reading order, is selected whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSelection {
    anchor: DocumentPosition,
    focus: DocumentPosition,
}

impl DocumentSelection {
    /// The selection from `anchor` to `focus`.
    #[must_use]
    pub fn new(anchor: DocumentPosition, focus: DocumentPosition) -> Self {
        Self { anchor, focus }
    }

    /// A collapsed selection: a caret at `at`.
    #[must_use]
    pub fn collapsed(at: DocumentPosition) -> Self {
        Self {
            anchor: at.clone(),
            focus: at,
        }
    }

    /// Where the selection starts.
    #[must_use]
    pub fn anchor(&self) -> &DocumentPosition {
        &self.anchor
    }

    /// Where the selection ends.
    #[must_use]
    pub fn focus(&self) -> &DocumentPosition {
        &self.focus
    }
}
