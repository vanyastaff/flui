//! FLUI Application Framework
//!
//! This crate provides the application framework for FLUI, hosting:
//! - an owner-affine `UiRealm` with `WidgetsBinding` (build phase)
//! - `PipelineOwner` from flui_rendering (layout/paint phases)
//! - a realm-owned `GestureBinding` from flui_interaction (input handling + event coalescing)
//!
//! # Architecture
//!
//! ```text
//! flui_app
//!   ├── app/
//!   │   ├── runtime.rs      - AppRuntime: the loop-scoped composition root
//!   │   ├── ui_realm.rs     - owner-affine widget, render, and gesture runtime
//!   │   ├── presentation.rs - per-presentation window/haptics/frame-accounting state
//!   │   ├── config.rs       - AppConfig
//!   │   ├── direct.rs       - direct rendering mode (bypasses the widget tree)
//!   │   └── runner.rs       - platform bootstrap (desktop/android/web run loops)
//!   │
//!   ├── bindings/           - Re-exports from other crates
//!   └── embedder/           - Platform embedder adapters (window handle, GPU surface)
//! ```
//!
//! # This crate owns no design tokens
//!
//! Colours, typography, spacing, radius, and motion are design-system
//! concerns and live in `flui-material` / `flui-cupertino`. Appearance is
//! per-presentation: the OS light/dark signal reaches a tree as
//! `MediaQueryData::platform_brightness`, and the resolved theme is published
//! by an in-tree inherited widget, separately in each window. See
//! [ADR-0042](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0042-theming-ownership.md);
//! the app-shell widgets that implement it are tracked in issue #573.
//!
//! Applications normally enter through [`run_app`] or
//! [`run_app_with_config`]; the runner constructs and owns the UI realm.

// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

// Modules
pub mod app;
pub mod bindings;
pub mod embedder; // PORT-CHECK-OK-SP4: embedder API surface; binding entry for app integrators

// Primary exports - Flutter naming
pub use app::{
    AppConfig, DiagnosticsProfile, RootRenderElement, RootRenderView, run_app, run_app_with_config,
    run_direct,
};
// Host-injected execution seam (issue #557): an embedded host provides its
// own worker pools via `AppConfig::with_executors`, and the runtime's
// default pools are then never constructed. `DeterministicExecutors` is the
// single-threaded reference implementation for tests and reproducible hosts.
pub use app::{
    ComputeJob, DeterministicExecutors, HostComputePool, HostExecutors, HostIoPool, IoFuture,
    SpawnError,
};
// Typed frame-failure route (issue #561): the embedder-visible half of the
// presentation-frame transaction boundary (ADR-0048). A failed frame is
// contained to its own presentation; these types are how the embedder hears
// about it.
pub use app::{FrameFailureHandler, FrameFailureKind, FrameFailureReport};
// Multi-window policy knobs (issue #555's embedder-facing seam) — not
// available on iOS, where `AppRuntime`/`UiRealm`'s realm-hosting machinery
// itself is not compiled (see `runtime::ExitPolicy`'s own doc).
#[cfg(not(target_os = "ios"))]
pub use app::{ExitPolicy, WindowPolicy};
// `open_secondary_window` additionally needs the desktop GPU renderer path,
// so it is narrower than the policy enums above: desktop only (matching
// `run_desktop`'s own cfg gate).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use app::open_secondary_window;
// Android-specific entry points
#[cfg(target_os = "android")]
pub use app::{run_app_android, run_app_android_with_config};
// Bindings re-exports
pub use bindings::{
    GestureBinding, PaintingBinding, PipelineCell, PipelineOwner, RenderingFlutterBinding,
    UpdateScheduler, WidgetsBinding,
};
// Application identity is part of `AppConfig`; low-level subscriber/filter
// controls remain in `flui-log` rather than leaking through this API surface.
pub use flui_log::{AppIdentity, AppleBundleId};
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
    // Logging: the macros come straight from `tracing`, which is where every
    // FLUI crate emits from. Nothing in the prelude installs a subscriber.
    pub use tracing::{debug, error, info, trace, warn};

    pub use crate::{AppConfig, run_app, run_app_with_config, run_direct};
    // Bindings
    pub use crate::{
        GestureBinding, PaintingBinding, PipelineOwner, RenderingFlutterBinding, UpdateScheduler,
        WidgetsBinding,
    };
}
