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
    /// A rectangular clip is the hardware scissor, whatever the mode.
    ///
    /// `Clip::AntiAlias` on a *rect* is therefore **not yet honoured** — it
    /// renders as `HardEdge`. Routing it to the SDF instead (which is what
    /// gives a rounded clip its feathered edge) was tried and reverted: the
    /// SDF is a per-instance uniform and the scissor is not, so the swap
    /// silently gave up three things the scissor does.
    ///
    /// - **It reaches everything.** Text is handed to glyphon with the
    ///   scissor alone, so an SDF-only clip does not clip a label at all.
    /// - **It intersects.** Nested clips share one SDF slot and the inner one
    ///   clears the outer; scissors intersect by construction.
    /// - **It is exact.** The SDF's coarse scissor is deliberately expanded,
    ///   so pixels leak up to a pixel outside a clip that used to be tight.
    ///
    /// A feathered rectangular edge is not worth those three. Honouring it
    /// properly means giving the shader a clip *stack* and routing text
    /// through the same mask — a different piece of work, tracked on #848.
    ///
    /// `hard` is still taken so these call sites read like the rounded ones,
    /// and so that the day the SDF can carry a rect clip safely, only this
    /// body changes.
    pub fn clip_rect(&mut self, rect: Rect<Pixels>, hard: bool) {
        let _ = hard;
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
    /// `hard` selects the layer's `Clip` mode: a hard edge thresholds the SDF
    /// instead of feathering it. Both go through the SDF either way — unlike a
    /// rect, a rounded clip has no scissor equivalent that would keep the
    /// corners.
    pub fn clip_rrect(&mut self, rrect: RRect, hard: bool) {
        self.state.clip_rrect(rrect, self.size, hard);
    }

    /// The active scissor as a device-space rect, or the whole surface when no
    /// scissor is set.
    ///
    /// This is the compositing bounds for a `Clip::AntiAliasWithSaveLayer`
    /// layer: the scissor is the clip's device-space bounding box already
    /// intersected with every ancestor clip, and every draw inside the layer is
    /// subject to it, so nothing the offscreen holds can fall outside.
    pub(crate) fn clip_bounds(&self) -> Rect<Pixels> {
        self.state.current_scissor().map_or_else(
            || {
                Rect::from_xywh(
                    flui_types::geometry::px(0.0),
                    flui_types::geometry::px(0.0),
                    flui_types::geometry::px(self.size.0 as f32),
                    flui_types::geometry::px(self.size.1 as f32),
                )
            },
            |(x, y, width, height)| {
                Rect::from_xywh(
                    flui_types::geometry::px(x as f32),
                    flui_types::geometry::px(y as f32),
                    flui_types::geometry::px(width as f32),
                    flui_types::geometry::px(height as f32),
                )
            },
        )
    }

    /// Apply a rounded clip's bounding-box scissor and return the clip for a
    /// group composite to apply once — the `Clip::AntiAliasWithSaveLayer` half
    /// of [`Self::clip_rrect`].
    ///
    /// The per-draw SDF slot is deliberately NOT written: the caller is about
    /// to open an offscreen with [`Self::save_layer_clipped`], and the returned
    /// clip is applied to that layer's composite instead. Setting both would
    /// apply the clip's coverage twice — once per draw and once to the group —
    /// which reads as a darker, more opaque edge wherever the content overlaps
    /// itself. See `GpuStateStack::clip_rrect_at_composite`.
    /// The squircle counterpart of [`Self::clip_rrect_at_composite`]: installs
    /// the bounding scissor and returns the coverage for the group composite
    /// to apply once. See `GpuStateStack::clip_rsuperellipse_at_composite`.
    pub(crate) fn clip_rsuperellipse_at_composite(
        &mut self,
        rse: flui_types::geometry::RSuperellipse,
    ) -> super::super::state_stack::ResolvedClip {
        self.state.clip_rsuperellipse_at_composite(rse, self.size)
    }

    pub(crate) fn clip_rrect_at_composite(
        &mut self,
        rrect: RRect,
    ) -> super::super::state_stack::ResolvedClip {
        self.state.clip_rrect_at_composite(rrect, self.size)
    }

    /// Set an SDF rounded-superellipse clip (iOS-squircle).
    ///
    /// Parallel to [`Self::clip_rrect`]: populates `current_rsuperellipse_clip`
    /// with the bounding rect + per-corner radii, applies a bounding-rect
    /// scissor for early rasterizer rejection, and relies on
    /// `rect_instanced.wgsl`'s per-pixel SDF evaluation to clip pixels
    /// outside the iOS-squircle curve.
    /// `hard` selects the layer's `Clip` mode, exactly as for
    /// [`Self::clip_rrect`]: a squircle has no scissor equivalent that keeps
    /// its corners, so both modes go through the SDF and the mode chooses
    /// between thresholding and feathering.
    pub fn clip_rsuperellipse(&mut self, rse: flui_types::geometry::RSuperellipse, hard: bool) {
        self.state.clip_rsuperellipse(rse, self.size, hard);
    }

    /// Clip to an arbitrary path, approximated by its BOUNDING BOX.
    ///
    /// The exact edge needs a stencil pass (even-odd / non-zero fill rule) this
    /// engine does not have, so what is installed is the path's conservative
    /// bounding rectangle as a hardware scissor. A bounding box is a superset
    /// of the shape it bounds, so this can only remove content the exact clip
    /// would also remove — it never clips away a pixel the path keeps. What
    /// still renders is everything inside the box but outside the shape: the
    /// notch of a star, the bite of a crescent. That is the whole of the
    /// remaining gap, and it is the reason this still reports itself.
    ///
    /// Until this, nothing was installed at all, which is issue #934.
    /// `RenderPhysicalShape` under `Clip::AntiAliasWithSaveLayer` fills its
    /// colour with `Canvas::draw_paint` INSIDE the clip scope — deliberate
    /// Flutter parity, so the shape's edge is anti-aliased once rather than
    /// twice (`proxy_box.dart:2346`, citing flutter/flutter#18057) — and a fill
    /// with no geometry of its own is bounded by nothing but the clip. A
    /// `Material` therefore painted the entire window.
    ///
    /// The scissor is rounded OUTWARD, by `GpuStateStack::clip_rect_enclosing`
    /// rather than the truncating `clip_rect` every other clip uses. Truncation
    /// would take the superset property away and up to a column and a row of
    /// content the path keeps with it; a rounded clip can afford it because its
    /// exact SDF is what actually cuts the edge, and it deliberately does not
    /// pad outward for the mirror-image reason — there the pad would let text
    /// leak past a real edge. The rounding has to happen after the transform,
    /// which is why it is not done here: a box grown to whole pixels in local
    /// space is re-fractioned by any translation with a fractional part.
    ///
    /// Under a rotation the scissor is the AABB of the transformed box, which
    /// is larger again — the approximation gets looser, never tighter, so the
    /// superset property survives.
    pub fn clip_path(&mut self, path: &Path) {
        if !self.path_clip_approximated {
            self.path_clip_approximated = true;
            tracing::warn!(
                "WgpuPainter::clip_path: exact path clipping is not implemented; \
                 clipping to the path's bounding box instead, so content inside \
                 the box but outside the path shape still renders. Use ClipRect, \
                 ClipRRect or ClipRSuperellipse for an exact clip. Reported once \
                 per painter."
            );
        }
        self.state
            .clip_rect_enclosing(path.compute_bounds(), self.size);
    }
}
