//! What a platform entry point starts a run with, and how starting fails.

use super::{AppConfig, capability::AppCapabilities};

/// What a run starts with: its configuration and the capability providers it
/// registers (ADR-0152). Each platform's entry point takes one, so an option
/// added here reaches every platform without a new entry point.
///
/// ```rust,ignore
/// flui::run_app(
///     MyRoot,
///     AppLaunch::new()
///         .config(AppConfig::new().with_title("My App"))
///         .capabilities(AppCapabilities::new().plugin(MyPlugin)),
/// )?;
/// ```
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct AppLaunch {
    pub(crate) config: AppConfig,
    pub(crate) capabilities: AppCapabilities,
}

impl AppLaunch {
    /// The default configuration and the built-in providers alone.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run with `config`.
    #[must_use = "the builder returns the updated launch; assign or chain it"]
    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = config;
        self
    }

    /// Register `capabilities`: the application's overrides and plugins.
    #[must_use = "the builder returns the updated launch; assign or chain it"]
    pub fn capabilities(mut self, capabilities: AppCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

/// Why a platform entry point could not start the application.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LaunchError {
    /// Two capability registrations the run cannot choose between: two
    /// plugins, or two overrides, for one capability, or two capabilities
    /// sharing a name. Found before the platform starts.
    #[error("capability registration failed: {0}")]
    CapabilityConflict(#[source] flui_runtime::capability::CapabilityConflict),
    /// The platform could not be initialized.
    #[error("platform startup failed: {source}")]
    Platform {
        /// Original native failure.
        #[source]
        source: std::sync::Arc<dyn std::error::Error + Send + Sync>,
    },
}
