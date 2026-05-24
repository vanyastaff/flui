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
pub mod embedder; // PORT-CHECK-OK-SP4: embedder API surface; binding entry for app integrators
pub mod overlay; // PORT-CHECK-OK-SP4: overlay API surface; binding entry for app integrators
pub mod theme; // PORT-CHECK-OK-SP4: theme API surface; binding entry for app integrators

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
// Convenience re-exports from flui_foundation::log (merged from flui-log in
// D-block PR-C-1 U2).
pub use flui_foundation::log::{Level, Logger, debug, error, info, trace, warn};
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
    pub use flui_foundation::log::{debug, error, info, trace, warn};

    pub use crate::{
        AppConfig, LifecycleState, WidgetsFlutterBinding, run_app, run_app_with_config, run_direct,
    };
    // Bindings
    pub use crate::{
        GestureBinding, PaintingBinding, PipelineOwner, RenderingFlutterBinding, Scheduler,
        SemanticsBinding, WidgetsBinding,
    };
}
