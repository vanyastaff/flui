//! GPU draw-state stack: transform, scissor, and SDF clip.
//!
//! `GpuStateStack` owns the four parallel save/restore stacks that
//! must stay in sync across every `save()`/`restore()` call:
//!
//! | Stack field                  | Cached current value            |
//! |------------------------------|---------------------------------|
//! | `transform_stack`            | `current_transform`             |
//! | `scissor_stack`              | `current_scissor`               |
//! | `rrect_clip_stack`           | `current_rrect_clip`            |
//! | `rsuperellipse_clip_stack`   | `current_rsuperellipse_clip`    |
//!
//! # Scissor asymmetry
//!
//! The scissor stack uses a **conditional-push / unconditional-pop** strategy
//! (ported verbatim from Impeller `canvas.cc Save()`/`Restore()`):
//!
//! - `save()` pushes `current_scissor` **only when it is `Some`**.
//! - `restore()` pops from `scissor_stack` **only when the stack is
//!   non-empty**; when it is empty, `current_scissor` is set to `None`.
//!
//! This maintains the invariant
//! `scissor_stack.len() ≤ transform_stack.len()` at all times and is
//! semantically equivalent to always saving the scissor without the
//! per-save allocation cost when no scissor is active.
//!
//! # Balance assertion
//!
//! The frame boundary (`WgpuPainter::reset_frame_state`) calls
//! `self.state.debug_assert_balanced()` **before** calling `self.state.reset()`.
//! The assertion logic lives in `GpuStateStack::debug_assert_balanced` so it
//! can be exercised in unit tests without a GPU.
//! No `Drop` impl is provided: the Backend implicit-single-save (a lazy
//! `active_transform` save, balanced by `Backend`'s own `Drop`) must not
//! false-positive-panic, and a `Drop` panic during unwind would trigger an abort.

use flui_types::{
    Offset, Point, Rect,
    geometry::{Pixels, RRect, px},
};

/// The four parallel GPU draw-state stacks, plus their cached top-of-stack
/// values.
///
/// `WgpuPainter` holds one `GpuStateStack` and delegates all
/// transform/scissor/clip mutation through it, keeping every draw method free
/// to read the current values without a whole-struct borrow conflict (all
/// accessors return by **copy**, never by reference).
#[derive(Debug)]
pub(super) struct GpuStateStack {
    // ===== Transform Stack =====
    /// Saved transforms, one entry per `save()` call.
    transform_stack: Vec<glam::Mat4>,

    /// Current accumulated transform (CTM). Identity at frame start.
    current_transform: glam::Mat4,

    // ===== Scissor Stack =====
    /// Saved scissor rects. Only pushed when `current_scissor` is `Some` at
    /// `save()` time — see module-level doc on the asymmetry.
    scissor_stack: Vec<(u32, u32, u32, u32)>,

    /// Current active scissor rectangle in physical pixels `(x, y, w, h)`.
    /// `None` means no axis-aligned scissor clip is active.
    current_scissor: Option<(u32, u32, u32, u32)>,

    // ===== SDF RRect Clip Stack =====
    /// Saved SDF rounded-rectangle clip uniforms.
    /// Each entry is `[x, y, w, h, r_tl, r_tr, r_br, r_bl]`.
    rrect_clip_stack: Vec<[f32; 8]>,

    /// Active SDF rounded-rectangle clip uniform. All-zeros means no clip.
    current_rrect_clip: [f32; 8],
    /// Whether the active SDF clip has a hard edge (`Clip::HardEdge`) rather
    /// than a feathered one (`Clip::AntiAlias`).
    ///
    /// Saved and restored with the clip itself, so a hard clip nested inside a
    /// smooth one does not leak its mode outward on `restore`.
    current_clip_hard: bool,
    clip_hard_stack: Vec<bool>,

    // ===== SDF RSuperellipse Clip Stack =====
    /// Saved SDF superellipse clip uniforms.
    /// Each entry is `[x, y, w, h, tl_rx, tl_ry, tr_rx, tr_ry,
    ///                              br_rx, br_ry, bl_rx, bl_ry]`.
    rsuperellipse_clip_stack: Vec<[f32; 12]>,

    /// Active SDF superellipse clip uniform. All-zeros means no clip.
    current_rsuperellipse_clip: [f32; 12],

    // ===== Clip-local mapping =====
    /// Saved device-to-clip-local mappings, one per `save()`.
    clip_inv_stack: Vec<[f32; 6]>,

    /// Maps a device-space point into the space the active clip's bounds and
    /// radii are expressed in: `[a, b, c, d, tx, ty]`, columns first, so
    /// `local = (a, b) * p.x + (c, d) * p.y + (tx, ty)`.
    ///
    /// The clip slots hold LOCAL bounds — the shape the caller asked for,
    /// untransformed — and this is the inverse of the CTM that was current
    /// when the clip was set. Storing device-space bounds instead cannot
    /// express a rotation (an axis-aligned box has no rotated form) and
    /// mangles a non-uniform scale (a scaled circular corner is an ellipse,
    /// and one radius per corner cannot hold one).
    ///
    /// The drawn rrect already worked this way — `with_affine_transform` sends
    /// local bounds plus the affine and lets the shader place them. This is
    /// the same information, inverted, because the fragment stage starts from
    /// a device-space position and has to get back.
    ///
    /// Identity is `[1, 0, 0, 1, 0, 0]`.
    current_clip_inv: [f32; 6],
}

/// Identity device-to-clip-local mapping: columns `(1, 0)`, `(0, 1)`, no
/// translation.
const CLIP_INV_IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// The active SDF clip, in the form every consumer stores it.
///
/// One value rather than three loose arrays because the three travel together
/// everywhere — instance slots, the tessellated batch uniform, and the
/// offscreen remaps — and a caller that carried two of them would produce a
/// clip evaluated in the wrong space with nothing to catch it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedClip {
    /// `[x, y, w, h, tl, tr, br, bl]` in CLIP-LOCAL space.
    pub(crate) rrect: [f32; 8],
    /// `[kind, _, hard, _]`: kind is 0 = none, 1 = rrect, 2 = rounded
    /// superellipse; `hard` is 1 for `Clip::HardEdge` and 0 for
    /// `Clip::AntiAlias`. The vertex stages pack the two into one varying —
    /// see `CLIP_HARD_BIT` in `shaders/common/clip.wgsl`.
    pub(crate) kind: [u32; 4],
    /// Device-to-clip-local mapping — see `GpuStateStack::current_clip_inv`.
    pub(crate) device_to_local: [f32; 6],
}

impl ResolvedClip {
    /// No clip active.
    pub(crate) const NONE: Self = Self {
        rrect: [0.0; 8],
        kind: [0; 4],
        device_to_local: CLIP_INV_IDENTITY,
    };
}

/// Whether a scissor's fractional bounds round to the pixels it covers exactly
/// or to a superset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScissorRounding {
    /// The scissor is the clip. Truncation is the `Clip::HardEdge` semantic.
    Exact,
    /// The scissor is an early reject in front of an SDF. It must cover every
    /// pixel the SDF might shade, including the anti-aliased fringe.
    Conservative,
}

impl ScissorRounding {
    /// The rounding an SDF clip's coarse scissor wants for a given mode.
    ///
    /// A feathered clip needs the pad so the fringe is not cut; a hard one has
    /// no fringe, and padding it would only loosen the bound that non-SDF
    /// consumers — text, above all — rely on as their whole clip.
    const fn for_clip(hard: bool) -> Self {
        if hard {
            Self::Exact
        } else {
            Self::Conservative
        }
    }
}

impl GpuStateStack {
    /// Construct a pristine stack — identity transform, no scissor, no SDF
    /// clips, all stacks empty. Equivalent to the post-`reset()` state.
    pub(super) fn new() -> Self {
        Self {
            transform_stack: Vec::new(),
            current_transform: glam::Mat4::IDENTITY,
            scissor_stack: Vec::new(),
            current_scissor: None,
            rrect_clip_stack: Vec::new(),
            current_rrect_clip: [0.0; 8],
            current_clip_hard: false,
            clip_hard_stack: Vec::new(),
            rsuperellipse_clip_stack: Vec::new(),
            current_rsuperellipse_clip: [0.0; 12],
            clip_inv_stack: Vec::new(),
            current_clip_inv: CLIP_INV_IDENTITY,
        }
    }

    /// Construct a pristine `GpuStateStack` for unit tests that need to call
    /// functions accepting `&GpuStateStack` but do not require a GPU device.
    ///
    /// Produces the same state as `new()` (identity transform, no scissor) and
    /// is gated to `#[cfg(test)]` so it is never reachable from production code.
    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self::new()
    }

    // =========================================================================
    // Frame boundary
    // =========================================================================

    /// Reset all stacks and cached values to the pristine frame-start state.
    ///
    /// The caller (`WgpuPainter::reset_frame_state`) is responsible for
    /// asserting `depth() == 0` **before** calling this method.
    pub(super) fn reset(&mut self) {
        self.current_scissor = None;
        self.scissor_stack.clear();
        self.current_rrect_clip = [0.0; 8];
        self.current_clip_hard = false;
        self.clip_hard_stack.clear();
        self.rrect_clip_stack.clear();
        self.current_rsuperellipse_clip = [0.0; 12];
        self.rsuperellipse_clip_stack.clear();
        self.current_clip_inv = CLIP_INV_IDENTITY;
        self.clip_inv_stack.clear();
        // Identity is the construction-time value. Reset to the same initial
        // value so no cross-frame CTM leak can occur.
        self.current_transform = glam::Mat4::IDENTITY;
        self.transform_stack.clear();
    }

    // =========================================================================
    // Depth query
    // =========================================================================

    /// Current save-stack depth — equals `transform_stack.len()`, the single
    /// source of truth. No parallel counter is maintained.
    #[inline]
    pub(super) fn depth(&self) -> usize {
        self.transform_stack.len()
    }

    /// Assert that the save/restore stack is balanced (depth == 0).
    ///
    /// Called by `WgpuPainter::reset_frame_state` at the frame boundary,
    /// **before** `reset()`, to catch mismatched save/restore pairs.
    ///
    /// The logic lives here (rather than inline in `reset_frame_state`) so
    /// that unit tests can exercise it without a GPU: construct a
    /// `GpuStateStack`, drive it into an unbalanced state, call this method,
    /// and observe the panic via `#[should_panic]`.
    ///
    /// Compiled out in release builds (`debug_assert!`).
    pub(super) fn debug_assert_balanced(&self) {
        debug_assert!(
            self.transform_stack.is_empty(),
            "unbalanced save/restore at frame boundary: depth={}",
            self.depth()
        );
    }

    // =========================================================================
    // Save / restore
    // =========================================================================

    /// Push current transform, scissor (conditionally), and both SDF clip
    /// uniforms onto their respective stacks.
    ///
    /// Scissor is pushed only when `current_scissor` is `Some`; see the
    /// module-level doc on the asymmetry.
    pub(super) fn save(&mut self) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::save: depth={}", self.transform_stack.len());

        self.transform_stack.push(self.current_transform);

        // Conditional-push: preserve the `scissor_stack.len() ≤
        // transform_stack.len()` invariant without allocating for the
        // common no-scissor case.
        if let Some(scissor) = self.current_scissor {
            self.scissor_stack.push(scissor);
        }

        self.rrect_clip_stack.push(self.current_rrect_clip);
        self.clip_hard_stack.push(self.current_clip_hard);
        self.rsuperellipse_clip_stack
            .push(self.current_rsuperellipse_clip);
        self.clip_inv_stack.push(self.current_clip_inv);
    }

    /// Pop transform, scissor (conditionally), and both SDF clip uniforms.
    ///
    /// Logs a warning on underflow (no matching `save()`) and returns early
    /// without mutating state.
    pub(super) fn restore(&mut self) {
        let Some(saved_transform) = self.transform_stack.pop() else {
            #[cfg(debug_assertions)]
            tracing::warn!("GpuStateStack::restore: stack underflow");
            return;
        };

        self.current_transform = saved_transform;

        // Asymmetric pop: if the stack is empty the save() for this level
        // contributed nothing, meaning no scissor was active — restore None.
        if self.scissor_stack.is_empty() {
            self.current_scissor = None;
        } else {
            self.current_scissor = self.scissor_stack.pop();
        }

        // rrect/rsuperellipse stacks are pushed unconditionally in save(), so
        // this pop always succeeds when transform_stack did.
        self.current_rrect_clip = self
            .rrect_clip_stack
            .pop()
            .expect("rrect_clip_stack parallel to transform_stack");
        self.current_rsuperellipse_clip = self
            .rsuperellipse_clip_stack
            .pop()
            .expect("rsuperellipse_clip_stack parallel to transform_stack");
        self.current_clip_hard = self
            .clip_hard_stack
            .pop()
            .expect("BUG: clip_hard_stack is pushed unconditionally in save(), so it stays parallel to transform_stack");
        self.current_clip_inv = self
            .clip_inv_stack
            .pop()
            .expect("BUG: clip_inv_stack is pushed in save() alongside transform_stack");

        #[cfg(debug_assertions)]
        tracing::trace!(
            "GpuStateStack::restore: depth={}",
            self.transform_stack.len()
        );
    }

    // =========================================================================
    // Transform mutators
    // =========================================================================

    /// Post-multiply the CTM by a translation.
    pub(super) fn translate(&mut self, offset: Offset<Pixels>) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::translate: offset={:?}", offset);

        let translation = glam::Mat4::from_translation(glam::vec3(offset.dx.0, offset.dy.0, 0.0));
        self.current_transform *= translation;
    }

    /// Post-multiply the CTM by a Z-axis rotation.
    pub(super) fn rotate(&mut self, angle_radians: f32) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::rotate: angle={}", angle_radians);

        let rotation = glam::Mat4::from_rotation_z(angle_radians);
        self.current_transform *= rotation;
    }

    /// Post-multiply the CTM by a uniform scale.
    pub(super) fn scale(&mut self, sx: f32, sy: f32) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::scale: sx={}, sy={}", sx, sy);

        let scaling = glam::Mat4::from_scale(glam::vec3(sx, sy, 1.0));
        self.current_transform *= scaling;
    }

    // =========================================================================
    // Transform reads (Copy — no borrow conflict with mutable draw state)
    // =========================================================================

    /// The current accumulated transform as a `glam::Mat4`.
    ///
    /// Returns by **copy** so callers can read the transform then mutate other
    /// painter fields (e.g. `current_segment`) without a borrow conflict.
    #[inline]
    pub(super) fn current_transform(&self) -> glam::Mat4 {
        self.current_transform
    }

    /// The accumulated CTM as a [`flui_types::Matrix4`] (column-major).
    ///
    /// Both `glam::Mat4` and `flui_types::Matrix4` are column-major `[f32; 16]`,
    /// so the conversion is a direct reinterpret of the 16 floats.
    ///
    /// Ported verbatim from `WgpuPainter::current_transform_matrix` — the
    /// float column ordering is **not** changed; round-4/5 transform-bake and
    /// HiDPI device-sizing correctness depend on the exact layout.
    pub(super) fn current_transform_matrix(&self) -> flui_types::Matrix4 {
        let c = self.current_transform.to_cols_array();
        flui_types::Matrix4::new(
            c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9], c[10], c[11], c[12], c[13],
            c[14], c[15],
        )
    }

    /// Apply the CTM to a local-space point and return the screen-space result.
    pub(super) fn apply_transform(&self, point: Point<Pixels>) -> Point<Pixels> {
        let p = self.current_transform * glam::vec4(point.x.0, point.y.0, 0.0, 1.0);
        Point::new(px(p.x), px(p.y))
    }

    /// `true` when the current transform has no rotation or skew component.
    ///
    /// When `false`, rects must be tessellated rather than instanced.
    pub(super) fn is_axis_aligned(&self) -> bool {
        let m = self.current_transform;
        m.x_axis.y.abs() < 1e-6 && m.y_axis.x.abs() < 1e-6
    }

    /// Maximum basis-vector length of the CTM's 2D linear part.
    ///
    /// Mirrors Impeller `Matrix::GetMaxBasisLengthXY`: the larger of the two
    /// column-vector lengths of the upper-left 2×2. The tessellator uses this
    /// to budget curve-flattening tolerance at the correct magnification.
    ///
    /// **Do not use this for device-area thresholds.** Under anisotropic scale
    /// (e.g. `scale(0.5, 10)`) `max_scale()` returns 10, squaring to 100,
    /// while the true area scale is 5. Use [`Self::area_scale`] for area thresholds.
    pub(super) fn max_scale(&self) -> f32 {
        let m = self.current_transform;
        let col_x = (m.x_axis.x * m.x_axis.x + m.x_axis.y * m.x_axis.y).sqrt();
        let col_y = (m.y_axis.x * m.y_axis.x + m.y_axis.y * m.y_axis.y).sqrt();
        col_x.max(col_y)
    }

    /// Absolute determinant of the CTM's 2D linear part — the device-pixel² area
    /// scale factor (`area_device = area_local × area_scale`).
    ///
    /// Mirrors Impeller `Matrix::GetDeterminant` (upper-left 2×2 only):
    ///   `|det(M)| = |m.x_axis.x * m.y_axis.y − m.x_axis.y * m.y_axis.x|`
    ///
    /// This is rotation- and shear-invariant: a pure rotation has `|det|=1`;
    /// `diag(sx, sy)` gives `|sx*sy|`. For device-area thresholds this is
    /// more accurate than `max_scale²`, which overestimates under anisotropic
    /// scale (e.g. `scale(0.5, 10)` → `area_scale=5`, `max_scale²=100`).
    pub(super) fn area_scale(&self) -> f32 {
        let m = self.current_transform;
        (m.x_axis.x * m.y_axis.y - m.x_axis.y * m.y_axis.x).abs()
    }

    // =========================================================================
    // Scissor / SDF clip reads (Copy)
    // =========================================================================

    /// Current scissor rectangle in physical pixels `(x, y, w, h)`.
    ///
    /// Returns by **copy**.
    #[inline]
    pub(super) fn current_scissor(&self) -> Option<(u32, u32, u32, u32)> {
        self.current_scissor
    }

    // =========================================================================
    // Clip mutators
    // =========================================================================

    /// Set an axis-aligned scissor clip, intersecting with any existing one.
    ///
    /// `surface_size` is `(width_px, height_px)` of the render surface and is
    /// used to clamp the scissor to the surface bounds. It is passed as a
    /// parameter rather than stored on the stack so the painter remains the
    /// single owner of the surface dimensions.
    pub(super) fn clip_rect(&mut self, rect: Rect<Pixels>, surface_size: (u32, u32)) {
        self.clip_rect_with_rounding(rect, surface_size, ScissorRounding::Exact);
    }

    /// Intersect the current scissor with `rect`, rounding per `rounding`.
    pub(super) fn clip_rect_with_rounding(
        &mut self,
        rect: Rect<Pixels>,
        surface_size: (u32, u32),
        rounding: ScissorRounding,
    ) {
        let transform = self.current_transform;

        // Compute axis-aligned bounding box in screen space.
        let (x, y, width, height) = if transform == glam::Mat4::IDENTITY {
            // Fast path: no transform.
            let (left, top, right_f, bottom_f) = match rounding {
                // The scissor IS the clip: truncation is the hard edge.
                ScissorRounding::Exact => (
                    rect.left().0.max(0.0),
                    rect.top().0.max(0.0),
                    rect.right().0.min(surface_size.0 as f32),
                    rect.bottom().0.min(surface_size.1 as f32),
                ),
                // The scissor is only an early reject in front of an SDF that
                // does the precise work, so it must never cut a pixel the SDF
                // would have shaded. Expand outward by a pixel: fractional
                // bounds otherwise truncate (a right edge at 10.75 ends the
                // scissor at column 10, discarding the pixel centred at 10.5
                // that is inside the clip), and the feather itself reaches
                // about half a pixel past the boundary.
                ScissorRounding::Conservative => (
                    (rect.left().0 - 1.0).floor().max(0.0),
                    (rect.top().0 - 1.0).floor().max(0.0),
                    (rect.right().0 + 1.0).ceil().min(surface_size.0 as f32),
                    (rect.bottom().0 + 1.0).ceil().min(surface_size.1 as f32),
                ),
            };
            let x = left as u32;
            let y = top as u32;
            let right = right_f as u32;
            let bottom = bottom_f as u32;
            (x, y, right.saturating_sub(x), bottom.saturating_sub(y))
        } else {
            // Transform all four corners and compute a conservative AABB.
            let corners = [
                transform.transform_point3(glam::Vec3::new(rect.left().0, rect.top().0, 0.0)),
                transform.transform_point3(glam::Vec3::new(rect.right().0, rect.top().0, 0.0)),
                transform.transform_point3(glam::Vec3::new(rect.right().0, rect.bottom().0, 0.0)),
                transform.transform_point3(glam::Vec3::new(rect.left().0, rect.bottom().0, 0.0)),
            ];
            let min_x = corners.iter().map(|c| c.x).fold(f32::INFINITY, f32::min);
            let min_y = corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
            let max_x = corners
                .iter()
                .map(|c| c.x)
                .fold(f32::NEG_INFINITY, f32::max);
            let max_y = corners
                .iter()
                .map(|c| c.y)
                .fold(f32::NEG_INFINITY, f32::max);

            // The conservative pad applies here too. The AABB is already
            // conservative about ROTATION, which is a different question from
            // whether its fractional bounds round outward: a clip translated
            // by 0.75 still truncates to `x = 0, w = 10` and rejects the
            // column whose centre at 10.5 is inside the SDF's edge at 10.75.
            let pad = match rounding {
                ScissorRounding::Exact => 0.0,
                ScissorRounding::Conservative => 1.0,
            };
            let left = (min_x - pad).max(0.0);
            let top = (min_y - pad).max(0.0);
            let right = (max_x + pad).min(surface_size.0 as f32);
            let bottom = (max_y + pad).min(surface_size.1 as f32);
            let x = left.floor() as u32;
            let y = top.floor() as u32;
            let w = (right.ceil() - left.floor()).max(0.0) as u32;
            let h = (bottom.ceil() - top.floor()).max(0.0) as u32;
            (x, y, w, h)
        };

        // Intersect with the existing scissor when one is active.
        let new_scissor = if let Some((cur_x, cur_y, cur_w, cur_h)) = self.current_scissor {
            let inter_x = x.max(cur_x);
            let inter_y = y.max(cur_y);
            let inter_w = (x + width).min(cur_x + cur_w).saturating_sub(inter_x);
            let inter_h = (y + height).min(cur_y + cur_h).saturating_sub(inter_y);
            (inter_x, inter_y, inter_w, inter_h)
        } else {
            (x, y, width, height)
        };

        // Clamp the scissor to the render target. wgpu rejects a scissor whose
        // origin or right/bottom edge lies outside the attachment; the AABB math
        // above clamps the right/bottom edges but leaves the origin unclamped, so
        // a clip lying entirely past the right/bottom edge (left >= surface width)
        // would emit an out-of-bounds `x`/`y`. Clamping the origin first keeps the
        // `surface - origin` extent subtraction from underflowing.
        let (raw_x, raw_y, raw_w, raw_h) = new_scissor;
        let clamped_x = raw_x.min(surface_size.0);
        let clamped_y = raw_y.min(surface_size.1);
        let clamped_scissor = (
            clamped_x,
            clamped_y,
            raw_w.min(surface_size.0 - clamped_x),
            raw_h.min(surface_size.1 - clamped_y),
        );

        self.current_scissor = Some(clamped_scissor);

        #[cfg(debug_assertions)]
        tracing::trace!(
            "GpuStateStack::clip_rect: rect={:?} → scissor=({}, {}, {}, {})",
            rect,
            clamped_scissor.0,
            clamped_scissor.1,
            clamped_scissor.2,
            clamped_scissor.3,
        );
    }

    /// Set a SDF rounded-rectangle clip and clear any active rsuperellipse
    /// clip (the two kinds are mutually exclusive per-instance).
    ///
    /// Also applies a bounding-box `clip_rect` for early rasterizer rejection.
    pub(super) fn clip_rrect(&mut self, rrect: RRect, surface_size: (u32, u32), hard: bool) {
        self.current_clip_hard = hard;
        let rect = rrect.rect;

        // LOCAL bounds and radii — exactly what the caller asked for. The
        // mapping below is what places them; see `current_clip_inv`.
        //
        // This slot carries ONE radius per corner, so a caller's own
        // elliptical corner still has to collapse. That is now the only
        // approximation left here: a corner made elliptical by a non-uniform
        // CTM stays circular in this space and the mapping produces the
        // ellipse, which is exact.
        let max_radius = (rect.width().0 * 0.5).min(rect.height().0 * 0.5).max(0.0);
        let collapse = |rx: f32, ry: f32| rx.max(ry).min(max_radius);

        self.current_rrect_clip = [
            rect.left().0,
            rect.top().0,
            rect.width().0,
            rect.height().0,
            collapse(rrect.top_left.x.0, rrect.top_left.y.0),
            collapse(rrect.top_right.x.0, rrect.top_right.y.0),
            collapse(rrect.bottom_right.x.0, rrect.bottom_right.y.0),
            collapse(rrect.bottom_left.x.0, rrect.bottom_left.y.0),
        ];
        self.current_clip_inv = Self::device_to_local(self.current_transform);
        // Clearing the superellipse clip prevents `apply_active_clip` from
        // continuing to apply the squircle SDF after the caller switches to a
        // plain rrect. The two clip kinds are mutually exclusive at the
        // per-instance `clip_kind` level.
        self.current_rsuperellipse_clip = [0.0; 12];

        // Bounding-box scissor for early rasterizer rejection. Conservative
        // under rotation — `clip_rect` takes the AABB of all four transformed
        // corners — which is exactly what a coarse pre-pass should be now that
        // the SDF does the precise work.
        // Conservative only when the clip actually feathers. The pad exists
        // for the AA fringe, and it is not free: text is handed to glyphon
        // with `current_scissor()` alone and never evaluates the SDF, so a
        // widened scissor lets a glyph render outside the clip's own bounding
        // rectangle. Under `HardEdge` there is no fringe to protect, so the
        // exact bounds are both correct and tighter.
        self.clip_rect_with_rounding(rrect.rect, surface_size, ScissorRounding::for_clip(hard));
    }

    /// Invert the CTM's 2D affine part into the `[a, b, c, d, tx, ty]` column
    /// form `current_clip_inv` holds.
    ///
    /// The fallback is keyed on the arithmetic actually breaking down, not on
    /// the determinant clearing a threshold. An absolute epsilon rejects
    /// matrices that ARE invertible: `f32::EPSILON` is ~1.19e-7, so
    /// `scale(1e-4, 1e-4)` has determinant 1e-8 and would be declared
    /// singular — and a scale animation starting near zero passes through
    /// exactly those frames. With the clip silently identity-mapped there, its
    /// local bounds get compared against device coordinates and the draw can
    /// vanish for a frame or two at the start of every such animation.
    ///
    /// A genuinely singular matrix — `scale(0, …)` collapses geometry to a
    /// line — yields infinities, which the finiteness check catches along with
    /// NaN and any overflow the reciprocal produces. The identity is returned
    /// there, which leaves the clip evaluated in device space; nothing is drawn
    /// through a singular CTM anyway, so the choice is unobservable. It exists
    /// so the shader never compares against a non-finite mapping.
    fn device_to_local(transform: glam::Mat4) -> [f32; 6] {
        let a = transform.x_axis.x;
        let b = transform.x_axis.y;
        let c = transform.y_axis.x;
        let d = transform.y_axis.y;
        let tx = transform.w_axis.x;
        let ty = transform.w_axis.y;

        let inv_det = 1.0 / (a * d - b * c);
        let (ia, ib, ic, id) = (d * inv_det, -b * inv_det, -c * inv_det, a * inv_det);
        // local = M⁻¹ · (p − t)
        let inv = [ia, ib, ic, id, -(ia * tx + ic * ty), -(ib * tx + id * ty)];

        if inv.iter().all(|v| v.is_finite()) {
            inv
        } else {
            CLIP_INV_IDENTITY
        }
    }

    /// Set an SDF superellipse (iOS-squircle) clip and clear any active rrect
    /// clip (the two kinds are mutually exclusive per-instance).
    ///
    /// Stores LOCAL bounds and radii plus the device-to-local mapping, exactly
    /// as [`Self::clip_rrect`] does. Unlike the rrect slot this one carries rx
    /// and ry per corner, so nothing collapses here at all.
    pub(super) fn clip_rsuperellipse(
        &mut self,
        rse: flui_types::geometry::RSuperellipse,
        surface_size: (u32, u32),
        hard: bool,
    ) {
        // Assigned, not left alone: without this the squircle inherits
        // whatever mode an enclosing rounded clip set inside the same save,
        // which is a different clip's answer.
        self.current_clip_hard = hard;
        let rect = rse.outer_rect();
        let tl_r = rse.tl_radius();
        let tr_r = rse.tr_radius();
        let br_r = rse.br_radius();
        let bl_r = rse.bl_radius();

        self.current_rsuperellipse_clip = [
            rect.left().0,
            rect.top().0,
            rect.width().0,
            rect.height().0,
            tl_r.x.0,
            tl_r.y.0,
            tr_r.x.0,
            tr_r.y.0,
            br_r.x.0,
            br_r.y.0,
            bl_r.x.0,
            bl_r.y.0,
        ];
        self.current_clip_inv = Self::device_to_local(self.current_transform);
        // Clear the rrect clip to prevent `apply_active_clip` from falling
        // back to it. Mirror of the corresponding clear in `clip_rrect`.
        self.current_rrect_clip = [0.0; 8];

        // Same rule as `clip_rrect`: pad only when the clip feathers, because
        // the pad costs text its tight bound.
        self.clip_rect_with_rounding(rect, surface_size, ScissorRounding::for_clip(hard));
    }

    // =========================================================================
    // SDF clip application
    // =========================================================================

    /// The currently-active SDF clip, resolved to the single form every
    /// consumer stores.
    ///
    /// Branch order: a non-trivial `current_rsuperellipse_clip` wins
    /// (`kind = 2`), otherwise the rrect slot is used (`kind = 1` when
    /// non-zero, `kind = 0` when both are zero).
    ///
    /// This is the ONE place that selection happens. Instanced primitives
    /// reach it through `apply_active_clip`; tessellated geometry, which has
    /// no instance to hang a slot on, reads it directly for its batch uniform.
    pub(super) fn active_clip(&self) -> ResolvedClip {
        // Exact equality against the all-zero "no clip active" sentinel is
        // intentional: the field is set bit-exact whenever the clip is
        // cleared, never via arithmetic that would introduce ULP noise.
        let superellipse_active = self.current_rsuperellipse_clip != [0.0; 12];
        if superellipse_active {
            return ResolvedClip {
                rrect: super::instancing::reduce_superellipse_clip(self.current_rsuperellipse_clip),
                kind: [2, 0, u32::from(self.current_clip_hard), 0],
                device_to_local: self.current_clip_inv,
            };
        }
        let rrect_active = self.current_rrect_clip != [0.0; 8];
        if rrect_active {
            ResolvedClip {
                rrect: self.current_rrect_clip,
                kind: [1, 0, u32::from(self.current_clip_hard), 0],
                device_to_local: self.current_clip_inv,
            }
        } else {
            ResolvedClip::NONE
        }
    }

    /// Apply the currently-active SDF clip to an instance.
    ///
    /// Delegates the selection to [`Self::active_clip`] so the instanced and
    /// tessellated paths cannot disagree about which clip is in force.
    pub(super) fn apply_active_clip<I: super::instancing::ClippableInstance>(
        &self,
        instance: I,
    ) -> I {
        instance.with_clip(self.active_clip())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Offset;

    fn identity_stack() -> GpuStateStack {
        GpuStateStack::new()
    }

    /// (1) `debug_assert_balanced` panics in debug mode on an unbalanced stack.
    ///
    /// This test exercises `GpuStateStack::debug_assert_balanced` — the same
    /// method called by `WgpuPainter::reset_frame_state`. If the method or its
    /// `debug_assert!` were deleted, this test would fail to panic and be
    /// reported as a test failure by `#[should_panic]`.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unbalanced save/restore at frame boundary")]
    fn debug_assert_balanced_panics_on_unbalanced_stack() {
        let mut stack = identity_stack();
        stack.save();
        // At this point depth() == 1 — the frame boundary assert must fire.
        stack.debug_assert_balanced();
    }

    /// (1b) `depth()` is non-zero after a save without a matching restore.
    ///
    /// Documents the depth-tracking invariant independently of the assert.
    #[test]
    fn save_without_restore_leaves_nonzero_depth() {
        let mut stack = identity_stack();
        stack.save();
        assert_ne!(stack.depth(), 0, "an unmatched save must increment depth()");
    }

    /// (2) `depth()` equals `transform_stack.len()` and `save_count()` on the
    /// painter would agree. Verified via multiple save/restore cycles.
    #[test]
    fn depth_equals_transform_stack_len() {
        let mut stack = identity_stack();
        assert_eq!(stack.depth(), 0);

        stack.save();
        assert_eq!(stack.depth(), 1);

        stack.save();
        assert_eq!(stack.depth(), 2);

        stack.restore();
        assert_eq!(stack.depth(), 1);

        stack.restore();
        assert_eq!(stack.depth(), 0);
    }

    /// (3) Asymmetric scissor pop: save with no scissor → clip_rect → restore
    /// returns to no-scissor state.
    #[test]
    fn asymmetric_scissor_restore_clears_scissor() {
        let mut stack = identity_stack();
        assert_eq!(stack.current_scissor(), None, "starts with no scissor");

        // Save BEFORE setting a scissor — nothing pushed to scissor_stack.
        stack.save();
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.current_scissor(), None, "no scissor after save");

        // Now set a scissor.
        let surface = (800, 600);
        let clip_rect = Rect::from_ltrb(
            flui_types::geometry::px(10.0),
            flui_types::geometry::px(10.0),
            flui_types::geometry::px(200.0),
            flui_types::geometry::px(200.0),
        );
        stack.clip_rect(clip_rect, surface);
        assert!(
            stack.current_scissor().is_some(),
            "scissor set after clip_rect"
        );

        // Restore must clear the scissor because the paired save pushed nothing.
        stack.restore();
        assert_eq!(stack.depth(), 0);
        assert_eq!(
            stack.current_scissor(),
            None,
            "restore must clear scissor when save contributed nothing"
        );
    }

    /// (4) `reset()` restores IDENTITY current_transform (cross-frame CTM leak
    /// regression guard).
    #[test]
    fn reset_restores_identity_transform() {
        let mut stack = identity_stack();

        // Apply a non-identity transform and save.
        stack.translate(Offset::new(
            flui_types::geometry::px(42.0),
            flui_types::geometry::px(7.0),
        ));
        stack.save();
        stack.scale(2.0, 3.0);

        // Confirm the CTM is no longer identity.
        assert_ne!(
            stack.current_transform(),
            glam::Mat4::IDENTITY,
            "CTM must be non-identity before reset"
        );

        // reset() must restore identity regardless of stack depth.
        stack.reset();
        assert_eq!(
            stack.current_transform(),
            glam::Mat4::IDENTITY,
            "reset() must restore identity CTM"
        );
        assert_eq!(stack.depth(), 0, "reset() must clear all stacks");
    }

    // =========================================================================
    // P1: area_scale() correctness
    // =========================================================================

    /// diag(sx, sy) → |sx * sy|.
    ///
    /// Confirms the determinant formula on a pure axis-aligned scale.
    /// max_scale() returns `max(sx, sy)` = sy; squaring gives sy² ≠ sx*sy,
    /// which is the overestimate this fix closes.
    #[test]
    fn area_scale_diagonal_equals_product_of_scales() {
        let sx = 3.0_f32;
        let sy = 7.0_f32;
        let mut stack = identity_stack();
        stack.scale(sx, sy);
        let area = stack.area_scale();
        assert!(
            (area - sx * sy).abs() < 1e-5,
            "area_scale() for diag({sx}, {sy}) should be {expected}, got {area}",
            expected = sx * sy
        );
        // Document the overestimate max_scale² closes.
        let max_s = stack.max_scale();
        assert!(
            (max_s - sy).abs() < 1e-5,
            "max_scale() for diag({sx}, {sy}) should be {sy}, got {max_s}"
        );
        assert!(
            max_s * max_s > area * 1.01,
            "max_scale²={} should exceed area_scale={} for anisotropic scale",
            max_s * max_s,
            area
        );
    }

    /// Pure 45° rotation → area_scale == 1.0 (rotation preserves area).
    ///
    /// `max_scale()` also returns 1.0 here, so this is a parity check.
    #[test]
    fn area_scale_pure_rotation_is_one() {
        let angle = std::f32::consts::FRAC_PI_4; // 45°
        let mut stack = identity_stack();
        stack.rotate(angle);
        let area = stack.area_scale();
        assert!(
            (area - 1.0).abs() < 1e-5,
            "45° rotation must preserve area: area_scale={area}"
        );
    }

    /// Anisotropic scale(0.5, 10) → area_scale == 5.0; max_scale == 10.0.
    ///
    /// This is the canonical case the fix closes: max_scale² = 100 (4× over),
    /// while area_scale = 5 is the correct threshold multiplier.
    #[test]
    fn area_scale_anisotropic_closes_max_scale_overestimate() {
        let mut stack = identity_stack();
        stack.scale(0.5, 10.0);
        let area = stack.area_scale();
        let max_s = stack.max_scale();
        assert!(
            (area - 5.0).abs() < 1e-5,
            "area_scale() for scale(0.5, 10) should be 5.0, got {area}"
        );
        assert!(
            (max_s - 10.0).abs() < 1e-5,
            "max_scale() for scale(0.5, 10) should be 10.0, got {max_s}"
        );
        // max_scale² = 100.0 vs area_scale = 5.0 — documents the 20× overestimate.
        let overestimate_ratio = (max_s * max_s) / area;
        assert!(
            overestimate_ratio > 15.0,
            "expected ≥15× overestimate for scale(0.5, 10), got {overestimate_ratio:.1}×"
        );
    }

    /// Additional: save/restore round-trips the scissor correctly when it IS
    /// set at save time (symmetric case).
    #[test]
    fn scissor_survives_nested_save_restore() {
        let mut stack = identity_stack();
        let surface = (800, 600);
        let outer_rect = Rect::from_ltrb(
            flui_types::geometry::px(0.0),
            flui_types::geometry::px(0.0),
            flui_types::geometry::px(400.0),
            flui_types::geometry::px(300.0),
        );

        // Set outer scissor.
        stack.clip_rect(outer_rect, surface);
        let outer_scissor = stack.current_scissor();
        assert!(outer_scissor.is_some());

        // Save WITH a scissor active → it IS pushed to scissor_stack.
        stack.save();
        assert_eq!(stack.depth(), 1);

        // Apply a smaller inner clip.
        let inner_rect = Rect::from_ltrb(
            flui_types::geometry::px(50.0),
            flui_types::geometry::px(50.0),
            flui_types::geometry::px(200.0),
            flui_types::geometry::px(200.0),
        );
        stack.clip_rect(inner_rect, surface);
        let inner_scissor = stack.current_scissor();
        assert_ne!(
            inner_scissor, outer_scissor,
            "inner scissor must be smaller"
        );

        // Restore must recover the outer scissor.
        stack.restore();
        assert_eq!(
            stack.current_scissor(),
            outer_scissor,
            "outer scissor restored"
        );
    }

    /// A clip rect lying entirely past the surface's right edge must clamp
    /// its origin to the surface bound, not emit an out-of-bounds `x`.
    ///
    /// This is the exact crash case the origin clamp in `clip_rect` fixes:
    /// before the AABB math clamps the right edge, `right.saturating_sub(x)`
    /// already yields `width == 0` (900 vs 800 no wgpu problem by itself),
    /// but `x == 900` is still past the 800-wide surface — passed straight to
    /// `set_scissor_rect` that `x` alone fails wgpu's scissor-containment
    /// validation regardless of `width` being zero.
    ///
    /// Red-check: remove the `raw_x.min(surface_size.0)` / `raw_y.min(...)`
    /// origin clamp (restoring `clamped_x = raw_x`, `clamped_y = raw_y`) and
    /// this test fails — the stored scissor's `x` becomes 900, past the
    /// surface's 800px width.
    #[test]
    fn clip_rect_entirely_past_right_edge_clamps_origin_into_bounds() {
        let mut stack = identity_stack();
        let surface = (800, 600);
        let far_right_rect = Rect::from_ltrb(
            flui_types::geometry::px(900.0),
            flui_types::geometry::px(0.0),
            flui_types::geometry::px(950.0),
            flui_types::geometry::px(50.0),
        );

        stack.clip_rect(far_right_rect, surface);

        let (x, _y, w, _h) = stack
            .current_scissor()
            .expect("clip_rect always sets a scissor, even a zero-width one");
        assert!(
            x <= surface.0,
            "scissor origin x={x} must not exceed the surface width {}; an out-of-bounds x \
             fails wgpu's scissor-rect containment validation even when w == 0",
            surface.0
        );
        assert_eq!(
            w, 0,
            "a clip rect entirely past the right edge must clamp to zero width, not leave a \
             residual extent past the surface bound"
        );
    }

    /// The untransformed path must be untouched by the scaling fix.
    #[test]
    fn an_unscaled_clip_keeps_its_radii_bit_exact() {
        let mut stack = GpuStateStack::new_for_test();
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(10.0), px(20.0), px(80.0), px(40.0)),
            px(7.5),
        );

        stack.clip_rrect(rrect, (400, 400), false);

        assert_eq!(
            stack.current_rrect_clip,
            [10.0, 20.0, 80.0, 40.0, 7.5, 7.5, 7.5, 7.5],
            "an identity CTM must divide out to exactly 1.0, not to 0.999…",
        );
    }

    /// The clip slot holds the caller's shape untouched, whatever the CTM.
    ///
    /// The slot used to hold DEVICE-space bounds, which forced the radii to
    /// make the same trip — and a scaled circular corner is an ellipse, which
    /// one radius per corner cannot hold. Keeping the shape local moves that
    /// job to the mapping, where it is exact.
    ///
    /// This replaces three tests that pinned the device-space form: a HiDPI
    /// proportionality check, a non-uniform-scale radius clamp, and a
    /// per-axis superellipse scale. Each protected the same underlying rule —
    /// a circular clip must not become a rounded square — which now holds by
    /// construction rather than by arithmetic.
    #[test]
    fn a_scaled_clip_keeps_its_shape_and_scales_through_the_mapping() {
        let mut stack = GpuStateStack::new_for_test();
        let circle = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            px(50.0),
        );

        stack.scale(2.0, 3.0);
        stack.clip_rrect(circle, (600, 600), false);

        let clip = stack.active_clip();
        assert_eq!(
            &clip.rrect[0..4],
            &[0.0, 0.0, 100.0, 100.0],
            "bounds stay in the caller's space"
        );
        assert_eq!(&clip.rrect[4..8], &[50.0; 4], "and so do the radii");

        // The mapping is the inverse: device (2x, 3y) must land back on (x, y).
        let m = clip.device_to_local;
        let local_x = m[0] * 200.0 + m[2] * 300.0 + m[4];
        let local_y = m[1] * 200.0 + m[3] * 300.0 + m[5];
        assert!(
            (local_x - 100.0).abs() < 1e-3 && (local_y - 100.0).abs() < 1e-3,
            "device (200, 300) must map to local (100, 100), got ({local_x}, {local_y})"
        );
    }

    /// A rotated CTM now populates the slot instead of surrendering to the
    /// scissor.
    ///
    /// The old device-space form could not express a rotation at all — its
    /// bounds came from two corners, which for a rotated rect is not even the
    /// AABB — so `clip_rrect` detected the case and fell back to the coarse
    /// scissor, over-including by 41% per axis at 45°. A local shape plus a
    /// mapping has no such limit.
    #[test]
    fn a_rotated_clip_populates_the_slot_and_maps_back() {
        let mut stack = GpuStateStack::new_for_test();
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            px(10.0),
        );

        stack.rotate(std::f32::consts::FRAC_PI_2);
        stack.clip_rrect(rrect, (400, 400), false);

        let clip = stack.active_clip();
        assert_eq!(
            clip.kind[0], 1,
            "a rotated clip must still populate the slot"
        );
        assert_eq!(
            &clip.rrect[0..4],
            &[0.0, 0.0, 100.0, 100.0],
            "with the caller's own bounds"
        );

        // A 90° rotation sends local (100, 0) to device (0, 100); the mapping
        // must send it back.
        let m = clip.device_to_local;
        let local_x = m[0] * 0.0 + m[2] * 100.0 + m[4];
        let local_y = m[1] * 0.0 + m[3] * 100.0 + m[5];
        assert!(
            (local_x - 100.0).abs() < 1e-3 && local_y.abs() < 1e-3,
            "device (0, 100) must map to local (100, 0), got ({local_x}, {local_y})"
        );

        assert!(
            stack.current_scissor().is_some(),
            "the coarse scissor still applies as an early-rejection pre-pass"
        );
    }

    /// A tiny-but-invertible scale is inverted, not written off as singular.
    ///
    /// An absolute determinant threshold gets this wrong: `f32::EPSILON` is
    /// ~1.19e-7, so `scale(1e-4, 1e-4)` has determinant 1e-8 and clears no
    /// such bar despite being perfectly invertible. A scale animation starting
    /// near zero passes through exactly these frames, and an identity-mapped
    /// clip there compares its local bounds against device coordinates — the
    /// draw can vanish for a frame or two at the start of every one.
    ///
    /// A local 1,000,000-unit box under this scale is a 100-unit box on
    /// screen, so a device point at 50 must map back to 500,000.
    #[test]
    fn a_tiny_but_invertible_scale_is_still_inverted() {
        let mut stack = GpuStateStack::new_for_test();
        stack.scale(1e-4, 1e-4);
        stack.clip_rrect(
            RRect::from_rect_circular(
                Rect::from_xywh(px(0.0), px(0.0), px(1.0e6), px(1.0e6)),
                px(1.0e5),
            ),
            (400, 400),
            false,
        );

        let m = stack.active_clip().device_to_local;
        assert_ne!(
            m, CLIP_INV_IDENTITY,
            "an invertible CTM must not be written off as singular"
        );

        let local_x = m[0] * 50.0 + m[2] * 50.0 + m[4];
        let local_y = m[1] * 50.0 + m[3] * 50.0 + m[5];
        assert!(
            (local_x - 500_000.0).abs() < 1.0 && (local_y - 500_000.0).abs() < 1.0,
            "device (50, 50) must map to local (500000, 500000), got ({local_x}, {local_y})"
        );
    }

    /// A singular CTM has no inverse; the mapping falls back to the identity
    /// rather than producing infinities the shader would compare against.
    #[test]
    fn a_singular_ctm_yields_the_identity_mapping() {
        let mut stack = GpuStateStack::new_for_test();
        stack.scale(0.0, 1.0);
        stack.clip_rrect(
            RRect::from_rect_circular(
                Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
                px(10.0),
            ),
            (400, 400),
            false,
        );
        assert_eq!(
            stack.active_clip().device_to_local,
            CLIP_INV_IDENTITY,
            "a collapsed CTM must not put NaN or inf in the mapping"
        );
    }
}
