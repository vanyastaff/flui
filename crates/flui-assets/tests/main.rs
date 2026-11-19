//! flui-assets's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]

mod cases;
mod image_load;
#[allow(
    dead_code,
    reason = "shared with the decode-scheduling binary, which uses the IO handle"
)]
mod lanes;
mod network_load;
mod pixel_limit;
