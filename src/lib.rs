//! # FLUI - Flutter-inspired UI Framework for Rust
//!
//! FLUI is a production-ready, declarative UI framework built on egui 0.33,
//! featuring the proven three-tree architecture (View → Element → Render)
//! with modern Rust idioms.
//!
//! ## Feature Flags
//!
//! ### Backends (mutually exclusive)
//! - **`egui`** (default) - Use egui backend for immediate mode rendering
//! - **`wgpu`** - Use wgpu backend for retained mode GPU rendering
//!
//! ### Performance
//! - **`parallel`** - Enable parallel processing with rayon (⚠️ currently has bugs)
//! - **`profiling`** - Enable puffin profiling
//! - **`tracy`** - Enable Tracy profiler integration
//! - **`full-profiling`** - Enable both puffin and tracy
//!
//! ### Optional Features
//! - **`persistence`** (default) - Enable state persistence
//! - **`serde`** - Enable serialization support for core types
//! - **`devtools`** - Enable developer tools integration
//! - **`memory-profiler`** - Enable memory profiling (requires devtools)
//!
//! ## Quick Start
//!
//! Add to your `Cargo.toml`:
//! ```toml
//! [dependencies]
//! flui = "0.1"
//! ```
//!
//! Basic example:
//! ```rust,no_run
//! # use flui::prelude::*;
//! #
//! #[derive(Clone)]
//! struct MyApp;
//!
//! impl View for MyApp {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         Text::new("Hello, FLUI!")
//!     }
//! }
//!
//! # fn main() {
//! run_app("My App", MyApp);
//! # }
//! ```
//!
//! ## Using Different Backends
//!
//! ### egui backend (default):
//! ```toml
//! flui = "0.1"  # egui is default
//! ```
//!
//! ### wgpu backend:
//! ```toml
//! flui = { version = "0.1", default-features = false, features = ["wgpu"] }
//! ```
//!
//! ## Module Organization
//!
//! - [`types`] - Core types (Size, Offset, Color, etc.)
//! - [`core`] - Core framework (View trait, BuildContext, Element tree, etc.)
//! - [`engine`] - Rendering engine (Layer, Scene, Painter)
//! - [`rendering`] - Render objects (RenderPadding, RenderFlex, etc.)
//! - [`widgets`] - Built-in widgets (Container, Row, Column, Text, etc.)
//! - [`app`] - Application framework (run_app, FluiApp)
//! - [`prelude`] - Common imports

// Re-export all crates for modular access
pub use flui_app as app;
pub use flui_core as core;
pub use flui_engine as engine;
pub use flui_rendering as rendering;
pub use flui_types as types;
pub use flui_widgets as widgets;

/// Prelude for common imports - brings in everything needed for most use cases
///
/// # Example
/// ```rust,no_run
/// use flui::prelude::*;
///
/// #[derive(Clone)]
/// struct MyView;
///
/// impl View for MyView {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Text::new("Hello!")
///     }
/// }
/// ```
pub mod prelude {
    // Core types
    pub use flui_types::layout::{Alignment, CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
    pub use flui_types::typography::TextAlign;
    pub use flui_types::{Axis, BoxConstraints, Color, EdgeInsets, Offset, Size};

    // Core traits and context
    pub use flui_core::prelude::*;

    // Engine types
    // BoxedLayer removed - use Box<flui_engine::PictureLayer> directly

    // Widgets - always available
    pub use flui_widgets::prelude::*;

    // App framework - always available
    pub use flui_app::{run_app, FluiApp};
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
