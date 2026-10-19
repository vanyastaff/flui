//! Admission and identity of one owner-local pointer sequence.

use std::{
    cell::{Cell, RefCell},
    num::NonZeroU64,
    rc::Weak,
};

use core::time::Duration;
use flui_foundation::{
    geometry::Offset,
    panic::{FailureClass, RecoveryScope},
};

use flui_platform_api::EventTime;

use crate::{
    containment::RoutePanic,
    events::{PointerEvent, PointerEventExt, PointerKind},
    gesture::arena::{
        AdmissionContext, GestureArena, GestureArenaEntry, GestureArenaMember,
        GestureDeadlineRegistration, GestureDisposition, MotionRole, SweepModel,
    },
    gesture::observer::GestureContest,
    gesture::settings::GestureSettings,
    ids::PointerId,
    pointer::{PointerDispatch, pointer_capture::ContactFeeder},
};

use super::callback_containment::withdraw_cancelled;

/// Weak identity used to join an arena without creating an ownership cycle.
pub struct ArenaMembership {
    arena: GestureArena,
    this: Weak<dyn GestureArenaMember>,
    role: Option<MotionRole>,
    holds: bool,
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
        Self {
            arena,
            this,
            role: None,
            holds: false,
        }
    }

    /// Join every contact as a moving member, for the ownership table
    /// (ADR-0208): a slop crossing claims through
    /// [`GestureArenaEntry::claim_motion`].
    #[must_use]
    pub fn with_motion_role(mut self, role: MotionRole) -> Self {
        self.role = Some(role);
        self
    }

    /// Join every contact as a hold (ADR-0208 rule 4): the members of that
    /// contact that do not move, a tap or a long press, lose to it on
    /// admission, and such members joining after it are refused. Moving
    /// members still compete, and take the contact from the hold.
    #[must_use]
    pub fn holding_contact(mut self) -> Self {
        self.holds = true;
        self
    }

    /// Join an open competition, refusing a dead owner or closed arena.
    #[must_use]
    pub fn join(&self, pointer: PointerId) -> Option<GestureArenaEntry> {
        self.join_from(pointer, None)
    }

    /// Join for a Down `feeder` delivered: the route target whose other
    /// targets lose the contact if this member wins it.
    pub(crate) fn join_from(
        &self,
        pointer: PointerId,
        feeder: Option<ContactFeeder>,
    ) -> Option<GestureArenaEntry> {
        let member = self.this.upgrade()?;
        self.arena.try_add_erased(
            pointer,
            &member,
            AdmissionContext {
                role: self.role,
                holds: self.holds,
                feeder,
            },
        )
    }

    /// Read the arena clock. User-provided clocks may reenter the recognizer.
    #[must_use]
    pub fn now(&self) -> EventTime {
        self.arena.now()
    }

    /// The instant one interaction-clock reading names, for velocity samples.
    pub(crate) fn instant_at(&self, time: EventTime) -> web_time::Instant {
        self.arena.instant_at(time)
    }

    pub(crate) fn arena(&self) -> &GestureArena {
        &self.arena
    }

    pub(crate) fn register_deadline(
        &self,
        pointer: PointerId,
    ) -> Option<GestureDeadlineRegistration> {
        let member = self.this.upgrade()?;
        self.arena.register_deadline_member(pointer, &member).ok()
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
    /// The hardware behind the contact, when the platform reports it.
    pub device: Option<flui_platform_api::pointer::DeviceId>,
    /// The input sequence the binding minted at the contact's Down; `None`
    /// in an arena no binding drives (ADR-0207 §4).
    pub sequence: Option<flui_foundation::InputSequenceId>,
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
    deadline_registration: GestureDeadlineRegistration,
    deadline: Option<EventTime>,
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
            .join_from(pointer, down.feeder())
            .ok_or(BeginContactError::ArenaClosed)?;
        let contact = Contact {
            snapshot: ContactSnapshot {
                id,
                pointer,
                kind: data.pointer.kind,
                device: data.pointer.device,
                sequence: entry.sequence(),
                local,
                global,
                settings: settings.clone(),
            },
            entry,
            deadline_registration: registration,
            deadline: None,
        };
        *self.current.borrow_mut() = Some(contact);
        tracing::debug!(name: "recognizer.start_tracking", ?pointer,
            event = %crate::gesture::observability::GestureEvent::StartedTracking,
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

    /// Whether a verdict for `contest` settles this contact: the contest its
    /// Down joined, never an earlier one of the same pointer ID.
    #[must_use]
    pub fn tracks_contest(&self, contest: GestureContest) -> bool {
        self.current
            .borrow()
            .as_ref()
            .is_some_and(|contact| contact.entry.contest() == contest)
    }

    /// Claim this contact's exact arena entry.
    pub fn accept(&self) {
        self.arena().run_owned_recovery(|recovery| {
            self.accept_with_recovery(recovery);
        });
    }

    /// Claim this contact after crossing a slop, with the root-space
    /// displacement accumulated since Down; the arena's ownership table may
    /// give the contact to a competing moving member instead (ADR-0208 §2).
    pub fn claim_motion(&self, displacement: Offset<f64>) {
        if let Some(entry) = self.entry() {
            entry.claim_motion(displacement);
        }
    }

    pub(crate) fn accept_with_recovery(&self, recovery: &mut RecoveryScope<'_>) {
        let entry = self.entry();
        if let Some(entry) = entry {
            entry.resolve_with_recovery(GestureDisposition::Accepted, recovery);
        }
    }

    /// Bow out without cancelling the remaining competition.
    pub fn withdraw(&self) -> Option<ContactSnapshot> {
        let mut outgoing = None;
        self.arena().run_owned_recovery(|recovery| {
            outgoing = self.withdraw_with_recovery(recovery);
        });
        outgoing
    }

    pub(crate) fn withdraw_with_recovery(
        &self,
        recovery: &mut RecoveryScope<'_>,
    ) -> Option<ContactSnapshot> {
        let outgoing = self.current.borrow_mut().take()?;
        outgoing.deadline_registration.withdraw();
        outgoing.entry.reject_without_self_with_recovery(recovery);
        self.arena()
            .invoke_with_recovery(FailureClass::Delivery, recovery, |_| {
                tracing::debug!(name: "recognizer.reject", pointer = ?outgoing.snapshot.pointer,
                    event = %crate::gesture::observability::GestureEvent::ArenaRejected,
                    "recognizer rejected gesture");
            });
        Some(outgoing.snapshot)
    }

    /// Complete pointer-up tracking, sweeping only in a self-driven arena.
    pub fn finish(&self) -> Option<ContactSnapshot> {
        let snapshot = self.current()?;
        if let Some(contact) = self.current.borrow().as_ref() {
            contact.deadline_registration.withdraw();
        }
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
            event = %crate::gesture::observability::GestureEvent::StoppedTracking,
            "recognizer stopped tracking");
        Some(snapshot)
    }

    /// Arm from the owner clock, refusing overflow or a reentrant contact change.
    pub fn arm_deadline(&self, after: Duration) -> Option<EventTime> {
        let id = self.current()?.id;
        let deadline = self.arm_deadline_unpublished(after);
        if self.publish_deadline(id) {
            deadline
        } else {
            None
        }
    }

    /// Used when a recognizer must commit its matching phase before publication.
    pub(crate) fn arm_deadline_unpublished(&self, after: Duration) -> Option<EventTime> {
        let id = self.current()?.id;
        let deadline = self.membership.now().checked_add(after);
        let mut current = self.current.borrow_mut();
        let contact = current
            .as_mut()
            .filter(|contact| contact.snapshot.id == id)?;
        contact.deadline = deadline;
        deadline
    }

    pub(crate) fn publish_deadline(&self, id: ContactId) -> bool {
        let publication = self
            .current
            .borrow()
            .as_ref()
            .filter(|contact| contact.snapshot.id == id)
            .map(|contact| {
                (
                    contact.deadline_registration.update_handle(),
                    contact.deadline,
                )
            });
        if let Some((update, deadline)) = publication {
            if update.publish(deadline).is_ok() {
                return self.is_current(id);
            }
            let mut current = self.current.borrow_mut();
            if let Some(contact) = current.as_mut().filter(|contact| contact.snapshot.id == id) {
                contact.deadline = None;
                contact.deadline_registration.withdraw();
            }
        }
        false
    }

    /// Disarm this contact's deadline.
    pub fn disarm_deadline(&self) {
        let publication = {
            let mut current = self.current.borrow_mut();
            current.as_mut().map(|contact| {
                contact.deadline = None;
                contact.deadline_registration.update_handle()
            })
        };
        if let Some(update) = publication
            && update.publish(None).is_err()
            && let Some(contact) = self.current.borrow().as_ref()
        {
            contact.deadline_registration.withdraw();
        }
    }

    /// Pure deadline query; no clocks or callbacks run here.
    #[must_use]
    pub fn deadline(&self) -> Option<EventTime> {
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

    pub(crate) fn now(&self) -> EventTime {
        self.membership.now()
    }

    pub(crate) fn instant_at(&self, time: EventTime) -> web_time::Instant {
        self.membership.instant_at(time)
    }

    pub(crate) fn arena(&self) -> &GestureArena {
        self.membership.arena()
    }

    pub(crate) fn entry(&self) -> Option<GestureArenaEntry> {
        self.current
            .borrow()
            .as_ref()
            .map(|contact| contact.entry.clone())
    }

    pub(crate) fn cancel(&self) -> Option<ContactSnapshot> {
        let outgoing = self.current.borrow_mut().take()?;
        outgoing.deadline_registration.withdraw();
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
