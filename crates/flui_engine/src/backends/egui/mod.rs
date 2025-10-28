//! Egui backend implementation
//!
//! This module provides integration with the egui immediate mode GUI framework.
//! It implements the Painter trait using egui's rendering primitives.

pub mod painter;

// Re-export main types
pub use painter::EguiPainter;
