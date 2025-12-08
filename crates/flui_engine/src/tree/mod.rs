//! Tree module - LayerTree and related types
//!
//! This module provides the LayerTree data structure for storing compositor layers
//! in a separate tree from Elements and RenderObjects, following Flutter's architecture.
//!
//! # Re-exports from flui-layer
//!
//! All core layer tree types are now provided by the `flui-layer` crate.
//! This module re-exports them for backwards compatibility.

// Re-export all tree types from flui-layer
pub use flui_layer::{ConcreteLayerNode, LayerId, LayerNode, LayerTree};
