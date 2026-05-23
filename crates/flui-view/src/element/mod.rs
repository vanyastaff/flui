//! Element system - mutable tree nodes that manage View lifecycle.
//!
//! Elements are the retained, mutable counterparts to immutable Views.
//! They manage:
//! - View lifecycle (mount, build, update, unmount)
//! - State persistence (for StatefulViews)
//! - Child element relationships
//! - RenderObject connections

mod inherited_access;
mod lifecycle;
mod notification;
mod render_object_element;
mod root;

// New generic infrastructure
pub mod arity;
pub mod behavior;
pub(crate) mod behavior_commons;
pub mod child_storage;
// `dispatch` is the default-features path for view-update routing
// (Phase 1 §U8). Under `feature = "legacy-downcast"` the module is
// not compiled — the inline body inside `ElementCore::update_view`
// takes over instead, and the dispatch helpers
// (`replace_view_for_dispatch`, `mark_dirty_for_dispatch`) would
// otherwise trigger dead-code warnings.
#[cfg(not(feature = "legacy-downcast"))]
pub(crate) mod dispatch;
pub mod generic;
pub mod kind;
pub mod unified;

// Workspace-internal feature isolation guard (plan §U8 / KTD-4).
// If a downstream consumer enables `feature = "legacy-downcast"`
// without also setting `cfg(__flui_legacy_downcast_internal)` via
// rustflags, the build fails HERE with a clear message rather than
// silently re-introducing the pre-FR-021 runtime-downcast path.
#[cfg(all(feature = "legacy-downcast", not(__flui_legacy_downcast_internal)))]
compile_error!(
    "the `legacy-downcast` feature on `flui-view` is workspace-internal only. \
     It is gated behind the additional `cfg(__flui_legacy_downcast_internal)` flag, \
     set only by `crates/flui-view`'s own benchmark via \
     `[[bench]] rustflags = [\"--cfg=__flui_legacy_downcast_internal\"]`. \
     If you reached this error from a workspace consumer that enables the feature \
     (directly or via cargo's resolver-v2 feature unification), remove the feature \
     declaration. See docs/PORT.md."
);

use flui_foundation::ElementId;
// Slot types live in flui-tree (canonical home per `flui-tree-unified-interface-intent`
// memory + STRATEGY.md "Behavior loyal, structure Rust-native"). flui-view re-exports
// the bare `IndexedSlot` and aliases `ElementSlot` to its `ElementId` instantiation.
pub use flui_tree::IndexedSlot;

// Re-export commonly used arity and generic types
pub use arity::{ElementArity, Leaf, Optional, Single, Variable};
pub use behavior::{
    AnimatedBehavior, ElementBehavior, InheritedBehavior, ProxyBehavior, RenderBehavior,
    StatefulBehavior, StatelessBehavior,
};
pub use child_storage::{
    ElementChildStorage, NoChildStorage, OptionalChildStorage, SingleChildStorage,
    VariableChildStorage,
};
pub use generic::ElementCore;
pub use inherited_access::InheritedElementAccess;
pub use kind::{
    AnimationListener, ElementKind, InheritedElementBase, ProxyElementBase, RenderElementBase,
    StatefulElementBase, StatelessElementBase,
};
pub use lifecycle::Lifecycle;
pub use notification::{
    DragEndNotification, DragStartNotification, FocusNotification, KeepAliveNotification,
    LayoutChangedNotification, NotifiableElement, Notification, ScrollNotification,
    SizeChangedNotification,
};
pub use render_object_element::{RenderObjectElement, RenderSlot, RenderTreeRootElement};
pub use root::{RootElement, RootElementImpl};
pub use unified::Element;

/// Slot describing a child element's position in its parent's children list.
///
/// Type alias for `flui_tree::IndexedSlot<ElementId>`. The slot tracks the
/// child's 0-based index plus an optional previous-sibling `ElementId` (the
/// payload semantics view-local code used to spell `IndexedSlot<Option<ElementId>>`).
///
/// # Flutter equivalent
///
/// Mirrors Flutter's `IndexedSlot<T extends Element?>` used by
/// `MultiChildRenderObjectElement` for O(1) child insertion. The previous-sibling
/// reference enables in-place reordering without re-mounting.
///
/// # Example
///
/// ```rust
/// use flui_foundation::ElementId;
/// use flui_view::ElementSlot;
///
/// let first = ElementSlot::first();
/// assert_eq!(first.index(), 0);
/// assert!(first.is_first());
/// assert!(first.previous().is_none());
///
/// let second = first.next(ElementId::new(1));
/// assert_eq!(second.index(), 1);
/// assert_eq!(second.previous(), Some(ElementId::new(1)));
/// ```
pub type ElementSlot = IndexedSlot<ElementId>;

// ============================================================================
// Type Aliases for Ergonomic Element Creation
// ============================================================================

/// Stateless element with single child.
///
/// This is the element type created for views implementing `StatelessView`.
pub type StatelessElement<V> = Element<V, Single, StatelessBehavior>;

/// Proxy element with single child.
///
/// This is the element type created for views implementing `ProxyView`.
pub type ProxyElement<V> = Element<V, Single, ProxyBehavior>;

/// Stateful element with single child and persistent state.
///
/// This is the element type created for views implementing `StatefulView`.
pub type StatefulElement<V> = Element<V, Single, StatefulBehavior<V>>;

/// Render element with variable children and a RenderObject.
///
/// This is the element type created for views implementing `RenderView`.
pub type RenderElement<V> = Element<V, Variable, RenderBehavior<V>>;

/// Inherited element with single child and dependent tracking.
///
/// This is the element type created for views implementing `InheritedView`.
pub type InheritedElement<V> = Element<V, Single, InheritedBehavior<V>>;

/// Animated element with single child and automatic animation listener.
///
/// This is the element type created for views implementing `AnimatedView`.
/// Automatically subscribes to listenable changes and marks element dirty
/// when the animation value changes.
pub type AnimatedElement<V> = Element<V, Single, AnimatedBehavior<V>>;
