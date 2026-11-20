//! The scenario apps in a window of their own, for the Windows lane's runs: the same trees the
//! headless scenarios drive (`tests/apps.rs`), in an 800 by 600 window, its debug build serving
//! the driver agent (ADR-0239). `FLUI_DRIVER_SCENARIO_APP` picks the app: `controls` (the
//! default), `notes` or `roles`.
//!
//! The driver's Windows tests start it (`tests/windows_desktop.rs`); `flui-app` is a dev-dependency
//! of this crate on Windows only, so on any other host this example is empty.

#[cfg(windows)]
#[path = "../tests/apps.rs"]
#[allow(
    dead_code,
    reason = "the headless tests read the apps' helpers; the window shows the apps alone"
)]
mod apps;

#[cfg(windows)]
fn main() -> Result<(), flui_app::LaunchError> {
    use flui_app::{AppConfig, Application};
    let config = AppConfig::new()
        .title("flui-driver scenario")
        .size(800, 600);
    let app = Application::new().config(config);
    match std::env::var("FLUI_DRIVER_SCENARIO_APP").as_deref() {
        Ok("notes") => app.run(apps::Notes),
        Ok("roles") => app.run(apps::Roles),
        _ => app.run(apps::Controls),
    }
}

#[cfg(not(windows))]
fn main() {}
