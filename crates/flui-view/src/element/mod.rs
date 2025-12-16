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
mod root;
mod slot;

pub use lifecycle::Lifecycle;
pub use notification::{
    BoxedNotification, DragEndNotification, DragStartNotification, FocusNotification,
    KeepAliveNotification, LayoutChangedNotification, NotifiableElement, Notification,
    NotificationCallback, NotificationHandler, NotificationNode, ScrollNotification,
    SizeChangedNotification,
};
pub use root::{RootElement, RootElementImpl};
pub use slot::{ElementSlot, IndexedSlot, IndexedSlotBuilder};

// Element trait and implementations will be added in later phases
