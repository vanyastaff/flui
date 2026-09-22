//! flui-hot-reload's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "loader.rs"]
mod loader;

#[path = "scene_ownership.rs"]
mod scene_ownership;
