//! Event routing infrastructure
//!
//! This module provides the core event routing system:
//!
//! - [`HitTestResult`] - Spatial hit testing
//! - [`FocusManager`] - Keyboard focus management
//! - [`FocusScopeNode`] - Groups focusable elements for keyboard navigation
//! - [`FocusTraversalPolicy`] - Determines Tab/Shift+Tab navigation order
//! - [`PointerRouter`] - Centralized pointer event routing
//!
//! [`crate::GestureBinding`] retains pointer hit routes resolved by
//! [`InteractionLane`]. [`FocusManager`] owns keyboard dispatch for the
//! presentation.

mod focus;
pub mod focus_scope;
mod hit_test;
mod interaction_lane;
pub(crate) mod mouse_tracker;
mod pointer_router;
mod traversal;

pub use traversal::{FocusDirection, FocusTraversalOverrides};

pub use focus::{FocusChangeCallback, FocusManager};
pub use focus_scope::{
    FocusAttachment, FocusDetachOutcome, FocusNode, FocusNodeChangeCallback, FocusNodeId,
    FocusNodeRegistration, FocusRequestOutcome, FocusScopeNode, FocusSubscription,
    FocusTraversalPolicy, FocusTreeError, KeyEventHandler, KeyEventResult, NodeContext,
    ReadingOrderPolicy, RectProvider, ResolvedStep, TraversalDirection, TraversalEdgeBehavior,
};
pub use hit_test::{
    CursorRequest, EventPropagation, HitTestBehavior, HitTestEntry, HitTestResult, RenderId,
    TransformGuard,
};
#[doc(hidden)]
pub use interaction_lane::DispatchCustody;
pub(crate) use interaction_lane::OwnerLatch;
pub(crate) use interaction_lane::active_dispatch_handle;
pub use interaction_lane::{
    HitTestHandle, HitTestProbe, HitTestSnapshot, InteractionDispatchError,
    InteractionDispatchHandle, InteractionLane, LocalPayloadTarget, MouseEnterCallback,
    MouseExitCallback, MouseHoverCallback, MouseRegionCallbacks, MouseRegionTarget, PanZoomTarget,
    PathClipTarget, PointerDispatch, PointerTarget, ResolvedRouteToken, RoutePanic,
    RouteResolution, RouteResolutionMiss, ScrollTarget, ShaderMaskTarget, resolve_local_payload,
    resolve_path_clip_target, resolve_shader_mask_target,
};
pub use mouse_tracker::{
    CursorChangeCallback, DeviceId, MouseTracker, MouseTrackerAnnotation, PointerMotionKind,
};
pub use pointer_router::{GlobalPointerHandler, PointerRouteHandler, PointerRouter};
