//! Canvas composition: multi-canvas merge and static constructors.
//!
//! Mythos chain U4 extracted these from the 3,305-LOC `canvas.rs` god
//! module. Composition is the seam between parent and child render
//! objects: a parent records its content, gets the children's
//! `Canvas`es, and merges them in via `extend_from`/`merge`/`extend`.
//!
//! The first child append is O(1) (`Vec::mem::swap` underneath via
//! `DisplayList::append`); subsequent appends are O(N) where N is the
//! child's command count.

use flui_types::geometry::{Offset, Pixels, px};

use super::Canvas;
use crate::display_list::{DisplayList, DisplayListCore};

impl Canvas {
    /// Extends this canvas with all commands from another canvas.
    ///
    /// Takes ownership of the child canvas and moves all its commands
    /// into this canvas.
    ///
    /// # Performance
    ///
    /// - O(1) if self is empty (vector swap).
    /// - O(N) otherwise where N = `other.len()` (move, no clone).
    #[tracing::instrument(skip(self, other), fields(
        parent_commands = self.display_list.len(),
        child_commands = other.display_list.len(),
    ))]
    pub fn extend_from(&mut self, other: Canvas) {
        let child_count = other.display_list.len();

        self.display_list.append(other.display_list);

        tracing::debug!(
            total_commands = self.display_list.len(),
            appended = child_count,
            "Canvas composition complete"
        );
    }

    /// Extends this canvas from multiple canvases.
    ///
    /// Efficiently appends commands from multiple child canvases in
    /// order. Useful for multi-child render objects like Column, Row,
    /// Stack.
    pub fn extend(&mut self, others: impl IntoIterator<Item = Canvas>) {
        for canvas in others {
            self.extend_from(canvas);
        }
    }

    /// Merges two canvases into a new canvas.
    ///
    /// Unlike `extend_from` which modifies `self`, this creates a new
    /// canvas containing commands from both canvases.
    pub fn merge(mut self, other: Canvas) -> Self {
        self.extend_from(other);
        self
    }

    /// Appends a cached `DisplayList` at a given offset.
    ///
    /// Used by layer caching (RepaintBoundary) to replay cached
    /// drawing commands at a specified offset.
    pub fn append_display_list_at_offset(
        &mut self,
        display_list: &DisplayList,
        offset: Offset<Pixels>,
    ) {
        if offset.dx == px(0.0) && offset.dy == px(0.0) {
            self.display_list.append(display_list.clone());
            return;
        }

        self.save();
        self.translate(offset.dx.0, offset.dy.0);
        self.display_list.append(display_list.clone());
        self.restore();
    }

    /// Appends a cached `DisplayList` directly (no offset).
    pub fn append_display_list(&mut self, display_list: DisplayList) {
        self.display_list.append(display_list);
    }

    // ===== Static Constructors =====

    /// Creates a new Canvas, executes a closure on it, and returns the
    /// finished `DisplayList`.
    ///
    /// Useful for creating isolated drawing contexts.
    #[inline]
    pub fn record<F>(f: F) -> DisplayList
    where
        F: FnOnce(&mut Canvas),
    {
        let mut canvas = Canvas::new();
        f(&mut canvas);
        canvas.finish()
    }

    /// Builds a Canvas using a closure and returns it (not consumed).
    #[inline]
    pub fn build<F>(f: F) -> Self
    where
        F: FnOnce(&mut Canvas),
    {
        let mut canvas = Canvas::new();
        f(&mut canvas);
        canvas
    }
}
