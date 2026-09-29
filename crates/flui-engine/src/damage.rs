//! Damage accumulation for incremental rendering, and the plan a frame is
//! rendered by.
//!
//! [`DamageTracker`](crate::damage::DamageTracker) is the renderer's per-frame accumulator of dirty area:
//! the consuming half of ADR-0061 (the scissor + self-heal in
//! `Renderer::render_scene`). The producer is `flui_layer::LayerDiffer`
//! (ADR-0087 §3); its `flui_layer::DamageRegion` reaches the tracker through
//! `RasterOwner::pump`, which marks every frame it retires — presented or not
//! — so debt a frame did not present stays here until one does.
//!
//! The accumulator keeps one bounding rectangle, because the one consumer
//! ([`damage_rect`](crate::damage::DamageTracker::damage_rect)) reads exactly that. A multi-rect scissor
//! (Slint keeps up to three) is a change to the consumer, and the tracker
//! grows with it.
//!
//! [`plan_frame`](crate::damage::plan_frame) turns the tracker's answer into where a frame renders.
//! wgpu does not expose a swapchain image's age, so the pixels outside a
//! scissor on a freshly acquired swapchain image are whatever an older frame
//! left there; a partial frame therefore renders into a
//! [`RetainedTarget`](crate::retained_target::RetainedTarget) that holds the
//! last frame and blits the result to the swapchain.

use flui_foundation::geometry::Rect;

/// Accumulates the area that changed since the last frame.
///
/// A fresh tracker needs a full repaint (nothing has been rendered yet);
/// [`DamageTracker::reset`] starts the next frame clean.
#[derive(Debug, Clone)]
pub(crate) struct DamageTracker {
    bounds: Option<Rect<f64>>,
    full_repaint: bool,
}

impl DamageTracker {
    /// A tracker that needs a full repaint.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            bounds: None,
            full_repaint: true,
        }
    }

    /// Adds `rect` to the damaged area. A zero-sized rect is a no-op.
    pub(crate) fn mark_dirty(&mut self, rect: Rect<f64>) {
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        self.bounds = Some(match self.bounds {
            Some(bounds) => bounds.union(&rect),
            None => rect,
        });
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

    /// The scissor for a partial repaint: `None` when the whole frame
    /// repaints (or nothing is dirty), else the union of every marked rect.
    #[must_use]
    pub(crate) fn damage_rect(&self) -> Option<Rect<f64>> {
        if self.full_repaint {
            return None;
        }
        self.bounds
    }

    /// Whether anything needs painting.
    #[must_use]
    pub(crate) fn has_damage(&self) -> bool {
        self.full_repaint || self.bounds.is_some()
    }

    /// Starts a new frame with nothing dirty.
    pub(crate) fn reset(&mut self) {
        self.bounds = None;
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
    /// Only this rectangle, scissored, into a valid retained target, then a
    /// blit of the whole target to the swapchain.
    RetainedPartial(Rect<f64>),
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
    match tracker.damage_rect() {
        Some(damage) if retained_valid => FramePlan::RetainedPartial(damage),
        Some(_) => FramePlan::RetainedFull,
        None if intermediate_required => FramePlan::RetainedFull,
        None => FramePlan::Direct,
    }
}

/// Opens a partial frame on `painter`: scissors every later draw to the
/// whole pixels `damage` touches, and repaints them the clear colour.
///
/// The clear has to be a draw inside the scissor rather than the full-frame
/// clear pass, which would wipe the retained pixels outside the damage; and
/// it has to come first, because the content inside the damage blends over
/// the background, not over what the previous frame left there. An opaque
/// fill under `SrcOver` overwrites exactly as `Src` would, without taking the
/// coverage-destructive blend path; the rect reaches one pixel past the
/// scissor on every side, so every scissored pixel is fully covered.
pub(crate) fn begin_partial(painter: &mut crate::painter::WgpuPainter, damage: Rect<f64>) {
    painter.clip_rect_enclosing(damage);
    let covering = Rect::from_ltrb(
        damage.left().floor() - 1.0,
        damage.top().floor() - 1.0,
        damage.right().ceil() + 1.0,
        damage.bottom().ceil() + 1.0,
    );
    painter.draw_rect(
        covering,
        &flui_painting::Paint::fill(flui_painting::styling::Color::WHITE),
    );
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn plan_frame_table() {
        let partial = Rect::from_ltrb(10.0, 10.0, 20.0, 20.0);
        let clean = {
            let mut tracker = DamageTracker::new();
            tracker.reset();
            tracker
        };
        let full = DamageTracker::new();
        let dirty = {
            let mut tracker = clean.clone();
            tracker.mark_dirty(partial);
            tracker
        };
        // (tracker, retained valid, intermediate required) -> plan
        let cases = [
            (&clean, false, false, FramePlan::Skip),
            (&clean, true, true, FramePlan::Skip),
            (&full, false, false, FramePlan::Direct),
            (&full, true, false, FramePlan::Direct),
            (&full, false, true, FramePlan::RetainedFull),
            (&full, true, true, FramePlan::RetainedFull),
            (&dirty, false, false, FramePlan::RetainedFull),
            (&dirty, false, true, FramePlan::RetainedFull),
            (&dirty, true, false, FramePlan::RetainedPartial(partial)),
            (&dirty, true, true, FramePlan::RetainedPartial(partial)),
        ];
        for (tracker, valid, intermediate, expected) in cases {
            assert_eq!(
                plan_frame(tracker, valid, intermediate),
                expected,
                "full={} dirty={:?} valid={valid} intermediate={intermediate}",
                tracker.needs_full_repaint(),
                tracker.damage_rect(),
            );
        }
    }

    fn rect(l: f32, t: f32, r: f32, b: f32) -> Rect<f64> {
        Rect::from_ltrb(f64::from(l), f64::from(t), f64::from(r), f64::from(b))
    }

    #[test]
    fn a_fresh_tracker_needs_a_full_repaint() {
        let tracker = DamageTracker::new();
        assert!(tracker.needs_full_repaint());
        assert!(tracker.has_damage());
        assert_eq!(tracker.damage_rect(), None);
    }

    #[test]
    fn reset_starts_a_clean_frame() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        assert!(!tracker.needs_full_repaint());
        assert!(!tracker.has_damage());
        assert_eq!(tracker.damage_rect(), None);
    }

    #[test]
    fn marked_rects_union_into_one_scissor() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_dirty(rect(0.0, 0.0, 10.0, 10.0));
        assert_eq!(tracker.damage_rect(), Some(rect(0.0, 0.0, 10.0, 10.0)));
        tracker.mark_dirty(rect(50.0, 50.0, 60.0, 60.0));
        assert!(tracker.has_damage());
        assert_eq!(tracker.damage_rect(), Some(rect(0.0, 0.0, 60.0, 60.0)));
    }

    #[test]
    fn a_zero_sized_rect_marks_nothing() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_dirty(rect(10.0, 10.0, 10.0, 20.0));
        tracker.mark_dirty(rect(10.0, 10.0, 20.0, 10.0));
        assert!(!tracker.has_damage());
    }

    #[test]
    fn full_repaint_wins_over_rects_until_reset() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_dirty(rect(0.0, 0.0, 10.0, 10.0));
        tracker.mark_full_repaint();
        assert!(tracker.needs_full_repaint());
        assert!(tracker.has_damage());
        assert_eq!(tracker.damage_rect(), None, "no scissor on a full repaint");
        tracker.reset();
        assert!(!tracker.needs_full_repaint());
        assert_eq!(tracker.damage_rect(), None);
    }
}
