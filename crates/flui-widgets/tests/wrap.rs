//! Layout tests for the [`Wrap`] widget — verifies run-building, wrapping,
//! spacing, and alignment.

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

// ── Reading direction ─────────────────────────────────────────────────────────

/// Each axis follows its own direction: the reading direction flips the
/// horizontal axis and `vertical_direction` the vertical one, whichever of
/// them is a wrap's main axis. Every row after the first is a rebuild of the
/// same wrap, so a change of either direction alone lays it out again.
pub(crate) fn wrap_axes_follow_the_reading_and_vertical_directions() {
    use flui_foundation::TextDirection;
    use flui_foundation::geometry::Axis;
    use flui_widgets::{Directionality, VerticalDirection};

    // Three 40×40 boxes in a 100-wide (or 100-tall) space: two in the first
    // run, one in the second, in an 80×80 wrap.
    let view = |axis, text, vertical| {
        Directionality::new(
            text,
            Wrap::new(vec![
                SizedBox::new(40.0, 40.0).boxed(),
                SizedBox::new(40.0, 40.0).boxed(),
                SizedBox::new(40.0, 40.0).boxed(),
            ])
            .direction(axis)
            .vertical_direction(vertical),
        )
    };
    use Axis::{Horizontal, Vertical};
    use TextDirection::{Ltr, Rtl};
    use VerticalDirection::{Down, Up};
    let rows = [
        (Horizontal, Ltr, Up, [(0.0, 40.0), (40.0, 40.0), (0.0, 0.0)]),
        (
            Horizontal,
            Ltr,
            Down,
            [(0.0, 0.0), (40.0, 0.0), (0.0, 40.0)],
        ),
        (
            Horizontal,
            Rtl,
            Up,
            [(40.0, 40.0), (0.0, 40.0), (40.0, 0.0)],
        ),
        (Vertical, Rtl, Down, [(40.0, 0.0), (40.0, 40.0), (0.0, 0.0)]),
        (Vertical, Ltr, Down, [(0.0, 0.0), (0.0, 40.0), (40.0, 0.0)]),
        (Vertical, Ltr, Up, [(0.0, 40.0), (0.0, 0.0), (40.0, 40.0)]),
        (Vertical, Rtl, Up, [(40.0, 40.0), (40.0, 0.0), (0.0, 40.0)]),
    ];
    let (axis, text, vertical, _) = rows[0];
    let mut laid = lay_out(view(axis, text, vertical), loose(100.0));
    for (index, (axis, text, vertical, expected)) in rows.into_iter().enumerate() {
        if index > 0 {
            laid.pump_widget(view(axis, text, vertical));
        }
        let root = laid.root();
        let offsets: Vec<_> = (0..3)
            .map(|child| laid.offset(laid.child(root, child)))
            .collect();
        let expected: Vec<_> = expected.iter().map(|&(x, y)| offset(x, y)).collect();
        assert_eq!(offsets, expected, "{axis:?} wrap, {text:?}, {vertical:?}");
        assert_eq!(
            laid.size(root),
            size(80.0, 80.0),
            "{axis:?} wrap, {text:?}, {vertical:?}"
        );
    }
}

/// In a right-to-left subtree `Start` and `End` name the reading edges, and a
/// lone child under `SpaceBetween` sits at the start, as it does left to right.
pub(crate) fn wrap_alignment_names_the_reading_edges() {
    use crate::common::tight;
    use flui_foundation::TextDirection;
    use flui_widgets::{Directionality, WrapAlignment};

    for (text, alignment, x) in [
        (TextDirection::Rtl, WrapAlignment::Start, 60.0),
        (TextDirection::Rtl, WrapAlignment::End, 0.0),
        (TextDirection::Rtl, WrapAlignment::Center, 30.0),
        (TextDirection::Rtl, WrapAlignment::SpaceBetween, 60.0),
        (TextDirection::Ltr, WrapAlignment::SpaceBetween, 0.0),
        (TextDirection::Ltr, WrapAlignment::End, 60.0),
    ] {
        let laid = lay_out(
            Directionality::new(
                text,
                Wrap::new(vec![SizedBox::new(40.0, 40.0).boxed()]).alignment(alignment),
            ),
            tight(100.0, 100.0),
        );
        let child = laid.child(laid.root(), 0);
        assert_eq!(
            laid.offset(child),
            offset(x, 0.0),
            "{text:?}, {alignment:?}"
        );
    }
}

/// A wrap in a right-to-left subtree starts each run at the right, and
/// follows the ambient direction when it changes.
///
/// Red-check: never hand the render object the ambient direction (every
/// run starts at the left).
pub(crate) fn wrap_starts_its_runs_at_the_reading_edge() {
    use flui_foundation::TextDirection;
    use flui_widgets::Directionality;

    let view = |direction| {
        Directionality::new(
            direction,
            Wrap::new(vec![
                SizedBox::new(40.0, 40.0).boxed(),
                SizedBox::new(40.0, 40.0).boxed(),
                SizedBox::new(40.0, 40.0).boxed(),
            ]),
        )
    };
    let mut laid = lay_out(view(TextDirection::Rtl), loose(100.0));
    let offsets = |laid: &crate::common::LaidOut| {
        let root = laid.root();
        (0..3)
            .map(|index| laid.offset(laid.child(root, index)))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        offsets(&laid),
        [offset(40.0, 0.0), offset(0.0, 0.0), offset(40.0, 40.0)],
        "right to left: each run starts at the right"
    );
    laid.pump_widget(view(TextDirection::Ltr));
    assert_eq!(
        offsets(&laid),
        [offset(0.0, 0.0), offset(40.0, 0.0), offset(0.0, 40.0)],
        "a change of direction lays the wrap out again"
    );
}
