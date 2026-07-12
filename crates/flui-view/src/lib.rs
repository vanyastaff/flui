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
//!     fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
//!         Text::new(format!("Count: {}", self.count)).boxed()
//!     }
//! }
//! ```

// Lint levels come from `[workspace.lints]`. Ship bar (wave 3): every public
// item is documented; keep it that way.
#![deny(missing_docs)]
// `element/element.rs`, `view/view.rs`: a one-type family module named after
// its type is the catalog's house style (matches flui-widgets/flui-objects).
#![allow(clippy::module_inception)]
// ADR-0027: the owner-plane view/element/build/global-key graph is intentionally
// `!Send`, while existing internal handles are still `Arc`-shaped. Do not restore
// `Send + Sync` to UI callbacks or tree owners to satisfy this lint; a focused
// owner-local handle migration can replace these with `Rc` later.
#![allow(clippy::arc_with_non_send_sync)]

// ============================================================================
// Modules
// ============================================================================

pub mod binding;
pub mod child;
pub mod context;
pub mod element;
pub mod key;
pub mod macros; // PORT-CHECK-OK-SP4: macros consumed via #[macro_export] (no qualified path); intentional API surface
pub mod owner;
pub mod seq; // PORT-CHECK-OK-SP4: seq/Children API surface; consumed via prelude re-exports
pub mod tree;
pub mod view;

// Legacy test-only global-key registry shims. Live in `lib.rs` rather than
// `key/mod.rs` so the public name is `flui_view::test_only_*` — short,
// clearly tagged as test-only via the prefix. Production code activates
// the registry handle owned by `WidgetsBinding`; integration fixtures that
// bypass a binding still need this entrypoint until they adopt scoped harnesses.
mod test_only_global_key_registry {
    use std::sync::Arc;

    use parking_lot::RwLock;

    use crate::{
        key::registry::{GlobalKeyRegistryHandle, install_registry, take_registry},
        owner::BuildOwner,
        tree::ElementTree,
    };

    /// Install the given `ElementTree` + `BuildOwner` as the
    /// current-thread registry source for `GlobalKey::current_*` lookups.
    ///
    /// Tests in `tests/global_key.rs` install a handle pointing to a
    /// local tree, run their assertions, then call
    /// [`test_only_clear_global_key_registry`].
    ///
    /// **Not for production code.** Production activates the realm-owned
    /// binding handle only inside `UiRealm::enter`.
    #[doc(hidden)]
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

    /// Clear the current-thread registry handle. No-op if no handle was
    /// installed. Tests call this in their teardown so subsequent
    /// tests start from a quiescent state.
    #[doc(hidden)]
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
    AppExitResponse, AppLifecycleState, AttachError, PredictiveBackEvent, RouteInformation,
    ViewFocusDirection, ViewFocusEvent, ViewFocusState, WidgetsBinding, WidgetsBindingObserver,
};
// Child helpers
pub use child::{Child, Children};
// Context
pub use context::{BuildContext, BuildContextExt, ElementBuildContext, ElementBuildContextBuilder};
// Element types
pub use element::Lifecycle;
// Notification system
pub use element::{
    DragEndNotification, DragStartNotification, FocusNotification, KeepAliveNotification,
    LayoutChangedNotification, NotifiableElement, Notification, ScrollNotification,
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
// Logging re-exports from flui_foundation::log (merged from flui-log).
pub use flui_foundation::log::{Level, Logger, debug, error, info, trace, warn};
// Keys
pub use key::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey};
// Legacy test-only handle for `GlobalKey::current_*` lookup. Production code
// activates the handle through `UiRealm::enter`; tests bypass the binding.
pub use test_only_global_key_registry::{
    test_only_clear_global_key_registry, test_only_set_global_key_registry,
};
// Tree management
pub use owner::{BuildOwner, ElementOwner, RebuildHandle};
pub use tree::{ElementNode, ElementTree};
pub use view::{
    AnimatedElement, AnimatedView, BoxedElement, BoxedView, ElementBase, ElementExt, ErrorElement,
    ErrorView, ErrorViewBuilder, FlutterError, InheritedElement, InheritedView, IntoElement,
    IntoView, Memo, ParentDataConfig, ParentDataElement, ParentDataView, ProxyElement, ProxyView,
    RenderElement, RenderObjectContext, RenderObjectContextError, RenderView, RootRenderElement,
    RootRenderView, StatefulElement, StatefulView, StatelessElement, StatelessView, View, ViewExt,
    ViewState, clear_error_view_builder, set_error_view_builder,
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
    pub use flui_foundation::log::{debug, error, info, trace, warn};
    pub use flui_foundation::{ElementId, RenderId};
    // The proc-macro derives ship from `flui-macros` but are surfaced
    // here so a single `use flui_view::prelude::*;` picks them up
    // alongside the supporting trait.
    //
    // The re-export deliberately preserves the macro names
    // (`StatelessView`, `StatefulView`) so they sit alongside the
    // traits with the same names — this is the standard Rust
    // `#[derive(Trait)] + trait Trait` pattern (e.g. `Clone`,
    // `Serialize`). Rust's namespace separation (macros vs types vs
    // traits) makes the collision well-defined: `#[derive(StatelessView)]`
    // picks the macro, `impl StatelessView for X { … }` picks the trait.
    pub use flui_macros::{StatefulView, StatelessView};

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
            Notification, RootElement,
        },
        key::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey},
        owner::{BuildOwner, ElementOwner},
        tree::{ElementNode, ElementTree},
        view::{
            AnimatedView, BoxedView, InheritedView, IntoView, Memo, ParentDataConfig,
            ParentDataView, ProxyView, RenderObjectContext, RenderObjectContextError, RenderView,
            StatefulView, StatelessView, View, ViewExt, ViewState,
        },
    };
}
