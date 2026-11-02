//! Run membership and quit consultation. All mutations are serialized by the
//! ingress lock; native notifications and application callbacks run afterwards.

use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroU64;
use std::sync::Arc;

use flui_foundation::{IdExhausted, IdMint, PresentationAddress};
use flui_native::PlatformProxy;

use super::QuitAnswer;
use crate::app::owner_document::{DocumentRefusal, Envelope};

/// An application incarnation. Keeping the allocation alive prevents address
/// reuse while a delayed answer still names this run.
#[derive(Clone, Debug)]
struct RunId(Arc<()>);

impl PartialEq for RunId {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for RunId {}

/// Never reissued within a run, including after a failed start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OwnerId(NonZeroU64);

impl OwnerId {
    pub(crate) const INITIAL: Self = Self(NonZeroU64::MIN);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuitTicket {
    run: RunId,
    owner: OwnerId,
    round: NonZeroU64,
    revision: NonZeroU64,
}

#[derive(Debug)]
enum Vote {
    Unasked,
    Consulting(QuitTicket),
    Waiting(Vec<PresentationAddress>),
    Consented,
}

#[derive(Debug)]
struct Member {
    retiring: bool,
    thread_result: Option<std::thread::Result<()>>,
    ordinary: VecDeque<Envelope>,
    in_flight: usize,
    retained_bytes: usize,
    proxy: Option<PlatformProxy>,
    revision: NonZeroU64,
    pending: usize,
    vote: Vote,
}

pub(super) struct FinishedOwner {
    pub(super) result: std::thread::Result<()>,
    pub(super) proxy: Option<PlatformProxy>,
    pub(super) commands: VecDeque<Envelope>,
}

/// The reliable control lane has one reserved answer slot per admitted owner.
/// A new round replaces consultation, never an owner's recorded close guard.
#[derive(Debug)]
pub(super) struct Membership {
    run: RunId,
    owners: IdMint<NonZeroU64>,
    revisions: IdMint<NonZeroU64>,
    rounds: IdMint<NonZeroU64>,
    requests: IdMint<NonZeroU64>,
    round: Option<NonZeroU64>,
    members: BTreeMap<OwnerId, Member>,
}

impl Membership {
    pub(super) fn new(proxy: PlatformProxy) -> Self {
        Self {
            run: RunId(Arc::new(())),
            owners: IdMint::starting_after(1),
            revisions: IdMint::starting_after(1),
            rounds: IdMint::new(),
            requests: IdMint::new(),
            round: None,
            members: BTreeMap::from([(
                OwnerId::INITIAL,
                Member {
                    retiring: false,
                    thread_result: None,
                    ordinary: VecDeque::new(),
                    in_flight: 0,
                    retained_bytes: 0,
                    proxy: Some(proxy),
                    revision: NonZeroU64::MIN,
                    pending: 0,
                    vote: Vote::Unasked,
                },
            )]),
        }
    }

    pub(super) fn request_quit(&mut self) -> Result<(), IdExhausted> {
        let round = self.rounds.next()?;
        self.round = Some(round);
        for member in self.members.values_mut() {
            member.vote = if member.retiring {
                Vote::Consented
            } else {
                Vote::Unasked
            };
        }
        Ok(())
    }

    pub(super) fn cancel_quit(&mut self) {
        self.round = None;
    }

    pub(super) fn has_quit(&self) -> bool {
        self.round.is_some()
    }

    pub(super) fn changed(&mut self, owner: OwnerId) -> Result<(), IdExhausted> {
        let revision = self.revisions.next()?;
        let member = self
            .members
            .get_mut(&owner)
            .expect("BUG: changed owner is registered");
        member.revision = revision;
        member.vote = Vote::Unasked;
        Ok(())
    }

    pub(super) fn proxies(&self) -> Vec<PlatformProxy> {
        self.members
            .values()
            .filter_map(|member| member.proxy.clone())
            .collect::<Vec<_>>()
    }

    pub(super) fn count(&self) -> usize {
        self.members.len()
    }

    pub(super) fn ordinary_count(&self, owner: OwnerId) -> usize {
        self.members
            .get(&owner)
            .map_or(0, |member| member.in_flight)
    }

    pub(super) fn reserve_owner(&mut self) -> Result<OwnerId, IdExhausted> {
        let owner = OwnerId(self.owners.next()?);
        let revision = self.revisions.next()?;
        self.members.insert(
            owner,
            Member {
                retiring: false,
                thread_result: None,
                ordinary: VecDeque::new(),
                in_flight: 0,
                retained_bytes: 0,
                proxy: None,
                revision,
                pending: 1,
                vote: Vote::Unasked,
            },
        );
        Ok(owner)
    }

    pub(super) fn attach(&mut self, owner: OwnerId, proxy: PlatformProxy) -> bool {
        let Some(member) = self.members.get_mut(&owner) else {
            return false;
        };
        if member.proxy.is_some() {
            return false;
        }
        member.proxy = Some(proxy);
        true
    }

    pub(super) fn retire(&mut self, owner: OwnerId) -> (Option<PlatformProxy>, VecDeque<Envelope>) {
        self.members
            .remove(&owner)
            .map(|member| (member.proxy, member.ordinary))
            .unwrap_or_default()
    }

    pub(super) fn quiesce(
        &mut self,
        owner: OwnerId,
    ) -> (Option<PlatformProxy>, VecDeque<Envelope>) {
        let member = self
            .members
            .get_mut(&owner)
            .expect("BUG: retiring owner remains registered");
        member.retiring = true;
        member.vote = Vote::Consented;
        (member.proxy.take(), std::mem::take(&mut member.ordinary))
    }

    pub(super) fn thread_finished(&mut self, owner: OwnerId, result: std::thread::Result<()>) {
        let member = self
            .members
            .get_mut(&owner)
            .expect("BUG: join retains membership");
        assert!(
            member.thread_result.is_none(),
            "BUG: one join result per owner"
        );
        member.thread_result = Some(result);
    }

    pub(super) fn take_finished(&mut self) -> Vec<FinishedOwner> {
        self.members
            .extract_if(.., |_, member| member.thread_result.is_some())
            .map(|(_, member)| FinishedOwner {
                result: member
                    .thread_result
                    .expect("BUG: completed owner has a join result"),
                proxy: member.proxy,
                commands: member.ordinary,
            })
            .collect::<Vec<_>>()
    }

    pub(super) fn reserve_command(
        &mut self,
        owner: OwnerId,
        bytes: usize,
        limits: crate::app::OwnerLimits,
    ) -> Result<NonZeroU64, DocumentRefusal> {
        let member = self
            .members
            .get_mut(&owner)
            .ok_or(DocumentRefusal::OwnerGone)?;
        if member.retiring {
            return Err(DocumentRefusal::OwnerGone);
        }
        if bytes > limits.payload_bytes.get() {
            return Err(DocumentRefusal::Oversize);
        }
        let retained = member
            .retained_bytes
            .checked_add(bytes)
            .ok_or(DocumentRefusal::Full)?;
        if member.in_flight >= limits.commands.get() || retained > limits.payload_bytes.get() {
            return Err(DocumentRefusal::Full);
        }
        let sequence = self
            .requests
            .next()
            .map_err(|_| DocumentRefusal::IdentityExhausted)?;
        member.in_flight += 1;
        member.retained_bytes = retained;
        Ok(sequence)
    }

    pub(super) fn enqueue(&mut self, owner: OwnerId, command: Envelope) {
        self.members
            .get_mut(&owner)
            .expect("BUG: admitted owner exists until enqueue commits")
            .ordinary
            .push_back(command);
    }

    pub(super) fn release_budget(&mut self, owner: OwnerId, bytes: usize) {
        if let Some(member) = self.members.get_mut(&owner) {
            member.in_flight = member
                .in_flight
                .checked_sub(1)
                .expect("BUG: one reserved completion per command");
            member.retained_bytes = member
                .retained_bytes
                .checked_sub(bytes)
                .expect("BUG: payload reservation settled once");
        }
    }

    pub(super) fn drain_commands(&mut self) -> Vec<Envelope> {
        self.members
            .values_mut()
            .flat_map(|member| std::mem::take(&mut member.ordinary))
            .collect::<Vec<_>>()
    }

    /// Keep the initial native loop alive until every worker has released its
    /// native resources. It is the final owner of the application's services.
    pub(super) fn retiring_proxies(&self) -> Vec<PlatformProxy> {
        self.members
            .iter()
            .filter(|(owner, _)| self.members.len() == 1 || **owner != OwnerId::INITIAL)
            .filter_map(|(_, member)| member.proxy.clone())
            .collect::<Vec<_>>()
    }

    /// Reserve membership before accepting a local launch. The publication
    /// and its rollback both owe `finish_launch`, even if its observer drops.
    pub(super) fn begin_launch(&mut self, owner: OwnerId) -> Result<(), IdExhausted> {
        let revision = self.revisions.next()?;
        let member = self
            .members
            .get_mut(&owner)
            .expect("BUG: launch owner is registered");
        let pending = member.pending.checked_add(1).ok_or(IdExhausted)?;
        member.revision = revision;
        member.pending = pending;
        member.vote = Vote::Unasked;
        Ok(())
    }

    pub(super) fn finish_launch(&mut self, owner: OwnerId) {
        let member = self
            .members
            .get_mut(&owner)
            .expect("BUG: launch reservation keeps its owner");
        member.pending = member
            .pending
            .checked_sub(1)
            .expect("BUG: launch settled once");
    }

    pub(super) fn begin_consult(&mut self, owner: OwnerId) -> Option<QuitTicket> {
        let round = self.round?;
        let member = self.members.get_mut(&owner)?;
        if member.pending != 0 || !matches!(member.vote, Vote::Unasked) {
            return None;
        }
        let ticket = QuitTicket {
            run: self.run.clone(),
            owner,
            round,
            revision: member.revision,
        };
        member.vote = Vote::Consulting(ticket.clone());
        Some(ticket)
    }

    pub(super) fn answer(&mut self, ticket: QuitTicket, answer: QuitAnswer) {
        if ticket.run != self.run || self.round != Some(ticket.round) {
            return;
        }
        let Some(member) = self.members.get_mut(&ticket.owner) else {
            return;
        };
        if !matches!(&member.vote, Vote::Consulting(current) if *current == ticket) {
            return;
        }
        match answer {
            QuitAnswer::Cancelled => self.round = None,
            QuitAnswer::Waiting(waiting) if waiting.is_empty() => member.vote = Vote::Consented,
            QuitAnswer::Waiting(waiting) => member.vote = Vote::Waiting(waiting),
        }
    }

    pub(super) fn waiting(&self, owner: OwnerId) -> Vec<PresentationAddress> {
        if self.round.is_none() {
            return Vec::new();
        }
        match self.members.get(&owner).map(|member| &member.vote) {
            Some(Vote::Waiting(waiting)) => waiting.clone(),
            _ => Vec::new(),
        }
    }

    pub(super) fn consent(&mut self, owner: OwnerId, address: PresentationAddress) {
        if self.round.is_none() {
            return;
        }
        if let Some(member) = self.members.get_mut(&owner)
            && let Vote::Waiting(waiting) = &mut member.vote
        {
            waiting.retain(|waited| *waited != address);
            if waiting.is_empty() {
                member.vote = Vote::Consented;
            }
        }
    }

    pub(super) fn decline(&mut self, owner: OwnerId, address: PresentationAddress) {
        if self.members.get(&owner).is_some_and(
            |member| matches!(&member.vote, Vote::Waiting(waiting) if waiting.contains(&address)),
        ) {
            self.round = None;
        }
    }

    pub(super) fn may_commit(&self) -> bool {
        self.round.is_some()
            && self
                .members
                .values()
                .all(|member| member.pending == 0 && matches!(member.vote, Vote::Consented))
    }
}
