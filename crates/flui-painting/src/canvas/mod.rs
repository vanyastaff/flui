//! `Canvas` — records drawing commands into a [`DisplayList`].
//!
//! The surface is `dart:ui`'s `Canvas` (the contract a `CustomPaint`
//! painter is written against): every command is recorded with the current
//! transform baked in, clips are scoped by `save`/`restore`, and nothing is
//! rasterised here — `flui-engine` replays the list.
//!
//! - [`state`]     — save/restore/save_layer.
//! - [`transform`] — translate/scale/rotate/skew/transform.
//! - [`clipping`]  — clip_rect/clip_rrect/clip_rsuperellipse/clip_path.
//! - [`drawing`]   — the `draw_*` methods, one per `DrawOp` variant.
//! - [`scoped`]    — `with_*` helpers that pair a `save` with its `restore`.

use std::sync::Arc;

use flui_types::geometry::{Matrix4, Pixels, Rect};

use crate::display_list::{DisplayList, DrawCommand, DrawOp, Paint};

pub mod clipping;
pub mod drawing;
pub mod scoped;
pub mod state;
pub mod transform;

pub(crate) use state::CanvasState;

/// Records drawing commands into a [`DisplayList`]; `flui-engine` replays
/// them.
///
/// ```rust
/// use flui_painting::{Canvas, Paint};
/// use flui_types::{Rect, geometry::px, styling::Color};
///
/// let mut canvas = Canvas::new();
/// canvas.save();
/// canvas.translate(50.0, 50.0);
/// canvas.rotate(std::f32::consts::FRAC_PI_4);
/// canvas.draw_rect(
///     Rect::from_ltrb(px(10.0), px(10.0), px(100.0), px(100.0)),
///     &Paint::fill(Color::RED),
/// );
/// canvas.restore();
/// let display_list = canvas.finish();
/// assert_eq!(display_list.len(), 3);
/// ```
#[derive(Debug, Clone)]
pub struct Canvas {
    /// Commands being recorded.
    pub(crate) display_list: DisplayList,

    /// Current transform matrix.
    pub(crate) transform: Matrix4,

    /// Save/restore stack (stores previous states).
    pub(crate) save_stack: Vec<CanvasState>,

    /// Per-recording paint interning pool.
    ///
    /// Equal paints share an allocation. `Paint` is not `Hash`, so lookup
    /// uses equality over this vector. `reset()` clears the pool.
    pub(crate) paint_pool: Vec<Arc<Paint>>,
}

impl Canvas {
    /// Creates a new empty canvas.
    pub fn new() -> Self {
        Self {
            display_list: DisplayList::new(),
            transform: Matrix4::identity(),
            save_stack: Vec::new(),
            paint_pool: Vec::new(),
        }
    }

    /// Records `op` under the current transform.
    #[inline]
    pub(crate) fn record(&mut self, op: DrawOp) {
        self.display_list.push(DrawCommand {
            transform: self.transform,
            op,
        });
    }

    /// Returns an `Arc<Paint>` from the per-canvas interning pool,
    /// inserting a fresh allocation only on a cache miss.
    pub(crate) fn intern_paint(&mut self, paint: &Paint) -> Arc<Paint> {
        for existing in &self.paint_pool {
            if **existing == *paint {
                return Arc::clone(existing);
            }
        }
        let arc = Arc::new(paint.clone());
        self.paint_pool.push(Arc::clone(&arc));
        arc
    }

    /// Returns an `Option<Arc<Paint>>` interned through the canvas
    /// pool, mirroring the `Option<&Paint>` shape used by the
    /// image-family `draw_*` APIs.
    #[inline]
    pub(crate) fn intern_optional_paint(&mut self, paint: Option<&Paint>) -> Option<Arc<Paint>> {
        paint.map(|p| self.intern_paint(p))
    }

    // ===== Finalization =====

    /// Finishes recording and returns the [`DisplayList`].
    ///
    /// Consumes the canvas. On unrestored save() calls, fires
    /// `debug_assert!` (caught during tests) and `tracing::warn!`
    /// (release-build observability); release behaviour matches Flutter's
    /// `PictureRecorder.endRecording()` silent finalisation.
    #[tracing::instrument(skip(self), fields(
        commands = self.display_list.len(),
        save_depth = self.save_stack.len(),
    ))]
    pub fn finish(self) -> DisplayList {
        debug_assert!(
            self.save_stack.is_empty(),
            "Canvas finished with {} unrestored save() calls",
            self.save_stack.len()
        );

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
    pub fn reset(&mut self) {
        self.display_list.clear();
        self.transform = Matrix4::identity();
        self.save_stack.clear();
        self.paint_pool.clear();
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

    /// The union of every recorded command that contributes bounds, or
    /// `None` when none has yet.
    #[inline]
    #[must_use]
    pub fn bounds(&self) -> Option<Rect<Pixels>> {
        self.display_list.bounds()
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
