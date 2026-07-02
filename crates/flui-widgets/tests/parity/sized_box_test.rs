//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/sized_box_test.dart` line 39
//! Test name: `'SizedBox - no child'`
//!
//! Widget → render-object mapping:
//! - `SizedBox(w, h)` → `RenderConstrainedBox`
//! - `SizedBox::expand()` → `RenderConstrainedBox` (tight double-infinite
//!   constraints resolved to the parent's tight constraints)
//! - `Center` → `RenderCenter` (root render object under a tight surface)
//!
//! Divergence: Flutter's test uses a `GlobalKey` to read `currentContext!.size`
//! across `pumpWidget` calls. FLUI uses `find_by_render_type` to locate the
//! `RenderConstrainedBox` instead — the geometry invariant is identical.

use crate::common::size;
use flui_widgets::{Center, SizedBox};

use crate::harness;

/// `SizedBox(100, 100)` inside a `Center` on an 800×600 surface lays out to
/// 100×100; after root-swapping to `SizedBox::expand()` it fills the surface.
///
/// Flutter parity: sized_box_test.dart line 39 — checks that an explicit
/// `SizedBox(100, 100)` measures to exactly that size and that `SizedBox.expand`
/// fills the available space (both cases within a `Center` parent on 800×600).
#[test]
fn sized_box_explicit_then_expand_via_pump_widget() {
    // Initial mount: SizedBox(100×100) inside a Center on the default 800×600 surface.
    let mut laid = harness::pump_widget(
        Center::new().child(SizedBox::new(100.0, 100.0)),
        harness::screen(),
    );

    let constrained_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(constrained_box_id),
        size(100.0, 100.0),
        "SizedBox(100, 100) must measure 100×100"
    );

    // Root-swap: replace with SizedBox::expand() — must fill the 800×600 surface.
    laid.pump_widget(Center::new().child(SizedBox::expand()));

    let expanded_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(expanded_box_id),
        size(800.0, 600.0),
        "SizedBox::expand() inside Center must stretch to 800×600"
    );
}

/// `SizedBox::shrink()` collapses to zero size when centered on a normal surface.
///
/// Flutter parity: sized_box_test.dart line 35 — `SizedBox.shrink()` must
/// report `Size.zero`.
#[test]
fn sized_box_shrink_measures_zero() {
    let laid = harness::pump_widget(Center::new().child(SizedBox::shrink()), harness::screen());

    let constrained_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(constrained_box_id),
        size(0.0, 0.0),
        "SizedBox::shrink() must measure 0×0"
    );
}
