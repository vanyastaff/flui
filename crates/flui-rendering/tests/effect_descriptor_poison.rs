//! A panic while building a node's own paint-effects descriptor -- in
//! `paint_effects` itself, or in the walk's resolution of a
//! `PaintClip::PathTarget` it reports -- poisons the frame rather than
//! propagating raw: `RenderError::Poisoned` carries a `PoisonPhase` that
//! tells the poison points apart (an ordinary repaint vs. a
//! composited-layer-update patch), and a node the paint walk gates out
//! before painting never builds a descriptor it will not use.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_foundation::RenderId;
use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderFlex, RenderRepaintBoundary};
use flui_rendering::{
    constraints::BoxConstraints,
    error::PoisonPhase,
    pipeline::{Idle, PipelineOwner},
    testing::{box_node, edit_render_object, inspect::first_opacity_alpha, tree},
};

/// A `Single`-arity proxy whose `paint_effects` panics while `armed`, for
/// tests that poison through a descriptor build.
///
/// `skip_paint` gates the walk out before `paint_effects` runs at all.
#[derive(Debug)]
struct PoisonedDescriptor {
    armed: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    alpha: u8,
    skip_paint: bool,
}

impl flui_foundation::Diagnosticable for PoisonedDescriptor {}

impl flui_rendering::traits::RenderBox for PoisonedDescriptor {
    type Arity = flui_foundation::Single;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Single,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_foundation::Single,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> bool {
        false
    }

    fn skip_paint(&self) -> bool {
        self.skip_paint
    }

    fn paint_effects(&self, _size: Size) -> flui_rendering::traits::PaintEffects {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(
            !self.armed.load(Ordering::Relaxed),
            "PoisonedDescriptor: armed, poisoning descriptor build on purpose",
        );
        flui_rendering::traits::PaintEffects::NONE
            .with_opacity(flui_rendering::traits::PaintOpacity::new(self.alpha))
    }
}

/// Mounts a [`PoisonedDescriptor`] as `RenderFlex(row) ->
/// RenderRepaintBoundary -> PoisonedDescriptor("fx") -> RenderColoredBox`,
/// tight at 200x200, and returns the owner (still at frame 0, `run_frame`
/// not yet called) plus `fx`'s id.
fn mount(
    armed: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    alpha: u8,
    skip_paint: bool,
) -> (PipelineOwner<Idle>, RenderId) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderRepaintBoundary::new()).child(
                box_node(PoisonedDescriptor {
                    armed,
                    calls,
                    alpha,
                    skip_paint,
                })
                .label("fx")
                .child(box_node(RenderColoredBox::red(20.0, 20.0))),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let fx = registry.get("fx").expect("fx is labelled");
    (owner, fx)
}

/// The paint-phase twin of the test above: a panic while building the SAME
/// kind of descriptor, reached via an ordinary repaint rather than a
/// layer-only patch, poisons the frame with phase [`PoisonPhase::Paint`] --
/// once -- and a disarmed retry paints cleanly.
pub(crate) fn a_panicking_descriptor_under_a_repaint_poisons_the_frame_in_the_paint_phase() {
    let armed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (owner, fx) = mount(Arc::clone(&armed), Arc::clone(&calls), 128, false);

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame paints cleanly");
    let calls_before_arm = calls.load(Ordering::Relaxed);

    edit_render_object::<PoisonedDescriptor, _, _>(&mut owner, fx, |object| {
        object.armed.store(true, Ordering::Relaxed);
    });
    owner.mark_needs_paint(fx);

    let (mut owner, result) = owner.run_frame();
    let observed = format!("{result:?}");
    let Err(flui_rendering::error::RenderError::Poisoned { phase, .. }) = result else {
        panic!(
            "precondition: the armed descriptor must poison this pass \
             (paint); got {observed}"
        );
    };
    assert_eq!(
        phase,
        PoisonPhase::Paint,
        "a panic while recording an ordinary repaint's own effect layers \
         must surface as the paint phase, not the layer-update phase",
    );
    assert_eq!(
        calls.load(Ordering::Relaxed) - calls_before_arm,
        1,
        "the panicking descriptor must run exactly once for this poisoned \
         pass",
    );

    edit_render_object::<PoisonedDescriptor, _, _>(&mut owner, fx, |object| {
        object.armed.store(false, Ordering::Relaxed);
    });
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("the retry paints cleanly")
        .expect("the retry produces a layer tree");
    drop(owner);

    assert_eq!(
        first_opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(50.0),
        "the disarmed retry must produce a correct, ordinary repaint",
    );
}
