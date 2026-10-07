//! Button-aware tap recognition with one arena identity per contact.

use super::{
    callback_containment::{CallbackSequence, finish_containment, invoke_callback},
    contact::{ArenaMembership, ContactId, PrimaryContact},
    recognizer::{CancelOutcome, GestureRecognizer},
};
use crate::events::PointerButton;
use crate::{
    arena::{GestureArena, GestureArenaEntry, GestureArenaMember},
    events::{PointerEvent, PointerEventExt, PointerKind},
    ids::PointerId,
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
};
use flui_foundation::geometry::Offset;
use smallvec::SmallVec;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

/// The three supported button families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TapButton {
    /// Primary mouse button or touch contact.
    Primary,
    /// Secondary mouse button.
    Secondary,
    /// Auxiliary mouse button.
    Tertiary,
}
impl TryFrom<PointerButton> for TapButton {
    type Error = PointerButton;
    fn try_from(button: PointerButton) -> Result<Self, Self::Error> {
        match button {
            PointerButton::PRIMARY => Ok(Self::Primary),
            PointerButton::SECONDARY => Ok(Self::Secondary),
            PointerButton::AUXILIARY => Ok(Self::Tertiary),
            other => Err(other),
        }
    }
}
impl TapButton {
    /// Map a supported raw pointer button to its tap family.
    pub fn from_pointer_button(button: PointerButton) -> Option<Self> {
        Self::try_from(button).ok()
    }
}
/// Callback carrying the contact in local and root coordinates.
pub type TapCallback = Rc<dyn Fn(TapDetails)>;
/// Position and device kind of a tap contact.
#[derive(Debug, Clone, PartialEq)]
pub struct TapDetails {
    /// Position in the root coordinate space.
    pub global_position: Offset<f64>,
    /// Position in the recognizer coordinate space.
    pub local_position: Offset<f64>,
    /// Device kind frozen at admission.
    pub kind: PointerKind,
}

#[derive(Default)]
#[expect(clippy::struct_field_names)]
struct TapCallbacks {
    on_tap_down: Option<TapCallback>,
    on_tap_move: Option<TapCallback>,
    on_tap_up: Option<TapCallback>,
    on_tap: Option<TapCallback>,
    on_tap_cancel: Option<TapCallback>,
    on_secondary_tap_down: Option<TapCallback>,
    on_secondary_tap_up: Option<TapCallback>,
    on_secondary_tap: Option<TapCallback>,
    on_secondary_tap_cancel: Option<TapCallback>,
    on_tertiary_tap_down: Option<TapCallback>,
    on_tertiary_tap_up: Option<TapCallback>,
    on_tertiary_tap: Option<TapCallback>,
    on_tertiary_tap_cancel: Option<TapCallback>,
}
impl TapCallbacks {
    fn down(&self, button: TapButton) -> Option<TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_down.clone(),
            TapButton::Secondary => self.on_secondary_tap_down.clone(),
            TapButton::Tertiary => self.on_tertiary_tap_down.clone(),
        }
    }
    fn up(&self, button: TapButton) -> Option<TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_up.clone(),
            TapButton::Secondary => self.on_secondary_tap_up.clone(),
            TapButton::Tertiary => self.on_tertiary_tap_up.clone(),
        }
    }
    fn tap(&self, button: TapButton) -> Option<TapCallback> {
        match button {
            TapButton::Primary => self.on_tap.clone(),
            TapButton::Secondary => self.on_secondary_tap.clone(),
            TapButton::Tertiary => self.on_tertiary_tap.clone(),
        }
    }
    fn cancel(&self, button: TapButton) -> Option<TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_cancel.clone(),
            TapButton::Secondary => self.on_secondary_tap_cancel.clone(),
            TapButton::Tertiary => self.on_tertiary_tap_cancel.clone(),
        }
    }
}
impl Drop for TapCallbacks {
    fn drop(&mut self) {
        super::callback_containment::retire_callbacks!(self;
            on_tap_down, on_tap_move, on_tap_up, on_tap, on_tap_cancel,
            on_secondary_tap_down, on_secondary_tap_up, on_secondary_tap, on_secondary_tap_cancel,
            on_tertiary_tap_down, on_tertiary_tap_up, on_tertiary_tap, on_tertiary_tap_cancel
        );
    }
}

/// Configure callbacks before they become immutable. Use a `Weak` through an
/// external slot when a callback needs to reach its recognizer.
#[must_use]
pub struct TapGestureRecognizerBuilder {
    arena: GestureArena,
    settings: GestureSettings,
    callbacks: TapCallbacks,
}
impl std::fmt::Debug for TapGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapGestureRecognizerBuilder")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}
impl TapGestureRecognizerBuilder {
    /// Freeze gesture settings for all contacts admitted by this recognizer.
    pub fn settings(mut self, settings: GestureSettings) -> Self {
        self.settings = settings;
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tap_down(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tap_down = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tap_move(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tap_move = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tap_up(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tap_up = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tap(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tap = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tap_cancel(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tap_cancel = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_secondary_tap_down(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_secondary_tap_down = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_secondary_tap_up(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_secondary_tap_up = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_secondary_tap(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_secondary_tap = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_secondary_tap_cancel(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_secondary_tap_cancel = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tertiary_tap_down(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tertiary_tap_down = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tertiary_tap_up(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tertiary_tap_up = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tertiary_tap(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tertiary_tap = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_tertiary_tap_cancel(mut self, callback: impl Fn(TapDetails) + 'static) -> Self {
        self.callbacks.on_tertiary_tap_cancel = Some(Rc::new(callback));
        self
    }
    /// Allocate the owner-local recognizer with immutable callbacks.
    #[must_use]
    pub fn build(self) -> Rc<TapGestureRecognizer> {
        Rc::new_cyclic(|this| TapGestureRecognizer {
            arena: self.arena,
            this: this.clone(),
            sequences: RefCell::new(TapSequences::default()),
            settings: self.settings,
            callbacks: self.callbacks,
        })
    }
}

/// Recognizes primary, secondary and tertiary taps after their arena verdict.
pub struct TapGestureRecognizer {
    arena: GestureArena,
    this: Weak<Self>,
    sequences: RefCell<TapSequences>,
    settings: GestureSettings,
    callbacks: TapCallbacks,
}
impl std::fmt::Debug for TapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapGestureRecognizer")
            .field(
                "live_sequences",
                &self.sequences.try_borrow().map(|s| s.live.len()).ok(),
            )
            .finish_non_exhaustive()
    }
}
struct TapSequence {
    member: Rc<TapArenaMember>,
    id: ContactId,
    down: TapDetails,
    button: TapButton,
    up: Option<TapDetails>,
    accepted: bool,
    delivering: bool,
}
#[derive(Default)]
struct TapSequences {
    last_id: Cell<u64>,
    current: Option<ContactId>,
    live: SmallVec<[TapSequence; 2]>,
}
impl TapSequences {
    fn index(&self, id: ContactId) -> Option<usize> {
        self.live.iter().position(|sequence| sequence.id == id)
    }
    fn remove(&mut self, id: ContactId) -> Option<TapSequence> {
        if self.current == Some(id) {
            self.current = None;
        }
        let index = self.index(id)?;
        Some(self.live.remove(index))
    }
}
struct TapArenaMember {
    contact: PrimaryContact,
    entry: RefCell<Option<GestureArenaEntry>>,
    recognizer: Weak<TapGestureRecognizer>,
    sequence: ContactId,
}
impl GestureArenaMember for TapArenaMember {
    fn accept_gesture(&self, _: PointerId) {
        if let Some(recognizer) = self.recognizer.upgrade() {
            recognizer.accept_sequence(self.sequence);
        }
    }
    fn reject_gesture(&self, _: PointerId) {
        if let Some(recognizer) = self.recognizer.upgrade() {
            recognizer.reject_sequence(self.sequence);
        }
    }
}
impl Drop for TapArenaMember {
    fn drop(&mut self) {
        // A lifted sequence can still await a held verdict after its contact
        // finished; its independently retained weak token must withdraw too.
        if let Some(entry) = self.entry.get_mut().take() {
            entry.withdraw_deferred();
        }
    }
}
impl TapGestureRecognizer {
    /// Start configuring an owner-local recognizer.
    #[must_use]
    pub fn builder(arena: GestureArena) -> TapGestureRecognizerBuilder {
        TapGestureRecognizerBuilder {
            arena,
            settings: GestureSettings::default(),
            callbacks: TapCallbacks::default(),
        }
    }
    fn current_member(&self) -> Option<Rc<TapArenaMember>> {
        let sequences = self.sequences.borrow();
        let id = sequences.current?;
        Some(sequences.live[sequences.index(id)?].member.clone())
    }
    fn is_live(&self, id: ContactId) -> bool {
        self.sequences.borrow().index(id).is_some()
    }
    fn accept_sequence(&self, id: ContactId) {
        {
            let mut sequences = self.sequences.borrow_mut();
            let Some(index) = sequences.index(id) else {
                return;
            };
            sequences.live[index].accepted = true;
        }
        self.deliver_if_won(id);
    }
    fn reject_sequence(&self, id: ContactId) {
        let retired = self.sequences.borrow_mut().remove(id);
        if let Some(retired) = retired {
            retired.member.contact.withdraw();
        }
    }
    fn deliver_if_won(&self, id: ContactId) {
        let delivery = {
            let mut sequences = self.sequences.borrow_mut();
            let Some(index) = sequences.index(id) else {
                return;
            };
            let sequence = &mut sequences.live[index];
            if !sequence.accepted || sequence.delivering {
                return;
            }
            let Some(up) = sequence.up.clone() else {
                return;
            };
            sequence.delivering = true;
            (sequence.down.clone(), up, sequence.button)
        };
        let (down, up, button) = delivery;
        let mut run = CallbackSequence::new();
        run.call(self.callbacks.down(button), |callback| callback(down));
        if self.is_live(id) {
            run.call(self.callbacks.up(button), |callback| callback(up.clone()));
        }
        if self.is_live(id) {
            run.call(self.callbacks.tap(button), |callback| callback(up));
        }
        let retired = self.sequences.borrow_mut().remove(id);
        drop(retired);
        run.finish();
    }
    fn cancel_sequence(&self, id: ContactId, details: TapDetails) {
        let retired = self.sequences.borrow_mut().remove(id);
        if let Some(retired) = retired {
            invoke_callback(
                self.callbacks.cancel(retired.button),
                || {
                    retired.member.contact.cancel();
                },
                |callback| callback(details),
            );
        }
    }
    fn button(event: &PointerEvent) -> Option<TapButton> {
        match event {
            PointerEvent::Down(data) => TapButton::from_pointer_button(data.button()),
            PointerEvent::Up(data) => TapButton::from_pointer_button(data.button()),
            _ => None,
        }
    }
}
impl GestureRecognizer for TapGestureRecognizer {
    fn add_pointer(&self, dispatch: PointerDispatch<'_>) {
        let pointer = dispatch.local.pointer_id();
        let _span = tracing::info_span!(
            "tap.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        let PointerEvent::Down(_) = dispatch.local else {
            return;
        };
        let Some(button) = Self::button(dispatch.local) else {
            return;
        };
        if self.sequences.borrow().current.is_some() {
            return;
        }
        let id = {
            let sequences = self.sequences.borrow();
            let Some(id) = ContactId::next(&sequences.last_id) else {
                return;
            };
            id
        };
        let member = Rc::new_cyclic(|this: &Weak<TapArenaMember>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            TapArenaMember {
                contact: PrimaryContact::new(ArenaMembership::new(self.arena.clone(), member)),
                entry: RefCell::new(None),
                recognizer: self.this.clone(),
                sequence: id,
            }
        });
        if member.contact.begin(dispatch, &self.settings).is_err() {
            return;
        }
        *member.entry.borrow_mut() = member.contact.entry();
        let Some(snapshot) = member.contact.current() else {
            return;
        };
        let down = TapDetails {
            global_position: snapshot.global,
            local_position: snapshot.local,
            kind: snapshot.kind,
        };
        let mut sequences = self.sequences.borrow_mut();
        sequences.current = Some(id);
        sequences.live.push(TapSequence {
            member,
            id,
            down,
            button,
            up: None,
            accepted: false,
            delivering: false,
        });
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let _span = tracing::info_span!(
            "tap.handle_event",
            kind = %crate::observability::pointer_event_kind(dispatch.local),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        let Some(member) = self.current_member() else {
            return;
        };
        let Some(snapshot) = member.contact.current() else {
            return;
        };
        if !dispatch
            .local
            .pointer_id()
            .is_some_and(|pointer| member.contact.tracks(pointer))
        {
            return;
        }
        let details = TapDetails {
            local_position: dispatch.local.position().unwrap_or(snapshot.local),
            global_position: dispatch.global.position().unwrap_or(snapshot.global),
            kind: snapshot.kind,
        };
        match dispatch.local {
            PointerEvent::Move(_) => {
                if member.contact.moved_beyond(
                    details.local_position,
                    snapshot.settings.hit_slop(snapshot.kind),
                ) {
                    self.cancel_sequence(member.sequence, details);
                } else {
                    invoke_callback(
                        self.callbacks.on_tap_move.clone(),
                        || {},
                        |callback| callback(details),
                    );
                }
            }
            PointerEvent::Up(_) => {
                let Some(button) = Self::button(dispatch.local) else {
                    return;
                };
                let mismatch = {
                    let mut sequences = self.sequences.borrow_mut();
                    let Some(index) = sequences.index(member.sequence) else {
                        return;
                    };
                    let sequence = &mut sequences.live[index];
                    if sequence.button != button {
                        true
                    } else {
                        sequence.up = Some(details.clone());
                        sequences.current = None;
                        false
                    }
                };
                if mismatch {
                    self.cancel_sequence(member.sequence, details);
                } else {
                    member.contact.finish();
                    self.deliver_if_won(member.sequence);
                }
            }
            PointerEvent::Cancel(_) => self.cancel_sequence(
                member.sequence,
                TapDetails {
                    local_position: snapshot.local,
                    global_position: snapshot.global,
                    kind: snapshot.kind,
                },
            ),
            _ => {}
        }
    }
    fn cancel(&self) -> CancelOutcome {
        let live = {
            let mut sequences = self.sequences.borrow_mut();
            sequences.current = None;
            std::mem::take(&mut sequences.live)
        };
        if live.is_empty() {
            return CancelOutcome::Idle;
        }
        let incoming = std::thread::panicking();
        let mut first = None;
        for sequence in live {
            let candidate = RoutePanic::capture(|| {
                invoke_callback(
                    self.callbacks.cancel(sequence.button),
                    || {
                        sequence.member.contact.cancel();
                    },
                    |callback| callback(sequence.down),
                )
            });
            RoutePanic::preserve_first(&mut first, candidate, "tap cancellation");
        }
        finish_containment(first, incoming);
        CancelOutcome::Cancelled
    }
}
impl GestureArenaMember for TapGestureRecognizer {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, _: PointerId) {}
}
