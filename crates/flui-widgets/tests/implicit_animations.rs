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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{lay_out_animated, loose, tight};
use flui_animation::{Curves, Threshold, Vsync};
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedBuilder, AnimatedContainer, AnimatedOpacity, SizedBox, VsyncScope};
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

#[test]
fn animated_container_interpolates_size_over_frames() {
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

#[derive(Clone, StatefulView)]
struct ZeroDurationContainerProbe {
    vsync: Vsync,
    side: Arc<Mutex<f64>>,
}

struct ZeroDurationContainerProbeState {
    vsync: Vsync,
    side: Arc<Mutex<f64>>,
}

impl StatefulView for ZeroDurationContainerProbe {
    type State = ZeroDurationContainerProbeState;

    fn create_state(&self) -> Self::State {
        ZeroDurationContainerProbeState {
            vsync: self.vsync.clone(),
            side: Arc::clone(&self.side),
        }
    }
}

impl ViewState<ZeroDurationContainerProbe> for ZeroDurationContainerProbeState {
    fn build(&self, _view: &ZeroDurationContainerProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let side = *self.side.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedContainer::new(SizedBox::new(10.0, 10.0))
                .width(side)
                .height(side)
                .duration(Duration::ZERO),
        )
    }
}

/// Counts `element_rebuilt` observations for `AnimatedBuilder` specifically —
/// `element_rebuilt` fires only with a `TreeObserver` installed, and this is
/// the "built exactly once" oracle the same-drain-absorption fix needs.
#[derive(Default)]
struct AnimatedBuilderRebuildCounter {
    count: AtomicUsize,
}

impl AnimatedBuilderRebuildCounter {
    fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

impl flui_foundation::observe::TreeObserver for AnimatedBuilderRebuildCounter {
    fn element_rebuilt(&self, event: &flui_foundation::observe::ElementRebuilt) {
        if event.view_type_id == std::any::TypeId::of::<AnimatedBuilder>() {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// A `Duration::ZERO` implicit animation retargeted in a rebuild (issue
/// #1171's synchronous settle) lays out the new target on the SAME pump
/// that observes the widget's new configuration — `did_update_view`'s
/// `restart_from_zero()` snaps `ImplicitController`'s value to the new
/// target before `build()` runs again for the same element, so the child
/// widget tree `build()` produces already reflects it, with no extra frame
/// needed.
///
/// **The one-frame deferral is gone (issue #1180).** The synchronous settle
/// still fires a value notification (the value DID move), and
/// `AnimatedBuilder`'s listener schedules its own rebuild the same way any
/// out-of-frame `Listenable` change would — through the owner's external
/// inbox. That inbox used to be drained only once, at the START of a drain,
/// so a schedule landing mid-drain (this one: the container's retargeting
/// build runs first, and its synchronous settle notifies `AnimatedBuilder`
/// AFTER that initial drain already happened) sat until the NEXT
/// `build_scope` — a whole extra frame for a widget that, on paper, "lays out
/// the new target on the same pump." `BuildOwner::drain_build_scope` now
/// absorbs the inbox at the top of EVERY heap pop, not just the first, so
/// the retargeting pump's own drain reaches `AnimatedBuilder` too: this
/// test is the pin for that fix — it flips from "one rebuild left queued for
/// the next pump" to "built once, same pump, nothing left".
///
/// Red-check: gate `forward_from`'s settle on distance alone (drop
/// `run_duration.is_zero()`) — the FIRST `assert_eq!` below (layout on the
/// retargeting pump) fails; the width still reads the old target.
#[test]
fn zero_duration_retarget_lays_out_the_new_target_on_the_same_pump() {
    let vsync = Vsync::new();
    let side = Arc::new(Mutex::new(20.0));
    let probe = ZeroDurationContainerProbe {
        vsync: vsync.clone(),
        side: Arc::clone(&side),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync.clone());

    let width = |laid: &crate::common::LaidOut| -> f64 { laid.size(laid.current_root()).width };
    assert!((width(&laid) - 20.0).abs() < 1e-3, "sanity: initial width");
    assert!(
        !vsync.has_running(),
        "sanity: nothing is animating before the retarget"
    );

    let animated_builder_rebuilds = Arc::new(AnimatedBuilderRebuildCounter::default());
    laid.build_owner_mut()
        .set_tree_observer(Arc::clone(&animated_builder_rebuilds)
            as Arc<dyn flui_foundation::observe::TreeObserver>);

    *side.lock() = 100.0;
    laid.pump();

    assert!(
        (width(&laid) - 100.0).abs() < 1e-3,
        "a Duration::ZERO retarget must lay out the new target on the SAME \
         pump that rebuilds with it, got {}",
        width(&laid)
    );
    assert_eq!(
        animated_builder_rebuilds.count(),
        1,
        "sanity: exactly one rebuild — `AnimatedBuilder` already built exactly once on the \
         PRE-#1180 behavior too, just a whole pump later; the two assertions below, not this \
         count, are the actual #1180 discriminators (WHEN it built, not how many times)"
    );
    assert_eq!(
        laid.build_owner_mut().pending_external_builds(),
        0,
        "same-drain absorption must leave nothing queued for the next pump — red before \
         #1180's fix: the notification is still sitting in the inbox here"
    );
    assert!(
        !laid.build_owner_mut().has_dirty_elements(),
        "nothing should be left dirty after the retargeting pump"
    );
    assert!(
        !vsync.has_running(),
        "a synchronous zero-duration settle must leave nothing running for \
         the frame loop to keep the window open for"
    );

    // A further tick is a genuine no-op: nothing queued, nothing running,
    // layout unchanged. This is the OTHER #1180 discriminator alongside
    // `pending_external_builds() == 0` above: on the pre-#1180 behavior, the
    // deferred rebuild would have landed on THIS tick, so the count staying
    // at 1 here — not the count after the retargeting pump — is what proves
    // the rebuild already happened during the retargeting pump itself.
    laid.tick();
    assert_eq!(
        animated_builder_rebuilds.count(),
        1,
        "no extra rebuild from an idle tick — red before #1180's fix, which would show 2 here"
    );
    assert!(
        (width(&laid) - 100.0).abs() < 1e-3,
        "layout unchanged by the idle tick"
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

#[derive(Clone, StatefulView)]
struct CurveSwapContainerProbe {
    vsync: Vsync,
    side: Arc<Mutex<f64>>,
    use_threshold_curve: Arc<Mutex<bool>>,
}

struct CurveSwapContainerProbeState {
    vsync: Vsync,
    side: Arc<Mutex<f64>>,
    use_threshold_curve: Arc<Mutex<bool>>,
}

impl StatefulView for CurveSwapContainerProbe {
    type State = CurveSwapContainerProbeState;

    fn create_state(&self) -> Self::State {
        CurveSwapContainerProbeState {
            vsync: self.vsync.clone(),
            side: Arc::clone(&self.side),
            use_threshold_curve: Arc::clone(&self.use_threshold_curve),
        }
    }
}

impl ViewState<CurveSwapContainerProbe> for CurveSwapContainerProbeState {
    fn build(&self, _view: &CurveSwapContainerProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let side = *self.side.lock();
        let widget = AnimatedContainer::new(SizedBox::new(10.0, 10.0))
            .width(side)
            .height(side)
            .duration(RUN);
        let widget = if *self.use_threshold_curve.lock() {
            widget.curve(Threshold::new(0.5))
        } else {
            widget.curve(Curves::Linear)
        };
        VsyncScope::new(self.vsync.clone(), widget)
    }
}

/// The `AnimatedBuilder`-path sibling of the `AnimatedOpacity` curve-swap
/// test above: `AnimatedContainer` rebuilds its child every tick, so this
/// also proves the curve threads through `ImplicitController::set_curve`
/// (shared by every multi-property `OptTween`), not just `ImplicitAnimation`.
#[test]
fn animated_container_curve_only_change_reapplies_the_new_curve_mid_flight() {
    let vsync = Vsync::new();
    let side = Arc::new(Mutex::new(20.0));
    let use_threshold_curve = Arc::new(Mutex::new(false));
    let probe = CurveSwapContainerProbe {
        vsync: vsync.clone(),
        side: Arc::clone(&side),
        use_threshold_curve: Arc::clone(&use_threshold_curve),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);
    let width = |laid: &crate::common::LaidOut| -> f64 { laid.size(laid.current_root()).width };
    assert!(
        (width(&laid) - 20.0).abs() < 1e-3,
        "starts at the initial 20px width"
    );

    // Start a genuine 20 -> 100 run under the Linear curve.
    *side.lock() = 100.0;
    laid.pump();
    laid.pump_for(FRAME); // detection frame (still ~20px), see other tests in this file

    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let before_swap = width(&laid);
    assert!(
        (before_swap - 52.0).abs() < 5.0,
        "sanity: Linear curve at raw progress 0.4 over a 20px -> 100px span \
         should read ~52px, got {before_swap}",
    );

    // Swap ONLY the curve (side still 100.0, unchanged).
    *use_threshold_curve.lock() = true;
    laid.pump();

    let after_swap = width(&laid);
    assert!(
        (after_swap - 20.0).abs() < 5.0,
        "the new Threshold(0.5) curve must apply to the run already in \
         flight (raw progress 0.4, below the threshold): expected ~20px \
         (the run's begin), got {after_swap} (this fails against the \
         pre-fix code, which keeps easing on the curve captured at \
         construction and would still read ~{before_swap})",
    );
}

// ----------------------------------------------------------------------------
// Non-cubic curve — compile-and-run gate
// ----------------------------------------------------------------------------
