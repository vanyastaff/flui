//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/fitted_box_test.dart` (tag
//! `3.44.0`, 15 cases).
//!
//! Ported cases (15 upstream names, 12 Rust tests — every geometry, clip-
//! storage, and hit-test case. The `getLayers()` layer-count assertions are
//! dropped, but no longer for want of a harness: `LaidOut::layer_kinds` /
//! `LaidOut::layer_tree` report composited output now, and what actually
//! blocks those four cases is `RenderFittedBox`'s missing clip — see "Out of
//! scope" below. FLUI still has no golden-file harness, the separate reason
//! `clip_test.rs` drops paint-pattern assertions):
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
//! `'FittedBox layers - contain'`, `'- cover - horizontal'`, `'- cover -
//! vertical'`, `'- none - clip'` — ported as
//! [`fitted_box_composites_the_layer_shape_each_fit_calls_for`], which
//! carries the per-case table and explains why the raw `getLayers()` lists
//! cannot be compared entry-for-entry (the two frameworks' composited roots
//! differ).
//!
//! These four were out of scope through two successive reasons, both of which
//! turned out to be shallower than the real one. First "no compositing-layer
//! capture", which the harness gained; then "`RenderFittedBox` does not clip",
//! which was true but incomplete — the clip machinery existed, and what
//! actually blocked it was layer *ordering* (see this render object's module
//! doc). With the fit transform and the clip both emitted from
//! `RenderFittedBox::paint`, all four shapes are reachable.
//!
//! Framework gap (1 leg, not a full case — folded into the ported
//! `'Child can be aligned multiple ways in a row'` above rather than double
//! -counted): the "align RTL" leg (`AlignmentDirectional.bottomEnd` under
//! `TextDirection.rtl`) has no port target — `FittedBox`
//! (`crates/flui-widgets/src/layout/fitted_box.rs`) takes only a physical
//! `Alignment`, with no `AlignmentDirectional` constructor or ambient
//! `TextDirection` resolution at all.
//!
//! Denominator: 15 ported + 0 out of scope = 15 (the RTL leg is a partial
//! gap within a ported case, not a 16th case).
//!
//! Widget → render-object mapping: `FittedBox` → `RenderFittedBox`
//! (`crates/flui-objects/src/layout/fitted_box.rs`).
//!
//! **Divergence (real bug found and fixed during this port, not a pre-
//! existing documented gap):** the root cause lived in
//! `flui_types::BoxFit::apply` (`crates/flui-types/src/layout/box.rs`), not
//! in the render object — every variant returned `source: input_size`
//! (never cropping), where Flutter's `applyBoxFit` (`box_fit.dart`) returns
//! a *cropped* source for `Cover` and the cover-like halves of
//! `FitWidth`/`FitHeight`, and caps `None` per axis. With the full-source
//! answer, `RenderFittedBox`'s `translate(-source_offset)` transform term
//! was unreachable dead state (always zero), so paint and hit-testing
//! mis-mapped whenever a real crop should have applied — which
//! `'Child can cover'` (this file) exercises under the default `CENTER`
//! alignment. Fixed by porting the oracle's cropping contract into
//! `BoxFit::apply` itself, which makes `RenderFittedBox::source_offset`
//! genuinely live; the same correction exposed and fixed
//! `has_visual_overflow` (Flutter's `_hasVisualOverflow` asks "was the
//! source cropped", not "does the destination overflow the box").
//! Regression-tested at both layers, mutation-verified by execution:
//! reverting the `source_offset` transform term fails this file's
//! `child_can_cover` at `(500, 300)` instead of `(400, 300)`, and fails the
//! pipeline-driven harness test
//! (`harness_fitted_box_cover_crops_the_source_and_offsets_the_transform`,
//! `crates/flui-objects/tests/render_object_harness.rs`), which reads the
//! nonzero crop offset off a node populated by the real `perform_layout`.
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
use flui_types::{Alignment, Color, Offset};
use flui_widgets::{
    Center, ColoredBox, ConstrainedBox, FittedBox, GestureDetector, RepaintBoundary, SizedBox,
};

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
fn fitted_box_alignment_and_fit_changes_refresh_transform_across_widget_swaps() {
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

/// A cropping fit under a non-`None` clip composites a clip layer, and
/// composites it **outside** the fit transform.
///
/// Flutter parity: `fitted_box_test.dart` `'FittedBox layers - cover -
/// horizontal'` / `'- vertical'` (3.44.0), whose `getLayers()` walk reads
/// `[TransformLayer, ClipRectLayer, TransformLayer, OffsetLayer]` — the
/// leading `TransformLayer` is upstream's render-view root, so the
/// `FittedBox`-attributable part is `ClipRectLayer` wrapping `TransformLayer`.
/// `RenderFittedBox.paint` produces that by wrapping the transformed child
/// paint in `pushClipRect` when `_hasVisualOverflow && clipBehavior !=
/// Clip.none`.
///
/// The absolute chain does not port: FLUI's composited root is an `Offset`
/// layer, not a `Transform`, so the two frameworks differ by one layer before
/// the subject is involved. The *order* is the contract, and it is what this
/// asserts — a clip nested inside the transform would be stated in this box's
/// coordinates but applied against the child's scaled ones, clipping the wrong
/// region.
#[test]
fn a_cropping_fit_clips_outside_the_transform() {
    let mut laid = harness::pump_widget(
        Center::new().child(
            SizedBox::new(100.0, 10.0).child(
                FittedBox::new()
                    .fit(BoxFit::Cover)
                    .clip(Clip::HardEdge)
                    .child(SizedBox::new(10.0, 50.0)),
            ),
        ),
        harness::screen(),
    );
    laid.pump();

    let kinds = laid.layer_kinds();
    let clip_at = kinds.iter().position(|k| *k == "ClipRect");
    let transform_at = kinds.iter().position(|k| *k == "Transform");

    let clip_at = clip_at.unwrap_or_else(|| {
        panic!("BoxFit::Cover crops the source, so Clip::HardEdge must composite a clip layer; got {kinds:?}")
    });
    let transform_at = transform_at
        .unwrap_or_else(|| panic!("the fit transform must still be composited; got {kinds:?}"));
    assert!(
        clip_at < transform_at,
        "the clip must wrap the transform, not sit inside it — a clip stated in \
         this box's coordinates would otherwise be applied against the child's \
         scaled ones; got {kinds:?}"
    );
}

/// The same fit with clipping switched off composites no clip layer, and a
/// fit that does not crop composites none either.
///
/// Flutter parity: `fitted_box_test.dart` `'FittedBox layers - contain'`
/// (3.44.0) reads `[TransformLayer, TransformLayer, OffsetLayer]` — no
/// `ClipRectLayer`, because `BoxFit.contain` never crops, so
/// `_hasVisualOverflow` is false whatever `clipBehavior` says.
///
/// Both legs matter: `Clip::None` guards the behavior half of the condition
/// and a non-cropping fit guards the overflow half, so neither alone can be
/// dropped without the other silently passing.
#[test]
fn no_clip_layer_without_both_a_crop_and_a_clip_behavior() {
    let mut cropping_but_unclipped = harness::pump_widget(
        Center::new().child(
            SizedBox::new(100.0, 10.0).child(
                FittedBox::new()
                    .fit(BoxFit::Cover)
                    .clip(Clip::None)
                    .child(SizedBox::new(10.0, 50.0)),
            ),
        ),
        harness::screen(),
    );
    cropping_but_unclipped.pump();
    assert!(
        !cropping_but_unclipped.layer_kinds().contains(&"ClipRect"),
        "Clip::None must not composite a clip layer even when the fit crops; \
         got {:?}",
        cropping_but_unclipped.layer_kinds()
    );

    let mut clipped_but_not_cropping = harness::pump_widget(
        Center::new().child(
            SizedBox::new(100.0, 10.0).child(
                FittedBox::new()
                    .fit(BoxFit::Contain)
                    .clip(Clip::HardEdge)
                    .child(SizedBox::new(50.0, 50.0)),
            ),
        ),
        harness::screen(),
    );
    clipped_but_not_cropping.pump();
    assert!(
        !clipped_but_not_cropping.layer_kinds().contains(&"ClipRect"),
        "BoxFit::Contain never crops, so there is nothing to clip regardless of \
         clip behavior; got {:?}",
        clipped_but_not_cropping.layer_kinds()
    );
}

/// A zero-area child is neither painted nor hit-testable, even when the box
/// itself has a real size.
///
/// Flutter parity: `RenderFittedBox.paint` opens with
/// `if (child == null || size.isEmpty || child!.size.isEmpty) return;`, and
/// `hitTestChildren` carries the matching
/// `if (size.isEmpty || (child?.size.isEmpty ?? false)) return false;`
/// (`rendering/proxy_box.dart`, 3.44.0).
///
/// The child clause is the one under test, so the box is given a real 200×200
/// size — with an empty box too, the box's own guard would pass this
/// vacuously. A zero-area child is not inert: it still runs its own `paint`,
/// and a fit transform onto or from a zero extent is degenerate, so letting it
/// through paints content at an undefined scale.
///
/// Red-check, stated precisely because the two halves are not symmetric:
/// dropping `self.child_is_empty` from the **paint** guard fails the layer
/// assertion (composited `["Offset", "Transform"]` — the fit transform opened
/// around a child with no area). Dropping it from the **hit-test** guard
/// changes nothing measurable here: a zero-area child fails its own bounds
/// check anyway, so the tap misses either way. That guard is kept for oracle
/// parity and as a cheap early-out, not because this test discriminates it —
/// saying so here rather than letting the tap assertion imply otherwise.
#[test]
fn a_zero_area_child_neither_paints_nor_hit_tests() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let mut laid = harness::pump_widget(
        Center::new().child(
            SizedBox::square(200.0).child(
                FittedBox::new().child(
                    SizedBox::shrink().child(
                        GestureDetector::new()
                            .behavior(HitTestBehavior::Opaque)
                            .on_tap(move || {
                                in_cb.fetch_add(1, Ordering::SeqCst);
                            }),
                    ),
                ),
            ),
        ),
        harness::screen(),
    );

    let fitted = laid.find_by_render_type("RenderFittedBox");
    assert_eq!(
        laid.size(fitted),
        crate::common::size(200.0, 200.0),
        "the box itself must have a real size, so this exercises the child \
         clause rather than the empty-box one"
    );

    laid.pump();
    assert!(
        !laid.layer_kinds().contains(&"Transform"),
        "a zero-area child must not be painted through the fit transform; \
         composited {:?}",
        laid.layer_kinds()
    );

    let centre = box_point_to_absolute(&laid, fitted, offset(100.0, 100.0));
    laid.dispatch_pointer_down(centre.dx.get(), centre.dy.get());
    laid.dispatch_pointer_up(centre.dx.get(), centre.dy.get());
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a child that cannot be painted must not be reachable by a pointer"
    );
}

/// The four upstream `'FittedBox layers'` cases, ported as the layer *shape*
/// each one asserts.
///
/// Flutter source: `fitted_box_test.dart` (3.44.0) `'FittedBox layers -
/// contain'`, `'- cover - horizontal'`, `'- cover - vertical'`, `'- none -
/// clip'`. Its local `getLayers()` walks the single-child container chain from
/// the root and records each layer's type:
///
/// | case | upstream `getLayers()` |
/// |---|---|
/// | contain | `[Transform, Transform, Offset]` |
/// | cover (either axis) | `[Transform, ClipRect, Transform, Offset]` |
/// | none, child overflows | `[Transform, ClipRect, Offset]` |
/// | none, child fits | `[Transform, Offset]` |
///
/// The leading `Transform` is upstream's render-view root; FLUI's composited
/// root is an `Offset` layer instead, so the chains differ by one entry before
/// `FittedBox` is involved and the raw lists cannot be compared. What ports is
/// everything after it — which is where the whole content of these cases lives:
///
/// - `Contain` scales, so it composites a transform and never crops.
/// - `Cover` crops, so `Clip::HardEdge` composites a clip, outside the
///   transform.
/// - `None` neither scales nor rotates: the fit reduces to a pure translation,
///   which is applied as a child offset with **no transform layer at all**
///   (`_paintChildWithTransform` forks on `MatrixUtils.getAsTranslation`) —
///   with a clip iff the child overflows.
///
/// That last row is the one this file could not express until the fit
/// transform and the clip both moved into `RenderFittedBox::paint`; it is why
/// the four cases were out of scope rather than merely unported.
#[test]
fn fitted_box_composites_the_layer_shape_each_fit_calls_for() {
    fn layers(ow: f32, oh: f32, fit: BoxFit, clip: Clip, iw: f32, ih: f32) -> Vec<&'static str> {
        let mut laid = harness::pump_widget(
            Center::new().child(SizedBox::new(ow, oh).child(
                FittedBox::new().fit(fit).clip(clip).child(
                    SizedBox::new(iw, ih).child(
                        RepaintBoundary::new().child(ColoredBox::new(Color::rgb(10, 20, 30))),
                    ),
                ),
            )),
            harness::screen(),
        );
        laid.pump();
        laid.layer_kinds()
    }

    // Every leg below asserts the ABSENCE of some layer, which a subtree that
    // composited nothing at all would satisfy for the wrong reason. The child
    // is a real painted leaf and this pins that it reached the output, so the
    // absence assertions can only pass because the fit did not call for the
    // layer — not because there was nothing to paint.
    for (label, kinds) in [
        (
            "contain",
            layers(100.0, 10.0, BoxFit::Contain, Clip::None, 50.0, 50.0),
        ),
        (
            "cover",
            layers(100.0, 10.0, BoxFit::Cover, Clip::HardEdge, 10.0, 50.0),
        ),
        (
            "none-overflow",
            layers(10.0, 50.0, BoxFit::None, Clip::HardEdge, 100.0, 50.0),
        ),
        (
            "none-fits",
            layers(100.0, 100.0, BoxFit::None, Clip::HardEdge, 10.0, 10.0),
        ),
    ] {
        assert!(
            kinds.contains(&"Picture"),
            "{label}: the child must actually paint, or this test's absence \
             assertions prove nothing; got {kinds:?}"
        );
    }

    // `'FittedBox layers - contain'`: scales, never crops.
    let contain = layers(100.0, 10.0, BoxFit::Contain, Clip::None, 50.0, 50.0);
    assert!(
        contain.contains(&"Transform") && !contain.contains(&"ClipRect"),
        "Contain scales and never crops: a transform, no clip; got {contain:?}"
    );

    // `'- cover - horizontal'` / `'- cover - vertical'`: crop, so clip outside
    // the transform.
    for (label, ow, oh, iw, ih) in [
        ("horizontal", 100.0, 10.0, 10.0, 50.0),
        ("vertical", 10.0, 100.0, 50.0, 10.0),
    ] {
        let cover = layers(ow, oh, BoxFit::Cover, Clip::HardEdge, iw, ih);
        let clip_at = cover.iter().position(|k| *k == "ClipRect");
        let transform_at = cover.iter().position(|k| *k == "Transform");
        assert!(
            matches!((clip_at, transform_at), (Some(c), Some(t)) if c < t),
            "Cover {label} crops, so a clip must wrap the transform; got {cover:?}"
        );
    }

    // `'- none - clip'`, overflowing leg: pure translation, so a clip and NO
    // transform layer.
    let overflowing = layers(10.0, 50.0, BoxFit::None, Clip::HardEdge, 100.0, 50.0);
    assert!(
        overflowing.contains(&"ClipRect") && !overflowing.contains(&"Transform"),
        "None with an overflowing child clips, and its fit is a pure \
         translation that must not composite a transform layer; got {overflowing:?}"
    );

    // `'- none - clip'`, fitting leg: neither.
    let fitting = layers(100.0, 100.0, BoxFit::None, Clip::HardEdge, 10.0, 10.0);
    assert!(
        !fitting.contains(&"ClipRect") && !fitting.contains(&"Transform"),
        "None with a child that fits neither crops nor scales, so it composites \
         neither layer; got {fitting:?}"
    );
}
