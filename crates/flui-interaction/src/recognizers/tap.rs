//! Tap gesture recognizer
//!
//! Recognizes tap gestures (pointer down + up within slop tolerance).
//!
//! A tap is defined as:
//! - Pointer down
//! - Pointer stays within touch_slop of initial position
//! - Pointer up within timeout
//!
//! # Button support
//!
//! The recogniser is button-aware: callers can register
//! separate callbacks for [`TapButton::Primary`],
//! [`TapButton::Secondary`], and [`TapButton::Tertiary`] clicks. The
//! primary path keeps the legacy `on_tap*` callbacks and fires when
//! the down event's `button` mask includes `Primary`. The secondary
//! path fires the `on_secondary_tap*` callbacks on
//! [`PointerButton::Secondary`] events; the tertiary path fires on
//! [`PointerButton::Auxiliary`]
//! ("tertiary" is the middle / auxiliary mouse button).
//! If no button-specific callback is registered, the event is
//! silently dropped (the recogniser stays a no-op for that button).

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Weak},
};

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;
use smallvec::SmallVec;
use ui_events::pointer::PointerButton;

use super::recognizer::{CallbackSequence, GestureRecognizer, RecognizerBase};
use crate::{
    arena::{GestureArenaMember, GestureDisposition},
    events::{PointerEvent, PointerType},
    ids::PointerId,
    routing::PointerDispatch,
    settings::GestureSettings,
    traits::PointerEventExtTrait,
};

/// Tap button slot: primary, secondary and tertiary are tracked separately.
///
/// Button mapping:
/// - [`TapButton::Primary`]   ↔ `ui_events::pointer::PointerButton::Primary`
/// - [`TapButton::Secondary`] ↔ `ui_events::pointer::PointerButton::Secondary`
/// - [`TapButton::Tertiary`]  ↔ `ui_events::pointer::PointerButton::Auxiliary`
///   (the middle mouse button).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TapButton {
    /// Primary (default left mouse button / touch contact).
    Primary,
    /// Secondary (right mouse button).
    Secondary,
    /// Tertiary (auxiliary / middle mouse button).
    Tertiary,
}

impl TryFrom<PointerButton> for TapButton {
    /// The unsupported button (outside the three tracked slots).
    type Error = PointerButton;

    /// Map a raw [`PointerButton`] event payload to a [`TapButton`] slot.
    ///
    /// Errors for buttons outside the three tracked slots
    /// (X1/X2/pen-eraser/etc.) — those events are ignored by the tap
    /// recogniser entirely.
    fn try_from(button: PointerButton) -> Result<Self, Self::Error> {
        match button {
            PointerButton::Primary => Ok(Self::Primary),
            PointerButton::Secondary => Ok(Self::Secondary),
            PointerButton::Auxiliary => Ok(Self::Tertiary),
            other => Err(other),
        }
    }
}

impl TapButton {
    /// Map a raw [`PointerButton`] to a [`TapButton`] slot, or `None` for the
    /// untracked buttons. Convenience wrapper over the [`TryFrom`] impl for the
    /// `Option`-combinator call sites.
    #[inline]
    pub fn from_pointer_button(button: PointerButton) -> Option<Self> {
        Self::try_from(button).ok()
    }
}

/// Callback for tap events
pub type TapCallback = Rc<dyn Fn(TapDetails)>;

/// Details about a tap gesture
#[derive(Debug, Clone, PartialEq)]
pub struct TapDetails {
    /// Global position where tap occurred
    pub global_position: Offset<f64>,
    /// Local position (relative to widget)
    pub local_position: Offset<f64>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Recognizes tap gestures
///
/// A tap is a quick press-and-release within a small movement tolerance.
///
/// # Example
///
/// ```rust
/// use flui_interaction::arena::GestureArena;
/// use flui_interaction::recognizers::TapGestureRecognizer;
///
/// let arena = GestureArena::new();
/// // The recogniser is shared via `Arc`; clone the inner `Arc` to
/// // register callbacks (`on_tap` fires on Primary button up).
/// let recognizer = TapGestureRecognizer::new(arena)
///     .with_on_tap(|details| {
///         // The callback fires AFTER the arena confirms this
///         // recogniser won; a released contact waits for that
///         // verdict, so only the arena winner receives the user callback.
///         let _pos = details.global_position;
///     });
/// // `add_pointer` and `handle_event` are wired by
/// // `flui_interaction::GestureBinding` at runtime.
#[derive(Clone)]
pub struct TapGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Callbacks
    callbacks: Rc<RefCell<TapCallbacks>>,

    /// Every tap sequence that has not reached its arena verdict yet.
    sequences: Rc<RefCell<TapSequences>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,
}

impl std::fmt::Debug for TapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapGestureRecognizer")
            .field("state", &self.state)
            .field(
                "live_sequences",
                &self
                    .sequences
                    .try_borrow()
                    .map(|sequences| sequences.live.len())
                    .ok(),
            )
            .finish_non_exhaustive()
    }
}

// Field names keep the `on_tap_down`-style callback names.
#[expect(clippy::struct_field_names)]
#[derive(Default)]
struct TapCallbacks {
    on_tap_down: Option<TapCallback>,
    on_tap_move: Option<TapCallback>,
    on_tap_up: Option<TapCallback>,
    on_tap: Option<TapCallback>,
    on_tap_cancel: Option<TapCallback>,

    // Secondary-button callbacks (right mouse).
    on_secondary_tap_down: Option<TapCallback>,
    on_secondary_tap_up: Option<TapCallback>,
    on_secondary_tap: Option<TapCallback>,
    on_secondary_tap_cancel: Option<TapCallback>,

    // Tertiary-button callbacks (auxiliary / middle mouse).
    on_tertiary_tap_down: Option<TapCallback>,
    on_tertiary_tap_up: Option<TapCallback>,
    on_tertiary_tap: Option<TapCallback>,
    on_tertiary_tap_cancel: Option<TapCallback>,
}

impl TapCallbacks {
    /// Per-button down-callback lookup.
    #[inline]
    fn down(&self, button: TapButton) -> Option<&TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_down.as_ref(),
            TapButton::Secondary => self.on_secondary_tap_down.as_ref(),
            TapButton::Tertiary => self.on_tertiary_tap_down.as_ref(),
        }
    }

    /// Per-button up-callback lookup.
    #[inline]
    fn up(&self, button: TapButton) -> Option<&TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_up.as_ref(),
            TapButton::Secondary => self.on_secondary_tap_up.as_ref(),
            TapButton::Tertiary => self.on_tertiary_tap_up.as_ref(),
        }
    }

    /// Per-button tap-callback lookup.
    #[inline]
    fn tap(&self, button: TapButton) -> Option<&TapCallback> {
        match button {
            TapButton::Primary => self.on_tap.as_ref(),
            TapButton::Secondary => self.on_secondary_tap.as_ref(),
            TapButton::Tertiary => self.on_tertiary_tap.as_ref(),
        }
    }

    /// Per-button cancel-callback lookup.
    #[inline]
    fn cancel(&self, button: TapButton) -> Option<&TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_cancel.as_ref(),
            TapButton::Secondary => self.on_secondary_tap_cancel.as_ref(),
            TapButton::Tertiary => self.on_tertiary_tap_cancel.as_ref(),
        }
    }
}

/// A contact's details and the button stream it belongs to.
#[derive(Debug, Clone, PartialEq)]
struct PendingDown {
    details: TapDetails,
    button: TapButton,
}

/// One tap sequence (contact down to arena verdict).
///
/// Its arena verdict can arrive after the contact lifted — a double tap holds
/// the first contact's arena across its inter-tap window — and by then the
/// next contact may already be down under the same pointer ID: a mouse
/// reports one ID for every click. So a sequence is addressed by its own
/// identity, which the arena reaches through the sequence's own member
/// ([`TapArenaMember`]), never by pointer ID.
#[derive(Debug)]
struct TapSequence {
    id: u64,
    pointer: PointerId,
    /// The contact as it went down. Consumed when `on_*_tap_down` fires.
    down: Option<PendingDown>,
    /// The release, recorded at Up; the tap fires once it is accepted.
    up: Option<PendingDown>,
    /// Whether the arena accepted this sequence.
    accepted: bool,
}

#[derive(Debug, Default)]
struct TapSequences {
    /// The last sequence identity handed out.
    last_id: u64,
    /// The sequence whose contact is still down, if any.
    current: Option<u64>,
    /// Sequences still waiting for their down to lift or their verdict.
    live: SmallVec<[TapSequence; 2]>,
}

impl TapSequences {
    fn index_of(&self, id: u64) -> Option<usize> {
        self.live.iter().position(|sequence| sequence.id == id)
    }

    fn current_mut(&mut self) -> Option<&mut TapSequence> {
        let id = self.current?;
        self.live.iter_mut().find(|sequence| sequence.id == id)
    }

    /// Remove sequence `id`, clearing `current` when it named it.
    fn remove(&mut self, id: u64) -> Option<TapSequence> {
        if self.current == Some(id) {
            self.current = None;
        }
        let index = self.index_of(id)?;
        Some(self.live.remove(index))
    }
}

/// The arena member standing for one tap sequence.
///
/// Registered in place of the recognizer itself so a verdict names the exact
/// sequence it decides. It holds the recognizer weakly: the arena must not
/// keep an unmounted recognizer alive.
#[derive(Clone)]
struct TapArenaMember {
    recognizer: Weak<TapGestureRecognizer>,
    sequence: u64,
}

impl crate::sealed::arena_member::Sealed for TapArenaMember {}

impl GestureArenaMember for TapArenaMember {
    fn accept_gesture(&self, _pointer: PointerId) {
        if let Some(recognizer) = self.recognizer.upgrade() {
            recognizer.accept_sequence(self.sequence);
        }
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        if let Some(recognizer) = self.recognizer.upgrade() {
            recognizer.reject_sequence(self.sequence);
        }
    }
}

impl TapGestureRecognizer {
    /// Create a new tap recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Self::with_settings(arena, GestureSettings::default())
    }

    /// Create a new tap recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(TapCallbacks::default())),
            sequences: Rc::new(RefCell::new(TapSequences::default())),
            settings: Arc::new(Mutex::new(settings)),
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

    /// Set the tap down callback
    pub fn with_on_tap_down(self: Arc<Self>, callback: impl Fn(TapDetails) + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tap_down = Some(Rc::new(callback));
        self
    }

    /// Set the tap move callback (called when pointer moves during tap)
    ///
    /// This callback is triggered when a pointer that initiated a tap moves
    /// but stays within the slop tolerance.
    pub fn with_on_tap_move(self: Arc<Self>, callback: impl Fn(TapDetails) + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tap_move = Some(Rc::new(callback));
        self
    }

    /// Set the tap up callback
    pub fn with_on_tap_up(self: Arc<Self>, callback: impl Fn(TapDetails) + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tap_up = Some(Rc::new(callback));
        self
    }

    /// Set the tap callback (called on successful tap)
    pub fn with_on_tap(self: Arc<Self>, callback: impl Fn(TapDetails) + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tap = Some(Rc::new(callback));
        self
    }

    /// Set the tap cancel callback
    pub fn with_on_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tap_cancel = Some(Rc::new(callback));
        self
    }

    // ========================================================================
    // Secondary-button builders (right mouse).
    // ========================================================================

    /// Set the secondary-button tap-down callback.
    pub fn with_on_secondary_tap_down(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_secondary_tap_down = Some(Rc::new(callback));
        self
    }

    /// Set the secondary-button tap-up callback.
    pub fn with_on_secondary_tap_up(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_secondary_tap_up = Some(Rc::new(callback));
        self
    }

    /// Set the secondary-button tap callback (fires on successful up).
    pub fn with_on_secondary_tap(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_secondary_tap = Some(Rc::new(callback));
        self
    }

    /// Set the secondary-button tap-cancel callback.
    pub fn with_on_secondary_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_secondary_tap_cancel = Some(Rc::new(callback));
        self
    }

    // ========================================================================
    // Tertiary-button builders (auxiliary / middle mouse).
    // ========================================================================

    /// Set the tertiary-button tap-down callback.
    pub fn with_on_tertiary_tap_down(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tertiary_tap_down = Some(Rc::new(callback));
        self
    }

    /// Set the tertiary-button tap-up callback.
    pub fn with_on_tertiary_tap_up(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tertiary_tap_up = Some(Rc::new(callback));
        self
    }

    /// Set the tertiary-button tap callback (fires on successful up).
    pub fn with_on_tertiary_tap(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tertiary_tap = Some(Rc::new(callback));
        self
    }

    /// Set the tertiary-button tap-cancel callback.
    pub fn with_on_tertiary_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tertiary_tap_cancel = Some(Rc::new(callback));
        self
    }

    /// Start a sequence for a contact that went down.
    ///
    /// A tap follows one contact at a time: while one is down, another
    /// contact (a second finger) is not admitted. A new contact under the
    /// pointer of a sequence still marked down means that pointer's Up or
    /// Cancel never arrived; that sequence is withdrawn first. A sequence that
    /// has lifted and only waits for its arena verdict is left alone — its
    /// verdict still reaches it through its own arena member.
    fn begin_sequence(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
        kind: PointerType,
        button: TapButton,
    ) {
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        let stale = {
            let mut sequences = self.sequences.borrow_mut();
            match sequences.current_mut() {
                Some(current) if current.pointer != pointer => return,
                Some(current) => {
                    let id = current.id;
                    sequences.remove(id)
                }
                None => None,
            }
        };
        if stale.is_some() {
            self.state.reject();
        }
        let id = {
            let mut sequences = self.sequences.borrow_mut();
            let id = sequences
                .last_id
                .checked_add(1)
                .expect("BUG: tap sequence identity exhausted");
            sequences.last_id = id;
            sequences.current = Some(id);
            sequences.live.push(TapSequence {
                id,
                pointer,
                down: Some(PendingDown {
                    details: TapDetails {
                        global_position,
                        local_position: position,
                        kind,
                    },
                    button,
                }),
                up: None,
                accepted: false,
            });
            id
        };
        let member = Arc::new(TapArenaMember {
            recognizer: Arc::downgrade(self),
            sequence: id,
        });
        self.state
            .start_tracking(pointer, position, global_position, &member);
    }

    /// Refine the current sequence's contact from its real `Down` event.
    ///
    /// `add_pointer` carries only positions, so it stages a primary touch
    /// contact; a caller that also routes the `Down` corrects kind and button
    /// here, before any up.
    fn refine_down(
        &self,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
        kind: PointerType,
        button: TapButton,
    ) {
        let mut sequences = self.sequences.borrow_mut();
        if let Some(current) = sequences.current_mut()
            && current.pointer == pointer
            && current.down.is_some()
        {
            current.down = Some(PendingDown {
                details: TapDetails {
                    global_position,
                    local_position: position,
                    kind,
                },
                button,
            });
        }
    }

    /// Record the arena's acceptance of sequence `id`, delivering the tap if
    /// its contact already lifted.
    fn accept_sequence(&self, id: u64) {
        {
            let mut sequences = self.sequences.borrow_mut();
            let Some(index) = sequences.index_of(id) else {
                return;
            };
            sequences.live[index].accepted = true;
        }
        self.deliver_if_won(id);
    }

    /// Forget sequence `id` after the arena rejected it. A rejected contact
    /// that is still down stops being tracked, so its later events are
    /// ignored.
    fn reject_sequence(&self, id: u64) {
        let was_current = {
            let mut sequences = self.sequences.borrow_mut();
            let was_current = sequences.current == Some(id);
            sequences.remove(id);
            was_current
        };
        if was_current {
            self.state.stop_tracking();
        }
    }

    /// Deliver sequence `id` once it is both accepted and lifted.
    ///
    /// The sequence leaves the recognizer before any callback runs, so it
    /// fires exactly once, and a callback that panics, disposes this
    /// recognizer or starts the next contact finds no half-delivered state.
    fn deliver_if_won(&self, id: u64) {
        let won = {
            let mut sequences = self.sequences.borrow_mut();
            match sequences.index_of(id) {
                Some(index)
                    if sequences.live[index].accepted && sequences.live[index].up.is_some() =>
                {
                    sequences.remove(id)
                }
                _ => None,
            }
        };
        let Some(TapSequence {
            down, up: Some(up), ..
        }) = won
        else {
            return;
        };
        let (down_callback, up_callback, tap_callback) = {
            let callbacks = self.callbacks.borrow();
            (
                down.as_ref()
                    .and_then(|down| callbacks.down(down.button).cloned()),
                callbacks.up(up.button).cloned(),
                callbacks.tap(up.button).cloned(),
            )
        };
        let mut run = CallbackSequence::new();
        if let Some(down) = down {
            run.call(down_callback, |callback| callback(down.details));
        }
        let details = up.details;
        run.call(up_callback, |callback| callback(details.clone()));
        run.call(tap_callback, |callback| callback(details));
        run.finish();
    }

    /// Handle tap up event.
    ///
    /// Records the release and stops tracking; the tap fires only once the
    /// arena accepts this sequence — now if it already did, otherwise when
    /// the verdict arrives (a self-driven sweep inside `stop_tracking`, or the
    /// binding's sweep, release or deferred resolution).
    ///
    /// Button mismatch (down was Primary, up is Secondary) cancels the tap
    /// rather than firing the secondary slot: the up is routed to whichever
    /// button stream initiated the down.
    fn handle_tap_up(&self, pointer: PointerId, details: TapDetails, button: TapButton) {
        enum Release {
            Recorded(u64),
            Mismatch(TapButton),
            Untracked,
        }
        let release = {
            let mut sequences = self.sequences.borrow_mut();
            match sequences.current_mut() {
                Some(current) if current.pointer == pointer => {
                    let id = current.id;
                    let down_button = current.down.as_ref().map(|down| down.button);
                    if let Some(down_button) = down_button
                        && down_button != button
                    {
                        sequences.remove(id);
                        Release::Mismatch(down_button)
                    } else {
                        current.up = Some(PendingDown {
                            details: details.clone(),
                            button,
                        });
                        sequences.current = None;
                        Release::Recorded(id)
                    }
                }
                _ => Release::Untracked,
            }
        };
        match release {
            Release::Recorded(id) => {
                self.state.stop_tracking();
                self.deliver_if_won(id);
            }
            Release::Mismatch(down_button) => {
                // Withdraw before user code can unwind or start the next
                // contact from the callback.
                self.state.reject();
                let callback = self.callbacks.borrow().cancel(down_button).cloned();
                if let Some(callback) = callback {
                    callback(details);
                }
            }
            Release::Untracked => self.state.stop_tracking(),
        }
    }

    /// Cancel the contact that is down (slop exceeded, or `PointerCancel`).
    fn cancel_current(&self, details: TapDetails) {
        let cancelled = {
            let mut sequences = self.sequences.borrow_mut();
            let Some(id) = sequences.current else {
                return;
            };
            sequences.remove(id)
        };
        let Some(cancelled) = cancelled else {
            return;
        };
        let callback = cancelled
            .down
            .as_ref()
            .and_then(|down| self.callbacks.borrow().cancel(down.button).cloned());
        // Withdraw and clear tracking before user code can unwind or start
        // another sequence from the callback.
        self.state.reject();
        if let Some(callback) = callback {
            callback(details);
        }
    }

    /// Fire `on_tap_move` for a contact still within its slop.
    fn handle_tap_move(&self, details: TapDetails) {
        if self.sequences.borrow().current.is_none() {
            return;
        }
        // Primary-only: there is no secondary/tertiary move; a primary-button
        // tap that moves is still observed by `on_tap_move`.
        let callback = self.callbacks.borrow().on_tap_move.clone();
        if let Some(callback) = callback {
            callback(details);
        }
    }

    /// Whether the contact drifted beyond the hit slop of its device kind.
    fn exceeds_slop(&self, position: Offset<f64>, kind: PointerType) -> bool {
        self.state.initial_position().is_some_and(|initial| {
            (position - initial).distance() > self.settings.lock().hit_slop(kind)
        })
    }

    /// The [`TapButton`] family a `Down`/`Up` belongs to — the one mapping
    /// every path uses.
    ///
    /// A transition without a button (touch and pen may not carry one) is
    /// primary. A button outside the three families (X1, X2, pen eraser) is
    /// no tap at all: `None`, and so is any other event.
    fn tap_button(event: &PointerEvent) -> Option<TapButton> {
        match event {
            PointerEvent::Down(data) | PointerEvent::Up(data) => data
                .button
                .map_or(Some(TapButton::Primary), TapButton::from_pointer_button),
            _ => None,
        }
    }
}

impl GestureRecognizer for TapGestureRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
    ) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        // Positions only: the contact is staged as a primary touch, which a
        // routed `Down` refines in `handle_event`.
        self.begin_sequence(
            pointer,
            position,
            global_position,
            PointerType::Touch,
            TapButton::Primary,
        );
    }

    fn add_pointer_down(self: &Arc<Self>, dispatch: PointerDispatch<'_>) {
        let PointerEvent::Down(data) = dispatch.local else {
            return;
        };
        // Primary, secondary and tertiary presses each have their own tap
        // family; any other button is not a tap at all.
        let Some(button) = Self::tap_button(dispatch.local) else {
            return;
        };
        let position = dispatch.local.position();
        self.begin_sequence(
            dispatch.local.pointer_id(),
            position,
            dispatch.global.position(),
            data.pointer.pointer_type,
            button,
        );
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap.handle_event",
            kind = %crate::observability::pointer_event_kind(event),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Only the pointer this recognizer tracks; a single-pointer
        // recognizer ignores every other contact.
        let Some(primary) = self.state.primary_pointer() else {
            return;
        };
        if event.pointer_id() != primary {
            return;
        }
        // Read once, here: this is the only point at which the untransformed
        // position is available at all (issue #908).
        let global_position = dispatch.global.position();
        let details = |position: Offset<f64>, kind: PointerType| TapDetails {
            global_position,
            local_position: position,
            kind,
        };

        match event {
            PointerEvent::Down(data) => {
                // A press of a button outside the tap families refines nothing.
                if let Some(button) = Self::tap_button(event) {
                    let pos = data.state.position;
                    self.refine_down(
                        primary,
                        Offset::new(pos.x, pos.y),
                        global_position,
                        data.pointer.pointer_type,
                        button,
                    );
                }
            }
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let position = Offset::new(pos.x, pos.y);
                let kind = data.pointer.pointer_type;
                if self.exceeds_slop(position, kind) {
                    self.cancel_current(details(position, kind));
                } else {
                    self.handle_tap_move(details(position, kind));
                }
            }
            PointerEvent::Up(data) => {
                // Releasing a button outside the tap families (X1 while the
                // primary is held) neither ends nor cancels the tap.
                if let Some(button) = Self::tap_button(event) {
                    let pos = data.state.position;
                    self.handle_tap_up(
                        primary,
                        details(Offset::new(pos.x, pos.y), data.pointer.pointer_type),
                        button,
                    );
                }
            }
            PointerEvent::Cancel(info) => {
                // A cancel carries no position at all, in EITHER space — the
                // event's own `position()` answers `Offset::ZERO`. Both halves
                // therefore fall back to the recorded contact, and they fall
                // back together: reporting the stored local beside a zero
                // global would restate the very defect this pair exists to fix.
                if let Some(pos) = self.state.initial_position() {
                    let global = self.state.initial_global_position().unwrap_or(pos);
                    self.cancel_current(TapDetails {
                        global_position: global,
                        local_position: pos,
                        kind: info.pointer_type,
                    });
                }
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Forget every sequence, then withdraw from the arena, so a verdict
        // arriving during or after disposal finds nothing to deliver.
        let live = {
            let mut sequences = self.sequences.borrow_mut();
            sequences.current = None;
            std::mem::take(&mut sequences.live)
        };
        self.state.reject();
        // Callback captures are dropped outside the cell, so a capture whose
        // destructor reaches this recognizer finds it unborrowed.
        let callbacks = std::mem::take(&mut *self.callbacks.borrow_mut());
        drop(live);
        drop(callbacks);
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

// =============================================================================
// Canonical trait hierarchy adoption
// =============================================================================
//
// Tap implements the trait infrastructure in one_sequence.rs and
// primary_pointer.rs.

impl crate::recognizers::OneSequenceGestureRecognizer for TapGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.state
            .primary_pointer()
            .map(|p| vec![p])
            .unwrap_or_default()
    }

    /// Resolves the tracked contact's own arena entry; the verdict then
    /// reaches that sequence through its member. A pointer this recognizer
    /// does not track (including a lifted contact awaiting its verdict) is
    /// left alone.
    fn resolve_pointer(&self, pointer: PointerId, disposition: GestureDisposition) {
        if self.state.primary_pointer() != Some(pointer) {
            return;
        }
        if let Some(entry) = self.state.tracked_entry() {
            entry.resolve(disposition);
        }
    }

    fn stop_tracking_pointer(&self, _pointer: PointerId) {
        self.state.stop_tracking();
    }
}

impl crate::recognizers::PrimaryPointerGestureRecognizer for TapGestureRecognizer {
    fn initial_position(&self) -> Option<Offset<f64>> {
        self.state.initial_position()
    }

    fn handle_primary_pointer(&self, dispatch: PointerDispatch<'_>) {
        // Tap dispatches all primary-pointer events through handle_event;
        // delegate via the supertrait method.
        <Self as GestureRecognizer>::handle_event(self, dispatch);
    }
}

/// The recognizer itself is never an arena member: each sequence registers
/// its own `TapArenaMember`, and only that member's verdict decides the
/// sequence. A verdict addressed to the recognizer carries nothing but a
/// pointer ID, which cannot tell a mouse's held earlier click from the
/// current one, so it decides nothing.
impl GestureArenaMember for TapGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {}

    fn reject_gesture(&self, _pointer: PointerId) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;

    fn pos(x: f64, y: f64) -> Offset<f64> {
        Offset::new(x, y)
    }

    fn primary_down(p: Offset<f64>) -> PointerEvent {
        crate::events::make_down_event_with_button(p, PointerType::Touch, PointerButton::Primary)
    }
    // Tap recognizer matrix: callback delivery and containment of a panicking cancel.
    #[test]
    fn tap_recognizer_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "test_tap_recognizer_with_callback",
                test_tap_recognizer_with_callback,
            ),
            (
                "panicking_cancel_callback_cannot_strand_tap_tracking",
                panicking_cancel_callback_cannot_strand_tap_tracking,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    fn panicking_cancel_callback_cannot_strand_tap_tracking() {
        let arena = GestureArena::new();
        let recognizer = TapGestureRecognizer::new(arena.clone())
            .with_on_tap_cancel(|_| panic!("tap cancel panic"));
        let position = pos(1.0, 2.0);
        recognizer.add_pointer(PointerId::PRIMARY, position, position);
        arena.close(PointerId::PRIMARY);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            recognizer.handle_event(PointerDispatch::at_root(&crate::events::make_cancel_event(
                PointerType::Touch,
            )));
        }));

        assert!(unwind.is_err());
        assert_eq!(recognizer.primary_pointer(), None);
        assert!(arena.is_empty());
    }
    fn primary_up(p: Offset<f64>) -> PointerEvent {
        crate::events::make_up_event_with_button(p, PointerType::Touch, PointerButton::Primary)
    }
    /// Legacy primary path: down + up with the Primary button
    /// fires `on_tap` (`add_pointer` no longer
    /// pre-stages the down; the down event itself does).
    fn test_tap_recognizer_with_callback() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena).with_on_tap(move |_details| {
            *tapped_clone.lock() = true;
        });

        let pointer = PointerId::PRIMARY;
        let position = pos(100.0, 100.0);

        recognizer.add_pointer(pointer, position, position);
        recognizer.handle_event(PointerDispatch::at_root(&primary_down(position)));
        recognizer.handle_event(PointerDispatch::at_root(&primary_up(position)));

        assert!(*tapped.lock());
    }
}
