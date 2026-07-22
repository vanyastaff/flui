//! Canvas clipping operations: clip_rect, clip_rrect, clip_path
//! variants + bounds query helpers (local_clip_bounds,
//! device_clip_bounds, would_be_clipped).
//!
//! These were extracted from the 3,305-LOC `canvas.rs`
//! god module. Each clip method pushes a `ClipShape` onto the clip
//! stack AND emits a `DrawCommand::Clip*` command; the engine
//! consumes the command for GPU-side clipping, the stack entry lets
//! `restore()` truncate back to the right depth.

use flui_types::{
    geometry::{Pixels, RRect, RSuperellipse, Rect},
    painting::{Clip, ClipOp, Path},
};

use super::{Canvas, ClipShape};
use crate::display_list::DrawCommand;

impl Canvas {
    // ===== Clipping =====

    /// Clips to a rectangle.
    ///
    /// All subsequent drawing will be clipped to this rectangle.
    /// Uses default clip behavior (intersect, anti-aliased).
    pub fn clip_rect(&mut self, rect: Rect<Pixels>) {
        self.clip_stack.push(ClipShape::Rect(rect));
        self.display_list.push(DrawCommand::ClipRect {
            rect,
            clip_op: ClipOp::default(),
            clip_behavior: Clip::default(),
            transform: self.transform,
        });
    }

    /// Clips to a rounded rectangle.
    ///
    /// Uses default clip behavior (intersect, anti-aliased).
    pub fn clip_rrect(&mut self, rrect: RRect) {
        self.clip_stack.push(ClipShape::RRect(rrect));
        self.display_list.push(DrawCommand::ClipRRect {
            rrect,
            clip_op: ClipOp::default(),
            clip_behavior: Clip::default(),
            transform: self.transform,
        });
    }

    /// Clips to a rounded superellipse (Flutter `RSuperellipse`).
    ///
    /// Uses default clip behavior (intersect, anti-aliased). Records a
    /// `DrawCommand::ClipRSuperellipse` carrying the rounded-superellipse
    /// (iOS-squircle) intent. Exact corner-curve rendering is
    /// backend-dependent: the `CommandRenderer::clip_rsuperellipse` default
    /// approximates via `clip_rrect` against the outer rect plus per-corner
    /// radii, while a backend may override with a real superellipse SDF for
    /// pixel-perfect parity.
    pub fn clip_rsuperellipse(&mut self, rsuperellipse: RSuperellipse) {
        self.clip_stack
            .push(ClipShape::RSuperellipse(rsuperellipse));
        self.display_list.push(DrawCommand::ClipRSuperellipse {
            rsuperellipse,
            clip_op: ClipOp::default(),
            clip_behavior: Clip::default(),
            transform: self.transform,
        });
    }

    /// Clips to an arbitrary path.
    ///
    /// Uses default clip behavior (intersect, anti-aliased).
    pub fn clip_path(&mut self, path: &Path) {
        self.clip_stack
            .push(ClipShape::Path(Box::new((*path).clone())));
        self.display_list.push(DrawCommand::ClipPath {
            path: (*path).clone(),
            clip_op: ClipOp::default(),
            clip_behavior: Clip::default(),
            transform: self.transform,
        });
    }

    /// Clips to a rectangle with explicit options.
    ///
    /// Supports clip operations (intersect/difference) and
    /// anti-aliasing.
    pub fn clip_rect_ext(&mut self, rect: Rect<Pixels>, clip_op: ClipOp, clip_behavior: Clip) {
        self.clip_stack.push(ClipShape::Rect(rect));
        self.display_list.push(DrawCommand::ClipRect {
            rect,
            clip_op,
            clip_behavior,
            transform: self.transform,
        });
    }

    /// Clips to a rounded rectangle with explicit options.
    pub fn clip_rrect_ext(&mut self, rrect: RRect, clip_op: ClipOp, clip_behavior: Clip) {
        self.clip_stack.push(ClipShape::RRect(rrect));
        self.display_list.push(DrawCommand::ClipRRect {
            rrect,
            clip_op,
            clip_behavior,
            transform: self.transform,
        });
    }

    /// Clips to a rounded superellipse with explicit options.
    pub fn clip_rsuperellipse_ext(
        &mut self,
        rsuperellipse: RSuperellipse,
        clip_op: ClipOp,
        clip_behavior: Clip,
    ) {
        self.clip_stack
            .push(ClipShape::RSuperellipse(rsuperellipse));
        self.display_list.push(DrawCommand::ClipRSuperellipse {
            rsuperellipse,
            clip_op,
            clip_behavior,
            transform: self.transform,
        });
    }

    /// Clips to a path with explicit options.
    pub fn clip_path_ext(&mut self, path: &Path, clip_op: ClipOp, clip_behavior: Clip) {
        self.clip_stack
            .push(ClipShape::Path(Box::new((*path).clone())));
        self.display_list.push(DrawCommand::ClipPath {
            path: (*path).clone(),
            clip_op,
            clip_behavior,
            transform: self.transform,
        });
    }

    // ===== Clip Query Methods =====

    /// Returns the local-space bounds of the current clip, if
    /// available.
    ///
    /// Returns the bounds of the most recent clip operation, without
    /// applying transformations. Returns `None` if:
    /// - No clip is active (clip stack is empty).
    /// - The current clip is a Path (bounds require mutable access).
    #[inline]
    #[must_use]
    pub fn local_clip_bounds(&self) -> Option<Rect<Pixels>> {
        self.clip_stack.last().and_then(|clip| match clip {
            ClipShape::Rect(rect) => Some(*rect),
            ClipShape::RRect(rrect) => Some(rrect.bounding_rect()),
            ClipShape::RSuperellipse(rse) => Some(rse.outer_rect()),
            ClipShape::Path(_) => None,
        })
    }

    /// Returns the device-space (transformed) bounds of the current
    /// clip, if available.
    ///
    /// Applies the current transformation matrix to the clip bounds.
    #[inline]
    #[must_use]
    pub fn device_clip_bounds(&self) -> Option<Rect<Pixels>> {
        self.local_clip_bounds()
            .map(|local_bounds| self.transform.transform_rect(&local_bounds))
    }

    /// Checks if the given rectangle is completely outside the current
    /// clip bounds.
    ///
    /// Use this for culling optimizations.
    ///
    /// # Returns
    ///
    /// - `Some(true)` -- the rect is definitely outside the clip
    ///   (can skip drawing).
    /// - `Some(false)` -- the rect may be visible (should draw).
    /// - `None` -- cannot determine (no clip active or clip is a
    ///   Path).
    #[inline]
    #[must_use]
    pub fn would_be_clipped(&self, rect: &Rect<Pixels>) -> Option<bool> {
        self.local_clip_bounds()
            .map(|clip_bounds| !clip_bounds.intersects(rect))
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test code: expect/panic IS the assertion path"
)]
mod tests {
    use flui_types::geometry::{Radius, px};

    use super::*;

    fn make_rse() -> RSuperellipse {
        RSuperellipse::from_rect_and_radius(
            Rect::from_ltwh(px(10.0), px(20.0), px(100.0), px(50.0)),
            Radius::circular(px(8.0)),
        )
    }

    #[test]
    fn clip_rsuperellipse_records_command() {
        let mut canvas = Canvas::new();
        let rse = make_rse();
        canvas.clip_rsuperellipse(rse);

        assert_eq!(canvas.len(), 1);
        // Smoke: the most recent command is a ClipRSuperellipse with default options.
        let last = canvas
            .display_list()
            .iter()
            .next_back()
            .expect("command recorded");
        match last {
            DrawCommand::ClipRSuperellipse {
                rsuperellipse: recorded,
                clip_op,
                clip_behavior,
                ..
            } => {
                assert_eq!(recorded, &rse);
                assert_eq!(clip_op, &ClipOp::default());
                assert_eq!(clip_behavior, &Clip::default());
            }
            other => panic!("expected ClipRSuperellipse, got {other:?}"),
        }
    }

    #[test]
    fn clip_rsuperellipse_ext_respects_clip_op_and_behavior() {
        let mut canvas = Canvas::new();
        let rse = make_rse();
        canvas.clip_rsuperellipse_ext(rse, ClipOp::Difference, Clip::HardEdge);

        let last = canvas
            .display_list()
            .iter()
            .next_back()
            .expect("command recorded");
        match last {
            DrawCommand::ClipRSuperellipse {
                clip_op,
                clip_behavior,
                ..
            } => {
                assert_eq!(clip_op, &ClipOp::Difference);
                assert_eq!(clip_behavior, &Clip::HardEdge);
            }
            other => panic!("expected ClipRSuperellipse, got {other:?}"),
        }
    }

    #[test]
    fn clip_rsuperellipse_updates_local_clip_bounds() {
        let mut canvas = Canvas::new();
        let rse = make_rse();
        let expected = rse.outer_rect();

        assert_eq!(canvas.local_clip_bounds(), None);
        canvas.clip_rsuperellipse(rse);
        assert_eq!(canvas.local_clip_bounds(), Some(expected));
    }

    #[test]
    fn clip_rsuperellipse_save_restore_returns_to_prior_bounds() {
        let mut canvas = Canvas::new();
        let outer = Rect::from_ltwh(px(0.0), px(0.0), px(200.0), px(200.0));
        canvas.clip_rect(outer);

        canvas.save();
        let rse = make_rse();
        canvas.clip_rsuperellipse(rse);
        assert_eq!(canvas.local_clip_bounds(), Some(rse.outer_rect()));

        canvas.restore();
        // Restored to the outer rect clip established before the save.
        assert_eq!(canvas.local_clip_bounds(), Some(outer));
    }
}
