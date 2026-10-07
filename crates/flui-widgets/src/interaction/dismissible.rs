//! [`Dismissible`] — drag a child out of view to dismiss it, then collapse the
//! space it occupied.
//!
//! State machine: a drag (or a
//! sufficiently fast fling) accumulates a signed `drag_extent`; releasing past
//! [`Dismissible::dismiss_threshold`] (default `0.4`, per direction) or with a
//! qualifying fling drives `move_controller` to 1.0 over
//! [`Dismissible::movement_duration`]; on completion the widget starts the
//! resize collapse over [`Dismissible::resize_duration`] (or fires
//! [`Dismissible::on_dismissed`] immediately when that is `None`) and then
//! fires `on_dismissed`.
//!
//! # Known limits (framework-surface gaps)
//!
//! 1. **No `confirm_dismiss`.** An async veto gate awaited between the move
//!    animation completing and the resize collapse starting would need a
//!    widget-level "await a caller future, then keep going" seam, which FLUI
//!    does not have yet (`FutureBuilder` rebuilds *from* future state; it does
//!    not let an imperative callback block a state transition on one).
//!    Inventing that seam here, one-off, for a single widget would be a local
//!    hack; it is deferred rather than faked with a synchronous stand-in that
//!    would misrepresent an async, vetoable contract.
//! 2. **No progressive background clip.** Ideally `background` is revealed
//!    only as the sliver between the sliding child's edge and the container
//!    edge, growing as the drag proceeds. [`ClipRect`] has no
//!    arbitrary-rect / custom-clipper primitive yet — only a fixed
//!    [`flui_painting::paint::Clip`] behavior. This widget shows/hides
//!    `background` by *presence* (mounted whenever `move_controller.value()
//!    != 0.0`) but does not crop it to the revealed sliver — it paints at full
//!    extent under the sliding child from the first pixel of drag. The
//!    presence/absence signal is exact; the crop is visually observable.
//! 3. **`Vertical`/`Up`/`Down` ride `on_pan_*`, not a vertical-drag family.**
//!    `GestureDetector` has no `on_vertical_drag_*` recognizer family (see
//!    that type's own docs on why) — only `on_horizontal_drag_*` and the
//!    free-axis `on_pan_*`. For the vertical-family directions this widget
//!    wires `on_pan_*` and reads the raw `delta.dy` / `velocity.dy` component
//!    directly instead of `primary_delta` / `primary_velocity` (which are
//!    `Free`-axis distance *magnitudes* on that recognizer, not the signed
//!    per-axis component the math needs). Functionally equivalent
//!    for the one component this widget reads, but slightly looser: a pan
//!    recognizer's slop is not axis-locked the way a dedicated vertical
//!    recognizer's would be, so a mostly-horizontal drag can still start a
//!    vertical `Dismissible`'s gesture. Horizontal-family directions are
//!    unaffected — they use the real `on_horizontal_drag_*` family.
//! 4. **No live "my own size" query.** Event handlers run well after `build`
//!    and would need this widget's last-laid-out size, but FLUI's
//!    `BuildContext` has no such accessor. This widget
//!    wraps its content in [`LayoutBuilder`] instead and
//!    uses the incoming `BoxConstraints` (`max_width`/`max_height`) as the
//!    drag-axis extent and the resize collapse's prior size — exact when the
//!    constraints are tight (the common case: a fixed-extent list item), but
//!    **`Dismissible` requires bounded constraints along its dismiss axis**;
//!    an unbounded axis has no extent to divide the drag fraction by.
//! 5. **No keep-alive.** A mid-flight `Dismissible` can be disposed by a lazy
//!    list's viewport GC. FLUI has no keep-alive mechanism at all yet — a
//!    framework-wide gap, not specific to this widget.
//! 6. **No required `Key`.** A dismissed list item's slot is not re-synced
//!    onto the next item by index, because FLUI's reconciliation is not
//!    index-keyed that way; a key is orthogonal to the
//!    drag/threshold/callback behavior here.
//! 7. **No drag-start behavior.** The drag's origin is where the gesture *won
//!    the arena*, never where the initial *down* event landed (more
//!    reactive). [`GestureDetector`] has no drag-start-behavior concept at all
//!    yet, so this is not configurable, for the same reason divergence #3's
//!    vertical-drag family and #5's keep-alive gap aren't: the primitive this
//!    widget would delegate to does not exist in FLUI yet.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use flui_animation::curve::{Curve, Interval};
use flui_animation::{
    Animation, AnimationController, AnimationStatus, Curves, Vsync, VsyncRegistration,
};
use flui_foundation::geometry::Size;
use flui_foundation::{Listenable, ListenerId};
use flui_interaction::{DragEndDetails, DragStartDetails, DragUpdateDetails};
use flui_painting::paint::Clip;
use flui_painting::typography::TextDirection;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{
    BoxedView, BuildContextExt, EventCx, EventOutcome, IntoView, LocalPostFrameHandle,
    RebuildHandle, ViewExt, ViewState, WriterSource,
};

use crate::animated::VsyncScope;
use crate::localization::Directionality;
use crate::{
    ClipRect, FractionalTranslation, GestureDetector, LayoutBuilder, Positioned, SizedBox, Stack,
};

/// Minimum fling speed — a fling below this speed
/// never dismisses, regardless of direction.
const MIN_FLING_VELOCITY: f64 = 700.0;
/// Minimum margin — the primary-axis
/// velocity must clear the cross-axis velocity by at least this much, or the
/// gesture is not "generally in the right direction".
const MIN_FLING_VELOCITY_DELTA: f64 = 400.0;
/// The default fraction of
/// `overall_drag_axis_extent` that must be crossed to dismiss.
const DEFAULT_DISMISS_THRESHOLD: f64 = 0.4;

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

/// Fired when the [`Dismissible`] has been dismissed, after any resize
/// collapse has finished (or immediately, if `resize_duration` is `None`).
pub type DismissDirectionCallback = Rc<dyn Fn(&mut EventCx<'_>, DismissDirection)>;

/// Fired on every drag/threshold-state change while a [`Dismissible`] is
/// being dragged.
pub type DismissUpdateCallback = Rc<dyn Fn(&mut EventCx<'_>, DismissUpdateDetails)>;

type ResizeCallback = Rc<dyn Fn(&mut EventCx<'_>)>;
type DirectionDelivery = Rc<dyn Fn(DismissDirection)>;
type UpdateDelivery = Rc<dyn Fn(DismissUpdateDetails)>;

/// Details delivered to [`Dismissible::on_update`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DismissUpdateDetails {
    /// The direction the dismissible is currently being dragged toward.
    pub direction: DismissDirection,
    /// Whether the dismiss threshold is reached as of this delivery.
    pub reached: bool,
    /// Whether the dismiss threshold was reached as of the *previous*
    /// delivery — pairs with `reached` to catch the crossing moment.
    pub previous_reached: bool,
    /// `move_controller`'s value: `0.0` at rest, `1.0` fully off-screen.
    pub progress: f64,
}

/// A widget that can be dismissed by dragging in [`DismissDirection`].
///
/// See the module docs for the documented
/// limits (no `confirm_dismiss`, no progressive background clip, no
/// vertical-drag recognizer family, bounded-constraints contract, no
/// keep-alive, no required key).
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
    dismiss_thresholds: HashMap<DismissDirection, f64>,
    movement_duration: Duration,
    cross_axis_end_offset: f64,
    behavior: HitTestBehavior,
}

impl Dismissible {
    /// A `Dismissible` wrapping `child`, dismissible horizontally by default
    /// collapsing over 300ms after a 200ms slide.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
            background: None,
            secondary_background: None,
            on_resize: None,
            on_dismissed: None,
            on_update: None,
            direction: DismissDirection::Horizontal,
            resize_duration: Some(Duration::from_millis(300)),
            dismiss_thresholds: HashMap::new(),
            movement_duration: Duration::from_millis(200),
            cross_axis_end_offset: 0.0,
            behavior: HitTestBehavior::Opaque,
        }
    }

    /// A widget stacked behind `child`, exposed as it slides away. Shown at
    /// full extent whenever the drag offset is nonzero (see divergence #2 in
    /// the module docs — not clipped to the revealed sliver).
    #[must_use]
    pub fn background(mut self, background: impl IntoView) -> Self {
        self.background = Some(background.into_view().boxed());
        self
    }

    /// A widget shown behind `child` instead of [`Self::background`] while
    /// dragging toward [`DismissDirection::EndToStart`] or
    /// [`DismissDirection::Up`]. Only meaningful once `background` is set.
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

    /// Called once the dismissible has been dismissed — after the resize
    /// collapse finishes, or immediately if `resize_duration` is `None`.
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

    /// Called on every drag update with the current direction/threshold
    /// state.
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

    /// Overrides the dismiss threshold (fraction of the drag-axis extent)
    /// for one [`DismissDirection`]. Unset directions use the default
    /// (`0.4`). A threshold `>= 1.0` makes that direction undismissable by
    /// drag or fling, even though [`Self::direction`] still allows dragging
    /// it (it always springs back).
    #[must_use]
    pub fn dismiss_threshold(mut self, direction: DismissDirection, threshold: f64) -> Self {
        self.dismiss_thresholds.insert(direction, threshold);
        self
    }

    /// The duration of the slide-to-dismiss / spring-back animation.
    /// Default: `200ms`.
    #[must_use]
    pub fn movement_duration(mut self, movement_duration: Duration) -> Self {
        self.movement_duration = movement_duration;
        self
    }

    /// The end-of-slide offset across the axis perpendicular to the dismiss
    /// direction, as a fraction of the widget's extent on that axis. Default
    /// `0.0` (no cross-axis drift).
    #[must_use]
    pub fn cross_axis_end_offset(mut self, cross_axis_end_offset: f64) -> Self {
        self.cross_axis_end_offset = cross_axis_end_offset;
        self
    }

    /// How this widget behaves during hit-testing. Default:
    /// [`HitTestBehavior::Opaque`].
    #[must_use]
    pub fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }
}

impl std::fmt::Debug for Dismissible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dismissible")
            .field("direction", &self.direction)
            .field("resize_duration", &self.resize_duration)
            .field("movement_duration", &self.movement_duration)
            .field("has_background", &self.background.is_some())
            .finish_non_exhaustive()
    }
}

// ============================================================================
// Direction / threshold / fling helpers — free functions so the state
// machine's math is unit-testable without a laid-out tree.
// ============================================================================

/// Whether `direction` drags along the horizontal axis.
fn direction_is_x_axis(direction: DismissDirection) -> bool {
    matches!(
        direction,
        DismissDirection::Horizontal | DismissDirection::EndToStart | DismissDirection::StartToEnd
    )
}

/// `extent.sign`, but `0.0` maps to `0.0` (Rust's `f64::signum` maps `+0.0` to
/// `1.0`, which would wrongly treat "no drag yet" as "dragged positive").
fn drag_sign(extent: f64) -> f64 {
    if extent == 0.0 { 0.0 } else { extent.signum() }
}

/// The direction a signed extent points toward for `direction`.
fn extent_to_direction(
    extent: f64,
    direction: DismissDirection,
    text_direction: TextDirection,
) -> DismissDirection {
    if extent == 0.0 {
        return DismissDirection::None;
    }
    if direction_is_x_axis(direction) {
        match (text_direction, extent) {
            (TextDirection::Rtl, e) if e < 0.0 => DismissDirection::StartToEnd,
            (TextDirection::Ltr, e) if e > 0.0 => DismissDirection::StartToEnd,
            (TextDirection::Rtl | TextDirection::Ltr, _) => DismissDirection::EndToStart,
        }
    } else if extent > 0.0 {
        DismissDirection::Down
    } else {
        DismissDirection::Up
    }
}

/// Accumulates `delta` into `current` only
/// when doing so keeps the extent on the side `direction` (and, for the
/// reading-direction-relative variants, `text_direction`) allows.
fn accumulate_drag_extent(
    direction: DismissDirection,
    text_direction: TextDirection,
    current: f64,
    delta: f64,
) -> f64 {
    let proposed = current + delta;
    match direction {
        DismissDirection::Horizontal | DismissDirection::Vertical => proposed,
        DismissDirection::Up => {
            if proposed < 0.0 {
                proposed
            } else {
                current
            }
        }
        DismissDirection::Down => {
            if proposed > 0.0 {
                proposed
            } else {
                current
            }
        }
        DismissDirection::EndToStart => match text_direction {
            TextDirection::Rtl if proposed > 0.0 => proposed,
            TextDirection::Ltr if proposed < 0.0 => proposed,
            TextDirection::Rtl | TextDirection::Ltr => current,
        },
        DismissDirection::StartToEnd => match text_direction {
            TextDirection::Rtl if proposed < 0.0 => proposed,
            TextDirection::Ltr if proposed > 0.0 => proposed,
            TextDirection::Rtl | TextDirection::Ltr => current,
        },
        DismissDirection::None => 0.0,
    }
}

/// The resolved threshold for `direction`: the per-direction override, else the
/// default.
fn dismiss_threshold_for(
    thresholds: &HashMap<DismissDirection, f64>,
    direction: DismissDirection,
) -> f64 {
    thresholds
        .get(&direction)
        .copied()
        .unwrap_or(DEFAULT_DISMISS_THRESHOLD)
}

/// How a release velocity relates to the drag axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlingGestureKind {
    /// Too slow, or not clearly aimed along the drag axis — not a fling.
    None,
    /// Fast enough, aimed the same way the drag is already leaning.
    Forward,
    /// Fast enough, aimed the opposite way.
    Reverse,
}

/// Classifies a release velocity as a fling toward the dismiss edge, away
/// from it, or no fling.
fn describe_fling_gesture(
    drag_extent: f64,
    direction: DismissDirection,
    text_direction: TextDirection,
    primary_velocity: f64,
    cross_velocity: f64,
) -> FlingGestureKind {
    if drag_extent == 0.0 {
        return FlingGestureKind::None;
    }
    if primary_velocity.abs() - cross_velocity.abs() < MIN_FLING_VELOCITY_DELTA
        || primary_velocity.abs() < MIN_FLING_VELOCITY
    {
        return FlingGestureKind::None;
    }
    let fling_direction = extent_to_direction(primary_velocity, direction, text_direction);
    let dismiss_direction = extent_to_direction(drag_extent, direction, text_direction);
    if fling_direction == dismiss_direction {
        FlingGestureKind::Forward
    } else {
        FlingGestureKind::Reverse
    }
}

// ============================================================================
// STATE
// ============================================================================

/// Interior-mutable drag/animation progress, shared (via `Rc`) between
/// `DismissibleState` and the `'static` `GestureDetector` closures `build()`
/// reconstructs every rebuild.
///
/// Kept out of `AnimationController` listener closures (which must be
/// `Send + Sync`, per `flui_foundation::ListenerCallback`): those closures
/// only ever touch the `Arc<Atomic*>` signal fields below, never this
/// `Rc`/`Cell`-based state directly. `build()` (single-threaded, run on the
/// frame/build thread) is the only place that reads or reacts to those
/// signals and mutates this state.
#[derive(Default)]
struct DragState {
    /// Signed pixel extent dragged so far.
    drag_extent: Cell<f64>,
    /// Whether a drag contact is currently down.
    drag_underway: Cell<bool>,
    /// The size occupied right before the resize collapse began.
    size_prior_to_collapse: Cell<Option<Size>>,
    /// The dismiss-threshold-reached flag from the last `on_update`
    /// delivery, for `DismissUpdateDetails::previous_reached`.
    dismiss_threshold_reached: Cell<bool>,
    /// `move_controller.value()` at the last `on_update` delivery, so an
    /// unrelated rebuild (e.g. a parent prop change) that leaves the drag
    /// position untouched does not re-fire `on_update`.
    last_delivered_move_value: Cell<f64>,

    /// `move_controller`'s current `Vsync` registration — re-registered (not
    /// just registered once) on every direct `set_value` while dragging; see
    /// `reanchor_move_controller_vsync`'s doc for why.
    move_vsync_registration: RefCell<Option<VsyncRegistration>>,

    /// Lazily created once the move animation completes past threshold.
    resize_controller: RefCell<Option<AnimationController>>,
    resize_listener_id: RefCell<Option<ListenerId>>,
    resize_vsync_registration: RefCell<Option<VsyncRegistration>>,

    /// Bumped by `move_controller`'s status listener on every transition to
    /// `Completed`. `Send + Sync` (an atomic), so the listener may touch it
    /// directly; `build()` diffs it against `delivered_move_completions`.
    ///
    /// All atomics in this struct use `Relaxed`: they carry bare counts, not
    /// data publication. Every consumer runs in `build()` after the
    /// listener's `RebuildHandle::schedule` call, whose queue
    /// synchronization already orders the listener-side bump before the
    /// rebuild that reads it; a load racing a concurrent tick can at worst
    /// miss an increment that the tick's own scheduled rebuild then
    /// delivers.
    move_completed_runs: Arc<AtomicU64>,
    delivered_move_completions: Cell<u64>,

    /// Bumped by the resize controller's listener on every non-final tick.
    resize_progress_ticks: Arc<AtomicU64>,
    delivered_resize_ticks: Cell<u64>,
    /// Set by the resize controller's listener once it observes completion.
    resize_completed: Arc<AtomicBool>,
    delivered_resize_dismissal: Cell<bool>,
}

/// Configuration captured once per `build()` as an owned (non-borrowing)
/// snapshot, so the `'static` closures `build()` constructs — invoked later,
/// against whatever `view` was current when they were built — read a
/// consistent value instead of a borrow that cannot outlive `build()`.
#[derive(Clone)]
struct ResolvedConfig {
    direction: DismissDirection,
    text_direction: TextDirection,
    dismiss_thresholds: HashMap<DismissDirection, f64>,
    resize_duration: Option<Duration>,
    cross_axis_end_offset: f64,
    on_dismissed: Option<DirectionDelivery>,
    on_resize: Option<Rc<dyn Fn()>>,
}

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

/// Layout discovers transitions but cannot run user effects. Snapshot their
/// payloads there, then dispatch after the frame through the latest callbacks.
/// The input-time completion bypass dispatches immediately with the same source.
struct DismissEvents {
    writer: WriterSource,
    post_frame: Option<LocalPostFrameHandle>,
    mounted: Cell<bool>,
    callbacks: Rc<RefCell<DismissCallbacks>>,
}

impl DismissEvents {
    fn dispatch(&self, event: DismissEvent) {
        if !self.mounted.get() {
            return;
        }
        let callbacks = self.callbacks.borrow().clone();
        self.writer.write(|cx| match event {
            DismissEvent::Resize => {
                if let Some(callback) = callbacks.resize {
                    callback(cx);
                }
            }
            DismissEvent::Dismissed(direction) => {
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
        let events = self.clone();
        if let Err(error) = post_frame.schedule_local(move |_| events.dispatch(event)) {
            tracing::warn!(
                ?error,
                "Dismissible: event dropped because the owner post-frame lane is closed"
            );
        }
    }
}

/// State for [`Dismissible`]. Owns the persistent `move_controller` (created
/// once) plus the shared
/// `DragState` and the [`RebuildHandle`] acquired in `init_state` (per
/// ADR-0018 — never acquired from `build`/layout) that the lazily created
/// resize controller's listener needs later.
pub struct DismissibleState {
    events: Option<Rc<DismissEvents>>,
    callbacks: Rc<RefCell<DismissCallbacks>>,
    move_controller: AnimationController,
    move_value_listener_id: Option<ListenerId>,
    move_status_listener_id: Option<ListenerId>,
    vsync: Option<Vsync>,
    rebuild: Option<RebuildHandle>,
    drag: Rc<DragState>,
}

impl std::fmt::Debug for DismissibleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DismissibleState")
            .field("drag_extent", &self.drag.drag_extent.get())
            .field("drag_underway", &self.drag.drag_underway.get())
            .field("resizing", &self.drag.resize_controller.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for Dismissible {
    type State = DismissibleState;

    fn create_state(&self) -> Self::State {
        // A real, but permanently detached, ticker -- not `without_ticker`:
        // this module reads `move_controller.is_animating()` extensively
        // (`handle_drag_start`/`handle_drag_update`/`handle_drag_end`), and
        // `is_animating` is intentionally ticker-based,
        // not status-based — a ticker-less controller
        // can never report `is_animating() == true`. `VsyncScope` still
        // drives the actual value ticks deterministically via `tick_at`;
        // `with_detached_ticker` gives this controller a ticker whose
        // `start()`/`stop()` transition real ticker state without needing an
        // `UpdateScheduler` at all.
        let move_controller = AnimationController::with_detached_ticker(self.movement_duration);
        DismissibleState {
            events: None,
            callbacks: Rc::new(RefCell::new(DismissCallbacks::from(self))),
            move_controller,
            move_value_listener_id: None,
            move_status_listener_id: None,
            vsync: None,
            rebuild: None,
            drag: Rc::new(DragState::default()),
        }
    }
}

impl ViewState<Dismissible> for DismissibleState {
    fn did_update_view(&mut self, _old_view: &Dismissible, new_view: &Dismissible) {
        let previous = self.callbacks.replace(DismissCallbacks::from(new_view));
        drop(previous);
    }

    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.events = Some(Rc::new(DismissEvents {
            writer: ctx.writer_source(),
            post_frame: ctx.local_post_frame_handle(),
            mounted: Cell::new(true),
            callbacks: self.callbacks.clone(),
        }));
        let rebuild = ctx.rebuild_handle();

        let rebuild_for_value = rebuild.clone();
        self.move_value_listener_id =
            Some(self.move_controller.add_listener(Arc::new(move || {
                rebuild_for_value.schedule(flui_view::RebuildReason::AnimationTick);
            })));

        let move_completed_runs = Arc::clone(&self.drag.move_completed_runs);
        let rebuild_for_status = rebuild.clone();
        self.move_status_listener_id = Some(self.move_controller.add_status_listener(Arc::new(
            move |status| {
                if status == AnimationStatus::Completed {
                    move_completed_runs.fetch_add(1, Ordering::Relaxed);
                    rebuild_for_status.schedule(flui_view::RebuildReason::AnimationTick);
                }
            },
        )));

        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register(self.move_controller.clone());
            *self.drag.move_vsync_registration.borrow_mut() = Some(registration);
            self.vsync = Some(vsync);
        }

        self.rebuild = Some(rebuild);
    }

    fn build(&self, view: &Dismissible, ctx: &dyn BuildContext) -> impl IntoView {
        // An ambient `Directionality` matters only
        // for the X-axis directions (`EndToStart`/`StartToEnd` read it to
        // resolve against the reading direction; plain `Horizontal` reads it
        // too, for `DismissUpdateDetails`/`on_dismissed`'s reported
        // direction). `Vertical`/`Up`/`Down`/`None` never consult
        // `text_direction` (see `accumulate_drag_extent`/`extent_to_direction`),
        // so defaulting instead of requiring an ancestor here avoids an
        // unnecessary hard panic for callers who never wrap a purely-vertical
        // `Dismissible` in one.
        // ...and for the same reason the lookup itself is GATED, not merely
        // defaulted. `Directionality::maybe_of` registers an inherited
        // dependency, so calling it for a purely-vertical `Dismissible` would
        // rebuild it on every ambient direction change while
        // `extent_to_direction` -- the only consumer -- ignores the value on
        // that path.
        let text_direction = if direction_is_x_axis(view.direction) {
            Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr)
        } else {
            TextDirection::Ltr
        };
        let events = self
            .events
            .clone()
            .expect("BUG: Dismissible initialized before build");
        let direct_events = events.clone();
        let resolved = Rc::new(ResolvedConfig {
            direction: view.direction,
            text_direction,
            dismiss_thresholds: view.dismiss_thresholds.clone(),
            resize_duration: view.resize_duration,
            cross_axis_end_offset: view.cross_axis_end_offset,
            on_dismissed: view.on_dismissed.as_ref().map(|_| {
                Rc::new(move |direction| direct_events.dispatch(DismissEvent::Dismissed(direction)))
                    as DirectionDelivery
            }),
            on_resize: None,
        });
        let mut deferred = (*resolved).clone();
        let dismiss_events = events.clone();
        deferred.on_dismissed = view.on_dismissed.as_ref().map(|_| {
            Rc::new(move |direction| dismiss_events.defer(DismissEvent::Dismissed(direction)))
                as DirectionDelivery
        });
        let resize_events = events.clone();
        deferred.on_resize = view
            .on_resize
            .as_ref()
            .map(|_| Rc::new(move || resize_events.defer(DismissEvent::Resize)) as Rc<dyn Fn()>);
        let deferred = Rc::new(deferred);
        let on_update: Option<UpdateDelivery> = view.on_update.as_ref().map(|_| {
            Rc::new(move |details| events.defer(DismissEvent::Update(details))) as UpdateDelivery
        });
        let behavior = view.behavior;
        let direction = view.direction;
        let child = view.child.clone();
        let background = resolve_background(view, self.drag.drag_extent.get(), text_direction);
        let move_controller = self.move_controller.clone();
        let drag = Rc::clone(&self.drag);
        let vsync = self.vsync.clone();
        // `rebuild` is set in `init_state`, which always runs before the
        // first `build` — see `ViewState`'s lifecycle contract.
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: Dismissible::build ran before init_state acquired a RebuildHandle");

        LayoutBuilder::new(move |_ctx, constraints| {
            let axis_is_x = direction_is_x_axis(direction);
            let overall_extent = if axis_is_x {
                constraints.max_width
            } else {
                constraints.max_height
            };
            // Module docs divergence #4: this widget divides by
            // `overall_extent` to turn a drag delta into a fraction, so it
            // requires bounded constraints along the dismiss axis. An
            // unbounded axis (`f64::INFINITY`) would silently produce a
            // stuck-at-zero or NaN drag fraction instead of a loud failure —
            // catch the caller error here instead.
            debug_assert!(
                overall_extent.is_finite(),
                "BUG: Dismissible requires bounded constraints along its dismiss axis \
                 (got an unbounded/non-finite extent) — see module docs divergence #4"
            );

            // This closure is an ordinary build pass serviced between layout
            // passes. Resolve the animation transitions with these constraints,
            // but dispatch their user effects only after the frame: a signal
            // write from here would be rejected as WrittenDuringBuild.
            deliver_move_completion(
                &drag,
                &move_controller,
                &deferred,
                vsync.as_ref(),
                &rebuild,
                constraints,
            );
            deliver_resize_progress(&drag, &deferred);
            deliver_on_update(
                &drag,
                &move_controller,
                direction,
                text_direction,
                &resolved,
                on_update.as_ref(),
            );

            if let Some(resize_controller) = drag.resize_controller.borrow().as_ref() {
                let prior = drag.size_prior_to_collapse.get().expect(
                    "BUG: resize_controller exists only after size_prior_to_collapse is set",
                );
                return resize_collapse_view(
                    resize_controller,
                    axis_is_x,
                    prior,
                    background.clone(),
                );
            }

            let content = sliding_content_view(
                &move_controller,
                drag.drag_extent.get(),
                direction,
                resolved.cross_axis_end_offset,
                child.clone(),
            );
            let content = match background.clone() {
                Some(bg) if move_controller.value() != 0.0 => Stack::new(vec![
                    Positioned::fill(ClipRect::new().clip_behavior(Clip::HardEdge).child(bg))
                        .boxed(),
                    content.boxed(),
                ])
                .boxed(),
                _ => content.boxed(),
            };

            if direction == DismissDirection::None {
                return content;
            }

            let mut detector = GestureDetector::new().behavior(behavior);
            let drag_for_start = Rc::clone(&drag);
            let controller_for_start = move_controller.clone();
            let vsync_for_start = vsync.clone();
            let drag_for_update = Rc::clone(&drag);
            let controller_for_update = move_controller.clone();
            let resolved_for_update = Rc::clone(&resolved);
            let drag_for_end = Rc::clone(&drag);
            let controller_for_end = move_controller.clone();
            let resolved_for_end = Rc::clone(&resolved);
            let vsync_for_end = vsync.clone();
            let rebuild_for_end = rebuild.clone();

            if axis_is_x {
                detector = detector
                    .on_horizontal_drag_start(move |_cx, _details: DragStartDetails| {
                        handle_drag_start(
                            &drag_for_start,
                            &controller_for_start,
                            vsync_for_start.as_ref(),
                            overall_extent,
                        );
                    })
                    .on_horizontal_drag_update(move |_cx, details: DragUpdateDetails| {
                        handle_drag_update(
                            &drag_for_update,
                            &controller_for_update,
                            direction,
                            resolved_for_update.text_direction,
                            overall_extent,
                            details.delta.dx,
                        );
                    })
                    .on_horizontal_drag_end(move |_cx, details: DragEndDetails| {
                        handle_drag_end(
                            &drag_for_end,
                            &controller_for_end,
                            &resolved_for_end,
                            vsync_for_end.as_ref(),
                            &rebuild_for_end,
                            constraints,
                            details.reason,
                            details.velocity.pixels_per_second.dx,
                            details.velocity.pixels_per_second.dy,
                        );
                    });
            } else {
                detector = detector
                    .on_pan_start(move |_cx, _details: DragStartDetails| {
                        handle_drag_start(
                            &drag_for_start,
                            &controller_for_start,
                            vsync_for_start.as_ref(),
                            overall_extent,
                        );
                    })
                    .on_pan_update(move |_cx, details: DragUpdateDetails| {
                        handle_drag_update(
                            &drag_for_update,
                            &controller_for_update,
                            direction,
                            resolved_for_update.text_direction,
                            overall_extent,
                            details.delta.dy,
                        );
                    })
                    .on_pan_end(move |_cx, details: DragEndDetails| {
                        handle_drag_end(
                            &drag_for_end,
                            &controller_for_end,
                            &resolved_for_end,
                            vsync_for_end.as_ref(),
                            &rebuild_for_end,
                            constraints,
                            details.reason,
                            details.velocity.pixels_per_second.dy,
                            details.velocity.pixels_per_second.dx,
                        );
                    });
            }

            detector.child(content).boxed()
        })
    }

    fn dispose(&mut self) {
        if let Some(events) = &self.events {
            events.mounted.set(false);
        }
        if let Some(id) = self.move_value_listener_id.take() {
            self.move_controller.remove_listener(id);
        }
        if let Some(id) = self.move_status_listener_id.take() {
            self.move_controller.remove_status_listener(id);
        }
        if let (Some(vsync), Some(registration)) =
            (&self.vsync, self.drag.move_vsync_registration.take())
        {
            vsync.unregister(&registration);
        }
        self.move_controller.dispose();

        let resize_controller = self.drag.resize_controller.borrow_mut().take();
        if let Some(resize_controller) = resize_controller {
            if let Some(id) = self.drag.resize_listener_id.borrow_mut().take() {
                resize_controller.remove_listener(id);
            }
            if let (Some(vsync), Some(registration)) =
                (&self.vsync, self.drag.resize_vsync_registration.take())
            {
                vsync.unregister(&registration);
            }
            resize_controller.dispose();
        }
    }
}

// ============================================================================
// Gesture handlers — free functions called from the `'static` closures `build()` reconstructs.
// ============================================================================

/// Unregisters `move_controller` from `vsync` for the duration of a raw drag.
///
/// `set_value` (called on every drag update) leaves the controller's `AnimationStatus`
/// at `Forward`/`Reverse` for any value strictly between the bounds — the
/// same status a REAL `.forward()`/`.reverse()` run leaves it at
/// (`AnimationController::settled_status_keep_direction`, which "keeps
/// direction" rather than reporting a settled `Dismissed`/`Completed` for a
/// non-bound value). `Vsync::tick_all` gates ticking on
/// `status().is_running()` — indistinguishable, from `Vsync`'s side, from a
/// genuine run in progress — so it ticks the controller via `tick_at`, which
/// recomputes `value` from the controller's own internal run epoch
/// (`AnimationController::tick_time_based`) rather than treating `set_value`
/// as authoritative. Since `set_value` does not update that internal epoch
/// (only `.forward()`/`.reverse()`/`.fling()` do), any tick while merely
/// drag-tracking silently overwrites the value just written — with a STALE
/// epoch this drifts gradually; even freshly re-anchoring on every update
/// (an earlier, insufficient attempt at this fix) still overwrites it
/// immediately, just with a near-zero value instead of a drifting one. The
/// only way to keep a direct `set_value` authoritative is to make sure the
/// controller is not ticked AT ALL while it is happening — hence full
/// unregistration for the drag's duration, paired with
/// [`ensure_move_controller_registered`] re-registering right before the
/// REAL run (`.forward()`/`.reverse()`/`.fling()`) that should actually be
/// vsync-driven.
///
/// This is a real interaction gap between `AnimationController::set_value`
/// and `Vsync::tick_all` (the latter should likely gate on the ticker-based
/// `is_animating()` this module already prefers elsewhere, not the
/// status-based `is_running()`, and/or `tick_at` should no-op when nothing
/// bumped the run epoch since the last tick) — worth fixing at the
/// `flui-animation` layer for every future widget that combines direct
/// value-tracking with vsync-driven settling on the same controller, not
/// just this one.
fn unregister_move_controller_vsync(drag: &DragState, vsync: Option<&Vsync>) {
    let (Some(vsync), Some(registration)) = (vsync, drag.move_vsync_registration.take()) else {
        return;
    };
    vsync.unregister(&registration);
}

/// Re-registers `move_controller` with `vsync` if it is not already
/// registered — called right before a REAL run
/// (`.forward()`/`.reverse()`/`.fling()`) starts, so `Vsync`'s tick anchor and
/// the controller's own internal run epoch both correspond to "now", the
/// run's true start. See [`unregister_move_controller_vsync`]'s doc for the
/// full rationale.
fn ensure_move_controller_registered(
    drag: &DragState,
    move_controller: &AnimationController,
    vsync: Option<&Vsync>,
) {
    let Some(vsync) = vsync else { return };
    if drag.move_vsync_registration.borrow().is_some() {
        return;
    }
    let registration = vsync.register(move_controller.clone());
    *drag.move_vsync_registration.borrow_mut() = Some(registration);
}

/// Marks whatever `move_completed_runs` currently holds as already
/// "delivered", discarding any `Completed` transition a direct `set_value`
/// call (in `handle_drag_start`/`handle_drag_update`) might just have caused
/// — without running the actual completion behavior for it.
///
/// **The bug this fixes:** `move_controller.set_value(...)` clamps to
/// `[0.0, 1.0]`, and a drag whose extent reaches (or overshoots) 100% of
/// `overall_extent` therefore lands the controller at the upper bound —
/// which `AnimationController` reports as `Completed`, *mid-drag*, well
/// before `handle_drag_end` ever runs. The right behavior is to discard
/// exactly this case — a `Completed` event that fires while still dragging is
/// dropped outright, never queued for later.
///
/// That check cannot live *in the listener*: the listener
/// registered in `init_state` must be `Send + Sync` (`flui_foundation::ListenerCallback`),
/// so it can only touch the `Arc<AtomicU64>` `move_completed_runs` counter,
/// never the `Cell<bool>` `drag_underway` flag (see `DragState`'s own doc on
/// why). Bumping the counter unconditionally and
/// leaving `deliver_move_completion`'s build()-driven consumer to skip
/// delivery *while* `drag_underway` was still true — but skipping is not
/// discarding: the bump stayed on the counter, unconsumed. The very next
/// time `deliver_move_completion` ran with `drag_underway` false again (e.g.
/// after the user dragged back below threshold and released, which
/// correctly springs back via `.reverse()`), it saw a "new" completion it
/// had never delivered and ran the collapse + `on_dismissed` anyway — a
/// false dismissal.
///
/// The fix moves the discard to the only place that reliably knows
/// `drag_underway` is true: right here, synchronously after every direct
/// `set_value`, in the same call stack that might have just caused the
/// bump. Marking it "delivered" immediately means `deliver_move_completion`
/// can never later mistake it for a real, undelivered completion. The
/// legitimate "released exactly at 100%" dismissal does not depend on this
/// counter at all: `handle_drag_end` calls [`run_move_completion`] directly
/// and unconditionally when `move_controller.is_completed()` holds at the
/// exact moment of release, which never consults the status
/// listener.
fn discard_transient_move_completion(drag: &DragState) {
    let completed_runs = drag.move_completed_runs.load(Ordering::Relaxed);
    drag.delivered_move_completions.set(completed_runs);
}

/// Begins a drag (no confirm-dismiss guard — see module docs limit #1).
fn handle_drag_start(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    vsync: Option<&Vsync>,
    overall_extent: f64,
) {
    drag.drag_underway.set(true);
    if move_controller.is_animating() {
        let sign = drag_sign(drag.drag_extent.get());
        drag.drag_extent
            .set(move_controller.value() * overall_extent * sign);
        let _ = move_controller.stop();
    } else {
        drag.drag_extent.set(0.0);
        move_controller.set_value(0.0);
    }
    // Unregister for the drag's duration — see `unregister_move_controller_vsync`'s
    // doc for why a vsync-ticked controller cannot also be a direct-`set_value`-tracked
    // one at the same time.
    unregister_move_controller_vsync(drag, vsync);
    // A `set_value(0.0)` above cannot itself clamp to the upper bound, but a
    // resumed-mid-animation `set_value` (the `is_animating()` branch) could in
    // principle land exactly at a bound too — discard defensively; see
    // `discard_transient_move_completion`'s doc. This same discard also eats a
    // real `.forward()` completion in the sub-frame window where the run
    // finished but its scheduled build has not yet delivered — a deliberate
    // user-interaction-wins divergence: the new drag resets the card under
    // the finger instead of dismissing it out from under an active gesture.
    discard_transient_move_completion(drag);
}

/// Applies a drag update — `delta` is
/// the raw signed per-axis pointer delta (see module docs limit #3 on
/// why this reads the raw component rather than `primary_delta`).
fn handle_drag_update(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    direction: DismissDirection,
    text_direction: TextDirection,
    overall_extent: f64,
    delta: f64,
) {
    if !drag.drag_underway.get() || move_controller.is_animating() {
        return;
    }
    let new_extent =
        accumulate_drag_extent(direction, text_direction, drag.drag_extent.get(), delta);
    drag.drag_extent.set(new_extent);
    if !move_controller.is_animating() {
        // `move_controller` is unregistered from `Vsync` for the whole drag
        // (see `handle_drag_start`), so this direct write is not immediately
        // clobbered by a tick — no re-registration needed here.
        move_controller.set_value(new_extent.abs() / overall_extent);
        // A drag that reaches (or overshoots) 100% of `overall_extent` clamps
        // `set_value` to the upper bound, which reports `Completed` — mid-drag,
        // as any `AnimationController.value` write can.
        // That case must be discarded; see
        // `discard_transient_move_completion`'s doc for why it is discarded
        // here instead of in the listener.
        discard_transient_move_completion(drag);
    }
}

/// The release speed in `move_controller` units per second: the gesture's
/// px/s over the same dismiss-axis extent `handle_drag_update` divides drag
/// deltas by, so the controller keeps the rate the drag gave it and the card
/// leaves as fast as it was moving. Under tight constraints (the common
/// case) that extent is the card's, and the card leaves at the finger's
/// speed whatever its size. Under loose ones it is the maximum, not the
/// laid-out child, so drag and fling alike move the card slower than the
/// finger by the same factor (module docs divergence #4: no laid-out size
/// accessor). An extent that is not positive and finite (unbounded
/// constraints, already caught in debug builds) yields one unit per second.
fn fling_speed(
    primary_velocity: f64,
    constraints: BoxConstraints,
    direction: DismissDirection,
) -> f64 {
    let extent = if direction_is_x_axis(direction) {
        constraints.max_width
    } else {
        constraints.max_height
    };
    let speed = primary_velocity.abs() / extent;
    if extent > 0.0 && speed.is_finite() {
        speed
    } else {
        1.0
    }
}

/// Ends a drag: fling, threshold, or spring back.
#[expect(clippy::too_many_arguments)] // the release handler receives its captured state and terminal details
fn handle_drag_end(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    resolved: &Rc<ResolvedConfig>,
    vsync: Option<&Vsync>,
    rebuild: &RebuildHandle,
    constraints: BoxConstraints,
    reason: flui_interaction::GestureEndReason,
    primary_velocity: f64,
    cross_velocity: f64,
) {
    if !drag.drag_underway.get() || move_controller.is_animating() {
        return;
    }
    drag.drag_underway.set(false);
    if reason == flui_interaction::GestureEndReason::Cancelled {
        discard_transient_move_completion(drag);
        ensure_move_controller_registered(drag, move_controller, vsync);
        let _ = move_controller.reverse();
        return;
    }
    if move_controller.is_completed() {
        // The direct bypass for a drag released exactly at 100%. Calls
        // `run_move_completion` unconditionally, NOT the counter-gated
        // `deliver_move_completion`: a drag released exactly at 100% never
        // bumped `move_completed_runs` in the first place (see
        // `discard_transient_move_completion`), so gating on that counter here
        // would wrongly skip this legitimate completion.
        run_move_completion(drag, move_controller, resolved, vsync, rebuild, constraints);
        return;
    }
    let dismiss_direction = extent_to_direction(
        drag.drag_extent.get(),
        resolved.direction,
        resolved.text_direction,
    );
    let threshold = dismiss_threshold_for(&resolved.dismiss_thresholds, dismiss_direction);
    // Every branch below starts a REAL run (`.forward()`/`.reverse()`/`.fling()`)
    // — re-register now so `Vsync`'s tick anchor lines up with the run's true
    // start (see `unregister_move_controller_vsync`'s doc).
    ensure_move_controller_registered(drag, move_controller, vsync);
    let fling_speed = fling_speed(primary_velocity, constraints, resolved.direction);
    match describe_fling_gesture(
        drag.drag_extent.get(),
        resolved.direction,
        resolved.text_direction,
        primary_velocity,
        cross_velocity,
    ) {
        FlingGestureKind::Forward => {
            if threshold >= 1.0 {
                let _ = move_controller.reverse();
            } else {
                drag.drag_extent.set(primary_velocity.signum());
                let _ = move_controller.fling(fling_speed);
            }
        }
        FlingGestureKind::Reverse => {
            drag.drag_extent.set(primary_velocity.signum());
            let _ = move_controller.fling(-fling_speed);
        }
        FlingGestureKind::None => {
            if !move_controller.is_dismissed() {
                if move_controller.value() > threshold {
                    let _ = move_controller.forward();
                } else {
                    let _ = move_controller.reverse();
                }
            }
        }
    }
}

/// The actual completion behavior, run unconditionally (no counter/latch gate
/// of any kind). Two call sites reach this:
///
/// - `handle_drag_end`'s direct bypass, when `move_controller.is_completed()`
///   holds at the exact moment of release.
/// - [`deliver_move_completion`], the deferred, counter-gated path for a
///   `.forward()`/`.fling()` run that settles to `Completed` sometime AFTER
///   release, driven by the status listener.
///
/// Calling this twice for the "same" logical completion cannot happen: the
/// direct-bypass site is reached only once per release, and the deferred
/// site only ever observes a completion the direct-bypass site did not
/// already consume (see `discard_transient_move_completion`'s doc for why a
/// mid-drag `Completed` never reaches the deferred path's counter at all).
// the six pieces of state the completion needs are not otherwise grouped
fn run_move_completion(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    resolved: &Rc<ResolvedConfig>,
    vsync: Option<&Vsync>,
    rebuild: &RebuildHandle,
    constraints: BoxConstraints,
) {
    let dismiss_direction = extent_to_direction(
        drag.drag_extent.get(),
        resolved.direction,
        resolved.text_direction,
    );
    let threshold = dismiss_threshold_for(&resolved.dismiss_thresholds, dismiss_direction);
    if threshold >= 1.0 {
        // A real run starts here too (reached via the `is_completed()` bypass
        // in `handle_drag_end`, where the drag itself — never vsync-registered,
        // see `unregister_move_controller_vsync` — pushed the value to 1.0).
        ensure_move_controller_registered(drag, move_controller, vsync);
        let _ = move_controller.reverse();
        return;
    }

    match resolved.resize_duration {
        None => {
            if let Some(on_dismissed) = &resolved.on_dismissed {
                on_dismissed(dismiss_direction);
            }
        }
        Some(duration) => {
            start_resize_animation(drag, duration, vsync, rebuild, constraints.biggest());
        }
    }
}

/// The deferred half of completion handling: reacts to `move_controller` completing
/// AFTER release (a `.forward()`/`.fling()` run settling), observed via the
/// status listener registered in `init_state` and the `move_completed_runs` /
/// `delivered_move_completions` counter pair. Idempotent: a call that finds
/// nothing new to deliver is a no-op. Never reached for a mid-drag
/// `Completed` event — those are discarded at the source (see
/// `discard_transient_move_completion`) — nor does it need its own
/// `drag_underway` check for that reason: by the time this runs,
/// `move_completed_runs` only ever counts completions the drag was not
/// underway for.
// mirrors `run_move_completion`'s arity — see its own note
fn deliver_move_completion(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    resolved: &Rc<ResolvedConfig>,
    vsync: Option<&Vsync>,
    rebuild: &RebuildHandle,
    constraints: BoxConstraints,
) {
    let completed_runs = drag.move_completed_runs.load(Ordering::Relaxed);
    if completed_runs <= drag.delivered_move_completions.get() {
        return;
    }
    drag.delivered_move_completions.set(completed_runs);
    run_move_completion(drag, move_controller, resolved, vsync, rebuild, constraints);
}

/// Starts the resize collapse: the
/// `resize_duration.is_some()` branch (the `None` branch is handled inline in
/// [`deliver_move_completion`]).
fn start_resize_animation(
    drag: &Rc<DragState>,
    duration: Duration,
    vsync: Option<&Vsync>,
    rebuild: &RebuildHandle,
    size_prior_to_collapse: Size,
) {
    if drag.resize_controller.borrow().is_some() {
        return;
    }
    drag.size_prior_to_collapse
        .set(Some(size_prior_to_collapse));

    let resize_controller = AnimationController::without_ticker(duration);

    let resize_ref = resize_controller.clone();
    let progress_ticks = Arc::clone(&drag.resize_progress_ticks);
    let completed_flag = Arc::clone(&drag.resize_completed);
    let rebuild_for_resize = rebuild.clone();
    let listener_id = resize_controller.add_listener(Arc::new(move || {
        if resize_ref.is_completed() {
            completed_flag.store(true, Ordering::Relaxed);
        } else {
            progress_ticks.fetch_add(1, Ordering::Relaxed);
        }
        rebuild_for_resize.schedule(flui_view::RebuildReason::AnimationTick);
    }));
    *drag.resize_listener_id.borrow_mut() = Some(listener_id);

    if let Some(vsync) = vsync {
        let registration = vsync.register(resize_controller.clone());
        *drag.resize_vsync_registration.borrow_mut() = Some(registration);
    }

    let _ = resize_controller.forward();
    let _prev = drag
        .resize_controller
        .borrow_mut()
        .replace(resize_controller);
}

/// Fires `on_resize` for every delivered progress tick, or `on_dismissed`
/// once when the resize controller completes.
fn deliver_resize_progress(drag: &Rc<DragState>, resolved: &Rc<ResolvedConfig>) {
    if drag.resize_completed.load(Ordering::Relaxed) {
        if !drag.delivered_resize_dismissal.get() {
            drag.delivered_resize_dismissal.set(true);
            let direction = extent_to_direction(
                drag.drag_extent.get(),
                resolved.direction,
                resolved.text_direction,
            );
            if let Some(on_dismissed) = &resolved.on_dismissed {
                on_dismissed(direction);
            }
        }
        return;
    }
    let ticks = drag.resize_progress_ticks.load(Ordering::Relaxed);
    let delivered = drag.delivered_resize_ticks.get();
    if ticks > delivered {
        drag.delivered_resize_ticks.set(ticks);
        if let Some(on_resize) = &resolved.on_resize {
            for _ in delivered..ticks {
                on_resize();
            }
        }
    }
}

/// Delivers `on_update` when the drag or threshold state changed.
fn deliver_on_update(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    direction: DismissDirection,
    text_direction: TextDirection,
    resolved: &Rc<ResolvedConfig>,
    on_update: Option<&UpdateDelivery>,
) {
    let Some(on_update) = on_update else { return };
    let value = move_controller.value();
    if value == drag.last_delivered_move_value.get() {
        return;
    }
    drag.last_delivered_move_value.set(value);

    let dismiss_direction = extent_to_direction(drag.drag_extent.get(), direction, text_direction);
    let threshold = dismiss_threshold_for(&resolved.dismiss_thresholds, dismiss_direction);
    let previous_reached = drag.dismiss_threshold_reached.get();
    let reached = value > threshold;
    drag.dismiss_threshold_reached.set(reached);
    on_update(DismissUpdateDetails {
        direction: dismiss_direction,
        reached,
        previous_reached,
        progress: value,
    });
}

// ============================================================================
// View construction
// ============================================================================

/// `background`, or `secondary_background` while dragging toward
/// `EndToStart`/`Up`.
fn resolve_background(
    view: &Dismissible,
    drag_extent: f64,
    text_direction: TextDirection,
) -> Option<BoxedView> {
    let dismiss_direction = extent_to_direction(drag_extent, view.direction, text_direction);
    if view.secondary_background.is_some()
        && matches!(
            dismiss_direction,
            DismissDirection::EndToStart | DismissDirection::Up
        )
    {
        view.secondary_background.clone()
    } else {
        view.background.clone()
    }
}

/// The slid-and-translated content: `FractionalTranslation` driven directly
/// by `move_controller.value()` and the drag's current sign (a
/// zero-`begin` offset tween's value at `t` is just `t * end`).
fn sliding_content_view(
    move_controller: &AnimationController,
    drag_extent: f64,
    direction: DismissDirection,
    cross_axis_end_offset: f64,
    child: BoxedView,
) -> FractionalTranslation {
    let t = move_controller.value();
    let sign = drag_sign(drag_extent);
    let (dx, dy) = if direction_is_x_axis(direction) {
        (t * sign, t * cross_axis_end_offset)
    } else {
        (t * cross_axis_end_offset, t * sign)
    };
    FractionalTranslation::new(dx, dy).child(child)
}

/// The post-dismiss collapse: a `background`-filled box shrinking along the
/// axis perpendicular to the dismiss direction, on an
/// `Interval(0.4, 1.0, Curves::Ease)` — a 40% pause, then an eased collapse
/// to zero. See module docs limit #2 for why this
/// clips at full size rather than progressively (`ClipRect::clip_behavior`
/// has no arbitrary-rect clipper to crop the un-collapsed axis to the
/// revealed sliver — irrelevant here anyway, since by this point the
/// collapsing box IS the background at its full prior size).
fn resize_collapse_view(
    resize_controller: &AnimationController,
    axis_is_x: bool,
    prior: Size,
    background: Option<BoxedView>,
) -> BoxedView {
    let curved = Interval::new(0.4, 1.0, Curves::Ease).transform(resize_controller.value());
    let factor = 1.0 - curved;
    let (width, height) = if axis_is_x {
        (prior.width, prior.height * factor)
    } else {
        (prior.width * factor, prior.height)
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
