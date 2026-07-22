//! Damage-tracker integration tests: dirty-region accumulation and reset.

use flui_layer::damage::DamageTracker;
use flui_types::geometry::{Rect, px};

#[test]
fn test_new_tracker_needs_full_repaint() {
    let tracker = DamageTracker::new();
    assert!(tracker.needs_full_repaint());
    assert!(tracker.has_damage());
    assert_eq!(tracker.damage_rect(), None);
}

#[test]
fn test_reset_clears_full_repaint() {
    let mut tracker = DamageTracker::new();
    assert!(tracker.needs_full_repaint());

    tracker.reset();
    assert!(!tracker.needs_full_repaint());
    assert!(!tracker.has_damage());
}

#[test]
fn test_mark_dirty_adds_region() {
    let mut tracker = DamageTracker::new();
    tracker.reset();

    let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
    tracker.mark_dirty(rect);

    assert!(tracker.has_damage());
    assert_eq!(tracker.region_count(), 1);
    assert_eq!(tracker.damage_rect(), Some(rect));
}

#[test]
fn test_damage_rect_union() {
    let mut tracker = DamageTracker::new();
    tracker.reset();

    // Two overlapping rects
    let r1 = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
    let r2 = Rect::from_xywh(px(25.0), px(25.0), px(50.0), px(50.0));
    tracker.mark_dirty(r1);
    tracker.mark_dirty(r2);

    let damage = tracker.damage_rect().expect("should have damage rect");
    // Bounding box: (0,0) to (75,75)
    let expected = Rect::from_xywh(px(0.0), px(0.0), px(75.0), px(75.0));
    assert_eq!(damage, expected);
}

#[test]
fn test_no_damage_after_reset() {
    let mut tracker = DamageTracker::new();
    tracker.reset();

    assert!(!tracker.has_damage());
    assert_eq!(tracker.region_count(), 0);
    // No regions, no full repaint -> zero rect
    assert_eq!(tracker.damage_rect(), Some(Rect::ZERO));
}

#[test]
fn test_full_repaint_returns_none() {
    let tracker = DamageTracker::new();
    assert_eq!(tracker.damage_rect(), None);

    // Also after marking full repaint explicitly
    let mut tracker2 = DamageTracker::new();
    tracker2.reset();
    tracker2.mark_dirty(Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)));
    tracker2.mark_full_repaint();
    assert_eq!(tracker2.damage_rect(), None);
}

#[test]
fn test_mark_full_repaint() {
    let mut tracker = DamageTracker::new();
    tracker.reset();
    assert!(!tracker.needs_full_repaint());

    tracker.mark_full_repaint();
    assert!(tracker.needs_full_repaint());
    assert!(tracker.has_damage());
}

#[test]
fn test_region_count() {
    let mut tracker = DamageTracker::new();
    tracker.reset();
    assert_eq!(tracker.region_count(), 0);

    tracker.mark_dirty(Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)));
    assert_eq!(tracker.region_count(), 1);

    tracker.mark_dirty(Rect::from_xywh(px(20.0), px(20.0), px(30.0), px(30.0)));
    assert_eq!(tracker.region_count(), 2);

    tracker.mark_dirty(Rect::from_xywh(px(50.0), px(50.0), px(10.0), px(10.0)));
    assert_eq!(tracker.region_count(), 3);

    // Reset clears regions
    tracker.reset();
    assert_eq!(tracker.region_count(), 0);
}
