//! Structured logging utilities for FLUI framework.
//!
//! This module provides a clean, hierarchical logging API with different modes
//! for development, performance profiling, and production use.

use tracing_subscriber::{
    fmt::{self},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

/// Logging configuration for FLUI applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    /// Development mode with hierarchical, colored output.
    /// Shows all debug information with proper indentation.
    Development,

    /// Performance mode - only logs frame times and performance metrics.
    /// Minimal overhead for profiling.
    Performance,

    /// Production mode with compact JSON output.
    /// Suitable for production deployments.
    Production,

    /// Silent mode - no logging output.
    /// Useful for benchmarks and tests.
    Silent,
}

/// Configuration for FLUI logging system.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Logging mode (development, performance, production, silent).
    pub mode: LogMode,

    /// Custom filter directives (overrides defaults).
    /// Example: "flui_core=debug,flui_rendering=info"
    pub filter: Option<String>,

    /// Enable ANSI colors in output (default: true for Development mode).
    pub colors: Option<bool>,

    /// Show file names and line numbers (default: true for Development).
    pub show_location: Option<bool>,

    /// Show thread IDs (default: false).
    pub show_threads: Option<bool>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            mode: LogMode::Development,
            filter: None,
            colors: None,
            show_location: None,
            show_threads: None,
        }
    }
}

impl LogConfig {
    /// Create a new config with the specified mode.
    pub fn new(mode: LogMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    /// Set custom filter directives.
    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    /// Enable/disable ANSI colors.
    pub fn with_colors(mut self, colors: bool) -> Self {
        self.colors = Some(colors);
        self
    }

    /// Enable/disable file location information.
    pub fn with_location(mut self, show: bool) -> Self {
        self.show_location = Some(show);
        self
    }

    /// Enable/disable thread ID display.
    pub fn with_threads(mut self, show: bool) -> Self {
        self.show_threads = Some(show);
        self
    }
}

/// Initialize FLUI logging with the specified configuration.
///
/// This should be called once at application startup.
///
/// # Examples
///
/// ```no_run
/// use flui_core::logging::{init_logging, LogConfig, LogMode};
///
/// // Development mode with hierarchical output
/// init_logging(LogConfig::new(LogMode::Development));
///
/// // Performance mode (minimal logging)
/// init_logging(LogConfig::new(LogMode::Performance));
///
/// // Custom filter
/// init_logging(
///     LogConfig::new(LogMode::Development)
///         .with_filter("flui_core=debug,flui_rendering=info")
/// );
/// ```
pub fn init_logging(config: LogConfig) {
    match config.mode {
        LogMode::Development => init_dev_logging(config),
        LogMode::Performance => init_perf_logging(config),
        LogMode::Production => init_prod_logging(config),
        LogMode::Silent => {
            // No-op: don't initialize any subscriber
        }
    }
}

/// Initialize development logging with hierarchical output.
fn init_dev_logging(config: LogConfig) {
    let filter = create_filter(&config, "flui_core=debug,flui_rendering=debug,flui_engine=debug");

    let colors = config.colors.unwrap_or(true);
    let show_threads = config.show_threads.unwrap_or(false);

    // Use tracing-tree for hierarchical output
    let layer = tracing_tree::HierarchicalLayer::new(2)
        .with_targets(true)
        .with_bracketed_fields(true)
        .with_ansi(colors)
        .with_indent_lines(true)
        .with_thread_ids(show_threads)
        .with_filter(filter);

    tracing_subscriber::registry().with(layer).init();
}

/// Initialize performance logging (minimal overhead).
fn init_perf_logging(config: LogConfig) {
    let filter = create_filter(&config, "flui_core::pipeline=info,flui_engine=warn");

    let colors = config.colors.unwrap_or(true);

    let layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .with_ansi(colors)
        .with_timer(fmt::time::uptime())
        .compact()
        .with_filter(filter);

    tracing_subscriber::registry().with(layer).init();
}

/// Initialize production logging with compact output.
fn init_prod_logging(config: LogConfig) {
    let filter = create_filter(&config, "flui_core=info,flui_rendering=info,flui_engine=info");

    let show_location = config.show_location.unwrap_or(false);
    let show_threads = config.show_threads.unwrap_or(false);

    let layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(show_threads)
        .with_thread_names(false)
        .with_file(show_location)
        .with_line_number(show_location)
        .with_ansi(false)
        .compact()
        .with_filter(filter);

    tracing_subscriber::registry().with(layer).init();
}

/// Create an environment filter with custom directives.
fn create_filter(config: &LogConfig, default: &str) -> EnvFilter {
    if let Some(custom) = &config.filter {
        EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new(custom))
            .unwrap_or_else(|_| EnvFilter::new(default))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default))
    }
}

/// Macro for logging in hot paths (only enabled with feature flag).
///
/// Use this for very frequent operations that would otherwise spam the logs.
///
/// # Examples
///
/// ```no_run
/// use flui_core::trace_hot_path;
///
/// fn paint_child(id: ElementId) {
///     trace_hot_path!("paint_child called", id = ?id);
///     // ... painting logic ...
/// }
/// ```
#[macro_export]
macro_rules! trace_hot_path {
    ($($arg:tt)*) => {
        #[cfg(feature = "trace-hot-paths")]
        ::tracing::trace!($($arg)*);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_config_builder() {
        let config = LogConfig::new(LogMode::Development)
            .with_filter("flui_core=debug")
            .with_colors(true)
            .with_location(false);

        assert_eq!(config.mode, LogMode::Development);
        assert_eq!(config.filter.as_deref(), Some("flui_core=debug"));
        assert_eq!(config.colors, Some(true));
        assert_eq!(config.show_location, Some(false));
    }

    #[test]
    fn test_default_config() {
        let config = LogConfig::default();
        assert_eq!(config.mode, LogMode::Development);
        assert_eq!(config.filter, None);
    }
}
