//! Event routing infrastructure
//!
//! This module provides the core event routing system:
//!
//! - [`EventRouter`] - Main event dispatcher
//! - [`HitTestResult`] - Spatial hit testing
//! - [`FocusManager`] - Keyboard focus management
//! - [`PointerRouter`] - Centralized pointer event routing
//!
//! # Architecture
//!
//! ```text
//! Platform Events
//!       ↓
//! EventRouter (dispatches based on event type)
//!       ├─ Pointer Events → HitTest → Handlers
//!       ├─ Key Events → FocusManager → Focused Element
//!       └─ Scroll Events → HitTest → Scroll Handlers
//! ```

pub(crate) mod event_router;
mod focus;
mod hit_test;
mod pointer_router;

pub use event_router::EventRouter;
pub use focus::{FocusManager, KeyEventCallback};
pub use hit_test::{
    ElementId, EventPropagation, HitTestBehavior, HitTestEntry, HitTestResult, HitTestable,
    PointerEventHandler, ScrollEventHandler, TransformGuard,
};
pub use pointer_router::{GlobalPointerHandler, PointerRouteHandler, PointerRouter};
