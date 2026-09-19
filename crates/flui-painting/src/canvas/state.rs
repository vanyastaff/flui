//! Canvas state stack: save/restore and the save_layer family.
//!
//! A saved state is the transform and
//! whether the save opened a layer (so `restore` emits the matching
//! `RestoreLayer`). `restore()` on an empty save stack is a silent no-op,
//! as `dart:ui`'s `Canvas.restore()` is in release builds.

use flui_types::{
    geometry::{Matrix4, Pixels, Rect},
    painting::BlendMode,
    styling::Color,
};

use super::Canvas;
use crate::display_list::{DrawOp, Paint};

/// Saved canvas state (for save/restore).
#[derive(Debug, Clone)]
pub(crate) struct CanvasState {
    /// Saved transform matrix.
    pub(crate) transform: Matrix4,
    /// Whether this save created a layer (for save_layer).
    pub(crate) is_layer: bool,
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
            is_layer: false,
        });
        // The backend needs the scope marker too, not just this stack: a clip
        // recorded after this point narrows the backend's state, and only a
        // matching `Restore` tells it when to stop.
        self.record(DrawOp::Save);
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
            // Exactly one closing command per opening one: `save_layer`
            // recorded a `SaveLayer` and is closed by `RestoreLayer`, which
            // composites the offscreen target and unwinds its own state; a
            // plain `save` recorded a `Save` and is closed by `Restore`.
            if state.is_layer {
                self.record(DrawOp::RestoreLayer);
            } else {
                self.record(DrawOp::Restore);
            }

            self.transform = state.transform;
        }
    }

    /// The number of saved states plus one for the initial state, so an
    /// unmodified canvas answers 1 (`dart:ui`'s `Canvas.getSaveCount()`).
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
            is_layer: true,
        });

        let paint = self.intern_paint(paint);
        self.record(DrawOp::SaveLayer { bounds, paint });

        tracing::debug!(layer_depth = self.save_stack.len(), "Layer created");
    }

    /// Saves the canvas state with a layer that applies alpha
    /// transparency.
    ///
    /// Equivalent to `save_layer` with a paint whose opacity is `alpha / 255`.
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

    use super::{Canvas, DrawOp};

    /// `save_layer_blend` must record an OPAQUE paint carrying the blend
    /// mode: the engine derives layer opacity from `paint.color.a`, so a
    /// `Color::TRANSPARENT` paint (alpha 0) makes the advanced-blend layer a
    /// silent backdrop passthrough whatever mode was asked for.
    #[test]
    fn save_layer_blend_records_an_opaque_paint_with_the_mode() {
        let mut canvas = Canvas::new();
        canvas.save_layer_blend(None, BlendMode::Multiply);
        let recorded = &canvas.display_list()[0].op;
        assert!(
            matches!(recorded, DrawOp::SaveLayer { .. }),
            "got {recorded:?}"
        );
        if let DrawOp::SaveLayer { paint, .. } = recorded {
            assert_eq!(
                paint.color.a, 255,
                "alpha 0 silently no-ops the advanced-blend layer"
            );
            assert_eq!(paint.blend_mode, BlendMode::Multiply);
        }
    }
}
