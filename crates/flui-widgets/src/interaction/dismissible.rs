//! [`Dismissible`] — drag a child out of view to dismiss it, then collapse the
//! space it occupied.
//!
//! Flutter parity: `widgets/dismissible.dart` (tag `3.44.0`) — `Dismissible`,
//! `_DismissibleState`, `DismissDirection`, `DismissUpdateDetails`,
//! `_FlingGestureKind`, `_DismissibleClipper`, and the four tuned constants
//! `_kResizeTimeCurve` / `_kMinFlingVelocity` / `_kMinFlingVelocityDelta` /
//! `_kFlingVelocityScale` / `_kDismissThreshold`. State machine: a drag (or a
//! sufficiently fast fling) accumulates a signed `drag_extent`; releasing past
//! [`Dismissible::dismiss_threshold`] (default `0.4`, per direction) or with a
//! qualifying fling drives `move_controller` to 1.0 over
//! [`Dismissible::movement_duration`]; on completion the widget starts the
//! resize collapse over [`Dismissible::resize_duration`] (or fires
//! [`Dismissible::on_dismissed`] immediately when that is `None`) and then
//! fires `on_dismissed`.
//!
//! # Deliberate divergences from the oracle (framework-surface gaps)
//!
//! 1. **No `confirmDismiss`.** The oracle's `confirmDismiss` is an async gate
//!    (`Future<bool?> Function(DismissDirection)`) awaited between the move
//!    animation completing and the resize collapse starting. FLUI has no
//!    established widget-level "await a caller future, then keep going"
//!    seam yet (`FutureBuilder` rebuilds *from* future state; it does not let
//!    an imperative callback block a state transition on one). Inventing that
//!    seam here — one-off, for a single widget — would be exactly the kind of
//!    local hack this port avoids. Deferred; tracked as a follow-up rather
//!    than faked with a synchronous stand-in that would silently misrepresent
//!    the oracle's actual (async, vetoable) contract.
//! 2. **No progressive background clip.** The oracle's `_DismissibleClipper`
//!    is a `CustomClipper<Rect>` that reveals only the sliver of `background`
//!    between the sliding child's edge and the container edge, growing as the
//!    drag proceeds. `flui-widgets`' [`ClipRect`] has no
//!    arbitrary-rect / custom-clipper primitive yet — only a fixed
//!    [`flui_types::painting::Clip`] behavior. This port shows/hides
//!    `background` by *presence* (mounted whenever `move_controller.value()
//!    != 0.0`, matching the oracle's `!_moveAnimation.isDismissed` guard) but
//!    does not crop it to the revealed sliver — it paints at full extent
//!    under the sliding child from the first pixel of drag. Visually
//!    observable divergence; behaviorally the presence/absence signal (what
//!    the parity corpus asserts) matches exactly.
//! 3. **`Vertical`/`Up`/`Down` ride `on_pan_*`, not a vertical-drag family.**
//!    `GestureDetector` has no `on_vertical_drag_*` recognizer family (see
//!    that type's own docs on why) — only `on_horizontal_drag_*` and the
//!    free-axis `on_pan_*`. For the vertical-family directions this port
//!    wires `on_pan_*` and reads the raw `delta.dy` / `velocity.dy` component
//!    directly instead of `primary_delta` / `primary_velocity` (which are
//!    `Free`-axis distance *magnitudes* on that recognizer, not the signed
//!    per-axis component the oracle's math needs). Functionally equivalent
//!    for the one component this widget reads, but slightly looser: a pan
//!    recognizer's slop is not axis-locked the way a dedicated vertical
//!    recognizer's would be, so a mostly-horizontal drag can still start a
//!    vertical `Dismissible`'s gesture where the oracle's `VerticalDragGestureRecognizer`
//!    would hold off. Horizontal-family directions are unaffected — they use
//!    the real `on_horizontal_drag_*` family.
//! 4. **No live "my own size" query.** The oracle reads `context.size!`
//!    (this widget's last-laid-out size) from event handlers running well
//!    after `build`. FLUI's `BuildContext` has no such accessor. This port
//!    wraps its content in [`LayoutBuilder`] instead and
//!    uses the incoming `BoxConstraints` (`max_width`/`max_height`) as the
//!    drag-axis extent and the resize collapse's prior size — exact when the
//!    constraints are tight (the common case: a fixed-extent list item), but
//!    **`Dismissible` requires bounded constraints along its dismiss axis**;
//!    an unbounded axis has no extent to divide the drag fraction by.
//! 5. **No `AutomaticKeepAlive`.** The oracle mixes in
//!    `AutomaticKeepAliveClientMixin` so a mid-flight `Dismissible` is not
//!    disposed by a lazy list's viewport GC. FLUI has no keep-alive mechanism
//!    at all yet — a framework-wide gap, not a regression specific to this
//!    port.
//! 6. **No required `Key`.** The oracle's constructor requires one (so a
//!    dismissed list item's slot doesn't get re-synced onto the next item by
//!    index). FLUI's reconciliation is not index-keyed the same way; omitted
//!    as orthogonal to the drag/threshold/callback behavior this port
//!    targets.
//! 7. **No `dragStartBehavior`.** The oracle's `dragStartBehavior` field
//!    (`DragStartBehavior.start` by default, `.down` optionally) is passed
//!    straight through to its `GestureDetector`, choosing whether the drag's
//!    origin is where the gesture *won the arena* (`start`, smoother) or
//!    where the initial *down* event landed (`down`, more reactive).
//!    `flui-widgets`' [`GestureDetector`] has no
//!    `DragStartBehavior` concept at all yet — every drag effectively behaves
//!    as `start`. Not configurable here for the same reason divergence #3's
//!    vertical-drag family and this list's #5 keep-alive gap aren't: the
//!    primitive this widget would delegate to does not exist in FLUI yet.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use flui_animation::curve::{Curve, Interval};
use flui_animation::{
    Animation, AnimationController, AnimationStatus, Curves, Scheduler, Vsync, VsyncRegistration,
};
use flui_foundation::{Listenable, ListenerId};
use flui_interaction::{DragEndDetails, DragStartDetails, DragUpdateDetails};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::Size;
use flui_types::painting::Clip;
use flui_types::typography::TextDirection;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, BuildContextExt, IntoView, RebuildHandle, ViewExt, ViewState};

use crate::animated::VsyncScope;
use crate::localization::Directionality;
use crate::{
    ClipRect, FractionalTranslation, GestureDetector, LayoutBuilder, Positioned, SizedBox, Stack,
};

/// `_kMinFlingVelocity` (`dismissible.dart:19`) — a fling below this speed
/// never dismisses, regardless of direction.
const MIN_FLING_VELOCITY: f32 = 700.0;
/// `_kMinFlingVelocityDelta` (`dismissible.dart:20`) — the primary-axis
/// velocity must clear the cross-axis velocity by at least this much, or the
/// gesture is not "generally in the right direction".
const MIN_FLING_VELOCITY_DELTA: f32 = 400.0;
/// `_kFlingVelocityScale` (`dismissible.dart:21`) — pointer velocity
/// (px/s) is scaled into the `AnimationController.fling` velocity domain.
const FLING_VELOCITY_SCALE: f32 = 1.0 / 300.0;
/// `_kDismissThreshold` (`dismissible.dart:22`) — the default fraction of
/// `overall_drag_axis_extent` that must be crossed to dismiss.
const DEFAULT_DISMISS_THRESHOLD: f32 = 0.4;

/// The direction(s) in which a [`Dismissible`] can be dismissed.
///
/// Flutter parity: `DismissDirection` (`dismissible.dart:42`).
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
///
/// Flutter parity: `DismissDirectionCallback` (`dismissible.dart:28`).
pub type DismissDirectionCallback = Rc<dyn Fn(DismissDirection)>;

/// Fired on every drag/threshold-state change while a [`Dismissible`] is
/// being dragged.
///
/// Flutter parity: `DismissUpdateCallback` (`dismissible.dart:39`).
pub type DismissUpdateCallback = Rc<dyn Fn(DismissUpdateDetails)>;

/// Details delivered to [`Dismissible::on_update`].
///
/// Flutter parity: `DismissUpdateDetails` (`dismissible.dart:231`).
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
    pub progress: f32,
}

/// A widget that can be dismissed by dragging in [`DismissDirection`].
///
/// See the module docs for the oracle citation and this port's documented
/// divergences (no `confirmDismiss`, no progressive background clip, no
/// vertical-drag recognizer family, bounded-constraints contract, no
/// keep-alive, no required key).
#[derive(Clone, StatefulView)]
pub struct Dismissible {
    child: BoxedView,
    background: Option<BoxedView>,
    secondary_background: Option<BoxedView>,
    on_resize: Option<Rc<dyn Fn()>>,
    on_dismissed: Option<DismissDirectionCallback>,
    on_update: Option<DismissUpdateCallback>,
    direction: DismissDirection,
    resize_duration: Option<Duration>,
    dismiss_thresholds: HashMap<DismissDirection, f32>,
    movement_duration: Duration,
    cross_axis_end_offset: f32,
    behavior: HitTestBehavior,
}

impl Dismissible {
    /// A `Dismissible` wrapping `child`, dismissible horizontally by default
    /// (Flutter parity default), collapsing over 300ms after a 200ms slide.
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
    pub fn on_resize(mut self, on_resize: impl Fn() + 'static) -> Self {
        self.on_resize = Some(Rc::new(on_resize));
        self
    }

    /// Called once the dismissible has been dismissed — after the resize
    /// collapse finishes, or immediately if `resize_duration` is `None`.
    #[must_use]
    pub fn on_dismissed(mut self, on_dismissed: impl Fn(DismissDirection) + 'static) -> Self {
        self.on_dismissed = Some(Rc::new(on_dismissed));
        self
    }

    /// Called on every drag update with the current direction/threshold
    /// state.
    #[must_use]
    pub fn on_update(mut self, on_update: impl Fn(DismissUpdateDetails) + 'static) -> Self {
        self.on_update = Some(Rc::new(on_update));
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
    pub fn dismiss_threshold(mut self, direction: DismissDirection, threshold: f32) -> Self {
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
    pub fn cross_axis_end_offset(mut self, cross_axis_end_offset: f32) -> Self {
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

/// Whether `direction` drags along the horizontal axis. Flutter parity:
/// `_directionIsXAxis` (`dismissible.dart:337`).
fn direction_is_x_axis(direction: DismissDirection) -> bool {
    matches!(
        direction,
        DismissDirection::Horizontal | DismissDirection::EndToStart | DismissDirection::StartToEnd
    )
}

/// `extent.sign`, but `0.0` maps to `0.0` (Rust's `f32::signum` maps `+0.0` to
/// `1.0`, which would wrongly treat "no drag yet" as "dragged positive").
fn drag_sign(extent: f32) -> f32 {
    if extent == 0.0 { 0.0 } else { extent.signum() }
}

/// Flutter parity: `_extentToDirection` (`dismissible.dart:343`).
fn extent_to_direction(
    extent: f32,
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

/// Flutter parity: the direction-gating `switch` inside `_handleDragUpdate`
/// (`dismissible.dart:390`-`431`) — accumulates `delta` into `current` only
/// when doing so keeps the extent on the side `direction` (and, for the
/// reading-direction-relative variants, `text_direction`) allows.
fn accumulate_drag_extent(
    direction: DismissDirection,
    text_direction: TextDirection,
    current: f32,
    delta: f32,
) -> f32 {
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

/// The resolved threshold for `direction` (Flutter parity:
/// `widget.dismissThresholds[_dismissDirection] ?? _kDismissThreshold`).
fn dismiss_threshold_for(
    thresholds: &HashMap<DismissDirection, f32>,
    direction: DismissDirection,
) -> f32 {
    thresholds
        .get(&direction)
        .copied()
        .unwrap_or(DEFAULT_DISMISS_THRESHOLD)
}

/// Flutter parity: `_FlingGestureKind` (`dismissible.dart:296`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlingGestureKind {
    /// Too slow, or not clearly aimed along the drag axis — not a fling.
    None,
    /// Fast enough, aimed the same way the drag is already leaning.
    Forward,
    /// Fast enough, aimed the opposite way.
    Reverse,
}

/// Flutter parity: `_describeFlingGesture` (`dismissible.dart:468`).
fn describe_fling_gesture(
    drag_extent: f32,
    direction: DismissDirection,
    text_direction: TextDirection,
    primary_velocity: f32,
    cross_velocity: f32,
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
/// reconstructs every rebuild. Flutter parity: the mutable instance fields of
/// `_DismissibleState` (`_dragExtent`, `_dragUnderway`,
/// `_sizePriorToCollapse`, `_dismissThresholdReached`, `_resizeController`).
///
/// Kept out of `AnimationController` listener closures (which must be
/// `Send + Sync`, per `flui_foundation::ListenerCallback`): those closures
/// only ever touch the `Arc<Atomic*>` signal fields below, never this
/// `Rc`/`Cell`-based state directly. `build()` (single-threaded, run on the
/// frame/build thread) is the only place that reads or reacts to those
/// signals and mutates this state.
#[derive(Default)]
struct DragState {
    /// Signed pixel extent dragged so far. Flutter parity: `_dragExtent`.
    drag_extent: Cell<f32>,
    /// Whether a drag contact is currently down. Flutter parity:
    /// `_dragUnderway`.
    drag_underway: Cell<bool>,
    /// The size occupied right before the resize collapse began. Flutter
    /// parity: `_sizePriorToCollapse`.
    size_prior_to_collapse: Cell<Option<Size>>,
    /// The dismiss-threshold-reached flag from the last `on_update`
    /// delivery, for `DismissUpdateDetails::previous_reached`. Flutter
    /// parity: `_dismissThresholdReached`.
    dismiss_threshold_reached: Cell<bool>,
    /// `move_controller.value()` at the last `on_update` delivery, so an
    /// unrelated rebuild (e.g. a parent prop change) that leaves the drag
    /// position untouched does not re-fire `on_update`.
    last_delivered_move_value: Cell<f32>,

    /// `move_controller`'s current `Vsync` registration — re-registered (not
    /// just registered once) on every direct `set_value` while dragging; see
    /// `reanchor_move_controller_vsync`'s doc for why.
    move_vsync_registration: Cell<Option<VsyncRegistration>>,

    /// Lazily created once the move animation completes past threshold.
    /// Flutter parity: `_resizeController`.
    resize_controller: RefCell<Option<AnimationController>>,
    resize_listener_id: RefCell<Option<ListenerId>>,
    resize_vsync_registration: Cell<Option<VsyncRegistration>>,

    /// Bumped by `move_controller`'s status listener on every transition to
    /// `Completed`. `Send + Sync` (an atomic), so the listener may touch it
    /// directly; `build()` diffs it against `delivered_move_completions`.
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
struct ResolvedConfig {
    direction: DismissDirection,
    text_direction: TextDirection,
    dismiss_thresholds: HashMap<DismissDirection, f32>,
    resize_duration: Option<Duration>,
    cross_axis_end_offset: f32,
    on_dismissed: Option<DismissDirectionCallback>,
    on_resize: Option<Rc<dyn Fn()>>,
}

/// State for [`Dismissible`]. Owns the persistent `move_controller` (created
/// once, per Flutter's `late final _moveController`) plus the shared
/// `DragState` and the [`RebuildHandle`] acquired in `init_state` (per
/// ADR-0018 — never acquired from `build`/layout) that the lazily created
/// resize controller's listener needs later.
pub struct DismissibleState {
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
        // A fresh, never-pumped scheduler: on a real display its ticker
        // drives the controller off wall-clock time; under a `VsyncScope`
        // the binding drives it deterministically instead (mirrors
        // `AnimatedSize`/`RefreshIndicator`).
        let move_controller =
            AnimationController::new(self.movement_duration, Arc::new(Scheduler::new()));
        DismissibleState {
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
    fn init_state(&mut self, ctx: &dyn BuildContext) {
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
                    move_completed_runs.fetch_add(1, Ordering::SeqCst);
                    rebuild_for_status.schedule(flui_view::RebuildReason::AnimationTick);
                }
            },
        )));

        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register(self.move_controller.clone());
            self.drag.move_vsync_registration.set(Some(registration));
            self.vsync = Some(vsync);
        }

        self.rebuild = Some(rebuild);
    }

    fn build(&self, view: &Dismissible, ctx: &dyn BuildContext) -> impl IntoView {
        // Flutter parity: `_directionIsXAxis || debugCheckHasDirectionality`
        // (`dismissible.dart:611`) requires an ambient `Directionality` only
        // for the X-axis directions (`EndToStart`/`StartToEnd` read it to
        // resolve against the reading direction; plain `Horizontal` reads it
        // too, for `DismissUpdateDetails`/`on_dismissed`'s reported
        // direction). `Vertical`/`Up`/`Down`/`None` never consult
        // `text_direction` (see `accumulate_drag_extent`/`extent_to_direction`),
        // so defaulting instead of requiring an ancestor here avoids an
        // unnecessary hard panic for callers who never wrap a purely-vertical
        // `Dismissible` in one.
        let text_direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
        let resolved = Rc::new(ResolvedConfig {
            direction: view.direction,
            text_direction,
            dismiss_thresholds: view.dismiss_thresholds.clone(),
            resize_duration: view.resize_duration,
            cross_axis_end_offset: view.cross_axis_end_offset,
            on_dismissed: view.on_dismissed.clone(),
            on_resize: view.on_resize.clone(),
        });
        let on_update = view.on_update.clone();
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
                constraints.max_width.get()
            } else {
                constraints.max_height.get()
            };
            // Module docs divergence #4: this widget divides by
            // `overall_extent` to turn a drag delta into a fraction, so it
            // requires bounded constraints along the dismiss axis. An
            // unbounded axis (`f32::INFINITY`) would silently produce a
            // stuck-at-zero or NaN drag fraction instead of a loud failure —
            // catch the caller error here instead.
            debug_assert!(
                overall_extent.is_finite(),
                "BUG: Dismissible requires bounded constraints along its dismiss axis \
                 (got an unbounded/non-finite extent) — see module docs divergence #4"
            );

            // Firing `on_update`/`on_resize`/`on_dismissed` (and scheduling
            // further rebuilds via `RebuildHandle::schedule()`) from HERE runs
            // them during LAYOUT, not from an animation-listener callback the
            // way the oracle's `_handleDismissUpdateValueChanged`/
            // `_handleResizeProgressChanged`/`_handleMoveCompleted` do
            // (Dart's `AnimationController` listeners are not
            // thread-constrained, so the oracle fires straight from them). This
            // port cannot: the `Send + Sync`-bounded status/value listeners
            // registered in `init_state` can only touch `Arc<Atomic*>` signals,
            // never the `Rc`-based `on_update`/`on_resize`/`on_dismissed`
            // callbacks (see `DragState`'s own doc) — so delivery is deferred
            // to here, the next time this `LayoutBuilder` (re-)runs.
            //
            // Verified, not assumed, safe to do from a layout-phase closure:
            // - `RebuildHandle::schedule()`'s own contract (`rebuild_handle.rs`)
            //   states it "never touches the element tree, the render tree, or
            //   the pipeline. It writes to a mutex-guarded map and calls one
            //   `Fn()`" — callable from any thread, any phase, by design.
            // - `BuildOwner::service_layout_builders` (`owner/layout_builder.rs`,
            //   the fixpoint that actually invokes this closure) calls
            //   `self.build_scope(tree)` with the pipeline write-lock
            //   EXPLICITLY released first — "with NO pipeline lock held, so a
            //   builder that mounts a child can insert its render objects".
            //   That is: this closure runs as a genuine, ordinary build pass
            //   (the same `build_scope` every other `StatefulView::build` runs
            //   through), just one sequenced *between* layout passes rather
            //   than at the top of the frame — not a restricted context with
            //   different rules. Port-check trigger #22 (see
            //   `scripts/check-frame-capability-scope.sh`) governs *acquiring*
            //   `rebuild_handle()`/`post_frame_handle()` from `build`/`layout`/
            //   `paint` (an unbounded-rebuild-loop hazard); it says nothing
            //   about *calling* `.schedule(reason)` on a handle already acquired in
            //   `init_state` (this one), which is exactly what every listener
            //   callback in this file already does.
            deliver_move_completion(
                &drag,
                &move_controller,
                &resolved,
                vsync.as_ref(),
                &rebuild,
                constraints,
            );
            deliver_resize_progress(&drag, &resolved);
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
                    .on_horizontal_drag_start(move |_details: DragStartDetails| {
                        handle_drag_start(
                            &drag_for_start,
                            &controller_for_start,
                            vsync_for_start.as_ref(),
                            overall_extent,
                        );
                    })
                    .on_horizontal_drag_update(move |details: DragUpdateDetails| {
                        handle_drag_update(
                            &drag_for_update,
                            &controller_for_update,
                            direction,
                            resolved_for_update.text_direction,
                            overall_extent,
                            details.delta.dx.get(),
                        );
                    })
                    .on_horizontal_drag_end(move |details: DragEndDetails| {
                        handle_drag_end(
                            &drag_for_end,
                            &controller_for_end,
                            &resolved_for_end,
                            vsync_for_end.as_ref(),
                            &rebuild_for_end,
                            constraints,
                            details.velocity.pixels_per_second.dx.get(),
                            details.velocity.pixels_per_second.dy.get(),
                        );
                    });
            } else {
                detector = detector
                    .on_pan_start(move |_details: DragStartDetails| {
                        handle_drag_start(
                            &drag_for_start,
                            &controller_for_start,
                            vsync_for_start.as_ref(),
                            overall_extent,
                        );
                    })
                    .on_pan_update(move |details: DragUpdateDetails| {
                        handle_drag_update(
                            &drag_for_update,
                            &controller_for_update,
                            direction,
                            resolved_for_update.text_direction,
                            overall_extent,
                            details.delta.dy.get(),
                        );
                    })
                    .on_pan_end(move |details: DragEndDetails| {
                        handle_drag_end(
                            &drag_for_end,
                            &controller_for_end,
                            &resolved_for_end,
                            vsync_for_end.as_ref(),
                            &rebuild_for_end,
                            constraints,
                            details.velocity.pixels_per_second.dy.get(),
                            details.velocity.pixels_per_second.dx.get(),
                        );
                    });
            }

            detector.child(content).boxed()
        })
    }

    fn dispose(&mut self) {
        if let Some(id) = self.move_value_listener_id.take() {
            self.move_controller.remove_listener(id);
        }
        if let Some(id) = self.move_status_listener_id.take() {
            self.move_controller.remove_status_listener(id);
        }
        if let (Some(vsync), Some(registration)) =
            (&self.vsync, self.drag.move_vsync_registration.take())
        {
            vsync.unregister(registration);
        }
        self.move_controller.dispose();

        if let Some(resize_controller) = self.drag.resize_controller.borrow_mut().take() {
            if let Some(id) = self.drag.resize_listener_id.borrow_mut().take() {
                resize_controller.remove_listener(id);
            }
            if let (Some(vsync), Some(registration)) =
                (&self.vsync, self.drag.resize_vsync_registration.take())
            {
                vsync.unregister(registration);
            }
            resize_controller.dispose();
        }
    }
}

// ============================================================================
// Gesture handlers — free functions mirroring `_DismissibleState`'s private
// methods, called from the `'static` closures `build()` reconstructs.
// ============================================================================

/// Unregisters `move_controller` from `vsync` for the duration of a raw drag.
///
/// `set_value` (called on every drag update, exactly like the oracle's
/// `_moveController.value = ...`) leaves the controller's `AnimationStatus`
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
    vsync.unregister(registration);
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
    if drag.move_vsync_registration.get().is_some() {
        return;
    }
    let registration = vsync.register(move_controller.clone());
    drag.move_vsync_registration.set(Some(registration));
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
/// before `handle_drag_end` ever runs. The oracle's own status listener
/// (`_handleDismissStatusChanged`) discards exactly this case:
/// `status.isCompleted && !_dragUnderway` — a `Completed` event that fires
/// while still dragging is dropped outright, never queued for later.
///
/// This port cannot replicate that check *in the listener*: the listener
/// registered in `init_state` must be `Send + Sync` (`flui_foundation::ListenerCallback`),
/// so it can only touch the `Arc<AtomicU64>` `move_completed_runs` counter,
/// never the `Cell<bool>` `drag_underway` flag (see `DragState`'s own doc on
/// why). Earlier drafts of this port bumped the counter unconditionally and
/// left `deliver_move_completion`'s build()-driven consumer to skip
/// delivery *while* `drag_underway` was still true — but skipping is not
/// discarding: the bump stayed on the counter, unconsumed. The very next
/// time `deliver_move_completion` ran with `drag_underway` false again (e.g.
/// after the user dragged back below threshold and released, which
/// correctly springs back via `.reverse()`), it saw a "new" completion it
/// had never delivered and ran the collapse + `on_dismissed` anyway — a
/// false dismissal the oracle never produces.
///
/// The fix moves the discard to the only place that reliably knows
/// `drag_underway` is true: right here, synchronously after every direct
/// `set_value`, in the same call stack that might have just caused the
/// bump. Marking it "delivered" immediately means `deliver_move_completion`
/// can never later mistake it for a real, undelivered completion. The
/// legitimate "released exactly at 100%" dismissal does not depend on this
/// counter at all: `handle_drag_end` calls [`run_move_completion`] directly
/// and unconditionally when `move_controller.is_completed()` holds at the
/// exact moment of release — mirroring the oracle's own direct
/// `_handleDragEnd` bypass, which likewise never consults the status
/// listener for that case.
fn discard_transient_move_completion(drag: &DragState) {
    let completed_runs = drag.move_completed_runs.load(Ordering::SeqCst);
    drag.delivered_move_completions.set(completed_runs);
}

/// Flutter parity: `_handleDragStart` (`dismissible.dart:366`), minus the
/// `_confirming` guard (no `confirmDismiss` — see module docs divergence #1).
fn handle_drag_start(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    vsync: Option<&Vsync>,
    overall_extent: f32,
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

/// Flutter parity: `_handleDragUpdate` (`dismissible.dart:383`) — `delta` is
/// the raw signed per-axis pointer delta (see module docs divergence #3 on
/// why this reads the raw component rather than `primary_delta`).
fn handle_drag_update(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    direction: DismissDirection,
    text_direction: TextDirection,
    overall_extent: f32,
    delta: f32,
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
        // exactly like the oracle's own `AnimationController.value` setter can.
        // The oracle's status listener discards that case explicitly
        // (`status.isCompleted && !_dragUnderway`, dismissible.dart); see
        // `discard_transient_move_completion`'s doc for why this port discards
        // it here instead of in the listener.
        discard_transient_move_completion(drag);
    }
}

/// Flutter parity: `_handleDragEnd` (`dismissible.dart:500`).
#[allow(clippy::too_many_arguments)] // mirrors the oracle's own `_handleDragEnd`, which reaches the same seven pieces of state via `widget`/instance fields rather than parameters
fn handle_drag_end(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    resolved: &Rc<ResolvedConfig>,
    vsync: Option<&Vsync>,
    rebuild: &RebuildHandle,
    constraints: BoxConstraints,
    primary_velocity: f32,
    cross_velocity: f32,
) {
    if !drag.drag_underway.get() || move_controller.is_animating() {
        return;
    }
    drag.drag_underway.set(false);
    if move_controller.is_completed() {
        // The direct bypass — mirrors the oracle's own `if (_moveController.isCompleted)
        // { _handleMoveCompleted(); return; }` in `_handleDragEnd`. Calls
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
                let _ = move_controller.fling(primary_velocity.abs() * FLING_VELOCITY_SCALE);
            }
        }
        FlingGestureKind::Reverse => {
            drag.drag_extent.set(primary_velocity.signum());
            let _ = move_controller.fling(-primary_velocity.abs() * FLING_VELOCITY_SCALE);
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

/// Flutter parity: `_handleMoveCompleted` (`dismissible.dart:548`-`561`) — the
/// actual completion behavior, run unconditionally (no counter/latch gate of
/// any kind). Two call sites reach this, matching the oracle's own two ways
/// into `_handleMoveCompleted`:
///
/// - `handle_drag_end`'s direct bypass, when `move_controller.is_completed()`
///   holds at the exact moment of release (oracle: `_handleDragEnd`'s own
///   `if (_moveController.isCompleted) { _handleMoveCompleted(); return; }`).
/// - [`deliver_move_completion`], the deferred, counter-gated path for a
///   `.forward()`/`.fling()` run that settles to `Completed` sometime AFTER
///   release (oracle: `_handleDismissStatusChanged`, driven by the status
///   listener).
///
/// Calling this twice for the "same" logical completion cannot happen: the
/// direct-bypass site is reached only once per release, and the deferred
/// site only ever observes a completion the direct-bypass site did not
/// already consume (see `discard_transient_move_completion`'s doc for why a
/// mid-drag `Completed` never reaches the deferred path's counter at all).
#[allow(clippy::too_many_arguments)] // mirrors the oracle's own `_handleMoveCompleted`, which reaches the same six pieces of state via `widget`/instance fields rather than parameters
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

/// Flutter parity: the deferred half of `_handleDismissStatusChanged`
/// (`dismissible.dart:539`-`546`) — reacts to `move_controller` completing
/// AFTER release (a `.forward()`/`.fling()` run settling), observed via the
/// status listener registered in `init_state` and the `move_completed_runs` /
/// `delivered_move_completions` counter pair. Idempotent: a call that finds
/// nothing new to deliver is a no-op. Never reached for a mid-drag
/// `Completed` event — those are discarded at the source (see
/// `discard_transient_move_completion`) — nor does it need its own
/// `drag_underway` check for that reason: by the time this runs,
/// `move_completed_runs` only ever counts completions the drag was not
/// underway for.
#[allow(clippy::too_many_arguments)] // mirrors `run_move_completion`'s arity — see its own note
fn deliver_move_completion(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    resolved: &Rc<ResolvedConfig>,
    vsync: Option<&Vsync>,
    rebuild: &RebuildHandle,
    constraints: BoxConstraints,
) {
    let completed_runs = drag.move_completed_runs.load(Ordering::SeqCst);
    if completed_runs <= drag.delivered_move_completions.get() {
        return;
    }
    drag.delivered_move_completions.set(completed_runs);
    run_move_completion(drag, move_controller, resolved, vsync, rebuild, constraints);
}

/// Flutter parity: `_startResizeAnimation` (`dismissible.dart:576`), the
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

    let resize_controller = AnimationController::new(duration, Arc::new(Scheduler::new()));

    let resize_ref = resize_controller.clone();
    let progress_ticks = Arc::clone(&drag.resize_progress_ticks);
    let completed_flag = Arc::clone(&drag.resize_completed);
    let rebuild_for_resize = rebuild.clone();
    let listener_id = resize_controller.add_listener(Arc::new(move || {
        if resize_ref.is_completed() {
            completed_flag.store(true, Ordering::SeqCst);
        } else {
            progress_ticks.fetch_add(1, Ordering::SeqCst);
        }
        rebuild_for_resize.schedule(flui_view::RebuildReason::AnimationTick);
    }));
    *drag.resize_listener_id.borrow_mut() = Some(listener_id);

    if let Some(vsync) = vsync {
        let registration = vsync.register(resize_controller.clone());
        drag.resize_vsync_registration.set(Some(registration));
    }

    let _ = resize_controller.forward();
    *drag.resize_controller.borrow_mut() = Some(resize_controller);
}

/// Flutter parity: `_handleResizeProgressChanged` (`dismissible.dart:599`) —
/// fires `on_resize` for every delivered progress tick, or `on_dismissed`
/// once when the resize controller completes.
fn deliver_resize_progress(drag: &Rc<DragState>, resolved: &Rc<ResolvedConfig>) {
    if drag.resize_completed.load(Ordering::SeqCst) {
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
    let ticks = drag.resize_progress_ticks.load(Ordering::SeqCst);
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

/// Flutter parity: `_handleDismissUpdateValueChanged` (`dismissible.dart:442`).
fn deliver_on_update(
    drag: &Rc<DragState>,
    move_controller: &AnimationController,
    direction: DismissDirection,
    text_direction: TextDirection,
    resolved: &Rc<ResolvedConfig>,
    on_update: Option<&DismissUpdateCallback>,
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
/// `EndToStart`/`Up` (Flutter parity: `dismissible.dart:613`-`619`).
fn resolve_background(
    view: &Dismissible,
    drag_extent: f32,
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
/// by `move_controller.value()` and the drag's current sign — mathematically
/// identical to the oracle's `Tween<Offset>(begin: Offset.zero, end:
/// ...).animate(_moveController)` (a zero-`begin` tween's value at `t` is
/// just `t * end`). Flutter parity: `_updateMoveAnimation` +
/// `SlideTransition` inside `build` (`dismissible.dart:456`-`466`,
/// `648`-`651`).
fn sliding_content_view(
    move_controller: &AnimationController,
    drag_extent: f32,
    direction: DismissDirection,
    cross_axis_end_offset: f32,
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
/// axis perpendicular to the dismiss direction, per `_kResizeTimeCurve`
/// (`Interval(0.4, 1.0, Curves.ease)`) — a 40% pause, then an eased collapse
/// to zero. Flutter parity: the `SizeTransition` branch of `build`
/// (`dismissible.dart:621`-`646`); see module docs divergence #2 for why this
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
        (prior.width.get(), prior.height.get() * factor)
    } else {
        (prior.width.get() * factor, prior.height.get())
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
    use super::*;

    // ------------------------------------------------------------------
    // `extent_to_direction` — Flutter parity: `_extentToDirection`.
    // ------------------------------------------------------------------

    #[test]
    fn extent_to_direction_zero_extent_is_none_regardless_of_configured_direction() {
        assert_eq!(
            extent_to_direction(0.0, DismissDirection::Horizontal, TextDirection::Ltr),
            DismissDirection::None
        );
        assert_eq!(
            extent_to_direction(0.0, DismissDirection::Vertical, TextDirection::Rtl),
            DismissDirection::None
        );
    }

    #[test]
    fn extent_to_direction_horizontal_ltr_maps_sign_to_start_end() {
        assert_eq!(
            extent_to_direction(10.0, DismissDirection::Horizontal, TextDirection::Ltr),
            DismissDirection::StartToEnd,
            "LTR: positive (rightward) extent is the reading-direction way, StartToEnd"
        );
        assert_eq!(
            extent_to_direction(-10.0, DismissDirection::Horizontal, TextDirection::Ltr),
            DismissDirection::EndToStart,
            "LTR: negative (leftward) extent is against reading direction, EndToStart"
        );
    }

    #[test]
    fn extent_to_direction_horizontal_rtl_flips_the_mapping() {
        assert_eq!(
            extent_to_direction(10.0, DismissDirection::Horizontal, TextDirection::Rtl),
            DismissDirection::EndToStart,
            "RTL: positive (rightward) extent is AGAINST the RTL reading direction"
        );
        assert_eq!(
            extent_to_direction(-10.0, DismissDirection::Horizontal, TextDirection::Rtl),
            DismissDirection::StartToEnd,
            "RTL: negative (leftward) extent follows the RTL reading direction"
        );
    }

    #[test]
    fn extent_to_direction_vertical_ignores_text_direction() {
        assert_eq!(
            extent_to_direction(10.0, DismissDirection::Vertical, TextDirection::Rtl),
            DismissDirection::Down
        );
        assert_eq!(
            extent_to_direction(-10.0, DismissDirection::Up, TextDirection::Ltr),
            DismissDirection::Up
        );
    }

    // ------------------------------------------------------------------
    // `accumulate_drag_extent` — Flutter parity: the gating `switch` in
    // `_handleDragUpdate`.
    // ------------------------------------------------------------------

    #[test]
    fn accumulate_drag_extent_up_direction_only_admits_negative_extent() {
        assert_eq!(
            accumulate_drag_extent(DismissDirection::Up, TextDirection::Ltr, 0.0, -10.0),
            -10.0,
            "a proposed negative (upward) extent is admitted"
        );
        assert_eq!(
            accumulate_drag_extent(DismissDirection::Up, TextDirection::Ltr, 0.0, 10.0),
            0.0,
            "a proposed positive (downward) extent is REJECTED — Up cannot go positive"
        );
    }

    #[test]
    fn accumulate_drag_extent_down_direction_only_admits_positive_extent() {
        assert_eq!(
            accumulate_drag_extent(DismissDirection::Down, TextDirection::Ltr, 0.0, 10.0),
            10.0
        );
        assert_eq!(
            accumulate_drag_extent(DismissDirection::Down, TextDirection::Ltr, 0.0, -10.0),
            0.0,
            "a proposed negative extent is REJECTED — Down cannot go negative"
        );
    }

    #[test]
    fn accumulate_drag_extent_end_to_start_ltr_only_admits_negative() {
        assert_eq!(
            accumulate_drag_extent(DismissDirection::EndToStart, TextDirection::Ltr, 0.0, -5.0),
            -5.0
        );
        assert_eq!(
            accumulate_drag_extent(DismissDirection::EndToStart, TextDirection::Ltr, 0.0, 5.0),
            0.0
        );
    }

    #[test]
    fn accumulate_drag_extent_end_to_start_rtl_only_admits_positive() {
        assert_eq!(
            accumulate_drag_extent(DismissDirection::EndToStart, TextDirection::Rtl, 0.0, 5.0),
            5.0
        );
        assert_eq!(
            accumulate_drag_extent(DismissDirection::EndToStart, TextDirection::Rtl, 0.0, -5.0),
            0.0
        );
    }

    #[test]
    fn accumulate_drag_extent_none_direction_always_collapses_to_zero() {
        assert_eq!(
            accumulate_drag_extent(DismissDirection::None, TextDirection::Ltr, 40.0, 10.0),
            0.0
        );
    }

    #[test]
    fn accumulate_drag_extent_horizontal_and_vertical_admit_every_delta_unconditionally() {
        assert_eq!(
            accumulate_drag_extent(DismissDirection::Horizontal, TextDirection::Ltr, 5.0, -30.0),
            -25.0
        );
        assert_eq!(
            accumulate_drag_extent(DismissDirection::Vertical, TextDirection::Ltr, -5.0, 30.0),
            25.0
        );
    }

    // ------------------------------------------------------------------
    // `describe_fling_gesture` — Flutter parity: `_describeFlingGesture`.
    // ------------------------------------------------------------------

    #[test]
    fn describe_fling_gesture_zero_extent_is_none() {
        assert_eq!(
            describe_fling_gesture(
                0.0,
                DismissDirection::Horizontal,
                TextDirection::Ltr,
                900.0,
                0.0
            ),
            FlingGestureKind::None,
            "a fling released at exactly zero displacement has no direction to fling toward"
        );
    }

    #[test]
    fn describe_fling_gesture_below_min_velocity_is_none() {
        assert_eq!(
            describe_fling_gesture(
                10.0,
                DismissDirection::Horizontal,
                TextDirection::Ltr,
                699.9,
                0.0
            ),
            FlingGestureKind::None,
            "699.9 px/s is 0.1 short of kMinFlingVelocity (700)"
        );
    }

    #[test]
    fn describe_fling_gesture_at_min_velocity_with_clean_cross_axis_is_forward() {
        assert_eq!(
            describe_fling_gesture(
                10.0,
                DismissDirection::Horizontal,
                TextDirection::Ltr,
                700.0,
                0.0
            ),
            FlingGestureKind::Forward,
            "700 px/s clears kMinFlingVelocity, aimed the same way the drag already leans"
        );
    }

    #[test]
    fn describe_fling_gesture_insufficient_axis_delta_is_none() {
        assert_eq!(
            describe_fling_gesture(
                10.0,
                DismissDirection::Horizontal,
                TextDirection::Ltr,
                900.0,
                900.0
            ),
            FlingGestureKind::None,
            "900 vs 900 cross-axis: the 0 delta is short of kMinFlingVelocityDelta (400) — \
             not clearly aimed along the drag axis"
        );
    }

    #[test]
    fn describe_fling_gesture_opposite_direction_is_reverse() {
        assert_eq!(
            describe_fling_gesture(
                10.0,
                DismissDirection::Horizontal,
                TextDirection::Ltr,
                -900.0,
                0.0
            ),
            FlingGestureKind::Reverse,
            "dragged rightward (extent > 0, StartToEnd) but flung leftward (EndToStart)"
        );
    }

    // ------------------------------------------------------------------
    // `dismiss_threshold_for` / `drag_sign`.
    // ------------------------------------------------------------------

    #[test]
    fn dismiss_threshold_for_unset_direction_is_the_default() {
        let thresholds = HashMap::new();
        assert_eq!(
            dismiss_threshold_for(&thresholds, DismissDirection::StartToEnd),
            DEFAULT_DISMISS_THRESHOLD
        );
    }

    #[test]
    fn dismiss_threshold_for_overridden_direction_wins() {
        let mut thresholds = HashMap::new();
        thresholds.insert(DismissDirection::StartToEnd, 1.0);
        assert_eq!(
            dismiss_threshold_for(&thresholds, DismissDirection::StartToEnd),
            1.0
        );
        assert_eq!(
            dismiss_threshold_for(&thresholds, DismissDirection::EndToStart),
            DEFAULT_DISMISS_THRESHOLD,
            "the override is per-direction — an unrelated direction keeps the default"
        );
    }

    #[test]
    fn drag_sign_treats_positive_and_negative_zero_as_no_drag() {
        assert_eq!(drag_sign(0.0), 0.0);
        assert_eq!(drag_sign(-0.0), 0.0);
        assert_eq!(drag_sign(5.0), 1.0);
        assert_eq!(drag_sign(-5.0), -1.0);
    }

    // ------------------------------------------------------------------
    // Mid-drag `Completed` latch — the stale-completion bug caught in
    // review: a drag reaching 100% of `overall_extent` clamps
    // `move_controller` to its upper bound, which reports `Completed` even
    // though the oracle discards that (`status.isCompleted && !_dragUnderway`)
    // as not a real move-completion. This exercises the actual
    // `handle_drag_start`/`handle_drag_update`/`handle_drag_end` state
    // machine — not achievable through the `tests/parity/dismissible_test.rs`
    // integration harness, whose touch-slop-then-delta drag helper cannot
    // reach literal 100% within a laid-out box (see that file's own module
    // doc). `RebuildHandle` has no public standalone constructor (by design:
    // only a real mount ever mints one — see `flui_view::owner::rebuild_handle`),
    // so this mounts the smallest possible real element tree to capture one.
    // ------------------------------------------------------------------

    /// A trivial `StatefulView` whose only job is to capture the
    /// `RebuildHandle` `init_state` acquires. Mirrors
    /// `flui_view::owner::rebuild_handle`'s own `Capturing` test fixture.
    #[derive(Clone, StatefulView)]
    struct RebuildHandleCapture {
        captured: Rc<RefCell<Option<RebuildHandle>>>,
    }

    struct RebuildHandleCaptureState {
        captured: Rc<RefCell<Option<RebuildHandle>>>,
    }

    impl StatefulView for RebuildHandleCapture {
        type State = RebuildHandleCaptureState;

        fn create_state(&self) -> Self::State {
            RebuildHandleCaptureState {
                captured: Rc::clone(&self.captured),
            }
        }
    }

    impl ViewState<RebuildHandleCapture> for RebuildHandleCaptureState {
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            *self.captured.borrow_mut() = Some(ctx.rebuild_handle());
        }

        fn build(&self, _view: &RebuildHandleCapture, _ctx: &dyn BuildContext) -> impl IntoView {
            crate::SizedBox::shrink()
        }
    }

    /// Mounts [`RebuildHandleCapture`] as a bare root (no pipeline owner —
    /// this probe never lays out or paints, only runs `init_state`/`build`)
    /// and returns the handle its `init_state` captured.
    fn mount_and_capture_rebuild_handle() -> RebuildHandle {
        use flui_view::{BuildOwner, ElementTree};

        let captured = Rc::new(RefCell::new(None));
        let view = RebuildHandleCapture {
            captured: Rc::clone(&captured),
        };
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let root = tree.mount_root(&view, &mut owner.element_owner_mut());
        owner.schedule_build_for(root, 0, flui_view::RebuildReason::InitialMount);
        owner.build_scope(&mut tree);

        captured
            .borrow()
            .clone()
            .expect("init_state must have captured a handle")
    }

    #[test]
    fn move_controller_reaching_the_clamp_mid_drag_does_not_leave_a_stale_completion_latch() {
        let rebuild = mount_and_capture_rebuild_handle();

        let move_controller =
            AnimationController::new(Duration::from_millis(200), Arc::new(Scheduler::new()));
        let drag = Rc::new(DragState::default());
        let move_completed_runs = Arc::clone(&drag.move_completed_runs);
        let _status_listener_id = move_controller.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed {
                move_completed_runs.fetch_add(1, Ordering::SeqCst);
            }
        }));

        let dismissed = Arc::new(AtomicU64::new(0));
        let on_dismissed_probe: DismissDirectionCallback = {
            let dismissed = Arc::clone(&dismissed);
            Rc::new(move |_direction| {
                dismissed.fetch_add(1, Ordering::SeqCst);
            })
        };
        // `resize_duration: None` so a wrongful `run_move_completion` call
        // fires `on_dismissed` SYNCHRONOUSLY (the `None` branch), making the
        // probe below observe the bug directly — with `Some(duration)` the
        // symptom would instead be "wrongly starts the resize collapse",
        // requiring the collapse's own controller to actually tick (real time,
        // never advanced in this synchronous unit test) before `on_dismissed`
        // would fire.
        let resolved = Rc::new(ResolvedConfig {
            direction: DismissDirection::EndToStart,
            text_direction: TextDirection::Ltr,
            dismiss_thresholds: HashMap::new(),
            resize_duration: None,
            cross_axis_end_offset: 0.0,
            on_dismissed: Some(on_dismissed_probe),
            on_resize: None,
        });
        let overall_extent = 100.0_f32;
        let constraints = BoxConstraints::tight(Size::new(
            flui_types::geometry::px(overall_extent),
            flui_types::geometry::px(50.0),
        ));

        // Drag straight past the clamp: a single update's delta already
        // exceeds 100% of `overall_extent`, exactly like a real drag that
        // runs off the end of the axis. `drag_underway` is still true —
        // matching the oracle's mid-drag `Completed` event that
        // `!_dragUnderway` discards.
        handle_drag_start(&drag, &move_controller, None, overall_extent);
        handle_drag_update(
            &drag,
            &move_controller,
            DismissDirection::EndToStart,
            TextDirection::Ltr,
            overall_extent,
            -150.0,
        );
        assert!(
            move_controller.is_completed(),
            "the clamp must actually reach Completed for this probe to mean anything"
        );
        assert_eq!(
            drag.move_completed_runs.load(Ordering::SeqCst),
            drag.delivered_move_completions.get(),
            "a mid-drag Completed event must be discarded immediately (delivered synced to \
             the counter), not left as a stale, unconsumed latch"
        );

        // Spring back below threshold — still mid-drag.
        handle_drag_update(
            &drag,
            &move_controller,
            DismissDirection::EndToStart,
            TextDirection::Ltr,
            overall_extent,
            130.0, // -150 + 130 = -20: 20% of the 100-unit extent
        );

        // Release below the 40% default threshold: `.reverse()`, no dismissal.
        handle_drag_end(
            &drag,
            &move_controller,
            &resolved,
            None,
            &rebuild,
            constraints,
            0.0,
            0.0,
        );

        // Simulate the build pass `deliver_move_completion` runs from on
        // every rebuild (`Dismissible::build`'s `LayoutBuilder` closure) —
        // the site a stale, unconsumed latch would actually be replayed
        // from in production. Calling it directly here is what makes this
        // probe exercise the full bug chain end to end, not just the
        // latch-invariant assertion above in isolation.
        deliver_move_completion(
            &drag,
            &move_controller,
            &resolved,
            None,
            &rebuild,
            constraints,
        );

        assert_eq!(
            dismissed.load(Ordering::SeqCst),
            0,
            "a spring-back release must not fire on_dismissed — the mid-drag clamp's stale \
             Completed event must not be replayed once drag_underway goes false again"
        );
    }
}
