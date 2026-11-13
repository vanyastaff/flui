//! `Canvas` — records drawing commands into a [`DisplayList`].
//!
//! This is the surface a `CustomPaint` painter is written against: every
//! command is recorded with the current
//! transform baked in, clips are scoped by `save`/`restore`, and nothing is
//! rasterised here — `flui-engine` replays the list.
//!
//! - [`state`]     — save/restore/save_layer.
//! - [`transform`] — translate/scale/rotate/skew/transform.
//! - [`clipping`]  — clip_rect/clip_rrect/clip_rsuperellipse/clip_path.
//! - [`drawing`]   — the `draw_*` methods, one per `DrawOp` variant.
//! - [`scoped`]    — `with_*` helpers that pair a `save` with its `restore`.

use std::sync::Arc;

use smallvec::SmallVec;

use flui_foundation::geometry::{Matrix4, Rect};

use crate::display_list::{DisplayList, DrawOp, Paint, paint_finite};

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
/// use flui_foundation::geometry::Rect;
/// use flui_painting::styling::Color;
///
/// let mut canvas = Canvas::new();
/// canvas.save();
/// canvas.translate(50.0, 50.0);
/// canvas.rotate(std::f64::consts::FRAC_PI_4);
/// canvas.draw_rect(
///     Rect::from_ltrb(10.0, 10.0, 100.0, 100.0),
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

    /// Per-recording paint interning pool, each paint with whether its values
    /// are finite, checked once when it was interned.
    ///
    /// Equal paints share an allocation. `Paint` is not `Hash`, so lookup
    /// uses equality over this vector. `reset()` clears the pool;
    /// [`Canvas::take_recording`] keeps up to [`KEPT_PAINTS`] of its
    /// shader-free paints for the next recording.
    pub(crate) paint_pool: SmallVec<[InternedPaint; 4]>,
}

/// How many interned paints [`Canvas::take_recording`] keeps for the next
/// recording: the ones interned last, so a canvas that records many runs in
/// turn (one per render object of a pass) allocates each paint it repeats
/// once, and the scan an intern costs stays short.
const KEPT_PAINTS: usize = 4;

/// The most saved states whose room [`Canvas::take_recording`] keeps for the
/// next recording.
const KEPT_SAVES: usize = 32;

/// A paint of the canvas's pool and whether its values are finite.
#[derive(Debug, Clone)]
pub(crate) struct InternedPaint {
    paint: Arc<Paint>,
    finite: bool,
}

impl Canvas {
    /// Creates a new empty canvas.
    pub fn new() -> Self {
        Self {
            display_list: DisplayList::new(),
            transform: Matrix4::identity(),
            save_stack: Vec::new(),
            paint_pool: SmallVec::new(),
        }
    }

    /// Records `op` under the current transform; an op that is not finite, or
    /// a transform that is not, is refused by the list (ADR-0306 §4).
    #[inline]
    pub(crate) fn record(&mut self, op: DrawOp) {
        let pool = &self.paint_pool;
        self.display_list.record(&self.transform, op, |paint| {
            // Every paint an op holds came from the pool; a miss is checked
            // as it stands.
            pool.iter()
                .find(|interned| Arc::ptr_eq(&interned.paint, paint))
                .map_or_else(|| paint_finite(paint), |interned| interned.finite)
        });
    }

    /// Returns an `Arc<Paint>` from the per-canvas interning pool,
    /// inserting a fresh allocation only on a cache miss.
    pub(crate) fn intern_paint(&mut self, paint: &Paint) -> Arc<Paint> {
        for existing in &self.paint_pool {
            if *existing.paint == *paint {
                return Arc::clone(&existing.paint);
            }
        }
        let arc = Arc::new(paint.clone());
        self.paint_pool.push(InternedPaint {
            paint: Arc::clone(&arc),
            finite: paint_finite(paint),
        });
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
    /// (release-build observability); release builds finalise silently.
    #[tracing::instrument(skip(self), fields(
        commands = self.display_list.len(),
        save_depth = self.save_stack.len(),
    ))]
    pub fn finish(self) -> DisplayList {
        self.report_finished();
        self.display_list
    }

    /// Finishes the recording made so far, as [`Self::finish`] does, and
    /// leaves the canvas empty at the identity transform to record the next
    /// one.
    ///
    /// The last few paints it interned that hold no shader stay interned: a
    /// next recording drawing with an equal paint shares its allocation. A
    /// paint with a shader is let go, since a shader can hold a caller's
    /// image whose last drop must not wait for a later recording.
    #[tracing::instrument(skip(self), fields(
        commands = self.display_list.len(),
        save_depth = self.save_stack.len(),
    ))]
    #[must_use = "the recording is returned; dropping it discards what was drawn"]
    pub fn take_recording(&mut self) -> DisplayList {
        self.report_finished();
        let recording = std::mem::take(&mut self.display_list);
        self.transform = Matrix4::identity();
        self.save_stack.clear();
        self.paint_pool
            .retain(|interned| interned.paint.shader.is_none());
        let surplus = self.paint_pool.len().saturating_sub(KEPT_PAINTS);
        self.paint_pool.drain(..surplus);
        // The canvas lives on between recordings: one that interned or saved
        // far more than the next needs keeps none of that room.
        if self.paint_pool.spilled() {
            self.paint_pool.shrink_to_fit();
        }
        if self.save_stack.capacity() > KEPT_SAVES {
            self.save_stack = Vec::new();
        }
        recording
    }

    /// What [`Self::finish`] and [`Self::take_recording`] report about the
    /// recording they end.
    fn report_finished(&self) {
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

        let non_finite = self.display_list.non_finite();
        if !non_finite.is_empty() {
            tracing::warn!(
                transform = non_finite.transform,
                geometry = non_finite.geometry,
                "Canvas refused non-finite ops"
            );
        }

        tracing::debug!(
            commands = self.display_list.len(),
            bounds = ?self.display_list.bounds(),
            "Canvas finalized"
        );
    }

    /// Returns a reference to the inner display list without consuming
    /// the canvas.
    pub fn display_list(&self) -> &DisplayList {
        &self.display_list
    }

    /// Resets the canvas to its initial state, clearing all commands and
    /// state. More efficient than `Canvas::new()` when reusing.
    ///
    /// Every field is reset before the recorded ops are dropped: an op's
    /// destructor can run user code (a paragraph's font bytes are a caller's
    /// `Arc<dyn AsRef<[u8]>>`), and one that panics leaves an empty canvas
    /// with no cached extent (ADR-0306 §7).
    pub fn reset(&mut self) {
        let commands = std::mem::take(&mut self.display_list);
        let paints = std::mem::take(&mut self.paint_pool);
        self.transform = Matrix4::identity();
        self.save_stack.clear();
        drop(paints);
        drop(commands);
    }

    // ===== Query Methods =====

    /// The room, in paints and in saved states, the canvas holds for its next
    /// recording: a probe for the crate's tests, outside the public API.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    #[doc(hidden)]
    pub fn kept_room(&self) -> (usize, usize) {
        (self.paint_pool.capacity(), self.save_stack.capacity())
    }

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
    pub fn bounds(&self) -> Option<Rect<f64>> {
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
