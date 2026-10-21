//! [`Scrollable`] — gesture-driven interactive scroll widget.
//!
//! `Scrollable` composes:
//! - A [`GestureDetector`] whose drag runs on the scroll axis and turns its
//!   motion into steps of the nested scroll channel.
//! - A [`SingleChildScrollView`] as the layout/paint host, sharing the
//!   controller's `ScrollPosition` (not a pushed pixel value).
//!
//! # One offset primitive (ADR-0209)
//!
//! Every change of the offset is a step of an input sequence dispatched
//! through the scrollable's nested scroll chain ([`crate::scroll::nested`]):
//! a drag (`UserInput`, under the contact's sequence), a wheel or trackpad
//! packet (`UserInput` or `Momentum`, under the binding's latch), an
//! assistive scroll action (`UserInput`), a fling or a settle
//! (`SideEffect`, under the sequence that released it), and a controller
//! jump, animation or reveal (`Programmatic`). The mounted scrollable's
//! `ScrollOwner` (`scroll_owner.rs`) is that chain's leaf for its own input,
//! and a participant of the chains of the scrollables inside it.
//!
//! # Scrolling is a layout event, not a build event
//!
//! `RenderViewport` subscribes to the offset and marks itself needing layout
//! (`crates/flui-objects/src/sliver/viewport.rs`), so a scroll re-lays out the
//! viewport and rebuilds nothing. The one thing that needs the controller's
//! notify is servicing an `animate_to`/`jump_to` command, a listener installed
//! alongside the fling ones (`install_command_listener`).
//!
//! # Runs
//!
//! A release starts a `ScrollPhysics` ballistic simulation (or a spring to a
//! [`SnapPolicy`] target) on the scrollable's one
//! `AnimationController`, registered with the ambient [`VsyncScope`] so the
//! binding ticks it each frame. Each tick is a step of the sequence that
//! released it. A run that reaches a hard extent with velocity to spare
//! offers the residual to the chain above (`post_fling`), and an accepting
//! scrollable runs it under the same sequence.
//!
//! A drag start halts any run, so grabbing a moving list stops it; a press on
//! a moving list holds it (ADR-0208 rule 4).
//!
//! # Gesture orientation
//!
//! Drag deltas and fling velocities are oriented by the resolved
//! [`AxisDirection`] (see [`Scrollable::axis_direction`]), not the bare
//! [`Axis`] alone: a horizontal `Scrollable` under an RTL ambient
//! `Directionality` resolves `RightToLeft` and flips the sign relative to
//! the default `LeftToRight` case.
//!
//! # `animate_to` servicing (ADR-0037)
//!
//! [`ScrollController::animate_to`]/[`jump_to`](ScrollController::jump_to)
//! don't drive the animation controller directly — they queue a command (see
//! `scroll_controller.rs`'s module docs) that this widget services from a
//! listener on the controller's own notify. Reusing the one
//! `AnimationController` means a drag start cancels a running `animate_to`
//! for free, and `jump_to` queues an explicit cancel for the same reason.
//!
//! # One position per controller
//!
//! `ScrollPosition` is merged into `ScrollController` (v1 restriction: one
//! position per controller).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, DrivenController, StatusSubscription, Vsync,
};
use flui_foundation::geometry::{Axis, Point, Rect, Vec2};
use flui_foundation::{Listenable, ListenerId};
use flui_rendering::constraints::AxisDirection;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::WeakPipelineCell;
use flui_rendering::view::{ScrollDirection, ScrollPosition};
use flui_view::prelude::StatefulView;
use flui_view::{BoxedView, BuildContext, Child, IntoView, LifecycleContext, ViewExt, ViewState};

use crate::animated::VsyncScope;
use crate::interaction::focus_reveal::{
    FocusRevealScope, RevealTarget, SharedRevealTarget, reveal_distance,
};
use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
    axis_direction_from_axis_reverse_and_directionality,
};
use crate::scroll::drag_autoscroll::{DragAutoscroll, DragAutoscrollAnchor};
use crate::scroll::nested::{
    NestedScrollScope, OverscrollBehavior, ScrollCause, ScrollDevice,
    presentation_device_pixel_ratio,
};
use crate::scroll::scroll_owner::{
    MoveOutcome, OwnerConfig, OwnerEnv, ScrollOwner, SharedSnapPolicy,
};
use crate::scroll::{
    ClampingScrollPhysics, ScrollController, ScrollMetrics, SharedScrollPhysics, SnapPolicy,
};
use crate::support::command_modifiers;
use crate::{
    DragSlot, Focus, GestureArenaScope, GestureDetector, Listener, Semantics, SingleChildScrollView,
};
use flui_interaction::focus::{FocusNode, FocusNodeRegistration, KeyEventResult};
use flui_interaction::pointer::EventPropagation;
use flui_interaction::{HoldGestureRecognizer, PointerEvent, PointerKindSet};
use flui_platform_api::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, NamedKey,
};
use flui_platform_api::{
    WheelPreferences, WheelStep,
    keyboard::Modifiers,
    pointer::{ScrollEvent, ScrollPhase, ScrollPrecision, ScrollUnit},
};
use flui_rendering::semantics::{Reveal, ScrollUnit as SemanticsScrollUnit};
use flui_scheduler::PostFrameHandle;

use super::anchored_drag::AnchoredLeaf;
use super::scroll_controller::ScrollableHooks;
use super::scroll_position_scope::ScrollPositionScope;
use crate::anchored_box::AnchoredBox;

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
    /// The shared position + notification hub, when the application holds
    /// it; `None` uses one the state keeps for its lifetime, so a rebuild
    /// of a parent never swaps the position under a drag.
    controller: Option<ScrollController>,
    wheel_distances: WheelScrollDistances,
    /// An authored boundary / fling policy; `None` uses the owner's default.
    physics: Option<SharedScrollPhysics>,
    /// Where a release comes to rest, when not where the physics coasts.
    snap: Option<SharedSnapPolicy>,
    /// Whether this scrollable's remainder goes up its scroll chain.
    overscroll: OverscrollBehavior,
    /// The pointer kinds whose drags and presses this scrollable admits.
    devices: PointerKindSet,
    /// What assistive technology reads for the scroll view.
    semantic_label: Option<String>,
    /// Take the keyboard focus when mounted.
    autofocus: bool,
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
    /// Set when an [`AnchoredDrag`](super::AnchoredDrag) drives this
    /// scrollable: see [`Scrollable::anchored`].
    anchored: Option<AnchoredLeaf>,
}

impl std::fmt::Debug for Scrollable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scrollable")
            .field("scroll_direction", &self.scroll_direction)
            .field("axis_direction", &self.axis_direction)
            .field("controller", &self.controller)
            .field("physics", &self.physics)
            .field("snap", &self.snap)
            .field("overscroll", &self.overscroll)
            .field("devices", &self.devices)
            .field("autofocus", &self.autofocus)
            .field("has_viewport_builder", &self.viewport_builder.is_some())
            .field("anchored", &self.anchored.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for Scrollable {
    fn default() -> Self {
        Self {
            controller: None,
            wheel_distances: WheelScrollDistances::default(),
            physics: None,
            snap: None,
            overscroll: OverscrollBehavior::Auto,
            devices: PointerKindSet::all(),
            semantic_label: None,
            autofocus: false,
            scroll_direction: Axis::Vertical,
            axis_direction: None,
            child: Child::empty(),
            viewport_builder: None,
            anchored: None,
        }
    }
}

impl Scrollable {
    /// A new vertical `Scrollable` with clamping physics and a position of
    /// its own, which it keeps across rebuilds for as long as it is mounted.
    /// Call `.controller(...)` to share the position with a
    /// [`Scrollbar`](super::Scrollbar) or to read the offset programmatically.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a [`ScrollController`] (position + notification hub). Multiple
    /// clones of the same controller share state, so a `Scrollbar` can listen
    /// to the same controller.
    #[must_use]
    pub fn controller(mut self, controller: ScrollController) -> Self {
        self.controller = Some(controller);
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

    /// Settle every release on the target `policy` picks — a page, an
    /// anchor — instead of where the physics would coast (ADR-0209 §9).
    /// A snapping scrollable keeps its fling: no residual goes up its chain.
    #[must_use]
    pub fn snap_policy(mut self, policy: SharedSnapPolicy) -> Self {
        self.snap = Some(policy);
        self
    }

    /// Whether drag remainders and fling residuals this scrollable and the
    /// scrollables inside it leave go on up the nested scroll chain
    /// (default [`OverscrollBehavior::Auto`]) or stop here
    /// ([`OverscrollBehavior::Contain`]).
    #[must_use]
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll = behavior;
        self
    }

    /// Admit only drags and presses of these pointer kinds (default: all). A
    /// refused contact never reaches this scrollable's arena, so a pen that
    /// draws beside a finger that scrolls leaves the scrollable alone. Wheel
    /// and trackpad scrolling, keys, assistive actions and the controller are
    /// not contacts and are unaffected.
    #[must_use]
    pub fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.devices = devices;
        self
    }

    /// What assistive technology reads for the scroll view, in place of
    /// nothing.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// Take the keyboard focus when mounted, so the scroll keys work without a
    /// click first (default `false`).
    ///
    /// A focused scrollable, or one holding the focus in its subtree, scrolls
    /// by the arrow keys along its axis, PageUp/PageDown and Space/Shift+Space
    /// by a page, and Home/End to its extents. Each key press is a sequence
    /// from the keyboard device in the nested scroll chain; a scrollable at
    /// the end of its range lets the key bubble to the enclosing one. It is
    /// not a Tab stop, and a drag on it takes the focus.
    #[must_use]
    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
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

    /// Drive this scrollable as an [`AnchoredDrag`](super::AnchoredDrag)'s
    /// bounded leaf: its offset is the drag's, `leaf` hears every move and
    /// learns the owner. The composed content is not a scroll view, so no
    /// wheel, drop autoscroll, scroll semantics or position scope; the arrow
    /// keys along the axis move to the next anchor the way they point.
    #[must_use]
    pub(crate) fn anchored(mut self, leaf: AnchoredLeaf) -> Self {
        self.anchored = Some(leaf);
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
    /// The node arrow, page, space, Home and End keys reach this scrollable
    /// through, when it or something inside it has the focus.
    focus: Rc<FocusNode>,
    /// Ends the keyboard sequence a held key began when the key is cancelled
    /// instead of released here: focus moved to another unit meanwhile, or
    /// the window lost activation.
    _key_cancel: FocusNodeRegistration,
    /// The scroll controller from the current view configuration. Kept in
    /// state so the fling listener (installed by
    /// [`install_fling_listener`](ScrollableState::install_fling_listener))
    /// can reach it without re-capturing on every `build`. Updated in
    /// `did_update_view` BEFORE both `install_fling_listener` and
    /// `install_hooks` re-run — each always reads whatever this field
    /// currently holds, so a controller SWAP moves both onto the new
    /// controller in the same call.
    scroll_controller: ScrollController,
    hooks: Option<Rc<ScrollableHooks>>,
    /// The ballistic simulation driver. Bounds span `(NEG_INFINITY, INFINITY)`
    /// so pixel-space simulation positions are not clamped to `[0, 1]`.
    ///
    /// Created once in `create_state`; registered with the ambient
    /// `VsyncScope` in `init_state`; disposed in `dispose`.
    fling_controller: DrivenController,
    /// Registry identity for the scroll-specific cancel-on-clock-change policy.
    /// The driven controller alone owns its registration.
    fling_clock: Option<Vsync>,
    /// Owner-local accepted wheel work, independent of the displayed pixels.
    wheel_motion: Rc<RefCell<Option<Rc<WheelMotion>>>>,
    /// This scrollable's place in the nested scroll channel, for the
    /// current configuration (`scroll_owner.rs`).
    owner: Rc<RefCell<Option<Rc<ScrollOwner>>>>,
    /// The drop receiver that autoscrolls this scrollable while a drag
    /// hovers near its edges, registered by the anchor around the viewport.
    autoscroll: Rc<DragAutoscroll>,
    /// What every owner reads from the lifecycle: the arena and presets.
    env: Rc<OwnerEnv>,
    /// The hold a press on a moving scrollable joins its arena with
    /// (ADR-0208 rule 4), created with the arena in
    /// `init_state`/`did_change_dependencies`.
    hold: Option<Rc<HoldGestureRecognizer>>,
    /// Value-listener ID on `fling_controller` that pushes pixels into
    /// `scroll_controller` each tick. Installed by
    /// [`install_fling_listener`](ScrollableState::install_fling_listener)
    /// (called from `init_state`, and re-run on every `did_update_view` so a
    /// controller swap moves it onto the new controller), removed in
    /// `dispose`.
    fling_listener_id: Option<ListenerId>,
    /// Owning status subscription on `fling_controller` that marks the shared
    /// `ScrollPosition` idle when the ballistic run settles or is stopped.
    /// Withdrawn before controller teardown or attachment replacement.
    fling_status_subscription: Option<StatusSubscription>,
    /// Listener ID on the *scroll* controller that services an
    /// `animate_to`/`jump_to`-queued command
    /// (`ScrollOwner::service_pending_command`). Installed by
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
    command_listener: Option<(std::rc::Rc<dyn Listenable>, ListenerId)>,
    /// Post-frame capability for the wheel-scroll activity pulse — acquired
    /// in `init_state`/`did_change_dependencies` (never from `build`), per
    /// the frame-capability scope rule. A wheel tick raises the scroll
    /// activity synchronously and this ends it AFTER the frame that
    /// consumed the pixel write, so the viewport's layout still observes
    /// the user direction.
    post_frame: Option<PostFrameHandle>,
    /// The presentation's pipeline, acquired in `init_state`/
    /// `did_change_dependencies`; a release reads its device pixel ratio so
    /// the ballistic run rests within half a device pixel.
    pipeline: Option<WeakPipelineCell>,
    /// This scrollable as the reveal scope of the focus inside it.
    reveal: Rc<ScrollReveal>,
    /// What `apply_view` resolved from the view and the ambient scopes, for
    /// `build` to read: the axis direction, the semantic mover (none for an
    /// anchored drag) and the label of the position control.
    applied: Option<Applied>,
}

/// The configuration `ScrollableState::apply_view` hands to `build`.
#[derive(Clone)]
struct Applied {
    axis_direction: AxisDirection,
    owner: Rc<ScrollOwner>,
    move_to: Option<Rc<dyn Fn(f64, ScrollCause)>>,
    position_label: &'static str,
}

impl std::fmt::Debug for ScrollableState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollableState")
            .field("scroll_controller", &self.scroll_controller)
            .field("fling_registered", &self.fling_controller.is_bound())
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
        let owner = Rc::new(RefCell::new(None));
        let fling_controller = AnimationController::builder(Duration::from_millis(1))
            .debug_label("Scrollable.fling")
            .behavior(flui_animation::AnimationBehavior::Preserve)
            .unbounded()
            .build_on(None);
        let focus = FocusNode::with_debug_label("Scrollable");
        let cancelled = Rc::clone(&owner);
        let key_cancel = focus.register_on_key_cancel(Rc::new(move |press: &KeyEvent| {
            let owner: Option<Rc<ScrollOwner>> = cancelled.borrow().clone();
            if let Some(owner) = owner {
                owner.key_released(press.code);
            }
        }));

        ScrollableState {
            _key_cancel: key_cancel,
            recognizer_owner: Rc::new(()),
            default_physics: Arc::new(ClampingScrollPhysics::new()),
            focus: Rc::clone(&focus),
            reveal: Rc::new(ScrollReveal::new(focus)),
            scroll_controller: self.controller.clone().unwrap_or_default(),
            hooks: None,
            fling_controller,
            fling_clock: None,
            wheel_motion: Rc::new(RefCell::new(None)),
            autoscroll: Rc::new(DragAutoscroll::new(Rc::clone(&owner))),
            owner,
            env: Rc::new(OwnerEnv::default()),
            hold: None,
            fling_listener_id: None,
            fling_status_subscription: None,
            post_frame: None,
            command_listener: None,
            pipeline: None,
            applied: None,
        }
    }
}

impl ScrollableState {
    fn bind_vsync(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = VsyncScope::maybe_of(ctx);
        if VsyncScope::same_registry(self.fling_clock.as_ref(), incoming.as_ref()) {
            return;
        }
        self.fling_clock = incoming;
        if self.fling_clock.is_none() {
            self.wheel_motion.borrow_mut().take();
            // A missing clock must not jump a physical scroll to its unbound
            // settlement. A transfer between clocks preserves the accepted run.
            let _ = self.fling_controller.controller().stop();
        }
        if let Err(error) = self.fling_controller.rebind(self.fling_clock.as_ref()) {
            tracing::error!(%error, "Scrollable lost its frame registry");
        }
    }

    /// The owner serving `view`'s configuration under the chain above
    /// `ctx`: the current one when nothing it was created for changed, a
    /// new one otherwise, retiring the old into it. A new physics or snap
    /// policy value is taken in place, so a view that allocates one every
    /// time keeps its owner. Runs from `apply_view`, outside `build`: it
    /// services the command queued before any owner served the controller.
    fn owner(
        &self,
        view: &Scrollable,
        physics: &SharedScrollPhysics,
        axis_direction: AxisDirection,
        ctx: &dyn BuildContext,
    ) -> Rc<ScrollOwner> {
        let config = OwnerConfig {
            controller: self.scroll_controller.clone(),
            fling: self.fling_controller.controller().clone(),
            physics: physics.clone(),
            snap: view.snap.clone(),
            axis: view.scroll_direction,
            reversed: axis_direction.is_reversed(),
            overscroll: view.overscroll,
            parent: NestedScrollScope::parent_of(ctx),
        };
        let existing = self.owner.borrow().clone();
        if let Some(owner) = existing.as_ref().filter(|owner| owner.serves(&config)) {
            owner.set_physics(config.physics);
            owner.set_snap(config.snap);
            return Rc::clone(owner);
        }
        let owner = ScrollOwner::new(config, Rc::clone(&self.env));
        let outgoing = self.owner.replace(Some(Rc::clone(&owner)));
        if let Some(outgoing) = outgoing {
            outgoing.retire_into(&owner);
        }
        // A command queued before any owner served the controller — an
        // `animate_to` before mount, or across a controller swap — already
        // fired its notify with nobody to service it, and nothing guarantees
        // another one.
        owner.service_pending_command();
        owner
    }

    fn retire_owner(&mut self) {
        let outgoing = self.owner.borrow_mut().take();
        if let Some(owner) = outgoing {
            owner.retire();
        }
    }

    /// The hold a press on this scrollable joins with while it moves
    /// (ADR-0208 rule 4): the press stops the motion, starts its contact
    /// sequence, and never becomes a tap of the content under it. When the
    /// hold ends without this scrollable's own drag taking over, the
    /// position settles through its physics from rest, so a pager stopped
    /// between pages still lands on one.
    fn hold_recognizer(&self, arena: flui_interaction::GestureArena) -> Rc<HoldGestureRecognizer> {
        let fling = self.fling_controller.controller().clone();
        let wheel_motion = Rc::clone(&self.wheel_motion);
        let held = Rc::clone(&self.owner);
        let released = Rc::clone(&self.owner);
        let pipeline = self.pipeline.clone();
        HoldGestureRecognizer::builder(arena)
            .on_hold(move |details| {
                wheel_motion.borrow_mut().take();
                let owner = held.borrow().clone();
                match owner {
                    Some(owner) => owner.hold(details.sequence, details.kind),
                    None => {
                        let _ = fling.stop();
                    }
                }
            })
            .on_release(move |end| {
                let owner = released.borrow().clone();
                if let Some(owner) = owner
                    && !owner.hold_ended(end, presentation_device_pixel_ratio(pipeline.as_ref()))
                {
                    owner.controller().position().set_is_scrolling(false);
                }
            })
            .build()
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

    /// Installs the hooks `jump_to` calls on [`ScrollController`]: the
    /// synchronous cancellation (ADR-0037), closing over this state's own
    /// fling controller — see `ScrollController`'s `hooks` field docs for why
    /// `jump_to` needs one called AT jump_to time rather than a merely-queued
    /// command serviced on the next rebuild — and the jump through the
    /// owner's chain.
    ///
    /// Idempotent (mirrors `install_flush_handle`): called from `init_state`
    /// AND `did_update_view` (a controller swap must move the hooks onto the
    /// NEW controller — see `scroll_controller`'s field doc), always against
    /// whatever `self.scroll_controller` currently is.
    fn install_hooks(&mut self) {
        let fling = self.fling_controller.controller().clone();
        // A jump is a programmatic step through the owner's chain, and
        // waits while a pass of that chain runs.
        let (jump_owner, pass_owner) = (Rc::clone(&self.owner), Rc::clone(&self.owner));
        let hooks = Rc::new(ScrollableHooks {
            stop: Box::new(move || {
                let _ = fling.stop();
            }),
            jump: Box::new(move |target| {
                let owner = jump_owner.borrow().clone();
                owner.is_some_and(|owner| owner.jump(target))
            }),
            in_pass: Box::new(move || {
                pass_owner
                    .borrow()
                    .as_ref()
                    .is_some_and(|owner| owner.in_pass())
            }),
        });
        self.scroll_controller.attach_hooks(Rc::clone(&hooks));
        self.hooks = Some(hooks);
    }

    fn detach_hooks(&mut self) {
        if let Some(hooks) = self.hooks.take()
            && self.scroll_controller.detach_hooks(&hooks)
        {
            self.scroll_controller.retire_pending_command();
        }
    }

    /// Installs (or re-installs) the fling value listener that pushes the
    /// ballistic simulation's current pixel position into
    /// `self.scroll_controller` each tick.
    ///
    /// Idempotent, mirroring `install_hooks`: called from `init_state`
    /// AND `did_update_view`, always against whatever `self.scroll_controller`
    /// currently is. Removes any previously-installed listener first — this
    /// is what closes the swap-blindness bug: without it, a controller SWAP
    /// left the listener captured in `init_state` pushing ticks into the OLD
    /// controller forever, so an `animate_to`/fling on the NEW controller
    /// drove `fling_controller`'s value but the new controller's own
    /// `pixels()` never moved.
    fn install_fling_listener(&mut self) {
        if let Some(id) = self.fling_listener_id.take() {
            self.fling_controller.controller().remove_listener(id);
        }
        let owner = Rc::clone(&self.owner);
        let listener_id = self
            .fling_controller
            .controller()
            .add_listener(std::rc::Rc::new(move || {
                // Each tick is a step of the sequence the run belongs to.
                let owner = owner.borrow().clone();
                if let Some(owner) = owner {
                    owner.tick();
                }
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
        let owner = Rc::clone(&self.owner);
        let listenable = self.scroll_controller.listenable();
        let listener_id = listenable.add_listener(std::rc::Rc::new(move || {
            service_pending_command(&owner, &scroll_ref);
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
        service_pending_command(&self.owner, &self.scroll_controller);
    }

    /// Detach the command listener from whichever listenable it was installed
    /// on. Idempotent.
    fn remove_command_listener(&mut self) {
        if let Some((listenable, id)) = self.command_listener.take() {
            listenable.remove_listener(id);
        }
    }

    /// Installs the status listener that ends the position's scroll
    /// activity when the ballistic run settles or is stopped by a grab or
    /// teardown (`ForwardSettled` or `ReverseSettled`). The activity signal is what a
    /// floating header's snap trigger listens to — without this half, a
    /// fling would leave `is_scrolling` stuck true forever.
    fn install_fling_status_listener(&mut self) {
        drop(self.fling_status_subscription.take());
        let position = self.scroll_controller.position();
        let subscription = self
            .fling_controller
            .controller()
            .subscribe_status(std::rc::Rc::new(move |status| {
                if matches!(
                    status,
                    AnimationStatus::ForwardSettled | AnimationStatus::ReverseSettled
                ) {
                    position.set_is_scrolling(false);
                }
            }));
        self.fling_status_subscription = Some(subscription);
    }
}

impl ViewState<Scrollable> for ScrollableState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.post_frame = ctx.post_frame_handle();
        self.pipeline = ctx.pipeline_owner().map(|cell| cell.downgrade());
        self.install_flush_handle(ctx);
        self.install_hooks();
        self.install_fling_listener();
        self.install_fling_status_listener();
        self.install_command_listener();

        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.post_frame = ctx.post_frame_handle();
        self.pipeline = ctx.pipeline_owner().map(|cell| cell.downgrade());
        self.env.plane.set_pipeline(self.pipeline.clone());
        self.install_flush_handle(ctx);
        self.bind_vsync(ctx);
        let arena = GestureArenaScope::of(ctx);
        self.env.sequences.bind(arena.clone());
        *self.env.settings.borrow_mut() = GestureArenaScope::settings_or_default_of(ctx);
        self.hold = Some(self.hold_recognizer(arena));
    }

    fn apply_view(&mut self, view: &Scrollable, ctx: &dyn BuildContext) {
        let physics = view
            .physics
            .as_ref()
            .unwrap_or(&self.default_physics)
            .clone();
        // No explicit override: resolve the same way the `.child()` fast
        // path's internally-composed `SingleChildScrollView` resolves its
        // own `AxisDirection` — same helper, same ambient `Directionality`
        // ancestor, same `reverse: false` — so the two stay in agreement
        // without `Scrollable` needing to see what `SingleChildScrollView`
        // computed. See `axis_direction`'s field docs for the override case.
        let axis_direction = view.axis_direction.unwrap_or_else(|| {
            axis_direction_from_axis_reverse_and_directionality(ctx, view.scroll_direction, false)
        });
        let owner = self.owner(view, &physics, axis_direction, ctx);
        let motion = view.anchored.as_ref().map(|leaf| {
            *leaf.slot.owner.borrow_mut() = Rc::downgrade(&owner);
            Rc::clone(&leaf.motion)
        });
        // The hook a user callback may replace runs after this borrow ends.
        let outgoing = self.env.motion.replace(motion);
        drop(outgoing);
        // An anchored drag publishes its own adjustable node and is no
        // reveal scope: it is not a view onto its content.
        let move_to = if view.anchored.is_some() {
            self.reveal.configure(None, ctx);
            None
        } else {
            let move_to = semantic_scroll_mover(
                Rc::clone(&owner),
                &self.scroll_controller,
                self.post_frame.clone(),
                self.pipeline.clone(),
            );
            self.reveal.configure(
                Some(RevealMove {
                    move_to: Rc::clone(&move_to),
                    controller: self.scroll_controller.clone(),
                    axis: view.scroll_direction,
                    reversed: axis_direction.is_reversed(),
                }),
                ctx,
            );
            Some(move_to)
        };
        let position_label = match BoxedWidgetsLocalizations::maybe_of(ctx) {
            Some(localizations) => localizations.scroll_position_label(),
            None => DefaultWidgetsLocalizations.scroll_position_label(),
        };
        self.applied = Some(Applied {
            axis_direction,
            owner,
            move_to,
            position_label,
        });
    }

    fn build(&self, view: &Scrollable, ctx: &dyn BuildContext) -> impl IntoView {
        let wheel_preferences = GestureArenaScope::wheel_preferences_of(ctx);
        let wheel_distances = view.wheel_distances;
        let scroll_controller = self.scroll_controller.clone();
        let a11y_controller = self.scroll_controller.clone();
        let Applied {
            axis_direction,
            owner,
            move_to,
            position_label,
        } = self
            .applied
            .clone()
            .expect("BUG: `apply_view` runs before every build");
        let scroll_direction = view.scroll_direction;
        let anchored = view.anchored.is_some();
        let admission = view
            .anchored
            .as_ref()
            .and_then(|leaf| leaf.admission.clone());
        let cancelled = view
            .anchored
            .as_ref()
            .and_then(|leaf| leaf.cancelled.clone());
        let devices = view.devices;
        let snap = view.snap.clone();
        let semantic_label = view.semantic_label.clone();
        let child = view.child.clone();
        let viewport_builder = view.viewport_builder.clone();
        let fling_controller = self.fling_controller.controller().clone();
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
            let owner_start = Rc::clone(&owner);
            let focus_pan = Rc::clone(&self.focus);
            let owner_update = Rc::clone(&owner);
            let owner_end = Rc::clone(&owner);
            let owner_lost = Rc::clone(&owner);
            let pipeline_fling = self.pipeline.clone();

            // Position mode, not `.offset(pixels)`: the composed viewport's
            // offset IS this controller's shared `ScrollPosition`, so a
            // gesture write is observed directly (no push from this rebuild)
            // and `RenderViewport::perform_layout`'s committed content
            // extents flush back into the same position — see
            // `ScrollPosition`'s docs and `Viewport::position`.
            let scroll_view: BoxedView = if let Some(build_viewport) = &viewport_builder
                && anchored
            {
                // An anchored drag's content is no scroll view: nothing
                // inside it scrolls with this position.
                build_viewport(scroll_controller.position())
            } else if let Some(build_viewport) = &viewport_builder {
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
            let position_start = scroll_controller.position();
            let position_end = scroll_controller.position();
            let mut gestures = GestureDetector::new();
            if let Some(admission) = admission {
                gestures = gestures.admission(admission);
            }
            let gestures = gestures
                .recognizer_owner(Rc::clone(&self.recognizer_owner))
                // An anchored drag is hit where its moved child is painted,
                // which its content makes opaque; a scroll view over its
                // whole box.
                .behavior(if anchored {
                    HitTestBehavior::DeferToChild
                } else {
                    HitTestBehavior::Opaque
                })
                // The drag claims a contact only past its own axis's slop, so
                // a scrollable inside one on the other axis shares the
                // contact's motion by axis.
                .drag(match scroll_direction {
                    Axis::Vertical => DragSlot::vertical(),
                    Axis::Horizontal => DragSlot::horizontal(),
                }
                .pointer_strategy(flui_interaction::DragPointerStrategy::ContinueWithRemaining)
                .allowed_devices(devices)
                .on_start(move |_cx, details| {
                    wheel_drag.borrow_mut().take();
                    // A drag on a scrollable gives it the keys.
                    let _ = focus_pan.request_focus();
                    // The contact's sequence: the one a press on the moving
                    // scrollable already began, or a new one that preempts
                    // whatever motion the scrollable drives.
                    owner_start.begin_contact(details.sequence, details.kind, details.device);
                    // Grab: halt any in-flight run so the list stops at the
                    // finger's contact position.
                    let _ = fling_stop.stop();
                    // AFTER the stop: stopping fires the status listener,
                    // which marks the position idle — the grab that begins a
                    // new drag must win that ordering, or a fling-into-drag
                    // hand-off would flicker the activity signal off.
                    position_start.set_is_scrolling(true);
                })
                .on_update(move |_cx, details| {
                    let finger = match scroll_direction {
                        Axis::Vertical => details.delta.dy,
                        Axis::Horizontal => details.delta.dx,
                    };
                    if finger == 0.0 {
                        return;
                    }
                    owner_update.drag(
                        owner_update.finger_to_physical(finger),
                        Point::new(details.local_position.dx, details.local_position.dy),
                    );
                })
                .on_end(move |_cx, details| {
                    // Pointer velocity is in "screen coordinates": positive
                    // dy/dx = finger moving down/right; the content follows the
                    // finger, so the offset moves against it.
                    let completed = details.reason != flui_interaction::GestureEndReason::Cancelled;
                    let finger = if completed {
                        match scroll_direction {
                            Axis::Vertical => details.fling_velocity().pixels_per_second.dy,
                            Axis::Horizontal => details.fling_velocity().pixels_per_second.dx,
                        }
                    } else {
                        0.0
                    };
                    if !owner_end.drag_ended(
                        completed,
                        owner_end.finger_to_physical(finger),
                        presentation_device_pixel_ratio(pipeline_fling.as_ref()),
                    ) {
                        // No run: the release IS the end of scrolling.
                        position_end.set_is_scrolling(false);
                    }
                    // After the drag's own settle: an anchored drag's host may
                    // replace it with a return.
                    if !completed && let Some(cancelled) = &cancelled {
                        cancelled();
                    }
                })
                .on_cancel(move |_cx| owner_lost.drag_lost()))
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
            let post_frame_wheel = post_frame;
            let fling_wheel = fling_controller.clone();
            let wheel_motion = Rc::clone(&self.wheel_motion);
            let owner_wheel = Rc::clone(&owner);
            let listener = Listener::new()
                .on_scroll_claim(move |data: &ScrollEvent| {
                    let handled = (|| {
                        // A wheel does not drag to an anchor: it scrolls what
                        // encloses the anchored drag.
                        if anchored {
                            return EventPropagation::Continue;
                        }
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
                                            && fling_wheel.status().is_directional()
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
                        let target = proposed
                            .clamp(position.min_scroll_extent(), position.max_scroll_extent());
                        tracing::trace!(
                            delta,
                            target,
                            pixels = ctrl_wheel.pixels(),
                            "pointer-scroll tick"
                        );
                        let pointer = data.position.get();
                        let pointer = Point::new(pointer.x, pointer.y);
                        let device = ScrollDevice::Pointer {
                            kind: data.pointer.kind,
                            device: data.pointer.device,
                        };
                        let cause = match data.phase {
                            Some(
                                ScrollPhase::MomentumBegan
                                | ScrollPhase::MomentumChanged
                                | ScrollPhase::MomentumEnded,
                            ) => ScrollCause::Momentum,
                            _ => ScrollCause::UserInput,
                        };
                        if target == base {
                            // At its extent the leaf still carries the packet to
                            // a participant above it (a collapsing header), which
                            // has no claim of its own.
                            return if owner_wheel.wheel_at_extent(device, cause, delta, pointer) {
                                EventPropagation::Stop
                            } else {
                                EventPropagation::Continue
                            };
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
                        // The packet is a step of the binding's latch for this
                        // source; a new latch preempts the motion of the last one.
                        let Some(sequence) = owner_wheel.wheel_sequence(device) else {
                            wheel_motion.borrow_mut().take();
                            return EventPropagation::Continue;
                        };
                        sequence.aim(pointer);
                        if !is_current() {
                            return EventPropagation::Stop;
                        }
                        // A wheel tick interrupts whatever animation is driving
                        // the position — otherwise the controller's run would
                        // overwrite the wheel write on its next tick (the same
                        // cancel the drag-grab and `jump_to` paths do).
                        owner_wheel.halt(&sequence);
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
                            let started = owner_wheel.animate_wheel(
                                &sequence,
                                target,
                                Duration::from_millis(150),
                            );
                            if is_current() {
                                if started {
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
                        owner_wheel.wheel_step(&sequence, cause, target);
                        if !is_current() {
                            return EventPropagation::Stop;
                        }
                        wheel_motion.borrow_mut().take();
                        match &post_frame_wheel {
                            Some(post_frame) => {
                                let pulse_end = position.clone();
                                let motion_end = fling_wheel.clone();
                                if post_frame
                                    .schedule(move |_timing| {
                                        // A subsequent notch may have started after
                                        // this immediate tick but before the frame.
                                        if !motion_end.status().is_directional() {
                                            pulse_end.set_is_scrolling(false);
                                        }
                                    })
                                    .is_err()
                                {
                                    position.set_is_scrolling(false);
                                }
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
                    })();
                    // Moved or not, the packet tells the sequence this leaf
                    // leads for its latch where the latch ends.
                    owner_wheel.wheel_latch();
                    handled
                })
                .child(if anchored {
                    // An anchored drag is no drop autoscroll target.
                    gestures.boxed()
                } else {
                    DragAutoscrollAnchor::new(Rc::clone(&self.autoscroll), Child::some(gestures))
                        .boxed()
                });
            // A press joins as a hold only while motion it catches moves the
            // scrollable: a wheel's easing goes on under a click, which
            // reaches the content.
            let listener = match &self.hold {
                Some(hold) => {
                    let (moving, catching) = (fling_controller, Rc::clone(&owner));
                    listener.recognizer_when(hold, move |dispatch| {
                        moving.is_animating()
                            && catching.catchable()
                            && match dispatch.local {
                                PointerEvent::Down(down) => {
                                    devices == PointerKindSet::all()
                                        || devices.contains_kind(down.pointer.kind)
                                }
                                _ => true,
                            }
                    })
                }
                None => listener,
            };
            let a11y_position = a11y_controller.position();
            let key_item_step = match scroll_direction {
                Axis::Vertical => wheel_distances.line,
                Axis::Horizontal => wheel_distances.character,
            };
            // The scrollable's own plane: its deltas and its pointer are
            // local to this box.
            let listener = AnchoredBox::new(self.env.plane.anchor(), listener);
            let content: BoxedView = if let Some(move_to) = move_to {
                let (viewport_semantics, position_semantics) = scroll_semantics(
                    a11y_controller,
                    scroll_direction,
                    axis_direction.is_reversed(),
                    move_to,
                    semantic_label,
                    position_label,
                    // AccessKit's item: a line when scrolling vertically, a
                    // character when scrolling horizontally.
                    match scroll_direction {
                        Axis::Vertical => wheel_distances.line,
                        Axis::Horizontal => wheel_distances.character,
                    },
                );
                let reveal: SharedRevealTarget = Rc::clone(&self.reveal) as SharedRevealTarget;
                FocusRevealScope::new(
                    reveal,
                    viewport_semantics.child(position_semantics.child(listener)),
                )
                .boxed()
            } else {
                // The anchored drag publishes its own adjustable node, and
                // is no reveal scope: it is not a view onto its content.
                listener.boxed()
            };
            let scope = NestedScrollScope::new(Rc::clone(owner.node()), content);
            let key_snap = snap;
            let key_owner = Rc::clone(&owner);
            let key_pipeline = self.pipeline.clone();
            let key_position = a11y_position;
            let key_post_frame = self.post_frame.clone();
            Focus::new(scope)
                .focus_node(Rc::clone(&self.focus))
                .autofocus(view.autofocus)
                .can_request_focus(true)
                // No stop of traversal, and no semantics of its own: an
                // anchored drag's stop is the row of its gesture
                // alternatives, around it.
                .skip_traversal(true)
                .include_semantics(false)
                .on_key_event(move |_cx, event| {
                    let pipeline = key_pipeline.as_ref();
                    if event.state() == KeyState::Up {
                        key_owner.key_released(event.code);
                        return KeyEventResult::Ignored;
                    }
                    if event.composition == ImeComposition::Active {
                        return KeyEventResult::Ignored;
                    }
                    let position = key_owner.controller().position();
                    let target = if anchored {
                        anchored_key_target(
                            event,
                            scroll_direction,
                            axis_direction.is_reversed(),
                            &position,
                            key_snap.as_deref(),
                        )
                    } else {
                        key_target(
                            event,
                            scroll_direction,
                            axis_direction.is_reversed(),
                            key_item_step,
                            &position,
                            key_snap.as_deref(),
                        )
                    };
                    let Some(target) = target else {
                        return KeyEventResult::Ignored;
                    };
                    let outcome = key_owner.key_scroll(
                        event.code,
                        event.repeat() == KeyRepeat::AutoRepeat,
                        target,
                        presentation_device_pixel_ratio(pipeline),
                    );
                    match outcome {
                        MoveOutcome::Unmoved => KeyEventResult::Ignored,
                        MoveOutcome::Moved => {
                            end_scroll_pulse(&key_position, key_post_frame.as_ref());
                            KeyEventResult::Handled
                        }
                        MoveOutcome::Settling => KeyEventResult::Handled,
                    }
                })
        }
    }

    fn did_update_view(&mut self, old_view: &Scrollable, new_view: &Scrollable) {
        // Without a controller of its own, a rebuild keeps the position this
        // state made; one taken away from the application gets a fresh one.
        let incoming = match (&new_view.controller, &old_view.controller) {
            (Some(controller), _) => controller.clone(),
            (None, None) => return,
            (None, Some(_)) => ScrollController::new(),
        };
        if self
            .scroll_controller
            .position()
            .ptr_eq(&incoming.position())
        {
            return;
        }
        // Stop the retired trajectory while its listeners still target the
        // old position; its metrics must never drive the incoming position.
        self.retire_owner();
        self.wheel_motion.borrow_mut().take();
        let _ = self.fling_controller.controller().stop();
        self.scroll_controller.position().set_is_scrolling(false);
        self.remove_command_listener();
        self.detach_hooks();
        self.scroll_controller = incoming;
        self.recognizer_owner = Rc::new(());
        self.install_fling_listener();
        self.install_fling_status_listener();
        self.install_command_listener();
        self.install_hooks();
    }

    fn dispose(&mut self) {
        self.retire_owner();
        self.reveal.motion.borrow_mut().take();
        self.reveal.parent.borrow_mut().take();
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
            self.fling_controller.controller().remove_listener(id);
        }
        drop(self.fling_status_subscription.take());
        self.remove_command_listener();
        // Release the vsync registration so the binding does not hold a
        // reference to the disposed controller.
        // Detach the ADR-0037 stop hook and drop any not-yet-serviced
        // pending command — without this, the user-held `ScrollController`
        // would keep an `Arc` closing over this about-to-be-disposed
        // `fling_controller` alive (and reachable via `jump_to`) forever, and
        // a command queued while still attached to THIS widget would
        // otherwise resurface against a DIFFERENT `ScrollableState` if the
        // same controller is later re-attached to a new `Scrollable`.
        self.detach_hooks();
        self.fling_controller.dispose();
    }
}

/// Fraction of the viewport one assistive-technology scroll step moves, so
/// the last line before the step stays on screen after it.
const A11Y_SCROLL_STEP: f64 = 0.8;

/// The release speed, logical px/s, a key stands for when it asks a snap
/// policy for the next target: a flick, faster than any policy's tolerance.
const SNAP_KEY_FLICK: f64 = 1000.0;

/// The one movement every semantic scroll goes through: an assistive scroll
/// action (`UserInput` from the assistive device) or a reveal
/// (`Programmatic`) dispatched through the owner's chain, clamped to the
/// extents, then settling through the physics as a release from rest would.
fn semantic_scroll_mover(
    owner: Rc<ScrollOwner>,
    controller: &ScrollController,
    post_frame: Option<PostFrameHandle>,
    pipeline: Option<WeakPipelineCell>,
) -> Rc<dyn Fn(f64, ScrollCause)> {
    let position = controller.position();
    Rc::new(move |target, cause| {
        let outcome = owner.move_to(
            target,
            cause,
            presentation_device_pixel_ratio(pipeline.as_ref()),
        );
        if outcome != MoveOutcome::Moved {
            return;
        }
        // Moved without a settle run: end the activity after the frame that
        // lays the new offset out.
        end_scroll_pulse(&position, post_frame.as_ref());
    })
}

/// How a scroll view moves to reveal: the semantic mover and the
/// orientation it moves in, from the current configuration.
#[derive(Clone)]
struct RevealMove {
    move_to: Rc<dyn Fn(f64, ScrollCause)>,
    controller: ScrollController,
    axis: Axis,
    reversed: bool,
}

/// A scrollable as the reveal scope of the focus inside it
/// (`focus_reveal.rs`): it moves its offset until the rectangle shows in its
/// viewport, by the programmatic step a semantic reveal takes through the
/// chain, then hands what of the rectangle lies in its viewport to the
/// scope above. The viewport's geometry is the scrollable's own focus node,
/// whose `Focus` wraps the whole scroll view.
struct ScrollReveal {
    viewport: Rc<FocusNode>,
    motion: RefCell<Option<RevealMove>>,
    parent: RefCell<Option<SharedRevealTarget>>,
}

impl ScrollReveal {
    fn new(viewport: Rc<FocusNode>) -> Self {
        Self {
            viewport,
            motion: RefCell::new(None),
            parent: RefCell::new(None),
        }
    }

    /// Take the configuration of a build: how to move (none for a scrollable
    /// that is not a scroll view) and the scope above.
    fn configure(&self, motion: Option<RevealMove>, ctx: &dyn BuildContext) {
        let parent = FocusRevealScope::of(ctx);
        let outgoing = (self.motion.replace(motion), self.parent.replace(parent));
        drop(outgoing);
    }
}

impl RevealTarget for ScrollReveal {
    fn reveal(&self, rect: Rect<f64>) {
        let motion = self.motion.borrow().clone();
        let parent = self.parent.borrow().clone();
        let viewport = self.viewport.rect();
        let mut shown = rect;
        if let Some(motion) = motion
            && viewport.width() > 0.0
            && viewport.height() > 0.0
        {
            let distance = match motion.axis {
                Axis::Vertical => {
                    reveal_distance(rect.top(), rect.bottom(), viewport.top(), viewport.bottom())
                }
                Axis::Horizontal => {
                    reveal_distance(rect.left(), rect.right(), viewport.left(), viewport.right())
                }
            };
            if distance != 0.0 && distance.is_finite() {
                let before = motion.controller.pixels();
                (motion.move_to)(
                    before + if motion.reversed { -distance } else { distance },
                    ScrollCause::Programmatic,
                );
                // What actually moved, clamped to the extents: the content
                // moved against the offset, or with it when reversed.
                let moved = motion.controller.pixels() - before;
                let shift = if motion.reversed { moved } else { -moved };
                if shift.is_finite() {
                    shown = shown.translate(match motion.axis {
                        Axis::Vertical => Vec2::new(0.0, shift),
                        Axis::Horizontal => Vec2::new(shift, 0.0),
                    });
                }
            }
            // The enclosing scope reveals what shows here, not the parts
            // this viewport clips.
            shown = shown.intersect(&viewport).unwrap_or(viewport);
        }
        if let Some(parent) = parent {
            parent.reveal(shown);
        }
    }
}

/// End the scroll activity of a move that started no run, after the frame
/// that lays its offset out.
fn end_scroll_pulse(position: &ScrollPosition, post_frame: Option<&PostFrameHandle>) {
    let Some(post_frame) = post_frame else {
        position.set_is_scrolling(false);
        return;
    };
    let pulse_end = position.clone();
    if post_frame
        .schedule(move |_| pulse_end.set_is_scrolling(false))
        .is_err()
    {
        position.set_is_scrolling(false);
    }
}

/// Where a scroll key takes a scrollable along `axis`, or `None` for a key
/// it does not scroll by. Arrow keys follow the axis and the resolved
/// orientation (`reversed` includes RTL); PageUp, PageDown and Space move a
/// page of the viewport along a vertical axis; Home and End go to the extents.
///
/// A scrollable with a `snap` policy turns the policy's pages instead: every
/// arrow along the axis, PageUp, PageDown and Space (on either axis) is a
/// nominal flick toward the next target in that direction, so the key lands
/// where a release would. Along a horizontal axis PageDown and Space go to
/// the next target in reading order whatever the direction, since targets are
/// laid out in reading order.
fn key_target(
    event: &KeyEvent,
    axis: Axis,
    reversed: bool,
    item_step: f64,
    position: &ScrollPosition,
    snap: Option<&dyn SnapPolicy>,
) -> Option<f64> {
    let commands = command_modifiers(event.modifiers);
    let shifted = commands == Modifiers::SHIFT;
    if !(commands.is_empty() || shifted && event.code == Code::Space) {
        return None;
    }
    let page = (position.viewport_dimension() * A11Y_SCROLL_STEP).max(0.0);
    let pixels = position.extents_snapshot().pixels;
    // `forward`: the way the offset grows.
    let step = |forward: bool, distance: f64| {
        Some(match snap {
            Some(snap) => next_snap_target(position, snap, forward),
            None => pixels + if forward { distance } else { -distance },
        })
    };
    let item = item_step.min(page.max(1.0));
    let named = match &event.key {
        Key::Named(named) => Some(*named),
        _ => None,
    };
    // Page keys on a horizontal axis ignore the reading direction.
    let page_reversed = axis == Axis::Vertical && reversed;
    match (axis, named) {
        (Axis::Vertical, Some(NamedKey::ArrowDown))
        | (Axis::Horizontal, Some(NamedKey::ArrowRight)) => step(!reversed, item),
        (Axis::Vertical, Some(NamedKey::ArrowUp))
        | (Axis::Horizontal, Some(NamedKey::ArrowLeft)) => step(reversed, item),
        (axis, Some(NamedKey::PageDown)) if axis == Axis::Vertical || snap.is_some() => {
            step(!page_reversed, page)
        }
        (axis, Some(NamedKey::PageUp)) if axis == Axis::Vertical || snap.is_some() => {
            step(page_reversed, page)
        }
        (axis, _) if event.code == Code::Space && (axis == Axis::Vertical || snap.is_some()) => {
            step(shifted == page_reversed, page)
        }
        (_, Some(NamedKey::Home)) => Some(position.min_scroll_extent()),
        (_, Some(NamedKey::End)) => Some(position.max_scroll_extent()),
        _ => None,
    }
}

/// The target after the one a release from `position` would rest on, the way
/// the offset grows (`forward`) or back: from where a release would rest, so
/// a scrollable that came to rest a hair off its page still turns a whole one.
fn next_snap_target(position: &ScrollPosition, snap: &dyn SnapPolicy, forward: bool) -> f64 {
    let mut metrics = ScrollMetrics::from(position);
    metrics.pixels = snap.target(&metrics, 0.0);
    let flick = if forward {
        SNAP_KEY_FLICK
    } else {
        -SNAP_KEY_FLICK
    };
    snap.target(&metrics, flick)
}

/// Where an arrow key along `axis` moves an anchored drag: the next anchor
/// the way the arrow points, or `None` for any other key. `reversed` is the
/// scroll orientation the drag's offset is mapped through; an arrow moves the
/// dragged content, not the view, so it is read the other way round.
fn anchored_key_target(
    event: &KeyEvent,
    axis: Axis,
    reversed: bool,
    position: &ScrollPosition,
    snap: Option<&dyn SnapPolicy>,
) -> Option<f64> {
    let snap = snap?;
    if !command_modifiers(event.modifiers).is_empty() {
        return None;
    }
    let Key::Named(named) = &event.key else {
        return None;
    };
    let toward_end = match (axis, named) {
        (Axis::Vertical, NamedKey::ArrowDown) | (Axis::Horizontal, NamedKey::ArrowRight) => true,
        (Axis::Vertical, NamedKey::ArrowUp) | (Axis::Horizontal, NamedKey::ArrowLeft) => false,
        _ => return None,
    };
    Some(next_snap_target(position, snap, toward_end == reversed))
}

/// Service a command `controller` queued through the owner serving it, and
/// rebuild its release run when the notify brought new content extents (the
/// flush after a layout). With no owner yet, the command waits: the build
/// that creates the owner services it.
fn service_pending_command(
    owner: &RefCell<Option<Rc<ScrollOwner>>>,
    controller: &ScrollController,
) {
    let current = owner.borrow().clone();
    if let Some(owner) = current
        && owner.controller().position().ptr_eq(&controller.position())
    {
        owner.follow_content_extents();
        owner.service_pending_command();
    }
}

/// Native range adjustment and directional actions share the gesture activity and physics path.
/// A page step moves [`A11Y_SCROLL_STEP`] of the viewport, an item step `item_step` (the wheel's
/// line distance vertically, its character distance horizontally), never more than a page.
fn scroll_semantics(
    controller: ScrollController,
    axis: Axis,
    reversed: bool,
    move_to: Rc<dyn Fn(f64, ScrollCause)>,
    label: Option<String>,
    position_label: &'static str,
    item_step: f64,
) -> (Semantics, Semantics) {
    let mut semantics = Semantics::new()
        .container(true)
        .role(crate::SemanticsRole::ScrollView)
        .scroll_source(controller.position(), axis, reversed);
    if let Some(label) = label {
        semantics = semantics.label(label);
    }
    // AccessKit only admits native value writes on adjustable control roles.
    let adjustment = Semantics::new()
        .container(true)
        .explicit_child_nodes(true)
        .role(crate::SemanticsRole::Slider)
        .label(position_label)
        .scroll_source(controller.position(), axis, reversed);
    let reveal_controller = controller.clone();
    let step: Rc<dyn Fn(bool, SemanticsScrollUnit)> = {
        let move_to = Rc::clone(&move_to);
        Rc::new(move |increase, unit| {
            let page = controller.position().viewport_dimension() * A11Y_SCROLL_STEP;
            let delta = match unit {
                SemanticsScrollUnit::Item => item_step.min(page),
                _ => page,
            };
            move_to(
                controller.pixels() + if increase { delta } else { -delta },
                ScrollCause::UserInput,
            );
        })
    };
    let reveal = Rc::clone(&move_to);
    let semantics = semantics.on_scroll_into_view(move |_cx, request| {
        let Some(Reveal {
            target_rect,
            viewport_rect,
            scroll_position: Some(sampled_pixels),
        }) = request
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
        let delta = reveal_distance(start, end, viewport_start, viewport_end);
        reveal(
            reveal_controller.pixels() + if reversed { -delta } else { delta },
            ScrollCause::Programmatic,
        );
    });
    let set = move_to;
    let increase = Rc::clone(&step);
    let decrease = Rc::clone(&step);
    let adjustment = adjustment
        .on_set_numeric_value(move |_cx, value| set(value, ScrollCause::UserInput))
        .on_increment(move |_cx| increase(true, SemanticsScrollUnit::Page))
        .on_decrement(move |_cx| decrease(false, SemanticsScrollUnit::Page));
    let forward = Rc::clone(&step);
    let semantics = match axis {
        Axis::Vertical => semantics
            .on_scroll_down(move |_cx, unit| forward(!reversed, unit))
            .on_scroll_up(move |_cx, unit| step(reversed, unit)),
        Axis::Horizontal => semantics
            .on_scroll_right(move |_cx, unit| forward(!reversed, unit))
            .on_scroll_left(move |_cx, unit| step(reversed, unit)),
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
pub(crate) fn wheel_axis_delta(
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
