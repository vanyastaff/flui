//! flui-assets's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

mod cases;

mod memory_trim;

#[path = "image_asset_integration.rs"]
mod image_asset_integration;

#[path = "image_load.rs"]
mod image_load;

#[cfg(feature = "images")]
#[allow(
    dead_code,
    reason = "shared with the decode-scheduling binary, which uses the IO handle"
)]
mod lanes;

#[path = "load_contract.rs"]
mod load_contract;

#[cfg(feature = "images")]
mod network_load;
