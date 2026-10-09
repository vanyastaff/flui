//! Owner-local recognition of a tap involving several pointer contacts.

use super::{
    ArenaMembership, CancelOutcome, ContactId,
    callback_containment::{
        CallbackSequence, finish_containment, retire_callbacks, withdraw_cancelled,
    },
    recognizer::{GestureRecognizer, measured_positions},
};
use crate::{
    arena::{
        GestureArena, GestureArenaEntry, GestureArenaMember, GestureDeadlineRegistration,
        GestureDisposition,
    },
    events::{PointerEvent, PointerEventExt, PointerKind},
    ids::PointerId,
    routing::{PointerDispatch, RoutePanic},
    settings::{GestureSettings, GestureSettingsProvider},
};
use flui_foundation::geometry::Offset;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use web_time::{Duration, Instant};

/// Callback for completion or cancellation of a multi-contact tap.
pub type MultiTapCallback = Rc<dyn Fn(MultiTapDetails)>;
/// Positions and device kind of a multi-contact tap.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MultiTapDetails {
    /// Number of admitted contacts.
    pub pointer_count: usize,
    /// Initial local positions in pointer identity order.
    pub positions: Vec<Offset<f64>>,
    /// Center of the admitted positions.
    pub center: Offset<f64>,
    /// Kind of the first admitted contact.
    pub kind: PointerKind,
}
#[derive(Default)]
struct MultiTapCallbacks {
    on_multi_tap: Option<MultiTapCallback>,
    on_multi_tap_cancel: Option<MultiTapCallback>,
}
impl Drop for MultiTapCallbacks {
    fn drop(&mut self) {
        retire_callbacks!(self; on_multi_tap, on_multi_tap_cancel);
    }
}
struct PointerContact {
    initial: Offset<f64>,
    kind: PointerKind,
    down: bool,
    entry: GestureArenaEntry,
}
struct MultiTapSequence {
    id: ContactId,
    contacts: BTreeMap<PointerId, PointerContact>,
    kind: PointerKind,
    settings: GestureSettings,
    deadline: Option<Instant>,
    deadline_registration: Option<GestureDeadlineRegistration>,
}

/// Recognizes a tap after all required contacts have been released.
///
/// Callbacks are immutable after construction. Capture a `Weak` in an external
/// slot for reentry; storing a strong owner in that slot can form a cycle.
pub struct MultiTapGestureRecognizer {
    membership: ArenaMembership,
    sequence: RefCell<Option<MultiTapSequence>>,
    last_id: Cell<u64>,
    callbacks: MultiTapCallbacks,
    required_pointer_count: usize,
    settings: GestureSettingsProvider,
}
/// Immutable multi-tap policy and callbacks, consumed to create one owner.
#[must_use]
pub struct MultiTapGestureRecognizerBuilder {
    arena: GestureArena,
    callbacks: MultiTapCallbacks,
    required_pointer_count: usize,
    settings: GestureSettingsProvider,
}
impl MultiTapGestureRecognizerBuilder {
    /// Freeze gesture settings at admission.
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = settings.into();
        self
    }
    /// Called after all required contacts release.
    pub fn on_multi_tap(mut self, callback: impl Fn(MultiTapDetails) + 'static) -> Self {
        self.callbacks.on_multi_tap = Some(Rc::new(callback));
        self
    }
    /// Called on explicit cancellation, timeout, or lost arena competition.
    pub fn on_multi_tap_cancel(mut self, callback: impl Fn(MultiTapDetails) + 'static) -> Self {
        self.callbacks.on_multi_tap_cancel = Some(Rc::new(callback));
        self
    }
    /// Create the allocation used for dispatch and every pointer's competition.
    #[must_use]
    pub fn build(self) -> Rc<MultiTapGestureRecognizer> {
        Rc::new_cyclic(|this: &Weak<MultiTapGestureRecognizer>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            MultiTapGestureRecognizer {
                membership: ArenaMembership::new(self.arena, member),
                sequence: RefCell::new(None),
                last_id: Cell::new(0),
                callbacks: self.callbacks,
                required_pointer_count: self.required_pointer_count,
                settings: self.settings,
            }
        })
    }
}
impl MultiTapGestureRecognizer {
    /// Start immutable owner configuration for at least two contacts.
    ///
    /// # Panics
    /// Panics when `required_pointer_count` is less than two.
    pub fn builder(
        arena: GestureArena,
        required_pointer_count: usize,
    ) -> MultiTapGestureRecognizerBuilder {
        assert!(
            required_pointer_count >= 2,
            "MultiTapGestureRecognizer requires at least 2 pointers, got {required_pointer_count}"
        );
        MultiTapGestureRecognizerBuilder {
            arena,
            required_pointer_count,
            callbacks: MultiTapCallbacks::default(),
            settings: GestureSettingsProvider::default(),
        }
    }
    fn details(sequence: &MultiTapSequence) -> MultiTapDetails {
        let positions: Vec<_> = sequence
            .contacts
            .values()
            .map(|contact| contact.initial)
            .collect();
        // Normalize before summing: finite contact positions must not overflow
        // while computing their center, including contacts at opposite extremes.
        let mean = |axis: fn(&Offset<f64>) -> f64| {
            let sum = positions.iter().map(axis).sum::<f64>();
            if sum.is_finite() {
                return sum / positions.len() as f64;
            }
            let scale = positions.iter().map(axis).map(f64::abs).fold(0.0, f64::max);
            if scale == 0.0 {
                return 0.0;
            }
            let normalized = positions
                .iter()
                .map(|position| axis(position) / scale)
                .sum::<f64>()
                / positions.len() as f64;
            normalized.clamp(-1.0, 1.0) * scale
        };
        let center = Offset::new(mean(|position| position.dx), mean(|position| position.dy));
        MultiTapDetails {
            pointer_count: positions.len(),
            positions,
            center,
            kind: sequence.kind,
        }
    }
    fn retire_entries(
        &self,
        sequence: &MultiTapSequence,
        disposition: GestureDisposition,
        first: &mut Option<RoutePanic>,
    ) {
        for contact in sequence.contacts.values() {
            RoutePanic::preserve_first(
                first,
                RoutePanic::capture(|| {
                    if disposition.is_accepted() {
                        contact.entry.resolve(GestureDisposition::Accepted);
                    } else {
                        withdraw_cancelled(&contact.entry, self.membership.arena());
                    }
                }),
                "multi tap arena completion",
            );
            RoutePanic::preserve_first(
                first,
                RoutePanic::capture(|| contact.entry.release()),
                "multi tap hold release",
            );
        }
    }
    fn complete(&self, sequence: MultiTapSequence) {
        let details = Self::details(&sequence);
        let mut first = None;
        self.retire_entries(&sequence, GestureDisposition::Accepted, &mut first);
        let mut notices = CallbackSequence::new();
        notices.call(self.callbacks.on_multi_tap.clone(), |callback| {
            callback(details);
        });
        RoutePanic::preserve_first(
            &mut first,
            RoutePanic::capture(|| notices.finish()),
            "multi tap completion callback",
        );
        finish_containment(first, std::thread::panicking());
    }
}
impl GestureRecognizer for MultiTapGestureRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        let PointerEvent::Down(data) = down.local else {
            return;
        };
        let (Some(local), Some(global)) = (down.local.position(), down.global.position()) else {
            return;
        };
        if !local.dx.is_finite()
            || !local.dy.is_finite()
            || !global.dx.is_finite()
            || !global.dy.is_finite()
        {
            return;
        }
        let pointer = data.pointer.id;
        let generation = self.last_id.get();
        let existing = self.sequence.borrow().as_ref().map(|sequence| {
            (
                sequence.id,
                sequence.deadline,
                sequence.contacts.len(),
                sequence.contacts.contains_key(&pointer),
            )
        });
        if let Some((_, _, _, true)) = existing {
            return;
        }
        if existing.is_some_and(|(_, _, count, _)| count >= self.required_pointer_count) {
            self.cancel();
            return;
        }
        let settings = self.settings.snapshot();
        let now = self.membership.now();
        if self.last_id.get() != generation
            || self.sequence.borrow().as_ref().map(|sequence| sequence.id)
                != existing.map(|(id, _, _, _)| id)
        {
            return;
        }
        if existing
            .is_some_and(|(_, deadline, _, _)| deadline.is_some_and(|deadline| now >= deadline))
        {
            self.cancel();
            if self.last_id.get() == generation && self.sequence.borrow().is_none() {
                self.add_pointer(down);
            }
            return;
        }
        let Some(entry) = self.membership.join(pointer) else {
            return;
        };
        entry.hold();
        if self.last_id.get() != generation
            || self.sequence.borrow().as_ref().map(|sequence| sequence.id)
                != existing.map(|(id, _, _, _)| id)
        {
            entry.release_deferred();
            entry.withdraw_deferred();
            return;
        }
        let registration = if existing.is_none() {
            self.membership.register_deadline(pointer)
        } else {
            None
        };
        if existing.is_none() && registration.is_none() {
            entry.release_deferred();
            entry.withdraw_deferred();
            return;
        }
        let outgoing_registration = {
            let mut state = self.sequence.borrow_mut();
            if state.is_none() {
                let Some(id) = ContactId::next(&self.last_id) else {
                    drop(state);
                    entry.release_deferred();
                    entry.withdraw_deferred();
                    return;
                };
                *state = Some(MultiTapSequence {
                    id,
                    contacts: BTreeMap::new(),
                    kind: data.pointer.kind,
                    settings,
                    deadline: now.checked_add(Duration::from_millis(100)),
                    deadline_registration: registration,
                });
            }
            let sequence = state.as_mut().expect("BUG: multi tap sequence admitted");
            sequence.contacts.insert(
                pointer,
                PointerContact {
                    initial: local,
                    kind: data.pointer.kind,
                    down: true,
                    entry,
                },
            );
            if sequence.contacts.len() == self.required_pointer_count {
                sequence.deadline = None;
                sequence.deadline_registration.take()
            } else {
                None
            }
        };
        drop(outgoing_registration);
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let Some(pointer) = dispatch.local.pointer_id() else {
            return;
        };
        match dispatch.local {
            PointerEvent::Move(_) => {
                let Some(position) = dispatch.local.position() else {
                    return;
                };
                let exceeded = {
                    let state = self.sequence.borrow();
                    let Some(sequence) = state.as_ref() else {
                        return;
                    };
                    let Some(contact) = sequence.contacts.get(&pointer) else {
                        return;
                    };
                    !position.dx.is_finite()
                        || !position.dy.is_finite()
                        || measured_positions(dispatch.local).any(|position| {
                            let delta = position - contact.initial;
                            sequence.settings.exceeds_hit_slop(contact.kind, delta)
                        })
                };
                if exceeded {
                    self.cancel();
                }
            }
            PointerEvent::Up(_) => {
                let completed = {
                    let mut state = self.sequence.borrow_mut();
                    let Some(sequence) = state.as_mut() else {
                        return;
                    };
                    let Some(contact) = sequence.contacts.get_mut(&pointer) else {
                        return;
                    };
                    contact.down = false;
                    if sequence.contacts.len() == self.required_pointer_count
                        && sequence.contacts.values().all(|contact| !contact.down)
                    {
                        state.take()
                    } else {
                        None
                    }
                };
                if let Some(sequence) = completed {
                    self.complete(sequence);
                }
            }
            PointerEvent::Cancel(_) => {
                let tracks = self
                    .sequence
                    .borrow()
                    .as_ref()
                    .is_some_and(|sequence| sequence.contacts.contains_key(&pointer));
                if tracks {
                    self.cancel();
                }
            }
            _ => {}
        }
    }
    fn cancel(&self) -> CancelOutcome {
        let Some(sequence) = self.sequence.borrow_mut().take() else {
            return CancelOutcome::Idle;
        };
        let details = Self::details(&sequence);
        let mut first = None;
        self.retire_entries(&sequence, GestureDisposition::Rejected, &mut first);
        let mut notices = CallbackSequence::new();
        notices.call(self.callbacks.on_multi_tap_cancel.clone(), |callback| {
            callback(details);
        });
        RoutePanic::preserve_first(
            &mut first,
            RoutePanic::capture(|| notices.finish()),
            "multi tap cancellation callback",
        );
        finish_containment(first, std::thread::panicking());
        CancelOutcome::Cancelled
    }
}
impl GestureArenaMember for MultiTapGestureRecognizer {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, pointer: PointerId) {
        let tracks = self
            .sequence
            .borrow()
            .as_ref()
            .is_some_and(|sequence| sequence.contacts.contains_key(&pointer));
        if tracks {
            self.cancel();
        }
    }
    fn deadline(&self) -> Option<Instant> {
        self.sequence
            .borrow()
            .as_ref()
            .and_then(|sequence| sequence.deadline)
    }
    fn poll_deadline(&self, now: Instant) {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            self.cancel();
        }
    }
}
impl Drop for MultiTapGestureRecognizer {
    fn drop(&mut self) {
        if let Some(sequence) = self.sequence.get_mut().take() {
            for contact in sequence.contacts.values() {
                contact.entry.release_deferred();
                contact.entry.withdraw_deferred();
            }
        }
    }
}
impl std::fmt::Debug for MultiTapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiTapGestureRecognizer")
            .field("required_pointer_count", &self.required_pointer_count)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for MultiTapGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiTapGestureRecognizerBuilder")
            .field("required_pointer_count", &self.required_pointer_count)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}
