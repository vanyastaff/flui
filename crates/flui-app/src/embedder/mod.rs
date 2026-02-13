//! Platform embedder utilities for FLUI
//!
//! This module provides the adapter types that connect
//! the FLUI framework to the underlying platform (windowing, GPU, events).
//!
//! # Architecture
//!
//! ```text
//! Platform callbacks → AppBinding (central coordinator)
//!   ├── GestureBinding (pointer events + hit testing)
//!   ├── FocusManager (keyboard events)
//!   ├── WidgetsBinding (build phase)
//!   ├── RenderPipelineOwner (layout/paint)
//!   └── Renderer (GPU rendering, owned by runner callback)
//! ```
//!
//! # Platform Support
//!
//! - **Desktop**: Windows, macOS, Linux via flui-platform + wgpu
//! - **Android**: (future) android-activity integration
//! - **iOS**: (future) UIKit integration
//! - **Web**: (future) wasm-bindgen integration

mod desktop;
mod embedder_scheduler;

pub use desktop::EmbedderError;
pub(crate) use desktop::PlatformWindowHandle;

// Re-export GestureBinding from flui_interaction (no duplication)
pub use flui_interaction::binding::GestureBinding;
