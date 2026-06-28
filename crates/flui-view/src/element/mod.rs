//! Element system - mutable tree nodes that manage View lifecycle.
//!
//! Elements are the retained, mutable counterparts to immutable Views.
//! They manage:
//! - View lifecycle (mount, build, update, unmount)
//! - State persistence (for StatefulViews)
//! - Child element relationships
//! - RenderObject connections

pub(crate) mod child_manager;
mod inherited_access;
mod lifecycle;
mod notification;
mod render_object_element;
mod root;
pub(crate) mod sliver_adaptor;
pub(crate) mod sparse_children;

// New generic infrastructure
pub mod arity;
pub mod behavior;
pub(crate) mod behavior_commons;
// `dispatch` is the only View-update routing path post-§U27. The
// pre-FR-021 `feature = "legacy-downcast"` gate (Phase 1 §U8 /
// KTD-4) is gone: `ElementCore::update_view` unconditionally
// routes through `dispatch_view_update`, which performs a
// `TypeId`-keyed dispatch + `Downcast::into_any` + `Box::downcast`
// (no `downcast_ref::<V>()` pattern). The workspace-internal
// feature isolation guard (`cfg(__flui_legacy_downcast_internal)`)
// is retired here.
pub(crate) mod dispatch;
pub mod generic;
pub mod kind;
pub mod unified;

use flui_foundation::ElementId;
// Slot types live in flui-tree (canonical home per `flui-tree-unified-interface-intent`
// memory + STRATEGY.md "Behavior loyal, structure Rust-native"). flui-view re-exports
// the bare `IndexedSlot` and aliases `ElementSlot` to its `ElementId` instantiation.
pub use flui_tree::IndexedSlot;

// Re-export commonly used arity and generic types
pub use arity::{ElementArity, Leaf, Optional, Single, Variable};
pub use behavior::{
    AnimatedBehavior, ElementBehavior, InheritedBehavior, ParentDataBehavior, ProxyBehavior,
    RenderBehavior, StatefulBehavior, StatelessBehavior,
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
pub use sliver_adaptor::SliverListAdaptorView;
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

/// Parent-data element with a single child.
///
/// This is the element type created for views implementing `ParentDataView`
/// (`Flexible`, `Expanded`, `Positioned`). It is a transparent proxy: it
/// reconciles its wrapped child unchanged, and additionally surfaces the
/// view's configured parent-data through `parent_data_config()` so the
/// `ElementTree` insert/update seams can write it onto the child render node.
pub type ParentDataElement<V> = Element<V, Single, ParentDataBehavior>;
