//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/wrap_test.dart` (tag
//! `3.44.0`, 21 cases).
//!
//! Ported cases (11 upstream names, 11 Rust tests — the run-building,
//! main-axis-alignment, run-alignment, spacing, and hit-test geometry is the
//! portable core):
//! - `'Basic Wrap test (LTR)'` (all 6 root-swap legs: default order, `center`/
//!   `end` main-axis alignment, the mixed-height default-`Start`
//!   cross-alignment leg, and `center`/`end` cross-axis alignment) —
//!   [`basic_wrap_ltr_lays_out_rows_and_respects_alignment_and_cross_alignment`].
//! - `'Empty wrap'` — [`empty_wrap_measures_zero`].
//! - `'Wrap alignment (LTR)'` (all 4 `WrapAlignment` variants:
//!   `center`/`spaceBetween`/`spaceAround`/`spaceEvenly`) —
//!   [`wrap_alignment_ltr_distributes_free_main_axis_space`].
//! - `'Wrap runAlignment (DOWN)'` (all 4 `runAlignment` variants, including
//!   the leg-3 cross-extent variation from the taller last child) —
//!   [`wrap_run_alignment_down_distributes_free_cross_axis_space`].
//! - `'Shrink-wrapping Wrap test'` (both legs: ascending and descending child
//!   sizes) — [`shrink_wrapping_wrap_sizes_to_content_under_loose_constraints`].
//! - `'Wrap spacing test'` — [`wrap_spacing_test_stacks_runs_with_run_spacing`].
//! - `'Vertical Wrap test with spacing'` (both legs: vertical direction with
//!   `spacing`+`runSpacing`, then horizontal with the same knobs) —
//!   [`vertical_and_horizontal_wrap_apply_spacing_and_run_spacing`].
//! - `'Hit test children in wrap'` — [`hit_test_children_in_wrap_only_hits_the_last_row_child`].
//! - `'RenderWrap toString control test'` (misleadingly named — the body is
//!   a `getMinIntrinsicWidth` regression check, not a `toString` check) —
//!   [`wrap_vertical_min_intrinsic_width_sums_run_cross_extents_with_spacing`].
//! - `'Spacing with slight overflow'` (the sub-`PRECISION_TOLERANCE`
//!   run-break boundary) — [`spacing_with_slight_overflow_wraps_within_precision_tolerance`].
//! - `'Object exactly matches container width'` (both oracle legs, plus a
//!   non-oracle third leg pinning the exact-fit run-break boundary — two
//!   children exactly filling the width stay on one run) —
//!   [`object_exactly_matches_container_width_avoids_a_spurious_extra_run`].
//!
//! Out of scope (8 cases):
//! - `'Basic Wrap test (RTL)'`, `'Wrap alignment (RTL)'`, `'Wrap runAlignment
//!   (UP)'`, `'Wrap alignment flipped spaceInBetween'` — all four need
//!   `TextDirection::rtl` and/or `VerticalDirection::up`; `RenderWrap` (see
//!   its own module doc) has no `TextDirection`/`VerticalDirection` field at
//!   all and always lays out LTR/TTB.
//! - `'RenderWrap toStringShallow control test'` — asserts
//!   `hasOneLineDescription` against Dart's `toStringShallow()` diagnostics
//!   format; FLUI's `Diagnosticable` has no equivalent single-line-format
//!   contract to assert against.
//! - `'Wrap baseline control test'` — pins an exact `Text` glyph height
//!   against Flutter's deterministic `'FlutterTest'` test font; FLUI's
//!   cosmic-text stack has no Ahem-equivalent deterministic test font (same
//!   reason `parity/text_test.rs` ports every text case as geometry-relative,
//!   never exact-pixel).
//! - `'Horizontal wrap - IntrinsicsHeight'`, `'Vertical wrap -
//!   IntrinsicsWidth'` — both pin an exact pixel sum that includes real
//!   `Text` glyph metrics (`2 * 16 + 40`); same no-deterministic-test-font
//!   reason as the baseline case above.
//!
//! Framework gaps (2 cases, filed under `docs/ROADMAP.md` Cross.H): `'Wrap
//! can set and update clipBehavior'` and `'Visual overflow generates a
//! clip'` — `RenderWrap` (`crates/flui-objects/src/layout/wrap.rs`) has no
//! `clip_behavior` field at all (unlike `RenderFittedBox`, which stores the
//! value even before clip-painting lands); there is nothing to get or set,
//! and no clip is ever applied on overflow.
//!
//! Denominator: 11 ported + 8 out of scope + 2 framework gaps = 21.
//!
//! Widget → render-object mapping: `Wrap` → `RenderWrap`
//! (`crates/flui-objects/src/layout/wrap.rs`).
//!
//! New harness primitive: `LaidOut::intrinsic_dimension`, wrapping
//! `PipelineOwner::box_intrinsic_dimension` (Flutter's `getMinIntrinsicWidth`/
//! `getMaxIntrinsicWidth`/`getMinIntrinsicHeight`/`getMaxIntrinsicHeight`
//! family).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::storage::IntrinsicDimension;
use flui_types::Axis;
use flui_types::geometry::px;
use flui_widgets::{GestureDetector, SizedBox, Wrap, WrapAlignment, WrapCrossAlignment};

use crate::common::{offset, size};
use crate::harness;

/// Flutter parity: `wrap_test.dart` `'Basic Wrap test (LTR)'` (3.44.0) — all
/// 6 root-swap legs, each pumped fresh and read back via the 4 children's
/// (`RenderConstrainedBox`, per `SizedBox`'s widget→render mapping) offsets
/// in child order.
#[test]
fn basic_wrap_ltr_lays_out_rows_and_respects_alignment_and_cross_alignment() {
    fn offsets(wrap: flui_widgets::Wrap) -> [flui_types::Offset; 4] {
        let laid = harness::pump_widget(wrap, harness::screen());
        let wrap_id = laid.current_root();
        std::array::from_fn(|i| laid.offset(laid.child(wrap_id, i)))
    }

    fn boxes(heights: [f32; 4]) -> Wrap {
        Wrap::new(
            heights
                .into_iter()
                .map(|h| flui_view::ViewExt::boxed(SizedBox::new(300.0, h)))
                .collect::<Vec<_>>(),
        )
    }

    // Leg 1: default alignment/cross-alignment — 2 children per row (300+300
    // fits the 800px surface; a 3rd would overflow to 900).
    assert_eq!(
        offsets(boxes([100.0, 100.0, 100.0, 100.0])),
        [
            offset(0.0, 0.0),
            offset(300.0, 0.0),
            offset(0.0, 100.0),
            offset(300.0, 100.0),
        ],
        "leg 1: default Start/Start"
    );

    // Leg 2: alignment=Center — free main-axis space (200px) is halved as
    // leading space in each row.
    assert_eq!(
        offsets(boxes([100.0, 100.0, 100.0, 100.0]).alignment(WrapAlignment::Center)),
        [
            offset(100.0, 0.0),
            offset(400.0, 0.0),
            offset(100.0, 100.0),
            offset(400.0, 100.0),
        ],
        "leg 2: alignment=Center"
    );

    // Leg 3: alignment=End — the full 200px free space leads each row.
    assert_eq!(
        offsets(boxes([100.0, 100.0, 100.0, 100.0]).alignment(WrapAlignment::End)),
        [
            offset(200.0, 0.0),
            offset(500.0, 0.0),
            offset(200.0, 100.0),
            offset(500.0, 100.0),
        ],
        "leg 3: alignment=End"
    );

    // Leg 4: mixed heights [50,100,100,50], default cross-alignment=Start —
    // every child sits at cross offset 0 regardless of its own height.
    assert_eq!(
        offsets(boxes([50.0, 100.0, 100.0, 50.0])),
        [
            offset(0.0, 0.0),
            offset(300.0, 0.0),
            offset(0.0, 100.0),
            offset(300.0, 100.0),
        ],
        "leg 4: mixed heights, default cross Start"
    );

    // Leg 5: crossAxisAlignment=Center — each child bisects its row's cross
    // extent (100px): the 50-tall children land 25px in, the 100-tall ones
    // stay at 0.
    assert_eq!(
        offsets(boxes([50.0, 100.0, 100.0, 50.0]).cross_axis_alignment(WrapCrossAlignment::Center)),
        [
            offset(0.0, 25.0),
            offset(300.0, 0.0),
            offset(0.0, 100.0),
            offset(300.0, 125.0),
        ],
        "leg 5: crossAxisAlignment=Center"
    );

    // Leg 6: crossAxisAlignment=End — the 50-tall children align to the
    // bottom of their row's 100px cross extent.
    assert_eq!(
        offsets(boxes([50.0, 100.0, 100.0, 50.0]).cross_axis_alignment(WrapCrossAlignment::End)),
        [
            offset(0.0, 50.0),
            offset(300.0, 0.0),
            offset(0.0, 100.0),
            offset(300.0, 150.0),
        ],
        "leg 6: crossAxisAlignment=End"
    );
}

/// Flutter parity: `wrap_test.dart` `'Empty wrap'` (3.44.0) — a childless
/// `Wrap` under loose constraints (here, `Center`'s loosened surface) sizes
/// to `Size.zero`.
#[test]
fn empty_wrap_measures_zero() {
    use flui_widgets::Center;

    let empty_children: Vec<flui_view::BoxedView> = Vec::new();
    let laid = harness::pump_widget(
        Center::new().child(Wrap::new(empty_children).alignment(WrapAlignment::Center)),
        harness::screen(),
    );
    let wrap_id = laid.find_by_render_type("RenderWrap");
    assert_eq!(laid.size(wrap_id), size(0.0, 0.0));
}

/// Flutter parity: `wrap_test.dart` `'Wrap alignment (LTR)'` (3.44.0) — all
/// 4 `WrapAlignment` variants distribute the row's free main-axis space
/// differently; the container itself stays forced to the tight 800×600
/// surface regardless (`BoxConstraints` tight bounds win over the smaller
/// run extent).
#[test]
fn wrap_alignment_ltr_distributes_free_main_axis_space() {
    fn offsets_and_size(
        widths: [f32; 3],
        heights: [f32; 3],
        alignment: WrapAlignment,
    ) -> ([flui_types::Offset; 3], flui_types::Size) {
        let children: Vec<_> = widths
            .into_iter()
            .zip(heights)
            .map(|(w, h)| flui_view::ViewExt::boxed(SizedBox::new(w, h)))
            .collect();
        let laid = harness::pump_widget(
            Wrap::new(children).alignment(alignment).spacing(5.0),
            harness::screen(),
        );
        let wrap_id = laid.current_root();
        let offsets = std::array::from_fn(|i| laid.offset(laid.child(wrap_id, i)));
        (offsets, laid.size(wrap_id))
    }

    // Leg 1: Center — single row (100+200+300 + 2*5 spacing = 610 <= 800),
    // free 190px halved as leading space.
    let (offsets, wrap_size) = offsets_and_size(
        [100.0, 200.0, 300.0],
        [10.0, 20.0, 30.0],
        WrapAlignment::Center,
    );
    assert_eq!(wrap_size, size(800.0, 600.0));
    assert_eq!(
        offsets,
        [offset(95.0, 0.0), offset(200.0, 0.0), offset(405.0, 0.0)]
    );

    // Leg 2: SpaceBetween — free 190px splits into 1 gap of 190, plus the
    // mandatory 5px spacing.
    let (offsets, wrap_size) = offsets_and_size(
        [100.0, 200.0, 300.0],
        [10.0, 20.0, 30.0],
        WrapAlignment::SpaceBetween,
    );
    assert_eq!(wrap_size, size(800.0, 600.0));
    assert_eq!(
        offsets,
        [offset(0.0, 0.0), offset(200.0, 0.0), offset(500.0, 0.0)]
    );

    // Leg 3: SpaceAround — widths [100,200,310] (610 main extent), free 180px
    // splits into 3 equal per-item shares (60 each: half leads, half trails).
    let (offsets, wrap_size) = offsets_and_size(
        [100.0, 200.0, 310.0],
        [10.0, 20.0, 30.0],
        WrapAlignment::SpaceAround,
    );
    assert_eq!(wrap_size, size(800.0, 600.0));
    assert_eq!(
        offsets,
        [offset(30.0, 0.0), offset(195.0, 0.0), offset(460.0, 0.0)]
    );

    // Leg 4: SpaceEvenly — same widths, free 180px splits into 4 equal gaps
    // (45 each, including both edges).
    let (offsets, wrap_size) = offsets_and_size(
        [100.0, 200.0, 310.0],
        [10.0, 20.0, 30.0],
        WrapAlignment::SpaceEvenly,
    );
    assert_eq!(wrap_size, size(800.0, 600.0));
    assert_eq!(
        offsets,
        [offset(45.0, 0.0), offset(195.0, 0.0), offset(445.0, 0.0)]
    );
}

/// Flutter parity: `wrap_test.dart` `'Wrap runAlignment (DOWN)'` (3.44.0) —
/// all 4 `WrapAlignment` variants applied to `run_alignment` distribute free
/// CROSS-axis space between the 3 runs 5 children of increasing width
/// produce. Leg 3 (`SpaceAround`) also varies the last child's height
/// (70 instead of 60), changing the third run's cross extent and proving the
/// distribution recomputes from the actual run metrics, not a cached value.
#[test]
fn wrap_run_alignment_down_distributes_free_cross_axis_space() {
    fn offsets_and_size(
        heights: [f32; 5],
        run_alignment: WrapAlignment,
    ) -> ([flui_types::Offset; 5], flui_types::Size) {
        let widths = [100.0, 200.0, 300.0, 400.0, 500.0];
        let children: Vec<_> = widths
            .into_iter()
            .zip(heights)
            .map(|(w, h)| flui_view::ViewExt::boxed(SizedBox::new(w, h)))
            .collect();
        let laid = harness::pump_widget(
            Wrap::new(children)
                .run_alignment(run_alignment)
                .run_spacing(5.0),
            harness::screen(),
        );
        let wrap_id = laid.current_root();
        let offsets = std::array::from_fn(|i| laid.offset(laid.child(wrap_id, i)));
        (offsets, laid.size(wrap_id))
    }

    // Runs (widths sum against the 800px main limit): run0 = children 0-2
    // (main 600, cross 30), run1 = child 3 (main 400, cross 40), run2 = child
    // 4 (main 500, cross 60). total_cross = 30+40+60+2*5 = 140; free = 460.

    // Leg 1: Center — leading = 460/2 = 230.
    let (offsets, wrap_size) =
        offsets_and_size([10.0, 20.0, 30.0, 40.0, 60.0], WrapAlignment::Center);
    assert_eq!(wrap_size, size(800.0, 600.0));
    assert_eq!(
        offsets,
        [
            offset(0.0, 230.0),
            offset(100.0, 230.0),
            offset(300.0, 230.0),
            offset(0.0, 265.0),
            offset(0.0, 310.0),
        ]
    );

    // Leg 2: SpaceBetween — 2 gaps of 460/2 + 5 = 235 each.
    let (offsets, wrap_size) =
        offsets_and_size([10.0, 20.0, 30.0, 40.0, 60.0], WrapAlignment::SpaceBetween);
    assert_eq!(wrap_size, size(800.0, 600.0));
    assert_eq!(
        offsets,
        [
            offset(0.0, 0.0),
            offset(100.0, 0.0),
            offset(300.0, 0.0),
            offset(0.0, 265.0),
            offset(0.0, 540.0),
        ]
    );

    // Leg 3: SpaceAround — last child's height changes to 70, so run2's cross
    // extent is 70 (not 60): total_cross = 30+40+70+10 = 150, free = 450,
    // per_run = 150, leading = 75, between = 155.
    let (offsets, wrap_size) =
        offsets_and_size([10.0, 20.0, 30.0, 40.0, 70.0], WrapAlignment::SpaceAround);
    assert_eq!(wrap_size, size(800.0, 600.0));
    assert_eq!(
        offsets,
        [
            offset(0.0, 75.0),
            offset(100.0, 75.0),
            offset(300.0, 75.0),
            offset(0.0, 260.0),
            offset(0.0, 455.0),
        ]
    );

    // Leg 4: SpaceEvenly — back to height 60 for the last child; 4 equal gaps
    // of 460/4 = 115 each.
    let (offsets, wrap_size) =
        offsets_and_size([10.0, 20.0, 30.0, 40.0, 60.0], WrapAlignment::SpaceEvenly);
    assert_eq!(wrap_size, size(800.0, 600.0));
    assert_eq!(
        offsets,
        [
            offset(0.0, 115.0),
            offset(100.0, 115.0),
            offset(300.0, 115.0),
            offset(0.0, 265.0),
            offset(0.0, 425.0),
        ]
    );
}

/// Flutter parity: `wrap_test.dart` `'Shrink-wrapping Wrap test'` (3.44.0) —
/// a `Wrap` under LOOSE constraints (Flutter's `Align` handing its child the
/// parent's loosened bounds; ported directly as `BoxConstraints::loose`)
/// shrink-wraps to its content instead of filling the surface, with
/// `alignment: End` / `crossAxisAlignment: End` on both legs.
#[test]
fn shrink_wrapping_wrap_sizes_to_content_under_loose_constraints() {
    fn offsets_and_size(
        widths: [f32; 4],
        heights: [f32; 4],
    ) -> (Vec<flui_types::Offset>, flui_types::Size) {
        let children: Vec<_> = widths
            .into_iter()
            .zip(heights)
            .map(|(w, h)| flui_view::ViewExt::boxed(SizedBox::new(w, h)))
            .collect();
        let loose = BoxConstraints::loose(size(800.0, 600.0));
        let laid = harness::pump_widget(
            Wrap::new(children)
                .alignment(WrapAlignment::End)
                .cross_axis_alignment(WrapCrossAlignment::End),
            loose,
        );
        let wrap_id = laid.current_root();
        let offsets = (0..4)
            .map(|i| laid.offset(laid.child(wrap_id, i)))
            .collect();
        (offsets, laid.size(wrap_id))
    }

    // Leg 1: ascending sizes — 2 rows (600 then 400 main extent); the
    // container shrink-wraps to 600×70 (max run main × summed run cross).
    let (offsets, wrap_size) =
        offsets_and_size([100.0, 200.0, 300.0, 400.0], [10.0, 20.0, 30.0, 40.0]);
    assert_eq!(wrap_size, size(600.0, 70.0));
    assert_eq!(
        offsets,
        vec![
            offset(0.0, 20.0),
            offset(100.0, 10.0),
            offset(300.0, 0.0),
            offset(200.0, 30.0),
        ]
    );

    // Leg 2: descending sizes — 700×60.
    let (offsets, wrap_size) =
        offsets_and_size([400.0, 300.0, 200.0, 100.0], [40.0, 30.0, 20.0, 10.0]);
    assert_eq!(wrap_size, size(700.0, 60.0));
    assert_eq!(
        offsets,
        vec![
            offset(0.0, 0.0),
            offset(400.0, 10.0),
            offset(400.0, 40.0),
            offset(600.0, 50.0),
        ]
    );
}

/// Flutter parity: `wrap_test.dart` `'Wrap spacing test'` (3.44.0) — 4
/// same-width (500px) children each force their own row under a
/// loose-800-wide surface (500+500 > 800), so `runSpacing` alone (10px)
/// stacks the 4 rows.
#[test]
fn wrap_spacing_test_stacks_runs_with_run_spacing() {
    let children: Vec<_> = [10.0, 20.0, 30.0, 40.0]
        .into_iter()
        .map(|h| flui_view::ViewExt::boxed(SizedBox::new(500.0, h)))
        .collect();
    let loose = BoxConstraints::loose(size(800.0, 600.0));
    let laid = harness::pump_widget(Wrap::new(children).run_spacing(10.0), loose);
    let wrap_id = laid.current_root();

    assert_eq!(laid.size(wrap_id), size(500.0, 130.0));
    assert_eq!(
        (0..4)
            .map(|i| laid.offset(laid.child(wrap_id, i)))
            .collect::<Vec<_>>(),
        vec![
            offset(0.0, 0.0),
            offset(0.0, 20.0),
            offset(0.0, 50.0),
            offset(0.0, 90.0),
        ]
    );
}

/// Flutter parity: `wrap_test.dart` `'Vertical Wrap test with spacing'`
/// (3.44.0) — leg 1 exercises `direction: Axis::Vertical` (`spacing`
/// controls the gap between columns' items, `runSpacing` between columns);
/// leg 2 goes back to the default horizontal direction with the same two
/// knobs.
#[test]
fn vertical_and_horizontal_wrap_apply_spacing_and_run_spacing() {
    let loose = BoxConstraints::loose(size(800.0, 600.0));

    // Leg 1: vertical direction, 6 children widths [10..60] all height 250 —
    // main limit is the 600px height; 3 columns of 2 result.
    let children: Vec<_> = [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]
        .into_iter()
        .map(|w| flui_view::ViewExt::boxed(SizedBox::new(w, 250.0)))
        .collect();
    let laid = harness::pump_widget(
        Wrap::new(children)
            .direction(Axis::Vertical)
            .spacing(10.0)
            .run_spacing(15.0),
        loose,
    );
    let wrap_id = laid.current_root();
    assert_eq!(laid.size(wrap_id), size(150.0, 510.0));
    assert_eq!(
        (0..6)
            .map(|i| laid.offset(laid.child(wrap_id, i)))
            .collect::<Vec<_>>(),
        vec![
            offset(0.0, 0.0),
            offset(0.0, 260.0),
            offset(35.0, 0.0),
            offset(35.0, 260.0),
            offset(90.0, 0.0),
            offset(90.0, 260.0),
        ]
    );

    // Leg 2: default (horizontal) direction, same widths, `spacing: 12,
    // runSpacing: 8` — all 6 fit in a single row (270 main extent <= 800).
    let children: Vec<_> = [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]
        .into_iter()
        .map(|w| flui_view::ViewExt::boxed(SizedBox::new(w, 250.0)))
        .collect();
    let loose = BoxConstraints::loose(size(800.0, 600.0));
    let laid = harness::pump_widget(Wrap::new(children).spacing(12.0).run_spacing(8.0), loose);
    let wrap_id = laid.current_root();
    assert_eq!(laid.size(wrap_id), size(270.0, 250.0));
    assert_eq!(
        (0..6)
            .map(|i| laid.offset(laid.child(wrap_id, i)))
            .collect::<Vec<_>>(),
        vec![
            offset(0.0, 0.0),
            offset(22.0, 0.0),
            offset(54.0, 0.0),
            offset(96.0, 0.0),
            offset(148.0, 0.0),
            offset(210.0, 0.0),
        ]
    );
}

/// Flutter parity: `wrap_test.dart` `'Hit test children in wrap'` (3.44.0) —
/// 5 same-size (200×300) children with `spacing: 10, runSpacing: 15` place
/// the 5th child's run at absolute `(210, 315)`; 4 taps straddle that corner
/// and only the one strictly inside hits.
#[test]
fn hit_test_children_in_wrap_only_hits_the_last_row_child() {
    let hit_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hit_count);

    let plain = || flui_view::ViewExt::boxed(SizedBox::new(200.0, 300.0));
    let children: Vec<flui_view::BoxedView> = vec![
        plain(),
        plain(),
        plain(),
        plain(),
        flui_view::ViewExt::boxed(
            SizedBox::new(200.0, 300.0).child(
                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_tap(move || {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }),
            ),
        ),
    ];

    let laid = harness::pump_widget(
        Wrap::new(children).spacing(10.0).run_spacing(15.0),
        harness::screen(),
    );

    for (x, y) in [(209.0, 314.0), (211.0, 314.0), (209.0, 316.0)] {
        laid.dispatch_pointer_down(x, y);
        laid.dispatch_pointer_up(x, y);
    }
    assert_eq!(
        hit_count.load(Ordering::SeqCst),
        0,
        "the 3 boundary taps outside the last child's (210, 315) top-left corner must miss"
    );

    laid.dispatch_pointer_down(211.0, 316.0);
    laid.dispatch_pointer_up(211.0, 316.0);
    assert_eq!(
        hit_count.load(Ordering::SeqCst),
        1,
        "a tap strictly inside the last child's run must hit exactly once"
    );
}

/// Flutter parity: `wrap_test.dart` `'RenderWrap toString control test'`
/// (3.44.0) — despite the name, the body queries
/// `getMinIntrinsicWidth(600.0)` on a vertical `Wrap` with `runSpacing: 7.0`
/// and 4 tight 500×400 children, expecting `2021.0`: 4 columns of 1 child
/// each (400+400 > 600 breaks every pair), cross (width) sums to
/// `4*500 + 3*7 = 2021`.
#[test]
fn wrap_vertical_min_intrinsic_width_sums_run_cross_extents_with_spacing() {
    let children: Vec<_> =
        std::iter::repeat_with(|| flui_view::ViewExt::boxed(SizedBox::new(500.0, 400.0)))
            .take(4)
            .collect();
    let laid = harness::pump_widget(
        Wrap::new(children)
            .direction(Axis::Vertical)
            .run_spacing(7.0),
        harness::screen(),
    );
    let wrap_id = laid.current_root();

    let min_width = laid.intrinsic_dimension(wrap_id, IntrinsicDimension::MinWidth, 600.0);
    assert_eq!(min_width, 2021.0);
}

/// Flutter parity: `wrap_test.dart` `'Spacing with slight overflow'`
/// (3.44.0) — 3 children of 200px plus a 4th of 171px, `spacing: 10`: the
/// first 3 sum to `620`; adding the 4th would need `620 + 10 + 171 = 801`,
/// 1px over the 800px limit — just past `PRECISION_TOLERANCE` (1e-6), so it
/// wraps to its own row instead of overflowing the first.
#[test]
fn spacing_with_slight_overflow_wraps_within_precision_tolerance() {
    let children: Vec<_> = [200.0, 200.0, 200.0, 171.0]
        .into_iter()
        .map(|w| flui_view::ViewExt::boxed(SizedBox::new(w, 10.0)))
        .collect();
    let laid = harness::pump_widget(
        Wrap::new(children).spacing(10.0).run_spacing(10.0),
        harness::screen(),
    );
    let wrap_id = laid.current_root();

    assert_eq!(laid.size(wrap_id), size(800.0, 600.0));
    assert_eq!(
        (0..4)
            .map(|i| laid.offset(laid.child(wrap_id, i)))
            .collect::<Vec<_>>(),
        vec![
            offset(0.0, 0.0),
            offset(210.0, 0.0),
            offset(420.0, 0.0),
            offset(0.0, 20.0),
        ]
    );
}

/// Flutter parity: `wrap_test.dart` `'Object exactly matches container
/// width'` (3.44.0). Ported directly against a `BoxConstraints` shaped like
/// Flutter's `Column` hands a non-stretched child (loose width bounded to
/// 800, unbounded height) rather than via an actual `Column` widget — the
/// constraint shape is what matters here, not `Column`'s own layout
/// algorithm.
///
/// Legs 1-2 are the oracle's own two pumps (single 800px-wide child; two
/// 800px-wide children forced onto separate rows by `spacing: 10`
/// overflowing a shared row). Neither leg alone exercises the run-break
/// COMPARISON at its exact boundary: leg 1's single child never reaches it
/// at all (`needs_new_run` short-circuits on `run_child_count > 0`, which is
/// false for a run's first child, regardless of what the comparison would
/// say), and leg 2's pair overflows by a clear 10px, nowhere near the
/// boundary. Leg 3 (added here, not in the oracle) closes that gap: two
/// 400px children with zero spacing sum to EXACTLY the 800px main limit —
/// `run_main + child_main + spacing - main_limit == 0`, which must NOT
/// exceed `PRECISION_TOLERANCE` and must NOT start a new row. A sign-flipped
/// or reversed comparison here would wrap the second child onto its own row
/// instead of keeping both in one.
#[test]
fn object_exactly_matches_container_width_avoids_a_spurious_extra_run() {
    let column_child_constraints =
        BoxConstraints::new(px(0.0), px(800.0), px(0.0), px(f32::INFINITY));

    // Leg 1: a single child exactly 800px wide.
    let children = vec![flui_view::ViewExt::boxed(SizedBox::new(800.0, 10.0))];
    let laid = harness::pump_widget(
        Wrap::new(children).spacing(10.0).run_spacing(10.0),
        column_child_constraints,
    );
    let wrap_id = laid.current_root();
    assert_eq!(laid.size(wrap_id), size(800.0, 10.0));
    assert_eq!(laid.offset(laid.child(wrap_id, 0)), offset(0.0, 0.0));

    // Leg 2: two such children — `spacing: 10` would overflow a shared row
    // (800 + 10 + 800 > 800), so each gets its own row.
    let children = vec![
        flui_view::ViewExt::boxed(SizedBox::new(800.0, 10.0)),
        flui_view::ViewExt::boxed(SizedBox::new(800.0, 10.0)),
    ];
    let laid = harness::pump_widget(
        Wrap::new(children).spacing(10.0).run_spacing(10.0),
        column_child_constraints,
    );
    let wrap_id = laid.current_root();
    assert_eq!(laid.size(wrap_id), size(800.0, 30.0));
    assert_eq!(
        (0..2)
            .map(|i| laid.offset(laid.child(wrap_id, i)))
            .collect::<Vec<_>>(),
        vec![offset(0.0, 0.0), offset(0.0, 20.0)]
    );

    // Leg 3 (not in the oracle): two 400px children, zero spacing — the
    // second child's main-axis sum lands EXACTLY on the 800px limit
    // (400 + 0 + 400 - 800 == 0), which must stay in the same row.
    let children = vec![
        flui_view::ViewExt::boxed(SizedBox::new(400.0, 10.0)),
        flui_view::ViewExt::boxed(SizedBox::new(400.0, 10.0)),
    ];
    let laid = harness::pump_widget(Wrap::new(children), column_child_constraints);
    let wrap_id = laid.current_root();
    assert_eq!(
        laid.size(wrap_id),
        size(800.0, 10.0),
        "both children must share one row at the exact-fit boundary"
    );
    assert_eq!(
        (0..2)
            .map(|i| laid.offset(laid.child(wrap_id, i)))
            .collect::<Vec<_>>(),
        vec![offset(0.0, 0.0), offset(400.0, 0.0)]
    );
}
