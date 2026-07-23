//! ## Test parity notes
//!
//! Flutter source: no dedicated `packages/flutter/test/widgets/padding_test.dart`
//! exists at tag `3.44.0` (confirmed: `git log --follow` on that path in
//! `flutter/flutter` returns zero commits — the file has never existed under
//! that name). `Padding`'s geometry and hit-test contracts are instead defined
//! directly by its render object and exercised through generic rendering-layer
//! tests, so this port cites those:
//! - Geometry: `packages/flutter/lib/src/rendering/shifted_box.dart` (tag
//!   `3.44.0`) `RenderPadding.performLayout` (lines 253-268) — `childParentData
//!   .offset = Offset(padding.left, padding.top)`; own `size` is the child's
//!   size inflated by the padding on each axis.
//! - Hit-test translation: `RenderShiftedBox.hitTestChildren` (same file, lines
//!   102-117), which `RenderPadding` inherits unchanged — it calls
//!   `BoxHitTestResult.addWithPaintOffset(offset: childParentData.offset, ...)`,
//!   translating the hit position by the child's offset before testing it. The
//!   generic behavioral oracle for that translation is
//!   `packages/flutter/test/rendering/box_test.dart` (tag `3.44.0`)
//!   `'addWithPaintOffset'` (line 957): a position outside `[0, size)` after
//!   translation reports no hit.
//!
//! Widget → render-object mapping: `Padding` → `RenderPadding` (single-child
//! box, `flui-objects` `crates/flui-objects/src/layout/padding.rs`).
//!
//! Divergence: none. FLUI's `RenderPadding::hit_test`
//! (`crates/flui-objects/src/layout/padding.rs:200-210`) delegates to
//! `hit_test_child_at_layout_offset`, which applies the same offset-translation
//! + containment gate as Flutter's `addWithPaintOffset`.
//!
//! Overlap: `tests/layout.rs` `padding_wraps_child_plus_insets` and
//! `padding_symmetric_deflates_each_axis` already cover uniform/symmetric
//! insets; this file adds the non-uniform (`only`) case and the hit-test
//! position-correction case (position translation + gutter miss), neither of
//! which the non-parity file exercises.

use crate::common::{lay_out, loose, offset, size, tight};
use flui_types::Color;
use flui_widgets::{ColoredBox, GestureDetector, Padding, SizedBox};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Non-uniform insets translate the child's geometry independently per edge:
/// the outer size grows by each edge's own inset, and the child's offset is
/// exactly `(left, top)` — not an average or a shared value.
///
/// Flutter parity: `RenderPadding.performLayout`
/// (`rendering/shifted_box.dart:253-268`) — `size = child.size inflated by
/// padding.horizontal/vertical`; `childParentData.offset = Offset(padding.left,
/// padding.top)`. `Padding::only(left, top, right, bottom)` around a
/// `SizedBox(100, 60)` with `left=5, top=10, right=30, bottom=40` must produce
/// an outer size of `135×110` and a child offset of `(5, 10)`.
#[test]
fn padding_only_insets_translate_child_geometry_per_edge() {
    let laid = lay_out(
        Padding::only(5.0, 10.0, 30.0, 40.0).child(SizedBox::new(100.0, 60.0)),
        loose(1000.0),
    );

    assert_eq!(
        laid.size(laid.root()),
        size(135.0, 110.0),
        "outer size must be the child (100×60) inflated by left+right=35 and top+bottom=50"
    );
    let child = laid.only_child(laid.root());
    assert_eq!(
        laid.offset(child),
        offset(5.0, 10.0),
        "child offset must be exactly (padding.left, padding.top), not a symmetric average"
    );
}

/// A tap inside the padded (inset-deflated) region hits the child at
/// translated coordinates; a tap in the padding gutter reaches no hittable
/// target and the callback never fires.
///
/// Flutter parity: `RenderShiftedBox.hitTestChildren`
/// (`rendering/shifted_box.dart:102-117`) translates the hit position by the
/// child's paint offset via `BoxHitTestResult.addWithPaintOffset` before
/// testing the child; a translated position outside the child's `size` reports
/// no hit (`rendering/box_test.dart:957` `'addWithPaintOffset'`, and the base
/// `RenderBox.hitTest` containment gate at `rendering/box.dart:2952`).
/// `Padding::all(20)` around a hittable 100×100 `GestureDetector` under a
/// tight 140×140 box places the child at `[20, 120)` on each axis.
#[test]
fn padding_hit_test_translates_position_tap_in_gutter_misses() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        Padding::all(20.0).child(
            GestureDetector::new()
                .on_tap(move || {
                    in_cb.fetch_add(1, Ordering::SeqCst);
                })
                .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        ),
        tight(140.0, 140.0),
    );

    // A tap at root-local (70, 70) falls inside the padded region [20, 120):
    // translated by the child's (20, 20) offset it lands at (50, 50), well
    // within the 100×100 child — the tap must fire.
    laid.dispatch_pointer_down(70.0, 70.0);
    laid.dispatch_pointer_up(70.0, 70.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap inside the padded region must reach the child at translated coordinates"
    );

    // A tap at root-local (5, 5) falls in the top-left gutter (padding band,
    // outside [20, 120)): translated it lands at (-15, -15), outside the
    // child's bounds — no hittable target, the tap must not fire.
    laid.dispatch_pointer_down(5.0, 5.0);
    laid.dispatch_pointer_up(5.0, 5.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap in the padding gutter must miss the child (count unchanged)"
    );

    // Symmetric check on the bottom-right gutter (root-local (135, 135) is
    // past the padded region's [20, 120) upper bound on both axes).
    laid.dispatch_pointer_down(135.0, 135.0);
    laid.dispatch_pointer_up(135.0, 135.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap in the bottom-right gutter must also miss the child"
    );
}
