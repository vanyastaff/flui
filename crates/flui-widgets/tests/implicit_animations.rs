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
use flui_animation::{Curves, Vsync};
use flui_foundation::geometry::{Angle, EdgeInsets, Matrix4};
use flui_painting::styling::Color;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{
    AnimatedContainer, AnimatedOpacity, AnimatedRotation, Container, RotationPath, SizedBox,
    VsyncScope,
};
use parking_lot::Mutex;

/// A 100 ms run pumped in 20 ms frames spans the run in five steps.
const FRAME: Duration = Duration::from_millis(20);
const RUN: Duration = Duration::from_millis(100);

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

/// A NaN width or height reaches the container as NaN, the way a plain
/// `Container` takes it, instead of being replaced by zero (ADR-0149).
pub(crate) fn nan_size_passes_through_like_container() {
    let animated = lay_out_animated(
        VsyncScope::new(
            Vsync::new(),
            AnimatedContainer::new(SizedBox::new(10.0, 10.0))
                .width(f64::NAN)
                .height(f64::NAN),
        ),
        loose(200.0),
        Vsync::new(),
    );
    let plain = lay_out_animated(
        Container::new()
            .width(f64::NAN)
            .height(f64::NAN)
            .child(SizedBox::new(10.0, 10.0)),
        loose(200.0),
        Vsync::new(),
    );
    let size = |laid: &LaidOut| laid.try_size(laid.find_by_render_type("RenderContainer"));
    let (animated, plain) = (size(&animated), size(&plain));
    assert_eq!(
        format!("{animated:?}"),
        format!("{plain:?}"),
        "AnimatedContainer laid out NaN size as {animated:?}, Container as {plain:?}"
    );
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
    shown_turns(&laid)
}

/// The rotation, in turns, that `AnimatedRotation`'s animated transform applies
/// to its child, recovered from the matrix as `atan2(m[1][0], m[0][0])`.
fn shown_turns(laid: &LaidOut) -> f64 {
    let node = laid.find_by_render_type("RenderAnimatedTransform");
    let child = laid.only_child(node);
    let matrix = laid
        .pipeline_owner()
        .with(|owner| owner.transform_to(child, node))
        .expect("child below the rotation is laid out");
    matrix.get(1, 0).atan2(matrix.get(0, 0)) / std::f64::consts::TAU
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
/// `Numeric` 0 → ¾ turn switched to `Shorter` a quarter of the way turns back.
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
    let before = shown_turns(&laid);
    assert!(
        before > 0.1 && before < 0.3,
        "a quarter of the way: {before} turns"
    );
    *path.lock() = RotationPath::Shorter;
    laid.pump();
    laid.pump_for(FRAME); // detection
    // Kept short so both candidate angles stay inside (-½, ½] turn, where the
    // read-back rotation is unambiguous.
    laid.pump_for(RUN / 10);
    let after = shown_turns(&laid);
    assert!(
        after < before,
        "the shorter arc turns back from {before}: now {after} turns"
    );
    assert!(after > 0.0, "still short of the target: {after} turns");
}
