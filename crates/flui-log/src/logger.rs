use tracing::Level;
use tracing_subscriber::util::{SubscriberInitExt, TryInitError};

/// Error returned when logging initialization fails.
///
/// This is a thin wrapper around [`tracing_subscriber::util::TryInitError`],
/// returned by [`Logger::try_init`] when the global subscriber has already been set.
#[derive(Debug)]
pub struct InitError(TryInitError);

impl core::fmt::Display for InitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "failed to initialize logger: {}", self.0)
    }
}

impl From<TryInitError> for InitError {
    fn from(err: TryInitError) -> Self {
        Self(err)
    }
}

impl std::error::Error for InitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Cross-platform logging for FLUI
///
/// Automatically configures the appropriate logging backend for each platform:
///
/// | Platform | Backend | Output | Viewing Tools |
/// |----------|---------|--------|---------------|
/// | **Desktop** | `tracing-forest` or `fmt` | stdout/stderr | Terminal |
/// | **Android** | `android_log-sys` | logcat | `adb logcat` |
/// | **iOS** | `tracing-oslog` | `os_log` | Xcode Console, Console.app, `log stream` |
/// | **WASM** | `tracing-wasm` | Browser console | `DevTools` (F12) |
///
/// # Platform-Specific Details
///
/// ## Android
/// - Logs to Android's logcat system via native FFI
/// - View with: `adb logcat`, `adb logcat -s flui:*`, or Android Studio Logcat
/// - Tag format: module path (e.g., `flui_core::pipeline`)
/// - Priority mapping: TRACE→VERBOSE, DEBUG→DEBUG, INFO→INFO, WARN→WARN, ERROR→ERROR
///
/// ## iOS
/// - Integrates with Apple's unified logging system (`os_log`)
/// - View with: Xcode Console, Console.app, or `log stream --predicate 'subsystem == "com.flui.app"'`
/// - Subsystem: "com.flui.app" (configurable in source)
/// - Privacy-preserving by default, efficient structured logging
///
/// ## WASM
/// - Outputs to browser `DevTools` console with color coding
/// - Requires browser environment (Chrome, Firefox, Safari, Edge)
/// - **Does NOT work** in Node.js or Cloudflare Workers
/// - Uses `window.performance` API for timing measurements
/// - View Performance tab for span durations
///
/// ## Desktop
/// - Standard output with optional hierarchical formatting
/// - Enable `pretty` feature for `tracing-forest` (recommended for development)
///
/// # Examples
///
/// ```rust,no_run
/// use flui_log::Logger;
///
/// // Use defaults (info level, wgpu=warn filter, app_name="flui")
/// Logger::default().init();
///
/// // Custom configuration with app name
/// use flui_log::Level;
/// Logger::new()
///     .with_app_name("my_game")
///     .with_filter("debug,wgpu=error,flui_core=trace")
///     .with_level(Level::DEBUG)
///     .init();
/// ```
///
/// ## Application Name Usage
///
/// The `app_name` field is used differently on each platform:
///
/// ```rust,no_run
/// use flui_log::Logger;
///
/// // For a game called "space_shooter"
/// Logger::new()
///     .with_app_name("space_shooter")
///     .init();
///
/// // Results in:
/// // - Android: logcat tag "space_shooter" (when module path unavailable)
/// // - iOS: subsystem "com.space_shooter.app"
/// // - WASM: Reserved for future features
/// // - Desktop: Reserved for future features
/// ```
///
/// # Pretty Logging (Desktop only)
///
/// Enable the `"pretty"` feature to use tracing-forest for hierarchical logs:
///
/// ```toml
/// [dependencies]
/// flui_log = { path = "../flui_log", features = ["pretty"] }
/// ```
///
/// ```rust,no_run
/// # #[cfg(feature = "pretty")]
/// use flui_log::Logger;
///
/// # #[cfg(feature = "pretty")]
/// Logger::new()
///     .with_pretty(true)  // Only available with "pretty" feature
///     .init();
/// ```
#[derive(Debug, Clone)]
pub struct Logger {
    /// Application name used for platform-specific logging
    ///
    /// - **Android**: Used as fallback logcat tag when module path is unavailable
    /// - **iOS**: Used as subsystem identifier (e.g., `"com.{app_name}.app"`)
    /// - **WASM**: Could be used for console grouping (future enhancement)
    /// - **Desktop**: Not currently used, reserved for future features
    ///
    /// Default: "flui"
    app_name: String,

    /// Log filter string (e.g. "info,wgpu=warn,flui=debug")
    ///
    /// This uses the `tracing_subscriber::EnvFilter` syntax:
    /// - "info" - Set global level to INFO
    /// - "debug,wgpu=warn" - DEBUG globally, but WARN for wgpu
    /// - "`flui_core=trace`" - TRACE level for `flui_core` module
    filter: String,

    /// Global log level (used as fallback)
    level: Level,

    /// Use pretty hierarchical logging (tracing-forest)
    ///
    /// Only available on desktop with "pretty" feature enabled.
    /// Automatically enabled in debug builds if feature is present.
    #[cfg(feature = "pretty")]
    use_pretty: bool,
}

impl Default for Logger {
    fn default() -> Self {
        Self {
            app_name: "flui".to_string(),
            filter: "info,wgpu=warn".to_string(),
            level: Level::INFO,
            #[cfg(feature = "pretty")]
            use_pretty: cfg!(debug_assertions), // auto-enable in debug mode
        }
    }
}

impl Logger {
    /// Create a new Logger with default settings
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current application name
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::Logger;
    ///
    /// let logger = Logger::new();
    /// assert_eq!(logger.app_name(), "flui");
    /// ```
    #[inline]
    #[must_use]
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// Get the current log filter string
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::Logger;
    ///
    /// let logger = Logger::new();
    /// assert_eq!(logger.filter(), "info,wgpu=warn");
    /// ```
    #[inline]
    #[must_use]
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Get the current global log level
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::{Logger, Level};
    ///
    /// let logger = Logger::new();
    /// assert_eq!(logger.level(), &Level::INFO);
    /// ```
    #[inline]
    #[must_use]
    pub fn level(&self) -> &Level {
        &self.level
    }

    /// Check if pretty logging is enabled (desktop only)
    ///
    /// Only available with the "pretty" feature flag.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::Logger;
    ///
    /// let logger = Logger::new();
    /// #[cfg(feature = "pretty")]
    /// let is_pretty = logger.use_pretty();
    /// ```
    #[cfg(feature = "pretty")]
    #[inline]
    #[must_use]
    pub fn use_pretty(&self) -> bool {
        self.use_pretty
    }

    /// Set the application name
    ///
    /// The application name is used for platform-specific logging:
    /// - **Android**: Fallback logcat tag when module path unavailable
    /// - **iOS**: Subsystem identifier (formatted as `"com.{app_name}.app"`)
    /// - **WASM**: Reserved for future console grouping features
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::Logger;
    ///
    /// let logger = Logger::new()
    ///     .with_app_name("my_game");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_app_name(mut self, app_name: impl Into<String>) -> Self {
        self.app_name = app_name.into();
        self
    }

    /// Set the log filter string
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::Logger;
    ///
    /// let logger = Logger::new()
    ///     .with_filter("debug,wgpu=warn,flui_core=trace");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = filter.into();
        self
    }

    /// Set the global log level
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::{Logger, Level};
    ///
    /// let logger = Logger::new()
    ///     .with_level(Level::DEBUG);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Enable/disable pretty hierarchical logging (desktop only)
    ///
    /// Only available with the "pretty" feature flag.
    /// Has no effect on mobile/WASM platforms.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::Logger;
    ///
    /// let logger = Logger::new()
    ///     .with_pretty(true);  // Requires "pretty" feature
    /// ```
    #[cfg(feature = "pretty")]
    #[inline]
    #[must_use]
    pub fn with_pretty(mut self, pretty: bool) -> Self {
        self.use_pretty = pretty;
        self
    }

    /// Initialize the logging system
    ///
    /// This should be called once at application startup, before any logging occurs.
    /// The appropriate backend will be selected based on the target platform.
    ///
    /// # Platform Selection
    ///
    /// - **Desktop**: Uses `tracing-forest` (if "pretty" feature enabled) or `fmt` layer
    /// - **Android**: Uses `android_log-sys` → logcat
    /// - **iOS**: Uses `tracing-oslog` → `os_log`
    /// - **WASM**: Uses `tracing-wasm` → browser console
    ///
    /// # Panics
    ///
    /// Panics if the global tracing subscriber has already been set.
    /// Use [`try_init`](Self::try_init) for a non-panicking alternative.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use flui_log::Logger;
    ///
    /// // Simple initialization
    /// Logger::default().init();
    ///
    /// // With custom config
    /// Logger::new()
    ///     .with_filter("debug,wgpu=error")
    ///     .init();
    /// ```
    pub fn init(&self) {
        self.try_init().expect("Failed to initialize logger");
    }

    /// Try to initialize the logging system, returning an error on failure.
    ///
    /// This is the non-panicking alternative to [`init`](Self::init). Returns
    /// an error if the global tracing subscriber has already been set — useful
    /// in tests where multiple test cases may attempt initialization.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use flui_log::Logger;
    ///
    /// // In tests — ignore double-init
    /// let _ = Logger::default().try_init();
    ///
    /// // In applications — handle the error
    /// Logger::default().try_init().expect("logging already initialized");
    /// ```
    pub fn try_init(&self) -> Result<(), InitError> {
        use tracing_subscriber::{layer::SubscriberExt, Registry};

        // Create filter layer from environment or config
        let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
            .or_else(|_| tracing_subscriber::EnvFilter::try_new(&self.filter))
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

        // === DESKTOP (not Android/iOS/WASM) ===
        #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
        {
            // Pretty hierarchical logging with tracing-forest (if feature enabled)
            #[cfg(feature = "pretty")]
            if self.use_pretty {
                use tracing_forest::{util::LevelFilter, ForestLayer};

                let subscriber = Registry::default()
                    .with(filter_layer)
                    .with(LevelFilter::from_level(self.level))
                    .with(ForestLayer::default());

                subscriber.try_init()?;
                return Ok(());
            }

            // Standard fmt layer for desktop (fallback or when "pretty" not enabled)
            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_level(true)
                .with_line_number(true);

            let subscriber = Registry::default()
                .with(filter_layer)
                .with(tracing_subscriber::filter::LevelFilter::from_level(
                    self.level,
                ))
                .with(fmt_layer);

            subscriber.try_init()?;
        }

        // === ANDROID ===
        #[cfg(target_os = "android")]
        {
            let android_layer = crate::android_layer::AndroidLayer::new(&self.app_name);

            let subscriber = Registry::default().with(filter_layer).with(android_layer);

            subscriber.try_init()?;

            tracing::info!("Logging initialized (Android/logcat)");
        }

        // === iOS ===
        #[cfg(target_os = "ios")]
        {
            use tracing_oslog::OsLogger;

            let subsystem = format!("com.{}.app", self.app_name);
            let os_logger = OsLogger::new(&subsystem, "default");

            let subscriber = Registry::default().with(filter_layer).with(os_logger);

            subscriber.try_init()?;

            tracing::info!("Logging initialized (iOS/os_log)");
        }

        // === WASM ===
        #[cfg(target_arch = "wasm32")]
        {
            use tracing_wasm::WASMLayerConfigBuilder;

            let wasm_layer = tracing_wasm::WASMLayer::new(
                WASMLayerConfigBuilder::new()
                    .set_max_level(self.level)
                    .build(),
            );

            let subscriber = Registry::default().with(filter_layer).with(wasm_layer);

            subscriber.try_init()?;

            tracing::info!("Logging initialized (WASM/browser console)");
        }

        Ok(())
    }
}

// Convenience functions for quick setup
impl Logger {
    /// Initialize with default settings
    ///
    /// Equivalent to `Logger::default().init()`
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use flui_log::Logger;
    ///
    /// Logger::init_default();
    /// ```
    pub fn init_default() {
        Self::default().init();
    }

    /// Initialize with a custom filter string
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use flui_log::Logger;
    ///
    /// Logger::init_with_filter("debug,wgpu=warn");
    /// ```
    pub fn init_with_filter(filter: impl Into<String>) {
        Self::new().with_filter(filter).init();
    }
}
