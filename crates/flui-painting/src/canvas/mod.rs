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
//! The 3,305-LOC `canvas.rs` god module was split into seven
//! concern-based files: this `mod.rs` plus six submodules.
//!
//! - `mod.rs` (this file) -- the `Canvas` struct, lifecycle (`new`,
//!   `finish`, `reset`, `clear_commands`), queries (`is_empty`,
//!   `len`, `bounds`, `display_list`), and `AsRef<DisplayList>`.
//! - [`state`]       -- `CanvasState`, `ClipShape`, save/restore/save_layer.
//! - [`transform`]   -- translate/scale/rotate/skew/transform.
//! - [`clipping`]    -- clip_rect/clip_rrect/clip_path + bounds queries.
//! - [`drawing`]     -- 29 primary `draw_*` methods (one per DrawCommand variant).
//! - [`scoped`]      -- 12 `with_*` closure-based scoped helpers.
//! - [`composition`] -- extend_from/extend/merge/append_* multi-canvas ops.

use std::sync::Arc;

use flui_types::geometry::{Matrix4, Pixels, Rect};

use crate::display_list::{DisplayList, DisplayListCore, Paint};

pub mod clipping;
pub mod composition;
pub mod drawing;
pub mod scoped;
pub mod state;
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

    /// Per-recording paint interning pool.
    ///
    /// Each `draw_*` call goes through [`Self::intern_paint`], which
    /// linearly scans this pool for an existing `Arc<Paint>` whose
    /// inner `Paint` is structurally equal (full equality including
    /// shader — see [`paints_equal`]). On hit, the call returns an
    /// `Arc::clone` of the existing entry (refcount bump). On miss,
    /// it inserts a freshly-allocated `Arc::new(paint.clone())` and
    /// returns its clone.
    ///
    /// # Why a linear `Vec` instead of a `HashMap`
    ///
    /// `Paint` carries `f32` fields and a non-`Hash` `Shader` payload;
    /// a `HashMap` would need a bespoke `PaintKey` wrapper that hashes
    /// f32 bit patterns and walks the shader enum. Most realistic
    /// canvases use a handful of distinct paints (1–8 typical, 32
    /// worst case) — linear search across that small window is
    /// faster than a hash + miss-rate-dependent collision chase and
    /// keeps the code free of `unsafe` and bit-pattern foot-guns.
    ///
    /// # Lifetime
    ///
    /// The pool lives for the duration of one `Canvas` instance.
    /// `reset()` clears it; `clear_commands()` deliberately does
    /// *not* — pre-reset paints stay live as long as the consumer
    /// keeps the canvas around for re-recording.
    pub(crate) paint_pool: Vec<Arc<Paint>>,
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
            paint_pool: Vec::new(),
        }
    }

    /// Returns an `Arc<Paint>` from the per-canvas interning pool,
    /// inserting a fresh allocation only on a cache miss.
    ///
    /// See [`Self::paint_pool`] for the rationale behind the
    /// linear-scan strategy.
    pub(crate) fn intern_paint(&mut self, paint: &Paint) -> Arc<Paint> {
        for existing in &self.paint_pool {
            if paints_equal(existing, paint) {
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
        self.paint_pool.clear();
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

/// Structural equality for two paints, including the optional
/// [`Shader`] (which the public [`Paint::eq`] deliberately skips for
/// internal-state-comparison reasons — see the note on the impl in
/// `flui_types::painting::paint`).
///
/// The interning pool needs shader-sensitive equality because two
/// paints that differ *only* in shader must produce two distinct
/// `Arc<Paint>` entries. Without this stricter check, a freshly
/// drawn gradient-shaded rectangle would silently share an `Arc`
/// with an earlier solid-coloured rectangle and render with the
/// wrong fill.
///
/// [`Shader`]: crate::display_list::Shader
#[inline]
fn paints_equal(a: &Paint, b: &Paint) -> bool {
    // `Paint::eq` already compares every non-shader field. Layer the
    // shader equality on top — `Shader: PartialEq` is derived
    // structurally over its variants and their f32 payloads, which
    // is the correct identity here.
    a == b && a.shader == b.shader
}
