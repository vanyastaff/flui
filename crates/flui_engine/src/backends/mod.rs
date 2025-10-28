//! Backend implementations for flui_engine
//!
//! This module contains various rendering backend implementations.
//! Each backend provides:
//! - **Painter**: Implementation of the Painter trait for actual rendering
//! - **Integration**: Window/platform integration utilities
//! - **Utils**: Backend-specific utilities and helpers
//!
//! # Available Backends
//!
//! - **egui** (feature = "egui"): Immediate mode GUI framework integration
//! - **wgpu** (feature = "wgpu"): GPU-accelerated rendering with wgpu
//!
//! # Architecture
//!
//! ```text
//! backends/
//!   ├── egui/         # Egui backend
//!   │   ├── painter.rs
//!   │   └── ...
//!   └── wgpu/         # WGPU backend
//!       ├── mod.rs (painter)
//!       ├── pipeline.rs
//!       ├── text.rs
//!       └── ...
//! ```

#[cfg(feature = "egui")]
pub mod egui;

#[cfg(feature = "wgpu")]
pub mod wgpu;

// Re-export commonly used backend types
#[cfg(feature = "egui")]
pub use egui::EguiPainter;

#[cfg(feature = "wgpu")]
pub use wgpu::{WgpuPainter, WgpuRenderer, TextRenderer, TextCommand, TextAlign, TextRenderError};
