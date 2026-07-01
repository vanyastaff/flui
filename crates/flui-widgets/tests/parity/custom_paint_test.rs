//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/custom_paint_test.dart` line 124
//! Test name: `'CustomPaint sizing'`
//!
//! Widget → render-object mapping:
//! - `CustomPaint` → `RenderCustomPaint`
//!
//! The childless-preferred-size and sizes-to-child cases from the same oracle
//! test are already covered by the widget smoke test
//! `crates/flui-widgets/tests/custom_paint.rs`. This file adds the one oracle
//! edge that smoke test does not exercise: an oversized preferred size is
//! CONSTRAINED by the incoming layout bounds, not passed through raw
//! (`constraints.constrain(preferredSize)`, oracle `computeSizeForNoChild`,
//! `custom_paint.dart` line 579).

use crate::common::size;
use crate::harness;
use flui_widgets::{Center, CustomPaint};

/// `CustomPaint(size: 2000×100)` inside a `Center` on the 800×600 default
/// surface clamps its width to the available 800px while keeping the
/// requested (in-bounds) height of 100px.
///
/// Flutter parity: `custom_paint_test.dart` line 124 — `CustomPaint(key:
/// target, size: const Size(2000.0, 100.0))` measures to `Size(800.0,
/// 100.0)`: the width is clamped (2000 > 800), the height is not (100 < 600).
#[test]
fn custom_paint_oversized_preferred_size_is_constrained_to_available_bounds() {
    let laid = harness::pump_widget(
        Center::new().child(CustomPaint::new().size(size(2000.0, 100.0))),
        harness::screen(),
    );

    let custom_paint_id = laid.find_by_render_type("RenderCustomPaint");
    assert_eq!(
        laid.size(custom_paint_id),
        size(800.0, 100.0),
        "an oversized preferred size must be constrained to the incoming bounds, \
         not passed through unclamped (flutter: CustomPaint sizing, line 124)"
    );
}
