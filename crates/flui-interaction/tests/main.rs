//! Single-binary consolidation of flui-interaction's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `interaction_it` binary cuts link time
//! and `target/` disk. Source files stay in place (see `autotests = false` +
//! `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-testing/tests/main.rs`): a test that WRITES
//! process-global state keeps its own `[[test]]` target instead.

#[path = "headless_long_press.rs"]
mod headless_long_press;

#[path = "interaction_lane.rs"]
mod interaction_lane;
