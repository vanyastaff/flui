//! Equal-time acceptance through the mounted runtime, layout and committed scene.

use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, loose};
use flui_animation::{Linear, SpringDescription, Vsync};
use flui_painting::{display_list::DrawOp, styling::Color};
use flui_rendering::layer::Layer;
use flui_widgets::{AnimatedContainer, AnimatedOpacity, MotionBuilder, SizedBox, VsyncScope};

fn tree(registry: &Vsync, opacity: f64, side: f64, spring: bool) -> VsyncScope {
    let mut geometry = AnimatedContainer::new(SizedBox::shrink())
        .width(side)
        .height(side)
        .color(Color::RED)
        .duration(Duration::from_secs(1))
        .curve(Linear);
    if spring {
        geometry = geometry.spring(
            SpringDescription::with_damping_ratio(1.0, 100.0, 0.8).expect("valid geometry spring"),
        );
    }
    VsyncScope::new(
        registry.clone(),
        AnimatedOpacity::new(opacity, geometry)
            .duration(Duration::from_secs(1))
            .curve(Linear),
    )
}

/// Read the scene retained by the runtime sink, independently of motion state.
fn sample(laid: &LaidOut) -> [f64; 3] {
    let scene = laid.layer_tree().expect("a mounted scene was submitted");
    let mut opacity = 1.0;
    let mut bounds = None;
    for (_, node) in scene.iter() {
        match node.layer() {
            Layer::Opacity(layer) => opacity *= layer.alpha(),
            Layer::Picture(picture) => {
                for command in picture.picture() {
                    if let DrawOp::Rect { rect, paint } = &command.op {
                        assert_eq!(paint.color, Color::RED);
                        assert!(bounds.is_none(), "one visible animated rectangle");
                        bounds = Some(*rect);
                    }
                }
            }
            _ => {}
        }
    }
    let bounds = bounds.expect("the animated geometry was actually painted");
    let layout = laid.size(laid.current_root());
    assert_eq!(layout.width, bounds.width(), "paint uses committed layout");
    assert_eq!(
        layout.height,
        bounds.height(),
        "paint uses committed layout"
    );
    let sample = [opacity, bounds.width(), bounds.height()];
    assert!(sample.into_iter().all(f64::is_finite));
    sample
}

fn advance(laid: &mut LaidOut, interval: Duration, steps: &[Duration]) {
    let mut remaining = interval;
    let mut index = 0;
    while !remaining.is_zero() {
        let step = steps[index % steps.len()].min(remaining);
        laid.pump_for(step);
        remaining -= step;
        index += 1;
    }
}

fn assert_close(actual: [f64; 3], expected: [f64; 3], context: &str) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-8,
            "{context}: actual {actual}, expected {expected}"
        );
    }
}

fn run(steps: &[Duration], spring: bool) -> Vec<([f64; 3], bool)> {
    let registry = Vsync::new();
    let mut laid = lay_out_animated(
        tree(&registry, 0.25, 20.0, spring),
        loose(200.0),
        registry.clone(),
    );
    assert_close(sample(&laid), [0.25, 20.0, 20.0], "initial scene");
    laid.pump_widget(tree(&registry, 0.85, 100.0, spring));
    // Both paths observe the same run anchor before any positive-time frame.
    laid.pump_for(Duration::ZERO);
    let mut samples = Vec::new();
    let mut previous_ms = 0;
    for checkpoint_ms in [250, 400, 537, 900, 4000] {
        advance(
            &mut laid,
            Duration::from_millis(checkpoint_ms - previous_ms),
            steps,
        );
        previous_ms = checkpoint_ms;
        let displayed = sample(&laid);
        if checkpoint_ms == 250 {
            assert!(displayed[0] > 0.25 && displayed[0] < 0.85);
            assert!(displayed[1] > 20.0, "geometry must advance visibly");
        }
        samples.push((displayed, registry.has_running()));
        if checkpoint_ms == 400 {
            laid.pump_widget(tree(&registry, 0.4, 60.0, spring));
            laid.pump_for(Duration::ZERO);
            assert_close(sample(&laid), displayed, "equal-time replacement seam");
        }
    }
    assert_close(sample(&laid), [0.4, 60.0, 60.0], "settled scene");
    assert!(!registry.has_running(), "both motion owners have settled");
    let painted = laid.painted_frame_count();
    advance(&mut laid, Duration::from_millis(100), steps);
    assert_eq!(
        laid.painted_frame_count(),
        painted,
        "settled frames are idle"
    );
    samples
}

pub(crate) fn mounted_motion_reaches_equal_scenes_across_frame_partitions() {
    // Integer-nanosecond refresh periods plus an exact checkpoint remainder.
    let schedules: [(&str, &[Duration]); 5] = [
        ("30 Hz", &[Duration::from_nanos(33_333_333)]),
        ("60 Hz", &[Duration::from_nanos(16_666_667)]),
        ("120 Hz", &[Duration::from_nanos(8_333_333)]),
        ("144 Hz", &[Duration::from_nanos(6_944_444)]),
        (
            "skipped and repeated frames",
            &[
                Duration::from_millis(7),
                Duration::from_millis(111),
                Duration::ZERO,
                Duration::from_millis(3),
                Duration::from_millis(89),
            ],
        ),
    ];
    for spring in [false, true] {
        // A single advancing frame per checkpoint is an independent partition.
        let reference = run(&[Duration::from_secs(10)], spring);
        for (name, steps) in schedules {
            let actual = run(steps, spring);
            for ((actual, active), (expected, expected_active)) in
                actual.into_iter().zip(&reference)
            {
                assert_close(actual, *expected, name);
                assert_eq!(active, *expected_active, "{name}: settlement at equal time");
            }
        }
    }
}
