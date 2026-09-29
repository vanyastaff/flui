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
    testing::{
        box_node, edit_render_object,
        inspect::{first_opacity_alpha, layer_structure},
        tree,
    },
};

/// A `Single`-arity proxy whose `paint_effects` panics while `armed`. Shared
/// by every test in this module that poisons through a descriptor build
/// (as opposed to [`a_panicking_path_clipper_poisons_the_frame_on_both_arms`],
/// which poisons through a walk-resolved clip instead).
///
/// `skip_paint` gates the walk out before `paint_effects` runs at all
/// ([`a_node_gated_out_by_skip_paint_never_builds_its_descriptor`]); every
/// other test leaves it `false`.
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
    let mut owner = PipelineOwner::new();
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
#[test]
fn a_panicking_descriptor_under_a_repaint_poisons_the_frame_in_the_paint_phase() {
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

/// A panicking `PaintClip::PathTarget` resolver -- the walk-resolved clip
/// path a registered owner-lane clipper builds, never the producer itself --
/// poisons the frame on both arms: [`PoisonPhase::LayerUpdate`] under a
/// composited-layer-update mark, [`PoisonPhase::Paint`] under an ordinary
/// repaint.
///
/// Both `run_frame` calls in each sub-case run inside the SAME entered
/// `InteractionLane`: outside an active lane the resolver degrades to the
/// whole box and the registered clipper never runs at all (see
/// `a_path_target_resolves_through_the_active_lane_once`), which would make
/// this test pass for the wrong reason.
#[test]
fn a_panicking_path_clipper_poisons_the_frame_on_both_arms() {
    use std::sync::atomic::AtomicBool;

    /// A `Single`-arity proxy whose own clip is a walk-resolved
    /// `PathTarget`. `paint_effects` itself never panics -- only the
    /// registered clipper does -- so this pins the walk's resolution inside
    /// `own_effect_layers`'s clip arm, not the descriptor build
    /// [`PoisonedDescriptor`] above already covers.
    #[derive(Debug)]
    struct PathClipDescriptor {
        target: flui_interaction::PathClipTarget,
    }

    impl flui_foundation::Diagnosticable for PathClipDescriptor {}

    impl flui_rendering::traits::RenderBox for PathClipDescriptor {
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

        fn paint_effects(&self, size: Size) -> flui_rendering::traits::PaintEffects {
            flui_rendering::traits::PaintEffects::NONE.with_clip(
                flui_rendering::traits::PaintClip::PathTarget {
                    target: self.target,
                    size,
                    behavior: flui_painting::paint::Clip::AntiAlias,
                },
            )
        }
    }

    /// Mounts the fixture with a path clipper registered on `lane`, armed
    /// through `armed`, and runs its first (clean) frame. Must be called
    /// from inside `lane.enter(..)`.
    fn mount(
        lane: &flui_interaction::InteractionLane,
        armed: Arc<AtomicBool>,
    ) -> (PipelineOwner<Idle>, RenderId) {
        let handle = lane.dispatch_handle();
        let target = handle
            .register_path_clipper(move |size| {
                assert!(
                    !armed.load(Ordering::Relaxed),
                    "path clipper: armed, poisoning resolution on purpose",
                );
                let mut path = flui_painting::paint::Path::new();
                path.add_rect(flui_foundation::geometry::Rect::from_origin_size(
                    flui_foundation::geometry::Point::ZERO,
                    size,
                ));
                path
            })
            .expect("register path clipper");

        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(PathClipDescriptor { target })
                        .label("fx")
                        .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                ),
            ),
        );
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
        let fx = registry.get("fx").expect("fx is labelled");

        let (owner, result) = owner.run_frame();
        let first = result
            .expect("first frame paints cleanly")
            .expect("first frame produces a layer tree");
        assert!(
            layer_structure(&first).contains(&"ClipPath"),
            "precondition: the first frame must give the node an effect \
             slot -- a ClipPath layer -- for the layer-update arm to patch",
        );
        (owner, fx)
    }

    // --- Sub-case A: layer-update arm ---
    let lane =
        flui_interaction::InteractionLane::try_new().expect("lane construction should succeed");
    lane.enter(|| {
        let armed = Arc::new(AtomicBool::new(false));
        let (mut owner, fx) = mount(&lane, Arc::clone(&armed));

        armed.store(true, Ordering::Relaxed);
        owner.mark_needs_composited_layer_update(fx);
        let (owner, result) = owner.run_frame();
        drop(owner);
        let observed = format!("{result:?}");
        let Err(flui_rendering::error::RenderError::Poisoned { phase, .. }) = result else {
            panic!(
                "precondition: the armed path clipper must poison this \
                 pass under a layer-update mark; got {observed}"
            );
        };
        assert_eq!(
            phase,
            PoisonPhase::LayerUpdate,
            "a panicking PathTarget resolution during a layer-only patch \
             must surface as the layer-update phase",
        );
    });

    // --- Sub-case B: paint (repaint) arm ---
    let lane =
        flui_interaction::InteractionLane::try_new().expect("lane construction should succeed");
    lane.enter(|| {
        let armed = Arc::new(AtomicBool::new(false));
        let (mut owner, fx) = mount(&lane, Arc::clone(&armed));

        armed.store(true, Ordering::Relaxed);
        owner.mark_needs_paint(fx);
        let (owner, result) = owner.run_frame();
        drop(owner);
        let observed = format!("{result:?}");
        let Err(flui_rendering::error::RenderError::Poisoned { phase, .. }) = result else {
            panic!(
                "precondition: the armed path clipper must poison this \
                 pass under a full repaint; got {observed}"
            );
        };
        assert_eq!(
            phase,
            PoisonPhase::Paint,
            "a panicking PathTarget resolution during an ordinary repaint \
             must surface as the paint phase",
        );
    });
}
