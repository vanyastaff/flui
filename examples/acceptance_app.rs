//! The acceptance application in a window, for native runs.
//!
//! Run with `cargo run --example acceptance_app --features material`. The
//! application and its headless rows live in `tests/fixtures/acceptance_app/`;
//! `acceptance_app_faults` runs it with an injected fault.

#[path = "../tests/fixtures/acceptance_app/app.rs"]
#[allow(
    dead_code,
    reason = "the launcher runs the application; the headless rows use the rest"
)]
mod app;

fn main() -> Result<(), flui::LaunchError> {
    flui::run_app(app::AcceptanceApp::new(app::Faults::NONE), app::launch())
}
