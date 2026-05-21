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

pub mod binding;
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
// Binding
pub use binding::{
    AppExitResponse, AppLifecycleState, PredictiveBackEvent, RouteInformation, ViewFocusDirection,
    ViewFocusEvent, ViewFocusState, WidgetsBinding, WidgetsBindingObserver,
};
// Child helpers
pub use child::{Child, Children};
// Context
pub use context::{BuildContext, BuildContextExt, ElementBuildContext, ElementBuildContextBuilder};
// Element types
pub use element::Lifecycle;
// Notification system
pub use element::{
    BoxedNotification, DragEndNotification, DragStartNotification, FocusNotification,
    KeepAliveNotification, LayoutChangedNotification, NotifiableElement, Notification,
    NotificationCallback, NotificationHandler, NotificationNode, ScrollNotification,
    SizeChangedNotification,
};
// Slot types for multi-child elements (re-exported from flui-tree, canonical home)
pub use element::{ElementSlot, IndexedSlot};
// RenderObjectElement traits
pub use element::{RenderObjectElement, RenderSlot, RenderTreeRootElement};
// Root element
pub use element::{RootElement, RootElementImpl};
// Behavior types
pub use element::{StatefulBehavior, StatelessBehavior};
// Re-export from flui-foundation
pub use flui_foundation::{ElementId, RenderId};
// Logging re-exports from flui_log
pub use flui_log::{Level, Logger, debug, error, info, trace, warn};
// Keys
pub use key::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey};
// Tree management
pub use owner::{BuildOwner, ElementOwner};
pub use tree::{ElementNode, ElementTree, reconcile_children};
pub use view::{
    BoxedElement, BoxedView, ElementBase, ElementExt, ErrorElement, ErrorView, ErrorViewBuilder,
    FlutterError, InheritedElement, InheritedView, IntoElement, IntoView, ParentData,
    ParentDataElement, ParentDataView, ProxyElement, ProxyView, RenderElement, RenderView,
    RootRenderElement, RootRenderView, StatefulElement, StatefulView, StatelessElement,
    StatelessView, View, ViewExt, ViewState, clear_error_view_builder, set_error_view_builder,
};

// ============================================================================
// Prelude
// ============================================================================

/// Commonly used types for convenient importing.
///
/// ```rust,ignore
/// use flui_view::prelude::*;
/// ```
pub mod prelude {
    pub use flui_foundation::{ElementId, RenderId};
    pub use flui_log::{debug, error, info, trace, warn};

    // Logging
    pub use crate::context::{BuildContext, BuildContextExt};
    pub use crate::{
        binding::{
            AppExitResponse, AppLifecycleState, PredictiveBackEvent, RouteInformation,
            ViewFocusDirection, ViewFocusEvent, ViewFocusState, WidgetsBinding,
            WidgetsBindingObserver,
        },
        child::{Child, Children},
        element::{
            ElementSlot, IndexedSlot, LayoutChangedNotification, Lifecycle, NotifiableElement,
            Notification, NotificationNode, RootElement,
        },
        key::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey},
        owner::{BuildOwner, ElementOwner},
        tree::{ElementNode, ElementTree, reconcile_children},
        view::{
            BoxedView, InheritedView, IntoView, ParentData, ParentDataView, ProxyView, RenderView,
            StatefulView, StatelessView, View, ViewExt, ViewState,
        },
    };
}
