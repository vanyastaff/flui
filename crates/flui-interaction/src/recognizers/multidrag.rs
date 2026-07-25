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
//! 2. Each pointer joins its own arena. A lone immediate multi-drag can win by
//!    default after Down; under competition, crossing `slop` (or the selected
//!    axis slop) self-declares acceptance.
//! 3. On acceptance, `on_start(pointer_id, position)` fires; the closure
//!    returns an opaque `MultiDragHandle` whose `update`/`end`/`cancel`
//!    methods are called for the lifetime of that drag.
//! 4. On `Up`/`Cancel`, the exact arena entry and per-pointer client are
//!    retired before the terminal callback can re-enter.
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
//! | Tap → drag use case | yes | no (use [`TapAndDragGestureRecognizer`](crate::recognizers::TapAndDragGestureRecognizer)) |

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Instant};

use flui_types::{
    Offset,
    geometry::{PixelDelta, Pixels},
};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::{GestureArenaEntry, GestureArenaMember, GestureDisposition},
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    routing::RoutePanic,
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
///
/// Handles run on the UI owner and may capture `Rc` state. They are not a
/// cross-thread dispatch boundary.
pub trait MultiDragHandle: 'static {
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
    Rc<dyn Fn(PointerId, Offset<Pixels>) -> Option<Box<dyn MultiDragHandle>>>; // PORT-CHECK-OK-DYN: per-pointer handle trait; ≤3 workspace sites, marker preferred over allowlist promotion.

/// Per-pointer state. Mirrors Flutter's `MultiDragPointerState`.
///
/// State lifecycle: `Possible` (added) → `Accepted` (arena victory) → `Ended` /
/// `Cancelled` (terminal). The client is installed only after `on_start`
/// returns; pending movement is retained until then.
struct MultiDragPointerState {
    /// Global position where this contact began.
    initial_position: Offset<Pixels>,
    /// Last reported position (for delta computation).
    last_position: Offset<Pixels>,
    /// Pointer device kind (slop, velocity-tracker flavour).
    kind: PointerType,
    /// Slop threshold for this pointer kind.
    slop: f32,
    /// Accumulated delta while `pending` (pre-acceptance).
    pending_delta: Offset<PixelDelta>,
    /// `true` once the arena has accepted this pointer.
    accepted: bool,
    /// User's handle, populated after `accepted`.
    client: Option<Rc<dyn MultiDragHandle>>, // PORT-CHECK-OK-DYN: owner-local per-pointer drag client.
    /// Velocity tracker fed while `pending` and after `accepted`.
    velocity_tracker: VelocityTracker,
    /// Timestamp of the most recent movement accumulated before acceptance.
    last_pending_timestamp: Option<Instant>,
    /// Stale-safe handle to the exact arena generation and member registered
    /// for this contact. It holds only weak references, so storing it beside
    /// recognizer state cannot create a cycle.
    arena_entry: Option<GestureArenaEntry>,
}

impl MultiDragPointerState {
    fn new(initial_position: Offset<Pixels>, kind: PointerType, slop: f32) -> Self {
        Self {
            initial_position,
            last_position: initial_position,
            kind,
            slop,
            pending_delta: Offset::new(PixelDelta::ZERO, PixelDelta::ZERO),
            accepted: false,
            client: None,
            velocity_tracker: VelocityTracker::new(),
            last_pending_timestamp: None,
            arena_entry: None,
        }
    }

    /// Per-axis primary slop test — accepts when |delta.{axis}| exceeds slop.
    fn check_for_resolution_after_move(&mut self, axis: MultiDragAxis) -> bool {
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
    on_start: Rc<RefCell<Option<MultiDragStartCallback>>>,
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
            on_start: Rc::new(RefCell::new(None)),
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
            on_start: Rc::new(RefCell::new(None)),
            settings: Arc::new(Mutex::new(settings)),
        })
    }

    /// Set the per-pointer start callback. The callback may return `None` to
    /// reject the drag (caller can read pointer position to filter by region).
    pub fn with_on_start(self: Arc<Self>, callback: MultiDragStartCallback) -> Arc<Self> {
        *self.on_start.borrow_mut() = Some(callback);
        self
    }

    /// Slop threshold for the given pointer kind.
    ///
    /// `GestureSettings` currently exposes one `pan_slop` knob that all
    /// pointer kinds share — Flutter's `computeHitSlop` per-device split
    /// (touch/mouse/pen/trackpad) is collapsed here until a per-device
    /// slop field is added. The pre-existing settings shape is the
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
    fn add_pointer_impl(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<Pixels>,
        kind: PointerType,
    ) {
        let slop = self.slop_for(kind);
        let mut state = MultiDragPointerState::new(position, kind, slop);

        // Compete in the arena with a stable member identity. A lone immediate
        // multi-drag may win by default after Down, exactly like Flutter;
        // delayed variants belong in a distinct recognizer policy, not an
        // arena-wide hold.
        let member: Arc<dyn GestureArenaMember> = Arc::<Self>::clone(self);
        let entry = self.state.arena().add(pointer, member);
        state.arena_entry = Some(entry);

        self.pointers.lock().insert(pointer, state);
    }

    /// Withdraw this recogniser's exact arena entry.
    ///
    /// Both operations are stale-safe and idempotent after acceptance,
    /// teardown, or pointer-ID reuse.
    fn retire_arena_entry(entry: Option<&GestureArenaEntry>) -> Option<RoutePanic> {
        let entry = entry?;
        RoutePanic::capture(|| entry.resolve(GestureDisposition::Rejected))
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
        let (client, update, arena_entry) = {
            let mut map = self.pointers.lock();
            let Some(state) = map.get_mut(&pointer) else {
                return;
            };
            let delta = (position - state.last_position).to_delta();
            state.last_position = position;
            state.kind = kind;
            state.velocity_tracker.add_position(timestamp, position);

            if let Some(client) = state.client.clone() {
                let update = MultiDragUpdateDetails {
                    pointer_id: pointer,
                    global_position: position,
                    local_position: position,
                    delta,
                    kind,
                    timestamp,
                };
                (Some(client), Some(update), None)
            } else {
                // Before the arena accepts us, retain every delta. This also
                // covers re-entrant movement while `on_start` is running:
                // `accepted` is already true, but the client is not installed
                // until that callback returns.
                state.pending_delta += delta;
                state.last_pending_timestamp = Some(timestamp);
                let arena_entry = (!state.accepted
                    && state.check_for_resolution_after_move(self.axis))
                .then(|| state.arena_entry.clone())
                .flatten();
                (None, None, arena_entry)
            }
        };

        if let (Some(client), Some(update)) = (client, update) {
            // No recognizer-state lock crosses user code.
            client.update(update);
        } else if let Some(entry) = arena_entry {
            // The arena callback, not the slop detector, starts the client.
            // This preserves eager/default wins and competition ordering.
            entry.resolve(GestureDisposition::Accepted);
        }
    }

    /// Start the per-pointer client only after the arena actually accepts it.
    fn start_accepted_drag(&self, pointer: PointerId) {
        let initial_position = {
            let mut map = self.pointers.lock();
            let Some(state) = map.get_mut(&pointer) else {
                return;
            };
            if state.accepted {
                return;
            }
            state.accepted = true;
            state.initial_position
        };

        let on_start = self.on_start.borrow().clone();
        let handle = match on_start {
            Some(callback) => match RoutePanic::try_run(|| callback(pointer, initial_position)) {
                Ok(handle) => handle,
                Err(panic) => {
                    let removed = self.remove_pointer(pointer);
                    let mut first_panic = Some(panic);
                    let retired = removed
                        .as_ref()
                        .and_then(|state| Self::retire_arena_entry(state.arena_entry.as_ref()));
                    RoutePanic::preserve_first(
                        &mut first_panic,
                        retired,
                        "multi-drag failed-start arena retirement",
                    );
                    first_panic
                        .expect("the failed start callback supplied a panic")
                        .resume();
                }
            },
            None => None,
        };

        let Some(handle) = handle else {
            let removed = self.remove_pointer(pointer);
            if let Some(panic) = removed
                .as_ref()
                .and_then(|state| Self::retire_arena_entry(state.arena_entry.as_ref()))
            {
                panic.resume();
            }
            return;
        };
        let client: Rc<dyn MultiDragHandle> = Rc::from(handle); // PORT-CHECK-OK-DYN: owner-local per-pointer drag client returned by the public factory.

        let update = {
            let mut map = self.pointers.lock();
            let Some(state) = map.get_mut(&pointer) else {
                drop(map);
                let mut first_panic = RoutePanic::capture(|| client.cancel());
                let dropped = RoutePanic::capture(|| drop(client));
                RoutePanic::preserve_first(
                    &mut first_panic,
                    dropped,
                    "multi-drag orphaned client cleanup",
                );
                if let Some(panic) = first_panic {
                    panic.resume();
                }
                return;
            };

            let update = MultiDragUpdateDetails {
                pointer_id: pointer,
                global_position: state.initial_position,
                local_position: state.initial_position,
                delta: state.pending_delta,
                kind: state.kind,
                timestamp: state.last_pending_timestamp.unwrap_or_else(Instant::now),
            };
            state.pending_delta = Offset::new(PixelDelta::ZERO, PixelDelta::ZERO);
            state.last_pending_timestamp = None;
            state.client = Some(Rc::clone(&client));
            update
        };

        // Store the client before its first callback, then call it last. A
        // re-entrant terminal event can now find and retire this exact drag.
        client.update(update);
    }

    /// Handle pointer up.
    fn handle_up(&self, pointer: PointerId, position: Offset<Pixels>, _kind: PointerType) {
        let Some(mut state) = self.remove_pointer(pointer) else {
            return;
        };
        let mut first_panic = Self::retire_arena_entry(state.arena_entry.as_ref());
        if let Some(client) = state.client.take() {
            // Read the velocity first (it borrows the tracker mutably to
            // memoize) before invoking the client.
            let velocity = state.velocity_tracker.get_velocity();
            let details = MultiDragEndDetails {
                pointer_id: pointer,
                global_position: position,
                velocity,
                kind: state.kind,
            };
            let ended = RoutePanic::capture(|| client.end(details));
            RoutePanic::preserve_first(&mut first_panic, ended, "multi-drag client end");
            let dropped = RoutePanic::capture(|| drop(client));
            RoutePanic::preserve_first(
                &mut first_panic,
                dropped,
                "multi-drag ended client cleanup",
            );
        }
        if let Some(panic) = first_panic {
            panic.resume();
        }
    }

    /// Handle pointer cancel.
    fn handle_cancel(&self, pointer: PointerId) {
        let Some(mut state) = self.remove_pointer(pointer) else {
            return;
        };
        // Retire the arena entry before user code runs. A panicking cancel
        // callback must not strand a held contact or target a reused ID.
        let mut first_panic = Self::retire_arena_entry(state.arena_entry.as_ref());
        if let Some(client) = state.client.take() {
            let cancelled = RoutePanic::capture(|| client.cancel());
            RoutePanic::preserve_first(&mut first_panic, cancelled, "multi-drag client cancel");
            let dropped = RoutePanic::capture(|| drop(client));
            RoutePanic::preserve_first(&mut first_panic, dropped, "multi-drag client cleanup");
        }
        if let Some(panic) = first_panic {
            panic.resume();
        }
    }
}

impl GestureRecognizer for MultiDragGestureRecognizer {
    fn add_pointer(self: &Arc<Self>, pointer: PointerId, position: Offset<Pixels>) {
        // per-impl span (trait fn disallows `#[instrument]`).
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
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "multidrag.handle_event",
            kind = %crate::observability::pointer_event_kind(event),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        let pointer = crate::events::extract_pointer_id(event);

        // `Cancel` carries no meaningful position; route it before the Move/Up
        // position extraction below, whose `_ => return` arm would otherwise
        // swallow it and leak the pointer's accepted drag state.
        if matches!(event, PointerEvent::Cancel(_)) {
            self.handle_cancel(pointer);
            return;
        }

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
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Detach every pointer before arena or user callbacks can re-enter.
        // Retire all exact arena entries first, then notify accepted clients;
        // one hostile callback cannot strand another pointer's arena entry.
        let mut removed: Vec<(PointerId, MultiDragPointerState)> =
            self.pointers.lock().drain().collect();
        removed.sort_unstable_by_key(|(pointer, _)| *pointer);

        let mut first_panic = None;
        for (_, state) in &removed {
            let retired = Self::retire_arena_entry(state.arena_entry.as_ref());
            RoutePanic::preserve_first(
                &mut first_panic,
                retired,
                "multi-drag dispose arena retirement",
            );
        }
        for handle in removed.into_iter().filter_map(|(_, state)| state.client) {
            let cancelled = RoutePanic::capture(|| handle.cancel());
            RoutePanic::preserve_first(
                &mut first_panic,
                cancelled,
                "multi-drag dispose client cancel",
            );
            let dropped = RoutePanic::capture(|| drop(handle));
            RoutePanic::preserve_first(
                &mut first_panic,
                dropped,
                "multi-drag dispose client cleanup",
            );
        }
        let on_start = self.on_start.borrow_mut().take();
        let dropped_on_start = RoutePanic::capture(|| drop(on_start));
        RoutePanic::preserve_first(
            &mut first_panic,
            dropped_on_start,
            "multi-drag start callback cleanup",
        );
        if let Some(panic) = first_panic {
            panic.resume();
        }
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        // Multi-drag has no single primary — return the first tracked
        // pointer for parity with monodrag (which reports its own primary).
        self.pointers.lock().keys().next().copied()
    }
}

impl GestureArenaMember for MultiDragGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        self.start_accepted_drag(pointer);
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
    use crate::events::{make_cancel_event, make_move_event_for_id, make_up_event_for_id};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static_assertions::assert_not_impl_any!(MultiDragGestureRecognizer: Send, Sync);

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

    struct PanickingCancelHandle;
    impl MultiDragHandle for PanickingCancelHandle {
        fn update(&self, _details: MultiDragUpdateDetails) {}
        fn end(&self, _details: MultiDragEndDetails) {}
        fn cancel(&self) {
            panic!("multi-drag cancel panic");
        }
    }

    struct ReentrantCancelHandle {
        recognizer: Arc<MultiDragGestureRecognizer>,
        did_reenter: Cell<bool>,
        cancels: Rc<Cell<usize>>,
    }
    impl MultiDragHandle for ReentrantCancelHandle {
        fn update(&self, _details: MultiDragUpdateDetails) {
            if !self.did_reenter.replace(true) {
                self.recognizer
                    .handle_event(&make_cancel_event(PointerType::Touch));
            }
        }
        fn end(&self, _details: MultiDragEndDetails) {}
        fn cancel(&self) {
            self.cancels.set(self.cancels.get() + 1);
        }
    }

    fn pointer_id(n: u64) -> PointerId {
        PointerId::new(n).expect("nonzero pointer id")
    }

    // Returns the concrete fixture; callers box it at the `Option<Box<dyn …>>`
    // slot, so no `dyn` appears in this signature (keeps port-check trigger 9
    // satisfied without a fmt-fragile inline marker).
    fn counting_handle(cancels: Arc<AtomicUsize>) -> CountingHandle {
        CountingHandle {
            updates: Arc::new(AtomicUsize::new(0)),
            ends: Arc::new(AtomicUsize::new(0)),
            cancels,
        }
    }

    /// Minimal competing arena member that records whether it was rejected.
    struct RejectableMember {
        rejected: Arc<Mutex<bool>>,
    }
    impl crate::sealed::arena_member::Sealed for RejectableMember {}
    impl crate::arena::GestureArenaMember for RejectableMember {
        fn accept_gesture(&self, _pointer: PointerId) {}
        fn reject_gesture(&self, _pointer: PointerId) {
            *self.rejected.lock() = true;
        }
    }

    #[derive(Default)]
    struct AcceptingMember {
        accepts: AtomicUsize,
    }
    impl crate::sealed::arena_member::Sealed for AcceptingMember {}
    impl crate::arena::GestureArenaMember for AcceptingMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            self.accepts.fetch_add(1, Ordering::SeqCst);
        }
        fn reject_gesture(&self, _pointer: PointerId) {}
    }

    #[test]
    fn cancel_event_routes_to_handle_cancel() {
        // A Cancel event for a tracked pointer must reach `handle_cancel` (fire
        // the cancel handle + clear the per-pointer state), not be swallowed by
        // the Move/Up position-extraction match.
        let arena = crate::arena::GestureArena::new();
        let cancels = Arc::new(AtomicUsize::new(0));
        let cancels_cb = cancels.clone();
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |_pointer, _pos| {
                Some(Box::new(counting_handle(cancels_cb.clone())) as _)
            }));

        let p = PointerId::PRIMARY;
        rec.add_pointer(p, Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.close(p);
        // Cross slop so a client handle exists, then cancel.
        rec.handle_event(&make_move_event_for_id(
            p,
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        ));
        rec.handle_event(&make_cancel_event(PointerType::Touch));

        assert_eq!(
            cancels.load(Ordering::SeqCst),
            1,
            "cancel handle should fire"
        );
        assert_eq!(
            rec.tracked_pointer_count(),
            0,
            "pointer state should be cleared on cancel"
        );
    }

    #[test]
    fn slop_cross_rejects_competing_arena_member() {
        // Multi-drag must really compete in the arena: crossing slop wins the
        // pointer and rejects other members contending for it.
        let arena = crate::arena::GestureArena::new();
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(|_pointer, _pos| {
                Some(Box::new(counting_handle(Arc::new(AtomicUsize::new(0)))) as _)
            }));

        let p = pointer_id(9);
        rec.add_pointer(p, Offset::new(Pixels(0.0), Pixels(0.0)));

        // A competitor joins the same arena entry.
        let rejected = Arc::new(Mutex::new(false));
        arena.add(
            p,
            Arc::new(RejectableMember {
                rejected: rejected.clone(),
            }),
        );
        arena.close(p);

        // Cross slop -> multi-drag wins -> competitor rejected.
        rec.handle_event(&make_move_event_for_id(
            p,
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        ));

        assert!(
            *rejected.lock(),
            "competing member should be rejected when multi-drag wins the arena"
        );
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
    fn lone_pointer_starts_when_the_closed_arena_awards_default_victory() {
        let arena = crate::arena::GestureArena::new();
        let starts = Rc::new(Cell::new(0));
        let starts_for_callback = Rc::clone(&starts);
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |_pointer, _position| {
                starts_for_callback.set(starts_for_callback.get() + 1);
                Some(Box::new(counting_handle(Arc::new(AtomicUsize::new(0)))) as _)
            }));
        let pointer = pointer_id(2);

        rec.add_pointer(pointer, Offset::new(Pixels(4.0), Pixels(8.0)));
        arena.close(pointer);
        arena.drain_deferred_resolutions();

        assert_eq!(
            starts.get(),
            1,
            "Flutter's immediate multi-drag starts when it wins by default"
        );
        assert_eq!(rec.tracked_pointer_count(), 1);
    }

    #[test]
    fn on_start_accepts_owner_local_rc_state() {
        let arena = crate::arena::GestureArena::new();
        let starts = Rc::new(Cell::new(0));
        let starts_for_callback = Rc::clone(&starts);
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |_pointer, _pos| {
                starts_for_callback.set(starts_for_callback.get() + 1);
                Some(Box::new(CountingHandle {
                    updates: Arc::new(AtomicUsize::new(0)),
                    ends: Arc::new(AtomicUsize::new(0)),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            }));

        rec.add_pointer(pointer_id(1), Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.close(pointer_id(1));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(1),
            Offset::new(Pixels(25.0), Pixels(0.0)),
            PointerType::Touch,
        ));

        assert_eq!(
            starts.get(),
            1,
            "multi-drag start callback captured Rc<Cell<_>>"
        );
    }

    #[test]
    fn initial_client_update_can_reenter_terminal_input() {
        let arena = crate::arena::GestureArena::new();
        let recognizer_slot = Rc::new(RefCell::new(None));
        let slot_for_callback = Rc::clone(&recognizer_slot);
        let cancels = Rc::new(Cell::new(0));
        let cancels_for_callback = Rc::clone(&cancels);
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |_pointer, _position| {
                let recognizer = slot_for_callback
                    .borrow()
                    .as_ref()
                    .cloned()
                    .expect("recognizer installed before input");
                Some(Box::new(ReentrantCancelHandle {
                    recognizer,
                    did_reenter: Cell::new(false),
                    cancels: Rc::clone(&cancels_for_callback),
                }) as _)
            }));
        *recognizer_slot.borrow_mut() = Some(rec.clone());

        rec.add_pointer(PointerId::PRIMARY, Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.close(PointerId::PRIMARY);
        rec.handle_event(&make_move_event_for_id(
            PointerId::PRIMARY,
            Offset::new(Pixels(25.0), Pixels(0.0)),
            PointerType::Touch,
        ));

        assert_eq!(cancels.get(), 1);
        assert_eq!(rec.tracked_pointer_count(), 0);
        assert!(arena.is_empty());
    }

    #[test]
    fn each_pointer_gets_independent_drag() {
        // Two pointers, both with handles, both moved past slop.
        // Verifies per-pointer isolation: one drag's events don't leak
        // into the other.
        let arena = crate::arena::GestureArena::new();
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free);

        let updates_p1 = Arc::new(AtomicUsize::new(0));
        let updates_p2 = Arc::new(AtomicUsize::new(0));
        let p1_updates = updates_p1.clone();
        let p2_updates = updates_p2.clone();
        let rec2 = rec.with_on_start(Rc::new(move |pointer, _pos| {
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
        arena.close(pointer_id(1));
        arena.close(pointer_id(2));

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
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |_pointer, _pos| {
                Some(Box::new(FirstDeltaRecorder {
                    first: first_clone.clone(),
                }))
            }));

        rec.add_pointer(pointer_id(7), Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.close(pointer_id(7));
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
            "expected first update to carry ≥slop delta, got {recorded:?}"
        );
    }

    #[test]
    fn up_after_acceptance_fires_end() {
        let arena = crate::arena::GestureArena::new();
        let ends = Arc::new(AtomicUsize::new(0));
        let ends_clone = ends.clone();
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |_pointer, _pos| {
                Some(Box::new(CountingHandle {
                    updates: Arc::new(AtomicUsize::new(0)),
                    ends: ends_clone.clone(),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            }));

        rec.add_pointer(pointer_id(3), Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.close(pointer_id(3));
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
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |_pointer, _pos| {
                Some(Box::new(CountingHandle {
                    updates: Arc::new(AtomicUsize::new(0)),
                    ends: ends_clone.clone(),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            }));

        rec.add_pointer(pointer_id(4), Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.add(
            pointer_id(4),
            Arc::new(RejectableMember {
                rejected: Arc::new(Mutex::new(false)),
            }),
        );
        arena.close(pointer_id(4));
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
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Horizontal)
            .with_on_start(Rc::new(move |_pointer, _pos| {
                Some(Box::new(CountingHandle {
                    updates: updates_clone.clone(),
                    ends: Arc::new(AtomicUsize::new(0)),
                    cancels: Arc::new(AtomicUsize::new(0)),
                }))
            }));

        rec.add_pointer(pointer_id(5), Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.add(
            pointer_id(5),
            Arc::new(RejectableMember {
                rejected: Arc::new(Mutex::new(false)),
            }),
        );
        arena.close(pointer_id(5));
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
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |pointer, _pos| {
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
            }));

        rec.add_pointer(pointer_id(7), Offset::new(Pixels(0.0), Pixels(0.0)));
        rec.add_pointer(pointer_id(8), Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.close(pointer_id(7));
        arena.close(pointer_id(8));
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
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |_pointer, _pos| {
                Some(Box::new(CountingHandle {
                    updates: Arc::new(AtomicUsize::new(0)),
                    ends: Arc::new(AtomicUsize::new(0)),
                    cancels: cancels_clone.clone(),
                }))
            }));

        rec.add_pointer(pointer_id(9), Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.close(pointer_id(9));
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
    fn dispose_releases_pending_arena_entries() {
        let arena = crate::arena::GestureArena::new();
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free);
        let competitor = Arc::new(AcceptingMember::default());
        let pointer = pointer_id(10);

        rec.add_pointer(pointer, Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.add(pointer, competitor.clone());
        arena.close(pointer);

        rec.dispose();
        arena.drain_deferred_resolutions();

        assert_eq!(
            competitor.accepts.load(Ordering::SeqCst),
            1,
            "disposing a pending multi-drag must let its competitor resolve"
        );
        assert!(
            arena.is_empty(),
            "dispose must not leave a held arena generation behind"
        );
    }

    #[test]
    fn dispose_finishes_every_pointer_before_resuming_a_cancel_panic() {
        let arena = crate::arena::GestureArena::new();
        let later_cancels = Arc::new(AtomicUsize::new(0));
        let later_cancels_for_callback = later_cancels.clone();
        let first_pointer = pointer_id(12);
        let second_pointer = pointer_id(13);
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(move |pointer, _position| {
                if pointer == first_pointer {
                    Some(Box::new(PanickingCancelHandle) as _)
                } else {
                    Some(Box::new(counting_handle(later_cancels_for_callback.clone())) as _)
                }
            }));

        for pointer in [first_pointer, second_pointer] {
            rec.add_pointer(pointer, Offset::new(Pixels(0.0), Pixels(0.0)));
            arena.close(pointer);
            rec.handle_event(&make_move_event_for_id(
                pointer,
                Offset::new(Pixels(25.0), Pixels(0.0)),
                PointerType::Touch,
            ));
        }

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rec.dispose()));

        assert!(unwind.is_err(), "the first user panic must still propagate");
        assert_eq!(
            later_cancels.load(Ordering::SeqCst),
            1,
            "a hostile client must not starve later pointer cleanup"
        );
        assert_eq!(rec.tracked_pointer_count(), 0);
        assert!(arena.is_empty());
    }

    #[test]
    fn on_start_returning_none_drops_pointer() {
        // User callback rejects the drag — pointer state is removed
        // silently, no further updates flow to it.
        let arena = crate::arena::GestureArena::new();
        let rec = MultiDragGestureRecognizer::new(arena.clone(), MultiDragAxis::Free)
            .with_on_start(Rc::new(|_pointer, _pos| None));
        rec.add_pointer(pointer_id(11), Offset::new(Pixels(0.0), Pixels(0.0)));
        arena.close(pointer_id(11));
        rec.handle_event(&make_move_event_for_id(
            pointer_id(11),
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        // After rejection, the pointer state is removed.
        assert_eq!(rec.tracked_pointer_count(), 0);
    }
}
