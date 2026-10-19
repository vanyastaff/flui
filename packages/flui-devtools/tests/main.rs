//! flui-devtools' inspector integration tests, compiled as modules of one
//! binary. The frame-profile test and the agent endpoint keep their own
//! `[[test]]` targets: they write process-global state (see `Cargo.toml`).

mod gesture_inspector;
mod tree_observer_inspector;
