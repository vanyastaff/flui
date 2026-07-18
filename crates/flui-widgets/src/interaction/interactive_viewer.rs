//! [`InteractiveViewer`] — pans and zooms its child through a transformation
//! matrix.
//!
//! Flutter parity: `widgets/interactive_viewer.dart` (tag `3.44.0`). The
//! contract this ports: a single [`TransformationController`]-held
//! [`Matrix4`] maps the child's scene coordinates to viewport coordinates;
//! gestures update that matrix; `min_scale`/`max_scale` clamp the zoom level;
//! `boundary_margin` constrains how far the transformed viewport may drift
//! from the child's own rect (an all-infinite margin removes the boundary
//! entirely); `pan_enabled`/`scale_enabled` gate whether a gesture is allowed
//! to *apply*, though `on_interaction_*` callbacks still fire regardless
//! (matching Flutter's documented "will be called even if the interaction is
//! disabled" contract).
//!
//! # Scope of this port (V1)
//!
//! - **Pan** is wired through [`GestureDetector::on_pan_start`]/`on_pan_update`/
//!   `on_pan_end` — a genuine single-pointer drag, dispatched and slop-tested
//!   through the real gesture arena in tests.
//! - **Scale** is wired through [`Listener::on_pointer_signal`] — a real mouse
//!   wheel / discrete-scroll event, matching Flutter's `_receivedPointerSignal`
//!   mouse-wheel branch (`scaleChange = exp(-scrollDelta.dy / scaleFactor)`).
//! - **Pinch-to-zoom and two-finger rotation are out of scope.** Flutter
//!   recognizes them through `GestureDetector`'s combined scale gesture
//!   (`onScaleStart`/`onScaleUpdate`/`onScaleEnd`, fed by two simultaneous
//!   pointers); FLUI's `GestureDetector` has no such recognizer yet — this is
//!   a framework-level gap, not merely a test-harness one. Rotation is in the
//!   same position Flutter's own upstream is: `_rotateEnabled` is hardcoded
//!   `false` in the oracle too (`interactive_viewer.dart` — rotation is
//!   unimplemented pending flutter/flutter#57698), so dropping the
//!   `Quad`/rotation-aware boundary math it would otherwise need is a
//!   faithful simplification, not a cut corner: with rotation permanently
//!   off, the general `Quad` axis-aligned-bounding-box algorithm and the
//!   plain-`Rect` containment math below produce identical boundary
//!   decisions.
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
//!   (mirroring Flutter's `_parentKey` vs. `_childKey` split) becomes load
//!   bearing again.
//! - **Inertia/fling after a pan release** (Flutter's `FrictionSimulation` in
//!   `_onScaleEnd`) is deferred — `on_interaction_end` fires with the
//!   release velocity, but no animation follows it. Needs an
//!   `AnimationController`/`Vsync` wiring pass of its own.
//!
//! `on_interaction_start`/`on_interaction_update`/`on_interaction_end` carry
//! FLUI's own detail types ([`InteractionStartDetails`] etc.), not a literal
//! port of Flutter's `ScaleStartDetails`/`ScaleUpdateDetails`/`ScaleEndDetails`
//! — those describe a combined pan+scale+rotate gesture this port does not
//! recognize as one gesture. The shape here carries what pan and wheel-scale
//! actually produce: a focal point, a scale multiplier (1.0 for a pure pan),
//! a translation delta, and a release velocity.

use std::cell::Cell;
use std::rc::Rc;

use flui_geometry::{Matrix4, px};
use flui_interaction::events::ScrollEventData;
use flui_interaction::{DragEndDetails, DragStartDetails, DragUpdateDetails};
use flui_objects::SubtreeAnchor;
use flui_rendering::hit_testing::{HitTestBehavior, PointerEvent};
use flui_rendering::pipeline::PipelineOwner;
use flui_types::geometry::Pixels;
use flui_types::gestures::Velocity;
use flui_types::painting::Clip;
use flui_types::{Alignment, Axis, EdgeInsets, Offset, Point, Rect};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{Child, IntoView, View, ViewState};
use parking_lot::RwLock;

use crate::navigator::AnchoredBox;
use crate::{AnimatedBuilder, ClipRect, GestureDetector, Listener, Transform};

use super::transformation_controller::TransformationController;

// ============================================================================
// PanAxis
// ============================================================================

/// Constrains which axis (or axes) [`InteractiveViewer`] pans along.
///
/// Flutter parity: `widgets/interactive_viewer.dart` `PanAxis`.
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
    pub focal_point: Offset<Pixels>,
    /// The interaction's focal point, in the coordinates of
    /// `InteractiveViewer` itself (viewport-local).
    pub local_focal_point: Offset<Pixels>,
}

/// Details passed to `on_interaction_update`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionUpdateDetails {
    /// The interaction's current focal point, in the coordinates of the
    /// widget that contains `InteractiveViewer`.
    pub focal_point: Offset<Pixels>,
    /// The interaction's current focal point, in `InteractiveViewer`'s own
    /// (viewport-local) coordinates.
    pub local_focal_point: Offset<Pixels>,
    /// The multiplicative scale change applied by this update. `1.0` for a
    /// pure pan update (no scale change).
    pub scale: f32,
    /// The translation applied by this update, in viewport pixels. Zero for
    /// a pure wheel-scale update.
    pub focal_point_delta: Offset<Pixels>,
}

/// Details passed to `on_interaction_end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionEndDetails {
    /// The gesture's release velocity. [`Velocity::ZERO`] for a discrete
    /// wheel-scale interaction (there is no release to measure).
    pub velocity: Velocity,
}

type StartCallback = Rc<dyn Fn(InteractionStartDetails)>;
type UpdateCallback = Rc<dyn Fn(InteractionUpdateDetails)>;
type EndCallback = Rc<dyn Fn(InteractionEndDetails)>;

// ============================================================================
// InteractiveViewer
// ============================================================================

/// Pans and zooms `child` through a [`TransformationController`]-held
/// [`Matrix4`].
///
/// See the module docs for the exact scope this V1 port covers.
#[derive(Clone)]
pub struct InteractiveViewer {
    controller: TransformationController,
    boundary_margin: EdgeInsets,
    min_scale: f32,
    max_scale: f32,
    pan_enabled: bool,
    scale_enabled: bool,
    pan_axis: PanAxis,
    scale_factor: f32,
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
            .field("pan_axis", &self.pan_axis)
            .finish_non_exhaustive()
    }
}

impl Default for InteractiveViewer {
    fn default() -> Self {
        Self {
            controller: TransformationController::new(),
            boundary_margin: EdgeInsets::ZERO,
            // Eyeballed defaults, matching Flutter's own (`minScale: 0.8`,
            // `maxScale: 2.5`) — reasonable limits for common use cases.
            min_scale: 0.8,
            max_scale: 2.5,
            pan_enabled: true,
            scale_enabled: true,
            pan_axis: PanAxis::Free,
            // Flutter parity: `kDefaultMouseScrollToScaleFactor`.
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
    /// A new `InteractiveViewer` with Flutter's default limits (`minScale:
    /// 0.8`, `maxScale: 2.5`, zero boundary margin, pan and wheel-scale both
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
    /// boundary is clamped at the boundary. Pass `EdgeInsets::all(f32::
    /// INFINITY)` for no boundary at all.
    ///
    /// # Precondition
    ///
    /// Every edge must be finite, or every edge must be infinite — not a mix
    /// (checked with `debug_assert!`, matching Flutter's own `assert`, which
    /// is likewise debug-only).
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
    pub fn min_scale(mut self, min_scale: f32) -> Self {
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
    pub fn max_scale(mut self, max_scale: f32) -> Self {
        debug_assert!(
            max_scale > 0.0 && !max_scale.is_nan(),
            "InteractiveViewer::max_scale must be greater than zero"
        );
        self.max_scale = max_scale;
        self
    }

    /// If `false`, single-pointer drags do not pan the child.
    /// `on_interaction_*` callbacks still fire (Flutter parity).
    #[must_use]
    pub fn pan_enabled(mut self, pan_enabled: bool) -> Self {
        self.pan_enabled = pan_enabled;
        self
    }

    /// If `false`, mouse-wheel scroll does not scale the child.
    /// `on_interaction_*` callbacks still fire (Flutter parity).
    #[must_use]
    pub fn scale_enabled(mut self, scale_enabled: bool) -> Self {
        self.scale_enabled = scale_enabled;
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
    /// faster. Defaults to Flutter's `kDefaultMouseScrollToScaleFactor`
    /// (`200.0`).
    #[must_use]
    pub fn scale_factor(mut self, scale_factor: f32) -> Self {
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
    pub fn on_interaction_start(
        mut self,
        callback: impl Fn(InteractionStartDetails) + 'static,
    ) -> Self {
        self.on_interaction_start = Some(Rc::new(callback));
        self
    }

    /// Called on every applied (or attempted, if disabled) pan/wheel-scale
    /// update.
    #[must_use]
    pub fn on_interaction_update(
        mut self,
        callback: impl Fn(InteractionUpdateDetails) + 'static,
    ) -> Self {
        self.on_interaction_update = Some(Rc::new(callback));
        self
    }

    /// Called when a pan or wheel-scale interaction ends.
    #[must_use]
    pub fn on_interaction_end(
        mut self,
        callback: impl Fn(InteractionEndDetails) + 'static,
    ) -> Self {
        self.on_interaction_end = Some(Rc::new(callback));
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
            }),
        }
    }
}

// ============================================================================
// State
// ============================================================================

/// Tracks the in-flight pan gesture's dominant-axis lock (for
/// [`PanAxis::Aligned`]) across the several `on_pan_update` calls one drag
/// produces. `Rc`-shared into the closures `build` hands to `GestureDetector`
/// so it survives from `on_pan_start` through the matching `on_pan_end`, even
/// across a rebuild that swaps in fresh closures mid-gesture.
#[derive(Debug)]
struct GestureTracking {
    /// Viewport-local position at the most recent `on_pan_start`. `None`
    /// between gestures.
    pan_start_local: Cell<Option<Offset<Pixels>>>,
    /// The axis a [`PanAxis::Aligned`] drag has locked to, established from
    /// the first non-zero movement of the gesture. `None` before that, and
    /// reset to `None` at the end of every gesture.
    current_axis: Cell<Option<Axis>>,
}

/// Persistent state for [`InteractiveViewer`].
#[derive(Debug)]
pub struct InteractiveViewerState {
    /// Publishes the child's `RenderId` while mounted — see
    /// [`geometry`](Self::geometry) for why one anchor is enough in V1.
    subtree_anchor: SubtreeAnchor,
    gesture: Rc<GestureTracking>,
}

impl ViewState<InteractiveViewer> for InteractiveViewerState {
    #[allow(clippy::too_many_lines)] // one gesture-wiring build(); splitting fragments the callback capture set
    fn build(&self, view: &InteractiveViewer, ctx: &dyn BuildContext) -> impl IntoView {
        let controller = view.controller.clone();
        let anchor = self.subtree_anchor.clone();
        let gesture = Rc::clone(&self.gesture);
        let pipeline_owner = ctx.pipeline_owner();
        let boundary_margin = view.boundary_margin;
        let min_scale = view.min_scale;
        let max_scale = view.max_scale;
        let pan_enabled = view.pan_enabled;
        let scale_enabled = view.scale_enabled;
        let pan_axis = view.pan_axis;
        let scale_factor = view.scale_factor;
        let clip_behavior = view.clip_behavior;
        let alignment = view.alignment;
        let child = view.child.clone();
        let on_start = view.on_interaction_start.clone();
        let on_update = view.on_interaction_update.clone();
        let on_end = view.on_interaction_end.clone();

        let listenable = controller.as_listenable();

        AnimatedBuilder::new(listenable, move || {
            let matrix = controller.value();

            // -- Pan (GestureDetector) -------------------------------------
            let gesture_start = Rc::clone(&gesture);
            let on_start_pan = on_start.clone();
            let pan_start_details = move |details: DragStartDetails| {
                gesture_start.current_axis.set(None);
                gesture_start
                    .pan_start_local
                    .set(Some(details.local_position));
                if let Some(callback) = &on_start_pan {
                    callback(InteractionStartDetails {
                        focal_point: details.global_position,
                        local_focal_point: details.local_position,
                    });
                }
            };

            let gesture_update = Rc::clone(&gesture);
            let controller_update = controller.clone();
            let anchor_update = anchor.clone();
            let pipeline_owner_update = pipeline_owner.clone();
            let on_update_pan = on_update.clone();
            let pan_update_details = move |details: DragUpdateDetails| {
                if pan_enabled {
                    if let Some(start_local) = gesture_update.pan_start_local.get()
                        && gesture_update.current_axis.get().is_none()
                        && pan_axis != PanAxis::Free
                    {
                        let total = details.local_position - start_local;
                        if total != Offset::ZERO {
                            gesture_update.current_axis.set(Some(dominant_axis(total)));
                        }
                    }
                    if let Some((viewport, boundary)) = InteractiveViewerState::geometry(
                        pipeline_owner_update.as_ref(),
                        &anchor_update,
                        boundary_margin,
                    ) {
                        let current_matrix = controller_update.value();
                        let scale = uniform_scale(&current_matrix);
                        let raw_delta = Offset::new(
                            px(details.delta.dx.get() / scale),
                            px(details.delta.dy.get() / scale),
                        );
                        let aligned = match pan_axis {
                            PanAxis::Free => raw_delta,
                            PanAxis::Horizontal => align_to_axis(raw_delta, Axis::Horizontal),
                            PanAxis::Vertical => align_to_axis(raw_delta, Axis::Vertical),
                            PanAxis::Aligned => match gesture_update.current_axis.get() {
                                Some(axis) => align_to_axis(raw_delta, axis),
                                None => raw_delta,
                            },
                        };
                        let next = clamp_translation(current_matrix, aligned, viewport, boundary);
                        controller_update.set_value(next);
                    }
                }
                if let Some(callback) = &on_update_pan {
                    callback(InteractionUpdateDetails {
                        focal_point: details.global_position,
                        local_focal_point: details.local_position,
                        scale: 1.0,
                        focal_point_delta: Offset::new(
                            px(details.delta.dx.get()),
                            px(details.delta.dy.get()),
                        ),
                    });
                }
            };

            let gesture_end = Rc::clone(&gesture);
            let on_end_pan = on_end.clone();
            let pan_end_details = move |details: DragEndDetails| {
                gesture_end.pan_start_local.set(None);
                gesture_end.current_axis.set(None);
                if let Some(callback) = &on_end_pan {
                    callback(InteractionEndDetails {
                        velocity: details.velocity,
                    });
                }
            };

            // -- Wheel scale (Listener::on_pointer_signal) -----------------
            let controller_wheel = controller.clone();
            let anchor_wheel = anchor.clone();
            let pipeline_owner_wheel = pipeline_owner.clone();
            let on_start_wheel = on_start.clone();
            let on_update_wheel = on_update.clone();
            let on_end_wheel = on_end.clone();
            let pointer_signal = move |event: &PointerEvent| {
                let PointerEvent::Scroll(scroll) = event else {
                    return;
                };
                let data = ScrollEventData::from(scroll);
                if data.delta.dy.get() == 0.0 {
                    // Ignore horizontal-only wheel scroll, matching the
                    // oracle (`_receivedPointerSignal` returns early on
                    // `scrollDelta.dy == 0.0`).
                    return;
                }

                if let Some(callback) = &on_start_wheel {
                    callback(InteractionStartDetails {
                        focal_point: data.position,
                        local_focal_point: data.position,
                    });
                }

                let scale_change = (-data.delta.dy.get() / scale_factor).exp();

                if scale_enabled
                    && let Some((viewport, boundary)) = InteractiveViewerState::geometry(
                        pipeline_owner_wheel.as_ref(),
                        &anchor_wheel,
                        boundary_margin,
                    )
                {
                    let scene_before = controller_wheel.to_scene(data.position);
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
                    // after the scale (Flutter parity).
                    let scene_after = controller_wheel.to_scene(data.position);
                    let correction = Offset::new(
                        scene_after.dx - scene_before.dx,
                        scene_after.dy - scene_before.dy,
                    );
                    let translated =
                        clamp_translation(controller_wheel.value(), correction, viewport, boundary);
                    controller_wheel.set_value(translated);
                }

                if let Some(callback) = &on_update_wheel {
                    callback(InteractionUpdateDetails {
                        focal_point: data.position,
                        local_focal_point: data.position,
                        scale: scale_change,
                        focal_point_delta: Offset::ZERO,
                    });
                }
                if let Some(callback) = &on_end_wheel {
                    callback(InteractionEndDetails {
                        velocity: Velocity::ZERO,
                    });
                }
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

            let recognized = GestureDetector::new()
                // Flutter parity: `HitTestBehavior.opaque` — "necessary when
                // panning off screen" (the child's own hit-test area can end
                // up smaller than the viewport once transformed).
                .behavior(HitTestBehavior::Opaque)
                .on_pan_start(pan_start_details)
                .on_pan_update(pan_update_details)
                .on_pan_end(pan_end_details)
                .child(clipped);

            Listener::new()
                .on_pointer_signal(pointer_signal)
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
    /// `pipeline_owner`/`anchor`/`boundary_margin` by clone (they must be
    /// `'static`, so they cannot borrow the `ViewState`), and call this with
    /// those clones instead.
    ///
    /// V1 only supports `constrained: true`, under which
    /// `Listener`/`GestureDetector`/`ClipRect`/`Transform` are all
    /// layout-transparent proxies that adopt the child's own size — nothing
    /// between this widget's box and the child imposes a different size. So
    /// the **viewport** rect (Flutter's `_viewport`, `parentRenderBox.size`)
    /// and the child's own unmargined rect (the base Flutter inflates by
    /// `boundaryMargin` to get `_boundaryRect`) are the same rectangle. This
    /// collapses Flutter's two keys (`_parentKey` on the outer `Listener`,
    /// `_childKey` inside `Transform`) into the single `subtree_anchor` field.
    fn geometry(
        pipeline_owner: Option<&std::sync::Arc<RwLock<PipelineOwner>>>,
        anchor: &SubtreeAnchor,
        boundary_margin: EdgeInsets,
    ) -> Option<(Rect<Pixels>, Rect<Pixels>)> {
        let owner = pipeline_owner?;
        let render_id = anchor.get()?;
        let size = owner.read().box_size(render_id)?;
        let rect = Rect::from_origin_size(Point::new(px(0.0), px(0.0)), size);
        Some((rect, boundary_margin.inflate_rect(rect)))
    }
}

// ============================================================================
// Matrix math — boundary-clamped translate/scale
// ============================================================================

/// The uniform scale factor of a matrix built solely from translation +
/// uniform scale (no rotation — see the module docs on why rotation is out of
/// scope): the length of the transformed x basis vector.
fn uniform_scale(matrix: &Matrix4) -> f32 {
    let m = matrix.to_col_major_array();
    m[0].hypot(m[1])
}

/// Transforms `viewport`'s four corners by the inverse of `matrix` and
/// returns their axis-aligned bounding box — the viewport's rect in scene
/// coordinates after the child has been transformed by `matrix`. Falls back
/// to `viewport` unchanged if `matrix` is singular (should not happen for a
/// translation + uniform-scale matrix with a non-zero scale).
fn transform_viewport(matrix: Matrix4, viewport: Rect<Pixels>) -> Rect<Pixels> {
    match matrix.try_inverse() {
        Some(inverse) => inverse.transform_rect(&viewport),
        None => viewport,
    }
}

/// How far `[view_min, view_max]` lies outside `[bound_min, bound_max]` along
/// one axis, signed so that adding it to the viewport's position moves it
/// back inside the boundary. Zero when already inside (inclusive).
///
/// Flutter parity: the axis-aligned specialization of
/// `InteractiveViewer._exceedsBy`/`getNearestPointInside` — with rotation
/// permanently disabled (see the module docs), the general `Quad`
/// nearest-point algorithm and this plain interval comparison agree on every
/// case, including a viewport wider than the boundary on this axis (checked
/// against both edges; the edge quoting the larger-magnitude excess wins,
/// exactly as the `Quad` algorithm's per-corner comparison would).
fn axis_excess(view_min: f32, view_max: f32, bound_min: f32, bound_max: f32) -> f32 {
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

fn rect_excess(boundary: Rect<Pixels>, viewport: Rect<Pixels>) -> Offset<Pixels> {
    Offset::new(
        px(axis_excess(
            viewport.min.x.get(),
            viewport.max.x.get(),
            boundary.min.x.get(),
            boundary.max.x.get(),
        )),
        px(axis_excess(
            viewport.min.y.get(),
            viewport.max.y.get(),
            boundary.min.y.get(),
            boundary.max.y.get(),
        )),
    )
}

/// Floating-point tolerance for the "did this transform round-trip produce
/// zero excess" checks in [`clamp_translation`].
///
/// Flutter parity: `InteractiveViewer`'s own `_round` helper exists for
/// exactly this reason — `_exceedsBy`'s result is rounded to 9 decimal
/// places before the `== Offset.zero` check, because
/// `_transformViewport`'s inverse-then-transform round trip leaves residue
/// that *should* be exactly zero but isn't once the matrix carries a
/// non-unit (and non-power-of-two) scale, per the oracle's own comment:
/// "values that should have been zero were given as within 10^-10 of zero".
/// `f32` carries far fewer significant digits than the `f64` the oracle
/// rounds, so a fixed decimal count doesn't transfer numerically; this
/// snaps anything within `EXCESS_EPSILON` of zero back to exactly zero
/// instead. Chosen against the scale of one gesture's excess (tens to
/// thousands of pixels) rather than absolute machine epsilon — comfortably
/// larger than the ~1e-4 residue a `scale * (a - b)` round trip leaves at
/// these magnitudes, comfortably smaller than any excess a real boundary
/// hit produces.
const EXCESS_EPSILON: f32 = 1e-3;

/// Whether a single excess component is within [`EXCESS_EPSILON`] of zero.
fn is_negligible(component: f32) -> bool {
    component.abs() < EXCESS_EPSILON
}

/// Whether `excess` is within [`EXCESS_EPSILON`] of `Offset::ZERO` on both
/// axes — the round-trip-tolerant replacement for `excess == Offset::ZERO`.
fn excess_is_negligible(excess: Offset<Pixels>) -> bool {
    is_negligible(excess.dx.get()) && is_negligible(excess.dy.get())
}

/// Locks a `PanAxis::Aligned` drag to whichever axis dominates `delta`.
/// `delta` must be non-zero (callers only invoke this on real movement).
fn dominant_axis(delta: Offset<Pixels>) -> Axis {
    if delta.dx.get().abs() > delta.dy.get().abs() {
        Axis::Horizontal
    } else {
        Axis::Vertical
    }
}

/// Zeroes out the off-axis component of `delta`.
fn align_to_axis(delta: Offset<Pixels>, axis: Axis) -> Offset<Pixels> {
    match axis {
        Axis::Horizontal => Offset::new(delta.dx, px(0.0)),
        Axis::Vertical => Offset::new(px(0.0), delta.dy),
    }
}

/// Applies `translation` (in scene units) to `matrix`, clamped so the
/// transformed viewport stays within `boundary` when `boundary` is finite.
///
/// Flutter parity: `_InteractiveViewerState._matrixTranslate`. Composed via
/// `matrix * Matrix4::translation(..)` (post-multiply — the translation
/// happens in the matrix's own local/scene space before the rest of the
/// transform is applied), matching `vector_math`'s `translateByDouble`
/// instance-method convention that the oracle relies on. `flui_geometry`'s
/// own `Matrix4::translate` mutator has the *opposite* (pre-multiply,
/// global-space) convention and must not be used here.
fn clamp_translation(
    matrix: Matrix4,
    translation: Offset<Pixels>,
    viewport: Rect<Pixels>,
    boundary: Rect<Pixels>,
) -> Matrix4 {
    if translation == Offset::ZERO {
        return matrix;
    }
    let next = matrix * Matrix4::translation(translation.dx.get(), translation.dy.get(), 0.0);

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
    let corrected_tx = next_tx - excess.dx.get() * current_scale;
    let corrected_ty = next_ty - excess.dy.get() * current_scale;
    let mut corrected = matrix;
    corrected.set_translation(corrected_tx, corrected_ty, next_tz);

    let corrected_viewport = transform_viewport(corrected, viewport);
    let corrected_excess = rect_excess(boundary, corrected_viewport);
    if excess_is_negligible(corrected_excess) {
        return corrected;
    }

    if !is_negligible(corrected_excess.dx.get()) && !is_negligible(corrected_excess.dy.get()) {
        // Neither axis fits at all (the viewport is larger than the
        // boundary in both directions): no translation, matching the
        // oracle.
        return matrix;
    }

    let unidirectional_tx = if is_negligible(corrected_excess.dx.get()) {
        corrected_tx
    } else {
        0.0
    };
    let unidirectional_ty = if is_negligible(corrected_excess.dy.get()) {
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
/// Flutter parity: `_InteractiveViewerState._matrixScale`. Composed via
/// `matrix * Matrix4::scaling(..)` — see [`clamp_translation`]'s doc for why
/// the mutating `Matrix4::scale` method is the wrong tool here.
fn clamp_scale(
    matrix: Matrix4,
    scale: f32,
    min_scale: f32,
    max_scale: f32,
    viewport: Rect<Pixels>,
    boundary: Rect<Pixels>,
) -> Matrix4 {
    if scale == 1.0 {
        return matrix;
    }
    // Flutter parity: `assert(maxScale >= minScale)` on `InteractiveViewer`'s
    // constructor — debug-only there too (Dart's `assert` is stripped in
    // release), so this does not replace `clamp_double`'s non-panicking
    // behavior below; it only surfaces the misconfiguration in debug builds.
    debug_assert!(
        max_scale >= min_scale,
        "InteractiveViewer: max_scale ({max_scale}) must be >= min_scale ({min_scale})"
    );
    let current_scale = uniform_scale(&matrix);
    // Finite / infinite (unbounded boundary) is naturally 0.0 here — no
    // separate infinite-boundary branch needed.
    let boundary_floor = (viewport.width().get() / boundary.width().get())
        .max(viewport.height().get() / boundary.height().get());
    let total_scale = (current_scale * scale).max(boundary_floor);
    let clamped_total = clamp_double(total_scale, min_scale, max_scale);
    let applied = clamped_total / current_scale;
    matrix * Matrix4::scaling(applied, applied, applied)
}

/// Flutter parity: `foundation.dart`'s `clampDouble`. Unlike `f32::clamp`
/// (which panics — in every build profile, not just debug — whenever `min >
/// max`), this never panics: a misconfigured `min_scale > max_scale` falls
/// through to Dart's own release-mode behavior (the `assert` above is
/// debug-only) instead of crashing a release build over a caller error that
/// should have been caught in testing.
fn clamp_double(x: f32, min: f32, max: f32) -> f32 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child_rect() -> Rect<Pixels> {
        Rect::from_min_max(
            Point::new(px(0.0), px(0.0)),
            Point::new(px(200.0), px(200.0)),
        )
    }

    /// Regression: at a non-unit, non-power-of-two scale,
    /// `transform_viewport`'s inverse-then-transform round trip can leave
    /// floating-point residue in the corrected excess that is not exactly
    /// zero even where the corrected transform mathematically fits the
    /// boundary exactly. Comparing that residue to `0.0` exactly (the
    /// pre-fix code) misread "should be zero, isn't quite" as "this axis
    /// still doesn't fit", so the unidirectional branch discarded the
    /// corrected translation on that axis entirely instead of keeping it —
    /// observably, the transform snaps back to zero translation on a
    /// boundary hit while zoomed, instead of clamping to the boundary edge.
    ///
    /// Scenario: child/viewport 200x200, `boundary_margin: 20` (boundary
    /// -20..220), scale = e, a hard-left drag attempting -1000 scene units.
    /// Confirmed empirically (see the PR review fix-up) that this exact
    /// input reproduces nonzero residue in `corrected_excess.dx` against
    /// the pre-fix exact-`f32`-equality code, which returns exactly `0.0`
    /// here; the epsilon-tolerant fix returns the correctly clamped
    /// `~= -398.022`. Reverting the `excess_is_negligible`/`is_negligible`
    /// calls in `clamp_translation` back to `== Offset::ZERO` / `== 0.0` /
    /// `!= 0.0` makes this test fail with `tx == 0.0`.
    #[test]
    fn clamp_translation_clamps_instead_of_snapping_to_zero_at_non_unit_scale() {
        let rect = child_rect();
        let boundary = EdgeInsets::all(px(20.0)).inflate_rect(rect);
        let scale = std::f32::consts::E;
        let matrix = Matrix4::scaling(scale, scale, scale);
        let translation = Offset::new(px(-1000.0), px(0.0));

        let result = clamp_translation(matrix, translation, rect, boundary);
        let (tx, ty, _tz) = result.translation_component();

        assert!(
            (tx - (-398.022)).abs() < 0.01,
            "expected the clamped translation to land near -398.022, got {tx} \
             (exactly 0.0 here means the fix regressed back to the \
             snap-to-zero bug)"
        );
        assert_eq!(ty, 0.0);
    }

    /// `f32::clamp` panics — in every build profile, not just debug builds —
    /// whenever `min > max`. `clamp_double` must never panic on that input,
    /// matching Dart's `clampDouble` (whose own `assert(min <= max)` is
    /// debug-only, so release Flutter never crashes over this either).
    #[test]
    fn clamp_double_never_panics_when_min_exceeds_max() {
        assert_eq!(
            clamp_double(5.0, 10.0, 1.0),
            10.0,
            "x < min returns min first"
        );
        assert_eq!(
            clamp_double(50.0, 10.0, 1.0),
            1.0,
            "x not < min falls through to the x > max check, which returns max"
        );
    }
}
