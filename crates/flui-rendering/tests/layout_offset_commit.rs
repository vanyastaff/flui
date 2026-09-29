//! Layout offset commit — `position_child` offsets reach `RenderState.offset`.
//!
//! The layout walk builds a transient `Vec<ChildState>` per parent; the
//! offsets `perform_layout` writes via `ctx.position_child` historically
//! died with that stack frame. Paint and hit-test read `RenderState.offset`
//! as the authoritative child position, so without the commit every child
//! would render at the parent origin.
//!
//! These tests pin the commit contract, expressed via the
//! `flui_rendering::testing` harness at `run_layout` depth (Box, layout-only):
//! 1. positioned offsets are persisted after `run_layout`;
//! 2. re-layout overwrites with fresh positions (`update` + `relayout`);
//! 3. a child the parent does NOT position keeps its prior offset.
//!
//! Refs:
//!   * docs/research/2026-06-10-rendering-design-amendments.md §D9.1

use flui_foundation::geometry::{EdgeInsets, Offset};
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{
    constraints::BoxConstraints,
    testing::{Probe, RenderTester, box_node},
};

/// Loose `0..=200 x 0..=200` root constraints shared by every scenario.
fn constraints() -> BoxConstraints {
    BoxConstraints::new(0.0, 200.0, 0.0, 200.0)
}

// ============================================================================
// 1. Positioned offsets are persisted
// ============================================================================

// ============================================================================
// 2. Re-layout overwrites with fresh positions
// ============================================================================

#[test]
fn relayout_overwrites_committed_offset() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::blue(40.0, 40.0)).label("child")),
    )
    .with_constraints(constraints())
    .run_layout();
    let pad = run.root();
    let child = run.id("child");

    assert_eq!(run.offset(child), Offset::new(5.0, 5.0));

    // Change padding → re-position on the next layout pass.
    run.update::<RenderPadding>(pad, |padding| {
        assert_eq!(
            padding.set_padding(EdgeInsets::all(9.0)),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );
    });
    run.relayout();

    assert_eq!(
        run.offset(child),
        Offset::new(9.0, 9.0),
        "re-position must overwrite the previously committed offset",
    );
}

// ============================================================================
// 3. Unpositioned child keeps its prior offset (seed semantics)
// ============================================================================
