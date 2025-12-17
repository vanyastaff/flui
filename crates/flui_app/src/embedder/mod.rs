//! Platform embedders for FLUI
//!
//! This module contains the embedder implementations that connect
//! the FLUI framework to the underlying platform (windowing, GPU, events).
//!
//! # Architecture
//!
//! ```text
//! embedder/
//!   ├── core.rs           - EmbedderCore (shared 90% logic)
//!   ├── desktop.rs        - DesktopEmbedder (Windows/macOS/Linux)
//!   ├── frame_coordinator.rs - Frame rendering coordination
//!   ├── pointer_state.rs  - Pointer tracking and coalescing
//!   ├── scene_cache.rs    - Scene caching for hit testing
//!   └── scheduler_binding.rs - Frame scheduling integration
//! ```
//!
//! # Platform Support
//!
//! - **Desktop**: Windows, macOS, Linux via winit + wgpu
//! - **Android**: (future) android-activity integration
//! - **iOS**: (future) UIKit integration
//! - **Web**: (future) wasm-bindgen integration

mod core;
mod desktop;
mod embedder_scheduler;
mod frame_coordinator;
mod pointer_state;
mod scene_cache;

pub use core::EmbedderCore;
pub use desktop::{DesktopEmbedder, EmbedderError};
pub use embedder_scheduler::{EmbedderScheduler, SchedulerStats};
pub use frame_coordinator::{FrameCoordinator, FrameResult};
pub use pointer_state::PointerState;
pub use scene_cache::SceneCache;

// Re-export GestureBinding from flui_interaction (no duplication)
pub use flui_interaction::binding::GestureBinding;
