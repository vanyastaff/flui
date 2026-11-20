//! Platform-facing framework types.
//!
//! # Architecture
//!
//! ```text
//! Platform callbacks → entered UiRuntime
//!   ├── UiRuntime::GestureBinding (pointer events + hit testing)
//!   ├── UiRuntime::FocusManager (keyboard events)
//!   ├── UiRuntime::WidgetsBinding (build phase)
//!   ├── PresentationState::PipelineOwner (layout/paint)
//!   └── RasterHost (GPU rendering: a slot of the process's GpuContext)
//! ```
//!
//! # Platform Support
//!
//! - **Desktop**: Windows, macOS, Linux via flui-native + wgpu
//! - **Android**: (future) android-activity integration
//! - **iOS**: (future) UIKit integration
//! - **Web**: (future) wasm-bindgen integration

// Re-export GestureBinding from flui_interaction (no duplication)
pub use flui_interaction::binding::GestureBinding;
