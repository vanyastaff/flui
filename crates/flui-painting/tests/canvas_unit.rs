//! Canvas unit tests extracted from `crates/flui-painting/src/canvas/mod.rs`
//! during Mythos chain U8.
//!
//! These tests live as integration tests so the new submodule split
//! (`canvas/{mod,state,transform,clipping,drawing,scoped,composition,sugar}.rs`)
//! does not carry inline `#[cfg(test)] mod tests` blocks for surface
//! that is already exercised through the public API.

use flui_painting::{Canvas, DisplayListCore, Paint};
use flui_types::{
    geometry::{Point, Rect, px},
    styling::Color,
};

#[test]
fn test_canvas_creation() {
    let canvas = Canvas::new();
    assert_eq!(canvas.save_count(), 1);
    assert_eq!(canvas.display_list().len(), 0);
}

#[test]
fn test_canvas_draw_rect() {
    let mut canvas = Canvas::new();
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);

    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_canvas_save_restore() {
    let mut canvas = Canvas::new();

    assert_eq!(canvas.save_count(), 1);

    canvas.save();
    assert_eq!(canvas.save_count(), 2);

    canvas.translate(50.0, 50.0);

    canvas.save();
    assert_eq!(canvas.save_count(), 3);

    canvas.restore();
    assert_eq!(canvas.save_count(), 2);

    canvas.restore();
    assert_eq!(canvas.save_count(), 1);
}

#[test]
fn test_canvas_transform() {
    let mut canvas = Canvas::new();

    let original_transform = canvas.transform_matrix();
    canvas.translate(100.0, 50.0);
    let translated_transform = canvas.transform_matrix();

    assert_ne!(original_transform, translated_transform);
}

#[test]
fn test_canvas_clip() {
    let mut canvas = Canvas::new();
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));

    canvas.clip_rect(rect);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_canvas_multiple_commands() {
    let mut canvas = Canvas::new();
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);

    canvas.draw_rect(rect, &paint);
    canvas.draw_circle(Point::new(px(50.0), px(50.0)), px(25.0), &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 2);
}

#[test]
fn test_canvas_restore_without_save() {
    // Test that restore() without matching save() is safe (no-op)
    let mut canvas = Canvas::new();
    canvas.restore();

    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(
        Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
        &paint,
    );
    assert_eq!(canvas.len(), 1);
}

/// Mythos chain U10 wired a `debug_assert!` in `Canvas::finish` to
/// catch unrestored `save()` calls during test runs. Release builds
/// preserve Flutter parity (silent finalisation via `tracing::warn!`).
///
/// This test fires only when `debug_assertions` is on (cargo test
/// default). Release-build `cargo test --release` would skip the
/// panic-expectation, matching the documented per-mode behavior.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "unrestored save() calls")]
fn test_canvas_finish_panics_in_debug_on_unrestored_save() {
    let mut canvas = Canvas::new();
    canvas.save();
    canvas.translate(50.0, 50.0);
    // No matching restore() -- save_stack has 1 entry at finish() time.
    let _ = canvas.finish();
}

/// A balanced save/restore pair must not trip the imbalance assert.
#[test]
fn test_canvas_finish_clean_after_balanced_save_restore() {
    let mut canvas = Canvas::new();
    canvas.save();
    canvas.translate(50.0, 50.0);
    canvas.restore();
    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 0);
}

/// `Canvas::reset()` must clear commands, transform, clip stack, and
/// save stack back to a fresh-canvas state.
#[test]
fn test_canvas_reset_returns_to_fresh_state() {
    let mut canvas = Canvas::new();
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0));
    canvas.draw_rect(rect, &Paint::fill(Color::RED));
    canvas.save();
    canvas.translate(50.0, 50.0);
    canvas.save();
    assert!(!canvas.is_empty());
    assert_eq!(canvas.save_count(), 3);

    canvas.reset();

    assert!(canvas.is_empty());
    // After reset, save_count is the implicit 1 of a fresh canvas.
    assert_eq!(canvas.save_count(), 1);
}

/// `Canvas::clear_commands()` must drop recorded commands but preserve
/// the save stack and current transform.
#[test]
fn test_canvas_clear_commands_preserves_state() {
    let mut canvas = Canvas::new();
    canvas.save();
    canvas.translate(25.0, 25.0);
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0));
    canvas.draw_rect(rect, &Paint::fill(Color::BLUE));
    assert_eq!(canvas.display_list().len(), 1);
    let before_count = canvas.save_count();

    canvas.clear_commands();

    assert_eq!(canvas.display_list().len(), 0);
    // State (save stack, transform) survives `clear_commands`.
    assert_eq!(canvas.save_count(), before_count);

    // Pop the save we still have outstanding so finish() does not
    // trip the unrestored-save debug_assert.
    canvas.restore();
    let dl = canvas.finish();
    assert_eq!(dl.len(), 0);
}

/// `Canvas::add_hit_region` must route through to the underlying
/// `DisplayList` so the region surfaces in `hit_regions()` after
/// finish.
#[test]
fn test_canvas_add_hit_region_round_trips_through_finish() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_painting::{HitRegion, HitRegionHandler};

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let handler: HitRegionHandler = Arc::new(move |_evt| {
        counter_clone.fetch_add(1, Ordering::Relaxed);
    });

    let mut canvas = Canvas::new();
    let bounds = Rect::from_ltrb(px(10.0), px(10.0), px(50.0), px(50.0));
    canvas.add_hit_region(HitRegion::new(bounds, handler));

    let dl = canvas.finish();

    assert_eq!(dl.hit_regions().len(), 1);
    assert!(
        dl.hit_regions()[0].contains(Point::new(px(20.0), px(20.0))),
        "hit region bounds must round-trip through finish()"
    );
    // The handler is the same `Arc<dyn Fn>` we registered; invoke it
    // to confirm it survived the round trip.
    (dl.hit_regions()[0].handler)(&flui_painting::display_list::PointerEvent::new(
        flui_painting::display_list::PointerEventKind::Down,
        flui_types::geometry::Offset::new(px(20.0), px(20.0)),
        0,
    ));
    assert_eq!(counter.load(Ordering::Relaxed), 1);
}
