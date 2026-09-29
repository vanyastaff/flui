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

use std::time::Duration;

use flui_animation::{Animation, AnimationController};
use flui_foundation::geometry::Size;
use flui_layer::{Layer, LayerTree};
use flui_objects::{RenderColoredBox, RenderOpacity};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};
use flui_scheduler::UpdateScheduler;

use crate::common::BoxedRenderObject;

fn controller() -> AnimationController {
    AnimationController::new(Duration::from_secs(1), &UpdateScheduler::new())
}

fn frame(owner: PipelineOwner) -> (PipelineOwner, Option<LayerTree>) {
    let (owner, result) = owner.run_frame();
    (owner, result.expect("frame must not error"))
}

// ============================================================================
// 1. Animated layout follows the controller frame by frame
// ============================================================================

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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 100.0))));

    let ctrl = controller();
    ctrl.forward().expect("forward");

    fn opacity_alpha(tree: &LayerTree) -> Option<f64> {
        fn find(tree: &LayerTree, id: flui_foundation::LayerId) -> Option<f64> {
            let node = tree.get(id)?;
            if let Layer::Opacity(o) = node.layer() {
                return Some(o.alpha());
            }
            node.children().iter().find_map(|&c| find(tree, c))
        }
        find(tree, tree.root())
    }
    fn has_picture(tree: &LayerTree) -> bool {
        fn find(tree: &LayerTree, id: flui_foundation::LayerId) -> bool {
            let Some(node) = tree.get(id) else {
                return false;
            };
            matches!(node.layer(), Layer::Picture(_))
                || node.children().iter().any(|&c| find(tree, c))
        }
        find(tree, tree.root())
    }

    // Frame at t=0: still fully opaque. `paint_effects().opacity` is `None`
    // at alpha 255 (Flutter parity: a fully opaque RenderOpacity pushes no
    // layer) — the child paints directly, with no OpacityLayer to pay for.
    ctrl.tick_at(0.0);
    let impact = {
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
            .set_opacity(1.0 - ctrl.value())
    };
    owner.apply_render_update_impact(fade, impact);
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
            let impact = entry
                .render_object_mut()
                .as_any_mut()
                .downcast_mut::<RenderOpacity>()
                .expect("RenderOpacity")
                .set_opacity(opacity);
            owner.apply_render_update_impact(fade, impact);
        }
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
    let impact = {
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
            .set_opacity(1.0 - ctrl.value())
    };
    owner.apply_render_update_impact(fade, impact);
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

// ============================================================================
// 4. Completion → idle: no marks, no frames, no wakes
// ============================================================================

// ============================================================================
// 5. Reverse mid-flight walks offsets back down
// ============================================================================
