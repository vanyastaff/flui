//! `flui-driver`'s integration tests, through its public API.

mod apps;
mod coordinates;
mod failure_matrix;
mod fixtures;
mod policy;
mod report;
mod scenarios;
#[cfg(any(unix, windows))]
mod socket;
mod steps;
#[cfg(windows)]
mod windows_desktop;
mod windows_lane;
