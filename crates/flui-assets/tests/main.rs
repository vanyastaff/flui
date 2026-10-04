//! flui-assets's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

mod cases;

#[path = "image_asset_integration.rs"]
mod image_asset_integration;

#[path = "image_bridge.rs"]
mod image_bridge;

#[path = "load_contract.rs"]
mod load_contract;

#[cfg(all(feature = "images", feature = "network"))]
mod network_bridge;

#[cfg(feature = "network")]
mod network_support;

#[cfg(feature = "network")]
mod network_loader;
