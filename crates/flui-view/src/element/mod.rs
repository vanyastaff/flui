//! Element system - mutable tree nodes that manage View lifecycle.
//!
//! Elements are the retained, mutable counterparts to immutable Views.
//! They manage:
//! - View lifecycle (mount, build, update, unmount)
//! - State persistence (for StatefulViews)
//! - Child element relationships
//! - RenderObject connections

mod lifecycle;
mod notification;
mod render_object_element;
mod root;
mod slot;

// New generic infrastructure
pub mod arity;
pub mod behavior;
pub mod child_storage;
pub mod generic;
pub mod unified;

pub use lifecycle::Lifecycle;
pub use notification::{
    BoxedNotification, DragEndNotification, DragStartNotification, FocusNotification,
    KeepAliveNotification, LayoutChangedNotification, NotifiableElement, Notification,
    NotificationCallback, NotificationHandler, NotificationNode, ScrollNotification,
    SizeChangedNotification,
};
pub use render_object_element::{RenderObjectElement, RenderSlot, RenderTreeRootElement};
pub use root::{RootElement, RootElementImpl};
pub use slot::{ElementSlot, IndexedSlot, IndexedSlotBuilder};

// Re-export commonly used arity and generic types
pub use arity::{ElementArity, Leaf, Optional, Single, Variable};
pub use behavior::{
    AnimationBehavior, ElementBehavior, InheritedBehavior, ProxyBehavior, RenderBehavior,
    StatefulBehavior, StatelessBehavior,
};
pub use child_storage::{
    ElementChildStorage, NoChildStorage, OptionalChildStorage, SingleChildStorage,
    VariableChildStorage,
};
pub use generic::ElementCore;
pub use unified::Element;

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
pub type AnimatedElement<V> = Element<V, Single, AnimationBehavior<V>>;
