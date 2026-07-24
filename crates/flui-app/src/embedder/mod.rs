//! Platform-facing framework types.
//!
//! # Architecture
//!
//! ```text
//! Platform callbacks → entered UiRealm + transitional AppBinding
//!   ├── UiRealm::GestureBinding (pointer events + hit testing)
//!   ├── UiRealm::FocusManager (keyboard events)
//!   ├── UiRealm::WidgetsBinding (build phase)
//!   ├── AppBinding::RenderPipelineOwner (layout/paint)
//!   └── Renderer (GPU rendering, owned by runner callback)
//! ```
//!
//! # Platform Support
//!
//! - **Desktop**: Windows, macOS, Linux via flui-platform + wgpu
//! - **Android**: (future) android-activity integration
//! - **iOS**: (future) UIKit integration
//! - **Web**: (future) wasm-bindgen integration

// Re-export GestureBinding from flui_interaction (no duplication)
pub use flui_interaction::binding::GestureBinding;
