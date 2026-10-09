//! [`Scrollable`] — gesture-driven interactive scroll widget.
//!
//! `Scrollable` composes:
//! - A [`GestureDetector`] that translates pan events into offset mutations on
//!   the [`ScrollController`].
//! - A [`SingleChildScrollView`] as the layout/paint host, sharing the
//!   controller's `ScrollPosition` (not a pushed pixel value).
//!
//! # Scrolling is a layout event, not a build event
//!
//! `RenderViewport` subscribes to the offset and marks itself needing layout
//! (`crates/flui-objects/src/sliver/viewport.rs`), so a scroll re-lays out the
//! viewport and rebuilds nothing. `Scrollable` used to *also* wrap the whole
//! viewport in an `AnimatedBuilder` on the controller's notify, which
//! re-created every sliver view — and every delegate closure — on every
//! `set_pixels`. The one thing that genuinely needed the notify is servicing
//! an `animate_to`/`jump_to` command, which is now a listener installed
//! alongside the fling ones (`install_command_listener`).
//!
//! # Fling ballistic simulation
//!
//! On `on_pan_end`, a `ScrollPhysics` ballistic simulation is started via
//! `AnimationController::animate_with`. The fling controller is registered
//! with the ambient [`VsyncScope`] in `init_state` so the binding ticks it
//! each frame deterministically; a value listener on the controller pushes the
//! current pixel position into the [`ScrollController`] each tick.
//!
//! `on_pan_start` halts any in-flight fling via `stop()`, so grabbing a
//! scrolling list feels physically correct.
//!
//! # Gesture orientation
//!
//! `on_pan_update`/`on_pan_end` orient the drag delta and fling velocity by
//! the resolved [`AxisDirection`] (see [`Scrollable::axis_direction`]), not
//! the bare [`Axis`] alone: a horizontal `Scrollable` under an RTL ambient
//! `Directionality` resolves `RightToLeft` and flips the sign relative to
//! the default `LeftToRight` case.
//!
//! # `animate_to` servicing (ADR-0037)
//!
//! [`ScrollController::animate_to`]/[`jump_to`](ScrollController::jump_to)
//! don't drive the fling controller directly — they queue a command (see
//! `scroll_controller.rs`'s module docs) that this widget services from a
//! listener on the controller's own notify, via
//! [`ScrollController::service_pending_command`]. Reusing the SAME
//! `AnimationController` the ballistic fling above drives means `on_pan_start`
//! cancels a running `animate_to` for free — it stops whichever of the two
//! (fling or curve-driven tween) happens to be active — and `jump_to` queues
//! an explicit cancel for the same reason.
//!
//! # One position per controller
//!
//! `ScrollPosition` is merged into `ScrollController` (v1 restriction: one
//! position per controller).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, Curves, Vsync, VsyncRegistration,
};
use flui_foundation::geometry::Axis;
use flui_foundation::{Listenable, ListenerId};
use flui_rendering::constraints::AxisDirection;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::{PipelineOwner, WeakPipelineCell};
use flui_rendering::view::{ScrollDirection, ScrollPosition};
use flui_view::prelude::StatefulView;
use flui_view::{
    BoxedView, BuildContext, BuildContextExt, Child, InheritedView, IntoView, LifecycleContext,
    View, ViewExt, ViewState, impl_inherited_view,
};

use crate::animated::VsyncScope;
use crate::localization::axis_direction_from_axis_reverse_and_directionality;
use crate::scroll::{ClampingScrollPhysics, ScrollController, ScrollMetrics, SharedScrollPhysics};
use crate::{GestureArenaScope, GestureDetector, Listener, Semantics, SingleChildScrollView};
use flui_interaction::routing::EventPropagation;
use flui_platform_api::{
    WheelPreferences, WheelStep,
    keyboard::Modifiers,
    pointer::{ScrollEvent, ScrollPrecision, ScrollUnit},
};
use flui_rendering::semantics::{ActionArgs, SemanticsAction};
use flui_scheduler::PostFrameHandle;

use super::scroll_position_scope::ScrollPositionScope;

/// A mounted scroll owner, shared with exact-run animation continuations.
/// The parent is the owner observed at admission, never a later replacement.
#[derive(Debug)]
struct FlingEndpoint {
    controller: ScrollController,
    fling: AnimationController,
    physics: SharedScrollPhysics,
    axis: Axis,
    reversed: bool,
    parent: Option<Weak<FlingEndpoint>>,
    alive: AtomicBool,
}

impl FlingEndpoint {
    fn start(self: &Arc<Self>, velocity: f64, device_pixel_ratio: f64) -> bool {
        if !self.alive.load(Ordering::Acquire) {
            return false;
        }
        let position = self.controller.position();
        let metrics = ScrollMetrics::from(&position).with_device_pixel_ratio(device_pixel_ratio);
        let generation = self.fling.run_generation();
        let is_current = || {
            self.alive.load(Ordering::Acquire)
                && self.fling.run_generation() == generation
                && self.controller.pixels() == metrics.pixels
        };
        // Determine transfer policy before owning a returned simulation. A
        // failing custom callback must not unwind through its arbitrary Drop.
        let remaining = self.physics.boundary_velocity(&metrics, velocity);
        if !is_current() {
            return true;
        }
        let Some(simulation) = self.physics.create_ballistic_simulation(&metrics, velocity) else {
            return false;
        };
        if !is_current() {
            return true;
        }
        position.set_is_scrolling(true);
        if !is_current() {
            return true;
        }
        position.set_user_scroll_direction(if velocity > 0.0 {
            ScrollDirection::Reverse
        } else {
            ScrollDirection::Forward
        });
        // Activity observers may jump, replace the owner or start another run.
        if !is_current() {
            return true;
        }
        let Ok(future) = self.fling.animate_with(simulation) else {
            return false;
        };
        if let Some(remaining) = remaining {
            let owner = Arc::downgrade(self);
            let generation = self.fling.run_generation();
            let edge = if remaining > 0.0 {
                metrics.max_scroll_extent
            } else {
                metrics.min_scroll_extent
            };
            future.when_complete_or_cancel(move |result| {
                let Some(owner) = owner.upgrade() else {
                    return;
                };
                if result.is_ok()
                    && owner.alive.load(Ordering::Acquire)
                    && owner.fling.run_generation() == generation
                    && owner.controller.pixels() == edge
                {
                    // A same-pixel jump still queues an authoritative Cancel
                    // even though no position notify services it. The run's
                    // completion was published before those pixel listeners,
                    // so honor their accepted command before old handoff work.
                    if let Some(command) = owner.controller.take_pending_command() {
                        owner.controller.service_command(command, &owner.fling);
                        return;
                    }
                    let physical = if owner.reversed {
                        remaining
                    } else {
                        -remaining
                    };
                    owner.transfer(physical, device_pixel_ratio);
                }
            });
        }
        true
    }

    fn transfer(&self, physical_velocity: f64, device_pixel_ratio: f64) {
        let mut parent = self.parent.as_ref().and_then(Weak::upgrade);
        while let Some(owner) = parent {
            // A retired link must not retarget an accepted impulse into a
            // replacement occupying the same place in the view tree.
            if !owner.alive.load(Ordering::Acquire) {
                return;
            }
            let velocity = if owner.reversed {
                physical_velocity
            } else {
                -physical_velocity
            };
            if owner.axis == self.axis {
                let metrics = ScrollMetrics::from(&owner.controller.position())
                    .with_device_pixel_ratio(device_pixel_ratio);
                let generation = owner.fling.run_generation();
                // Ask the owner's real boundary policy whether motion in this
                // direction is admitted. Bouncing at an extent remains willing;
                // a hard clamp at the same extent is skipped.
                let proposed = metrics.pixels + velocity.signum();
                let allowed = owner.physics.apply_boundary_conditions(&metrics, proposed);
                if !owner.alive.load(Ordering::Acquire)
                    || owner.fling.run_generation() != generation
                    || owner.controller.pixels() != metrics.pixels
                {
                    return;
                }
                if allowed.is_finite()
                    && allowed != metrics.pixels
                    && owner.start(velocity, device_pixel_ratio)
                {
                    return;
                }
            }
            parent = owner.parent.as_ref().and_then(Weak::upgrade);
        }
    }
}

#[derive(Clone, Debug)]
struct FlingScope {
    endpoint: Arc<FlingEndpoint>,
    child: BoxedView,
}

impl InheritedView for FlingScope {
    type Data = Arc<FlingEndpoint>;

    fn data(&self) -> &Self::Data {
        &self.endpoint
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Arc::ptr_eq(&self.endpoint, &old.endpoint)
    }
}

impl_inherited_view!(FlingScope);

/// Authored logical distances used to resolve wheel input.
///
/// The defaults retain the framework's 53-pixel line step and use a 16-pixel
/// character step. These are application policy fallbacks, not measured line
/// heights or character widths. Applications can supply their content's policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelScrollDistances {
    line: f64,
    character: f64,
}

/// An authored wheel distance that cannot produce meaningful finite geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidWheelScrollDistance {
    /// The line distance is not finite and strictly positive.
    #[error("wheel line distance must be finite and strictly positive")]
    Line,
    /// The character distance is not finite and strictly positive.
    #[error("wheel character distance must be finite and strictly positive")]
    Character,
}

impl WheelScrollDistances {
    /// Author logical distances for translated lines and raw horizontal characters.
    ///
    /// # Errors
    /// Refuses zero, negative or nonfinite distances. Products are checked again
    /// when resolving a packet because finite inputs can overflow.
    pub fn try_new(line: f64, character: f64) -> Result<Self, InvalidWheelScrollDistance> {
        if !line.is_finite() || line <= 0.0 {
            return Err(InvalidWheelScrollDistance::Line);
        }
        if !character.is_finite() || character <= 0.0 {
            return Err(InvalidWheelScrollDistance::Character);
        }
        Ok(Self { line, character })
    }
}

impl Default for WheelScrollDistances {
    fn default() -> Self {
        Self {
            line: 53.0,
            character: 16.0,
        }
    }
}

/// A caller-supplied composition of the scrollable content, receiving the
/// [`Scrollable`]'s shared [`ScrollPosition`] and returning the view to
/// scroll. See [`Scrollable::viewport_builder`].
///
/// `Rc`, not `Arc + Send + Sync`: `BoxedView` erases to `Box<dyn View>`, and
/// `View` carries no `Send`/`Sync` supertrait (widget trees are built and
/// laid out on one thread), so a closure that captures pre-built view
/// content (e.g. an eager child list) can never satisfy `+ Send + Sync` —
/// same reason `AnimatedBuilder`'s own builder closure
/// (`transitions/animated_builder.rs`) is `Rc<dyn Fn() -> BoxedView>`, not
/// `Arc<... + Send + Sync>`.
pub type ViewportBuilder = Rc<dyn Fn(ScrollPosition) -> BoxedView>;

// ---------------------------------------------------------------------------
// View (configuration)
// ---------------------------------------------------------------------------

/// Detects pan gestures on its child and maps them to scroll-offset changes
/// on the given [`ScrollController`], including a ballistic fling simulation
/// after the user lifts their finger.
///
/// Scrolling rebuilds nothing. [`ScrollController::set_pixels`] notifies the
/// shared [`ScrollPosition`], and `RenderViewport` — which subscribes to it
/// directly — marks itself needing layout; no element in the subtree is
/// dirtied. See this module's docs.
///
/// A [`VsyncScope`] must be above the `Scrollable` in the tree (or provided
/// by the application's binding) for fling, `animate_to` and notched-wheel
/// animations to advance. There is no wall-clock fallback: the animation
/// controller is built with `AnimationController::unbounded_without_ticker`
/// and registered with that scope. Drag updates and precise or unknown wheel
/// packets write the position directly and do not require animation ticks.
///
/// # Example
///
/// ```rust
/// use flui_animation::Vsync;
/// use flui_widgets::{ScrollController, Scrollable, SizedBox, VsyncScope};
///
/// let controller = ScrollController::new();
///
/// let view = VsyncScope::new(
///     Vsync::new(),
///     Scrollable::new()
///         .controller(controller)
///         .child(SizedBox::new(300.0, 1400.0)),
/// );
/// ```
///
/// [`Listenable`]: flui_foundation::Listenable
#[derive(Clone, StatefulView)]
pub struct Scrollable {
    /// The shared position + notification hub.
    controller: ScrollController,
    wheel_distances: WheelScrollDistances,
    /// An authored boundary / fling policy; `None` uses the owner's default.
    physics: Option<SharedScrollPhysics>,
    /// The axis along which the child scrolls.
    scroll_direction: Axis,
    /// Overrides the resolved [`AxisDirection`] used to orient gesture
    /// deltas/velocity, bypassing this widget's own ambient-`Directionality`
    /// resolution. `None` (the default) means: resolve it the same way the
    /// `.child()` fast path's internally-composed [`SingleChildScrollView`]
    /// does — [`axis_direction_from_axis_reverse_and_directionality`] against
    /// `scroll_direction` with `reverse: false`. Set this explicitly when
    /// composing a [`Scrollable::viewport_builder`] whose content already
    /// resolved its own `AxisDirection` (e.g. [`PageView`](crate::PageView)):
    /// passing that SAME value here keeps gesture orientation in sync with
    /// what the composed content actually paints, instead of `Scrollable`
    /// re-deriving a value that has no visibility into the builder's content.
    axis_direction: Option<AxisDirection>,
    /// The content to make scrollable.
    child: Child,
    /// Overrides the scrollable content's composition entirely; `None`
    /// keeps the `SingleChildScrollView`-over-`child` fast path. See
    /// [`Scrollable::viewport_builder`].
    viewport_builder: Option<ViewportBuilder>,
}

impl std::fmt::Debug for Scrollable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scrollable")
            .field("scroll_direction", &self.scroll_direction)
            .field("axis_direction", &self.axis_direction)
            .field("controller", &self.controller)
            .field("physics", &self.physics)
            .field("has_viewport_builder", &self.viewport_builder.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for Scrollable {
    fn default() -> Self {
        Self {
            controller: ScrollController::new(),
            wheel_distances: WheelScrollDistances::default(),
            physics: None,
            scroll_direction: Axis::Vertical,
            axis_direction: None,
            child: Child::empty(),
            viewport_builder: None,
        }
    }
}

impl Scrollable {
    /// A new vertical `Scrollable` with clamping physics and a fresh
    /// `ScrollController`. Call `.controller(...)` to share the position with
    /// a [`Scrollbar`](super::Scrollbar) or to read the offset programmatically.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a [`ScrollController`] (position + notification hub). Multiple
    /// clones of the same controller share state, so a `Scrollbar` can listen
    /// to the same controller.
    #[must_use]
    pub fn controller(mut self, controller: ScrollController) -> Self {
        self.controller = controller;
        self
    }

    /// Author the logical distance of one translated line or raw wheel character.
    ///
    /// These distances are application scroll policy, not measurements of glyphs
    /// or native typography. Raw detents additionally use system counts; translated
    /// line packets use only the line distance and pixel packets remain unchanged.
    #[must_use]
    pub fn wheel_distances(mut self, distances: WheelScrollDistances) -> Self {
        self.wheel_distances = distances;
        self
    }

    /// Override the boundary / fling behaviour (default:
    /// [`ClampingScrollPhysics`]).
    #[must_use]
    pub fn physics(mut self, physics: SharedScrollPhysics) -> Self {
        self.physics = Some(physics);
        self
    }

    /// The scroll axis (default [`Axis::Vertical`]).
    #[must_use]
    pub fn scroll_direction(mut self, axis: Axis) -> Self {
        self.scroll_direction = axis;
        self
    }

    /// Override the resolved [`AxisDirection`] gestures are oriented by
    /// (default: `None`, resolved from ambient `Directionality` — see the
    /// field docs). Pass the exact `AxisDirection` a
    /// [`Scrollable::viewport_builder`]'s composed content resolved for
    /// itself, so a drag in a given physical direction moves the offset the
    /// way that content actually paints.
    #[must_use]
    pub fn axis_direction(mut self, direction: AxisDirection) -> Self {
        self.axis_direction = Some(direction);
        self
    }

    /// The scrollable content.
    ///
    /// Ignored when [`Scrollable::viewport_builder`] is set — the builder
    /// closure is responsible for its own content in that case.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Override the scrollable content's composition entirely.
    ///
    /// By default (`None`) `Scrollable` composes a [`SingleChildScrollView`]
    /// wrapping [`Scrollable::child`] — the fast path most callers want. Set
    /// this to compose an arbitrary scrollable widget instead — e.g. a
    /// [`Viewport`](super::Viewport) over several slivers, a `ListView`, or a
    /// `CustomScrollView` — when a single child in a `SingleChildScrollView`
    /// isn't the right shape.
    ///
    /// The closure receives this `Scrollable`'s controller's shared
    /// [`ScrollPosition`] and must inject it into whatever it builds
    /// (typically via that widget's own `.position(...)`) so the drag/fling
    /// gesture wiring above still drives it, and `RenderViewport`'s
    /// committed content extents still flush back into the same controller.
    ///
    /// When this is `Some`, [`Scrollable::child`] is ignored.
    #[must_use]
    pub fn viewport_builder(mut self, builder: ViewportBuilder) -> Self {
        self.viewport_builder = Some(builder);
        self
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// An accepted wheel destination; identity also protects against reentrant
/// replacement while stopping or starting the shared animation controller.
struct WheelMotion {
    target: f64,
    generation: Cell<Option<u64>>,
}

/// Persistent state for [`Scrollable`].
///
/// Owns the ballistic fling [`AnimationController`] and its vsync
/// registration. The fling controller has effectively unbounded value range
/// (`f64::NEG_INFINITY` → `f64::INFINITY`) so pixel-space simulation values
/// are never clamped. A value listener on the controller pushes the live pixel
/// position into the [`ScrollController`] each tick.
pub struct ScrollableState {
    recognizer_owner: Rc<()>,
    /// Stable policy identity across ordinary default-config rebuilds.
    default_physics: SharedScrollPhysics,
    /// The scroll controller from the current view configuration. Kept in
    /// state so the fling listener (installed by
    /// [`install_fling_listener`](ScrollableState::install_fling_listener))
    /// can reach it without re-capturing on every `build`. Updated in
    /// `did_update_view` BEFORE both `install_fling_listener` and
    /// `install_stop_hook` re-run — each always reads whatever this field
    /// currently holds, so a controller SWAP moves both onto the new
    /// controller in the same call.
    scroll_controller: ScrollController,
    stop_hook: Option<super::scroll_controller::StopHook>,
    /// The ballistic simulation driver. Bounds span `(NEG_INFINITY, INFINITY)`
    /// so pixel-space simulation positions are not clamped to `[0, 1]`.
    ///
    /// Created once in `create_state`; registered with the ambient
    /// `VsyncScope` in `init_state`; disposed in `dispose`.
    fling_controller: AnimationController,
    /// Owner-local accepted wheel work, independent of the displayed pixels.
    wheel_motion: Rc<RefCell<Option<Rc<WheelMotion>>>>,
    fling_endpoint: RefCell<Option<Arc<FlingEndpoint>>>,
    /// Value-listener ID on `fling_controller` that pushes pixels into
    /// `scroll_controller` each tick. Installed by
    /// [`install_fling_listener`](ScrollableState::install_fling_listener)
    /// (called from `init_state`, and re-run on every `did_update_view` so a
    /// controller swap moves it onto the new controller), removed in
    /// `dispose`.
    fling_listener_id: Option<ListenerId>,
    /// Status-listener ID on `fling_controller` that marks the shared
    /// `ScrollPosition` idle when the ballistic run settles or is stopped.
    /// Same install/remove lifecycle as `fling_listener_id`.
    fling_status_listener_id: Option<ListenerId>,
    /// Listener ID on the *scroll* controller that services an
    /// `animate_to`/`jump_to`-queued command
    /// ([`ScrollController::service_pending_command`]). Installed by
    /// [`install_command_listener`](ScrollableState::install_command_listener)
    /// alongside the fling listeners, re-run on `did_update_view` so a
    /// controller swap moves it, removed in `dispose`.
    ///
    /// This is the *only* thing `Scrollable` still needs a scroll notify for.
    /// The viewport's own re-layout comes from the render side, where
    /// `RenderViewport` subscribes to the offset directly.
    ///
    /// Stored *with* the listenable it was installed on, not just its id:
    /// `did_update_view` reassigns `scroll_controller` before the installers
    /// run, so a removal that read the field would detach from the incoming
    /// controller and leave the outgoing one holding the listener forever.
    command_listener: Option<(Arc<dyn Listenable>, ListenerId)>,
    /// Post-frame capability for the wheel-scroll activity pulse — acquired
    /// in `init_state`/`did_change_dependencies` (never from `build`), per
    /// the frame-capability scope rule. A wheel tick raises the scroll
    /// activity synchronously and this ends it AFTER the frame that
    /// consumed the pixel write, so the viewport's layout still observes
    /// the user direction.
    post_frame: Option<PostFrameHandle>,
    /// Vsync handle kept for `unregister` in `dispose`.
    vsync: Option<Vsync>,
    /// Registration handle returned by `vsync.register(fling_controller)`.
    vsync_registration: Option<VsyncRegistration>,
    /// The presentation's pipeline, acquired in `init_state`/
    /// `did_change_dependencies`; a release reads its device pixel ratio so
    /// the ballistic run rests within half a device pixel.
    pipeline: Option<WeakPipelineCell>,
}

/// The device pixel ratio of the presentation `pipeline` belongs to, read at
/// the moment of the call so a window moved to another monitor is honoured.
/// `1.0` once the presentation is gone or while a frame holds the pipeline.
pub(crate) fn presentation_device_pixel_ratio(pipeline: Option<&WeakPipelineCell>) -> f64 {
    pipeline
        .and_then(WeakPipelineCell::upgrade)
        .and_then(|cell| cell.try_with(PipelineOwner::device_pixel_ratio))
        .unwrap_or(1.0)
}

impl std::fmt::Debug for ScrollableState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollableState")
            .field("scroll_controller", &self.scroll_controller)
            .field("fling_registered", &self.vsync_registration.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for Scrollable {
    type State = ScrollableState;

    fn create_state(&self) -> Self::State {
        // Unbounded: pixel values from the ballistic simulation are never
        // clamped — the simulation's own `is_done` terminates the run.
        // Unboundedness is a constructor fact (#1183), not a bound value —
        // `without_ticker_bounds` now REJECTS a wide-open pair. No ticker:
        // `Vsync` drives this controller once registered below.
        let fling_controller =
            AnimationController::unbounded_without_ticker(Duration::from_millis(1));

        ScrollableState {
            recognizer_owner: Rc::new(()),
            default_physics: Arc::new(ClampingScrollPhysics::new()),
            scroll_controller: self.controller.clone(),
            stop_hook: None,
            fling_controller,
            wheel_motion: Rc::new(RefCell::new(None)),
            fling_endpoint: RefCell::new(None),
            fling_listener_id: None,
            fling_status_listener_id: None,
            post_frame: None,
            command_listener: None,
            vsync: None,
            vsync_registration: None,
            pipeline: None,
        }
    }
}

impl ScrollableState {
    fn endpoint(
        &self,
        view: &Scrollable,
        physics: &SharedScrollPhysics,
        axis_direction: AxisDirection,
        ctx: &dyn BuildContext,
    ) -> Arc<FlingEndpoint> {
        let parent = ctx.get::<FlingScope, _>(|scope| Arc::downgrade(&scope.endpoint));
        let existing = self.fling_endpoint.borrow().clone();
        if let Some(endpoint) = &existing {
            let same_parent = match (&endpoint.parent, &parent) {
                (None, None) => true,
                (Some(old), Some(new)) => Weak::ptr_eq(old, new),
                _ => false,
            };
            if endpoint
                .controller
                .position()
                .ptr_eq(&view.controller.position())
                && Arc::ptr_eq(&endpoint.physics, physics)
                && endpoint.axis == view.scroll_direction
                && endpoint.reversed == axis_direction.is_reversed()
                && same_parent
            {
                return Arc::clone(endpoint);
            }
            endpoint.alive.store(false, Ordering::Release);
        }
        let endpoint = Arc::new(FlingEndpoint {
            controller: view.controller.clone(),
            fling: self.fling_controller.clone(),
            physics: physics.clone(),
            axis: view.scroll_direction,
            reversed: axis_direction.is_reversed(),
            parent,
            alive: AtomicBool::new(true),
        });
        let outgoing = self.fling_endpoint.replace(Some(Arc::clone(&endpoint)));
        drop(outgoing);
        endpoint
    }

    fn retire_endpoint(&mut self) {
        if let Some(endpoint) = self.fling_endpoint.get_mut().take() {
            endpoint.alive.store(false, Ordering::Release);
        }
    }

    /// Installs the binding's post-frame capability on the scroll
    /// controller's shared `ScrollPosition`, so `RenderViewport::
    /// perform_layout`'s committed content extents (`apply_viewport_dimension`/
    /// `apply_content_dimensions`) can flush a coalesced notification after
    /// layout instead of never notifying at all.
    ///
    /// Lifecycle-only (ADR-0021): called from
    /// `init_state`/`did_change_dependencies`, never from `build`. A no-op
    /// when no handle is available yet — `set_flush_handle` is idempotent, so
    /// a later call (e.g. from `did_change_dependencies`) still installs it.
    fn install_flush_handle(&self, ctx: &dyn LifecycleContext) {
        if let Some(handle) = ctx.post_frame_handle() {
            self.scroll_controller.position().set_flush_handle(handle);
        }
    }

    /// Installs the synchronous `jump_to` cancellation hook (ADR-0037) on
    /// [`ScrollController`], closing over this state's own fling controller —
    /// see `ScrollController`'s `stop_hook` field docs for why `jump_to`
    /// needs a hook called AT jump_to time rather than a merely-queued
    /// command serviced on the next rebuild.
    ///
    /// Idempotent (mirrors `install_flush_handle`): called from `init_state`
    /// AND `did_update_view` (a controller swap must move the hook onto the
    /// NEW controller — see `scroll_controller`'s field doc), always against
    /// whatever `self.scroll_controller` currently is.
    fn install_stop_hook(&mut self) {
        let fling = self.fling_controller.clone();
        let hook: super::scroll_controller::StopHook = Arc::new(move || {
            let _ = fling.stop();
        });
        self.scroll_controller.set_stop_hook(hook.clone());
        self.stop_hook = Some(hook);
    }

    fn detach_stop_hook(&mut self) {
        if let Some(hook) = self.stop_hook.take()
            && self.scroll_controller.clear_stop_hook(&hook)
        {
            self.scroll_controller.clear_pending_command();
        }
    }

    /// Installs (or re-installs) the fling value listener that pushes the
    /// ballistic simulation's current pixel position into
    /// `self.scroll_controller` each tick.
    ///
    /// Idempotent, mirroring `install_stop_hook`: called from `init_state`
    /// AND `did_update_view`, always against whatever `self.scroll_controller`
    /// currently is. Removes any previously-installed listener first — this
    /// is what closes the swap-blindness bug: without it, a controller SWAP
    /// left the listener captured in `init_state` pushing ticks into the OLD
    /// controller forever, so an `animate_to`/fling on the NEW controller
    /// drove `fling_controller`'s value but the new controller's own
    /// `pixels()` never moved.
    fn install_fling_listener(&mut self) {
        if let Some(id) = self.fling_listener_id.take() {
            self.fling_controller.remove_listener(id);
        }
        let fling_ref = self.fling_controller.clone();
        let scroll_ref = self.scroll_controller.clone();
        let listener_id = self.fling_controller.add_listener(Arc::new(move || {
            scroll_ref.set_pixels(fling_ref.value());
        }));
        self.fling_listener_id = Some(listener_id);
    }

    /// Installs the listener that services an `animate_to`/`jump_to`-queued
    /// command when the controller notifies (ADR-0037's "notify path").
    ///
    /// This used to happen inside the `build` closure of an `AnimatedBuilder`
    /// wrapping the whole viewport, which meant every scroll pixel rebuilt
    /// the viewport and every sliver view underneath it. Servicing the queue
    /// is the one thing that genuinely needed the notify, so it moved here
    /// and the rebuild went away.
    ///
    /// Re-entrancy: the notifier snapshots its listener list and wraps each
    /// callback, so a command that writes back through the same controller
    /// cannot disturb the notify in flight.
    fn install_command_listener(&mut self) {
        self.remove_command_listener();
        let scroll_ref = self.scroll_controller.clone();
        let fling_ref = self.fling_controller.clone();
        let listenable = self.scroll_controller.as_listenable();
        let listener_id = listenable.add_listener(Arc::new(move || {
            scroll_ref.service_pending_command(&fling_ref);
        }));
        self.command_listener = Some((listenable, listener_id));
        // Drain once on install. A command queued before this listener existed
        // — `animate_to` on a controller that is not attached to a mounted
        // `Scrollable` yet, or one handed over by a rebuild — already fired
        // its notify with nobody listening, and nothing guarantees a second
        // one: if the first layout's dimensions match what the controller
        // already held, it notifies nothing at all and the command waits
        // forever. The `AnimatedBuilder` this replaced serviced the queue on
        // its initial build, which covered the same case implicitly.
        self.scroll_controller
            .service_pending_command(&self.fling_controller);
    }

    /// Detach the command listener from whichever listenable it was installed
    /// on. Idempotent.
    fn remove_command_listener(&mut self) {
        if let Some((listenable, id)) = self.command_listener.take() {
            listenable.remove_listener(id);
        }
    }

    /// Installs the status listener that ends the position's scroll
    /// activity when the ballistic run settles (`Completed`) or is stopped
    /// by a grab or teardown (`Dismissed`). The activity signal is what a
    /// floating header's snap trigger listens to — without this half, a
    /// fling would leave `is_scrolling` stuck true forever.
    fn install_fling_status_listener(&mut self) {
        if let Some(id) = self.fling_status_listener_id.take() {
            self.fling_controller.remove_status_listener(id);
        }
        let position = self.scroll_controller.position();
        let listener_id = self
            .fling_controller
            .add_status_listener(Arc::new(move |status| {
                if matches!(
                    status,
                    AnimationStatus::Completed | AnimationStatus::Dismissed
                ) {
                    position.set_is_scrolling(false);
                }
            }));
        self.fling_status_listener_id = Some(listener_id);
    }
}

impl ViewState<Scrollable> for ScrollableState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.post_frame = ctx.post_frame_handle();
        self.pipeline = ctx.pipeline_owner().map(|cell| cell.downgrade());
        self.install_flush_handle(ctx);
        self.install_stop_hook();
        self.install_fling_listener();
        self.install_fling_status_listener();
        self.install_command_listener();

        // Register with the ambient VsyncScope so the binding ticks the fling
        // controller on each virtual frame — the same pattern used by
        // `ImplicitController::register`.
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register(self.fling_controller.clone());
            self.vsync = Some(vsync);
            self.vsync_registration = Some(registration);
        }
        // If no VsyncScope is present, the fling controller has no ticker at
        // all (built via `unbounded_without_ticker`) and simply never
        // advances — there is no wall-clock fallback.
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.post_frame = ctx.post_frame_handle();
        self.pipeline = ctx.pipeline_owner().map(|cell| cell.downgrade());
        self.install_flush_handle(ctx);
    }

    fn build(&self, view: &Scrollable, ctx: &dyn BuildContext) -> impl IntoView {
        let wheel_preferences = GestureArenaScope::wheel_preferences_of(ctx);
        let wheel_distances = view.wheel_distances;
        let scroll_controller = view.controller.clone();
        let a11y_controller = view.controller.clone();
        let physics = view
            .physics
            .as_ref()
            .unwrap_or(&self.default_physics)
            .clone();
        let scroll_direction = view.scroll_direction;
        // No explicit override: resolve the same way the `.child()` fast
        // path's internally-composed `SingleChildScrollView` resolves its
        // own `AxisDirection` — same helper, same ambient `Directionality`
        // ancestor, same `reverse: false` — so the two stay in agreement
        // without `Scrollable` needing to see what `SingleChildScrollView`
        // computed. See `axis_direction`'s field docs for the override case.
        let axis_direction = view.axis_direction.unwrap_or_else(|| {
            axis_direction_from_axis_reverse_and_directionality(ctx, scroll_direction, false)
        });
        let endpoint = self.endpoint(view, &physics, axis_direction, ctx);
        let child = view.child.clone();
        let viewport_builder = view.viewport_builder.clone();
        let fling_controller = self.fling_controller.clone();
        let post_frame = self.post_frame.clone();

        // The viewport re-lays itself out from the render side:
        // `RenderViewport` subscribes to the offset and marks needs-layout
        // (`crates/flui-objects/src/sliver/viewport.rs`). So this build runs when the
        // *configuration* changes, never per scroll pixel — the
        // `AnimatedBuilder` that used to wrap it rebuilt the viewport and
        // every sliver view underneath on every `set_pixels`. What genuinely
        // needed the notify — servicing an `animate_to`/`jump_to` command —
        // is a listener now (`install_command_listener`).
        {
            // Clones for the gesture callbacks; each closure needs its own
            // `Arc`-counted handle (no refcount bump at call time).
            let fling_stop = fling_controller.clone();
            let wheel_drag = Rc::clone(&self.wheel_motion);
            let ctrl_update = scroll_controller.clone();
            let phys_update = physics.clone();
            let endpoint_fling = Arc::clone(&endpoint);
            let pipeline_fling = self.pipeline.clone();

            // Position mode, not `.offset(pixels)`: the composed viewport's
            // offset IS this controller's shared `ScrollPosition`, so a
            // gesture write is observed directly (no push from this rebuild)
            // and `RenderViewport::perform_layout`'s committed content
            // extents flush back into the same position — see
            // `ScrollPosition`'s docs and `Viewport::position`.
            let scroll_view: BoxedView = if let Some(build_viewport) = &viewport_builder {
                // Custom composition: the closure owns injecting the shared
                // position into whatever it builds. The scope re-publishes
                // that same position to the built subtree, so content INSIDE
                // the scrolled slivers (a floating header's snap trigger, a
                // scrollbar) can subscribe to the position the gestures
                // actually drive.
                ScrollPositionScope::new(
                    scroll_controller.position(),
                    build_viewport(scroll_controller.position()),
                )
                .boxed()
            } else {
                let mut scsv = SingleChildScrollView::new()
                    .scroll_direction(scroll_direction)
                    .position(scroll_controller.position());
                if let Some(content) = child.into_inner() {
                    scsv = scsv.child(content);
                }
                scsv.boxed()
            };

            // Scrollable uses HitTestBehavior::Opaque so the
            // gesture area fires regardless of whether the child content is
            // itself hittable (e.g. an empty SizedBox).
            let position_start = ctrl_update.position();
            let position_update = ctrl_update.position();
            let position_end = ctrl_update.position();
            let gestures = GestureDetector::new()
                .recognizer_owner(Rc::clone(&self.recognizer_owner))
                .drag_pointer_strategy(flui_interaction::DragPointerStrategy::ContinueWithRemaining)
                .behavior(HitTestBehavior::Opaque)
                .on_pan_start(move |_cx, _details| {
                    wheel_drag.borrow_mut().take();
                    // Grab: halt any in-flight fling so the list stops at the
                    // finger's contact position.
                    let _ = fling_stop.stop();
                    // AFTER the stop: stopping fires the status listener,
                    // which marks the position idle — the grab that begins a
                    // new drag must win that ordering, or a fling-into-drag
                    // hand-off would flicker the activity signal off.
                    position_start.set_is_scrolling(true);
                })
                .on_pan_update(move |_cx, details| {
                    // A downward/rightward finger drag
                    // (positive delta on the scroll axis) moves the viewport
                    // toward the axis's START, so the offset DECREASES — but
                    // only for a NOT-reversed `AxisDirection` (`down`/`right`).
                    // For a reversed one (`up`/`left`, e.g. `RightToLeft`
                    // under RTL `Directionality`) the sign flips before the
                    // `pixels - delta` below.
                    //
                    // `apply_boundary_conditions` enforces the physics limits
                    // (hard clamp or spring resistance) before committing.
                    let raw_delta = match scroll_direction {
                        Axis::Vertical => details.delta.dy,
                        Axis::Horizontal => details.delta.dx,
                    };
                    let signed_delta = if axis_direction.is_reversed() {
                        -raw_delta
                    } else {
                        raw_delta
                    };
                    // The user's direction, for activity subscribers (a
                    // floating header's snap decision): a positive signed
                    // delta moves the offset toward the start — Forward.
                    if signed_delta > 0.0 {
                        position_update.set_user_scroll_direction(ScrollDirection::Forward);
                    } else if signed_delta < 0.0 {
                        position_update.set_user_scroll_direction(ScrollDirection::Reverse);
                    }
                    let proposed = ctrl_update.pixels() - signed_delta;
                    let metrics = ScrollMetrics::from(&ctrl_update.position());
                    let clamped = phys_update.apply_boundary_conditions(&metrics, proposed);
                    ctrl_update.set_pixels(clamped);
                })
                .on_pan_end(move |_cx, details| {
                    // Pointer velocity is in "screen coordinates": positive dy/dx
                    // = finger moving down/right. For a NOT-reversed axis
                    // direction the scroll offset increases when the finger
                    // moves the opposite way, so we negate; for a reversed one
                    // (`up`/`left`) the two negations cancel.
                    let admitted_velocity =
                        if details.reason == flui_interaction::GestureEndReason::Cancelled {
                            0.0
                        } else {
                            match scroll_direction {
                                Axis::Vertical => details.fling_velocity().pixels_per_second.dy,
                                Axis::Horizontal => details.fling_velocity().pixels_per_second.dx,
                            }
                        };
                    let fling_velocity_px_per_sec = if axis_direction.is_reversed() {
                        admitted_velocity
                    } else {
                        -admitted_velocity
                    };
                    if !endpoint_fling.start(
                        fling_velocity_px_per_sec,
                        presentation_device_pixel_ratio(pipeline_fling.as_ref()),
                    ) {
                        // No ballistic run: the release IS the end of
                        // scrolling.
                        position_end.set_is_scrolling(false);
                    }
                })
                .child(scroll_view);

            // Wheel / trackpad scrolling has no drag semantics: no slop or
            // arena hold-and-release. Known notches animate; precise and
            // unknown packets apply immediately. Every accepted destination
            // accumulates independently of the displayed intermediate pixels.
            // The tick
            // clamps HARD to the extents (a wheel never overscrolls), pulses the
            // scroll activity with the USER direction around the pixel write, and
            // ends the pulse after the frame that consumes it.
            //
            // The claim channel makes nested scrollables arbitrate instead
            // of both scrolling: `Continue` when the tick cannot move this
            // position (zero delta, or already clamped at the extent) means
            // "only express interest in the event if it would actually result
            // in a scroll" — the outer scrollable then takes the tick.
            let ctrl_wheel = scroll_controller;
            let post_frame_wheel = post_frame.clone();
            let fling_wheel = fling_controller.clone();
            let wheel_motion = Rc::clone(&self.wheel_motion);
            let listener = Listener::new()
                .on_scroll_claim(move |data: &ScrollEvent| {
                    // Deliberately modifier-agnostic: a ctrl+wheel tick over a
                    // plain list scrolls like any other. The ctrl+wheel-zooms contract needs no
                    // decline here: a chord-gated zoom consumer sits INSIDE
                    // the scrollable, and the leaf-first claim walk asks it
                    // first.
                    let axis_delta = wheel_axis_delta(
                        scroll_direction,
                        data,
                        ctrl_wheel.position().viewport_dimension(),
                        &wheel_preferences.snapshot(),
                        wheel_distances,
                    );
                    // Platform deltas arrive already normalized —
                    // positive = content scrolls down (each backend converts its native axes
                    // and units at its own boundary). Only the reversed-axis
                    // flip the drag arms use applies here.
                    let mut delta = axis_delta;
                    if axis_direction.is_reversed() {
                        delta = -delta;
                    }
                    if delta == 0.0 || !delta.is_finite() {
                        return EventPropagation::Continue;
                    }
                    let position = ctrl_wheel.position();
                    let pixels = ctrl_wheel.pixels();
                    let notched = data.precision == ScrollPrecision::Notched;
                    let accepted = wheel_motion.borrow().clone();
                    let base = if notched {
                        accepted
                            .filter(|motion| {
                                motion.generation.get().is_none_or(|generation| {
                                    generation == fling_wheel.run_generation()
                                        && fling_wheel.status().is_running()
                                })
                            })
                            .map_or(pixels, |motion| motion.target)
                    } else {
                        pixels
                    };
                    let proposed = base + delta;
                    if !proposed.is_finite() {
                        return EventPropagation::Continue;
                    }
                    let target =
                        proposed.clamp(position.min_scroll_extent(), position.max_scroll_extent());
                    tracing::trace!(
                        delta,
                        target,
                        pixels = ctrl_wheel.pixels(),
                        "pointer-scroll tick"
                    );
                    if target == base {
                        return EventPropagation::Continue;
                    }
                    // Commit admission before callbacks. A nested tick inherits
                    // this destination and replaces its owner; the older handler
                    // then stops without overwriting the newer accepted work.
                    let motion = Rc::new(WheelMotion {
                        target,
                        generation: Cell::new(None),
                    });
                    *wheel_motion.borrow_mut() = Some(Rc::clone(&motion));
                    let is_current = || {
                        wheel_motion
                            .borrow()
                            .as_ref()
                            .is_some_and(|current| Rc::ptr_eq(current, &motion))
                    };
                    // A wheel tick interrupts whatever animation is driving
                    // the position — otherwise the fling controller's value
                    // listener overwrites the wheel write on its next tick
                    // (the same cancel the drag-grab and `jump_to` paths do).
                    let _ = fling_wheel.stop();
                    if !is_current() {
                        return EventPropagation::Stop;
                    }
                    if notched {
                        // Start at the displayed position, never at the previous
                        // destination or a stale programmatic animation value.
                        fling_wheel.set_value(pixels);
                        if !is_current() {
                            return EventPropagation::Stop;
                        }
                        let started = fling_wheel.animate_to_curved(
                            target,
                            Some(Duration::from_millis(150)),
                            Arc::new(Curves::EaseOut),
                        );
                        if is_current() {
                            if started.is_ok() && fling_wheel.status().is_running() {
                                motion.generation.set(Some(fling_wheel.run_generation()));
                                position.set_is_scrolling(true);
                                if !is_current() {
                                    return EventPropagation::Stop;
                                }
                                position.set_user_scroll_direction(if delta > 0.0 {
                                    ScrollDirection::Reverse
                                } else {
                                    ScrollDirection::Forward
                                });
                            } else {
                                wheel_motion.borrow_mut().take();
                            }
                        }
                        return EventPropagation::Stop;
                    }
                    // The wheel pulse: direction is only recordable
                    // while an activity is live, so raise first.
                    position.set_is_scrolling(true);
                    if !is_current() {
                        return EventPropagation::Stop;
                    }
                    position.set_user_scroll_direction(if delta > 0.0 {
                        ScrollDirection::Reverse
                    } else {
                        ScrollDirection::Forward
                    });
                    if !is_current() {
                        return EventPropagation::Stop;
                    }
                    position.set_pixels(target);
                    if !is_current() {
                        return EventPropagation::Stop;
                    }
                    wheel_motion.borrow_mut().take();
                    match &post_frame_wheel {
                        Some(post_frame) => {
                            let pulse_end = position;
                            let motion_end = fling_wheel.clone();
                            post_frame.schedule(move |_timing| {
                                // A subsequent notch may have started after
                                // this immediate tick but before the frame.
                                if !motion_end.status().is_running() {
                                    pulse_end.set_is_scrolling(false);
                                }
                            });
                        }
                        // No post-frame capability (a bare harness without
                        // the binding wiring): end the pulse synchronously
                        // rather than leaving the activity stuck live. The
                        // layout that consumes the pixels then sees an Idle
                        // direction — degraded, not wrong: it matches a
                        // plain programmatic jump.
                        None => position.set_is_scrolling(false),
                    }
                    EventPropagation::Stop
                })
                .child(gestures);
            let (viewport_semantics, position_semantics) = scroll_semantics(
                a11y_controller,
                scroll_direction,
                axis_direction.is_reversed(),
                physics,
                fling_controller,
                post_frame,
                self.pipeline.clone(),
            );
            FlingScope {
                endpoint,
                child: viewport_semantics
                    .child(position_semantics.child(listener))
                    .boxed(),
            }
        }
    }

    fn did_update_view(&mut self, _old_view: &Scrollable, new_view: &Scrollable) {
        if self
            .scroll_controller
            .position()
            .ptr_eq(&new_view.controller.position())
        {
            return;
        }
        // Stop the retired trajectory while its listeners still target the
        // old position; its metrics must never drive the incoming position.
        self.retire_endpoint();
        self.wheel_motion.borrow_mut().take();
        let _ = self.fling_controller.stop();
        self.scroll_controller.position().set_is_scrolling(false);
        self.remove_command_listener();
        self.detach_stop_hook();
        self.scroll_controller = new_view.controller.clone();
        self.recognizer_owner = Rc::new(());
        self.install_fling_listener();
        self.install_fling_status_listener();
        self.install_command_listener();
        self.install_stop_hook();
    }

    fn dispose(&mut self) {
        self.retire_endpoint();
        self.wheel_motion.borrow_mut().take();
        // An unmount mid-drag or mid-ballistic-run must not leave the shared
        // position claiming a scroll is underway — end the activity FIRST,
        // while this state still knows which position it was driving (the
        // status listener below is about to be detached and could never
        // deliver the settle).
        self.scroll_controller.position().set_is_scrolling(false);
        // Remove the value listener before disposing the controller so the
        // listener closure cannot fire after the state is gone.
        if let Some(id) = self.fling_listener_id.take() {
            self.fling_controller.remove_listener(id);
        }
        if let Some(id) = self.fling_status_listener_id.take() {
            self.fling_controller.remove_status_listener(id);
        }
        self.remove_command_listener();
        // Release the vsync registration so the binding does not hold a
        // reference to the disposed controller.
        if let (Some(vsync), Some(registration)) =
            (self.vsync.take(), self.vsync_registration.take())
        {
            vsync.unregister(&registration);
        }
        // Detach the ADR-0037 stop hook and drop any not-yet-serviced
        // pending command — without this, the user-held `ScrollController`
        // would keep an `Arc` closing over this about-to-be-disposed
        // `fling_controller` alive (and reachable via `jump_to`) forever, and
        // a command queued while still attached to THIS widget would
        // otherwise resurface against a DIFFERENT `ScrollableState` if the
        // same controller is later re-attached to a new `Scrollable`.
        self.detach_stop_hook();
        self.fling_controller.dispose();
    }
}

/// Fraction of the viewport one assistive-technology scroll step moves, so
/// the last line before the step stays on screen after it.
const A11Y_SCROLL_STEP: f64 = 0.8;

/// Native range adjustment and directional actions share the gesture activity and physics path.
fn scroll_semantics(
    controller: ScrollController,
    axis: Axis,
    reversed: bool,
    physics: SharedScrollPhysics,
    fling: AnimationController,
    post_frame: Option<PostFrameHandle>,
    pipeline: Option<WeakPipelineCell>,
) -> (Semantics, Semantics) {
    let semantics = Semantics::new()
        .container(true)
        .role(crate::SemanticsRole::ScrollView)
        .scroll_source(controller.position(), axis, reversed);
    // AccessKit only admits native value writes on adjustable control roles.
    let adjustment = Semantics::new()
        .container(true)
        .explicit_child_nodes(true)
        .slider(true)
        .label("Scroll position")
        .scroll_source(controller.position(), axis, reversed);
    let movement_controller = controller.clone();
    let reveal_fling = fling.clone();
    let move_to: Rc<dyn Fn(f64)> = Rc::new(move |target| {
        let position = movement_controller.position();
        if !target.is_finite() {
            return;
        }
        let target = target.clamp(position.min_scroll_extent(), position.max_scroll_extent());
        let old = movement_controller.pixels();
        if target == old {
            return;
        }
        let _ = fling.stop();
        position.set_is_scrolling(true);
        position.set_user_scroll_direction(if target > old {
            ScrollDirection::Reverse
        } else {
            ScrollDirection::Forward
        });
        position.set_pixels(target);
        let metrics = ScrollMetrics::from(&position)
            .with_device_pixel_ratio(presentation_device_pixel_ratio(pipeline.as_ref()));
        if let Some(simulation) = physics.create_ballistic_simulation(&metrics, 0.0) {
            let _ = fling.animate_with(simulation);
        } else if let Some(post_frame) = &post_frame {
            post_frame.schedule(move |_| position.set_is_scrolling(false));
        } else {
            position.set_is_scrolling(false);
        }
    });
    let reveal_controller = controller.clone();
    let step: Rc<dyn Fn(bool)> = {
        let move_to = Rc::clone(&move_to);
        Rc::new(move |increase| {
            let delta = controller.position().viewport_dimension() * A11Y_SCROLL_STEP;
            move_to(controller.pixels() + if increase { delta } else { -delta });
        })
    };
    let reveal = Rc::clone(&move_to);
    let semantics = semantics.on_action(SemanticsAction::ShowOnScreen, move |_cx, args| {
        let Some(ActionArgs::ShowOnScreen {
            target_rect,
            viewport_rect,
            scroll_position: Some(sampled_pixels),
        }) = args
        else {
            return;
        };
        let current_pixels = reveal_controller.pixels();
        let difference = current_pixels - sampled_pixels;
        if !sampled_pixels.is_finite() || !current_pixels.is_finite() || !difference.is_finite() {
            return;
        }
        let (start, end, viewport_start, viewport_end) = match axis {
            Axis::Vertical => (
                target_rect.top(),
                target_rect.bottom(),
                viewport_rect.top(),
                viewport_rect.bottom(),
            ),
            Axis::Horizontal => (
                target_rect.left(),
                target_rect.right(),
                viewport_rect.left(),
                viewport_rect.right(),
            ),
        };
        // The published geometry may precede an earlier reveal in this same
        // callback walk. Project that measured target into the current offset.
        let displacement = if reversed { -difference } else { difference };
        let start = start - displacement;
        let end = end - displacement;
        if !start.is_finite() || !end.is_finite() {
            return;
        }
        // A target larger than the viewport already exposing both edges
        // stays put; otherwise move the nearest obscured edge into view.
        let delta = if start < viewport_start && end > viewport_end {
            0.0
        } else if start < viewport_start {
            start - viewport_start
        } else if end > viewport_end {
            end - viewport_end
        } else {
            0.0
        };
        let _ = reveal_fling.stop();
        reveal(reveal_controller.pixels() + if reversed { -delta } else { delta });
    });
    let set = move_to;
    let increase = Rc::clone(&step);
    let decrease = Rc::clone(&step);
    let adjustment = adjustment
        .on_set_numeric_value(move |_cx, value| set(value))
        .on_increase(move |_cx| increase(true))
        .on_decrease(move |_cx| decrease(false));
    let forward = Rc::clone(&step);
    let semantics = match axis {
        Axis::Vertical => semantics
            .on_scroll_down(move |_cx| forward(!reversed))
            .on_scroll_up(move |_cx| step(reversed)),
        Axis::Horizontal => semantics
            .on_scroll_right(move |_cx| forward(!reversed))
            .on_scroll_left(move |_cx| step(reversed)),
    };
    (semantics, adjustment)
}

/// The part of a wheel tick that moves a scrollable along `axis`.
///
/// A plain mouse wheel only reports vertical ticks. With Shift held, a tick
/// that carries no horizontal component scrolls horizontally instead, the
/// desktop convention on Windows and Linux; a vertical scrollable then takes
/// nothing from it, so an enclosing horizontal one can. A device that already
/// reports horizontal motion (a trackpad, a tilt wheel, or macOS, which swaps
/// the axes itself) is passed through unchanged.
fn wheel_axis_delta(
    axis: Axis,
    data: &ScrollEvent,
    viewport_dimension: f64,
    preferences: &WheelPreferences,
    distances: WheelScrollDistances,
) -> f64 {
    if (data.delta.unit() == ScrollUnit::Pages
        || (data.delta.unit() == ScrollUnit::Detents && axis == Axis::Vertical))
        && (!viewport_dimension.is_finite() || viewport_dimension < 0.0)
    {
        return 0.0;
    }
    let shifted = data.modifiers.contains(Modifiers::SHIFT) && data.delta.x() == 0.0;
    let delta = match axis {
        Axis::Vertical if shifted => 0.0,
        Axis::Vertical => data.delta.y(),
        Axis::Horizontal if shifted => data.delta.y(),
        Axis::Horizontal => data.delta.x(),
    };
    let pixels_per_unit = match data.delta.unit() {
        ScrollUnit::Pixels => 1.0,
        // Environment-translated lines bypass native lines-per-detent counts.
        ScrollUnit::Lines => distances.line,
        ScrollUnit::Pages => viewport_dimension,
        ScrollUnit::Detents => match axis {
            Axis::Vertical => match preferences.vertical().unwrap_or(WheelStep::Lines(1)) {
                WheelStep::Lines(count) => {
                    let distance = f64::from(count) * distances.line;
                    if !distance.is_finite() {
                        return 0.0;
                    }
                    // Win32 recommends page-like behavior when the configured
                    // line count exceeds the viewport. This cap applies only to
                    // raw detents, never to already translated line packets.
                    distance.min(viewport_dimension)
                }
                WheelStep::Page => viewport_dimension,
                _ => return 0.0,
            },
            Axis::Horizontal => {
                f64::from(preferences.horizontal_characters().unwrap_or(1)) * distances.character
            }
        },
        _ => return 0.0,
    };
    let distance = delta * pixels_per_unit;
    if pixels_per_unit.is_finite() && pixels_per_unit >= 0.0 && distance.is_finite() {
        distance
    } else {
        0.0
    }
}
