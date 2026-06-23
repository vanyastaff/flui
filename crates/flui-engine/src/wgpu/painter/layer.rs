// ===== Layer Operations (Opacity, Filters, Save/Restore) =====
//
// Moved from `painter.rs` into `painter/layer.rs` as part of the C1 LOC-cap
// refactor.  Zero behaviour changes.

use smallvec::smallvec;

use flui_painting::Paint;
use flui_types::{
    Rect,
    geometry::{Pixels, px},
};

use super::super::command_ir::{
    DrawItem, DrawSegment, FilterOp, ImageFilterPass, ImageFilterSpec, LayerFilter,
    LayerFilterChain, PendingOpacityLayer,
};
use super::super::layer_compositor::RestoreOutcome;
use super::WgpuPainter;

impl WgpuPainter {
    // ===== Viewport Information =====

    /// Return the full viewport bounds as a device-pixel rect.
    ///
    /// Equivalent to `Rect::from_ltrb(0, 0, width, height)` where
    /// `(width, height)` is the size last set by [`Self::new`] or
    /// [`Self::resize`].  Used as the fallback composite rect when a
    /// `save_layer` carries no explicit bounds.
    pub fn viewport_bounds(&self) -> Rect<Pixels> {
        Rect::from_ltrb(
            px(0.0),
            px(0.0),
            px(self.size.0 as f32),
            px(self.size.1 as f32),
        )
    }

    /// Compute the integer-aligned offscreen frame rectangle for a filter intermediate.
    ///
    /// ## Integer-grid composite invariant (Task 6)
    ///
    /// The texture-batch composite sampler is **bilinear** (`default_sampler` Linear
    /// in `replay.rs`).  Production `grown_bounds` may have fractional edges after
    /// AABB-expansion and viewport intersection.  Compositing a fractional-origin
    /// grown texture at its fractional dst_rect keeps the texel grid aligned with
    /// the device-pixel grid — a valid 1:1 aligned blit.
    ///
    /// Shrinking to a smaller intermediate but compositing at the same fractional
    /// `grown_bounds` would offset the two grids by `frac(grown_left)`, shifting
    /// every pixel by a sub-texel.  To avoid this, BOTH the intermediate size and
    /// the composite dst_rect MUST share one integer grid:
    ///
    /// ```text
    /// fb_origin = (floor(grown.left), floor(grown.top))
    /// fb_far    = (ceil(grown.right),  ceil(grown.bottom))   // clamped to viewport
    /// fb_dim    = fb_far - fb_origin
    /// composite: dst_rect = Rect(fb_origin, fb_far),  src_uv = [0, 0, 1, 1]
    /// ```
    ///
    /// For the entire readback test suite (all integer-aligned margins) floor/ceil
    /// are no-ops → fb rect == grown_bounds → bit-identical output → zero re-baseline.
    ///
    /// ## Return value
    ///
    /// `(fb_origin, fb_dim)` where both components are `(u32, u32)` integer pixel
    /// coordinates.  `fb_dim` is clamped to `[1, viewport]` per axis so the pool
    /// acquire is always valid.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "grown_bounds coords are in [0, vp_dim] after viewport intersection; \
                  floor/ceil of non-negative f32 ≤ u32::MAX fits safely in u32"
    )]
    fn filter_fb_rect(&self, grown_bounds: Rect<Pixels>) -> ((u32, u32), (u32, u32)) {
        let (vp_w, vp_h) = self.size;

        // Integer-grid origin: floor the fractional grown-bounds top-left.
        let origin_x = grown_bounds.left().0.floor() as u32;
        let origin_y = grown_bounds.top().0.floor() as u32;

        // Integer-grid far corner: ceil the fractional grown-bounds bottom-right,
        // then clamp to the viewport so we never allocate past the surface edge.
        let far_x = (grown_bounds.right().0.ceil() as u32).min(vp_w);
        let far_y = (grown_bounds.bottom().0.ceil() as u32).min(vp_h);

        // Dimension: must be at least 1×1 (pool acquire contract).
        let dim_x = far_x.saturating_sub(origin_x).max(1);
        let dim_y = far_y.saturating_sub(origin_y).max(1);

        ((origin_x, origin_y), (dim_x, dim_y))
    }

    /// Compute a conservative device-space content AABB from a `DrawSegment`.
    ///
    /// Returns the union of all geometry bounding boxes in the segment, in device
    /// pixels.  Returns `None` when the segment is empty OR when any geometry kind
    /// cannot be conservatively bounded (the caller falls back to the full viewport).
    ///
    /// ## Conservative-or-fallback contract (CRITICAL)
    ///
    /// This function MUST NEVER return an AABB smaller than the true device-space
    /// content extent.  An under-estimate would clip drawn pixels — a visible
    /// correctness regression worse than no win at all.  Over-estimation is always
    /// safe (it merely reduces the VRAM benefit).
    ///
    /// When in doubt about a geometry kind, return `None` so the caller falls back
    /// to the viewport.  The fallback is correct; it only forgoes the VRAM saving.
    ///
    /// ## Repositionable vs. fallback kinds
    ///
    /// Grown-bounds rendering (`render_segment_to_grown_offscreen`) currently
    /// repositions only:
    ///
    /// - tessellated vertices (`segment.vertices`)
    /// - `RectInstance`, `CircleInstance`, `ArcInstance` (instanced batches)
    ///
    /// Segments containing **shadows, gradients, or images** fall back to the
    /// full-viewport path (correct, no VRAM win) because those kinds are not
    /// repositioned by the grown-offscreen renderer.  Repositioning them is a
    /// tracked follow-up.  Returning `None` here causes the caller's
    /// `.unwrap_or(viewport)` to select `fb_dim == viewport`, which makes
    /// `render_segment_to_grown_offscreen`'s remap an identity transform —
    /// rendering is correct with zero VRAM saving.
    ///
    /// ## Geometry kinds covered when returning `Some`
    ///
    /// | Kind | Bound source |
    /// |------|-------------|
    /// | `vertices` | Exact min/max of `Vertex::position` (device px) |
    /// | `RectInstance` baked (identity M, zero t) | `bounds [x,y,w,h]` in device px |
    /// | `RectInstance` affine | 4 corners transformed by M+t, convex hull |
    /// | `CircleInstance` / `ArcInstance` | center ± (‖col_x‖₁ + ‖col_y‖₁) (conservative) |
    fn content_aabb(segment: &DrawSegment) -> Option<Rect<Pixels>> {
        // ── Fallback gate (P0 regression fix) ────────────────────────────────
        //
        // Shadows, gradients, and images cannot be repositioned by
        // `render_segment_to_grown_offscreen`.  Returning `None` here forces the
        // caller's `.unwrap_or(viewport)` to select `composite_bounds = viewport`
        // → `fb_dim == viewport` → the remap in the grown renderer is the identity
        // transform → those kinds render at the correct position.
        //
        // This is a conservative-or-fallback: the only cost is forgoing the VRAM
        // optimisation for layers that contain these kinds.  Correctness is fully
        // preserved.  Repositioning shadows/gradients/images in the grown
        // intermediate is a tracked follow-up.
        if !segment.shadow_batch.is_empty()
            || !segment.linear_gradient_batch.is_empty()
            || !segment.radial_gradient_batch.is_empty()
            || !segment.sweep_gradient_batch.is_empty()
            || !segment.cached_images.is_empty()
            || !segment.external_images.is_empty()
        {
            return None;
        }

        // Running AABB accumulators.
        // Initialised to sentinel values: min→+∞, max→−∞.
        // After the loops, `min_x <= max_x` iff at least one point was unioned.
        let mut min_x: f32 = f32::MAX;
        let mut min_y: f32 = f32::MAX;
        let mut max_x: f32 = f32::NEG_INFINITY;
        let mut max_y: f32 = f32::NEG_INFINITY;

        /// Inline helper: union a device-px point into the running AABB.
        ///
        /// Does NOT set a `has_any` flag — emptiness is detected at the end by
        /// checking `min_x <= max_x` (which is only true when at least one point
        /// was unioned into an initially-sentinel accumulator).
        macro_rules! union_pt {
            ($x:expr, $y:expr) => {{
                let x: f32 = $x;
                let y: f32 = $y;
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
            }};
        }

        // ── 1. Tessellated vertices (device px, exact) ────────────────────────
        for v in &segment.vertices {
            let [x, y] = v.position;
            union_pt!(x, y);
        }

        // ── 2. RectInstance ──────────────────────────────────────────────────
        //
        // `bounds = [x, y, w, h]` in local-space.
        // Baked (M = identity, t = zero): bounds are already device px.
        // Affine: bounds are local-space; apply `device = M * local_corner + t`.
        //
        // M is stored column-major as `transform = [a, b, c, d]`:
        //   x_col = (a, b), y_col = (c, d).
        // t is `transform_translate = [tx, ty, _, _]`.
        //
        // We check identity M exactly (all RectInstance::rect / ::rounded_rect_corners
        // constructions set `[1,0,0,1]`); a non-identity M triggers the affine path.
        for instance in &segment.rect_batch.instances {
            let [lx, ly, lw, lh] = instance.bounds;
            let [a, b, c, d] = instance.transform;
            let [tx, ty, _, _] = instance.transform_translate;

            // Exact bit-equality check against the identity 2×2.
            // These are the only values written by `RectInstance::rect` and
            // `::rounded_rect_corners` — never computed via arithmetic, so
            // ULP slop is not a concern.
            #[expect(
                clippy::float_cmp,
                reason = "exact comparison against the bit-exact identity matrix [1,0,0,1] \
                          written by RectInstance::rect / ::rounded_rect_corners; \
                          never produced by arithmetic"
            )]
            let is_identity_m = a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0;

            // Exact bit comparison against zero is clippy-exempt (literal `0.0`).
            let is_zero_t = tx == 0.0 && ty == 0.0;

            if is_identity_m && is_zero_t {
                // Baked path: bounds are device px.
                union_pt!(lx, ly);
                union_pt!(lx + lw, ly);
                union_pt!(lx + lw, ly + lh);
                union_pt!(lx, ly + lh);
            } else {
                // Affine path: transform 4 corners of the local rect.
                // device = M * (lx_corner, ly_corner) + (tx, ty)
                // where M = [[a,c],[b,d]] (column-major: x_col=(a,b), y_col=(c,d)).
                let corners = [(lx, ly), (lx + lw, ly), (lx + lw, ly + lh), (lx, ly + lh)];
                for (cx, cy) in corners {
                    let dx = a * cx + c * cy + tx;
                    let dy = b * cx + d * cy + ty;
                    union_pt!(dx, dy);
                }
            }
        }

        // ── 3. CircleInstance ────────────────────────────────────────────────
        //
        // Center in `transform_translate.xy` (device px, added AFTER M).
        // M encodes `M_world * diag(rx, ry)` or `M_world * r` (column-major).
        //
        // Conservative bounding box: the axis-aligned box of the transformed
        // unit circle at origin is `center ± (‖col_x‖₂ , ‖col_y‖₂)`, but we
        // use the L1-norm of columns as a simple over-estimate that avoids sqrt:
        //   half_x = |col_x|_max ≤ actually |col_x|_2
        // Wait — we need an OVER-estimate, not an under-estimate.
        // The true half-extents of M*unit_circle are the singular values of M.
        // Conservative upper bound: ‖col_x‖₁ + ‖col_y‖₁ ≥ σ₁ (max singular value).
        // This is always safe (never clips content).
        for instance in &segment.circle_batch.instances {
            let [center_x, center_y, _, _] = instance.transform_translate;
            let [a, b, c, d] = instance.transform;
            // `center_radius[2]` is the radius factor:
            //   - Baked path (`CircleInstance::new`):  radius stored here; transform = diag(sx,sy).
            //     Device half-extent = radius * sx (X), radius * sy (Y).
            //   - Affine path (`with_affine_transform`): center_radius[2] = 1.0; radius folded
            //     into transform columns. Multiplying by 1.0 is a safe no-op.
            // Without this factor the baked path produces half_x = sx (missing `* radius`),
            // which clips a circle of radius R at scale 1 to a ~2×2 box around its center.
            let radius_factor = instance.center_radius[2];
            let half_x = radius_factor * (a.abs() + c.abs());
            let half_y = radius_factor * (b.abs() + d.abs());
            union_pt!(center_x - half_x, center_y - half_y);
            union_pt!(center_x + half_x, center_y + half_y);
        }

        // ── 4. ArcInstance ───────────────────────────────────────────────────
        //
        // Same layout as CircleInstance (unit circle at origin, center in
        // `transform_translate`).  Conservative: use the full circle box (we
        // never under-estimate a swept arc by using the enclosing circle).
        // `center_radius[2]` is always 1.0 for arcs (radius folded into transform),
        // so multiplying is a safe no-op that keeps the two loops structurally uniform.
        for instance in &segment.arc_batch.instances {
            let [center_x, center_y, _, _] = instance.transform_translate;
            let [a, b, c, d] = instance.transform;
            let radius_factor = instance.center_radius[2];
            let half_x = radius_factor * (a.abs() + c.abs());
            let half_y = radius_factor * (b.abs() + d.abs());
            union_pt!(center_x - half_x, center_y - half_y);
            union_pt!(center_x + half_x, center_y + half_y);
        }

        // ── 5. (Shadow / gradient / image kinds) ─────────────────────────────
        //
        // These kinds are excluded by the early-return gate above.  If any of
        // them is non-empty, `None` has already been returned, so none of these
        // loops can have instances to iterate.  The loops are omitted; the gate
        // is the single authoritative location.

        // Sentinel check: if no point was ever unioned, min_x > max_x still
        // holds (f32::MAX > f32::NEG_INFINITY) — return None for the empty case.
        if min_x > max_x {
            return None;
        }

        Some(Rect::from_ltrb(px(min_x), px(min_y), px(max_x), px(max_y)))
    }

    // ===== Layer Operations (Opacity) =====

    /// Open a compositing layer for group opacity or blend mode.
    ///
    /// All drawing between `save_layer` and the matching [`Self::restore_layer`]
    /// is captured into an offscreen texture.  On restore the offscreen is
    /// composited onto the parent surface with the layer's effective group
    /// opacity (derived from `paint.color.a`) and `paint.blend_mode`.
    ///
    /// The `paint.color` RGB is intentionally ignored (not used as a tint) —
    /// per Flutter semantics, chroma is set via an explicit `ColorFilter`, not
    /// the layer paint's RGB.  For a tinted layer use
    /// [`Self::save_layer_with_tint`].
    ///
    /// `bounds` hints the maximum bounds of the offscreen; `None` defaults to
    /// the full viewport.  This is a hint only — the compositor may expand it.
    pub fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint) {
        let paint_alpha = f32::from(paint.color.a) / 255.0;
        let layer_opacity = self.compositor.effective_layer_opacity(paint_alpha);

        // A saveLayer paint's RGB is NOT a compositing tint. Per Flutter
        // semantics the layer's group opacity comes from the paint's *alpha*,
        // and chroma comes only from an explicit ColorFilter — never from
        // `paint.color`'s RGB. The public canvas opacity helpers build
        // alpha-only layer paints as `Paint::fill(Color::TRANSPARENT)
        // .with_opacity(..)` (RGB `[0,0,0]`, see flui-painting
        // `canvas/state.rs`), so reading RGB here would tint group-opacity
        // layers black. Always use a white (no-op) chroma; ColorFilter chroma
        // arrives explicitly via `save_layer_with_tint` from
        // `push_color_filter`.
        //
        // The blend mode IS propagated: an advanced blend mode (e.g. Multiply)
        // on the saveLayer paint means the entire layer composites onto its
        // parent with that mode — the dominant real-world use case for
        // advanced blend.
        self.save_layer_impl(
            bounds,
            layer_opacity,
            [1.0, 1.0, 1.0],
            paint.blend_mode,
            LayerFilterChain::new(),
        );
    }

    /// Like [`Self::save_layer`] but applies an explicit per-channel chroma
    /// `tint_rgb` to the composited layer.
    ///
    /// Used by the ColorFilter layer path (`push_color_filter`), which
    /// approximates a color matrix as a single multiply tint
    /// (`filter.apply([1,1,1,1])`). `opacity` is the layer's effective alpha in
    /// `[0, 1]`; `tint_rgb` components are clamped to `[0, 1]`. The composite
    /// applies `(C.r*O, C.g*O, C.b*O, O)` to the premultiplied offscreen, so a
    /// hue shift survives compositing — see `flush_opacity_layer`.
    pub fn save_layer_with_tint(
        &mut self,
        bounds: Option<Rect<Pixels>>,
        opacity: f32,
        tint_rgb: [f32; 3],
    ) {
        let layer_opacity = self
            .compositor
            .effective_layer_opacity(opacity.clamp(0.0, 1.0));
        let tint = [
            tint_rgb[0].clamp(0.0, 1.0),
            tint_rgb[1].clamp(0.0, 1.0),
            tint_rgb[2].clamp(0.0, 1.0),
        ];
        // ColorFilter tint layers always use SrcOver — chroma is encoded via
        // the tint, not the blend mode.
        self.save_layer_impl(
            bounds,
            layer_opacity,
            tint,
            flui_types::painting::BlendMode::SrcOver,
            LayerFilterChain::new(), // no filter — tint carries the color
        );
    }

    /// Like [`Self::save_layer`] but routes the layer through a per-pixel GPU
    /// filter (currently only [`LayerFilter::ColorMatrix`]) before compositing.
    ///
    /// The filter is applied AFTER `render_layer_to_offscreen` and BEFORE the
    /// composite step, so it receives the fully-rendered premultiplied offscreen
    /// and emits a filtered premultiplied texture.  Opacity and blend mode carry
    /// through normally.
    ///
    /// Used by `push_color_filter` and the `Matrix`/`ColorAdjust` branches of
    /// `push_image_filter` in `backend.rs`.
    pub(crate) fn save_layer_with_filter(
        &mut self,
        bounds: Option<Rect<Pixels>>,
        filter: LayerFilter,
    ) {
        // Filter layers composite with white tint and SrcOver.  `effective_layer_opacity(1.0)`
        // multiplies 1.0 by the current ancestor opacity, so a filter layer nested inside an
        // outer opacity layer correctly inherits that opacity — matching Flutter semantics where
        // a color-filter saveLayer respects the parent's opacity.
        let layer_opacity = self.compositor.effective_layer_opacity(1.0);
        self.save_layer_impl(
            bounds,
            layer_opacity,
            [1.0, 1.0, 1.0],
            flui_types::painting::BlendMode::SrcOver,
            smallvec![filter],
        );
    }

    /// Open a bounds-GROWING image filter layer.
    ///
    /// Unlike `save_layer` (which closes over an offscreen with group opacity) and
    /// `save_layer_with_filter` (which applies a `LayerFilter::ColorMatrix` that
    /// does NOT grow bounds), this method routes the layer's offscreen content
    /// through a `DrawItem::Filter` at `restore_layer` time instead of
    /// `DrawItem::OpacityLayer`.  The `FilterOp` carries the pass chain derived
    /// from `spec` and a `grown_bounds` rect that expands beyond the content AABB,
    /// allowing morphology/blur to composite at a larger area than the input.
    ///
    /// The layer is pushed with opacity=inherited (so any outer group opacity still
    /// applies), white tint, SrcOver, and empty color-filter chain — identical to
    /// `save_layer_with_filter`.  The `image_filter` field on the top `SavedLayer`
    /// is then set so `restore_layer` can detect the bounds-growing path.
    ///
    /// Used by `push_image_filter` in `backend.rs` for `Dilate`, `Erode`, `Blur`,
    /// and `Compose` (the latter via a pre-flattened `ImageFilterSpec::Chain`).
    pub(crate) fn save_layer_with_image_filter(&mut self, spec: ImageFilterSpec) {
        // Inherit the current ancestor opacity (same as `save_layer_with_filter`).
        let layer_opacity = self.compositor.effective_layer_opacity(1.0);
        self.save_layer_impl(
            None, // bounds determined at restore time from content AABB + radius
            layer_opacity,
            [1.0, 1.0, 1.0],
            flui_types::painting::BlendMode::SrcOver,
            LayerFilterChain::new(), // no color-filter chain (image filter is separate)
        );
        // Mark the freshly-pushed SavedLayer with the image filter spec so that
        // `restore_layer` knows to emit DrawItem::Filter instead of OpacityLayer.
        // Log before the move so that the trace can capture `?spec` without needing
        // `Copy` on `ImageFilterSpec` (which was removed when `Chain` was added).
        tracing::trace!(
            ?spec,
            "WgpuPainter::save_layer_with_image_filter: image filter layer opened"
        );
        self.compositor.set_top_image_filter(spec);
    }

    /// Shared implementation for [`Self::save_layer`] /
    /// [`Self::save_layer_with_tint`] / [`Self::save_layer_with_filter`] /
    /// [`Self::save_layer_with_image_filter`]:
    /// snapshot the draw state and push a layer with the given composite
    /// `layer_opacity`, `layer_tint_rgb`, `layer_blend`, and color-filter chain.
    fn save_layer_impl(
        &mut self,
        bounds: Option<Rect<Pixels>>,
        layer_opacity: f32,
        layer_tint_rgb: [f32; 3],
        layer_blend: flui_types::painting::BlendMode,
        filters: LayerFilterChain,
    ) {
        // Convert bounds to [x, y, w, h] if provided.
        let bounds_array = bounds.map(|r| [r.left().0, r.top().0, r.width().0, r.height().0]);

        // Hand the current draw-record accumulators to the compositor; it wraps
        // them in a SavedLayer and resets current_opacity to 1.0 for the subtree.
        let saved_draw_order = std::mem::take(&mut self.draw_order);
        let saved_segment = std::mem::replace(&mut self.current_segment, DrawSegment::new());
        tracing::trace!(
            "WgpuPainter::save_layer: layer_opacity={:.3}, tint={:?}, blend={:?}, \
             filters={:?}, bounds={:?}",
            layer_opacity,
            layer_tint_rgb,
            layer_blend,
            filters,
            bounds_array
        );
        self.compositor.push_layer(
            saved_draw_order,
            saved_segment,
            layer_opacity,
            layer_tint_rgb,
            layer_blend,
            bounds_array,
            filters, // moved here after the trace
        );
    }

    /// Close the current compositing layer and composite it onto the parent.
    ///
    /// Must be called after each [`Self::save_layer`] / [`Self::save_layer_with_tint`] /
    /// `save_layer_with_filter` / `save_layer_with_image_filter` call.
    /// The layer's offscreen content is routed to the appropriate
    /// `DrawItem` variant:
    ///
    /// - **Empty layer** → nothing emitted (`RestoreOutcome::Empty`).
    /// - **Opacity ≈ 1.0 + white tint** → content re-integrated directly into the
    ///   parent draw order without an offscreen blit
    ///   (`RestoreOutcome::Reintegrate`).
    /// - **Opacity-/tint-/blend-mode layer** → `DrawItem::OpacityLayer`
    ///   (`RestoreOutcome::Composite`).
    /// - **Image filter layer** (opened via `save_layer_with_image_filter`) →
    ///   `DrawItem::Filter` with the computed `grown_bounds` and pass chain.
    ///
    /// Calling `restore_layer` without a matching open is a logic error; the
    /// compositor logs a warning and reinstates the pre-restore draw state
    /// (`RestoreOutcome::Underflow`).
    pub fn restore_layer(&mut self) {
        // Capture the offscreen content drawn since save_layer.
        let offscreen_final_segment =
            std::mem::replace(&mut self.current_segment, DrawSegment::new());
        let offscreen_items = std::mem::take(&mut self.draw_order);

        // Determine compositing bounds before calling pop_layer so the painter
        // can resolve the viewport fallback using its own `size` field.
        // We need the SavedLayer bounds — peek at the top without popping.
        // The compositor's pop_layer needs the already-resolved Rect, so we
        // resolve it here using the pattern from the original restore_layer.
        // We peek the bounds from the top of the layer_stack before delegating.
        let composite_bounds = self.compositor.peek_layer_bounds().map_or_else(
            || self.viewport_bounds(),
            |b| Rect::from_ltrb(px(b[0]), px(b[1]), px(b[0] + b[2]), px(b[1] + b[3])),
        );

        let outcome =
            self.compositor
                .pop_layer(offscreen_final_segment, offscreen_items, composite_bounds);

        match outcome {
            RestoreOutcome::Composite {
                offscreen_items,
                offscreen_final_segment,
                layer_opacity,
                tint_rgb,
                composite_bounds,
                layer_blend,
                layer_filter,
                image_filter,
                saved_segment,
                saved_draw_order,
            } => {
                // Restore the parent draw-record accumulators.
                self.current_segment = saved_segment;
                self.draw_order = saved_draw_order;

                // Finalize the current parent segment so the new draw item is
                // inserted at the correct Z-position in the draw order.
                let parent_segment =
                    std::mem::replace(&mut self.current_segment, DrawSegment::new());
                if !parent_segment.is_empty() {
                    self.draw_order.push(DrawItem::Segment(parent_segment));
                }

                // Route to DrawItem::Filter for bounds-growing image filters
                // (Morph/Blur); fall through to DrawItem::OpacityLayer for
                // plain opacity/tint/blend-mode layers.
                match image_filter {
                    Some(ImageFilterSpec::Morph { radius, op }) => {
                        // Package the offscreen content as a FilterOp.
                        //
                        // `FilterOp::input` is a flat `DrawSegment` consumed by
                        // `render_segment_to_offscreen` at replay time.  For a
                        // morphology layer opened with `save_layer_with_image_filter`,
                        // callers do not nest opacity layers inside, so
                        // `offscreen_items` is empty and `offscreen_final_segment`
                        // holds all the content.  If `offscreen_items` is non-empty
                        // (e.g., a nested texture from a draw_image call), log a
                        // debug trace — the items are silently ignored because the
                        // current FilterOp::input is a single DrawSegment; a future
                        // task can extend FilterOp::input to Vec<DrawItem> if needed.
                        if !offscreen_items.is_empty() {
                            tracing::debug!(
                                item_count = offscreen_items.len(),
                                "restore_layer(Morph): offscreen_items discarded; \
                                 FilterOp::input only captures the final DrawSegment. \
                                 Nested opacity layers inside a morphology layer are \
                                 not yet supported."
                            );
                        }
                        // `_ = layer_opacity` — morphology is applied as a DrawItem::Filter
                        // that composites directly; the opacity field is inherited via
                        // `effective_layer_opacity(1.0)` in `save_layer_with_image_filter`
                        // and is already baked into the save-layer setup.  The composite
                        // step (flush_texture_batch_premultiplied) uses REPLACE blend, so
                        // the group opacity is effectively 1.0 at this stage.
                        let _ = (layer_opacity, tint_rgb, layer_blend, layer_filter);

                        // Override composite_bounds for the image-filter path: use the
                        // content AABB of the drawn segment (conservative device-space
                        // union) rather than the full viewport.  This is the producer wiring
                        // that makes grown-bounds VRAM reduction real: when bounds=None was
                        // passed to save_layer_with_image_filter, composite_bounds was
                        // previously always the full viewport (the inert façade that Task 6
                        // identified and this code fixes).  content_aabb falls back to the
                        // viewport if the segment is empty or contains an un-boundable kind.
                        let composite_bounds = {
                            let vp = self.viewport_bounds();
                            Self::content_aabb(&offscreen_final_segment)
                                .and_then(|aabb| aabb.intersect(&vp))
                                .unwrap_or(vp)
                        };

                        // Growth via the shared helper (one source of truth for Morph).
                        let single_pass = ImageFilterPass::Morph { radius, op };
                        let growth_px =
                            px(super::cumulative_growth(std::slice::from_ref(&single_pass)));
                        let grown = composite_bounds.expand(growth_px);
                        let viewport_rect = self.viewport_bounds();
                        let grown_bounds =
                            grown.intersect(&viewport_rect).unwrap_or(composite_bounds);

                        // Compute the integer-aligned offscreen frame rectangle so BOTH
                        // composite arms (replay.rs + opacity_layer.rs nested Filter arm)
                        // share one authoritative value and cannot drift (non-negotiable #4).
                        let (fb_origin, fb_dim) = self.filter_fb_rect(grown_bounds);

                        tracing::trace!(
                            radius,
                            op = ?op,
                            content_bounds = ?composite_bounds,
                            grown_bounds = ?grown_bounds,
                            fb_origin = ?fb_origin,
                            fb_dim = ?fb_dim,
                            "WgpuPainter::restore_layer: queued DrawItem::Filter (Morph)"
                        );
                        self.draw_order.push(DrawItem::Filter(FilterOp {
                            input: offscreen_final_segment,
                            passes: smallvec![single_pass],
                            content_bounds: composite_bounds,
                            grown_bounds,
                            fb_origin,
                            fb_dim,
                        }));
                    }
                    Some(ImageFilterSpec::Blur { sigma_x, sigma_y }) => {
                        // Gaussian blur via two H/V sub-passes (separable, anisotropic).
                        // Identical seam to Morph: grow by kernel_radius(max(σx,σy))
                        // on each side, clip to viewport, emit DrawItem::Filter.
                        //
                        // Growth via the shared `cumulative_growth` helper (one source
                        // of truth for Blur; `kernel_radius` uses Impeller's √3·σ rule).
                        if !offscreen_items.is_empty() {
                            tracing::debug!(
                                item_count = offscreen_items.len(),
                                "restore_layer(Blur): offscreen_items discarded; \
                                 FilterOp::input only captures the final DrawSegment. \
                                 Nested opacity layers inside a blur layer are not yet supported."
                            );
                        }
                        let _ = (layer_opacity, tint_rgb, layer_blend, layer_filter);

                        // Content-AABB override (same rationale as the Morph arm above).
                        let composite_bounds = {
                            let vp = self.viewport_bounds();
                            Self::content_aabb(&offscreen_final_segment)
                                .and_then(|aabb| aabb.intersect(&vp))
                                .unwrap_or(vp)
                        };

                        let single_pass = ImageFilterPass::Blur { sigma_x, sigma_y };
                        let halo_px =
                            px(super::cumulative_growth(std::slice::from_ref(&single_pass)));
                        let grown = composite_bounds.expand(halo_px);
                        let viewport_rect = self.viewport_bounds();
                        let grown_bounds =
                            grown.intersect(&viewport_rect).unwrap_or(composite_bounds);

                        // Integer-aligned fb rect — one home for both composite arms.
                        let (fb_origin, fb_dim) = self.filter_fb_rect(grown_bounds);

                        tracing::trace!(
                            sigma_x,
                            sigma_y,
                            content_bounds = ?composite_bounds,
                            grown_bounds = ?grown_bounds,
                            fb_origin = ?fb_origin,
                            fb_dim = ?fb_dim,
                            "WgpuPainter::restore_layer: queued DrawItem::Filter (Blur)"
                        );
                        self.draw_order.push(DrawItem::Filter(FilterOp {
                            input: offscreen_final_segment,
                            passes: smallvec![single_pass],
                            content_bounds: composite_bounds,
                            grown_bounds,
                            fb_origin,
                            fb_dim,
                        }));
                    }
                    Some(ImageFilterSpec::Chain(passes)) => {
                        // Multi-pass Compose chain: the passes vec is already flattened
                        // at record time by `flatten_compose` in `backend.rs`.
                        //
                        // Identical offscreen_items guard as Morph/Blur arms above.
                        if !offscreen_items.is_empty() {
                            tracing::debug!(
                                item_count = offscreen_items.len(),
                                pass_count = passes.len(),
                                "restore_layer(Chain): offscreen_items discarded; \
                                 FilterOp::input only captures the final DrawSegment. \
                                 Nested opacity layers inside a Compose chain layer are \
                                 not yet supported."
                            );
                        }
                        let _ = (layer_opacity, tint_rgb, layer_blend, layer_filter);

                        // Content-AABB override (same rationale as the Morph/Blur arms above).
                        let composite_bounds = {
                            let vp = self.viewport_bounds();
                            Self::content_aabb(&offscreen_final_segment)
                                .and_then(|aabb| aabb.intersect(&vp))
                                .unwrap_or(vp)
                        };

                        // Cumulative growth = Σ per-pass radii (ColorMatrix/Identity = 0).
                        let growth_px = px(super::cumulative_growth(&passes));
                        let grown = composite_bounds.expand(growth_px);
                        let viewport_rect = self.viewport_bounds();
                        let grown_bounds =
                            grown.intersect(&viewport_rect).unwrap_or(composite_bounds);

                        // Integer-aligned fb rect — one home for both composite arms.
                        let (fb_origin, fb_dim) = self.filter_fb_rect(grown_bounds);

                        tracing::trace!(
                            pass_count = passes.len(),
                            content_bounds = ?composite_bounds,
                            grown_bounds = ?grown_bounds,
                            fb_origin = ?fb_origin,
                            fb_dim = ?fb_dim,
                            "WgpuPainter::restore_layer: queued DrawItem::Filter (Chain)"
                        );
                        self.draw_order.push(DrawItem::Filter(FilterOp {
                            input: offscreen_final_segment,
                            passes,
                            content_bounds: composite_bounds,
                            grown_bounds,
                            fb_origin,
                            fb_dim,
                        }));
                    }
                    None => {
                        // Plain opacity/tint/blend-mode composite — existing path.
                        tracing::trace!(
                            "WgpuPainter::restore_layer: queued OpacityLayer \
                             (opacity={:.3}, tint_rgb={:?}, blend={:?}, filters={:?}, bounds={:?})",
                            layer_opacity,
                            tint_rgb,
                            layer_blend,
                            layer_filter,
                            composite_bounds
                        );
                        self.draw_order
                            .push(DrawItem::OpacityLayer(PendingOpacityLayer {
                                items: offscreen_items,
                                final_segment: offscreen_final_segment,
                                opacity: layer_opacity,
                                tint_rgb,
                                bounds: composite_bounds,
                                blend: layer_blend,
                                filters: layer_filter,
                            }));
                    }
                }
            }
            RestoreOutcome::Reintegrate {
                offscreen_items,
                offscreen_final_segment,
                saved_segment,
                saved_draw_order,
            } => {
                // Restore the parent draw-record accumulators.
                self.current_segment = saved_segment;
                self.draw_order = saved_draw_order;

                // Opacity is ~1.0 AND tint is white — no compositing needed.
                // Finalize the parent's pre-save content into the draw order
                // BEFORE re-integrating the offscreen items so that parent
                // content renders beneath the layer subtree (correct Z-order).
                let parent_segment =
                    std::mem::replace(&mut self.current_segment, DrawSegment::new());
                if !parent_segment.is_empty() {
                    self.draw_order.push(DrawItem::Segment(parent_segment));
                }
                super::super::replay::GpuReplay::reintegrate_offscreen_content(
                    offscreen_final_segment,
                    offscreen_items,
                    1.0,
                    &mut self.draw_order,
                );
            }
            RestoreOutcome::Empty {
                saved_segment,
                saved_draw_order,
            } => {
                // Layer was empty — restore draw-record state, emit nothing.
                self.current_segment = saved_segment;
                self.draw_order = saved_draw_order;
            }
            RestoreOutcome::Underflow {
                current_segment,
                draw_order,
            } => {
                // Compositor already logged the warning and handled the
                // legacy opacity_stack fallback.
                //
                // Restore the records that were unconditionally captured before
                // the pop_layer call, so the frame's in-flight draws are not
                // wiped.  This matches the original painter behaviour where the
                // mem::take was guarded inside the `if let Some(saved)` block.
                self.current_segment = current_segment;
                self.draw_order = draw_order;
            }
        }

        tracing::trace!(
            "WgpuPainter::restore_layer: restored opacity={:.3}",
            self.compositor.current_opacity(),
        );
    }
}
