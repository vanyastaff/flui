//! Animated Box — the animation engine driving the real pipeline on GPU.
//!
//! The window breathes between red and blue: an `AnimationController`
//! bouncing 0 → 1 → 0 forever, with the fill crossfading through Oklab
//! (perceptually uniform — no muddy gray midpoint like gamma-sRGB lerp).
//! The box itself fills the window: the root hands a bare render child
//! tight window constraints, exactly like a bare `ColoredBox` under
//! the app root.
//!
//! The full production loop, no shortcuts:
//!
//! ```text
//! AnimationController::repeat(reverse) → registered with the UI runtime's Vsync
//!   → the UI runtime ticks Vsync once per frame (`UiRuntime::draw_frame`)
//!   → the controller's Listenable notification schedules the state's rebuild
//!     through its lifecycle rebuild handle → `build` recolors the
//!     leaf render object from the controller's current value
//!   → next frame, next tick → …
//! ```
//!
//! The mounted state owns a [`DrivenController`], bound to the ambient
//! `VsyncScope`. Dependency changes move its registry seat while preserving
//! run elapsed time. Disposal releases the seat and cancels the run.
//!
//! The loop is self-sustaining and STOPS sustaining itself the moment
//! the controller stops — no busy-looping while idle.
//!
//! Run with: cargo run -p flui --example animated_box_app
//!
//! Set `FLUI_FRAME_HISTOGRAM=1` to also log inter-tick wall-clock deltas
//! (median/p90/max) every [`WINDOW_SAMPLE_COUNT`] ticks — the real-window
//! pacing evidence for App.1's vsync-pacing exit criterion. Off by default
//! so the interactive demo is unaffected. This measures the SAME `build`
//! call the controller's notification drives, not a second synthetic
//! controller: the histogram is exactly the cadence this window's frame
//! loop delivers.
//!
//! The state acquires its rebuild handle during initialization. Each value
//! notification schedules a rebuild; the resulting view updates the render
//! object's color through the ordinary render-view path.

use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_animation::{Animation, AnimationController, DrivenController};
use flui_app::run_app;
use flui_foundation::Listenable;
use flui_foundation::geometry::Size;
use flui_objects::RenderColoredBox;
use flui_painting::styling::Color;
use flui_view::{
    BuildContext, IntoView, LifecycleContext, RenderView, StatefulView, StatelessView, View,
    ViewExt, ViewState,
};
use flui_widgets::VsyncScope;

/// Env var that turns the frame histogram on; see the module doc.
const FRAME_HISTOGRAM_ENV_VAR: &str = "FLUI_FRAME_HISTOGRAM";

/// Ticks collected per logged window. ~300 ticks at this controller's
/// wake-driven cadence is a several-second window — long enough to smooth
/// startup jitter without a long wait between log lines.
const WINDOW_SAMPLE_COUNT: usize = 300;

/// Accumulates inter-tick wall-clock deltas for the current histogram
/// window, draining and logging once [`WINDOW_SAMPLE_COUNT`] accumulate.
#[derive(Debug, Default)]
struct FrameHistogram {
    last_tick_at: Option<Instant>,
    deltas: Vec<Duration>,
}

impl FrameHistogram {
    /// Records `now` as a tick; logs and drains the window once full.
    fn record(&mut self, now: Instant) {
        if let Some(previous_tick_at) = self.last_tick_at {
            self.deltas.push(now.duration_since(previous_tick_at));
        }
        self.last_tick_at = Some(now);

        if self.deltas.len() < WINDOW_SAMPLE_COUNT {
            return;
        }
        let mut deltas = std::mem::take(&mut self.deltas);
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
}

/// Leaf render view: a colored box whose fill tracks `color`, recomputed
/// each build from the controller's current value.
#[derive(Clone, Debug)]
struct AnimatedBox {
    color: [f32; 4],
}

impl RenderView for AnimatedBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderColoredBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderColoredBox::new((self.color).map(|v| v), Size::new(60.0, 60.0))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_color((self.color).map(|v| v))
    }
}

flui_view::impl_render_view!(AnimatedBox);

/// Stateless root: `run_app` requires a `StatelessView` entry point, so the
/// actual stateful animation owner mounts one level down.
#[derive(Clone, Debug)]
pub struct App {
    red: Color,
    blue: Color,
    histogram_enabled: bool,
    histogram: Arc<parking_lot::Mutex<FrameHistogram>>,
}

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedBoxDemo {
            red: self.red,
            blue: self.blue,
            histogram_enabled: self.histogram_enabled,
            histogram: Arc::clone(&self.histogram),
        }
        .boxed()
    }
}

impl View for App {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl App {
    /// A ready-to-mount instance with the default red/blue palette.
    ///
    /// `App`'s fields are private, so this is the one public construction
    /// path both `main` (below) and the headless screenshot harness
    /// (`examples/screenshot.rs`, which captures the initial frame) go through.
    pub fn new() -> Self {
        Self {
            red: Color::rgb(244, 67, 54),
            blue: Color::rgb(33, 150, 243),
            histogram_enabled: std::env::var_os(FRAME_HISTOGRAM_ENV_VAR).is_some(),
            histogram: Arc::new(parking_lot::Mutex::new(FrameHistogram::default())),
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// The animated leaf host. Its mounted state owns the controller and schedules
/// rebuilds through its lifecycle handle on value notification.
#[derive(Clone)]
struct AnimatedBoxDemo {
    red: Color,
    blue: Color,
    histogram_enabled: bool,
    histogram: Arc<parking_lot::Mutex<FrameHistogram>>,
}

impl View for AnimatedBoxDemo {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for AnimatedBoxDemo {
    type State = AnimatedBoxDemoState;

    fn create_state(&self) -> Self::State {
        AnimatedBoxDemoState {
            controller: AnimationController::builder(Duration::from_millis(1400)).build_on(None),
        }
    }
}

struct AnimatedBoxDemoState {
    controller: DrivenController,
}

impl ViewState<AnimatedBoxDemo> for AnimatedBoxDemoState {
    /// Lifecycle-only (ADR-0021): registers with the
    /// ambient `VsyncScope` and starts the bounce here, never from `build`.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let rebuild = ctx.rebuild_handle();
        self.controller
            .controller()
            .add_listener(std::rc::Rc::new(move || {
                rebuild.schedule(flui_foundation::RebuildReason::StateChange);
            }));
        let _ = self.controller.rebind(VsyncScope::maybe_of(ctx).as_ref());
        // Bounce 0 → 1 → 0 forever. A freshly built controller always
        // accepts `repeat()`.
        self.controller
            .controller()
            .repeat(true)
            .expect("a freshly created controller accepts repeat()");
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let _ = self.controller.rebind(VsyncScope::maybe_of(ctx).as_ref());
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }

    fn build(&self, view: &AnimatedBoxDemo, _ctx: &dyn BuildContext) -> impl IntoView {
        if view.histogram_enabled {
            view.histogram.lock().record(Instant::now());
        }
        let value = self.controller.controller().value();
        let color = Color::lerp(view.red, view.blue, value).to_f32_array();
        AnimatedBox { color }.boxed()
    }
}

fn main() {
    let app = App::new();

    // This "enabled" log (like any log before `run_app` installs the
    // process-global subscriber) is dropped by the no-op default
    // dispatcher — harmless, since the periodic histogram windows below
    // only start firing once the event loop is running and the subscriber
    // is up. Same accepted pattern as `vertical_slice_demo`'s
    // `frame_histogram` module.
    if app.histogram_enabled {
        tracing::info!(
            window_sample_count = WINDOW_SAMPLE_COUNT,
            "frame histogram enabled ({FRAME_HISTOGRAM_ENV_VAR}=1)"
        );
    }

    // The bounce itself starts once mounted -- see
    // `AnimatedBoxDemoState::init_state`, which registers with the ambient
    // `VsyncScope` before calling `repeat(true)`.
    run_app(app);
}
