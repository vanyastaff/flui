//! FLUI Core - Reactive UI framework for Rust
//!
//! FLUI is a declarative UI framework inspired by Flutter, built for Rust.
//! It provides a powerful widget system with efficient reactivity and
//! high-performance rendering.
//!
//! # Architecture
//!
//! FLUI uses a three-tree architecture:
//!
//! ```text
//! Widget Tree          Element Tree         Render Tree
//! (immutable)          (mutable state)      (layout/paint)
//!     ↓                      ↓                    ↓
//! Configuration  ←→  State Management  ←→  Visual Output
//! ```
//!
//! ## Widget Tree (Immutable)
//!
//! Widgets are lightweight, immutable configuration objects that describe
//! what the UI should look like. When configuration changes, you create
//! new widget instances.
//!
//! ```rust
//! use flui_core::{StatelessWidget, Widget, BuildContext};
//!
//! #[derive(Debug)]
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessWidget for Greeting {
//!     fn build(&self, context: &BuildContext) -> Widget {
//!         Box::new(Text::new(format!("Hello, {}!", self.name)))
//!     }
//! }
//! ```
//!
//! ## Element Tree (Mutable State)
//!
//! Elements hold the mutable state and lifecycle of widgets. They persist
//! across rebuilds and manage the widget-to-render-object relationship.
//!
//! ## Render Tree (Layout & Paint)
//!
//! Renders perform layout calculations and painting. They form the
//! actual visual representation that gets displayed.
//!
//! # Widget Types
//!
//! FLUI provides five core widget types:
//!
//! ## StatelessWidget
//!
//! Pure functional widgets without mutable state.
//!
//! ```rust
//! # use flui_core::{StatelessWidget, Widget, BuildContext};
//! #[derive(Debug)]
//! struct HelloWorld;
//!
//! impl StatelessWidget for HelloWorld {
//!     fn build(&self, context: &BuildContext) -> Widget {
//!         Box::new(Text::new("Hello, World!"))
//!     }
//! }
//! ```
//!
//! ## StatefulWidget
//!
//! Widgets with persistent mutable state. FLUI offers two approaches for managing
//! state in StatefulWidgets:
//!
//! ### Approach 1: Using `ctx.set_state()` (Flutter-style)
//!
//! For simple widgets and familiar Flutter-like API:
//!
//! ```rust,ignore
//! # use flui_core::{StatefulWidget, State, Widget, BuildContext};
//! struct CounterState {
//!     count: i32,
//! }
//!
//! impl State for CounterState {
//!     fn build(&mut self, ctx: &BuildContext) -> Widget {
//!         column![
//!             text(format!("Count: {}", self.count)),
//!             button("+").on_press({
//!                 let ctx = ctx.clone();  // Cheap Arc clone
//!                 move |_| {
//!                     ctx.set_state(|state: &mut CounterState| {
//!                         state.count += 1;
//!                     });
//!                 }
//!             })
//!         ]
//!     }
//! }
//! ```
//!
//! ### Approach 2: Using `Signal<T>` (Fine-grained reactivity)
//!
//! For high-performance, fine-grained updates:
//!
//! ```rust,ignore
//! # use flui_core::{StatefulWidget, State, Widget, BuildContext, Signal};
//! struct CounterState {
//!     count: Signal<i32>,  // Signal is Copy (8 bytes)
//! }
//!
//! impl State for CounterState {
//!     fn build(&mut self, ctx: &BuildContext) -> Widget {
//!         column![
//!             // Only this text rebuilds when count changes
//!             text(format!("Count: {}", self.count.get())),
//!             button("+").on_press({
//!                 let count = self.count;  // Copy, not clone!
//!                 move |_| count.increment()
//!             })
//!         ]
//!     }
//! }
//! ```
//!
//! **When to use each:**
//!
//! - **`ctx.set_state()`**: Simple widgets, few state changes, Flutter familiarity
//! - **`Signal<T>`**: High-frequency updates, animations, maximum performance
//!
//! ## InheritedWidget
//!
//! Efficient data propagation down the widget tree.
//!
//! ```rust
//! # use flui_core::{InheritedWidget, Widget};
//! # use std::sync::Arc;
//! #[derive(Debug)]
//! struct Theme {
//!     colors: Arc<ColorScheme>,
//!     child: Widget,
//! }
//!
//! impl InheritedWidget for Theme {
//!     fn update_should_notify(&self, old: &Self) -> bool {
//!         !Arc::ptr_eq(&self.colors, &old.colors)
//!     }
//!
//!     fn child(&self) -> Widget {
//!         self.child.clone()
//!     }
//! }
//! ```
//!
//! ## RenderWidget
//!
//! Widgets that create render objects for custom layout/paint.
//!
//! ```rust
//! # use flui_core::{RenderWidget, Render};
//! #[derive(Debug)]
//! struct Container {
//!     width: f64,
//!     height: f64,
//! }
//!
//! impl RenderWidget for Container {
//!     type Render = RenderContainer;
//!
//!     fn create_render_object(&self) -> Self::Render {
//!         RenderContainer {
//!             width: self.width,
//!             height: self.height,
//!         }
//!     }
//!
//!     fn update_render_object(&self, render_object: &mut Self::Render) {
//!         render_object.width = self.width;
//!         render_object.height = self.height;
//!     }
//! }
//! ```
//!
//! ## ParentDataWidget
//!
//! Widgets that attach layout metadata to descendants.
//!
//! ```rust
//! # use flui_core::{ParentDataWidget, Widget, Render};
//! #[derive(Debug)]
//! struct Flexible {
//!     flex: i32,
//!     child: Widget,
//! }
//!
//! impl ParentDataWidget for Flexible {
//!     type ParentDataType = FlexParentData;
//!
//!     fn apply_parent_data(&self, render_object: &mut dyn Render) {
//!         // Apply flex data to child's render object
//!     }
//!
//!     fn child(&self) -> &Widget {
//!         &self.child
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
//! ## Automatic DynWidget
//!
//! All widgets automatically get object-safe `DynWidget` trait via blanket impl:
//!
//! ```rust
//! # use flui_core::{StatelessWidget, Widget, DynWidget, BuildContext};
//! #[derive(Debug)]
//! struct MyWidget;
//!
//! impl StatelessWidget for MyWidget {
//!     fn build(&self, context: &BuildContext) -> Widget {
//!         Box::new(Text::new("Test"))
//!     }
//! }
//!
//! // DynWidget is automatic!
//! let widget: Widget = Box::new(MyWidget);
//! ```
//!
//! ## No Forced Clone
//!
//! Widgets don't require Clone, enabling use of closures and non-Clone types:
//!
//! ```rust
//! # use flui_core::{Widget, Widget};
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
//! Widget keys can be compile-time constants:
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
pub use flui_engine::BoxedLayer;
pub use flui_types::{Offset, Size};

// Re-export reactive types
pub use flui_reactive::{create_scope, with_scope, Signal, SignalId, ScopeId, ReactiveScope};
// ============================================================================
// Debug Infrastructure
// ============================================================================

/// Debug flags, diagnostics, and validation
pub mod debug;
pub mod element;
pub mod foundation;
pub mod render;

// New modules (Phase 1: Week 1)
pub mod view;
pub mod pipeline;
pub mod hooks;
pub mod context;
pub mod testing;

// Re-export debug types
pub use debug::DebugFlags;

// ============================================================================
// Error Types
// ============================================================================

// Re-export error types from foundation (moved in Phase 1)
pub use foundation::error::{CoreError, Result};

// ============================================================================
// Foundation
// ============================================================================

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


// ============================================================================
// Element System
// ============================================================================

// Re-export element types
pub use element::{
    // Element types
    ComponentElement,
    // Dependency tracking
    DependencyInfo,
    DependencyTracker,

    // Core enum
    Element,
    ElementId,

    InheritedElement,

    RenderElement,
};

// Re-export view types (moved in Phase 1)
pub use view::BuildContext;

// Re-export pipeline types (moved in Phase 1)
pub use pipeline::{ElementTree, PipelineBuilder, PipelineOwner};

// ============================================================================
// Render System
// ============================================================================

// Re-export render types
pub use render::{LeafRender, MultiRender, RenderNode, RenderState, SingleRender};

// ============================================================================
// Macros
// ============================================================================

// TODO(Phase 2): Add macros for common widget patterns

// ============================================================================
// Prelude
// ============================================================================

/// Prelude module for convenient imports
///
/// Import everything you need with:
///
/// ```rust
/// use flui_core::prelude::*;
/// ```
pub mod prelude {
    pub use crate::foundation::{Key, KeyRef};

    // Element and View system
    pub use crate::view::{BuildContext, View, ViewElement, ViewSequence, AnyView, ChangeFlags};
    pub use crate::element::Element;

    // Render system
    pub use crate::render::{LeafRender, MultiRender, RenderNode, SingleRender};

    // Reactive primitives
    pub use flui_reactive::{Signal, create_scope};
}

// ============================================================================
// Version Information
// ============================================================================

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
        assert!(!VERSION.is_empty());
        assert!(!VERSION_MAJOR.is_empty());
        assert!(!VERSION_MINOR.is_empty());
        assert!(!VERSION_PATCH.is_empty());
    }

    #[test]
    fn test_prelude_imports() {
        use crate::prelude::*;

        // Test that all major types are available
        let _key: Option<Key> = None;
        let _element: Option<Element> = None;
        let _signal: Option<Signal<i32>> = None;
    }
}

