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
// Legacy alias
pub use app::{
    AppBinding, AppConfig, DefaultLifecycle, LifecycleEvent, LifecycleState, RootRenderElement,
    RootRenderView, WidgetsFlutterBinding, run_app, run_app_with_config, run_direct,
};
// Android-specific entry points
#[cfg(target_os = "android")]
pub use app::{run_app_android, run_app_android_with_config};
// Bindings re-exports
pub use bindings::{
    GestureBinding, PaintingBinding, PipelineOwner, RenderingFlutterBinding, Scheduler,
    SemanticsBinding, WidgetsBinding,
};
// Debug exports
pub use debug::DebugFlags;
// Convenience re-exports from flui_log
pub use flui_log::{Level, Logger, debug, error, info, trace, warn};
// Convenience re-exports from flui-view
pub use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementBase, ElementTree, StatefulView,
    StatelessElement, StatelessView, View,
};

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
    // Logging
    pub use flui_log::{debug, error, info, trace, warn};

    // Debug
    pub use crate::DebugFlags;
    pub use crate::{
        AppConfig, LifecycleState, WidgetsFlutterBinding, run_app, run_app_with_config,
        run_direct,
    };
    // Bindings
    pub use crate::{
        GestureBinding, PaintingBinding, PipelineOwner, RenderingFlutterBinding, Scheduler,
        SemanticsBinding, WidgetsBinding,
    };
}
