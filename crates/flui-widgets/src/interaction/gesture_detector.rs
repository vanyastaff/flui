//! [`GestureDetector`] — recognizes high-level gestures (tap, long-press,
//! double-tap, and pan/drag) from the raw pointer stream a [`Listener`] delivers.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::{Arc, Mutex},
};

use flui_interaction::{
    DoubleTapDetails, DoubleTapGestureRecognizer, DragAxis, DragDownDetails, DragEndDetails,
    DragGestureRecognizer, DragStartDetails, DragUpdateDetails, GestureRecognizer,
    LongPressGestureRecognizer, TapGestureRecognizer, cancel_all,
};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;

use crate::support::{event_callback, value_callback};
use crate::{GestureArenaScope, Listener, Semantics};

/// A no-argument gesture callback (tap, long press, double tap) — fired with the dispatch's [`EventCx`] and no details when
/// the gesture is recognized. Stored already adapted to report its outcome.
type GestureCallback = Rc<dyn Fn(&mut EventCx<'_>)>;
/// Carries the tap's position.
/// See [`GestureDetector::on_double_tap_down`]'s doc for why this is a
/// separate callback from `on_double_tap` rather than widening it.
type DoubleTapDownHandler = Rc<dyn Fn(&mut EventCx<'_>, DoubleTapDetails)>;
/// Pan callbacks carry the drag's details (position, delta, velocity).
type PanStartHandler = Rc<dyn Fn(&mut EventCx<'_>, DragStartDetails)>;
type PanUpdateHandler = Rc<dyn Fn(&mut EventCx<'_>, DragUpdateDetails)>;
type PanEndHandler = Rc<dyn Fn(&mut EventCx<'_>, DragEndDetails)>;
/// Horizontal-drag callbacks carry the same detail types as pan, but the
/// underlying recognizer is axis-constrained ([`DragAxis::Horizontal`])
/// rather than free — see [`GestureDetector`]'s docs on why this and
/// `on_pan_*` are mutually exclusive on one detector.
type HorizontalDragDownHandler = Rc<dyn Fn(&mut EventCx<'_>, DragDownDetails)>;
type HorizontalDragStartHandler = Rc<dyn Fn(&mut EventCx<'_>, DragStartDetails)>;
type HorizontalDragUpdateHandler = Rc<dyn Fn(&mut EventCx<'_>, DragUpdateDetails)>;
type HorizontalDragEndHandler = Rc<dyn Fn(&mut EventCx<'_>, DragEndDetails)>;
type HorizontalDragCancelHandler = Rc<dyn Fn(&mut EventCx<'_>)>;

/// Detects gestures on its child and invokes the matching callback.
///
/// It owns a
/// set of gesture recognizers, wraps its child in a [`Listener`], and feeds the
/// pointer stream to every recognizer; an arena resolves the competition and the
/// winning recognizer fires its callback.
///
/// Five gesture families are wired:
/// - **tap** (`on_tap`) / **secondary tap** (`on_secondary_tap`) — a primary- /
///   secondary-button down + up without moving past the touch slop.
/// - **long press** (`on_long_press`) — the contact held still past the
///   long-press deadline. Deadline-driven: it needs a [`GestureArenaScope`] +
///   binding above to poll the deadline (see [arena acquisition](#arena-acquisition)).
/// - **double tap** (`on_double_tap`) — two quick taps within the double-tap
///   window. Combines correctly with `on_tap`: the double-tap recognizer holds
///   the presentation arena across the inter-tap
///   window, so the binding's first-up sweep is deferred and the front-member
///   tap cannot win early — two quick taps fire `on_double_tap` once (not `on_tap`
///   twice), and a lone tap is held until the window closes, then fires
///   `on_tap` once.
/// - **pan/drag** (`on_pan_start` / `on_pan_update` / `on_pan_end`) — a contact
///   that moves past the drag slop, reported with running deltas and a release
///   velocity. Free axis: recognized on any direction of travel.
/// - **horizontal drag** (`on_horizontal_drag_down` / `on_horizontal_drag_start`
///   / `on_horizontal_drag_update` / `on_horizontal_drag_end` /
///   `on_horizontal_drag_cancel`) — a contact that moves past the drag slop on
///   the horizontal axis specifically. A separate, axis-constrained
///   [`DragGestureRecognizer`] ([`DragAxis::Horizontal`]) from the free-axis pan
///   recognizer above; see the [conflict](#pan-and-horizontal-drag-conflict)
///   note on why the two are mutually exclusive on one detector.
///
/// Only the recognizers whose callback is set participate in the arena for a
/// contact. Admission reads the current callbacks on Down; every admitted
/// contact still receives its terminal event if those callbacks change.
/// They compete in one arena: a quick down→up resolves to the tap
/// (the front member), a hold resolves to the long-press, a drag past slop hands
/// off to whichever drag-family recognizer is configured — so at most one
/// gesture fires per contact.
///
/// # Pan and horizontal-drag conflict
///
/// Configuring both `on_pan_*` and `on_horizontal_drag_*` on the same detector
/// is a `debug_assert!` failure: FLUI's pan recognizer is already
/// [`DragAxis::Free`] — it spans the horizontal axis too — so it would compete
/// directly with the horizontal recognizer for the exact same horizontal
/// motion, and which family wins becomes registration-order-dependent rather
/// than deterministic.
///
/// FLUI has no `on_vertical_drag_*` family yet, so the guard is
/// `on_pan_*` + `on_horizontal_drag_*` alone — that pairing is already
/// redundant without waiting for a vertical family to complete the overlap. Combine the two into one family instead: `on_horizontal_drag_*`
/// alone, or `on_pan_*` alone.
///
/// # Assistive-technology activation
///
/// A detector
/// with `on_tap` advertises a semantics *tap* action, and one with
/// `on_long_press` a *long press* action, so a screen reader's activate
/// gesture (VoiceOver's VO-Space, `AXPress` on macOS) presses the control
/// with no pointer event anywhere. The [`Semantics`] node is not a boundary:
/// the action merges into the nearest ancestor node, which for a Material
/// button is the button's own node (`ButtonStyleButtonCore`).
///
/// The platform's action arrives through a `Send + Sync` handler while the
/// detector's callbacks are `Rc` (they capture the tree's own state), so the
/// handler only records the request and schedules a rebuild
/// (`RebuildHandle`); the next `build`, on the UI thread, hands the request
/// to a `LocalPostFrameHandle` which runs the `Rc` callback after that
/// frame — never inside `build`, where a callback that sets state would be
/// re-entrant and its signal writes are refused. One frame of latency, no
/// unsafe, and a request that arrives while the detector is unmounted is
/// dropped with its element. A context with no local post-frame lane drops
/// the request with a warning rather than run the callback inside `build`.
///
/// # Event context
///
/// Every callback receives the dispatch's `&mut EventCx<'_>` first, so it
/// writes a signal directly (ADR-0086):
///
/// ```rust,ignore
/// GestureDetector::new()
///     .on_tap(move |cx| count.update(cx, |n| *n += 1))
///     .on_pan_update(move |cx, details| offset.update(cx, |o| *o += details.delta))
/// ```
///
/// A callback may return `()` or the `Result` of a write; a refused write is
/// logged on the `flui::signals` target. The detector acquires a
/// [`WriterSource`] in `init_state` and opens one write per dispatch around
/// the callback, so the gesture recognizers themselves do not change
/// (ADR-0086 §4). A closure bound with `let` before it is passed needs
/// [`callback`] or
/// [`callback_with`] to fix its signature.
///
/// # Arena acquisition
///
/// In `init_state` the detector reads an ambient [`GestureArenaScope`] via
/// [`GestureArenaScope::of`]. All recognizers use the presentation's shared,
/// clock-bound arena. The binding drives the *close*-on-down /
/// *sweep*-on-up lifecycle after routing each event to the whole hit-test path,
/// so overlapping detectors genuinely compete in one arena and
/// `on_tap` + `on_double_tap` combine correctly. Mounting outside that scope is
/// an invariant violation; there is no private-arena execution mode.
///
/// # Hit behavior
///
/// The default is [`DeferToChild`](HitTestBehavior::DeferToChild): a gesture is
/// recognized only when the contact lands on a hit-testable descendant. Override
/// with [`behavior`](Self::behavior) — for example, scroll areas use
/// [`Opaque`](HitTestBehavior::Opaque) so they fire regardless of content.
#[derive(Clone, StatefulView)]
pub struct GestureDetector {
    on_tap: Option<GestureCallback>,
    on_secondary_tap: Option<GestureCallback>,
    on_long_press: Option<GestureCallback>,
    on_double_tap: Option<GestureCallback>,
    on_double_tap_down: Option<DoubleTapDownHandler>,
    on_pan_start: Option<PanStartHandler>,
    on_pan_update: Option<PanUpdateHandler>,
    on_pan_end: Option<PanEndHandler>,
    on_horizontal_drag_down: Option<HorizontalDragDownHandler>,
    on_horizontal_drag_start: Option<HorizontalDragStartHandler>,
    on_horizontal_drag_update: Option<HorizontalDragUpdateHandler>,
    on_horizontal_drag_end: Option<HorizontalDragEndHandler>,
    on_horizontal_drag_cancel: Option<HorizontalDragCancelHandler>,
    /// How the underlying [`Listener`] participates in hit-testing.
    behavior: HitTestBehavior,
    child: Child,
}

impl Default for GestureDetector {
    fn default() -> Self {
        Self {
            on_tap: None,
            on_secondary_tap: None,
            on_long_press: None,
            on_double_tap: None,
            on_double_tap_down: None,
            on_pan_start: None,
            on_pan_update: None,
            on_pan_end: None,
            on_horizontal_drag_down: None,
            on_horizontal_drag_start: None,
            on_horizontal_drag_update: None,
            on_horizontal_drag_end: None,
            on_horizontal_drag_cancel: None,
            behavior: HitTestBehavior::DeferToChild,
            child: Child::empty(),
        }
    }
}

impl std::fmt::Debug for GestureDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetector")
            .field("on_tap", &self.on_tap.is_some())
            .field("on_secondary_tap", &self.on_secondary_tap.is_some())
            .field("on_long_press", &self.on_long_press.is_some())
            .field("on_double_tap", &self.on_double_tap.is_some())
            .field("on_double_tap_down", &self.on_double_tap_down.is_some())
            .field("on_pan_start", &self.on_pan_start.is_some())
            .field("on_pan_update", &self.on_pan_update.is_some())
            .field("on_pan_end", &self.on_pan_end.is_some())
            .field(
                "on_horizontal_drag_down",
                &self.on_horizontal_drag_down.is_some(),
            )
            .field(
                "on_horizontal_drag_start",
                &self.on_horizontal_drag_start.is_some(),
            )
            .field(
                "on_horizontal_drag_update",
                &self.on_horizontal_drag_update.is_some(),
            )
            .field(
                "on_horizontal_drag_end",
                &self.on_horizontal_drag_end.is_some(),
            )
            .field(
                "on_horizontal_drag_cancel",
                &self.on_horizontal_drag_cancel.is_some(),
            )
            .field("behavior", &self.behavior)
            .finish_non_exhaustive()
    }
}

impl GestureDetector {
    /// A detector with no callbacks yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when the child is tapped (a primary-button down + up without
    /// moving past the touch slop).
    #[must_use]
    pub fn on_tap<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_tap = Some(event_callback(callback));
        self
    }

    /// Called when the child receives a secondary-button tap (right-click down
    /// + up without moving past the touch slop).
    #[must_use]
    pub fn on_secondary_tap<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_secondary_tap = Some(event_callback(callback));
        self
    }

    /// Called when the child is long-pressed (the contact held still past the
    /// long-press deadline).
    ///
    /// Deadline-driven: the presentation binding polls the shared arena's hold
    /// deadline each frame. The detector must be mounted beneath
    /// [`GestureArenaScope`]; see [arena acquisition](Self#arena-acquisition).
    #[must_use]
    pub fn on_long_press<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_long_press = Some(event_callback(callback));
        self
    }

    /// Called when the child is double-tapped (two quick taps within the
    /// double-tap window). The inter-tap timing reads the arena clock.
    ///
    /// Combines correctly with [`on_tap`](Self::on_tap): the double-tap
    /// recognizer holds the presentation arena across the inter-tap window, so
    /// the binding's first-up sweep is deferred and the tap cannot win early.
    /// Two quick taps fire `on_double_tap` once (never `on_tap` twice); a lone
    /// tap is held until the window closes, then fires `on_tap` once.
    #[must_use]
    pub fn on_double_tap<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_double_tap = Some(event_callback(callback));
        self
    }

    /// Called the instant the second contact of a double-tap goes down.
    /// Fires before, and independently of,
    /// [`on_double_tap`](Self::on_double_tap): the recognizer already knows
    /// the gesture is a double tap once the second contact is validated
    /// (timing + slop against the first), and a consumer that wants the
    /// tap's position as soon as that is known — double-tap word selection,
    /// say, which wants to select immediately rather than wait for the
    /// second contact to also lift cleanly — should not have to wait the
    /// extra down-to-up round trip `on_double_tap` needs. Both callbacks
    /// fire for a gesture that completes normally; only `on_double_tap_down`
    /// fires if the second contact is then dragged past slop or cancelled.
    #[must_use]
    pub fn on_double_tap_down<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DoubleTapDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_double_tap_down = Some(value_callback(callback));
        self
    }

    /// Called once when a pan/drag begins (the contact crosses the drag slop).
    #[must_use]
    pub fn on_pan_start<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragStartDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pan_start = Some(value_callback(callback));
        self
    }

    /// Called for each pointer move while a pan/drag is in progress, carrying
    /// the incremental delta since the previous update.
    #[must_use]
    pub fn on_pan_update<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragUpdateDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pan_update = Some(value_callback(callback));
        self
    }

    /// Called once when the pan/drag ends (pointer up), carrying the release
    /// velocity.
    #[must_use]
    /// Accepted pointer cancellation also delivers this callback, with
    /// [`DragEndDetails::reason`] set to [`flui_interaction::GestureEndReason::Cancelled`].
    pub fn on_pan_end<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragEndDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pan_end = Some(value_callback(callback));
        self
    }

    /// Called when a pointer that might begin a horizontal drag contacts the
    /// screen — before any movement threshold is met. Mutually exclusive with
    /// `on_pan_*` on one detector; see the type docs.
    #[must_use]
    pub fn on_horizontal_drag_down<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragDownDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_horizontal_drag_down = Some(value_callback(callback));
        self
    }

    /// Called once when a horizontal drag begins (the contact crosses the
    /// drag slop on the horizontal axis). Mutually exclusive with `on_pan_*`
    /// on one detector; see the type docs.
    #[must_use]
    pub fn on_horizontal_drag_start<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragStartDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_horizontal_drag_start = Some(value_callback(callback));
        self
    }

    /// Called for each pointer move while a horizontal drag is in progress,
    /// carrying the incremental delta since the previous update. Mutually
    /// exclusive with `on_pan_*` on one detector; see the type docs.
    #[must_use]
    pub fn on_horizontal_drag_update<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragUpdateDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_horizontal_drag_update = Some(value_callback(callback));
        self
    }

    /// Called once when the horizontal drag ends (pointer up), carrying the
    /// release velocity. Mutually exclusive with `on_pan_*` on one detector;
    /// see the type docs.
    #[must_use]
    /// Accepted pointer cancellation delivers an end with
    /// [`DragEndDetails::reason`] set to [`flui_interaction::GestureEndReason::Cancelled`].
    pub fn on_horizontal_drag_end<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragEndDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_horizontal_drag_end = Some(value_callback(callback));
        self
    }

    /// Called when the horizontal drag is cancelled (e.g. the arena rejects
    /// it). Mutually exclusive with `on_pan_*` on one detector; see the type
    /// docs.
    #[must_use]
    pub fn on_horizontal_drag_cancel<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_horizontal_drag_cancel = Some(event_callback(callback));
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

/// The pan callbacks the drag recognizer reads, refreshed from the view on every
/// `build`.
#[derive(Clone, Default)]
struct PanCallbacks {
    start: Option<PanStartHandler>,
    update: Option<PanUpdateHandler>,
    end: Option<PanEndHandler>,
}

/// The horizontal-drag callbacks the axis-constrained recognizer reads,
/// refreshed from the view on every `build`. Mirrors [`PanCallbacks`] plus the
/// `down`/`cancel` pair the horizontal-drag family also exposes.
#[derive(Clone, Default)]
struct HorizontalDragCallbacks {
    down: Option<HorizontalDragDownHandler>,
    start: Option<HorizontalDragStartHandler>,
    update: Option<HorizontalDragUpdateHandler>,
    end: Option<HorizontalDragEndHandler>,
    cancel: Option<HorizontalDragCancelHandler>,
}

/// The recognizers, built once in [`GestureDetectorState::init_state`] against
/// the presentation arena.
///
/// They are not built in `create_state` because that has no `BuildContext` and
/// so cannot read the ambient [`GestureArenaScope`]; a recognizer's arena is
/// captured at construction and is not swappable, so construction must wait for
/// the live context `init_state` receives.
struct Recognizers {
    /// Tap recognizer — added to the arena FIRST so it is the front member that
    /// wins an ambiguous quick tap on sweep.
    tap: Rc<TapGestureRecognizer>,
    /// Long-press recognizer — wins when its hold deadline fires (binding-polled).
    long_press: Rc<LongPressGestureRecognizer>,
    /// Double-tap recognizer — completes purely from the event stream; its
    /// give-up timer is binding-polled.
    double_tap: Rc<DoubleTapGestureRecognizer>,
    /// Pan/drag recognizer (free axis) — wins by attrition when a move past the
    /// slop makes the tap reject itself.
    drag: Rc<DragGestureRecognizer>,
    /// Horizontal-drag recognizer (axis-constrained) — mutually exclusive with
    /// `drag` on one detector, see [`GestureDetector`]'s conflict doc.
    horizontal_drag: Rc<DragGestureRecognizer>,
}

/// Persistent gesture state: the recognizers + their shared arena survive
/// rebuilds (the pointer stream is stateful), and are cancelled on unmount.
///
/// `create_state` allocates only the live callback slots; the recognizers are
/// built in `init_state` (which has the `BuildContext` needed to read the
/// ambient arena) and read — never rebuilt — by `build`.
pub struct GestureDetectorState {
    /// Shared admission authority for captured pointer and semantics delivery.
    mounted: Rc<Cell<bool>>,
    /// The live `on_tap`, refreshed each `build`. The recognizer reads THIS slot
    /// rather than a frozen capture, so a rebuild with a new closure is honored.
    tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    /// The live `on_secondary_tap`, refreshed each `build`.
    secondary_tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    /// The live `on_long_press`, refreshed each `build`.
    long_press_slot: Rc<RefCell<Option<GestureCallback>>>,
    /// The live `on_double_tap`, refreshed each `build`.
    double_tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    /// The live `on_double_tap_down`, refreshed each `build`.
    double_tap_down_slot: Rc<RefCell<Option<DoubleTapDownHandler>>>,
    /// The live pan callbacks, refreshed each `build`.
    pan_slot: Rc<RefCell<PanCallbacks>>,
    /// The live horizontal-drag callbacks, refreshed each `build`.
    horizontal_drag_slot: Rc<RefCell<HorizontalDragCallbacks>>,
    /// The recognizers + arena, built once in `init_state`. `None` only in the
    /// window between `create_state` and the first `init_state` — never observed
    /// by `build`, which always runs after `init_state`.
    recognizers: Option<Recognizers>,
    /// Activation requests from assistive technology, recorded by the
    /// `Send + Sync` semantics-action handlers and drained on the next
    /// `build` — see the type doc's "Assistive-technology activation".
    semantics_requests: Arc<SemanticsRequests>,
    /// Minted in `init_state`; the handlers schedule the draining rebuild
    /// through it.
    rebuild: Option<RebuildHandle>,
    /// Minted in `init_state`; the drained request's `Rc` callback runs
    /// through it after the frame.
    local_post_frame: Option<flui_view::LocalPostFrameHandle>,
    /// Owner-local target for queued semantics delivery. Post-frame callbacks
    /// retain only a weak reference, so the lane cannot keep this detector's
    /// callbacks or writer source alive after the state is dropped.
    semantics_delivery: Option<Rc<SemanticsDeliveryTarget>>,
}

/// The owner-local resources needed to deliver one queued semantics action.
///
/// The state owns this target. A post-frame queue holds only [`std::rc::Weak`]
/// references to it, which preserves live callback replacement while the
/// detector is mounted without extending any of these resources past teardown.
struct SemanticsDeliveryTarget {
    tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    long_press_slot: Rc<RefCell<Option<GestureCallback>>>,
    writer: WriterSource,
    mounted: Rc<Cell<bool>>,
}

impl SemanticsDeliveryTarget {
    fn deliver(&self, action: PendingSemanticsAction) {
        if !self.mounted.get() {
            return;
        }
        let callback = match action {
            PendingSemanticsAction::Tap => self.tap_slot.borrow().clone(),
            PendingSemanticsAction::LongPress => self.long_press_slot.borrow().clone(),
        };
        if let Some(callback) = callback {
            self.writer.write(|cx| callback(cx));
        }
    }
}

/// The assistive-technology activations a detector has accepted but not yet
/// performed. Each platform request is a distinct command; the FIFO preserves
/// both multiplicity and ordering across action kinds while rebuild requests
/// remain free to coalesce as a wake-up optimization.
#[derive(Default)]
struct SemanticsRequests {
    pending: Mutex<VecDeque<PendingSemanticsAction>>,
}

#[derive(Clone, Copy)]
enum PendingSemanticsAction {
    Tap,
    LongPress,
}

impl SemanticsRequests {
    fn push(&self, action: PendingSemanticsAction) {
        self.pending
            .lock()
            .expect("BUG: semantics request lock is never held across user code")
            .push_back(action);
    }

    fn take_all(&self) -> VecDeque<PendingSemanticsAction> {
        std::mem::take(
            &mut *self
                .pending
                .lock()
                .expect("BUG: semantics request lock is never held across user code"),
        )
    }
}

impl std::fmt::Debug for GestureDetectorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetectorState")
            .field("initialized", &self.recognizers.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for GestureDetector {
    type State = GestureDetectorState;

    fn create_state(&self) -> Self::State {
        // Allocate the live callback slots only — recognizers are built in
        // `init_state`, which has the context needed to read the ambient arena.
        GestureDetectorState {
            mounted: Rc::new(Cell::new(true)),
            tap_slot: Rc::new(RefCell::new(self.on_tap.clone())),
            secondary_tap_slot: Rc::new(RefCell::new(self.on_secondary_tap.clone())),
            long_press_slot: Rc::new(RefCell::new(self.on_long_press.clone())),
            double_tap_slot: Rc::new(RefCell::new(self.on_double_tap.clone())),
            double_tap_down_slot: Rc::new(RefCell::new(self.on_double_tap_down.clone())),
            pan_slot: Rc::new(RefCell::new(PanCallbacks {
                start: self.on_pan_start.clone(),
                update: self.on_pan_update.clone(),
                end: self.on_pan_end.clone(),
            })),
            horizontal_drag_slot: Rc::new(RefCell::new(HorizontalDragCallbacks {
                down: self.on_horizontal_drag_down.clone(),
                start: self.on_horizontal_drag_start.clone(),
                update: self.on_horizontal_drag_update.clone(),
                end: self.on_horizontal_drag_end.clone(),
                cancel: self.on_horizontal_drag_cancel.clone(),
            })),
            recognizers: None,
            semantics_requests: Arc::new(SemanticsRequests::default()),
            rebuild: None,
            local_post_frame: None,
            semantics_delivery: None,
        }
    }
}

impl GestureDetectorState {
    /// Perform the assistive-technology activations recorded since the last
    /// `build`: each pending request's live `Rc` callback is scheduled to
    /// run after this frame, inside a write the detector's source opens.
    /// Called at the top of `build`, on the UI thread.
    ///
    /// A context with no local post-frame lane has no moment after the frame
    /// to offer, and running the callback here would run it inside `build`,
    /// where its writes are refused. The activation is dropped with a
    /// warning instead.
    fn drain_semantics_requests(&self) {
        let mut pending = self.semantics_requests.take_all();
        if pending.is_empty() {
            return;
        }
        let Some(handle) = self.local_post_frame.as_ref() else {
            tracing::warn!(
                count = pending.len(),
                "GestureDetector: dropping an assistive-technology activation batch — \
                 the context has no local post-frame lane, and running them now would \
                 run them inside build"
            );
            return;
        };
        let delivery = self
            .semantics_delivery
            .as_ref()
            .expect("BUG: init_state creates the semantics delivery target before the first build");
        while let Some(action) = pending.pop_front() {
            let delivery = Rc::downgrade(delivery);
            if let Err(error) = handle.schedule_local(move |_timing| {
                if let Some(delivery) = delivery.upgrade() {
                    delivery.deliver(action);
                }
            }) {
                tracing::warn!(
                    ?error,
                    dropped = pending.len() + 1,
                    "GestureDetector: dropping an assistive-technology activation batch — \
                     the owning lane is gone"
                );
                break;
            }
        }
    }

    /// The semantics node advertising this detector's activations to
    /// assistive technology, or `None` when it has nothing to advertise.
    fn semantics_actions(&self, view: &GestureDetector) -> Option<Semantics> {
        if view.on_tap.is_none() && view.on_long_press.is_none() {
            return None;
        }
        let rebuild = self.rebuild.clone()?;
        let mut semantics = Semantics::new();
        if view.on_tap.is_some() {
            let requests = Arc::clone(&self.semantics_requests);
            let rebuild = rebuild.clone();
            semantics = semantics.on_tap(move |_cx| {
                requests.push(PendingSemanticsAction::Tap);
                rebuild.schedule(flui_view::RebuildReason::StateChange);
            });
        }
        if view.on_long_press.is_some() {
            let requests = Arc::clone(&self.semantics_requests);
            semantics = semantics.on_long_press(move |_cx| {
                requests.push(PendingSemanticsAction::LongPress);
                rebuild.schedule(flui_view::RebuildReason::StateChange);
            });
        }
        Some(semantics)
    }
}

impl ViewState<GestureDetector> for GestureDetectorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let writer = ctx.writer_source();
        self.semantics_delivery = Some(Rc::new(SemanticsDeliveryTarget {
            tap_slot: Rc::clone(&self.tap_slot),
            long_press_slot: Rc::clone(&self.long_press_slot),
            writer: writer.clone(),
            mounted: Rc::clone(&self.mounted),
        }));
        let arena = GestureArenaScope::of(ctx);
        self.rebuild = Some(ctx.rebuild_handle());
        self.local_post_frame = ctx.local_post_frame_handle();

        // Each recognizer reads its live slot OUT before invoking it, so a slot
        // lock is never held across user code (no re-entrancy / poison hazard),
        // and runs it inside a write the detector's source opens (ADR-0086 §4:
        // the recognizers themselves are unchanged).
        let tap = {
            let primary_slot = Rc::clone(&self.tap_slot);
            let secondary_slot = Rc::clone(&self.secondary_tap_slot);
            let primary_writer = writer.clone();
            let secondary_writer = writer.clone();
            TapGestureRecognizer::builder(arena.clone())
                .on_tap(move |_details| {
                    let handler = primary_slot.borrow().clone();
                    if let Some(handler) = handler {
                        primary_writer.write(|cx| handler(cx));
                    }
                })
                .on_secondary_tap(move |_details| {
                    let handler = secondary_slot.borrow().clone();
                    if let Some(handler) = handler {
                        secondary_writer.write(|cx| handler(cx));
                    }
                })
                .build()
        };

        let long_press = {
            let slot = Rc::clone(&self.long_press_slot);
            let writer = writer.clone();
            LongPressGestureRecognizer::builder(arena.clone())
                .on_long_press(move || {
                    let handler = slot.borrow().clone();
                    if let Some(handler) = handler {
                        writer.write(|cx| handler(cx));
                    }
                })
                .build()
        };

        let double_tap = {
            let slot = Rc::clone(&self.double_tap_slot);
            let down_slot = Rc::clone(&self.double_tap_down_slot);
            let tap_writer = writer.clone();
            let down_writer = writer.clone();
            DoubleTapGestureRecognizer::builder(arena.clone())
                .on_double_tap(move |_details| {
                    let handler = slot.borrow().clone();
                    if let Some(handler) = handler {
                        tap_writer.write(|cx| handler(cx));
                    }
                })
                .on_double_tap_down(move |details| {
                    let handler = down_slot.borrow().clone();
                    if let Some(handler) = handler {
                        down_writer.write(|cx| handler(cx, details));
                    }
                })
                .build()
        };

        let drag = {
            let start_slot = Rc::clone(&self.pan_slot);
            let update_slot = Rc::clone(&self.pan_slot);
            let end_slot = Rc::clone(&self.pan_slot);
            let start_writer = writer.clone();
            let update_writer = writer.clone();
            let end_writer = writer.clone();
            DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
                .on_start(move |details| {
                    let callback = start_slot.borrow().start.clone();
                    if let Some(callback) = callback {
                        start_writer.write(|cx| callback(cx, details));
                    }
                })
                .on_update(move |details| {
                    let callback = update_slot.borrow().update.clone();
                    if let Some(callback) = callback {
                        update_writer.write(|cx| callback(cx, details));
                    }
                })
                .on_end(move |details| {
                    let callback = end_slot.borrow().end.clone();
                    if let Some(callback) = callback {
                        end_writer.write(|cx| callback(cx, details));
                    }
                })
                .build()
        };

        let horizontal_drag = {
            let down_slot = Rc::clone(&self.horizontal_drag_slot);
            let start_slot = Rc::clone(&self.horizontal_drag_slot);
            let update_slot = Rc::clone(&self.horizontal_drag_slot);
            let end_slot = Rc::clone(&self.horizontal_drag_slot);
            let cancel_slot = Rc::clone(&self.horizontal_drag_slot);
            let down_writer = writer.clone();
            let start_writer = writer.clone();
            let update_writer = writer.clone();
            let end_writer = writer.clone();
            let cancel_writer = writer;
            DragGestureRecognizer::builder(arena, DragAxis::Horizontal)
                .on_down(move |details| {
                    let callback = down_slot.borrow().down.clone();
                    if let Some(callback) = callback {
                        down_writer.write(|cx| callback(cx, details));
                    }
                })
                .on_start(move |details| {
                    let callback = start_slot.borrow().start.clone();
                    if let Some(callback) = callback {
                        start_writer.write(|cx| callback(cx, details));
                    }
                })
                .on_update(move |details| {
                    let callback = update_slot.borrow().update.clone();
                    if let Some(callback) = callback {
                        update_writer.write(|cx| callback(cx, details));
                    }
                })
                .on_end(move |details| {
                    let callback = end_slot.borrow().end.clone();
                    if let Some(callback) = callback {
                        end_writer.write(|cx| callback(cx, details));
                    }
                })
                .on_cancel(move || {
                    let callback = cancel_slot.borrow().cancel.clone();
                    if let Some(callback) = callback {
                        cancel_writer.write(|cx| callback(cx));
                    }
                })
                .build()
        };

        self.recognizers = Some(Recognizers {
            tap,
            long_press,
            double_tap,
            drag,
            horizontal_drag,
        });
    }

    fn build(&self, view: &GestureDetector, _ctx: &dyn BuildContext) -> impl IntoView {
        assert_no_pan_horizontal_drag_conflict(view);

        // Refresh the live callbacks the recognizers read, so a rebuild with new
        // closures is honored (the recognizers themselves persist).
        self.tap_slot.borrow_mut().clone_from(&view.on_tap);
        self.secondary_tap_slot
            .borrow_mut()
            .clone_from(&view.on_secondary_tap);
        self.long_press_slot
            .borrow_mut()
            .clone_from(&view.on_long_press);
        self.double_tap_slot
            .borrow_mut()
            .clone_from(&view.on_double_tap);
        self.double_tap_down_slot
            .borrow_mut()
            .clone_from(&view.on_double_tap_down);
        {
            let mut slot = self.pan_slot.borrow_mut();
            slot.start.clone_from(&view.on_pan_start);
            slot.update.clone_from(&view.on_pan_update);
            slot.end.clone_from(&view.on_pan_end);
        }
        {
            let mut slot = self.horizontal_drag_slot.borrow_mut();
            slot.down.clone_from(&view.on_horizontal_drag_down);
            slot.start.clone_from(&view.on_horizontal_drag_start);
            slot.update.clone_from(&view.on_horizontal_drag_update);
            slot.end.clone_from(&view.on_horizontal_drag_end);
            slot.cancel.clone_from(&view.on_horizontal_drag_cancel);
        }

        self.drain_semantics_requests();

        // `init_state` runs exactly once before the first `build`, so the
        // recognizers are always present here.
        let recognizers = self
            .recognizers
            .as_ref()
            .expect("init_state builds the recognizers before the first build");

        let listener = self.make_listener(recognizers).behavior(view.behavior);

        let listener = match view.child.clone().into_inner() {
            Some(child) => listener.child(child),
            None => listener,
        };
        match self.semantics_actions(view) {
            Some(semantics) => semantics.child(listener).boxed(),
            None => listener.boxed(),
        }
    }

    fn dispose(&mut self) {
        // Cached Listener routes remain deliverable through their terminal
        // event. Revoke this group's admission before recognizer retirement can
        // invoke cancellation callbacks or reenter pointer dispatch.
        self.mounted.set(false);
        if let Some(recognizers) = self.recognizers.take() {
            cancel_all([
                &*recognizers.tap as &dyn GestureRecognizer,
                &*recognizers.long_press,
                &*recognizers.double_tap,
                &*recognizers.drag,
                &*recognizers.horizontal_drag,
            ]);
        }
    }
}

/// Debug-only conflict guard for `on_pan_*` and `on_horizontal_drag_*` — see
/// [`GestureDetector`]'s "Pan and horizontal-drag conflict" doc section for
/// the full rationale.
/// Only `start`/`update`/`end` count toward "configured" (`down`/`cancel`
/// don't participate).
fn assert_no_pan_horizontal_drag_conflict(view: &GestureDetector) {
    let have_pan =
        view.on_pan_start.is_some() || view.on_pan_update.is_some() || view.on_pan_end.is_some();
    let have_horizontal_drag = view.on_horizontal_drag_start.is_some()
        || view.on_horizontal_drag_update.is_some()
        || view.on_horizontal_drag_end.is_some();
    debug_assert!(
        !(have_pan && have_horizontal_drag),
        "GestureDetector: on_pan_* and on_horizontal_drag_* are both configured on one \
         detector. FLUI's pan recognizer is DragAxis::Free — it already spans the horizontal \
         axis — so it competes directly with the horizontal recognizer for the same \
         horizontal motion, and which family wins the arena becomes registration-order- \
         dependent rather than deterministic. Use on_horizontal_drag_* alone, or on_pan_* \
         alone.",
    );
}

impl GestureDetectorState {
    /// Build the [`Listener`] that drives the recognizers from the pointer
    /// stream.
    ///
    /// Each recognizer participates for a contact only when its configured
    /// callback is live. Participation is read from
    /// the live slots at event time (the `*_active` predicates), so a rebuild
    /// with a changed configuration is honored, and a double-tap-only detector
    /// does not let its tap recognizer steal the first up.
    fn make_listener(&self, recognizers: &Recognizers) -> Listener {
        let gates = Rc::new(RecognizerGates {
            mounted: Rc::clone(&self.mounted),
            tap_slot: Rc::clone(&self.tap_slot),
            secondary_tap_slot: Rc::clone(&self.secondary_tap_slot),
            long_press_slot: Rc::clone(&self.long_press_slot),
            double_tap_slot: Rc::clone(&self.double_tap_slot),
            double_tap_down_slot: Rc::clone(&self.double_tap_down_slot),
            pan_slot: Rc::clone(&self.pan_slot),
            horizontal_drag_slot: Rc::clone(&self.horizontal_drag_slot),
        });

        // The whole `PointerDispatch` goes through, both spaces. Dispatch
        // rewrites an event into the receiving node's coordinates before a
        // handler runs, so the untransformed position exists nowhere below
        // this point except in the pair's global half — a recogniser handed
        // only the local event has no way to report a global position and can
        // only restate the local one under that name (issue #908).
        Listener::new()
            .recognizer_when(&recognizers.tap, {
                let gates = Rc::clone(&gates);
                move |_| gates.tap_active()
            })
            .recognizer_when(&recognizers.long_press, {
                let gates = Rc::clone(&gates);
                move |_| gates.long_press_active()
            })
            .recognizer_when(&recognizers.double_tap, {
                let gates = Rc::clone(&gates);
                move |_| gates.double_tap_active()
            })
            .recognizer_when(&recognizers.drag, {
                let gates = Rc::clone(&gates);
                move |_| gates.drag_active()
            })
            .recognizer_when(&recognizers.horizontal_drag, move |_| {
                gates.horizontal_drag_active()
            })
    }
}

/// Live admission predicates. The Listener holds these slots, while only the
/// widget state owns the recognizers themselves.
struct RecognizerGates {
    mounted: Rc<Cell<bool>>,
    tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    secondary_tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    long_press_slot: Rc<RefCell<Option<GestureCallback>>>,
    double_tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    double_tap_down_slot: Rc<RefCell<Option<DoubleTapDownHandler>>>,
    pan_slot: Rc<RefCell<PanCallbacks>>,
    horizontal_drag_slot: Rc<RefCell<HorizontalDragCallbacks>>,
}

impl RecognizerGates {
    /// The tap recognizer participates iff a primary- OR secondary-tap callback
    /// is currently set.
    fn tap_active(&self) -> bool {
        self.mounted.get()
            && (slot_is_some(&self.tap_slot) || slot_is_some(&self.secondary_tap_slot))
    }

    /// The long-press recognizer participates iff `on_long_press` is set.
    fn long_press_active(&self) -> bool {
        self.mounted.get() && slot_is_some(&self.long_press_slot)
    }

    /// The double-tap recognizer participates iff `on_double_tap` OR
    /// `on_double_tap_down` is set — a detector configured with only the
    /// latter (word selection, which never needs `on_double_tap` itself)
    /// must still join the arena, or its own callback would never fire.
    fn double_tap_active(&self) -> bool {
        self.mounted.get()
            && (slot_is_some(&self.double_tap_slot) || slot_is_some(&self.double_tap_down_slot))
    }

    /// The drag recognizer participates iff any pan callback is set.
    fn drag_active(&self) -> bool {
        if !self.mounted.get() {
            return false;
        }
        let pan = self.pan_slot.borrow();
        pan.start.is_some() || pan.update.is_some() || pan.end.is_some()
    }

    /// The horizontal-drag recognizer participates iff any horizontal-drag
    /// callback is set.
    fn horizontal_drag_active(&self) -> bool {
        if !self.mounted.get() {
            return false;
        }
        let horizontal = self.horizontal_drag_slot.borrow();
        horizontal.down.is_some()
            || horizontal.start.is_some()
            || horizontal.update.is_some()
            || horizontal.end.is_some()
            || horizontal.cancel.is_some()
    }
}

/// `true` when a callback slot currently holds a handler — generic over
/// the callback's own type (`GestureCallback`, `DoubleTapDownHandler`,
/// ...) since only presence, never the callback itself, is read here.
fn slot_is_some<T>(slot: &Rc<RefCell<Option<T>>>) -> bool {
    slot.borrow().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[expect(
        clippy::arc_with_non_send_sync,
        reason = "ElementBuildContext's test seam accepts Arc over owner-local state"
    )]
    fn queued_semantics_delivery_rechecks_the_callback_and_mount_lifetime() {
        let tree = Arc::new(parking_lot::RwLock::new(flui_view::ElementTree::new()));
        let owner = Arc::new(parking_lot::RwLock::new(flui_view::BuildOwner::new()));
        let writer = flui_view::ElementBuildContext::new(
            flui_foundation::ElementId::new(1),
            0,
            false,
            tree,
            owner,
        )
        .writer_source();
        for change in ["replace", "remove", "dispose"] {
            let scheduler = flui_scheduler::UpdateScheduler::new();
            let owner_frame = flui_scheduler::OwnerFrame::new(&scheduler)
                .expect("the scheduler has no live owner frame");
            let calls = Rc::new(Cell::new(0));
            let old_calls = Rc::clone(&calls);
            let mut state = GestureDetector::new()
                .on_tap(move |_cx| old_calls.set(1))
                .create_state();
            state.semantics_delivery = Some(Rc::new(SemanticsDeliveryTarget {
                tap_slot: Rc::clone(&state.tap_slot),
                long_press_slot: Rc::clone(&state.long_press_slot),
                writer: writer.clone(),
                mounted: Rc::clone(&state.mounted),
            }));
            state.local_post_frame = Some(owner_frame.local_post_frame_handle());
            state.semantics_requests.push(PendingSemanticsAction::Tap);
            // Queue through the production semantics-to-post-frame bridge,
            // then alter its target before the real scheduler delivers it.
            state.drain_semantics_requests();
            match change {
                "replace" => {
                    let new_calls = Rc::clone(&calls);
                    *state.tap_slot.borrow_mut() =
                        Some(event_callback(move |_cx| new_calls.set(2)));
                }
                "remove" => *state.tap_slot.borrow_mut() = None,
                "dispose" => state.dispose(),
                _ => unreachable!(),
            }
            scheduler.execute_frame(&owner_frame);
            assert_eq!(
                calls.get(),
                if change == "replace" { 2 } else { 0 },
                "{change}"
            );
        }
    }

    /// The gesture-detector contracts: queued semantics delivery re-checks the callback
    /// and mount lifetime, and (debug builds only, where the guard exists) configuring
    /// pan and horizontal-drag together is refused with a named diagnostic.
    #[test]
    fn gesture_detector_delivery_lifetime_and_conflict_guard() {
        queued_semantics_delivery_rechecks_the_callback_and_mount_lifetime();
        #[cfg(debug_assertions)]
        {
            let detector = GestureDetector::new()
                .on_pan_start(|_, _| {})
                .on_horizontal_drag_start(|_, _| {});
            let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_no_pan_horizontal_drag_conflict(&detector);
            }))
            .expect_err("pan and horizontal drag together must be refused");
            let message = refused
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| refused.downcast_ref::<&str>().copied())
                .unwrap_or_default();
            assert!(
                message.contains("on_pan_* and on_horizontal_drag_* are both configured"),
                "conflict guard: unexpected diagnostic {message:?}"
            );
        }
    }
}
