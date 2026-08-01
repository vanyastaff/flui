//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/rendering/flex_test.dart`, tag
//! `3.44.0`, the `'Flex RTL'` test — the section that sets `flex.direction =
//! Axis.vertical` and drives `crossAxisAlignment` through `start`/`end`
//! while `textDirection` is `rtl`. That test uses `RenderFlex` directly
//! under a tight `800×600` test surface (Flutter's `rendering_tester.dart`
//! `layout()` helper default); this port reproduces the same tight `800×600`
//! constraint on the `Column` *widget* directly (no `Center` wrapper — a
//! `Center` would hand `Column` loose constraints, and its cross axis
//! (width) shrink-wraps to the widest child under a loose bound, collapsing
//! the free space these cases exist to measure).
//!
//! Widget → render-object mapping: `Column` → `RenderFlex` (`direction:
//! Vertical`); `SizedBox(w, h)` → `RenderConstrainedBox`.
//!
//! # Cross.H: flex text-direction gap (Column half)
//!
//! `Row`'s main-axis half of this gap (`RenderFlex._flipMainAxis`) is
//! ported in `row_test.rs`. This file ports the other half:
//! `RenderFlex._flipCrossAxis` only ever triggers for a **vertical** flex
//! (`Column`), consulting the same ambient `Directionality` — a `Column`'s
//! own **main** axis is governed by `VerticalDirection` instead, which FLUI
//! does not model and this port does not touch. See `row_test.rs`'s Cross.H
//! note for the shared root cause and the "no textDirection assertion"
//! divergence, not repeated here.

use flui_types::typography::TextDirection;
use flui_view::ViewExt;
use flui_widgets::prelude::*;

use crate::common::{size, tight};
use crate::harness;

/// `CrossAxisAlignment::Start` under the default (no ambient
/// `Directionality`) `Ltr` direction aligns every child flush to the left
/// edge — `dx == 0`, regardless of how much free cross-axis space is
/// available.
///
/// Flutter parity: `rendering/flex_test.dart` `'Flex RTL'`, the
/// `flex.direction = Axis.vertical` / `crossAxisAlignment = start` /
/// `textDirection = ltr` combination (reached later in the same test via
/// `flex.textDirection = TextDirection.ltr` then `crossAxisAlignment =
/// start`) — `box1`/`box2`/`box3` all land at `dx: 0.0`.
#[test]
fn column_cross_axis_alignment_start_ltr_aligns_children_to_the_left_edge() {
    let laid = harness::pump_widget(
        Column::new(vec![
            SizedBox::new(100.0, 100.0).boxed(),
            SizedBox::new(100.0, 100.0).boxed(),
            SizedBox::new(100.0, 100.0).boxed(),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Start),
        tight(800.0, 600.0),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(laid.size(flex_id), size(800.0, 600.0));
    for i in 0..3 {
        assert_eq!(laid.size(laid.child(flex_id, i)), size(100.0, 100.0));
        assert_eq!(
            laid.offset(laid.child(flex_id, i)).dx,
            flui_types::geometry::px(0.0),
            "child {i} must be flush against the left edge under Ltr"
        );
    }
}

/// `CrossAxisAlignment::End` under `Ltr` aligns every child flush to the
/// **right** edge — the mirror image of `Start` above.
///
/// Flutter parity: `rendering/flex_test.dart` `'Flex RTL'`, the same
/// vertical/`Ltr` block's `crossAxisAlignment = end` step —
/// `box1`/`box2`/`box3` all land at `dx: 700.0` (`800 − 100`).
#[test]
fn column_cross_axis_alignment_end_ltr_aligns_children_to_the_right_edge() {
    let laid = harness::pump_widget(
        Column::new(vec![
            SizedBox::new(100.0, 100.0).boxed(),
            SizedBox::new(100.0, 100.0).boxed(),
            SizedBox::new(100.0, 100.0).boxed(),
        ])
        .cross_axis_alignment(CrossAxisAlignment::End),
        tight(800.0, 600.0),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(laid.size(flex_id), size(800.0, 600.0));
    for i in 0..3 {
        assert_eq!(laid.size(laid.child(flex_id, i)), size(100.0, 100.0));
        assert_eq!(
            laid.offset(laid.child(flex_id, i)).dx,
            flui_types::geometry::px(700.0),
            "child {i} must be flush against the right edge under Ltr"
        );
    }
}

/// `CrossAxisAlignment::Start` under an **Rtl** `Directionality` flips a
/// `Column`'s cross axis: `Start` now means the **right** edge, not the
/// left. `RenderFlex._flipCrossAxis` only swaps which physical edge each
/// child's own offset is measured from — it never reorders children (unlike
/// `Row`'s main-axis flip), so every child still shifts by the same amount.
///
/// Flutter parity: `rendering/flex_test.dart` `'Flex RTL'` —
/// `flex.direction = Axis.vertical; // and main=start, cross=start, up,
/// rtl'` step: `box1`/`box2`/`box3` all land at `dx: 700.0`.
#[test]
fn column_cross_axis_alignment_start_rtl_aligns_children_to_the_right_edge() {
    let laid = harness::pump_widget(
        Directionality::new(
            TextDirection::Rtl,
            Column::new(vec![
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Start),
        ),
        tight(800.0, 600.0),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(laid.size(flex_id), size(800.0, 600.0));
    for i in 0..3 {
        assert_eq!(laid.size(laid.child(flex_id, i)), size(100.0, 100.0));
        assert_eq!(
            laid.offset(laid.child(flex_id, i)).dx,
            flui_types::geometry::px(700.0),
            "child {i} must flip to the right edge under CrossAxisAlignment::Start + Rtl"
        );
    }
}

/// `CrossAxisAlignment::End` under an **Rtl** `Directionality` flips to the
/// **left** edge — the mirror of `Start` above, and the mirror of `End`'s
/// own `Ltr` behavior.
///
/// Flutter parity: `rendering/flex_test.dart` `'Flex RTL'` — the
/// `crossAxisAlignment = end` step immediately after: `box1`/`box2`/`box3`
/// all land at `dx: 0.0`.
#[test]
fn column_cross_axis_alignment_end_rtl_aligns_children_to_the_left_edge() {
    let laid = harness::pump_widget(
        Directionality::new(
            TextDirection::Rtl,
            Column::new(vec![
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
            ])
            .cross_axis_alignment(CrossAxisAlignment::End),
        ),
        tight(800.0, 600.0),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(laid.size(flex_id), size(800.0, 600.0));
    for i in 0..3 {
        assert_eq!(laid.size(laid.child(flex_id, i)), size(100.0, 100.0));
        assert_eq!(
            laid.offset(laid.child(flex_id, i)).dx,
            flui_types::geometry::px(0.0),
            "child {i} must flip to the left edge under CrossAxisAlignment::End + Rtl"
        );
    }
}

/// A `Column`'s own **main** axis (vertical) never consults `Directionality`
/// — only `VerticalDirection` (which FLUI does not model) governs it, per
/// Flutter's `RenderFlex._flipMainAxis`. `MainAxisAlignment::Start` under
/// `Rtl` must therefore position children exactly as it does with no
/// ambient direction at all: sequentially from the top, in list order.
///
/// Flutter parity: `rendering/flex_test.dart` `'Flex RTL'`'s own `direction
/// = Axis.vertical` block never varies main-axis child order by
/// `textDirection` — only `verticalDirection` does (the `up`/`down` toggle
/// later in the same test), which this port does not exercise.
#[test]
fn column_main_axis_alignment_start_rtl_does_not_flip_the_vertical_order() {
    let laid = harness::pump_widget(
        Directionality::new(
            TextDirection::Rtl,
            Column::new(vec![
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
            ]),
        ),
        tight(800.0, 600.0),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    // Only the main (vertical) axis is under test here — cross alignment
    // stays at its `Center` default, so `dx` is not asserted.
    assert_eq!(
        laid.offset(laid.child(flex_id, 0)).dy,
        flui_types::geometry::px(0.0)
    );
    assert_eq!(
        laid.offset(laid.child(flex_id, 1)).dy,
        flui_types::geometry::px(100.0)
    );
    assert_eq!(
        laid.offset(laid.child(flex_id, 2)).dy,
        flui_types::geometry::px(200.0)
    );
}
