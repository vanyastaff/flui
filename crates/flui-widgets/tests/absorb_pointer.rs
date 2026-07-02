//! Tests for `AbsorbPointer` — a layout pass-through (mirroring
//! `tests/clip.rs`/`tests/decorated_box.rs`'s convention for paint/no-op
//! proxy widgets) whose real behavior is hit-test absorption: when
//! `absorbing = true` (the default), its subtree must never receive a hit,
//! not even a `GestureDetector` directly beneath it.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, size, tight};
use flui_types::Color;
use flui_widgets::{AbsorbPointer, ColoredBox, GestureDetector};

/// A hit-testable child so the detector's tap recognizer registers.
fn target() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}

#[test]
fn absorb_pointer_is_a_layout_passthrough() {
    let laid = lay_out(AbsorbPointer::new().child(target()), tight(120.0, 80.0));
    assert_eq!(laid.size(laid.root()), size(120.0, 80.0));
}

#[test]
fn absorb_pointer_mounts_a_render_absorb_pointer() {
    let laid = lay_out(AbsorbPointer::new().child(target()), tight(50.0, 50.0));
    let _ = laid.find_by_render_type("RenderAbsorbPointer");
}

#[test]
fn absorbing_true_blocks_the_tap_from_reaching_a_child_gesture_detector() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    // Default `AbsorbPointer::new()` absorbs (absorbing = true).
    let laid = lay_out(
        AbsorbPointer::new().child(
            GestureDetector::new()
                .on_tap(move || {
                    in_cb.fetch_add(1, Ordering::SeqCst);
                })
                .child(target()),
        ),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "absorbing = true must prevent the child GestureDetector from ever \
         registering the tap -- its subtree is never hit-tested",
    );
}

#[test]
fn absorbing_false_lets_the_tap_reach_a_child_gesture_detector() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        AbsorbPointer::new().absorbing(false).child(
            GestureDetector::new()
                .on_tap(move || {
                    in_cb.fetch_add(1, Ordering::SeqCst);
                })
                .child(target()),
        ),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "absorbing = false must behave as a transparent structural pass-through",
    );
}
