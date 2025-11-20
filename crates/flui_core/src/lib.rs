//! FLUI Core - Reactive UI framework for Rust
//!
//! FLUI is a declarative UI framework inspired by Flutter, built for Rust.
//! It provides a powerful View system with efficient reactivity and
//! high-performance rendering.
//!
//! # Architecture
//!
//! FLUI uses a three-tree architecture:
//!
//! ```text
//! View Tree            Element Tree         Render Tree
//! (immutable)          (mutable state)      (layout/paint)
//!     ↓                      ↓                    ↓
//! Configuration  ←→  State Management  ←→  Visual Output
//! ```
//!
//! ## View Tree (Immutable)
//!
//! Views are lightweight, immutable configuration objects that describe
//! what the UI should look like. When configuration changes, you create
//! new view instances.
//!
//! ```rust,ignore
//! use flui_core::{Component, View, BuildContext};
//!
//! #[derive(Debug)]
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl Component for Greeting {
//!     fn build(&self, context: &BuildContext) -> View {
//!         Text::new(format!("Hello, {}!", self.name)).into()
//!     }
//! }
//! ```
//!
//! ## Element Tree (Mutable State)
//!
//! Elements hold the mutable state and lifecycle of views. They persist
//! across rebuilds and manage the view-to-render-object relationship.
//!
//! ## Render Tree (Layout & Paint)
//!
//! Renders perform layout calculations and painting. They form the
//! actual visual representation that gets displayed.
//!
//! # View Types
//!
//! FLUI provides three core view types:
//!
//! ## Component Views
//!
//! Component views are composable, building UIs from other views.
//! They can have optional state managed via hooks or the State type parameter.
//!
//! ```rust,ignore
//! use flui_core::{Component, View, BuildContext};
//! use flui_core::hooks::use_signal;
//!
//! #[derive(Debug)]
//! struct Counter;
//!
//! impl Component for Counter {
//!     fn build(&self, ctx: &BuildContext) -> View {
//!         let count = use_signal(ctx, 0);
//!
//!         column![
//!             text(format!("Count: {}", count.get())),
//!             button("+").on_press(move || count.update(|n| n + 1))
//!         ].into()
//!     }
//! }
//! ```
//!
//! ## Provider Views (formerly InheritedWidget)
//!
//! Provider views enable efficient data propagation down the view tree with automatic
//! dependency tracking.
//!
//! ```rust,ignore
//! use flui_core::{Provider, View};
//!
//! #[derive(Debug, Clone)]
//! struct Theme {
//!     primary_color: Color,
//! }
//!
//! impl Provider for Theme {
//!     fn should_notify(&self, old: &Self) -> bool {
//!         self.primary_color != old.primary_color
//!     }
//! }
//! ```
//!
//! ## Render Views
//!
//! Render views create custom render objects for layout and painting.
//!
//! ```rust,ignore
//! use flui_core::render::{Render, Arity, LayoutContext, PaintContext};
//! use flui_types::Size;
//!
//! #[derive(Debug)]
//! struct CustomBox {
//!     width: f64,
//!     height: f64,
//! }
//!
//! impl Render for CustomBox {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size {
//!         Size::new(self.width, self.height)
//!     }
//!
//!     fn paint(&self, ctx: &PaintContext) -> Box<flui_engine::PictureLayer> {
//!         // Custom painting code
//!         Box::new(flui_engine::PictureLayer::new())
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Exact(0)  // Leaf render object
//!     }
//! }
//! ```
//!
//! # Key Features
//!
//! ## Reactive State Management
//!
//! FLUI provides two powerful approaches for reactive state management:
//!
//! ### Fine-Grained Reactivity with `Signal<T>`
//!
//! Signals are Copy-able reactive primitives inspired by Leptos and SolidJS:
//!
//! ```rust,ignore
//! use flui_core::Signal;
//!
//! // Create a signal - just 8 bytes, Copy-able!
//! let count = Signal::new(0);
//!
//! // Signal is Copy, no cloning needed
//! let count_copy = count;
//!
//! // Read and update
//! println!("Count: {}", count.get());
//! count.set(10);
//! count.update(|v| *v += 1);
//! count.increment();  // Convenience method
//!
//! // Subscribe to changes
//! count.subscribe(Arc::new(|| {
//!     println!("Count changed!");
//! }));
//!
//! // Automatic dependency tracking
//! let (_, result, deps) = create_scope(|| {
//!     count.get() * 2  // Automatically tracked
//! });
//! ```
//!
//! **Benefits:**
//! - ✅ Copy semantics (8 bytes)
//! - ✅ Fine-grained updates (only affected parts rebuild)
//! - ✅ Automatic dependency tracking
//! - ✅ Zero allocations for signal handles
//! - ✅ Thread-local arena storage
//!
//! ### Flutter-Style `ctx.set_state()`
//!
//! Familiar API for Flutter developers:
//!
//! ```rust,ignore
//! button("+").on_press({
//!     let ctx = ctx.clone();
//!     move |_| {
//!         ctx.set_state(|state: &mut CounterState| {
//!             state.count += 1;
//!         });
//!     }
//! })
//! ```
//!
//! **Benefits:**
//! - ✅ Familiar Flutter API
//! - ✅ Simple mental model
//! - ✅ Direct state access
//! - ✅ Automatic rebuilds
//!
//! ## No Forced Clone
//!
//! Views don't require Clone, enabling use of closures and non-Clone types:
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct Button<F> {
//!     label: String,
//!     on_click: F,  // FnMut - not Clone!
//! }
//!
//! // Works without Clone!
//! ```
//!
//! ## Compile-Time Keys
//!
//! View keys can be compile-time constants:
//!
//! ```rust
//! use flui_core::Key;
//!
//! const HEADER_KEY: Key = Key::from_str("app_header");
//! const FOOTER_KEY: Key = Key::from_str("app_footer");
//! ```
//!
//! ## Memory Optimization
//!
//! `Option<Key>` is only 8 bytes thanks to niche optimization:
//!
//! ```rust
//! use flui_core::Key;
//! use std::mem::size_of;
//!
//! assert_eq!(size_of::<Option<Key>>(), 8);  // Not 16!
//! ```

#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
#![deny(unsafe_op_in_unsafe_fn)]

// Re-export external types
// BoxedLayer removed - use Box<flui_engine::PictureLayer> directly
pub use flui_types::{Offset, Size};

/// Debug flags, diagnostics, and validation
pub mod context;
pub mod debug;
pub mod element;
pub mod foundation;
pub mod hooks;
pub mod macros;
pub mod pipeline;
pub mod prelude;
pub mod render;
pub mod testing;
pub mod view;

// Re-export debug types
pub use debug::DebugFlags;

// Re-export error types from foundation
pub use foundation::error::{CoreError, Result};

// Re-export logging
pub use flui_log;

// Re-export foundation types
pub use foundation::{
    ChangeNotifier,
    // Diagnostics
    DiagnosticLevel,
    Diagnosticable,
    DiagnosticsBuilder,
    DiagnosticsNode,
    DiagnosticsProperty,
    DiagnosticsTreeStyle,
    DynNotification,
    ElementId,
    FocusChangedNotification,
    KeepAliveNotification,
    Key,
    KeyRef,
    LayoutChangedNotification,
    // Change notification
    Listenable,
    ListenerCallback,
    ListenerId,
    MergedListenable,
    // Notifications (bubbling events)
    Notification,
    ScrollNotification,
    SizeChangedNotification,
    Slot,
    ValueNotifier,
};

// Re-export element types
pub use element::{
    ComponentElement, DependencyInfo, DependencyTracker, Element, ElementTree, ProviderElement,
    RenderElement,
};

// Re-export view types
pub use view::BuildContext;

// Re-export simplified API (View, IntoElement, tuple syntax)
pub use view::{IntoElement, View};

// Re-export pipeline types
pub use pipeline::{PipelineBuilder, PipelineOwner};

// Re-export render types
pub use render::{Arity, RenderBox, RenderState};

/// Prelude module for convenient imports
///
/// Import everything you need with:
///
/// ```rust
/// use flui_core::prelude::*;
/// ```
// Prelude module is now in separate file (src/prelude.rs)
// See prelude module documentation for details
/// FLUI version string
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// FLUI major version
pub const VERSION_MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");

/// FLUI minor version
pub const VERSION_MINOR: &str = env!("CARGO_PKG_VERSION_MINOR");

/// FLUI patch version
pub const VERSION_PATCH: &str = env!("CARGO_PKG_VERSION_PATCH");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        // Check version strings are defined (use len() for const strings)
        assert_ne!(VERSION.len(), 0);
        assert_ne!(VERSION_MAJOR.len(), 0);
        assert_ne!(VERSION_MINOR.len(), 0);
        assert_ne!(VERSION_PATCH.len(), 0);
    }

    #[test]
    fn test_prelude_imports() {
        use crate::prelude::*;

        // Test that all major types are available
        let _key: Option<Key> = None;
        let _element: Option<Element> = None;

        // Test hooks are available
        let _signal: Option<Signal<i32>> = None; // Signal from prelude

        // Test render types are available
        let _arity = RuntimeArity::Variable;
    }
}
