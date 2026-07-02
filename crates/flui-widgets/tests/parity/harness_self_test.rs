//! Self-verification: confirms `pump_widget` + `find_by_render_type` + `size`
//! agree before any parity port relies on them.
//!
//! Mirrors the role of `flui-rendering/tests/harness_self_test.rs`: a test
//! that would *fail* if the harness primitives silently stopped working, so a
//! broken harness surfaces here rather than as mysterious parity-test failures.

use crate::common::size;
use flui_widgets::{Center, SizedBox};

use crate::harness;

/// Mount → find render node → assert size → swap root → re-assert size.
///
/// This is the canonical self-test from the plan's Phase-1 spec. All three new
/// primitives must agree for any parity assertion built on top to be meaningful.
#[test]
fn pump_widget_find_by_render_type_and_size_are_consistent() {
    // Initial mount: Center(SizedBox(60×40)) inside an 800×600 surface.
    let mut laid = harness::pump_widget(
        Center::new().child(SizedBox::new(60.0, 40.0)),
        harness::screen(),
    );

    // Center fills the surface; SizedBox is 60×40.
    assert_eq!(
        laid.size(laid.root()),
        size(800.0, 600.0),
        "Center must fill the tight screen constraint"
    );
    let constrained_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(constrained_box_id),
        size(60.0, 40.0),
        "SizedBox(60, 40) must lay out to 60×40 inside a Center"
    );

    // Root-swap: replace with SizedBox.expand() — should fill 800×600.
    laid.pump_widget(Center::new().child(SizedBox::expand()));
    let expanded_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(expanded_box_id),
        size(800.0, 600.0),
        "SizedBox::expand() inside Center must stretch to the full screen"
    );
}
