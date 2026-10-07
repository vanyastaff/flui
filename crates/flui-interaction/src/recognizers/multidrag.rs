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
//! The recogniser:
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
//! use flui_foundation::geometry::Offset;
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

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use web_time::Instant;

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::recognizer::{EventTimeline, GestureRecognizer, RecognizerBase, event_time};
use crate::{
    arena::{GestureArenaEntry, GestureArenaMember, GestureDisposition},
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    routing::PointerDispatch,
    routing::RoutePanic,
    settings::GestureSettings,
    traits::PointerEventExtTrait,
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
    Rc<dyn Fn(PointerId, Offset<f64>) -> Option<Box<dyn MultiDragHandle>>>; // per-pointer handle trait; ≤3 workspace sites, marker preferred over allowlist promotion.

/// Per-pointer state.
///
/// State lifecycle: `Possible` (added) → `Accepted` (arena victory) → `Ended` /
/// `Cancelled` (terminal). The client is installed only after `on_start`
/// returns; pending movement is retained until then.
struct MultiDragPointerState {
    /// Global position where this contact began.
    initial_position: Offset<f64>,
    /// The same contact as `initial_position`, in the root's space — stored
    /// because dispatch localises the event before this recognizer sees it
    /// (issue #908).
    initial_global_position: Offset<f64>,
    /// The most recent contact position in the root's space.
    last_global_position: Offset<f64>,
    /// Last reported position (for delta computation).
    last_position: Offset<f64>,
    /// Pointer device kind (slop, velocity-tracker flavour).
    kind: PointerType,
    /// Slop threshold for this pointer kind.
    slop: f64,
    /// Accumulated delta while `pending` (pre-acceptance).
    pending_delta: Offset<f64>,
    /// `true` once the arena has accepted this pointer.
    accepted: bool,
    /// User's handle, populated after `accepted`.
    client: Option<Rc<dyn MultiDragHandle>>, // owner-local per-pointer drag client.
    /// Velocity tracker fed while `pending` and after `accepted`.
    velocity_tracker: VelocityTracker,
    /// Places this contact's event timestamps on the arena clock, so velocity
    /// samples are spaced by when the device produced them.
    timeline: EventTimeline,
    /// Timestamp of the most recent movement accumulated before acceptance.
    last_pending_timestamp: Option<Instant>,
    /// Stale-safe handle to the exact arena generation and member registered
    /// for this contact. It holds only weak references, so storing it beside
    /// recognizer state cannot create a cycle.
    arena_entry: Option<GestureArenaEntry>,
}

impl MultiDragPointerState {
    fn new(
        initial_position: Offset<f64>,
        initial_global_position: Offset<f64>,
        kind: PointerType,
        slop: f64,
    ) -> Self {
        Self {
            initial_position,
            initial_global_position,
            last_global_position: initial_global_position,
            last_position: initial_position,
            kind,
            slop,
            pending_delta: Offset::new(0.0, 0.0),
            accepted: false,
            client: None,
            velocity_tracker: VelocityTracker::new(),
            timeline: EventTimeline::default(),
            last_pending_timestamp: None,
            arena_entry: None,
        }
    }

    /// Per-axis primary slop test — accepts when |delta.{axis}| exceeds slop.
    fn check_for_resolution_after_move(&mut self, axis: MultiDragAxis) -> bool {
        let magnitude = match axis {
            MultiDragAxis::Free => self.pending_delta.distance(),
            MultiDragAxis::Horizontal => self.pending_delta.dx.abs(),
            MultiDragAxis::Vertical => self.pending_delta.dy.abs(),
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
    pub global_position: Offset<f64>,
    /// Pointer's current local position (same as `global_position` for the
    /// multi-pointer recogniser; user code can transform).
    pub local_position: Offset<f64>,
    /// Delta since the last `update` (or, for the first update, the
    /// accumulated pending delta).
    pub delta: Offset<f64>,
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
    pub global_position: Offset<f64>,
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
    pub fn new(arena: crate::arena::GestureArena, axis: MultiDragAxis) -> Rc<Self> {
        Rc::new(Self {
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
    ) -> Rc<Self> {
        Rc::new(Self {
            state: RecognizerBase::new(arena),
            axis,
            pointers: Arc::new(Mutex::new(HashMap::new())),
            on_start: Rc::new(RefCell::new(None)),
            settings: Arc::new(Mutex::new(settings)),
        })
    }

    /// Set the per-pointer start callback. The callback may return `None` to
    /// reject the drag (caller can read pointer position to filter by region).
    pub fn with_on_start(self: Rc<Self>, callback: MultiDragStartCallback) -> Rc<Self> {
        let _prev = self.on_start.borrow_mut().replace(callback);
        self
    }

    /// Slop threshold for the given pointer kind.
    ///
    /// Every axis mode checks the plain hit slop for the pointer kind; none of
    /// them use the pan slop. Exactly the mouse kind is treated as "precise":
    /// it always gets the fixed precise-pointer hit slop, unconditionally, and
    /// `with_settings` customization has no effect on it. Every other kind
    /// resolves through the touch tier.
    ///
    /// Both arms live in [`GestureSettings::hit_slop`]; this method only picks
    /// the tier.
    ///
    /// That non-mouse arm reads [`GestureSettings::touch_slop`], not
    /// `pan_slop`: the hit slop resolves through the *touch* tier, and
    /// the two differ in the platform profiles
    /// ([`android_defaults`](GestureSettings::android_defaults) is 8 vs 16,
    /// [`ios_defaults`](GestureSettings::ios_defaults) 10 vs 20) even though
    /// they coincide at 18 under [`touch_defaults`](GestureSettings::touch_defaults).
    fn slop_for(&self, kind: PointerType) -> f64 {
        self.settings.lock().hit_slop(kind)
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
        self: &Rc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
        kind: PointerType,
    ) {
        let slop = self.slop_for(kind);
        let mut state = MultiDragPointerState::new(position, global_position, kind, slop);

        // Compete in the arena with a stable member identity. A lone immediate
        // multi-drag may win by default after Down;
        // delayed variants belong in a distinct recognizer policy, not an
        // arena-wide hold.
        let member: Rc<dyn GestureArenaMember> = Rc::<Self>::clone(self);
        let entry = self.state.arena().add_erased(pointer, &member);
        state.arena_entry = Some(entry);

        let _prev = self.pointers.lock().insert(pointer, state);
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
        position: Offset<f64>,
        global_position: Offset<f64>,
        kind: PointerType,
        stamp: Option<u64>,
    ) {
        let now = self.state.now();
        let (client, update, arena_entry) = {
            let mut map = self.pointers.lock();
            let Some(state) = map.get_mut(&pointer) else {
                return;
            };
            let timestamp = state.timeline.instant(stamp, now);
            let delta = (position - state.last_position).to_delta();
            state.last_position = position;
            state.last_global_position = global_position;
            state.kind = kind;
            // Recompute the threshold from the live event's kind, not the
            // kind captured at Down (`add_pointer` cannot report kind at
            // all — see `GestureRecognizer::add_pointer`'s signature — so
            // the pointer's real kind is only known from the first Move
            // event onward; `Self::slop_for` is recomputed on every check
            // rather than cached).
            state.slop = self.slop_for(kind);
            state.velocity_tracker.add_position(timestamp, position);

            if let Some(client) = state.client.clone() {
                let update = MultiDragUpdateDetails {
                    pointer_id: pointer,
                    global_position,
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
        let client: Rc<dyn MultiDragHandle> = Rc::from(handle); // owner-local per-pointer drag client returned by the public factory.

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
                global_position: state.initial_global_position,
                local_position: state.initial_position,
                delta: state.pending_delta,
                kind: state.kind,
                timestamp: state.last_pending_timestamp.unwrap_or_else(Instant::now),
            };
            state.pending_delta = Offset::new(0.0, 0.0);
            state.last_pending_timestamp = None;
            state.client = Some(Rc::clone(&client));
            update
        };

        // Store the client before its first callback, then call it last. A
        // re-entrant terminal event can now find and retire this exact drag.
        client.update(update);
    }

    /// Handle pointer up.
    fn handle_up(
        &self,
        pointer: PointerId,
        _position: Offset<f64>,
        global_position: Offset<f64>,
        _kind: PointerType,
        stamp: Option<u64>,
    ) {
        let Some(mut state) = self.remove_pointer(pointer) else {
            return;
        };
        // The up carries a fresher contact than the last move did, and a lift
        // with no move before it carries the ONLY one after the down. Recording
        // it here rather than reading `last_global_position` as it stands is
        // what keeps `MultiDragEndDetails` from reporting where the finger was
        // rather than where it left.
        state.last_global_position = global_position;
        let mut first_panic = Self::retire_arena_entry(state.arena_entry.as_ref());
        if let Some(client) = state.client.take() {
            // Read the velocity first (it borrows the tracker mutably to
            // memoize) before invoking the client.
            let now = state.timeline.instant(stamp, self.state.now());
            let velocity = state.velocity_tracker.velocity_at(now);
            let details = MultiDragEndDetails {
                pointer_id: pointer,
                global_position: state.last_global_position,
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
    fn add_pointer(
        self: &Rc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
    ) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "multidrag.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        self.add_pointer_impl(pointer, position, global_position, PointerType::Touch);
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
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
        // Position is `PhysicalPosition<f64>`; convert to Offset.
        let position = Offset::new(position.x, position.y);
        // The only point at which the untransformed position exists at all.
        let global_position = dispatch.global.position();
        match event {
            PointerEvent::Move(_) => {
                self.handle_move(pointer, position, global_position, kind, event_time(event));
            }
            PointerEvent::Up(_) => {
                self.handle_up(pointer, position, global_position, kind, event_time(event));
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Detach every pointer before arena or user callbacks can re-enter.
        // Retire all exact arena entries first, then notify accepted clients;
        // one hostile callback cannot strand another pointer's arena entry.
        let mut removed: Vec<(PointerId, MultiDragPointerState)> = {
            let mut pointers = self.pointers.lock();
            pointers.drain().collect()
        };
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
    use crate::events::make_move_event_for_id;

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

    struct PanickingCancelHandle;
    impl MultiDragHandle for PanickingCancelHandle {
        fn update(&self, _details: MultiDragUpdateDetails) {}
        fn end(&self, _details: MultiDragEndDetails) {}
        fn cancel(&self) {
            panic!("multi-drag cancel panic");
        }
    }

    fn pointer_id(n: u64) -> PointerId {
        PointerId::new(n).expect("nonzero pointer id")
    }

    // Returns the concrete fixture; callers box it at the `Option<Box<dyn …>>`
    // slot, so no `dyn` appears in this signature.
    fn counting_handle(cancels: Arc<AtomicUsize>) -> CountingHandle {
        CountingHandle {
            updates: Arc::new(AtomicUsize::new(0)),
            ends: Arc::new(AtomicUsize::new(0)),
            cancels,
        }
    }

    // Multi-drag recognizer matrix: independent pointers and dispose under a panicking cancel.
    #[test]
    fn multidrag_recognizer_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "each_pointer_gets_independent_drag",
                each_pointer_gets_independent_drag,
            ),
            (
                "dispose_finishes_every_pointer_before_resuming_a_cancel_panic",
                dispose_finishes_every_pointer_before_resuming_a_cancel_panic,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

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

        // Add two pointers.
        rec2.add_pointer(pointer_id(1), Offset::new(0.0, 0.0), Offset::new(0.0, 0.0));
        rec2.add_pointer(
            pointer_id(2),
            Offset::new(50.0, 50.0),
            Offset::new(50.0, 50.0),
        );
        arena.close(pointer_id(1));
        arena.close(pointer_id(2));

        // Pointer 1 makes one big move (0→25 = 25px > 18px slop) — fires the
        // pending-delta flush, which is the first update. The second move
        // is post-acceptance and fires a second update.
        rec2.handle_event(PointerDispatch::at_root(&make_move_event_for_id(
            pointer_id(1),
            Offset::new(25.0, 0.0),
            PointerType::Touch,
        )));
        rec2.handle_event(PointerDispatch::at_root(&make_move_event_for_id(
            pointer_id(1),
            Offset::new(30.0, 0.0),
            PointerType::Touch,
        )));
        // Pointer 2 also moves and accepts.
        rec2.handle_event(PointerDispatch::at_root(&make_move_event_for_id(
            pointer_id(2),
            Offset::new(70.0, 50.0),
            PointerType::Touch,
        )));

        // Pointer 1: pending-flush + one post-acceptance update = 2.
        // Pointer 2: pending-flush only = 1.
        assert_eq!(updates_p1.load(Ordering::SeqCst), 2);
        assert_eq!(updates_p2.load(Ordering::SeqCst), 1);
    }

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
            rec.add_pointer(pointer, Offset::new(0.0, 0.0), Offset::new(0.0, 0.0));
            arena.close(pointer);
            rec.handle_event(PointerDispatch::at_root(&make_move_event_for_id(
                pointer,
                Offset::new(25.0, 0.0),
                PointerType::Touch,
            )));
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
}
