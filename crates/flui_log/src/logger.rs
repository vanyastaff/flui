use tracing::Level;

/// Cross-platform logging for FLUI
///
/// Automatically configures the appropriate logging backend for each platform:
/// - Desktop: stdout with optional pretty formatting (tracing-forest)
/// - Android: logcat integration
/// - iOS: os_log integration
/// - WASM: browser console
///
/// # Examples
///
/// ```rust,no_run
/// use flui_log::Logger;
///
/// // Use defaults (info level, wgpu=warn filter)
/// Logger::default().init();
///
/// // Custom configuration
/// use flui_log::Level;
/// Logger::new()
///     .with_filter("debug,wgpu=error,flui_core=trace")
///     .with_level(Level::DEBUG)
///     .init();
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
pub struct Logger {
    /// Log filter string (e.g. "info,wgpu=warn,flui=debug")
    ///
    /// This uses the `tracing_subscriber::EnvFilter` syntax:
    /// - "info" - Set global level to INFO
    /// - "debug,wgpu=warn" - DEBUG globally, but WARN for wgpu
    /// - "flui_core=trace" - TRACE level for flui_core module
    pub filter: String,

    /// Global log level (used as fallback)
    pub level: Level,

    /// Use pretty hierarchical logging (tracing-forest)
    ///
    /// Only available on desktop with "pretty" feature enabled.
    /// Automatically enabled in debug builds if feature is present.
    #[cfg(feature = "pretty")]
    pub use_pretty: bool,
}

impl Default for Logger {
    fn default() -> Self {
        Self {
            filter: "info,wgpu=warn".to_string(),
            level: Level::INFO,
            #[cfg(feature = "pretty")]
            use_pretty: cfg!(debug_assertions), // auto-enable in debug mode
        }
    }
}

impl Logger {
    /// Create a new Logger with default settings
    pub fn new() -> Self {
        Self::default()
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
    /// - **iOS**: Uses `tracing-oslog` → os_log
    /// - **WASM**: Uses `tracing-wasm` → browser console
    ///
    /// # Panics
    ///
    /// Panics if the global tracing subscriber has already been set.
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

                tracing::subscriber::set_global_default(subscriber)
                    .expect("Failed to set tracing subscriber");

                return;
            }

            // Standard fmt layer for desktop (fallback or when "pretty" not enabled)
            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_level(true)
                .with_line_number(true);

            let subscriber = Registry::default().with(filter_layer).with(fmt_layer);

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }

        // === ANDROID ===
        #[cfg(target_os = "android")]
        {
            let subscriber = Registry::default()
                .with(filter_layer)
                .with(crate::android_layer::AndroidLayer::default());

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");

            tracing::info!("Logging initialized (Android/logcat)");
        }

        // === iOS ===
        #[cfg(target_os = "ios")]
        {
            use tracing_oslog::OsLogger;

            let os_logger = OsLogger::new("com.flui.app", "default");

            let subscriber = Registry::default().with(filter_layer).with(os_logger);

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");

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

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");

            tracing::info!("Logging initialized (WASM/browser console)");
        }
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
