//! Frame-time evidence for Core.1's 60 fps criterion (env-gated, off by
//! default so the interactive demo is unaffected).
//!
//! Set `FLUI_FRAME_HISTOGRAM` to any value (e.g. `=1`) before `cargo run
//! --example vertical_slice_demo` to wrap the demo root in [`HistogramProbe`],
//! which mounts a free-running [`AnimationController`] (same pattern as
//! `examples/animated_box_app.rs`) registered with the ambient `VsyncScope` —
//! so its ticks are paced by the real per-frame `Vsync::tick_all` the
//! mounted realm drives every frame. Its listener records the wall-clock
//! [`Instant`] delta since the previous tick; every [`WINDOW_SAMPLE_COUNT`]
//! deltas it sorts the window and logs median/p90/max via `tracing::info!`.
//!
//! This controller is independent of [`tree::DemoRoot`](super::tree)'s own
//! `AnimatedContainer` — it exists purely to keep the frame loop busy and to
//! observe it, so the histogram reflects genuine per-frame wall-clock cost
//! on this machine's real window, not a headless pipeline-only measurement
//! (which would run at µs scale with no GPU/window and could never fail).
//!
//! The listener only records timestamps and logs; it never touches the
//! element/render tree or schedules a rebuild, so it stays outside the
//! controller-tick frame phase's setState/rebuild restriction.

use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_animation::{AnimationController, Vsync, VsyncRegistration};
use flui_foundation::Listenable;
use flui_view::{
    BuildContext, BuildContextExt, IntoView, StatefulView, StatelessView, View, ViewState,
};
use flui_widgets::VsyncScope;
use parking_lot::Mutex;

/// Env var that turns the histogram on. Unset (the default): zero overhead,
/// identical behavior to the plain interactive demo.
const ENABLE_ENV_VAR: &str = "FLUI_FRAME_HISTOGRAM";

/// Deltas collected per logged window. ~300 ticks at a wake-driven ~60 Hz
/// loop is a ~5 s window — long enough to smooth out startup jitter without
/// making the operator wait too long between log lines.
const WINDOW_SAMPLE_COUNT: usize = 300;

/// The controller's own cycle length. Cosmetic: with no other listener
/// driving the tree, only the *tick cadence* (paced by the realm's per-frame
/// `Vsync::tick_all`) is measured, not this value.
const CONTROLLER_CYCLE: Duration = Duration::from_millis(1400);

/// Accumulates inter-tick deltas for the current window.
#[derive(Default)]
struct TickWindow {
    last_tick_at: Option<Instant>,
    deltas: Vec<Duration>,
}

impl TickWindow {
    /// Records `now` as a tick, returning the completed window's deltas once
    /// [`WINDOW_SAMPLE_COUNT`] have accumulated (draining it for the next
    /// window), or `None` while the window is still filling.
    fn record(&mut self, now: Instant) -> Option<Vec<Duration>> {
        if let Some(previous_tick_at) = self.last_tick_at {
            self.deltas.push(now.duration_since(previous_tick_at));
        }
        self.last_tick_at = Some(now);

        (self.deltas.len() >= WINDOW_SAMPLE_COUNT).then(|| std::mem::take(&mut self.deltas))
    }
}

/// Sorts `deltas` and logs median/p90/max at info level.
///
/// `deltas` must be non-empty — the only caller is [`TickWindow::record`],
/// gated on `deltas.len() >= WINDOW_SAMPLE_COUNT` (a `const` `> 0`).
fn log_window(mut deltas: Vec<Duration>) {
    deltas.sort_unstable();
    let sample_count = deltas.len();
    let median = deltas[sample_count / 2];
    let p90 = deltas[sample_count * 9 / 10];
    let max = deltas[sample_count - 1];

    tracing::info!(
        sample_count,
        median_ms = median.as_secs_f64() * 1000.0,
        p90_ms = p90.as_secs_f64() * 1000.0,
        max_ms = max.as_secs_f64() * 1000.0,
        "frame histogram window"
    );
}

/// Wraps `child` with the free-running histogram probe. A no-op wrapper
/// (`controller: None`) when [`ENABLE_ENV_VAR`] is unset — identical
/// behavior to mounting `child` directly.
#[derive(Clone)]
pub struct HistogramProbe<V> {
    child: V,
    controller: Option<Arc<AnimationController>>,
}

impl<V: View + Clone + 'static> HistogramProbe<V> {
    /// Builds the wrapper, installing the recording listener now (so it is
    /// live the instant the controller starts ticking) iff
    /// [`ENABLE_ENV_VAR`] is set. The controller itself does not start
    /// running here — no `BuildContext` exists yet — that happens once
    /// mounted, in [`HistogramProbeState::init_state`], after registering
    /// with the ambient `VsyncScope`.
    pub fn wrap(child: V) -> Self {
        let Some(()) = std::env::var_os(ENABLE_ENV_VAR).map(|_| ()) else {
            return Self {
                child,
                controller: None,
            };
        };

        let controller = AnimationController::without_ticker(CONTROLLER_CYCLE);
        let window = Arc::new(Mutex::new(TickWindow::default()));
        controller.add_listener(Arc::new(move || {
            if let Some(deltas) = window.lock().record(Instant::now()) {
                log_window(deltas);
            }
        }));

        tracing::info!(
            window_sample_count = WINDOW_SAMPLE_COUNT,
            "frame histogram enabled ({ENABLE_ENV_VAR}=1)"
        );

        Self {
            child,
            controller: Some(Arc::new(controller)),
        }
    }
}

impl<V: View + Clone + 'static> View for HistogramProbe<V> {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl<V: View + Clone + 'static> StatelessView for HistogramProbe<V> {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        HistogramProbeInner {
            child: self.child.clone(),
            controller: self.controller.clone(),
        }
    }
}

/// The `StatefulView` layer, one level in — mirrors `animated_box_app.rs`'s
/// `App`/`AnimatedBoxDemo` split: the outer `StatelessView` is what `run_app`
/// requires at the root, and this inner layer holds the actual `init_state`/
/// `dispose` lifecycle the `Vsync` registration needs.
#[derive(Clone)]
struct HistogramProbeInner<V> {
    child: V,
    controller: Option<Arc<AnimationController>>,
}

impl<V: View + Clone + 'static> View for HistogramProbeInner<V> {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl<V: View + Clone + 'static> StatefulView for HistogramProbeInner<V> {
    type State = HistogramProbeState;

    fn create_state(&self) -> Self::State {
        HistogramProbeState {
            controller: self.controller.clone(),
            registration: None,
        }
    }
}

struct HistogramProbeState {
    controller: Option<Arc<AnimationController>>,
    /// The ambient `Vsync` this state registered with, plus the
    /// registration handle -- stays `None` (and the controller never runs)
    /// when the probe is disabled, or when mounted with no `VsyncScope`
    /// above it.
    registration: Option<(Vsync, VsyncRegistration)>,
}

impl<V: View + Clone + 'static> ViewState<HistogramProbeInner<V>> for HistogramProbeState {
    /// Lifecycle-only (ADR-0021, port-check trigger #22): registers with the
    /// ambient `VsyncScope` and starts the free-running cycle here, never
    /// from `build`.
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let Some(controller) = self.controller.as_ref() else {
            return;
        };
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register((**controller).clone());
            self.registration = Some((vsync, registration));
        }
        controller
            .repeat(true)
            .expect("a freshly created controller accepts repeat()");
    }

    fn dispose(&mut self) {
        if let Some((vsync, registration)) = self.registration.take() {
            vsync.unregister(registration);
        }
    }

    fn build(&self, view: &HistogramProbeInner<V>, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}
