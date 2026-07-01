//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/flex_test.dart`
//! Ported cases:
//! - Line 70: `'Flexible defaults to loose'` — `Flexible(child: SizedBox(100×200))`
//!   inside a `Row` receives loose constraints; its natural 100 px width wins.
//! - Line 84: `"Doesn't overflow because of floating point accumulated error"` —
//!   see `column_no_overflow_fp_test.rs` (ported there to keep file sizes small).
//!
//! Widget → render-object mapping:
//! - `Row` → `RenderFlex` (horizontal, root)
//! - `Flexible` → parent-data only (no render object of its own)
//! - `SizedBox(w, h)` → `RenderConstrainedBox` (child of `RenderFlex`)
//!
//! Divergence: Flutter's test uses `find.byType(SizedBox)` to locate the child;
//! FLUI uses `find_by_render_type("RenderConstrainedBox")` — the type-finder
//! operates on render objects, not widget types, per the documented finder design.
//! The geometry invariant (width == 100.0) is identical.

use crate::common::size;
use flui_widgets::prelude::*;
// `row!` is intentionally absent from the prelude glob to avoid collision with
// `std`; import explicitly per the flui-widgets crate doc.
use flui_widgets::row;

use crate::harness;

/// `Flexible` (loose fit) wrapping a `SizedBox(100×200)` inside a `Row` must
/// let the child take its natural 100 px width.
///
/// Flutter parity: flex_test.dart line 70 — `box.size.width == 100.0`.
/// `Flexible` defaults to `FlexFit::Loose`: the child is given loose
/// constraints over its flex share, so it can be its natural width.
#[test]
fn flexible_defaults_to_loose_child_takes_natural_width() {
    let laid = harness::pump_widget(
        Row::new(row![Flexible::new(SizedBox::new(100.0, 200.0))]),
        harness::screen(),
    );

    // RenderFlex (Row) is the root; RenderConstrainedBox (SizedBox) is its child.
    let constrained_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(constrained_box_id).width,
        flui_types::geometry::px(100.0),
        "Flexible(loose) child SizedBox(100, 200) must retain its natural width of 100 px"
    );
}

/// `Expanded` (tight fit) forces its child to fill the flex share on the main axis.
///
/// Flutter parity: derived from flex_test.dart — `Expanded` is `Flexible`
/// with `FlexFit::Tight`, so the child must equal the full main-axis budget.
/// One `Expanded` child in an 800-wide `Row` must be 800 px wide.
#[test]
fn expanded_fills_available_main_axis_width() {
    let laid = harness::pump_widget(
        Row::new(row![flui_widgets::Expanded::new(SizedBox::shrink())]),
        harness::screen(),
    );

    let constrained_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(constrained_box_id),
        size(800.0, 0.0),
        "Expanded child must fill the full 800 px Row width; height is SizedBox::shrink height (0)"
    );
}
