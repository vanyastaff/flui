//! Application core module.
//!
//! This module contains the core application infrastructure:
//! - `AppBinding` - Combines all framework bindings
//! - `AppConfig` - Application configuration
//! - `AppLifecycle` - Lifecycle state management

mod binding;
mod config;
mod lifecycle;
pub mod runner;

pub use binding::AppBinding;
pub use config::AppConfig;
pub use lifecycle::AppLifecycle;
