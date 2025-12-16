//! # FLUI View
//!
//! Flutter-inspired View and Element system for FLUI.
//!
//! This crate implements the View layer of FLUI's three-tree architecture:
//!
//! ```text
//! View (immutable config) → Element (mutable lifecycle) → RenderObject (layout/paint)
//! ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
//! This crate!
//! ```
//!
//! ## Architecture
//!
//! ### Views (Immutable)
//!
//! Views are declarative UI descriptions. They are:
//! - **Short-lived**: Created each build cycle, used for diffing, then dropped
//! - **Immutable**: Never mutated after creation
//! - **Composable**: Build trees of nested Views
//!
//! ### Elements (Mutable)
//!
//! Elements are the retained tree nodes that manage View lifecycle:
//! - **Long-lived**: Persist across builds
//! - **Mutable**: Hold state and manage children
//! - **Lifecycle**: Handle mount, build, update, unmount
//!
//! ## View Types
//!
//! - [`StatelessView`] - Views without internal state
//! - [`StatefulView`] - Views with persistent mutable state
//! - [`InheritedView`] - Views that provide data to descendants
//! - [`RenderView`] - Views that create RenderObjects
//! - [`ProxyView`] - Single-child wrapper Views
//! - [`ParentDataView`] - Views that configure parent data on RenderObjects
//!
//! ## Example
//!
//! ```rust,ignore
//! use flui_view::prelude::*;
//!
//! struct Counter {
//!     initial: i32,
//! }
//!
//! struct CounterState {
//!     count: i32,
//! }
//!
//! impl StatefulView for Counter {
//!     type State = CounterState;
//!
//!     fn create_state(&self) -> Self::State {
//!         CounterState { count: self.initial }
//!     }
//! }
//!
//! impl ViewState<Counter> for CounterState {
//!     fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> Box<dyn View> {
//!         Text::new(format!("Count: {}", self.count)).boxed()
//!     }
//! }
//! ```

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    clippy::all,
    clippy::pedantic
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::doc_markdown,
    clippy::module_inception,
    clippy::missing_fields_in_debug,
    clippy::bool_to_int_with_if
)]

// ============================================================================
// Modules
// ============================================================================

pub mod child;
pub mod context;
pub mod element;
pub mod key;
pub mod owner;
pub mod tree;
pub mod view;

// ============================================================================
// Re-exports
// ============================================================================

// View traits
pub use view::{
    clear_error_view_builder, set_error_view_builder, BoxedView, ElementBase, ErrorElement,
    ErrorView, ErrorViewBuilder, FlutterError, InheritedElement, InheritedView, IntoView,
    ParentData, ParentDataElement, ParentDataView, ProxyElement, ProxyView, RenderElement,
    RenderView, StatefulElement, StatefulView, StatelessElement, StatelessView, View, ViewExt,
    ViewKey, ViewState,
};

// Keys
pub use key::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey};

// Child helpers
pub use child::{Child, Children};

// Element types
pub use element::Lifecycle;

// Notification system
pub use element::{
    BoxedNotification, DragEndNotification, DragStartNotification, FocusNotification,
    KeepAliveNotification, LayoutChangedNotification, NotifiableElement, Notification,
    NotificationCallback, NotificationHandler, NotificationNode, ScrollNotification,
    SizeChangedNotification,
};

// Root element
pub use element::{RootElement, RootElementImpl};

// Slot types for multi-child elements
pub use element::{ElementSlot, IndexedSlot, IndexedSlotBuilder};

// Context
pub use context::{BuildContext, BuildContextExt};

// Tree management
pub use owner::BuildOwner;
pub use tree::{reconcile_children, ElementNode, ElementTree};

// Re-export from flui-foundation
pub use flui_foundation::{ElementId, RenderId};

// ============================================================================
// Prelude
// ============================================================================

/// Commonly used types for convenient importing.
///
/// ```rust,ignore
/// use flui_view::prelude::*;
/// ```
pub mod prelude {
    pub use crate::child::{Child, Children};
    pub use crate::context::{BuildContext, BuildContextExt};
    pub use crate::element::{
        ElementSlot, IndexedSlot, IndexedSlotBuilder, LayoutChangedNotification, Lifecycle,
        NotifiableElement, Notification, NotificationNode, RootElement,
    };
    pub use crate::key::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey};
    pub use crate::owner::BuildOwner;
    pub use crate::tree::{reconcile_children, ElementNode, ElementTree};
    pub use crate::view::{
        BoxedView, InheritedView, IntoView, ParentData, ParentDataView, ProxyView, RenderView,
        StatefulView, StatelessView, View, ViewExt, ViewState,
    };
    pub use flui_foundation::{ElementId, RenderId};
}
