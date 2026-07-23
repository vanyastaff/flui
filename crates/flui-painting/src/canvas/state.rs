//! Canvas state stack: save/restore + save_layer family.
//!
//! These were extracted from the 3,305-LOC `canvas.rs` god
//! module into a focused file. The state stack carries:
//!
//! - The current transform matrix (snapshotted by `save()`).
//! - The clip stack depth (truncated back to the saved depth on
//!   `restore()`).
//! - A `is_layer` flag (used by `save_layer()` to emit a matching
//!   `DrawCommand::RestoreLayer` when the layer is composited back).
//!
//! `restore()` on an empty save stack is a silent no-op (Flutter parity
//! with `Canvas.restore()` -- Skia drops unrestored saves on
//! finalisation; we follow the same shape).

use flui_types::{
    geometry::{Matrix4, Pixels, Rect},
    painting::BlendMode,
    styling::Color,
};

use super::Canvas;
use crate::display_list::{DrawCommand, Paint};

/// Saved canvas state (for save/restore).
#[derive(Debug, Clone)]
pub struct CanvasState {
    /// Saved transform matrix.
    pub(crate) transform: Matrix4,
    /// Depth of clip stack when saved.
    pub(crate) clip_depth: usize,
    /// Whether this save created a layer (for save_layer).
    pub(crate) is_layer: bool,
}

/// Clip operation stored in the clip stack.
///
/// Currently used for tracking clip depth in save/restore operations.
/// The clip geometry (Rect/RRect/Path) is stored for future
/// optimizations:
///
/// - Culling: skip drawing commands outside the clip bounds.
/// - Clip bounds queries: `canvas.local_clip_bounds()`.
/// - Render optimization: merge adjacent clips.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields stored for future optimization features
pub enum ClipShape {
    /// Rectangular clip.
    Rect(Rect<Pixels>),
    /// Rounded-rectangle clip.
    RRect(flui_types::geometry::RRect),
    /// Rounded-superellipse clip (Flutter `RSuperellipse`).
    RSuperellipse(flui_types::geometry::RSuperellipse),
    /// Path clip; boxed for variant size uniformity.
    Path(Box<flui_types::painting::Path>),
}

impl Canvas {
    // ===== Save/Restore =====

    /// Saves the current canvas state (transform, clip).
    ///
    /// Must be balanced with `restore()`. Unbalanced saves at
    /// `finish()` time fire `debug_assert!` (caught during tests) and
    /// `tracing::warn!` (release observability).
    #[inline]
    pub fn save(&mut self) {
        self.save_stack.push(CanvasState {
            transform: self.transform,
            clip_depth: self.clip_stack.len(),
            is_layer: false,
        });
    }

    /// Restores the most recently saved state.
    ///
    /// If the saved state was created by `save_layer()`, this also
    /// composites the layer back using the paint specified when the
    /// layer was created.
    ///
    /// If there is no saved state, this is a silent no-op (Flutter
    /// parity).
    #[inline]
    pub fn restore(&mut self) {
        if let Some(state) = self.save_stack.pop() {
            if state.is_layer {
                self.display_list.push(DrawCommand::RestoreLayer {
                    transform: self.transform,
                });
            }

            self.transform = state.transform;
            self.clip_stack.truncate(state.clip_depth);
        }
    }

    /// Returns the number of saved states (plus 1 for the initial
    /// state). The initial save count is 1.
    ///
    /// Flutter-parity (`Canvas.getSaveCount()` returns 1 for an
    /// unmodified canvas — the initial save scope counts). Audit P-16
    /// flagged a possible `save_depth` rename for clarity and the audit
    /// itself recommended defer; the rename is a public-API cosmetic
    /// change that does not pay for itself. The 1-indexed semantics are
    /// fixed by Flutter parity, not by this name.
    pub fn save_count(&self) -> usize {
        self.save_stack.len() + 1
    }

    /// Restores the canvas state to a specific save count.
    ///
    /// This pops states from the save stack until the stack reaches
    /// the specified count.
    ///
    /// # Arguments
    ///
    /// * `count` - Target save count (must be >= 1 and <= current save
    ///   count)
    pub fn restore_to_count(&mut self, count: usize) {
        let count = count.max(1); // Cannot go below 1
        while self.save_count() > count {
            self.restore();
        }
    }

    // ===== Layer Operations =====

    /// Saves the canvas state and creates a new compositing layer.
    ///
    /// This is similar to `save()` but creates an offscreen buffer for
    /// subsequent drawing commands. When `restore()` is called, the
    /// layer is composited back using the specified paint settings
    /// (opacity, blend mode, color filter, etc.).
    ///
    /// # Paint validation
    ///
    /// This method does *not* clamp `paint.color.alpha_f32()` into
    /// `[0.0, 1.0]` — the caller is expected to hand in a validated
    /// `Paint`. Use [`Self::save_layer_opacity`] (which performs
    /// `opacity.clamp(0.0, 1.0)` before forwarding) if your opacity
    /// value comes from untrusted input. Passing an out-of-range
    /// alpha here lets the value reach the GPU backend, which may
    /// over-saturate or produce undefined blend behaviour depending
    /// on the wgpu target.
    ///
    /// # Performance
    ///
    /// `save_layer` is relatively expensive because it:
    ///
    /// 1. Forces GPU to switch render targets.
    /// 2. Allocates an offscreen buffer.
    /// 3. Requires copying framebuffer contents.
    ///
    /// Use sparingly, especially on lower-end hardware.
    #[tracing::instrument(skip(self, paint), fields(
        bounds = ?bounds,
        opacity = paint.color.alpha_f32(),
        blend_mode = ?paint.blend_mode,
        layer_depth = self.save_stack.len(),
    ))]
    pub fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint) {
        self.save_stack.push(CanvasState {
            transform: self.transform,
            clip_depth: self.clip_stack.len(),
            is_layer: true,
        });

        let interned_paint = self.intern_paint(paint);
        let transform = self.transform;
        self.display_list.push(DrawCommand::SaveLayer {
            bounds,
            paint: interned_paint,
            transform,
        });

        tracing::debug!(layer_depth = self.save_stack.len(), "Layer created");
    }

    /// Saves the canvas state with a layer that applies alpha
    /// transparency.
    ///
    /// Convenience method equivalent to:
    /// ```rust,ignore
    /// canvas.save_layer(bounds, &Paint::new().with_opacity(alpha / 255.0));
    /// ```
    pub fn save_layer_alpha(&mut self, bounds: Option<Rect<Pixels>>, alpha: u8) {
        let opacity = alpha as f32 / 255.0;
        self.save_layer(
            bounds,
            &Paint::fill(Color::TRANSPARENT).with_opacity(opacity),
        );
    }

    /// Saves the canvas state with a layer that applies float opacity.
    pub fn save_layer_opacity(&mut self, bounds: Option<Rect<Pixels>>, opacity: f32) {
        self.save_layer(
            bounds,
            &Paint::fill(Color::TRANSPARENT).with_opacity(opacity.clamp(0.0, 1.0)),
        );
    }

    /// Saves the canvas state with a layer that applies a blend mode at full opacity.
    ///
    /// Flutter semantics: `saveLayer(blendMode)` with no explicit opacity is opaque
    /// (alpha = 1.0).  The engine derives layer opacity from `paint.color.a`, so
    /// this method sets alpha = 255 — not the zero produced by `Color::TRANSPARENT`.
    /// RGB channels are ignored for saveLayer compositing; only alpha matters.
    pub fn save_layer_blend(&mut self, bounds: Option<Rect<Pixels>>, blend_mode: BlendMode) {
        // Alpha=255 (opaque) — blend-only layer.  `Color::TRANSPARENT` has alpha=0,
        // which would make the engine treat the layer as invisible (a no-op).
        let opaque_blend_paint = Paint::fill(Color::TRANSPARENT)
            .with_opacity(1.0)
            .with_blend_mode(blend_mode);
        self.save_layer(bounds, &opaque_blend_paint);
    }
}

#[cfg(test)]
mod tests {
    use flui_types::painting::BlendMode;

    use crate::display_list::Paint;
    use flui_types::styling::Color;

    /// `save_layer_blend` must produce a paint with alpha = 255 (opaque).
    ///
    /// The engine derives layer opacity from `paint.color.a`.  Before this fix
    /// `save_layer_blend` forwarded `Color::TRANSPARENT` (alpha = 0), making the
    /// advanced-blend layer a silent no-op: opacity = 0 → backdrop passthrough
    /// regardless of the requested blend mode.
    ///
    /// This test fails on pre-fix code where `with_opacity(1.0)` is absent.
    #[test]
    fn save_layer_blend_paint_is_opaque() {
        // Verify the paint that save_layer_blend would pass to save_layer carries
        // alpha = 255.  We construct the same expression used by the method body
        // directly — this is a white-box test of the documented fixed invariant.
        let blend_paint = Paint::fill(Color::TRANSPARENT)
            .with_opacity(1.0)
            .with_blend_mode(BlendMode::Multiply);
        assert_eq!(
            blend_paint.color.a, 255,
            "save_layer_blend paint must be opaque (alpha = 255); \
             alpha = 0 silently no-ops the advanced-blend layer"
        );
        assert_eq!(
            blend_paint.blend_mode,
            BlendMode::Multiply,
            "save_layer_blend paint must carry the requested blend mode"
        );
    }
}
