//! FLUI Application Framework
//!
//! This crate provides the application framework for FLUI, combining:
//! - `WidgetsBinding` from flui-view (build phase)
//! - `PipelineOwner` from flui_rendering (layout/paint phases)
//! - `GestureBinding` from flui_interaction (input handling + event coalescing)
//!
//! # Architecture
//!
//! ```text
//! flui_app
//!   ├── app/
//!   │   ├── binding.rs      - WidgetsFlutterBinding (combines all bindings)
//!   │   ├── config.rs       - AppConfig
//!   │   └── lifecycle.rs    - AppLifecycle
//!   │
//!   ├── bindings/           - Re-exports from other crates
//!   │
//!   └── debug/
//!       └── flags.rs        - DebugFlags
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_app::WidgetsFlutterBinding;
//!
//! fn main() {
//!     let binding = WidgetsFlutterBinding::instance();
//!     // Use binding to manage the application lifecycle
//! }
//! ```

#![warn(missing_docs)]
#![warn(missing_debug_implementations)]

// Modules
pub mod app;
pub mod bindings;
pub mod debug;
pub mod embedder;
pub mod overlay;
pub mod theme;

// Primary exports - Flutter naming
pub use app::{
    run_app, run_app_with_config, AppConfig, AppLifecycle, RootRenderElement, RootRenderView,
    WidgetsFlutterBinding,
};

// Legacy alias
pub use app::AppBinding;

// Debug exports
pub use debug::DebugFlags;

// Bindings re-exports
pub use bindings::{
    GestureBinding, PaintingBinding, PipelineOwner, RenderingFlutterBinding, Scheduler,
    SemanticsBinding, WidgetsBinding,
};

// Convenience re-exports from flui-view
pub use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementBase, ElementTree, StatefulView,
    StatelessElement, StatelessView, View,
};

// Convenience re-exports from flui_log
pub use flui_log::{debug, error, info, trace, warn, Level, Logger};

// ============================================================================
// PRELUDE
// ============================================================================

/// Prelude module with commonly used types.
///
/// # Usage
///
/// ```rust,ignore
/// use flui_app::prelude::*;
/// ```
pub mod prelude {
    // Application types
    pub use crate::{run_app, run_app_with_config, AppConfig, AppLifecycle, WidgetsFlutterBinding};

    // Bindings
    pub use crate::{
        GestureBinding, PaintingBinding, PipelineOwner, RenderingFlutterBinding, Scheduler,
        SemanticsBinding, WidgetsBinding,
    };

    // Debug
    pub use crate::DebugFlags;

    // Logging
    pub use flui_log::{debug, error, info, trace, warn};
}
