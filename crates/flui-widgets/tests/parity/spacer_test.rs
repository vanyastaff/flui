//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/spacer_test.dart`
//! Oracle tests ported:
//! - `'Spacer takes up space.'` (line 9) — a `Spacer` between two 10×10
//!   `SizedBox`es in a 600-tall `Column` occupies 580 px on the main axis.
//! - `'Spacer takes up space proportional to flex.'` (line 24) — two `Spacer`s
//!   of equal flex in an 800-wide `Row` with 10-wide items split the residual
//!   evenly.
//!
//! Widget → render-object mapping:
//! - `Spacer` → `RenderConstrainedBox` (via `Expanded(SizedBox::shrink())`)
//!   inside `RenderFlex` (the `Column`/`Row`)
//!
//! Divergence: Flutter's test uses `tester.getRect(find.byType(Spacer))` for
//! position; FLUI uses `find_by_render_type("RenderConstrainedBox")` on the
//! Spacer's child render object because the type-finder operates on render
//! objects, not widget types.

use crate::common::size;
use crate::harness;
use flui_widgets::prelude::*;
use flui_widgets::{column, row};

/// A default `Spacer` between two 10-tall `SizedBox`es in a 600-tall `Column`
/// fills the remaining 580 px main-axis height.
///
/// Flutter parity: `spacer_test.dart` line 9 — `spacerRect.size == Size(0, 580)`.
#[test]
fn spacer_fills_remaining_height_in_column() {
    let laid = harness::pump_widget(
        Column::new(column![
            SizedBox::new(10.0, 10.0),
            Spacer::new(),
            SizedBox::new(10.0, 10.0),
        ]),
        harness::screen(),
    );

    // The Spacer composes to `Expanded(SizedBox::shrink())`.
    // `RenderFlex` gives the `Expanded` child tight constraints on the main
    // axis: 600 − 10 − 10 = 580 px. `SizedBox::shrink` clamps cross-axis to 0.
    // There are two 10-tall SizedBox nodes and one Spacer node → 3 boxes total.
    let spacer_boxes = laid.find_all_by_render_type("RenderConstrainedBox");
    // The Spacer's RenderConstrainedBox is the one at 580 px tall.
    let spacer_id = spacer_boxes
        .into_iter()
        .find(|&id| laid.size(id).height == flui_types::geometry::px(580.0))
        .expect("a RenderConstrainedBox of height 580 (the Spacer) must exist");

    assert_eq!(
        laid.size(spacer_id),
        size(0.0, 580.0),
        "Spacer in 600-tall Column with two 10-tall siblings must occupy 580 px \
         height and 0 px width (flutter: spacerRect.size == Size(0.0, 580.0))"
    );
}

/// Two `Spacer(flex=1)`s in an 800-wide `Row` with four 10-wide `SizedBox`
/// children each receive half the remaining space (≈ 390 px).
///
/// Flutter parity: `spacer_test.dart` line 24 — `spacer1Rect.size.width ≈ 93.8`
/// in a row with 8 children totalling 800 px. FLUI uses a simpler layout:
/// two spacers + two 10-px items in an 800-wide row, leaving 780 px for the
/// two equal spacers → each spacer is 390 px wide.
///
/// Geometry: 800 − 10 − 10 = 780 remaining, split equally → 390 px each.
#[test]
fn two_equal_spacers_split_remaining_width_evenly() {
    let laid = harness::pump_widget(
        Row::new(row![
            SizedBox::new(10.0, 10.0),
            Spacer::new(),
            Spacer::new(),
            SizedBox::new(10.0, 10.0),
        ]),
        harness::screen(),
    );

    let spacer_boxes = laid.find_all_by_render_type("RenderConstrainedBox");
    let spacer_widths: Vec<_> = spacer_boxes
        .iter()
        .map(|&id| laid.size(id).width)
        .filter(|&w| w == flui_types::geometry::px(390.0))
        .collect();

    assert_eq!(
        spacer_widths.len(),
        2,
        "two equal Spacer(flex=1)s must each be 390 px wide \
         (800 − 10 − 10 = 780, split 50/50)"
    );
}
