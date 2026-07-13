//! Application core module.
//!
//! This module contains the core application infrastructure:
//! - `AppBinding` - Hosts transitional process-scoped services
//! - `UiRealm` - Owns one owner-affine widget session (crate-private during extraction)
//! - `AppConfig` - Application configuration
//! - `LifecycleState` - Lifecycle state management (re-exported from
//!   flui-platform)

mod binding;
mod config;
pub mod direct;
mod lifecycle;
pub mod runner;
pub(crate) mod ui_realm;

pub use binding::AppBinding;
pub use config::AppConfig;
pub use direct::run_direct;
pub use lifecycle::{DefaultLifecycle, LifecycleEvent, LifecycleState};
#[cfg(target_os = "android")]
pub use runner::{run_app_android, run_app_android_with_config};
pub use runner::{run_app_impl as run_app, run_app_with_config_impl as run_app_with_config};

/// Legacy alias for the transitional process service host.
///
/// New application code should use [`run_app`] instead of accessing this
/// process-scoped migration seam directly.
pub type WidgetsFlutterBinding = AppBinding;

// Re-export RootRenderView and RootRenderElement from flui-view
pub use flui_view::{RootRenderElement, RootRenderView};
