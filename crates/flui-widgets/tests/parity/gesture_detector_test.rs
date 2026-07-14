//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/gesture_detector_test.dart`
//! (tag `3.44.0`), group `'Tap'` → `'Translucent'` at line 176. The upstream
//! test stacks a `Listener` (wrapping a colored, hit-testable `Container`)
//! behind a `GestureDetector` with **no child of its own** ("empty space" from
//! the detector's point of view — nothing under it but the sibling behind),
//! sweeping `behavior` through `null` (→ default), `deferToChild`, `opaque`,
//! and `translucent`, and asserting whether the `Listener` still receives the
//! pointer-down and whether the detector's own tap fires.
//!
//! Widget → render-object mapping: `Stack` → `RenderStack`
//! (`crates/flui-objects/src/layout/stack.rs`, hit-tests children top-most
//! first and **stops at the first hit** — Flutter parity for `Opaque`
//! "blocking" a sibling behind it is this stop, not a property of
//! `HitTestBehavior` itself). `GestureDetector`'s underlying `Listener` →
//! `RenderListener` (`crates/flui-objects/src/interaction/listener.rs:143-160`).
//!
//! Divergence: Flutter's childless `GestureDetector` defaults to
//! `translucent` hit-test behavior
//! (`widgets/gesture_detector.dart:1571-1573` at tag `3.44.0`), while FLUI's
//! `GestureDetector` (`crates/flui-widgets/src/interaction/gesture_detector.rs:93-107`)
//! always defaults to `DeferToChild` regardless of whether a child is set —
//! a known parity gap, not covered by the tests in this file (each test below
//! sets `behavior` explicitly rather than relying on the default).

use crate::common::{lay_out, tight};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::Color;
use flui_widgets::{ColoredBox, GestureDetector, Listener, SizedBox, Stack};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// `HitTestBehavior::Opaque` on a childless `GestureDetector` fires its own
/// tap AND blocks the `Listener`/content stacked behind it from receiving the
/// pointer — the detector "occupies" its bounds even with nothing under it.
///
/// Flutter parity: `gesture_detector_test.dart:235-238` (`opaque` case of the
/// `'Translucent'` group) — `didReceivePointerDown == false`,
/// `didTap == true`.
#[test]
fn opaque_behavior_over_empty_space_fires_and_blocks_content_behind() {
    let received_down = Arc::new(AtomicBool::new(false));
    let did_tap = Arc::new(AtomicBool::new(false));
    let (down_cb, tap_cb) = (Arc::clone(&received_down), Arc::clone(&did_tap));

    let laid = lay_out(
        Stack::new((
            Listener::new()
                .on_pointer_down(move |_| down_cb.store(true, Ordering::SeqCst))
                .child(ColoredBox::new(Color::rgb(0, 255, 0)).child(SizedBox::new(100.0, 100.0))),
            GestureDetector::new()
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst))
                .behavior(HitTestBehavior::Opaque),
        )),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        !received_down.load(Ordering::SeqCst),
        "Opaque must block the Listener stacked behind the detector from receiving the pointer"
    );
    assert!(
        did_tap.load(Ordering::SeqCst),
        "Opaque must still fire the detector's own tap even with no hittable child"
    );
}

/// `HitTestBehavior::Translucent` on a childless `GestureDetector` fires its
/// own tap AND lets the `Listener`/content behind it also receive the
/// pointer — translucent targets never block what is visually behind them.
///
/// Flutter parity: `gesture_detector_test.dart:242-245` (`translucent` case)
/// — `didReceivePointerDown == true`, `didTap == true`.
#[test]
fn translucent_behavior_over_empty_space_fires_and_lets_content_behind_through() {
    let received_down = Arc::new(AtomicBool::new(false));
    let did_tap = Arc::new(AtomicBool::new(false));
    let (down_cb, tap_cb) = (Arc::clone(&received_down), Arc::clone(&did_tap));

    let laid = lay_out(
        Stack::new((
            Listener::new()
                .on_pointer_down(move |_| down_cb.store(true, Ordering::SeqCst))
                .child(ColoredBox::new(Color::rgb(0, 255, 0)).child(SizedBox::new(100.0, 100.0))),
            GestureDetector::new()
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst))
                .behavior(HitTestBehavior::Translucent),
        )),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        received_down.load(Ordering::SeqCst),
        "Translucent must let the pointer continue through to the Listener behind it"
    );
    assert!(
        did_tap.load(Ordering::SeqCst),
        "Translucent must still fire the detector's own tap"
    );
}

/// `HitTestBehavior::DeferToChild` (explicit) on a childless `GestureDetector`
/// never fires its own tap (nothing to defer to) but does NOT block the
/// content behind it — the pointer falls through to the `Listener`.
///
/// Flutter parity: `gesture_detector_test.dart:228-231` (`deferToChild` case)
/// — `didReceivePointerDown == true`, `didTap == false`.
#[test]
fn defer_to_child_behavior_over_empty_space_never_fires_but_lets_pointer_through() {
    let received_down = Arc::new(AtomicBool::new(false));
    let did_tap = Arc::new(AtomicBool::new(false));
    let (down_cb, tap_cb) = (Arc::clone(&received_down), Arc::clone(&did_tap));

    let laid = lay_out(
        Stack::new((
            Listener::new()
                .on_pointer_down(move |_| down_cb.store(true, Ordering::SeqCst))
                .child(ColoredBox::new(Color::rgb(0, 255, 0)).child(SizedBox::new(100.0, 100.0))),
            GestureDetector::new()
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst))
                .behavior(HitTestBehavior::DeferToChild),
        )),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        received_down.load(Ordering::SeqCst),
        "DeferToChild with no child must not block the Listener behind it"
    );
    assert!(
        !did_tap.load(Ordering::SeqCst),
        "DeferToChild with no child must never fire its own tap"
    );
}
