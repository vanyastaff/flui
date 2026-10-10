//! Implicitly-animated widgets driven deterministically through the headless
//! binding: a configuration change animates the property over frames instead of
//! snapping, and re-reads the interpolated value into the render tree — no
//! `thread::sleep`.
//!
//! Each test drives a small stateful *probe* that holds a shared target and
//! rebuilds the animated widget under a `VsyncScope` over the harness's `Vsync`.
//! Mutating the target then `pump()`-ing reconciles the animated widget (which
//! retargets its controller in `did_update_view`); `pump_for(dt)` then advances
//! the controller frame-by-frame.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, loose, tight};
use flui_animation::{ArcCurve, Curves, Vsync};
use flui_foundation::geometry::{Angle, EdgeInsets, Matrix4};
use flui_painting::Alignment;
use flui_painting::styling::Color;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewExt, ViewState};
use flui_widgets::{
    AnimatedAlign, AnimatedContainer, AnimatedOpacity, AnimatedPadding, AnimatedRotation,
    RotationPath, SizedBox, VsyncScope,
};
use parking_lot::Mutex;

/// A 100 ms run pumped in 20 ms frames spans the run in five steps.
const FRAME: Duration = Duration::from_millis(20);
const RUN: Duration = Duration::from_millis(100);

fn assert_target_and_curve_retarget_preserves_sample<V: flui_view::View, const N: usize>(
    tree: impl Fn(Vsync, f64, ArcCurve) -> V,
    sample: impl Fn(&mut LaidOut) -> [f64; N],
    [initial, target, replacement]: [f64; 3],
) {
    let registry = Vsync::new();
    let mut laid = lay_out_animated(
        tree(registry.clone(), initial, ArcCurve::new(Curves::Linear)),
        loose(200.0),
        registry.clone(),
    );
    let initial_sample = sample(&mut laid);
    laid.pump_widget(tree(
        registry.clone(),
        target,
        ArcCurve::new(Curves::Linear),
    ));
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let displayed = sample(&mut laid);
    assert!(
        displayed
            .into_iter()
            .zip(initial_sample)
            .any(|(a, b)| (a - b).abs() > 1e-6),
        "the mounted property actually moved: {initial_sample:?} to {displayed:?}",
    );

    laid.pump_widget(tree(registry, replacement, ArcCurve::new(Curves::EaseIn)));
    let retargeted = sample(&mut laid);
    for (before, after) in displayed.into_iter().zip(retargeted) {
        assert!(
            (after - before).abs() < 1e-12,
            "retarget advances no time: displayed {displayed:?}, after changing target and curve {retargeted:?}",
        );
    }
}

pub(crate) fn opacity_retarget_with_a_new_curve_keeps_the_displayed_sample() {
    assert_target_and_curve_retarget_preserves_sample(
        |registry, target, curve| {
            VsyncScope::new(
                registry,
                AnimatedOpacity::new(target, SizedBox::new(100.0, 50.0))
                    .duration(RUN)
                    .curve(curve),
            )
        },
        |laid| [laid.opacity(laid.current_root())],
        [0.0, 1.0, 0.0],
    );
}

pub(crate) fn opacity_retarget_preserves_the_painted_velocity() {
    let registry = Vsync::new();
    let tree = |target, curve: ArcCurve| {
        VsyncScope::new(
            registry.clone(),
            AnimatedOpacity::new(target, SizedBox::new(100.0, 50.0))
                .duration(Duration::from_secs(1))
                .curve(curve),
        )
    };
    let mut laid = lay_out_animated(
        tree(0.0, ArcCurve::new(Curves::Linear)),
        tight(100.0, 50.0),
        registry.clone(),
    );
    laid.pump_widget(tree(1.0, ArcCurve::new(Curves::Linear)));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(250));
    let h = Duration::from_micros(100);
    let previous = laid.opacity(laid.current_root());
    laid.pump_for(h);
    let seam = laid.opacity(laid.current_root());
    let arriving = (seam - previous) / h.as_secs_f64();
    assert!(
        arriving > 0.1,
        "the rendered opacity was moving before retarget"
    );
    laid.pump_widget(tree(0.0, ArcCurve::new(Curves::EaseIn)));
    assert!((laid.opacity(laid.current_root()) - seam).abs() < 1e-12);
    laid.pump_for(h);
    let departing = (laid.opacity(laid.current_root()) - seam) / h.as_secs_f64();
    assert!(
        (departing - arriving).abs() < 0.01,
        "the painted path must inherit its velocity: before {arriving}, after {departing}"
    );
}

pub(crate) fn padding_retarget_preserves_the_laid_out_velocity() {
    let registry = Vsync::new();
    let tree = |padding, curve: ArcCurve| {
        VsyncScope::new(
            registry.clone(),
            AnimatedPadding::new(padding, SizedBox::new(20.0, 10.0))
                .duration(Duration::from_secs(1))
                .curve(curve),
        )
    };
    let mut laid = lay_out_animated(
        tree(EdgeInsets::ZERO, ArcCurve::new(Curves::Linear)),
        tight(100.0, 80.0),
        registry.clone(),
    );
    let position = |laid: &mut LaidOut| laid.offset(laid.child(laid.current_root(), 0));
    laid.pump_widget(tree(
        EdgeInsets::new(10.0, 0.0, 0.0, 20.0),
        ArcCurve::new(Curves::Linear),
    ));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(250));
    let h = Duration::from_micros(100);
    let previous = position(&mut laid);
    laid.pump_for(h);
    let seam = position(&mut laid);
    let arriving = [
        (seam.dx - previous.dx) / h.as_secs_f64(),
        (seam.dy - previous.dy) / h.as_secs_f64(),
    ];
    assert!(
        arriving.iter().all(|velocity| *velocity > 1.0),
        "both layout components must be moving"
    );
    laid.pump_widget(tree(
        EdgeInsets::new(30.0, 0.0, 0.0, 5.0),
        ArcCurve::new(Curves::EaseIn),
    ));
    assert_eq!(position(&mut laid), seam);
    laid.pump_for(h);
    let after = position(&mut laid);
    let departing = [
        (after.dx - seam.dx) / h.as_secs_f64(),
        (after.dy - seam.dy) / h.as_secs_f64(),
    ];
    for (arriving, departing) in arriving.into_iter().zip(departing) {
        assert!(
            (arriving - departing).abs() < 0.05,
            "laid-out velocity was lost: {arriving} -> {departing}"
        );
    }
}

pub(crate) fn padding_retarget_with_a_new_curve_keeps_the_displayed_sample() {
    assert_target_and_curve_retarget_preserves_sample(
        |registry, target, curve| {
            VsyncScope::new(
                registry,
                AnimatedPadding::new(EdgeInsets::all(target), SizedBox::new(20.0, 10.0))
                    .duration(RUN)
                    .curve(curve),
            )
        },
        |laid| {
            let offset = laid.offset(laid.child(laid.current_root(), 0));
            [offset.dx, offset.dy]
        },
        [0.0, 40.0, 5.0],
    );
}

pub(crate) fn container_retarget_with_a_new_curve_keeps_the_displayed_sample() {
    assert_target_and_curve_retarget_preserves_sample(
        |registry, target, curve| {
            VsyncScope::new(
                registry,
                AnimatedContainer::new(SizedBox::shrink())
                    .width(target)
                    .height(if target == 60.0 { 50.0 } else { target / 2.0 })
                    .duration(RUN)
                    .curve(curve),
            )
        },
        |laid| {
            let size = laid.size(laid.current_root());
            [size.width, size.height]
        },
        [20.0, 100.0, 60.0],
    );
}

pub(crate) fn container_retarget_preserves_the_laid_out_size_velocity() {
    let registry = Vsync::new();
    let tree = |width, height, curve: ArcCurve| {
        VsyncScope::new(
            registry.clone(),
            AnimatedContainer::new(SizedBox::shrink())
                .width(width)
                .height(height)
                .duration(Duration::from_secs(1))
                .curve(curve),
        )
    };
    let mut laid = lay_out_animated(
        tree(20.0, 30.0, ArcCurve::new(Curves::Linear)),
        loose(200.0),
        registry.clone(),
    );
    laid.pump_widget(tree(100.0, 90.0, ArcCurve::new(Curves::Linear)));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(250));
    // Layout rounds sizes to hundredths of a logical pixel. A 5 ms interval
    // resolves both moving axes; the tolerance includes that quantization.
    let h = Duration::from_millis(5);
    let previous = laid.size(laid.current_root());
    laid.pump_for(h);
    let seam = laid.size(laid.current_root());
    let arriving = [
        (seam.width - previous.width) / h.as_secs_f64(),
        (seam.height - previous.height) / h.as_secs_f64(),
    ];
    assert!(
        arriving.iter().all(|v| *v > 1.0),
        "size must be moving: previous {previous:?}, seam {seam:?}, velocities {arriving:?}"
    );
    laid.pump_widget(tree(40.0, 10.0, ArcCurve::new(Curves::EaseIn)));
    assert_eq!(laid.size(laid.current_root()), seam);
    laid.pump_for(h);
    let after = laid.size(laid.current_root());
    let departing = [
        (after.width - seam.width) / h.as_secs_f64(),
        (after.height - seam.height) / h.as_secs_f64(),
    ];
    for (arriving, departing) in arriving.into_iter().zip(departing) {
        assert!(
            (arriving - departing).abs() < 5.0,
            "container size velocity was lost: {arriving} -> {departing}"
        );
    }
}

pub(crate) fn container_color_retarget_preserves_painted_alpha_progress() {
    use flui_painting::display_list::DrawOp;
    let alpha = |laid: &LaidOut| {
        laid.draw_ops()
            .into_iter()
            .find_map(|command| {
                if let DrawOp::Rect { paint, .. } = command.op {
                    Some(f64::from(paint.color.alpha_f32()))
                } else {
                    None
                }
            })
            .unwrap_or(0.0)
    };
    let registry = Vsync::new();
    let tree = |color, curve: ArcCurve| {
        VsyncScope::new(
            registry.clone(),
            AnimatedContainer::new(SizedBox::square(20.0))
                .color(color)
                .duration(Duration::from_secs(1))
                .curve(curve),
        )
    };
    let mut laid = lay_out_animated(
        tree(Color::TRANSPARENT, ArcCurve::new(Curves::Linear)),
        loose(200.0),
        registry.clone(),
    );
    laid.pump_widget(tree(Color::WHITE, ArcCurve::new(Curves::Linear)));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(250));
    let h = Duration::from_millis(20);
    let previous = alpha(&laid);
    laid.pump_for(h);
    let seam = alpha(&laid);
    let arriving = seam - previous;
    assert!(
        arriving >= 3.0 / 255.0,
        "painted alpha must be moving: {arriving}"
    );
    laid.pump_widget(tree(Color::BLACK, ArcCurve::new(Curves::EaseIn)));
    assert_eq!(alpha(&laid), seam);
    laid.pump_for(h);
    let departing = alpha(&laid) - seam;
    assert!(
        (departing - arriving).abs() <= 2.0 / 255.0,
        "painted alpha progress was lost: {arriving} -> {departing}"
    );
}

pub(crate) fn container_property_motion_settles_and_unmounts_independently() {
    for spring in [false, true] {
        let registry = Vsync::new();
        let curve = ArcCurve::new(Curves::Linear);
        let tree = |size, scale, color| {
            let mut container = AnimatedContainer::new(SizedBox::shrink())
                .width(size)
                .height(size)
                .color(color)
                .transform(Matrix4::scaling(scale, scale, 1.0))
                .duration(RUN)
                .curve(curve.clone());
            if spring {
                container = container.spring(
                    flui_animation::SpringDescription::with_damping_ratio(1.0, 100.0, 1.0),
                );
            }
            VsyncScope::new(registry.clone(), container)
        };
        let mut laid = lay_out_animated(
            tree(20.0, 1.0, Color::BLACK),
            loose(200.0),
            registry.clone(),
        );
        laid.pump_widget(tree(100.0, 2.0, Color::BLACK));
        laid.pump_for(Duration::from_millis(1));
        laid.pump_for(Duration::from_millis(40));
        let seam = laid.size(laid.current_root());
        assert!(seam.width > 20.0 && seam.width < 100.0);
        laid.pump_widget(tree(100.0, 2.0, Color::WHITE));
        assert_eq!(laid.size(laid.current_root()), seam);
        laid.pump_for(if spring {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(60)
        });
        let settled = laid.size(laid.current_root());
        assert_eq!(
            (settled.width, settled.height),
            (100.0, 100.0),
            "color replacement cannot delay size settlement"
        );
        assert_eq!(layer_scale(&mut laid), 2.0);
        laid.pump_widget(SizedBox::square(10.0));
        assert!(
            registry.is_empty(),
            "all property owners withdraw on unmount"
        );
    }
}

fn assert_refused_container_motion_preserves_the_admitted_run(accepted_slopes: usize) {
    use flui_animation::curve::Curve;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct RefusingCurve {
        calls: AtomicUsize,
        accepted: usize,
        refused: Arc<AtomicBool>,
    }
    impl Curve for RefusingCurve {
        fn transform(&self, t: f64) -> f64 {
            t
        }
        fn slope(&self, _t: f64) -> f64 {
            if self.calls.fetch_add(1, Ordering::Relaxed) < self.accepted {
                1.0
            } else {
                self.refused.store(true, Ordering::Relaxed);
                f64::NAN
            }
        }
    }

    let registry = Vsync::new();
    let tree = |size, scale, curve| {
        VsyncScope::new(
            registry.clone(),
            AnimatedContainer::new(SizedBox::shrink())
                .width(size)
                .height(size)
                .transform(Matrix4::scaling(scale, scale, 1.0))
                .duration(RUN)
                .curve(curve),
        )
    };
    let linear = || ArcCurve::new(Curves::Linear);
    let mut laid = lay_out_animated(tree(20.0, 1.0, linear()), loose(300.0), registry.clone());
    laid.pump_widget(tree(100.0, 2.0, linear()));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(40));
    let seam = laid.size(laid.current_root());
    let scale = layer_scale(&mut laid);
    assert!(seam.width > 20.0 && seam.width < 100.0);
    assert!(scale > 1.0 && scale < 2.0);

    let refused = Arc::new(AtomicBool::new(false));
    laid.pump_widget(tree(
        200.0,
        3.0,
        ArcCurve::new(RefusingCurve {
            calls: AtomicUsize::new(0),
            accepted: accepted_slopes,
            refused: Arc::clone(&refused),
        }),
    ));
    assert!(
        refused.load(Ordering::Relaxed),
        "the update must encounter refusal"
    );
    assert_eq!(laid.size(laid.current_root()), seam);
    assert_eq!(layer_scale(&mut laid), scale);
    laid.pump_for(Duration::from_millis(60));
    let settled = laid.size(laid.current_root());
    assert_eq!(
        (settled.width, settled.height),
        (100.0, 100.0),
        "refused motion must preserve both previously admitted goals"
    );
    assert_eq!(
        layer_scale(&mut laid),
        2.0,
        "refused progress must preserve the previously admitted matrix run"
    );

    laid.pump_widget(tree(200.0, 4.0, linear()));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(RUN);
    let recovered = laid.size(laid.current_root());
    assert_eq!((recovered.width, recovered.height), (200.0, 200.0));
    assert_eq!(layer_scale(&mut laid), 4.0);
    laid.pump_widget(SizedBox::shrink());
    assert!(registry.is_empty());
}

pub(crate) fn container_refused_property_motion_preserves_the_admitted_goals() {
    // A scalar segment prepares its start and end slopes. Width prepares
    // successfully; the same curve then refuses height preparation.
    assert_refused_container_motion_preserves_the_admitted_run(2);
}

pub(crate) fn container_refused_transform_motion_preserves_the_admitted_matrix() {
    assert_refused_container_motion_preserves_the_admitted_run(0);
    // Both numeric segments prepare before matrix progress refuses its slope.
    assert_refused_container_motion_preserves_the_admitted_run(4);
}

pub(crate) fn an_absent_container_transform_owns_no_frame_registration() {
    let registry = Vsync::new();
    let tree = |transform: Option<Matrix4>| {
        let mut container = AnimatedContainer::new(SizedBox::new(20.0, 20.0))
            .duration(RUN)
            .curve(Curves::Linear);
        if let Some(transform) = transform {
            container = container.transform(transform);
        }
        VsyncScope::new(registry.clone(), container)
    };
    let mut laid = lay_out_animated(tree(None), loose(200.0), registry.clone());
    assert!(
        registry.is_empty(),
        "absent properties have no motion owner to register"
    );
    laid.pump_widget(tree(Some(Matrix4::scaling(2.0, 2.0, 1.0))));
    assert_eq!(
        layer_scale(&mut laid),
        2.0,
        "appearance snaps to its only endpoint"
    );
    assert!(!registry.is_empty());
    laid.pump_widget(tree(Some(Matrix4::scaling(4.0, 4.0, 1.0))));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(40));
    assert!(layer_scale(&mut laid) > 2.0 && layer_scale(&mut laid) < 4.0);
    laid.pump_widget(tree(None));
    assert!(
        registry.is_empty(),
        "disappearance withdraws the moving transform owner"
    );
    assert_eq!(laid.transform_layer_matrices(), [] as [Matrix4; 0]);
    laid.pump_widget(tree(Some(Matrix4::scaling(3.0, 3.0, 1.0))));
    assert_eq!(layer_scale(&mut laid), 3.0);
    laid.pump_widget(SizedBox::shrink());
    assert!(registry.is_empty());
}

fn assert_registry_migration_survives_a_wake_failure<V: flui_view::View>(
    tree: impl Fn(Vsync, bool) -> V,
) {
    use std::cell::Cell;
    use std::rc::Rc;
    let outer = Vsync::new();
    let old = Vsync::new();
    let next = Vsync::new();
    outer.attach_child(&old).unwrap();
    outer.attach_child(&next).unwrap();
    let mut laid = lay_out_animated(tree(old.clone(), false), loose(300.0), outer);
    laid.pump_widget(tree(old.clone(), true));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(40));
    assert!(old.has_running());
    let woke = Rc::new(Cell::new(false));
    let migrated = Rc::new(Cell::new(false));
    next.set_frame_requester(Some(Rc::new({
        let woke = Rc::clone(&woke);
        let migrated = Rc::clone(&migrated);
        let old = old.clone();
        move || {
            woke.set(true);
            migrated.set(old.is_empty());
            panic!("migration wake failed");
        }
    })));
    // The owner-call boundary may contain the callback failure. In either
    // case every property belongs to the new scope before it can wake.
    let _outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        laid.pump_widget(tree(next.clone(), true));
    }));
    assert!(
        woke.get(),
        "the new registry must encounter the failing wake hook"
    );
    assert!(
        old.is_empty(),
        "a wake failure must not leave any property on its previous clock"
    );
    assert!(
        migrated.get(),
        "all properties must leave the old registry before the wake callback"
    );
    next.set_frame_requester(None);
    laid.pump_widget(SizedBox::shrink());
    laid.pump_widget(tree(next.clone(), false));
    laid.pump_widget(tree(next.clone(), true));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_secs(1));
    assert!(!next.has_running());
    laid.pump_widget(SizedBox::shrink());
    assert!(old.is_empty() && next.is_empty());
}

pub(crate) fn container_registry_migration_finishes_before_a_wake_failure() {
    assert_registry_migration_survives_a_wake_failure(|registry, changed| {
        let size = if changed { 100.0 } else { 20.0 };
        let scale = if changed { 2.0 } else { 1.0 };
        VsyncScope::new(
            registry,
            AnimatedContainer::new(SizedBox::shrink())
                .width(size)
                .height(size)
                .transform(Matrix4::scaling(scale, scale, 1.0))
                .duration(RUN)
                .curve(Curves::Linear),
        )
    });
}

pub(crate) fn align_registry_migration_finishes_before_a_wake_failure() {
    assert_registry_migration_survives_a_wake_failure(|registry, changed| {
        let (alignment, factor) = if changed {
            (Alignment::BOTTOM_RIGHT, 2.0)
        } else {
            (Alignment::TOP_LEFT, 1.0)
        };
        VsyncScope::new(
            registry,
            AnimatedAlign::new(alignment, SizedBox::new(20.0, 20.0))
                .width_factor(factor)
                .height_factor(factor)
                .duration(RUN)
                .curve(Curves::Linear),
        )
    });
}

pub(crate) fn align_retarget_with_a_new_curve_keeps_the_displayed_sample() {
    assert_target_and_curve_retarget_preserves_sample(
        |registry, target, curve| {
            VsyncScope::new(
                registry,
                AnimatedAlign::new(
                    if target == 0.0 {
                        Alignment::TOP_LEFT
                    } else {
                        Alignment::BOTTOM_RIGHT
                    },
                    SizedBox::new(20.0, 10.0),
                )
                .duration(RUN)
                .curve(curve),
            )
        },
        |laid| {
            let offset = laid.offset(laid.child(laid.current_root(), 0));
            [offset.dx, offset.dy]
        },
        [0.0, 1.0, 0.0],
    );
}

pub(crate) fn align_retarget_preserves_the_laid_out_velocity() {
    let registry = Vsync::new();
    let tree = |target, curve: ArcCurve| {
        VsyncScope::new(
            registry.clone(),
            AnimatedAlign::new(target, SizedBox::new(20.0, 10.0))
                .duration(Duration::from_secs(1))
                .curve(curve),
        )
    };
    let mut laid = lay_out_animated(
        tree(Alignment::TOP_LEFT, ArcCurve::new(Curves::Linear)),
        tight(100.0, 80.0),
        registry.clone(),
    );
    let position = |laid: &mut LaidOut| laid.offset(laid.child(laid.current_root(), 0));
    laid.pump_widget(tree(Alignment::BOTTOM_RIGHT, ArcCurve::new(Curves::Linear)));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(250));
    let h = Duration::from_micros(100);
    let previous = position(&mut laid);
    laid.pump_for(h);
    let seam = position(&mut laid);
    let arriving = [
        (seam.dx - previous.dx) / h.as_secs_f64(),
        (seam.dy - previous.dy) / h.as_secs_f64(),
    ];
    assert!(arriving.iter().all(|velocity| *velocity > 1.0));
    laid.pump_widget(tree(Alignment::TOP_LEFT, ArcCurve::new(Curves::EaseIn)));
    assert_eq!(
        position(&mut laid),
        seam,
        "the layout seam must be continuous"
    );
    laid.pump_for(h);
    let after = position(&mut laid);
    let departing = [
        (after.dx - seam.dx) / h.as_secs_f64(),
        (after.dy - seam.dy) / h.as_secs_f64(),
    ];
    for (arriving, departing) in arriving.into_iter().zip(departing) {
        assert!(
            (arriving - departing).abs() < 0.1,
            "laid-out alignment velocity was lost: {arriving} -> {departing}"
        );
    }
}

pub(crate) fn align_changes_leave_an_unchanged_factor_on_its_original_deadline() {
    let registry = Vsync::new();
    let curve = ArcCurve::new(Curves::Linear);
    let tree = |alignment, factor| {
        VsyncScope::new(
            registry.clone(),
            AnimatedAlign::new(alignment, SizedBox::square(10.0))
                .width_factor(factor)
                .duration(RUN)
                .curve(curve.clone()),
        )
    };
    let mut laid = lay_out_animated(tree(Alignment::CENTER, 2.0), loose(200.0), registry.clone());
    laid.pump_widget(tree(Alignment::BOTTOM_RIGHT, 5.0));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(40));
    let seam = laid.size(laid.current_root()).width;
    assert!(seam > 20.0 && seam < 50.0, "factor motion is live: {seam}");
    laid.pump_widget(tree(Alignment::TOP_LEFT, 5.0));
    assert_eq!(laid.size(laid.current_root()).width, seam);
    laid.pump_for(Duration::from_millis(60));
    assert_eq!(
        laid.size(laid.current_root()).width,
        50.0,
        "changing alignment must not extend the factor's deadline"
    );
}

pub(crate) fn align_spring_settles_and_optional_factors_snap_independently() {
    let registry = Vsync::new();
    let tree = |alignment, width: Option<f64>| {
        let mut align = AnimatedAlign::new(alignment, SizedBox::square(10.0)).spring(
            flui_animation::SpringDescription::with_damping_ratio(1.0, 100.0, 1.0),
        );
        if let Some(width) = width {
            align = align.width_factor(width);
        }
        VsyncScope::new(registry.clone(), align)
    };
    let mut laid = lay_out_animated(
        tree(Alignment::TOP_LEFT, None),
        loose(200.0),
        registry.clone(),
    );
    assert_eq!(laid.size(laid.current_root()).width, 200.0);
    laid.pump_widget(tree(Alignment::BOTTOM_RIGHT, Some(3.0)));
    assert_eq!(laid.size(laid.current_root()).width, 30.0);
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(200));
    let child = laid.child(laid.current_root(), 0);
    let seam = laid.offset(child).dx;
    assert!(seam > 0.0 && seam < 20.0, "alignment spring moved: {seam}");
    laid.pump_widget(tree(Alignment::BOTTOM_RIGHT, None));
    assert_eq!(laid.size(laid.current_root()).width, 200.0);
    laid.pump_for(Duration::from_secs(3));
    let child = laid.child(laid.current_root(), 0);
    assert_eq!(laid.offset(child).dx, 190.0);
    assert_eq!(laid.offset(child).dy, 190.0);
    laid.pump_widget(SizedBox::square(10.0));
    assert!(registry.is_empty(), "unmount withdraws all property motion");
}

pub(crate) fn invalid_align_targets_preserve_all_running_layout_properties() {
    let registry = Vsync::new();
    let curve = ArcCurve::new(Curves::Linear);
    let tree = |alignment, factor| {
        VsyncScope::new(
            registry.clone(),
            AnimatedAlign::new(alignment, SizedBox::square(10.0))
                .width_factor(factor)
                .duration(RUN)
                .curve(curve.clone()),
        )
    };
    let mut laid = lay_out_animated(
        tree(Alignment::TOP_LEFT, 2.0),
        loose(200.0),
        registry.clone(),
    );
    laid.pump_widget(tree(Alignment::BOTTOM_RIGHT, 5.0));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(40));
    let size = laid.size(laid.current_root());
    let child = laid.child(laid.current_root(), 0);
    let offset = laid.offset(child);
    laid.pump_widget(tree(Alignment::TOP_LEFT, f64::NAN));
    assert_eq!(laid.size(laid.current_root()), size);
    let child = laid.child(laid.current_root(), 0);
    assert_eq!(laid.offset(child), offset);
    laid.pump_for(Duration::from_millis(60));
    assert_eq!(laid.size(laid.current_root()).width, 50.0);
    let child = laid.child(laid.current_root(), 0);
    assert_eq!(laid.offset(child).dx, 40.0);
    assert_eq!(laid.offset(child).dy, 190.0);
}

pub(crate) fn rotation_retarget_preserves_the_painted_velocity() {
    let registry = Vsync::new();
    let tree = |target, curve: ArcCurve| {
        VsyncScope::new(
            registry.clone(),
            AnimatedRotation::new(Angle::from_turns(target), SizedBox::new(20.0, 10.0))
                .path(RotationPath::Numeric)
                .duration(Duration::from_secs(1))
                .curve(curve),
        )
    };
    let mut laid = lay_out_animated(
        tree(0.0, ArcCurve::new(Curves::Linear)),
        tight(100.0, 80.0),
        registry.clone(),
    );
    laid.pump_widget(tree(0.25, ArcCurve::new(Curves::Linear)));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(250));
    let h = Duration::from_micros(100);
    let previous = layer_turns(&mut laid);
    laid.pump_for(h);
    let seam = layer_turns(&mut laid);
    let arriving = (seam - previous) / h.as_secs_f64();
    assert!(arriving > 0.1, "the painted rotation was moving");
    laid.pump_widget(tree(0.0, ArcCurve::new(Curves::EaseIn)));
    assert!((layer_turns(&mut laid) - seam).abs() < 1e-12);
    laid.pump_for(h);
    let departing = (layer_turns(&mut laid) - seam) / h.as_secs_f64();
    assert!(
        (departing - arriving).abs() < 0.01,
        "rotation must inherit velocity: before {arriving}, after {departing}"
    );
}

pub(crate) fn rotation_retarget_with_a_new_curve_keeps_the_displayed_sample() {
    assert_target_and_curve_retarget_preserves_sample(
        |registry, target, curve| {
            VsyncScope::new(
                registry,
                AnimatedRotation::new(Angle::from_turns(target), SizedBox::new(20.0, 10.0))
                    .path(RotationPath::Numeric)
                    .duration(RUN)
                    .curve(curve),
            )
        },
        |laid| [layer_turns(laid)],
        [0.125, 0.375, 0.125],
    );
}

pub(crate) fn changing_only_the_curve_keeps_the_existing_deadline() {
    let registry = Vsync::new();
    let tree = |target, curve: ArcCurve| {
        VsyncScope::new(
            registry.clone(),
            AnimatedOpacity::new(target, SizedBox::new(100.0, 50.0))
                .duration(RUN)
                .curve(curve),
        )
    };
    let mut laid = lay_out_animated(
        tree(0.0, ArcCurve::new(Curves::Linear)),
        tight(100.0, 50.0),
        registry.clone(),
    );
    laid.pump_widget(tree(1.0, ArcCurve::new(Curves::Linear)));
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let linear = laid.opacity(laid.current_root());
    assert!(linear > 0.1 && linear < 0.3);
    laid.pump_widget(tree(1.0, ArcCurve::new(Curves::EaseIn)));
    laid.pump_for(RUN.checked_sub(FRAME).expect("run exceeds one frame"));
    assert_eq!(
        laid.opacity(laid.current_root()),
        1.0,
        "a curve-only update does not restart time"
    );
    laid.pump_widget(SizedBox::shrink());
    assert!(
        registry.is_empty(),
        "unmount releases the owning controller"
    );
}

pub(crate) fn swapping_the_scope_registry_preserves_an_implicit_run() {
    let old = Vsync::new();
    let new = Vsync::new();
    let tree = |registry: Vsync, target| {
        VsyncScope::new(
            registry,
            AnimatedOpacity::new(target, SizedBox::new(100.0, 50.0)).duration(RUN),
        )
    };
    let mut laid = lay_out_animated(tree(old.clone(), 0.0), tight(100.0, 50.0), old.clone());
    laid.pump_widget(tree(old.clone(), 1.0));
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let displayed = laid.opacity(laid.current_root());
    assert!(displayed > 0.0 && displayed < 1.0);

    laid.pump_widget(tree(new.clone(), 1.0));
    assert!(
        old.is_empty(),
        "the retained widget releases its old registry"
    );
    assert_eq!(new.len(), 1);
    laid.adopt_vsync(new.clone());
    laid.pump_for(Duration::ZERO);
    assert!((laid.opacity(laid.current_root()) - displayed).abs() < 1e-9);
    laid.pump_for(FRAME);
    assert!(laid.opacity(laid.current_root()) > displayed);

    laid.pump_widget(SizedBox::shrink());
    assert!(new.is_empty(), "unmount releases the owning handle");
}

pub(crate) fn switching_entries_migrate_their_incoming_and_outgoing_runs() {
    exercise_switching_registry(false);
}

pub(crate) fn switching_entries_finish_migration_before_a_wake_failure() {
    exercise_switching_registry(true);
}

fn exercise_switching_registry(fail_wake: bool) {
    use flui_widgets::{AnimatedSwitcher, ColoredBox};

    let old = Vsync::new();
    let next = Vsync::new();
    let tree = |registry, replacement| {
        let child = if replacement {
            ColoredBox::new(Color::rgb(10, 20, 30)).boxed()
        } else {
            SizedBox::new(100.0, 100.0).boxed()
        };
        VsyncScope::new(registry, AnimatedSwitcher::new(RUN).child(child))
    };
    let mut laid = lay_out_animated(tree(old.clone(), false), tight(100.0, 100.0), old.clone());
    laid.pump_widget(tree(old.clone(), true));
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let mut fades = laid.find_all_by_render_type("RenderAnimatedOpacity");
    assert_eq!(fades.len(), 2, "both cross-fade participants are present");
    let mut samples: Vec<_> = fades.iter().map(|id| laid.opacity(*id)).collect();
    assert!(samples.iter().all(|value| *value > 0.0 && *value < 1.0));
    assert!(old.has_running());

    let woke = std::rc::Rc::new(std::cell::Cell::new(false));
    let committed = std::rc::Rc::new(std::cell::Cell::new(true));
    if fail_wake {
        next.set_frame_requester(Some(std::rc::Rc::new({
            let (woke, committed, old) = (woke.clone(), committed.clone(), old.clone());
            move || {
                woke.set(true);
                committed.set(committed.get() && old.is_empty());
                panic!("switcher migration wake failed");
            }
        })));
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        laid.pump_widget(tree(next.clone(), true));
    }));
    if let Err(payload) = outcome {
        assert!(fail_wake);
        assert_eq!(
            flui_foundation::panic::payload_text(payload.as_ref()),
            Some("switcher migration wake failed")
        );
    }
    next.set_frame_requester(None);
    assert!(old.is_empty(), "incoming and outgoing owners both migrate");
    if fail_wake {
        assert!(woke.get(), "migration reaches the failing hook");
        assert!(
            committed.get(),
            "all seats transfer before the first callout"
        );
        assert!(
            next.is_empty(),
            "the failed lifecycle actor retires all its owners"
        );
        laid.pump_widget(SizedBox::shrink());
        laid.pump_widget(tree(next.clone(), false));
        laid.pump_widget(tree(next.clone(), true));
        laid.adopt_vsync(next.clone());
        laid.pump_for(FRAME);
        laid.pump_for(FRAME);
        fades = laid.find_all_by_render_type("RenderAnimatedOpacity");
        assert_eq!(
            fades.len(),
            2,
            "a fresh actor starts both cross-fade participants"
        );
        samples = fades.iter().map(|id| laid.opacity(*id)).collect();
        assert!(samples.iter().all(|value| *value > 0.0 && *value < 1.0));
    }
    laid.adopt_vsync(next.clone());
    laid.pump_for(Duration::ZERO);
    for (id, before) in fades.iter().zip(&samples) {
        assert!((laid.opacity(*id) - before).abs() < 1e-8);
    }
    laid.pump_for(FRAME);
    for (id, before) in fades.iter().zip(&samples) {
        let advanced = laid.opacity(*id);
        assert!(advanced > 0.0 && advanced < 1.0);
        assert!(
            (advanced - before).abs() > 0.01,
            "each cross-fade run advances"
        );
    }
    laid.pump_for(RUN);
    let remaining = laid.find_all_by_render_type("RenderAnimatedOpacity");
    assert_eq!(
        remaining.len(),
        1,
        "the outgoing child retires after completion"
    );
    assert_eq!(laid.opacity(remaining[0]), 1.0);
    assert!(!next.has_running());
    laid.pump_widget(SizedBox::shrink());
    assert!(next.is_empty());
}

pub(crate) fn a_detached_ticker_mode_lands_an_implicit_run() {
    let tree = |target| {
        VsyncScope::detached(flui_widgets::TickerMode::new(
            AnimatedOpacity::new(target, SizedBox::new(100.0, 50.0)).duration(RUN),
        ))
    };
    let mut laid = crate::common::lay_out(tree(0.0), tight(100.0, 50.0));
    laid.pump_widget(tree(1.0));
    assert_eq!(laid.opacity(laid.current_root()), 1.0);
}

#[derive(Clone)]
struct KeyedOpacity {
    key: flui_view::GlobalKey<KeyedOpacityState>,
    target: f64,
    registry: std::rc::Rc<std::cell::RefCell<Option<Vsync>>>,
    inits: std::rc::Rc<std::cell::Cell<usize>>,
}

struct KeyedOpacityState {
    registry: std::rc::Rc<std::cell::RefCell<Option<Vsync>>>,
    inits: std::rc::Rc<std::cell::Cell<usize>>,
}

impl flui_view::View for KeyedOpacity {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

impl StatefulView for KeyedOpacity {
    type State = KeyedOpacityState;
    fn create_state(&self) -> Self::State {
        KeyedOpacityState {
            registry: self.registry.clone(),
            inits: self.inits.clone(),
        }
    }
}

impl ViewState<KeyedOpacity> for KeyedOpacityState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        self.inits.set(self.inits.get() + 1);
        *self.registry.borrow_mut() = VsyncScope::maybe_of(ctx);
    }
    fn did_change_dependencies(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        *self.registry.borrow_mut() = VsyncScope::maybe_of(ctx);
    }
    fn build(&self, view: &KeyedOpacity, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedOpacity::new(view.target, SizedBox::new(40.0, 30.0)).duration(RUN)
    }
}

pub(crate) fn reparenting_across_ticker_mode_moves_the_registration() {
    use flui_view::ViewExt;
    let root = Vsync::new();
    let registry = std::rc::Rc::new(std::cell::RefCell::new(None));
    let inits = std::rc::Rc::new(std::cell::Cell::new(0));
    let key = flui_view::GlobalKey::new();
    let tree = |moved, target, enabled| {
        let child = KeyedOpacity {
            key: key.clone(),
            target,
            registry: registry.clone(),
            inits: inits.clone(),
        };
        let empty = SizedBox::shrink().boxed();
        let (left, right) = if moved {
            (empty, child.boxed())
        } else {
            (child.boxed(), empty)
        };
        VsyncScope::new(
            root.clone(),
            flui_widgets::Row::new((
                flui_widgets::TickerMode::new(left),
                flui_widgets::TickerMode::new(right).enabled(enabled),
            )),
        )
    };
    let mut laid = lay_out_animated(tree(false, 0.0, false), tight(200.0, 60.0), root.clone());
    laid.pump_widget(tree(false, 1.0, false));
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let rendered = laid.find_by_render_type("RenderAnimatedOpacity");
    let shown = laid.opacity(rendered);
    assert!(shown > 0.0 && shown < 1.0);
    let old = registry.borrow().clone().expect("old scope recorded");

    laid.pump_widget(tree(true, 1.0, false));
    let new = registry.borrow().clone().expect("new scope recorded");
    assert!(!old.is_same(&new));
    assert!(old.is_empty());
    assert_eq!(new.len(), 1);
    assert_eq!(
        inits.get(),
        1,
        "the GlobalKey retains the animation subtree"
    );
    laid.pump_for(FRAME);
    assert_eq!(
        laid.opacity(rendered),
        shown,
        "the new muted scope holds the sample"
    );
    laid.pump_widget(tree(true, 1.0, true));
    laid.pump_for(FRAME);
    assert_eq!(
        laid.opacity(rendered),
        shown,
        "the first eligible tick anchors the preserved elapsed time"
    );
    laid.pump_for(FRAME);
    assert!(
        laid.opacity(rendered) > shown,
        "the new enabled scope advances the same run"
    );
    laid.pump_widget(SizedBox::shrink());
    assert!(new.is_empty());
}

// ----------------------------------------------------------------------------
// AnimatedOpacity
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct OpacityProbe {
    vsync: Vsync,
    target: Arc<Mutex<f64>>,
}

struct OpacityProbeState {
    vsync: Vsync,
    target: Arc<Mutex<f64>>,
}

impl StatefulView for OpacityProbe {
    type State = OpacityProbeState;

    fn create_state(&self) -> Self::State {
        OpacityProbeState {
            vsync: self.vsync.clone(),
            target: Arc::clone(&self.target),
        }
    }
}

impl ViewState<OpacityProbe> for OpacityProbeState {
    fn build(&self, _view: &OpacityProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedOpacity::new(*self.target.lock(), SizedBox::new(100.0, 50.0)).duration(RUN),
        )
    }
}

pub(crate) fn animated_opacity_retargets_from_the_current_value_midflight() {
    let vsync = Vsync::new();
    let target = Arc::new(Mutex::new(0.0));
    let probe = OpacityProbe {
        vsync: vsync.clone(),
        target: Arc::clone(&target),
    };
    let mut laid = lay_out_animated(probe, tight(100.0, 50.0), vsync);

    // Start a 0 → 1 run and advance partway.
    *target.lock() = 1.0;
    laid.pump();
    laid.pump_for(FRAME); // detection (~0.0)
    laid.pump_for(FRAME);
    laid.pump_for(FRAME); // ~0.4 along the linear-ish curve
    let midflight = laid.opacity(laid.current_root());
    assert!(
        midflight > 0.1 && midflight < 0.9,
        "should be partway through the first run, got {midflight}",
    );

    // Retarget back toward 0.0 mid-flight: the new run must begin from the
    // CURRENT displayed value, not snap to 1.0 first.
    *target.lock() = 0.0;
    laid.pump();
    laid.pump_for(FRAME); // detection frame of the reverse run holds ~midflight
    let after_retarget = laid.opacity(laid.current_root());
    assert!(
        (after_retarget - midflight).abs() < 0.2,
        "retarget holds near the current value {midflight}, not a snap to 1.0 \
         or 0.0; got {after_retarget}",
    );

    // Then it descends toward the new target.
    for _ in 0..5 {
        laid.pump_for(FRAME);
    }
    assert!(
        laid.opacity(laid.current_root()) < 0.1,
        "the reverse run settles near 0.0, got {}",
        laid.opacity(laid.current_root()),
    );
}

// ----------------------------------------------------------------------------
// AnimatedPadding
// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// AnimatedAlign
// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// AnimatedContainer
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct ContainerProbe {
    vsync: Vsync,
    side: Arc<Mutex<f64>>,
}

struct ContainerProbeState {
    vsync: Vsync,
    side: Arc<Mutex<f64>>,
}

impl StatefulView for ContainerProbe {
    type State = ContainerProbeState;

    fn create_state(&self) -> Self::State {
        ContainerProbeState {
            vsync: self.vsync.clone(),
            side: Arc::clone(&self.side),
        }
    }
}

impl ViewState<ContainerProbe> for ContainerProbeState {
    fn build(&self, _view: &ContainerProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let side = *self.side.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedContainer::new(SizedBox::new(10.0, 10.0))
                .width(side)
                .height(side)
                .duration(RUN),
        )
    }
}

pub(crate) fn animated_container_interpolates_size_over_frames() {
    let vsync = Vsync::new();
    let side = Arc::new(Mutex::new(20.0));
    let probe = ContainerProbe {
        vsync: vsync.clone(),
        side: Arc::clone(&side),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);

    let width = |laid: &crate::common::LaidOut| -> f64 { laid.size(laid.current_root()).width };

    assert!(
        (width(&laid) - 20.0).abs() < 1e-3,
        "container starts at the initial 20px width, got {}",
        width(&laid),
    );

    *side.lock() = 100.0;
    laid.pump();
    laid.pump_for(FRAME); // detection (~20)

    let mut samples = Vec::new();
    for _ in 0..5 {
        laid.pump_for(FRAME);
        samples.push(width(&laid));
    }
    for pair in samples.windows(2) {
        assert!(
            pair[1] >= pair[0] - 1e-3,
            "the container width must grow monotonically: {samples:?}",
        );
    }
    assert!(
        (samples[4] - 100.0).abs() < 1.0,
        "the run ends at the new 100px width, got {}",
        samples[4],
    );
    assert!(
        samples[1] > 21.0 && samples[1] < 99.0,
        "an intermediate frame shows a partial width, got {}",
        samples[1],
    );
}

// ----------------------------------------------------------------------------
// Curve-only retarget — a rebuild that changes ONLY `curve` (not the target)
// must re-ease the run already in flight, not keep coasting on the curve
// captured at construction.
//
// A curve change swaps in a fresh curved animation over the SAME controller,
// without restarting it (a restart is strictly gated on a genuine target
// change). Both probes below
// start a genuine 0->target run under `Curves::Linear`, advance it to raw
// progress `0.4`, then swap ONLY the curve to a step at `0.5` (target held
// fixed) — a run-restart would also produce a value change, so the "target
// unchanged" half of each probe's second `pump()` is what isolates a curve
// swap from a retarget. Under Linear at `0.4` the eased value tracks the raw
// progress; the instant a step at `0.5` applies at that SAME raw progress
// (still `< 0.5`), the value must snap back to the run's `begin`.
// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// Non-cubic curve — compile-and-run gate
// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// Overshoot stays inside the property's domain
// ----------------------------------------------------------------------------

/// A container whose one animated property is set by `configure` from a shared
/// value, eased along `Curves::EaseOutBack` (which overshoots past the target).
#[derive(Clone, StatefulView)]
struct OvershootProbe {
    vsync: Vsync,
    value: Arc<Mutex<f64>>,
    configure: fn(AnimatedContainer, f64) -> AnimatedContainer,
}

struct OvershootProbeState {
    probe: OvershootProbe,
}

impl StatefulView for OvershootProbe {
    type State = OvershootProbeState;

    fn create_state(&self) -> Self::State {
        OvershootProbeState {
            probe: self.clone(),
        }
    }
}

impl ViewState<OvershootProbe> for OvershootProbeState {
    fn build(&self, _view: &OvershootProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let container = AnimatedContainer::new(SizedBox::new(10.0, 10.0))
            .duration(RUN)
            .curve(Curves::EaseOutBack);
        VsyncScope::new(
            self.probe.vsync.clone(),
            (self.probe.configure)(container, *self.probe.value.lock()),
        )
    }
}

/// Animate the configured property from `from` to `0` along the overshooting
/// curve, checking `check` on every 10 ms frame of the run.
fn overshoot_to_zero(
    configure: fn(AnimatedContainer, f64) -> AnimatedContainer,
    from: f64,
    check: fn(&LaidOut),
) {
    let vsync = Vsync::new();
    let value = Arc::new(Mutex::new(from));
    let probe = OvershootProbe {
        vsync: vsync.clone(),
        value: Arc::clone(&value),
        configure,
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);
    *value.lock() = 0.0;
    laid.pump();
    for _ in 0..12 {
        laid.pump_for(Duration::from_millis(10));
        check(&laid);
    }
}

/// An overshooting padding (16 → 0 along a back-out curve) never pushes the
/// child outside the container.
pub(crate) fn overshooting_padding_stays_non_negative() {
    overshoot_to_zero(
        |container, value| container.padding(EdgeInsets::all(value)),
        16.0,
        |laid| {
            let container = laid.find_by_render_type("RenderContainer");
            let child = laid.only_child(container);
            let offset = laid.offset(child);
            assert!(
                offset.dx >= 0.0 && offset.dy >= 0.0,
                "padding pushed the child to {offset:?}"
            );
        },
    );
}

/// An overshooting margin never makes the box smaller than its decorated area.
pub(crate) fn overshooting_margin_stays_non_negative() {
    overshoot_to_zero(
        |container, value| container.margin(EdgeInsets::all(value)),
        16.0,
        |laid| {
            let root = laid.find_by_render_type("RenderContainer");
            let outer = laid.size(root);
            let inner = laid.container_inner_size(root);
            assert!(
                outer.width >= inner.width && outer.height >= inner.height,
                "margin shrank the box: outer {outer:?}, inner {inner:?}"
            );
        },
    );
}

/// An overshooting width and height never go negative.
pub(crate) fn overshooting_size_stays_non_negative() {
    overshoot_to_zero(
        |container, value| container.width(value).height(value),
        10.0,
        |laid| {
            let size = laid.size(laid.find_by_render_type("RenderContainer"));
            assert!(
                size.width >= 0.0 && size.height >= 0.0,
                "negative size {size:?}"
            );
        },
    );
}

/// Owning motion refuses non-finite components before changing live goals.
pub(crate) fn non_finite_container_targets_preserve_the_last_admitted_layout() {
    let registry = Vsync::new();
    let mut animated = lay_out_animated(
        VsyncScope::new(
            registry.clone(),
            AnimatedContainer::new(SizedBox::new(10.0, 10.0))
                .width(f64::NAN)
                .height(f64::NAN),
        ),
        loose(200.0),
        registry.clone(),
    );
    assert_eq!(animated.size(animated.current_root()).width, 10.0);
    let tree = |width, height| {
        VsyncScope::new(
            registry.clone(),
            AnimatedContainer::new(SizedBox::square(10.0))
                .width(width)
                .height(height)
                .duration(RUN)
                .curve(Curves::Linear),
        )
    };
    animated.pump_widget(tree(20.0, 30.0));
    animated.pump_widget(tree(100.0, 90.0));
    animated.pump_for(Duration::from_millis(1));
    animated.pump_for(Duration::from_millis(40));
    let seam = animated.size(animated.current_root());
    assert!(seam.width > 20.0 && seam.width < 100.0);
    animated.pump_widget(tree(40.0, f64::INFINITY));
    assert_eq!(animated.size(animated.current_root()), seam);
    animated.pump_for(Duration::from_millis(60));
    let settled = animated.size(animated.current_root());
    assert_eq!((settled.width, settled.height), (100.0, 90.0));
}

// ----------------------------------------------------------------------------
// AnimatedContainer transform
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct TransformProbe {
    vsync: Vsync,
    transform: Arc<Mutex<Matrix4>>,
    color: Arc<Mutex<Color>>,
}

struct TransformProbeState {
    probe: TransformProbe,
}

impl StatefulView for TransformProbe {
    type State = TransformProbeState;

    fn create_state(&self) -> Self::State {
        TransformProbeState {
            probe: self.clone(),
        }
    }
}

impl ViewState<TransformProbe> for TransformProbeState {
    fn build(&self, _view: &TransformProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        VsyncScope::new(
            self.probe.vsync.clone(),
            AnimatedContainer::new(SizedBox::new(10.0, 10.0))
                .transform(*self.probe.transform.lock())
                .color(*self.probe.color.lock())
                .duration(RUN)
                .curve(Curves::Linear),
        )
    }
}

/// A scale-in from a collapsed transform: every intermediate layer matrix is the
/// finite uniform scale `scaling(s, s, 1)`, with `s` growing strictly between the
/// ends.
pub(crate) fn animated_container_animates_its_transform() {
    let vsync = Vsync::new();
    let transform = Arc::new(Mutex::new(Matrix4::scaling(0.0, 0.0, 1.0)));
    let probe = TransformProbe {
        vsync: vsync.clone(),
        transform: Arc::clone(&transform),
        color: Arc::new(Mutex::new(Color::BLACK)),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);
    *transform.lock() = Matrix4::IDENTITY;
    laid.pump();
    laid.pump_for(FRAME); // detection
    let mut scales = Vec::new();
    for _ in 0..3 {
        laid.pump_for(FRAME);
        let matrices = laid.transform_layer_matrices();
        let [matrix] = matrices.as_slice() else {
            panic!("one transform layer expected, got {matrices:?}");
        };
        let s = matrix.m[0];
        assert!(s > 0.0 && s < 1.0, "intermediate scale {s} in {matrix:?}");
        let expected = Matrix4::scaling(s, s, 1.0);
        for (index, (got, want)) in matrix.m.iter().zip(expected.m.iter()).enumerate() {
            assert!(
                (got - want).abs() < 1e-12,
                "element {index}: {got} vs {want} in {matrix:?}"
            );
        }
        scales.push(s);
    }
    assert!(
        scales.windows(2).all(|pair| pair[1] > pair[0]),
        "the scale grows: {scales:?}"
    );
}

/// The layer's uniform scale, from the one transform layer.
#[track_caller]
fn layer_scale(laid: &mut LaidOut) -> f64 {
    let matrices = laid.transform_layer_matrices();
    let [matrix] = matrices.as_slice() else {
        panic!("one transform layer expected, got {matrices:?}");
    };
    matrix.m[0]
}

/// A change to another property restarts the shared controller; the running
/// transform re-anchors at the scale shown now instead of snapping back to its start.
pub(crate) fn animated_container_reanchors_unchanged_properties_on_restart() {
    let vsync = Vsync::new();
    let transform = Arc::new(Mutex::new(Matrix4::scaling(0.0, 0.0, 1.0)));
    let color = Arc::new(Mutex::new(Color::BLACK));
    let probe = TransformProbe {
        vsync: vsync.clone(),
        transform: Arc::clone(&transform),
        color: Arc::clone(&color),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);
    *transform.lock() = Matrix4::IDENTITY;
    laid.pump();
    laid.pump_for(FRAME); // detection
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let before = layer_scale(&mut laid);
    assert!(before > 0.3 && before < 1.0, "half way: scale {before}");
    *color.lock() = Color::WHITE;
    laid.pump();
    laid.pump_for(FRAME); // detection
    laid.pump_for(FRAME);
    let after = layer_scale(&mut laid);
    assert!(
        after >= before && after < 1.0,
        "the transform continues from {before}: now {after}"
    );
}

/// A colour change restarts the shared controller; an unchanged collapsed, rotated
/// transform keeps its exact matrix on every frame instead of dropping its rotation.
pub(crate) fn animated_container_keeps_an_unchanged_collapsed_transform_on_restart() {
    let vsync = Vsync::new();
    let collapsed = Matrix4::rotation_z(1.0) * Matrix4::scaling(0.0, 1.0, 1.0);
    let color = Arc::new(Mutex::new(Color::BLACK));
    let probe = TransformProbe {
        vsync: vsync.clone(),
        transform: Arc::new(Mutex::new(collapsed)),
        color: Arc::clone(&color),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);
    *color.lock() = Color::WHITE;
    laid.pump();
    laid.pump_for(FRAME); // detection
    for frame in 0..3 {
        laid.pump_for(FRAME);
        // A collapsed matrix paints no layer, so read the render object.
        let container = laid.find_by_render_type("RenderContainer");
        let shown = laid.container_transform(container);
        assert_eq!(shown.map(|m| m.m), Some(collapsed.m), "frame {frame}");
    }
}

// ----------------------------------------------------------------------------
// AnimatedRotation
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct RotationProbe {
    vsync: Vsync,
    angle: Arc<Mutex<Angle>>,
    path: Arc<Mutex<RotationPath>>,
}

struct RotationProbeState {
    probe: RotationProbe,
}

impl StatefulView for RotationProbe {
    type State = RotationProbeState;

    fn create_state(&self) -> Self::State {
        RotationProbeState {
            probe: self.clone(),
        }
    }
}

impl ViewState<RotationProbe> for RotationProbeState {
    fn build(&self, _view: &RotationProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        VsyncScope::new(
            self.probe.vsync.clone(),
            AnimatedRotation::new(*self.probe.angle.lock(), SizedBox::new(10.0, 10.0))
                .path(*self.probe.path.lock())
                .duration(RUN)
                .curve(Curves::Linear),
        )
    }
}

/// The Z-rotation in turns of the one transform layer, `atan2(m[1][0], m[0][0])`;
/// the centring translation does not affect it.
#[track_caller]
fn layer_turns(laid: &mut LaidOut) -> f64 {
    let matrices = laid.transform_layer_matrices();
    let [matrix] = matrices.as_slice() else {
        panic!("one transform layer expected, got {matrices:?}");
    };
    matrix.get(1, 0).atan2(matrix.get(0, 0)) / std::f64::consts::TAU
}

/// Rotate 0 → ¾ turn and read the child's rotation, in turns, half way through
/// the run.
fn rotation_at_half_way(path: RotationPath) -> f64 {
    let vsync = Vsync::new();
    let angle = Arc::new(Mutex::new(Angle::ZERO));
    let probe = RotationProbe {
        vsync: vsync.clone(),
        angle: Arc::clone(&angle),
        path: Arc::new(Mutex::new(path)),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);
    *angle.lock() = Angle::from_turns(0.75);
    laid.pump();
    laid.pump_for(FRAME); // detection
    laid.pump_for(RUN / 2);
    layer_turns(&mut laid)
}

/// `Shorter` reaches ¾ turn by turning back: half way it shows −⅛ turn.
pub(crate) fn animated_rotation_takes_the_shorter_arc() {
    let turns = rotation_at_half_way(RotationPath::Shorter);
    assert!(
        (turns - -0.125).abs() < 1e-9,
        "shorter arc half way: {turns} turns"
    );
}

/// `Numeric` turns forward through the whole ¾: half way it shows ⅜ turn.
pub(crate) fn animated_rotation_takes_the_numeric_arc() {
    let turns = rotation_at_half_way(RotationPath::Numeric);
    assert!(
        (turns - 0.375).abs() < 1e-9,
        "numeric half way: {turns} turns"
    );
}

/// Changing only the path mid-run re-anchors from the angle shown now: a
/// `Numeric` 0 → ¾ turn switched to `Shorter` a quarter of the way brakes its
/// incoming velocity before turning back to the nearest equivalent.
pub(crate) fn animated_rotation_retargets_on_a_path_change() {
    let vsync = Vsync::new();
    let angle = Arc::new(Mutex::new(Angle::ZERO));
    let path = Arc::new(Mutex::new(RotationPath::Numeric));
    let probe = RotationProbe {
        vsync: vsync.clone(),
        angle: Arc::clone(&angle),
        path: Arc::clone(&path),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);
    *angle.lock() = Angle::from_turns(0.75);
    laid.pump();
    laid.pump_for(FRAME); // detection
    laid.pump_for(RUN / 4);
    let before = layer_turns(&mut laid);
    assert!(
        before > 0.1 && before < 0.3,
        "a quarter of the way: {before} turns"
    );
    *path.lock() = RotationPath::Shorter;
    laid.pump();
    assert!((layer_turns(&mut laid) - before).abs() < 1e-12);
    laid.pump_for(Duration::from_micros(100));
    assert!(
        layer_turns(&mut laid) > before,
        "changing the path preserves the incoming direction at the seam"
    );
    // Kept short so both candidate angles stay inside (-½, ½] turn, where the
    // read-back rotation is unambiguous.
    laid.pump_for(RUN / 2);
    let after = layer_turns(&mut laid);
    assert!(
        after < before,
        "the shorter arc turns back from {before}: now {after} turns"
    );
    assert!(after > 0.0, "still short of the target: {after} turns");
    laid.pump_for(RUN);
    assert!(
        (layer_turns(&mut laid) + 0.25).abs() < 1e-12,
        "the shorter path settles at the nearest equivalent"
    );
}
