//! Canvas composition: multi-canvas merge and static constructors.
//!
//! These were extracted from the 3,305-LOC `canvas.rs` god
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
    /// drawing commands at a specified offset. The appended commands
    /// carry the offset baked into their per-command transforms, so
    /// the replayed picture renders at the new position.
    ///
    /// # Why offset must be baked into per-command transforms
    ///
    /// `Canvas::translate` only mutates the canvas's *current*
    /// transform for *future* recorded commands; commands that come
    /// in through `append` keep the transform they were recorded
    /// with. Without baking the offset into each appended command's
    /// transform, the `offset` argument would silently drop on the
    /// floor.
    ///
    /// # Performance
    ///
    /// The previous shape was `let mut shifted = dl.clone();
    /// shifted.apply_transform(...); self.display_list.append(shifted)` —
    /// three O(N) passes (deep-clone the inner `Vec<DrawCommand>`,
    /// rewrite every command's transform AND recompute bounds of
    /// the throw-away intermediate, then move the vec into self).
    /// The current shape walks the source once, clones each command
    /// while applying the translation, and pushes directly into self;
    /// `DisplayList::push` maintains the running bounds union
    /// incrementally, so the throw-away `recalculate_bounds` pass is
    /// gone and the temporary `DisplayList` allocation is gone.
    /// Net: O(N) with one clone per command (down from clone, rewrite,
    /// move, bounds-recompute on the intermediate) and one fewer heap
    /// allocation (the intermediate `DisplayList`).
    ///
    /// No new `DrawCommand` opcode is introduced — `DrawCommand` has
    /// no pure `Save`/`Concat`/`Restore` triple (only `SaveLayer` /
    /// `RestoreLayer`, which allocate a real offscreen buffer and
    /// would be far heavier than per-command transform baking).
    ///
    /// For the zero-offset shortcut we fall through to the
    /// `DisplayList::append` move path (no per-command rewrite
    /// needed); `DisplayList::clone` is still required because the
    /// input is `&DisplayList`.
    pub fn append_display_list_at_offset(
        &mut self,
        display_list: &DisplayList,
        offset: Offset<Pixels>,
    ) {
        if offset.dx == px(0.0) && offset.dy == px(0.0) {
            self.display_list.append(display_list.clone());
            return;
        }

        let translation = Matrix4::translation(offset.dx.0, offset.dy.0, 0.0);
        for cmd in display_list {
            let mut shifted = cmd.clone();
            shifted.apply_transform(translation);
            self.display_list.push(shifted);
        }
    }

    /// Appends a cached `DisplayList` directly (no offset).
    pub fn append_display_list(&mut self, display_list: DisplayList) {
        self.display_list.append(display_list);
    }
}
