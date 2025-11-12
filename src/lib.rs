//! # FLUI - Flutter-inspired UI Framework for Rust
//!
//! FLUI is a production-ready, declarative UI framework built with **wgpu** for GPU-accelerated rendering,
//! featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms.
//!
//! ## Feature Flags
//!
//! ### Performance
//! - **`parallel`** - ✅ Enable parallel processing with rayon (stable, thread-safe)
//! - **`profiling`** - Enable puffin profiling
//! - **`tracy`** - Enable Tracy profiler integration
//! - **`full-profiling`** - Enable both puffin and tracy
//!
//! ### Asset Management (flui-assets)
//! - **`images`** - Enable image loading (PNG, JPEG, GIF, WebP, etc.)
//! - **`bundles`** - Asset bundling and manifest support
//! - **`network`** - Network-based asset loading via HTTP
//! - **`hot-reload`** - File watching for development
//! - **`mmap-fonts`** - Memory-mapped font loading (performance optimization)
//! - **`parallel-decode`** - Parallel image/video decoding with rayon
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
//! struct MyApp;
//!
//! impl View for MyApp {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         Text::new("Hello, FLUI!")
//!     }
//! }
//!
//! # fn main() {
//! run_app(MyApp);
//! # }
//! ```
//!
//! ## GPU-Accelerated Rendering
//!
//! FLUI uses **wgpu** as its rendering backend, providing:
//! - ✅ Cross-platform GPU acceleration (Vulkan/Metal/DX12/WebGPU)
//! - ✅ High-performance rendering pipeline
//! - ✅ Modern shader-based graphics
//! - ✅ Native performance on all platforms
//!
//! ## Module Organization
//!
//! - [`types`] - Core types (Size, Offset, Color, etc.)
//! - [`core`] - Core framework (View trait, BuildContext, Element tree, hooks)
//! - [`engine`] - Rendering engine (Scene, CanvasLayer, GpuRenderer)
//! - [`rendering`] - Render objects (RenderPadding, RenderFlex, etc.)
//! - [`animation`] - Animation system (AnimationController, Tween, Curves)
//! - [`gestures`] - Gesture recognition (TapGestureRecognizer, DragGestureRecognizer)
//! - [`widgets`] - Built-in widgets (Container, Row, Column, Text, etc.)
//! - [`app`] - Application framework (run_app, AppBinding, WgpuEmbedder)
//! - [`prelude`] - Common imports

// Re-export all crates for modular access
pub use flui_animation as animation;
pub use flui_app as app;
pub use flui_core as core;
pub use flui_engine as engine;
pub use flui_gestures as gestures;
pub use flui_rendering as rendering;
pub use flui_types as types;
pub use flui_widgets as widgets;

/// Prelude for common imports - brings in everything needed for most use cases
///
/// # Example
/// ```rust,no_run
/// use flui::prelude::*;
///
/// struct MyView;
///
/// impl View for MyView {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Text::new("Hello!")
///     }
/// }
/// ```
pub mod prelude {
    // ============================================================
    // CORE TYPES (geometry, layout, styling)
    // ============================================================
    pub use flui_types::prelude::*;

    // ============================================================
    // CORE FRAMEWORK (View, BuildContext, hooks, render traits)
    // ============================================================
    pub use flui_core::prelude::*;

    // ============================================================
    // WIDGETS (Container, Row, Column, Text, etc.)
    // ============================================================
    pub use flui_widgets::prelude::*;

    // ============================================================
    // APP FRAMEWORK (run_app)
    // ============================================================
    pub use flui_app::run_app;

    // ============================================================
    // RENDERING (for custom render objects)
    // ============================================================
    pub use flui_rendering::{
        // Core render traits from flui_core (re-exported by flui_rendering)
        Render,
        Arity,
        LayoutContext,
        PaintContext,

        // Common render objects
        RenderPadding,
        RenderConstrainedBox,
        RenderTransform,
        RenderOpacity,
        RenderClipRRect,
        RenderAlign,
        RenderFlex,

        // Decoration
        RenderDecoratedBox,
        DecorationPosition,
    };

    // ============================================================
    // ANIMATION (controllers and types)
    // ============================================================
    pub use flui_animation::{
        // Animation traits and controllers
        Animation,
        AnimationController,

        // Tween animation
        TweenAnimation,
        CurvedAnimation,
    };

    // Re-export animation types from flui_types
    pub use flui_types::animation::{
        Curve,
        Curves,
        Tween,
        ColorTween,
    };

    // ============================================================
    // GESTURES (for interaction)
    // ============================================================
    pub use flui_gestures::{
        // Gesture recognizers
        TapGestureRecognizer,
        DragGestureRecognizer,
        LongPressGestureRecognizer,
        ScaleGestureRecognizer,

        // Gesture detector widget
        GestureDetector,
    };

    // Re-export gesture types from flui_types
    pub use flui_types::gestures::{
        TapDownDetails,
        TapUpDetails,
        DragStartDetails,
        DragUpdateDetails,
        DragEndDetails,
    };

    // ============================================================
    // ENGINE (Scene, Layer - for advanced rendering)
    // ============================================================
    pub use flui_engine::{
        Scene,
        CanvasLayer,
        GpuRenderer,
    };
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
