//! Damage accumulation for incremental rendering, and the plan a frame is
//! rendered by.
//!
//! [`crate::damage::DamageTracker`] is the renderer's per-frame accumulator of dirty area:
//! the consuming half of ADR-0061 (the scissor + self-heal in
//! `Renderer::render_scene`). The producer is the retained scene: each
//! publication carries the footprint of its patches since the version on screen,
//! held to that base by `flui_layer::RasterBase` (ADR-0304 §5, §7), and reaches the tracker through
//! the raster executor's slot when the frame renders. A frame that is hidden or
//! stale never reaches the tracker: its slot owes the next frame a full repaint instead.
//!
//! The accumulator keeps a [`Region`](flui_layer::Region): the rects of every publication owed,
//! up to [`Region::MAX`](flui_layer::Region::MAX) of them, a further one merging the cheapest pair as
//! the region itself does. [`crate::damage::plan_frame`] turns it into where a frame renders;
//! a partial frame redraws its region in one pass per rect, each under its own scissor and culled
//! by its own rect. The frame protocol has already folded the rects into their bounding box when
//! `region_cost` predicted the box cheaper (ADR-0304 §9).
//!
//! wgpu does not expose a swapchain image's age, so the pixels outside a
//! scissor on a freshly acquired swapchain image are whatever an older frame
//! left there; a partial frame therefore renders into a
//! [`RetainedTarget`](crate::retained_target::RetainedTarget) that holds the
//! last frame and blits the result to the swapchain.

use flui_foundation::geometry::Rect;
use flui_layer::{DamageRect, Region};

/// Accumulates the area that changed since the last frame.
///
/// A fresh tracker needs a full repaint (nothing has been rendered yet);
/// [`DamageTracker::reset`] starts the next frame clean.
#[derive(Debug, Clone)]
pub(crate) struct DamageTracker {
    region: Option<Region>,
    full_repaint: bool,
}

impl DamageTracker {
    /// A tracker that needs a full repaint.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            region: None,
            full_repaint: true,
        }
    }

    /// Adds every rect of `region` to the damaged area.
    pub(crate) fn mark_region(&mut self, region: &Region) {
        for &rect in region.rects() {
            self.mark_rect(rect);
        }
    }

    /// Adds `rect` to the damaged area.
    pub(crate) fn mark_rect(&mut self, rect: DamageRect) {
        match self.region.as_mut() {
            Some(region) => region.insert(rect),
            None => self.region = Some(Region::new(rect)),
        }
    }

    /// Adds the whole pixels `rect` touches on a `surface` of
    /// `(width, height)`. An empty rect, or one off the surface, is a no-op;
    /// a rect that is not finite owes a full repaint.
    pub(crate) fn mark_dirty(&mut self, rect: Rect<f64>, surface: (u32, u32)) {
        match DamageRect::try_enclosing(rect, surface) {
            Ok(Some(rect)) => self.mark_rect(rect),
            Ok(None) => {}
            Err(flui_layer::NonFinite) => self.mark_full_repaint(),
        }
    }

    /// Marks the whole frame dirty; wins over any rect.
    pub(crate) fn mark_full_repaint(&mut self) {
        self.full_repaint = true;
    }

    /// Whether the whole frame must repaint. Production reads this through
    /// [`plan_frame`], which folds it into the plan.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn needs_full_repaint(&self) -> bool {
        self.full_repaint
    }

    /// The region a partial repaint redraws: `None` when the whole frame
    /// repaints (or nothing is dirty), else every marked rect.
    #[must_use]
    pub(crate) fn region(&self) -> Option<Region> {
        if self.full_repaint {
            return None;
        }
        self.region
    }

    /// Whether anything needs painting.
    #[must_use]
    pub(crate) fn has_damage(&self) -> bool {
        self.full_repaint || self.region.is_some()
    }

    /// Starts a new frame with nothing dirty.
    pub(crate) fn reset(&mut self) {
        self.region = None;
        self.full_repaint = false;
    }
}

/// Where and how much of a frame is rendered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FramePlan {
    /// Nothing is owed: no damage since the last presented frame.
    Skip,
    /// Everything, straight into the swapchain image. The retained target
    /// does not see this frame, so it stops being valid.
    Direct,
    /// Everything, into the retained target, then a blit to the swapchain.
    /// Taken when the frame needs an intermediate anyway (a surface without
    /// `COPY_SRC`), where it costs nothing extra, and to warm the target up
    /// for a partial frame that found it invalid.
    RetainedFull,
    /// Only the region, into a valid retained target, then a blit of the
    /// whole target to the swapchain (ADR-0304 §9).
    ///
    /// The region is redrawn rect by rect, at most [`Region::MAX`] passes:
    /// each pass clears its rect to the background and redraws, under that
    /// rect's scissor, everything a walk culled by that rect reaches. No
    /// pixel outside the rects is written, and every chunk that meets a
    /// written pixel is drawn there once, in paint order. The region is the
    /// damage the frame closed over its backdrop readers, already folded into
    /// its bounding box where that was predicted cheaper
    /// (`FrameProtocol::include_backdrop_dependencies`, ADR-0304 §9).
    RetainedPartial(Region),
}

/// The plan for the next frame, from the damage owed, whether the retained
/// target holds the last presented frame, and whether the surface needs an
/// intermediate target at all.
///
/// A partial frame is never rendered straight into the swapchain: without a
/// buffer age, the pixels outside its scissor would come from an arbitrary
/// older frame.
pub(crate) fn plan_frame(
    tracker: &DamageTracker,
    retained_valid: bool,
    intermediate_required: bool,
) -> FramePlan {
    if !tracker.has_damage() {
        return FramePlan::Skip;
    }
    match tracker.region() {
        Some(region) if retained_valid => FramePlan::RetainedPartial(region),
        Some(_) => FramePlan::RetainedFull,
        None if intermediate_required => FramePlan::RetainedFull,
        None => FramePlan::Direct,
    }
}

/// Opens one pass of a partial frame on `painter`: scissors every later draw
/// to `rect`, and repaints it the frame's
/// [`BACKGROUND`](crate::frame_protocol::BACKGROUND), the colour the full
/// clear pass uses. Returns the pixels the pass redraws, which a walk of it
/// must not skip.
///
/// The clear has to be a draw inside the scissor rather than the full-frame
/// clear pass, which would wipe the retained pixels outside the damage; and
/// it has to come first, because the content inside the damage blends over
/// the background, not over what the previous frame left there. An opaque
/// fill under `SrcOver` overwrites exactly as `Src` would, without taking the
/// coverage-destructive blend path; the rect reaches one pixel past the
/// scissor on every side, so every scissored pixel is fully covered.
#[must_use = "a walk of the frame culls by the pixels it redraws"]
pub(crate) fn begin_partial(
    painter: &mut crate::painter::WgpuPainter,
    rect: DamageRect,
) -> Rect<f64> {
    let redrawn = rect.to_rect();
    painter.clip_rect_enclosing(redrawn);
    let covering = Rect::from_ltrb(
        redrawn.left() - 1.0,
        redrawn.top() - 1.0,
        redrawn.right() + 1.0,
        redrawn.bottom() + 1.0,
    );
    painter.draw_rect(
        covering,
        &flui_painting::paint::Paint::fill(crate::frame_protocol::BACKGROUND),
    );
    redrawn
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACE: (u32, u32) = (200, 200);

    fn rect(l: f64, t: f64, r: f64, b: f64) -> DamageRect {
        DamageRect::try_enclosing(Rect::from_ltrb(l, t, r, b), SURFACE)
            .expect("finite")
            .expect("on the surface")
    }

    fn region(rects: &[DamageRect]) -> Region {
        let (first, rest) = rects.split_first().expect("a region has a rect");
        let mut region = Region::new(*first);
        for &rect in rest {
            region.insert(rect);
        }
        region
    }

    fn tracker_of(rects: &[DamageRect]) -> DamageTracker {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_region(&region(rects));
        tracker
    }

    /// The plan from the damage owed and the retained target: a partial
    /// frame redraws its region rect by rect, a region of distant rects as
    /// those rects and not as their bounding box.
    #[test]
    fn plan_frame_table() {
        let clean = {
            let mut tracker = DamageTracker::new();
            tracker.reset();
            tracker
        };
        let full = DamageTracker::new();
        let dirty = {
            let mut tracker = clean.clone();
            tracker.mark_dirty(Rect::from_ltrb(10.0, 10.0, 20.0, 20.0), SURFACE);
            tracker
        };
        let partial = |rects: &[DamageRect]| FramePlan::RetainedPartial(region(rects));
        let dirty_rect = partial(&[rect(10.0, 10.0, 20.0, 20.0)]);
        let one = rect(10.0, 10.0, 30.0, 30.0);
        let far = rect(170.0, 170.0, 190.0, 190.0);
        // Beside `one` with a 2-pixel gap.
        let adjacent = rect(32.0, 10.0, 52.0, 30.0);

        // (case, tracker, retained valid, intermediate required) -> plan
        let cases = [
            ("clean", &clean, false, false, FramePlan::Skip),
            ("clean, valid", &clean, true, true, FramePlan::Skip),
            ("full", &full, false, false, FramePlan::Direct),
            ("full, valid", &full, true, false, FramePlan::Direct),
            (
                "full, intermediate",
                &full,
                false,
                true,
                FramePlan::RetainedFull,
            ),
            ("full, both", &full, true, true, FramePlan::RetainedFull),
            (
                "dirty, invalid",
                &dirty,
                false,
                false,
                FramePlan::RetainedFull,
            ),
            (
                "dirty, invalid, intermediate",
                &dirty,
                false,
                true,
                FramePlan::RetainedFull,
            ),
            ("dirty", &dirty, true, false, dirty_rect),
            ("dirty, intermediate", &dirty, true, true, dirty_rect),
            (
                "one rect",
                &tracker_of(&[one]),
                true,
                false,
                partial(&[one]),
            ),
            (
                "two distant rects",
                &tracker_of(&[one, far]),
                true,
                false,
                partial(&[one, far]),
            ),
            (
                "two adjacent rects",
                &tracker_of(&[one, adjacent]),
                true,
                false,
                partial(&[one, adjacent]),
            ),
            (
                "two distant rects, invalid",
                &tracker_of(&[one, far]),
                false,
                false,
                FramePlan::RetainedFull,
            ),
        ];
        for (case, tracker, valid, intermediate, expected) in cases {
            assert_eq!(
                plan_frame(tracker, valid, intermediate),
                expected,
                "{case}: full={} region={:?} valid={valid} intermediate={intermediate}",
                tracker.needs_full_repaint(),
                tracker.region(),
            );
        }
    }

    /// The tracker keeps each rect apart, merging overlapping ones as the
    /// region does, and a rect that is not finite owes a full repaint.
    #[test]
    fn the_tracker_keeps_the_rects_apart() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_dirty(Rect::from_ltrb(10.0, 10.0, 30.0, 30.0), SURFACE);
        tracker.mark_region(&region(&[rect(170.0, 170.0, 190.0, 190.0)]));
        tracker.mark_dirty(Rect::from_ltrb(20.5, 20.5, 40.0, 40.0), SURFACE);
        let held = tracker.region().expect("partial damage");
        assert_eq!(
            held.rects(),
            &[
                rect(170.0, 170.0, 190.0, 190.0),
                rect(10.0, 10.0, 40.0, 40.0)
            ],
            "the overlapping rects merged into their box, the distant one stayed apart"
        );
        tracker.mark_dirty(Rect::from_ltrb(300.0, 300.0, 310.0, 310.0), SURFACE);
        assert_eq!(tracker.region(), Some(held), "a rect off the surface");
        tracker.mark_dirty(Rect::from_ltrb(0.0, 0.0, f64::INFINITY, 10.0), SURFACE);
        assert!(tracker.needs_full_repaint(), "a rect that is not finite");
        assert_eq!(tracker.region(), None);
    }
}
