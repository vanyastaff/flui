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
pub(crate) mod hot_reload;
pub(crate) mod logging;
pub(crate) mod presentation;
pub(crate) mod presentation_forest;
pub mod runner;
pub(crate) mod runtime;
pub(crate) mod semantics_host;
pub(crate) mod ui_realm;
pub(crate) mod window_registry;

pub use config::{AppConfig, DiagnosticsProfile};
pub use direct::run_direct;
#[cfg(target_os = "android")]
pub use runner::{run_app_android, run_app_android_with_config};
pub use runner::{run_app_impl as run_app, run_app_with_config_impl as run_app_with_config};

// Re-export RootRenderView and RootRenderElement from flui-view
pub use flui_view::{RootRenderElement, RootRenderView};
