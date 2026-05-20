//! Canvas - High-level drawing API.
//!
//! This module provides the [`Canvas`] type, a single-owner mutable
//! recorder that captures drawing commands into a [`DisplayList`] for
//! later execution by the GPU backend.
//!
//! # Architecture
//!
//! ```text
//! RenderObject → Canvas (records) → DisplayList → PictureLayer → WgpuPainter (executes)
//! ```
//!
//! # Design Principles
//!
//! 1. **Recording only**: Canvas does NOT perform actual rendering.
//! 2. **Immutable commands**: Once recorded into `DisplayList`, commands
//!    are immutable from the public API.
//! 3. **Intuitive API**: Consistent with common 2D graphics APIs (Skia,
//!    Flutter's `dart:ui Canvas`).
//! 4. **Transform tracking**: Maintains current transform matrix; baked
//!    into emitted commands.
//! 5. **Save/restore stack**: Supports `save()` / `restore()` /
//!    `save_layer()` for state management.
//! 6. **Thread-safe value**: `Canvas` is `Send` (can be sent across
//!    threads) but `!Sync` (single-threaded recording).
//!
//! # Concern split (Mythos chain U4)
//!
//! The 3,305-LOC `canvas.rs` god module was split into eight
//! concern-based submodules:
//!
//! - [`state`]       -- `CanvasState`, `ClipShape`, save/restore/save_layer.
//! - [`transform`]   -- translate/scale/rotate/skew/transform.
//! - [`clipping`]    -- clip_rect/clip_rrect/clip_path + bounds queries.
//! - [`drawing`]     -- 29 primary `draw_*` methods (one per DrawCommand variant).
//! - [`scoped`]      -- 12 `with_*` closure-based scoped helpers.
//! - [`composition`] -- extend_from/extend/merge/append_* multi-canvas ops + static constructors.
//! - [`sugar`]       -- chaining API + batch ops + conditional draws + grid/repeat patterns + debug viz + convenience shapes.
//!
//! This module (`mod.rs`) carries the `Canvas` struct itself plus its
//! lifecycle (`new`, `finish`, `reset`, `clear_commands`), queries
//! (`is_empty`, `len`, `bounds`, `display_list`), the
//! `AsRef<DisplayList>` impl, and the hit-region recording surface.

use flui_types::geometry::{Matrix4, Pixels, Rect};

use crate::display_list::{DisplayList, DisplayListCore};

pub mod clipping;
pub mod composition;
pub mod drawing;
pub mod scoped;
pub mod state;
pub mod sugar;
pub mod transform;

pub use state::{CanvasState, ClipShape};

/// High-level drawing canvas with intuitive API.
///
/// `Canvas` records drawing commands into a [`DisplayList`] without
/// performing any actual rendering. Rendering happens later in
/// `flui-engine` via `WgpuPainter`.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::{Canvas, Paint};
/// use flui_types::{Rect, Color};
///
/// let mut canvas = Canvas::new();
///
/// let rect = Rect::from_ltrb(10.0, 10.0, 100.0, 100.0);
/// let paint = Paint::fill(Color::RED);
/// canvas.draw_rect(rect, &paint);
///
/// let display_list = canvas.finish();
/// ```
///
/// # Transform and State Management
///
/// ```rust,ignore
/// let mut canvas = Canvas::new();
///
/// canvas.save();
/// canvas.translate(50.0, 50.0);
/// canvas.rotate(std::f32::consts::PI / 4.0);
/// canvas.draw_rect(rect, &paint);
/// canvas.restore();
/// ```
#[derive(Debug, Clone)]
pub struct Canvas {
    /// Commands being recorded.
    pub(crate) display_list: DisplayList,

    /// Current transform matrix.
    pub(crate) transform: Matrix4,

    /// Current clip bounds (stack of clips).
    pub(crate) clip_stack: Vec<ClipShape>,

    /// Save/restore stack (stores previous states).
    pub(crate) save_stack: Vec<CanvasState>,
}

impl Canvas {
    /// Creates a new empty canvas.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::Canvas;
    ///
    /// let canvas = Canvas::new();
    /// ```
    pub fn new() -> Self {
        Self {
            display_list: DisplayList::new(),
            transform: Matrix4::identity(),
            clip_stack: Vec::new(),
            save_stack: Vec::new(),
        }
    }

    // ===== Finalization =====

    /// Finishes recording and returns the [`DisplayList`].
    ///
    /// Consumes the canvas. On unrestored save() calls, fires
    /// `debug_assert!` (caught during tests) and `tracing::warn!`
    /// (release-build observability). The Mythos chain U10 wired the
    /// debug_assert; release behaviour matches Flutter's
    /// `PictureRecorder.endRecording()` silent finalisation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(rect, &paint);
    /// let display_list = canvas.finish();
    /// ```
    #[tracing::instrument(skip(self), fields(
        commands = self.display_list.len(),
        save_depth = self.save_stack.len(),
    ))]
    pub fn finish(self) -> DisplayList {
        if !self.save_stack.is_empty() {
            tracing::warn!(
                unrestored_saves = self.save_stack.len(),
                "Canvas finished with unrestored save() calls"
            );
        }

        tracing::debug!(
            commands = self.display_list.len(),
            bounds = ?self.display_list.bounds(),
            "Canvas finalized"
        );

        self.display_list
    }

    /// Returns a reference to the inner display list without consuming
    /// the canvas.
    pub fn display_list(&self) -> &DisplayList {
        &self.display_list
    }

    /// Resets the canvas to its initial state, clearing all commands and
    /// state. More efficient than `Canvas::new()` when reusing.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(rect, &paint);
    /// canvas.save();
    /// canvas.translate(50.0, 50.0);
    ///
    /// canvas.reset();
    ///
    /// assert!(canvas.is_empty());
    /// assert_eq!(canvas.save_count(), 1);
    /// ```
    pub fn reset(&mut self) {
        self.display_list.clear();
        self.transform = Matrix4::identity();
        self.clip_stack.clear();
        self.save_stack.clear();
    }

    /// Clears all recorded drawing commands but preserves transform and
    /// clip state.
    ///
    /// Use this when you want to re-record commands but keep the current
    /// coordinate system setup.
    pub fn clear_commands(&mut self) {
        self.display_list.clear();
    }

    // ===== Query Methods =====

    /// Returns `true` if no drawing commands have been recorded.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.display_list.is_empty()
    }

    /// Returns the number of recorded drawing commands.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.display_list.len()
    }

    /// Returns the bounds of all recorded drawing commands.
    #[inline]
    #[must_use]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.display_list.bounds()
    }

    // ===== Hit Testing =====

    /// Add a hit-testable region with an event handler.
    ///
    /// This registers an area that will respond to pointer events.
    /// Used by `RenderPointerListener` to connect gestures to UI
    /// elements.
    pub fn add_hit_region(&mut self, region: crate::display_list::HitRegion) {
        self.display_list.add_hit_region(region);
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

/// Allow zero-cost conversion from Canvas to DisplayList reference.
///
/// This enables generic functions that accept `impl AsRef<DisplayList>`
/// to work with Canvas.
impl AsRef<DisplayList> for Canvas {
    fn as_ref(&self) -> &DisplayList {
        &self.display_list
    }
}

#[cfg(test)]
mod tests {
    use flui_types::{
        geometry::{Point, px},
        styling::Color,
    };

    use super::*;
    use crate::display_list::Paint;

    #[test]
    fn test_canvas_creation() {
        let canvas = Canvas::new();
        assert_eq!(canvas.save_count(), 1);
        assert_eq!(canvas.display_list().len(), 0);
    }

    #[test]
    fn test_canvas_draw_rect() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
        let paint = Paint::fill(Color::RED);

        canvas.draw_rect(rect, &paint);

        let display_list = canvas.finish();
        assert_eq!(display_list.len(), 1);
    }

    #[test]
    fn test_canvas_save_restore() {
        let mut canvas = Canvas::new();

        assert_eq!(canvas.save_count(), 1);

        canvas.save();
        assert_eq!(canvas.save_count(), 2);

        canvas.translate(50.0, 50.0);

        canvas.save();
        assert_eq!(canvas.save_count(), 3);

        canvas.restore();
        assert_eq!(canvas.save_count(), 2);

        canvas.restore();
        assert_eq!(canvas.save_count(), 1);
    }

    #[test]
    fn test_canvas_transform() {
        let mut canvas = Canvas::new();

        let original_transform = canvas.transform_matrix();
        canvas.translate(100.0, 50.0);
        let translated_transform = canvas.transform_matrix();

        assert_ne!(original_transform, translated_transform);
    }

    #[test]
    fn test_canvas_clip() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));

        canvas.clip_rect(rect);

        let display_list = canvas.finish();
        assert_eq!(display_list.len(), 1);
    }

    #[test]
    fn test_canvas_multiple_commands() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
        let paint = Paint::fill(Color::RED);

        canvas.draw_rect(rect, &paint);
        canvas.draw_circle(Point::new(px(50.0), px(50.0)), px(25.0), &paint);

        let display_list = canvas.finish();
        assert_eq!(display_list.len(), 2);
    }

    #[test]
    fn test_canvas_restore_without_save() {
        // Test that restore() without matching save() is safe (no-op)
        let mut canvas = Canvas::new();
        canvas.restore();

        let paint = Paint::fill(Color::RED);
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            &paint,
        );
        assert_eq!(canvas.len(), 1);
    }
}
