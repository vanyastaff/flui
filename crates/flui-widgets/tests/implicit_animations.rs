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

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{lay_out_animated, loose, tight};
use flui_animation::{ElasticOutCurve, Vsync};
use flui_geometry::{EdgeInsets, px};
use flui_types::{Alignment, Offset};
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{
    AnimatedAlign, AnimatedContainer, AnimatedOpacity, AnimatedPadding, SizedBox, VsyncScope,
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
    target: Arc<Mutex<f32>>,
}

struct OpacityProbeState {
    vsync: Vsync,
    target: Arc<Mutex<f32>>,
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

#[test]
fn animated_opacity_first_frame_holds_target_with_no_motion() {
    let vsync = Vsync::new();
    let target = Arc::new(Mutex::new(0.25));
    let probe = OpacityProbe {
        vsync: vsync.clone(),
        target: Arc::clone(&target),
    };
    let laid = lay_out_animated(probe, tight(100.0, 50.0), vsync);

    // No configuration change yet: the widget sits AT its target, no animation.
    let root = laid.current_root();
    assert!(
        (laid.opacity(root) - 0.25).abs() < 1e-4,
        "first frame shows the target opacity, got {}",
        laid.opacity(root),
    );
}

#[test]
fn animated_opacity_interpolates_to_a_new_target_over_frames() {
    let vsync = Vsync::new();
    let target = Arc::new(Mutex::new(0.0));
    let probe = OpacityProbe {
        vsync: vsync.clone(),
        target: Arc::clone(&target),
    };
    let mut laid = lay_out_animated(probe, tight(100.0, 50.0), vsync);

    assert!(
        laid.opacity(laid.current_root()).abs() < 1e-4,
        "starts transparent"
    );

    // Change the target and rebuild the probe: the AnimatedOpacity reconciles,
    // sees a new opacity in did_update_view, and starts a run from 0.0 to 1.0.
    *target.lock() = 1.0;
    laid.pump();

    // The detection frame (first pump after the retarget) still holds the
    // run-start value (~0.0): the controller's first tick is elapsed 0.
    laid.pump_for(FRAME);
    assert!(
        laid.opacity(laid.current_root()) < 0.1,
        "first frame after retarget holds near the start, got {}",
        laid.opacity(laid.current_root()),
    );

    // Five 20 ms frames over the 100 ms run climb monotonically toward 1.0.
    let mut samples = Vec::new();
    for _ in 0..5 {
        laid.pump_for(FRAME);
        samples.push(laid.opacity(laid.current_root()));
    }
    for pair in samples.windows(2) {
        assert!(
            pair[1] >= pair[0] - 1e-6,
            "opacity must not regress across frames: {samples:?}",
        );
    }
    let intermediate = samples[1];
    assert!(
        intermediate > 0.05 && intermediate < 0.95,
        "an intermediate frame shows partial opacity, got {intermediate}",
    );
    assert!(
        (samples[4] - 1.0).abs() < 1e-3,
        "the run ends at the new target (1.0), got {}",
        samples[4],
    );
}

#[test]
fn animated_opacity_retargets_from_the_current_value_midflight() {
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

#[derive(Clone, StatefulView)]
struct PaddingProbe {
    vsync: Vsync,
    target: Arc<Mutex<f32>>,
}

struct PaddingProbeState {
    vsync: Vsync,
    target: Arc<Mutex<f32>>,
}

impl StatefulView for PaddingProbe {
    type State = PaddingProbeState;

    fn create_state(&self) -> Self::State {
        PaddingProbeState {
            vsync: self.vsync.clone(),
            target: Arc::clone(&self.target),
        }
    }
}

impl ViewState<PaddingProbe> for PaddingProbeState {
    fn build(&self, _view: &PaddingProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let inset = *self.target.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedPadding::new(EdgeInsets::all(px(inset)), SizedBox::new(20.0, 20.0))
                .duration(RUN),
        )
    }
}

#[test]
fn animated_padding_interpolates_child_offset_over_frames() {
    let vsync = Vsync::new();
    let target = Arc::new(Mutex::new(0.0));
    let probe = PaddingProbe {
        vsync: vsync.clone(),
        target: Arc::clone(&target),
    };
    // Loose-enough tight box so the padded child has room to shift.
    let mut laid = lay_out_animated(probe, tight(100.0, 100.0), vsync);

    let child_offset = |laid: &common::LaidOut| -> Offset {
        let root = laid.current_root();
        laid.offset(laid.only_child(root))
    };

    // Padding starts at 0 → child sits at the origin.
    assert!(
        child_offset(&laid).dx.get().abs() < 1e-4,
        "child starts at x=0"
    );

    // Animate padding to 20px on all sides.
    *target.lock() = 20.0;
    laid.pump();
    laid.pump_for(FRAME); // detection (~0 padding)

    let mut samples = Vec::new();
    for _ in 0..5 {
        laid.pump_for(FRAME);
        samples.push(child_offset(&laid).dx.get());
    }
    for pair in samples.windows(2) {
        assert!(
            pair[1] >= pair[0] - 1e-4,
            "the left inset must grow monotonically: {samples:?}",
        );
    }
    assert!(
        (samples[4] - 20.0).abs() < 0.5,
        "the run ends at 20px of left padding, got {}",
        samples[4],
    );
    let intermediate = samples[1];
    assert!(
        intermediate > 0.5 && intermediate < 19.5,
        "an intermediate frame shows partial padding, got {intermediate}",
    );
}

// ----------------------------------------------------------------------------
// AnimatedAlign
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct AlignProbe {
    vsync: Vsync,
    alignment: Arc<Mutex<Alignment>>,
}

struct AlignProbeState {
    vsync: Vsync,
    alignment: Arc<Mutex<Alignment>>,
}

impl StatefulView for AlignProbe {
    type State = AlignProbeState;

    fn create_state(&self) -> Self::State {
        AlignProbeState {
            vsync: self.vsync.clone(),
            alignment: Arc::clone(&self.alignment),
        }
    }
}

impl ViewState<AlignProbe> for AlignProbeState {
    fn build(&self, _view: &AlignProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedAlign::new(*self.alignment.lock(), SizedBox::new(20.0, 20.0)).duration(RUN),
        )
    }
}

#[test]
fn animated_align_interpolates_child_position_over_frames() {
    let vsync = Vsync::new();
    let alignment = Arc::new(Mutex::new(Alignment::TOP_LEFT));
    let probe = AlignProbe {
        vsync: vsync.clone(),
        alignment: Arc::clone(&alignment),
    };
    // 100×100 box, 20×20 child: TOP_LEFT → child at (0,0); BOTTOM_RIGHT → (80,80).
    let mut laid = lay_out_animated(probe, tight(100.0, 100.0), vsync);

    let child_x = |laid: &common::LaidOut| -> f32 {
        let root = laid.current_root();
        laid.offset(laid.only_child(root)).dx.get()
    };

    assert!(
        child_x(&laid).abs() < 1e-4,
        "TOP_LEFT starts the child at x=0"
    );

    *alignment.lock() = Alignment::BOTTOM_RIGHT;
    laid.pump();
    laid.pump_for(FRAME); // detection (~top-left)

    let mut samples = Vec::new();
    for _ in 0..5 {
        laid.pump_for(FRAME);
        samples.push(child_x(&laid));
    }
    for pair in samples.windows(2) {
        assert!(
            pair[1] >= pair[0] - 1e-4,
            "the child must slide right monotonically: {samples:?}",
        );
    }
    assert!(
        (samples[4] - 80.0).abs() < 1.0,
        "the run ends with the child at the bottom-right (x=80), got {}",
        samples[4],
    );
    assert!(
        samples[1] > 1.0 && samples[1] < 79.0,
        "an intermediate frame shows the child partway across, got {}",
        samples[1],
    );
}

// ----------------------------------------------------------------------------
// AnimatedContainer
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct ContainerProbe {
    vsync: Vsync,
    side: Arc<Mutex<f32>>,
}

struct ContainerProbeState {
    vsync: Vsync,
    side: Arc<Mutex<f32>>,
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

#[test]
fn animated_container_interpolates_size_over_frames() {
    let vsync = Vsync::new();
    let side = Arc::new(Mutex::new(20.0));
    let probe = ContainerProbe {
        vsync: vsync.clone(),
        side: Arc::clone(&side),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);

    let width = |laid: &common::LaidOut| -> f32 { laid.size(laid.current_root()).width.get() };

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
// Non-cubic curve — compile-and-run gate
// ----------------------------------------------------------------------------

/// Probe that wires an `ElasticOutCurve` into `AnimatedOpacity`.
///
/// This struct would FAIL TO COMPILE if `AnimatedOpacity::curve()` still only
/// accepted `Cubic` — `ElasticOutCurve` is a distinct type that does not
/// implement `Into<Cubic>`.
#[derive(Clone, StatefulView)]
struct ElasticOpacityProbe {
    vsync: Vsync,
    target: Arc<Mutex<f32>>,
}

struct ElasticOpacityProbeState {
    vsync: Vsync,
    target: Arc<Mutex<f32>>,
}

impl StatefulView for ElasticOpacityProbe {
    type State = ElasticOpacityProbeState;

    fn create_state(&self) -> Self::State {
        ElasticOpacityProbeState {
            vsync: self.vsync.clone(),
            target: Arc::clone(&self.target),
        }
    }
}

impl ViewState<ElasticOpacityProbe> for ElasticOpacityProbeState {
    fn build(&self, _view: &ElasticOpacityProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedOpacity::new(*self.target.lock(), SizedBox::new(100.0, 50.0))
                .duration(RUN)
                // Key line: passes a non-Cubic curve — compile error without type erasure.
                .curve(ElasticOutCurve::default()),
        )
    }
}

#[test]
fn animated_opacity_accepts_non_cubic_elastic_out_curve() {
    // The elastic-out curve overshoots the target before settling, so do not
    // assert monotonicity.  Only check start and convergence to the target.
    let vsync = Vsync::new();
    let target = Arc::new(Mutex::new(0.0));
    let probe = ElasticOpacityProbe {
        vsync: vsync.clone(),
        target: Arc::clone(&target),
    };
    let mut laid = lay_out_animated(probe, tight(100.0, 50.0), vsync);

    assert!(
        laid.opacity(laid.current_root()).abs() < 1e-4,
        "starts transparent"
    );

    *target.lock() = 1.0;
    laid.pump();
    laid.pump_for(FRAME); // detection frame (~0.0)
    for _ in 0..5 {
        laid.pump_for(FRAME);
    }
    let final_opacity = laid.opacity(laid.current_root());
    assert!(
        (final_opacity - 1.0).abs() < 0.05,
        "elastic-out run converges to the target (1.0), got {final_opacity}",
    );
}
