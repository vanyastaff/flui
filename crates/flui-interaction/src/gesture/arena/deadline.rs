//! Scheduling authority and recovery for owner-local member deadlines.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use core::time::Duration;
use flui_foundation::panic::{FailureClass, PanicRecovery, RecoveryScope};
use flui_platform::PlatformWindow;
use smallvec::SmallVec;

use flui_platform::EventTime;

use super::{ArenaGeneration, GestureArenaMember, recovery::ArenaRecovery};
use crate::gesture::observer::{GestureContest, GestureObservation, ObservedMember, ObserverHub};
use crate::retain::Retain;

/// An explicit deadline authority could not be admitted or updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GestureDeadlineError {
    /// The owning presentation has withdrawn deadline admission.
    #[error("the gesture deadline owner is closed")]
    Closed,
    /// The exact registration has retired or its member no longer exists.
    #[error("the gesture deadline registration has retired")]
    Retired,
    /// This owner has permanently exhausted registration or revision identities.
    #[error("gesture deadline identities are exhausted")]
    Exhausted,
    /// This member already owns its one member-wide deadline registration.
    #[error("the gesture member already has a deadline registration")]
    AlreadyRegistered,
}

/// Linear ownership of one member-wide timer registration.
///
/// Arena resolution does not end its lifetime. Dropping the registration
/// withdraws its cached contribution without executing the member or a wake.
/// Use [`Self::update_handle`] to publish changes after committing recognizer
/// state and releasing its borrows.
#[must_use = "keep this registration alive while its deadline source is attached"]
pub struct GestureDeadlineRegistration {
    registry: Weak<DeadlineRegistry>,
    source: Rc<DeadlineSource>,
}

impl GestureDeadlineRegistration {
    /// Weak update authority for this exact registration.
    #[must_use]
    pub fn update_handle(&self) -> GestureDeadlineUpdate {
        GestureDeadlineUpdate {
            registry: self.registry.clone(),
            source: Rc::downgrade(&self.source),
        }
    }

    /// Withdraw cached timing before terminal notifications, without a callout.
    pub(crate) fn withdraw(&self) {
        self.source.active.set(false);
    }
}

impl std::fmt::Debug for GestureDeadlineRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDeadlineRegistration")
            .field("active", &self.source.active.get())
            .finish_non_exhaustive()
    }
}

impl Drop for GestureDeadlineRegistration {
    fn drop(&mut self) {
        self.withdraw();
        if let Some(registry) = self.registry.upgrade() {
            registry.unregister(self.source.id);
        }
    }
}

/// Owner-local authority to change one live registration's scheduling revision.
///
/// It keeps no member, contact or presentation alive. Every accepted publication
/// creates a new revision, including an unchanged numerical deadline, then wakes
/// the owner after framework borrows are released. A failed wake cannot roll back
/// the committed revision or cached observation.
#[derive(Clone)]
pub struct GestureDeadlineUpdate {
    registry: Weak<DeadlineRegistry>,
    source: Weak<DeadlineSource>,
}

impl GestureDeadlineUpdate {
    /// Publish the member's committed deadline or cancellation.
    ///
    /// # Errors
    /// Refuses a closed owner, retired registration or exhausted revision space.
    pub fn publish(&self, deadline: Option<EventTime>) -> Result<(), GestureDeadlineError> {
        let (registry, source) = self.live()?;
        source.advance()?;
        source.published.set(true);
        source.observation.set(Observation::Observed(deadline));
        registry.wake();
        Ok(())
    }

    /// Request a fresh pure-query revision, including after parked discovery.
    ///
    /// The query runs at a guard-free owner boundary; this call never executes
    /// the member. Use [`Self::publish`] when committed timing is already known.
    /// Unrelated owner operations reuse a successful observation. A completed
    /// due poll may query the member's changed timing again.
    ///
    /// # Errors
    /// Refuses a closed owner, retired registration or exhausted revision space.
    pub fn refresh(&self) -> Result<(), GestureDeadlineError> {
        let (registry, source) = self.live()?;
        source.advance()?;
        source.published.set(false);
        source.observation.set(Observation::Unobserved);
        registry.wake();
        Ok(())
    }

    fn live(&self) -> Result<(Rc<DeadlineRegistry>, Rc<DeadlineSource>), GestureDeadlineError> {
        let registry = self
            .registry
            .upgrade()
            .ok_or(GestureDeadlineError::Retired)?;
        if registry.closed.get() {
            return Err(GestureDeadlineError::Closed);
        }
        let source = self.source.upgrade().ok_or(GestureDeadlineError::Retired)?;
        if !source.active.get() || source.member.strong_count() == 0 {
            return Err(GestureDeadlineError::Retired);
        }
        Ok((registry, source))
    }
}

impl std::fmt::Debug for GestureDeadlineUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDeadlineUpdate")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
enum Observation {
    Unobserved,
    Observed(Option<EventTime>),
    Retry(EventTime),
    Parked,
}

impl Observation {
    /// Both native wake timing and pump admission observe the same armed debt.
    fn wake_deadline(self) -> Option<EventTime> {
        match self {
            Self::Observed(deadline) => deadline,
            Self::Retry(due) => Some(due),
            Self::Unobserved | Self::Parked => None,
        }
    }
}

fn query_retry(now: EventTime) -> Observation {
    now.checked_add(Duration::from_millis(100))
        .filter(|due| *due > now)
        .map_or(Observation::Parked, Observation::Retry)
}

struct DeadlineSource {
    id: u64,
    /// The contest the member was polled for: the latest one it joined.
    contest: Cell<Option<GestureContest>>,
    member: Weak<dyn GestureArenaMember>,
    registration: bool,
    generations: RefCell<SmallVec<[ArenaGeneration; 2]>>,
    active: Cell<bool>,
    revision: Cell<u64>,
    next_revision: Cell<u64>,
    published: Cell<bool>,
    observation: Cell<Observation>,
}

impl DeadlineSource {
    fn advance(&self) -> Result<(), GestureDeadlineError> {
        let revision = self.next_revision.get();
        if revision == 0 {
            return Err(GestureDeadlineError::Exhausted);
        }
        self.next_revision.set(revision.checked_add(1).unwrap_or(0));
        self.revision.set(revision);
        Ok(())
    }

    fn is_current(&self, revision: u64) -> bool {
        self.active.get() && self.revision.get() == revision
    }
}

pub(super) struct DeadlinePoll {
    pub(super) registration: Option<u64>,
    pub(super) contest: Option<GestureContest>,
    pub(super) member: Weak<dyn GestureArenaMember>,
    pub(super) generations: SmallVec<[ArenaGeneration; 2]>,
}

pub(super) struct DeadlineRegistry {
    next_id: Cell<u64>,
    closed: Cell<bool>,
    sources: RefCell<Vec<Rc<DeadlineSource>>>,
    wake: RefCell<Option<Weak<dyn PlatformWindow>>>,
    recovery: Rc<ArenaRecovery>,
}

impl DeadlineRegistry {
    pub(super) fn new(recovery: Rc<ArenaRecovery>) -> Self {
        Self {
            next_id: Cell::new(1),
            closed: Cell::new(false),
            sources: RefCell::new(Vec::new()),
            wake: RefCell::new(None),
            recovery,
        }
    }

    pub(super) fn register(
        self: &Rc<Self>,
        contest: Option<GestureContest>,
        member: &Rc<dyn GestureArenaMember>,
        generations: SmallVec<[ArenaGeneration; 2]>,
    ) -> Result<GestureDeadlineRegistration, GestureDeadlineError> {
        if self.closed.get() {
            return Err(GestureDeadlineError::Closed);
        }
        let weak = Rc::downgrade(member);
        if self.sources.borrow().iter().any(|source| {
            source.registration && source.active.get() && Weak::ptr_eq(&source.member, &weak)
        }) {
            return Err(GestureDeadlineError::AlreadyRegistered);
        }
        let id = self.allocate_id()?;
        for source in self
            .sources
            .borrow()
            .iter()
            .filter(|source| !source.registration && Weak::ptr_eq(&source.member, &weak))
        {
            source.active.set(false);
        }
        let source = Rc::new(Self::source(id, contest, weak, true, generations));
        self.sources.borrow_mut().push(Rc::clone(&source));
        Ok(GestureDeadlineRegistration {
            registry: Rc::downgrade(self),
            source,
        })
    }

    fn source(
        id: u64,
        contest: Option<GestureContest>,
        member: Weak<dyn GestureArenaMember>,
        registration: bool,
        generations: SmallVec<[ArenaGeneration; 2]>,
    ) -> DeadlineSource {
        DeadlineSource {
            id,
            contest: Cell::new(contest),
            member,
            registration,
            generations: RefCell::new(generations),
            active: Cell::new(true),
            revision: Cell::new(1),
            next_revision: Cell::new(2),
            published: Cell::new(false),
            observation: Cell::new(Observation::Unobserved),
        }
    }

    fn allocate_id(&self) -> Result<u64, GestureDeadlineError> {
        let id = self.next_id.get();
        if id == 0 {
            return Err(GestureDeadlineError::Exhausted);
        }
        self.next_id.set(id.checked_add(1).unwrap_or(0));
        Ok(id)
    }

    fn unregister(&self, id: u64) {
        Vec::retain(&mut *self.sources.borrow_mut(), |source| {
            source.id != id || !source.generations.borrow().is_empty()
        });
    }

    pub(super) fn admit_generation(
        &self,
        member: &Rc<dyn GestureArenaMember>,
        generation: ArenaGeneration,
        contest: GestureContest,
    ) {
        let weak = Rc::downgrade(member);
        for source in self.sources.borrow().iter().filter(|source| {
            source.registration && source.active.get() && Weak::ptr_eq(&source.member, &weak)
        }) {
            source.contest.set(Some(contest));
            let mut generations = source.generations.borrow_mut();
            if !generations.contains(&generation) {
                generations.push(generation);
            }
        }
    }

    pub(super) fn contains(&self, id: u64) -> bool {
        self.sources
            .borrow()
            .iter()
            .any(|source| source.id == id && source.active.get())
    }

    pub(super) fn snapshot(&self) -> SmallVec<[DeadlinePoll; 8]> {
        self.sources
            .borrow()
            .iter()
            .filter(|source| {
                source.registration && source.active.get() && source.member.strong_count() != 0
            })
            .map(|source| DeadlinePoll {
                registration: Some(source.id),
                contest: source.contest.get(),
                member: source.member.clone(),
                generations: SmallVec::new(),
            })
            .collect()
    }

    pub(super) fn set_wake(&self, window: Weak<dyn PlatformWindow>) {
        *self.wake.borrow_mut() = Some(window);
    }

    fn wake(&self) {
        let window = self
            .wake
            .borrow()
            .clone()
            .and_then(|window| window.upgrade());
        if let Some(window) = window {
            self.recovery.standalone(|scope| {
                self.recovery.invoke(FailureClass::Delivery, scope, |_| {
                    window.request_redraw();
                });
                if scope.has_failure() || self.recovery.preserving() {
                    std::mem::forget(window);
                } else {
                    self.recovery
                        .invoke(FailureClass::Unknown, scope, |_| drop(window));
                }
            });
        }
    }

    pub(super) fn close(&self) {
        self.closed.set(true);
        for source in self.sources.borrow().iter() {
            source.active.set(false);
        }
    }

    pub(super) fn withdraw_generation(&self, generation: ArenaGeneration) {
        for source in self.sources.borrow().iter() {
            let mut generations = source.generations.borrow_mut();
            generations.retain(|current| *current != generation);
            if generations.is_empty() && !source.registration {
                source.active.set(false);
            }
        }
    }

    pub(super) fn withdraw_member_generation(
        &self,
        member: &Weak<dyn GestureArenaMember>,
        generation: ArenaGeneration,
    ) {
        for source in self
            .sources
            .borrow()
            .iter()
            .filter(|source| Weak::ptr_eq(&source.member, member))
        {
            let mut generations = source.generations.borrow_mut();
            generations.retain(|current| *current != generation);
            if generations.is_empty() && !source.registration {
                source.active.set(false);
            }
        }
    }

    fn sources(&self, polls: SmallVec<[DeadlinePoll; 8]>) -> SmallVec<[Rc<DeadlineSource>; 8]> {
        let mut stored = self.sources.borrow_mut();
        Vec::retain(&mut *stored, |source| {
            (source.active.get() || !source.generations.borrow().is_empty())
                && source.member.strong_count() != 0
        });
        let mut selected = SmallVec::new();
        for poll in polls {
            let source = if let Some(id) = poll.registration {
                stored.iter().find(|source| source.id == id).cloned()
            } else {
                stored
                    .iter()
                    .find(|source| {
                        Weak::ptr_eq(&source.member, &poll.member)
                            && (!source.registration && source.active.get()
                                || !source.active.get()
                                    && poll.generations.iter().all(|generation| {
                                        source.generations.borrow().contains(generation)
                                    }))
                    })
                    .cloned()
            };
            let source = if let Some(source) = source {
                if !source.registration {
                    let new_admission = poll
                        .generations
                        .iter()
                        .any(|generation| !source.generations.borrow().contains(generation));
                    if new_admission {
                        if source.advance().is_ok() {
                            source.observation.set(Observation::Unobserved);
                        } else {
                            source.observation.set(Observation::Parked);
                        }
                    }
                }
                source
            } else {
                let Ok(id) = self.allocate_id() else { continue };
                let source = Rc::new(Self::source(
                    id,
                    poll.contest,
                    poll.member.clone(),
                    false,
                    poll.generations.clone(),
                ));
                stored.push(Rc::clone(&source));
                source
            };
            if poll.contest.is_some() {
                source.contest.set(poll.contest);
            }
            *source.generations.borrow_mut() = poll.generations;
            selected.push(source);
        }
        for source in stored.iter().filter(|source| !source.registration) {
            if !selected.iter().any(|selected| Rc::ptr_eq(source, selected)) {
                source.active.set(false);
            }
        }
        selected
    }

    pub(super) fn refresh(
        &self,
        polls: SmallVec<[DeadlinePoll; 8]>,
        now: EventTime,
        poll_due: bool,
        observers: &ObserverHub,
        recovery: &mut RecoveryScope<'_>,
    ) {
        if self.closed.get() {
            return;
        }
        for source in self.sources(polls) {
            if !source.active.get() {
                continue;
            }
            let Some(member) = source.member.upgrade() else {
                continue;
            };
            let revision = source.revision.get();
            let observation = source.observation.get();
            let mut preserving = recovery.has_failure();
            // Registered sources cache timing across unrelated owner work.
            // Legacy arena slots have no publisher and follow owner activity.
            let query = matches!(observation, Observation::Unobserved)
                || matches!(observation, Observation::Observed(_)) && !source.registration;
            let retry = matches!(observation, Observation::Retry(due) if due <= now);
            if query || retry {
                let mut result = None;
                let success = if retry {
                    // This existing obligation already reported its first fault.
                    // A failed re-probe retains its payload without repeating it.
                    let mut probe = PanicRecovery::new();
                    if self.recovery.preserving() {
                        probe.inherit_failure();
                    }
                    self.recovery
                        .invoke(FailureClass::Unknown, &mut probe.scope(), |_| {
                            result = Some(member.deadline());
                        })
                } else {
                    self.recovery.invoke(FailureClass::Unknown, recovery, |_| {
                        result = Some(member.deadline());
                    })
                };
                preserving |= retry && !success;
                if source.is_current(revision) {
                    source.observation.set(if success {
                        Observation::Observed(
                            result.expect("BUG: completed deadline query has a result"),
                        )
                    } else if retry {
                        Observation::Parked
                    } else {
                        query_retry(now)
                    });
                }
            }
            if poll_due
                && source.is_current(revision)
                && matches!(source.observation.get(), Observation::Observed(Some(due)) if due <= now)
            {
                observers.record(|| GestureObservation::DeadlineElapsed {
                    contest: source.contest.get(),
                    member: ObservedMember::new(source.member.clone()),
                    now,
                });
                let mut committed = false;
                let returned = self
                    .recovery
                    .invoke(FailureClass::Unknown, recovery, |scope| {
                        committed = member.poll_deadline_with_recovery(now, scope);
                    });
                let completed = returned && committed;
                if source.is_current(revision) {
                    if !completed {
                        source.observation.set(Observation::Parked);
                    } else if !source.published.get() {
                        let mut result = None;
                        let success = self.recovery.invoke(FailureClass::Unknown, recovery, |_| {
                            result = Some(member.deadline());
                        });
                        if source.is_current(revision) {
                            source.observation.set(if success {
                                Observation::Observed(
                                    result.expect("BUG: completed deadline query has a result"),
                                )
                            } else {
                                query_retry(now)
                            });
                        }
                    }
                }
            }
            if recovery.has_failure()
                || self.recovery.preserving()
                || preserving
                || matches!(source.observation.get(), Observation::Parked)
            {
                if Rc::strong_count(&member) == 1 {
                    // Exceptional custody keeps the opaque final owner, but
                    // does not re-admit a member its application already released.
                    source.active.set(false);
                }
                member.retain();
            } else {
                self.recovery
                    .invoke(FailureClass::Unknown, recovery, |_| drop(member));
            }
        }
    }

    pub(super) fn next_snapshot(&self) -> Option<EventTime> {
        if self.closed.get() {
            return None;
        }
        self.sources
            .borrow()
            .iter()
            .filter(|source| source.active.get() && source.member.strong_count() != 0)
            .filter_map(|source| source.observation.get().wake_deadline())
            .min()
    }

    pub(super) fn has_pending_snapshot(&self) -> bool {
        !self.closed.get()
            && self.sources.borrow().iter().any(|source| {
                source.active.get()
                    && source.member.strong_count() != 0
                    && source.observation.get().wake_deadline().is_some()
            })
    }
}

#[cfg(test)]
mod identity_exhaustion {
    use super::*;

    struct Member;
    impl GestureArenaMember for Member {
        fn accept_gesture(&self, _: GestureContest) {}
        fn reject_gesture(&self, _: GestureContest) {}
    }

    // Public calls cannot reach billions of registrations/revisions in a test.
    #[test]
    fn deadline_identity_exhaustion_refuses_permanently() {
        let registry = Rc::new(DeadlineRegistry::new(Rc::new(ArenaRecovery::default())));
        registry.next_id.set(u64::MAX);
        let member: Rc<dyn GestureArenaMember> = Rc::new(Member);
        let registration = registry
            .register(None, &member, SmallVec::new())
            .expect("last registration identity");
        let other: Rc<dyn GestureArenaMember> = Rc::new(Member);
        for _ in 0..2 {
            assert_eq!(
                registry.register(None, &other, SmallVec::new()).err(),
                Some(GestureDeadlineError::Exhausted)
            );
        }
        registration.source.next_revision.set(u64::MAX);
        let update = registration.update_handle();
        update
            .publish(Some(EventTime::from_nanos(1)))
            .expect("last revision identity");
        for _ in 0..2 {
            assert_eq!(update.refresh(), Err(GestureDeadlineError::Exhausted));
        }
    }
}
