//! flui-devtools' inspector integration tests, compiled as modules of one
//! binary. The frame-profile test and the agent endpoint keep their own
//! `[[test]]` targets: they write process-global state (see `Cargo.toml`).

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
mod gesture_inspector;
mod tree_observer_inspector;
