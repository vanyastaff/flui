//! Admission and identity of one owner-local pointer sequence.

use std::{
    cell::{Cell, RefCell},
    num::NonZeroU64,
    rc::Weak,
};

use flui_foundation::geometry::Offset;
use web_time::{Duration, Instant};

use crate::{
    arena::{
        GestureArena, GestureArenaEntry, GestureArenaMember, GestureDeadlineRegistration,
        GestureDisposition, SweepModel,
    },
    events::{PointerEvent, PointerEventExt, PointerKind},
    ids::PointerId,
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
};

use super::callback_containment::withdraw_cancelled;

/// Weak identity used to join an arena without creating an ownership cycle.
pub struct ArenaMembership {
    arena: GestureArena,
    this: Weak<dyn GestureArenaMember>,
}

impl std::fmt::Debug for ArenaMembership {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArenaMembership")
            .field("owner_alive", &(self.this.strong_count() != 0))
            .finish_non_exhaustive()
    }
}

impl ArenaMembership {
    /// Bind an arena to the exact member allocation created by `Rc::new_cyclic`.
    pub fn new(arena: GestureArena, this: Weak<dyn GestureArenaMember>) -> Self {
        Self { arena, this }
    }

    /// Join an open competition, refusing a dead owner or closed arena.
    #[must_use]
    pub fn join(&self, pointer: PointerId) -> Option<GestureArenaEntry> {
        let member = self.this.upgrade()?;
        self.arena.try_add_erased(pointer, &member)
    }

    /// Read the arena clock. User-provided clocks may reenter the recognizer.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.arena.now()
    }

    pub(crate) fn arena(&self) -> &GestureArena {
        &self.arena
    }

    pub(crate) fn register_deadline(
        &self,
        pointer: PointerId,
    ) -> Option<GestureDeadlineRegistration> {
        let member = self.this.upgrade()?;
        Some(self.arena.register_deadline_member(pointer, &member))
    }
}

/// Identity of one admission, independent of a reusable pointer identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContactId(NonZeroU64);

impl ContactId {
    pub(crate) fn next(last: &Cell<u64>) -> Option<Self> {
        let next = last.get().checked_add(1)?;
        last.set(next);
        Some(Self(
            NonZeroU64::new(next).expect("BUG: contact identity is nonzero"),
        ))
    }
}

/// Owned admission snapshot. Settings apply to this whole sequence.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ContactSnapshot {
    /// Exact admission identity.
    pub id: ContactId,
    /// Device pointer identifier, which may be reused by a later sequence.
    pub pointer: PointerId,
    /// Device kind captured on Down.
    pub kind: PointerKind,
    /// Down position in recognizer coordinates.
    pub local: Offset<f64>,
    /// Down position in root coordinates.
    pub global: Offset<f64>,
    /// Settings frozen at admission.
    pub settings: GestureSettings,
}

/// A pointer sequence could not be admitted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BeginContactError {
    /// Another sequence already owns this contact.
    #[error("already tracking {current:?}")]
    Busy {
        /// Pointer that remains tracked.
        current: PointerId,
    },
    /// Both coordinate spaces must contain finite values.
    #[error("down position is not finite")]
    NonFinite,
    /// Arena membership was refused.
    #[error("arena refused the member")]
    ArenaClosed,
    /// Admission requires a Down event.
    #[error("contact admission requires a down event")]
    NotDown,
    /// This owner has permanently exhausted its admission identities.
    #[error("contact identities exhausted")]
    Exhausted,
}

struct Contact {
    snapshot: ContactSnapshot,
    entry: GestureArenaEntry,
    _deadline_registration: GestureDeadlineRegistration,
    deadline: Option<Instant>,
}

/// One pointer sequence with exact arena and deadline lifetimes.
pub struct PrimaryContact {
    membership: ArenaMembership,
    current: RefCell<Option<Contact>>,
    last_id: Cell<u64>,
}

impl std::fmt::Debug for PrimaryContact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = self.current();
        f.debug_struct("PrimaryContact")
            .field("current", &current)
            .finish_non_exhaustive()
    }
}

impl PrimaryContact {
    /// Construct an idle contact for an exact weak member identity.
    pub fn new(membership: ArenaMembership) -> Self {
        Self {
            membership,
            current: RefCell::new(None),
            last_id: Cell::new(0),
        }
    }

    /// Admit a finite Down without replacing an existing sequence.
    ///
    /// # Errors
    /// Refuses non-Down or non-finite input, an already active contact, closed
    /// arena membership, and permanently exhausted contact identities.
    pub fn begin(
        &self,
        down: PointerDispatch<'_>,
        settings: &GestureSettings,
    ) -> Result<ContactId, BeginContactError> {
        let PointerEvent::Down(data) = down.local else {
            return Err(BeginContactError::NotDown);
        };
        if let Some(current) = self.current() {
            return Err(BeginContactError::Busy {
                current: current.pointer,
            });
        }
        let local = down.local.position().ok_or(BeginContactError::NonFinite)?;
        let global = down.global.position().ok_or(BeginContactError::NonFinite)?;
        if !finite(local) || !finite(global) {
            return Err(BeginContactError::NonFinite);
        }
        let id = ContactId::next(&self.last_id).ok_or(BeginContactError::Exhausted)?;
        let pointer = data.pointer.id;
        // Complete fallible private allocation before arena admission. The
        // registration token retires silently when admission is refused.
        let registration = self
            .membership
            .register_deadline(pointer)
            .ok_or(BeginContactError::ArenaClosed)?;
        let entry = self
            .membership
            .join(pointer)
            .ok_or(BeginContactError::ArenaClosed)?;
        let contact = Contact {
            snapshot: ContactSnapshot {
                id,
                pointer,
                kind: data.pointer.kind,
                local,
                global,
                settings: settings.clone(),
            },
            entry,
            _deadline_registration: registration,
            deadline: None,
        };
        *self.current.borrow_mut() = Some(contact);
        tracing::debug!(name: "recognizer.start_tracking", ?pointer,
            event = %crate::observability::GestureEvent::StartedTracking,
            "recognizer started tracking");
        Ok(id)
    }

    /// Return an owned view without retaining an internal borrow.
    #[must_use]
    pub fn current(&self) -> Option<ContactSnapshot> {
        self.current
            .borrow()
            .as_ref()
            .map(|contact| contact.snapshot.clone())
    }

    /// Whether delivery still belongs to this exact admission.
    #[must_use]
    pub fn is_current(&self, id: ContactId) -> bool {
        self.current
            .borrow()
            .as_ref()
            .is_some_and(|contact| contact.snapshot.id == id)
    }

    /// Whether this contact tracks the supplied device pointer.
    #[must_use]
    pub fn tracks(&self, pointer: PointerId) -> bool {
        self.current
            .borrow()
            .as_ref()
            .is_some_and(|contact| contact.snapshot.pointer == pointer)
    }

    /// Claim this contact's exact arena entry.
    pub fn accept(&self) {
        let entry = self.entry();
        if let Some(entry) = entry {
            entry.resolve(GestureDisposition::Accepted);
        }
    }

    /// Bow out without cancelling the remaining competition.
    pub fn withdraw(&self) -> Option<ContactSnapshot> {
        let outgoing = self.current.borrow_mut().take()?;
        outgoing.entry.reject_without_self();
        tracing::debug!(name: "recognizer.reject", pointer = ?outgoing.snapshot.pointer,
            event = %crate::observability::GestureEvent::ArenaRejected,
            "recognizer rejected gesture");
        Some(outgoing.snapshot)
    }

    /// Complete pointer-up tracking, sweeping only in a self-driven arena.
    pub fn finish(&self) -> Option<ContactSnapshot> {
        let snapshot = self.current()?;
        let entry = self.entry();
        let failure = RoutePanic::capture(|| {
            if self.membership.arena.sweep_model() == SweepModel::SelfDriven
                && let Some(entry) = entry
            {
                entry.sweep();
            }
        });
        let outgoing = {
            let mut current = self.current.borrow_mut();
            if current
                .as_ref()
                .is_some_and(|contact| contact.snapshot.id == snapshot.id)
            {
                current.take()
            } else {
                None
            }
        };
        drop(outgoing);
        if let Some(failure) = failure {
            failure.resume();
        }
        tracing::debug!(name: "recognizer.stop_tracking", pointer = ?snapshot.pointer,
            event = %crate::observability::GestureEvent::StoppedTracking,
            "recognizer stopped tracking");
        Some(snapshot)
    }

    /// Arm from the owner clock, refusing overflow or a reentrant contact change.
    pub fn arm_deadline(&self, after: Duration) -> Option<Instant> {
        let id = self.current()?.id;
        let deadline = self.membership.now().checked_add(after);
        let mut current = self.current.borrow_mut();
        let contact = current
            .as_mut()
            .filter(|contact| contact.snapshot.id == id)?;
        contact.deadline = deadline;
        deadline
    }

    /// Disarm this contact's deadline.
    pub fn disarm_deadline(&self) {
        if let Some(contact) = self.current.borrow_mut().as_mut() {
            contact.deadline = None;
        }
    }

    /// Pure deadline query; no clocks or callbacks run here.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.current
            .borrow()
            .as_ref()
            .and_then(|contact| contact.deadline)
    }

    /// Test movement from admission; invalid geometry exceeds every tolerance.
    #[must_use]
    pub fn moved_beyond(&self, local: Offset<f64>, slop: f64) -> bool {
        if !finite(local) || !slop.is_finite() || slop < 0.0 {
            return true;
        }
        self.current().is_some_and(|contact| {
            let delta = local - contact.local;
            delta.dx.hypot(delta.dy) > slop
        })
    }

    pub(crate) fn now(&self) -> Instant {
        self.membership.now()
    }

    pub(crate) fn entry(&self) -> Option<GestureArenaEntry> {
        self.current
            .borrow()
            .as_ref()
            .map(|contact| contact.entry.clone())
    }

    pub(crate) fn cancel(&self) -> Option<ContactSnapshot> {
        let outgoing = self.current.borrow_mut().take()?;
        withdraw_cancelled(&outgoing.entry, &self.membership.arena);
        Some(outgoing.snapshot)
    }
}

impl Drop for PrimaryContact {
    fn drop(&mut self) {
        if let Some(contact) = self.current.get_mut().take() {
            contact.entry.withdraw_deferred();
        }
    }
}

fn finite(position: Offset<f64>) -> bool {
    position.dx.is_finite() && position.dy.is_finite()
}

#[cfg(test)]
mod identity_exhaustion {
    use super::ContactId;
    use std::cell::Cell;

    // The final counter state cannot be reached through a practical public run.
    #[test]
    fn contact_identity_exhaustion_refuses_permanently() {
        let last = Cell::new(u64::MAX - 1);
        let final_id = ContactId::next(&last).expect("last identity remains available");
        assert_eq!(final_id.0.get(), u64::MAX);
        assert!(ContactId::next(&last).is_none());
        assert!(ContactId::next(&last).is_none());
    }
}
