//! flui-hot-reload's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "scene_ownership.rs"]
mod scene_ownership;

#[cfg(feature = "app-plugin")]
#[path = "plugin_pipeline_text.rs"]
mod plugin_pipeline_text;

#[cfg(feature = "app-plugin")]
#[path = "plugin_pipeline_layout.rs"]
mod plugin_pipeline_layout;

#[path = "worker_lifetime.rs"]
mod worker_lifetime;

#[path = "refused_entries.rs"]
mod refused_entries;

#[cfg(feature = "app-plugin")]
#[path = "worker_init.rs"]
mod worker_init;

#[cfg(feature = "host-hook")]
#[path = "worker_hook.rs"]
mod worker_hook;
