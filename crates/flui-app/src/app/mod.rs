//! Application core module.
//!
//! This module contains the core application infrastructure:
//! - `AppRuntime` (`runtime.rs`) - the loop-scoped composition root
//! - `UiRealm` - Owns one owner-affine widget session (build/render/gesture
//!   state, the frame pipeline, and the per-presentation semantics/haptics/
//!   clipboard surfaces retired from the former `AppBinding`)
//! - `AppConfig` - Application configuration
//!
//! Application lifecycle state is `flui_scheduler::AppLifecycleState`;
//! the runner drives the scheduler directly.

mod config;
pub mod direct;
pub(crate) mod execution;
mod frame_failure;
pub(crate) mod hot_reload;
pub(crate) mod logging;
pub(crate) mod media_query_root;
pub(crate) mod presentation;
pub(crate) mod presentation_forest;
pub mod runner;
pub(crate) mod runtime;
pub(crate) mod semantics_host;
pub(crate) mod ui_realm;
pub(crate) mod window_registry;

pub use config::{AppConfig, DiagnosticsProfile};
pub use direct::run_direct;
pub use execution::{
    ComputeJob, DeterministicExecutors, HostComputePool, HostExecutors, HostIoPool, IoFuture,
    SpawnError,
};
pub use frame_failure::{FrameFailureHandler, FrameFailureKind, FrameFailureReport};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use runner::open_secondary_window;
#[cfg(target_os = "android")]
pub use runner::{run_app_android, run_app_android_with_config};
pub use runner::{run_app_impl as run_app, run_app_with_config_impl as run_app_with_config};
#[cfg(not(target_os = "ios"))]
pub use runtime::{ExitPolicy, WindowPolicy};

// Re-export RootRenderView and RootRenderElement from flui-view
pub use flui_view::{RootRenderElement, RootRenderView};
