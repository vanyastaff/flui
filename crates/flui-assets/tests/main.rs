//! flui-assets's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
mod cases;

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

#[cfg(feature = "images")]
mod network_load;
