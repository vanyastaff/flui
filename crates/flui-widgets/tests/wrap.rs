//! Layout-parity tests for the [`Wrap`] widget — verifies that run-building,
//! wrapping, spacing, and alignment match Flutter's `RenderWrap` semantics.

use crate::common::{lay_out, loose, offset, size};
use flui_view::ViewExt;
use flui_widgets::{SizedBox, Wrap};

// ── Run wrapping ──────────────────────────────────────────────────────────────

pub(crate) fn wrap_three_boxes_form_two_runs_when_width_is_narrow() {
    // Three 40×40 boxes in a max-100-wide loose constraint.
    // Run 1: box[0] at (0,0), box[1] at (40,0) — both fit (80 ≤ 100).
    // Run 2: box[2] wraps to (0,40).
    //
    // Without wrapping, box[2] would land at (80, 0) and this assertion fails.
    let laid = lay_out(
        Wrap::new(vec![
            SizedBox::new(40.0, 40.0).boxed(),
            SizedBox::new(40.0, 40.0).boxed(),
            SizedBox::new(40.0, 40.0).boxed(),
        ]),
        loose(100.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(80.0, 80.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(40.0, 0.0));
    assert_eq!(
        laid.offset(laid.child(root, 2)),
        offset(0.0, 40.0),
        "third child must wrap to a second row, not overflow the first run",
    );
}

// ── Spacing and run_spacing ───────────────────────────────────────────────────

// ── Main-axis alignment ───────────────────────────────────────────────────────

// ── Cross-axis alignment ──────────────────────────────────────────────────────

// ── Container sizing ──────────────────────────────────────────────────────────
