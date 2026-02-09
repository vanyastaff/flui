//! Platform embedders for FLUI
//!
//! This module contains the embedder implementations that connect
//! the FLUI framework to the underlying platform (windowing, GPU, events).
//!
//! # Architecture
//!
//! ```text
//! AppBinding (central coordinator - all framework logic)
//!   ├── WidgetsBinding (build phase)
//!   ├── RenderPipelineOwner (layout/paint)
//!   ├── GestureBinding (hit testing)
//!   ├── SceneCache (hit testing cache)
//!   └── FrameCoordinator (frame stats)
//!
//! embedder/
//!   ├── desktop.rs        - DesktopEmbedder (Windows/macOS/Linux)
//!   ├── frame_coordinator.rs - Frame rendering coordination
//!   ├── pointer_state.rs  - Pointer tracking and coalescing
//!   └── scene_cache.rs    - Scene caching for hit testing
//! ```
//!
//! # Platform Support
//!
//! - **Desktop**: Windows, macOS, Linux via winit + wgpu
//! - **Android**: (future) android-activity integration
//! - **iOS**: (future) UIKit integration
//! - **Web**: (future) wasm-bindgen integration

mod desktop;
mod frame_coordinator;
mod pointer_state;
mod scene_cache;

pub use desktop::{DesktopEmbedder, EmbedderError};
pub use frame_coordinator::{FrameCoordinator, FrameResult};
pub use pointer_state::PointerState;
pub use scene_cache::SceneCache;

// Re-export GestureBinding from flui_interaction (no duplication)
pub use flui_interaction::binding::GestureBinding;
