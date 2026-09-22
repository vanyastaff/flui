//! `a11y_probe` — the generated counter, run for an assistive technology.
//!
//! The tree is the CLI `counter` template's (`Center` → `Column` → prompt
//! `Text` / count `Text` / `ElevatedButton`), built with the facade's `a11y`
//! feature so the semantics tree the framework assembles — `RenderParagraph`
//! labels, the Material button node — is handed to the OS through the
//! AccessKit adapter. The process does nothing else: it opens the window and
//! waits for [`RUN_FOR`], then quits, so an external accessibility client
//! can be the one that interacts. On macOS that client is
//! `scripts/check-macos-a11y.py` (`just macos-a11y`), which reads the
//! window's `NSAccessibility` tree through `AXUIElement`, finds the button
//! by its label, performs `AXPress`, and reads the count back — the
//! screen-reader path end to end, with no pointer event anywhere.
//!
//! Without the `a11y` feature this compiles and runs, and the client finds
//! an empty tree: the feature is what installs the adapter.

use std::time::Duration;

use flui::app::{AppConfig, AppHandle, Application, StartupWindow};
use flui::prelude::*;
use flui::widgets::column;

/// How long the window stays up for the client before the probe quits on
/// its own — a hang guard for the script, generous for a manual VoiceOver
/// session.
const RUN_FOR: Duration = Duration::from_secs(60);

#[derive(Clone, StatefulView)]
struct Counter;

struct CounterState {
    count: StateCell<usize>,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: StateCell::new(0),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.count.bind(ctx);
    }

    fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.clone();
        Theme::new(
            ThemeData::light(),
            Center::new().child(
                Column::new(column![
                    Text::new("You have pushed the button this many times:"),
                    SizedBox::height(16.0),
                    Text::new(self.count.get().to_string()),
                    SizedBox::height(16.0),
                    ElevatedButton::new(Text::new("Increment"))
                        .on_pressed(move || count.update(|n| n + 1)),
                ])
                .main_axis_alignment(MainAxisAlignment::Center),
            ),
        )
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string()))
        .with_writer(std::io::stderr)
        .init();

    let result = Application::new(|handle: &AppHandle| {
        let handle = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(RUN_FOR);
            let _ = handle.request_quit();
        });
        Counter
    })
    .with_config(
        AppConfig::new()
            .with_title("FLUI Accessibility Probe")
            .with_size(480, 320),
    )
    .with_startup_window(StartupWindow::Open)
    .run();

    if let Err(error) = result {
        eprintln!("a11y_probe: application run failed: {error}");
        std::process::exit(1);
    }
}
