//! Base traits and types for gesture recognizers
//!
//! Defines the core `GestureRecognizer` trait and common types used by all
//! recognizers.

use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;
use tracing::instrument;
use web_time::{Duration, Instant};

use crate::{
    arena::{GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition},
    events::{PointerButton, PointerEvent},
    ids::PointerId,
    retain::Retain,
    routing::{PointerDispatch, RoutePanic},
    traits::PointerEventExtTrait,
};

/// Whether a `Down` presses the primary button.
///
/// A `Down` that carries no button (touch and pen contacts may not) counts as
/// primary. Any other event answers `false`.
pub(crate) fn is_primary_down(event: &PointerEvent) -> bool {
    matches!(event, PointerEvent::Down(data)
        if data.button.is_none_or(|button| button == PointerButton::Primary))
}

/// The event's own timestamp in nanoseconds, if it carries one.
///
/// The wire stamps `0` when it has no time (a synthetic event); that
/// convention ends here.
pub(crate) fn event_time(event: &PointerEvent) -> Option<u64> {
    let nanos = match event {
        PointerEvent::Down(data) | PointerEvent::Up(data) => data.state.time,
        PointerEvent::Move(data) => data.current.time,
        PointerEvent::Scroll(data) => data.state.time,
        PointerEvent::Gesture(data) => data.state.time,
        PointerEvent::Cancel(_) | PointerEvent::Enter(_) | PointerEvent::Leave(_) => 0,
    };
    (nanos != 0).then_some(nanos)
}

/// Places a pointer sequence's event timestamps on the arena clock.
///
/// Velocity is a ratio of distance to the time between samples, so samples
/// must be stamped when the device produced them, not when dispatch got to
/// them: events that queued up behind one frame are dispatched back to back.
/// The first stamped event of a sequence anchors its hardware time to the
/// arena clock's reading at dispatch; every later event lands at the anchor
/// plus its own hardware offset. An event without a timestamp is stamped at
/// dispatch. Every returned instant is at least the previous one, so a
/// sequence that mixes stamped and unstamped events never runs backwards;
/// when a stamped event would land before an unstamped one, the hardware
/// timeline is re-anchored there, so the stamped events after it keep their
/// own spacing instead of collapsing onto one instant.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EventTimeline {
    anchor: Option<(u64, Instant)>,
    last: Option<Instant>,
}

impl EventTimeline {
    /// The arena-clock instant at which an event stamped `event_nanos` happened,
    /// given the arena clock reads `now` at dispatch.
    pub(crate) fn instant(&mut self, event_nanos: Option<u64>, now: Instant) -> Instant {
        let raw = match (event_nanos, self.anchor) {
            (None, _) => now,
            (Some(event_nanos), None) => {
                self.anchor = Some((event_nanos, now));
                now
            }
            (Some(event_nanos), Some((anchor_nanos, anchor))) => {
                let offset = Duration::from_nanos(event_nanos.saturating_sub(anchor_nanos));
                anchor.checked_add(offset).unwrap_or(now)
            }
        };
        let instant = match (self.last, event_nanos) {
            (Some(last), Some(event_nanos)) if raw < last => {
                // An unstamped event ran ahead of the hardware timeline: carry the
                // stamped events on from it.
                self.anchor = Some((event_nanos, last));
                last
            }
            (Some(last), _) => raw.max(last),
            (None, _) => raw,
        };
        self.last = Some(instant);
        instant
    }
}

/// Drop one outgoing user callback, preserving the first failure.
///
/// Once a failure is recorded, or while the thread is already unwinding, the
/// capture is retained rather than destroyed: a second panic from its
/// destructor would replace the first failure or abort.
pub(crate) fn retire_callback<T: ?Sized>(callback: Option<Rc<T>>, first: &mut Option<RoutePanic>) {
    if first.is_some() || std::thread::panicking() {
        callback.retain();
    } else {
        RoutePanic::preserve_first(
            first,
            RoutePanic::capture(|| drop(callback)),
            "recognizer callback retirement",
        );
    }
}

/// Several user callbacks that one transition fires back to back.
///
/// Each callback runs even when an earlier one panicked: they report the same
/// committed transition, and a consumer listening on a later one must not miss
/// it because another listener failed. Each capture retires after its call;
/// [`finish`](Self::finish) resumes the first failure. Entered while the
/// thread is already unwinding, no callback runs and every capture is
/// retained.
pub(crate) struct CallbackSequence {
    first: Option<RoutePanic>,
    incoming_failure: bool,
}

impl CallbackSequence {
    pub(crate) fn new() -> Self {
        Self {
            first: None,
            incoming_failure: std::thread::panicking(),
        }
    }

    /// Invoke `callback` (if any) and retire its capture.
    pub(crate) fn call<T: ?Sized>(&mut self, callback: Option<Rc<T>>, invoke: impl FnOnce(&T)) {
        if self.incoming_failure {
            callback.retain();
            return;
        }
        if let Some(callback) = callback.as_ref() {
            let candidate = RoutePanic::capture(|| invoke(callback.as_ref()));
            RoutePanic::preserve_first(&mut self.first, candidate, "recognizer callback");
        }
        retire_callback(callback, &mut self.first);
    }

    /// Retire one capture without invoking it, preserving the first failure:
    /// disposal drops each capture on its own, so two panicking destructors
    /// never unwind at once.
    pub(crate) fn retire<T: ?Sized>(&mut self, callback: Option<Rc<T>>) {
        if self.incoming_failure {
            callback.retain();
        } else {
            retire_callback(callback, &mut self.first);
        }
    }

    /// Resume the first failure, if any.
    pub(crate) fn finish(self) {
        if let Some(panic) = self.first {
            if self.incoming_failure {
                panic.retain();
            } else {
                panic.resume();
            }
        }
    }
}

/// Resume the first captured panic, or retain it when the thread was already
/// unwinding before the containment began.
pub(crate) fn finish_containment(first: Option<RoutePanic>, incoming_failure: bool) {
    if let Some(panic) = first {
        if incoming_failure {
            panic.retain();
        } else {
            panic.resume();
        }
    }
}

/// Give up `entry` after its contact was cancelled.
///
/// In a self-driven arena the recognizer closes the generation itself, and a
/// cancelled contact has no winner: the generation is abandoned rather than
/// swept, since a sweep awards it to the first remaining member as a pointer
/// up does. In a binding-driven arena the entry is rejected; the binding
/// abandons the sequence on the cancel.
pub(crate) fn withdraw_cancelled(entry: &GestureArenaEntry, arena: &GestureArena) {
    if arena.sweep_model() == crate::arena::SweepModel::SelfDriven {
        entry.abandon();
    } else {
        entry.resolve(GestureDisposition::Rejected);
    }
}

/// Run `before` (the recognizer's own state commit), then the user callback.
///
/// The callback runs only when `before` completed. Its capture is retired
/// afterwards and the first failure resumes once everything is settled; a
/// failure arriving while the thread is already unwinding is retained.
pub(crate) fn invoke_callback<T: ?Sized>(
    callback: Option<Rc<T>>,
    before: impl FnOnce(),
    invoke: impl FnOnce(&T),
) {
    let incoming_failure = std::thread::panicking();
    // The opaque capture owner stays outside the catch around its body.
    let mut first = RoutePanic::capture(before);
    if first.is_none()
        && !incoming_failure
        && let Some(callback) = callback.as_ref()
    {
        first = RoutePanic::capture(|| invoke(callback.as_ref()));
    }
    retire_callback(callback, &mut first);
    if let Some(panic) = first {
        if incoming_failure {
            panic.retain();
        } else {
            panic.resume();
        }
    }
}

/// Base trait for all gesture recognizers
///
/// A gesture recognizer detects specific gestures (tap, drag, scale, etc.) from
/// pointer events and calls user-provided callbacks.
///
/// # Lifecycle
///
/// 1. `add_pointer()` - Recognizer starts tracking a new pointer
/// 2. `handle_event()` - Process pointer events (move, up, cancel)
/// 3. Win/lose in gesture arena
/// 4. Call user callbacks on successful recognition
/// 5. `dispose()` - Clean up when done
pub trait GestureRecognizer: GestureArenaMember {
    /// Add a new pointer to track
    ///
    /// Called when a pointer goes down. The recognizer should add itself to the
    /// gesture arena if it wants to compete for this pointer.
    ///
    /// The receiver is the owning [`Arc`] so the exact recognizer identity is
    /// registered in the arena. Manufacturing an `Arc` from a cloned struct
    /// creates a different allocation, makes weak entry handles go stale as
    /// soon as the arena resolves, and cannot support post-resolution timers.
    /// `position` is in the recognizer's own space — the one its slop,
    /// velocity and axis maths are measured in. `global_position` is the same
    /// contact in the root's space, carried alongside because a recognizer
    /// cannot recover it: the event it is handed has already been localised.
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
    );

    /// Admit a pointer from the `Down` dispatch that started it.
    ///
    /// The event-carrying form of [`add_pointer`](Self::add_pointer), and the
    /// one a caller holding the `Down` should use: the device kind (which
    /// selects the slop tier) and the pressed button exist only on the event.
    /// A recognizer that answers only some buttons — drag and long press
    /// admit the primary button, tap its primary, secondary and tertiary
    /// families — leaves a `Down` it does not admit out of the arena
    /// entirely, so a right-button press neither scrolls a list nor fires a
    /// long press. A `Down` without button information (touch, pen) counts as
    /// the primary button. Events other than `Down` are ignored.
    ///
    /// The default filters nothing: it admits every `Down`, whatever its
    /// button, by forwarding the dispatch's position pair to `add_pointer`.
    /// The button and kind rules above belong to the recognizers that
    /// override it.
    fn add_pointer_down(self: &Arc<Self>, dispatch: PointerDispatch<'_>) {
        if let PointerEvent::Down(_) = dispatch.local {
            self.add_pointer(
                dispatch.local.pointer_id(),
                dispatch.local.position(),
                dispatch.global.position(),
            );
        }
    }

    /// Handle a pointer event.
    ///
    /// Process move, up, and cancel events for tracked pointers.
    ///
    /// Takes the whole [`PointerDispatch`] rather than one event. Dispatch
    /// localises the event for the receiving target and keeps the untransformed
    /// one beside it; handing a recognizer only the local half is what made
    /// every detail struct's `global_position` a local position under a new
    /// name (issue #908). Everything a recognizer measures against its own box
    /// comes from `dispatch.local`; `dispatch.global` exists to be reported,
    /// not measured with.
    fn handle_event(&self, dispatch: PointerDispatch<'_>);

    /// Dispose of this recognizer
    ///
    /// Clean up resources and clear callbacks. Called when the recognizer is
    /// no longer needed.
    fn dispose(&self);

    /// Get the primary pointer being tracked (if any)
    fn primary_pointer(&self) -> Option<PointerId>;
}

/// Base composition data for gesture recognizers.
///
/// Provides common functionality that all recognizers need:
/// - Arena membership
/// - Primary pointer tracking
/// - Initial position tracking
/// - Disposal
///
/// Named `RecognizerBase` to free `GestureRecognizerState` for the
/// recognizer FSM enum (see below).
#[derive(Clone)]
pub struct RecognizerBase {
    /// Gesture arena for conflict resolution
    arena: GestureArena,

    /// Primary pointer ID being tracked, as a raw `u64` (`0` == none).
    ///
    /// Read on every event (the per-pointer filter), so it is a lock-free
    /// `AtomicU64` rather than `Mutex<Option<PointerId>>`. `PointerId` is
    /// `NonZeroU64`-backed, so `0` is an unambiguous "none" sentinel.
    primary_pointer: Arc<AtomicU64>,

    /// Where the primary pointer went down, in BOTH spaces.
    ///
    /// One field rather than two, because the two must agree about which
    /// contact they describe and a caller that could set them apart would
    /// eventually do so. That is not hypothetical: the cancel paths fall back
    /// to the stored LOCAL position because a `Cancel` event carries none, and
    /// the global half was left reading the event's own `Offset::ZERO` until
    /// they became one value.
    initial_contact: Arc<Mutex<Option<InitialContact>>>,

    /// Whether recognizer has been disposed. Checked on every event via
    /// `assert_not_disposed`, so a lock-free `AtomicBool`.
    disposed: Arc<AtomicBool>,

    /// Stale-safe handle to the exact member and arena generation registered
    /// by [`start_tracking`](Self::start_tracking). The handle stores weak
    /// identities, so retaining it here cannot form a recognizer cycle.
    tracked_entry: Arc<Mutex<Option<GestureArenaEntry>>>,

    /// Bumped by every [`start_tracking`](Self::start_tracking). A contact's
    /// retirement withdraws tracking only while this still names that contact,
    /// so a contact admitted reentrantly during the retiring sweep survives it.
    contact: Arc<AtomicU64>,
}

/// Withdraws a retiring contact's local tracking when dropped, unless a newer
/// contact was admitted since. Runs on unwind too; it touches only framework
/// state, never user code.
struct WithdrawContact<'a> {
    base: &'a RecognizerBase,
    contact: u64,
}

impl Drop for WithdrawContact<'_> {
    fn drop(&mut self) {
        if self.base.contact.load(Ordering::Acquire) == self.contact {
            let retired = self.base.tracked_entry.lock().take();
            self.base.set_primary_pointer(None);
            self.base.clear_initial_contact();
            drop(retired);
        }
    }
}

/// Where a gesture's primary pointer went down, in both coordinate spaces.
///
/// Recorded as one value by [`RecognizerBase::start_tracking`] and never
/// written apart, so the two halves always describe the same contact.
#[derive(Debug, Clone, Copy)]
struct InitialContact {
    /// The receiving node's space — what a `Move`/`Up` event carries after
    /// hit-test dispatch has rewritten it.
    local: Offset<f64>,
    /// The root's space — what dispatch rewrote away.
    global: Offset<f64>,
}

impl RecognizerBase {
    /// Create new recognizer base data with arena
    pub fn new(arena: GestureArena) -> Self {
        Self {
            arena,
            primary_pointer: Arc::new(AtomicU64::new(0)),
            initial_contact: Arc::new(Mutex::new(None)),
            disposed: Arc::new(AtomicBool::new(false)),
            tracked_entry: Arc::new(Mutex::new(None)),
            contact: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Get the gesture arena
    #[inline]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }

    /// The current instant on the arena's clock — the time a deadline-driven
    /// recognizer compares its captured down-time against. Reads the OS clock in
    /// production; a headless frame driver's virtual clock in tests, so a
    /// deadline elapses deterministically without a wall-clock sleep.
    #[inline]
    pub fn now(&self) -> web_time::Instant {
        self.arena.now()
    }

    /// Get the primary pointer ID (if tracking one)
    #[inline]
    pub fn primary_pointer(&self) -> Option<PointerId> {
        // `PointerId::new` is `0 -> None`, so it round-trips the sentinel.
        PointerId::new(self.primary_pointer.load(Ordering::Relaxed))
    }

    /// Set the primary pointer
    pub fn set_primary_pointer(&self, pointer: Option<PointerId>) {
        let raw = pointer.map_or(0, |id| id.get_inner().get());
        self.primary_pointer.store(raw, Ordering::Relaxed);
    }

    /// Where the primary pointer went down, in the receiving node's space.
    #[inline]
    pub fn initial_position(&self) -> Option<Offset<f64>> {
        self.initial_contact.lock().map(|c| c.local)
    }

    /// Where the primary pointer went down, in the root's space.
    ///
    /// The half a `Cancel` cannot supply: dispatch rewrites an event into the
    /// receiving node's coordinates, and a cancel carries no position at all,
    /// so a recogniser reporting a cancelled gesture has nowhere else to read
    /// an untransformed position from.
    #[inline]
    pub fn initial_global_position(&self) -> Option<Offset<f64>> {
        self.initial_contact.lock().map(|c| c.global)
    }

    /// Forget the recorded contact. Set only by
    /// [`start_tracking`](Self::start_tracking), so the pair cannot drift.
    pub fn clear_initial_contact(&self) {
        *self.initial_contact.lock() = None;
    }

    /// Check if recognizer has been disposed
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::Relaxed)
    }

    /// Mark as disposed
    pub fn mark_disposed(&self) {
        self.disposed.store(true, Ordering::Relaxed);
    }

    /// Debug-assert that the recognizer has not been disposed.
    ///
    /// Adopts the `ChangeNotifier::dispose` lifecycle pattern at
    /// [`crates/flui-foundation/src/notifier.rs`](../../crates/flui-foundation/src/notifier.rs)
    /// — use-after-dispose triggers a `debug_assert!` panic in debug
    /// builds + `tracing::warn!` + no-op semantics in release.
    ///
    /// Returns `true` if the recognizer is still live (call sites should
    /// proceed); `false` if disposed (call sites should early-return).
    #[instrument(
        name = "recognizer.assert_not_disposed",
        level = "trace",
        skip(self),
        fields(op = %op, primary = ?self.primary_pointer())
    )]
    #[inline]
    pub fn assert_not_disposed(&self, op: &'static str) -> bool {
        if self.is_disposed() {
            debug_assert!(
                false,
                "Recognizer::{op} called after dispose (use-after-dispose)"
            );
            tracing::warn!(op, "Recognizer used after dispose");
            return false;
        }
        true
    }

    /// Which contact is tracked: bumped by every [`start_tracking`](Self::start_tracking).
    /// A caller compares it around user code to tell whether that code admitted
    /// a contact of its own, even on the same pointer id.
    pub(crate) fn contact_generation(&self) -> u64 {
        self.contact.load(Ordering::Acquire)
    }

    /// Start tracking a pointer
    ///
    /// Sets this as the primary pointer and stores initial position.
    /// Adds recognizer to gesture arena.
    #[instrument(
        name = "recognizer.start_tracking",
        level = "debug",
        skip(self, recognizer),
        fields(
            pointer = ?pointer,
            position = ?position,
            event = %crate::observability::GestureEvent::StartedTracking,
        )
    )]
    pub fn start_tracking<T: GestureArenaMember + Clone + 'static>(
        &self,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
        recognizer: &Arc<T>,
    ) {
        if self.is_disposed() {
            return;
        }

        self.contact.fetch_add(1, Ordering::AcqRel);
        self.set_primary_pointer(Some(pointer));
        *self.initial_contact.lock() = Some(InitialContact {
            local: position,
            global: global_position,
        });

        // Register with the arena and retain the exact slot/member identity.
        let member: Arc<dyn GestureArenaMember> = recognizer.clone();
        let entry = self.arena.add(pointer, member);
        *self.tracked_entry.lock() = Some(entry);
    }

    /// Claim the arena win for the currently-tracked pointer.
    ///
    /// Resolves the arena in favour of this recognizer using the stable member
    /// identity captured in [`start_tracking`](Self::start_tracking), so
    /// competing members receive `reject_gesture`. No-op when not tracking a
    /// pointer or when the arena entry is already resolved or gone.
    pub fn accept_tracked(&self) {
        let entry = self.tracked_entry.lock().clone();
        if let Some(entry) = entry {
            entry.resolve(GestureDisposition::Accepted);
        }
    }

    /// The exact `Arc<dyn GestureArenaMember>` this recognizer registered with
    /// the arena in [`start_tracking`](Self::start_tracking), upgraded from the
    /// stored entry handle.
    ///
    /// Returns `None` once the recognizer is no longer tracking (the member was
    /// dropped). Used by the double-tap lifecycle to resolve the first contact's
    /// entry in favour of the double-tap.
    #[inline]
    pub fn tracked_member(&self) -> Option<Arc<dyn GestureArenaMember>> {
        self.tracked_entry
            .lock()
            .as_ref()
            .and_then(GestureArenaEntry::member)
    }

    /// The exact stale-safe entry registered for the tracked pointer.
    pub fn tracked_entry(&self) -> Option<GestureArenaEntry> {
        self.tracked_entry.lock().clone()
    }

    /// Stop tracking (called on success or rejection)
    ///
    /// The retiring contact stays visible through a self-driven arena sweep,
    /// so a member the sweep accepts still recognizes its own pointer. Local
    /// tracking is withdrawn after the sweep, also when it unwinds, unless the
    /// sweep admitted the next contact reentrantly: that contact is not
    /// cleared by completion of this one. Diagnostics run afterward.
    pub fn stop_tracking(&self) {
        let pointer = self.primary_pointer();
        let entry = self.tracked_entry.lock().clone();
        let withdraw = WithdrawContact {
            base: self,
            contact: self.contact.load(Ordering::Acquire),
        };
        // Sweep only when this recognizer owns the arena lifecycle. In a
        // binding-driven arena the binding sweeps on `PointerUp` after routing
        // the event to the whole hit-test path; a recognizer self-sweeping here
        // would force-resolve a shared entry to the front member before a
        // double-tap (or a peer detector) could complete. Local tracking is
        // cleared in both modes.
        if pointer.is_some()
            && self.arena.sweep_model() == crate::arena::SweepModel::SelfDriven
            && let Some(entry) = entry
        {
            entry.sweep();
        }
        drop(withdraw);
        if pointer.is_some() {
            tracing::debug!(
                name: "recognizer.stop_tracking",
                ?pointer,
                event = %crate::observability::GestureEvent::StoppedTracking,
                "recognizer stopped tracking"
            );
        }
    }

    /// Reject this gesture (lose the arena or explicit rejection)
    ///
    /// Local tracking and arena membership are withdrawn before diagnostics.
    /// A subscriber can fail or reenter while recording the event without
    /// leaving this recognizer admitted under its retired contact.
    pub fn reject(&self) {
        let pointer = self.primary_pointer();
        // Withdraw ONLY this recognizer from the arena, using the stable member
        // identity captured in `start_tracking`. Resolving the whole entry with
        // no winner (the previous behavior) rejected every *competing* member
        // too, so a recognizer that bowed out of a shared arena (e.g. a tap
        // exceeding its slop) silently killed the drag it was competing with.
        let entry = self.tracked_entry.lock().take();
        // Clear ONLY this recognizer's local tracking — do NOT `stop_tracking`
        // (it would `sweep`, force-resolving any still-open competition in
        // first-member-wins fashion mid-gesture). Tear the shared entry down
        // only once it has actually settled (a winner emerged, or no members
        // remain); a competition that still has rivals must keep running until
        // one accepts or the pointer lifts.
        self.set_primary_pointer(None);
        self.clear_initial_contact();
        if let Some(entry) = entry {
            entry.resolve(GestureDisposition::Rejected);
        }
        if pointer.is_some() {
            // Subscriber callbacks are arbitrary user code. The complete
            // rejection transaction precedes the event.
            tracing::debug!(
                name: "recognizer.reject",
                ?pointer,
                event = %crate::observability::GestureEvent::ArenaRejected,
                "recognizer rejected"
            );
        }
    }
}

impl std::fmt::Debug for RecognizerBase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecognizerBase")
            .field("primary_pointer", &self.primary_pointer())
            .field("initial_position", &self.initial_position())
            .field("disposed", &self.is_disposed())
            .finish()
    }
}

/// Canonical gesture recognizer FSM state.
///
/// A recognizer cycles `Ready` → `Possible` → (`Ready` | `Defunct`) and stays
/// in `Defunct` until all tracked pointers are removed, at which point it
/// returns to `Ready` for the next sequence.
///
/// Concrete recognizers typically retain a richer private FSM (e.g. Tap's
/// down-then-up timing, DoubleTap's between-tap window) but expose progress
/// through this canonical 3-state enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GestureRecognizerState {
    /// The recognizer is ready to start recognizing a gesture.
    #[default]
    Ready,

    /// The sequence of pointer events seen thus far is consistent with the
    /// gesture this recognizer is attempting to recognize but the gesture has
    /// not been accepted definitively.
    Possible,

    /// Further pointer events cannot cause this recognizer to recognize the
    /// gesture until the recognizer returns to [`Self::Ready`] (typically when all
    /// tracked pointers are removed).
    Defunct,
}

/// Constants for gesture recognition
pub mod constants {
    /// Maximum distance in pixels a pointer can move before it's no longer
    /// considered a tap
    pub const TAP_SLOP: f64 = 18.0;

    /// Maximum distance between two taps for double-tap
    pub const DOUBLE_TAP_SLOP: f64 = 100.0;

    /// Maximum time between taps for double-tap (milliseconds)
    pub const DOUBLE_TAP_TIMEOUT_MS: u64 = 300;

    /// Minimum duration for long press (milliseconds)
    pub const LONG_PRESS_DURATION_MS: u64 = 500;

    /// Minimum distance to start a drag
    pub const DRAG_SLOP: f64 = 18.0;

    /// Minimum distance to start a pan
    pub const PAN_SLOP: f64 = 18.0;

    /// Minimum distance between two pointers to start scale
    pub const SCALE_SLOP: f64 = 18.0;

    /// Minimum velocity to trigger fling (pixels per second)
    pub const MIN_FLING_VELOCITY: f64 = 50.0;

    /// Minimum distance for fling
    pub const MIN_FLING_DISTANCE: f64 = 50.0;
}

#[cfg(test)]
mod event_timeline_tests {
    use super::EventTimeline;
    use std::time::{Duration, Instant};

    /// A stamped event whose hardware offset lands before an unstamped event
    /// dispatched in between is clamped to it: the timeline never runs
    /// backwards, so velocity never sees a reversed gap.
    #[test]
    fn mixed_stamped_and_unstamped_events_stay_monotonic() {
        let start = Instant::now();
        let mut timeline = EventTimeline::default();
        assert_eq!(timeline.instant(Some(0), start), start);
        let unstamped = timeline.instant(None, start + Duration::from_millis(100));
        let stamped = timeline.instant(Some(10_000_000), start + Duration::from_millis(101));
        assert!(
            stamped >= unstamped,
            "{stamped:?} ran back before {unstamped:?}"
        );
        // The stamped events after it keep their own 10 ms spacing.
        let next = timeline.instant(Some(20_000_000), start + Duration::from_millis(102));
        assert_eq!(
            next - stamped,
            Duration::from_millis(10),
            "the timeline did not collapse"
        );
    }
}
