//! The competition state of one contact generation.

use std::rc::{Rc, Weak};

use flui_foundation::geometry::Offset;
use smallvec::SmallVec;

use super::{
    Admission, AdmissionContext, GestureArenaMember, GestureDisposition,
    composition::{BranchLink, CompositionKind, CompositionNode},
    ownership::{MotionCandidate, PanClaimNode, motion_owner},
};
use crate::pointer::pointer_capture::ContactFeeder;

type Member = Weak<dyn GestureArenaMember>;

/// Member callbacks deferred out of the borrowed state.
///
/// Arena resolution must never invoke `accept_gesture`/`reject_gesture`
/// while the per-entry borrow is held: a member's handler may call back
/// into the arena (e.g. `reject_gesture` -> `state.reject()` ->
/// `arena.resolve`), which needs to borrow the same entry. Internal `ArenaEntryData` mutators
/// therefore return the pending notifications; the public `GestureArena`
/// methods dispatch them after releasing the borrow.
/// Pending verdicts keep only weak ownership: an earlier callback may release
/// a later member, so each member is upgraded immediately before its callback.
pub(super) type PendingNotifications = SmallVec<[(Member, GestureDisposition); 4]>;

pub(super) enum ArenaFollowUp {
    None,
    RemoveEmpty,
    DeferDefault,
    ResolveInFavorOf(Member),
}

/// How a verdict reached its winner, which decides what the contact does next.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Resolution {
    /// The contact ended, or the competition was abandoned.
    Terminal,
    /// A lone competitor won by default; the contact continues unclaimed, so
    /// the rest of the route keeps it.
    Default,
    /// The winner asked for the continuing contact: the route target feeding
    /// it takes the contact (ADR-0208 rule 5).
    Claimed,
}

/// A successor its hold's win kept admitted, for the arena to adopt.
pub(super) struct CarriedSuccessor {
    pub(super) node: Weak<CompositionNode>,
    pub(super) member: Member,
    pub(super) requested: bool,
    pub(super) feeder: Option<ContactFeeder>,
}

/// Arena entry for a single pointer.
///
/// Tracks which recognizers are competing for this pointer. Inline capacity
/// of 4 avoids heap allocation for typical scenarios (tap, drag, long-press,
/// double-tap).
pub(super) struct ArenaEntryData {
    /// Members competing in this arena, in admission (route) order.
    pub(super) members: SmallVec<[Member; 4]>,
    admissions: SmallVec<[(Member, Admission); 4]>,
    requested: SmallVec<[Member; 4]>,
    /// Whether the arena is still open for new members.
    /// When open, accepts are stored as eager_winner instead of resolving
    /// immediately.
    pub(super) is_open: bool,
    /// Whether this entry is held open (waiting for more information).
    pub(super) is_held: bool,
    /// Whether arena has been resolved.
    pub(super) is_resolved: bool,
    /// Eager winner - first recognizer to accept while arena is open.
    /// When arena closes, eager winner wins immediately.
    pub(super) eager_winner: Option<Member>,
    /// Whether sweep is pending (requested while held).
    pub(super) has_pending_sweep: bool,
    /// Successors the latest resolution kept admitted for their hold.
    pub(super) carried: SmallVec<[CarriedSuccessor; 1]>,
}

impl std::fmt::Debug for ArenaEntryData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArenaEntryData")
            .field("member_count", &self.members.len())
            .field("is_open", &self.is_open)
            .field("is_held", &self.is_held)
            .field("is_resolved", &self.is_resolved)
            .field("has_eager_winner", &self.eager_winner.is_some())
            .field("has_pending_sweep", &self.has_pending_sweep)
            .finish_non_exhaustive()
    }
}

fn contains(list: &[Member], member: &Member) -> bool {
    list.iter().any(|entry| Weak::ptr_eq(entry, member))
}

impl ArenaEntryData {
    pub(super) fn new() -> Self {
        Self {
            members: SmallVec::new(),
            admissions: SmallVec::new(),
            requested: SmallVec::new(),
            is_open: true,
            is_held: false,
            is_resolved: false,
            eager_winner: None,
            has_pending_sweep: false,
            carried: SmallVec::new(),
        }
    }

    fn admission(&self, member: &Member) -> Option<&Admission> {
        self.admissions
            .iter()
            .find(|(candidate, _)| Weak::ptr_eq(candidate, member))
            .map(|(_, admission)| admission)
    }

    fn branch_of(&self, member: &Member) -> Option<&BranchLink> {
        self.admission(member)?.branch.as_ref()
    }

    /// A `Sequenced` successor: it never wins the arena itself.
    fn is_successor(&self, member: &Member) -> bool {
        self.branch_of(member)
            .is_some_and(|branch| branch.sequenced_successor_of().is_some())
    }

    /// A `RequireFailure` fallback whose preferred member is still competing.
    fn waits_on_failure(&self, member: &Member) -> bool {
        let Some(branch) = self.branch_of(member) else {
            return false;
        };
        self.admissions.iter().any(|(other, admission)| {
            !Weak::ptr_eq(other, member)
                && admission.branch.as_ref().is_some_and(|theirs| {
                    branch.waits_for(theirs) == Some(CompositionKind::RequireFailure)
                })
        })
    }

    pub(super) fn is_blocked(&self, member: &Member) -> bool {
        self.is_successor(member) || self.waits_on_failure(member)
    }

    fn is_fallback(&self, member: &Member) -> bool {
        self.branch_of(member).is_some_and(|branch| {
            branch.path().any(|link| {
                link.position == super::composition::BranchPosition::Second
                    && link.node.kind == CompositionKind::RequireFailure
            })
        })
    }

    /// Members that can win the arena: every member but `Sequenced`
    /// successors, which belong to their hold's single membership.
    fn competitors(&self) -> impl Iterator<Item = &Member> {
        self.members
            .iter()
            .filter(|member| !self.is_successor(member))
    }

    /// The member a lone default resolution grants.
    pub(super) fn default_winner(&self) -> Option<Rc<dyn GestureArenaMember>> {
        self.competitors().next().and_then(Weak::upgrade)
    }

    /// Close membership and report the resolution work the manager must run.
    pub(super) fn close(&mut self) -> ArenaFollowUp {
        if !self.is_open || self.is_resolved {
            return ArenaFollowUp::None;
        }
        self.is_open = false;
        self.prune_departed();
        self.follow_up()
    }

    /// Accept gesture for a member.
    /// If arena is open, store as eager winner. If closed, resolve immediately.
    ///
    /// Only a current member can win: an accept from a member that already
    /// withdrew (or never joined) is ignored, since resolving in its favour
    /// would reject every remaining member and accept no one.
    ///
    /// The candidate is borrowed: the caller still owns it, so an ignored
    /// candidate that is its own last owner is dropped after the slot borrow is
    /// released, never inside it.
    #[must_use]
    pub(super) fn accept(&mut self, member: &Rc<dyn GestureArenaMember>) -> ArenaFollowUp {
        self.prune_departed();
        self.accept_identity(Rc::downgrade(member))
    }

    fn accept_identity(&mut self, identity: Member) -> ArenaFollowUp {
        if self.is_resolved || !contains(&self.members, &identity) {
            return ArenaFollowUp::None;
        }
        let blocked = self.is_blocked(&identity);
        if self.is_open || blocked {
            if !contains(&self.requested, &identity) {
                self.requested.push(identity.clone());
            }
            if self.is_open && self.eager_winner.is_none() && !blocked {
                self.eager_winner = Some(identity);
            }
            // If already have eager winner, ignore subsequent accepts
            ArenaFollowUp::None
        } else {
            ArenaFollowUp::ResolveInFavorOf(identity)
        }
    }

    /// A moving member crossed its slop with `displacement` accumulated since
    /// Down: the ownership table picks the owner among the moving members
    /// (ADR-0208 §2). A member without a motion role, or one its composition
    /// blocks, claims as an acceptance.
    #[must_use]
    pub(super) fn claim_motion(
        &mut self,
        member: &Rc<dyn GestureArenaMember>,
        displacement: Offset<f64>,
    ) -> ArenaFollowUp {
        self.prune_departed();
        let identity = Rc::downgrade(member);
        if self.is_resolved || !contains(&self.members, &identity) {
            return ArenaFollowUp::None;
        }
        let claimant_moves = self
            .admission(&identity)
            .is_some_and(|admission| admission.role.is_some());
        if !claimant_moves || self.is_blocked(&identity) {
            return self.accept_identity(identity);
        }
        let owner = {
            let mut movers: SmallVec<[&Member; 4]> = SmallVec::new();
            let mut candidates: SmallVec<[MotionCandidate<'_>; 4]> = SmallVec::new();
            for candidate in &self.members {
                if self.is_blocked(candidate) {
                    continue;
                }
                let Some(admission) = self.admission(candidate) else {
                    continue;
                };
                let Some(role) = admission.role else {
                    continue;
                };
                movers.push(candidate);
                candidates.push(MotionCandidate {
                    role,
                    pan_claim: admission.pan_claim.as_ref(),
                });
            }
            motion_owner(&candidates, displacement)
                .map_or_else(|| identity.clone(), |index| movers[index].clone())
        };
        self.accept_identity(owner)
    }

    /// Reject gesture for a member.
    #[must_use]
    pub(super) fn reject(
        &mut self,
        member: &Rc<dyn GestureArenaMember>,
    ) -> (PendingNotifications, ArenaFollowUp) {
        let mut pending = SmallVec::new();
        if self.is_resolved {
            return (pending, ArenaFollowUp::None);
        }

        let identity = Rc::downgrade(member);
        let Some(index) = self
            .members
            .iter()
            .position(|entry| Weak::ptr_eq(entry, &identity))
        else {
            return (pending, ArenaFollowUp::None);
        };
        let rejected = self.members.remove(index);

        // Remove from eager winner if it was this member
        if self
            .eager_winner
            .as_ref()
            .is_some_and(|eager| Weak::ptr_eq(eager, &identity))
        {
            self.eager_winner = None;
        }

        // Defer the member's rejection callback (dispatched after the entry
        // borrow is released to permit arena reentry).
        pending.push((rejected, GestureDisposition::Rejected));
        self.prune_departed();

        let follow_up = if self.is_open {
            ArenaFollowUp::None
        } else {
            self.follow_up()
        };
        (pending, follow_up)
    }

    pub(super) fn follow_up(&self) -> ArenaFollowUp {
        if self.is_resolved || self.is_open {
            return ArenaFollowUp::None;
        }
        if self.members.is_empty() {
            return ArenaFollowUp::RemoveEmpty;
        }
        let mut competitors = self.competitors();
        if let (Some(lone), None) = (competitors.next(), competitors.next())
            && !self.is_blocked(lone)
        {
            ArenaFollowUp::DeferDefault
        } else if let Some(eager) = self
            .eager_winner
            .clone()
            .filter(|eager| !self.is_blocked(eager))
        {
            ArenaFollowUp::ResolveInFavorOf(eager)
        } else if let Some(request) = self
            .requested
            .iter()
            .find(|request| !self.is_blocked(request))
        {
            ArenaFollowUp::ResolveInFavorOf(request.clone())
        } else {
            ArenaFollowUp::None
        }
    }

    /// Reject the `Sequenced` successors whose hold has left a closed
    /// competition: a successor never wins on its own.
    #[must_use]
    pub(super) fn reject_orphans(&mut self) -> PendingNotifications {
        let mut pending = PendingNotifications::new();
        if self.is_open || self.is_resolved {
            return pending;
        }
        self.prune_departed();
        let orphans: SmallVec<[Member; 2]> = self
            .members
            .iter()
            .filter(|member| {
                self.branch_of(member)
                    .and_then(BranchLink::sequenced_successor_of)
                    .is_some_and(|node| {
                        !self.members.iter().any(|hold| {
                            self.branch_of(hold)
                                .is_some_and(|branch| branch.is_hold_of(node))
                        })
                    })
            })
            .cloned()
            .collect();
        for orphan in orphans {
            self.members.retain(|member| !Weak::ptr_eq(member, &orphan));
            pending.push((orphan, GestureDisposition::Rejected));
        }
        self.prune_departed();
        pending
    }

    pub(super) fn withdraw(&mut self, member: &Member) -> ArenaFollowUp {
        if self.is_resolved {
            return ArenaFollowUp::None;
        }
        let Some(index) = self
            .members
            .iter()
            .position(|entry| Weak::ptr_eq(entry, member))
        else {
            return ArenaFollowUp::None;
        };
        self.members.remove(index);
        if self
            .eager_winner
            .as_ref()
            .is_some_and(|eager| Weak::ptr_eq(eager, member))
        {
            self.eager_winner = None;
        }
        self.prune_departed();
        self.follow_up()
    }

    pub(super) fn prune_departed(&mut self) {
        self.members.retain(|member| member.strong_count() != 0);
        let members = &self.members;
        self.admissions
            .retain(|(member, _)| contains(members, member));
        self.requested.retain(|member| contains(members, member));
        if self
            .eager_winner
            .as_ref()
            .is_some_and(|member| member.strong_count() == 0)
        {
            self.eager_winner = None;
        }
    }

    pub(super) fn has_blocked_fallback(&mut self) -> bool {
        self.prune_departed();
        self.members
            .iter()
            .any(|member| self.waits_on_failure(member))
    }

    /// Add a member to this arena.
    pub(super) fn add(
        &mut self,
        member: &Rc<dyn GestureArenaMember>,
        branch: Option<&BranchLink>,
        pan_claim: Option<&Rc<PanClaimNode>>,
        context: AdmissionContext,
    ) -> bool {
        if !self.is_open || self.is_resolved {
            return false;
        }
        let identity = Rc::downgrade(member);
        if contains(&self.members, &identity)
            && (branch.is_some() || self.branch_of(&identity).is_some())
        {
            return false;
        }
        self.members.push(identity.clone());
        self.admissions.push((
            identity,
            Admission {
                branch: branch.cloned(),
                pan_claim: pan_claim.cloned(),
                role: context.role,
                feeder: context.feeder,
            },
        ));
        true
    }

    /// Hold the arena open (delay resolution).
    pub(super) fn hold(&mut self) {
        self.is_held = true;
    }

    /// Release the hold on this arena.
    pub(super) fn release(&mut self) {
        self.is_held = false;
    }

    /// Resolve the arena with a single winner.
    ///
    /// Losers are reported in registration order before winner callbacks.
    /// While the contact continues, the winner's `Sequenced` successors stay
    /// admitted in [`Self::carried`]; a winner that claimed the contact
    /// otherwise takes it for the route target feeding it (ADR-0208 rule 5).
    #[must_use]
    pub(super) fn resolve(
        &mut self,
        winner: Option<&Rc<dyn GestureArenaMember>>,
        resolution: Resolution,
    ) -> PendingNotifications {
        self.resolve_weak(winner.map(Rc::downgrade).as_ref(), resolution)
    }

    pub(super) fn resolve_weak(
        &mut self,
        winner: Option<&Member>,
        resolution: Resolution,
    ) -> PendingNotifications {
        if self.is_resolved {
            return PendingNotifications::new();
        }

        self.is_resolved = true;
        let members = std::mem::take(&mut self.members);
        self.eager_winner = None;
        let mut losers = PendingNotifications::new();
        let mut accepted = PendingNotifications::new();
        let hold = winner
            .filter(|_| resolution != Resolution::Terminal)
            .and_then(|winner| self.branch_of(winner).cloned());

        // Explicit/eager resolution rejects every loser before
        // accepting the winner. This ordering is observable when callbacks
        // re-enter or panic.
        for member in members {
            if winner.is_some_and(|winner| Weak::ptr_eq(&member, winner)) {
                accepted.push((member, GestureDisposition::Accepted));
                continue;
            }
            let node = self
                .branch_of(&member)
                .and_then(BranchLink::sequenced_successor_of)
                .filter(|node| hold.as_ref().is_some_and(|hold| hold.is_hold_of(node)))
                .map(Rc::downgrade);
            if let Some(node) = node {
                let feeder = self
                    .admission(&member)
                    .and_then(|admission| admission.feeder.clone());
                self.carried.push(CarriedSuccessor {
                    node,
                    requested: contains(&self.requested, &member),
                    member,
                    feeder,
                });
            } else {
                losers.push((member, GestureDisposition::Rejected));
            }
        }

        // A carried successor still sees the contact; the owner is chosen at
        // its hand-off instead.
        if resolution == Resolution::Claimed
            && self.carried.is_empty()
            && let Some(feeder) = winner
                .and_then(|winner| self.admission(winner))
                .and_then(|admission| admission.feeder.as_ref())
        {
            feeder.hand_off();
        }

        losers.extend(accepted);
        losers
    }

    /// Sweep ordering is intentionally different from an explicit
    /// resolution: the front competitor is accepted first, then every other
    /// member is rejected in registration order.
    #[must_use]
    pub(super) fn sweep(&mut self) -> PendingNotifications {
        let mut pending = PendingNotifications::new();
        if self.is_resolved {
            return pending;
        }
        self.prune_departed();
        if let Some(request) = self
            .requested
            .iter()
            .find(|request| !self.is_blocked(request) && self.is_fallback(request))
            .cloned()
        {
            return self.resolve_weak(Some(&request), Resolution::Terminal);
        }
        let winner = self.competitors().next().cloned();
        self.is_resolved = true;
        let members = std::mem::take(&mut self.members);
        self.eager_winner = None;
        if let Some(winner) = &winner {
            pending.push((winner.clone(), GestureDisposition::Accepted));
        }
        pending.extend(
            members
                .into_iter()
                .filter(|member| {
                    member.strong_count() != 0
                        && winner
                            .as_ref()
                            .is_none_or(|winner| !Weak::ptr_eq(member, winner))
                })
                .map(|member| (member, GestureDisposition::Rejected)),
        );
        pending
    }
}
