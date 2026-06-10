//! Animation → render-pipeline integration: a ticking controller
//! drives real frames.
//!
//! The animation engine and the render pipeline meet exactly the way
//! production meets them — per frame: advance the controller
//! (`tick_at`, deterministic simulated time), apply its value to a
//! render object, mark dirty, `run_frame`, then assert on COMMITTED
//! offsets, layer output, and hits. No wall clock, no ticker thread.
//!
//! Scenarios:
//! 1. animated layout — padding follows the controller value across
//!    five frames, offsets and picture bounds tracking exactly;
//! 2. animated opacity — the alpha hook's OpacityLayer follows the
//!    value down, and the alpha==0 frame skips the subtree entirely;
//! 3. animated transform — mid-animation hits walk the inverse of the
//!    CURRENT frame's matrix, not a stale one;
//! 4. completion → idle — once the controller completes and marks
//!    stop, the next frame produces nothing and no wake fires;
//! 5. reverse mid-flight — offsets walk back down without artifacts.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController};
use flui_layer::{Layer, LayerTree};
use flui_painting::DisplayListCore;
use flui_rendering::{
    constraints::BoxConstraints,
    hit_testing::HitTestResult,
    objects::{RenderColoredBox, RenderOpacity, RenderPadding, RenderTransform},
    pipeline::PipelineOwner,
};
use flui_scheduler::Scheduler;
use flui_types::{EdgeInsets, Matrix4, Offset, Size, geometry::px};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

fn controller() -> AnimationController {
    AnimationController::new(Duration::from_secs(1), Arc::new(Scheduler::new()))
}

fn frame(owner: PipelineOwner) -> (PipelineOwner, Option<LayerTree>) {
    let (owner, result) = owner.run_frame();
    (owner, result.expect("frame must not error"))
}

fn state_offset(owner: &PipelineOwner, id: flui_foundation::RenderId) -> Offset {
    owner
        .render_tree()
        .get(id)
        .and_then(|n| n.as_box())
        .map(|e| e.state().offset())
        .expect("node state")
}

fn set_padding(owner: &mut PipelineOwner, id: flui_foundation::RenderId, value: f32) {
    let entry = owner
        .render_tree_mut()
        .get_mut(id)
        .expect("padding node")
        .as_box_mut()
        .expect("box entry");
    entry
        .render_object_mut()
        .as_any_mut()
        .downcast_mut::<RenderPadding>()
        .expect("RenderPadding")
        .set_padding(EdgeInsets::all(px(value)));
}

// ============================================================================
// 1. Animated layout follows the controller frame by frame
// ============================================================================

#[test]
fn animated_padding_tracks_controller_value_across_frames() {
    let mut owner = PipelineOwner::new();
    let pad = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let child = owner
        .insert_child_render_object(pad, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child");
    owner.set_root_id(Some(pad));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(300.0),
        px(0.0),
        px(300.0),
    )));

    let ctrl = controller();
    ctrl.forward().expect("forward");

    // 5 simulated frames at t = 0, .25, .5, .75, 1.0 — padding tweens
    // 5 → 55 (lerp over the controller's linear value).
    for (i, t) in [0.0f64, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
        ctrl.tick_at(*t);
        let value = ctrl.value();
        let padding = 5.0 + 50.0 * value;
        set_padding(&mut owner, pad, padding);
        owner.mark_needs_layout(pad);

        let (next, tree) = frame(owner);
        owner = next;
        let tree = tree.unwrap_or_else(|| panic!("animation frame {i} must paint"));

        assert_eq!(
            state_offset(&owner, child),
            Offset::new(px(padding), px(padding)),
            "frame {i}: committed offset must equal the animated padding",
        );
        // The picture's bounds track the animated origin exactly.
        let bounds = {
            fn find(tree: &LayerTree, id: flui_foundation::LayerId) -> Option<flui_types::Rect> {
                let node = tree.get(id)?;
                if let Layer::Picture(p) = node.layer() {
                    return Some(p.picture().bounds());
                }
                node.children().iter().find_map(|&c| find(tree, c))
            }
            find(&tree, tree.root().expect("root")).expect("picture")
        };
        assert_eq!(
            bounds,
            flui_types::Rect::from_ltrb(
                px(padding),
                px(padding),
                px(padding + 40.0),
                px(padding + 40.0),
            ),
            "frame {i}: painted bounds must track the animated origin",
        );
    }

    assert!(
        ctrl.value() >= 1.0 - f32::EPSILON,
        "controller reached its upper bound",
    );
}

// ============================================================================
// 2. Animated opacity: layer alpha follows; alpha==0 skips the subtree
// ============================================================================

#[test]
fn animated_opacity_layer_follows_and_zero_alpha_skips() {
    let mut owner = PipelineOwner::new();
    let fade = owner.insert(Box::new(RenderOpacity::new(1.0)) as BoxedRenderObject);
    let _child = owner
        .insert_child_render_object(fade, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child");
    owner.set_root_id(Some(fade));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));

    let ctrl = controller();
    ctrl.forward().expect("forward");

    fn opacity_alpha(tree: &LayerTree) -> Option<f32> {
        fn find(tree: &LayerTree, id: flui_foundation::LayerId) -> Option<f32> {
            let node = tree.get(id)?;
            if let Layer::Opacity(o) = node.layer() {
                return Some(o.alpha());
            }
            node.children().iter().find_map(|&c| find(tree, c))
        }
        find(tree, tree.root()?)
    }
    fn has_picture(tree: &LayerTree) -> bool {
        fn find(tree: &LayerTree, id: flui_foundation::LayerId) -> bool {
            let Some(node) = tree.get(id) else {
                return false;
            };
            matches!(node.layer(), Layer::Picture(_))
                || node.children().iter().any(|&c| find(tree, c))
        }
        tree.root().is_some_and(|r| find(tree, r))
    }

    // Frame at t=0: still fully opaque. `paint_alpha` returns None at
    // alpha 255 (Flutter parity: a fully opaque RenderOpacity pushes no
    // layer) — the child paints directly, with no OpacityLayer to pay for.
    ctrl.tick_at(0.0);
    {
        let entry = owner
            .render_tree_mut()
            .get_mut(fade)
            .expect("fade node")
            .as_box_mut()
            .expect("box");
        entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<RenderOpacity>()
            .expect("RenderOpacity")
            .set_opacity(1.0 - ctrl.value());
    }
    owner.add_node_needing_paint(fade, 0);
    let (next, tree) = frame(owner);
    owner = next;
    let tree = tree.expect("fully-opaque frame paints");
    assert!(
        opacity_alpha(&tree).is_none(),
        "alpha == 255 must not pay for an OpacityLayer",
    );
    assert!(has_picture(&tree), "fully-opaque child paints directly");

    // Fade out: opacity = 1 - value, the layer alpha tracks per frame.
    for (i, t) in [0.25f64, 0.5].iter().enumerate() {
        ctrl.tick_at(*t);
        let opacity = 1.0 - ctrl.value();
        {
            let entry = owner
                .render_tree_mut()
                .get_mut(fade)
                .expect("fade node")
                .as_box_mut()
                .expect("box");
            entry
                .render_object_mut()
                .as_any_mut()
                .downcast_mut::<RenderOpacity>()
                .expect("RenderOpacity")
                .set_opacity(opacity);
        }
        owner.add_node_needing_paint(fade, 0);
        let (next, tree) = frame(owner);
        owner = next;
        let tree = tree.unwrap_or_else(|| panic!("fade frame {i} must paint"));

        let alpha = opacity_alpha(&tree).expect("opacity layer present while 0 < alpha < 1");
        assert!(
            (alpha - opacity).abs() < 0.01,
            "frame {i}: layer alpha {alpha} must track the animated opacity {opacity}",
        );
        assert!(has_picture(&tree), "frame {i}: child still paints");
    }

    // Final frame: opacity 0 — the subtree is skipped entirely.
    ctrl.tick_at(1.0);
    {
        let entry = owner
            .render_tree_mut()
            .get_mut(fade)
            .expect("fade node")
            .as_box_mut()
            .expect("box");
        entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<RenderOpacity>()
            .expect("RenderOpacity")
            .set_opacity(1.0 - ctrl.value());
    }
    owner.add_node_needing_paint(fade, 0);
    let (_owner, tree) = frame(owner);
    let tree = tree.expect("the zero-alpha frame still produces a (empty) tree");
    assert!(
        !has_picture(&tree),
        "alpha == 0 must skip recording the subtree — no picture at all",
    );
}

// ============================================================================
// 3. Animated transform: hits follow THIS frame's inverse
// ============================================================================

#[test]
fn animated_transform_hits_follow_current_frame_matrix() {
    let mut owner = PipelineOwner::new();
    // Origin pinned to the top-left corner: the default CENTER alignment
    // would scale around the node's midpoint, putting the probe point on
    // the (exclusive) scaled edge instead of inside it.
    let scaler = owner.insert(
        Box::new(RenderTransform::identity().with_origin(Offset::ZERO)) as BoxedRenderObject,
    );
    let child = owner
        .insert_child_render_object(scaler, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child");
    owner.set_root_id(Some(scaler));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let ctrl = controller();
    ctrl.forward().expect("forward");

    let hit_first = |owner: &PipelineOwner, x: f32, y: f32| {
        let mut result = HitTestResult::new();
        owner.hit_test(Offset::new(px(x), px(y)), &mut result);
        result.path().first().map(|e| e.target)
    };

    // value 0 → scale 1: (60,60) is OUTSIDE the 40×40 child.
    ctrl.tick_at(0.0);
    let (next, _) = frame(owner);
    owner = next;
    assert_eq!(
        hit_first(&owner, 60.0, 60.0),
        None,
        "scale 1: (60,60) misses"
    );

    // value 1 → scale 2: the SAME point is now inside (inverse → 30,30).
    ctrl.tick_at(1.0);
    let scale = 1.0 + ctrl.value();
    {
        let entry = owner
            .render_tree_mut()
            .get_mut(scaler)
            .expect("scaler")
            .as_box_mut()
            .expect("box");
        entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<RenderTransform>()
            .expect("RenderTransform")
            .set_transform(Matrix4::scaling(scale, scale, 1.0));
    }
    owner.add_node_needing_paint(scaler, 0);
    let (owner, tree) = frame(owner);
    assert!(tree.is_some(), "transform frame paints");
    assert_eq!(
        hit_first(&owner, 60.0, 60.0),
        Some(child),
        "scale 2: hits walk THIS frame's inverse, not a stale matrix",
    );
}

// ============================================================================
// 4. Completion → idle: no marks, no frames, no wakes
// ============================================================================

#[test]
fn completed_animation_leaves_the_pipeline_idle() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_clone = Arc::clone(&wake_count);
    let mut owner = PipelineOwner::new();
    owner.set_on_need_visual_update(move || {
        wake_clone.fetch_add(1, Ordering::Relaxed);
    });

    let pad = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let _child = owner
        .insert_child_render_object(pad, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child");
    owner.set_root_id(Some(pad));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let ctrl = controller();
    ctrl.forward().expect("forward");

    // Drive to completion in two frames.
    for t in [0.5f64, 1.0] {
        ctrl.tick_at(t);
        set_padding(&mut owner, pad, 5.0 + 20.0 * ctrl.value());
        owner.mark_needs_layout(pad);
        let (next, tree) = frame(owner);
        owner = next;
        assert!(tree.is_some());
    }
    assert!(
        !ctrl.is_animating(),
        "controller completed at its upper bound",
    );

    // The animation is done: no further marks. The pipeline must stay
    // silent — no frames, no wakes.
    let wakes_after_completion = wake_count.load(Ordering::Relaxed);
    for n in 0..3 {
        let (next, tree) = frame(owner);
        owner = next;
        assert!(tree.is_none(), "post-completion frame {n} must be empty");
    }
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wakes_after_completion,
        "no new wakes after the animation completed — a leak here is \
         the battery-drain bug class",
    );
    assert!(!owner.has_dirty_nodes());
}

// ============================================================================
// 5. Reverse mid-flight walks offsets back down
// ============================================================================

#[test]
fn reverse_mid_flight_walks_offsets_back() {
    let mut owner = PipelineOwner::new();
    let pad = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let child = owner
        .insert_child_render_object(pad, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child");
    owner.set_root_id(Some(pad));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(300.0),
        px(0.0),
        px(300.0),
    )));

    let ctrl = controller();
    ctrl.forward().expect("forward");
    ctrl.tick_at(0.6);
    let mid = ctrl.value();
    assert!((mid - 0.6).abs() < 1e-4);

    set_padding(&mut owner, pad, 5.0 + 50.0 * mid);
    owner.mark_needs_layout(pad);
    let (next, _) = frame(owner);
    owner = next;
    assert_eq!(state_offset(&owner, child).dx, px(35.0));

    // Reverse from 0.6. reverse() restarts the ticker (elapsed re-zeroes)
    // and the leg's duration is scaled by the remaining fraction — 0.6 of
    // the range in 0.6s, constant velocity through the turn.
    ctrl.reverse().expect("reverse");
    ctrl.tick_at(0.3); // 0.3s into the 0.6s reverse leg → value 0.3
    let back = ctrl.value();
    assert!((back - 0.3).abs() < 1e-3, "value walked back, got {back}");

    set_padding(&mut owner, pad, 5.0 + 50.0 * back);
    owner.mark_needs_layout(pad);
    let (owner, tree) = frame(owner);
    assert!(tree.is_some(), "reverse frame paints");
    assert_eq!(
        state_offset(&owner, child).dx,
        px(5.0 + 50.0 * back),
        "offsets follow the reversed value without artifacts",
    );
}
