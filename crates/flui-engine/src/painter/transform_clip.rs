// ===== Transform Stack & Clipping =====
//
// Moved from `painter.rs` into `painter/transform_clip.rs` as part of the
// C1 LOC-cap refactor.  Zero behaviour changes.

use flui_foundation::geometry::{Offset, RRect, Rect};
use flui_painting::paint::Path;

use super::WgpuPainter;

impl WgpuPainter {
    fn validate_current_transform(&mut self) {
        if let Err(error) = crate::clip_geometry::ValidatedAffine::new(
            glam::DMat4::from_cols_array(&self.state.current_transform_matrix().m),
        ) {
            self.current_segment
                .budget
                .record_error(crate::command_ir::RecordError::Geometry(error));
        }
    }

    fn append_validated_clip(
        &mut self,
        shape: Result<crate::clip_geometry::ValidatedClip, crate::error::GeometryError>,
        op: crate::clip_chain::ClipOp,
        hard: bool,
    ) -> bool {
        let result = shape
            .map_err(crate::command_ir::RecordError::Geometry)
            .and_then(|shape| {
                self.state
                    .append_clip(shape, op, hard, &self.current_segment.budget)
            });
        match result {
            Ok(()) => true,
            Err(error) => {
                self.current_segment.budget.record_error(error);
                false
            }
        }
    }

    pub(crate) fn restore_transform(&mut self, matrix: &flui_foundation::geometry::Matrix4) {
        self.state.restore_transform(matrix);
        self.validate_current_transform();
    }

    pub(crate) fn captured_group_clip(&self) -> crate::command_ir::GroupClip {
        crate::command_ir::GroupClip {
            legacy: crate::state_stack::ResolvedClip::NONE,
            chain: self.state.clip_chain(),
        }
    }

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
    /// Popping from an empty stack is a caller bug: it logs a
    /// `tracing::warn!` (in every build) and leaves the current state
    /// unchanged.
    pub fn restore(&mut self) {
        self.state.restore();
    }

    /// Concatenate a translation onto the current transform.
    ///
    /// `offset` is in device pixels.  Equivalent to premultiplying the CTM by
    /// `T(offset.dx, offset.dy)`.
    pub fn translate(&mut self, offset: Offset<f64>) {
        self.state.translate(offset);
        self.validate_current_transform();
    }

    /// Concatenate an arbitrary matrix onto the current transform — the whole
    /// matrix, so skew is retained. Perspective and non-finite results are rejected
    /// through the frame recording error. Equivalent to `Canvas::transform`.
    pub fn transform(&mut self, matrix: &flui_foundation::geometry::Matrix4) {
        self.state.concat(matrix);
        self.validate_current_transform();
    }

    /// Concatenate a clockwise rotation onto the current transform.
    ///
    /// `angle` is in radians.  Equivalent to premultiplying the CTM by
    /// `R(angle)` (rotation about the origin in the current coordinate space).
    pub fn rotate(&mut self, angle: f32) {
        self.state.rotate(angle);
        self.validate_current_transform();
    }

    /// Concatenate a non-uniform scale onto the current transform.
    ///
    /// `sx` and `sy` are scale factors along the X and Y axes respectively.
    /// Equivalent to premultiplying the CTM by `S(sx, sy)`.  Negative values
    /// produce a reflection; zero produces a degenerate transform that collapses
    /// all geometry to a line or point.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.state.scale(sx, sy);
        self.validate_current_transform();
    }

    // ===== Clipping =====

    /// Intersect with a captured rectangle; AA coverage is resolved with the full chain.
    pub fn clip_rect(&mut self, rect: Rect<f64>, clip: flui_painting::paint::Clip) {
        self.clip_rect_operation(rect, clip, crate::clip_chain::ClipOp::Intersect);
    }

    pub(crate) fn clip_rect_operation(
        &mut self,
        rect: Rect<f64>,
        clip: flui_painting::paint::Clip,
        op: crate::clip_chain::ClipOp,
    ) {
        if self.current_segment.recording_result().is_err() {
            return;
        }
        if matches!(clip, flui_painting::paint::Clip::None) {
            return;
        }
        let hard = matches!(clip, flui_painting::paint::Clip::HardEdge);
        if !self.append_validated_clip(crate::clip_geometry::ValidatedClip::rect(rect), op, hard) {
            return;
        }
        if op == crate::clip_chain::ClipOp::Intersect {
            if hard {
                self.state.clip_rect(rect, self.size);
            } else {
                self.state.clip_rect_enclosing(rect, self.size);
            }
        }
    }

    /// Intersect the current scissor with the pixels `rect` touches at all:
    /// floor of the minimum, ceiling of the maximum (ADR-0098 §6). For bounds
    /// that must never lose a partly covered pixel, such as a damage region.
    pub(crate) fn clip_rect_enclosing(&mut self, rect: Rect<f64>) {
        self.state.clip_rect_enclosing(rect, self.size);
    }

    /// Intersect with a captured rounded rectangle, retaining both corner radii.
    pub fn clip_rrect(&mut self, rrect: RRect, clip: flui_painting::paint::Clip) {
        self.clip_rrect_operation(rrect, clip, crate::clip_chain::ClipOp::Intersect);
    }

    pub(crate) fn clip_rrect_operation(
        &mut self,
        rrect: RRect,
        clip: flui_painting::paint::Clip,
        op: crate::clip_chain::ClipOp,
    ) {
        if self.current_segment.recording_result().is_err() {
            return;
        }
        if matches!(clip, flui_painting::paint::Clip::None) {
            return;
        }
        let hard = matches!(clip, flui_painting::paint::Clip::HardEdge);
        if !self.append_validated_clip(crate::clip_geometry::ValidatedClip::rrect(rrect), op, hard)
        {
            return;
        }
        if op == crate::clip_chain::ClipOp::Intersect {
            self.state.clip_rrect(rrect, self.size, hard);
        }
    }

    /// The active scissor as a device-space rect, or the whole surface when no
    /// scissor is set.
    ///
    /// This is the compositing bounds for a `Clip::AntiAliasWithSaveLayer`
    /// layer: the scissor is the clip's device-space bounding box already
    /// intersected with every ancestor clip, and every draw inside the layer is
    /// subject to it, so nothing the offscreen holds can fall outside.
    pub(crate) fn clip_bounds(&self) -> Rect<f64> {
        self.state.current_scissor().map_or_else(
            || match self.filter_desired_output() {
                Ok(bounds) => bounds,
                Err(error) => {
                    self.current_segment.budget.record_error(error);
                    Rect::ZERO
                }
            },
            |(x, y, width, height)| {
                Rect::from_xywh(
                    f64::from(x as f32),
                    f64::from(y as f32),
                    f64::from(width as f32),
                    f64::from(height as f32),
                )
            },
        )
    }

    /// Capture soft superellipse coverage for the group composite.
    pub(crate) fn clip_rsuperellipse_at_composite(
        &mut self,
        rse: flui_foundation::geometry::RSuperellipse,
    ) -> crate::command_ir::GroupClip {
        self.clip_rsuperellipse(rse, flui_painting::paint::Clip::AntiAlias);
        self.captured_group_clip()
    }

    pub(crate) fn clip_rrect_at_composite(&mut self, rrect: RRect) -> crate::command_ir::GroupClip {
        self.clip_rrect(rrect, flui_painting::paint::Clip::AntiAlias);
        self.captured_group_clip()
    }

    /// Intersect with a captured rounded superellipse.
    pub fn clip_rsuperellipse(
        &mut self,
        rse: flui_foundation::geometry::RSuperellipse,
        clip: flui_painting::paint::Clip,
    ) {
        self.clip_rsuperellipse_operation(rse, clip, crate::clip_chain::ClipOp::Intersect);
    }

    pub(crate) fn clip_rsuperellipse_operation(
        &mut self,
        rse: flui_foundation::geometry::RSuperellipse,
        clip: flui_painting::paint::Clip,
        op: crate::clip_chain::ClipOp,
    ) {
        if self.current_segment.recording_result().is_err() {
            return;
        }
        if matches!(clip, flui_painting::paint::Clip::None) {
            return;
        }
        let hard = matches!(clip, flui_painting::paint::Clip::HardEdge);
        if !self.append_validated_clip(
            crate::clip_geometry::ValidatedClip::rsuperellipse(rse),
            op,
            hard,
        ) {
            return;
        }
        if op == crate::clip_chain::ClipOp::Intersect {
            self.state.clip_rsuperellipse(rse, self.size, hard);
        }
    }

    /// Intersect with a captured path using its fill rule and hard-edge coverage.
    pub fn clip_path(&mut self, path: &Path) {
        self.clip_path_operation(
            path,
            flui_painting::paint::Clip::HardEdge,
            crate::clip_chain::ClipOp::Intersect,
        );
    }

    pub(crate) fn clip_path_operation(
        &mut self,
        path: &Path,
        clip: flui_painting::paint::Clip,
        op: crate::clip_chain::ClipOp,
    ) {
        if self.current_segment.recording_result().is_err() {
            return;
        }
        if matches!(clip, flui_painting::paint::Clip::None) {
            return;
        }
        let hard = matches!(clip, flui_painting::paint::Clip::HardEdge);
        if !self.append_validated_clip(
            crate::clip_geometry::ValidatedClip::path(path, crate::clip_chain::MAX_PATH_COMMANDS),
            op,
            hard,
        ) {
            return;
        }
        if op == crate::clip_chain::ClipOp::Intersect {
            self.state
                .clip_rect_enclosing(path.compute_bounds(), self.size);
        }
    }
}
