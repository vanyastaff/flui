//! flui-devtools' integration tests, compiled as modules of one binary. The
//! frame-profile test keeps its own `[[test]]` target: it re-runs itself in a
//! child process (see `Cargo.toml`).

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
mod capture_sinks;
mod frame_snapshots;
mod gesture_inspector;
mod profiler;
mod tree_observer_inspector;
