//! Layout offset commit — `position_child` offsets reach `RenderState.offset`.
//!
//! The layout walk builds a transient `Vec<ChildState>` per parent; the
//! offsets `perform_layout` writes via `ctx.position_child` historically
//! died with that stack frame (`RenderState::set_offset` had zero
//! production callers). Paint and hit-test read `RenderState.offset` as
//! the authoritative child position, so without the commit every child
//! would render at the parent origin.
//!
//! These tests pin the commit contract:
//! 1. positioned offsets are persisted after `run_layout`;
//! 2. re-layout overwrites with fresh positions;
//! 3. a child the parent does NOT position keeps its prior offset
//!    (seed-from-state semantics, Flutter `BoxParentData.offset` parity).
//!
//! Refs:
//!   * docs/research/2026-06-10-rendering-design-amendments.md §D9.1

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    objects::{RenderColoredBox, RenderPadding},
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};
use flui_tree::Variable;
use flui_types::{Offset, Size, geometry::px};

/// Reads the committed offset off a node's persistent `RenderState`.
fn state_offset(
    owner: &flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> Offset {
    owner
        .render_tree()
        .get(id)
        .expect("node must be in tree")
        .as_box()
        .expect("node must be Box-protocol")
        .state()
        .offset()
}

// ============================================================================
// 1. Positioned offsets are persisted
// ============================================================================

#[test]
fn run_layout_commits_positioned_offsets_to_render_state() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<
            dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
        >);
    let child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored child insert");

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let mut owner = owner.into_layout();
    owner.run_layout().expect("first-frame run_layout");

    assert_eq!(
        state_offset(&owner, child_id),
        Offset::new(px(5.0), px(5.0)),
        "Padding(5) positions its child at (5,5) via position_child; \
         the layout walk must commit that offset into the child's \
         RenderState.offset, not drop it with the transient ChildState vec",
    );
}

// ============================================================================
// 2. Re-layout overwrites with fresh positions
// ============================================================================

#[test]
fn relayout_overwrites_committed_offset() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<
            dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
        >);
    let child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::blue(40.0, 40.0)))
        .expect("colored child insert");

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let mut owner = owner.into_layout();
    owner.run_layout().expect("frame 1");
    assert_eq!(
        state_offset(&owner, child_id),
        Offset::new(px(5.0), px(5.0))
    );

    // Change padding → re-position on frame 2.
    let mut owner = owner.into_idle();
    {
        let node = owner
            .render_tree_mut()
            .get_mut(padding_id)
            .expect("padding in tree");
        let entry = node.as_box_mut().expect("box entry");
        let padding = entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<RenderPadding>()
            .expect("padding downcast");
        padding.set_padding(flui_types::EdgeInsets::all(px(9.0)));
    }
    owner.mark_needs_layout(padding_id);
    let mut owner = owner.into_layout();
    owner.run_layout().expect("frame 2");

    assert_eq!(
        state_offset(&owner, child_id),
        Offset::new(px(9.0), px(9.0)),
        "frame 2 re-position must overwrite the previously committed offset",
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
struct PositionFirstChildOnly {
    size: Size,
}

impl PositionFirstChildOnly {
    fn new() -> Self {
        Self { size: Size::ZERO }
    }
}

impl flui_foundation::Diagnosticable for PositionFirstChildOnly {}
impl PaintEffectsCapability for PositionFirstChildOnly {}
impl SemanticsCapability for PositionFirstChildOnly {}
impl HotReloadCapability for PositionFirstChildOnly {}

impl RenderBox for PositionFirstChildOnly {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) {
        let constraints = *ctx.constraints();
        let child_count = ctx.child_count();
        for i in 0..child_count {
            let _ = ctx.layout_child(i, constraints);
        }
        if child_count > 0 {
            ctx.position_child(0, Offset::new(px(7.0), px(3.0)));
        }
        self.size = constraints.constrain(Size::new(px(100.0), px(100.0)));
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        false
    }
}

#[test]
fn unpositioned_child_keeps_prior_offset_across_relayout() {
    let mut owner = PipelineOwner::new();
    let parent_id = owner.insert(Box::new(PositionFirstChildOnly::new())
        as Box<
            dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
        >);
    let positioned_id = owner
        .insert_child_render_object(parent_id, Box::new(RenderColoredBox::red(10.0, 10.0)))
        .expect("child 0 insert");
    let unpositioned_id = owner
        .insert_child_render_object(parent_id, Box::new(RenderColoredBox::blue(10.0, 10.0)))
        .expect("child 1 insert");

    owner.set_root_id(Some(parent_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let mut owner = owner.into_layout();
    owner.run_layout().expect("frame 1");

    assert_eq!(
        state_offset(&owner, positioned_id),
        Offset::new(px(7.0), px(3.0))
    );
    assert_eq!(
        state_offset(&owner, unpositioned_id),
        Offset::ZERO,
        "never-positioned child starts at the default zero offset",
    );

    // Simulate an out-of-band offset write (e.g. a future compositor
    // adjustment), then re-layout. The parent still does not position
    // child 1, so the seed must carry the prior value through.
    let mut owner = owner.into_idle();
    owner
        .render_tree()
        .get(unpositioned_id)
        .expect("child 1 in tree")
        .as_box()
        .expect("box entry")
        .state()
        .set_offset(Offset::new(px(11.0), px(13.0)));

    owner.mark_needs_layout(parent_id);
    let mut owner = owner.into_layout();
    owner.run_layout().expect("frame 2");

    assert_eq!(
        state_offset(&owner, positioned_id),
        Offset::new(px(7.0), px(3.0)),
        "positioned child is re-positioned every layout",
    );
    assert_eq!(
        state_offset(&owner, unpositioned_id),
        Offset::new(px(11.0), px(13.0)),
        "unpositioned child must keep its prior RenderState.offset: the \
         per-walk ChildState is seeded from state, so skipping \
         position_child must not reset the offset to zero",
    );
}
