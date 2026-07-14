//! Frame-time evidence for Core.1's 60 fps criterion (env-gated, off by
//! default so the interactive demo is unaffected).
//!
//! Set `FLUI_FRAME_HISTOGRAM` to any value (e.g. `=1`) before `cargo run
//! --example vertical_slice_demo` to attach a free-running [`AnimationController`]
//! (same pattern as `examples/animated_box_app.rs`) whose ticks are paced by
//! the real scheduler's wake-driven frame loop. Its listener records the
//! wall-clock [`Instant`] delta since the previous tick; every
//! [`WINDOW_SAMPLE_COUNT`] deltas it sorts the window and logs
//! median/p90/max via `tracing::info!`.
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

use flui_animation::AnimationController;
use flui_app::Scheduler;
use flui_foundation::{HasInstance, Listenable};
use parking_lot::Mutex;

/// Env var that turns the histogram on. Unset (the default): zero overhead,
/// identical behavior to the plain interactive demo.
const ENABLE_ENV_VAR: &str = "FLUI_FRAME_HISTOGRAM";

/// Deltas collected per logged window. ~300 ticks at a wake-driven ~60 Hz
/// loop is a ~5 s window — long enough to smooth out startup jitter without
/// making the operator wait too long between log lines.
const WINDOW_SAMPLE_COUNT: usize = 300;

/// The controller's own cycle length. Cosmetic: with no other listener
/// driving the tree, only the *tick cadence* (paced by the scheduler's frame
/// wake-ups) is measured, not this value.
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

/// Installs the free-running controller and its recording listener iff
/// [`ENABLE_ENV_VAR`] is set; otherwise a no-op.
///
/// Returns the controller so the caller can keep it alive for the process's
/// lifetime — dropping it deregisters its ticker and silently stops the
/// histogram.
pub fn install_if_requested() -> Option<AnimationController> {
    std::env::var_os(ENABLE_ENV_VAR)?;

    let scheduler = Arc::new(Scheduler::instance().clone());
    let controller = AnimationController::new(CONTROLLER_CYCLE, scheduler);

    let window = Arc::new(Mutex::new(TickWindow::default()));
    controller.add_listener(Arc::new(move || {
        if let Some(deltas) = window.lock().record(Instant::now()) {
            log_window(deltas);
        }
    }));

    controller
        .repeat(true)
        .expect("a freshly created controller accepts repeat()");

    tracing::info!(
        window_sample_count = WINDOW_SAMPLE_COUNT,
        "frame histogram enabled ({ENABLE_ENV_VAR}=1)"
    );

    Some(controller)
}
