//! # FLUI - Flutter-inspired UI Framework for Rust
//!
//! FLUI is a production-ready, declarative UI framework built with **wgpu** for GPU-accelerated
//! rendering, featuring the proven three-tree architecture (View → Element → Render) with modern
//! Rust idioms.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use flui::prelude::*;
//!
//! // Create a simple view
//! fn my_app() -> impl View {
//!     Center::new(
//!         Text::new("Hello, FLUI!")
//!     )
//! }
//! ```
//!
//! ## Architecture
//!
//! FLUI implements Flutter's proven three-tree architecture:
//!
//! - **View Tree**: Immutable configuration describing UI (like Flutter's Widget)
//! - **Element Tree**: Mutable state management and tree reconciliation
//! - **Render Tree**: Layout computation and painting
//!
//! ## Module Organization
//!
//! FLUI is organized into layered crates:
//!
//! ### Foundation Layer
//! - [`types`] - Core geometry, layout, styling, and typography types
//! - [`foundation`] - IDs, keys, callbacks, change notification
//! - [`tree`] - Tree abstractions with type-safe arity system
//!
//! ### Core Layer
//! - [`layer`] - Compositor layer tree
//! - [`semantics`] - Accessibility tree
//! - [`interaction`] - Event routing, hit testing, gestures
//! - [`painting`] - Canvas drawing commands
//!
//! ### Rendering Layer
//! - [`rendering`] - Render object system (RenderBox, RenderSliver)
//!
//! ### View Layer
//! - [`view`] - View, Element, and BuildOwner
//!
//! ### Reactive Layer
//! - [`scheduler`] - Frame scheduling and ticker
//! - [`animation`] - Curves, tweens, animation controllers
//! - [`reactivity`] - Signals, hooks, and effects
//!
//! ### Application Layer
//! - [`app`] - Application runner and bindings
//!
//! ## Feature Flags
//!
//! - `serde` - Serialization support for types

#![warn(missing_docs)]

// ============================================================================
// CRATE RE-EXPORTS
// ============================================================================

/// Foundation types: IDs, keys, callbacks, change notification.
pub use flui_foundation as foundation;

/// Tree abstractions with type-safe arity system.
pub use flui_tree as tree;

/// Core geometry, layout, styling, and typography types.
pub use flui_types as types;

/// Compositor layer tree for rendering.
pub use flui_layer as layer;

/// Accessibility tree for screen readers.
pub use flui_semantics as semantics;

/// Event routing, hit testing, and gesture recognition.
pub use flui_interaction as interaction;

/// Canvas drawing commands and painting primitives.
pub use flui_painting as painting;

/// Render object system: RenderBox, RenderSliver, layout, paint.
pub use flui_rendering as rendering;

/// View, Element, and BuildOwner for the widget system.
pub use flui_view as view;

/// Frame scheduling and ticker management.
pub use flui_scheduler as scheduler;

/// Animation system: curves, tweens, controllers.
pub use flui_animation as animation;

/// Reactive state management: signals, hooks, effects.
pub use flui_reactivity as reactivity;

/// Application runner and bindings.
pub use flui_app as app;

// ============================================================================
// VERSION
// ============================================================================

/// FLUI version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ============================================================================
// PRELUDE
// ============================================================================

/// The FLUI prelude - import everything you need to build UIs.
///
/// This module provides the most commonly used types and traits from all FLUI
/// crates via chained prelude re-exports:
///
/// ```rust,ignore
/// use flui::prelude::*;
/// ```
///
/// Each sub-crate has its own prelude that is re-exported here. For more
/// specific imports, use the crate modules directly (e.g., `flui::types::*`).
#[allow(ambiguous_glob_reexports)]
pub mod prelude {
    // ========================================================================
    // FOUNDATION LAYER - Types, IDs, Keys, Tree abstractions
    // ========================================================================

    /// Core geometry, layout, styling, typography types.
    pub use flui_types::prelude::*;

    /// IDs, keys, callbacks, change notification.
    pub use flui_foundation::prelude::*;

    /// Tree abstractions with type-safe arity system.
    pub use flui_tree::prelude::*;

    // ========================================================================
    // CORE LAYER - Layers, Semantics, Interaction, Painting
    // ========================================================================

    /// Compositor layer tree.
    pub use flui_layer::prelude::*;

    /// Accessibility tree.
    pub use flui_semantics::prelude::*;

    /// Event routing, hit testing, gestures.
    pub use flui_interaction::prelude::*;

    /// Canvas drawing commands.
    pub use flui_painting::prelude::*;

    // ========================================================================
    // RENDERING LAYER - RenderObject system
    // ========================================================================

    /// Render objects, layout, painting, constraints.
    pub use flui_rendering::prelude::*;

    // ========================================================================
    // VIEW LAYER - View, Element, BuildOwner
    // ========================================================================

    /// View, Element, BuildContext, widget identity.
    pub use flui_view::prelude::*;

    // ========================================================================
    // REACTIVE LAYER - Scheduler, Animation, Reactivity
    // ========================================================================

    /// Frame scheduling and tickers.
    pub use flui_scheduler::prelude::*;

    /// Animation system.
    pub use flui_animation::prelude::*;

    /// Signals, hooks, effects.
    pub use flui_reactivity::prelude::*;

    // ========================================================================
    // APPLICATION LAYER - App runner and bindings
    // ========================================================================

    /// Application runner and bindings.
    pub use flui_app::prelude::*;
}
