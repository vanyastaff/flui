//! `flui-driver`'s integration tests, through its public API.

mod apps;
mod chrome_browser;
mod chrome_host;
mod chrome_lane;
mod coordinates;
mod failure_matrix;
mod fixtures;
#[cfg(unix)]
mod launch;
#[cfg(target_os = "macos")]
mod macos_desktop;
mod macos_lane;
mod pixels;
mod policy;
mod private_desktop;
mod report;
mod scenarios;
#[cfg(any(unix, windows))]
mod socket;
mod steps;
mod trace;
mod trace_bounds;
// The test `trace::codegen` writes from a trace of the counter, compiled and run here as it was
// written: rustfmt would rewrite what the comparison holds.
#[rustfmt::skip]
mod trace_generated;
mod wayland_lane;
#[cfg(windows)]
mod windows_desktop;
mod windows_lane;
#[cfg(target_os = "linux")]
mod x11_desktop;
mod x11_lane;
