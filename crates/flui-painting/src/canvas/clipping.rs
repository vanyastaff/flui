//! Canvas clipping operations: clip_rect, clip_rrect, clip_path
//! variants + bounds query helpers (local_clip_bounds,
//! device_clip_bounds, would_be_clipped).
//!
//! Mythos chain U4 extracted these from the 3,305-LOC `canvas.rs`
//! god module. Each clip method pushes a `ClipShape` onto the clip
//! stack AND emits a `DrawCommand::Clip*` command; the engine
//! consumes the command for GPU-side clipping, the stack entry lets
//! `restore()` truncate back to the right depth.

use flui_types::{
    geometry::{Pixels, RRect, Rect},
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
