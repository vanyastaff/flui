//! Event routing infrastructure
//!
//! This module provides the core event routing system:
//!
//! - [`EventRouter`] - Main event dispatcher
//! - [`HitTestResult`] - Spatial hit testing
//! - [`FocusManager`] - Keyboard focus management
//! - [`FocusScope`] - Groups focusable elements for keyboard navigation
//! - [`FocusTraversalPolicy`] - Determines Tab/Shift+Tab navigation order
//! - [`PointerRouter`] - Centralized pointer event routing
//!
//! # Architecture
//!
//! ```text
//! Platform Events
//!       ↓
//! EventRouter (dispatches based on event type)
//!       ├─ Pointer Events → HitTest → Handlers
//!       ├─ Key Events → FocusManager → FocusScope → Focused Element
//!       └─ Scroll Events → HitTest → Scroll Handlers
//! ```

pub(crate) mod event_router;
mod focus;
pub mod focus_scope;
mod hit_test;
mod pointer_router;

pub use event_router::EventRouter;
pub use focus::{FocusManager, KeyEventCallback};
pub use focus_scope::{
    DirectionalFocusPolicy, FocusNode, FocusNodeId, FocusScopeNode, FocusTraversalPolicy,
    KeyEventHandler, KeyEventResult, OrderedTraversalPolicy, ReadingOrderPolicy,
    TraversalDirection,
};
pub use hit_test::{
    ElementId, EventPropagation, HitTestBehavior, HitTestEntry, HitTestResult, HitTestable,
    PointerEventHandler, ScrollEventHandler, TransformGuard,
};
pub use pointer_router::{GlobalPointerHandler, PointerRouteHandler, PointerRouter};
