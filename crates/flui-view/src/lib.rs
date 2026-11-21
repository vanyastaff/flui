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
//!     fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
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

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Lint levels come from `[workspace.lints]`. Every public
// item is documented; keep it that way.
#![deny(missing_docs)]
// Hash tables on the owner and frame paths are FxHash (clippy.toml `disallowed-types`).
#![warn(clippy::disallowed_types)]
#![expect(
    clippy::module_inception,
    reason = "a one-type family module named after its type (`element/element.rs`, `view/view.rs`) is the catalog's house style"
)]

// ============================================================================
// Modules
// ============================================================================

// Derive expansions use the same absolute owner path in library and integration targets.
#[allow(
    unused_extern_crates,
    reason = "derive expansions resolve the owner by its absolute crate name"
)]
extern crate self as flui_view;

// Open modules: namespaces whose items are named through the module and
// nowhere else. Closed modules are private; the items of theirs that other
// crates name are re-exported at the root below, and the ones only the
// composition roots name are re-exported from `__runtime`.
pub mod background;
mod binding;
mod child;
mod context;
pub mod dev_agent;
pub mod dev_reload;
pub mod element;
pub mod environment;
mod inspect;
mod key;
pub mod navigation;
mod owner;
pub mod persist;
mod reactive;
mod seq;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod tree;
mod view;

// Legacy test-only global-key registry shims. Live in `lib.rs` rather than
// `key/mod.rs` so the public name is `flui_view::test_only_*` — short,
// clearly tagged as test-only via the prefix. Production code activates
// the registry handle owned by `WidgetsBinding`; integration fixtures that
// bypass a binding still need this entrypoint until they adopt scoped harnesses.
mod test_only_global_key_registry {
    use std::{cell::RefCell, rc::Rc};

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
    /// **Not for production code.** Production activates the UI runtime-owned
    /// binding handle only inside `UiRuntime::enter`.
    #[doc(hidden)]
    pub fn test_only_set_global_key_registry(
        tree: &Rc<RefCell<ElementTree>>,
        owner: &Rc<RefCell<BuildOwner>>,
    ) {
        let owner_for_lookup = Rc::clone(owner);
        let tree_for_visit = Rc::clone(tree);
        let handle = GlobalKeyRegistryHandle::new(
            move |key| Ok(owner_for_lookup.borrow().element_for_global_key(key)),
            move |id, f| {
                let tree = tree_for_visit.borrow();
                if let Some(node) = tree.get(id) {
                    f(node.element());
                }
                Ok(())
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

#[cfg(test)]
mod table_test;
// Runtime seam for flui-runtime, flui-testing and flui-app (ADR-0081 §4)
#[doc(hidden)]
pub mod __runtime;
/// The paths this crate's exported macros expand to. No semver promise: only
/// the expansions of `impl_render_view!` and `impl_stateful_render_view!`
/// name it.
#[doc(hidden)]
pub mod __private {
    pub use flui_foundation::CreationSite;
}
// Lifecycle notifications
mod lifecycle;
mod observation;
mod window_handle;
pub use lifecycle::{
    HostNoticeSubscription, LifecycleClosed, LifecycleHandle, LifecycleSubscription,
};
pub use window_handle::{WindowClosed, WindowHandle, WindowSubscription};
// Holding a presentation's close while work finishes
mod close_guard;
pub use close_guard::{CloseChanged, CloseGuard, CloseHold, CloseReason, PendingClose, StaleClose};
// The host's registry of published document bytes and the owner-notify
// channel; reached through `__runtime`
mod flush_registry;
mod haptics_mode;
pub use haptics_mode::HapticsMode;
mod owner_notify;
// The application lifecycle a `LifecycleHandle` reports
pub use binding::AppLifecycleState;
// Child helpers
pub use child::{Child, Children};
// Context
pub use context::{BuildContext, BuildContextExt, LifecycleContext, LifecycleContextExt};
// Notification system
pub use element::notification::{
    DragEndNotification, DragStartNotification, FocusNotification, KeepAliveNotification,
    LayoutChangedNotification, Notification, ScrollNotification, SizeChangedNotification,
};
// Values a view or a state publishes to its subtree
pub use element::provided::{Provided, ProvidedValue};
// Keys
pub use key::{GlobalKey, KeyLookup, ObjectKey, ValueKey};
// Legacy test-only handle for `GlobalKey::current_*` lookup. Production code
// activates the handle through `UiRuntime::enter`; tests bypass the binding.
pub use test_only_global_key_registry::{
    test_only_clear_global_key_registry, test_only_set_global_key_registry,
};
// An element's owner, and the handles a state keeps
pub use owner::{
    ElementOwner, KeepAliveHandle, KeepAliveLease, RebuildHandle, RebuildReason, UnmountHook,
};
pub use reactive::{
    EventContext, EventContextError, EventError, EventOutcome, Reactive, ReadScope, Signal,
    SignalError, SignalSender, SignalSlot, SignalWriteExt, WriteTarget, Writer, WriterSource,
    callback, callback_ref, callback_with,
};
// Heterogeneous children
pub use seq::ViewSeq;
// The keyed reconciler's trace stream
pub use tree::{RECONCILE_TARGET, ReconcileEvent, ReconcileEventKind};
pub use view::{
    AnimatedView, BoxedElement, BoxedView, ChildViews, ElementBase, ElementDepth, ElementExt,
    ErrorView, ErrorViewBuilder, FieldMask, FieldSet, FrameworkError, InheritedData, InheritedView,
    IntoElement, IntoView, Memo, ParentDataConfig, ParentDataView, ProxyView, RenderObjectContext,
    RenderObjectContextError, RenderView, RenderViewState, StatefulRenderView, StatefulView,
    StatelessView, View, ViewExt, ViewState,
};

// ============================================================================
// Prelude
// ============================================================================

/// The forms a widget author writes with: the view and state traits and their
/// derives, the build and lifecycle contexts, signals and event callbacks,
/// children, keys, rebuild handles, notifications and the environment scopes a
/// tree is wrapped in.
///
/// ```rust,ignore
/// use flui_view::prelude::*;
/// ```
pub mod prelude {
    // The view traits with their derives, which share the traits' names.
    pub use crate::{InheritedData, StatefulView, StatelessView};

    pub use crate::{BuildContext, BuildContextExt, LifecycleContext, LifecycleContextExt};
    // Signal writes (`set`/`update`/`set_if_changed`). Reads need no import:
    // `Signal::get` is inherent and takes the context as a `ReadScope`, so
    // `ReadScope` is not in the prelude. `ctx.scope()` still resolves on any
    // `BuildContext` without it (a supertrait's methods do); the scope it
    // returns is opaque, so that reaches nothing but a signal read.
    pub use crate::SignalWriteExt;
    // The write capability an event callback receives, the handle it writes,
    // the source a widget opens it from, the outcome a callback reports, and
    // the helpers that fix a `let`-bound callback's signature (ADR-0086).
    pub use crate::{
        AnimatedView, AppLifecycleState, BoxedView, Child, Children, GlobalKey, InheritedView,
        IntoView, LayoutChangedNotification, Memo, Notification, ObjectKey, ParentDataConfig,
        ParentDataView, ProxyView, RebuildHandle, RebuildReason, RenderObjectContext,
        RenderObjectContextError, RenderView, ValueKey, View, ViewExt, ViewState,
        environment::{GestureArenaScope, GesturePolicy, MediaQuery, MediaQueryData, VsyncScope},
    };
    pub use crate::{
        EventContext, EventContextError, EventError, EventOutcome, Signal, WriterSource, callback,
        callback_ref, callback_with,
    };
}

pub(crate) use flui_foundation::panic::drop_contained;
