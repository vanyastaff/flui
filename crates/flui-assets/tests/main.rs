//! Single-binary consolidation of flui-assets's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `assets_it` binary cuts link time
//! and `target/` disk. Source files stay in place (see `autotests = false` +
//! `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-testing/tests/main.rs`): a test that WRITES
//! process-global state keeps its own `[[test]]` target instead.

#[path = "image_asset_integration.rs"]
mod image_asset_integration;

#[path = "image_bridge.rs"]
mod image_bridge;
