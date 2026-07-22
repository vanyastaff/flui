//! FLUI Application Framework
//!
//! This crate provides the application framework for FLUI, hosting:
//! - an owner-affine `UiRealm` with `WidgetsBinding` (build phase)
//! - `PipelineOwner` from flui_rendering (layout/paint phases)
//! - `GestureBinding` from flui_interaction (input handling + event coalescing)
//!
//! # Architecture
//!
//! ```text
//! flui_app
//!   ├── app/
//!   │   ├── binding.rs      - transitional process service host (AppBinding)
//!   │   ├── ui_realm.rs     - owner-affine widget runtime
//!   │   ├── config.rs       - AppConfig
//!   │   ├── direct.rs       - direct rendering mode (bypasses the widget tree)
//!   │   └── runner.rs       - platform bootstrap (desktop/android/web run loops)
//!   │
//!   ├── bindings/           - Re-exports from other crates
//!   ├── embedder/           - Platform embedder adapters (window handle, GPU surface)
//!   └── theme/              - AppTheme/AppColorScheme (parked, unwired)
//! ```
//!
//! Applications normally enter through [`run_app`] or
//! [`run_app_with_config`]; the runner constructs and owns the UI realm.

// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

// Modules
pub mod app;
pub mod bindings;
pub mod embedder; // PORT-CHECK-OK-SP4: embedder API surface; binding entry for app integrators
pub mod theme; // PORT-CHECK-OK-SP4: theme API surface; binding entry for app integrators

// Primary exports - Flutter naming
// Legacy alias
pub use app::{
    AppBinding, AppConfig, RootRenderElement, RootRenderView, WidgetsFlutterBinding, run_app,
    run_app_with_config, run_direct,
};
// Android-specific entry points
#[cfg(target_os = "android")]
pub use app::{run_app_android, run_app_android_with_config};
// Bindings re-exports
pub use bindings::{
    GestureBinding, PaintingBinding, PipelineOwner, RenderingFlutterBinding, Scheduler,
    SemanticsBinding, WidgetsBinding,
};
// Convenience re-exports from flui_foundation::log (merged from flui-log).
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

    pub use crate::{AppConfig, WidgetsFlutterBinding, run_app, run_app_with_config, run_direct};
    // Bindings
    pub use crate::{
        GestureBinding, PaintingBinding, PipelineOwner, RenderingFlutterBinding, Scheduler,
        SemanticsBinding, WidgetsBinding,
    };
}
