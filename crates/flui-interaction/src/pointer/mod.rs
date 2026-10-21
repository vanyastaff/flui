//! Pointer delivery: hit testing, the interaction lane, routing, capture and
//! hover tracking, and the resampling of raw input to frame time.
//!
//! - [`HitTestResult`] - Spatial hit testing
//! - [`InteractionLane`] - Owner-local storage of the handlers a route invokes
//! - [`PointerRouter`] - Global and per-pointer routes
//! - [`PointerCapture`] - Exclusive delivery of a contact to one target
//! - [`MouseTracker`] - Mouse enter/exit/hover tracking
//! - [`PointerEventResampler`] - Resampling raw input at frame time
//!
//! [`crate::GestureBinding`] retains pointer hit routes resolved by
//! [`InteractionLane`].

mod hit_test;
pub(crate) mod interaction_lane;
pub(crate) mod mouse_tracker;
pub(crate) mod pointer_capture;
mod pointer_router;
mod resampler;
mod sampling_clock;
mod wheel_preferences;

pub use crate::containment::RoutePanic;
pub use hit_test::{
    CursorRequest, EventPropagation, HitTestBehavior, HitTestEntry, HitTestResult, RenderId,
    TransformGuard,
};
pub(crate) use hit_test::{PanZoomRoute, ScrollRoute, localize_point};
pub(crate) use interaction_lane::OwnerLatch;
pub(crate) use interaction_lane::active_dispatch_handle;
pub use interaction_lane::{
    HitTestHandle, HitTestProbe, HitTestSnapshot, InteractionDispatchError,
    InteractionDispatchHandle, InteractionLane, LocalPayloadTarget, MouseEnterCallback,
    MouseExitCallback, MouseHoverCallback, MouseRegionCallbacks, MouseRegionTarget,
    PanZoomDispatch, PanZoomTarget, PathClipTarget, PointerDispatch, PointerTarget,
    ResolvedRouteToken, RouteResolution, RouteResolutionMiss, ScrollTarget, ShaderMaskTarget,
    resolve_local_payload, resolve_path_clip_target, resolve_shader_mask_target,
};
pub(crate) use interaction_lane::{PanZoomAdmissionAuthority, PanZoomRetirement};
pub use mouse_tracker::{
    CursorChangeCallback, MouseTracker, MouseTrackerAnnotation, PointerMotionKind,
};
pub use pointer_capture::{PointerCapture, PointerCaptureError};
pub use pointer_router::{GlobalPointerHandler, PointerRouteHandler, PointerRouter};
pub use resampler::{DEFAULT_RESAMPLE_LOOKBACK, PointerEventResampler};
pub use sampling_clock::{DEFAULT_SAMPLE_PERIOD, SamplingClock};
pub use wheel_preferences::{WheelPreferencesProvider, WheelPreferencesSource};
