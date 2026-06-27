//! Layout-parity tests for the [`Wrap`] widget — verifies that run-building,
//! wrapping, spacing, and alignment match Flutter's `RenderWrap` semantics.

mod common;

use common::{lay_out, loose, offset, size, tight};
use flui_view::ViewExt;
use flui_widgets::{SizedBox, Wrap, WrapAlignment, WrapCrossAlignment};

// ── Run wrapping ──────────────────────────────────────────────────────────────

#[test]
fn wrap_three_boxes_form_two_runs_when_width_is_narrow() {
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

#[test]
fn wrap_single_run_when_all_children_fit() {
    // Two 40×40 boxes both fit in 100px → single run, no wrapping.
    let laid = lay_out(
        Wrap::new(vec![
            SizedBox::new(40.0, 40.0).boxed(),
            SizedBox::new(40.0, 40.0).boxed(),
        ]),
        loose(100.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(80.0, 40.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(40.0, 0.0));
}

// ── Spacing and run_spacing ───────────────────────────────────────────────────

#[test]
fn wrap_spacing_and_run_spacing_insert_gaps() {
    // Three 30×20 boxes, spacing=10, run_spacing=5, loose(100).
    // Run 1: a(30) + gap(10) + b(30) = 70. Next: 70+10+30=110 > 100 → new run.
    // Run 2: c at cross_offset = 20 + 5 = 25.
    // Container: (70, 45).
    let laid = lay_out(
        Wrap::new(vec![
            SizedBox::new(30.0, 20.0).boxed(),
            SizedBox::new(30.0, 20.0).boxed(),
            SizedBox::new(30.0, 20.0).boxed(),
        ])
        .spacing(10.0)
        .run_spacing(5.0),
        loose(100.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(70.0, 45.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    // b is offset by 30 (a) + 10 (spacing).
    assert_eq!(laid.offset(laid.child(root, 1)), offset(40.0, 0.0));
    // c wraps; cross_offset = run-1-cross(20) + run_spacing(5).
    assert_eq!(laid.offset(laid.child(root, 2)), offset(0.0, 25.0));
}

// ── Main-axis alignment ───────────────────────────────────────────────────────

#[test]
fn wrap_center_alignment_centres_children_within_tight_run() {
    // Two 30×20 boxes in a tight-100-wide container, alignment=Center.
    // Run main_extent=60. container_main=100. free=40.
    // Center: leading=20, between=0. a@(20,0), b@(50,0).
    let laid = lay_out(
        Wrap::new(vec![
            SizedBox::new(30.0, 20.0).boxed(),
            SizedBox::new(30.0, 20.0).boxed(),
        ])
        .alignment(WrapAlignment::Center),
        tight(100.0, 1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.offset(laid.child(root, 0)), offset(20.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(50.0, 0.0));
}

#[test]
fn wrap_end_alignment_flushes_children_to_trailing_edge() {
    // Two 30×20 boxes in a tight-100-wide container, alignment=End.
    // Run main_extent=60. free=40. End: leading=40.
    // a@(40,0), b@(70,0).
    let laid = lay_out(
        Wrap::new(vec![
            SizedBox::new(30.0, 20.0).boxed(),
            SizedBox::new(30.0, 20.0).boxed(),
        ])
        .alignment(WrapAlignment::End),
        tight(100.0, 1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.offset(laid.child(root, 0)), offset(40.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(70.0, 0.0));
}

#[test]
fn wrap_space_between_distributes_free_space_evenly_between_items() {
    // Two 30×20 boxes in a tight-100-wide container, alignment=SpaceBetween.
    // free=40, SpaceBetween with 2 items: leading=0, between=40.
    // a@(0,0), b@(0+30+40,0)=(70,0).
    let laid = lay_out(
        Wrap::new(vec![
            SizedBox::new(30.0, 20.0).boxed(),
            SizedBox::new(30.0, 20.0).boxed(),
        ])
        .alignment(WrapAlignment::SpaceBetween),
        tight(100.0, 1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(70.0, 0.0));
}

// ── Cross-axis alignment ──────────────────────────────────────────────────────

#[test]
fn wrap_cross_alignment_center_vertically_centres_short_child() {
    // a=40×40, b=40×10 — same run, run cross extent=40.
    // WrapCrossAlignment::Center: b cross offset = (40−10)/2 = 15.
    let laid = lay_out(
        Wrap::new(vec![
            SizedBox::new(40.0, 40.0).boxed(),
            SizedBox::new(40.0, 10.0).boxed(),
        ])
        .cross_axis_alignment(WrapCrossAlignment::Center),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(
        laid.offset(laid.child(root, 1)),
        offset(40.0, 15.0),
        "short child must be centred in the run's cross extent",
    );
}

#[test]
fn wrap_cross_alignment_end_pins_short_child_to_run_bottom() {
    // a=40×40, b=40×10 — run cross=40.
    // WrapCrossAlignment::End: b cross offset = 40−10 = 30.
    let laid = lay_out(
        Wrap::new(vec![
            SizedBox::new(40.0, 40.0).boxed(),
            SizedBox::new(40.0, 10.0).boxed(),
        ])
        .cross_axis_alignment(WrapCrossAlignment::End),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.offset(laid.child(root, 1)),
        offset(40.0, 30.0),
        "short child must be pinned to the bottom of the run's cross extent",
    );
}

// ── Container sizing ──────────────────────────────────────────────────────────

#[test]
fn wrap_empty_children_returns_smallest_constraint_size() {
    let laid = lay_out(Wrap::new(Vec::<flui_view::BoxedView>::new()), loose(200.0));
    // Smallest within [0,200]×[0,200] = (0,0).
    assert_eq!(laid.size(laid.root()), size(0.0, 0.0));
}

#[test]
fn wrap_tight_constraints_force_container_to_specified_size() {
    let laid = lay_out(
        Wrap::new(vec![SizedBox::new(30.0, 20.0).boxed()]),
        tight(100.0, 80.0),
    );
    // Tight constraints: regardless of run size, container = (100, 80).
    assert_eq!(laid.size(laid.root()), size(100.0, 80.0));
}
