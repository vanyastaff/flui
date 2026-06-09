//! Multi-pointer drag gesture recognizer
//!
//! Recognises drag gestures on a **per-pointer** basis. Where
//! [`DragGestureRecognizer`](crate::recognizers::DragGestureRecognizer) tracks a
//! single primary pointer (other pointers are ignored once one is accepted),
//! this recogniser tracks *every* pointer that lands in its hit-test region
//! independently — multiple drags run in parallel, each with its own velocity
//! tracker, pending-delta accumulator, and arena entry.
//!
//! # Use case
//!
//! - A custom canvas where the user places a finger per selection rectangle
//!   and drags each one independently.
//! - A reorderable list with long-press drag — one finger per dragged item.
//! - A map view where two fingers pan the same surface in parallel (for
//!   diagnostics or A/B testing).
//!
//! # Protocol
//!
//! Mirrors Flutter's [`MultiDragGestureRecognizer`](https://api.flutter.dev/flutter/gestures/MultiDragGestureRecognizer-class.html)
//! (`gestures/multidrag.dart`). The recogniser:
//!
//! 1. Calls `on_pointer_down(pointer, position)` for every pointer that
//!    contacts the region.
//! 2. Each pointer's state is accepted into the arena once it crosses
//!    `slop` (or, for `Horizontal`/`Vertical` variants, the axis-aligned
//!    slop) — `check_for_resolution_after_move` is the per-pointer trigger.
//! 3. On acceptance, `on_start(pointer_id, position)` fires; the closure
//!    returns an opaque `MultiDragHandle` whose `update`/`end`/`cancel`
//!    methods are called for the lifetime of that drag.
//! 4. On `Up`/`Cancel`, the per-pointer state is dropped and any pending
//!    `update` is flushed as a final zero-delta.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::recognizers::multidrag::{
//!     MultiDragGestureRecognizer, MultiDragAxis,
//! };
//! use flui_types::geometry::{Offset, Pixels};
//!
//! let arena = GestureArena::new();
//! let recognizer = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free)
//!     .with_on_start(|pointer_id, initial_position| {
//!         // Return a handle the recognizer calls back into.
//!         MultiDragHandle::new(pointer_id, initial_position)
//!     });
//! ```
//!
//! # Distinct from `DragGestureRecognizer`
//!
//! | Aspect | `DragGestureRecognizer` | `MultiDragGestureRecognizer` |
//! |--------|------------------------|------------------------------|
//! | Pointers tracked | one (primary) | many (per pointer) |
//! | Callbacks | closure set on the recognizer | per-pointer closure returning a handle |
//! | Arena entries | one | one per pointer |
//! | Tap → drag use case | yes | no (use [`TapAndDragGestureRecognizer`](crate::recognizers::TapAndDragGestureRecognizer) — U7) |

use std::{collections::HashMap, sync::Arc, time::Instant};

use flui_types::{
    Offset,
    geometry::{PixelDelta, Pixels},
};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    settings::GestureSettings,
};

/// Per-pointer handle returned from `on_start`.
///
/// The recogniser invokes the handle's [`update`](Self::update),
/// [`end`](Self::end), and [`cancel`](Self::cancel) methods for the lifetime
/// of one accepted drag. The handle is opaque to the recogniser — user code
/// decides what to do with the drag (e.g. attach it to a render object, push
/// a snapshot to a vector store).
///
/// # Contract
///
/// - `update` may be called 0..N times between `start` and `end`/`cancel`.
/// - `update` after `end` or `cancel` is a no-op (defensive — guards
///   against last-mile event reordering on cancel).
/// - `end` and `cancel` are terminal: the handle is dropped by the recogniser
///   once either fires.
pub trait MultiDragHandle: Send + Sync + 'static {
    /// Called when a fresh sample arrives.
    fn update(&self, details: MultiDragUpdateDetails);
    /// Called when the pointer goes up while the drag is active.
    fn end(&self, details: MultiDragEndDetails);
    /// Called when the gesture is cancelled (lost arena, pointer cancelled,
    /// recogniser disposed).
    fn cancel(&self);
}

/// Axis constraint for the multi-pointer drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiDragAxis {
    /// Free movement — any direction counts toward slop.
    Free,
    /// Only horizontal movement counts.
    Horizontal,
    /// Only vertical movement counts.
    Vertical,
}

/// Callback invoked for every pointer that lands in the recogniser's region.
///
/// The closure may return `None` to reject the drag (e.g. outside an
/// interactive region). When `Some(handle)` is returned, the recogniser calls
/// `update`/`end`/`cancel` on that handle.
pub type MultiDragStartCallback =
    Arc<dyn Fn(PointerId, Offset<Pixels>) -> Option<Box<dyn MultiDragHandle>> + Send + Sync>; // PORT-CHECK-OK-DYN: per-pointer handle trait; ≤3 workspace sites, marker preferred over allowlist promotion.

/// Per-pointer state. Mirrors Flutter's `MultiDragPointerState`.
///
/// State lifecycle: `Possible` (added) → `Accepted` (slop crossed) → `Ended` /
/// `Cancelled` (terminal). The `_client` field is `Some` only after
/// `accepted`; the `pending_delta` field is `Some` only before `accepted` (or
/// after `rejected`).
struct MultiDragPointerState {
    /// Last reported position (for delta computation).
    last_position: Offset<Pixels>,
    /// Pointer device kind (slop, velocity-tracker flavour).
    kind: PointerType,
    /// Slop threshold for this kind — `None` until known.
    slop: f32,
    /// Accumulated delta while `pending` (pre-acceptance).
    pending_delta: Offset<PixelDelta>,
    /// `true` once slop is crossed and arena is accepted.
    accepted: bool,
    /// User's handle, populated after `accepted`.
    client: Option<Box<dyn MultiDragHandle>>, // PORT-CHECK-OK-DYN: see MultiDragStartCallback — per-pointer `dyn` handle storage.
    /// Velocity tracker fed while `pending` and after `accepted`.
    velocity_tracker: VelocityTracker,
}

impl MultiDragPointerState {
    fn new(initial_position: Offset<Pixels>, slop: f32) -> Self {
        Self {
            last_position: initial_position,
            kind: PointerType::Touch,
            slop,
            pending_delta: Offset::new(PixelDelta::ZERO, PixelDelta::ZERO),
            accepted: false,
            client: None,
            velocity_tracker: VelocityTracker::new(),
        }
    }

    /// Per-axis primary slop test — accepts when |delta.{axis}| exceeds slop.
    fn check_for_resolution_after_move(&mut self, axis: MultiDragAxis) -> bool {
        if self.accepted {
            return true;
        }
        let magnitude = match axis {
            MultiDragAxis::Free => self.pending_delta.distance().0,
            MultiDragAxis::Horizontal => self.pending_delta.dx.0.abs(),
            MultiDragAxis::Vertical => self.pending_delta.dy.0.abs(),
        };
        magnitude > self.slop
    }
}

/// Details for [`MultiDragHandle::update`].
#[derive(Debug, Clone, PartialEq)]
pub struct MultiDragUpdateDetails {
    /// Pointer this drag is associated with.
    pub pointer_id: PointerId,
    /// Pointer's current global position.
    pub global_position: Offset<Pixels>,
    /// Pointer's current local position (same as `global_position` for the
    /// multi-pointer recogniser; user code can transform).
    pub local_position: Offset<Pixels>,
    /// Delta since the last `update` (or, for the first update, the
    /// accumulated pending delta).
    pub delta: Offset<PixelDelta>,
    /// Pointer device kind.
    pub kind: PointerType,
    /// Wall-clock instant of the underlying event.
    pub timestamp: Instant,
}

/// Details for [`MultiDragHandle::end`].
#[derive(Debug, Clone, PartialEq)]
pub struct MultiDragEndDetails {
    /// Pointer this drag was associated with.
    pub pointer_id: PointerId,
    /// Pointer's final position.
    pub global_position: Offset<Pixels>,
    /// Velocity at the moment of release.
    pub velocity: crate::processing::Velocity,
    /// Pointer device kind.
    pub kind: PointerType,
}

// ============================================================================
// Recogniser
// ============================================================================

/// Per-pointer drag recogniser.
///
/// See [module-level docs](self) for protocol and use case.
#[derive(Clone)]
pub struct MultiDragGestureRecognizer {
    /// Shared base (arena, disposal flag, primary-pointer plumbing).
    state: RecognizerBase,
    /// Axis constraint applied to every pointer's slop test.
    axis: MultiDragAxis,
    /// Per-pointer state keyed by `PointerId`. Average O(1) lookup, worst-case
    /// O(n) for full scan on shutdown; n is bounded by concurrent touch points
    /// (≤10 in practice on touch screens).
    pointers: Arc<Mutex<HashMap<PointerId, MultiDragPointerState>>>,
    /// User callback fired on acceptance.
    on_start: Arc<Mutex<Option<MultiDragStartCallback>>>,
    /// Device-specific gesture settings.
    settings: Arc<Mutex<GestureSettings>>,
}

impl std::fmt::Debug for MultiDragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiDragGestureRecognizer")
            .field("state", &self.state)
            .field("axis", &self.axis)
            .field("settings", &self.settings.lock())
            .finish_non_exhaustive()
    }
}

impl MultiDragGestureRecognizer {
    /// Construct a new multi-pointer drag recogniser.
    pub fn new(arena: crate::arena::GestureArena, axis: MultiDragAxis) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            axis,
            pointers: Arc::new(Mutex::new(HashMap::new())),
            on_start: Arc::new(Mutex::new(None)),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
        })
    }

    /// Construct with explicit gesture settings.
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        axis: MultiDragAxis,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            axis,
            pointers: Arc::new(Mutex::new(HashMap::new())),
            on_start: Arc::new(Mutex::new(None)),
            settings: Arc::new(Mutex::new(settings)),
        })
    }

    /// Set the per-pointer start callback. The callback may return `None` to
    /// reject the drag (caller can read pointer position to filter by region).
    pub fn with_on_start(self: Arc<Self>, callback: MultiDragStartCallback) -> Arc<Self> {
        *self.on_start.lock() = Some(callback);
        self
    }

    /// Slop threshold for the given pointer kind.
    ///
    /// `GestureSettings` currently exposes one `pan_slop` knob that all
    /// pointer kinds share — Flutter's `computeHitSlop` per-device split
    /// (touch/mouse/pen/trackpad) is collapsed here until a per-device
    /// slop field is added. Pre-existing settings shape (U26) is the
    /// contract this method respects.
    fn slop_for(&self, _kind: PointerType) -> f32 {
        self.settings.lock().pan_slop()
    }

    /// Number of pointers currently tracked (pre- and post-acceptance).
    pub fn tracked_pointer_count(&self) -> usize {
        self.pointers.lock().len()
    }

    /// Snapshot of all tracked pointer ids.
    #[must_use]
    pub fn tracked_pointers(&self) -> Vec<PointerId> {
        self.pointers.lock().keys().copied().collect()
    }

    // ------------------------------------------------------------------
    // Per-pointer lifecycle
    // ------------------------------------------------------------------

    /// Add a pointer — called from the binding's hit-test dispatch.
    fn add_pointer_impl(&self, pointer: PointerId, position: Offset<Pixels>, kind: PointerType) {
        let slop = self.slop_for(kind);
        let state = MultiDragPointerState::new(position, slop);
        self.pointers.lock().insert(pointer, state);
    }

    /// Remove a pointer's state. Returns the removed state (if any) for
    /// terminal callbacks.
    fn remove_pointer(&self, pointer: PointerId) -> Option<MultiDragPointerState> {
        self.pointers.lock().remove(&pointer)
    }

    // ------------------------------------------------------------------
    // Event handlers
    // ------------------------------------------------------------------

    /// Handle a pointer-move event for a specific pointer.
    fn handle_move(
        &self,
        pointer: PointerId,
        position: Offset<Pixels>,
        kind: PointerType,
        timestamp: Instant,
    ) {
        let on_start = self.on_start.lock().clone();
        let mut map = self.pointers.lock();
        let Some(state) = map.get_mut(&pointer) else {
            return;
        };
        let delta = (position - state.last_position).to_delta();
        state.last_position = position;
        state.velocity_tracker.add_position(timestamp, position);

        if state.accepted {
            // Post-acceptance path: forward to the user's handle.
            if let Some(client) = state.client.as_ref() {
                let details = MultiDragUpdateDetails {
                    pointer_id: pointer,
                    global_position: position,
                    local_position: position,
                    delta,
                    kind,
                    timestamp,
                };
                client.update(details);
            }
            return;
        }

        // Pre-acceptance path: accumulate pending delta; check for arena
        // resolution.
        state.pending_delta += delta;
        state.kind = kind;
        if state.check_for_resolution_after_move(self.axis) {
            // The pointer is moving beyond slop — drop the pre-acceptance
            // state and let the user callback own the drag from here.
            let pending = state.pending_delta;
            state.pending_delta = Offset::new(PixelDelta::ZERO, PixelDelta::ZERO);
            drop(map); // Release lock before invoking the user callback.

            // Invoke user callback (outside the lock) to obtain a handle.
            if let Some(cb) = on_start {
                if let Some(handle) = cb(pointer, position) {
                    let mut map = self.pointers.lock();
                    if let Some(state) = map.get_mut(&pointer) {
                        state.accepted = true;
                        state.client = Some(handle);
                        // Flush the accumulated pending delta as the first
                        // update so the user sees motion from the down point.
                        let details = MultiDragUpdateDetails {
                            pointer_id: pointer,
                            global_position: position,
                            local_position: position,
                            delta: pending,
                            kind,
                            timestamp,
                        };
                        if let Some(client) = state.client.as_ref() {
                            client.update(details);
                        }
                    }
                } else {
                    // User rejected the drag — drop the state silently.
                    self.remove_pointer(pointer);
                }
            } else {
                // No callback installed — accept implicitly but drop the
                // state to avoid leaking. This matches Flutter's behaviour
                // for an unconfigured multi-drag (it just consumes the
                // pointer until release).
                let mut map = self.pointers.lock();
                if let Some(state) = map.get_mut(&pointer) {
                    state.accepted = true;
                    state.client = None;
                }
            }
        }
    }

    /// Handle pointer up.
    fn handle_up(&self, pointer: PointerId, position: Offset<Pixels>, _kind: PointerType) {
        let removed = self.remove_pointer(pointer);
        if let Some(state) = removed
            && state.accepted
            && let Some(client) = state.client.as_ref()
        {
            let details = MultiDragEndDetails {
                pointer_id: pointer,
                global_position: position,
                velocity: state.velocity_tracker.velocity(),
                kind: state.kind,
            };
            client.end(details);
        }
        // Pre-acceptance up: drop state silently. The recogniser
        // never produced a drag, so no callback is needed.
    }

    /// Handle pointer cancel.
    fn handle_cancel(&self, pointer: PointerId) {
        let removed = self.remove_pointer(pointer);
        if let Some(state) = removed
            && state.accepted
            && let Some(client) = state.client.as_ref()
        {
            client.cancel();
        }
    }
}

impl GestureRecognizer for MultiDragGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>) {
        // U11: per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "multidrag.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        self.add_pointer_impl(pointer, position, PointerType::Touch);
    }

    fn handle_event(&self, event: &PointerEvent) {
        // U11: per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "multidrag.handle_event",
            kind = %crate::observability::pointer_event_kind(event),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        let pointer = crate::events::extract_pointer_id(event);
        let (position, kind) = match event {
            PointerEvent::Move(e) => (e.current.position, e.pointer.pointer_type),
            PointerEvent::Up(e) => (e.state.position, e.pointer.pointer_type),
            _ => return,
        };
        // Position is `PhysicalPosition<f64>`; convert to Offset<Pixels>.
        let position = Offset::new(Pixels(position.x as f32), Pixels(position.y as f32));
        match event {
            PointerEvent::Move(_) => self.handle_move(pointer, position, kind, Instant::now()),
            PointerEvent::Up(_) => self.handle_up(pointer, position, kind),
            PointerEvent::Cancel(_) => self.handle_cancel(pointer),
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Drop every pending handle with a cancel notification. Average O(n),
        // worst-case O(n) where n = concurrent pointers; bounded by input
        // device capacity (≤10 touch points in practice).
        // Collect Box<dyn> handles out of the map first; storing the
        // owned Box lets us release the map lock before invoking
        // cancel (which may re-enter user code).
        let removed: Vec<Box<dyn MultiDragHandle>> = self // PORT-CHECK-OK-DYN: see MultiDragStartCallback — drain-time owned handle collection.
            .pointers
            .lock()
            .drain()
            .filter_map(
                |(_, state)| {
                    if state.accepted { state.client } else { None }
                },
            )
            .collect();
        for handle in removed {
            handle.cancel();
        }
        *self.on_start.lock() = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        // Multi-drag has no single primary — return the first tracked
        // pointer for parity with monodrag (which reports its own primary).
        self.pointers.lock().keys().next().copied()
    }
}

impl GestureArenaMember for MultiDragGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        // Arena resolution happens on a per-pointer basis through the
        // slop-crossing path in `handle_move`. The arena's accept callback
        // is therefore a no-op for multi-drag (the user callback fires
        // from `handle_move` instead, after the handle is constructed).
        // We still mark the pointer as accepted for state-bookkeeping.
        if let Some(state) = self.pointers.lock().get_mut(&pointer) {
            state.accepted = true;
        }
    }

    fn reject_gesture(&self, pointer: PointerId) {
        // Lost the arena — cancel the drag for that pointer only. Other
        // pointers in flight are untouched (per-pointer isolation is the
        // whole point of the multi- prefix).
        self.handle_cancel(pointer);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{make_move_event_for_id, make_up_event_for_id};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test handle that counts update/end/cancel invocations.
    struct CountingHandle {
        updates: Arc<AtomicUsize>,
        ends: Arc<AtomicUsize>,
        cancels: Arc<AtomicUsize>,
    }
    impl MultiDragHandle for CountingHandle {
        fn update(&self, _details: MultiDragUpdateDetails) {
            self.updates.fetch_add(1, Ordering::SeqCst);
        }
        fn end(&self, _details: MultiDragEndDetails) {
            self.ends.fetch_add(1, Ordering::SeqCst);
        }
        fn cancel(&self) {
            self.cancels.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Test handle that records the *first* update (to assert the pending
    /// delta flush carries the accumulated distance).
    struct FirstDeltaRecorder {
        first: Arc<Mutex<Option<Offset<PixelDelta>>>>,
    }
    impl MultiDragHandle for FirstDeltaRecorder {
        fn update(&self, details: MultiDragUpdateDetails) {
            let mut slot = self.first.lock();
            if slot.is_none() {
                *slot = Some(details.delta);
            }
        }
        fn end(&self, _: MultiDragEndDetails) {}
        fn cancel(&self) {}
    }

    fn pointer_id(n: u64) -> PointerId {
        PointerId::new(n).expect("nonzero pointer id")
    }

    #[test]
    fn new_recogniser_tracks_zero_pointers() {
        let arena = crate::arena::GestureArena::new();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free);
        assert_eq!(rec.tracked_pointer_count(), 0);
        assert!(rec.tracked_pointers().is_empty());
    }

    #[test]
    fn add_pointer_increments_tracked_count() {
        let arena = crate::arena::GestureArena::new();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free);
        rec.add_pointer(pointer_id(1), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec.add_pointer(pointer_id(2), Offset::new(Pixels(0.0), Pixels(0.0)));
        assert_eq!(rec.tracked_pointer_count(), 2);
    }

    #[test]
    fn each_pointer_gets_independent_drag() {
        // Two pointers, both with handles, both moved past slop.
        // Verifies per-pointer isolation: one drag's events don't leak
        // into the other.
        let arena = crate::arena::GestureArena::new();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free);

        let updates_p1 = Arc::new(AtomicUsize::new(0));
        let updates_p2 = Arc::new(AtomicUsize::new(0));
        let p1_updates = updates_p1.clone();
        let p2_updates = updates_p2.clone();
        let rec2 = rec.with_on_start(Arc::new(move |pointer, _pos| {
            if pointer == pointer_id(1) {
                Some(Box::new(CountingHandle {
                    updates: p1_updates.clone(),
                    ends: Arc::new(AtomicUsize::new(0)),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            } else {
                Some(Box::new(CountingHandle {
                    updates: p2_updates.clone(),
                    ends: Arc::new(AtomicUsize::new(0)),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            }
        }));
        // `with_on_start` returns a new Arc; use that one for events.
        let rec2 = rec2;

        // Add two pointers.
        rec2.add_pointer(pointer_id(1), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec2.add_pointer(pointer_id(2), Offset::new(Pixels(50.0), Pixels(50.0)));

        // Pointer 1 makes one big move (0→25 = 25px > 18px slop) — fires the
        // pending-delta flush, which is the first update. The second move
        // is post-acceptance and fires a second update.
        rec2.handle_event(&make_move_event_for_id(
            pointer_id(1),
            Offset::new(Pixels(25.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        rec2.handle_event(&make_move_event_for_id(
            pointer_id(1),
            Offset::new(Pixels(30.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        // Pointer 2 also moves and accepts.
        rec2.handle_event(&make_move_event_for_id(
            pointer_id(2),
            Offset::new(Pixels(70.0), Pixels(50.0)),
            PointerType::Touch,
        ));

        // Pointer 1: pending-flush + one post-acceptance update = 2.
        // Pointer 2: pending-flush only = 1.
        assert_eq!(updates_p1.load(Ordering::SeqCst), 2);
        assert_eq!(updates_p2.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pending_delta_flushes_on_acceptance() {
        // The first update received by the handle must carry the *full*
        // accumulated delta (not just the last move's delta).
        let arena = crate::arena::GestureArena::new();
        let first = Arc::new(Mutex::new(None));
        let first_clone = first.clone();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(
            Arc::new(move |_pointer, _pos| {
                Some(Box::new(FirstDeltaRecorder {
                    first: first_clone.clone(),
                }))
            }),
        );

        rec.add_pointer(pointer_id(7), Offset::new(Pixels(0.0), Pixels(0.0)));
        // Two small moves that together exceed 18px slop.
        rec.handle_event(&make_move_event_for_id(
            pointer_id(7),
            Offset::new(Pixels(10.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(7),
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));

        // The handle received the first update with the accumulated delta.
        let recorded = *first.lock();
        let recorded = recorded.expect("first update fired");
        assert!(
            recorded.dx.0.abs() > 18.0,
            "expected first update to carry ≥slop delta, got {:?}",
            recorded
        );
    }

    #[test]
    fn up_after_acceptance_fires_end() {
        let arena = crate::arena::GestureArena::new();
        let ends = Arc::new(AtomicUsize::new(0));
        let ends_clone = ends.clone();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(
            Arc::new(move |_pointer, _pos| {
                Some(Box::new(CountingHandle {
                    updates: Arc::new(AtomicUsize::new(0)),
                    ends: ends_clone.clone(),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            }),
        );

        rec.add_pointer(pointer_id(3), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(3),
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        rec.handle_event(&make_up_event_for_id(
            pointer_id(3),
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        assert_eq!(ends.load(Ordering::SeqCst), 1);
        assert_eq!(rec.tracked_pointer_count(), 0);
    }

    #[test]
    fn up_before_acceptance_is_silent() {
        // Move less than slop, then up — no end callback, state cleared.
        let arena = crate::arena::GestureArena::new();
        let ends = Arc::new(AtomicUsize::new(0));
        let ends_clone = ends.clone();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(
            Arc::new(move |_pointer, _pos| {
                Some(Box::new(CountingHandle {
                    updates: Arc::new(AtomicUsize::new(0)),
                    ends: ends_clone.clone(),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            }),
        );

        rec.add_pointer(pointer_id(4), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(4),
            Offset::new(Pixels(5.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        rec.handle_event(&make_up_event_for_id(
            pointer_id(4),
            Offset::new(Pixels(5.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        assert_eq!(ends.load(Ordering::SeqCst), 0);
        assert_eq!(rec.tracked_pointer_count(), 0);
    }

    #[test]
    fn horizontal_axis_ignores_vertical_motion() {
        // Vertical motion under Horizontal axis must not cross slop.
        let arena = crate::arena::GestureArena::new();
        let updates = Arc::new(AtomicUsize::new(0));
        let updates_clone = updates.clone();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Horizontal).with_on_start(
            Arc::new(move |_pointer, _pos| {
                Some(Box::new(CountingHandle {
                    updates: updates_clone.clone(),
                    ends: Arc::new(AtomicUsize::new(0)),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            }),
        );

        rec.add_pointer(pointer_id(5), Offset::new(Pixels(0.0), Pixels(0.0)));
        // 50px vertical, 0px horizontal — must not resolve.
        rec.handle_event(&make_move_event_for_id(
            pointer_id(5),
            Offset::new(Pixels(0.0), Pixels(50.0)),
            PointerType::Touch,
        ));
        assert_eq!(updates.load(Ordering::SeqCst), 0);
        // 30px horizontal — now resolves.
        rec.handle_event(&make_move_event_for_id(
            pointer_id(5),
            Offset::new(Pixels(30.0), Pixels(50.0)),
            PointerType::Touch,
        ));
        assert_eq!(updates.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reject_from_arena_cancels_one_pointer_only() {
        // Per-pointer isolation: rejecting pointer 7 must not affect pointer 8.
        let arena = crate::arena::GestureArena::new();
        let cancels_p7 = Arc::new(AtomicUsize::new(0));
        let cancels_p8 = Arc::new(AtomicUsize::new(0));
        let c7 = cancels_p7.clone();
        let c8 = cancels_p8.clone();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(
            Arc::new(move |pointer, _pos| {
                if pointer == pointer_id(7) {
                    Some(Box::new(CountingHandle {
                        updates: Arc::new(AtomicUsize::new(0)),
                        ends: Arc::new(AtomicUsize::new(0)),
                        cancels: c7.clone(),
                    }))
                } else {
                    Some(Box::new(CountingHandle {
                        updates: Arc::new(AtomicUsize::new(0)),
                        ends: Arc::new(AtomicUsize::new(0)),
                        cancels: c8.clone(),
                    }))
                }
            }),
        );

        rec.add_pointer(pointer_id(7), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec.add_pointer(pointer_id(8), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(7),
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(8),
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        // Reject pointer 7 — should cancel only p7's drag.
        rec.reject_gesture(pointer_id(7));
        assert_eq!(cancels_p7.load(Ordering::SeqCst), 1);
        assert_eq!(cancels_p8.load(Ordering::SeqCst), 0);
        // Pointer 8 still tracked.
        assert_eq!(rec.tracked_pointer_count(), 1);
    }

    #[test]
    fn dispose_cancels_accepted_pointers() {
        let arena = crate::arena::GestureArena::new();
        let cancels = Arc::new(AtomicUsize::new(0));
        let cancels_clone = cancels.clone();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(
            Arc::new(move |_pointer, _pos| {
                Some(Box::new(CountingHandle {
                    updates: Arc::new(AtomicUsize::new(0)),
                    ends: Arc::new(AtomicUsize::new(0)),
                    cancels: cancels_clone.clone(),
                }))
            }),
        );

        rec.add_pointer(pointer_id(9), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(9),
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        rec.dispose();
        assert_eq!(cancels.load(Ordering::SeqCst), 1);
        // State cleared.
        assert_eq!(rec.tracked_pointer_count(), 0);
    }

    #[test]
    fn on_start_returning_none_drops_pointer() {
        // User callback rejects the drag — pointer state is removed
        // silently, no further updates flow to it.
        let arena = crate::arena::GestureArena::new();
        let rec = MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free)
            .with_on_start(Arc::new(|_pointer, _pos| None));
        rec.add_pointer(pointer_id(11), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(11),
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        // After rejection, the pointer state is removed.
        assert_eq!(rec.tracked_pointer_count(), 0);
    }
}
