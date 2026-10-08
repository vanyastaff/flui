//! [`InteractiveViewer`] — pans and zooms its child through a transformation
//! matrix.
//!
//! The contract: a single [`TransformationController`]-held
//! [`Matrix4`] maps the child's scene coordinates to viewport coordinates;
//! gestures update that matrix; `min_scale`/`max_scale` clamp the zoom level;
//! `boundary_margin` constrains how far the transformed viewport may drift
//! from the child's own rect (an all-infinite margin removes the boundary
//! entirely). Contact callbacks observe the recognized gesture even when an
//! application option disables its transform. Native sources are admitted only
//! when their enabled transform changes the scene, so an ancestor can act.
//!
//! Contact pan, pinch and rotation use one persistent Scale recognizer in
//! combined pan/scale mode. Adding or lifting a contact rebases measurement
//! without moving the scene or restarting the interaction. Rotation is opt-in.
//! Native pan-zoom uses the same actor after leaf-first admission; a started
//! source carries cumulative transforms until End or Cancelled. Unstarted
//! relative updates remain independent interactions. Wheel zoom uses the
//! existing scroll claim lane with `exp(-scroll_dy / scale_factor)`.
//!
//! The scene point under the moving focal point remains anchored when bounds
//! permit. Rotation that cannot fit the boundary at the admitted scale is
//! refused; no extra zoom is invented. Focal release velocity drives a library
//! friction simulation through the presentation's VsyncScope. New contact or
//! accepted wheel input stops it; source cancellation supplies no impulse.
//! - **`constrained: false`** (an unconstrained child laid out via an
//!   `OverflowBox`-equivalent, escaping the viewport) is **deferred**. V1
//!   only supports `constrained: true` — the child is laid out under
//!   whatever constraints this widget itself receives, exactly like
//!   [`Transform`]. A consequence used throughout this file: because nothing
//!   between the child and this widget's own box imposes a different size
//!   (`Listener`/`GestureDetector`/`ClipRect`/`Transform` are all
//!   layout-transparent, size-adopting proxies), **the viewport rect and the
//!   child's own (unmargined) rect are numerically identical in V1** — see
//!   [`InteractiveViewerState::geometry`]. Adding `constrained: false` later
//!   means that identity stops holding and a second, viewport-only anchor
//!   becomes load bearing again.
//!
//! `on_interaction_start`/`on_interaction_update`/`on_interaction_end` carry
//! FLUI's own detail types ([`InteractionStartDetails`] etc.) rather than
//! the recognizer's cumulative measurements. The update carries the applied
//! scale multiplier (1.0 for a pure pan), a focal point and a translation
//! delta. The end carries separate focal and scale release velocities.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, FrictionSimulation, Tolerance, Vsync, VsyncRegistration,
};
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::Matrix4;
use flui_foundation::geometry::{EdgeInsets, Offset, Point, Rect};
use flui_foundation::{Listenable, ListenerId};
use flui_interaction::GestureEndReason;
use flui_interaction::Velocity;
use flui_interaction::recognizers::scale::{
    ScaleEndDetails, ScaleStartDetails, ScaleStartMode, ScaleUpdateDetails,
};
use flui_interaction::routing::EventPropagation;
use flui_objects::SubtreeAnchor;
use flui_painting::Alignment;
use flui_painting::paint::Clip;
use flui_platform_api::{
    keyboard::Modifiers,
    pointer::{PanZoomEvent, PanZoomPhase, ScrollEvent, ScrollUnit},
};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::PipelineCell;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{Child, IntoView, View, ViewState};

use crate::anchored_box::AnchoredBox;
use crate::{AnimatedBuilder, ClipRect, GestureDetector, Listener, Transform, VsyncScope};

use super::transformation_controller::TransformationController;

// ============================================================================
// PanAxis
// ============================================================================

/// Constrains which axis (or axes) [`InteractiveViewer`] pans along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanAxis {
    /// Panning is allowed only along the horizontal axis.
    Horizontal,
    /// Panning is allowed only along the vertical axis.
    Vertical,
    /// Panning is allowed along the horizontal and vertical axes, but never
    /// diagonally — the drag's dominant axis (established on the first
    /// non-zero movement of the gesture) locks for the rest of the gesture.
    Aligned,
    /// Panning is allowed freely in any direction.
    #[default]
    Free,
}

// ============================================================================
// Interaction details
// ============================================================================

/// Details passed to `on_interaction_start`. See the module docs for why this
/// is FLUI's own shape rather than a `ScaleStartDetails` port.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionStartDetails {
    /// The interaction's focal point, in the coordinates of the widget that
    /// contains `InteractiveViewer`.
    pub focal_point: Offset<f64>,
    /// The interaction's focal point, in the coordinates of
    /// `InteractiveViewer` itself (viewport-local).
    pub local_focal_point: Offset<f64>,
}

/// Details passed to `on_interaction_update`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionUpdateDetails {
    /// The interaction's current focal point, in the coordinates of the
    /// widget that contains `InteractiveViewer`.
    pub focal_point: Offset<f64>,
    /// The interaction's current focal point, in `InteractiveViewer`'s own
    /// (viewport-local) coordinates.
    pub local_focal_point: Offset<f64>,
    /// The multiplicative scale change applied by this update. `1.0` for a
    /// pure pan update (no scale change).
    pub scale: f64,
    /// The translation applied by this update, in viewport pixels. Zero for
    /// a pure wheel-scale update.
    pub focal_point_delta: Offset<f64>,
}

/// Details passed to `on_interaction_end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionEndDetails {
    /// Normal completion or an interrupted pointer gesture.
    pub reason: GestureEndReason,
    /// The gesture's release velocity. [`Velocity::ZERO`] for a discrete
    /// wheel-scale interaction (there is no release to measure).
    pub velocity: Velocity,
    /// Measured scale change in scale units per second. Zero for cancellation
    /// and a discrete wheel step, which supply no scale release history.
    pub scale_velocity: f64,
}

type StartCallback = Rc<dyn Fn(&mut EventCx<'_>, InteractionStartDetails)>;
type UpdateCallback = Rc<dyn Fn(&mut EventCx<'_>, InteractionUpdateDetails)>;
type EndCallback = Rc<dyn Fn(&mut EventCx<'_>, InteractionEndDetails)>;

// ============================================================================
// InteractiveViewer
// ============================================================================

/// When the wheel is allowed to drive scroll-to-scale.
///
/// The desktop contract "wheel scrolls, ctrl+wheel zooms" is only
/// composable when the viewer restricts itself to the chord. It claims ticks
/// that actually zoom before the enclosing scrollable is asked. Under
/// [`CtrlWheel`], plain ticks reach that scrollable. [`AnyWheel`] zooms on every
/// vertical tick and is the default.
///
/// [`CtrlWheel`]: WheelScaleGate::CtrlWheel
/// [`AnyWheel`]: WheelScaleGate::AnyWheel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WheelScaleGate {
    /// Every vertical wheel tick zooms.
    #[default]
    AnyWheel,
    /// Only ctrl+wheel zooms; plain ticks are left to enclosing consumers.
    CtrlWheel,
}

/// Pans and zooms `child` through a [`TransformationController`]-held
/// [`Matrix4`].
///
/// See the module docs for the exact scope this V1 port covers.
#[derive(Clone)]
pub struct InteractiveViewer {
    controller: TransformationController,
    boundary_margin: EdgeInsets,
    min_scale: f64,
    max_scale: f64,
    pan_enabled: bool,
    scale_enabled: bool,
    rotation_enabled: bool,
    wheel_scale_gate: WheelScaleGate,
    pan_axis: PanAxis,
    scale_factor: f64,
    clip_behavior: Clip,
    alignment: Option<Alignment>,
    on_interaction_start: Option<StartCallback>,
    on_interaction_update: Option<UpdateCallback>,
    on_interaction_end: Option<EndCallback>,
    child: Child,
}

impl std::fmt::Debug for InteractiveViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractiveViewer")
            .field("controller", &self.controller)
            .field("boundary_margin", &self.boundary_margin)
            .field("min_scale", &self.min_scale)
            .field("max_scale", &self.max_scale)
            .field("pan_enabled", &self.pan_enabled)
            .field("scale_enabled", &self.scale_enabled)
            .field("wheel_scale_gate", &self.wheel_scale_gate)
            .field("pan_axis", &self.pan_axis)
            .finish_non_exhaustive()
    }
}

impl Default for InteractiveViewer {
    fn default() -> Self {
        Self {
            controller: TransformationController::new(),
            boundary_margin: EdgeInsets::ZERO,
            // Eyeballed defaults — reasonable limits for common use cases.
            min_scale: 0.8,
            max_scale: 2.5,
            pan_enabled: true,
            scale_enabled: true,
            rotation_enabled: false,
            wheel_scale_gate: WheelScaleGate::default(),
            pan_axis: PanAxis::Free,
            scale_factor: 200.0,
            clip_behavior: Clip::HardEdge,
            alignment: None,
            on_interaction_start: None,
            on_interaction_update: None,
            on_interaction_end: None,
            child: Child::empty(),
        }
    }
}

impl InteractiveViewer {
    /// Allow rotation around the gesture's focal point. Disabled by default.
    #[must_use]
    pub fn rotation_enabled(mut self, enabled: bool) -> Self {
        self.rotation_enabled = enabled;
        self
    }
    /// A new `InteractiveViewer` with default limits (`min_scale: 0.8`,
    /// `max_scale: 2.5`, zero boundary margin, pan and wheel-scale both
    /// enabled).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Share this transform through an external [`TransformationController`]
    /// instead of the one created internally. Multiple widgets can read (and
    /// drive) the same controller.
    #[must_use]
    pub fn controller(mut self, controller: TransformationController) -> Self {
        self.controller = controller;
        self
    }

    /// A margin for the visible boundaries of the child.
    ///
    /// Any transformation that would move the viewport outside of the
    /// boundary is clamped at the boundary. Pass `EdgeInsets::all(f64::
    /// INFINITY)` for no boundary at all.
    ///
    /// # Precondition
    ///
    /// Every edge must be finite, or every edge must be infinite — not a mix
    /// (checked with `debug_assert!`, so debug-only).
    #[must_use]
    pub fn boundary_margin(mut self, boundary_margin: EdgeInsets) -> Self {
        debug_assert!(
            (boundary_margin.left.is_infinite()
                && boundary_margin.top.is_infinite()
                && boundary_margin.right.is_infinite()
                && boundary_margin.bottom.is_infinite())
                || (boundary_margin.left.is_finite()
                    && boundary_margin.top.is_finite()
                    && boundary_margin.right.is_finite()
                    && boundary_margin.bottom.is_finite()),
            "InteractiveViewer::boundary_margin must be either fully finite or \
             fully infinite on all four edges, not a mix"
        );
        self.boundary_margin = boundary_margin;
        self
    }

    /// The minimum allowed scale. Must be finite and greater than zero, and
    /// no greater than [`max_scale`](Self::max_scale).
    #[must_use]
    pub fn min_scale(mut self, min_scale: f64) -> Self {
        debug_assert!(
            min_scale > 0.0 && min_scale.is_finite(),
            "InteractiveViewer::min_scale must be finite and greater than zero"
        );
        self.min_scale = min_scale;
        self
    }

    /// The maximum allowed scale. Must be greater than zero, not NaN, and no
    /// less than [`min_scale`](Self::min_scale).
    #[must_use]
    pub fn max_scale(mut self, max_scale: f64) -> Self {
        debug_assert!(
            max_scale > 0.0 && !max_scale.is_nan(),
            "InteractiveViewer::max_scale must be greater than zero"
        );
        self.max_scale = max_scale;
        self
    }

    /// If `false`, single-pointer drags do not pan the child.
    /// `on_interaction_*` callbacks still fire.
    #[must_use]
    pub fn pan_enabled(mut self, pan_enabled: bool) -> Self {
        self.pan_enabled = pan_enabled;
        self
    }

    /// If `false`, mouse-wheel scroll does not scale the child.
    /// `on_interaction_*` callbacks still fire.
    #[must_use]
    pub fn scale_enabled(mut self, scale_enabled: bool) -> Self {
        self.scale_enabled = scale_enabled;
        self
    }

    /// Gate wheel-driven zooming on the ctrl chord (default: every vertical
    /// tick zooms). See [`WheelScaleGate`].
    #[must_use]
    pub fn wheel_scale_gate(mut self, gate: WheelScaleGate) -> Self {
        self.wheel_scale_gate = gate;
        self
    }

    /// Restricts panning to one axis, or locks a free drag to whichever axis
    /// dominates it. Defaults to [`PanAxis::Free`].
    #[must_use]
    pub fn pan_axis(mut self, pan_axis: PanAxis) -> Self {
        self.pan_axis = pan_axis;
        self
    }

    /// The divisor applied to a mouse-wheel scroll delta before it becomes an
    /// exponential scale change (`scale_change = exp(-scroll_dy /
    /// scale_factor)`). Larger values feel slower; smaller values feel
    /// faster. Defaults to `200.0`.
    ///
    /// Raw wheel detents and normalized line packets each use an authored
    /// zoom distance of 53 logical pixels per unit. System scroll line counts
    /// and scroll disabling apply to scrolling, independently of this zoom policy.
    #[must_use]
    pub fn scale_factor(mut self, scale_factor: f64) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// How the child is clipped to this widget's bounds. Defaults to
    /// [`Clip::HardEdge`] — pass [`Clip::None`] to let a zoomed-in child
    /// paint outside its original area (it still won't receive gestures
    /// there).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// The alignment of the child's transform pivot. See
    /// [`Transform::alignment`] for the exact contribution when combined
    /// with an origin — `InteractiveViewer` never sets an origin, so this is
    /// simply the pivot the transform matrix scales/rotates around.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Called when a pan or wheel-scale interaction begins.
    #[must_use]
    pub fn on_interaction_start<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventCx<'_>, InteractionStartDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_start =
            Some(Rc::new(move |cx, details| callback(cx, details).report()));
        self
    }

    /// Called on every applied (or attempted, if disabled) pan/wheel-scale
    /// update.
    #[must_use]
    pub fn on_interaction_update<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventCx<'_>, InteractionUpdateDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_update =
            Some(Rc::new(move |cx, details| callback(cx, details).report()));
        self
    }

    /// Called when a pan or wheel-scale interaction ends.
    #[must_use]
    pub fn on_interaction_end<R: EventOutcome>(
        mut self,
        callback: impl Fn(&mut EventCx<'_>, InteractionEndDetails) -> R + 'static,
    ) -> Self {
        self.on_interaction_end = Some(Rc::new(move |cx, details| callback(cx, details).report()));
        self
    }

    /// The child to pan and zoom.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl View for InteractiveViewer {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for InteractiveViewer {
    type State = InteractiveViewerState;

    fn create_state(&self) -> Self::State {
        InteractiveViewerState {
            subtree_anchor: SubtreeAnchor::new(),
            gesture: Rc::new(GestureTracking {
                pan_start_local: Cell::new(None),
                current_axis: Cell::new(None),
                scale: Cell::new(1.0),
                rotation: Cell::new(0.0),
            }),
            pipeline_cell: None,
            writer: None,
            fling: Rc::new(FocalFling::new()),
            vsync: None,
            vsync_registration: None,
        }
    }
}

// ============================================================================
// State
// ============================================================================

/// Tracks the in-flight pan gesture's dominant-axis lock (for
/// [`PanAxis::Aligned`]) across the scale updates of one interaction
/// produces. `Rc`-shared into the closures `build` hands to `GestureDetector`
/// so it survives from `on_pan_start` through the matching `on_pan_end`, even
/// across a rebuild that swaps in fresh closures mid-gesture.
#[derive(Debug)]
struct GestureTracking {
    /// Viewport-local position at the most recent `on_pan_start`. `None`
    /// between gestures.
    pan_start_local: Cell<Option<Offset<f64>>>,
    /// The axis a [`PanAxis::Aligned`] drag has locked to, established from
    /// the first non-zero movement of the gesture. `None` before that, and
    /// reset to `None` at the end of every gesture.
    current_axis: Cell<Option<Axis>>,
    scale: Cell<f64>,
    rotation: Cell<f64>,
}

#[derive(Debug)]
struct FocalFling {
    controller: AnimationController,
    listener: RefCell<Option<(ListenerId, Arc<AtomicBool>)>>,
    closed: Cell<bool>,
    enabled: Cell<bool>,
}

impl FocalFling {
    fn new() -> Self {
        Self {
            controller: AnimationController::unbounded_without_ticker(Duration::from_millis(1)),
            listener: RefCell::new(None),
            closed: Cell::new(false),
            enabled: Cell::new(false),
        }
    }

    fn stop(&self) {
        let listener = self.listener.borrow_mut().take();
        if let Some((id, live)) = listener {
            live.store(false, Ordering::Release);
            self.controller.remove_listener(id);
        }
        let _ = self.controller.stop();
    }

    fn start(
        &self,
        target: TransformationController,
        velocity: Offset<f64>,
        viewport: Rect<f64>,
        boundary: Rect<f64>,
    ) {
        self.stop();
        let speed = velocity.dx.hypot(velocity.dy);
        if self.closed.get() || !self.enabled.get() || !speed.is_finite() || speed <= 0.0 {
            return;
        }
        // Ten percent of the release speed remains after one second. The
        // library simulation supplies finite admission and its rest threshold.
        let Ok(simulation) = FrictionSimulation::new(0.1, 0.0, speed, Tolerance::DEFAULT) else {
            return;
        };
        let origin = target.value();
        let direction = velocity / speed;
        let live = Arc::new(AtomicBool::new(true));
        let callback_live = live.clone();
        let animation = self.controller.clone();
        let id = self.controller.add_listener(Arc::new(move || {
            if !callback_live.load(Ordering::Acquire) {
                return;
            }
            let delta = direction * animation.value();
            let proposed = Matrix4::translation(delta.dx, delta.dy, 0.0) * origin;
            let Some(next) = contain_transform(proposed, viewport, boundary) else {
                let _ = animation.stop();
                return;
            };
            if next == target.value() && animation.value() != 0.0 {
                let _ = animation.stop();
            } else {
                target.set_value(next);
            }
        }));
        *self.listener.borrow_mut() = Some((id, live));
        let _ = self.controller.animate_with(simulation);
    }

    fn close(&self) {
        self.closed.set(true);
        self.stop();
        self.controller.dispose();
    }
}

/// Persistent state for [`InteractiveViewer`].
#[derive(Debug)]
pub struct InteractiveViewerState {
    writer: Option<WriterSource>,
    /// Publishes the child's `RenderId` while mounted — see
    /// [`geometry`](Self::geometry) for why one anchor is enough in V1.
    subtree_anchor: SubtreeAnchor,
    gesture: Rc<GestureTracking>,
    /// Acquired once in [`init_state`](ViewState::init_state), not `build`
    /// (lifecycle-only capability): `build` runs on
    /// every rebuild, and the gesture closures below only ever read this
    /// asynchronously, from a later `on_pan_update`/`on_pointer_signal`
    /// callback, never synchronously inside `build` itself — the same
    /// acquire-in-lifecycle-hook, use-from-callback shape
    /// `FocusState::init_state` uses for its own pipeline handle.
    pipeline_cell: Option<PipelineCell>,
    fling: Rc<FocalFling>,
    vsync: Option<Vsync>,
    vsync_registration: Option<VsyncRegistration>,
}

impl ViewState<InteractiveViewer> for InteractiveViewerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.pipeline_cell = ctx.pipeline_owner();
        self.writer = Some(ctx.writer_source());
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.vsync_registration = Some(vsync.register(self.fling.controller.clone()));
            self.vsync = Some(vsync);
            self.fling.enabled.set(true);
        }
    }

    fn did_update_view(&mut self, old: &InteractiveViewer, view: &InteractiveViewer) {
        if !old.controller.ptr_eq(&view.controller) {
            self.fling.stop();
        }
    }

    fn dispose(&mut self) {
        if let Some(registration) = self.vsync_registration.take()
            && let Some(vsync) = self.vsync.take()
        {
            vsync.unregister(&registration);
        }
        self.fling.close();
    }

    #[expect(clippy::too_many_lines)] // one gesture-wiring build(); splitting fragments the callback capture set
    fn build(&self, view: &InteractiveViewer, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = view.controller.clone();
        let anchor = self.subtree_anchor.clone();
        let gesture = Rc::clone(&self.gesture);
        let pipeline_cell = self.pipeline_cell.clone();
        let boundary_margin = view.boundary_margin;
        let min_scale = view.min_scale;
        let max_scale = view.max_scale;
        let pan_enabled = view.pan_enabled;
        let scale_enabled = view.scale_enabled;
        let rotation_enabled = view.rotation_enabled;
        let fling = self.fling.clone();
        let wheel_scale_gate = view.wheel_scale_gate;
        let pan_axis = view.pan_axis;
        let scale_factor = view.scale_factor;
        let clip_behavior = view.clip_behavior;
        let alignment = view.alignment;
        let child = view.child.clone();
        let on_start = view.on_interaction_start.clone();
        let on_update = view.on_interaction_update.clone();
        let on_end = view.on_interaction_end.clone();
        let writer = self
            .writer
            .clone()
            .expect("BUG: InteractiveViewer initialized before build");

        let listenable = controller.as_listenable();

        AnimatedBuilder::new(listenable, move || {
            let matrix = controller.value();

            // -- Pan (GestureDetector) -------------------------------------
            let gesture_start = Rc::clone(&gesture);
            let on_start_pan = on_start.clone();
            let fling_start = fling.clone();
            let pan_start_details = callback_with(move |cx, details: ScaleStartDetails| {
                fling_start.stop();
                gesture_start.current_axis.set(None);
                gesture_start.scale.set(1.0);
                gesture_start.rotation.set(0.0);
                gesture_start
                    .pan_start_local
                    .set(Some(details.local_focal_point));
                if let Some(callback) = &on_start_pan {
                    callback(
                        cx,
                        InteractionStartDetails {
                            focal_point: details.focal_point,
                            local_focal_point: details.local_focal_point,
                        },
                    );
                }
            });

            let gesture_update = Rc::clone(&gesture);
            let controller_update = controller.clone();
            let anchor_update = anchor.clone();
            let pipeline_cell_update = pipeline_cell.clone();
            let on_update_pan = on_update.clone();
            let pan_update_details = callback_with(move |cx, details: ScaleUpdateDetails| {
                let before = controller_update.value();
                let ratio = details.scale / gesture_update.scale.replace(details.scale);
                let rotation = details.rotation - gesture_update.rotation.replace(details.rotation);
                let mut delta = details.focal_point_delta;
                if pan_enabled {
                    if let Some(start_local) = gesture_update.pan_start_local.get()
                        && gesture_update.current_axis.get().is_none()
                        && pan_axis != PanAxis::Free
                    {
                        let total = details.local_focal_point - start_local;
                        if total != Offset::ZERO {
                            gesture_update.current_axis.set(Some(dominant_axis(total)));
                        }
                    }
                    delta = match pan_axis {
                        PanAxis::Free => delta,
                        PanAxis::Horizontal => align_to_axis(delta, Axis::Horizontal),
                        PanAxis::Vertical => align_to_axis(delta, Axis::Vertical),
                        PanAxis::Aligned => gesture_update
                            .current_axis
                            .get()
                            .map_or(delta, |axis| align_to_axis(delta, axis)),
                    };
                } else {
                    delta = Offset::ZERO;
                }
                if let Some((viewport, boundary)) = InteractiveViewerState::geometry(
                    pipeline_cell_update.as_ref(),
                    &anchor_update,
                    boundary_margin,
                ) && let Some(next) = gesture_transform(
                    before,
                    details.local_focal_point,
                    delta,
                    if scale_enabled { ratio } else { 1.0 },
                    if rotation_enabled { rotation } else { 0.0 },
                    min_scale,
                    max_scale,
                    viewport,
                    boundary,
                ) {
                    controller_update.set_value(next);
                }
                if let Some(callback) = &on_update_pan {
                    callback(
                        cx,
                        InteractionUpdateDetails {
                            focal_point: details.focal_point,
                            local_focal_point: details.local_focal_point,
                            scale: uniform_scale(&controller_update.value())
                                / uniform_scale(&before),
                            focal_point_delta: delta,
                        },
                    );
                }
            });

            let gesture_end = Rc::clone(&gesture);
            let on_end_pan = on_end.clone();
            let fling_end = fling.clone();
            let controller_end = controller.clone();
            let anchor_end = anchor.clone();
            let pipeline_end = pipeline_cell.clone();
            let pan_end_details = callback_with(move |cx, details: ScaleEndDetails| {
                let mut velocity = details.focal_velocity.pixels_per_second;
                velocity = match pan_axis {
                    PanAxis::Free => velocity,
                    PanAxis::Horizontal => align_to_axis(velocity, Axis::Horizontal),
                    PanAxis::Vertical => align_to_axis(velocity, Axis::Vertical),
                    PanAxis::Aligned => gesture_end
                        .current_axis
                        .get()
                        .map_or(velocity, |axis| align_to_axis(velocity, axis)),
                };
                gesture_end.pan_start_local.set(None);
                gesture_end.current_axis.set(None);
                gesture_end.scale.set(1.0);
                gesture_end.rotation.set(0.0);
                if pan_enabled
                    && let Some((viewport, boundary)) = InteractiveViewerState::geometry(
                        pipeline_end.as_ref(),
                        &anchor_end,
                        boundary_margin,
                    )
                {
                    fling_end.start(controller_end.clone(), velocity, viewport, boundary);
                }
                if let Some(callback) = &on_end_pan {
                    callback(
                        cx,
                        InteractionEndDetails {
                            reason: GestureEndReason::Completed,
                            velocity: details.focal_velocity,
                            scale_velocity: details.velocity,
                        },
                    );
                }
            });

            // -- Wheel scale (Listener::on_scroll_claim) -------------------
            //
            // The viewer goes through the arbitrated claim walk rather than
            // acting on the raw pointer signal: acting directly would make a
            // viewer nested in a scrollable both zoom and scroll on one wheel
            // tick. When the tick will actually zoom, the viewer claims it and
            // the outer scrollable stays still.
            let controller_wheel = controller.clone();
            let anchor_wheel = anchor.clone();
            let pipeline_cell_wheel = pipeline_cell.clone();
            let on_start_wheel = on_start.clone();
            let on_update_wheel = on_update.clone();
            let on_end_wheel = on_end.clone();
            let wheel_writer = writer.clone();
            let wheel_fling = fling.clone();
            let scroll_claim = move |data: &ScrollEvent| {
                wheel_writer.write(|cx| {
                    if wheel_scale_gate == WheelScaleGate::CtrlWheel
                        && !data.modifiers.contains(Modifiers::CONTROL)
                    {
                        // Plain ticks belong to an enclosing scrollable under
                        // the ctrl-gated contract.
                        return EventPropagation::Continue;
                    }
                    let geometry = InteractiveViewerState::geometry(
                        pipeline_cell_wheel.as_ref(),
                        &anchor_wheel,
                        boundary_margin,
                    );
                    let pixels_per_unit = match data.delta.unit() {
                        ScrollUnit::Pixels => 1.0,
                        // Raw rotation and already normalized lines both retain
                        // the authored zoom step. Scroll preferences govern
                        // translation, independently of the zoom divisor.
                        ScrollUnit::Detents | ScrollUnit::Lines => 53.0,
                        ScrollUnit::Pages => {
                            let Some((viewport, _)) = geometry else {
                                return EventPropagation::Continue;
                            };
                            let height = viewport.height();
                            if !height.is_finite() || height <= 0.0 {
                                return EventPropagation::Continue;
                            }
                            height
                        }
                        _ => return EventPropagation::Continue,
                    };
                    let delta = data.delta.y() * pixels_per_unit;
                    if delta == 0.0 || !delta.is_finite() {
                        // Ignore horizontal-only wheel scroll.
                        return EventPropagation::Continue;
                    }

                    let position = data.position.get();
                    let position = Offset::new(position.x, position.y);
                    let scale_change = (-delta / scale_factor).exp();
                    if !scale_change.is_finite() || scale_change <= 0.0 {
                        return EventPropagation::Continue;
                    }
                    wheel_fling.stop();
                    if let Some(callback) = &on_start_wheel {
                        callback(
                            cx,
                            InteractionStartDetails {
                                focal_point: position,
                                local_focal_point: position,
                            },
                        );
                    }

                    let value_before_zoom = controller_wheel.value();
                    if scale_enabled && let Some((viewport, boundary)) = geometry {
                        let scene_before = controller_wheel.to_scene(position);
                        let scaled = clamp_scale(
                            controller_wheel.value(),
                            scale_change,
                            min_scale,
                            max_scale,
                            viewport,
                            boundary,
                        );
                        controller_wheel.set_value(scaled);

                        // Keep the same scene point under the cursor before and
                        // after the scale.
                        let scene_after = controller_wheel.to_scene(position);
                        let correction = Offset::new(
                            scene_after.dx - scene_before.dx,
                            scene_after.dy - scene_before.dy,
                        );
                        let translated = clamp_translation(
                            controller_wheel.value(),
                            correction,
                            viewport,
                            boundary,
                        );
                        controller_wheel.set_value(translated);
                    }

                    if let Some(callback) = &on_update_wheel {
                        callback(
                            cx,
                            InteractionUpdateDetails {
                                focal_point: position,
                                local_focal_point: position,
                                scale: scale_change,
                                focal_point_delta: Offset::ZERO,
                            },
                        );
                    }
                    if let Some(callback) = &on_end_wheel {
                        callback(
                            cx,
                            InteractionEndDetails {
                                reason: GestureEndReason::Completed,
                                velocity: Velocity::ZERO,
                                scale_velocity: 0.0,
                            },
                        );
                    }
                    // Claim only when the viewer actually zoomed — the same
                    // shape as the scrollable's can-move predicate. Scaling
                    // disabled, or a zoom the boundary/min/max clamp collapsed
                    // to a no-op (e.g. zoom-out at identity with a zero
                    // boundary margin), leaves the tick to an enclosing
                    // scrollable; the interaction callbacks above still observed
                    // it even then.
                    if controller_wheel.value().m == value_before_zoom.m {
                        EventPropagation::Continue
                    } else {
                        EventPropagation::Stop
                    }
                })
            };

            let mut transform = Transform::new(matrix);
            if let Some(inner_child) = child.clone().into_inner() {
                transform = transform.child(AnchoredBox::new(anchor.clone(), inner_child));
            }
            if let Some(alignment) = alignment {
                transform = transform.alignment(alignment);
            }

            let clipped = ClipRect::new()
                .clip_behavior(clip_behavior)
                .child(transform);

            // Reject an idle native claimant before it can mutate its actor.
            // Once admitted, GestureDetector owns the source through terminal.
            let controller_pinch = controller.clone();
            let anchor_pinch = anchor.clone();
            let pipeline_cell_pinch = pipeline_cell.clone();
            let native_admission = move |event: &PanZoomEvent| {
                let PanZoomPhase::Update(transform) = event.phase else {
                    return false;
                };
                let Some((viewport, boundary)) = InteractiveViewerState::geometry(
                    pipeline_cell_pinch.as_ref(),
                    &anchor_pinch,
                    boundary_margin,
                ) else {
                    return false;
                };
                let point = event.position.get();
                let delta = if pan_enabled {
                    match pan_axis {
                        PanAxis::Horizontal => align_to_axis(transform.pan(), Axis::Horizontal),
                        PanAxis::Vertical => align_to_axis(transform.pan(), Axis::Vertical),
                        PanAxis::Aligned if transform.pan() != Offset::ZERO => {
                            align_to_axis(transform.pan(), dominant_axis(transform.pan()))
                        }
                        _ => transform.pan(),
                    }
                } else {
                    Offset::ZERO
                };
                let focal = Offset::new(point.x, point.y) + transform.pan();
                let before = controller_pinch.value();
                gesture_transform(
                    before,
                    focal,
                    delta,
                    if scale_enabled {
                        transform.scale()
                    } else {
                        1.0
                    },
                    if rotation_enabled {
                        transform.rotation()
                    } else {
                        0.0
                    },
                    min_scale,
                    max_scale,
                    viewport,
                    boundary,
                )
                .is_some_and(|next| next != before)
            };
            let cancel_gesture = gesture.clone();
            let cancel_fling = fling.clone();
            let cancel_end = on_end.clone();
            let recognized = GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .scale_start_mode(ScaleStartMode::PanOrScale)
                .native_scale_admission(native_admission)
                .on_scale_start(pan_start_details)
                .on_scale_update(pan_update_details)
                .on_scale_end(pan_end_details)
                .on_scale_cancel(callback(move |cx| {
                    cancel_fling.stop();
                    cancel_gesture.pan_start_local.set(None);
                    cancel_gesture.current_axis.set(None);
                    cancel_gesture.scale.set(1.0);
                    cancel_gesture.rotation.set(0.0);
                    if let Some(callback) = &cancel_end {
                        callback(
                            cx,
                            InteractionEndDetails {
                                reason: GestureEndReason::Cancelled,
                                velocity: Velocity::ZERO,
                                scale_velocity: 0.0,
                            },
                        );
                    }
                }))
                .child(clipped);
            let input_fling = fling.clone();
            Listener::new()
                .on_pointer_down(move |_, _| input_fling.stop())
                .on_scroll_claim(scroll_claim)
                .child(recognized)
        })
    }
}

impl InteractiveViewerState {
    /// The viewport rect and the boundary rect, in scene coordinates, or
    /// `None` before the child is mounted and laid out.
    ///
    /// Associated function rather than a `&self` method: the gesture
    /// closures built in [`build`](ViewState::build) capture
    /// `pipeline_cell`/`anchor`/`boundary_margin` by clone (they must be
    /// `'static`, so they cannot borrow the `ViewState`), and call this with
    /// those clones instead.
    ///
    /// V1 only supports `constrained: true`, under which
    /// `Listener`/`GestureDetector`/`ClipRect`/`Transform` are all
    /// layout-transparent proxies that adopt the child's own size — nothing
    /// between this widget's box and the child imposes a different size. So
    /// the **viewport** rect and the child's own unmargined rect (the base
    /// inflated by `boundary_margin` to get the boundary rect) are the same
    /// rectangle, so one `subtree_anchor` field serves both.
    fn geometry(
        pipeline_cell: Option<&PipelineCell>,
        anchor: &SubtreeAnchor,
        boundary_margin: EdgeInsets,
    ) -> Option<(Rect<f64>, Rect<f64>)> {
        let owner = pipeline_cell?;
        let render_id = anchor.get()?;
        let size = owner.with(|owner| owner.box_size(render_id))?;
        let rect = Rect::from_origin_size(Point::new(0.0, 0.0), size);
        Some((rect, boundary_margin.inflate_rect(rect)))
    }
}

// ============================================================================
// Matrix math — boundary-clamped translate/scale
// ============================================================================

/// Compose in viewport space, then keep the same scene point under the moving
/// focal point. Publish only an invertible finite matrix inside the boundary.
#[expect(clippy::too_many_arguments)]
fn gesture_transform(
    matrix: Matrix4,
    focal: Offset<f64>,
    delta: Offset<f64>,
    scale: f64,
    rotation: f64,
    min_scale: f64,
    max_scale: f64,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Option<Matrix4> {
    if !focal.is_finite()
        || !delta.is_finite()
        || scale.is_nan()
        || scale < 0.0
        || !rotation.is_finite()
        || !viewport.is_finite()
        || viewport.width() <= 0.0
        || viewport.height() <= 0.0
    {
        return None;
    }
    if delta == Offset::ZERO && scale == 1.0 && rotation == 0.0 {
        return contain_transform(matrix, viewport, boundary);
    }
    let previous_focal = focal - delta;
    if !previous_focal.is_finite() {
        return None;
    }
    let inverse = matrix.try_inverse()?;
    let pivot = inverse.transform_point(previous_focal.dx, previous_focal.dy);
    if !pivot.0.is_finite() || !pivot.1.is_finite() {
        return None;
    }
    let current_scale = uniform_scale(&matrix);
    if !current_scale.is_finite() || current_scale <= 0.0 {
        return None;
    }
    let scaled = clamp_scale(matrix, scale, min_scale, max_scale, viewport, boundary);
    let mut next = Matrix4::rotation_z(rotation) * scaled;
    let mapped = next.transform_point(pivot.0, pivot.1);
    let correction = focal - Offset::new(mapped.0, mapped.1);
    if !correction.is_finite() {
        return None;
    }
    next = Matrix4::translation(correction.dx, correction.dy, 0.0) * next;
    contain_transform(next, viewport, boundary)
}

fn contain_transform(
    mut matrix: Matrix4,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Option<Matrix4> {
    if !matrix
        .to_col_major_array()
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let values = matrix.to_col_major_array();
    if boundary.is_finite()
        && values[3] == 0.0
        && values[7] == 0.0
        && values[11] == 0.0
        && values[15] == 1.0
    {
        // Separate translation from the viewport's shape. Subtracting a huge
        // excess from a huge translation loses the small boundary coordinate.
        let (x, y, z) = matrix.translation_component();
        let mut linear = matrix;
        linear.set_translation(0.0, 0.0, 0.0);
        let inverse = linear.try_inverse()?;
        let shape = inverse.transform_rect(&viewport);
        if !shape.is_finite()
            || shape.width() > boundary.width() + EXCESS_EPSILON
            || shape.height() > boundary.height() + EXCESS_EPSILON
        {
            return None;
        }
        let shift = inverse.transform_point(-x, -y);
        if !shift.0.is_finite() || !shift.1.is_finite() {
            return None;
        }
        let lower = Offset::new(boundary.min.x - shape.min.x, boundary.min.y - shape.min.y);
        let upper = Offset::new(boundary.max.x - shape.max.x, boundary.max.y - shape.max.y);
        if !lower.is_finite() || !upper.is_finite() {
            return None;
        }
        let clamped = Offset::new(
            shift.0.clamp(lower.dx, upper.dx.max(lower.dx)),
            shift.1.clamp(lower.dy, upper.dy.max(lower.dy)),
        );
        if clamped.dx != shift.0 || clamped.dy != shift.1 {
            let translated = linear.transform_point(-clamped.dx, -clamped.dy);
            if !translated.0.is_finite() || !translated.1.is_finite() {
                return None;
            }
            matrix.set_translation(translated.0, translated.1, z);
        }
    }
    let inverse = matrix.try_inverse()?;
    let scene_viewport = inverse.transform_rect(&viewport);
    if !scene_viewport.is_finite() {
        return None;
    }
    if !boundary.is_finite() {
        return (![
            boundary.min.x,
            boundary.min.y,
            boundary.max.x,
            boundary.max.y,
        ]
        .iter()
        .any(|value| value.is_nan()))
        .then_some(matrix);
    }
    // A rotated viewport is a quad; its bounding rectangle fits this
    // axis-aligned scene boundary exactly when all four corners fit. If its
    // extent cannot fit at the admitted scale, reject rather than invent zoom.
    if scene_viewport.width() > boundary.width() + EXCESS_EPSILON
        || scene_viewport.height() > boundary.height() + EXCESS_EPSILON
    {
        return None;
    }
    let excess = rect_excess(boundary, scene_viewport);
    if !excess.is_finite() {
        return None;
    }
    if !excess_is_negligible(excess) {
        matrix *= Matrix4::translation(-excess.dx, -excess.dy, 0.0);
    }
    let result = matrix.try_inverse()?.transform_rect(&viewport);
    (matrix
        .to_col_major_array()
        .iter()
        .all(|value| value.is_finite())
        && result.is_finite()
        && excess_is_negligible(rect_excess(boundary, result)))
    .then_some(matrix)
}

/// Uniform scale of a translated/rotated matrix: length of its x basis vector.
fn uniform_scale(matrix: &Matrix4) -> f64 {
    let m = matrix.to_col_major_array();
    m[0].hypot(m[1])
}

/// Transforms `viewport`'s four corners by the inverse of `matrix` and
/// returns their axis-aligned bounding box — the viewport's rect in scene
/// coordinates after the child has been transformed by `matrix`. Falls back
/// to `viewport` unchanged if `matrix` is singular (should not happen for a
/// translation + uniform-scale matrix with a non-zero scale).
fn transform_viewport(matrix: Matrix4, viewport: Rect<f64>) -> Rect<f64> {
    match matrix.try_inverse() {
        Some(inverse) => inverse.transform_rect(&viewport),
        None => viewport,
    }
}

/// How far `[view_min, view_max]` lies outside `[bound_min, bound_max]` along
/// one axis, signed so that adding it to the viewport's position moves it
/// back inside the boundary. Zero when already inside (inclusive).
///
/// The caller supplies the transformed viewport's bounding rectangle, so the
/// interval comparison also covers rotation. If the viewport is wider than
/// the boundary, the edge with the larger-magnitude excess wins.
fn axis_excess(view_min: f64, view_max: f64, bound_min: f64, bound_max: f64) -> f64 {
    let excess_min = if view_min < bound_min {
        bound_min - view_min
    } else {
        0.0
    };
    let excess_max = if view_max > bound_max {
        bound_max - view_max
    } else {
        0.0
    };
    if excess_min.abs() >= excess_max.abs() {
        excess_min
    } else {
        excess_max
    }
}

fn rect_excess(boundary: Rect<f64>, viewport: Rect<f64>) -> Offset<f64> {
    Offset::new(
        axis_excess(
            viewport.min.x,
            viewport.max.x,
            boundary.min.x,
            boundary.max.x,
        ),
        axis_excess(
            viewport.min.y,
            viewport.max.y,
            boundary.min.y,
            boundary.max.y,
        ),
    )
}

/// Floating-point tolerance for the "did this transform round-trip produce
/// zero excess" checks in [`clamp_translation`].
///
/// The viewport's inverse-then-transform round trip leaves residue that
/// *should* be exactly zero but isn't once the matrix carries a non-unit (and
/// non-power-of-two) scale. This snaps anything within `EXCESS_EPSILON` of
/// zero back to exactly zero. Chosen against the scale of one gesture's excess (tens to
/// thousands of pixels) rather than absolute machine epsilon — comfortably
/// larger than the ~1e-4 residue a `scale * (a - b)` round trip leaves at
/// these magnitudes, comfortably smaller than any excess a real boundary
/// hit produces.
const EXCESS_EPSILON: f64 = 1e-3;

/// Whether a single excess component is within [`EXCESS_EPSILON`] of zero.
fn is_negligible(component: f64) -> bool {
    component.abs() < EXCESS_EPSILON
}

/// Whether `excess` is within [`EXCESS_EPSILON`] of `Offset::ZERO` on both
/// axes — the round-trip-tolerant replacement for `excess == Offset::ZERO`.
fn excess_is_negligible(excess: Offset<f64>) -> bool {
    is_negligible(excess.dx) && is_negligible(excess.dy)
}

/// Locks a `PanAxis::Aligned` drag to whichever axis dominates `delta`.
/// `delta` must be non-zero (callers only invoke this on real movement).
fn dominant_axis(delta: Offset<f64>) -> Axis {
    if delta.dx.abs() > delta.dy.abs() {
        Axis::Horizontal
    } else {
        Axis::Vertical
    }
}

/// Zeroes out the off-axis component of `delta`.
fn align_to_axis(delta: Offset<f64>, axis: Axis) -> Offset<f64> {
    match axis {
        Axis::Horizontal => Offset::new(delta.dx, 0.0),
        Axis::Vertical => Offset::new(0.0, delta.dy),
    }
}

/// Applies `translation` (in scene units) to `matrix`, clamped so the
/// transformed viewport stays within `boundary` when `boundary` is finite.
///
/// Composed via
/// `matrix * Matrix4::translation(..)` (post-multiply — the translation
/// happens in the matrix's own local/scene space before the rest of the
/// transform is applied). `flui_foundation::geometry`'s
/// own `Matrix4::translate` mutator has the *opposite* (pre-multiply,
/// global-space) convention and must not be used here.
fn clamp_translation(
    matrix: Matrix4,
    translation: Offset<f64>,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Matrix4 {
    if translation == Offset::ZERO {
        return matrix;
    }
    let next = matrix * Matrix4::translation(translation.dx, translation.dy, 0.0);

    if !boundary.is_finite() {
        return next;
    }

    let next_viewport = transform_viewport(next, viewport);
    let excess = rect_excess(boundary, next_viewport);
    if excess_is_negligible(excess) {
        return next;
    }

    let (next_tx, next_ty, next_tz) = next.translation_component();
    let current_scale = uniform_scale(&matrix);
    let corrected_tx = next_tx - excess.dx * current_scale;
    let corrected_ty = next_ty - excess.dy * current_scale;
    let mut corrected = matrix;
    corrected.set_translation(corrected_tx, corrected_ty, next_tz);

    let corrected_viewport = transform_viewport(corrected, viewport);
    let corrected_excess = rect_excess(boundary, corrected_viewport);
    if excess_is_negligible(corrected_excess) {
        return corrected;
    }

    if !is_negligible(corrected_excess.dx) && !is_negligible(corrected_excess.dy) {
        // Neither axis fits at all (the viewport is larger than the
        // boundary in both directions): no translation.
        return matrix;
    }

    let unidirectional_tx = if is_negligible(corrected_excess.dx) {
        corrected_tx
    } else {
        0.0
    };
    let unidirectional_ty = if is_negligible(corrected_excess.dy) {
        corrected_ty
    } else {
        0.0
    };
    let mut result = matrix;
    result.set_translation(unidirectional_tx, unidirectional_ty, next_tz);
    result
}

/// Applies `scale` (a multiplicative change) to `matrix`, clamped so the
/// resulting overall scale stays within `[min_scale, max_scale]` and never
/// shrinks the child so much it can't cover `boundary` from `viewport`.
///
/// Composed via
/// `matrix * Matrix4::scaling(..)` — see [`clamp_translation`]'s doc for why
/// the mutating `Matrix4::scale` method is the wrong tool here.
fn clamp_scale(
    matrix: Matrix4,
    scale: f64,
    min_scale: f64,
    max_scale: f64,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Matrix4 {
    if scale == 1.0 {
        return matrix;
    }
    // Debug-only: this does not replace `clamp_double`'s non-panicking
    // behavior below; it only surfaces the misconfiguration in debug builds.
    debug_assert!(
        max_scale >= min_scale,
        "InteractiveViewer: max_scale ({max_scale}) must be >= min_scale ({min_scale})"
    );
    let current_scale = uniform_scale(&matrix);
    // Finite / infinite (unbounded boundary) is naturally 0.0 here — no
    // separate infinite-boundary branch needed.
    let boundary_floor =
        (viewport.width() / boundary.width()).max(viewport.height() / boundary.height());
    let total_scale = (current_scale * scale).max(boundary_floor);
    let clamped_total = clamp_double(total_scale, min_scale, max_scale);
    let applied = clamped_total / current_scale;
    matrix * Matrix4::scaling(applied, applied, applied)
}

/// Unlike `f64::clamp`
/// (which panics — in every build profile, not just debug — whenever `min >
/// max`), this never panics: a misconfigured `min_scale > max_scale` falls
/// through (the assertion above is debug-only) instead of crashing a release build over a caller error that
/// should have been caught in testing.
fn clamp_double(x: f64, min: f64, max: f64) -> f64 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}
