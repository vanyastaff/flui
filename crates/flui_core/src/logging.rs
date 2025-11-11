//! Structured logging utilities for FLUI framework.
//!
//! This module provides a clean, hierarchical logging API with different modes
//! for development, performance profiling, and production use.

use tracing_subscriber::{
    fmt::{self},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer, Registry,
};
use tracing_forest::ForestLayer;

/// Logging configuration for FLUI applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    /// Development mode with hierarchical, colored output and automatic timing.
    /// Uses tracing-forest for context-preserving logs with [ duration | percentage ] display.
    /// Perfect for debugging async operations and complex frame rendering.
    Development,

    /// Performance mode - only logs frame times and performance metrics.
    /// Minimal overhead for profiling.
    Performance,

    /// Production mode with compact output.
    /// Suitable for production deployments.
    Production,

    /// Silent mode - no logging output.
    /// Useful for benchmarks and tests.
    Silent,
}

/// Predefined log detail levels for common use cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogDetail {
    /// Minimal - Only warnings and frame stats (production).
    /// Filter: "flui=warn,flui_app::app=info"
    Minimal,

    /// Standard - Frame timing and major phase completion (default development).
    /// Filter: "flui_app=info,flui_core::pipeline=debug"
    Standard,

    /// Verbose - All debug logs including element operations.
    /// Filter: "flui=debug"
    Verbose,

    /// Trace - Everything including hot paths (for debugging specific issues).
    /// Filter: "flui=trace"
    Trace,
}

/// Configuration for FLUI logging system.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Logging mode (development, performance, production, silent).
    pub mode: LogMode,

    /// Detail level (minimal, standard, verbose, trace).
    pub detail: Option<LogDetail>,

    /// Custom filter directives (overrides defaults and detail level).
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
            detail: Some(LogDetail::Standard),
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

    /// Set detail level (minimal, standard, verbose, trace).
    pub fn with_detail(mut self, detail: LogDetail) -> Self {
        self.detail = Some(detail);
        self
    }

    /// Set custom filter directives (overrides detail level).
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
/// # Features
///
/// **Development Mode:**
/// - Hierarchical output with tracing-forest
/// - Automatic timing: `[ 2.1ms | 100% ]` for each span
/// - Context-preserving logs (async-safe)
/// - Color-coded by log level
///
/// **Performance Mode:**
/// - Minimal overhead, only critical logs
/// - Frame timing and pipeline metrics
///
/// # Examples
///
/// ```no_run
/// use flui_core::logging::{init_logging, LogConfig, LogMode};
///
/// // Development mode with hierarchical output + automatic timing
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
///
/// # Expected Output (Development Mode)
///
/// ```text
/// INFO frame [ 2.1ms | 100.00% ] constraints: BoxConstraints { ... }
/// INFO ┝━ build [ 0.8ms | 38.10% ]
/// INFO │  ┕━ Build complete count: 1
/// INFO ┝━ layout [ 0.5ms | 23.81% ]
/// INFO │  ┕━ Layout complete count: 1
/// INFO ┝━ paint [ 0.3ms | 14.29% ]
/// INFO │  ┕━ Paint complete count: 1
/// INFO ┕━ Frame complete
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
    let filter = create_filter_from_detail(&config);

    // Use tracing-forest for hierarchical output with automatic timing
    // Note: tracing-forest always uses ANSI colors in default mode
    let forest_layer = ForestLayer::default().with_filter(filter);

    Registry::default().with(forest_layer).init();
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

/// Create an environment filter based on detail level.
fn create_filter_from_detail(config: &LogConfig) -> EnvFilter {
    // If custom filter provided, use it
    if let Some(custom) = &config.filter {
        return EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new(custom))
            .unwrap_or_else(|_| EnvFilter::new("flui=info"));
    }

    // Otherwise use detail level
    let detail_filter = match config.detail {
        Some(LogDetail::Minimal) => "flui=warn,flui_app::app=info",
        Some(LogDetail::Standard) => "flui_app=info,flui_core::pipeline=debug,flui_core::element=info,flui_rendering=info,flui_engine=warn",
        Some(LogDetail::Verbose) => "flui=debug",
        Some(LogDetail::Trace) => "flui=trace",
        None => "flui_app=info,flui_core::pipeline=debug",
    };

    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(detail_filter))
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
