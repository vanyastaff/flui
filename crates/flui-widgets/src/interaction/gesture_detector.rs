//! [`GestureDetector`] — recognizes high-level gestures (tap and pan/drag) from
//! the raw pointer stream a [`Listener`] delivers.

use std::sync::{Arc, Mutex};

use flui_interaction::arena::GestureArena;
use flui_interaction::{
    DragAxis, DragEndDetails, DragGestureRecognizer, DragStartDetails, DragUpdateDetails,
    GestureRecognizer, PointerEventExt, TapGestureRecognizer,
};
use flui_view::prelude::*;

use crate::Listener;

/// A no-argument tap callback (Flutter's `onTap`).
type TapHandler = Arc<dyn Fn() + Send + Sync>;
/// Pan callbacks carry the drag's details (position, delta, velocity).
type PanStartHandler = Arc<dyn Fn(DragStartDetails) + Send + Sync>;
type PanUpdateHandler = Arc<dyn Fn(DragUpdateDetails) + Send + Sync>;
type PanEndHandler = Arc<dyn Fn(DragEndDetails) + Send + Sync>;

/// Detects gestures on its child and invokes the matching callback.
///
/// Flutter parity: `widgets/gesture_detector.dart` `GestureDetector`. It owns a
/// set of gesture recognizers + a per-detector [`GestureArena`], wraps its child
/// in a [`Listener`], and feeds the pointer stream to every recognizer; the
/// arena resolves the competition and the winning recognizer fires its callback.
///
/// Two gesture families are wired:
/// - **tap** (`on_tap`) — a primary-button down + up without moving past the
///   touch slop.
/// - **secondary tap** (`on_secondary_tap`) — a secondary-button (right-click)
///   down + up without moving past the touch slop.
/// - **pan/drag** (`on_pan_start` / `on_pan_update` / `on_pan_end`) — a contact
///   that moves past the drag slop, reported with running deltas and a release
///   velocity.
///
/// tap and pan compete in the same arena: a quick down→up resolves to the tap
/// (it is the arena's front member), while a contact that drags past the slop
/// cancels the tap and hands the gesture to the drag recognizer — so at most one
/// of `on_tap` / `on_pan_*` fires per contact.
///
/// Hit behavior follows the wrapped `Listener` ([`DeferToChild`]): a gesture is
/// recognized only when the contact lands on a hit-testable descendant.
///
/// [`DeferToChild`]: flui_rendering::hit_testing::HitTestBehavior::DeferToChild
#[derive(Clone, Default, StatefulView)]
pub struct GestureDetector {
    on_tap: Option<TapHandler>,
    on_secondary_tap: Option<TapHandler>,
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

/// Persistent gesture state: the recognizers + their shared arena survive
/// rebuilds (the pointer stream is stateful), and are disposed on unmount.
pub struct GestureDetectorState {
    /// Per-detector arena. The detector drives `close` (on down) itself, so it
    /// resolves without depending on a global `GestureBinding` — which makes it
    /// work in any embedding, at the cost of NOT competing with overlapping
    /// detectors (that needs the binding's shared arena; a future enhancement).
    arena: GestureArena,
    /// Tap recognizer — added to the arena FIRST so it is the front member that
    /// wins an ambiguous quick tap when the arena is swept.
    tap: Arc<TapGestureRecognizer>,
    /// Pan/drag recognizer (free axis) — wins by attrition when a move past the
    /// slop makes the tap reject itself.
    drag: Arc<DragGestureRecognizer>,
    /// The live `on_tap`, refreshed each `build`. The recognizer reads THIS slot
    /// rather than a frozen capture, so a rebuild with a new closure is honored.
    tap_slot: Arc<Mutex<Option<TapHandler>>>,
    /// The live `on_secondary_tap`, refreshed each `build` (same rationale as
    /// `tap_slot`).
    secondary_tap_slot: Arc<Mutex<Option<TapHandler>>>,
    /// The live pan callbacks, refreshed each `build` (same rationale).
    pan_slot: Arc<Mutex<PanCallbacks>>,
}

impl std::fmt::Debug for GestureDetectorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetectorState")
            .finish_non_exhaustive()
    }
}

impl StatefulView for GestureDetector {
    type State = GestureDetectorState;

    fn create_state(&self) -> Self::State {
        let arena = GestureArena::new();

        let tap_slot = Arc::new(Mutex::new(self.on_tap.clone()));
        let secondary_tap_slot = Arc::new(Mutex::new(self.on_secondary_tap.clone()));
        let tap = {
            let primary_slot = Arc::clone(&tap_slot);
            let secondary_slot = Arc::clone(&secondary_tap_slot);
            TapGestureRecognizer::new(arena.clone())
                .with_on_tap(move |_details| {
                    // Clone the live handler OUT before invoking it, so the slot lock
                    // is never held across user code (no re-entrancy / poison hazard).
                    let handler = primary_slot.lock().ok().and_then(|guard| guard.clone());
                    if let Some(handler) = handler {
                        handler();
                    }
                })
                .with_on_secondary_tap(move |_details| {
                    let handler = secondary_slot.lock().ok().and_then(|guard| guard.clone());
                    if let Some(handler) = handler {
                        handler();
                    }
                })
        };

        let pan_slot = Arc::new(Mutex::new(PanCallbacks {
            start: self.on_pan_start.clone(),
            update: self.on_pan_update.clone(),
            end: self.on_pan_end.clone(),
        }));
        let drag = {
            let start_slot = Arc::clone(&pan_slot);
            let update_slot = Arc::clone(&pan_slot);
            let end_slot = Arc::clone(&pan_slot);
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

        GestureDetectorState {
            arena,
            tap,
            drag,
            tap_slot,
            secondary_tap_slot,
            pan_slot,
        }
    }
}

impl ViewState<GestureDetector> for GestureDetectorState {
    fn build(&self, view: &GestureDetector, _ctx: &dyn BuildContext) -> impl IntoView {
        // Refresh the live callbacks the recognizers read, so a rebuild with new
        // closures is honored (the recognizers themselves persist).
        if let Ok(mut slot) = self.tap_slot.lock() {
            slot.clone_from(&view.on_tap);
        }
        if let Ok(mut slot) = self.secondary_tap_slot.lock() {
            slot.clone_from(&view.on_secondary_tap);
        }
        if let Ok(mut slot) = self.pan_slot.lock() {
            slot.start.clone_from(&view.on_pan_start);
            slot.update.clone_from(&view.on_pan_update);
            slot.end.clone_from(&view.on_pan_end);
        }

        let arena = self.arena.clone();
        let tap_down = Arc::clone(&self.tap);
        let drag_down = Arc::clone(&self.drag);
        let tap_move = Arc::clone(&self.tap);
        let drag_move = Arc::clone(&self.drag);
        let tap_up = Arc::clone(&self.tap);
        let drag_up = Arc::clone(&self.drag);
        let tap_cancel = Arc::clone(&self.tap);
        let drag_cancel = Arc::clone(&self.drag);

        let listener = Listener::new()
            .on_pointer_down(move |event| {
                let pointer = event.pointer_id();
                let position = event.position();
                // Register every recognizer for this contact, THEN close the
                // arena — close must follow ALL adds, or the already-resolved
                // arena would drop the latecomer. Tap is added first so it is the
                // arena's front member (it wins a swept, otherwise-unclaimed quick
                // tap). The drag recognizer wins only by attrition: a move past
                // the slop makes the tap reject itself, leaving drag the sole
                // member. This is the role a global GestureBinding plays in
                // production.
                tap_down.add_pointer(pointer, position);
                // Forward the real Down event so the recognizer refines the
                // provisional TapButton::Primary set by add_pointer to the actual
                // button (Primary / Secondary / Tertiary). Without this, a
                // secondary-button down is recorded as Primary, and the
                // button-mismatch guard in handle_tap_up cancels the gesture.
                tap_down.handle_event(event);
                drag_down.add_pointer(pointer, position);
                arena.close(pointer);
            })
            .on_pointer_move(move |event| {
                tap_move.handle_event(event);
                drag_move.handle_event(event);
            })
            .on_pointer_up(move |event| {
                tap_up.handle_event(event);
                drag_up.handle_event(event);
            })
            // Forward cancel so an interrupted contact rejects every in-flight
            // gesture and sweeps the arena entry (otherwise it would leak and
            // wedge a later same-id pointer).
            .on_pointer_cancel(move |event| {
                tap_cancel.handle_event(event);
                drag_cancel.handle_event(event);
            });

        match view.child.clone().into_inner() {
            Some(child) => listener.child(child),
            None => listener,
        }
    }

    fn dispose(&mut self) {
        self.tap.dispose();
        self.drag.dispose();
    }
}
