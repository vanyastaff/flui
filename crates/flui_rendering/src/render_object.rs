//! RenderObject trait - the rendering layer
//!
//! RenderObjects perform layout and painting. This is the third tree in
//! Flutter's three-tree architecture: Widget → Element → RenderObject
//!
//! The RenderObject trait has been moved to flui_core. This module re-exports it for convenience.

// Re-export RenderObject from flui_core
pub use flui_core::RenderObject;
