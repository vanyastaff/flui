//! GPU instancing for batch rendering
//!
//! Based on Bevy's instancing pattern, this module provides efficient rendering
//! of multiple primitives in a single draw call using GPU instancing.
//!
//! # Performance Benefits
//!
//! - **100 rectangles:** 1 draw call instead of 100 (100x reduction)
//! - **1000 UI elements:** ~10 draw calls instead of 1000 (100x reduction)
//! - **CPU overhead:** Minimal (single draw call submission)
//! - **GPU efficiency:** Parallel processing of instances
//!
//! # Architecture
//!
//! ```text
//! Vertex Buffer (shared quad):
//!   [0,0] [1,0] [1,1] [0,1]  ← Single quad vertices
//!
//! Instance Buffer (per-rectangle data):
//!   Instance 0: bounds=[10,10,100,50], color=[255,0,0,255], radii=[0,0,0,0]
//!   Instance 1: bounds=[20,70,150,100], color=[0,255,0,255], radii=[5,5,5,5]
//!   Instance 2: bounds=[200,10,80,80], color=[0,0,255,255], radii=[10,10,10,10]
//!   ...
//!
//! Draw call: draw_indexed(indices=6, instances=N)
//! GPU processes N rectangles in parallel!
//! ```

use bytemuck::{Pod, Zeroable};
use flui_types::{Point, Rect, geometry::Pixels, styling::Color};

/// Instance data for a rectangle
///
/// This is uploaded to GPU as an instance buffer. Each rectangle gets one
/// instance. The GPU shader reads this data per-instance and transforms a
/// shared quad via a full 2×3 affine.
///
/// ## Affine representation
///
/// The vertex shader applies `device = M * local + t` where:
/// - `M` is the 2×2 linear part stored column-major in `transform`:
///   `[a, b, c, d]` → `mat2x2(a, b, c, d)` (x_col=(a,b), y_col=(c,d)).
/// - `t` is the translation stored in `transform_translate.xy`.
/// - `local` is the vertex position in local shape space (derived from
///   `bounds` which holds `[x_local, y_local, width_local, height_local]`).
///
/// For the baked-AABB fast path (axis-aligned SrcOver rect/rrect), `M` is
/// the identity matrix and `t` is zero, so `device = local + 0 = local` —
/// byte-identical to the pre-affine instanced output.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct RectInstance {
    /// Local-space bounding box `[x, y, width, height]`.
    ///
    /// For the baked-AABB path this is already in device pixels (the CPU
    /// baked the transform into the bounds before constructing the instance).
    /// For the affine path this holds the untransformed local-space extent;
    /// the vertex shader applies `transform` + `transform_translate` to map
    /// it to device space.
    pub bounds: [f32; 4],

    /// Color `[r, g, b, a]` in linear 0–1 range.
    pub color: [f32; 4],

    /// Corner radii `[top_left, top_right, bottom_right, bottom_left]`.
    pub corner_radii: [f32; 4],

    /// 2×2 linear part of the affine transform, column-major:
    /// `[a, b, c, d]` → x-column `(a, b)`, y-column `(c, d)`.
    ///
    /// Identity: `[1, 0, 0, 1]`. For the baked-AABB path this is always
    /// identity (the transform was pre-baked into `bounds`).
    pub transform: [f32; 4],

    /// SDF clip rounded rectangle: `[x, y, width, height, radius_tl, radius_tr, radius_br, radius_bl]`.
    /// All zeros means no clip active. When non-zero, the fragment shader
    /// uses an SDF test to discard pixels outside this rounded rectangle.
    pub clip_rrect: [f32; 8],

    /// Clip-kind flag tagging which SDF the fragment shader should evaluate
    /// against `clip_rrect`.
    ///
    /// - `[0, _, _, _]` — no clip (also detected by `clip_rrect == [0; 8]`).
    /// - `[1, _, _, _]` — `sdRoundedBox` (standard rounded rectangle).
    /// - `[2, _, _, _]` — `sdRoundedSuperellipse` (iOS-squircle). For this
    ///   kind, `clip_rrect[4..8]` carries the single-radius-per-corner
    ///   `[r_tl, r_tr, r_br, r_bl]` interpretation (averaged from the
    ///   superellipse's separate-axis rx/ry per corner).
    ///
    /// Stored as `[u32; 4]` for 16-byte alignment with surrounding vec4
    /// instance attributes. Only the `.x` lane carries the kind; the other
    /// three lanes are padding.
    pub clip_kind: [u32; 4],

    /// Translation part of the affine transform: `[tx, ty, 0, 0]`.
    ///
    /// The `.zw` lanes are padding for 16-byte vec4 alignment in the shader.
    /// For the baked-AABB path this is always `[0, 0, 0, 0]`.
    pub transform_translate: [f32; 4],
}

impl RectInstance {
    /// Create a simple rectangular instance (baked-AABB fast path).
    ///
    /// `rect` must already be in device pixels (the caller bakes the current
    /// transform into the bounds before calling this). The affine fields are
    /// set to identity / zero so the vertex shader produces an identical result
    /// to the pre-affine path.
    #[must_use]
    pub fn rect(rect: Rect<Pixels>, color: Color) -> Self {
        Self {
            bounds: [rect.left().0, rect.top().0, rect.width().0, rect.height().0],
            color: color.to_f32_array(),
            corner_radii: [0.0; 4],
            // Identity 2×2: x-col=(1,0), y-col=(0,1).
            transform: [1.0, 0.0, 0.0, 1.0],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            transform_translate: [0.0; 4],
        }
    }

    // Cycle 4 E-5: deleted `RectInstance::rounded_rect(rect, color,
    // single_radius)` (uniform-corner shortcut). Zero callsites --
    // production paths use `rounded_rect_corners` (per-corner).

    /// Create an instance with per-corner radii (baked-AABB fast path).
    ///
    /// `rect` must already be in device pixels. The affine fields are identity /
    /// zero — byte-identical to the pre-affine baked-AABB path.
    #[must_use]
    pub fn rounded_rect_corners(
        rect: Rect<Pixels>,
        color: Color,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
    ) -> Self {
        Self {
            bounds: [rect.left().0, rect.top().0, rect.width().0, rect.height().0],
            color: color.to_f32_array(),
            corner_radii: [top_left, top_right, bottom_right, bottom_left],
            // Identity 2×2: x-col=(1,0), y-col=(0,1).
            transform: [1.0, 0.0, 0.0, 1.0],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            transform_translate: [0.0; 4],
        }
    }

    /// Create an instance for a **rotated or skewed** rect/rrect (affine path).
    ///
    /// `local_bounds` is the untransformed local-space bounding box
    /// `[x, y, width, height]`. The vertex shader applies the full affine
    /// `device = M * local + t` where `M` is the 2×2 linear part from the
    /// current transform and `t` is the translation.
    ///
    /// `linear_cols` is column-major `[a, b, c, d]`: x-column `(a, b)`,
    /// y-column `(c, d)`. Use `glam::Mat4::x_axis`/`y_axis` to extract it.
    ///
    /// `translation` is `[tx, ty]` from the current device transform.
    ///
    /// Corner radii default to zero; call `.with_clip_rrect` /
    /// `.with_clip_rsuperellipse` afterwards to attach an SDF clip.
    #[must_use]
    pub fn with_affine_transform(
        local_bounds: [f32; 4],
        color: Color,
        corner_radii: [f32; 4],
        linear_cols: [f32; 4],
        translation: [f32; 2],
    ) -> Self {
        Self {
            bounds: local_bounds,
            color: color.to_f32_array(),
            corner_radii,
            transform: linear_cols,
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            transform_translate: [translation[0], translation[1], 0.0, 0.0],
        }
    }

    /// Set the SDF clip rounded rectangle on this instance.
    ///
    /// The clip is specified as `[x, y, width, height, radius_tl, radius_tr, radius_br, radius_bl]`.
    /// All zeros means no clip. When non-zero, the fragment shader discards
    /// pixels that fall outside the rounded rectangle using an SDF test.
    /// Sets `clip_kind = 1` (rrect) when the clip is non-trivial; leaves
    /// `clip_kind = 0` when all-zero (no clip).
    #[must_use]
    pub fn with_clip_rrect(mut self, clip: [f32; 8]) -> Self {
        self.clip_rrect = clip;
        // Exact equality against the bit-exact `[0.0; 8]` "no clip" sentinel —
        // never set via arithmetic, so ULP slop is not a concern.
        #[expect(
            clippy::float_cmp,
            reason = "exact comparison against the bit-exact `[0.0; 8]` 'no clip' sentinel"
        )]
        let is_empty = clip == [0.0; 8];
        self.clip_kind = if is_empty { [0; 4] } else { [1, 0, 0, 0] };
        self
    }

    /// Set an SDF clip rounded-superellipse (iOS-squircle) on this instance.
    ///
    /// The 12-float superellipse uniform produced by
    /// `Painter::clip_rsuperellipse` carries separate-axis radii per corner.
    /// At the per-instance level we average each corner's `rx`/`ry` into a
    /// single radius to fit the existing `clip_rrect` slot — this is the
    /// "single-radius-per-corner" first-pass interpretation called out in
    /// the plan's Outstanding Questions Q9. Sets `clip_kind = 2`.
    ///
    /// Layout of `superellipse_clip`: `[x, y, w, h, tl_x, tl_y, tr_x, tr_y,
    /// br_x, br_y, bl_x, bl_y]`. Layout in the resulting `clip_rrect` slot:
    /// `[x, y, w, h, avg(tl_x,tl_y), avg(tr_x,tr_y), avg(br_x,br_y),
    /// avg(bl_x,bl_y)]`.
    #[must_use]
    pub fn with_clip_rsuperellipse(mut self, superellipse_clip: [f32; 12]) -> Self {
        // Exact equality against the bit-exact `[0.0; 12]` "no clip" sentinel.
        #[expect(
            clippy::float_cmp,
            reason = "exact comparison against the bit-exact `[0.0; 12]` 'no clip' sentinel"
        )]
        let is_empty = superellipse_clip == [0.0; 12];
        if is_empty {
            self.clip_rrect = [0.0; 8];
            self.clip_kind = [0; 4];
            return self;
        }
        let tl = 0.5 * (superellipse_clip[4] + superellipse_clip[5]);
        let tr = 0.5 * (superellipse_clip[6] + superellipse_clip[7]);
        let br = 0.5 * (superellipse_clip[8] + superellipse_clip[9]);
        let bl = 0.5 * (superellipse_clip[10] + superellipse_clip[11]);
        self.clip_rrect = [
            superellipse_clip[0],
            superellipse_clip[1],
            superellipse_clip[2],
            superellipse_clip[3],
            tl,
            tr,
            br,
            bl,
        ];
        self.clip_kind = [2, 0, 0, 0];
        self
    }

    // Cycle 4 E-5: deleted `RectInstance::with_transform(scale_x,
    // scale_y, translate_x, translate_y)` (per-instance transform
    // setter; zero callsites -- transform comes from the painter's
    // matrix stack, not from per-instance helpers).
    // `with_clip_rsuperellipse` was retained against the audit's
    // recommendation: 1 live callsite at `painter.rs:3519`
    // (`instance.with_clip_rsuperellipse(self.current_rsuperellipse_clip)`)
    // -- audit text claimed zero callsites but missed the method-style
    // dispatch on `instance` (vs type-path `RectInstance::`).

    /// Get wgpu vertex buffer layout for instance data.
    ///
    /// Locations 2–8 are unchanged from the pre-affine layout. Location 9 is
    /// the new `transform_translate` field appended at the end of the struct;
    /// appending keeps all existing field offsets byte-identical.
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Bounds [x, y, width, height] (location 2)
            2 => Float32x4,
            // Color [r, g, b, a] (location 3)
            3 => Float32x4,
            // Corner radii [tl, tr, br, bl] (location 4)
            4 => Float32x4,
            // 2×2 linear affine [a, b, c, d] column-major (location 5)
            5 => Float32x4,
            // Clip rrect part 1: [x, y, width, height] (location 6)
            6 => Float32x4,
            // Clip rrect part 2: [radius_tl, radius_tr, radius_br, radius_bl] (location 7)
            7 => Float32x4,
            // Clip kind: [kind, _pad, _pad, _pad] (location 8) — 0=none, 1=rrect, 2=rsuperellipse
            8 => Uint32x4,
            // Affine translation [tx, ty, 0, 0] (location 9)
            9 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for a circle or ellipse rendered via the affine instanced SDF path.
///
/// ## Layout and affine design
///
/// Mirrors [`RectInstance`]: the vertex shader applies `device = M * local + t` where:
/// - `M` is the 2×2 linear part stored column-major in `transform`:
///   `[a, b, c, d]` → x-column `(a, b)`, y-column `(c, d)`.
/// - `t` is the translation stored in `transform_translate.xy`.
/// - `local` is `unit_pos * radius + [cx, cy]`, where `unit_pos ∈ [-1,1]²`
///   and `[cx, cy]` is the local-space center from `center_radius.xy`.
///
/// ## Baked fast path (axis-aligned SrcOver circles)
///
/// [`CircleInstance::new`] produces `transform = diag(sx, sy)`,
/// `transform_translate = [cx_dev, cy_dev, 0, 0]`, and `center_radius = [0, 0, r, 0]`.
/// The vertex shader computes `local = unit_pos * radius` (origin-centered) then
/// `device = diag(sx,sy)*local + center_dev`. The device center lives in the
/// translation so the scale never multiplies it — matching the pre-affine path,
/// which added `center` directly and scaled only `normalized_pos * radius`.
/// (Putting the center inside `local` would double-scale it under any non-unit
/// CTM scale, e.g. HiDPI DPR>1.)
///
/// ## Affine path (rotated circles, ellipses, ovals under a general transform)
///
/// [`CircleInstance::with_affine_transform`] uses `center_radius = [0,0,1,0]`
/// (unit circle at origin). `transform` encodes `M_world * diag(rx, ry)`:
///   - circle of radius `r` at center `c` under `M_w + t_w`:
///     `linear = [M_w.a*r, M_w.b*r, M_w.c*r, M_w.d*r]`,
///     `translate = M_w * c + t_w`.
///   - ellipse with semi-axes `(rx, ry)`:
///     `linear = [M_w.a*rx, M_w.b*rx, M_w.c*ry, M_w.d*ry]`.
///
/// The SDF fragment evaluates `length(unit_pos) - 1.0`, which is 0 at the unit-circle
/// edge; `fwidth` gives ~1-device-px AA at any radius, scale, or rotation.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct CircleInstance {
    /// Radius in `.z`; `.xy` is unused (the center lives in `transform_translate`).
    ///
    /// Baked fast path: `[0, 0, radius, 0]`.
    /// Affine path: `[0, 0, 1, 0]` (radius folded into `transform`).
    pub center_radius: [f32; 4],

    /// Color `[r, g, b, a]` in linear 0–1 range.
    pub color: [f32; 4],

    /// 2×2 linear part of the affine transform, column-major:
    /// `[a, b, c, d]` → x-column `(a, b)`, y-column `(c, d)`.
    ///
    /// Baked fast path: `diag(sx, sy)` (per-axis canvas scale).
    /// Affine path: `M_world * diag(rx, ry)`.
    pub transform: [f32; 4],

    /// Translation part of the affine transform: `[tx, ty, 0, 0]` = the device
    /// center. Added AFTER `M` in the shader, so the linear part never scales it.
    ///
    /// Baked fast path: `[cx_dev, cy_dev, 0, 0]`.
    /// Affine path: device-space center = `M_w * center_local + t_w`.
    /// The `.zw` lanes are padding for 16-byte vec4 alignment.
    pub transform_translate: [f32; 4],
}

impl CircleInstance {
    /// Create a circle instance (baked fast path).
    ///
    /// `center` must already be in device pixels (the caller applies the
    /// current transform). The affine encodes axis-aligned scale as
    /// `diag(sx, sy)` and carries the device center in the translation, so the
    /// vertex shader produces `device = diag(sx,sy) * (unit * radius) + center_dev`
    /// — the scale never multiplies the center (matches the pre-affine path).
    ///
    /// `scale_xy` is `[sx, sy]` extracted from the current transform matrix.
    /// Pass `[1.0, 1.0]` for identity / uniform scale.
    #[must_use]
    pub fn new(center: Point<Pixels>, radius: f32, color: Color, scale_xy: [f32; 2]) -> Self {
        Self {
            // `center` is already in device pixels. It is carried in
            // `transform_translate` (added AFTER M in the shader) so the scale in
            // M = diag(sx,sy) never multiplies it — the local shape is the
            // origin-centered unit circle scaled by `radius`. center_radius.xy is
            // unused; only .z (radius) is read.
            center_radius: [0.0, 0.0, radius, 0.0],
            color: color.to_f32_array(),
            // Baked scale: identity rotation, per-axis scale as diag(sx, sy).
            // x-col = (sx, 0), y-col = (0, sy).
            transform: [scale_xy[0], 0.0, 0.0, scale_xy[1]],
            transform_translate: [center.x.0, center.y.0, 0.0, 0.0],
        }
    }

    /// Create a circle or ellipse instance for the full-affine SDF path.
    ///
    /// Use this for any SrcOver circle/ellipse that needs rotation, shear,
    /// or non-uniform scale (rotated circles, oriented ellipses, ovals under
    /// a general world transform).
    ///
    /// The unit circle at origin is the canonical local shape: `center_radius =
    /// [0, 0, 1, 0]`. The vertex shader applies `device = M * unit_pos + t` and
    /// passes `unit_pos` to the fragment, which evaluates `length(unit_pos) - 1.0`
    /// as the signed distance — correct for any affine (fwidth gives ~1-device-px AA).
    ///
    /// `linear_cols` encodes `M_world * diag(rx, ry)` column-major `[a, b, c, d]`:
    ///   - circle radius `r` under `M_w`: `[M_w.a*r, M_w.b*r, M_w.c*r, M_w.d*r]`
    ///   - ellipse `(rx, ry)` under `M_w`: `[M_w.a*rx, M_w.b*rx, M_w.c*ry, M_w.d*ry]`
    ///
    /// `translation` is `[tx, ty]` = `M_w * center_local + t_w` in device pixels.
    #[must_use]
    pub fn with_affine_transform(
        linear_cols: [f32; 4],
        color: Color,
        translation: [f32; 2],
    ) -> Self {
        Self {
            // Unit circle at local origin; the affine encodes radius + world transform.
            center_radius: [0.0, 0.0, 1.0, 0.0],
            color: color.to_f32_array(),
            transform: linear_cols,
            transform_translate: [translation[0], translation[1], 0.0, 0.0],
        }
    }

    // Cycle 4 E-5: deleted `CircleInstance::ellipse(center, radius_x,
    // radius_y, color)`. Zero call sites — production paths use
    // `CircleInstance::new` with scale_xy. When per-axis radii independent of
    // the canvas scale are needed it relands with a concrete first consumer.

    /// Get wgpu vertex buffer layout for instance data.
    ///
    /// Locations 2–4 are unchanged from the pre-PR-2 layout. Location 5 is the
    /// new `transform_translate` field appended at the end of the struct;
    /// appending keeps all existing field offsets byte-identical.
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Center + radius [cx, cy, radius, _] (location 2)
            2 => Float32x4,
            // Color [r, g, b, a] (location 3)
            3 => Float32x4,
            // 2×2 linear affine [a, b, c, d] column-major (location 4)
            4 => Float32x4,
            // Affine translation [tx, ty, 0, 0] (location 5)
            5 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<CircleInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for an arc (pie sector) rendered via the affine instanced SDF path.
///
/// ## Layout and affine design
///
/// Mirrors [`CircleInstance`]'s affine path: the vertex shader applies
/// `device = M * local + t` where:
/// - `M` is the 2×2 linear part stored column-major in `transform`:
///   `[a, b, c, d]` → x-column `(a, b)`, y-column `(c, d)`.
/// - `t` is the translation stored in `transform_translate.xy` — the device
///   **center**, added AFTER M so the linear part never scales it (the PR-2
///   double-scale fix applied to arcs).
/// - `local` is `unit_pos` (origin-centered unit disk; the radius is folded into M).
///
/// ## Single (affine) path
///
/// Every SrcOver filled arc — axis-aligned or rotated — routes through
/// [`ArcInstance::with_affine_transform`], which uses `center_radius = [0,0,1,0]`
/// (unit circle at origin, radius folded into `transform`). `transform` encodes
/// `M_world * r`:
///   `linear = [M_w.a*r, M_w.b*r, M_w.c*r, M_w.d*r]`,
///   `translate = M_w * center_local + t_w`.
/// The axis-aligned case is just `M_world = diag(sx, sy)`, so a separate baked
/// constructor is unnecessary.
///
/// The fragment evaluates `length(unit_pos) - 1.0` for radial AA and a
/// screen-space angular SDF for the sector edges; `fwidth` gives ~1-device-px AA
/// at any radius, scale, or rotation.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ArcInstance {
    /// Always `[0, 0, 1, 0]`: unit circle at origin, radius folded into `transform`;
    /// `.xy` unused (the center lives in `transform_translate`).
    pub center_radius: [f32; 4],

    /// Angles in radians `[start_angle, sweep_angle, 0, 0]`.
    ///
    /// `start_angle`: where the arc begins (0 = +X right, π/2 = +Y down, π = left).
    /// `sweep_angle`: how much to sweep (positive = clockwise in screen Y-down space,
    /// negative = counter-clockwise). A sweep of ±2π or larger means a full circle.
    pub angles: [f32; 4],

    /// Color `[r, g, b, a]` in linear 0–1 range.
    pub color: [f32; 4],

    /// 2×2 linear part of the affine transform, column-major:
    /// `[a, b, c, d]` → x-column `(a, b)`, y-column `(c, d)`.
    /// Encodes `M_world * r` (radius folded in); axis-aligned is `diag(sx, sy) * r`.
    pub transform: [f32; 4],

    /// Translation: `[tx, ty, 0, 0]` = the device center `M_w * center_local + t_w`.
    /// Added AFTER `M` in the shader, so the linear part never scales it.
    /// The `.zw` lanes are padding for 16-byte vec4 alignment.
    pub transform_translate: [f32; 4],
}

impl ArcInstance {
    // PR-2b: deleted `ArcInstance::new(center, radius, start, sweep, color, scale_xy)`
    // (baked-AABB fast path for axis-aligned SrcOver arcs).
    //
    // All SrcOver filled arcs now route through `with_affine_transform` regardless
    // of axis-alignment: the full-affine path is correct for both axis-aligned and
    // rotated arcs. The old `new` constructor encoded `diag(sx,sy)` as the linear
    // part and placed the pre-transformed center in `transform_translate` — the
    // same semantics as `with_affine_transform` with `M_world = diag(sx,sy)` and
    // `radius` folded separately. `with_affine_transform` handles this uniformly.
    // Zero callers outside this crate; the unit test is updated below.

    /// Create an arc instance for the full-affine SDF path.
    ///
    /// Use this for any SrcOver arc that needs rotation or non-uniform scale
    /// (rotated arcs under a general world transform).
    ///
    /// The unit circle at origin is the canonical local shape: `center_radius =
    /// [0, 0, 1, 0]`. The vertex shader applies `device = M * unit_pos * 1 + t`
    /// and passes `unit_pos` to the fragment, which evaluates the radial and
    /// angular SDFs — correct for any affine (fwidth gives ~1-device-px AA).
    ///
    /// `linear_cols` encodes `M_world * r` column-major `[a, b, c, d]`:
    ///   `linear = [M_w.a*r, M_w.b*r, M_w.c*r, M_w.d*r]`
    ///
    /// `translation` is `[tx, ty]` = `M_w * center_local + t_w` in device pixels.
    #[must_use]
    pub fn with_affine_transform(
        linear_cols: [f32; 4],
        start_angle: f32,
        sweep_angle: f32,
        color: Color,
        translation: [f32; 2],
    ) -> Self {
        Self {
            // Unit circle at local origin; the affine encodes radius + world transform.
            center_radius: [0.0, 0.0, 1.0, 0.0],
            angles: [start_angle, sweep_angle, 0.0, 0.0],
            color: color.to_f32_array(),
            transform: linear_cols,
            transform_translate: [translation[0], translation[1], 0.0, 0.0],
        }
    }

    // Cycle 4 E-5: deleted `ArcInstance::ellipse(...)` (zero call sites).
    // Re-lands with a concrete consumer when needed. All SrcOver arcs now use
    // `with_affine_transform`; an elliptical arc folds `M_world * diag(rx, ry)`
    // into `linear_cols` (mirroring `oval`).

    /// Get wgpu vertex buffer layout for instance data.
    ///
    /// Locations 2–5 are unchanged from the pre-PR-2b layout. Location 6 is the
    /// new `transform_translate` field appended at the end of the struct;
    /// appending keeps all existing field offsets byte-identical.
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Center + radius [0, 0, radius, _] (location 2)
            2 => Float32x4,
            // Angles [start, sweep, 0, 0] (location 3)
            3 => Float32x4,
            // Color [r, g, b, a] (location 4)
            4 => Float32x4,
            // 2×2 linear affine [a, b, c, d] column-major (location 5)
            5 => Float32x4,
            // Affine translation [tx, ty, 0, 0] (location 6)
            6 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ArcInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for a textured quad (images, sprites, icons)
///
/// Used for rendering images, icons, and sprites with GPU instancing.
/// Supports texture atlases via UV coordinates.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct TextureInstance {
    /// Destination rectangle [x, y, width, height] in screen space
    pub dst_rect: [f32; 4],

    /// Source UV coordinates [u_min, v_min, u_max, v_max] in 0-1 range
    /// For whole texture: [0.0, 0.0, 1.0, 1.0]
    /// For atlas region: [u_start, v_start, u_end, v_end]
    pub src_uv: [f32; 4],

    /// Color tint [r, g, b, a] in 0-1 range
    /// Use [1.0, 1.0, 1.0, 1.0] for no tint
    pub tint: [f32; 4],

    /// Transform (rotation and additional translation)
    /// [cos(angle), sin(angle), translate_x, translate_y]
    /// For no rotation: [1.0, 0.0, 0.0, 0.0]
    pub transform: [f32; 4],
}

impl TextureInstance {
    /// Create a simple textured quad instance
    ///
    /// # Arguments
    /// * `dst_rect` - Destination rectangle in screen coordinates
    /// * `tint` - Color tint (use Color::WHITE for no tint)
    #[must_use]
    pub fn new(dst_rect: flui_types::Rect<flui_types::geometry::Pixels>, tint: Color) -> Self {
        Self {
            dst_rect: [
                dst_rect.left().0,
                dst_rect.top().0,
                dst_rect.width().0,
                dst_rect.height().0,
            ],
            src_uv: [0.0, 0.0, 1.0, 1.0], // Full texture
            tint: tint.to_f32_array(),
            transform: [1.0, 0.0, 0.0, 0.0], // No rotation
        }
    }

    /// Create a textured quad with custom UV coordinates (for texture atlas)
    ///
    /// # Arguments
    /// * `dst_rect` - Destination rectangle in screen coordinates
    /// * `src_uv` - Source UV rectangle [u_min, v_min, u_max, v_max]
    /// * `tint` - Color tint
    #[must_use]
    pub fn with_uv(
        dst_rect: flui_types::Rect<flui_types::geometry::Pixels>,
        src_uv: [f32; 4],
        tint: Color,
    ) -> Self {
        Self {
            dst_rect: [
                dst_rect.left().0,
                dst_rect.top().0,
                dst_rect.width().0,
                dst_rect.height().0,
            ],
            src_uv,
            tint: tint.to_f32_array(),
            transform: [1.0, 0.0, 0.0, 0.0],
        }
    }

    // Cycle 4 E-5: deleted `TextureInstance::with_rotation(dst_rect,
    // angle, tint)`. Zero callsites -- production paths use
    // `TextureInstance::with_uv` (canonical, 5 callsites in
    // painter.rs) and the painter's matrix stack handles rotation
    // composition. `TextureInstance::with_uv` was retained against
    // the audit's recommendation because it IS live (audit text
    // claimed otherwise; grep proved 5 painter callsites).

    /// Create a textured quad with custom UV and a raw `[f32; 4]` tint.
    ///
    /// Used by the offscreen-layer composite path, which needs a fractional
    /// premultiplied tint `(C.r*O, C.g*O, C.b*O, O)` that an 8-bit [`Color`]
    /// would quantize prematurely. The shader multiplies the sampled texel by
    /// this tint (`tex_color * in.tint`).
    #[must_use]
    pub fn with_uv_tint_f32(
        dst_rect: flui_types::Rect<flui_types::geometry::Pixels>,
        src_uv: [f32; 4],
        tint: [f32; 4],
    ) -> Self {
        Self {
            dst_rect: [
                dst_rect.left().0,
                dst_rect.top().0,
                dst_rect.width().0,
                dst_rect.height().0,
            ],
            src_uv,
            tint,
            transform: [1.0, 0.0, 0.0, 0.0],
        }
    }

    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Destination rect (location 2)
            2 => Float32x4,
            // Source UV (location 3)
            3 => Float32x4,
            // Tint color (location 4)
            4 => Float32x4,
            // Transform (location 5)
            5 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TextureInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Gradient Instances (from effects.rs for API consistency)
// =============================================================================

/// Linear gradient instance data for GPU instancing
///
/// See `crate::painter::effects::LinearGradientInstance` for full
/// documentation.
pub use super::effects::LinearGradientInstance;

impl LinearGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Bounds (location 2)
            2 => Float32x4,
            // Gradient start (location 3)
            3 => Float32x2,
            // Gradient end (location 4)
            4 => Float32x2,
            // Corner radii (location 5)
            5 => Float32x4,
            // Stop count (location 6)
            6 => Uint32,
            // Stop offset (location 7)
            7 => Uint32,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LinearGradientInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Radial gradient instance data for GPU instancing
pub use super::effects::RadialGradientInstance;

impl RadialGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Bounds (location 2)
            2 => Float32x4,
            // Center (location 3)
            3 => Float32x2,
            // Radius + padding (location 4)
            4 => Float32x2,
            // Corner radii (location 5)
            5 => Float32x4,
            // Stop count (location 6)
            6 => Uint32,
            // Stop offset (location 7)
            7 => Uint32,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RadialGradientInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Sweep Gradient Instances
// =============================================================================

/// Sweep gradient instance data for GPU instancing
pub use super::effects::SweepGradientInstance;

impl SweepGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Bounds (location 2)
            2 => Float32x4,
            // Center (location 3)
            3 => Float32x2,
            // Angles [start, end] (location 4)
            4 => Float32x2,
            // Corner radii (location 5)
            5 => Float32x4,
            // Stop count (location 6)
            6 => Uint32,
            // Stop offset (location 7)
            7 => Uint32,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SweepGradientInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Shadow Instances
// =============================================================================

/// Shadow instance data for GPU instancing
pub use super::effects::ShadowInstance;

impl ShadowInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Shadow bounds (location 2)
            2 => Float32x4,
            // Rect pos (location 3)
            3 => Float32x2,
            // Rect size (location 4)
            4 => Float32x2,
            // Corner radius + padding (location 5)
            5 => Float32x4,
            // Shadow offset (location 6)
            6 => Float32x2,
            // Blur sigma + padding (location 7)
            7 => Float32x2,
            // Shadow color (location 8)
            8 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ShadowInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Generic Instance Batch
// =============================================================================

/// Batch of instances ready for rendering
///
/// Groups instances by type for efficient rendering.
///
/// `Clone` is derived so that a recorded [`super::command_ir::DrawSegment`] can be
/// snapshotted before replay — used by the deterministic-replay test (T11) to assert that
/// `GpuReplay::submit` does not mutate the IR.
#[derive(Debug, Clone)]
pub struct InstanceBatch<T> {
    /// Instance data
    pub instances: Vec<T>,

    /// Maximum instances before auto-flush
    pub max_instances: usize,
}

impl<T> InstanceBatch<T> {
    /// Create a new instance batch
    #[must_use]
    pub fn new(max_instances: usize) -> Self {
        Self {
            instances: Vec::with_capacity(max_instances),
            max_instances,
        }
    }

    /// Add an instance to the batch
    ///
    /// Returns true if batch is full and should be flushed.
    #[must_use]
    pub fn add(&mut self, instance: T) -> bool {
        self.instances.push(instance);
        self.instances.len() >= self.max_instances
    }

    /// Check if batch is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Get number of instances
    #[must_use]
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Clear the batch
    pub fn clear(&mut self) {
        self.instances.clear();
    }

    /// Get instance data as byte slice
    pub fn as_bytes(&self) -> &[u8]
    where
        T: Pod,
    {
        bytemuck::cast_slice(&self.instances)
    }
}

impl<T> Default for InstanceBatch<T> {
    fn default() -> Self {
        Self::new(1024) // Default: 1024 instances per batch
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact expected values produced by exact arithmetic"
)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_rect_instance_size() {
        // RectInstance field layout (all #[repr(C)], tightly packed):
        //   bounds:              [f32; 4]  = 16 bytes
        //   color:               [f32; 4]  = 16 bytes
        //   corner_radii:        [f32; 4]  = 16 bytes
        //   transform:           [f32; 4]  = 16 bytes  ← 2×2 linear affine
        //   clip_rrect:          [f32; 8]  = 32 bytes
        //   clip_kind:           [u32; 4]  = 16 bytes
        //   transform_translate: [f32; 4]  = 16 bytes  ← appended for affine path
        //   Total: 128 bytes
        assert_eq!(std::mem::size_of::<RectInstance>(), 128);
    }

    #[test]
    fn test_circle_instance_size() {
        // CircleInstance field layout (all #[repr(C)], tightly packed):
        //   center_radius:       [f32; 4]  = 16 bytes
        //   color:               [f32; 4]  = 16 bytes
        //   transform:           [f32; 4]  = 16 bytes  ← 2×2 linear affine
        //   transform_translate: [f32; 4]  = 16 bytes  ← appended for affine path
        //   Total: 64 bytes
        assert_eq!(std::mem::size_of::<CircleInstance>(), 64);
    }

    #[test]
    fn test_arc_instance_size() {
        // ArcInstance field layout (all #[repr(C)], tightly packed):
        //   center_radius:       [f32; 4]  = 16 bytes
        //   angles:              [f32; 4]  = 16 bytes
        //   color:               [f32; 4]  = 16 bytes
        //   transform:           [f32; 4]  = 16 bytes  ← 2×2 linear affine
        //   transform_translate: [f32; 4]  = 16 bytes  ← appended for affine path
        //   Total: 80 bytes
        assert_eq!(std::mem::size_of::<ArcInstance>(), 80);
    }

    #[test]
    fn test_texture_instance_size() {
        // Verify struct is tightly packed for GPU
        assert_eq!(
            std::mem::size_of::<TextureInstance>(),
            16 * 4 // 16 floats = 64 bytes
        );
    }

    #[test]
    fn test_instance_batch() {
        let mut batch = InstanceBatch::<RectInstance>::new(2);

        // Add first instance
        let should_flush = batch.add(RectInstance::rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(50.0)),
            Color::RED,
        ));
        assert!(!should_flush);
        assert_eq!(batch.len(), 1);

        // Add second instance (reaches max)
        let should_flush = batch.add(RectInstance::rect(
            Rect::from_ltrb(px(10.0), px(10.0), px(110.0), px(60.0)),
            Color::BLUE,
        ));
        assert!(should_flush);
        assert_eq!(batch.len(), 2);

        // Clear
        batch.clear();
        assert!(batch.is_empty());
    }

    #[test]
    fn test_color_conversion() {
        let instance = RectInstance::rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
            Color::RED,
        );

        // RED should be [1.0, 0.0, 0.0, 1.0] in normalized form
        assert_eq!(instance.color[0], 1.0); // R
        assert_eq!(instance.color[1], 0.0); // G
        assert_eq!(instance.color[2], 0.0); // B
        assert_eq!(instance.color[3], 1.0); // A
    }

    #[test]
    fn test_rect_bounds_mapping() {
        // `rect` maps Rect fields to [left, top, width, height] — not ltrb.
        let instance = RectInstance::rect(
            Rect::from_ltrb(px(10.0), px(20.0), px(110.0), px(70.0)),
            Color::RED,
        );
        assert_eq!(instance.bounds[0], 10.0); // x = left
        assert_eq!(instance.bounds[1], 20.0); // y = top
        assert_eq!(instance.bounds[2], 100.0); // width = right − left
        assert_eq!(instance.bounds[3], 50.0); // height = bottom − top
    }

    #[test]
    fn test_rect_default_clip_is_no_clip() {
        // Plain rect: clip_rrect must be all-zeros and clip_kind must be 0
        // (no SDF clip active). The fragment shader reads clip_kind[0] == 0
        // as "skip clip test". The affine fields must be identity / zero.
        let instance = RectInstance::rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(50.0), px(50.0)),
            Color::RED,
        );
        assert_eq!(instance.clip_rrect, [0.0; 8]);
        assert_eq!(instance.clip_kind, [0u32; 4]);
        // Identity 2×2 and zero translation — baked-AABB path.
        assert_eq!(instance.transform, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(instance.transform_translate, [0.0; 4]);
    }

    /// The baked-AABB `rect` and `rounded_rect_corners` constructors must
    /// produce identity affine fields so the vertex shader yields
    /// `device = identity*local + 0 = local` — byte-identical to the
    /// pre-affine path.
    #[test]
    fn baked_aabb_constructors_have_identity_affine() {
        let r = Rect::from_ltrb(px(10.0), px(20.0), px(110.0), px(70.0));
        let plain = RectInstance::rect(r, Color::RED);
        assert_eq!(plain.transform, [1.0, 0.0, 0.0, 1.0], "identity 2×2");
        assert_eq!(plain.transform_translate, [0.0; 4], "zero translation");

        let rounded = RectInstance::rounded_rect_corners(r, Color::RED, 4.0, 4.0, 4.0, 4.0);
        assert_eq!(
            rounded.transform,
            [1.0, 0.0, 0.0, 1.0],
            "identity 2×2 rrect"
        );
        assert_eq!(
            rounded.transform_translate, [0.0; 4],
            "zero translation rrect"
        );
    }

    /// `with_affine_transform` must round-trip the caller's linear + translate
    /// components into the corresponding GPU fields without mangling them.
    #[test]
    fn with_affine_transform_stores_linear_and_translate() {
        // 30° rotation: cos30≈0.866, sin30=0.5
        let cos30 = std::f32::consts::FRAC_PI_6.cos();
        let sin30 = std::f32::consts::FRAC_PI_6.sin();
        // Column-major: x-col=(cos,-sin), y-col=(sin,cos) for CCW rotation.
        // But glam uses x_axis=(cos,sin), y_axis=(-sin,cos) for CW screen rotation;
        // we just round-trip whatever the caller provides — the math is the
        // shader's concern.
        let linear = [cos30, sin30, -sin30, cos30];
        let translation = [100.0_f32, 200.0_f32];
        let local_bounds = [0.0_f32, 0.0, 80.0, 40.0];
        let radii = [5.0_f32, 5.0, 5.0, 5.0];

        let instance = RectInstance::with_affine_transform(
            local_bounds,
            Color::BLUE,
            radii,
            linear,
            translation,
        );

        assert_eq!(instance.bounds, local_bounds);
        assert_eq!(instance.corner_radii, radii);
        assert_eq!(instance.transform, linear, "linear 2×2 stored verbatim");
        assert_eq!(
            instance.transform_translate[0], translation[0],
            "tx stored in .x"
        );
        assert_eq!(
            instance.transform_translate[1], translation[1],
            "ty stored in .y"
        );
        assert_eq!(instance.transform_translate[2], 0.0, ".z padding is zero");
        assert_eq!(instance.transform_translate[3], 0.0, ".w padding is zero");
        // Default: no clip.
        assert_eq!(instance.clip_kind, [0u32; 4]);
        assert_eq!(instance.clip_rrect, [0.0; 8]);
    }

    #[test]
    fn test_with_clip_rrect_sets_kind_one() {
        // Non-zero clip_rrect must set clip_kind[0] = 1 (sdRoundedBox).
        let clip: [f32; 8] = [5.0, 5.0, 90.0, 40.0, 4.0, 4.0, 4.0, 4.0];
        let instance = RectInstance::rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(50.0)),
            Color::RED,
        )
        .with_clip_rrect(clip);
        assert_eq!(instance.clip_rrect, clip);
        assert_eq!(instance.clip_kind[0], 1u32);
        // Padding lanes must be zero.
        assert_eq!(instance.clip_kind[1], 0u32);
        assert_eq!(instance.clip_kind[2], 0u32);
        assert_eq!(instance.clip_kind[3], 0u32);
    }

    #[test]
    fn test_with_clip_rrect_all_zeros_keeps_no_clip() {
        // Passing the all-zeros sentinel must leave clip_kind == 0.
        let instance = RectInstance::rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(50.0)),
            Color::RED,
        )
        .with_clip_rrect([0.0; 8]);
        assert_eq!(instance.clip_kind, [0u32; 4]);
    }

    #[test]
    fn test_with_clip_rsuperellipse_sets_kind_two() {
        // Non-zero squircle clip must set clip_kind[0] = 2.
        let se: [f32; 12] = [
            0.0, 0.0, 100.0, 50.0, 8.0, 10.0, 8.0, 10.0, 8.0, 10.0, 8.0, 10.0,
        ];
        let instance = RectInstance::rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(50.0)),
            Color::RED,
        )
        .with_clip_rsuperellipse(se);
        assert_eq!(instance.clip_kind[0], 2u32);
        // Averaged corner radii: avg(8,10) = 9.0 for each corner.
        assert_eq!(instance.clip_rrect[4], 9.0);
        assert_eq!(instance.clip_rrect[5], 9.0);
        assert_eq!(instance.clip_rrect[6], 9.0);
        assert_eq!(instance.clip_rrect[7], 9.0);
    }

    #[test]
    fn test_gradient_instance_sizes() {
        // LinearGradientInstance:
        //   bounds[4]=16  gradient_start[2]=8  gradient_end[2]=8
        //   corner_radii[4]=16  stop_count(u32)=4  stop_offset(u32)=4  padding[2u32]=8
        //   Total: 64 bytes
        assert_eq!(std::mem::size_of::<LinearGradientInstance>(), 64);

        // RadialGradientInstance:
        //   bounds[4]=16  center[2]=8  radius(f32)=4  padding1(f32)=4
        //   corner_radii[4]=16  stop_count(u32)=4  stop_offset(u32)=4  padding2[2u32]=8
        //   Total: 64 bytes
        assert_eq!(std::mem::size_of::<RadialGradientInstance>(), 64);

        // SweepGradientInstance:
        //   bounds[4]=16  center[2]=8  angles[2]=8
        //   corner_radii[4]=16  stop_count(u32)=4  stop_offset(u32)=4  padding[2u32]=8
        //   Total: 64 bytes
        assert_eq!(std::mem::size_of::<SweepGradientInstance>(), 64);
    }

    #[test]
    fn test_circle_instance_field_values() {
        use flui_types::{Point, geometry::Pixels};
        let center = Point::new(flui_types::geometry::Pixels(50.0), Pixels(75.0));
        let instance = CircleInstance::new(center, 20.0, Color::RED, [1.0, 1.0]);
        // The device center lives in `transform_translate` (added AFTER M in the
        // shader) so M = diag(sx,sy) never double-scales it. `center_radius.xy`
        // is unused; `.z` is the radius. (Unit-level guard for the baked-path
        // double-scale bug — see GPU test C4.)
        assert_eq!(instance.center_radius[0], 0.0); // unused
        assert_eq!(instance.center_radius[1], 0.0); // unused
        assert_eq!(instance.center_radius[2], 20.0); // radius
        assert_eq!(instance.center_radius[3], 0.0); // padding
        assert_eq!(instance.transform_translate[0], 50.0); // device center x
        assert_eq!(instance.transform_translate[1], 75.0); // device center y
    }

    /// `CircleInstance::new` must encode scale_xy as `diag(sx, sy)` in the 2×2
    /// transform field so the vertex shader computes the correct bounding-quad
    /// size, and carry the device center in `transform_translate` (so M never
    /// scales it). A non-origin center proves the center → translate mapping.
    #[test]
    fn circle_instance_scale_propagates_to_transform() {
        use flui_types::{Point, geometry::Pixels};
        let center = Point::new(Pixels(12.0), Pixels(34.0));
        let identity = CircleInstance::new(center, 10.0, Color::RED, [1.0, 1.0]);
        // diag(1,1): x-col=(1,0), y-col=(0,1)
        assert_eq!(identity.transform, [1.0, 0.0, 0.0, 1.0], "identity diag");
        // Center carried in the translation (NOT scaled by M).
        assert_eq!(identity.transform_translate[0], 12.0, "tx = center x");
        assert_eq!(identity.transform_translate[1], 34.0, "ty = center y");

        let scaled = CircleInstance::new(center, 10.0, Color::RED, [2.5, 3.0]);
        // diag(2.5, 3.0): x-col=(2.5,0), y-col=(0,3.0)
        assert_eq!(scaled.transform, [2.5, 0.0, 0.0, 3.0], "scaled diag");
        // Center is still the raw device center — diag(sx,sy) must NOT multiply it.
        assert_eq!(scaled.transform_translate[0], 12.0, "tx unscaled by sx");
        assert_eq!(scaled.transform_translate[1], 34.0, "ty unscaled by sy");
    }

    /// `CircleInstance::with_affine_transform` must store a unit circle at origin
    /// and round-trip the caller's linear + translate without mangling.
    #[test]
    fn circle_with_affine_transform_stores_unit_circle_and_affine() {
        // 30° rotation × radius 20 for a circle: col-major M_w*r.
        let cos30 = std::f32::consts::FRAC_PI_6.cos();
        let sin30 = std::f32::consts::FRAC_PI_6.sin();
        let r = 20.0_f32;
        // CW screen rotation: x-col=(cos,-sin)*r, y-col=(sin,cos)*r (col-major [a,b,c,d]).
        let linear = [cos30 * r, -sin30 * r, sin30 * r, cos30 * r];
        let translation = [64.0_f32, 64.0_f32];

        let instance = CircleInstance::with_affine_transform(linear, Color::BLUE, translation);

        // center_radius must be the unit circle at origin.
        assert_eq!(instance.center_radius[0], 0.0, "cx=0");
        assert_eq!(instance.center_radius[1], 0.0, "cy=0");
        assert_eq!(instance.center_radius[2], 1.0, "radius=1 (unit circle)");
        assert_eq!(instance.center_radius[3], 0.0, "padding");
        // Linear stored verbatim.
        assert_eq!(instance.transform, linear, "linear 2×2 round-trip");
        // Translation stored in xy; zw are padding.
        assert_eq!(instance.transform_translate[0], 64.0, "tx");
        assert_eq!(instance.transform_translate[1], 64.0, "ty");
        assert_eq!(instance.transform_translate[2], 0.0, "pad.z");
        assert_eq!(instance.transform_translate[3], 0.0, "pad.w");
    }

    /// `ArcInstance::with_affine_transform` encodes the baked axis-aligned path
    /// via `diag(sx*r, sy*r)` in the 2×2 transform field and carries the device
    /// center in `transform_translate` (so M never double-scales it).
    ///
    /// The old `ArcInstance::new` baked-fast-path stored `[sx, sy, 0, 0]` in
    /// `transform` (a flat 4-float encode). The PR-2b col-major layout
    /// `[sx*r, 0, 0, sy*r]` = `diag(sx*r, sy*r)` is equivalent under identity
    /// rotation and correctly composes with non-unit canvas scale + rotation.
    #[test]
    fn arc_instance_scale_propagates_to_transform() {
        // Simulate the shapes.rs baked-path encoding: axis-aligned scale(2,2),
        // center at device (12,34), radius 10.
        // Translation: M_world * center_local + t_world = diag(2,2)*[6,17]+[0,0] = [12,34].
        // linear_cols = [sx*r, 0, 0, sy*r] = [2*10, 0, 0, 2*10] = [20, 0, 0, 20].
        let sx = 2.0_f32;
        let sy = 2.0_f32;
        let r = 10.0_f32;
        let tx = 12.0_f32;
        let ty = 34.0_f32;

        let identity = ArcInstance::with_affine_transform(
            [1.0 * r, 0.0, 0.0, 1.0 * r], // diag(1,1)*r = identity scale
            0.0,
            1.0,
            Color::RED,
            [tx, ty],
        );
        // diag(1*r,1*r): x-col=(r,0), y-col=(0,r)
        assert_eq!(identity.transform, [r, 0.0, 0.0, r], "identity diag × r");
        // Center carried in transform_translate (NOT scaled by M).
        assert_eq!(identity.transform_translate[0], tx, "tx = center x");
        assert_eq!(identity.transform_translate[1], ty, "ty = center y");
        assert_eq!(identity.transform_translate[2], 0.0, "pad z");
        assert_eq!(identity.transform_translate[3], 0.0, "pad w");
        // center_radius.xy unused; .z is the unit-circle radius (1).
        assert_eq!(identity.center_radius[0], 0.0, "cx unused");
        assert_eq!(identity.center_radius[1], 0.0, "cy unused");
        assert_eq!(identity.center_radius[2], 1.0, "unit radius");

        let scaled = ArcInstance::with_affine_transform(
            [sx * r, 0.0, 0.0, sy * r], // diag(sx,sy)*r
            0.0,
            1.0,
            Color::RED,
            [tx, ty],
        );
        // diag(sx*r, sy*r): x-col=(sx*r,0), y-col=(0,sy*r)
        assert_eq!(scaled.transform, [sx * r, 0.0, 0.0, sy * r], "scaled diag");
        // Center is still the raw device center — diag(sx*r,sy*r) must NOT scale it.
        assert_eq!(scaled.transform_translate[0], tx, "tx unscaled by sx");
        assert_eq!(scaled.transform_translate[1], ty, "ty unscaled by sy");
    }

    /// `ArcInstance::with_affine_transform` must store a unit circle at origin
    /// and round-trip the caller's linear + translate without mangling.
    #[test]
    fn arc_with_affine_transform_stores_unit_circle_and_affine() {
        let cos30 = std::f32::consts::FRAC_PI_6.cos();
        let sin30 = std::f32::consts::FRAC_PI_6.sin();
        let r = 20.0_f32;
        // CW screen rotation × radius r: x-col=(cos,-sin)*r, y-col=(sin,cos)*r.
        let linear = [cos30 * r, -sin30 * r, sin30 * r, cos30 * r];
        let translation = [64.0_f32, 64.0_f32];

        let instance = ArcInstance::with_affine_transform(
            linear,
            0.5,  // start_angle
            1.57, // sweep_angle
            Color::BLUE,
            translation,
        );

        // center_radius must be the unit circle at origin.
        assert_eq!(instance.center_radius[0], 0.0, "cx=0");
        assert_eq!(instance.center_radius[1], 0.0, "cy=0");
        assert_eq!(instance.center_radius[2], 1.0, "radius=1 (unit circle)");
        assert_eq!(instance.center_radius[3], 0.0, "padding");
        // Angles round-trip.
        assert_eq!(instance.angles[0], 0.5, "start_angle");
        assert_eq!(instance.angles[1], 1.57, "sweep_angle");
        // Linear stored verbatim.
        assert_eq!(instance.transform, linear, "linear 2×2 round-trip");
        // Translation stored in xy; zw are padding.
        assert_eq!(instance.transform_translate[0], 64.0, "tx");
        assert_eq!(instance.transform_translate[1], 64.0, "ty");
        assert_eq!(instance.transform_translate[2], 0.0, "pad.z");
        assert_eq!(instance.transform_translate[3], 0.0, "pad.w");
    }
}
