//! Canvas clipping: `clip_rect` / `clip_rrect` / `clip_rsuperellipse` /
//! `clip_path`, each with an `_ext` form taking the `ClipOp` and `Clip`
//! that `dart:ui` exposes as optional arguments.
//!
//! Every enabled clip records one `DrawOp::Clip*`; the engine restores
//! its clip stack when it replays the matching `Restore` command.

use flui_types::{
    geometry::{Pixels, RRect, RSuperellipse, Rect},
    painting::{Clip, ClipOp, Path},
};

use super::Canvas;
use crate::display_list::DrawOp;

impl Canvas {
    /// Clips to a rectangle (intersect, anti-aliased).
    pub fn clip_rect(&mut self, rect: Rect<Pixels>) {
        self.clip_rect_ext(rect, ClipOp::default(), Clip::default());
    }

    /// Clips to a rounded rectangle (intersect, anti-aliased).
    pub fn clip_rrect(&mut self, rrect: RRect) {
        self.clip_rrect_ext(rrect, ClipOp::default(), Clip::default());
    }

    /// Clips to a rounded superellipse (intersect, anti-aliased).
    ///
    /// The command carries the squircle itself; the engine evaluates it as a
    /// signed distance field rather than approximating it with an rrect.
    pub fn clip_rsuperellipse(&mut self, rsuperellipse: RSuperellipse) {
        self.clip_rsuperellipse_ext(rsuperellipse, ClipOp::default(), Clip::default());
    }

    /// Clips to a path (intersect, anti-aliased).
    pub fn clip_path(&mut self, path: &Path) {
        self.clip_path_ext(path, ClipOp::default(), Clip::default());
    }

    /// Clips to a rectangle with an explicit operation and behaviour.
    pub fn clip_rect_ext(&mut self, rect: Rect<Pixels>, clip_op: ClipOp, clip_behavior: Clip) {
        self.push_clip(clip_behavior, || DrawOp::ClipRect {
            rect,
            clip_op,
            clip_behavior,
        });
    }

    /// Clips to a rounded rectangle with an explicit operation and behaviour.
    pub fn clip_rrect_ext(&mut self, rrect: RRect, clip_op: ClipOp, clip_behavior: Clip) {
        self.push_clip(clip_behavior, || DrawOp::ClipRRect {
            rrect,
            clip_op,
            clip_behavior,
        });
    }

    /// Clips to a rounded superellipse with an explicit operation and
    /// behaviour.
    pub fn clip_rsuperellipse_ext(
        &mut self,
        rsuperellipse: RSuperellipse,
        clip_op: ClipOp,
        clip_behavior: Clip,
    ) {
        self.push_clip(clip_behavior, || DrawOp::ClipRSuperellipse {
            rsuperellipse,
            clip_op,
            clip_behavior,
        });
    }

    /// Clips to a path with an explicit operation and behaviour.
    pub fn clip_path_ext(&mut self, path: &Path, clip_op: ClipOp, clip_behavior: Clip) {
        self.push_clip(clip_behavior, || DrawOp::ClipPath {
            path: path.clone(),
            clip_op,
            clip_behavior,
        });
    }

    /// `Clip::None` has no effect, so it contributes no command.
    fn push_clip(&mut self, clip_behavior: Clip, op: impl FnOnce() -> DrawOp) {
        if clip_behavior == Clip::None {
            return;
        }
        self.record(op());
    }
}

#[cfg(test)]
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
    fn a_clip_records_one_command() {
        let mut canvas = Canvas::new();
        canvas.clip_rsuperellipse(make_rse());
        assert_eq!(canvas.display_list().len(), 1);
        assert!(matches!(
            canvas.display_list()[0].op,
            DrawOp::ClipRSuperellipse { .. }
        ));
    }

    #[test]
    fn clip_none_records_nothing() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0));

        canvas.clip_rect_ext(rect, ClipOp::Intersect, Clip::None);
        assert!(canvas.display_list().is_empty());

        // The control: a real clip does record.
        canvas.clip_rect_ext(rect, ClipOp::Intersect, Clip::HardEdge);
        assert_eq!(canvas.display_list().len(), 1);
    }
}
