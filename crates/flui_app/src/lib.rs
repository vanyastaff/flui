//! FLUI Application Framework
//!
//! This crate provides the application framework for FLUI, combining:
//! - `WidgetsBinding` from flui-view (build phase)
//! - `PipelineOwner` from flui_rendering (layout/paint phases)
//! - `GestureBinding` from flui_interaction (input handling)
//!
//! # Architecture
//!
//! ```text
//! flui_app
//!   ├── app/
//!   │   ├── binding.rs      - AppBinding (combines all bindings)
//!   │   ├── runner.rs       - run_app() entry point
//!   │   └── lifecycle.rs    - App lifecycle management
//!   │
//!   ├── theme/
//!   │   ├── theme_data.rs   - ThemeData struct
//!   │   ├── colors.rs       - Color schemes
//!   │   └── typography.rs   - Text styles
//!   │
//!   ├── overlay/
//!   │   ├── overlay.rs      - Overlay system
//!   │   └── entries.rs      - OverlayEntry management
//!   │
//!   └── debug/
//!       ├── performance.rs  - Performance overlay
//!       └── inspector.rs    - Widget inspector
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_app::run_app;
//! use flui_view::prelude::*;
//!
//! struct MyApp;
//!
//! impl StatelessView for MyApp {
//!     fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View> {
//!         // Your UI here
//!         Box::new(MyApp)
//!     }
//! }
//!
//! fn main() {
//!     run_app(MyApp);
//! }
//! ```

#![warn(missing_docs)]
#![warn(missing_debug_implementations)]

// Modules
pub mod app;
pub mod debug;
pub mod overlay;
pub mod theme;

// Re-exports
pub use app::{AppBinding, AppConfig, AppLifecycle};
pub use debug::{DebugFlags, FrameStats, PerformanceOverlay};
pub use overlay::{OverlayEntry, OverlayManager, OverlayPosition};
pub use theme::{Color, ColorScheme, Theme, ThemeMode};

// Convenience re-exports from flui-view
pub use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementTree, StatefulView, StatelessView, View,
    WidgetsBinding,
};

// Convenience re-exports from flui_rendering
pub use flui_rendering::pipeline::PipelineOwner as RenderPipelineOwner;

// Convenience re-exports from flui_interaction
pub use flui_interaction::GestureBinding;

/// Run a FLUI application.
///
/// This is the main entry point for FLUI apps. It:
/// 1. Initializes the AppBinding singleton
/// 2. Attaches the root widget
/// 3. Starts the platform event loop
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::run_app;
///
/// fn main() {
///     run_app(MyApp);
/// }
/// ```
///
/// # Platform Support
///
/// - Desktop (Windows, macOS, Linux): Uses winit event loop
/// - Android: Uses android-activity
/// - iOS: Uses UIKit integration
/// - Web: Uses wasm-bindgen
pub fn run_app<V>(root: V)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    app::runner::run_app_impl(root);
}

/// Run a FLUI application with custom configuration.
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::{run_app_with_config, AppConfig};
///
/// fn main() {
///     let config = AppConfig::new()
///         .with_title("My App")
///         .with_size(800, 600);
///
///     run_app_with_config(MyApp, config);
/// }
/// ```
pub fn run_app_with_config<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    app::runner::run_app_with_config_impl(root, config);
}
