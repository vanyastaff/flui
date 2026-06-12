//! AnimationController-driven scenarios through the render harness.
//!
//! Mirrors `animation_pipeline.rs` but uses [`RenderTester`] /
//! [`FrameRun::advance_layout`] instead of hand-rolled `PipelineOwner` wiring.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController};
use flui_rendering::{
    constraints::BoxConstraints,
    objects::{RenderColoredBox, RenderPadding},
    testing::{Probe, RenderTester, box_node},
};
use flui_scheduler::Scheduler;
use flui_types::{EdgeInsets, Offset, Rect, geometry::px};

fn controller() -> AnimationController {
    AnimationController::new(Duration::from_secs(1), Arc::new(Scheduler::new()))
}

#[test]
fn harness_advance_layout_follows_animation_controller() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(300.0), px(0.0), px(300.0)))
    .run_frame();

    let ctrl = controller();
    ctrl.forward().expect("forward");

    let child = run.id("child");
    let pad = run.root();

    for (i, t) in [0.0f64, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
        ctrl.tick_at(*t);
        let padding = 5.0 + 50.0 * ctrl.value();
        let report = run.advance_layout::<RenderPadding>(pad, |p| {
            p.set_padding(EdgeInsets::all(px(padding)));
        });
        assert!(report.painted, "animation frame {i} must paint");

        assert_eq!(
            run.offset(child),
            Offset::new(px(padding), px(padding)),
            "frame {i}: committed offset must equal animated padding",
        );
        let bounds = run
            .picture_bounds()
            .expect("animated frame must paint a picture");
        assert_eq!(
            bounds,
            Rect::from_ltrb(
                px(padding),
                px(padding),
                px(padding + 40.0),
                px(padding + 40.0),
            ),
            "frame {i}: picture bounds must track the animated origin",
        );
    }

    assert!(
        ctrl.value() >= 1.0 - f32::EPSILON,
        "controller reached its upper bound",
    );

    run.pump_idle_frames(2);
}
