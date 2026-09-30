//! flui-hot-reload's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "scene_ownership.rs"]
mod scene_ownership;

#[cfg(feature = "app-plugin")]
#[path = "plugin_pipeline_text.rs"]
mod plugin_pipeline_text;
