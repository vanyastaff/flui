//! Event routing infrastructure
//!
//! This module provides the core event routing system:
//!
//! - [`EventRouter`] - Main event dispatcher
//! - [`HitTestResult`] - Spatial hit testing
//! - [`FocusManager`] - Keyboard focus management
//! - [`FocusScopeNode`] - Groups focusable elements for keyboard navigation
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
mod interaction_lane;
pub(crate) mod mouse_tracker;
mod pointer_router;

pub use event_router::EventRouter;
pub use focus::{FocusManager, KeyEventCallback};
pub use focus_scope::{
    FocusNode, FocusNodeId, FocusScopeNode, FocusTraversalPolicy, KeyEventHandler, KeyEventResult,
    ReadingOrderPolicy, RectProvider, ResolvedStep, TraversalEdgeBehavior,
};
pub use hit_test::{
    EventPropagation, HitTestBehavior, HitTestEntry, HitTestResult, HitTestable, RenderId,
    ScrollEventHandler, TransformGuard,
};
pub(crate) use interaction_lane::active_dispatch_handle;
pub use interaction_lane::{
    InteractionDispatchError, InteractionDispatchHandle, InteractionLane, MouseEnterCallback,
    MouseExitCallback, MouseHoverCallback, MouseRegionCallbacks, MouseRegionTarget, PointerTarget,
    ResolvedRouteToken, RoutePanic, RouteResolution, RouteResolutionMiss,
};
pub use mouse_tracker::{CursorChangeCallback, DeviceId, MouseTracker, MouseTrackerAnnotation};
pub use pointer_router::{GlobalPointerHandler, PointerRouteHandler, PointerRouter};
