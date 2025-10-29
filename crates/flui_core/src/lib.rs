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
//! use flui_core::{StatelessWidget, BoxedWidget, BuildContext};
//!
//! #[derive(Debug)]
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessWidget for Greeting {
//!     fn build(&self, context: &BuildContext) -> BoxedWidget {
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
//! RenderObjects perform layout calculations and painting. They form the
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
//! # use flui_core::{StatelessWidget, BoxedWidget, BuildContext};
//! #[derive(Debug)]
//! struct HelloWorld;
//!
//! impl StatelessWidget for HelloWorld {
//!     fn build(&self, context: &BuildContext) -> BoxedWidget {
//!         Box::new(Text::new("Hello, World!"))
//!     }
//! }
//! ```
//!
//! ## StatefulWidget
//!
//! Widgets with persistent mutable state.
//!
//! ```rust
//! # use flui_core::{StatefulWidget, State, BoxedWidget, BuildContext};
//! #[derive(Debug)]
//! struct Counter {
//!     initial: i32,
//! }
//!
//! struct CounterState {
//!     count: i32,
//! }
//!
//! impl StatefulWidget for Counter {
//!     type State = CounterState;
//!
//!     fn create_state(&self) -> Self::State {
//!         CounterState { count: self.initial }
//!     }
//! }
//!
//! impl State<Counter> for CounterState {
//!     fn build(&mut self, widget: &Counter) -> BoxedWidget {
//!         Box::new(Text::new(format!("Count: {}", self.count)))
//!     }
//! }
//! ```
//!
//! ## InheritedWidget
//!
//! Efficient data propagation down the widget tree.
//!
//! ```rust
//! # use flui_core::{InheritedWidget, BoxedWidget};
//! # use std::sync::Arc;
//! #[derive(Debug)]
//! struct Theme {
//!     colors: Arc<ColorScheme>,
//!     child: BoxedWidget,
//! }
//!
//! impl InheritedWidget for Theme {
//!     fn update_should_notify(&self, old: &Self) -> bool {
//!         !Arc::ptr_eq(&self.colors, &old.colors)
//!     }
//!
//!     fn child(&self) -> BoxedWidget {
//!         self.child.clone()
//!     }
//! }
//! ```
//!
//! ## RenderObjectWidget
//!
//! Widgets that create render objects for custom layout/paint.
//!
//! ```rust
//! # use flui_core::{RenderObjectWidget, RenderObject};
//! #[derive(Debug)]
//! struct Container {
//!     width: f64,
//!     height: f64,
//! }
//!
//! impl RenderObjectWidget for Container {
//!     type RenderObject = RenderContainer;
//!
//!     fn create_render_object(&self) -> Self::RenderObject {
//!         RenderContainer {
//!             width: self.width,
//!             height: self.height,
//!         }
//!     }
//!
//!     fn update_render_object(&self, render_object: &mut Self::RenderObject) {
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
//! # use flui_core::{ParentDataWidget, BoxedWidget, RenderObject};
//! #[derive(Debug)]
//! struct Flexible {
//!     flex: i32,
//!     child: BoxedWidget,
//! }
//!
//! impl ParentDataWidget for Flexible {
//!     type ParentDataType = FlexParentData;
//!
//!     fn apply_parent_data(&self, render_object: &mut dyn RenderObject) {
//!         // Apply flex data to child's render object
//!     }
//!
//!     fn child(&self) -> &BoxedWidget {
//!         &self.child
//!     }
//! }
//! ```
//!
//! # Key Features
//!
//! ## Automatic DynWidget
//!
//! All widgets automatically get object-safe `DynWidget` trait via blanket impl:
//!
//! ```rust
//! # use flui_core::{StatelessWidget, BoxedWidget, DynWidget, BuildContext};
//! #[derive(Debug)]
//! struct MyWidget;
//!
//! impl StatelessWidget for MyWidget {
//!     fn build(&self, context: &BuildContext) -> BoxedWidget {
//!         Box::new(Text::new("Test"))
//!     }
//! }
//!
//! // DynWidget is automatic!
//! let widget: Box<dyn DynWidget> = Box::new(MyWidget);
//! ```
//!
//! ## No Forced Clone
//!
//! Widgets don't require Clone, enabling use of closures and non-Clone types:
//!
//! ```rust
//! # use flui_core::{Widget, BoxedWidget};
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
// ============================================================================
// Debug Infrastructure
// ============================================================================

/// Debug flags, diagnostics, and validation
pub mod debug;
pub mod element;
pub mod error;
pub mod foundation;
pub mod render;
pub mod widget;

// Re-export debug types
pub use debug::DebugFlags;

// ============================================================================
// Error Types
// ============================================================================

// Re-export error types
pub use error::{CoreError, Result};

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
// Widget System
// ============================================================================

// Re-export widget types
pub use widget::{
    // Type aliases
    BoxedWidget,
    DynWidget,

    InheritedModel,
    InheritedWidget,
    // Helper types
    KeyedStatelessWidget,
    MultiChildRenderObjectWidget,
    NotificationListener,

    ParentData,

    ParentDataWidget,
    RenderObjectWidget,
    SharedWidget,

    SingleChildRenderObjectWidget,
    State,
    StatefulWidget,
    // Widget types
    StatelessWidget,
    // Core traits
    Widget,
    WidgetState,
    boxed,
    shared,
    // Helper functions
    with_key,
};

// ============================================================================
// Element System
// ============================================================================

// Re-export element types
pub use element::{
    // Type aliases
    BoxedElement,
    // Context types
    BuildContext,
    // Element types
    ComponentElement,
    // Dependency tracking
    DependencyInfo,
    DependencyTracker,
    DynElement,

    // Core traits
    Element,
    ElementId,

    ElementTree,

    InheritedElement,
    ParentDataElement,

    PipelineOwner,
    RenderElement,
    StatefulElement,
};

// ============================================================================
// Render System
// ============================================================================

// Re-export render types
pub use render::{
    // Arity types
    Arity,
    BoxedRenderObject,
    DynRenderObject,
    LayoutCx,
    LeafArity,
    MultiArity,
    PaintCx,

    RenderObject,
    RenderState,
    SingleArity,
};

// ============================================================================
// Macros
// ============================================================================

// impl_parent_data macro is defined in widget::parent_data_widget module and re-exported

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

    pub use crate::widget::{
        BoxedWidget, DynWidget, InheritedWidget, ParentDataWidget, RenderObjectWidget,
        SharedWidget, State, StatefulWidget, StatelessWidget, Widget, boxed, shared, with_key,
    };

    pub use crate::element::{BuildContext, Element};

    pub use crate::render::{LeafArity, MultiArity, RenderObject, SingleArity};
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
        let _widget: Option<BoxedWidget> = None;
    }
}
