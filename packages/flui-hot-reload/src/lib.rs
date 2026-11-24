//! # FLUI Hot-Reload
//!
//! Hot-reload support for FLUI scene and widget plugins via dynamic library loading.
//!
//! ## Development-only
//!
//! **Do not ship an application that reaches this crate at runtime.** The
//! boundary contracts are enforced as far as a dlopen design allows, and the
//! remaining exposure is documented rather than hidden:
//!
//! * Layout agreement for the `repr(Rust)` payloads is established by an
//!   ABI-token handshake ([`abi_token`]): every plugin macro exports
//!   `flui_*_abi_token`, and each loader refuses a library whose token
//!   (compiler identity + crate version + payload size/align) differs from
//!   the host's. Same-token builds from one worktree are the only supported
//!   configuration.
//! * Scene memory is allocated AND deallocated inside the plugin image: the
//!   host moves the value out with `ptr::read` and returns the emptied box
//!   through `flui_*_free`, so no cross-image allocator or drop-glue pairing
//!   is assumed.
//! * Admitted worker images remain mapped until process exit (ADR-0111):
//!   escaped host-tree views, callbacks and state can retain plugin-owned code,
//!   vtables or storage. Dropping a [`WorkerPlugin`] prunes its matching
//!   registrations without unmapping its image. Each distinct worker revision
//!   retains memory and can keep Windows staged files locked until host exit.
//! * What remains, and why this stays a dev-loop tool: a returned scene
//!   can hold `Arc<dyn Any>` (`AnnotatedRegionLayer`) and `Arc<dyn Fn>`
//!   (`InlineSpan::on_tap` inside a display list) whose vtables live in the
//!   plugin image. The [`PluginScene`] borrows its plugin, so the scene itself
//!   cannot outlive the image, but a clone of one of those payloads can:
//!   [`ScenePlugin::build_scene`] stays `unsafe` for exactly this, and the
//!   token handshake is a strong tripwire, not a proof of layout equality.
//!
//! ## Two-Layer Architecture
//!
//! ```text
//! Layer 1 — Build orchestration (dev-time, lives in the `flui` CLI)
//!   flui run's watcher  →  cargo build  →  new .so/.dll on disk
//!
//! Layer 2 — Artifact reload (runtime, always on native targets)
//!   HotReloadDriver  →  mtime poll  →  unload/load DynLib  →  new Scene
//!        ↑ used by scene_render, Android host, custom runners
//! ```
//!
//! [`env`](mod@env) names the environment variables `flui run` sets for the application.
//!
//! ## Plugin vs Host
//!
//! - **Plugin side** (`scene_plugin!` / `app_plugin!`): `extern "C"` FFI entry points.
//! - **Host side** ([`ScenePlugin`], [`crate::HotReloadDriver`]): load, poll, reload.
//!
//! ## How It Works
//!
//! The plugin builds a real [`Scene`] using normal FLUI APIs.
//! The macro wraps it with `extern "C"` functions that pass an opaque pointer
//! (`Box::into_raw`) across the FFI boundary. The host moves the value out
//! with `ptr::read` and hands the emptied box back to the plugin's
//! `flui_*_free`, after the load-time ABI-token handshake has established
//! that both sides agree on the layout. No serialization, no `#[repr(C)]`
//! types needed.
//!
//! ## Cross-Platform
//!
//! [`DynLib`] is a cross-platform abstraction over:
//! - Unix: `dlopen` / `dlsym` / `dlclose`
//! - Windows: `LoadLibraryW` / `GetProcAddress` / `FreeLibrary`
//!
//! ## Plugin Side (cdylib crate)
//!
//! ```rust,ignore
//! use flui_hot_reload::{Scene, scene_plugin};
//!
//! fn my_scene(_width: f64, _height: f64) -> Scene {
//!     Scene::default()
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
//! if let Ok(plugin) = ScenePlugin::load(Path::new("/path/to/libflui_scene.so")) {
//!     // SAFETY: the renderer keeps no clone of a scene payload past this
//!     // frame (see `ScenePlugin::build_scene`). `None` means "skip this
//!     // frame" (a contained panic, an app_plugin! wrong-thread refusal) —
//!     // never a caller error.
//!     if let Some(scene) = unsafe { plugin.build_scene(1080.0, 2400.0) } {
//!         renderer.render_scene(&scene);
//!     }
//!
//!     // Check for updates later
//!     if plugin.has_update() {
//!         plugin.unload();
//!         // reload...
//!     }
//! }
//! ```
//!
//! ## Installing in a host
//!
//! A `flui-app` host does not drive these types itself: it installs a
//! development reload hook (ADR-0094 §1) on its configuration, and the runner
//! drives it. With the `host-hook` feature, `WorkerReloadHook` drives the
//! host/worker split and `ScenePluginHook` lets a scene plugin draw a
//! window's frames on every native host. An application adds this package as
//! a dependency of its own (the facade names no official package, ADR-0311
//! §6).
//!
//! ```rust,ignore
//! use flui_hot_reload::WorkerReloadHook;
//!
//! let config = flui::app::AppConfig::new().dev_reload(WorkerReloadHook::new(worker_path));
//! ```

// Every public item is documented; keep it that way.
#![deny(missing_docs)]
#![warn(rust_2018_idioms, clippy::all, clippy::pedantic)]
// After the `warn(clippy::all)` above, which would otherwise turn the lint back on in tests.
#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// The crate-level `warn(clippy::pedantic)` above re-enables the pedantic
// lints the workspace allows; these five are re-suppressed. `allow`, not
// `expect`: which of them fire depends on the enabled features and target
// (the wasm32 subset), so no expectation holds
// in every configuration `cargo hack --each-feature` and the facade combos
// compile.
#![allow(
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "feature- and target-dependent after the crate-level pedantic re-enable"
)]

pub mod env;

#[cfg(not(target_arch = "wasm32"))]
mod timing;

mod engine;

mod abi;

#[cfg(not(target_arch = "wasm32"))]
mod load_error;
pub use abi::abi_token;
#[cfg(not(target_arch = "wasm32"))]
pub use load_error::LoadError;

/// Canonical scene payload shared by plugin factories and the host ABI.
pub use flui_sdk::hooks::Scene;

// Re-exported so `app_plugin!`'s generated `flui_app_build` can log its
// wrong-thread refusal via `$crate::__private_tracing::error!` instead of a
// bare `::tracing::error!` — the macro expands INTO the consumer crate (a
// plugin cdylib), which has no reason to otherwise depend on `tracing`
// directly. Not part of the public API; `#[doc(hidden)]` only, never named
// outside this crate's own macros.
#[doc(hidden)]
pub use tracing as __private_tracing;

// Re-exported so `app_plugin!`'s generated `flui_app_build` can build the
// plugin image's own text context through `$crate` — the plugin cdylib names
// no rendering crate itself. `#[doc(hidden)]`, never named outside this
// crate's own macros.
#[cfg(feature = "app-plugin")]
#[doc(hidden)]
pub mod __private_text {
    pub use flui_sdk::hooks::TextContextHandle;
}

mod plugin;

#[cfg(feature = "app-plugin")]
pub use flui_sdk::hooks::{PluginFrameError, PluginPipeline};

/// Draws `app_plugin!`'s frame at the size the host passed across the ABI.
///
/// A frame that produced no scene still hands the host a valid scene, an
/// empty one, and its reason with each `source` is written where the
/// plugin's platform shows it: the host's tracing subscriber does not reach a
/// `dlopen`ed image, so this writes to logcat on Android and to stderr
/// elsewhere. `#[doc(hidden)]`, never named outside this crate's own macros.
#[cfg(feature = "app-plugin")]
#[doc(hidden)]
pub fn __private_draw_frame(pipeline: &mut PluginPipeline, width: f64, height: f64) -> Scene {
    pipeline
        .draw_frame(flui_sdk::geometry::Size::new(width, height))
        .unwrap_or_else(|error| {
            report_frame(&error);
            Scene::default()
        })
}

#[cfg(feature = "app-plugin")]
fn report_frame(error: &PluginFrameError) {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    entry::report(&format!("PluginPipeline: {message}"));
}

mod entry;

// Called by every plugin macro's generated entry through `$crate`.
// `#[doc(hidden)]`, never named outside this crate's own macros.
#[doc(hidden)]
pub use entry::contain as __private_contain_entry;

// Dynamic library loading is not available on wasm32
#[cfg(not(target_arch = "wasm32"))]
mod dynlib;

#[cfg(not(target_arch = "wasm32"))]
mod driver;

#[cfg(not(target_arch = "wasm32"))]
mod host;

#[cfg(all(not(target_arch = "wasm32"), feature = "app-plugin"))]
mod dispatch;

#[cfg(not(target_arch = "wasm32"))]
mod worker;

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "host-hook")))]
mod hook;

#[cfg(not(target_arch = "wasm32"))]
pub use driver::HotReloadDriver;
#[cfg(not(target_arch = "wasm32"))]
pub use dynlib::DynLib;
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "host-hook")))]
pub use hook::{ScenePluginHook, WorkerReloadHook};
#[cfg(not(target_arch = "wasm32"))]
pub use host::{PluginKind, PluginScene, ScenePlugin};
#[cfg(not(target_arch = "wasm32"))]
pub use worker::{
    DEFAULT_FINGERPRINT, HostRequestRebuildFn, RegisterWorkerBuildFn, WorkerPlugin,
    WorkerPollOutcome, WorkerReloadDriver, get_worker_build_ptr, host_register_fn,
    worker_artifact_stamp,
};

#[cfg(all(not(target_arch = "wasm32"), feature = "app-plugin"))]
pub use dispatch::{
    RebuildHookRegistration, WorkerBuildEnv, register_request_rebuild, request_rebuild,
};

// Called by `hot_reload_worker!`'s generated `flui_worker_init` through
// `$crate`. `#[doc(hidden)]`, never named outside this crate's own macros.
#[cfg(all(not(target_arch = "wasm32"), feature = "app-plugin"))]
#[doc(hidden)]
pub use dispatch::forward_rebuild_requests_to as __private_forward_rebuild_requests_to;
pub use engine::{HotReloadOutcome, HotReloadTier};
