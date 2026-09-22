//! Single-binary consolidation of flui-hot-reload's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `hot_reload_it` binary cuts link time
//! and `target/` disk. Source files stay in place (see `autotests = false` +
//! `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-testing/tests/main.rs`): a test that WRITES
//! process-global state keeps its own `[[test]]` target instead.

#[path = "loader.rs"]
mod loader;

#[path = "scene_ownership.rs"]
mod scene_ownership;
