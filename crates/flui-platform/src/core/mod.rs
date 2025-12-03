//! Core shared implementation
//!
//! This module contains the shared logic used by all platform embedders.
//! The `EmbedderCore` struct provides 90%+ of the functionality.

mod embedder_core;
mod frame_coordinator;
mod pointer_state;
mod scene_cache;

pub use embedder_core::EmbedderCore;
pub use frame_coordinator::FrameCoordinator;
pub use pointer_state::PointerState;
pub use scene_cache::SceneCache;
