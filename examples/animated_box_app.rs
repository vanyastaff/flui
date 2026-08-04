//! Animated Box — the animation engine driving the real pipeline on GPU.
//!
//! The window breathes between red and blue: an `AnimationController`
//! bouncing 0 → 1 → 0 forever, with the fill crossfading through Oklab
//! (perceptually uniform — no muddy gray midpoint like gamma-sRGB lerp).
//! The box itself fills the window: the root hands a bare render child
//! tight window constraints, exactly like a bare `ColoredBox` under
//! Flutter's root.
//!
//! The full production loop, no shortcuts:
//!
//! ```text
//! AnimationController::repeat(reverse) → Ticker on the GLOBAL Scheduler
//!   → ticker registration fires the scheduler's frame-scheduled hook
//!   → AppRuntime's frame_wake_callback wakes the platform
//!   → runner pumps handle_begin_frame/handle_draw_frame on the frame
//!   → the controller's Listenable notification marks this AnimatedView's
//!     element dirty (see `flui_view::AnimatedView`) → `build` recolors the
//!     leaf render object from the controller's current value
//!   → next tick re-registers the ticker → next wake → …
//! ```
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
//! # Why `AnimatedView`, not a direct render-object mutation
//!
//! An earlier version of this example reached into the render pipeline
//! directly from the ticker listener (`AppBinding::instance().render_pipeline_mut()`)
//! to mutate the mounted `RenderColoredBox` in place, bypassing the widget
//! tree. That ambient process-wide reach retired along with `AppBinding`
//! (ADR-0027 step 3: `AppRuntime`/`UiRealm` are deliberately `pub(crate)`,
//! not a public escape hatch) — an application author now drives per-tick
//! recoloring the idiomatic way Flutter's own `AnimatedWidget` does:
//! subscribe an `AnimatedView` to the controller's `Listenable` and let the
//! framework mark it dirty and rebuild on every tick.

use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_animation::{Animation, AnimationController};
use flui_app::{Scheduler, run_app};
use flui_foundation::{HasInstance, Listenable};
use flui_objects::RenderColoredBox;
use flui_types::{Color, Size, geometry::px};
use flui_view::{
    AnimatedView, BuildContext, IntoView, RenderView, StatefulView, StatelessView, View, ViewExt,
    ViewState, impl_animated_view,
};

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
        RenderColoredBox::new(self.color, Size::new(px(60.0), px(60.0)))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_color(self.color);
    }
}

flui_view::impl_render_view!(AnimatedBox);

/// Stateless root: `run_app` requires a `StatelessView` entry point, so the
/// actual `AnimatedView` (which needs `StatefulView` state to hold the
/// registered listener) mounts one level down.
#[derive(Clone, Debug)]
pub struct App {
    controller: Arc<AnimationController>,
    red: Color,
    blue: Color,
    histogram_enabled: bool,
    histogram: Arc<parking_lot::Mutex<FrameHistogram>>,
}

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedBoxDemo {
            controller: Arc::clone(&self.controller),
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
    /// A ready-to-mount instance: the demo's standard 1400 ms bounce
    /// controller (not yet started — `repeat(true)` is the caller's job)
    /// and default red/blue palette.
    ///
    /// `App`'s fields are private, so this is the one public construction
    /// path both `main` (below) and the headless screenshot harness
    /// (`examples/screenshot.rs`, which mounts and captures a single frame
    /// at t=0 without ever starting the controller) go through.
    pub fn new() -> Self {
        // `Scheduler` is a cheap handle over shared `Arc` state, so cloning
        // the global singleton yields a handle onto the SAME registries the
        // runner pumps every frame — the controller's ticker actually ticks
        // once `main` starts it. The screenshot harness never starts it, so
        // touching the singleton here is harmless there too (it mounts its
        // own separate `HeadlessBinding` tree).
        let scheduler = Arc::new(Scheduler::instance().clone());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(1400),
            scheduler,
        ));

        Self {
            controller,
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

/// The animated leaf host: subscribed directly to the `AnimationController`
/// via `AnimatedView` — the framework marks this element dirty and rebuilds
/// it on every controller notification, no ambient pipeline reach involved.
#[derive(Clone)]
struct AnimatedBoxDemo {
    controller: Arc<AnimationController>,
    red: Color,
    blue: Color,
    histogram_enabled: bool,
    histogram: Arc<parking_lot::Mutex<FrameHistogram>>,
}

impl StatefulView for AnimatedBoxDemo {
    type State = AnimatedBoxDemoState;

    fn create_state(&self) -> Self::State {
        AnimatedBoxDemoState
    }
}

impl AnimatedView for AnimatedBoxDemo {
    fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::clone(&self.controller) as Arc<dyn Listenable>
    }
}

impl_animated_view!(AnimatedBoxDemo);

struct AnimatedBoxDemoState;

impl ViewState<AnimatedBoxDemo> for AnimatedBoxDemoState {
    fn build(&self, view: &AnimatedBoxDemo, _ctx: &dyn BuildContext) -> impl IntoView {
        if view.histogram_enabled {
            view.histogram.lock().record(Instant::now());
        }
        let value = view.controller.value();
        let color = Color::lerp_oklab(view.red, view.blue, value).to_f32_array();
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

    // Bounce 0 → 1 → 0 forever.
    app.controller
        .repeat(true)
        .expect("a freshly created controller accepts repeat()");

    run_app(app);
}
