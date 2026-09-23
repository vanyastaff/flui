//! `workload_probe` — a self-driving representative workload for
//! `docs/BETA.md`'s "Performance and resilience" row.
//!
//! Mounts a `Scaffold` with a 2,000-row `ListView::builder` (driven by a
//! [`ScrollController`]) and a Material [`TextField`] (driven by a
//! [`TextEditingController`]), then drives BOTH itself through three phases —
//! `scroll`, `type`, `idle` — with no operator input and no synthetic OS
//! pointer/keyboard events. `scripts/check-macos-workload.py` builds this in
//! release, runs it, and applies budgets declared in that script's header.
//!
//! # Why no synthetic OS events
//!
//! Posting synthetic pointer/keyboard events would hijack a working
//! operator's real mouse and keyboard — this example runs on a real, visible
//! window. Driving `ScrollController::jump_to` and
//! `TextEditingController::insert_str` directly, from a free-running
//! [`AnimationController`], exercises exactly the cost this probe wants to
//! measure — layout, paint, and present — without also measuring (or
//! risking) the platform's input-translation path, which is a different,
//! unrelated concern.
//!
//! # The tick source
//!
//! One [`AnimationController`] (`with_detached_ticker`, `repeat(true)`) is
//! registered with the ambient [`VsyncScope`] — the same free-running-probe
//! pattern `examples/vertical_slice_demo/frame_histogram.rs` uses. Its
//! listener runs once per real frame the mounted realm draws; it records the
//! wall-clock delta since the previous tick, then does that phase's work
//! (step the scroll offset, or insert one character) before checking whether
//! the phase's budget (elapsed time for `scroll`, tick count for `type`) is
//! exhausted.
//!
//! `phase` transitions read the previous phase's collected deltas, print one
//! JSON summary line to stdout, and reset for the next phase. The listener
//! itself is `Fn() + Send + Sync` (required by
//! [`flui_foundation::Listenable::add_listener`]), so every field it touches
//! (`ScrollController`, `TextEditingController`, the shared
//! [`TickState`]) is `Send + Sync` — matching `frame_histogram.rs`'s own
//! `Arc<Mutex<TickWindow>>` choice for the same reason.
//!
//! # A deliberate, bounded reference cycle
//!
//! On the `type` → `idle` transition the listener calls `stop()` on the very
//! [`AnimationController`] it is registered on, which means the listener's
//! [`Probe`] holds that controller — a single, bounded
//! `AnimationController` ⟷ listener cycle that never grows and is reclaimed
//! at process exit. That is an acceptable trade-off in this one-shot,
//! self-terminating measurement binary; it would not be in a long-running
//! service.
//!
//! # Idle-phase frame counting
//!
//! `stop()` halts this probe's OWN demand for continuous frames. To observe
//! whether anything ELSE still produces frames while idle, a second,
//! independent mechanism counts them: a self-re-arming
//! [`flui::view::PostFrameHandle`] callback chain. Unlike the
//! `AnimationController`, scheduling a post-frame callback does not itself
//! request a new frame — Flutter parity: `addPostFrameCallback` "does not
//! request a new frame" (`scheduler_binding.dart`) — so it can observe
//! frames from other sources without contributing to their cause. Each
//! callback is a fresh, one-shot `FnOnce` consumed by the scheduler's
//! transient post-frame queue, so this chain carries no persistent cycle.
//!
//! # Honest gaps
//!
//! - **No reachable display-refresh period.** `PlatformWindow::refresh_period`
//!   exists (`crates/flui-platform`) but nothing on the `flui::app`/
//!   `BuildContext` surface exposes it to application code. This example
//!   takes `FLUI_WORKLOAD_PERIOD_MS` (default `16.67`) and reports which
//!   source it used in every summary line's `period_source` field
//!   (`"default"` or `"env"`); `scripts/check-macos-workload.py` measures
//!   the main display through CoreGraphics and sets the variable, so under
//!   the script the budgets are stated against the real panel.
//! - **The idle frame count is a measurement, not an assumption.** A backend
//!   that kept redrawing with nothing dirty would show close to
//!   `idle_seconds × refresh_rate` here; the accepted macOS run
//!   (`docs/BETA.md`) shows 1 — the frame the controller's `stop()` lands
//!   on — and `scripts/check-macos-workload.py` budgets it at 5, reporting
//!   anything above as a finding rather than budgeting around it.

use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use flui::animation::{AnimationController, Vsync, VsyncRegistration};
use flui::app::{AppConfig, AppHandle, Application, StartupWindow};
use flui::foundation::Listenable;
use flui::material::{AppBar, InputDecoration, Scaffold, TextField, Theme, ThemeData};
use flui::prelude::*;
use flui::view::PostFrameHandle;
use flui::widgets::{VsyncScope, column};
use parking_lot::Mutex;

// ============================================================================
// Configuration (env-only — this example takes no argv)
// ============================================================================

const SCROLL_SECONDS_ENV: &str = "FLUI_WORKLOAD_SCROLL_SECONDS";
const TYPE_CHARS_ENV: &str = "FLUI_WORKLOAD_TYPE_CHARS";
const PERIOD_MS_ENV: &str = "FLUI_WORKLOAD_PERIOD_MS";

const DEFAULT_SCROLL_SECONDS: f64 = 20.0;
const DEFAULT_TYPE_CHARS: u64 = 500;
const DEFAULT_PERIOD_MS: f64 = 16.67;
const IDLE_SECONDS: f64 = 5.0;

/// Row count — enough to overflow any reasonably sized window many times
/// over, so scrolling stays a real, sustained layout/paint workload.
const ROW_COUNT: usize = 2_000;
/// Per-row extent estimate fed to `ListView::builder`'s virtualizer.
const ROW_EXTENT_ESTIMATE: f32 = 64.0;
/// Scroll offset moved per tick — a deliberately brisk pace so a 20 s phase
/// covers many screens of content, bouncing at both ends.
const SCROLL_STEP_PX: f32 = 18.0;
/// The repeating text the `type` phase inserts one character at a time.
const TYPE_SEQUENCE: &str = "The quick brown fox ";

#[derive(Debug, Clone, Copy)]
struct WorkloadConfig {
    scroll_seconds: f64,
    type_chars: u64,
    period_ms: f64,
    period_source: &'static str,
}

impl WorkloadConfig {
    fn from_env() -> Self {
        let scroll_seconds = parse_env_f64(SCROLL_SECONDS_ENV, DEFAULT_SCROLL_SECONDS);
        let type_chars = parse_env_u64(TYPE_CHARS_ENV, DEFAULT_TYPE_CHARS);
        let (period_ms, period_source) = match env::var(PERIOD_MS_ENV) {
            Ok(raw) => match raw.trim().parse::<f64>() {
                Ok(value) if value.is_finite() && value > 0.0 => (value, "env"),
                _ => {
                    eprintln!(
                        "workload_probe: ignoring invalid {PERIOD_MS_ENV}={raw:?}; \
                         falling back to the default"
                    );
                    (DEFAULT_PERIOD_MS, "default")
                }
            },
            // `PlatformWindow::refresh_period` is not reachable from the
            // facade (see the module doc's "Honest gaps") — this is always
            // the fallback path unless the operator overrides it.
            Err(_) => (DEFAULT_PERIOD_MS, "default"),
        };
        Self {
            scroll_seconds,
            type_chars,
            period_ms,
            period_source,
        }
    }
}

fn parse_env_f64(name: &str, default: f64) -> f64 {
    env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(default)
}

fn parse_env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

// ============================================================================
// Tick state — shared by the `Send + Sync` controller listener
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkloadPhase {
    Scroll,
    Type,
    Idle,
}

/// Everything the controller's tick listener mutates every frame, bundled
/// behind one lock — the same shape `frame_histogram.rs`'s `TickWindow`
/// uses, for the same reason: the listener must be `Send + Sync`.
struct TickState {
    phase: WorkloadPhase,
    phase_started_at: Instant,
    frames_in_phase: u64,
    last_tick_at: Option<Instant>,
    deltas_in_phase: Vec<Duration>,
    type_ticks: u64,
    scroll_direction: f32,
}

impl TickState {
    fn new(now: Instant) -> Self {
        Self {
            phase: WorkloadPhase::Scroll,
            phase_started_at: now,
            frames_in_phase: 0,
            last_tick_at: None,
            deltas_in_phase: Vec::new(),
            type_ticks: 0,
            scroll_direction: 1.0,
        }
    }

    /// Records `now` as a tick of the current phase.
    fn record_tick(&mut self, now: Instant) {
        self.frames_in_phase += 1;
        if let Some(previous) = self.last_tick_at {
            self.deltas_in_phase.push(now.duration_since(previous));
        }
        self.last_tick_at = Some(now);
    }

    /// Resets the recorder for a fresh phase starting at `now`. The first
    /// tick recorded afterward has no delta (it would otherwise straddle the
    /// phase boundary and misattribute a cross-phase gap to the new phase).
    fn begin_phase(&mut self, phase: WorkloadPhase, now: Instant) {
        self.phase = phase;
        self.phase_started_at = now;
        self.frames_in_phase = 0;
        self.last_tick_at = None;
        self.deltas_in_phase.clear();
        self.type_ticks = 0;
    }
}

/// Prints one JSON summary line for a completed `scroll`/`type` phase.
fn emit_phase_summary(phase_name: &str, state: &TickState, config: &WorkloadConfig) {
    let mut sorted = state.deltas_in_phase.clone();
    sorted.sort_unstable();
    let sample_count = sorted.len();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let percentile = |numerator: usize, denominator: usize| -> f64 {
        if sample_count == 0 {
            return 0.0;
        }
        let index = (sample_count * numerator / denominator).min(sample_count - 1);
        ms(sorted[index])
    };
    let p50_ms = percentile(50, 100);
    let p90_ms = percentile(90, 100);
    let p99_ms = percentile(99, 100);
    let max_ms = sorted.last().copied().map_or(0.0, ms);
    let period_seconds = config.period_ms / 1000.0;
    let over_2p_frames = sorted
        .iter()
        .filter(|delta| delta.as_secs_f64() > 2.0 * period_seconds)
        .count();

    println!(
        "{{\"phase\":\"{phase_name}\",\"frames\":{frames},\"p50_ms\":{p50_ms:.3},\
         \"p90_ms\":{p90_ms:.3},\"p99_ms\":{p99_ms:.3},\"max_ms\":{max_ms:.3},\
         \"over_2p_frames\":{over_2p_frames},\"period_ms\":{period_ms:.3},\
         \"period_source\":\"{period_source}\"}}",
        frames = state.frames_in_phase,
        period_ms = config.period_ms,
        period_source = config.period_source,
    );
}

/// One frame of the `scroll` phase: bounces the scroll offset between `0`
/// and `max_scroll_extent`, reversing direction at either end. A no-op
/// before the first layout has committed real extents.
fn step_scroll(scroll_controller: &ScrollController, state: &mut TickState) {
    let max_extent = scroll_controller.max_scroll_extent();
    if max_extent <= 0.0 {
        return;
    }
    let mut next = scroll_controller.pixels() + state.scroll_direction * SCROLL_STEP_PX;
    if next >= max_extent {
        next = max_extent;
        state.scroll_direction = -1.0;
    } else if next <= 0.0 {
        next = 0.0;
        state.scroll_direction = 1.0;
    }
    scroll_controller.jump_to(next);
}

/// One frame of the `type` phase: inserts the next character of the
/// repeating [`TYPE_SEQUENCE`].
fn step_type(text_controller: &TextEditingController, tick_index: u64) {
    let bytes = TYPE_SEQUENCE.as_bytes();
    let position = (tick_index % TYPE_SEQUENCE.len() as u64) as usize;
    // `TYPE_SEQUENCE` is ASCII, so every byte index is also a char boundary.
    let one_char = std::str::from_utf8(&bytes[position..=position])
        .expect("TYPE_SEQUENCE is ASCII; every single-byte slice is valid UTF-8");
    text_controller.insert_str(one_char);
}

/// Re-arms itself after every completed frame, counting how many occur
/// without this probe's own controller demanding them (see the module doc's
/// "Idle-phase frame counting").
fn schedule_idle_observer(post_frame: PostFrameHandle, idle_frames: Arc<AtomicU64>) {
    let next_post_frame = post_frame.clone();
    post_frame.schedule(move |_timing| {
        idle_frames.fetch_add(1, Ordering::SeqCst);
        schedule_idle_observer(next_post_frame, idle_frames);
    });
}

/// Everything the tick listener needs, owned once and shared with the
/// [`WorkloadDriverState`] that registers it: the listener is `Fn() + Send +
/// Sync`, so every field is `Send + Sync` (see the module doc).
struct Probe {
    controller: AnimationController,
    tick_state: Mutex<TickState>,
    scroll_controller: ScrollController,
    text_controller: TextEditingController,
    post_frame: Mutex<Option<PostFrameHandle>>,
    idle_frames: Arc<AtomicU64>,
    app_handle: AppHandle,
    config: WorkloadConfig,
}

impl Probe {
    /// Runs the whole probe: `scroll_seconds` of scrolling, `type_chars`
    /// ticks of typing, then [`IDLE_SECONDS`] of enforced idleness, then
    /// requests application quit. Registered as the tick controller's
    /// listener, so it runs once per real frame the mounted realm draws.
    fn on_tick(&self) {
        let now = Instant::now();
        let mut state = self.tick_state.lock();
        loop {
            match state.phase {
                WorkloadPhase::Scroll => {
                    let elapsed = now.saturating_duration_since(state.phase_started_at);
                    if elapsed.as_secs_f64() >= self.config.scroll_seconds {
                        emit_phase_summary("scroll", &state, &self.config);
                        state.begin_phase(WorkloadPhase::Type, now);
                        continue;
                    }
                    state.record_tick(now);
                    step_scroll(&self.scroll_controller, &mut state);
                    break;
                }
                WorkloadPhase::Type => {
                    if state.type_ticks >= self.config.type_chars {
                        emit_phase_summary("type", &state, &self.config);
                        state.begin_phase(WorkloadPhase::Idle, now);

                        // Stop this probe's own demand for continuous frames —
                        // see the module doc's "A deliberate, bounded reference
                        // cycle" for why holding `controller` here is safe.
                        let _ = self.controller.stop();

                        // Arm the independent, non-demanding idle observer.
                        let handle = self.post_frame.lock().clone();
                        if let Some(handle) = handle {
                            schedule_idle_observer(handle, Arc::clone(&self.idle_frames));
                        }

                        // A plain OS timer, not a frame tick, ends the idle
                        // window: with the controller stopped, nothing here is
                        // guaranteed to tick again at all.
                        let idle_frames_for_timer = Arc::clone(&self.idle_frames);
                        let handle_for_timer = self.app_handle.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_secs_f64(IDLE_SECONDS));
                            let frames = idle_frames_for_timer.load(Ordering::SeqCst);
                            println!("{{\"phase\":\"idle\",\"frames\":{frames}}}");
                            let _ = handle_for_timer.request_quit();
                        });
                        return;
                    }
                    state.record_tick(now);
                    step_type(&self.text_controller, state.type_ticks);
                    state.type_ticks += 1;
                    break;
                }
                WorkloadPhase::Idle => break,
            }
        }
    }
}

// ============================================================================
// Widget tree
// ============================================================================

fn build_row(index: usize) -> Option<BoxedView> {
    if index >= ROW_COUNT {
        return None;
    }
    let background = if index.is_multiple_of(2) {
        Color::rgb(255, 255, 255)
    } else {
        Color::rgb(238, 238, 238)
    };
    Some(
        ListTile::new()
            .tile_color(background)
            .title(Text::new(format!("Row {index}")))
            .subtitle(Text::new(format!("workload probe item #{index}")))
            .boxed(),
    )
}

#[derive(Clone, StatelessView)]
struct WorkloadApp {
    scroll_controller: ScrollController,
    text_controller: TextEditingController,
}

impl StatelessView for WorkloadApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(
            ThemeData::light(),
            WorkloadRoot {
                scroll_controller: self.scroll_controller.clone(),
                text_controller: self.text_controller.clone(),
            },
        )
    }
}

#[derive(Clone, StatelessView)]
struct WorkloadRoot {
    scroll_controller: ScrollController,
    text_controller: TextEditingController,
}

impl StatelessView for WorkloadRoot {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let decoration = InputDecoration {
            label_text: Some("Type here".to_string()),
            ..Default::default()
        };
        let text_field = TextField::new(self.text_controller.clone()).decoration(decoration);
        let list = ListView::builder(ROW_COUNT, ROW_EXTENT_ESTIMATE, build_row)
            .position(self.scroll_controller.position());

        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new("FLUI Workload Probe")))
            .body(
                Column::new(column![
                    Padding::new(EdgeInsets::all(px(12.0))).child(text_field),
                    Expanded::new(list),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Stretch),
            )
    }
}

/// Owns the tick controller's lifecycle: registers it with the ambient
/// [`VsyncScope`] and starts the free run in `init_state` (ADR-0021,
/// port-check trigger #22 — lifecycle-only acquisition), never from `build`.
#[derive(Clone, StatefulView)]
struct WorkloadDriver {
    scroll_controller: ScrollController,
    text_controller: TextEditingController,
    app_handle: AppHandle,
    config: WorkloadConfig,
}

struct WorkloadDriverState {
    probe: Arc<Probe>,
    registration: Option<(Vsync, VsyncRegistration)>,
}

impl StatefulView for WorkloadDriver {
    type State = WorkloadDriverState;

    fn create_state(&self) -> Self::State {
        WorkloadDriverState {
            probe: Arc::new(Probe {
                controller: AnimationController::with_detached_ticker(Duration::from_millis(1_000)),
                tick_state: Mutex::new(TickState::new(Instant::now())),
                scroll_controller: self.scroll_controller.clone(),
                text_controller: self.text_controller.clone(),
                post_frame: Mutex::new(None),
                idle_frames: Arc::new(AtomicU64::new(0)),
                app_handle: self.app_handle.clone(),
                config: self.config,
            }),
            registration: None,
        }
    }
}

impl ViewState<WorkloadDriver> for WorkloadDriverState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        if let Some(handle) = ctx.post_frame_handle() {
            *self.probe.post_frame.lock() = Some(handle);
        }

        let probe = Arc::clone(&self.probe);
        self.probe
            .controller
            .add_listener(Arc::new(move || probe.on_tick()));

        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register(self.probe.controller.clone());
            self.registration = Some((vsync, registration));
        }
        self.probe
            .controller
            .repeat(true)
            .expect("a freshly created controller accepts repeat()");
    }

    fn dispose(&mut self) {
        if let Some((vsync, registration)) = self.registration.take() {
            vsync.unregister(registration);
        }
    }

    fn build(&self, view: &WorkloadDriver, _ctx: &dyn BuildContext) -> impl IntoView {
        WorkloadApp {
            scroll_controller: view.scroll_controller.clone(),
            text_controller: view.text_controller.clone(),
        }
    }
}

fn main() {
    // Logging to stderr, not stdout: `scripts/check-macos-workload.py`
    // parses stdout as a stream of JSON summary lines, and interleaved
    // `tracing` output would not be valid JSON.
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string()))
        .with_writer(std::io::stderr)
        .init();

    let config = WorkloadConfig::from_env();
    let scroll_controller = ScrollController::new();
    let text_controller = TextEditingController::new();

    let scroll_controller_for_factory = scroll_controller.clone();
    let text_controller_for_factory = text_controller.clone();
    let result = Application::new(move |handle: &AppHandle| WorkloadDriver {
        scroll_controller: scroll_controller_for_factory.clone(),
        text_controller: text_controller_for_factory.clone(),
        app_handle: handle.clone(),
        config,
    })
    .with_config(
        AppConfig::new()
            .with_title("FLUI Workload Probe")
            .with_size(900, 700),
    )
    .with_startup_window(StartupWindow::Open)
    .run();

    if let Err(error) = result {
        eprintln!("workload_probe: application run failed: {error}");
        std::process::exit(1);
    }
}
