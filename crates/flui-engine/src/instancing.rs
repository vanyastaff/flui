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
use flui_foundation::geometry::{Point, Rect};
use flui_painting::styling::Color;

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
pub(crate) struct RectInstance {
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
    /// instance attributes. Three lanes are live:
    ///
    /// - `.x` — the clip's kind (`0` none, `1` rrect, `2` rounded superellipse)
    /// - `.y` — the PAINT's aliased flag (`1` = hard edge, `0` = anti-aliased)
    /// - `.z` — the CLIP's `Clip::HardEdge` mode (`1` = threshold, `0` = feather)
    ///
    /// `.w` is padding. The vertex stage packs `.x` and `.z` into one varying
    /// (`CLIP_HARD_BIT` in `shaders/common/clip.wgsl`).
    ///
    /// Two owners write this attribute: the clip owns lanes 0 and 2, the paint
    /// owns lane 1, and `with_clip` assigns lane by lane for that reason. The
    /// paint's flag and the clip's are independent — a hard-edged shape inside
    /// a smooth clip keeps the smooth clip.
    ///
    /// The polarity is deliberate. Every construction site zeroes this
    /// attribute, so `0` has to mean the behaviour they all had before —
    /// anti-aliased. A flag meaning "anti-alias me" would have silently
    /// turned AA off for every existing instance. Reusing a lane also keeps
    /// the vertex stride and the WGSL attribute list unchanged; a new `vec4`
    /// for one boolean costs 16 bytes per instance.
    pub clip_kind: [u32; 4],
    /// Device-to-clip-local linear part: `[a, b, c, d]`, columns first.
    ///
    /// The clip's bounds and radii are in the space the caller set them in;
    /// this maps a device-space fragment position back there. Identity is
    /// `[1, 0, 0, 1]`.
    pub clip_device_to_local: [f32; 4],
    /// Device-to-clip-local translation, padded to a `vec4` attribute:
    /// `[tx, ty, 0, 0]`.
    pub clip_local_origin: [f32; 4],

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
    pub(crate) fn rect(rect: Rect<f64>, color: Color) -> Self {
        Self {
            bounds: [
                (rect.left() as f32),
                (rect.top() as f32),
                (rect.width() as f32),
                (rect.height() as f32),
            ],
            color: color.to_f32_array(),
            corner_radii: [0.0; 4],
            // Identity 2×2: x-col=(1,0), y-col=(0,1).
            transform: [1.0, 0.0, 0.0, 1.0],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
            transform_translate: [0.0; 4],
        }
    }

    /// Marks this instance's own edge as hard, skipping the SDF's
    /// anti-aliasing (`Paint::anti_alias == false`).
    ///
    /// Affects the SHAPE's edge only; an SDF clip applied to it keeps its own
    /// smoothing, which is the clip's contract rather than the paint's.
    #[must_use]
    pub(crate) const fn aliased(mut self) -> Self {
        self.clip_kind[1] = 1;
        self
    }

    // `RectInstance::rounded_rect(rect, color, single_radius)` (the
    // uniform-corner shortcut) was deleted. Zero callsites -- production
    // paths use `rounded_rect_corners` (per-corner).

    /// Create an instance with per-corner radii (baked-AABB fast path).
    ///
    /// `rect` must already be in device pixels. The affine fields are identity /
    /// zero — byte-identical to the pre-affine baked-AABB path.
    #[must_use]
    pub(crate) fn rounded_rect_corners(
        rect: Rect<f64>,
        color: Color,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
    ) -> Self {
        Self {
            bounds: [
                (rect.left() as f32),
                (rect.top() as f32),
                (rect.width() as f32),
                (rect.height() as f32),
            ],
            color: color.to_f32_array(),
            corner_radii: [top_left, top_right, bottom_right, bottom_left],
            // Identity 2×2: x-col=(1,0), y-col=(0,1).
            transform: [1.0, 0.0, 0.0, 1.0],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
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
    pub(crate) fn with_affine_transform(
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
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
            transform_translate: [translation[0], translation[1], 0.0, 0.0],
        }
    }

    // `RectInstance::with_transform(scale_x, scale_y, translate_x,
    // translate_y)` (a per-instance transform setter) was deleted;
    // zero callsites -- transform comes from the painter's matrix
    // stack, not from per-instance helpers.
    // `with_clip_rsuperellipse` was kept despite an earlier code-review
    // pass recommending its removal: it has 1 live callsite in the
    // `painter` module (`instance.with_clip_rsuperellipse(self.current_rsuperellipse_clip)`)
    // -- that earlier pass had reported zero callsites but missed the
    // method-style dispatch on `instance` (vs type-path `RectInstance::`).

    /// Get wgpu vertex buffer layout for instance data.
    ///
    /// Locations 2–8 are unchanged from the pre-affine layout. Location 9 is
    /// the new `transform_translate` field appended at the end of the struct;
    /// appending keeps all existing field offsets byte-identical.
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
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
            // Clip/source flags: [kind, source mode, hard, _pad] (location 8)
            8 => Uint32x4,
            // Device-to-clip-local linear part (location 9)
            9 => Float32x4,
            // Device-to-clip-local translation, padded (location 10)
            10 => Float32x4,
            // Affine translation [tx, ty, 0, 0] (location 11 — it moved off 9
            // when the clip mapping took 9 and 10)
            11 => Float32x4,
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
pub(crate) struct CircleInstance {
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
    /// SDF clip rounded rectangle, in device space:
    /// `[x, y, width, height, radius_tl, radius_tr, radius_br, radius_bl]`.
    ///
    /// All zeros means no clip. Identical slot and semantics to
    /// [`RectInstance::clip_rrect`] — circles go through the same
    /// [`ClippableInstance`] seam so the two cannot drift.
    pub clip_rrect: [f32; 8],

    /// Which SDF the fragment evaluates against `clip_rrect`:
    /// `[0, _, _, _]` none, `[1, _, _, _]` rounded rect, `[2, _, _, _]`
    /// rounded superellipse. Lane `.z` carries the clip's `Clip::HardEdge`
    /// mode; `.y` and `.w` are padding for this instance type.
    pub clip_kind: [u32; 4],
    /// Device-to-clip-local linear part: `[a, b, c, d]`, columns first.
    ///
    /// The clip's bounds and radii are in the space the caller set them in;
    /// this maps a device-space fragment position back there. Identity is
    /// `[1, 0, 0, 1]`.
    pub clip_device_to_local: [f32; 4],
    /// Device-to-clip-local translation, padded to a `vec4` attribute:
    /// `[tx, ty, 0, 0]`.
    pub clip_local_origin: [f32; 4],
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
    pub(crate) fn new(center: Point<f64>, radius: f32, color: Color, scale_xy: [f32; 2]) -> Self {
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
            transform_translate: [(center.x as f32), (center.y as f32), 0.0, 0.0],
            // No clip until `ClippableInstance::with_clip_*` attaches one.
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
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
    pub(crate) fn with_affine_transform(
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
            // No clip until `ClippableInstance::with_clip_*` attaches one.
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
        }
    }

    // `CircleInstance::ellipse(center, radius_x, radius_y, color)` was
    // deleted. Zero call sites — production paths use
    // `CircleInstance::new` with scale_xy. When per-axis radii independent of
    // the canvas scale are needed it relands with a concrete first consumer.

    /// Get wgpu vertex buffer layout for instance data.
    ///
    /// Locations 2–4 are unchanged from the pre-PR-2 layout. Location 5 is the
    /// new `transform_translate` field appended at the end of the struct;
    /// appending keeps all existing field offsets byte-identical.
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Center + radius [cx, cy, radius, _] (location 2)
            2 => Float32x4,
            // Color [r, g, b, a] (location 3)
            3 => Float32x4,
            // 2×2 linear affine [a, b, c, d] column-major (location 4)
            4 => Float32x4,
            // Affine translation [tx, ty, 0, 0] (location 5)
            5 => Float32x4,
            // SDF clip bounds [x, y, w, h] (location 6)
            6 => Float32x4,
            // SDF clip corner radii [tl, tr, br, bl] (location 7)
            7 => Float32x4,
            // Clip kind [kind, _, _, _] (location 8)
            8 => Uint32x4,
            // Device-to-clip-local linear part (location 9)
            9 => Float32x4,
            // Device-to-clip-local translation, padded (location 10)
            10 => Float32x4,
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
pub(crate) struct ArcInstance {
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
    pub(crate) fn with_affine_transform(
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

    // `ArcInstance::ellipse(...)` was deleted (zero call sites).
    // Re-lands with a concrete consumer when needed. All SrcOver arcs now use
    // `with_affine_transform`; an elliptical arc folds `M_world * diag(rx, ry)`
    // into `linear_cols` (mirroring `oval`).

    /// Get wgpu vertex buffer layout for instance data.
    ///
    /// Locations 2–5 are unchanged from the pre-PR-2b layout. Location 6 is the
    /// new `transform_translate` field appended at the end of the struct;
    /// appending keeps all existing field offsets byte-identical.
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
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
pub(crate) struct TextureInstance {
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

    /// SDF clip rounded rectangle, same layout and sentinel as
    /// [`RectInstance::clip_rrect`]: `[x, y, width, height, radius_tl,
    /// radius_tr, radius_br, radius_bl]`, all zeros meaning no clip.
    ///
    /// A circular clip is this slot with every radius at half the shorter
    /// side — and because the fragment shader evaluates it as a signed
    /// distance rather than tessellating it, that is an exact circle, not a
    /// Bézier approximation of one.
    pub clip_rrect: [f32; 8],

    /// Clip-kind flag selecting which SDF to evaluate against `clip_rrect`.
    /// `0` none, `1` rounded box, `2` rounded superellipse — identical
    /// encoding to [`RectInstance::clip_kind`], including the `[u32; 4]`
    /// padding for 16-byte vec4 alignment. Slot 1 is the texture source mode:
    /// 0 unchanged, 1 ignore sampled alpha, 2 premultiply straight texel taps
    /// before bilinear filtering. Slot 2 retains the clip hard-edge flag.
    pub clip_kind: [u32; 4],
    /// Device-to-clip-local linear part: `[a, b, c, d]`, columns first.
    ///
    /// The clip's bounds and radii are in the space the caller set them in;
    /// this maps a device-space fragment position back there. Identity is
    /// `[1, 0, 0, 1]`.
    pub clip_device_to_local: [f32; 4],
    /// Device-to-clip-local translation, padded to a `vec4` attribute:
    /// `[tx, ty, 0, 0]`.
    pub clip_local_origin: [f32; 4],
}

impl TextureInstance {
    /// Declare an opaque source whose sampled alpha channel is ignored.
    /// Apply after recording the clip, which populates the same packed slot.
    pub(crate) fn with_opaque_source(mut self) -> Self {
        self.clip_kind[1] = 1;
        self
    }

    /// Bilinear straight sources interpolate premultiplied coverage, so the
    /// shader output and opacity tint must both use the premultiplied path.
    /// Apply after recording the clip, which populates this packed slot.
    pub(crate) fn with_linear_straight_source(mut self) -> Self {
        self.clip_kind[1] = 2;
        self.tint = [self.tint[3]; 4];
        self
    }

    pub(crate) fn filters_straight_as_premultiplied(&self) -> bool {
        self.clip_kind[1] == 2
    }

    /// Create a simple textured quad instance
    ///
    /// # Arguments
    /// * `dst_rect` - Destination rectangle in screen coordinates
    /// * `tint` - Color tint (use Color::WHITE for no tint)
    #[must_use]
    pub(crate) fn new(dst_rect: flui_foundation::geometry::Rect<f64>, tint: Color) -> Self {
        Self {
            dst_rect: [
                (dst_rect.left() as f32),
                (dst_rect.top() as f32),
                (dst_rect.width() as f32),
                (dst_rect.height() as f32),
            ],
            src_uv: [0.0, 0.0, 1.0, 1.0], // Full texture
            tint: tint.to_f32_array(),
            transform: [1.0, 0.0, 0.0, 0.0], // No rotation
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
        }
    }

    /// Create a textured quad with custom UV coordinates (for texture atlas)
    ///
    /// # Arguments
    /// * `dst_rect` - Destination rectangle in screen coordinates
    /// * `src_uv` - Source UV rectangle [u_min, v_min, u_max, v_max]
    /// * `tint` - Color tint
    #[must_use]
    pub(crate) fn with_uv(
        dst_rect: flui_foundation::geometry::Rect<f64>,
        src_uv: [f32; 4],
        tint: Color,
    ) -> Self {
        Self {
            dst_rect: [
                (dst_rect.left() as f32),
                (dst_rect.top() as f32),
                (dst_rect.width() as f32),
                (dst_rect.height() as f32),
            ],
            src_uv,
            tint: tint.to_f32_array(),
            transform: [1.0, 0.0, 0.0, 0.0],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
        }
    }

    // `TextureInstance::with_rotation(dst_rect, angle, tint)` was
    // deleted. Zero callsites -- production paths use
    // `TextureInstance::with_uv` (canonical, 5 callsites in
    // painter) and the painter's matrix stack handles rotation
    // composition. `TextureInstance::with_uv` was kept despite an
    // earlier code-review pass recommending its removal, because it
    // IS live (that pass claimed otherwise; grep proved 5 painter
    // callsites).

    /// Create a textured quad with custom UV and a raw `[f32; 4]` tint.
    ///
    /// Used by the offscreen-layer composite path, which needs a fractional
    /// premultiplied tint `(C.r*O, C.g*O, C.b*O, O)` that an 8-bit [`Color`]
    /// would quantize prematurely. The shader multiplies the sampled texel by
    /// this tint (`tex_color * in.tint`).
    #[must_use]
    pub(crate) fn with_uv_tint_f32(
        dst_rect: flui_foundation::geometry::Rect<f64>,
        src_uv: [f32; 4],
        tint: [f32; 4],
    ) -> Self {
        Self {
            dst_rect: [
                (dst_rect.left() as f32),
                (dst_rect.top() as f32),
                (dst_rect.width() as f32),
                (dst_rect.height() as f32),
            ],
            src_uv,
            tint,
            transform: [1.0, 0.0, 0.0, 0.0],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
        }
    }

    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Destination rect (location 2)
            2 => Float32x4,
            // Source UV (location 3)
            3 => Float32x4,
            // Tint color (location 4)
            4 => Float32x4,
            // Transform (location 5)
            5 => Float32x4,
            // Clip rrect part 1: [x, y, width, height] (location 6)
            6 => Float32x4,
            // Clip rrect part 2: [radius_tl, radius_tr, radius_br, radius_bl] (location 7)
            7 => Float32x4,
            // Clip/source flags: [kind, source mode, hard, _pad] (location 8)
            8 => Uint32x4,
            // Device-to-clip-local linear part (location 9)
            9 => Float32x4,
            // Device-to-clip-local translation, padded (location 10)
            10 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TextureInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// One rasterised glyph quad, sampled from the glyph atlas.
///
/// The atlas texel rectangle is carried in texels rather than normalised
/// UVs: the atlas may grow between this instance's recording and its draw,
/// and a texel position survives a grow (allocations keep their place)
/// where a normalised one would not. The shader divides by the atlas
/// dimensions it is bound with.
///
/// The quad is placed as `origin + transform × (rect.xy + corner × rect.zw)`:
/// under a uniform CTM `transform` is the identity, `origin` is zero and
/// `rect` is the device rectangle; under a rotated or anisotropic CTM `rect`
/// is in the paragraph's raster space and `transform` is the CTM's linear
/// part divided by the raster scale.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct GlyphInstance {
    /// Quad rectangle `[x, y, width, height]` in the space `transform` maps
    /// from.
    pub dst_rect: [f32; 4],
    /// Atlas texel origin `[u, v]` and the page kind in lane 2 (`0` mask,
    /// `1` colour); lane 3 unused.
    pub atlas: [f32; 4],
    /// Straight-alpha colour `[r, g, b, a]`, opacity already folded into
    /// `a`. A colour-page glyph uses only `a`.
    pub color: [f32; 4],
    /// 2×2 linear part `[a, b, c, d]`, column-major, applied to `dst_rect`
    /// points; identity for the uniform-CTM path.
    pub transform: [f32; 4],
    /// Device-pixel translation `[tx, ty, 0, 0]` added after `transform`.
    pub origin: [f32; 4],
    /// SDF clip, same layout and sentinel as [`RectInstance::clip_rrect`].
    pub clip_rrect: [f32; 8],
    /// Same encoding as [`TextureInstance::clip_kind`].
    pub clip_kind: [u32; 4],
    /// Same as [`TextureInstance::clip_device_to_local`].
    pub clip_device_to_local: [f32; 4],
    /// Same as [`TextureInstance::clip_local_origin`].
    pub clip_local_origin: [f32; 4],
}

impl GlyphInstance {
    /// A glyph quad covering `dst_rect` in device pixels, sampled from the
    /// page named by `color_page` at texel `(u, v)`.
    #[must_use]
    pub(crate) fn new(
        dst_rect: [f32; 4],
        atlas_texel: [u32; 2],
        color_page: bool,
        color: Color,
    ) -> Self {
        Self {
            dst_rect,
            atlas: [
                atlas_texel[0] as f32,
                atlas_texel[1] as f32,
                if color_page { 1.0 } else { 0.0 },
                0.0,
            ],
            color: color.to_f32_array(),
            transform: [1.0, 0.0, 0.0, 1.0],
            origin: [0.0; 4],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
        }
    }

    /// Places the quad through `linear` (column-major `[a, b, c, d]`) and
    /// then `origin`, for a paragraph recorded under a non-uniform CTM.
    #[must_use]
    pub(crate) fn with_affine(mut self, linear: [f32; 4], origin: [f32; 2]) -> Self {
        self.transform = linear;
        self.origin = [origin[0], origin[1], 0.0, 0.0];
        self
    }

    /// Vertex buffer layout for instance data.
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Quad rect (location 2)
            2 => Float32x4,
            // Atlas texel origin + page kind (location 3)
            3 => Float32x4,
            // Colour (location 4)
            4 => Float32x4,
            // Linear part [a, b, c, d] (location 5)
            5 => Float32x4,
            // Translation [tx, ty, 0, 0] (location 6)
            6 => Float32x4,
            // Clip rrect part 1: [x, y, width, height] (location 7)
            7 => Float32x4,
            // Clip rrect part 2: radii (location 8)
            8 => Float32x4,
            // Clip kind (location 9)
            9 => Uint32x4,
            // Device-to-clip-local linear part (location 10)
            10 => Float32x4,
            // Device-to-clip-local translation (location 11)
            11 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// An instance type that can carry the active SDF clip.
///
/// Exists so [`GpuStateStack::apply_active_clip`] can decide *once* which of
/// the two clip slots wins and hand the answer to any instance kind, instead
/// of each batch re-deriving that branch order and drifting from the others.
///
/// [`GpuStateStack::apply_active_clip`]: crate::state_stack::GpuStateStack::apply_active_clip
/// Reduce the 12-slot rounded-superellipse clip to the 8-slot form every
/// clip-capable instance stores.
///
/// The 12-slot form carries `(rx, ry)` per corner; an instance slot holds one
/// radius per corner, so each pair is averaged. This lived as six identical
/// copies — one per `ClippableInstance` impl — before it was a function.
///
/// Returns the all-zero "no clip" sentinel unchanged.
pub(crate) fn reduce_superellipse_clip(c: [f32; 12]) -> [f32; 8] {
    let is_empty = c == [0.0; 12];
    if is_empty {
        return [0.0; 8];
    }
    [
        c[0],
        c[1],
        c[2],
        c[3],
        f32::midpoint(c[4], c[5]),
        f32::midpoint(c[6], c[7]),
        f32::midpoint(c[8], c[9]),
        f32::midpoint(c[10], c[11]),
    ]
}

pub(crate) trait ClippableInstance {
    /// Store the active clip, or clear the slot when no clip is active.
    ///
    /// One method rather than a per-kind pair: the clip slot has one layout
    /// across every instance type, so a two-method form would make each
    /// implementor re-derive the kind flag and the superellipse reduction.
    ///
    /// Every implementor but [`RectInstance`] writes the slot verbatim;
    /// `impl_clippable_instance!` below holds that one body.
    #[must_use]
    fn with_clip(self, clip: crate::state_stack::ResolvedClip) -> Self;
}

/// Generate the verbatim `with_clip` body for instance types whose
/// `clip_kind` has the clip as its only writer.
///
/// [`RectInstance`] is not in this list: it shares lane 1 of `clip_kind` with
/// the paint's aliased flag and writes the slot by hand.
macro_rules! impl_clippable_instance {
    ($($instance:ty),* $(,)?) => {
        $(
            impl ClippableInstance for $instance {
                fn with_clip(mut self, clip: crate::state_stack::ResolvedClip) -> Self {
                    self.clip_rrect = clip.rrect;
                    self.clip_kind = clip.kind;
                    self.clip_device_to_local = [
                        clip.device_to_local[0],
                        clip.device_to_local[1],
                        clip.device_to_local[2],
                        clip.device_to_local[3],
                    ];
                    self.clip_local_origin =
                        [clip.device_to_local[4], clip.device_to_local[5], 0.0, 0.0];
                    self
                }
            }
        )*
    };
}

impl ClippableInstance for RectInstance {
    fn with_clip(mut self, clip: crate::state_stack::ResolvedClip) -> Self {
        self.clip_rrect = clip.rrect;
        // Three lanes, two owners. Lane 0 (clip kind) and lane 2 (the clip's
        // HARD-EDGE mode) belong to the clip; lane 1 is the PAINT's aliased
        // flag. Assigning `clip.kind` wholesale drops lane 1, and zeroing the
        // tail drops lane 2 — both have happened here. Each owner writes only
        // its own lanes, so the order the two are applied in cannot matter.
        self.clip_kind = [clip.kind[0], self.clip_kind[1], clip.kind[2], 0];
        self.clip_device_to_local = [
            clip.device_to_local[0],
            clip.device_to_local[1],
            clip.device_to_local[2],
            clip.device_to_local[3],
        ];
        self.clip_local_origin = [clip.device_to_local[4], clip.device_to_local[5], 0.0, 0.0];
        self
    }
}

impl_clippable_instance!(
    CircleInstance,
    LinearGradientInstance,
    RadialGradientInstance,
    SweepGradientInstance,
    TextureInstance,
    GlyphInstance,
);

// =============================================================================
// Gradient Instances (from effects.rs for API consistency)
// =============================================================================

/// Linear gradient instance data for GPU instancing
///
/// See `crate::painter::effects::LinearGradientInstance` for full
/// documentation.
pub(crate) use crate::effects::LinearGradientInstance;

impl LinearGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
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
            // Clip bounds [x, y, w, h] (location 8)
            8 => Float32x4,
            // Clip corner radii [tl, tr, br, bl] (location 9)
            9 => Float32x4,
            // Clip kind (location 10)
            10 => Uint32x4,
            // Device-to-clip-local linear part (location 11)
            11 => Float32x4,
            // Device-to-clip-local translation, padded (location 12)
            12 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LinearGradientInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Radial gradient instance data for GPU instancing
pub(crate) use crate::effects::RadialGradientInstance;

impl RadialGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
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
            // Clip bounds [x, y, w, h] (location 8)
            8 => Float32x4,
            // Clip corner radii [tl, tr, br, bl] (location 9)
            9 => Float32x4,
            // Clip kind (location 10)
            10 => Uint32x4,
            // Device-to-clip-local linear part (location 11)
            11 => Float32x4,
            // Device-to-clip-local translation, padded (location 12)
            12 => Float32x4,
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
pub(crate) use crate::effects::SweepGradientInstance;

impl SweepGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
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
            // Clip bounds [x, y, w, h] (location 8)
            8 => Float32x4,
            // Clip corner radii [tl, tr, br, bl] (location 9)
            9 => Float32x4,
            // Clip kind (location 10)
            10 => Uint32x4,
            // Device-to-clip-local linear part (location 11)
            11 => Float32x4,
            // Device-to-clip-local translation, padded (location 12)
            12 => Float32x4,
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
pub(crate) use crate::effects::ShadowInstance;

impl ShadowInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
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
/// `Clone` is derived so that a recorded [`crate::command_ir::DrawSegment`] can be
/// snapshotted before replay.
#[derive(Debug, Clone)]
pub(crate) struct InstanceBatch<T> {
    /// Instance data
    pub(crate) instances: crate::recording_budget::BudgetVec<T>,

    /// Maximum instances before auto-flush
    pub max_instances: usize,
}

impl<T> InstanceBatch<T> {
    /// Create a new instance batch
    #[must_use]
    pub(crate) fn new(max_instances: usize) -> Self {
        Self {
            instances: crate::recording_budget::BudgetVec::unbounded_capacity(max_instances),
            max_instances,
        }
    }

    /// Add an instance to the batch
    ///
    /// Returns true if batch is full and should be flushed.
    #[must_use]
    pub(crate) fn add(&mut self, instance: T) -> bool {
        self.instances.push(instance);
        self.instances.len() >= self.max_instances
    }

    /// Check if batch is empty
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Get number of instances
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.instances.len()
    }

    /// Clear the batch
    pub(crate) fn clear(&mut self) {
        self.instances.clear();
    }

    /// Get instance data as byte slice
    pub(crate) fn as_bytes(&self) -> &[u8]
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
