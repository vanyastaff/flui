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

use flui_types::geometry::{Matrix4, Offset, Pixels, px};

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
    ///
    /// The implementation clones the source list and rewrites every
    /// command's baked-in transform via
    /// [`DisplayList::apply_transform`] with a translation matching
    /// `offset` before appending. Without this rewrite the appended
    /// commands keep their original transforms (recorded against the
    /// child canvas's origin) and the `offset` argument silently
    /// drops on the floor; `Canvas::translate` only mutates the
    /// canvas's *current* transform for *future* recorded commands,
    /// not for ones that came in through `append`.
    ///
    /// # Performance
    ///
    /// O(N) clone + O(N) transform-rewrite, where N = `display_list.len()`.
    /// For the zero-offset shortcut we still pay one clone (necessary
    /// because the input is `&DisplayList`).
    pub fn append_display_list_at_offset(
        &mut self,
        display_list: &DisplayList,
        offset: Offset<Pixels>,
    ) {
        if offset.dx == px(0.0) && offset.dy == px(0.0) {
            self.display_list.append(display_list.clone());
            return;
        }

        let mut shifted = display_list.clone();
        shifted.apply_transform(Matrix4::translation(offset.dx.0, offset.dy.0, 0.0));
        self.display_list.append(shifted);
    }

    /// Appends a cached `DisplayList` directly (no offset).
    pub fn append_display_list(&mut self, display_list: DisplayList) {
        self.display_list.append(display_list);
    }
}
