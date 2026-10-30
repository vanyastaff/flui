//! The acceptance application with one injected fault, for the native runs
//! that check what a panic leaves behind.
//!
//! Run with `cargo run --example acceptance_app_faults --features material --
//! <fault>`, where `<fault>` is a `Fault` name (`Fault::name` in
//! `tests/fixtures/acceptance_app/app.rs`). A missing or unknown name exits
//! with code 2, listing the names, before a window opens.

#[path = "../tests/fixtures/acceptance_app/app.rs"]
#[allow(
    dead_code,
    reason = "the fixture runs the application; the headless rows use the rest"
)]
mod app;

use app::{AcceptanceApp, Fault, Faults};

fn main() -> Result<(), flui::LaunchError> {
    let fault = std::env::args()
        .nth(1)
        .unwrap_or_default()
        .parse::<Fault>()
        .unwrap_or_else(|unknown| {
            eprintln!("acceptance_app_faults: {unknown}");
            std::process::exit(2);
        });
    flui::run_app(
        AcceptanceApp::new(Faults::from(fault)),
        flui::AppLaunch::new(),
    )
}
