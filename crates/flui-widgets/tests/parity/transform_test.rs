//! ## Test parity notes
//!
//! Flutter sources:
//! - `packages/flutter/test/widgets/transform_test.dart` (tag `3.44.0`, 28 cases).
//! - `packages/flutter/test/widgets/basic_test.dart` (tag `3.44.0`), the
//!   `'FractionalTranslation'` group (4 cases).
//!
//! Ported cases (7 upstream names, 9 Rust tests — hit-testing under
//! translation/scale/composition and the alignment+origin combination the
//! render object's `compute_origin` fix addresses are the portable core; FLUI
//! has no golden-file/compositing-layer harness, so every `TransformLayer`
//! matrix/count assertion is dropped, same reason `clip_test.rs` drops
//! `paints..save()..clipRect()` assertions). Every case below that taps a
//! target starts from a fresh `AtomicBool::new(false)`, so upstream's pre-tap
//! `expect(didReceiveTap`/`pointerDown, isFalse)` — asserting only that the
//! flag is still at its default before any interaction, not a behavior — is
//! dropped uniformly across all of them (the `'Transform alignment'`,
//! `'Transform offset + alignment'`, `'Translated child into translated box -
//! hit test'`, and `'FractionalTranslation'` cases below, 8 Rust tests in
//! total):
//! - `'Transform alignment'` (the `tapAt` hit-test legs; the render-view
//!   `Positioned`/`Stack` decoy that proves the *unrotated* screen position is
//!   not itself hittable is dropped — nothing in the FLUI setup below occupies
//!   that position) —
//!   [`transform_alignment_hit_test_misses_outside_the_scaled_child`],
//!   [`transform_alignment_hit_test_hits_inside_the_scaled_child`].
//! - `'Transform offset + alignment'` (same drop as above) —
//!   [`transform_offset_and_alignment_hit_test_misses_outside_the_scaled_child`],
//!   [`transform_offset_and_alignment_hit_test_hits_inside_the_scaled_child`].
//!   This is the highest-value pair: it exercises `RenderTransform::compute_origin`'s
//!   additive `origin` + `alignment` combination (the bug the render object's own
//!   `compute_origin_combines_alignment_and_origin` unit test already covers in
//!   isolation) end-to-end through `Transform`'s widget → render-object wiring,
//!   which the unit test alone does not reach.
//! - `'Translated child into translated box - hit test'` (nested
//!   `Transform.translate` composition) —
//!   [`nested_translate_composition_hit_test_reaches_the_doubly_translated_child`].
//! - `'Transform.translate'` (the `getTopLeft` assert; ported as an
//!   equivalent hit-test proof rather than a direct offset assertion — the
//!   harness's `absolute_offset` sums each ancestor's *committed layout
//!   offset*, and `RenderTransform` never writes one for its child, so it
//!   cannot observe a paint-only shift; see the test's own doc comment for
//!   the empirical confirmation. The `expect(layers.length, 1)` half stays
//!   out of scope — see below) —
//!   [`transform_translate_hit_test_reaches_the_child_at_its_shifted_position`].
//! - `'FractionalTranslation'` group, all three `'hit test - ...'` cases (the
//!   `'semantics bounds are updated'` fourth case is out of scope — see
//!   below) —
//!   [`fractional_translation_hit_test_entirely_inside_the_bounding_box`],
//!   [`fractional_translation_hit_test_partially_inside_the_bounding_box`],
//!   [`fractional_translation_hit_test_completely_outside_the_bounding_box`].
//!
//! Delta ports (not named upstream `testWidgets` cases; cited against the
//! render-object source contract instead — the same convention
//! `padding_test.rs` uses where no dedicated test file exists for the
//! behavior):
//! - `RenderFractionalTranslation.hitTestChildren`'s `transformHitTests`
//!   conditional (`rendering/proxy_box.dart`, 3.44.0: `offset: transformHitTests
//!   ? Offset(...) : null`) — every upstream `'FractionalTranslation'` hit-test
//!   case leaves `transformHitTests` at its default `true`; this port adds the
//!   `false` leg (proving the child is hit-tested at its *unshifted* layout
//!   offset, ignoring the paint-time shift) since no upstream test exercises it
//!   at all —
//!   [`fractional_translation_transform_hit_tests_false_hit_tests_the_unshifted_child`].
//! - `'Transform.scale with 0.0 does not paint child layers'`'s three
//!   zero-determinant legs (`scale: 0.0`, `scaleX: 0.0`, `scaleY: 0.0`) —
//!   all four of upstream's `expect(tester.layers, hasLength(...))`
//!   assertions are layer counts (the fourth, `scale: 0.01`, is a non-zero
//!   sanity check), so none of this upstream test is a literal hit-test
//!   port; this delta port instead probes the hit-test consequence of the
//!   same three zero-determinant matrix shapes —
//!   `RenderTransform::hit_test`'s `try_inverse()` returns `None` for each,
//!   so the node reports no hit at all, regardless of tap position —
//!   [`transform_scale_zero_hit_test_misses_the_non_invertible_transform`],
//!   [`transform_scale_x_zero_hit_test_misses_the_non_invertible_transform`],
//!   [`transform_scale_y_zero_hit_test_misses_the_non_invertible_transform`].
//! - `'Transform.scale'`'s scale-factor assertion (the `m[0][0]` delta only —
//!   the full composited-layer matrix, including the CENTER-alignment pivot's
//!   translation component, is a `TransformLayer` assertion, out of scope) —
//!   proves the `Transform` widget's `create_render_object`/`update_render_object`
//!   wiring reaches `RenderTransform` correctly through the full
//!   widget-reconciliation pipeline, which `crates/flui-objects/src/layout/transform.rs`'s
//!   own `test_transform_scale` (a detached constructor call) does not exercise —
//!   [`transform_scale_widget_wires_the_scale_factor_through_to_the_render_object`].
//! - `'Transform.rotate'`'s rotation-factor assertion (same rationale, mirrored
//!   for rotation) —
//!   [`transform_rotation_widget_wires_the_angle_through_to_the_render_object`].
//!
//! Known framework gaps (filed under `docs/ROADMAP.md` Cross.H — see that file
//! for the full writeup):
//! - **`Transform`'s bare matrix constructor defaults `alignment` to
//!   `Alignment::CENTER` unconditionally**, where Flutter's bare
//!   `Transform(transform:, origin:)` constructor defaults `alignment` to
//!   `null` (no contribution — `origin` acts alone). Flutter's
//!   `Transform.rotate`/`Transform.scale`/`Transform.flip` factories *do*
//!   default `alignment` to `Alignment.center` explicitly, and `Transform.translate`
//!   is pivot-invariant either way, so this only diverges for `origin`-only
//!   usage of the general constructor. Confirmed by attempting to port
//!   `'Transform origin'`: Flutter's expected pivot for that case is `origin`
//!   alone, `(100.0, 50.0)`; FLUI's `Transform::new(..).origin(..)` (no
//!   `.alignment(..)` call) computes `(150.0, 100.0)` (CENTER's `(50, 50)`
//!   contribution added on top) for the same 100×100 box — a different
//!   pivot, so the upstream tap coordinates do not carry over. Not ported.
//! - **`Transform` has no `transformHitTests` toggle at all** — Flutter's
//!   `RenderTransform`/`Transform` widget (`rendering/proxy_box.dart`,
//!   `widgets/basic.dart`, both 3.44.0) carry a `transformHitTests` field
//!   (default `true`) that, when `false`, skips the transform for hit-testing
//!   while `applyPaintTransform`/`localToGlobal` still honor it unconditionally.
//!   `crates/flui-objects/src/layout/transform.rs`'s `RenderTransform` has no
//!   such field — `hit_test` always inverts `effective_transform`, with no way
//!   to opt out. `RenderFractionalTranslation` (the sibling render object in
//!   the same file) already carries this exact toggle, so the gap is
//!   `Transform`-specific, not systemic to the family.
//!
//! Out of scope (no golden/paint-capture harness, or no reachable analog):
//! - `'Transform origin'`, `'Transform AlignmentDirectional alignment'` — see
//!   the first Known gap above (origin-only pivot mismatch) and the second
//!   (no `AlignmentDirectional`/`TextDirection` resolution path exists on
//!   `Transform` at all — its `alignment` field is a bare `Alignment`, never
//!   an `AlignmentGeometry`).
//! - `'Composited transform offset'`, `'Transform.rotate'` (the layer-matrix
//!   half), `'applyPaintTransform of Transform in Padding'`, `'Transform.translate'`
//!   (the layer-avoidance-optimization half), `'3D transform renders the same
//!   with or without needsCompositing'`, `'Transform.rotate does not remove
//!   layers due to singular short-circuit'`, `'Transform.rotate creates nice
//!   rotation matrices for 0, 90, 180, 270 degrees'`, `'Transform.scale with
//!   0.0 does not paint child layers'` (all four `expect(tester.layers,
//!   hasLength(...))` legs are layer counts — see the Delta ports section
//!   above for the hit-test probes this port adds instead),
//!   `'Transform.translate/scale/rotate with FilterQuality produces filter
//!   layer'` (4 cases), `'Transform layers update to match child and
//!   filterQuality'`, `'Transform layers with filterQuality golden'` — all
//!   `TransformLayer`/`ImageFilterLayer`/`matchesGoldenFile` assertions; FLUI's
//!   headless harness has no compositing-layer introspection or golden-image
//!   capture.
//! - `'Transform with nan/inf/-inf value short-circuits rendering'` (3 cases)
//!   — Flutter's `Transform._computeRotation`/paint path short-circuits to a
//!   single (root) layer when the matrix carries a non-finite entry; whether
//!   `RenderTransform` has an equivalent guard is unverified (no layer count
//!   to assert against either way), and probing it would require the same
//!   missing layer-count harness.
//! - `"Transform.scale() does not accept all three ... to be non-null"`,
//!   `"Transform.scale() needs at least one of ... to be non-null"` —
//!   Dart-specific `assert()`-throws tests guarding `Transform.scale`'s
//!   `scale`/`scaleX`/`scaleY` mutually-exclusive-optional-parameter API. FLUI's
//!   `Transform::scale(sx, sy)` takes two required positional `f32`s — the
//!   ambiguous-overload state these tests guard against is not representable
//!   in the first place, not merely untested.
//! - `"Transform.scale() scales widget uniformly/according to scaleX and
//!   scaleY"` (2 cases), `'Transform.flip does flip child correctly'` — these
//!   assert the child's on-screen bounding-box corners
//!   (`tester.getBottomRight`/`tapAt` over a flipped grid) via the ancestor
//!   `applyPaintTransform` chain composed all the way to the child; FLUI's
//!   test harness (`tests/common/mod.rs`) has no "map a local point through
//!   the accumulated ancestor paint transform" helper — only `absolute_offset`,
//!   which is documented translation-only and explicitly invalid under
//!   scale/rotation. Adding that helper is harness-plumbing work beyond this
//!   test-porting pass.
//! - `'FractionalTranslation'` group's `'semantics bounds are updated'` case
//!   — a semantics-tree transform assertion; FLUI's headless harness has no
//!   semantics-tree assembly step.
//!
//! Widget → render-object mapping:
//! - `Transform` → `RenderTransform` (`crates/flui-objects/src/layout/transform.rs`)
//! - `FractionalTranslation` → `RenderFractionalTranslation`
//!   (`crates/flui-objects/src/layout/fractional_translation.rs`)
//!
//! Divergence (widget API extension made to reach this port, not a behavior
//! bug): `Transform`'s widget wrapper (`crates/flui-widgets/src/layout/transform.rs`)
//! previously exposed no way to set `alignment`/`origin` at all — only the
//! underlying `RenderTransform` supported them. `.alignment(..)`/`.origin(..)`
//! builders were added (mirroring `FractionalTranslation`'s existing
//! `build_render_object` pattern in the same crate) purely to reach the
//! alignment/origin hit-test cases above; no render-object behavior changed.

use flui_geometry::Matrix4;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::geometry::px;
use flui_types::{Alignment, Offset};
use flui_view::ViewExt;
use flui_widgets::{
    Center, FractionalTranslation, GestureDetector, Positioned, SizedBox, Stack, Transform,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::harness::{pump_widget, screen};

/// Wraps `child` at absolute screen position `(100, 100)` in a 100×100 box —
/// the same geometry `transform_test.dart`'s `'Transform alignment'`/`'Transform
/// offset + alignment'` cases build via `Positioned(top: 100, left: 100, child:
/// SizedBox.square(dimension: 100, ...))` inside a `Stack`. `Positioned`
/// (rather than `Padding`) is load-bearing here, not cosmetic: `Positioned`
/// with only `left`/`top` set hands its child LOOSE constraints, letting
/// `SizedBox::new(100.0, 100.0)` size itself exactly; `Padding` under the
/// screen's TIGHT 800×600 constraints would deflate to a tight 700×500 box,
/// forcing the inner `SizedBox` to that size instead (`BoxConstraints`
/// `min == max` overrides any requested size). The decoy `Container` the
/// upstream cases stack behind the `Transform` (proving the *unrotated*
/// screen position is not itself hittable) is dropped — nothing here
/// occupies that position either, so the same fact holds trivially.
fn positioned_100_square(child: impl flui_view::IntoView) -> Stack {
    Stack::new(vec![
        Positioned::new(SizedBox::new(100.0, 100.0).child(child))
            .left(100.0)
            .top(100.0)
            .boxed(),
    ])
}

/// A tap at the screen-space position the *untransformed* box would occupy
/// must miss — `Transform`'s `alignment: Alignment::CENTER_RIGHT` (no
/// `origin`) scales the child to a quarter of the box, anchored at local
/// `(100.0, 50.0)`: `RenderTransform::compute_origin` gives
/// `align = (100 * midpoint(1.0, 1.0), 100 * midpoint(0.0, 1.0)) = (100.0,
/// 50.0)`, `origin = Offset::ZERO` (unset), pivot = `(100.0, 50.0)`. The
/// scaled child spans local `x: [50.0, 100.0], y: [25.0, 75.0]` — absolute
/// `x: [150.0, 200.0], y: [125.0, 175.0]` once the 100×100 box's own
/// `(100.0, 100.0)` screen offset is added. `(110.0, 110.0)` (local `(10.0,
/// 10.0)`) falls outside that span.
///
/// Flutter parity: `transform_test.dart` `'Transform alignment'` (3.44.0) —
/// the `tapAt(110.0, 110.0)` leg (`didReceiveTap` stays `false`).
#[test]
fn transform_alignment_hit_test_misses_outside_the_scaled_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        positioned_100_square(
            Transform::new(Matrix4::scaling(0.5, 0.5, 1.0))
                .alignment(Alignment::CENTER_RIGHT)
                .child(
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
                ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(110.0, 110.0);
    laid.dispatch_pointer_up(110.0, 110.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a tap at the untransformed box's corner (110, 110) is outside the \
         alignment-scaled child (absolute x: [150, 200], y: [125, 175]) and \
         must not reach it"
    );
}

/// The other side of the `contains()`-equivalent branch
/// [`transform_alignment_hit_test_misses_outside_the_scaled_child`] exercises
/// — `(190.0, 150.0)` (local `(90.0, 50.0)`) falls inside the scaled span
/// `x: [50.0, 100.0], y: [25.0, 75.0]`.
///
/// Flutter parity: `transform_test.dart` `'Transform alignment'` (3.44.0) —
/// the `tapAt(190.0, 150.0)` leg (`didReceiveTap` becomes `true`).
#[test]
fn transform_alignment_hit_test_hits_inside_the_scaled_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        positioned_100_square(
            Transform::new(Matrix4::scaling(0.5, 0.5, 1.0))
                .alignment(Alignment::CENTER_RIGHT)
                .child(
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
                ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(190.0, 150.0);
    laid.dispatch_pointer_up(190.0, 150.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a tap inside the alignment-scaled child's absolute span (190, 150) \
         must reach it"
    );
}

/// Same expected pivot `(100.0, 50.0)` as the alignment case above, reached a
/// different way: `alignment: Alignment::CENTER_LEFT` contributes `(100 *
/// midpoint(-1.0, 1.0), 100 * midpoint(0.0, 1.0)) = (0.0, 50.0)`, plus an
/// explicit `origin: (100.0, 0.0)` — `RenderTransform::compute_origin`'s
/// additive combination gives `(0.0 + 100.0, 50.0 + 0.0) = (100.0, 50.0)`.
/// This is the case the render object's own
/// `compute_origin_combines_alignment_and_origin` unit test covers in
/// isolation; this port drives the *same* combination through
/// `Transform`'s widget → render-object wiring end-to-end.
///
/// Flutter parity: `transform_test.dart` `'Transform offset + alignment'`
/// (3.44.0) — the `tapAt(110.0, 110.0)` leg (`didReceiveTap` stays `false`).
#[test]
fn transform_offset_and_alignment_hit_test_misses_outside_the_scaled_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        positioned_100_square(
            Transform::new(Matrix4::scaling(0.5, 0.5, 1.0))
                .alignment(Alignment::CENTER_LEFT)
                .origin(Offset::new(px(100.0), px(0.0)))
                .child(
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
                ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(110.0, 110.0);
    laid.dispatch_pointer_up(110.0, 110.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a tap at (110, 110) is outside the origin+alignment-scaled child \
         (same absolute span as the alignment-only case: x: [150, 200], y: \
         [125, 175]) and must not reach it"
    );
}

/// The other side of the branch
/// [`transform_offset_and_alignment_hit_test_misses_outside_the_scaled_child`]
/// exercises.
///
/// Flutter parity: `transform_test.dart` `'Transform offset + alignment'`
/// (3.44.0) — the `tapAt(190.0, 150.0)` leg (`didReceiveTap` becomes `true`).
#[test]
fn transform_offset_and_alignment_hit_test_hits_inside_the_scaled_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        positioned_100_square(
            Transform::new(Matrix4::scaling(0.5, 0.5, 1.0))
                .alignment(Alignment::CENTER_LEFT)
                .origin(Offset::new(px(100.0), px(0.0)))
                .child(
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
                ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(190.0, 150.0);
    laid.dispatch_pointer_up(190.0, 150.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a tap inside the origin+alignment-scaled child's absolute span \
         (190, 150) must reach it"
    );
}

/// Two nested `Transform::translate` nodes must compose: the outer
/// `(100.0, 50.0)` and inner `(1000.0, 1000.0)` translations both apply, so a
/// tap at the doubly-translated child's actual painted center reaches it. The
/// child (`GestureDetector`, tightly sized to the 800×600 screen by the root
/// constraints) has local center `(400.0, 300.0)`; through both translations
/// that lands at `(400 + 1000 + 100, 300 + 1000 + 50) = (1500.0, 1350.0)` —
/// far outside the 800×600 viewport, matching Flutter's own oracle (whose
/// `Container` is likewise pushed off-screen): hit-testing is a coordinate
/// transform, not a viewport-clipped operation, and `RenderTransform::hit_test`
/// does not gate on its own (untransformed) bounds before delegating.
///
/// Flutter parity: `transform_test.dart` `'Translated child into translated
/// box - hit test'` (3.44.0).
#[test]
fn nested_translate_composition_hit_test_reaches_the_doubly_translated_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Transform::translate(100.0, 50.0).child(
            Transform::translate(1000.0, 1000.0).child(
                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
            ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(1500.0, 1350.0);
    laid.dispatch_pointer_up(1500.0, 1350.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a tap at (1500, 1350) — the child's local center (400, 300) plus \
         both nested translations (1000, 1000) then (100, 50) — must reach \
         the doubly-translated child"
    );
}

/// A uniform zero scale collapses `effective_transform` to a singular
/// (non-invertible) matrix — `RenderTransform::hit_test`'s `try_inverse()`
/// returns `None`, so the node reports no hit at all, regardless of position.
///
/// Flutter parity: `transform_test.dart` `'Transform.scale with 0.0 does not
/// paint child layers'` (3.44.0) — the `scale: 0.0` leg (hit-test half only;
/// the upstream assertion is a layer count, out of scope here).
#[test]
fn transform_scale_zero_hit_test_misses_the_non_invertible_transform() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Transform::scale(0.0, 0.0).child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a Transform::scale(0.0, 0.0) is a non-invertible matrix; even a tap \
         at the screen center must miss"
    );
}

/// A single collapsed axis (`scaleX: 0.0`, `scaleY` left non-zero) is also a
/// zero-determinant matrix — the same `try_inverse() == None` branch as
/// [`transform_scale_zero_hit_test_misses_the_non_invertible_transform`],
/// from a differently-shaped input (one axis collapsed, not both).
///
/// Flutter parity: `transform_test.dart` `'Transform.scale with 0.0 does not
/// paint child layers'` (3.44.0) — the `scaleX: 0.0` leg (hit-test half only).
#[test]
fn transform_scale_x_zero_hit_test_misses_the_non_invertible_transform() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Transform::scale(0.0, 1.0).child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a Transform::scale(0.0, 1.0) collapses the x axis to a \
         non-invertible matrix; even a tap at the screen center must miss"
    );
}

/// `Center` places the 100×100 `FractionalTranslation` box at absolute
/// `(350.0, 250.0)` on the 800×600 screen. A zero translation leaves the
/// child exactly where it was laid out — its center at `(400.0, 300.0)`, the
/// screen center, entirely inside the `FractionalTranslation`'s own 100×100
/// footprint.
///
/// Flutter parity: `basic_test.dart` `'FractionalTranslation'` group,
/// `'hit test - entirely inside the bounding box'` (3.44.0).
#[test]
fn fractional_translation_hit_test_entirely_inside_the_bounding_box() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Center::new().child(
            FractionalTranslation::new(0.0, 0.0).child(
                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_tap(move || tap_cb.store(true, Ordering::SeqCst))
                    .child(SizedBox::new(100.0, 100.0)),
            ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a zero translation must still hit the child at its laid-out center \
         (400, 300)"
    );
}

/// `translation: (0.5, 0.5)` shifts the child by half its own size —
/// `(50.0, 50.0)` — so its painted center moves from `(400.0, 300.0)` to
/// `(450.0, 350.0)`, half outside the `FractionalTranslation`'s own 100×100
/// footprint (`[350, 450] x [250, 350]`). `transform_hit_tests` defaults to
/// `true`, so hit-testing follows the shift.
///
/// Flutter parity: `basic_test.dart` `'FractionalTranslation'` group,
/// `'hit test - partially inside the bounding box'` (3.44.0).
#[test]
fn fractional_translation_hit_test_partially_inside_the_bounding_box() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Center::new().child(
            FractionalTranslation::new(0.5, 0.5).child(
                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_tap(move || tap_cb.store(true, Ordering::SeqCst))
                    .child(SizedBox::new(100.0, 100.0)),
            ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(450.0, 350.0);
    laid.dispatch_pointer_up(450.0, 350.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a half-size translation must hit the child at its shifted center \
         (450, 350), even though that point is half outside the \
         FractionalTranslation's own untranslated footprint"
    );
}

/// `translation: (1.0, 1.0)` shifts the child by its *entire* own size, so
/// its painted center (`(500.0, 400.0)`) lands completely outside the
/// `FractionalTranslation`'s own 100×100 footprint (`[350, 450] x [250,
/// 350]`) — zero overlap. `transform_hit_tests` still defaults to `true`, so
/// the tap still reaches it: `RenderFractionalTranslation::hit_test`
/// deliberately skips its own-bounds check (its doc: "a pointer over the
/// SHIFTED child still hits even when it lies outside the box's original
/// bounds").
///
/// Flutter parity: `basic_test.dart` `'FractionalTranslation'` group,
/// `'hit test - completely outside the bounding box'` (3.44.0).
#[test]
fn fractional_translation_hit_test_completely_outside_the_bounding_box() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Center::new().child(
            FractionalTranslation::new(1.0, 1.0).child(
                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_tap(move || tap_cb.store(true, Ordering::SeqCst))
                    .child(SizedBox::new(100.0, 100.0)),
            ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(500.0, 400.0);
    laid.dispatch_pointer_up(500.0, 400.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a full-size translation must still hit the child at its shifted \
         center (500, 400), which no longer overlaps the \
         FractionalTranslation's own original footprint at all"
    );
}

/// The delta upstream leaves untested: `transform_hit_tests(false)` makes hit
/// testing ignore the paint-time shift entirely, testing the child at its
/// *unshifted* layout offset (`Offset::ZERO`) instead. With the same `(1.0,
/// 1.0)` translation as
/// [`fractional_translation_hit_test_completely_outside_the_bounding_box`], a
/// tap at the child's now-*painted* center (500, 400) must MISS (nothing is
/// laid out there — the child never moved for hit-testing purposes), while a
/// tap at the original, unshifted center (400, 300) must HIT.
///
/// Flutter parity: no upstream `testWidgets` case exercises `transformHitTests:
/// false` on `FractionalTranslation`; cited instead against
/// `RenderFractionalTranslation.hitTestChildren`'s source contract
/// (`rendering/proxy_box.dart`, 3.44.0): `offset: transformHitTests ?
/// Offset(translation.dx * size.width, ...) : null`.
#[test]
fn fractional_translation_transform_hit_tests_false_hit_tests_the_unshifted_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Center::new().child(
            FractionalTranslation::new(1.0, 1.0)
                .transform_hit_tests(false)
                .child(
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || tap_cb.store(true, Ordering::SeqCst))
                        .child(SizedBox::new(100.0, 100.0)),
                ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(500.0, 400.0);
    laid.dispatch_pointer_up(500.0, 400.0);
    assert!(
        !did_tap.load(Ordering::SeqCst),
        "transform_hit_tests(false) must ignore the paint-time shift — a tap \
         at the child's painted center (500, 400) must miss"
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);
    assert!(
        did_tap.load(Ordering::SeqCst),
        "transform_hit_tests(false) must test the child at its unshifted \
         layout offset — a tap at the original center (400, 300) must hit"
    );
}

/// `Transform::scale(2.0, 3.0)` must set the underlying `RenderTransform`'s
/// scale factor through the full `pump_widget` reconciliation pipeline —
/// `Transform::create_render_object`/`build_render_object`, not just the
/// detached `RenderTransform::scale` constructor
/// `crates/flui-objects/src/layout/transform.rs::tests::test_transform_scale`
/// already covers.
///
/// Flutter parity: `transform_test.dart` `'Transform.scale'` (3.44.0) —
/// the scale-factor delta only; the full composited-layer matrix (including
/// the CENTER-alignment pivot's translation) is a `TransformLayer` assertion,
/// out of scope.
#[test]
fn transform_scale_widget_wires_the_scale_factor_through_to_the_render_object() {
    let laid = pump_widget(
        Transform::scale(2.0, 3.0).child(GestureDetector::new()),
        screen(),
    );
    let id = laid.find_by_render_type("RenderTransform");

    assert_eq!(
        laid.transform_scale(id),
        2.0,
        "Transform::scale(2.0, 3.0) must set the render object's x-scale to 2.0"
    );
}

/// `Transform::rotation(PI / 2.0)` must set the underlying `RenderTransform`'s
/// rotation through the full `pump_widget` pipeline — same rationale as
/// [`transform_scale_widget_wires_the_scale_factor_through_to_the_render_object`].
///
/// Flutter parity: `transform_test.dart` `'Transform.rotate'` (3.44.0) — the
/// rotation-factor delta only; the composited-layer matrix is out of scope.
#[test]
fn transform_rotation_widget_wires_the_angle_through_to_the_render_object() {
    let laid = pump_widget(
        Transform::rotation(std::f32::consts::FRAC_PI_2).child(GestureDetector::new()),
        screen(),
    );
    let id = laid.find_by_render_type("RenderTransform");

    assert!(
        (laid.transform_rotation(id) - std::f32::consts::FRAC_PI_2).abs() < 1e-4,
        "Transform::rotation(PI/2) must set the render object's rotation to \
         PI/2 (got {})",
        laid.transform_rotation(id)
    );
}

/// `Transform::translate` shifts the child by `(100.0, 50.0)` for painting
/// and hit-testing while leaving its own committed layout offset at
/// `Offset::ZERO` — same as Flutter, where `RenderTransform`'s child
/// `parentData.offset` also stays zero; `getTopLeft` there is a
/// `localToGlobal`/`applyPaintTransform` matrix walk, not a
/// `parentData.offset` sum. The harness's `absolute_offset` sums each
/// ancestor's *committed layout offset*, and `RenderTransform` never writes
/// one for its child (the shift lives only in `paint_transform`
/// /`effective_transform`) — confirmed empirically: `absolute_offset` reads
/// `Offset::ZERO` for the child below, not `(100.0, 50.0)`, so it cannot
/// stand in for `getTopLeft` here. This proves the same shift the way every
/// other hit-test case in this file does instead: the child fills the
/// tight 800×600 screen, so its *unshifted* footprint would be `x: [0,
/// 800], y: [0, 600]`; its *actual*, translated one is `x: [100, 900], y:
/// [50, 650]`. `(50.0, 300.0)` falls in the former but not the latter —
/// must miss; `(150.0, 300.0)` falls in both — must hit.
///
/// Flutter parity: `transform_test.dart` `'Transform.translate'` (3.44.0) —
/// the `expect(tester.getTopLeft(find.byType(Container)), const
/// Offset(100.0, 50.0))` assert, ported as an equivalent hit-test proof (see
/// above for why a direct `absolute_offset` assertion does not reach this
/// case). The `expect(layers.length, 1)` half (no transform layer for a
/// pure translation) stays out of scope — see the "Out of scope" list below.
#[test]
fn transform_translate_hit_test_reaches_the_child_at_its_shifted_position() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Transform::translate(100.0, 50.0).child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(50.0, 300.0);
    laid.dispatch_pointer_up(50.0, 300.0);
    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a tap at (50, 300) falls inside the child's unshifted 800x600 \
         footprint (x: [0, 800]) but outside its actual, translated one \
         (x: [100, 900]) and must miss"
    );

    laid.dispatch_pointer_down(150.0, 300.0);
    laid.dispatch_pointer_up(150.0, 300.0);
    assert!(
        did_tap.load(Ordering::SeqCst),
        "a tap at (150, 300) falls inside the child's translated footprint \
         (x: [100, 900], y: [50, 650]) and must hit"
    );
}

/// A single collapsed axis on the *other* dimension (`scaleY: 0.0`, `scaleX`
/// left non-zero) — upstream's third zero-determinant leg, alongside
/// [`transform_scale_zero_hit_test_misses_the_non_invertible_transform`] and
/// [`transform_scale_x_zero_hit_test_misses_the_non_invertible_transform`].
/// Same `try_inverse() == None` branch, from the third of the three
/// differently-shaped zero-determinant matrices upstream's test builds.
///
/// Flutter parity: `transform_test.dart` `'Transform.scale with 0.0 does not
/// paint child layers'` (3.44.0) — the `scaleY: 0.0` leg (delta port; see
/// the module doc's Delta ports section for why none of this upstream test
/// is a literal hit-test port).
#[test]
fn transform_scale_y_zero_hit_test_misses_the_non_invertible_transform() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Transform::scale(1.0, 0.0).child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "a Transform::scale(1.0, 0.0) collapses the y axis to a \
         non-invertible matrix; even a tap at the screen center must miss"
    );
}
