//! Tree module - LayerTree and related types
//!
//! This module provides the LayerTree data structure for storing compositor layers
//! in a separate tree from Elements and RenderObjects, following Flutter's architecture.

mod layer_tree;

pub use layer_tree::{ConcreteLayerNode, LayerId, LayerNode, LayerTree};
