//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/clip_test.dart` (tag
//! `3.44.0`, 28 cases).
//!
//! Ported cases (8 upstream names, 11 Rust tests — hit-test and
//! `updateRenderObject` geometry are the portable core; FLUI has no
//! golden-file harness, so every paint-pattern (`paints..save()..clipRect()`)
//! assertion is dropped):
//! - `'ClipRect updates clipBehavior in updateRenderObject'` —
//!   [`clip_rect_update_render_object_transitions_through_the_full_clip_behavior_chain`].
//! - `'ClipRRect constructs with the right default values'` (a plain `test()`,
//!   not `testWidgets`) —
//!   [`clip_rrect_constructs_with_the_right_default_values`].
//! - `'ClipRRect updates clipBehavior in updateRenderObject'` —
//!   [`clip_rrect_update_render_object_transitions_through_the_full_clip_behavior_chain`].
//! - `'ClipOval updates clipBehavior in updateRenderObject'` —
//!   [`clip_oval_update_render_object_transitions_to_clip_none`] (delta only;
//!   the `AntiAlias → HardEdge` leg is already unit-tested — see Overlap below).
//! - `'ClipPath updates clipBehavior in updateRenderObject'` —
//!   [`clip_path_update_render_object_transitions_to_clip_none`] (delta only,
//!   same reason).
//! - `'ClipPath'` (hit-testing through a custom Path clipper) —
//!   [`clip_path_custom_clipper_hit_test_misses_outside_the_clip_rect`],
//!   [`clip_path_custom_clipper_hit_test_hits_inside_the_clip_rect`].
//! - `'ClipOval'` (hit-testing the inscribed ellipse) —
//!   [`clip_oval_hit_test_misses_a_point_outside_the_inscribed_oval`],
//!   [`clip_oval_hit_test_hits_a_point_inside_the_inscribed_oval`].
//! - `'Transparent ClipOval hit test'` (opacity does not affect hit-testing) —
//!   [`transparent_clip_oval_hit_test_still_misses_outside_the_oval`],
//!   [`transparent_clip_oval_hit_test_still_hits_inside_the_oval`].
//!
//! Overlap (citation, not re-ported): the `AntiAlias → HardEdge`
//! `clipBehavior` transition for `ClipOval`/`ClipPath` is already covered by
//! `crates/flui-widgets/src/clip/clip_oval.rs::tests::update_render_object_applies_a_changed_clip_behavior`
//! and `crates/flui-widgets/src/clip/clip_path.rs::tests::update_render_object_applies_a_changed_clip_behavior`
//! (both exercise `create_render_object`/`update_render_object` directly with
//! a detached context). Those unit tests never reach `Clip::None` or run
//! through the full widget-reconciliation pipeline, so the two parity tests
//! above port only that delta: the `Clip::None` leg, driven end-to-end
//! through [`common::LaidOut::pump_widget`].
//!
//! Known framework gaps (filed under `docs/ROADMAP.md` Cross.H, not just
//! noted here — see that file for the full writeup):
//! - `'ClipRect'` (the `ValueClipper<Rect>`-driven reclip + hit-test case) —
//!   `ClipRect`/`ClipOval` accept no custom clipper at all; `RenderClip<S>`
//!   only ever resolves `S::default_for_size` for those two shapes, and
//!   `flui_rendering::delegates::CustomClipper<T>` exists as a trait but is
//!   wired into nothing.
//! - `'ClipPath.shape'` — no `ShapeBorder`/`ShapeBorderClipper`/
//!   `ClipPath::shape` constructor exists; `ClipPath::new` only takes a
//!   `Fn(Size) -> Path` closure with no ambient `TextDirection`.
//! - `'ClipRRect supports BorderRadiusDirectional'` and `'ClipRRect is
//!   direction-aware'` — `ClipRRect` only accepts a physical `BorderRadius`,
//!   and `RenderClipRRect` carries no `text_direction` field to resolve a
//!   `BorderRadiusDirectional` against even if the widget accepted one.
//!
//! Out of scope (no golden/paint-capture harness, or no reachable analog):
//! - `'ClipRect with a FittedBox child sized to zero works with semantics'` —
//!   a semantics-tree crash regression; FLUI's headless harness has no
//!   semantics-tree assembly step to reproduce that crash class against, and
//!   the layout-only slice (`FittedBox` scaling a zero-size child) already
//!   passes trivially via `BoxFit::apply`'s epsilon guards, so no meaningful
//!   port target remains.
//! - `'debugPaintSizeEnabled'` — FLUI has no debug-paint-size overlay.
//! - `'ClipRect painting'`, `'ClipRect save, overlay, and antialiasing'`,
//!   `'ClipRRect painting'`, `'ClipOval painting'`, `'ClipPath painting'` —
//!   golden-file (`matchesGoldenFile`) assertions.
//! - `'PhysicalModel painting with Clip.antiAlias/hardEdge/antiAliasWithSaveLayer'`,
//!   `'Default PhysicalModel painting'`, `'PhysicalShape painting with
//!   Clip.antiAlias/hardEdge/antiAliasWithSaveLayer'`, `'PhysicalShape
//!   painting'` (8 cases) — golden-file assertions; `flui-widgets` also has
//!   no `PhysicalModel`/`PhysicalShape` widget wrapper yet (only the
//!   `RenderPhysicalModel`/`RenderPhysicalShape` render objects exist, in
//!   `flui-objects::proxy::physical_model`), which is moot given the golden
//!   blocker either way.
//! - `'CustomClipper reclips when notified'` — the assertions are
//!   `paints..save()..clipRect(...)` golden patterns before/after a
//!   `ValueNotifier` push; the `debugNeedsPaint` toggle inside it is
//!   incidental to that paint check and depends on the same missing
//!   `ClipRect` custom-clipper wiring as `'ClipRect'` above.
//!
//! Widget → render-object mapping:
//! - `ClipRect` → `RenderClipRect` (`RenderClip<Rect<Pixels>>`)
//! - `ClipRRect` → `RenderClipRRect` (`RenderClip<RRect>`)
//! - `ClipOval` → `RenderClipOval` (`RenderClip<Oval>`)
//! - `ClipPath` → `RenderClipPath` (`RenderClip<Path>`)
//!   all four in `crates/flui-objects/src/proxy/clip.rs`; hit-testing in
//!   `RenderClip::hit_test` gates the child on
//!   `resolve_clip(size).contains(position)` before delegating — this is
//!   exactly what the hit-test cases below exercise.
//!
//! Divergence (noted, not a correctness gap): `RenderClip::resolve_clip`
//! recomputes the clip geometry fresh on every `paint`/`hit_test` call (no
//! `_clip`-style cache keyed on a `shouldReclip` identity check the way
//! Flutter's `_RenderCustomClip<T>` does). The `ClipPath` hit-test tests below
//! prove the *closure invocation count* rather than asserting it stays at
//! `1` the way Flutter's `log` assertion does — porting the exact call count
//! would assert a performance characteristic FLUI does not share, not the
//! observable hit-test behavior.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::Size;
use flui_types::geometry::px;
use flui_types::painting::{Clip, Path};
use flui_types::styling::{BorderRadius, BorderRadiusExt};
use flui_widgets::{ClipOval, ClipPath, ClipRRect, ClipRect, GestureDetector, Opacity};

use crate::harness::{pump_widget, screen};

/// `ClipRect` starts at Flutter's default (`Clip::HardEdge`), then follows
/// each subsequent `update_render_object` call through the same
/// `Clip::AntiAlias → Clip::None` chain Flutter's test drives via three
/// consecutive `pumpWidget` calls against the *same* live render object.
///
/// Flutter parity: `clip_test.dart` `'ClipRect updates clipBehavior in
/// updateRenderObject'` (3.44.0).
#[test]
fn clip_rect_update_render_object_transitions_through_the_full_clip_behavior_chain() {
    let mut laid = pump_widget(ClipRect::new(), screen());
    let id = laid.find_by_render_type("RenderClipRect");
    assert_eq!(
        laid.clip_behavior(id),
        Clip::HardEdge,
        "ClipRect's default clip_behavior must be Clip::HardEdge"
    );

    laid.pump_widget(ClipRect::new().clip_behavior(Clip::AntiAlias));
    assert_eq!(laid.clip_behavior(id), Clip::AntiAlias);

    laid.pump_widget(ClipRect::new().clip_behavior(Clip::None));
    assert_eq!(laid.clip_behavior(id), Clip::None);
}

/// `ClipRRect`'s defaults: `Clip::AntiAlias` and a zero `BorderRadius` (a
/// sharp rectangle) — Flutter's `ClipRRect()` construction defaults, not
/// exercised by the `updateRenderObject` chain test below since that only
/// varies `clipBehavior`.
///
/// Flutter parity: `clip_test.dart` `'ClipRRect constructs with the right
/// default values'` (3.44.0) — a plain `test()`, not `testWidgets`.
#[test]
fn clip_rrect_constructs_with_the_right_default_values() {
    let laid = pump_widget(ClipRRect::new(), screen());
    let id = laid.find_by_render_type("RenderClipRRect");

    assert_eq!(laid.clip_behavior(id), Clip::AntiAlias);
    assert_eq!(laid.clip_rrect_border_radius(id), BorderRadius::ZERO);
}

/// `ClipRRect` starts at its default (`Clip::AntiAlias`) and follows the same
/// `Clip::HardEdge → Clip::None` chain Flutter's test drives.
///
/// Flutter parity: `clip_test.dart` `'ClipRRect updates clipBehavior in
/// updateRenderObject'` (3.44.0).
#[test]
fn clip_rrect_update_render_object_transitions_through_the_full_clip_behavior_chain() {
    let mut laid = pump_widget(ClipRRect::new(), screen());
    let id = laid.find_by_render_type("RenderClipRRect");
    assert_eq!(laid.clip_behavior(id), Clip::AntiAlias);

    laid.pump_widget(ClipRRect::new().clip_behavior(Clip::HardEdge));
    assert_eq!(laid.clip_behavior(id), Clip::HardEdge);

    laid.pump_widget(ClipRRect::new().clip_behavior(Clip::None));
    assert_eq!(laid.clip_behavior(id), Clip::None);
}

/// `ClipOval`'s `Clip::HardEdge → Clip::None` leg, driven through the full
/// widget-reconciliation pipeline (`pump_widget`) rather than a detached
/// `create_render_object`/`update_render_object` call. The `AntiAlias →
/// HardEdge` leg is already unit-tested (see the module doc's Overlap note);
/// `Clip::None` is new coverage.
///
/// Flutter parity: `clip_test.dart` `'ClipOval updates clipBehavior in
/// updateRenderObject'` (3.44.0) — delta only.
#[test]
fn clip_oval_update_render_object_transitions_to_clip_none() {
    let mut laid = pump_widget(ClipOval::new().clip_behavior(Clip::HardEdge), screen());
    let id = laid.find_by_render_type("RenderClipOval");
    assert_eq!(laid.clip_behavior(id), Clip::HardEdge);

    laid.pump_widget(ClipOval::new().clip_behavior(Clip::None));
    assert_eq!(laid.clip_behavior(id), Clip::None);
}

/// `ClipPath`'s `Clip::HardEdge → Clip::None` leg — same rationale as
/// [`clip_oval_update_render_object_transitions_to_clip_none`].
///
/// Flutter parity: `clip_test.dart` `'ClipPath updates clipBehavior in
/// updateRenderObject'` (3.44.0) — delta only.
#[test]
fn clip_path_update_render_object_transitions_to_clip_none() {
    let mut laid = pump_widget(
        ClipPath::new(|size: Size| {
            let mut path = Path::new();
            path.add_rect(flui_types::Rect::from_origin_size(
                flui_types::Point::ZERO,
                size,
            ));
            path
        })
        .clip_behavior(Clip::HardEdge),
        screen(),
    );
    let id = laid.find_by_render_type("RenderClipPath");
    assert_eq!(laid.clip_behavior(id), Clip::HardEdge);

    laid.pump_widget(
        ClipPath::new(|size: Size| {
            let mut path = Path::new();
            path.add_rect(flui_types::Rect::from_origin_size(
                flui_types::Point::ZERO,
                size,
            ));
            path
        })
        .clip_behavior(Clip::None),
    );
    assert_eq!(laid.clip_behavior(id), Clip::None);
}

/// A tap outside the custom clip rect `(50,50,100,100)` a `PathClipper`
/// installs must not reach the `GestureDetector` child — `RenderClip::hit_test`
/// gates on `resolve_clip(size).contains(position)` before delegating.
///
/// Flutter parity: `clip_test.dart` `'ClipPath'` (3.44.0) — the `tapAt(10, 10)`
/// leg (`log` stays empty of `'tap'`). The upstream `log` also proves
/// `getClip` runs exactly once across the whole test; FLUI recomputes the
/// clip on every call (see the module doc's Divergence note), so this test
/// asserts the closure runs (hit-testing actually evaluates the clip) without
/// asserting a specific count.
#[test]
fn clip_path_custom_clipper_hit_test_misses_outside_the_clip_rect() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let clip_evaluations = Arc::new(AtomicU32::new(0));
    let (tap_cb, eval_cb) = (Arc::clone(&did_tap), Arc::clone(&clip_evaluations));

    let laid = pump_widget(
        ClipPath::new(move |_size: Size| {
            eval_cb.fetch_add(1, Ordering::SeqCst);
            let mut path = Path::new();
            path.add_rect(flui_types::Rect::from_ltwh(
                px(50.0),
                px(50.0),
                px(100.0),
                px(100.0),
            ));
            path
        })
        .child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a tap outside the custom clip rect (50,50,100,100) must not reach the child"
    );
    assert!(
        clip_evaluations.load(Ordering::SeqCst) > 0,
        "hit-testing must actually evaluate the custom clipper"
    );
}

/// A tap inside the custom clip rect must reach the `GestureDetector` child —
/// the other side of the `contains()` branch
/// [`clip_path_custom_clipper_hit_test_misses_outside_the_clip_rect`] exercises.
///
/// Flutter parity: `clip_test.dart` `'ClipPath'` (3.44.0) — the
/// `tapAt(100, 100)` leg (`log` gains `'tap'`).
#[test]
fn clip_path_custom_clipper_hit_test_hits_inside_the_clip_rect() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        ClipPath::new(|_size: Size| {
            let mut path = Path::new();
            path.add_rect(flui_types::Rect::from_ltwh(
                px(50.0),
                px(50.0),
                px(100.0),
                px(100.0),
            ));
            path
        })
        .child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(100.0, 100.0);
    laid.dispatch_pointer_up(100.0, 100.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a tap inside the custom clip rect (50,50,100,100) must reach the child"
    );
}

/// A tap near the bounding box's corner falls outside the oval `ClipOval`
/// inscribes in its bounds (the default clip — no custom clipper installed),
/// so it must not reach the child.
///
/// Flutter parity: `clip_test.dart` `'ClipOval'` (3.44.0) — the
/// `tapAt(10, 10)` leg (`log` stays empty).
#[test]
fn clip_oval_hit_test_misses_a_point_outside_the_inscribed_oval() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        ClipOval::new().child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a tap near the bounding-box corner (10,10) is outside the inscribed \
         oval and must not reach the child"
    );
}

/// A tap at the center of the box is inside the inscribed oval — the other
/// side of the `contains()` branch
/// [`clip_oval_hit_test_misses_a_point_outside_the_inscribed_oval`] exercises.
///
/// Flutter parity: `clip_test.dart` `'ClipOval'` (3.44.0) — the
/// `tapAt(400, 300)` leg (`log` gains `'tap'`; the 800×600 test surface's
/// center).
#[test]
fn clip_oval_hit_test_hits_a_point_inside_the_inscribed_oval() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        ClipOval::new().child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a tap at the screen center (400,300) is inside the inscribed oval \
         and must reach the child"
    );
}

/// Wrapping the same `ClipOval` in `Opacity::new(0.0)` must not change
/// hit-testing — opacity is a paint-time effect only (unlike `IgnorePointer`,
/// which FLUI has no equivalent stand-in for here); a fully transparent
/// widget is still hit-tested as if fully opaque.
///
/// Flutter parity: `clip_test.dart` `'Transparent ClipOval hit test'`
/// (3.44.0) — the `tapAt(10, 10)` leg (`log` stays empty).
#[test]
fn transparent_clip_oval_hit_test_still_misses_outside_the_oval() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Opacity::new(0.0).child(
            ClipOval::new().child(
                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
            ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a fully transparent ClipOval must still miss a tap outside the \
         inscribed oval"
    );
}

/// The other side of the `contains()` branch
/// [`transparent_clip_oval_hit_test_still_misses_outside_the_oval`] exercises,
/// under the same zero-opacity wrapper.
///
/// Flutter parity: `clip_test.dart` `'Transparent ClipOval hit test'`
/// (3.44.0) — the `tapAt(400, 300)` leg (`log` gains `'tap'`).
#[test]
fn transparent_clip_oval_hit_test_still_hits_inside_the_oval() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Opacity::new(0.0).child(
            ClipOval::new().child(
                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
            ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a fully transparent ClipOval must still hit a tap inside the \
         inscribed oval"
    );
}
