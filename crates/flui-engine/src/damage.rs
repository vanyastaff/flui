//! Damage accumulation for incremental rendering.
//!
//! [`DamageTracker`](crate::damage::DamageTracker) is the renderer's per-frame accumulator of dirty area:
//! the consuming half of ADR-0061 (the scissor + self-heal in
//! `Renderer::render_scene`). Its producer does not exist yet — every path
//! calls [`mark_full_repaint`](crate::damage::DamageTracker::mark_full_repaint) — which is why it lives here,
//! with the runtime state it gates, and not in the layer vocabulary crate.
//! The seam message a producer will send is `flui_layer::DamageRegion`.
//!
//! The accumulator keeps one bounding rectangle, because the one consumer
//! ([`damage_rect`](crate::damage::DamageTracker::damage_rect)) reads exactly that. A multi-rect scissor
//! (Slint keeps up to three) is a change to the consumer, and the tracker
//! grows with it.

use flui_types::geometry::{Pixels, Rect};

/// Accumulates the area that changed since the last frame.
///
/// A fresh tracker needs a full repaint (nothing has been rendered yet);
/// [`DamageTracker::reset`] starts the next frame clean.
#[derive(Debug, Clone)]
pub(crate) struct DamageTracker {
    bounds: Option<Rect<Pixels>>,
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
    pub(crate) fn mark_dirty(&mut self, rect: Rect<Pixels>) {
        if rect.width().0 <= 0.0 || rect.height().0 <= 0.0 {
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

    /// Whether the whole frame must repaint.
    #[must_use]
    pub(crate) fn needs_full_repaint(&self) -> bool {
        self.full_repaint
    }

    /// The scissor for a partial repaint: `None` when the whole frame
    /// repaints (or nothing is dirty), else the union of every marked rect.
    #[must_use]
    pub(crate) fn damage_rect(&self) -> Option<Rect<Pixels>> {
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

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    fn rect(l: f32, t: f32, r: f32, b: f32) -> Rect<Pixels> {
        Rect::from_ltrb(px(l), px(t), px(r), px(b))
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
