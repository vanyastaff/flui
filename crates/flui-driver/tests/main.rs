//! `flui-driver`'s integration tests, through its public API.

mod coordinates;
mod failure_matrix;
mod fixtures;
mod policy;
mod report;
#[cfg(any(unix, windows))]
mod socket;
mod steps;
