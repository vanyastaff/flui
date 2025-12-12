//! # FLUI - Flutter-inspired UI Framework for Rust
//!
//! FLUI is a production-ready, declarative UI framework built with **wgpu** for GPU-accelerated rendering,
//! featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms.
//!
//! ## Note: Minimal Build Mode
//!
//! This crate is currently in minimal build mode for flui_rendering development.
//! Most re-exports are temporarily disabled.

// Re-export core crates only
pub use flui_rendering as rendering;
pub use flui_types as types;

/// Prelude for common imports (minimal during flui_rendering development)
pub mod prelude {
    pub use flui_types::prelude::*;
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
