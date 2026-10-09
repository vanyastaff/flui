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
use flui_view::{IntoView, ViewState};
use flui_widgets::{
    AnimatedAlign, AnimatedContainer, AnimatedOpacity, AnimatedPadding, AnimatedRotation,
    Container, RotationPath, SizedBox, VsyncScope,
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

pub(crate) fn changing_only_the_curve_keeps_the_existing_run_timeline() {
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
    let eased = laid.opacity(laid.current_root());
    assert!(
        eased > 0.0 && eased < linear,
        "the new curve eases the existing progress"
    );
    laid.pump_for(RUN - FRAME);
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
    let before = layer_turns(&mut laid);
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
    let after = layer_turns(&mut laid);
    assert!(
        after < before,
        "the shorter arc turns back from {before}: now {after} turns"
    );
    assert!(after > 0.0, "still short of the target: {after} turns");
}
