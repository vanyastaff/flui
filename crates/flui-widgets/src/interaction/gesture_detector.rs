//! [`GestureDetector`] — recognizes high-level gestures (tap, long-press,
//! double-tap, and pan/drag) from the raw pointer stream a [`Listener`] delivers.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use flui_interaction::arena::{GestureArena, SweepModel};
use flui_interaction::{
    DoubleTapGestureRecognizer, DragAxis, DragEndDetails, DragGestureRecognizer, DragStartDetails,
    DragUpdateDetails, GestureRecognizer, LongPressGestureRecognizer, PointerEvent,
    PointerEventExt, TapGestureRecognizer,
};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;

use crate::{GestureArenaScope, Listener};

/// A no-argument gesture callback (Flutter's `onTap` / `onLongPress` /
/// `onDoubleTap`) — fired with no details when the gesture is recognized.
type GestureCallback = Rc<dyn Fn()>;
/// Pan callbacks carry the drag's details (position, delta, velocity).
type PanStartHandler = Rc<dyn Fn(DragStartDetails)>;
type PanUpdateHandler = Rc<dyn Fn(DragUpdateDetails)>;
type PanEndHandler = Rc<dyn Fn(DragEndDetails)>;

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
///   window. Combines correctly with `on_tap` under a [`GestureArenaScope`] +
///   binding: the double-tap recognizer holds the arena across the inter-tap
///   window, so the binding's first-up sweep is deferred and the front-member
///   tap cannot win early — two quick taps fire `on_double_tap` once (not `on_tap`
///   twice), and a lone tap is held until the window closes, then fires `on_tap`
///   once. Standalone (no scope), the inter-tap give-up is not polled, so prefer
///   `on_double_tap` under a scope.
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
///   arena (which answers [`SweepModel::BindingDriven`]) and poll its deadlines
///   each frame on the binding's virtual clock, so a long-press resolves
///   deterministically. The binding drives the *close*-on-down / *sweep*-on-up
///   lifecycle after routing each event to the whole hit-test path, so
///   overlapping detectors genuinely compete in the one arena (the recognizers
///   below never self-sweep), and `on_tap` + `on_double_tap` combine correctly.
/// - **None** (standalone) — build against a private [`GestureArena`] the
///   detector closes itself on down. Tap / secondary-tap / pan are fully
///   functional this way (byte-for-byte the historical behavior); the
///   deadline-driven gestures (long-press hold, double-tap give-up) are inert
///   without a binding to poll them — a documented limitation.
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
    on_pan_start: Option<PanStartHandler>,
    on_pan_update: Option<PanUpdateHandler>,
    on_pan_end: Option<PanEndHandler>,
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
            on_pan_start: None,
            on_pan_update: None,
            on_pan_end: None,
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
            .field("on_pan_start", &self.on_pan_start.is_some())
            .field("on_pan_update", &self.on_pan_update.is_some())
            .field("on_pan_end", &self.on_pan_end.is_some())
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
    pub fn on_tap(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_tap = Some(Rc::new(callback));
        self
    }

    /// Called when the child receives a secondary-button tap (right-click down
    /// + up without moving past the touch slop).
    #[must_use]
    pub fn on_secondary_tap(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_secondary_tap = Some(Rc::new(callback));
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
    pub fn on_long_press(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_long_press = Some(Rc::new(callback));
        self
    }

    /// Called when the child is double-tapped (two quick taps within the
    /// double-tap window). The inter-tap timing reads the arena clock.
    ///
    /// Combines correctly with [`on_tap`](Self::on_tap) under a
    /// [`GestureArenaScope`] + binding: the double-tap recognizer holds the arena
    /// across the inter-tap window, so the binding's first-up sweep is deferred
    /// and the tap cannot win early. Two quick taps fire `on_double_tap` once
    /// (never `on_tap` twice); a lone tap is held until the window closes, then
    /// fires `on_tap` once. The give-up is binding-polled, so combine the two
    /// under a scope.
    #[must_use]
    pub fn on_double_tap(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_double_tap = Some(Rc::new(callback));
        self
    }

    /// Called once when a pan/drag begins (the contact crosses the drag slop).
    #[must_use]
    pub fn on_pan_start(mut self, callback: impl Fn(DragStartDetails) + 'static) -> Self {
        self.on_pan_start = Some(Rc::new(callback));
        self
    }

    /// Called for each pointer move while a pan/drag is in progress, carrying
    /// the incremental delta since the previous update.
    #[must_use]
    pub fn on_pan_update(mut self, callback: impl Fn(DragUpdateDetails) + 'static) -> Self {
        self.on_pan_update = Some(Rc::new(callback));
        self
    }

    /// Called once when the pan/drag ends (pointer up), carrying the release
    /// velocity.
    #[must_use]
    pub fn on_pan_end(mut self, callback: impl Fn(DragEndDetails) + 'static) -> Self {
        self.on_pan_end = Some(Rc::new(callback));
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
    tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    /// The live `on_secondary_tap`, refreshed each `build`.
    secondary_tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    /// The live `on_long_press`, refreshed each `build`.
    long_press_slot: Rc<RefCell<Option<GestureCallback>>>,
    /// The live `on_double_tap`, refreshed each `build`.
    double_tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    /// The live pan callbacks, refreshed each `build`.
    pan_slot: Rc<RefCell<PanCallbacks>>,
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
            tap_slot: Rc::new(RefCell::new(self.on_tap.clone())),
            secondary_tap_slot: Rc::new(RefCell::new(self.on_secondary_tap.clone())),
            long_press_slot: Rc::new(RefCell::new(self.on_long_press.clone())),
            double_tap_slot: Rc::new(RefCell::new(self.on_double_tap.clone())),
            pan_slot: Rc::new(RefCell::new(PanCallbacks {
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
        let arena = ctx
            .get::<GestureArenaScope, _>(|scope| scope.arena().clone())
            .unwrap_or_else(GestureArena::new);
        // The arena's sweep model decides who owns close-on-down / sweep-on-up:
        // a binding-owned (shared) arena answers `BindingDriven` and runs the
        // lifecycle itself; a private `GestureArena::new()` answers `SelfDriven`,
        // so this detector closes it. Deriving the flag from the arena keeps the
        // two halves symmetric and subsumes a separate bool.
        let self_close = arena.sweep_model() == SweepModel::SelfDriven;

        // Each recognizer reads its live slot OUT before invoking it, so a slot
        // lock is never held across user code (no re-entrancy / poison hazard).
        let tap = {
            let primary_slot = Rc::clone(&self.tap_slot);
            let secondary_slot = Rc::clone(&self.secondary_tap_slot);
            TapGestureRecognizer::new(arena.clone())
                .with_on_tap(move |_details| {
                    if let Some(handler) = primary_slot.borrow().clone() {
                        handler();
                    }
                })
                .with_on_secondary_tap(move |_details| {
                    if let Some(handler) = secondary_slot.borrow().clone() {
                        handler();
                    }
                })
        };

        let long_press = {
            let slot = Rc::clone(&self.long_press_slot);
            LongPressGestureRecognizer::new(arena.clone()).with_on_long_press(move || {
                if let Some(handler) = slot.borrow().clone() {
                    handler();
                }
            })
        };

        let double_tap = {
            let slot = Rc::clone(&self.double_tap_slot);
            DoubleTapGestureRecognizer::new(arena.clone()).with_on_double_tap(move |_details| {
                if let Some(handler) = slot.borrow().clone() {
                    handler();
                }
            })
        };

        let drag = {
            let start_slot = Rc::clone(&self.pan_slot);
            let update_slot = Rc::clone(&self.pan_slot);
            let end_slot = Rc::clone(&self.pan_slot);
            DragGestureRecognizer::new(arena.clone(), DragAxis::Free)
                .with_on_start(move |details| {
                    let callback = start_slot.borrow().start.clone();
                    if let Some(callback) = callback {
                        callback(details);
                    }
                })
                .with_on_update(move |details| {
                    let callback = update_slot.borrow().update.clone();
                    if let Some(callback) = callback {
                        callback(details);
                    }
                })
                .with_on_end(move |details| {
                    let callback = end_slot.borrow().end.clone();
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
        {
            let mut slot = self.pan_slot.borrow_mut();
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

        let listener = self.make_listener(recognizers).behavior(view.behavior);

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
            tap_slot: Rc::clone(&self.tap_slot),
            secondary_tap_slot: Rc::clone(&self.secondary_tap_slot),
            long_press_slot: Rc::clone(&self.long_press_slot),
            double_tap_slot: Rc::clone(&self.double_tap_slot),
            pan_slot: Rc::clone(&self.pan_slot),
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
    tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    secondary_tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    long_press_slot: Rc<RefCell<Option<GestureCallback>>>,
    double_tap_slot: Rc<RefCell<Option<GestureCallback>>>,
    pan_slot: Rc<RefCell<PanCallbacks>>,
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
        let pan = self.pan_slot.borrow();
        pan.start.is_some() || pan.update.is_some() || pan.end.is_some()
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
fn slot_is_some(slot: &Rc<RefCell<Option<GestureCallback>>>) -> bool {
    slot.borrow().is_some()
}
