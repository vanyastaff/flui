//! Single-binary consolidation of flui-app's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `app_it` binary cuts link time
//! and `target/` disk. Source files stay in place (see `autotests = false` +
//! `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-testing/tests/main.rs`): a test that WRITES
//! process-global state keeps its own `[[test]]` target instead.

#[path = "data_transfer_transport.rs"]
mod data_transfer_transport;

#[path = "runner_frame_ordering.rs"]
mod runner_frame_ordering;
