//! Cross-platform logging for FLUI
//!
//! Provides automatic logging configuration for:
//! - **Desktop**: stdout via `tracing_subscriber::fmt` (or `tracing-forest` with "pretty" feature)
//! - **Android**: logcat via `android_log-sys`
//! - **iOS**: `os_log` via `tracing-oslog`
//! - **WASM**: browser console via `tracing-wasm`
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use flui_log::Logger;
//!
//! // Initialize with defaults
//! Logger::default().init();
//!
//! // Use tracing macros anywhere
//! tracing::info!("App started!");
//! tracing::debug!("Debug info");
//! ```
//!
//! # Custom Configuration
//!
//! ```rust,no_run
//! use flui_log::{Logger, Level};
//!
//! Logger::new()
//!     .with_filter("debug,wgpu=error,flui_core=trace")
//!     .with_level(Level::DEBUG)
//!     .init();
//! ```
//!
//! # Pretty Logging (Desktop Development)
//!
//! Enable the `"pretty"` feature to use hierarchical tree-based logging:
//!
//! ```toml
//! [dependencies]
//! flui_log = { path = "../flui_log", features = ["pretty"] }
//! ```
//!
//! ```rust,no_run
//! # #[cfg(feature = "pretty")]
//! use flui_log::Logger;
//!
//! # #[cfg(feature = "pretty")]
//! Logger::new()
//!     .with_pretty(true)  // Requires "pretty" feature
//!     .init();
//!
//! // Logs will show hierarchical structure:
//! // TRACE main [ 1.2s | 100.00% ]
//! // ├─ INFO ｉ App started
//! // ├─ TRACE init [ 850ms | 70.83% ]
//! // │  └─ DEBUG Loading resources
//! // └─ INFO ｉ Ready
//! ```
//!
//! # Platform Behavior
//!
//! The logger automatically selects the appropriate backend:
//!
//! | Platform | Backend | Where to see logs |
//! |----------|---------|-------------------|
//! | Desktop  | `fmt` or `tracing-forest` | Terminal/stdout |
//! | Android  | `android_log-sys` | `adb logcat` |
//! | iOS      | `tracing-oslog` | Xcode Console / Console.app |
//! | WASM     | `tracing-wasm` | Browser `DevTools` Console |
//!
//! # Environment Variables
//!
//! The `RUST_LOG` environment variable can override the default filter:
//!
//! ```bash
//! # Set log level
//! RUST_LOG=debug cargo run
//!
//! # Filter specific modules
//! RUST_LOG=info,wgpu=warn,flui_core=trace cargo run
//! ```

mod logger;

#[cfg(target_os = "android")]
pub mod android_layer;

pub use logger::Logger;

// Re-export tracing macros for convenience
pub use tracing::{debug, error, info, trace, warn, Level};

// Re-export common types
pub use tracing::{event, span, Instrument, Span};
