//! The acceptance application in a window, for native runs.
//!
//! Run with `cargo run --example acceptance_app --features material`, adding
//! `-- <fault>...` to inject failures: `note-build`, `save-pressed`,
//! `note-dispose`. A fault it does not know exits with code 2 before opening a
//! window. The application and its headless rows live in
//! `tests/fixtures/acceptance_app/`.

#[path = "../tests/fixtures/acceptance_app/app.rs"]
#[allow(
    dead_code,
    reason = "the launcher runs the application; the headless rows use the rest"
)]
mod app;

use app::{AcceptanceApp, Faults};

fn main() {
    let mut faults = Faults::default();
    for fault in std::env::args().skip(1) {
        match fault.as_str() {
            "note-build" => faults.note_build = true,
            "save-pressed" => faults.save_pressed = true,
            "note-dispose" => faults.note_dispose = true,
            unknown => {
                eprintln!("acceptance_app: unknown fault `{unknown}`");
                std::process::exit(2);
            }
        }
    }
    flui::run_app(AcceptanceApp::new(faults));
}
