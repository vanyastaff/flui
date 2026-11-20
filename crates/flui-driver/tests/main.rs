//! `flui-driver`'s integration tests, through its public API.

mod apps;
mod chrome_browser;
mod chrome_lane;
mod coordinates;
mod failure_matrix;
mod fixtures;
#[cfg(unix)]
mod launch;
#[cfg(target_os = "macos")]
mod macos_desktop;
mod macos_lane;
mod policy;
mod report;
mod scenarios;
#[cfg(any(unix, windows))]
mod socket;
mod steps;
mod wayland_lane;
#[cfg(windows)]
mod windows_desktop;
mod windows_lane;
#[cfg(target_os = "linux")]
mod x11_desktop;
mod x11_lane;
