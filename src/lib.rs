//! Flui - Flutter-inspired declarative UI framework for Rust
//!
//! This is the main library crate that re-exports all Flui functionality.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use flui::prelude::*;
//!
//! // Your UI code here
//! ```
//!
//! # Architecture
//!
//! Flui is organized into several crates:
//! - `flui_types` - Core types (Size, Offset, Color, etc.)
//! - `flui_core` - Element system and reactive primitives
//! - `flui_engine` - Rendering engine (Layer system)
//! - `flui_widgets` - Standard widget library
//! - `flui_app` - High-level application framework

// Re-export all public APIs
pub use flui_types as types;
pub use flui_core as core;
pub use flui_engine as engine;

#[cfg(feature = "flui_widgets")]
pub use flui_widgets as widgets;

#[cfg(feature = "flui_app")]
pub use flui_app as app;

/// Convenient prelude for common imports
pub mod prelude {
    // Core types
    pub use flui_types::{Offset, Size};
    pub use flui_core::prelude::*;

    // Engine types
    pub use flui_engine::BoxedLayer;

    // Widgets (if enabled)
    #[cfg(feature = "flui_widgets")]
    pub use flui_widgets::*;

    // App (if enabled)
    #[cfg(feature = "flui_app")]
    pub use flui_app::*;
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
