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

// Test-only global-key registry shims. Live in `lib.rs` rather than
// `key/mod.rs` so the public name is `flui_view::test_only_*` — short,
// clearly tagged as test-only via the prefix. Production code installs
// the registry handle inside `WidgetsBinding`; tests bypass the binding
// and need this entrypoint.
mod test_only_global_key_registry {
    use std::sync::Arc;

    use parking_lot::RwLock;

    use crate::{
        key::registry::{GlobalKeyRegistryHandle, install_registry, take_registry},
        owner::BuildOwner,
        tree::ElementTree,
    };

    /// Install the given `ElementTree` + `BuildOwner` as the
    /// process-wide registry source for `GlobalKey::current_*` lookups.
    ///
    /// Tests in `tests/global_key.rs` install a handle pointing to a
    /// local tree, run their assertions, then call
    /// [`test_only_clear_global_key_registry`].
    ///
    /// **Not for production code.** Production binds the handle inside
    /// `WidgetsBinding::new`.
    pub fn test_only_set_global_key_registry(
        tree: &Arc<RwLock<ElementTree>>,
        owner: &Arc<RwLock<BuildOwner>>,
    ) {
        let owner_for_lookup = Arc::clone(owner);
        let tree_for_visit = Arc::clone(tree);
        let handle = GlobalKeyRegistryHandle::new(
            move |hash| owner_for_lookup.read().element_for_global_key(hash),
            move |id, f| {
                let tree = tree_for_visit.read();
                if let Some(node) = tree.get(id) {
                    f(node.element());
                }
            },
        );
        let _ = install_registry(handle);
    }

    /// Clear the process-wide registry handle. No-op if no handle was
    /// installed. Tests call this in their teardown so subsequent
    /// tests start from a quiescent state.
    pub fn test_only_clear_global_key_registry() {
        let _ = take_registry();
    }
}

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
// Test-only handle for `GlobalKey::current_*` lookup. Production code
// installs the handle via `WidgetsBinding`; tests bypass the binding.
pub use test_only_global_key_registry::{
    test_only_clear_global_key_registry, test_only_set_global_key_registry,
};
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
