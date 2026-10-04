//! GPU draw-state stack: transform, scissor, and SDF clip.
//!
//! Each saved entry contains the complete transform and clip state.
//!
//! # Balance assertion
//!
//! The frame boundary (`WgpuPainter::reset_frame_state`) calls
//! `self.state.debug_assert_balanced()` **before** calling `self.state.reset()`.
//! The assertion logic lives in `GpuStateStack::debug_assert_balanced` so it
//! can be exercised in unit tests without a GPU.
//! No `Drop` assertion is provided: a panic during unwind would abort the process.

use flui_foundation::geometry::{self, Offset, Point, RRect, Rect};

/// GPU draw state and complete snapshots for nested save/restore scopes.
///
/// `WgpuPainter` holds one `GpuStateStack` and delegates all
/// transform/scissor/clip mutation through it, keeping every draw method free
/// to read the current values without a whole-struct borrow conflict (all
/// accessors return owned values, never a borrow of the mutable stack).
#[derive(Debug)]
pub(super) struct GpuStateStack {
    saved: Vec<SavedState>,
    current_clip_chain: crate::clip_chain::ClipChain,
    clip_groups: Vec<SavedState>,

    /// Current accumulated transform (CTM). Identity at frame start.
    ///
    /// Composed in `f64`, the precision the display list records offsets in,
    /// and narrowed to `f32` only when read: a large ancestor offset and its
    /// child's nearly cancelling one keep their residual.
    current_transform: glam::DMat4,

    /// Current active scissor rectangle in physical pixels `(x, y, w, h)`.
    /// `None` means no axis-aligned scissor clip is active.
    current_scissor: Option<(u32, u32, u32, u32)>,

    /// Active SDF rounded-rectangle clip uniform. All-zeros means no clip.
    current_rrect_clip: [f32; 8],
    /// Whether the active SDF clip has a hard edge (`Clip::HardEdge`) rather
    /// than a feathered one (`Clip::AntiAlias`).
    ///
    /// Saved and restored with the clip itself, so a hard clip nested inside a
    /// smooth one does not leak its mode outward on `restore`.
    current_clip_hard: bool,

    /// Active SDF superellipse clip uniform. All-zeros means no clip.
    current_rsuperellipse_clip: [f32; 12],

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

#[derive(Debug, Clone)]
struct SavedState {
    clip_chain: crate::clip_chain::ClipChain,
    transform: glam::DMat4,
    scissor: Option<(u32, u32, u32, u32)>,
    rrect_clip: [f32; 8],
    rsuperellipse_clip: [f32; 12],
    clip_hard: bool,
    device_to_local: [f32; 6],
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

impl GpuStateStack {
    /// Separate inherited coverage and culling from the child-local suffix.
    /// This stack is independent of canvas Save/Restore and never changes CTM.
    pub(super) fn begin_clip_group(&mut self) -> crate::clip_chain::ClipChain {
        let parent = std::mem::take(&mut self.current_clip_chain);
        self.clip_groups.push(SavedState {
            clip_chain: parent.clone(),
            transform: self.current_transform,
            scissor: self.current_scissor,
            rrect_clip: self.current_rrect_clip,
            rsuperellipse_clip: self.current_rsuperellipse_clip,
            clip_hard: self.current_clip_hard,
            device_to_local: self.current_clip_inv,
        });
        self.current_scissor = None;
        self.current_rrect_clip = [0.0; 8];
        self.current_rsuperellipse_clip = [0.0; 12];
        self.current_clip_hard = false;
        self.current_clip_inv = CLIP_INV_IDENTITY;
        parent
    }

    pub(super) fn end_clip_group(&mut self) {
        if let Some(parent) = self.clip_groups.pop() {
            self.current_clip_chain = parent.clip_chain;
            self.current_scissor = parent.scissor;
            self.current_rrect_clip = parent.rrect_clip;
            self.current_rsuperellipse_clip = parent.rsuperellipse_clip;
            self.current_clip_hard = parent.clip_hard;
            self.current_clip_inv = parent.device_to_local;
        }
    }

    pub(super) fn clip_chain(&self) -> crate::clip_chain::ClipChain {
        self.current_clip_chain.clone()
    }

    pub(super) fn append_clip(
        &mut self,
        shape: crate::clip_geometry::ValidatedClip,
        op: crate::clip_chain::ClipOp,
        hard: bool,
        budget: &std::sync::Arc<crate::recording_budget::RecordingBudget>,
    ) -> Result<(), crate::command_ir::RecordError> {
        let affine = crate::clip_geometry::ValidatedAffine::new(self.current_transform)
            .map_err(crate::command_ir::RecordError::Geometry)?;
        self.current_clip_chain = self
            .current_clip_chain
            .append(shape, affine, op, hard, budget)?;
        Ok(())
    }

    /// Construct a pristine stack — identity transform, no scissor, no SDF
    /// clips, all stacks empty. Equivalent to the post-`reset()` state.
    pub(super) fn new() -> Self {
        Self {
            saved: Vec::new(),
            current_clip_chain: crate::clip_chain::ClipChain::default(),
            clip_groups: Vec::new(),
            current_transform: glam::DMat4::IDENTITY,
            current_scissor: None,
            current_rrect_clip: [0.0; 8],
            current_clip_hard: false,
            current_rsuperellipse_clip: [0.0; 12],
            current_clip_inv: CLIP_INV_IDENTITY,
        }
    }

    // =========================================================================
    // Frame boundary
    // =========================================================================

    /// Reset all stacks and cached values to the pristine frame-start state.
    ///
    /// The caller (`WgpuPainter::reset_frame_state`) is responsible for
    /// asserting `depth() == 0` **before** calling this method.
    pub(super) fn reset(&mut self) {
        self.current_clip_chain = crate::clip_chain::ClipChain::default();
        self.clip_groups.clear();
        self.current_scissor = None;
        self.current_rrect_clip = [0.0; 8];
        self.current_clip_hard = false;
        self.current_rsuperellipse_clip = [0.0; 12];
        self.current_clip_inv = CLIP_INV_IDENTITY;
        // Identity is the construction-time value. Reset to the same initial
        // value so no cross-frame CTM leak can occur.
        self.current_transform = glam::DMat4::IDENTITY;
        self.saved.clear();
    }

    // =========================================================================
    // Depth query
    // =========================================================================

    /// Current save-stack depth — equals `saved.len()`, the single
    /// source of truth. No parallel counter is maintained.
    #[inline]
    pub(super) fn depth(&self) -> usize {
        self.saved.len()
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
            self.clip_groups.is_empty(),
            "unbalanced clip groups at frame boundary"
        );
        debug_assert!(
            self.saved.is_empty(),
            "unbalanced save/restore at frame boundary: depth={}",
            self.depth()
        );
    }

    // =========================================================================
    // Save / restore
    // =========================================================================

    /// Save the complete transform and clip state for a nested drawing scope.
    pub(super) fn save(&mut self) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::save: depth={}", self.saved.len());

        self.saved.push(SavedState {
            clip_chain: self.current_clip_chain.clone(),
            transform: self.current_transform,
            scissor: self.current_scissor,
            rrect_clip: self.current_rrect_clip,
            rsuperellipse_clip: self.current_rsuperellipse_clip,
            clip_hard: self.current_clip_hard,
            device_to_local: self.current_clip_inv,
        });
    }

    /// Restore the transform and clip state of the enclosing drawing scope.
    ///
    /// Logs a warning on underflow (no matching `save()`) and returns early
    /// without mutating state.
    pub(super) fn restore(&mut self) {
        let Some(saved) = self.saved.pop() else {
            // Unconditional: an unbalanced `restore` is a caller bug whose
            // symptom (wrong transforms for the rest of the frame) is far
            // from its cause, and a release build is where it is seen.
            tracing::warn!("GpuStateStack::restore: stack underflow");
            return;
        };

        self.current_clip_chain = saved.clip_chain;
        self.current_transform = saved.transform;
        self.current_scissor = saved.scissor;
        self.current_rrect_clip = saved.rrect_clip;
        self.current_rsuperellipse_clip = saved.rsuperellipse_clip;
        self.current_clip_hard = saved.clip_hard;
        self.current_clip_inv = saved.device_to_local;

        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::restore: depth={}", self.saved.len());
    }

    // =========================================================================
    // Transform mutators
    // =========================================================================

    /// Post-multiply the CTM by a translation.
    pub(super) fn translate(&mut self, offset: Offset<f64>) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::translate: offset={:?}", offset);

        self.current_transform *=
            glam::DMat4::from_translation(glam::dvec3(offset.dx, offset.dy, 0.0));
    }

    /// Post-multiply the CTM by a Z-axis rotation.
    pub(super) fn rotate(&mut self, angle_radians: f32) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::rotate: angle={}", angle_radians);

        self.current_transform *= glam::DMat4::from_rotation_z(f64::from(angle_radians));
    }

    /// Post-multiply the CTM by an arbitrary matrix — the whole matrix, so a
    /// skew or a perspective row survives. `Matrix4` and `glam::DMat4` are
    /// both column-major `[f64; 16]`; the conversion is a reinterpretation of
    /// the sixteen values, the inverse of [`Self::current_transform_matrix`].
    pub(super) fn concat(&mut self, matrix: &flui_foundation::geometry::Matrix4) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::concat: matrix={:?}", matrix);

        self.current_transform *= glam::DMat4::from_cols_array(&matrix.m);
    }

    /// Restore a command override without undoing clip mutations made under it.
    pub(super) fn restore_transform(&mut self, matrix: &geometry::Matrix4) {
        self.current_transform = glam::DMat4::from_cols_array(&matrix.m);
    }

    /// Post-multiply the CTM by a uniform scale.
    pub(super) fn scale(&mut self, sx: f32, sy: f32) {
        #[cfg(debug_assertions)]
        tracing::trace!("GpuStateStack::scale: sx={}, sy={}", sx, sy);

        self.current_transform *=
            glam::DMat4::from_scale(glam::dvec3(f64::from(sx), f64::from(sy), 1.0));
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
        self.current_transform.as_mat4()
    }

    /// The accumulated CTM as a [`flui_foundation::geometry::Matrix4`] (column-major).
    ///
    /// Both `glam::DMat4` and `flui_foundation::geometry::Matrix4` are column-major
    /// `[f64; 16]`, so the conversion is a direct reinterpret of the 16 values, at full
    /// precision. Transform baking and HiDPI device sizing depend on that layout.
    pub(super) fn current_transform_matrix(&self) -> flui_foundation::geometry::Matrix4 {
        flui_foundation::geometry::Matrix4 {
            m: self.current_transform.to_cols_array(),
        }
    }

    /// Apply the CTM to a local-space point and return the screen-space result.
    pub(super) fn apply_transform(&self, point: Point<f64>) -> Point<f64> {
        let p = self.current_transform * glam::dvec4(point.x, point.y, 0.0, 1.0);
        Point::new(p.x, p.y)
    }

    /// `true` when the current transform has no rotation or skew component.
    ///
    /// When `false`, rects must be tessellated rather than instanced.
    pub(super) fn is_axis_aligned(&self) -> bool {
        let m = self.current_transform();
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
        let m = self.current_transform();
        // Preserve NaN propagation: hypot(infinity, NaN) alone is infinity.
        let col_x = if m.x_axis.x.is_nan() || m.x_axis.y.is_nan() {
            f32::NAN
        } else {
            m.x_axis.x.hypot(m.x_axis.y)
        };
        let col_y = if m.y_axis.x.is_nan() || m.y_axis.y.is_nan() {
            f32::NAN
        } else {
            m.y_axis.x.hypot(m.y_axis.y)
        };
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
        let m = self.current_transform();
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
    ///
    /// A hard edge (ADR-0098 §6): under a translation plus a positive
    /// axis-aligned scale each device edge snaps to the nearest pixel boundary,
    /// so a pixel is kept exactly when its centre is inside the clip — Skia's
    /// non-antialiased clip. A clip ending at column 10.75 keeps column 10
    /// (centre 10.5) and one starting at 0.75 drops column 0 (centre 0.5).
    /// Under rotation, skew or a reflection the device bounding box is covered
    /// instead, since no pixel grid lines up with the clip's edges. A clip with
    /// nothing behind it that must keep every partly covered pixel uses
    /// [`Self::clip_rect_enclosing`].
    pub(super) fn clip_rect(&mut self, rect: Rect<f64>, surface_size: (u32, u32)) {
        let (x, y, width, height) = self.scissor_of(rect, surface_size);
        self.commit_scissor(rect, (x, y, width, height), surface_size);
    }

    /// Set an axis-aligned scissor clip rounded OUTWARD to the pixel grid.
    ///
    /// The sibling of [`Self::clip_rect`] for a clip that is the whole of its
    /// own enforcement. `WgpuPainter::clip_path` approximates an arbitrary path
    /// by its bounding box, and the property that makes that safe is that the
    /// box is a SUPERSET of the shape — truncating its right and bottom edges
    /// takes the superset away and with it up to a column and a row of content
    /// the path genuinely keeps.
    ///
    /// The growth happens HERE, after the transform, and cannot be done by the
    /// caller: the scissor is derived from the transformed corners, so a clip
    /// rect the caller has already grown to whole pixels is re-fractioned by
    /// any translation with a fractional part — which is every node at a
    /// non-integer offset, and every node at all under a fractional device
    /// pixel ratio. Growing before the transform is not merely insufficient,
    /// it is inert.
    pub(super) fn clip_rect_enclosing(&mut self, rect: Rect<f64>, surface_size: (u32, u32)) {
        let scissor =
            Self::clamp_to_surface(geometry::cover(self.device_bounds(rect)), surface_size);
        self.commit_scissor(rect, scissor, surface_size);
    }

    /// `true` when the transform is a translation plus a positive axis-aligned
    /// scale: the only transforms under which device edges can be snapped
    /// (ADR-0098 §6). A rotation, skew, reflection or perspective term fails it.
    pub(super) fn is_translate_scale(&self) -> bool {
        let m = self.current_transform();
        m.x_axis.y.abs() < 1e-6
            && m.y_axis.x.abs() < 1e-6
            && m.x_axis.x > 0.0
            && m.y_axis.y > 0.0
            && m.x_axis.w == 0.0
            && m.y_axis.w == 0.0
            && m.w_axis.w == 1.0
    }

    /// The clip rect's device bounding box as a [`Rect`].
    fn device_bounds(&self, rect: Rect<f64>) -> Rect<f64> {
        let (min_x, min_y, max_x, max_y) = self.device_aabb(rect);
        Rect::from_ltrb(
            f64::from(min_x),
            f64::from(min_y),
            f64::from(max_x),
            f64::from(max_y),
        )
    }

    /// A pixel-aligned device rect clamped to the attachment, as a scissor
    /// `(x, y, width, height)`.
    fn clamp_to_surface(rect: Rect<f64>, (width, height): (u32, u32)) -> (u32, u32, u32, u32) {
        let clamp = |v: f64, max: u32| v.clamp(0.0, f64::from(max)) as u32;
        let x = clamp(rect.left(), width);
        let y = clamp(rect.top(), height);
        let right = clamp(rect.right(), width);
        let bottom = clamp(rect.bottom(), height);
        (x, y, right.saturating_sub(x), bottom.saturating_sub(y))
    }

    /// The clip rect's axis-aligned bounding box in device space, unrounded.
    ///
    /// The identity case is exact; otherwise the four corners are transformed
    /// and their AABB taken, which is conservative for a rotation — the box
    /// around a rotated box is larger than the box.
    fn device_aabb(&self, rect: Rect<f64>) -> (f32, f32, f32, f32) {
        let transform = self.current_transform();
        if transform == glam::Mat4::IDENTITY {
            return (
                (rect.left() as f32),
                (rect.top() as f32),
                (rect.right() as f32),
                (rect.bottom() as f32),
            );
        }
        let corners = [
            transform.transform_point3(glam::Vec3::new(rect.left() as f32, rect.top() as f32, 0.0)),
            transform.transform_point3(glam::Vec3::new(
                rect.right() as f32,
                rect.top() as f32,
                0.0,
            )),
            transform.transform_point3(glam::Vec3::new(
                rect.right() as f32,
                rect.bottom() as f32,
                0.0,
            )),
            transform.transform_point3(glam::Vec3::new(
                rect.left() as f32,
                rect.bottom() as f32,
                0.0,
            )),
        ];
        (
            corners.iter().map(|c| c.x).fold(f32::INFINITY, f32::min),
            corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min),
            corners
                .iter()
                .map(|c| c.x)
                .fold(f32::NEG_INFINITY, f32::max),
            corners
                .iter()
                .map(|c| c.y)
                .fold(f32::NEG_INFINITY, f32::max),
        )
    }

    /// The hard-edge scissor of [`Self::clip_rect`]: edges snapped under a
    /// translation plus a positive scale, the bounding box covered otherwise.
    fn scissor_of(&self, rect: Rect<f64>, surface_size: (u32, u32)) -> (u32, u32, u32, u32) {
        let device = self.device_bounds(rect);
        let aligned = if self.is_translate_scale() {
            geometry::snap_edges(device)
        } else {
            geometry::cover(device)
        };
        Self::clamp_to_surface(aligned, surface_size)
    }

    /// Intersect a freshly computed scissor with any active one, clamp it to
    /// the attachment, and store it.
    ///
    /// wgpu rejects a scissor whose origin or right/bottom edge lies outside
    /// the attachment, and the origin is the half the AABB maths above leaves
    /// unclamped — a clip lying entirely past the right or bottom edge would
    /// emit an out-of-bounds `x`/`y`, and clamping the origin first keeps the
    /// `surface - origin` extent subtraction from underflowing.
    fn commit_scissor(
        &mut self,
        rect: Rect<f64>,
        scissor: (u32, u32, u32, u32),
        surface_size: (u32, u32),
    ) {
        let (x, y, width, height) = scissor;
        let new_scissor = if let Some((cur_x, cur_y, cur_w, cur_h)) = self.current_scissor {
            let inter_x = x.max(cur_x);
            let inter_y = y.max(cur_y);
            let inter_w = (x + width).min(cur_x + cur_w).saturating_sub(inter_x);
            let inter_h = (y + height).min(cur_y + cur_h).saturating_sub(inter_y);
            (inter_x, inter_y, inter_w, inter_h)
        } else {
            (x, y, width, height)
        };

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
        #[cfg(not(debug_assertions))]
        let _ = rect;
    }

    /// Set a SDF rounded-rectangle clip and clear any active rsuperellipse
    /// clip (the two kinds are mutually exclusive per-instance).
    ///
    /// Also applies a bounding-box `clip_rect` for early rasterizer rejection.
    pub(super) fn clip_rrect(&mut self, rrect: RRect, surface_size: (u32, u32), hard: bool) {
        let resolved = self.resolve_rrect_clip(rrect, hard);
        self.current_clip_hard = hard;
        self.current_rrect_clip = resolved.rrect;
        self.current_clip_inv = resolved.device_to_local;
        // Clearing the superellipse clip prevents `apply_active_clip` from
        // continuing to apply the squircle SDF after the caller switches to a
        // plain rrect. The two clip kinds are mutually exclusive at the
        // per-instance `clip_kind` level.
        self.current_rsuperellipse_clip = [0.0; 12];

        // Bounding-box scissor in front of the SDF, covering every pixel the
        // clip touches (ADR-0098 §6): the SDF does the exact work, so the
        // scissor must not cut the feathered fringe on either side. Snapping
        // the edges here instead drops a half-covered edge column the SDF
        // would have feathered. Text does not read the SDF yet, so a glyph can
        // show in a partly covered edge pixel; that is what #848 stays open for.
        let _ = hard;
        self.clip_rect_enclosing(rrect.rect, surface_size);
    }

    /// Resolve a rounded clip against the current transform WITHOUT installing
    /// it anywhere.
    ///
    /// The one place the local-bounds/radii layout and the device-to-local
    /// mapping are derived. [`Self::clip_rrect`] stores the result in the
    /// per-draw slot. A second copy of this arithmetic would let the
    /// two disagree about where the same clip is, with nothing failing.
    ///
    /// The `kind` lane layout is shared with [`Self::active_clip`], which builds
    /// the same value from the stored slots; a lane added to one belongs in the
    /// other.
    pub(super) fn resolve_rrect_clip(&self, rrect: RRect, hard: bool) -> ResolvedClip {
        let rect = rrect.rect;

        // LOCAL bounds and radii — exactly what the caller asked for. The
        // mapping below is what places them; see `current_clip_inv`.
        //
        // This slot carries ONE radius per corner, so a caller's own
        // elliptical corner still has to collapse. That is now the only
        // approximation left here: a corner made elliptical by a non-uniform
        // CTM stays circular in this space and the mapping produces the
        // ellipse, which is exact.
        let max_radius = (rect.width() * 0.5).min(rect.height() * 0.5).max(0.0);
        let collapse = |rx: f32, ry: f32| rx.max(ry).min(max_radius as f32);

        ResolvedClip {
            rrect: [
                (rect.left() as f32),
                (rect.top() as f32),
                (rect.width() as f32),
                (rect.height() as f32),
                collapse(rrect.top_left.x as f32, rrect.top_left.y as f32),
                collapse(rrect.top_right.x as f32, rrect.top_right.y as f32),
                collapse(rrect.bottom_right.x as f32, rrect.bottom_right.y as f32),
                collapse(rrect.bottom_left.x as f32, rrect.bottom_left.y as f32),
            ],
            kind: [1, 0, u32::from(hard), 0],
            device_to_local: Self::device_to_local(self.current_transform()),
        }
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
        rse: flui_foundation::geometry::RSuperellipse,
        surface_size: (u32, u32),
        hard: bool,
    ) {
        // Assigned, not left alone: without this the squircle inherits
        // whatever mode an enclosing rounded clip set inside the same save,
        // which is a different clip's answer.
        self.current_clip_hard = hard;
        let rect = rse.outer_rect();

        self.current_rsuperellipse_clip = Self::rsuperellipse_slots(rse);
        self.current_clip_inv = Self::device_to_local(self.current_transform());
        // Clear the rrect clip to prevent `apply_active_clip` from falling
        // back to it. Mirror of the corresponding clear in `clip_rrect`.
        self.current_rrect_clip = [0.0; 8];

        // Covering, for the reason spelled out in `clip_rrect`.
        let _ = hard;
        self.clip_rect_enclosing(rect, surface_size);
    }

    // =========================================================================
    // SDF clip application
    // =========================================================================

    /// Lay a rounded superellipse out into the twelve-float slot form:
    /// `[x, y, w, h]` then `rx, ry` per corner, clockwise from top-left.
    ///
    /// Shared by [`Self::clip_rsuperellipse`] and
    /// the group composite path for the reason
    /// [`Self::resolve_rrect_clip`]'s doc gives about its own pair: a second
    /// copy of this arithmetic would let the per-draw and at-composite routes
    /// disagree about where the same clip is, with nothing failing.
    fn rsuperellipse_slots(rse: flui_foundation::geometry::RSuperellipse) -> [f32; 12] {
        let rect = rse.outer_rect();
        let tl_r = rse.tl_radius();
        let tr_r = rse.tr_radius();
        let br_r = rse.br_radius();
        let bl_r = rse.bl_radius();
        [
            (rect.left() as f32),
            (rect.top() as f32),
            (rect.width() as f32),
            (rect.height() as f32),
            (tl_r.x as f32),
            (tl_r.y as f32),
            ((tr_r.x) as f32),
            ((tr_r.y) as f32),
            ((br_r.x) as f32),
            ((br_r.y) as f32),
            ((bl_r.x) as f32),
            ((bl_r.y) as f32),
        ]
    }

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
                rrect: crate::instancing::reduce_superellipse_clip(self.current_rsuperellipse_clip),
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
    pub(super) fn apply_active_clip<I: crate::instancing::ClippableInstance>(
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
    use flui_foundation::geometry::Offset;

    fn identity_stack() -> GpuStateStack {
        GpuStateStack::new()
    }

    // =========================================================================
    // P1: area_scale() correctness
    // =========================================================================

    /// A hard rect clip keeps a pixel exactly when its centre is inside, under
    /// the identity and under a translation plus a positive scale alike: edges
    /// snap to the nearest pixel boundary, ties toward +∞ (ADR-0098 §6).
    ///
    /// Red-check: truncating every edge (the rule this replaced) gives
    /// `(0, 0, 10, 10)` for the identity case, keeping column 0 whose centre
    /// is outside and dropping column 10 whose centre is inside.
    #[test]
    fn a_hard_rect_clip_keeps_the_pixels_whose_centres_are_inside() {
        let surface = (64, 64);

        let mut identity = identity_stack();
        identity.clip_rect(Rect::from_ltrb(0.75, 0.75, 10.75, 10.75), surface);
        assert_eq!(identity.current_scissor(), Some((1, 1, 10, 10)));

        // Half-way edges: 0.5 and 10.5 both snap up, so the width stays 10.
        let mut half = identity_stack();
        half.clip_rect(Rect::from_ltrb(0.5, 0.5, 10.5, 10.5), surface);
        assert_eq!(half.current_scissor(), Some((1, 1, 10, 10)));

        // Under scale(2) + translate(0.1): [0.3, 5.3] → device [0.7, 10.7] → [1, 11).
        let mut scaled = identity_stack();
        scaled.translate(Offset::new(0.1, 0.1));
        scaled.scale(2.0, 2.0);
        scaled.clip_rect(Rect::from_ltrb(0.3, 0.3, 5.3, 5.3), surface);
        assert_eq!(scaled.current_scissor(), Some((1, 1, 10, 10)));
    }
}
