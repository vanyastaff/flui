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

use crate::common::{lay_out_animated, loose, tight};
use flui_animation::Vsync;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedContainer, AnimatedOpacity, SizedBox, VsyncScope};
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
// Flutter parity: `ImplicitlyAnimatedWidgetState.didUpdateWidget`
// (`implicit_animations.dart` `didUpdateWidget`/`_createCurve` at tag `3.44.0`) swaps in a fresh
// `CurvedAnimation` over the SAME controller on a curve change, without
// restarting it (`controller.forward(from: 0.0)` is strictly gated on
// `_constructTweens()`, i.e. a genuine target change). Both probes below
// start a genuine 0->target run under `Curves::Linear`, advance it to raw
// progress `0.4`, then swap ONLY the curve to `Threshold(0.5)` (target held
// fixed) — a run-restart would also produce a value change, so the "target
// unchanged" half of each probe's second `pump()` is what isolates a curve
// swap from a retarget. Under Linear at `0.4` the eased value tracks the raw
// progress; the instant `Threshold(0.5)` applies at that SAME raw progress
// (still `< 0.5`), the value must snap back to the run's `begin`.
// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// Non-cubic curve — compile-and-run gate
// ----------------------------------------------------------------------------
