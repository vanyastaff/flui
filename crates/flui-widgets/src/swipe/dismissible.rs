//! [`Dismissible`] — drag a child out of view to dismiss it, then collapse
//! the space it occupied.
//!
//! The drag is an [`AnchoredDrag`] over three stops: closed, and out at the
//! row's extent on each side its [`DismissDirection`] allows. A release past
//! the side's threshold (default `0.4` of the extent), or a fling toward it,
//! settles out; anything else settles back, and a cancelled contact returns.
//! Resting out, the row performs its gesture command and starts the resize
//! collapse over [`Dismissible::resize_duration`] (or fires
//! [`Dismissible::on_dismissed`] at once when that is `None`), then fires
//! `on_dismissed`.
//!
//! # Known limits
//!
//! 1. **No `confirm_dismiss`.** An async veto awaited between the slide and
//!    the collapse needs a widget-level "await a caller future, then go on"
//!    seam FLUI does not have; a synchronous stand-in would misstate an
//!    async, vetoable contract.
//! 2. **Finite laid-out size.** The stops lie at the row's committed extent
//!    along the axis, read when a contact goes down. A row that has not been
//!    laid out, or lays out to no extent, cannot be dragged out.
//! 3. **No keep-alive.** A lazy list's viewport can dispose a row in the
//!    middle of its slide; FLUI has no keep-alive mechanism yet.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::curve::{Curve, Interval};
use flui_animation::{
    Animation, AnimationController, DrivenController, MotionPolicy, Vsync, VsyncUpdate, curves,
};
use flui_foundation::Listenable;
use flui_foundation::ListenerId;
use flui_foundation::geometry::{Axis, Size};
use flui_objects::StackFit;
use flui_painting::paint::Clip;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{
    BoxedView, EventCx, EventOutcome, IntoView, PostFrameHandle, RebuildHandle, ViewExt, ViewState,
    WriterSource,
};

use super::row::{
    ContactOrigin, OutWatch, RowGeometry, Stop, UncoverWatch, Uncovered, nothing_behind,
};
use crate::animated::VsyncScope;
use crate::interaction::{CommandHandle, ContactAdmission, GestureAlternatives};
use crate::scroll::{
    AnchoredDrag, AnchoredDragController, DraggableAnchors, ScrollMetrics, SnapPolicy,
};
use crate::{ClipRect, Listener, MediaQuery, Positioned, SizedBox, Stack};

/// The release speed, logical px/s, from which a release is a fling: below
/// it the threshold decides.
const MIN_FLING_VELOCITY: f64 = 700.0;

/// The direction(s) in which a [`Dismissible`] can be dismissed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DismissDirection {
    /// Dismissible by dragging up or down.
    Vertical,
    /// Dismissible by dragging left or right.
    Horizontal,
    /// Dismissible by dragging against the reading direction (right-to-left
    /// in LTR, left-to-right in RTL).
    EndToStart,
    /// Dismissible by dragging with the reading direction (left-to-right in
    /// LTR, right-to-left in RTL).
    StartToEnd,
    /// Dismissible by dragging up only.
    Up,
    /// Dismissible by dragging down only.
    Down,
    /// Cannot be dismissed by dragging.
    None,
}

impl DismissDirection {
    fn axis(self) -> Option<Axis> {
        match self {
            Self::Horizontal | Self::EndToStart | Self::StartToEnd => Some(Axis::Horizontal),
            Self::Vertical | Self::Up | Self::Down => Some(Axis::Vertical),
            Self::None => None,
        }
    }

    /// Whether the content may go out toward the positive offset (the
    /// reading end, or down) and toward the negative one.
    fn sides(self) -> (bool, bool) {
        match self {
            Self::Horizontal | Self::Vertical => (true, true),
            Self::StartToEnd | Self::Down => (true, false),
            Self::EndToStart | Self::Up => (false, true),
            Self::None => (false, false),
        }
    }

    /// Whether a row dismissible in `self` may go out toward `side`, one of
    /// the four single sides.
    fn allows(self, side: Self) -> bool {
        self == side
            || matches!(
                (self, side),
                (Self::Horizontal, Self::StartToEnd | Self::EndToStart)
                    | (Self::Vertical, Self::Up | Self::Down)
            )
    }

    /// The direction a signed offset points on `axis`.
    fn of_offset(axis: Axis, offset: f64) -> Self {
        match (axis, offset) {
            (_, 0.0) => Self::None,
            (Axis::Horizontal, offset) if offset > 0.0 => Self::StartToEnd,
            (Axis::Horizontal, _) => Self::EndToStart,
            (Axis::Vertical, offset) if offset > 0.0 => Self::Down,
            (Axis::Vertical, _) => Self::Up,
        }
    }
}

/// The fraction of a [`Dismissible`]'s extent along its axis that a release
/// must pass to dismiss it toward a side, in `0.0..=1.0`.
///
/// [`NEVER`](Self::NEVER) (`1.0`) keeps the side in: a drag toward it, or a
/// fling, always settles back.
///
/// ```rust
/// use flui_widgets::DismissThreshold;
///
/// assert_eq!(DismissThreshold::new(0.9).map(DismissThreshold::get), Ok(0.9));
/// assert!(DismissThreshold::new(1.5).is_err());
/// assert!(DismissThreshold::new(f64::NAN).is_err());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct DismissThreshold(f64);

impl DismissThreshold {
    /// The threshold a side has unless one is set: `0.4`.
    pub const DEFAULT: Self = Self(0.4);
    /// No release dismisses toward the side.
    pub const NEVER: Self = Self(1.0);

    /// The threshold `fraction` of the extent.
    ///
    /// # Errors
    ///
    /// [`DismissThresholdError`] for a fraction outside `0.0..=1.0`, NaN
    /// included.
    pub fn new(fraction: f64) -> Result<Self, DismissThresholdError> {
        if (0.0..=1.0).contains(&fraction) {
            Ok(Self(fraction))
        } else {
            Err(DismissThresholdError { fraction })
        }
    }

    /// The fraction of the extent.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Default for DismissThreshold {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<f64> for DismissThreshold {
    type Error = DismissThresholdError;

    fn try_from(fraction: f64) -> Result<Self, Self::Error> {
        Self::new(fraction)
    }
}

/// A dismiss threshold outside `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
#[error("a dismiss threshold is a fraction in 0.0..=1.0, not {fraction}")]
pub struct DismissThresholdError {
    fraction: f64,
}

impl DismissThresholdError {
    /// The refused fraction.
    #[must_use]
    pub fn fraction(&self) -> f64 {
        self.fraction
    }
}

/// Fired when the [`Dismissible`] has been dismissed, after any resize
/// collapse has finished (or immediately, if `resize_duration` is `None`).
pub type DismissDirectionCallback = Rc<dyn Fn(&mut EventCx<'_>, DismissDirection)>;

/// Fired on every drag/threshold-state change while a [`Dismissible`] is
/// being dragged.
pub type DismissUpdateCallback = Rc<dyn Fn(&mut EventCx<'_>, DismissUpdateDetails)>;

type ResizeCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

/// Details delivered to [`Dismissible::on_update`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct DismissUpdateDetails {
    /// The direction the dismissible is currently being dragged toward.
    pub direction: DismissDirection,
    /// Whether the dismiss threshold is reached as of this delivery.
    pub reached: bool,
    /// Whether the dismiss threshold was reached as of the *previous*
    /// delivery — pairs with `reached` to catch the crossing moment.
    pub previous_reached: bool,
    /// How far out the content is: `0.0` at rest, `1.0` fully out.
    pub progress: f64,
}

/// A widget that can be dismissed by dragging in [`DismissDirection`].
///
/// See the module docs for the documented limits (no `confirm_dismiss`,
/// finite laid-out size, no keep-alive).
#[derive(Clone, StatefulView)]
pub struct Dismissible {
    child: BoxedView,
    background: Option<BoxedView>,
    secondary_background: Option<BoxedView>,
    on_resize: Option<ResizeCallback>,
    on_dismissed: Option<DismissDirectionCallback>,
    on_update: Option<DismissUpdateCallback>,
    direction: DismissDirection,
    resize_duration: Option<Duration>,
    dismiss_thresholds: DismissThresholds,
    alternatives: GestureAlternatives,
}

impl Dismissible {
    /// A `Dismissible` wrapping `child`, dismissible horizontally by default,
    /// collapsing over 300ms once it is out.
    ///
    /// A completed dismissal performs the first command of `alternatives`,
    /// and so do its alternatives without the drag: assistive technology's
    /// custom action, the command's keys while the row has focus, and the
    /// visible control or context menu (ADR-0216). Those reach the same model
    /// effect with no slide and no collapse.
    pub fn new(child: impl IntoView, alternatives: GestureAlternatives) -> Self {
        Self {
            alternatives,
            child: child.into_view().boxed(),
            background: None,
            secondary_background: None,
            on_resize: None,
            on_dismissed: None,
            on_update: None,
            direction: DismissDirection::Horizontal,
            resize_duration: Some(Duration::from_millis(300)),
            dismiss_thresholds: DismissThresholds::default(),
        }
    }

    /// A widget behind `child`, mounted at the row's full extent while the
    /// drag holds the child away from rest, so the child's own paint covers
    /// all of it but the strip the drag uncovers.
    #[must_use]
    pub fn background(mut self, background: impl IntoView) -> Self {
        self.background = Some(background.into_view().boxed());
        self
    }

    /// A widget shown instead of [`Self::background`] while dragging toward
    /// [`DismissDirection::EndToStart`] or [`DismissDirection::Up`]. Only
    /// meaningful once `background` is set.
    #[must_use]
    pub fn secondary_background(mut self, secondary_background: impl IntoView) -> Self {
        self.secondary_background = Some(secondary_background.into_view().boxed());
        self
    }

    /// Called on every resize-collapse tick before the collapse completes.
    #[must_use]
    pub fn on_resize<R: EventOutcome>(
        mut self,
        on_resize: impl Fn(&mut EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_resize = Some(Rc::new(move |cx| on_resize(cx).report()));
        self
    }

    /// Called once the dismissible has been dismissed by the drag — after the
    /// resize collapse finishes, or immediately if `resize_duration` is
    /// `None` — right after the dismissal performed its command.
    #[must_use]
    pub fn on_dismissed<R: EventOutcome>(
        mut self,
        on_dismissed: impl Fn(&mut EventCx<'_>, DismissDirection) -> R + 'static,
    ) -> Self {
        self.on_dismissed = Some(Rc::new(move |cx, direction| {
            on_dismissed(cx, direction).report();
        }));
        self
    }

    /// Called after each frame the content moved in, with the current
    /// direction and threshold state.
    #[must_use]
    pub fn on_update<R: EventOutcome>(
        mut self,
        on_update: impl Fn(&mut EventCx<'_>, DismissUpdateDetails) -> R + 'static,
    ) -> Self {
        self.on_update = Some(Rc::new(move |cx, details| on_update(cx, details).report()));
        self
    }

    /// The direction(s) this widget can be dismissed in. Default:
    /// [`DismissDirection::Horizontal`].
    #[must_use]
    pub fn direction(mut self, direction: DismissDirection) -> Self {
        self.direction = direction;
        self
    }

    /// The duration of the post-dismiss resize collapse. `None` skips the
    /// collapse and fires `on_dismissed` immediately after the slide.
    /// Default: `Some(300ms)`.
    #[must_use]
    pub fn resize_duration(mut self, resize_duration: Option<Duration>) -> Self {
        self.resize_duration = resize_duration;
        self
    }

    /// Set the dismiss threshold of every side `sides` names: one side, both
    /// sides of an axis for [`DismissDirection::Horizontal`] or
    /// [`DismissDirection::Vertical`], none for [`DismissDirection::None`].
    /// A later call wins on the sides both name. Unset sides use
    /// [`DismissThreshold::DEFAULT`]; [`DismissThreshold::NEVER`] keeps a side
    /// in, even though [`Self::direction`] still lets the row be dragged
    /// toward it (it always settles back).
    #[must_use]
    pub fn dismiss_threshold(
        mut self,
        sides: DismissDirection,
        threshold: DismissThreshold,
    ) -> Self {
        self.dismiss_thresholds.set(sides, threshold);
        self
    }

    fn config(&self) -> DismissConfig {
        DismissConfig {
            direction: self.direction,
            thresholds: self.dismiss_thresholds,
            resize: self.resize_duration,
        }
    }
}

impl std::fmt::Debug for Dismissible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dismissible")
            .field("direction", &self.direction)
            .field("resize_duration", &self.resize_duration)
            .field("has_background", &self.background.is_some())
            .finish_non_exhaustive()
    }
}

/// What a dismissible's stops and its release follow.
#[derive(Clone, Debug, PartialEq)]
struct DismissConfig {
    direction: DismissDirection,
    thresholds: DismissThresholds,
    resize: Option<Duration>,
}

impl DismissConfig {
    fn threshold(&self, direction: DismissDirection) -> f64 {
        self.thresholds.of(direction)
    }

    /// The stops and the release policy for a row `extent` long.
    fn anchors(&self, extent: Option<f64>) -> DraggableAnchors<Stop> {
        let mut anchors = DraggableAnchors::builder().at(Stop::Closed, 0.0);
        let Some(axis) = self.direction.axis() else {
            return anchors
                .build()
                .expect("BUG: one stop at zero is a valid set");
        };
        let out = extent
            .filter(|extent| extent.is_finite() && *extent > 0.0)
            .map(|extent| extent.min(DraggableAnchors::<Stop>::MAX_OFFSET));
        let (positive, negative) = self.direction.sides();
        let side = |allowed: bool, sign: f64| {
            let out = out.filter(|_| allowed)?;
            let direction = DismissDirection::of_offset(axis, sign);
            Some(Side {
                out,
                threshold: self.threshold(direction),
            })
        };
        let snap = ThresholdSnap {
            positive: side(positive, 1.0),
            negative: side(negative, -1.0),
        };
        if let Some(side) = snap.positive {
            anchors = anchors.at(Stop::LeadingOut, side.out);
        }
        if let Some(side) = snap.negative {
            anchors = anchors.at(Stop::TrailingOut, -side.out);
        }
        anchors
            .build()
            .expect("BUG: a dismissible's stops are finite, in range and apart")
            .settled_by(Arc::new(snap))
    }
}

/// The threshold of each side a row can go out on, in the order
/// [`DismissThresholds::SIDES`] names them.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DismissThresholds([DismissThreshold; 4]);

impl Default for DismissThresholds {
    fn default() -> Self {
        Self([DismissThreshold::DEFAULT; 4])
    }
}

impl DismissThresholds {
    const SIDES: [DismissDirection; 4] = [
        DismissDirection::StartToEnd,
        DismissDirection::EndToStart,
        DismissDirection::Up,
        DismissDirection::Down,
    ];

    /// Set `threshold` on every side `direction` allows.
    fn set(&mut self, direction: DismissDirection, threshold: DismissThreshold) {
        for (side, slot) in Self::SIDES.iter().zip(&mut self.0) {
            if direction.allows(*side) {
                *slot = threshold;
            }
        }
    }

    /// The fraction a release toward the side `direction` must pass; the
    /// default for a direction that is not one side.
    fn of(self, direction: DismissDirection) -> f64 {
        Self::SIDES
            .iter()
            .position(|side| *side == direction)
            .map_or(DismissThreshold::DEFAULT, |side| self.0[side])
            .get()
    }
}

/// One side a dismissible can go out on: its stop's distance from rest and
/// the fraction of it a release must pass.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Side {
    out: f64,
    threshold: f64,
}

impl Side {
    /// Where a release on this side settles without a fling.
    fn settle(self, distance: f64) -> f64 {
        if self.threshold < 1.0 && distance / self.out > self.threshold {
            self.out
        } else {
            0.0
        }
    }
}

/// A dismissible's release policy: out past the side's threshold or on a
/// fling toward it, back to rest otherwise.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ThresholdSnap {
    positive: Option<Side>,
    negative: Option<Side>,
}

impl SnapPolicy for ThresholdSnap {
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64 {
        let pixels = metrics.pixels;
        if pixels == 0.0 || !pixels.is_finite() {
            return 0.0;
        }
        let (side, sign) = if pixels > 0.0 {
            (self.positive, 1.0)
        } else {
            (self.negative, -1.0)
        };
        let Some(side) = side else {
            return 0.0;
        };
        if velocity.abs() >= MIN_FLING_VELOCITY {
            // A fling toward the side goes out, unless its threshold keeps
            // it in; a fling back returns.
            return if velocity.signum() == sign && side.threshold < 1.0 {
                sign * side.out
            } else {
                0.0
            };
        }
        sign * side.settle(pixels.abs())
    }
}

// ============================================================================
// Events
// ============================================================================

#[derive(Clone)]
struct DismissCallbacks {
    resize: Option<ResizeCallback>,
    dismissed: Option<DismissDirectionCallback>,
    update: Option<DismissUpdateCallback>,
}

impl From<&Dismissible> for DismissCallbacks {
    fn from(view: &Dismissible) -> Self {
        Self {
            resize: view.on_resize.clone(),
            dismissed: view.on_dismissed.clone(),
            update: view.on_update.clone(),
        }
    }
}

enum DismissEvent {
    Resize,
    Dismissed(DismissDirection),
    Update(DismissUpdateDetails),
}

/// What the update deliveries read: the axis, the thresholds and the extent
/// the stops lie at, as the latest build and contact left them.
#[derive(Clone)]
struct UpdateConfig {
    axis: Option<Axis>,
    thresholds: DismissThresholds,
    extent: Option<f64>,
}

/// User effects run after the frame, through the latest callbacks: listeners
/// and layout discover transitions but run no user code.
struct DismissEvents {
    controller: AnchoredDragController<Stop>,
    config: RefCell<UpdateConfig>,
    /// `progress` and `reached` at the last update delivery.
    delivered: Cell<(f64, bool)>,
    writer: WriterSource,
    post_frame: Option<PostFrameHandle>,
    mounted: Cell<bool>,
    callbacks: Rc<RefCell<DismissCallbacks>>,
    /// The dismissal's command, resolved by the latest build.
    command: RefCell<Option<CommandHandle>>,
}

impl DismissEvents {
    /// The content moved: queue an update when its progress changed.
    fn moved(self: &Rc<Self>) {
        if !self.mounted.get() || self.callbacks.borrow().update.is_none() {
            return;
        }
        let config = self.config.borrow().clone();
        let (Some(axis), Some(extent)) = (config.axis, config.extent) else {
            return;
        };
        let offset = self.controller.offset();
        let progress = (offset.abs() / extent).min(1.0);
        let (last, previous_reached) = self.delivered.get();
        if progress == last {
            return;
        }
        let direction = DismissDirection::of_offset(axis, offset);
        let reached = progress > config.thresholds.of(direction);
        self.delivered.set((progress, reached));
        self.defer(DismissEvent::Update(DismissUpdateDetails {
            direction,
            reached,
            previous_reached,
            progress,
        }));
    }

    fn dispatch(&self, event: DismissEvent) {
        if !self.mounted.get() {
            return;
        }
        let callbacks = self.callbacks.borrow().clone();
        let command = self.command.borrow().clone();
        self.writer.write(|cx| match event {
            DismissEvent::Resize => {
                if let Some(callback) = callbacks.resize {
                    callback(cx);
                }
            }
            DismissEvent::Dismissed(direction) => {
                if let Some(command) = command {
                    let _ = command.perform(cx);
                }
                if let Some(callback) = callbacks.dismissed {
                    callback(cx, direction);
                }
            }
            DismissEvent::Update(details) => {
                if let Some(callback) = callbacks.update {
                    callback(cx, details);
                }
            }
        });
    }

    fn defer(self: &Rc<Self>, event: DismissEvent) {
        let Some(post_frame) = &self.post_frame else {
            tracing::warn!("Dismissible: event dropped because there is no owner post-frame lane");
            return;
        };
        let events = Rc::clone(self);
        if let Err(error) = post_frame.schedule(move |_| events.dispatch(event)) {
            tracing::warn!(
                ?error,
                "Dismissible: event dropped because the owner post-frame lane is closed"
            );
        }
    }
}

// ============================================================================
// Collapse
// ============================================================================

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ResizeCompletion {
    #[default]
    Waiting,
    Pending,
    Delivered,
}

/// The collapse after a dismissal: the size it starts from and its
/// controller, created when the row comes to rest out.
#[derive(Default)]
struct Collapse {
    /// The direction the row went out in, once it did.
    dismissed: Cell<Option<DismissDirection>>,
    size_prior: Cell<Option<Size>>,
    controller: RefCell<Option<DrivenController>>,
    listener: Cell<Option<ListenerId>>,
    completion: Cell<ResizeCompletion>,
}

impl Collapse {
    fn start(
        &self,
        duration: Duration,
        vsync: Option<&Vsync>,
        rebuild: &RebuildHandle,
        size: Size,
        events: &Rc<DismissEvents>,
        collapse: &Rc<Self>,
    ) {
        if self.controller.borrow().is_some() {
            return;
        }
        self.size_prior.set(Some(size));
        let controller = AnimationController::builder(duration)
            .debug_label("Dismissible.resize")
            .build_on(vsync);
        let observed = controller.controller().clone();
        let weak = Rc::downgrade(collapse);
        let events = Rc::downgrade(events);
        let rebuild = rebuild.clone();
        let listener = controller.controller().add_listener(Rc::new(move || {
            let Some(collapse) = weak.upgrade() else {
                return;
            };
            if observed.status().is_forward_settled() {
                if collapse.completion.get() == ResizeCompletion::Waiting {
                    collapse.completion.set(ResizeCompletion::Pending);
                }
            } else if let Some(events) = events.upgrade()
                && events.mounted.get()
                && events.callbacks.borrow().resize.is_some()
            {
                events.defer(DismissEvent::Resize);
            }
            rebuild.schedule(flui_view::RebuildReason::AnimationTick);
        }));
        self.listener.set(Some(listener));
        let _ = controller.controller().forward();
        let outgoing = self.controller.replace(Some(controller));
        drop(outgoing);
    }

    /// Queue the dismissal after the collapse samples already admitted.
    fn deliver_completion(&self, events: &Rc<DismissEvents>) {
        if self.completion.get() != ResizeCompletion::Pending {
            return;
        }
        self.completion.set(ResizeCompletion::Delivered);
        if let Some(direction) = self.dismissed.get() {
            events.defer(DismissEvent::Dismissed(direction));
        }
    }

    fn retire(&self) {
        let controller = self.controller.borrow_mut().take();
        if let Some(mut controller) = controller {
            if let Some(id) = self.listener.take() {
                controller.controller().remove_listener(id);
            }
            controller.dispose();
        }
    }
}

// ============================================================================
// State
// ============================================================================

/// State for [`Dismissible`]: the drag's stops, the committed row extent,
/// the post-frame event source and the collapse.
pub struct DismissibleState {
    controller: AnchoredDragController<Stop>,
    geometry: Rc<RowGeometry>,
    origin: Rc<ContactOrigin>,
    callbacks: Rc<RefCell<DismissCallbacks>>,
    /// The configuration the controller's stops were built from.
    config: Rc<RefCell<DismissConfig>>,
    events: Option<Rc<DismissEvents>>,
    offset_listener: Option<ListenerId>,
    watch: Option<OutWatch>,
    uncover: Option<UncoverWatch>,
    collapse: Rc<Collapse>,
    vsync: Rc<RefCell<Option<Vsync>>>,
    rebuild: Option<RebuildHandle>,
    motion: Rc<Cell<Option<MotionPolicy>>>,
}

impl std::fmt::Debug for DismissibleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DismissibleState")
            .field("offset", &self.controller.offset())
            .field("dismissed", &self.collapse.dismissed.get())
            .field("collapsing", &self.collapse.controller.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for Dismissible {
    type State = DismissibleState;

    fn create_state(&self) -> Self::State {
        DismissibleState {
            controller: AnchoredDragController::new(self.config().anchors(None), Stop::Closed)
                .expect("BUG: every dismissible has a closed stop"),
            geometry: Rc::default(),
            origin: Rc::default(),
            callbacks: Rc::new(RefCell::new(DismissCallbacks::from(self))),
            config: Rc::new(RefCell::new(self.config())),
            events: None,
            offset_listener: None,
            watch: None,
            uncover: None,
            collapse: Rc::default(),
            vsync: Rc::default(),
            rebuild: None,
            motion: Rc::default(),
        }
    }
}

impl DismissibleState {
    /// Give the controller the stops of the current configuration at the
    /// row's committed extent, when that extent changed or `force`.
    fn reanchor(
        controller: &AnchoredDragController<Stop>,
        geometry: &RowGeometry,
        config: &RefCell<DismissConfig>,
        events: Option<&Rc<DismissEvents>>,
        force: bool,
    ) {
        let view = config.borrow().clone();
        let Some(axis) = view.direction.axis() else {
            if force {
                controller.set_anchors(view.anchors(None));
            }
            return;
        };
        let measured = geometry.extent(axis);
        let config = events.map(|events| events.config.borrow().clone());
        let last = config.as_ref().and_then(|config| config.extent);
        let extent = measured.or(last);
        if !force && extent == last {
            return;
        }
        if let Some(events) = events {
            *events.config.borrow_mut() = UpdateConfig {
                axis: Some(axis),
                thresholds: view.thresholds,
                extent,
            };
        }
        controller.set_anchors(view.anchors(extent));
    }

    /// What the row does on coming to rest out: once, it collapses, or
    /// reports its dismissal at once without a collapse.
    fn rest_out(&self) -> Rc<dyn Fn(Stop)> {
        let events = Rc::downgrade(
            self.events
                .as_ref()
                .expect("BUG: Dismissible events exist from init_state"),
        );
        let (collapse, geometry, config, vsync) = (
            Rc::clone(&self.collapse),
            Rc::clone(&self.geometry),
            Rc::clone(&self.config),
            Rc::clone(&self.vsync),
        );
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: Dismissible acquires its RebuildHandle in init_state");
        Rc::new(move |stop| {
            let Some(events) = events.upgrade() else {
                return;
            };
            if collapse.dismissed.get().is_some() {
                return;
            }
            let config = config.borrow().clone();
            let Some(axis) = config.direction.axis() else {
                return;
            };
            let sign = if stop == Stop::LeadingOut { 1.0 } else { -1.0 };
            let direction = DismissDirection::of_offset(axis, sign);
            let resize = config.resize;
            collapse.dismissed.set(Some(direction));
            match (resize, geometry.size()) {
                (Some(duration), Some(size)) => {
                    let vsync = vsync.borrow().clone();
                    collapse.start(duration, vsync.as_ref(), &rebuild, size, &events, &collapse);
                    rebuild.schedule(flui_view::RebuildReason::AnimationTick);
                }
                _ => events.dispatch(DismissEvent::Dismissed(direction)),
            }
        })
    }
}

impl ViewState<Dismissible> for DismissibleState {
    fn did_update_view(&mut self, _old_view: &Dismissible, new_view: &Dismissible) {
        let previous = self.callbacks.replace(DismissCallbacks::from(new_view));
        drop(previous);
        let incoming = new_view.config();
        let outgoing = self.config.replace(incoming.clone());
        let changed =
            outgoing.direction != incoming.direction || outgoing.thresholds != incoming.thresholds;
        if changed {
            Self::reanchor(
                &self.controller,
                &self.geometry,
                &self.config,
                self.events.as_ref(),
                true,
            );
        }
    }

    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.geometry.acquire(ctx);
        let (axis, thresholds) = {
            let config = self.config.borrow();
            (config.direction.axis(), config.thresholds)
        };
        let events = Rc::new(DismissEvents {
            controller: self.controller.clone(),
            config: RefCell::new(UpdateConfig {
                axis,
                thresholds,
                extent: None,
            }),
            delivered: Cell::new((0.0, false)),
            writer: ctx.writer_source(),
            post_frame: ctx.post_frame_handle(),
            mounted: Cell::new(true),
            callbacks: Rc::clone(&self.callbacks),
            command: RefCell::new(None),
        });
        let moved = Rc::downgrade(&events);
        self.offset_listener = Some(self.controller.offset_listenable().add_listener(Rc::new(
            move || {
                if let Some(events) = moved.upgrade() {
                    events.moved();
                }
            },
        )));
        self.events = Some(events);
        let rebuild = ctx.rebuild_handle();
        self.uncover = Some(UncoverWatch::new(&self.controller, rebuild.clone()));
        self.rebuild = Some(rebuild);
        self.watch = Some(OutWatch::new(
            &self.controller,
            ctx.post_frame_handle(),
            self.rest_out(),
        ));
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.geometry.acquire(ctx);
        let vsync = VsyncScope::maybe_of(ctx);
        let outgoing = self.vsync.replace(vsync.clone());
        drop(outgoing);
        if let Err(error) = rebind_collapse(&self.collapse.controller, vsync.as_ref()) {
            tracing::error!(%error, "Dismissible lost its frame registry");
        }
    }

    /// The card inside its gesture alternatives, so the drag and every
    /// alternative perform one command.
    fn build(&self, view: &Dismissible, ctx: &dyn BuildContext) -> impl IntoView {
        self.motion.set(MediaQuery::motion_of(ctx));
        let events = self
            .events
            .clone()
            .expect("BUG: Dismissible initialized before build");
        let outgoing = events.command.replace(Some(view.alternatives.handle(ctx)));
        drop(outgoing);
        self.collapse.deliver_completion(&events);

        let collapsing = self
            .collapse
            .controller
            .borrow()
            .as_ref()
            .map(|controller| {
                let axis = view.direction.axis().unwrap_or(Axis::Horizontal);
                let prior = self
                    .collapse
                    .size_prior
                    .get()
                    .expect("BUG: a collapse starts from the row's committed size");
                let background = match self.collapse.dismissed.get() {
                    Some(DismissDirection::EndToStart | DismissDirection::Up) => view
                        .secondary_background
                        .clone()
                        .or_else(|| view.background.clone()),
                    _ => view.background.clone(),
                };
                resize_collapse_view(controller.controller(), axis, prior, background)
            });
        // The collapse is the dismissed row leaving: its controls leave with
        // the card instead of holding the row open at their height.
        if let Some(collapsing) = collapsing {
            return collapsing;
        }
        let card = self.card(view, ctx);
        view.alternatives.clone().child(card).boxed()
    }

    fn dispose(&mut self) {
        if let Some(watch) = self.watch.take() {
            watch.retire();
        }
        if let Some(uncover) = self.uncover.take() {
            uncover.retire();
        }
        if let Some(events) = &self.events {
            events.mounted.set(false);
        }
        if let Some(id) = self.offset_listener.take() {
            self.controller.offset_listenable().remove_listener(id);
        }
        self.collapse.retire();
        self.geometry.release();
    }
}

impl DismissibleState {
    fn card(&self, view: &Dismissible, ctx: &dyn BuildContext) -> BoxedView {
        let Some(axis) = view.direction.axis() else {
            return view.child.clone();
        };
        let controller = &self.controller;
        let (cancel, origin_cancel, motion) = (
            controller.clone(),
            Rc::clone(&self.origin),
            Rc::clone(&self.motion),
        );
        let drag = AnchoredDrag::new(controller.clone(), axis, view.child.clone())
            .withhold_semantics()
            .admission(ContactAdmission::outside_system_edges(ctx, axis))
            .on_cancelled(move || origin_cancel.cancelled(&cancel, motion.get()));
        let (down, origin_down) = (controller.clone(), Rc::clone(&self.origin));
        let (geometry, config) = (Rc::clone(&self.geometry), Rc::clone(&self.config));
        // Weak: the queued deliveries alone own a retired row's events.
        let events = self.events.as_ref().map(Rc::downgrade);
        // Every Down reaches the row before its drag captures the contact.
        let card = Listener::new()
            .on_pointer_down(move |_cx, _dispatch| {
                let events = events.as_ref().and_then(std::rc::Weak::upgrade);
                Self::reanchor(&down, &geometry, &config, events.as_ref(), false);
                origin_down.down(down.current());
            })
            .child(drag)
            .boxed();
        // Mounted while the card is away from rest, rebuilt when it crosses
        // to the other side; the slide itself rebuilds nothing.
        let background = match Uncovered::at(controller.offset()) {
            Some(Uncovered::Leading) => view.background.clone(),
            Some(Uncovered::Trailing) => view
                .secondary_background
                .clone()
                .or_else(|| view.background.clone()),
            None => None,
        };
        // The card keeps its place in the stack either way, so a contact on
        // it lives through the background coming and going.
        let behind = background.map_or_else(nothing_behind, |background| {
            Positioned::fill(
                ClipRect::new()
                    .clip_behavior(Clip::HardEdge)
                    .child(background),
            )
            .boxed()
        });
        self.geometry
            .measure(Stack::new(vec![behind, card]).fit(StackFit::Passthrough))
            .boxed()
    }
}

// ============================================================================
// View construction
// ============================================================================

/// Move the collapse owner, restoring its storage before any migration
/// callout.
fn rebind_collapse(
    slot: &RefCell<Option<DrivenController>>,
    vsync: Option<&Vsync>,
) -> Result<(), flui_animation::VsyncRegistrationError> {
    let mut collapse = slot.borrow_mut().take();
    let publication = VsyncUpdate::prepare(|update| {
        if let Some(owner) = collapse.as_mut() {
            update.rebind_controller(owner, vsync);
        }
    });
    // Binding preparation invokes no animation hooks. Restore the active owner and release
    // its slot before publication can wake, settle or reenter the widget.
    let outgoing = slot.replace(collapse);
    drop(outgoing);
    publication.publish()
}

/// The post-dismiss collapse: a background-filled box shrinking across the
/// dismiss axis, with a 40% pause followed by an eased collapse.
fn resize_collapse_view(
    resize_controller: &AnimationController,
    axis: Axis,
    prior: Size,
    background: Option<BoxedView>,
) -> BoxedView {
    let curved = Interval::new(0.4, 1.0, curves::EASE).transform(resize_controller.value());
    let factor = 1.0 - curved;
    let (width, height) = match axis {
        Axis::Horizontal => (prior.width, prior.height * factor),
        Axis::Vertical => (prior.width * factor, prior.height),
    };
    let collapsed = SizedBox::new(width, height);
    let collapsed = match background {
        Some(bg) => collapsed.child(bg),
        None => collapsed,
    };
    ClipRect::new()
        .clip_behavior(Clip::HardEdge)
        .child(collapsed)
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::rebind_collapse;
    use flui_animation::{Animation, AnimationController, MotionClock, Vsync};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        time::Duration,
    };

    // Only this seam can inspect shared owner storage from a migration hook
    // without the mounted lifecycle boundary retiring the faulted actor.
    #[test]
    fn collapse_owner_storage_is_committed_before_migration_callouts() {
        for fails in [false, true] {
            let old = Vsync::new();
            let next = Vsync::new();
            let resize = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
            let run = resize.controller().forward().expect("start collapse");
            let slot = Rc::new(RefCell::new(Some(resize)));
            let mut clock = MotionClock::new();
            old.tick_all(&clock.frame(Duration::ZERO));
            old.tick_all(&clock.frame(Duration::from_millis(100)));
            let called = Rc::new(Cell::new(false));
            next.set_frame_requester(Some(Rc::new({
                let (slot, called, old) = (Rc::downgrade(&slot), called.clone(), old);
                move || {
                    assert!(old.is_empty(), "every owner moved before wake");
                    let slot = slot.upgrade().expect("collapse owner still mounted");
                    let observer = slot
                        .borrow()
                        .as_ref()
                        .expect("active owner restored before wake")
                        .controller()
                        .clone();
                    assert!(observer.is_animating());
                    called.set(true);
                    assert!(!fails, "collapse migration delivery failure");
                }
            })));
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rebind_collapse(&slot, Some(&next))
            }));
            if fails {
                let payload = outcome.expect_err("failing wake propagates");
                assert_eq!(
                    flui_foundation::panic::payload_text(payload.as_ref()),
                    Some("collapse migration delivery failure")
                );
            } else {
                outcome
                    .expect("healthy delivery")
                    .expect("registry admitted");
            }
            assert!(called.get());
            assert!(!run.is_canceled());
            next.set_frame_requester(None);
            next.tick_all(&clock.frame(Duration::from_secs(2)));
            next.tick_all(&clock.frame(Duration::from_secs(3)));
            assert!(run.is_complete(), "retained collapse reaches completion");
            drop(slot);
            assert!(next.is_empty());
        }
    }
}
