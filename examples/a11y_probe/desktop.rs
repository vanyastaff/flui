//! `a11y_probe` — counter and action fixtures, run for an assistive technology.
//!
//! The tree is the CLI `counter` template's (`Center` → `Column` → prompt
//! `Text` / count `Text` / `ElevatedButton`), built with the facade's `a11y`
//! feature so the semantics tree the framework assembles — `RenderParagraph`
//! labels, the Material button node — is handed to the OS through the
//! AccessKit adapter. The process does nothing else: it opens the window and
//! waits for [`run_for`], then quits, so an external accessibility client
//! can be the one that interacts. On macOS that client is
//! `tools/device-checks/check-macos-a11y.py` (`cargo xtask device macos-a11y`),
//! which reads the window's `NSAccessibility` tree through `AXUIElement`, finds the button
//! by its label, performs `AXPress`, and reads the count back — the
//! screen-reader path end to end, with no pointer event anywhere.
//! The Windows UIA client also drives explicit expand/collapse and numeric
//! range actions, reading both native properties and visible sibling text.
//! Those two controls are semantics fixtures, not the widget catalog controls.
//!
//! Without the `a11y` feature this compiles and runs, and the client finds
//! an empty tree: the feature is what installs the adapter.

use std::time::Duration;

use flui::app::{AppConfig, AppHandle, Application, StartupWindow};
use flui::prelude::*;
use flui::widgets::column;

/// How long the window stays up for the client before the probe quits on
/// its own — a hang guard for the script, generous for a manual VoiceOver
/// session — unless the client sets [`RUN_FOR_ENV`].
const RUN_FOR: Duration = Duration::from_secs(60);
/// Whole seconds to stay up instead of [`RUN_FOR`]. The Windows UIA check
/// (`cargo xtask device windows-a11y`) sets it past its own deadline, which
/// its waits add up to more than [`RUN_FOR`].
const RUN_FOR_ENV: &str = "FLUI_PROBE_RUN_FOR_SECS";

/// [`RUN_FOR_ENV`] when it holds whole seconds, else [`RUN_FOR`].
fn run_for() -> Duration {
    std::env::var(RUN_FOR_ENV)
        .ok()
        .and_then(|secs| secs.parse().ok())
        .map_or(RUN_FOR, Duration::from_secs)
}

#[derive(Clone, StatefulView)]
struct Counter;

struct CounterState {
    count: StateCell<usize>,
    expanded: StateCell<bool>,
    numeric: StateCell<f64>,
    scroll: flui::widgets::ScrollController,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: StateCell::new(0),
            expanded: StateCell::new(false),
            numeric: StateCell::new(0.0),
            scroll: flui::widgets::ScrollController::new(),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count.bind(ctx);
        self.expanded.bind(ctx);
        self.numeric.bind(ctx);
    }

    fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.clone();
        let expand = self.expanded.clone();
        let collapse = self.expanded.clone();
        let numeric = self.numeric.clone();
        let details = if self.expanded.get() {
            "Details expanded"
        } else {
            "Details collapsed"
        };
        let range_text = format!("Range value: {}", self.numeric.get());
        Theme::new(
            ThemeData::light(),
            Center::new().child(
                Column::new(column![
                    Text::new("You have pushed the button this many times:"),
                    SizedBox::height(16.0),
                    Text::new(self.count.get().to_string()),
                    SizedBox::height(16.0),
                    ElevatedButton::new(Text::new("Increment"))
                        .on_pressed(move |_cx| count.update(|n| n + 1)),
                    SizedBox::height(16.0),
                    Focus::new(
                        Semantics::new()
                            .container(true)
                            .button(true)
                            .label("Probe disclosure")
                            .exclude_semantics(true)
                            .expandable(
                                self.expanded.get(),
                                move |_cx| expand.set(true),
                                move |_cx| collapse.set(false),
                            )
                            .child(Text::new(details))
                    ),
                    // This visible sibling is outside the control's excluded
                    // subtree, so the native client can read the frame result.
                    Text::new(format!(
                        "Disclosure state: {}",
                        if self.expanded.get() {
                            "expanded"
                        } else {
                            "collapsed"
                        }
                    )),
                    SizedBox::height(16.0),
                    Focus::new(
                        Semantics::new()
                            .container(true)
                            .label("Probe numeric range")
                            .numeric_range(
                                flui::rendering::NumericRange::new(
                                    self.numeric.get(),
                                    0.0,
                                    10.0,
                                    1.0
                                )
                                .expect("BUG: probe value is admitted by its range action")
                            )
                            .exclude_semantics(true)
                            .on_set_numeric_value(move |_cx, value| numeric.set(value))
                            .child(Text::new(range_text))
                    ),
                    Text::new(format!("Published range value: {}", self.numeric.get())),
                    SizedBox::new(220.0, 80.0).child(
                        flui::widgets::Scrollable::new()
                            .controller(self.scroll.clone())
                            .child(
                                Semantics::new()
                                    .label("Scrollable content")
                                    .child(SizedBox::new(220.0, 1000.0))
                            )
                    ),
                ])
                .main_axis_alignment(MainAxisAlignment::Center),
            ),
        )
    }
}

pub(super) fn run() {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string()))
        .with_writer(std::io::stderr)
        .init();

    let result = Application::new(|handle: &AppHandle| {
        let handle = handle.clone();
        let run_for = run_for();
        std::thread::spawn(move || {
            std::thread::sleep(run_for);
            let _ = handle.request_quit();
        });
        Counter
    })
    .with_config(
        AppConfig::new()
            .with_title("FLUI Accessibility Probe")
            .with_size(480, 560),
    )
    .with_startup_window(StartupWindow::Open)
    .run();

    if let Err(error) = result {
        eprintln!("a11y_probe: application run failed: {error}");
        std::process::exit(1);
    }
}
