//! [`GestureDetector`] — typed gesture slots (tap, double tap, long press,
//! multi-contact tap, force press, drag, transform) whose recognizers a
//! [`RawGestureDetector`] mounts.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_foundation::ValueKey;
use flui_foundation::geometry::{Offset, Point};
use flui_interaction::gesture::{
    GestureComposition, MultiTapDetails, PanZoomDisposition, ScaleEndDetails,
    ScaleGestureRecognizer, ScaleStartDetails, ScaleStartMode, ScaleUpdateDetails,
};
use flui_interaction::pointer::{EventPropagation, PanZoomDispatch};
use flui_interaction::{
    AxisLock, CancelOutcome, DoubleTapDetails, DoubleTapGestureRecognizer, DragAxis,
    DragDownDetails, DragEndDetails, DragGestureRecognizer, DragPointerStrategy, DragStartDetails,
    DragUpdateDetails, ForcePressDetails, ForcePressGestureRecognizer, GestureArenaMember,
    GestureContest, GestureRecognizer, LongPressGestureRecognizer, MultiTapGestureRecognizer,
    PointerDispatch, RecognizerSet, TapDetails, TapGestureRecognizer, TransformLock, cancel_all,
};
use flui_interaction::{PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerInfo, PointerKindSet};
use flui_platform_api::EventTime;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;

use super::raw_gesture_detector::{
    ContactAdmission, GestureRecognizerFactory, RawGestureDetector, RecognizerContext,
};
use crate::Semantics;
use crate::support::{EventCallback, ValueCallback, event_callback, value_callback};

/// Whether an unadmitted native update would act: the event, and how far its
/// focal point moved since the session's start (the pinch's translation).
type NativeScaleAdmission = Rc<dyn Fn(&PanZoomEvent, Offset<f64>) -> bool>;

// ============================================================================
// Slots
// ============================================================================

/// The callbacks of a [`DragSlot`].
#[derive(Clone, Default)]
struct DragCallbacks {
    down: Option<ValueCallback<DragDownDetails>>,
    start: Option<ValueCallback<DragStartDetails>>,
    update: Option<ValueCallback<DragUpdateDetails>>,
    end: Option<ValueCallback<DragEndDetails>>,
    cancel: Option<EventCallback>,
}

/// One drag recognizer for a [`GestureDetector`]: its axis, the devices it
/// admits, how contacts hand the drag on, and its callbacks.
///
/// A detector holds at most one drag slot, so two drag families that would
/// compete for the same motion on one detector cannot be configured: a free
/// pan already spans both axes. Detectors nested in the tree still compete
/// in the arena, where an axial drag takes a motion its axis carries at least
/// cos 45° of from a free pan (ADR-0208 rule 2).
#[derive(Clone)]
#[must_use]
pub struct DragSlot {
    axis: DragAxis,
    devices: PointerKindSet,
    strategy: DragPointerStrategy,
    axis_lock: AxisLock,
    callbacks: DragCallbacks,
}

impl DragSlot {
    fn along(axis: DragAxis) -> Self {
        Self {
            axis,
            devices: PointerKindSet::all(),
            strategy: DragPointerStrategy::PrimaryOnly,
            axis_lock: AxisLock::default(),
            callbacks: DragCallbacks::default(),
        }
    }

    /// Rail a pan that starts near an axis onto it (default
    /// [`Free`](AxisLock::Free)): its updates and release velocity follow
    /// that axis alone, for touch and touchpad (ADR-0208 §4). An axial drag
    /// is on its axis already. A mounted detector whose lock changes cancels
    /// the drag in progress.
    pub fn axis_lock(mut self, lock: AxisLock) -> Self {
        self.axis_lock = lock;
        self
    }

    /// A drag in any direction: it claims a contact past the pan slop and
    /// reports the full motion.
    pub fn pan() -> Self {
        Self::along(DragAxis::Free)
    }

    /// A drag along the horizontal axis: it claims a contact only past the
    /// horizontal slop and reports motion projected onto that axis.
    pub fn horizontal() -> Self {
        Self::along(DragAxis::Horizontal)
    }

    /// A drag along the vertical axis: it claims a contact only past the
    /// vertical slop and reports motion projected onto that axis.
    pub fn vertical() -> Self {
        Self::along(DragAxis::Vertical)
    }

    /// Admit only contacts of these device kinds (default: all), before any
    /// arena rule runs (ADR-0208 rule 1).
    pub fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.devices = devices;
        self
    }

    /// How contacts continue the drag (default
    /// [`PrimaryOnly`](DragPointerStrategy::PrimaryOnly)). A mounted detector
    /// whose strategy changes cancels the drag in progress; the next Down uses
    /// the new one.
    pub fn pointer_strategy(mut self, strategy: DragPointerStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Called when a contact that may begin the drag goes down, before any
    /// movement threshold is met.
    pub fn on_down<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragDownDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.down = Some(value_callback(callback));
        self
    }

    /// Called once when the drag begins (the contact crosses the slop).
    pub fn on_start<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragStartDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.start = Some(value_callback(callback));
        self
    }

    /// Called for each move of a drag in progress, with the delta since the
    /// previous update. A scope whose settings select position smoothing
    /// smooths the positions reported after recognition.
    pub fn on_update<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragUpdateDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.update = Some(value_callback(callback));
        self
    }

    /// Called once when the drag ends, with the release velocity. An accepted
    /// drag that is cancelled ends here too, with [`DragEndDetails::reason`]
    /// set to [`flui_interaction::GestureEndReason::Cancelled`].
    pub fn on_end<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragEndDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.end = Some(value_callback(callback));
        self
    }

    /// Called when a contact admitted after [`on_down`](Self::on_down) ends
    /// before the drag started: another member won the arena, or the contact
    /// was cancelled.
    pub fn on_cancel<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.cancel = Some(event_callback(callback));
        self
    }
}

impl std::fmt::Debug for DragSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragSlot")
            .field("axis", &self.axis)
            .field("devices", &self.devices)
            .field("strategy", &self.strategy)
            .field("axis_lock", &self.axis_lock)
            .finish_non_exhaustive()
    }
}

/// The callbacks of a [`TransformSlot`].
#[derive(Clone, Default)]
struct TransformCallbacks {
    start: Option<ValueCallback<ScaleStartDetails>>,
    update: Option<ValueCallback<ScaleUpdateDetails>>,
    end: Option<ValueCallback<ScaleEndDetails>>,
    cancel: Option<EventCallback>,
    native_admission: Option<NativeScaleAdmission>,
}

impl TransformCallbacks {
    fn is_active(&self) -> bool {
        self.start.is_some() || self.update.is_some() || self.end.is_some() || self.cancel.is_some()
    }
}

/// The scale-and-rotate recognizer of a [`GestureDetector`]: two or more
/// contacts, or a native trackpad pinch. Native input is claimed leaf-first
/// before any callback runs, so an enclosing detector does not also recognize
/// the same pinch.
#[derive(Clone, Default)]
#[must_use]
pub struct TransformSlot {
    start_mode: ScaleStartMode,
    axis_lock: AxisLock,
    transform_lock: TransformLock,
    callbacks: TransformCallbacks,
}

impl TransformSlot {
    /// A transform slot with no callbacks yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Rail focal motion that starts near an axis onto it, for touch and
    /// touchpad (default [`Free`](AxisLock::Free); ADR-0208 §4). A mounted
    /// detector whose lock changes cancels the transform in progress.
    pub fn axis_lock(mut self, lock: AxisLock) -> Self {
        self.axis_lock = lock;
        self
    }

    /// Keep rotation or scale at identity in a transform that begins as a
    /// pan (default [`None`](TransformLock::None); ADR-0208 §4). A mounted
    /// detector whose lock changes cancels the transform in progress.
    pub fn transform_lock(mut self, lock: TransformLock) -> Self {
        self.transform_lock = lock;
        self
    }

    /// Called when a multi-contact or native trackpad scale begins.
    pub fn on_start<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ScaleStartDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.start = Some(value_callback(callback));
        self
    }

    /// Called with the gesture's cumulative scale and incremental focal motion.
    pub fn on_update<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ScaleUpdateDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.update = Some(value_callback(callback));
        self
    }

    /// Called when the scale ends, with its scalar and focal velocities.
    pub fn on_end<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ScaleEndDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.end = Some(value_callback(callback));
        self
    }

    /// Called when an admitted scale is cancelled.
    pub fn on_cancel<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.cancel = Some(event_callback(callback));
        self
    }

    /// Begin recognition with one contact as a pan that becomes a scale when
    /// more join. Only a widget whose detector carries no drag slot uses it,
    /// so the one-contact motion has one recognizer.
    pub(crate) fn start_mode(mut self, mode: ScaleStartMode) -> Self {
        self.start_mode = mode;
        self
    }

    /// Admit a native update only when `admit` accepts it.
    pub(crate) fn native_admission(
        mut self,
        admit: impl Fn(&PanZoomEvent, Offset<f64>) -> bool + 'static,
    ) -> Self {
        self.callbacks.native_admission = Some(Rc::new(admit));
        self
    }
}

impl std::fmt::Debug for TransformSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformSlot")
            .field("start_mode", &self.start_mode)
            .field("axis_lock", &self.axis_lock)
            .field("transform_lock", &self.transform_lock)
            .finish_non_exhaustive()
    }
}

/// The callbacks of a [`ForcePressSlot`].
#[derive(Clone, Default)]
struct ForcePressCallbacks {
    start: Option<ValueCallback<ForcePressDetails>>,
    update: Option<ValueCallback<ForcePressDetails>>,
    peak: Option<ValueCallback<ForcePressDetails>>,
    end: Option<ValueCallback<ForcePressDetails>>,
}

/// The pressure recognizer of a [`GestureDetector`]: a contact whose device
/// measures pressure and presses past a start threshold, then a peak.
///
/// Recognition needs a measured reading; a device that reports no pressure
/// never starts a force press.
#[derive(Clone)]
#[must_use]
pub struct ForcePressSlot {
    start_pressure: f64,
    peak_pressure: f64,
    callbacks: ForcePressCallbacks,
}

impl Default for ForcePressSlot {
    fn default() -> Self {
        Self {
            start_pressure: flui_interaction::gesture::FORCE_PRESS_START_PRESSURE,
            peak_pressure: flui_interaction::gesture::FORCE_PRESS_PEAK_PRESSURE,
            callbacks: ForcePressCallbacks::default(),
        }
    }
}

impl ForcePressSlot {
    /// A force-press slot with the default thresholds and no callbacks yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The normalized pressure that starts the press, clamped to `0..=1`; a
    /// non-finite value is ignored.
    pub fn start_pressure(mut self, pressure: f64) -> Self {
        if pressure.is_finite() {
            self.start_pressure = pressure.clamp(0.0, 1.0);
        }
        self
    }

    /// The normalized pressure reported as the peak, clamped to `0..=1`; a
    /// non-finite value is ignored.
    pub fn peak_pressure(mut self, pressure: f64) -> Self {
        if pressure.is_finite() {
            self.peak_pressure = pressure.clamp(0.0, 1.0);
        }
        self
    }

    /// Called when the press passes the start pressure and wins its arena.
    pub fn on_start<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ForcePressDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.start = Some(value_callback(callback));
        self
    }

    /// Called for each measured sample of a press in progress.
    pub fn on_update<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ForcePressDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.update = Some(value_callback(callback));
        self
    }

    /// Called once when the press first reaches the peak pressure.
    pub fn on_peak<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ForcePressDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.peak = Some(value_callback(callback));
        self
    }

    /// Called when a started press ends.
    pub fn on_end<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ForcePressDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.callbacks.end = Some(value_callback(callback));
        self
    }
}

impl std::fmt::Debug for ForcePressSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForcePressSlot")
            .field("start_pressure", &self.start_pressure)
            .field("peak_pressure", &self.peak_pressure)
            .finish_non_exhaustive()
    }
}

/// The single-contact press callbacks: tap, secondary tap, long press and
/// double tap. They share one composition, so they are one recognizer group.
#[derive(Clone, Default)]
struct PressCallbacks {
    tap: Option<EventCallback>,
    tap_cancel: Option<ValueCallback<TapDetails>>,
    secondary_tap: Option<EventCallback>,
    long_press: Option<EventCallback>,
    double_tap: Option<EventCallback>,
    double_tap_down: Option<ValueCallback<DoubleTapDetails>>,
}

impl PressCallbacks {
    fn shape(&self) -> PressShape {
        PressShape {
            tap: self.tap.is_some() || self.secondary_tap.is_some(),
            long_press: self.long_press.is_some(),
            double_tap: self.double_tap.is_some() || self.double_tap_down.is_some(),
        }
    }
}

/// Which press recognizers a detector mounts. A change of shape replaces the
/// group, so a lone double tap never meets a tap that would steal its first
/// up.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PressShape {
    tap: bool,
    long_press: bool,
    double_tap: bool,
}

impl PressShape {
    fn is_empty(self) -> bool {
        !(self.tap || self.long_press || self.double_tap)
    }
}

/// The recognizer takes the same validated count the detector does.
pub use flui_interaction::{MultiTapCount, MultiTapCountError};

#[derive(Clone)]
struct MultiTapSlot {
    pointers: MultiTapCount,
    callback: ValueCallback<MultiTapDetails>,
}

// ============================================================================
// The detector
// ============================================================================

/// Detects gestures on its child and invokes the matching callback.
///
/// Each gesture is a typed slot. Setting a slot mounts its recognizer; the
/// detector hands them to a [`RawGestureDetector`], which owns the
/// recognizers, keeps them across rebuilds and retires them, so this widget
/// adds callbacks, assistive-technology actions and native pinch routing and
/// no recognition of its own:
///
/// - **tap** ([`on_tap`](Self::on_tap)) and **secondary tap**
///   ([`on_secondary_tap`](Self::on_secondary_tap)) — a down and up without
///   moving past the touch slop.
/// - **long press** ([`on_long_press`](Self::on_long_press)) — the contact
///   held still past the long-press deadline, which the binding polls.
/// - **double tap** ([`on_double_tap`](Self::on_double_tap),
///   [`on_double_tap_down`](Self::on_double_tap_down)) — two quick taps. With
///   `on_tap` the two compose so that the tap waits for the double tap to
///   fail: two quick taps fire `on_double_tap` once and never `on_tap`, and a
///   lone tap fires `on_tap` once the window closes.
/// - **multi-contact tap** ([`on_multi_tap`](Self::on_multi_tap)) — several
///   contacts down and released together.
/// - **force press** ([`force_press`](Self::force_press)) — a pressure
///   gesture on a device that measures pressure.
/// - **drag** ([`drag`](Self::drag)) — one [`DragSlot`]: a free pan or a
///   drag along one axis.
/// - **transform** ([`transform`](Self::transform)) — one [`TransformSlot`]:
///   scale and rotation with two contacts, or a native trackpad pinch.
///
/// The slots compete in the presentation's arena, so one contact fires at
/// most one gesture: a quick down and up resolves to the tap, a hold to the
/// long press, a motion past the slop to the drag. A detector has one drag
/// slot, so two drag families that would split the same motion between them
/// cannot be configured; setting [`drag`](Self::drag) again replaces the slot.
///
/// A rebuild that keeps a slot's configuration keeps its recognizer and the
/// contact it tracks, and reads the new callbacks at the next event. A rebuild
/// that changes a drag's axis, devices or strategy, the set of press
/// gestures, a force-press threshold or a multi-tap count replaces that
/// recognizer: the replacement is mounted before the outgoing one is
/// cancelled, and the cancellation reports to the current callbacks. A slot
/// that is cleared retires its recognizer without invoking the removed
/// callbacks.
///
/// # Alternatives
///
/// `GestureDetector` is a mechanism, not a behavioural widget (ADR-0216 §1):
/// it neither takes nor offers [`GestureAlternatives`](crate::GestureAlternatives).
/// A widget whose path or multipoint gesture performs an operation takes the
/// alternatives in its own constructor and mounts this detector inside them,
/// performing the gesture's command through
/// [`CommandHandle`](crate::CommandHandle). The detector's own taps and long
/// presses need no path, and assistive technology reaches them directly, as
/// below.
///
/// # Assistive-technology activation
///
/// A detector with `on_tap` advertises a semantics *tap* action, and one with
/// `on_long_press` a *show context menu* action, so a screen reader's
/// activate gesture presses the control with no pointer event. The
/// [`Semantics`] node is not a boundary: the action merges into the nearest
/// ancestor node, which for a Material button is the button's own node.
///
/// The platform's request runs the current callback inside its own
/// [`EventCx`], in the dispatch that resolved it, the way a key's
/// activation does, so its signal writes are accepted there. A request that
/// arrives after the detector is disposed runs nothing.
///
/// # Event context
///
/// Every callback receives the dispatch's `&mut EventCx<'_>` first, so it
/// writes a signal directly (ADR-0086):
///
/// ```rust
/// use flui_foundation::geometry::Offset;
/// use flui_view::{BuildContextExt, LifecycleContext, SignalWriteExt};
/// use flui_widgets::{DragSlot, GestureDetector};
///
/// fn detector(ctx: &dyn LifecycleContext) -> GestureDetector {
///     let count = ctx.signal(0_u32);
///     let offset = ctx.signal(Offset::<f64>::ZERO);
///     GestureDetector::new()
///         .on_tap(move |cx| count.update(cx, |n| *n += 1))
///         .drag(DragSlot::pan().on_update(move |cx, details| {
///             offset.update(cx, |o| *o += details.delta)
///         }))
/// }
/// ```
///
/// A callback may return `()` or the `Result` of a write; a refused write is
/// logged on the `flui::signals` target. A closure bound with `let` before it
/// is passed needs [`callback`] or [`callback_with`] to fix its signature.
///
/// # Arena and settings
///
/// The recognizers join the arena of the enclosing
/// [`GestureArenaScope`](crate::GestureArenaScope) and read its gesture
/// settings per pointer kind at admission, including the position smoothing
/// a scope selects. A scope whose settings change replaces every recognizer.
/// Mounting outside a scope is an invariant violation.
///
/// # Hit behavior
///
/// The default is [`DeferToChild`](HitTestBehavior::DeferToChild): a gesture is
/// recognized only when the contact lands on a hit-testable descendant. Override
/// with [`behavior`](Self::behavior) — for example, scroll areas use
/// [`Opaque`](HitTestBehavior::Opaque) so they fire regardless of content.
#[derive(Clone, StatefulView)]
pub struct GestureDetector {
    press: PressCallbacks,
    multi_tap: Option<MultiTapSlot>,
    force_press: Option<ForcePressSlot>,
    drag: Option<DragSlot>,
    transform: Option<TransformSlot>,
    /// How the underlying listener participates in hit-testing.
    behavior: HitTestBehavior,
    recognizer_owner: Option<Rc<()>>,
    admission: Option<ContactAdmission>,
    child: Child,
}

impl Default for GestureDetector {
    fn default() -> Self {
        Self {
            press: PressCallbacks::default(),
            multi_tap: None,
            force_press: None,
            drag: None,
            transform: None,
            behavior: HitTestBehavior::DeferToChild,
            recognizer_owner: None,
            admission: None,
            child: Child::empty(),
        }
    }
}

impl std::fmt::Debug for GestureDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetector")
            .field("on_tap", &self.press.tap.is_some())
            .field("on_tap_cancel", &self.press.tap_cancel.is_some())
            .field("on_secondary_tap", &self.press.secondary_tap.is_some())
            .field("on_long_press", &self.press.long_press.is_some())
            .field("on_double_tap", &self.press.double_tap.is_some())
            .field("on_double_tap_down", &self.press.double_tap_down.is_some())
            .field(
                "multi_tap",
                &self.multi_tap.as_ref().map(|slot| slot.pointers),
            )
            .field("force_press", &self.force_press)
            .field("drag", &self.drag)
            .field("transform", &self.transform)
            .field("behavior", &self.behavior)
            .finish_non_exhaustive()
    }
}

impl GestureDetector {
    /// A detector with no slots yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace every recognizer when a consumer's actual owner changes.
    #[must_use]
    pub(crate) fn recognizer_owner(mut self, owner: Rc<()>) -> Self {
        self.recognizer_owner = Some(owner);
        self
    }

    /// Hand a contact to the slots only when `admission` accepts its Down;
    /// a refused contact reaches whatever else is hit.
    #[must_use]
    pub fn admission(mut self, admission: ContactAdmission) -> Self {
        self.admission = Some(admission);
        self
    }

    /// Called when the child is tapped (a primary-button down and up without
    /// moving past the touch slop).
    #[must_use]
    pub fn on_tap<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.press.tap = Some(event_callback(callback));
        self
    }

    /// Called when a primary contact that could still have become a tap stops
    /// being one: it moved past the touch slop, lost the gesture arena (to a
    /// scrolling ancestor, say) or was cancelled. Fires at most once per
    /// contact, and never for a contact that taps. The [`TapDetails`] name
    /// the contact (`pointer`), so a caller tracking several can end only
    /// that one. Contacts are tracked only while [`on_tap`](Self::on_tap) or
    /// [`on_secondary_tap`](Self::on_secondary_tap) is set.
    #[must_use]
    pub fn on_tap_cancel<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, TapDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.press.tap_cancel = Some(value_callback(callback));
        self
    }

    /// Called when the child receives a secondary-button tap.
    #[must_use]
    pub fn on_secondary_tap<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.press.secondary_tap = Some(event_callback(callback));
        self
    }

    /// Called when the child is long-pressed (the contact held still past the
    /// long-press deadline, which the presentation binding polls).
    #[must_use]
    pub fn on_long_press<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.press.long_press = Some(event_callback(callback));
        self
    }

    /// Called when the child is double-tapped (two quick taps within the
    /// double-tap window, timed on the arena clock).
    ///
    /// With [`on_tap`](Self::on_tap), the tap waits for the double tap to
    /// fail: two quick taps fire this once and never `on_tap`; a lone tap is
    /// held until the window closes, then fires `on_tap` once.
    #[must_use]
    pub fn on_double_tap<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.press.double_tap = Some(event_callback(callback));
        self
    }

    /// Called the instant the second contact of a double tap goes down, with
    /// its position.
    ///
    /// It fires before, and independently of,
    /// [`on_double_tap`](Self::on_double_tap): the second contact is already
    /// validated against the first, so a consumer that acts on the position
    /// (word selection) need not wait for the second up. Both fire for a
    /// double tap that completes; only this one fires if the second contact
    /// is then dragged or cancelled.
    #[must_use]
    pub fn on_double_tap_down<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DoubleTapDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.press.double_tap_down = Some(value_callback(callback));
        self
    }

    /// Called when `pointers` contacts go down together and all release
    /// without moving past the tap slop.
    #[must_use]
    pub fn on_multi_tap<F, R>(mut self, pointers: MultiTapCount, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, MultiTapDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.multi_tap = Some(MultiTapSlot {
            pointers,
            callback: value_callback(callback),
        });
        self
    }

    /// Recognize a force press with `slot`'s thresholds and callbacks.
    #[must_use]
    pub fn force_press(mut self, slot: ForcePressSlot) -> Self {
        self.force_press = Some(slot);
        self
    }

    /// Recognize a drag with `slot`'s axis, policy and callbacks, replacing
    /// any drag slot set before.
    #[must_use]
    pub fn drag(mut self, slot: DragSlot) -> Self {
        self.drag = Some(slot);
        self
    }

    /// Recognize scale and rotation with `slot`'s callbacks.
    #[must_use]
    pub fn transform(mut self, slot: TransformSlot) -> Self {
        self.transform = Some(slot);
        self
    }

    /// Override the hit-test behavior (default:
    /// [`DeferToChild`](HitTestBehavior::DeferToChild)). Set
    /// [`Opaque`](HitTestBehavior::Opaque) for a scroll area or any gesture
    /// target that must fire even when the child has no hittable surface.
    #[must_use]
    pub fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    /// Set the child the gestures are detected on.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

// ============================================================================
// Live callbacks
// ============================================================================

/// The callbacks the mounted recognizers invoke, refreshed from the view on
/// every `build`. Recognizers read them at event time, so a rebuild with new
/// closures is honored without replacing the recognizer, and a cleared slot's
/// callbacks are never invoked again.
#[derive(Default)]
struct LiveCallbacks {
    press: RefCell<PressCallbacks>,
    multi_tap: RefCell<Option<ValueCallback<MultiTapDetails>>>,
    force_press: RefCell<ForcePressCallbacks>,
    drag: RefCell<DragCallbacks>,
    transform: RefCell<TransformCallbacks>,
}

impl LiveCallbacks {
    fn refresh(&self, view: &GestureDetector) {
        // Outgoing closures drop after every borrow is released: a capture's
        // `Drop` may reenter the detector.
        let outgoing = (
            self.press.replace(view.press.clone()),
            self.multi_tap
                .replace(view.multi_tap.as_ref().map(|slot| slot.callback.clone())),
            self.force_press.replace(
                view.force_press
                    .as_ref()
                    .map(|slot| slot.callbacks.clone())
                    .unwrap_or_default(),
            ),
            self.drag.replace(
                view.drag
                    .as_ref()
                    .map(|slot| slot.callbacks.clone())
                    .unwrap_or_default(),
            ),
            self.transform.replace(
                view.transform
                    .as_ref()
                    .map(|slot| slot.callbacks.clone())
                    .unwrap_or_default(),
            ),
        );
        drop(outgoing);
    }
}

/// Run the callback `read` takes from the live slots, inside one write. The
/// slot borrow ends before the callback runs.
fn deliver<T>(
    live: &LiveCallbacks,
    writer: &WriterSource,
    read: impl FnOnce(&LiveCallbacks) -> Option<ValueCallback<T>>,
    value: T,
) {
    if let Some(callback) = read(live) {
        writer.write(|cx| callback(cx, value));
    }
}

/// Run the press callback `read` takes from the live slots inside `cx`, an
/// assistive-technology request's dispatch, while the detector is mounted.
/// The slot borrow ends before the callback runs.
fn activate(
    live: &LiveCallbacks,
    mounted: &Cell<bool>,
    cx: &mut EventCx<'_>,
    read: impl FnOnce(&PressCallbacks) -> Option<EventCallback>,
) {
    if !mounted.get() {
        return;
    }
    let callback = read(&live.press.borrow());
    if let Some(callback) = callback {
        callback(cx);
    }
}

/// [`deliver`] for a callback without a value.
fn deliver_event(
    live: &LiveCallbacks,
    writer: &WriterSource,
    read: impl FnOnce(&LiveCallbacks) -> Option<EventCallback>,
) {
    if let Some(callback) = read(live) {
        writer.write(|cx| callback(cx));
    }
}

// ============================================================================
// Factories
// ============================================================================

/// The press group: tap, long press and double tap, in that arena order, with
/// the tap waiting for the double tap to fail when both are mounted.
struct PressRecognizers {
    members: RecognizerSet,
    tap: Option<Rc<TapGestureRecognizer>>,
    long_press: Option<Rc<LongPressGestureRecognizer>>,
    double_tap: Option<Rc<DoubleTapGestureRecognizer>>,
}

impl PressRecognizers {
    fn recognizers(&self) -> impl Iterator<Item = &dyn GestureRecognizer> {
        let tap = self.tap.as_deref().map(|r| r as &dyn GestureRecognizer);
        let long_press = self
            .long_press
            .as_deref()
            .map(|r| r as &dyn GestureRecognizer);
        let double_tap = self
            .double_tap
            .as_deref()
            .map(|r| r as &dyn GestureRecognizer);
        tap.into_iter().chain(long_press).chain(double_tap)
    }
}

impl GestureArenaMember for PressRecognizers {
    // The members join the arena themselves.
    fn accept_gesture(&self, _: GestureContest) {}
    fn reject_gesture(&self, _: GestureContest) {}
}

impl GestureRecognizer for PressRecognizers {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        self.members.dispatch(down);
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        self.members.dispatch(dispatch);
    }

    fn cancel(&self) -> CancelOutcome {
        cancel_all(self.recognizers())
    }
}

struct PressFactory {
    shape: PressShape,
    live: Rc<LiveCallbacks>,
}

impl GestureRecognizerFactory for PressFactory {
    type Recognizer = PressRecognizers;

    fn create(&self, cx: &RecognizerContext) -> Rc<PressRecognizers> {
        let arena = cx.arena();
        let settings = cx.settings();
        let (double_tap_arena, tap_arena) = if self.shape.tap && self.shape.double_tap {
            match arena.compose(GestureComposition::RequireFailure) {
                Ok(branches) => branches.into_branches(),
                Err(error) => {
                    composition_diagnostic(&error);
                    (arena.clone(), arena.clone())
                }
            }
        } else {
            (arena.clone(), arena.clone())
        };
        let tap = self.shape.tap.then(|| {
            let (primary, secondary) = (Rc::clone(&self.live), Rc::clone(&self.live));
            let (primary_writer, secondary_writer) = (cx.writer().clone(), cx.writer().clone());
            let (cancel, cancel_writer) = (Rc::clone(&self.live), cx.writer().clone());
            TapGestureRecognizer::builder(tap_arena)
                .settings(settings.clone())
                .on_tap(move |_| {
                    deliver_event(&primary, &primary_writer, |live| {
                        live.press.borrow().tap.clone()
                    });
                })
                .on_tap_cancel(move |details| {
                    deliver(
                        &cancel,
                        &cancel_writer,
                        |live| live.press.borrow().tap_cancel.clone(),
                        details,
                    );
                })
                .on_secondary_tap(move |_| {
                    deliver_event(&secondary, &secondary_writer, |live| {
                        live.press.borrow().secondary_tap.clone()
                    });
                })
                .build()
        });
        let long_press = self.shape.long_press.then(|| {
            let live = Rc::clone(&self.live);
            let writer = cx.writer().clone();
            LongPressGestureRecognizer::builder(arena.clone())
                .settings(settings.clone())
                .on_long_press(move || {
                    deliver_event(&live, &writer, |live| {
                        live.press.borrow().long_press.clone()
                    });
                })
                .build()
        });
        let double_tap = self.shape.double_tap.then(|| {
            let (done, down) = (Rc::clone(&self.live), Rc::clone(&self.live));
            let (done_writer, down_writer) = (cx.writer().clone(), cx.writer().clone());
            DoubleTapGestureRecognizer::builder(double_tap_arena)
                .settings(settings.clone())
                .on_double_tap(move |_| {
                    deliver_event(&done, &done_writer, |live| {
                        live.press.borrow().double_tap.clone()
                    });
                })
                .on_double_tap_down(move |details| {
                    deliver(
                        &down,
                        &down_writer,
                        |live| live.press.borrow().double_tap_down.clone(),
                        details,
                    );
                })
                .build()
        });
        let mut members = RecognizerSet::default();
        if let Some(tap) = &tap {
            members.attach(tap);
        }
        if let Some(long_press) = &long_press {
            members.attach(long_press);
        }
        if let Some(double_tap) = &double_tap {
            members.attach(double_tap);
        }
        Rc::new(PressRecognizers {
            members,
            tap,
            long_press,
            double_tap,
        })
    }

    // The shape is in the key and the callbacks are read live.
    fn update(&self, _: &PressRecognizers) {}
}

struct MultiTapFactory {
    pointers: MultiTapCount,
    live: Rc<LiveCallbacks>,
}

impl GestureRecognizerFactory for MultiTapFactory {
    type Recognizer = MultiTapGestureRecognizer;

    fn create(&self, cx: &RecognizerContext) -> Rc<MultiTapGestureRecognizer> {
        let live = Rc::clone(&self.live);
        let writer = cx.writer().clone();
        MultiTapGestureRecognizer::builder(cx.arena().clone(), self.pointers)
            .settings(cx.settings().clone())
            .on_multi_tap(move |details| {
                deliver(
                    &live,
                    &writer,
                    |live| live.multi_tap.borrow().clone(),
                    details,
                );
            })
            .build()
    }

    // The count is in the key and the callback is read live.
    fn update(&self, _: &MultiTapGestureRecognizer) {}
}

struct ForcePressFactory {
    live: Rc<LiveCallbacks>,
    start_pressure: f64,
    peak_pressure: f64,
}

impl GestureRecognizerFactory for ForcePressFactory {
    type Recognizer = ForcePressGestureRecognizer;

    fn create(&self, cx: &RecognizerContext) -> Rc<ForcePressGestureRecognizer> {
        let (start, update, peak, end) = (
            Rc::clone(&self.live),
            Rc::clone(&self.live),
            Rc::clone(&self.live),
            Rc::clone(&self.live),
        );
        let (start_writer, update_writer, peak_writer, end_writer) = (
            cx.writer().clone(),
            cx.writer().clone(),
            cx.writer().clone(),
            cx.writer().clone(),
        );
        ForcePressGestureRecognizer::builder(cx.arena().clone())
            .settings(cx.settings().clone())
            .start_pressure(self.start_pressure)
            .peak_pressure(self.peak_pressure)
            .on_start(move |details| {
                deliver(
                    &start,
                    &start_writer,
                    |live| live.force_press.borrow().start.clone(),
                    details,
                );
            })
            .on_update(move |details| {
                deliver(
                    &update,
                    &update_writer,
                    |live| live.force_press.borrow().update.clone(),
                    details,
                );
            })
            .on_peak(move |details| {
                deliver(
                    &peak,
                    &peak_writer,
                    |live| live.force_press.borrow().peak.clone(),
                    details,
                );
            })
            .on_end(move |details| {
                deliver(
                    &end,
                    &end_writer,
                    |live| live.force_press.borrow().end.clone(),
                    details,
                );
            })
            .build()
    }

    // The thresholds are in the key and the callbacks are read live.
    fn update(&self, _: &ForcePressGestureRecognizer) {}
}

struct DragFactory {
    axis: DragAxis,
    devices: PointerKindSet,
    strategy: DragPointerStrategy,
    axis_lock: AxisLock,
    live: Rc<LiveCallbacks>,
}

impl GestureRecognizerFactory for DragFactory {
    type Recognizer = DragGestureRecognizer;

    fn create(&self, cx: &RecognizerContext) -> Rc<DragGestureRecognizer> {
        let (down, start, update, end, cancel) = (
            Rc::clone(&self.live),
            Rc::clone(&self.live),
            Rc::clone(&self.live),
            Rc::clone(&self.live),
            Rc::clone(&self.live),
        );
        let (down_writer, start_writer, update_writer, end_writer, cancel_writer) = (
            cx.writer().clone(),
            cx.writer().clone(),
            cx.writer().clone(),
            cx.writer().clone(),
            cx.writer().clone(),
        );
        DragGestureRecognizer::builder(cx.arena().clone(), self.axis)
            .allowed_devices(self.devices)
            .pointer_strategy(self.strategy)
            .axis_lock(self.axis_lock)
            .settings(cx.settings().clone())
            .on_down(move |details| {
                deliver(
                    &down,
                    &down_writer,
                    |live| live.drag.borrow().down.clone(),
                    details,
                );
            })
            .on_start(move |details| {
                deliver(
                    &start,
                    &start_writer,
                    |live| live.drag.borrow().start.clone(),
                    details,
                );
            })
            .on_update(move |details| {
                deliver(
                    &update,
                    &update_writer,
                    |live| live.drag.borrow().update.clone(),
                    details,
                );
            })
            .on_end(move |details| {
                deliver(
                    &end,
                    &end_writer,
                    |live| live.drag.borrow().end.clone(),
                    details,
                );
            })
            .on_cancel(move || {
                deliver_event(&cancel, &cancel_writer, |live| {
                    live.drag.borrow().cancel.clone()
                });
            })
            .build()
    }

    // The axis, devices, strategy and lock are in the key and the callbacks
    // are read live.
    fn update(&self, _: &DragGestureRecognizer) {}
}

/// The transform recognizer the detector's native pinch route claims for.
type ScaleLink = RefCell<Weak<ScaleGestureRecognizer>>;

struct TransformFactory {
    start_mode: ScaleStartMode,
    axis_lock: AxisLock,
    transform_lock: TransformLock,
    live: Rc<LiveCallbacks>,
    link: Rc<ScaleLink>,
    route: Rc<RefCell<NativeScaleRoute>>,
}

impl GestureRecognizerFactory for TransformFactory {
    type Recognizer = ScaleGestureRecognizer;

    fn create(&self, cx: &RecognizerContext) -> Rc<ScaleGestureRecognizer> {
        let (start, update, end, cancel) = (
            Rc::clone(&self.live),
            Rc::clone(&self.live),
            Rc::clone(&self.live),
            Rc::clone(&self.live),
        );
        let (start_writer, update_writer, end_writer, cancel_writer) = (
            cx.writer().clone(),
            cx.writer().clone(),
            cx.writer().clone(),
            cx.writer().clone(),
        );
        let recognizer = ScaleGestureRecognizer::builder(cx.arena().clone())
            .settings(cx.settings().clone())
            .start_mode(self.start_mode)
            .axis_lock(self.axis_lock)
            .transform_lock(self.transform_lock)
            .on_start(move |details| {
                deliver(
                    &start,
                    &start_writer,
                    |live| live.transform.borrow().start.clone(),
                    details,
                );
            })
            .on_update(move |details| {
                deliver(
                    &update,
                    &update_writer,
                    |live| live.transform.borrow().update.clone(),
                    details,
                );
            })
            .on_end(move |details| {
                deliver(
                    &end,
                    &end_writer,
                    |live| live.transform.borrow().end.clone(),
                    details,
                );
            })
            .on_cancel(move || {
                deliver_event(&cancel, &cancel_writer, |live| {
                    live.transform.borrow().cancel.clone()
                });
            })
            .build();
        // The replacement owns the native route from here: a session the
        // outgoing recognizer staged cannot reach it.
        *self.link.borrow_mut() = Rc::downgrade(&recognizer);
        *self.route.borrow_mut() = NativeScaleRoute::default();
        recognizer
    }

    // The start mode and locks are in the key and the callbacks are read
    // live.
    fn update(&self, _: &ScaleGestureRecognizer) {}
}

/// The build diagnostic for a refused composition (ADR-0208 §3): the press
/// recognizers then compete in the enclosing branch uncomposed.
fn composition_diagnostic(error: &flui_interaction::gesture::CompositionError) {
    tracing::warn!(
        target: "flui_widgets::gesture_detector",
        %error,
        "GestureDetector: composition refused; its recognizers compete uncomposed"
    );
}

// ============================================================================
// Native pinch routing
// ============================================================================

#[derive(Default)]
struct NativeScaleRoute {
    pending: Option<PendingNativeScale>,
    active: Option<(PointerInfo, Rc<()>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeBeginAdmission {
    Staging,
    Admitted,
    Rejected,
}

#[derive(Clone)]
struct PendingNativeScale {
    source: PointerInfo,
    time: EventTime,
    /// The local focal point at Start: translation is measured from it.
    focal: Point<f64>,
    admission: NativeBeginAdmission,
    generation: Rc<()>,
}

fn same_native_source(left: &PointerInfo, right: &PointerInfo) -> bool {
    left.id == right.id && left.device == right.device
}

/// What the native pinch route reads besides the recognizer.
struct NativeScaleGate<'a> {
    mounted: &'a Cell<bool>,
    live: &'a LiveCallbacks,
}

impl NativeScaleGate<'_> {
    fn active(&self) -> bool {
        self.mounted.get() && self.live.transform.borrow().is_active()
    }
}

fn claim_native_scale(
    recognizer: &ScaleGestureRecognizer,
    gate: &NativeScaleGate<'_>,
    route: &Rc<RefCell<NativeScaleRoute>>,
    dispatch: PanZoomDispatch<'_>,
) -> EventPropagation {
    let event = dispatch.local;
    match event.phase {
        PanZoomPhase::Start => {
            let generation = Rc::new(());
            let retired = {
                let mut route = route.borrow_mut();
                if route
                    .active
                    .as_ref()
                    .is_some_and(|(active, _)| !same_native_source(active, event.pointer()))
                {
                    return EventPropagation::Continue;
                }
                let retired = route.active.take().is_some();
                route.pending = Some(PendingNativeScale {
                    source: *event.pointer(),
                    time: event.time,
                    focal: event.position.get(),
                    admission: NativeBeginAdmission::Staging,
                    generation: generation.clone(),
                });
                retired
            };
            let pending_route = Rc::downgrade(route);
            let _ = dispatch.on_retirement(move || {
                if let Some(route) = pending_route.upgrade() {
                    let mut route = route.borrow_mut();
                    if route
                        .pending
                        .as_ref()
                        .is_some_and(|pending| Rc::ptr_eq(&pending.generation, &generation))
                    {
                        route.pending = None;
                    }
                    if route
                        .active
                        .as_ref()
                        .is_some_and(|(_, active)| Rc::ptr_eq(active, &generation))
                    {
                        route.active = None;
                    }
                }
            });
            // Start captures the dormant actor's profile immediately. Claiming
            // and recognized callbacks still wait for an admitted real Update.
            let disposition = recognizer.handle_pan_zoom(dispatch);
            {
                let mut route = route.borrow_mut();
                if let Some(pending) = route.pending.as_mut()
                    && pending.admission == NativeBeginAdmission::Staging
                    && same_native_source(&pending.source, event.pointer())
                    && pending.time == event.time
                {
                    pending.admission = if disposition == PanZoomDisposition::Admitted {
                        NativeBeginAdmission::Admitted
                    } else {
                        NativeBeginAdmission::Rejected
                    };
                }
            }
            if retired {
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        }
        PanZoomPhase::Update(transform) => {
            let active = route.borrow().active.as_ref().map(|(source, _)| *source);
            if let Some(active) = active {
                if !same_native_source(&active, event.pointer()) {
                    return EventPropagation::Continue;
                }
            } else {
                if route.borrow().pending.as_ref().is_some_and(|pending| {
                    same_native_source(&pending.source, event.pointer())
                        && pending.admission == NativeBeginAdmission::Rejected
                }) {
                    return EventPropagation::Continue;
                }
                // Translation during a pinch is the focal point moving since
                // Start (ADR-0217 §4); the points are finite by construction.
                let translation = route
                    .borrow()
                    .pending
                    .as_ref()
                    .filter(|pending| same_native_source(&pending.source, event.pointer()))
                    .map_or(Offset::ZERO, |pending| {
                        let now = event.position.get();
                        Offset::new(now.x - pending.focal.x, now.y - pending.focal.y)
                    });
                if !gate.active()
                    || !translation.is_finite()
                    || (transform == PanZoomTransform::IDENTITY && translation == Offset::ZERO)
                {
                    return EventPropagation::Continue;
                }
                if !matches!(dispatch.global.phase, PanZoomPhase::Update(_)) {
                    return EventPropagation::Continue;
                }
                let admission = gate.live.transform.borrow().native_admission.clone();
                if admission.is_some_and(|admit| !admit(event, translation)) {
                    return EventPropagation::Continue;
                }
                {
                    let mut route = route.borrow_mut();
                    if route.pending.as_ref().is_some_and(|pending| {
                        same_native_source(&pending.source, event.pointer())
                            && pending.time <= event.time
                    }) {
                        let pending = route
                            .pending
                            .take()
                            .expect("BUG: matching pending native admission");
                        route.active = Some((*event.pointer(), pending.generation));
                    }
                }
            }
            let handled = recognizer.handle_pan_zoom(dispatch);
            if handled == PanZoomDisposition::Handled || active.is_some() {
                EventPropagation::Stop
            } else {
                let mut route = route.borrow_mut();
                if route
                    .active
                    .as_ref()
                    .is_some_and(|(active, _)| same_native_source(active, event.pointer()))
                {
                    route.active = None;
                }
                EventPropagation::Continue
            }
        }
        PanZoomPhase::End | PanZoomPhase::Cancelled => {
            let (active, staged) = {
                let mut route = route.borrow_mut();
                let staged = if let Some(pending) = route.pending.as_ref()
                    && same_native_source(&pending.source, event.pointer())
                {
                    let admitted = pending.admission != NativeBeginAdmission::Rejected;
                    route.pending = None;
                    admitted
                } else {
                    false
                };
                let active = route
                    .active
                    .as_ref()
                    .is_some_and(|(active, _)| same_native_source(active, event.pointer()));
                if active {
                    route.active = None;
                }
                (active, staged)
            };
            if active || staged {
                let _ = recognizer.handle_pan_zoom(dispatch);
            }
            if active {
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        }
        _ => EventPropagation::Continue,
    }
}

// ============================================================================
// State
// ============================================================================

/// The detector's mounted state: the live callbacks its recognizers read,
/// and the native pinch route. The recognizers themselves live in the
/// [`RawGestureDetector`] it builds.
pub struct GestureDetectorState {
    live: Rc<LiveCallbacks>,
    /// Shared admission authority for native pinch and assistive activation.
    mounted: Rc<Cell<bool>>,
    /// Bumped when the consumer's recognizer owner changes; every factory key
    /// carries it, so the change replaces every recognizer. The detector
    /// compares a key only with the one before it, so wrapping cannot alias a
    /// live owner.
    owner_generation: u64,
    scale_link: Rc<ScaleLink>,
    native_scale_route: Rc<RefCell<NativeScaleRoute>>,
}

impl std::fmt::Debug for GestureDetectorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetectorState")
            .field("mounted", &self.mounted.get())
            .finish_non_exhaustive()
    }
}

impl StatefulView for GestureDetector {
    type State = GestureDetectorState;

    fn create_state(&self) -> Self::State {
        let live = Rc::new(LiveCallbacks::default());
        live.refresh(self);
        GestureDetectorState {
            live,
            mounted: Rc::new(Cell::new(true)),
            owner_generation: 0,
            scale_link: Rc::new(RefCell::new(Weak::new())),
            native_scale_route: Rc::new(RefCell::new(NativeScaleRoute::default())),
        }
    }
}

impl GestureDetectorState {
    /// The semantics node advertising this detector's activations to
    /// assistive technology, or `None` when it has nothing to advertise.
    ///
    /// Each handler runs the current callback inside the platform request's
    /// own [`EventCx`], as a pointer or a key would.
    /// The tap and long-press actions assistive technology can invoke. A
    /// detector with neither still gets the (empty, contributing nothing)
    /// annotation, so that setting or clearing `on_tap` keeps the subtree's
    /// shape and the child's state.
    fn semantics_actions(&self, view: &GestureDetector) -> Semantics {
        let mut semantics = Semantics::new();
        if view.press.tap.is_some() {
            let (live, mounted) = (Rc::clone(&self.live), Rc::clone(&self.mounted));
            semantics = semantics.on_click(move |cx| {
                activate(&live, &mounted, cx, |press| press.tap.clone());
            });
        }
        if view.press.long_press.is_some() {
            let (live, mounted) = (Rc::clone(&self.live), Rc::clone(&self.mounted));
            semantics = semantics.on_show_context_menu(move |cx| {
                activate(&live, &mounted, cx, |press| press.long_press.clone());
            });
        }
        semantics
    }

    /// The raw detector mounting one recognizer per configured slot, each
    /// keyed by the configuration its recognizer is built with.
    fn raw_detector(&self, view: &GestureDetector) -> RawGestureDetector {
        let owner = self.owner_generation;
        let mut raw = RawGestureDetector::new()
            .behavior(view.behavior)
            .contact_input();
        if let Some(admission) = &view.admission {
            raw = raw.admission(admission.clone());
        }
        let shape = view.press.shape();
        if !shape.is_empty() {
            raw = raw.recognizer_with_key(
                ValueKey::new((owner, shape)),
                PressFactory {
                    shape,
                    live: Rc::clone(&self.live),
                },
            );
        }
        if let Some(slot) = &view.multi_tap {
            raw = raw.recognizer_with_key(
                ValueKey::new((owner, slot.pointers)),
                MultiTapFactory {
                    pointers: slot.pointers,
                    live: Rc::clone(&self.live),
                },
            );
        }
        if let Some(slot) = &view.force_press {
            raw = raw.recognizer_with_key(
                ValueKey::new((
                    owner,
                    slot.start_pressure.to_bits(),
                    slot.peak_pressure.to_bits(),
                )),
                ForcePressFactory {
                    live: Rc::clone(&self.live),
                    start_pressure: slot.start_pressure,
                    peak_pressure: slot.peak_pressure,
                },
            );
        }
        if let Some(slot) = &view.drag {
            raw = raw.recognizer_with_key(
                ValueKey::new((
                    owner,
                    slot.axis,
                    slot.devices,
                    slot.strategy,
                    slot.axis_lock,
                )),
                DragFactory {
                    axis: slot.axis,
                    devices: slot.devices,
                    strategy: slot.strategy,
                    axis_lock: slot.axis_lock,
                    live: Rc::clone(&self.live),
                },
            );
        }
        if let Some(slot) = &view.transform {
            raw = raw
                .recognizer_with_key(
                    ValueKey::new((owner, slot.start_mode, slot.axis_lock, slot.transform_lock)),
                    TransformFactory {
                        start_mode: slot.start_mode,
                        axis_lock: slot.axis_lock,
                        transform_lock: slot.transform_lock,
                        live: Rc::clone(&self.live),
                        link: Rc::clone(&self.scale_link),
                        route: Rc::clone(&self.native_scale_route),
                    },
                )
                .pan_zoom_claim({
                    let live = Rc::clone(&self.live);
                    let mounted = Rc::clone(&self.mounted);
                    let link = Rc::clone(&self.scale_link);
                    let route = Rc::clone(&self.native_scale_route);
                    move |dispatch| {
                        if !mounted.get() {
                            return EventPropagation::Continue;
                        }
                        let recognizer = link.borrow().upgrade();
                        let Some(recognizer) = recognizer else {
                            return EventPropagation::Continue;
                        };
                        let gate = NativeScaleGate {
                            mounted: &mounted,
                            live: &live,
                        };
                        claim_native_scale(&recognizer, &gate, &route, dispatch)
                    }
                });
        }
        raw
    }
}

impl ViewState<GestureDetector> for GestureDetectorState {
    fn did_update_view(&mut self, old_view: &GestureDetector, new_view: &GestureDetector) {
        let owner_changed = match (&old_view.recognizer_owner, &new_view.recognizer_owner) {
            (None, None) => false,
            (Some(old), Some(new)) => !Rc::ptr_eq(old, new),
            _ => true,
        };
        if owner_changed {
            self.owner_generation = self.owner_generation.wrapping_add(1);
        }
    }

    fn build(&self, view: &GestureDetector, _ctx: &dyn BuildContext) -> impl IntoView {
        self.live.refresh(view);
        let raw = self.raw_detector(view);
        let raw = match view.child.clone().into_inner() {
            Some(child) => raw.child(child),
            None => raw,
        };
        self.semantics_actions(view).child(raw)
    }

    fn dispose(&mut self) {
        // Revoke native pinch and semantics admission before the raw
        // detector's retirement can run cancellation callbacks.
        self.mounted.set(false);
    }
}
