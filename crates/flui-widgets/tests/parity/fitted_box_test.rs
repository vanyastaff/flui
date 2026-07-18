//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/fitted_box_test.dart` (tag
//! `3.44.0`, 15 cases).
//!
//! Ported cases (11 upstream names, 11 Rust tests — every geometry, clip-
//! storage, and hit-test case; FLUI has no golden-file/compositing-layer
//! harness, so every `getLayers()` layer-count assertion is dropped, same
//! reason `clip_test.rs`/`transform_test.rs` drop paint-pattern assertions):
//! - `'Can size according to aspect ratio'` — [`can_size_according_to_aspect_ratio`].
//! - `'Can contain child'` — [`can_contain_child`].
//! - `'Child can cover'` — [`child_can_cover`]. This case exercises
//!   `BoxFit::Cover`'s CROP path (the destination fills the box on one axis
//!   and overflows-then-crops on the other) under the default `CENTER`
//!   alignment — porting it surfaced and fixed a real bug, see *Divergence*
//!   below.
//! - `'FittedBox with no child'` — [`fitted_box_with_no_child_measures_zero`].
//! - `'Child can be aligned multiple ways in a row'` (4 of 5 root-swap
//!   legs: the LTR-resolved-alignment leg — ported directly as a physical
//!   `Alignment::BOTTOM_RIGHT`, since `FittedBox` takes no
//!   `AlignmentDirectional`/`TextDirection` to resolve from — plus the
//!   direction-agnostic `center`/`change-size`/`change-fit` legs; the
//!   RTL-resolved-alignment leg is out of scope, see below) —
//!   [`fitted_box_alignment_and_fit_changes_relayout_across_pump_widget_swaps`].
//! - `'Big child into small fitted box - hit testing'` —
//!   [`big_child_into_small_fitted_box_hit_test`].
//! - `'Can set and update clipBehavior'` —
//!   [`fitted_box_can_set_and_update_clip_behavior`].
//! - `'BoxFit.scaleDown matches size of child'` (both legs) —
//!   [`box_fit_scale_down_matches_size_of_child`].
//! - `'Switching to and from BoxFit.scaleDown causes relayout'` (all 3
//!   root-swap legs) —
//!   [`switching_to_and_from_scale_down_causes_relayout`].
//! - `'FittedBox without child does not throw'` —
//!   [`fitted_box_without_child_does_not_throw`].
//! - `'FittedBox with zero size child does not throw'` (both legs) —
//!   [`fitted_box_with_zero_size_child_does_not_throw`].
//!
//! Out of scope (4 cases): `'FittedBox layers - contain'`, `'FittedBox
//! layers - cover - horizontal'`, `'FittedBox layers - cover - vertical'`,
//! `'FittedBox layers - none - clip'` — all four assert `getLayers()`
//! (`TransformLayer`/`ClipRectLayer`/`OffsetLayer` composition-layer
//! counts); FLUI's headless harness has no compositing-layer-tree capture.
//!
//! Framework gap (1 leg, not a full case — folded into the ported
//! `'Child can be aligned multiple ways in a row'` above rather than double
//! -counted): the "align RTL" leg (`AlignmentDirectional.bottomEnd` under
//! `TextDirection.rtl`) has no port target — `FittedBox`
//! (`crates/flui-widgets/src/layout/fitted_box.rs`) takes only a physical
//! `Alignment`, with no `AlignmentDirectional` constructor or ambient
//! `TextDirection` resolution at all.
//!
//! Denominator: 11 ported + 4 out of scope = 15 (the RTL leg is a partial
//! gap within a ported case, not a 16th case).
//!
//! Widget → render-object mapping: `FittedBox` → `RenderFittedBox`
//! (`crates/flui-objects/src/layout/fitted_box.rs`).
//!
//! **Divergence (real bug found and fixed during this port, not a pre-
//! existing documented gap):** `RenderFittedBox::effective_transform` only
//! composed `translate(destination-alignment-offset) * scale`, omitting
//! Flutter's third term — `translate(-source-crop-offset)`
//! (`RenderFittedBox._updatePaintData`, `proxy_box.dart`). This is a no-op
//! for `Contain`/`Fill`/`ScaleDown` (their `source` is always the *whole*
//! child, so the crop offset is always zero) but silently mis-mapped both
//! paint and hit-testing for `Cover`/`FitWidth`/`FitHeight`/an overflowing
//! `None` whenever the crop is off-center — which `'Child can cover'` (this
//! file) exercises directly under the default `CENTER` alignment. Fixed by
//! adding a cached `source_offset` field (computed the same way as the
//! existing `align_offset`, against the child's own free space rather than
//! the box's) and folding `translate(-source_offset)` into
//! `effective_transform`. Regression-tested at both layers: a new unit test
//! in `RenderFittedBox`'s own module
//! (`effective_transform_accounts_for_a_cropped_cover_sources_offset`,
//! mutation-verified — reverting the fix reproduces the exact wrong point
//! this test caught, `(200, 100)` instead of `(100, 100)`) and this file's
//! `child_can_cover` end-to-end port.
//!
//! New harness primitives: `LaidOut::fitted_box_transform` (the composed
//! matrix `RenderFittedBox::effective_transform` produces, for verifying a
//! child's screen-space position without assuming pure translation); the
//! existing `LaidOut::clip_behavior` gained a `RenderFittedBox` branch.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::layout::BoxFit;
use flui_types::painting::Clip;
use flui_types::{Alignment, Offset};
use flui_widgets::{Center, ConstrainedBox, FittedBox, GestureDetector, SizedBox};

use crate::common::offset;
use crate::harness;

/// Maps `local_point` (a coordinate inside the FittedBox's CHILD) to
/// absolute screen space, using the FittedBox's own absolute offset plus its
/// [`RenderFittedBox::effective_transform`] — the harness analogue of
/// Flutter's `renderBox.localToGlobal(point)` for a scaled/cropped child,
/// where a plain [`common::LaidOut::absolute_offset`] sum (pure translation)
/// would silently ignore the scale.
fn child_point_to_absolute(
    laid: &crate::common::LaidOut,
    fitted_box_id: flui_foundation::RenderId,
    local_point: Offset,
) -> Offset {
    let transform = laid.fitted_box_transform(fitted_box_id);
    let (x, y) = transform.transform_point(local_point.dx, local_point.dy);
    laid.absolute_offset(fitted_box_id) + offset(x.get(), y.get())
}

/// Maps `local_point` (a coordinate inside the FittedBox's OWN box, i.e. not
/// scaled) to absolute screen space — Flutter's `outsideBox.localToGlobal`.
fn box_point_to_absolute(
    laid: &crate::common::LaidOut,
    fitted_box_id: flui_foundation::RenderId,
    local_point: Offset,
) -> Offset {
    laid.absolute_offset(fitted_box_id) + local_point
}

/// Flutter parity: `fitted_box_test.dart` `'Can size according to aspect
/// ratio'` (3.44.0) — a `FittedBox` under a width-only-constrained,
/// aspect-preserving parent sizes to `200×100` (preserving the 2:1 child
/// aspect against the box's tight-200 width), and the default `Contain` fit
/// scales the 100×50 child up to exactly fill it (aspect matches, no crop).
#[test]
fn can_size_according_to_aspect_ratio() {
    let laid = harness::pump_widget(
        Center::new().child(
            SizedBox::width(200.0).child(FittedBox::new().child(SizedBox::new(100.0, 50.0))),
        ),
        harness::screen(),
    );
    let outside_id = laid.find_by_render_type("RenderFittedBox");
    let inside_id = laid.only_child(outside_id);

    assert_eq!(laid.size(outside_id), crate::common::size(200.0, 100.0));
    assert_eq!(laid.size(inside_id), crate::common::size(100.0, 50.0));

    let outside_point = box_point_to_absolute(&laid, outside_id, offset(200.0, 100.0));
    assert_eq!(
        outside_point,
        offset(500.0, 350.0),
        "outsideBox's own bottom-right, in a 300×250-top-left Center, is (500, 350)"
    );
    let inside_point = child_point_to_absolute(&laid, outside_id, offset(100.0, 50.0));
    assert_eq!(
        inside_point, outside_point,
        "the child's own bottom-right (100, 50), scaled 2x with no crop, must land \
         exactly on the box's own bottom-right"
    );
}

/// Flutter parity: `fitted_box_test.dart` `'Can contain child'` (3.44.0) —
/// a `200×200` square box containing a `100×50` child scales uniformly by
/// 2 (limited by the width axis) with no crop.
#[test]
fn can_contain_child() {
    let laid = harness::pump_widget(
        Center::new().child(
            SizedBox::square(200.0).child(FittedBox::new().child(SizedBox::new(100.0, 50.0))),
        ),
        harness::screen(),
    );
    let outside_id = laid.find_by_render_type("RenderFittedBox");
    let inside_id = laid.only_child(outside_id);

    assert_eq!(laid.size(outside_id), crate::common::size(200.0, 200.0));
    assert_eq!(laid.size(inside_id), crate::common::size(100.0, 50.0));

    let outside_point = box_point_to_absolute(&laid, outside_id, offset(200.0, 50.0));
    let inside_point = child_point_to_absolute(&laid, outside_id, offset(100.0, 0.0));
    assert_eq!(inside_point, outside_point);
}

/// Flutter parity: `fitted_box_test.dart` `'Child can cover'` (3.44.0) — a
/// `200×200` box covering a `100×50` child scales by 4 (the larger factor,
/// `200/50`), overflowing and cropping the width axis: the visible source
/// is the centered `50×50` slice of the `100`-wide child. This is the case
/// that surfaced the missing `source_offset` term in
/// `RenderFittedBox::effective_transform` — see this file's module doc.
#[test]
fn child_can_cover() {
    let laid = harness::pump_widget(
        Center::new().child(
            SizedBox::square(200.0).child(
                FittedBox::new()
                    .fit(BoxFit::Cover)
                    .child(SizedBox::new(100.0, 50.0)),
            ),
        ),
        harness::screen(),
    );
    let outside_id = laid.find_by_render_type("RenderFittedBox");
    let inside_id = laid.only_child(outside_id);

    assert_eq!(laid.size(outside_id), crate::common::size(200.0, 200.0));
    assert_eq!(laid.size(inside_id), crate::common::size(100.0, 50.0));

    let outside_point = box_point_to_absolute(&laid, outside_id, offset(100.0, 100.0));
    let inside_point = child_point_to_absolute(&laid, outside_id, offset(50.0, 25.0));
    assert_eq!(
        inside_point, outside_point,
        "the cropped source window's own center (50, 25) must map to the box's own \
         center (100, 100) — this is exactly the source_offset term"
    );
}

/// Flutter parity: `fitted_box_test.dart` `'FittedBox with no child'`
/// (3.44.0) — a childless `FittedBox` under `Center`'s loose constraints
/// sizes to `Size.zero`.
#[test]
fn fitted_box_with_no_child_measures_zero() {
    let laid = harness::pump_widget(
        Center::new().child(FittedBox::new().fit(BoxFit::Cover)),
        harness::screen(),
    );
    let id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.size(id), crate::common::size(0.0, 0.0));
}

/// Flutter parity: `fitted_box_test.dart` `'Child can be aligned multiple
/// ways in a row'` (3.44.0) — 4 of the 5 root-swap legs (the RTL-resolved
/// leg is out of scope, see this file's module doc): the LTR-resolved
/// `bottomEnd` (ported as the physical `Alignment::BOTTOM_RIGHT` it
/// resolves to), a `CENTER` alignment, a larger child under the same
/// `CENTER` alignment, and a `Fill` fit. Every leg asserts the same
/// invariant Flutter does: the child's own corner maps, through the
/// FittedBox's transform, to the SAME absolute point its own
/// `localToGlobal`-equivalent box-space computation reaches.
#[test]
fn fitted_box_alignment_and_fit_changes_relayout_across_pump_widget_swaps() {
    fn build(alignment: Alignment, fit: BoxFit, child_size: (f32, f32)) -> Center {
        Center::new().child(
            SizedBox::square(100.0).child(
                FittedBox::new()
                    .fit(fit)
                    .alignment(alignment)
                    .child(SizedBox::new(child_size.0, child_size.1)),
            ),
        )
    }

    // Leg 1 ("change direction" in the oracle — bottomEnd resolved under
    // LTR): scaleDown, 10×10 child already smaller than the 100×100 box, so
    // it keeps its own size and aligns to the bottom-right corner.
    let mut laid = harness::pump_widget(
        build(Alignment::BOTTOM_RIGHT, BoxFit::ScaleDown, (10.0, 10.0)),
        harness::screen(),
    );
    let mut outside_id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.size(outside_id), crate::common::size(100.0, 100.0));
    assert_eq!(
        laid.size(laid.only_child(outside_id)),
        crate::common::size(10.0, 10.0)
    );
    let mut outside_point = box_point_to_absolute(&laid, outside_id, offset(90.0, 90.0));
    let mut inside_point = child_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    assert_eq!(inside_point, outside_point, "leg 1 (bottom-right corner)");
    outside_point = box_point_to_absolute(&laid, outside_id, offset(100.0, 100.0));
    inside_point = child_point_to_absolute(&laid, outside_id, offset(10.0, 10.0));
    assert_eq!(
        inside_point, outside_point,
        "leg 1 (child's own bottom-right corner)"
    );

    // Leg 2 ("change alignment"): CENTER, same 10×10 child.
    laid.pump_widget(build(Alignment::CENTER, BoxFit::ScaleDown, (10.0, 10.0)));
    outside_id = laid.find_by_render_type("RenderFittedBox");
    outside_point = box_point_to_absolute(&laid, outside_id, offset(45.0, 45.0));
    inside_point = child_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    assert_eq!(
        inside_point, outside_point,
        "leg 2 (top-left of the centered child)"
    );

    // Leg 3 ("change size"): CENTER, a wider 30×10 child (still smaller than
    // the box on both axes, so ScaleDown again keeps its own size).
    laid.pump_widget(build(Alignment::CENTER, BoxFit::ScaleDown, (30.0, 10.0)));
    outside_id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(
        laid.size(laid.only_child(outside_id)),
        crate::common::size(30.0, 10.0)
    );
    outside_point = box_point_to_absolute(&laid, outside_id, offset(35.0, 45.0));
    inside_point = child_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    assert_eq!(
        inside_point, outside_point,
        "leg 3 (top-left of the centered wider child)"
    );

    // Leg 4 ("change fit"): Fill, same 30×10 child — non-uniform scale to
    // exactly fill the 100×100 box on both axes.
    laid.pump_widget(build(Alignment::CENTER, BoxFit::Fill, (30.0, 10.0)));
    outside_id = laid.find_by_render_type("RenderFittedBox");
    outside_point = box_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    inside_point = child_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    assert_eq!(
        inside_point, outside_point,
        "leg 4 (top-left, Fill has no free space)"
    );
    outside_point = box_point_to_absolute(&laid, outside_id, offset(100.0, 100.0));
    inside_point = child_point_to_absolute(&laid, outside_id, offset(30.0, 10.0));
    assert_eq!(
        inside_point, outside_point,
        "leg 4 (child's own bottom-right, non-uniformly scaled to the box's)"
    );
}

/// Flutter parity: `fitted_box_test.dart`
/// `'Big child into small fitted box - hit testing'` (3.44.0) — a
/// `1000×1000` child inside a `100×100` `Contain`-fit box scales down by
/// `0.1` with no crop (aspect matches); a tap anywhere in the box must
/// reach the child through the inverse scale.
#[test]
fn big_child_into_small_fitted_box_hit_test() {
    let did_tap = Arc::new(AtomicUsize::new(0));
    let tap_cb = Arc::clone(&did_tap);

    let laid = harness::pump_widget(
        Center::new().child(
            SizedBox::square(100.0).child(
                FittedBox::new().alignment(Alignment::CENTER).child(
                    SizedBox::square(1000.0).child(
                        GestureDetector::new()
                            .behavior(HitTestBehavior::Opaque)
                            .on_tap(move || {
                                tap_cb.fetch_add(1, Ordering::SeqCst);
                            }),
                    ),
                ),
            ),
        ),
        harness::screen(),
    );

    let fitted_box_id = laid.find_by_render_type("RenderFittedBox");
    let box_center = laid.absolute_offset(fitted_box_id) + offset(50.0, 50.0);
    laid.dispatch_pointer_down(box_center.dx.get(), box_center.dy.get());
    laid.dispatch_pointer_up(box_center.dx.get(), box_center.dy.get());

    assert_eq!(
        did_tap.load(Ordering::SeqCst),
        1,
        "a tap at the box's own center must reach the 1000x1000 child through the 0.1 \
         inverse scale"
    );
}

/// Flutter parity: `fitted_box_test.dart` `'Can set and update clipBehavior'`
/// (3.44.0) — `clip_behavior` defaults to `Clip::None` and updates in place
/// across a root-swap (storage only; active clip-painting is a separate,
/// documented pending gap — see `RenderFittedBox`'s module doc).
#[test]
fn fitted_box_can_set_and_update_clip_behavior() {
    let mut laid = harness::pump_widget(
        FittedBox::new().fit(BoxFit::None).child(SizedBox::shrink()),
        harness::screen(),
    );
    let id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.clip_behavior(id), Clip::None);

    laid.pump_widget(
        FittedBox::new()
            .fit(BoxFit::None)
            .clip(Clip::AntiAlias)
            .child(SizedBox::shrink()),
    );
    assert_eq!(laid.clip_behavior(id), Clip::AntiAlias);
}

/// Flutter parity: `fitted_box_test.dart` `'BoxFit.scaleDown matches size of
/// child'` (3.44.0) — a width-only-tight (`200`), height-loose parent with
/// `BoxFit::ScaleDown`: a smaller child keeps its own size and is
/// centered horizontally (leg 1); a bigger child shrinks uniformly to fit,
/// leaving no free space (leg 2).
#[test]
fn box_fit_scale_down_matches_size_of_child() {
    // Leg 1: 100×50 child, smaller than the 200px width — kept at its own
    // size, height 50 (not stretched to fill the tight width).
    let laid = harness::pump_widget(
        Center::new().child(
            SizedBox::width(200.0).child(
                FittedBox::new()
                    .fit(BoxFit::ScaleDown)
                    .child(SizedBox::new(100.0, 50.0)),
            ),
        ),
        harness::screen(),
    );
    let outside_id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.size(outside_id), crate::common::size(200.0, 50.0));
    let outside_point = box_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    let inside_point = child_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    assert_eq!(inside_point - outside_point, offset(50.0, 0.0));

    // Leg 2: 400×200 child, bigger than the 200px width — scaled down by
    // 0.5 to (200, 100), filling the box exactly (no free space).
    let laid = harness::pump_widget(
        Center::new().child(
            SizedBox::width(200.0).child(
                FittedBox::new()
                    .fit(BoxFit::ScaleDown)
                    .child(SizedBox::new(400.0, 200.0)),
            ),
        ),
        harness::screen(),
    );
    let outside_id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.size(outside_id), crate::common::size(200.0, 100.0));
    let outside_point = box_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    let inside_point = child_point_to_absolute(&laid, outside_id, offset(0.0, 0.0));
    assert_eq!(inside_point - outside_point, offset(0.0, 0.0));
}

/// Flutter parity: `fitted_box_test.dart` `'Switching to and from
/// BoxFit.scaleDown causes relayout'` (3.44.0) — the same `100×50` child
/// under a width-tight-200 parent measures height `50` under `ScaleDown`
/// (kept at its own size), `100` under the default `Contain` (stretched to
/// preserve aspect against the tight width), and back to `50` on
/// switching back — proving both root-swaps actually relayout, not just the
/// first.
#[test]
fn switching_to_and_from_scale_down_causes_relayout() {
    fn scale_down() -> Center {
        Center::new().child(
            SizedBox::width(200.0).child(
                FittedBox::new()
                    .fit(BoxFit::ScaleDown)
                    .child(SizedBox::new(100.0, 50.0)),
            ),
        )
    }
    fn contain() -> Center {
        Center::new()
            .child(SizedBox::width(200.0).child(FittedBox::new().child(SizedBox::new(100.0, 50.0))))
    }

    let mut laid = harness::pump_widget(scale_down(), harness::screen());
    let mut outside_id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.size(outside_id).height.get(), 50.0, "leg 1: ScaleDown");

    laid.pump_widget(contain());
    outside_id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(
        laid.size(outside_id).height.get(),
        100.0,
        "leg 2: default Contain"
    );

    laid.pump_widget(scale_down());
    outside_id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(
        laid.size(outside_id).height.get(),
        50.0,
        "leg 3: back to ScaleDown"
    );
}

/// Flutter parity: `fitted_box_test.dart` `'FittedBox without child does not
/// throw'` (3.44.0) — a childless `FittedBox` under a tight `200×200` parent
/// lays out (to the tight size, the childless branch's `incoming.smallest()`)
/// and a tap over it dispatches without panicking.
#[test]
fn fitted_box_without_child_does_not_throw() {
    let laid = harness::pump_widget(
        Center::new().child(SizedBox::new(200.0, 200.0).child(FittedBox::new())),
        harness::screen(),
    );
    let id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.size(id), crate::common::size(200.0, 200.0));

    let center = laid.absolute_offset(id) + offset(100.0, 100.0);
    laid.dispatch_pointer_down(center.dx.get(), center.dy.get());
    laid.dispatch_pointer_up(center.dx.get(), center.dy.get());
}

/// Flutter parity: `fitted_box_test.dart`
/// `'FittedBox with zero size child does not throw'` (3.44.0) — a
/// zero-size (`SizedBox::shrink`) child hits the degenerate-child branch in
/// `perform_layout` under both a tight parent (leg one) and a loose parent
/// (leg two, via `ConstrainedBox`'s max-only bounds), landing on
/// `incoming.smallest()` without panicking in either case.
#[test]
fn fitted_box_with_zero_size_child_does_not_throw() {
    // Leg 1: tight 200×200 parent — smallest() is the tight size itself.
    let laid = harness::pump_widget(
        Center::new().child(
            SizedBox::square(200.0).child(
                FittedBox::new()
                    .fit(BoxFit::ScaleDown)
                    .child(SizedBox::shrink()),
            ),
        ),
        harness::screen(),
    );
    let id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.size(id), crate::common::size(200.0, 200.0));

    // Leg 2: a loose `ConstrainedBox(maxWidth: 200, maxHeight: 200)` parent —
    // smallest() is zero.
    let laid = harness::pump_widget(
        Center::new().child(
            ConstrainedBox::new(flui_rendering::constraints::BoxConstraints::new(
                flui_types::geometry::px(0.0),
                flui_types::geometry::px(200.0),
                flui_types::geometry::px(0.0),
                flui_types::geometry::px(200.0),
            ))
            .child(FittedBox::new().child(SizedBox::shrink())),
        ),
        harness::screen(),
    );
    let id = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(laid.size(id), crate::common::size(0.0, 0.0));
}
