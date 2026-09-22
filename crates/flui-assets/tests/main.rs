//! flui-assets's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "image_asset_integration.rs"]
mod image_asset_integration;

#[path = "image_bridge.rs"]
mod image_bridge;
