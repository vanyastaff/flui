//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/center_test.dart` line 17
//! Test name: `'Center does not crash at zero area'`
//!
//! Widget → render-object mapping:
//! - `Center` → `RenderCenter`
//!
//! Divergence: Flutter's test uses `tester.view.physicalSize = Size.zero` to
//! shrink the surface to zero area. FLUI uses `harness::screen_of(0.0, 0.0)`
//! to produce tight-zero constraints directly — same geometry contract, no
//! platform-surface manipulation needed in headless tests.
//! Flutter wraps in `Directionality` + uses a `Placeholder` child; FLUI omits
//! both since neither affects the Center's zero-area geometry assertion.

use crate::common::size;
use flui_widgets::Center;

use crate::harness;

/// `Center` on a zero-area surface lays out to `Size::ZERO` without panicking.
///
/// Flutter parity: center_test.dart line 17 — asserts
/// `tester.getSize(find.byType(Center)) == Size.zero`.
#[test]
fn center_on_zero_area_surface_measures_zero() {
    let laid = harness::pump_widget(Center::new(), harness::screen_of(0.0, 0.0));

    let center_id = laid.find_by_render_type("RenderCenter");
    assert_eq!(
        laid.size(center_id),
        size(0.0, 0.0),
        "Center on a zero-area surface must measure 0×0"
    );
}

/// `Center` fills its tight surface when it has no child.
///
/// Flutter parity: center_test.dart implicit behavior — a childless `Center`
/// takes the full space of its tight constraint.
#[test]
fn center_without_child_fills_tight_constraint() {
    let laid = harness::pump_widget(Center::new(), harness::screen());

    let center_id = laid.find_by_render_type("RenderCenter");
    assert_eq!(
        laid.size(center_id),
        size(800.0, 600.0),
        "childless Center must fill its 800×600 tight surface"
    );
}
