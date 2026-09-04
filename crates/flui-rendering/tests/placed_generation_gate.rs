//! A child a layout pass did not lay out is skipped by paint.
//!
//! Every multi-child render object that lays out a *subset* of its children —
//! a lazy sliver's band, anything virtualised — leaves the rest holding an
//! offset and a size from an earlier pass. Painting them there puts content
//! where the tree no longer says anything is.
//!
//! Nothing at the render-object level can prevent it: `PaintCx` exposes
//! neither parent data nor a child's index, so an object cannot tell paint
//! which of its children this pass actually placed. Until now the hazard was
//! closed one layer up, per object, by discipline — the eager grid tracked its
//! own `laid_out_band`, and the lazy slivers rely on the frame evicting
//! out-of-band residents before paint runs (ADR-0017's amendment).
//!
//! It is now structural. A layout stamps its own generation onto the children
//! it laid out, and the paint driver skips any child carrying a different one,
//! so every multi-child object gets the property whether or not it remembered
//! to ask for it.
//!
//! The fixture lays out a subset that SHRINKS between two passes, because that
//! is the only shape where the defect is observable: a child never laid out at
//! all has size zero and paints nothing regardless, so a single-pass test
//! passes with the gate removed. (It did — that version was written first.)

use flui_objects::RenderColoredBox;
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    testing::{Probe, RenderTester, box_node},
    traits::RenderBox,
};
use flui_tree::Variable;
use flui_types::{Offset, Size, geometry::px};

/// Lays out and positions children `0..laid_out`, stacked vertically, and
/// leaves the rest untouched — the shape of any virtualising parent.
#[derive(Debug)]
struct LaysOutFirstN {
    laid_out: usize,
}

impl flui_foundation::Diagnosticable for LaysOutFirstN {}

impl RenderBox for LaysOutFirstN {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let count = ctx.child_count().min(self.laid_out);
        for i in 0..count {
            let size = ctx.layout_child(i, constraints);
            ctx.position_child(i, Offset::new(px(0.0), px(i as f32 * size.height.get())));
        }
        constraints.constrain(Size::new(px(100.0), px(100.0)))
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        // Descends into every child unconditionally — the gate under test must
        // be the pipeline's, not this fixture's own bookkeeping. An object
        // that tracked its own laid-out band would hide the defect. The count is
        // hard-coded because the hit-test context exposes none — and asking
        // the fixture for its own would reintroduce the bookkeeping.
        // Reverse order, first hit wins — the ordinary multi-child shape.
        for index in (0..2usize).rev() {
            if ctx.hit_test_child_at_layout_offset(index) {
                return true;
            }
        }
        // The parent itself does not claim the hit, so the path below reflects
        // only which child answered.
        false
    }
}

#[test]
fn a_child_dropped_from_a_later_layout_pass_stops_painting() {
    let mut run = RenderTester::mount(
        box_node(LaysOutFirstN { laid_out: 2 })
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("kept"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("dropped")),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .with_size(Size::new(px(200.0), px(200.0)))
    .run_frame();

    let root = run.root();
    // Frame one laid both out, so the second has a real size and offset to go
    // stale — without this the test proves nothing, since a child never laid
    // out has size zero and paints nothing regardless.
    let first_frame = run.display_commands();
    assert!(
        first_frame
            .iter()
            .any(|c| c.line.contains("DrawRect") && c.line.contains("#00FF00FF")),
        "the second child must paint in frame one; commands: {first_frame:#?}"
    );

    // Frame two lays out only the first.
    run.update::<LaysOutFirstN>(root, |object| object.laid_out = 1);
    let run = run.run_frame_again();

    let commands = run.display_commands();
    let paints = |colour: &str| {
        commands
            .iter()
            .any(|command| command.line.contains("DrawRect") && command.line.contains(colour))
    };

    assert!(
        paints("#FF0000FF"),
        "the child this pass laid out must still paint; commands: {commands:#?}"
    );
    assert!(
        !paints("#00FF00FF"),
        "a child dropped from this pass must not paint at the offset the \
         previous pass left; commands: {commands:#?}"
    );
}

/// The same child stops being hit-testable, not just invisible.
///
/// Gating paint alone would be worse than gating nothing: a child that no
/// longer appears but still answers a hit is something the user can click and
/// cannot see. Both walks read the same stamp for that reason.
#[test]
fn a_child_dropped_from_a_later_layout_pass_stops_being_hit() {
    let mut run = RenderTester::mount(
        box_node(LaysOutFirstN { laid_out: 2 })
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("kept"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("dropped")),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .run_layout();

    let root = run.root();
    let dropped = run.id("dropped");
    // The second child sits at y = 40..80 after pass one, so a hit there must
    // reach it — otherwise the pass-two assertion proves nothing.
    assert!(
        run.hit(20.0, 60.0).contains(&dropped),
        "precondition: the second child is hit-testable while it is laid out"
    );

    run.update::<LaysOutFirstN>(root, |object| object.laid_out = 1);
    run.relayout();

    assert!(
        !run.hit(20.0, 60.0).contains(&dropped),
        "a child dropped from this pass must not answer a hit at the position \
         the previous pass gave it"
    );
    assert!(
        run.hit(20.0, 20.0).contains(&run.id("kept")),
        "the child this pass laid out is still hit-testable"
    );
}

/// A skipped child's cached paint output is not reused once its invalidation
/// has been dropped.
///
/// The gate skips an unplaced child, so a paint-only update it receives while
/// unplaced never reaches the descent. `run_paint`'s residue scan then clears
/// the queued dirty flag — it has always done that, with a warning, for
/// multi-root and detached subtrees — and the gate makes a third case reach it.
/// Clearing the flag without dropping the cached output is the hazard: when the
/// parent places the child again with unchanged constraints its layout
/// short-circuits and requeues nothing, so the stale capture would be grafted
/// and the update lost for good.
///
/// The child is wrapped in a repaint boundary because only a boundary owns a
/// retained capture; without one there is nothing to go stale and the test
/// would pass either way.
#[test]
fn a_skipped_boundary_repaints_rather_than_grafting_a_stale_capture() {
    use flui_objects::RenderRepaintBoundary;

    let mut run = RenderTester::mount(
        box_node(LaysOutFirstN { laid_out: 2 })
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("kept"))
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("boundary")
                    .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("dropped")),
            ),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .with_size(Size::new(px(200.0), px(200.0)))
    .run_frame();

    let root = run.root();
    let dropped = run.id("dropped");
    let paints = |run: &flui_rendering::testing::FrameRun, colour: &str| {
        run.display_commands()
            .iter()
            .any(|c| c.line.contains("DrawRect") && c.line.contains(colour))
    };
    assert!(
        paints(&run, "#00FF00FF"),
        "frame one paints the green child"
    );

    // Drop it from layout AND repaint it while it is unplaced: the update
    // lands in the residue scan.
    run.update::<LaysOutFirstN>(root, |object| object.laid_out = 1);
    run.update_paint::<RenderColoredBox>(dropped, |object| {
        let _ = object.set_color([0.0, 0.0, 1.0, 1.0]);
    });
    let mut run = run.run_frame_again();
    assert!(
        !paints(&run, "#0000FFFF") && !paints(&run, "#00FF00FF"),
        "while unplaced the child paints nothing"
    );

    // Place it again. Its constraints are unchanged, so its own layout
    // short-circuits and requeues no paint — the capture is all that stands
    // between the user and the update.
    run.update::<LaysOutFirstN>(root, |object| object.laid_out = 2);
    let run = run.run_frame_again();
    assert!(
        paints(&run, "#0000FFFF"),
        "the child must repaint with the colour it was given while unplaced, \
         not graft the capture from before it: {:#?}",
        run.display_commands()
    );
}

/// A stamp from one parent is not accepted by another that reaches the same
/// number.
///
/// The counter is per-parent, so every parent that has laid out N times has
/// issued the number N. A `GlobalKey` relocation makes the collision reachable
/// rather than theoretical: a child stamped `2` by parent A, reparented onto a
/// B that has never laid out, would pass B's first real layout — which also
/// reaches `2` — even though B laid out nothing, and would then paint at A's
/// offset.
///
/// Simulated directly on the state rather than through a relocation, because
/// the property is about the comparison and a relocation test would prove it
/// only for whatever ids that particular tree happened to allocate.
#[test]
fn a_stamp_from_one_parent_is_not_accepted_by_another() {
    let run = RenderTester::mount(
        box_node(LaysOutFirstN { laid_out: 1 })
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .run_layout();

    let parent = run.root();
    let child = run.id("child");
    let generation = run
        .pipeline()
        .render_tree()
        .get(parent)
        .unwrap()
        .layout_generation();

    let node = run.pipeline().render_tree().get(child).unwrap();
    assert!(
        node.was_placed_by(parent, generation),
        "the real parent's own stamp is accepted"
    );
    assert!(
        !node.was_placed_by(child, generation),
        "the SAME generation number from a different parent must be rejected — \
         this is what a GlobalKey reparent onto a parent with an equal counter \
         would look like"
    );
}
