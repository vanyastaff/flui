//! [`GestureDetector`] — recognizes high-level gestures (currently tap) from the
//! raw pointer stream a [`Listener`] delivers.

use std::sync::{Arc, Mutex};

use flui_interaction::arena::GestureArena;
use flui_interaction::{GestureRecognizer, PointerEventExt, TapGestureRecognizer};
use flui_view::prelude::*;

use crate::Listener;

/// A no-argument tap callback (Flutter's `onTap`).
type TapHandler = Arc<dyn Fn() + Send + Sync>;

/// Detects gestures on its child and invokes the matching callback.
///
/// Flutter parity: `widgets/gesture_detector.dart` `GestureDetector`. It owns a
/// gesture recognizer + a per-detector [`GestureArena`], wraps its child in a
/// [`Listener`], and feeds the pointer stream to the recognizer; the recognizer
/// resolves the gesture through the arena and fires the callback. Currently only
/// `on_tap` is wired (the recognizer catalog in `flui-interaction` also has
/// double-tap, long-press, drag, scale — future knobs).
///
/// Hit behavior follows the wrapped `Listener` ([`DeferToChild`]): a tap is
/// recognized only when it lands on a hit-testable descendant.
///
/// [`DeferToChild`]: flui_rendering::hit_testing::HitTestBehavior::DeferToChild
#[derive(Clone, Default, StatefulView)]
pub struct GestureDetector {
    on_tap: Option<TapHandler>,
    child: Child,
}

impl std::fmt::Debug for GestureDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetector")
            .field("on_tap", &self.on_tap.is_some())
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

    /// Set the child the gestures are detected on.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

/// Persistent gesture state: the recognizer + its arena survive rebuilds (the
/// pointer stream is stateful), and are disposed on unmount.
pub struct GestureDetectorState {
    /// Per-detector arena. The detector drives `close` (on down) itself, so it
    /// resolves without depending on a global `GestureBinding` — which makes it
    /// work in any embedding, at the cost of NOT competing with overlapping
    /// detectors (that needs the binding's shared arena; a future enhancement).
    arena: GestureArena,
    recognizer: Arc<TapGestureRecognizer>,
    /// The current `on_tap`, refreshed from the view on every `build` (Flutter's
    /// `didUpdateWidget`). The recognizer's callback reads THIS slot rather than
    /// a frozen capture, so a rebuild with a new closure is honored.
    on_tap: Arc<Mutex<Option<TapHandler>>>,
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
        let on_tap = Arc::new(Mutex::new(self.on_tap.clone()));
        let slot = Arc::clone(&on_tap);
        let recognizer = TapGestureRecognizer::new(arena.clone()).with_on_tap(move |_details| {
            // Clone the live handler OUT before invoking it, so the slot lock is
            // never held across user code (no re-entrancy / poison hazard).
            let handler = slot.lock().ok().and_then(|guard| guard.clone());
            if let Some(handler) = handler {
                handler();
            }
        });
        GestureDetectorState {
            arena,
            recognizer,
            on_tap,
        }
    }
}

impl ViewState<GestureDetector> for GestureDetectorState {
    fn build(&self, view: &GestureDetector, _ctx: &dyn BuildContext) -> impl IntoView {
        // Refresh the live callback the recognizer reads, so a rebuild with a
        // new `on_tap` closure is honored (the recognizer itself persists).
        if let Ok(mut slot) = self.on_tap.lock() {
            slot.clone_from(&view.on_tap);
        }

        let arena = self.arena.clone();
        let recognizer_down = Arc::clone(&self.recognizer);
        let recognizer_move = Arc::clone(&self.recognizer);
        let recognizer_up = Arc::clone(&self.recognizer);
        let recognizer_cancel = Arc::clone(&self.recognizer);

        // INVARIANTS this single-tap detector relies on (it has ONE recognizer
        // and is single-pointer, matching `TapGestureRecognizer`):
        // - `close` here follows the SOLE `add_pointer`; if this detector ever
        //   gains a second recognizer added on the same down, close must move
        //   AFTER all adds, else the already-resolved arena drops the latecomer.
        // - one down → one up per pointer id; a repeated down (resetting the
        //   recognizer) or a mismatched-id up would drop the tap. The production
        //   `GestureBinding` guarantees this ordering; a headless driver must too.
        let listener = Listener::new()
            .on_pointer_down(move |event| {
                // Start tracking this pointer, then close the (single-recognizer)
                // arena so a later up can resolve it — the role the global
                // GestureBinding plays in production.
                let pointer = event.pointer_id();
                recognizer_down.add_pointer(pointer, event.position());
                arena.close(pointer);
            })
            .on_pointer_move(move |event| recognizer_move.handle_event(event))
            .on_pointer_up(move |event| recognizer_up.handle_event(event))
            // Forward cancel so an interrupted contact rejects the in-flight
            // gesture and sweeps its arena entry (otherwise it would leak and
            // wedge a later same-id pointer).
            .on_pointer_cancel(move |event| recognizer_cancel.handle_event(event));

        match view.child.clone().into_inner() {
            Some(child) => listener.child(child),
            None => listener,
        }
    }

    fn dispose(&mut self) {
        self.recognizer.dispose();
    }
}
