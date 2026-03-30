//! Application core module.
//!
//! This module contains the core application infrastructure:
//! - `AppBinding` - Combines all framework bindings
//! - `AppConfig` - Application configuration
//! - `LifecycleState` - Lifecycle state management (re-exported from
//!   flui-platform)

mod binding;
mod config;
mod lifecycle;
pub mod runner;

pub use binding::AppBinding;
pub use config::AppConfig;
pub use lifecycle::{DefaultLifecycle, LifecycleEvent, LifecycleState};
#[cfg(target_os = "android")]
pub use runner::{run_app_android, run_app_android_with_config};
pub use runner::{run_app_impl as run_app, run_app_with_config_impl as run_app_with_config};

/// Alias for AppBinding matching Flutter naming convention.
pub type WidgetsFlutterBinding = AppBinding;

// Re-export RootRenderView and RootRenderElement from flui-view
pub use flui_view::{RootRenderElement, RootRenderView};
