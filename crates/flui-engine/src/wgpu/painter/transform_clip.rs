// ===== Transform Stack & Clipping =====
//
// Moved from `painter.rs` into `painter/transform_clip.rs` as part of the
// C1 LOC-cap refactor.  Zero behaviour changes.

use flui_types::{
    Offset, Rect,
    geometry::{Pixels, RRect},
    painting::Path,
};

use super::WgpuPainter;

impl WgpuPainter {
    // ===== Transform Stack =====

    /// Save the current transform, scissor, and SDF-clip state onto the stack.
    ///
    /// Must be balanced by a matching [`Self::restore`] call.  Nesting is
    /// unbounded; `GpuStateStack` grows the stack dynamically.  At the end of
    /// each frame `GpuStateStack::debug_assert_balanced` fires in debug builds
    /// if the counts do not match.
    pub fn save(&mut self) {
        self.state.save();
    }

    /// Restore the transform, scissor, and SDF-clip state saved by the
    /// matching [`Self::save`] call.
    ///
    /// Popping from an empty stack is a logic error; in debug builds
    /// `GpuStateStack` panics; in release builds it logs a `tracing::warn!`
    /// and leaves the current state unchanged.
    pub fn restore(&mut self) {
        self.state.restore();
    }

    /// Concatenate a translation onto the current transform.
    ///
    /// `offset` is in device pixels.  Equivalent to premultiplying the CTM by
    /// `T(offset.dx, offset.dy)`.
    pub fn translate(&mut self, offset: Offset<Pixels>) {
        self.state.translate(offset);
    }

    /// Concatenate a clockwise rotation onto the current transform.
    ///
    /// `angle` is in radians.  Equivalent to premultiplying the CTM by
    /// `R(angle)` (rotation about the origin in the current coordinate space).
    pub fn rotate(&mut self, angle: f32) {
        self.state.rotate(angle);
    }

    /// Concatenate a non-uniform scale onto the current transform.
    ///
    /// `sx` and `sy` are scale factors along the X and Y axes respectively.
    /// Equivalent to premultiplying the CTM by `S(sx, sy)`.  Negative values
    /// produce a reflection; zero produces a degenerate transform that collapses
    /// all geometry to a line or point.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.state.scale(sx, sy);
    }

    // ===== Clipping =====

    /// Intersect the current scissor rect with `rect`.
    ///
    /// The scissor is maintained as a hardware GPU scissor rect (integer pixel
    /// coordinates clamped to `[0, viewport]`).  Subsequent draw calls are
    /// rasterised only within the resulting intersection.  Call [`Self::restore`]
    /// to pop the clip state pushed by the matching [`Self::save`].
    pub fn clip_rect(&mut self, rect: Rect<Pixels>) {
        self.state.clip_rect(rect, self.size);
    }

    /// Intersect the clip region with a rounded rectangle.
    ///
    /// Applies a coarse bounding-rect scissor for early rasteriser rejection,
    /// then encodes the per-corner radii into `current_rrect_clip` so that the
    /// SDF evaluator in `rect_instanced.wgsl` discards fragments outside the
    /// rounded boundary.  The SDF clip is applied per-draw rather than as a
    /// hardware stencil, so it only affects shapes that read the clip uniforms
    /// (rect/circle/arc SDF batches).
    #[allow(
        clippy::similar_names,
        reason = "r_tl/r_tr/r_br/r_bl mirror the rrect-corner field names; renaming would obscure intent"
    )]
    pub fn clip_rrect(&mut self, rrect: RRect) {
        self.state.clip_rrect(rrect, self.size);
    }

    /// Look up or generate a tessellated superellipse path via the
    /// Painter-owned bounded cache.
    ///
    /// Consulted by `Backend::superellipse_path` (the `CommandRenderer`
    /// trait override) so `ClipSuperellipseLayer::render`'s layer-tree
    /// clip path benefits from frame-bounded caching. On a miss the path
    /// is generated via `generate_superellipse_path` (the iOS-squircle
    /// math) and inserted; eviction follows PathCache semantics
    /// (`max_entries` + `last_used_frame`).
    pub(crate) fn superellipse_path(
        &mut self,
        rse: &flui_types::geometry::RSuperellipse,
    ) -> std::sync::Arc<flui_types::painting::Path> {
        let key = super::super::superellipse_cache::SuperellipseKey::from_superellipse(rse);
        if let Some(arc_path) = self.batcher.superellipse_cache.get(&key) {
            return arc_path;
        }
        // Cache miss: generate the path, wrap it in Arc, and store a clone
        // of the Arc (reference-count bump, no deep copy). Return the Arc
        // so the caller holds shared ownership.
        let arc_path = std::sync::Arc::new(crate::superellipse::generate_superellipse_path(rse));
        self.batcher
            .superellipse_cache
            .insert(key, std::sync::Arc::clone(&arc_path));
        arc_path
    }

    /// Set an SDF rounded-superellipse clip (iOS-squircle).
    ///
    /// Parallel to [`Self::clip_rrect`]: populates `current_rsuperellipse_clip`
    /// with the bounding rect + per-corner radii, applies a bounding-rect
    /// scissor for early rasterizer rejection, and relies on
    /// `rect_instanced.wgsl`'s per-pixel SDF evaluation to clip pixels
    /// outside the iOS-squircle curve (wired in U9 / U10).
    #[allow(
        clippy::similar_names,
        reason = "tl_r/tr_r/br_r/bl_r mirror the rsuperellipse-corner field names; renaming would obscure intent"
    )]
    pub fn clip_rsuperellipse(&mut self, rse: flui_types::geometry::RSuperellipse) {
        self.state.clip_rsuperellipse(rse, self.size);
    }

    /// Clip to an arbitrary path (currently unimplemented; emits a `tracing::warn!`).
    ///
    /// Path clipping requires a stencil-buffer pass (even-odd or non-zero fill
    /// rule) that is not yet wired in this engine.  All calls are no-ops at
    /// the GPU level and will emit a release-build warning via `tracing::warn!`.
    /// Use [`Self::clip_rect`] or [`Self::clip_rrect`] for hardware-accelerated
    /// clipping; [`Self::clip_rsuperellipse`] for iOS-squircle clips.
    pub fn clip_path(&mut self, _path: &Path) {
        // Path clipping requires stencil buffer or path tessellation
        // This is a complex feature that needs:
        // 1. Stencil buffer configuration in render pass
        // 2. Tessellate path and render to stencil buffer
        // 3. Enable stencil test for subsequent draws
        // 4. Stack management for nested clips
        // 5. Handle even-odd vs non-zero fill rules
        //
        // Additionally, Path::bounds() requires &mut Path for caching,
        // but we only have &Path in this context.
        //
        // For now, this is a no-op. Applications should use ClipRect or ClipRRect
        // for hardware-accelerated clipping. Path clipping will be implemented
        // in a future version with proper stencil buffer support.

        // Cycle 4 E-1: pre-cycle this path emitted a debug-only
        // `tracing::trace!` and returned silently. Production scrapes
        // never saw the missing clip — content rendered without the
        // intended clip. Upgrade to release-build `tracing::warn!` so
        // any consumer that hits the path gets a visible signal.
        tracing::warn!(
            "WgpuPainter::clip_path: path clipping not implemented; \
             content will render without the intended clip. \
             Use ClipRect or ClipRRect for hardware-accelerated clipping. \
             Path clipping requires stencil-buffer support (cycle 4 E-1)"
        );
    }
}
