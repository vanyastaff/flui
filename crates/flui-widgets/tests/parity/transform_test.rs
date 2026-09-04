//! ## Test parity notes
//!
//! Flutter sources:
//! - `packages/flutter/test/widgets/transform_test.dart` (tag `3.44.0`, 28 cases).
//! - `packages/flutter/test/widgets/basic_test.dart` (tag `3.44.0`), the
//!   `'FractionalTranslation'` group (4 cases).
//!
//! Ported cases (9 upstream names, 11 Rust tests — hit-testing under
//! translation/scale/composition and the alignment+origin combination the
//! render object's `compute_origin` fix addresses are the portable core.
//! Composited output is fully reachable now: `LaidOut::layer_kinds` reports
//! layer *counts* — and measuring them exposed a real paint bug, recorded
//! under the zero-determinant case below — while `LaidOut::layer_tree`
//! reaches `TransformLayer::transform` for layer *matrices*. The
//! `TransformLayer` assertions that remain dropped are dropped for their own
//! reasons, named in "Out of scope"; FLUI still has no golden-file harness,
//! the separate reason `clip_test.rs` drops `paints..save()..clipRect()`
//! assertions). Every case below that taps a
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
//! - `'Composited transform offset'` — the composited transform layer's matrix
//!   folds in the alignment pivot, not just the raw scale. Upstream's raw
//!   translation does not port (its layers are parent-relative where FLUI's
//!   carry global geometry); the expectation is re-derived from the widget
//!   geometry instead, and the test's doc records the measurement and the
//!   arithmetic that pin the difference —
//!   [`the_composited_transform_layer_folds_in_the_alignment_pivot`].
//! - `'Transform with nan/inf/-inf value short-circuits rendering'` (3 upstream
//!   cases, one Rust test covering all three matrix shapes) — a non-finite
//!   determinant must short-circuit painting. Previously listed out of scope,
//!   and recorded there as *unverified*: nothing could observe composited
//!   output, so whether `RenderTransform` had the guard at all was unknown.
//!   Measuring it answered no, and the fix rides with this port —
//!   [`a_non_finite_transform_paints_no_child_content`].
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
//!
//!   The **layer** half of that case is ported too, once composited output
//!   became observable — and measuring it exposed a real paint bug, since
//!   fixed here. FLUI used to composite the identical chain for all four
//!   upstream legs: the child was painted under `scale: 0.0` exactly as under
//!   `scale: 0.01`, and the three non-finite cases below behaved the same
//!   way. `RenderTransform` now carries the oracle's guard via
//!   `RenderObject::skip_paint` — see
//!   [`a_singular_transform_paints_no_child_content_while_a_small_one_still_does`]
//!   and [`a_non_finite_transform_paints_no_child_content`]. Layer *counts*
//!   as such still do not port literally: upstream's root chain begins with a
//!   `TransformLayer` and FLUI's with an `Offset` layer, so the two differ by
//!   one layer before the subject is involved; both tests assert the contract
//!   (child content absent vs present) rather than the count.
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
//! - `'Transform.rotate'` (the layer-matrix
//!   half), `'applyPaintTransform of Transform in Padding'`, `'Transform.translate'`
//!   (the layer-avoidance-optimization half), `'3D transform renders the same
//!   with or without needsCompositing'`, `'Transform.rotate does not remove
//!   layers due to singular short-circuit'`, `'Transform.rotate creates nice
//!   rotation matrices for 0, 90, 180, 270 degrees'`,
//!   `'Transform.translate/scale/rotate with FilterQuality produces filter
//!   layer'` (4 cases), `'Transform layers update to match child and
//!   filterQuality'`, `'Transform layers with filterQuality golden'` — all
//!   `TransformLayer`/`ImageFilterLayer`/`matchesGoldenFile` assertions.
//!
//!   Neither layer *counts* nor layer *matrices* are harness-blocked any
//!   more — `LaidOut::layer_kinds` reports the former and `LaidOut::layer_tree`
//!   reaches `TransformLayer::transform` for the latter, which is how
//!   [`the_composited_transform_layer_folds_in_the_alignment_pivot`] ports
//!   `'Composited transform offset'`. What remains is named per case:
//!
//!   The five `ImageFilterLayer` cases need more than a widget field.
//!   `Transform` has no `filterQuality`, but the deeper gap is that
//!   `flui_types::painting::ImageFilter` has no *geometric* matrix variant at
//!   all (its `Matrix` variant is a 5×4 **colour** matrix); the oracle needs
//!   `ui.ImageFilter.matrix(m, filterQuality)`, i.e. resampling through a
//!   transform. Adding the field alone would emit a layer the engine cannot
//!   honour — a stub that satisfies a layer-shape assertion and changes
//!   nothing on screen.
//!
//!   The remaining layer-matrix halves state upstream's raw values, which are
//!   parent-relative; FLUI's layers carry global geometry (measured — see the
//!   ported case above). Porting each means re-deriving its expectation under
//!   FLUI's convention, case by case, rather than copying a number.
//!
//!   `matchesGoldenFile` needs golden-image capture, which does not exist.
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
use flui_rendering::layer::Layer;
use flui_types::geometry::px;
use flui_types::{Alignment, Color, Offset};
use flui_view::ViewExt;
use flui_widgets::{
    Center, ClipRect, ColoredBox, FractionalTranslation, GestureDetector, Positioned,
    RepaintBoundary, SizedBox, Stack, Transform,
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

/// Whether a singular transform paints its child, asserted on the composited
/// output — the half of `'Transform.scale with 0.0 does not paint child
/// layers'` that the three hit-test cases above could not reach.
///
/// Flutter parity: `transform_test.dart` (3.44.0), all four legs.
/// `RenderTransform.paint` (`rendering/proxy_box.dart`) computes
/// `transform.determinant()` and, when it is `0` or non-finite, clears its
/// layer and returns — *"if the matrix is singular the children would be
/// compressed to a line or single point, instead short-circuit and paint
/// nothing."* Upstream expresses this as `tester.layers` having length 1 (the
/// root alone) for the three zero legs and 3 for a small-but-non-zero
/// `scale: 0.01`.
///
/// The absolute counts do not port: upstream's root chain is a
/// `TransformLayer` and FLUI's is an `Offset` layer, so the frameworks differ
/// by one layer before the subject is even involved. The *contract* ports
/// exactly, and is what this asserts — the child's painted content is absent
/// under a singular matrix and present under a merely small one. The `Picture`
/// leaf is that content: it is where the child's draw commands land.
///
/// The non-zero leg is not decoration. Without it a `skip_paint` that returned
/// `true` unconditionally — suppressing every `Transform` in the framework —
/// would satisfy the three zero legs.
#[test]
fn a_singular_transform_paints_no_child_content_while_a_small_one_still_does() {
    const SINGULAR: [(&str, f32, f32); 3] = [
        ("scale(0.0, 0.0)", 0.0, 0.0),
        ("scale(0.0, 1.0)", 0.0, 1.0),
        ("scale(1.0, 0.0)", 1.0, 0.0),
    ];

    for (label, scale_x, scale_y) in SINGULAR {
        let mut laid = pump_widget(
            Transform::new(Matrix4::scaling(scale_x, scale_y, 1.0))
                .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
            screen(),
        );
        laid.pump();

        let kinds = laid.layer_kinds();
        assert!(
            !kinds.contains(&"Picture"),
            "Transform::{label} is singular, so its child cannot occupy a \
             single pixel and must not be painted at all; composited {kinds:?}"
        );
    }

    let mut laid = pump_widget(
        Transform::new(Matrix4::scaling(0.01, 0.01, 1.0))
            .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
        screen(),
    );
    laid.pump();

    let kinds = laid.layer_kinds();
    assert!(
        kinds.contains(&"Picture"),
        "a small but non-zero scale is still visible and must paint its child; \
         composited {kinds:?}"
    );
}

/// A non-finite matrix entry takes the same short-circuit as a zero
/// determinant — the other half of the same `det == 0 || !det.isFinite`
/// branch.
///
/// Flutter parity: `transform_test.dart` `'Transform with nan/inf/-inf value
/// short-circuits rendering'` (3.44.0, 3 cases). This file's module doc used
/// to record FLUI's behavior here as *unverified* for want of a way to observe
/// composited output; it is now observable, and this pins it.
///
/// `NAN` and `INFINITY` are deliberately separate cases rather than one loop
/// body's worth of parameters: they reach the guard by different arithmetic
/// (`NAN` fails every comparison, `INFINITY` compares greater than any
/// threshold), and an `abs() >= EPSILON`-style invertibility test — the
/// tempting reuse — rejects the first while accepting the second.
#[test]
fn a_non_finite_transform_paints_no_child_content() {
    for (label, scale_x) in [
        ("NAN", f32::NAN),
        ("INFINITY", f32::INFINITY),
        ("NEG_INFINITY", f32::NEG_INFINITY),
    ] {
        let mut laid = pump_widget(
            Transform::new(Matrix4::scaling(scale_x, 1.0, 1.0))
                .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
            screen(),
        );
        laid.pump();

        let kinds = laid.layer_kinds();
        assert!(
            !kinds.contains(&"Picture"),
            "a {label} scale gives a non-finite determinant, which must \
             short-circuit painting exactly as a zero one does; composited \
             {kinds:?}"
        );
    }
}

/// A pure translation composites **no** transform layer — the child is painted
/// at an offset instead.
///
/// Flutter parity: `RenderTransform.paint` forks on
/// `MatrixUtils.getAsTranslation` (`rendering/proxy_box.dart`, 3.44.0): a
/// matrix that only translates is applied as `super.paint(context, offset +
/// childOffset)` with the node's layer cleared, so upstream's
/// `'Transform.translate'` asserts `layers.length == 1` — the root alone.
///
/// A compositing layer per `Transform` is not free, and translation is the
/// common case: `Transform.translate`, and every `SlideTransition` built on
/// it, used to pay for one on every frame.
///
/// The scaled leg is what makes this discriminating — without it, a `paint`
/// that never pushed a transform at all would pass.
#[test]
fn a_pure_translation_composites_no_transform_layer() {
    for (label, matrix) in [
        ("identity", Matrix4::IDENTITY),
        ("translate(10, 20)", Matrix4::translation(10.0, 20.0, 0.0)),
    ] {
        let mut laid = pump_widget(
            Transform::new(matrix)
                .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
            screen(),
        );
        laid.pump();

        let kinds = laid.layer_kinds();
        assert!(
            !kinds.contains(&"Transform"),
            "{label} only translates, so the child should be painted at an \
             offset rather than through a compositing layer; got {kinds:?}"
        );
        assert!(
            kinds.contains(&"Picture"),
            "{label} must still paint its child; got {kinds:?}"
        );
    }

    let mut scaled = pump_widget(
        Transform::new(Matrix4::scaling(2.0, 2.0, 1.0))
            .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
        screen(),
    );
    scaled.pump();
    assert!(
        scaled.layer_kinds().contains(&"Transform"),
        "a scale cannot be expressed as an offset and must still composite a \
         transform layer; got {:?}",
        scaled.layer_kinds()
    );
}

/// The composited transform layer carries the oracle's own translation.
///
/// Flutter parity: `transform_test.dart` `'Composited transform offset'`
/// (3.44.0), whose expectation is `(100, 75)` for a
/// `Matrix4.diagonal3Values(0.5, 0.5, 1)` over a 400x300 box centred on an
/// 800x600 screen.
///
/// **This case used to be documented as a divergence, and the diagnosis was
/// wrong.** It asserted `(200, 150)` and explained the gap as a layer-space
/// convention — FLUI's layers carrying global geometry where upstream's are
/// parent-relative. The real cause was FLUI's bare `Transform::new` defaulting
/// `alignment` to `CENTER` where the oracle's bare constructor has none, so a
/// pivot term nothing asked for was folded into the matrix. With the default
/// corrected the number matches exactly, which also settles what `(100, 75)`
/// IS: not a pivot contribution, but the raw scale conjugated about the paint
/// offset — `offset - scale * offset` = `(200,150) - (100,75)`.
///
/// The expectation below is still derived rather than copied, so it fails on a
/// matrix that ignored the paint offset entirely.
#[test]
fn composited_transform_offset_matches_the_oracle_translation() {
    const SCALE: f32 = 0.5;

    let mut laid =
        pump_widget(
            Center::new().child(SizedBox::new(400.0, 300.0).child(
                ClipRect::new().child(
                    Transform::new(Matrix4::scaling(SCALE, SCALE, 1.0)).child(
                        RepaintBoundary::new().child(ColoredBox::new(Color::rgb(0, 255, 0))),
                    ),
                ),
            )),
            screen(),
        );
    laid.pump();

    let tree = laid.layer_tree().expect("the frame composites a tree");
    let mut matrices = Vec::new();
    let mut stack = vec![tree.root().expect("composited root")];
    while let Some(id) = stack.pop() {
        if let Some(Layer::Transform(t)) = tree.get_layer(id) {
            matrices.push(*t.transform());
        }
        if let Some(children) = tree.children(id) {
            stack.extend(children.iter().copied());
        }
    }

    assert_eq!(
        matrices.len(),
        1,
        "exactly one transform layer — FLUI's composited root is an Offset \
         layer, so unlike upstream there is no render-view transform to skip"
    );
    let matrix = matrices[0];

    // Derived from the widget geometry, not read back from the render object:
    // an 800×600 screen centres a 400×300 box at (200, 150), and the layer
    // conjugates the scale about that paint offset — `offset - scale*offset`.
    let offset_x = 200.0;
    let offset_y = 150.0;
    let expected_x = offset_x * (1.0 - SCALE);
    let expected_y = offset_y * (1.0 - SCALE);

    const TOLERANCE: f32 = 1e-3;
    assert!(
        (matrix.m[12] - expected_x).abs() < TOLERANCE
            && (matrix.m[13] - expected_y).abs() < TOLERANCE,
        "the composited matrix must translate by offset * (1 - scale) = \
         ({expected_x}, {expected_y}) — the oracle's own (100, 75); got ({}, {})",
        matrix.m[12],
        matrix.m[13],
    );
    // Both axes: the widget asks for a uniform scale, and asserting only `m[0]`
    // would accept a matrix that scaled x correctly and y not at all.
    assert!(
        (matrix.m[0] - SCALE).abs() < TOLERANCE && (matrix.m[5] - SCALE).abs() < TOLERANCE,
        "and still carry the uniform scale itself; got ({}, {})",
        matrix.m[0],
        matrix.m[5],
    );
}

/// An `origin` set on the bare constructor acts ALONE — nothing adds a
/// centre pivot underneath it.
///
/// Flutter parity: `transform_test.dart` `'Transform origin'` (3.44.0),
/// ported faithfully at last. The oracle passes `origin: Offset(100, 50)` and
/// no alignment; both of its taps are here.
///
/// This case could not be ported before: FLUI's bare `Transform::new`
/// defaulted `alignment` to `CENTER`, so `origin` combined with a pivot the
/// caller never asked for. The neighbouring `'Transform offset + alignment'`
/// port works around exactly that by decomposing this oracle's pivot into an
/// equivalent `CENTER_LEFT` + `(100, 0)` pair. With the default corrected the
/// oracle's own arguments work directly, and the workaround pair stays
/// because it is a real case of its own.
///
/// The geometry: a 100×100 box at (100, 100) scaled 0.5 about local
/// `(100, 50)` paints over local `(50, 25)..(100, 75)`, i.e. global
/// `(150, 125)..(200, 175)`.
#[test]
fn transform_origin_alone_hit_test_misses_outside_the_scaled_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        positioned_100_square(
            Transform::new(Matrix4::scaling(0.5, 0.5, 1.0))
                .origin(Offset::new(px(100.0), px(50.0)))
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
        "the oracle's first tap: (110, 110) is outside the child's global \
         span (150, 125)..(200, 175) and must not reach it",
    );
}

/// The other leg of [`transform_origin_alone_hit_test_misses_outside_the_scaled_child`].
///
/// Flutter parity: `transform_test.dart` `'Transform origin'` (3.44.0), the
/// `tapAt(190.0, 150.0)` leg. This is the assertion that fails under a
/// spurious centre pivot: with `CENTER` added to the oracle's `origin` the
/// child lands over `(175, 150)..(225, 200)` and the tap misses.
#[test]
fn transform_origin_alone_hit_test_hits_inside_the_scaled_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        positioned_100_square(
            Transform::new(Matrix4::scaling(0.5, 0.5, 1.0))
                .origin(Offset::new(px(100.0), px(50.0)))
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
        "the oracle's second tap: (190, 150) is inside the child's global \
         span and must reach it",
    );
}

/// `transform_hit_tests(false)` hits the child where it was LAID OUT, not
/// where it paints.
///
/// Flutter parity: `RenderTransform.hitTestChildren` passes a null transform
/// to `addWithPaintTransform` when `transformHitTests` is false. Flutter's own
/// widget suite has no case for it, so both legs here are derived from the
/// contract: the child is laid out over global `(100, 100)..(200, 200)` and
/// painted, scaled by half about its centre, over `(125, 125)..(175, 175)`.
///
/// `(110, 110)` is inside the laid-out square and outside the painted one,
/// which is precisely the point the flag decides — and the point that stays
/// silent if the flag is only stored and never read.
#[test]
fn transform_hit_tests_false_hits_the_laid_out_child_not_the_painted_one() {
    let hit_at = |transform_hit_tests: bool, x: f32, y: f32| {
        let did_tap = Arc::new(AtomicBool::new(false));
        let tap_cb = Arc::clone(&did_tap);
        let laid = pump_widget(
            positioned_100_square(
                Transform::new(Matrix4::scaling(0.5, 0.5, 1.0))
                    .alignment(Alignment::CENTER)
                    .transform_hit_tests(transform_hit_tests)
                    .child(
                        GestureDetector::new()
                            .behavior(HitTestBehavior::Opaque)
                            .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
                    ),
            ),
            screen(),
        );
        laid.dispatch_pointer_down(x, y);
        laid.dispatch_pointer_up(x, y);
        did_tap.load(Ordering::SeqCst)
    };

    assert!(
        !hit_at(true, 110.0, 110.0),
        "with the transform applied, (110, 110) is outside the painted \
         (125, 125)..(175, 175) and misses",
    );
    assert!(
        hit_at(false, 110.0, 110.0),
        "with `transform_hit_tests(false)` the same point is inside the \
         LAID-OUT (100, 100)..(200, 200) and hits",
    );
    assert!(
        hit_at(true, 150.0, 150.0),
        "the centre hits either way — a test that only checked the centre \
         would pass whatever the flag did",
    );
    assert!(hit_at(false, 150.0, 150.0));
}
