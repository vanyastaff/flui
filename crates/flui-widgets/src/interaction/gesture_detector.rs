//! [`GestureDetector`] — recognizes high-level gestures (tap, long-press,
//! double-tap, and pan/drag) from the raw pointer stream a [`Listener`] delivers.

use std::sync::{Arc, Mutex};

use flui_interaction::arena::GestureArena;
use flui_interaction::{
    DoubleTapGestureRecognizer, DragAxis, DragEndDetails, DragGestureRecognizer, DragStartDetails,
    DragUpdateDetails, GestureRecognizer, LongPressGestureRecognizer, PointerEvent,
    PointerEventExt, TapGestureRecognizer,
};
use flui_view::prelude::*;

use crate::{GestureArenaScope, Listener};

/// A no-argument gesture callback (Flutter's `onTap` / `onLongPress` /
/// `onDoubleTap`) — fired with no details when the gesture is recognized.
type GestureCallback = Arc<dyn Fn() + Send + Sync>;
/// Pan callbacks carry the drag's details (position, delta, velocity).
type PanStartHandler = Arc<dyn Fn(DragStartDetails) + Send + Sync>;
type PanUpdateHandler = Arc<dyn Fn(DragUpdateDetails) + Send + Sync>;
type PanEndHandler = Arc<dyn Fn(DragEndDetails) + Send + Sync>;

/// Detects gestures on its child and invokes the matching callback.
///
/// Flutter parity: `widgets/gesture_detector.dart` `GestureDetector`. It owns a
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
///   window. Works on its own. **Combining `on_double_tap` with `on_tap` on the
///   same detector is not yet correct**: the tap recognizer resolves the arena on
///   the first pointer-up, so a genuine double-tap fires `on_tap` twice instead.
///   Holding the tap until the double-tap window closes needs the binding-driven
///   arena sweep (the gesture lifecycle phase, tracked next). Use `on_double_tap`
///   without `on_tap` until then.
/// - **pan/drag** (`on_pan_start` / `on_pan_update` / `on_pan_end`) — a contact
///   that moves past the drag slop, reported with running deltas and a release
///   velocity.
///
/// Only the recognizers whose callback is set participate in the arena for a
/// contact (Flutter parity: a recognizer is constructed only when its callback
/// is non-null). They compete in one arena: a quick down→up resolves to the tap
/// (the front member), a hold resolves to the long-press, a drag past slop hands
/// off to the pan recognizer — so at most one gesture fires per contact.
///
/// # Arena acquisition
///
/// In `init_state` the detector reads an ambient [`GestureArenaScope`] via
/// `ctx.get::<GestureArenaScope, _>(..)`:
/// - **Some(scope)** — build all recognizers against the shared, clock-bound
///   arena and poll its deadlines each frame on the binding's virtual clock, so a
///   long-press resolves deterministically. NOTE: the arena *close*-on-down and
///   *sweep*-on-up are currently supplied by the test harness, not the binding —
///   until the binding drives that lifecycle, cross-detector competition between
///   overlapping detectors is not yet sound (the gesture lifecycle phase, next).
/// - **None** (standalone) — build against a private [`GestureArena`] the
///   detector closes itself on down. Tap / secondary-tap / pan are fully
///   functional this way (byte-for-byte the historical behavior); the
///   deadline-driven gestures (long-press hold, double-tap give-up) are inert
///   without a binding to poll them — a documented limitation.
///
/// Hit behavior follows the wrapped `Listener` ([`DeferToChild`]): a gesture is
/// recognized only when the contact lands on a hit-testable descendant.
///
/// [`DeferToChild`]: flui_rendering::hit_testing::HitTestBehavior::DeferToChild
#[derive(Clone, Default, StatefulView)]
pub struct GestureDetector {
    on_tap: Option<GestureCallback>,
    on_secondary_tap: Option<GestureCallback>,
    on_long_press: Option<GestureCallback>,
    on_double_tap: Option<GestureCallback>,
    on_pan_start: Option<PanStartHandler>,
    on_pan_update: Option<PanUpdateHandler>,
    on_pan_end: Option<PanEndHandler>,
    child: Child,
}

impl std::fmt::Debug for GestureDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetector")
            .field("on_tap", &self.on_tap.is_some())
            .field("on_secondary_tap", &self.on_secondary_tap.is_some())
            .field("on_long_press", &self.on_long_press.is_some())
            .field("on_double_tap", &self.on_double_tap.is_some())
            .field("on_pan_start", &self.on_pan_start.is_some())
            .field("on_pan_update", &self.on_pan_update.is_some())
            .field("on_pan_end", &self.on_pan_end.is_some())
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
    pub fn on_tap(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    /// Called when the child receives a secondary-button tap (right-click down
    /// + up without moving past the touch slop).
    #[must_use]
    pub fn on_secondary_tap(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_secondary_tap = Some(Arc::new(callback));
        self
    }

    /// Called when the child is long-pressed (the contact held still past the
    /// long-press deadline).
    ///
    /// Deadline-driven: this fires only when a [`GestureArenaScope`] + binding
    /// above the detector polls the hold deadline each frame. A standalone
    /// detector (no scope) never recognizes a long press — see
    /// [arena acquisition](Self#arena-acquisition).
    #[must_use]
    pub fn on_long_press(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_long_press = Some(Arc::new(callback));
        self
    }

    /// Called when the child is double-tapped (two quick taps within the
    /// double-tap window). The success path is purely event-driven (the inter-tap
    /// timing reads the arena clock).
    ///
    /// **Do not combine with [`on_tap`](Self::on_tap) on the same detector yet:**
    /// the tap recognizer resolves the arena on the first up, so a genuine
    /// double-tap fires `on_tap` twice instead of `on_double_tap` once. Holding the
    /// tap until the double-tap window closes needs the binding-driven arena sweep
    /// (the next gesture lifecycle phase).
    #[must_use]
    pub fn on_double_tap(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_double_tap = Some(Arc::new(callback));
        self
    }

    /// Called once when a pan/drag begins (the contact crosses the drag slop).
    #[must_use]
    pub fn on_pan_start(
        mut self,
        callback: impl Fn(DragStartDetails) + Send + Sync + 'static,
    ) -> Self {
        self.on_pan_start = Some(Arc::new(callback));
        self
    }

    /// Called for each pointer move while a pan/drag is in progress, carrying
    /// the incremental delta since the previous update.
    #[must_use]
    pub fn on_pan_update(
        mut self,
        callback: impl Fn(DragUpdateDetails) + Send + Sync + 'static,
    ) -> Self {
        self.on_pan_update = Some(Arc::new(callback));
        self
    }

    /// Called once when the pan/drag ends (pointer up), carrying the release
    /// velocity.
    #[must_use]
    pub fn on_pan_end(mut self, callback: impl Fn(DragEndDetails) + Send + Sync + 'static) -> Self {
        self.on_pan_end = Some(Arc::new(callback));
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
/// `build` (Flutter's `didUpdateWidget`).
#[derive(Clone, Default)]
struct PanCallbacks {
    start: Option<PanStartHandler>,
    update: Option<PanUpdateHandler>,
    end: Option<PanEndHandler>,
}

/// The recognizers + the arena they share, built once in
/// [`GestureDetectorState::init_state`] against the ambient (or private) arena.
///
/// They are not built in `create_state` because that has no `BuildContext` and
/// so cannot read the ambient [`GestureArenaScope`]; a recognizer's arena is
/// captured at construction and is not swappable, so construction must wait for
/// the live context `init_state` receives.
struct Recognizers {
    /// The arena the recognizers compete in. Shared (from a `GestureArenaScope`)
    /// when `self_close` is `false`, else private to this detector.
    arena: GestureArena,
    /// `true` when this detector owns a private arena and must `close` it itself
    /// on down; `false` when a binding owns the shared arena and closes it after
    /// dispatching the down to the whole hit-test path.
    self_close: bool,
    /// Tap recognizer — added to the arena FIRST so it is the front member that
    /// wins an ambiguous quick tap on sweep.
    tap: Arc<TapGestureRecognizer>,
    /// Long-press recognizer — wins when its hold deadline fires (binding-polled).
    long_press: Arc<LongPressGestureRecognizer>,
    /// Double-tap recognizer — completes purely from the event stream; its
    /// give-up timer is binding-polled.
    double_tap: Arc<DoubleTapGestureRecognizer>,
    /// Pan/drag recognizer (free axis) — wins by attrition when a move past the
    /// slop makes the tap reject itself.
    drag: Arc<DragGestureRecognizer>,
}

/// Persistent gesture state: the recognizers + their shared arena survive
/// rebuilds (the pointer stream is stateful), and are disposed on unmount.
///
/// `create_state` allocates only the live callback slots; the recognizers are
/// built in `init_state` (which has the `BuildContext` needed to read the
/// ambient arena) and read — never rebuilt — by `build`.
pub struct GestureDetectorState {
    /// The live `on_tap`, refreshed each `build`. The recognizer reads THIS slot
    /// rather than a frozen capture, so a rebuild with a new closure is honored.
    tap_slot: Arc<Mutex<Option<GestureCallback>>>,
    /// The live `on_secondary_tap`, refreshed each `build`.
    secondary_tap_slot: Arc<Mutex<Option<GestureCallback>>>,
    /// The live `on_long_press`, refreshed each `build`.
    long_press_slot: Arc<Mutex<Option<GestureCallback>>>,
    /// The live `on_double_tap`, refreshed each `build`.
    double_tap_slot: Arc<Mutex<Option<GestureCallback>>>,
    /// The live pan callbacks, refreshed each `build`.
    pan_slot: Arc<Mutex<PanCallbacks>>,
    /// The recognizers + arena, built once in `init_state`. `None` only in the
    /// window between `create_state` and the first `init_state` — never observed
    /// by `build`, which always runs after `init_state`.
    recognizers: Option<Recognizers>,
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
            tap_slot: Arc::new(Mutex::new(self.on_tap.clone())),
            secondary_tap_slot: Arc::new(Mutex::new(self.on_secondary_tap.clone())),
            long_press_slot: Arc::new(Mutex::new(self.on_long_press.clone())),
            double_tap_slot: Arc::new(Mutex::new(self.on_double_tap.clone())),
            pan_slot: Arc::new(Mutex::new(PanCallbacks {
                start: self.on_pan_start.clone(),
                update: self.on_pan_update.clone(),
                end: self.on_pan_end.clone(),
            })),
            recognizers: None,
        }
    }
}

impl ViewState<GestureDetector> for GestureDetectorState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // Read the ambient arena WITHOUT registering a dependency: the handle
        // never changes, and `get` (unlike `depend_on`) is legal here — Flutter
        // forbids `dependOnInheritedWidgetOfExactType` in `initState`, and the
        // no-dependency lookup sidesteps that rule.
        let shared = ctx.get::<GestureArenaScope, _>(|scope| scope.arena().clone());
        let (arena, self_close) = match shared {
            Some(arena) => (arena, false),
            None => (GestureArena::new(), true),
        };

        // Each recognizer reads its live slot OUT before invoking it, so a slot
        // lock is never held across user code (no re-entrancy / poison hazard).
        let tap = {
            let primary_slot = Arc::clone(&self.tap_slot);
            let secondary_slot = Arc::clone(&self.secondary_tap_slot);
            TapGestureRecognizer::new(arena.clone())
                .with_on_tap(move |_details| {
                    if let Some(handler) = primary_slot.lock().ok().and_then(|guard| guard.clone())
                    {
                        handler();
                    }
                })
                .with_on_secondary_tap(move |_details| {
                    if let Some(handler) =
                        secondary_slot.lock().ok().and_then(|guard| guard.clone())
                    {
                        handler();
                    }
                })
        };

        let long_press = {
            let slot = Arc::clone(&self.long_press_slot);
            LongPressGestureRecognizer::new(arena.clone()).with_on_long_press(move || {
                if let Some(handler) = slot.lock().ok().and_then(|guard| guard.clone()) {
                    handler();
                }
            })
        };

        let double_tap = {
            let slot = Arc::clone(&self.double_tap_slot);
            DoubleTapGestureRecognizer::new(arena.clone()).with_on_double_tap(move |_details| {
                if let Some(handler) = slot.lock().ok().and_then(|guard| guard.clone()) {
                    handler();
                }
            })
        };

        let drag = {
            let start_slot = Arc::clone(&self.pan_slot);
            let update_slot = Arc::clone(&self.pan_slot);
            let end_slot = Arc::clone(&self.pan_slot);
            DragGestureRecognizer::new(arena.clone(), DragAxis::Free)
                .with_on_start(move |details| {
                    let callback = start_slot.lock().ok().and_then(|guard| guard.start.clone());
                    if let Some(callback) = callback {
                        callback(details);
                    }
                })
                .with_on_update(move |details| {
                    let callback = update_slot
                        .lock()
                        .ok()
                        .and_then(|guard| guard.update.clone());
                    if let Some(callback) = callback {
                        callback(details);
                    }
                })
                .with_on_end(move |details| {
                    let callback = end_slot.lock().ok().and_then(|guard| guard.end.clone());
                    if let Some(callback) = callback {
                        callback(details);
                    }
                })
        };

        self.recognizers = Some(Recognizers {
            arena,
            self_close,
            tap,
            long_press,
            double_tap,
            drag,
        });
    }

    fn build(&self, view: &GestureDetector, _ctx: &dyn BuildContext) -> impl IntoView {
        // Refresh the live callbacks the recognizers read, so a rebuild with new
        // closures is honored (the recognizers themselves persist).
        if let Ok(mut slot) = self.tap_slot.lock() {
            slot.clone_from(&view.on_tap);
        }
        if let Ok(mut slot) = self.secondary_tap_slot.lock() {
            slot.clone_from(&view.on_secondary_tap);
        }
        if let Ok(mut slot) = self.long_press_slot.lock() {
            slot.clone_from(&view.on_long_press);
        }
        if let Ok(mut slot) = self.double_tap_slot.lock() {
            slot.clone_from(&view.on_double_tap);
        }
        if let Ok(mut slot) = self.pan_slot.lock() {
            slot.start.clone_from(&view.on_pan_start);
            slot.update.clone_from(&view.on_pan_update);
            slot.end.clone_from(&view.on_pan_end);
        }

        // `init_state` runs exactly once before the first `build`, so the
        // recognizers are always present here.
        let recognizers = self
            .recognizers
            .as_ref()
            .expect("init_state builds the recognizers before the first build");

        let listener = self.make_listener(recognizers);

        match view.child.clone().into_inner() {
            Some(child) => listener.child(child),
            None => listener,
        }
    }

    fn dispose(&mut self) {
        if let Some(recognizers) = self.recognizers.as_ref() {
            recognizers.tap.dispose();
            recognizers.long_press.dispose();
            recognizers.double_tap.dispose();
            recognizers.drag.dispose();
        }
    }
}

impl GestureDetectorState {
    /// Build the [`Listener`] that drives the recognizers from the pointer
    /// stream.
    ///
    /// Each recognizer participates for a contact only when its configured
    /// callback is live (Flutter parity: `gesture_detector.dart` constructs a
    /// recognizer only when its callback is non-null). Participation is read from
    /// the live slots at event time (the `*_active` predicates), so a rebuild
    /// with a changed configuration is honored, and a double-tap-only detector
    /// does not let its tap recognizer steal the first up.
    fn make_listener(&self, recognizers: &Recognizers) -> Listener {
        let group = RecognizerGroup {
            arena: recognizers.arena.clone(),
            self_close: recognizers.self_close,
            tap: Arc::clone(&recognizers.tap),
            long_press: Arc::clone(&recognizers.long_press),
            double_tap: Arc::clone(&recognizers.double_tap),
            drag: Arc::clone(&recognizers.drag),
            tap_slot: Arc::clone(&self.tap_slot),
            secondary_tap_slot: Arc::clone(&self.secondary_tap_slot),
            long_press_slot: Arc::clone(&self.long_press_slot),
            double_tap_slot: Arc::clone(&self.double_tap_slot),
            pan_slot: Arc::clone(&self.pan_slot),
        };

        let down = group.clone();
        let on_move = group.clone();
        let on_up = group.clone();
        let on_cancel = group;

        Listener::new()
            .on_pointer_down(move |event| down.handle_down(event))
            .on_pointer_move(move |event| on_move.forward(event))
            .on_pointer_up(move |event| on_up.forward(event))
            .on_pointer_cancel(move |event| on_cancel.forward(event))
    }
}

/// The recognizers + the live slots that gate their participation, captured by
/// the [`Listener`] callbacks. One shared bundle, cloned once per callback.
#[derive(Clone)]
struct RecognizerGroup {
    arena: GestureArena,
    self_close: bool,
    tap: Arc<TapGestureRecognizer>,
    long_press: Arc<LongPressGestureRecognizer>,
    double_tap: Arc<DoubleTapGestureRecognizer>,
    drag: Arc<DragGestureRecognizer>,
    tap_slot: Arc<Mutex<Option<GestureCallback>>>,
    secondary_tap_slot: Arc<Mutex<Option<GestureCallback>>>,
    long_press_slot: Arc<Mutex<Option<GestureCallback>>>,
    double_tap_slot: Arc<Mutex<Option<GestureCallback>>>,
    pan_slot: Arc<Mutex<PanCallbacks>>,
}

impl RecognizerGroup {
    /// The tap recognizer participates iff a primary- OR secondary-tap callback
    /// is currently set.
    fn tap_active(&self) -> bool {
        slot_is_some(&self.tap_slot) || slot_is_some(&self.secondary_tap_slot)
    }

    /// The long-press recognizer participates iff `on_long_press` is set.
    fn long_press_active(&self) -> bool {
        slot_is_some(&self.long_press_slot)
    }

    /// The double-tap recognizer participates iff `on_double_tap` is set.
    fn double_tap_active(&self) -> bool {
        slot_is_some(&self.double_tap_slot)
    }

    /// The drag recognizer participates iff any pan callback is set.
    fn drag_active(&self) -> bool {
        self.pan_slot
            .lock()
            .is_ok_and(|pan| pan.start.is_some() || pan.update.is_some() || pan.end.is_some())
    }

    /// Register every participating recognizer for this contact (tap first so it
    /// is the arena's front member), then — in standalone mode only — close the
    /// arena. In shared mode the binding closes the arena after the down has been
    /// dispatched to the entire hit-test path, so overlapping detectors all add
    /// their recognizers before the single close.
    fn handle_down(&self, event: &PointerEvent) {
        let pointer = event.pointer_id();
        let position = event.position();
        if self.tap_active() {
            self.tap.add_pointer(pointer, position);
            // Forward the real Down so the recognizer refines the provisional
            // Primary button `add_pointer` staged to the actual button
            // (Primary / Secondary / Tertiary).
            self.tap.handle_event(event);
        }
        if self.long_press_active() {
            self.long_press.add_pointer(pointer, position);
        }
        if self.double_tap_active() {
            self.double_tap.add_pointer(pointer, position);
        }
        if self.drag_active() {
            self.drag.add_pointer(pointer, position);
        }
        if self.self_close {
            self.arena.close(pointer);
        }
    }

    /// Forward a move / up / cancel event to every participating recognizer.
    fn forward(&self, event: &PointerEvent) {
        if self.tap_active() {
            self.tap.handle_event(event);
        }
        if self.long_press_active() {
            self.long_press.handle_event(event);
        }
        if self.double_tap_active() {
            self.double_tap.handle_event(event);
        }
        if self.drag_active() {
            self.drag.handle_event(event);
        }
    }
}

/// `true` when the no-argument callback slot currently holds a handler.
fn slot_is_some(slot: &Arc<Mutex<Option<GestureCallback>>>) -> bool {
    slot.lock().is_ok_and(|guard| guard.is_some())
}
