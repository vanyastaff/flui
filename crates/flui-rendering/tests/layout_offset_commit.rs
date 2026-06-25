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

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    objects::{RenderColoredBox, RenderPadding},
    parent_data::BoxParentData,
    testing::{Probe, RenderTester, box_node},
    traits::RenderBox,
};
use flui_tree::Variable;
use flui_types::{EdgeInsets, Offset, Size, geometry::px};

/// Loose `0..=200 x 0..=200` root constraints shared by every scenario.
fn constraints() -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0))
}

// ============================================================================
// 1. Positioned offsets are persisted
// ============================================================================

#[test]
fn run_layout_commits_positioned_offsets_to_render_state() {
    let run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(constraints())
    .run_layout();

    assert_eq!(
        run.offset(run.id("child")),
        Offset::new(px(5.0), px(5.0)),
        "Padding(5) positions its child at (5,5) via position_child; the \
         layout walk must commit that offset into RenderState.offset, not \
         drop it with the transient ChildState vec",
    );
    // The padding self-describes its insets through the layout pass.
    assert!(run.property(run.root(), "padding").is_some());
}

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

    assert_eq!(run.offset(child), Offset::new(px(5.0), px(5.0)));

    // Change padding → re-position on the next layout pass.
    run.update::<RenderPadding>(pad, |padding| {
        padding.set_padding(EdgeInsets::all(px(9.0)));
    });
    run.relayout();

    assert_eq!(
        run.offset(child),
        Offset::new(px(9.0), px(9.0)),
        "re-position must overwrite the previously committed offset",
    );
}

// ============================================================================
// 3. Unpositioned child keeps its prior offset (seed semantics)
// ============================================================================

/// Variable-arity fixture that lays out every child but positions ONLY
/// child 0. Children the parent skips must keep whatever offset their
/// `RenderState` already holds (Flutter parity: `BoxParentData.offset`
/// persists until `positionChild` overwrites it).
#[derive(Debug)]
struct PositionFirstChildOnly;

impl PositionFirstChildOnly {
    fn new() -> Self {
        Self
    }
}

impl flui_foundation::Diagnosticable for PositionFirstChildOnly {}

impl RenderBox for PositionFirstChildOnly {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let child_count = ctx.child_count();
        for i in 0..child_count {
            let _ = ctx.layout_child(i, constraints);
        }
        if child_count > 0 {
            ctx.position_child(0, Offset::new(px(7.0), px(3.0)));
        }
        constraints.constrain(Size::new(px(100.0), px(100.0)))
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        false
    }
}

#[test]
fn unpositioned_child_keeps_prior_offset_across_relayout() {
    let mut run = RenderTester::mount(
        box_node(PositionFirstChildOnly::new())
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("positioned"))
            .child(box_node(RenderColoredBox::blue(10.0, 10.0)).label("unpositioned")),
    )
    .with_constraints(constraints())
    .run_layout();
    let parent = run.root();
    let positioned = run.id("positioned");
    let unpositioned = run.id("unpositioned");

    assert_eq!(run.offset(positioned), Offset::new(px(7.0), px(3.0)));
    assert_eq!(
        run.offset(unpositioned),
        Offset::ZERO,
        "never-positioned child starts at the default zero offset",
    );

    // Simulate an out-of-band offset write (e.g. a future compositor
    // adjustment), then re-layout. The parent still does not position
    // child 1, so the seed must carry the prior value through.
    run.owner()
        .render_tree()
        .get(unpositioned)
        .expect("child 1 in tree")
        .as_box()
        .expect("box entry")
        .state()
        .set_offset(Offset::new(px(11.0), px(13.0)));
    run.owner_mut().mark_needs_layout(parent);
    run.relayout();

    assert_eq!(
        run.offset(positioned),
        Offset::new(px(7.0), px(3.0)),
        "positioned child is re-positioned every layout",
    );
    assert_eq!(
        run.offset(unpositioned),
        Offset::new(px(11.0), px(13.0)),
        "unpositioned child must keep its prior RenderState.offset: the \
         per-walk ChildState is seeded from state, so skipping \
         position_child must not reset the offset to zero",
    );
}
