//! # FLUI Hot-Reload
//!
//! Hot-reload support for FLUI scene and widget plugins via dynamic library loading.
//!
//! ## Two-Layer Architecture
//!
//! ```text
//! Layer 1 — Build orchestration (dev-time, optional `source-watch` feature)
//!   SourceWatcher  →  cargo build  →  new .so/.dll on disk
//!        ↑ used by flui-cli, flui-devtools
//!
//! Layer 2 — Artifact reload (runtime, always on native targets)
//!   HotReloadDriver  →  mtime poll  →  unload/load DynLib  →  new Scene
//!        ↑ used by scene_render, Android host, custom runners
//! ```
//!
//! See [`strategy`] for [`ReloadStrategy`] and shared env/timing constants.
//!
//! ## Plugin vs Host
//!
//! - **Plugin side** ([`scene_plugin!`] / [`app_plugin!`]): `extern "C"` FFI entry points.
//! - **Host side** ([`ScenePlugin`], [`HotReloadDriver`]): load, poll, reload.
//!
//! ## How It Works
//!
//! The plugin builds a real [`flui_layer::Scene`] using normal FLUI APIs.
//! The macro wraps it with `extern "C"` functions that pass an opaque pointer
//! (`Box::into_raw`) across the FFI boundary. The host takes ownership back
//! via `Box::from_raw`. No serialization, no `#[repr(C)]` types needed.
//!
//! ## Cross-Platform
//!
//! The [`dynlib`] module provides a cross-platform abstraction over:
//! - Unix: `dlopen` / `dlsym` / `dlclose`
//! - Windows: `LoadLibraryW` / `GetProcAddress` / `FreeLibrary`
//!
//! ## Plugin Side (cdylib crate)
//!
//! ```rust,ignore
//! use flui_hot_reload::scene_plugin;
//! use flui_layer::*;
//! use flui_types::geometry::{px, Rect, Size};
//! use flui_types::painting::Paint;
//! use flui_types::styling::Color;
//!
//! fn my_scene(width: f32, height: f32) -> Scene {
//!     let mut tree = LayerTree::new();
//!     let mut canvas_layer = CanvasLayer::new();
//!     let canvas = canvas_layer.canvas_mut();
//!     canvas.draw_rect(
//!         Rect::from_ltrb(px(0.0), px(0.0), px(width), px(height)),
//!         &Paint::fill(Color::rgb(128, 0, 128)),
//!     );
//!     let root = tree.insert(Layer::Canvas(canvas_layer));
//!     Scene::new(Size::new(px(width), px(height)), tree, Some(root), 1)
//! }
//!
//! scene_plugin!(my_scene);
//! ```
//!
//! ## Host Side
//!
//! ```rust,ignore
//! use flui_hot_reload::ScenePlugin;
//! use std::path::Path;
//!
//! if let Some(plugin) = ScenePlugin::load(Path::new("/path/to/libflui_scene.so")) {
//!     let scene = plugin.build_scene(1080.0, 2400.0);
//!     renderer.render_scene(&scene);
//!
//!     // Check for updates later
//!     if plugin.has_update() {
//!         plugin.unload();
//!         // reload...
//!     }
//! }
//! ```

#![warn(rust_2018_idioms, clippy::all, clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod strategy;

#[cfg(feature = "source-watch")]
pub mod dev;

pub mod engine;

#[cfg(feature = "app-plugin")]
mod pipeline;
mod plugin;

#[cfg(feature = "app-plugin")]
pub use pipeline::PluginPipeline;

// Dynamic library loading is not available on wasm32
#[cfg(not(target_arch = "wasm32"))]
pub mod dynlib; // PORT-CHECK-OK-SP4: dynlib API surface; binding entry for hot-reload integrators

#[cfg(not(target_arch = "wasm32"))]
mod driver;

#[cfg(not(target_arch = "wasm32"))]
mod host;

#[cfg(all(not(target_arch = "wasm32"), feature = "app-plugin"))]
mod dispatch;

#[cfg(not(target_arch = "wasm32"))]
pub mod worker;

#[cfg(not(target_arch = "wasm32"))]
pub use driver::HotReloadDriver;
#[cfg(not(target_arch = "wasm32"))]
pub use host::{PluginKind, ScenePlugin};
#[cfg(not(target_arch = "wasm32"))]
pub use worker::{
    RegisterWorkerBuildFn, WorkerPlugin, WorkerPollOutcome, WorkerReloadDriver,
    get_worker_build_ptr, host_register_fn,
};

#[cfg(all(not(target_arch = "wasm32"), feature = "app-plugin"))]
pub use dispatch::{WorkerBuildEnv, request_rebuild, set_request_rebuild};
pub use engine::{HotReloadOutcome, HotReloadTier};
pub use strategy::ReloadStrategy;
