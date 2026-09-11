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
use flui_types::{Size, geometry::px};

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
    type Arity = flui_tree::Single;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_tree::Single,
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
            flui_tree::Single,
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let fx = registry.get("fx").expect("fx is labelled");
    (owner, fx)
}

/// A panic while patching a boundary's own effect layers (a
/// `mark_needs_composited_layer_update` target) poisons the frame with phase
/// [`PoisonPhase::LayerUpdate`] exactly once -- refusing the patch and
/// falling back to a repaint was rejected in `layer_patches_for`'s own doc
/// comment precisely because that would call the same panicking
/// `paint_effects` a second time -- and the queued update survives for the
/// retry, mirroring `poisoned_frame_keeps_the_update` in
/// `retained_boundary_layers.rs`.
#[test]
fn a_panicking_descriptor_under_a_layer_update_poisons_the_frame_once() {
    let armed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (owner, fx) = mount(Arc::clone(&armed), Arc::clone(&calls), 128, false);

    let (mut owner, result) = owner.run_frame();
    let first = result
        .expect("first frame paints cleanly")
        .expect("first frame produces a layer tree");
    assert_eq!(
        first_opacity_alpha(&first).map(|a| (a * 100.0).round()),
        Some(50.0),
        "precondition: the first frame gives the node an effect slot to patch",
    );
    let calls_before_arm = calls.load(Ordering::Relaxed);

    // Arm the panic and change the reported alpha together: a correct retry
    // must see the QUEUED value, distinguishing "the update survived" from
    // "the retry happened to repaint from live state anyway".
    edit_render_object::<PoisonedDescriptor, _, _>(&mut owner, fx, |object| {
        object.armed.store(true, Ordering::Relaxed);
        object.alpha = 64;
    });
    owner.mark_needs_composited_layer_update(fx);

    let (mut owner, result) = owner.run_frame();
    let observed = format!("{result:?}");
    let Err(flui_rendering::error::RenderError::Poisoned {
        render_object,
        phase,
    }) = result
    else {
        panic!(
            "precondition: the armed descriptor must poison this pass \
             (layer-update); got {observed}"
        );
    };
    assert_eq!(
        phase,
        PoisonPhase::LayerUpdate,
        "a panic while patching a boundary's own effect layers must surface \
         as the layer-update phase, not the paint phase",
    );
    assert_eq!(
        render_object,
        core::any::type_name::<PoisonedDescriptor>(),
        "the poisoned render object's debug name must identify the double \
         whose paint_effects panicked",
    );
    assert_eq!(
        calls.load(Ordering::Relaxed) - calls_before_arm,
        1,
        "the panicking descriptor must run exactly once for this poisoned \
         pass; a refuse-and-repaint design would call it a second time and \
         panic again",
    );

    // Retry: disarm and let the queued update land.
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
        Some(25.0),
        "the queued layer update must survive the poisoned frame; clearing \
         the pending flag mid-walk would leave the retry grafting the stale \
         alpha",
    );
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

/// A node the paint walk gates out before painting -- here, one whose
/// `skip_paint()` is `true` -- never builds its own paint-effects
/// descriptor.
#[test]
fn a_node_gated_out_by_skip_paint_never_builds_its_descriptor() {
    let armed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (owner, fx) = mount(Arc::clone(&armed), Arc::clone(&calls), 128, false);

    let (mut owner, result) = owner.run_frame();
    let first = result
        .expect("first frame paints cleanly")
        .expect("first frame produces a layer tree");
    let calls_after_first_frame = calls.load(Ordering::Relaxed);
    assert_eq!(
        calls_after_first_frame, 1,
        "precondition: the first frame builds the descriptor exactly once",
    );
    assert!(
        first_opacity_alpha(&first).is_some(),
        "precondition: the first frame's boundary capture carries fx's \
         Opacity layer",
    );

    // Gate the node out AND arm the panic together: if the walk still built
    // the descriptor for a gated-out node, this frame would poison instead
    // of succeeding.
    edit_render_object::<PoisonedDescriptor, _, _>(&mut owner, fx, |object| {
        object.skip_paint = true;
    });
    armed.store(true, Ordering::Relaxed);
    owner.mark_needs_paint(fx);
    let (owner, result) = owner.run_frame();
    let second = result
        .expect("a gated-out node's panicking descriptor must never run")
        .expect("a gated-out pass still produces a layer tree");
    drop(owner);

    assert_eq!(
        calls.load(Ordering::Relaxed),
        calls_after_first_frame,
        "skip_paint must gate the walk out before it builds the node's own \
         paint-effects descriptor -- a panicking descriptor that still ran \
         would have incremented this count (and panicked)",
    );
    assert_eq!(
        first_opacity_alpha(&second),
        None,
        "fx must contribute NOTHING to this pass's re-capture -- confirming \
         the walk actually reached fx and exited via the skip_paint gate, \
         rather than grafting frame 1's capture (which carried fx's Opacity \
         layer) unmodified",
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
        type Arity = flui_tree::Single;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Single,
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
                flui_tree::Single,
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
                    behavior: flui_types::painting::Clip::AntiAlias,
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
                let mut path = flui_types::painting::Path::new();
                path.add_rect(flui_types::Rect::from_origin_size(
                    flui_types::Point::ZERO,
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
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
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

/// A node the paint walk gates out because it still `needs_layout()` --
/// reached via the phase-typed owner directly (`into_layout().into_compositing()
/// .into_paint()`, skipping `run_layout`/`run_compositing`) rather than
/// `run_frame`: that route is one way to leave a node's `NEEDS_LAYOUT` flag
/// set going into `run_paint` without an actual layout error, since
/// `run_frame` itself aborts before the paint phase ever runs on a layout
/// `Err` (`pipeline/owner/construction.rs`'s `run_frame`). This is the
/// "partial-frame path" `paint_subtree_impl`'s `needs_layout` gate comment
/// names.
///
/// Deliberately does NOT use `PipelineOwner::mark_needs_layout(fx)` for the
/// mark itself -- that ancestor walk (`scheduler.rs`'s `mark_needs_layout`)
/// sets `NEEDS_LAYOUT` on every node it visits INCLUDING the relayout
/// boundary it stops at, and `RenderRepaintBoundary` here is both a repaint
/// AND a relayout boundary. That taints the BOUNDARY's own `needs_layout()`,
/// so when the walk descends into the boundary it gates ITSELF out via the
/// very same check, before ever reaching `fx` -- the boundary's own trivial
/// descriptor never panics either way, so a correct and a broken
/// `paint.rs` would be indistinguishable through that route, and `fx`'s own
/// gate would never actually be exercised. Calling
/// `RenderNode::mark_layout_flag()` directly sets JUST `fx`'s flag, with no
/// ancestor propagation (its own doc: "flag-only, no propagation"),
/// representing what an interrupted layout pass leaves behind -- some nodes
/// stamped `NEEDS_LAYOUT` by the walk, the rest of the tree untouched --
/// more precisely than the full ancestor mark does here. `mark_needs_paint`
/// (a SEPARATE walk, `scheduler.rs`'s `mark_needs_paint`, which touches only
/// `NEEDS_PAINT`) is still needed alongside it so the boundary itself lands
/// in the paint queue and the walk descends into it instead of grafting the
/// retained capture from frame 1 unchanged.
///
/// `take_layer_tree().is_some()` after `run_paint` is the positive proof
/// that the walk genuinely ran this pass (`last_layer_tree` is populated
/// only on `paint_subtree`'s `Ok` path, `pipeline/owner/paint.rs`, and frame
/// 1 already drained it once); `first_opacity_alpha(&second) == None` is the
/// proof `fx` itself was reached and gated -- not merely that the boundary
/// was grafted unchanged (which would still carry frame 1's Opacity layer).
#[test]
fn a_node_gated_out_for_layout_never_builds_its_descriptor() {
    let armed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (owner, fx) = mount(Arc::clone(&armed), Arc::clone(&calls), 128, false);

    let (mut owner, result) = owner.run_frame();
    let first = result
        .expect("first frame paints cleanly")
        .expect("first frame produces a layer tree");
    let calls_after_first_frame = calls.load(Ordering::Relaxed);
    assert_eq!(
        calls_after_first_frame, 1,
        "precondition: the first frame builds the descriptor exactly once",
    );
    assert!(
        first_opacity_alpha(&first).is_some(),
        "precondition: the first frame's boundary capture carries fx's \
         Opacity layer",
    );

    // Arm the panic, then set ONLY `fx`'s own NEEDS_LAYOUT flag (no ancestor
    // walk -- see the doc comment above for why `PipelineOwner::mark_needs_layout`
    // itself is the wrong tool here), and separately mark the boundary paint
    // -dirty so the walk descends into it instead of grafting.
    armed.store(true, Ordering::Relaxed);
    owner
        .render_tree()
        .get(fx)
        .expect("fx is live")
        .mark_layout_flag();
    owner.mark_needs_paint(fx);

    // Drive layout -> compositing -> paint WITHOUT running layout or
    // compositing: the flag just set above is never cleared, so `run_paint`
    // reaches `fx` exactly as a partial frame that failed after layout
    // marked it but before layout committed would.
    let mut owner = owner.into_layout().into_compositing().into_paint();
    let result = owner.run_paint();
    result.expect("a needs-layout-gated node's panicking descriptor must never run");
    let second = owner
        .take_layer_tree()
        .expect("the walk must have genuinely run this pass and produced a fresh tree");
    drop(owner);

    assert_eq!(
        calls.load(Ordering::Relaxed),
        calls_after_first_frame,
        "needs_layout must gate the walk out before it builds the node's \
         own paint-effects descriptor -- a panicking descriptor that still \
         ran would have incremented this count (and panicked)",
    );
    assert_eq!(
        first_opacity_alpha(&second),
        None,
        "fx must contribute NOTHING to this pass's re-capture -- confirming \
         the walk reached it and exited via the needs_layout gate before \
         paint_raw or paint_effects, rather than reusing frame 1's capture \
         (which carried an Opacity layer) unmodified",
    );
}
