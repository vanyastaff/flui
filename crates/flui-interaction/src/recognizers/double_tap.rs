//! Double tap gesture recognizer
//!
//! Recognizes double tap gestures (two taps in quick succession).
//!
//! A double tap is defined as:
//! - First tap completes (down + up within slop)
//! - Second tap starts within DOUBLE_TAP_TIMEOUT_MS (300ms)
//! - Second tap within DOUBLE_TAP_SLOP (100px) of first tap
//! - Second tap completes successfully

use std::{cell::RefCell, rc::Rc, sync::Arc};

use web_time::{Duration, Instant};

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::recognizer::{CallbackSequence, GestureRecognizer, RecognizerBase, is_primary_down};
use crate::{
    arena::{GestureArenaEntry, GestureArenaMember, GestureDisposition},
    events::{PointerEvent, PointerEventExt, PointerType},
    ids::PointerId,
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
};

/// Callback for double tap events
pub type DoubleTapCallback = Rc<dyn Fn(DoubleTapDetails)>;

/// Details about a double tap gesture
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleTapDetails {
    /// Global position where double tap occurred
    pub global_position: Offset<f64>,
    /// Local position (relative to widget)
    pub local_position: Offset<f64>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Recognizes double tap gestures
///
/// A double tap requires two taps within 300ms and 100px of each other.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let arena = GestureArena::new();
/// let recognizer = DoubleTapGestureRecognizer::new(arena)
///     .with_on_double_tap(|details| {
///         println!("Double tapped at {:?}", details.global_position);
///     });
///
/// // Handle pointer events
/// recognizer.add_pointer(pointer_id, position, position);
/// recognizer.handle_event(PointerDispatch::at_root(&pointer_event));
/// ```
#[derive(Clone)]
pub struct DoubleTapGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Callbacks
    callbacks: Rc<RefCell<DoubleTapCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<DoubleTapState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,

    /// Exact first-contact arena generation held across the inter-tap window.
    first_entry: Arc<Mutex<Option<GestureArenaEntry>>>,
}

// Field names keep the `on_double_tap`-style callback names.
#[expect(clippy::struct_field_names)]
#[derive(Default)]
struct DoubleTapCallbacks {
    on_double_tap: Option<DoubleTapCallback>,
    on_double_tap_down: Option<DoubleTapCallback>,
    on_double_tap_cancel: Option<DoubleTapCallback>,
}

impl DoubleTapCallbacks {
    /// Retire every capture one by one (see [`CallbackSequence::retire`]).
    fn retire(self, sequence: &mut CallbackSequence) {
        let Self {
            on_double_tap,
            on_double_tap_down,
            on_double_tap_cancel,
        } = self;
        sequence.retire(on_double_tap);
        sequence.retire(on_double_tap_down);
        sequence.retire(on_double_tap_cancel);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoubleTapPhase {
    /// Ready to start
    Ready,
    /// First tap down
    FirstDown,
    /// Waiting for second tap
    WaitingForSecond,
    /// Second tap down
    SecondDown,
    /// Cancelled
    Cancelled,
}

#[derive(Debug, Clone)]
struct DoubleTapState {
    /// Current phase
    phase: DoubleTapPhase,
    /// Position of first tap down
    first_tap_position: Option<Offset<f64>>,
    /// The same contact as `first_tap_position`, in the root's space —
    /// stored because dispatch localises the event before this recognizer
    /// sees it, so the global position exists only on arrival (issue #908).
    first_tap_global_position: Option<Offset<f64>>,
    /// Time of first tap completion
    first_tap_time: Option<Instant>,
    /// Current position (for slop detection)
    current_position: Option<Offset<f64>>,
    /// Device kind
    device_kind: Option<PointerType>,
}

impl Default for DoubleTapState {
    fn default() -> Self {
        Self {
            phase: DoubleTapPhase::Ready,
            first_tap_position: None,
            first_tap_global_position: None,
            first_tap_time: None,
            current_position: None,
            device_kind: None,
        }
    }
}

impl DoubleTapGestureRecognizer {
    /// Create a new double tap recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Rc<Self> {
        Rc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(DoubleTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(DoubleTapState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
            first_entry: Arc::new(Mutex::new(None)),
        })
    }

    /// Create a new double tap recognizer with custom settings
    pub fn with_settings(arena: crate::arena::GestureArena, settings: GestureSettings) -> Rc<Self> {
        Rc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(DoubleTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(DoubleTapState::default())),
            settings: Arc::new(Mutex::new(settings)),
            first_entry: Arc::new(Mutex::new(None)),
        })
    }

    /// Get the current gesture settings
    pub fn settings(&self) -> GestureSettings {
        self.settings.lock().clone()
    }

    /// Update gesture settings
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
    }

    /// Get the double tap timeout from settings
    fn double_tap_timeout(&self) -> Duration {
        self.settings.lock().double_tap_timeout()
    }

    /// Set the double tap callback
    pub fn with_on_double_tap(
        self: Rc<Self>,
        callback: impl Fn(DoubleTapDetails) + 'static,
    ) -> Rc<Self> {
        self.callbacks.borrow_mut().on_double_tap = Some(Rc::new(callback));
        self
    }

    /// Set the double-tap-DOWN callback — fires the instant the second
    /// contact goes down (validated by timing/slop against the first tap),
    /// not after it lifts. It exists for exactly
    /// the case `on_double_tap` cannot serve: a consumer (double-tap word
    /// selection, say) that wants the tap's position as soon as the
    /// gesture is confirmed, without waiting the extra down-to-up round
    /// trip `on_double_tap` needs to also confirm the tap didn't drag past
    /// slop or get cancelled. Both callbacks may fire for the same
    /// gesture: `on_double_tap_down` first, `on_double_tap` after, if the
    /// second contact lifts cleanly.
    pub fn with_on_double_tap_down(
        self: Rc<Self>,
        callback: impl Fn(DoubleTapDetails) + 'static,
    ) -> Rc<Self> {
        self.callbacks.borrow_mut().on_double_tap_down = Some(Rc::new(callback));
        self
    }

    /// Set the double tap cancel callback
    pub fn with_on_double_tap_cancel(
        self: Rc<Self>,
        callback: impl Fn(DoubleTapDetails) + 'static,
    ) -> Rc<Self> {
        self.callbacks.borrow_mut().on_double_tap_cancel = Some(Rc::new(callback));
        self
    }

    /// Handle pointer down
    fn handle_down(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        match state.phase {
            DoubleTapPhase::Ready => {
                // First tap down
                state.phase = DoubleTapPhase::FirstDown;
                state.first_tap_position = Some(position);
                state.first_tap_global_position = Some(global_position);
                state.current_position = Some(position);
                state.device_kind = Some(kind);
            }
            DoubleTapPhase::WaitingForSecond => {
                // Second tap down: validate timing and distance.
                //
                // When routed through `add_pointer`, expired-window and
                // out-of-slop contacts are intercepted before `start_tracking`
                // and never reach this point. These guards are a safety fallback
                // for direct `handle_down` callers (unit tests).
                let settings = self.settings.lock().clone();

                if let Some(first_time) = state.first_tap_time {
                    let elapsed = self.state.now().duration_since(first_time);

                    if elapsed > settings.double_tap_timeout() {
                        // Window expired. `add_pointer` handles this via
                        // `check_timeout()` before registration. Reaching here
                        // means a direct call: stay in WaitingForSecond and let
                        // the next `check_timeout()` poll clean up the hold.
                        // Must not release the hold here — we don't know whether
                        // `start_tracking` was already called for this contact.
                        return;
                    }

                    // Out-of-slop contact: ignore it, keep the first entry held,
                    // and stay in WaitingForSecond (out-of-slop contacts are
                    // filtered before they compete). Do NOT reset to FirstDown — that would
                    // orphan the held first entry on the next up.
                    if let Some(first_pos) = state.first_tap_position {
                        let distance = (position - first_pos).distance();
                        if distance > settings.double_tap_slop() {
                            return;
                        }
                    }

                    // Valid second tap down.
                    state.phase = DoubleTapPhase::SecondDown;
                    state.current_position = Some(position);
                    drop(state);

                    // Clone-then-drop-then-call: `state` above is already
                    // released before this runs, but the callback itself is
                    // arbitrary user code that may re-enter this recognizer
                    // (e.g. through the arena) — never call it while a lock
                    // this function took is still held.
                    let callback = self.callbacks.borrow().on_double_tap_down.clone();
                    if let Some(callback) = callback {
                        callback(DoubleTapDetails {
                            global_position,
                            local_position: position,
                            kind,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle pointer move
    fn handle_move(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        state.current_position = Some(position);

        // Slop detection: only act on FirstDown / SecondDown when the finger
        // has moved beyond the tap-slop tolerance.
        if matches!(
            state.phase,
            DoubleTapPhase::FirstDown | DoubleTapPhase::SecondDown
        ) && self.check_slop(position, kind)
        {
            // Moved too far, cancel. Release the lock and let
            // `handle_cancel` itself drive the phase transition -- it
            // already resets to `DoubleTapPhase::Ready` via
            // `DoubleTapState::default()` once its own guard
            // (`phase != Ready && phase != Cancelled`) passes. Setting
            // `Cancelled` here FIRST used to make that guard fail
            // immediately (phase already reads `Cancelled` by the time
            // `handle_cancel` checks it), skipping the reset entirely and
            // stranding the recognizer in `Cancelled` forever: every
            // later `handle_down` falls through the `Ready`/
            // `WaitingForSecond` match arms into `_ => {}`, so a contact
            // that drags past slop then lifts permanently disables
            // double-tap on that field until it remounts.
            drop(state);

            self.handle_cancel(position, global_position, kind);
        }
    }

    /// Handle pointer up
    fn handle_up(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        match state.phase {
            DoubleTapPhase::FirstDown => {
                // First tap completed — enter the inter-tap window.
                state.phase = DoubleTapPhase::WaitingForSecond;
                state.first_tap_time = Some(self.state.now());
                state.first_tap_position = Some(position);
                state.first_tap_global_position = Some(global_position);
                drop(state); // Release before touching the arena.

                // Capture the first contact and HOLD its arena entry across the
                // window (the arena `hold`).
                // The binding's first-up sweep then sees the entry held and
                // defers, so a competing front-member tap cannot win yet. Do
                // NOT stop tracking — the recognizer stays live for the second
                // contact.
                let first_entry = self.state.tracked_entry();
                if let Some(entry) = &first_entry {
                    entry.hold();
                }
                *self.first_entry.lock() = first_entry;
            }
            DoubleTapPhase::SecondDown => {
                // Second tap completed — a double tap. Every state change
                // commits before the user callback runs, so a callback that
                // panics or disposes leaves the recognizer ready for the next
                // gesture and no arena held.
                *state = DoubleTapState::default();
                drop(state);

                // Resolve BOTH contended entries in favour of the double-tap so
                // neither single tap fires (two `accepted` resolutions): the
                // held first entry wins for
                // its captured member (rejecting tap1), and the current entry
                // wins for the second contact's tracked member (rejecting tap2).
                // Resolving in *favour of* the double-tap — not `resolve(p,
                // None)` — is required: a no-winner resolve would reject the
                // double-tap's own member and spuriously fire `on_double_tap_cancel`.
                let first_entry = self.first_entry.lock().take();
                if let Some(entry) = &first_entry {
                    entry.resolve(GestureDisposition::Accepted);
                }
                self.state.accept_tracked();

                // Release the first entry's hold (drains any deferred sweep)
                // and retire the second contact.
                if let Some(entry) = first_entry {
                    entry.release();
                }
                self.state.stop_tracking();

                // Fire the double-tap callback once.
                let callback = self.callbacks.borrow().on_double_tap.clone();
                if let Some(callback) = callback {
                    callback(DoubleTapDetails {
                        global_position,
                        local_position: position,
                        kind,
                    });
                }
            }
            _ => {}
        }
    }

    /// Handle cancel
    fn handle_cancel(
        &self,
        position: Offset<f64>,
        global_position: Offset<f64>,
        kind: PointerType,
    ) {
        let mut state = self.gesture_state.lock();

        if state.phase != DoubleTapPhase::Ready && state.phase != DoubleTapPhase::Cancelled {
            let callback = self.callbacks.borrow().on_double_tap_cancel.clone();
            *state = DoubleTapState::default();
            drop(state);

            // The held first contact can differ from the currently tracked
            // second contact. Withdraw both exact entries before user code.
            let entry = self.first_entry.lock().take();
            if let Some(entry) = entry {
                entry.resolve(GestureDisposition::Rejected);
                entry.release();
            }
            self.state.reject();

            if let Some(callback) = callback {
                callback(DoubleTapDetails {
                    global_position,
                    local_position: position,
                    kind,
                });
            }
        }
    }

    /// Whether a contact drifted beyond the hit slop of its device kind.
    fn check_slop(&self, current_position: Offset<f64>, kind: PointerType) -> bool {
        self.state.initial_position().is_some_and(|initial| {
            (current_position - initial).distance() > self.settings.lock().hit_slop(kind)
        })
    }

    /// Check if timeout for second tap has expired
    /// Should be called periodically
    pub fn check_timeout(&self) -> bool {
        self.check_timeout_at(self.state.now())
    }

    fn check_timeout_at(&self, now: Instant) -> bool {
        let expired = {
            let state = self.gesture_state.lock();
            state.phase == DoubleTapPhase::WaitingForSecond
                && state.first_tap_time.is_some_and(|first_time| {
                    now.duration_since(first_time) >= self.double_tap_timeout()
                })
        };
        if expired {
            self.give_up_first_tap();
        }
        expired
    }

    /// End a waiting first tap that will not become a double tap.
    ///
    /// The double tap withdraws from the held first entry and releases the
    /// hold — the entry was closed as `[tap1, double_tap]`, so the lone tap
    /// wins and finally fires — then reports `on_double_tap_cancel`. The arena
    /// work happens first: a cancel callback that panics or disposes must not
    /// leave the first contact's arena held.
    fn give_up_first_tap(&self) {
        let (position, global_position, kind) = {
            let mut state = self.gesture_state.lock();
            if state.phase != DoubleTapPhase::WaitingForSecond {
                return;
            }
            let position = state.first_tap_position.take().unwrap_or(Offset::ZERO);
            let global_position = state.first_tap_global_position.take().unwrap_or(position);
            let kind = state.device_kind.unwrap_or(PointerType::Touch);
            *state = DoubleTapState::default();
            (position, global_position, kind)
        };
        let first_entry = self.first_entry.lock().take();
        self.state.reject();
        if let Some(entry) = first_entry {
            entry.release();
        }
        let callback = self.callbacks.borrow().on_double_tap_cancel.clone();
        if let Some(callback) = callback {
            callback(DoubleTapDetails {
                global_position,
                local_position: position,
                kind,
            });
        }
    }

    /// The contact being tracked, in both spaces, for a cancel no event drives.
    fn contact_positions(&self) -> (Offset<f64>, Offset<f64>) {
        let local = self.state.initial_position().unwrap_or(Offset::ZERO);
        let global = self.state.initial_global_position().unwrap_or(local);
        (local, global)
    }

    /// Extract position and pointer type from a PointerEvent
    fn extract_event_data(event: &PointerEvent) -> (Offset<f64>, PointerType) {
        let position = event.position();
        let pointer_type = match event {
            PointerEvent::Down(e) | PointerEvent::Up(e) => e.pointer.pointer_type,
            PointerEvent::Move(e) => e.pointer.pointer_type,
            PointerEvent::Cancel(info) | PointerEvent::Enter(info) | PointerEvent::Leave(info) => {
                info.pointer_type
            }
            PointerEvent::Scroll(e) => e.pointer.pointer_type,
            PointerEvent::Gesture(e) => e.pointer.pointer_type,
        };
        (position, pointer_type)
    }
}

impl DoubleTapGestureRecognizer {
    /// The same registration [`GestureRecognizer::add_pointer`] performs,
    /// with the pointer's real device `kind` — trait callers that only have
    /// [`GestureRecognizer::add_pointer`]'s narrower signature (no `kind`
    /// parameter) fall back to [`PointerType::Touch`] through that method;
    /// a caller holding the concrete type and the originating
    /// [`crate::events::PointerEvent`] (`GestureDetector`'s own dispatch,
    /// which has both) should call this instead, so
    /// [`DoubleTapDetails::kind`] reports the actual device rather than a
    /// hard-coded guess.
    pub fn add_pointer_with_kind(
        self: &Rc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
        kind: PointerType,
    ) {
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }

        // Pre-registration checks. They run *before* the new pointer is
        // registered, so `reject`/`release` on the first entry runs while
        // `primary_pointer` still names the first contact.
        // A panic from a callback the restart runs is held until this contact
        // is admitted: the contact still becomes the next first tap.
        let mut failure = None;
        let phase = self.gesture_state.lock().phase;
        match phase {
            DoubleTapPhase::FirstDown | DoubleTapPhase::SecondDown => {
                // A contact is down. Another finger is not admitted; the same
                // pointer again means its Up/Cancel never arrived, so that
                // attempt is cancelled before this contact starts afresh.
                if self.state.primary_pointer() != Some(pointer) {
                    return;
                }
                let (local, global) = self.contact_positions();
                failure = RoutePanic::capture(|| self.handle_cancel(local, global, kind));
            }
            DoubleTapPhase::WaitingForSecond => {
                let settings = self.settings.lock().clone();
                let (window_expired, out_of_slop) = {
                    let state = self.gesture_state.lock();
                    (
                        state.first_tap_time.is_some_and(|first_time| {
                            self.state.now().duration_since(first_time)
                                > settings.double_tap_timeout()
                        }),
                        state.first_tap_position.is_some_and(|first_pos| {
                            (position - first_pos).distance() > settings.double_tap_slop()
                        }),
                    )
                };
                // A contact after the window, or too far from the first tap,
                // cannot complete this double tap. The first tap is given up
                // now — its single tap fires instead of waiting out the
                // window — and this contact becomes the next first tap.
                if window_expired || out_of_slop {
                    failure = RoutePanic::capture(|| self.give_up_first_tap());
                }
            }
            DoubleTapPhase::Ready | DoubleTapPhase::Cancelled => {}
        }
        // A callback above may have disposed this recognizer or admitted a
        // contact of its own; either way this admission is void.
        // A contact admitted from inside one of those callbacks leaves a contact
        // down; the restart itself never does.
        let reentered = matches!(
            self.gesture_state.lock().phase,
            DoubleTapPhase::FirstDown | DoubleTapPhase::SecondDown
        );
        if !self.state.is_disposed() && !reentered {
            self.state
                .start_tracking(pointer, position, global_position, self);
            self.handle_down(position, global_position, kind);
        }
        if let Some(panic) = failure {
            panic.resume();
        }
    }
}

impl GestureRecognizer for DoubleTapGestureRecognizer {
    fn add_pointer(
        self: &Rc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
    ) {
        // No `kind` in this trait method's signature — see
        // `add_pointer_with_kind`'s doc for the caller that should use it
        // instead when the real device kind is available.
        self.add_pointer_with_kind(pointer, position, global_position, PointerType::Touch);
    }

    fn add_pointer_down(self: &Rc<Self>, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        // Primary button only: a right- or middle-click has its own tap
        // family and must not register a double tap.
        let PointerEvent::Down(data) = event else {
            return;
        };
        if !is_primary_down(event) {
            return;
        }
        self.add_pointer_with_kind(
            crate::events::extract_pointer_id(event),
            event.position(),
            dispatch.global.position(),
            data.pointer.pointer_type,
        );
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Only the contact this recognizer follows: another finger's Up must
        // not complete or cancel this gesture.
        if self.state.primary_pointer() != Some(crate::events::extract_pointer_id(event)) {
            return;
        }

        let (position, pointer_type) = Self::extract_event_data(event);
        // The only point at which the untransformed position exists at all.
        let global_position = dispatch.global.position();

        match event {
            PointerEvent::Move(_) => {
                self.handle_move(position, global_position, pointer_type);
            }
            PointerEvent::Up(_) => {
                self.handle_up(position, global_position, pointer_type);
            }
            PointerEvent::Cancel(_) => {
                self.handle_cancel(position, global_position, pointer_type);
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Reject arena entries + clear tracked pointer (disposing a recognizer
        // clears arena state for tracked pointers). reject_gesture (fired by
        // state.reject) also drains the inter-tap hold via first_entry, but
        // that relies on the entry still being active.
        self.state.reject();
        // Belt-and-suspenders: clear first_entry even when
        // reject_gesture was a no-op (arena already settled or no active entry).
        // Avoids retaining one Arc<dyn GestureArenaMember> after unmount
        // mid-inter-tap-window.
        let entry = self.first_entry.lock().take();
        if let Some(entry) = entry {
            entry.release();
        }
        // Captures are dropped outside the cell, so a capture whose destructor
        // reaches this recognizer finds it unborrowed.
        let callbacks = std::mem::take(&mut *self.callbacks.borrow_mut());
        let mut retirement = CallbackSequence::new();
        callbacks.retire(&mut retirement);
        retirement.finish();
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

impl GestureArenaMember for DoubleTapGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
    }

    fn poll_deadline(&self, now: Instant) {
        // Frame-driven give-up check: after a completed first tap, the
        // inter-tap window must eventually expire if no second tap arrives.
        // Without this poll, `poll_deadlines()` never drives the timeout, so a
        // detector combining `on_tap` + `on_double_tap` could leave the lone
        // single-tap forever holding the arena. `check_timeout` is idempotent
        // (it only fires once the window has elapsed in the `WaitingForSecond`
        // phase) and drops the gesture_state lock before releasing the arena.
        self.check_timeout_at(now);
    }

    fn deadline(&self) -> Option<Instant> {
        // Same guard as `has_pending_deadline`. The deadline is exactly what
        // `check_timeout` compares `now` against: `first_tap_time +
        // double_tap_timeout()`.
        let state = self.gesture_state.lock();
        if state.phase != DoubleTapPhase::WaitingForSecond {
            return None;
        }
        state
            .first_tap_time
            .and_then(|first_time| first_time.checked_add(self.double_tap_timeout()))
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // We lost the arena - cancel the gesture
        if let Some(pos) = self.state.initial_position() {
            // No event drives an arena rejection, so the global position comes
            // from what the first contact recorded. Falling back to the local
            // one would restate the very defect this carries (issue #908), so
            // it is only reached when nothing was ever recorded.
            let (kind, global_pos) = {
                let state = self.gesture_state.lock();
                (
                    state.device_kind.unwrap_or(PointerType::Touch),
                    state.first_tap_global_position.unwrap_or(pos),
                )
            };
            self.handle_cancel(pos, global_pos, kind);
        }
        // If a competitor won while we held the first entry across the inter-tap
        // window, drain the hold so the entry is not left held.
        let entry = self.first_entry.lock().take();
        if let Some(entry) = entry {
            entry.release();
        }
    }
}

impl std::fmt::Debug for DoubleTapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoubleTapGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &self.gesture_state.lock())
            .field("settings", &self.settings.lock())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::{arena::GestureArena, events::make_up_event_for_id};

    #[test]
    fn test_double_tap_timing() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer =
            DoubleTapGestureRecognizer::new(arena).with_on_double_tap(move |_details| {
                *tapped_clone.lock() = true;
            });

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(100.0, 100.0);

        // First tap
        recognizer.add_pointer(pointer, position, position);
        let up_event = make_up_event_for_id(pointer, position, PointerType::Touch);
        recognizer.handle_event(PointerDispatch::at_root(&up_event));

        // Should be waiting for second tap
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, DoubleTapPhase::WaitingForSecond);
        drop(state);

        // Second tap (need to add pointer again for new sequence)
        recognizer.handle_down(position, position, PointerType::Touch);
        let up_event = make_up_event_for_id(pointer, position, PointerType::Touch);
        recognizer.handle_event(PointerDispatch::at_root(&up_event));

        // Should have called callback
        assert!(*tapped.lock());
    }
}
