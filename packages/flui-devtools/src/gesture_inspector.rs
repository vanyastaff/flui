//! What a gesture arena decided, contest by contest.
//!
//! [`GestureInspector`] registers a `GestureObserver` on one arena and folds
//! what it reports into a [`ContestReport`] per contest: the members that
//! joined, with the compositions they joined under, which of them claimed,
//! the winner, each rejection with its reason, the deadlines that elapsed and
//! the input sequence the contest decided.
//!
//! A recording is inspected by replaying it with an inspector attached to the
//! replaying host's arena: the replay re-runs the arena over the recorded
//! input, so the inspector sees every join and claim the recording's rows do
//! not carry, and builds the model the live run built.
//!
//! An arena with no inspector attached records nothing for it: the observer
//! seam allocates nothing while no observer is registered.
//!
//! ```
//! use flui_devtools::gesture_inspector::GestureInspector;
//! use flui_sdk::interaction::GestureArena;
//!
//! let arena = GestureArena::new();
//! let inspector = GestureInspector::attach(&arena).expect("an observer registration");
//! // ...dispatch input through the arena...
//! for contest in inspector.contests() {
//!     println!("{contest}");
//! }
//! ```

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU64;
use std::rc::Rc;

use flui_sdk::foundation::{IdExhausted, InputSequenceId};
use flui_sdk::interaction::{
    CompositionBranch, CompositionKind, ContestCancel, EventTime, GestureArena, GestureContest,
    GestureObservation, ObservedComposition, ObservedMember, ObserverHandle, PointerId,
    RejectionReason,
};

/// How many contests an inspector keeps unless told otherwise.
pub const DEFAULT_CONTEST_CAPACITY: usize = 256;

/// Watches one gesture arena and keeps a report of its most recent contests.
///
/// Observation lasts until [`detach`](Self::detach) or until the inspector is
/// dropped; the reports it built stay readable after detaching.
pub struct GestureInspector {
    model: Rc<RefCell<Model>>,
    registration: Option<ObserverHandle>,
}

impl GestureInspector {
    /// Inspect `arena`, keeping the last [`DEFAULT_CONTEST_CAPACITY`]
    /// contests.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] when the arena has issued its last observer
    /// registration.
    pub fn attach(arena: &GestureArena) -> Result<Self, IdExhausted> {
        Self::with_capacity(arena, DEFAULT_CONTEST_CAPACITY)
    }

    /// Inspect `arena`, keeping the last `capacity` contests: the oldest is
    /// dropped when a new one would exceed it.
    ///
    /// # Errors
    ///
    /// As [`attach`](Self::attach).
    pub fn with_capacity(arena: &GestureArena, capacity: usize) -> Result<Self, IdExhausted> {
        Self::observing(arena, capacity, None)
    }

    /// As [`with_capacity`](Self::with_capacity), calling `changed` after each
    /// observation is folded in, with the model released.
    pub(crate) fn observing(
        arena: &GestureArena,
        capacity: usize,
        changed: Option<Box<dyn Fn()>>,
    ) -> Result<Self, IdExhausted> {
        let model = Rc::new(RefCell::new(Model::new(capacity)));
        let sink = Rc::clone(&model);
        let registration = arena.observe(move |observation: &GestureObservation| {
            sink.borrow_mut().observe(observation);
            if let Some(changed) = &changed {
                changed();
            }
        })?;
        Ok(Self {
            model,
            registration: Some(registration),
        })
    }

    /// Stop observing the arena. The reports built so far are kept.
    pub fn detach(&mut self) {
        self.registration = None;
    }

    /// Whether the inspector still observes its arena.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.registration.is_some()
    }

    /// The retained contests, oldest first, as they stand now.
    #[must_use]
    pub fn contests(&self) -> Vec<ContestReport> {
        self.model
            .borrow()
            .contests
            .iter()
            .map(|tracked| tracked.report.clone())
            .collect()
    }

    /// Calls `visit` with each retained contest, oldest first, without
    /// copying them. The model is borrowed meanwhile, so `visit` must not
    /// reach this inspector's arena.
    pub(crate) fn visit_contests(&self, mut visit: impl FnMut(&ContestReport)) {
        for tracked in &self.model.borrow().contests {
            visit(&tracked.report);
        }
    }

    /// Forget every retained contest. Member and composition numbers keep
    /// counting, so a number never names two members.
    pub fn clear(&self) {
        self.model.borrow_mut().contests.clear();
    }
}

impl fmt::Debug for GestureInspector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let model = self.model.borrow();
        f.debug_struct("GestureInspector")
            .field("attached", &self.is_attached())
            .field("contests", &model.contests.len())
            .field("capacity", &model.capacity)
            .finish_non_exhaustive()
    }
}

/// An arena member as an inspector names it: numbered from 1 in the order the
/// inspector first saw it, normally its join. Two members of one type stay
/// apart, and two runs of one application over one input stream number their
/// members alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InspectedMember(NonZeroU64);

impl InspectedMember {
    /// The member's number.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// A composition node as an inspector names it, numbered as
/// [`InspectedMember`] is: members whose paths name one node are the two
/// branches of one relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InspectedComposition(NonZeroU64);

impl InspectedComposition {
    /// The node's number.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// One contest: a contact's competition for its single owner.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ContestReport {
    /// The contact the contest is for.
    pub pointer: PointerId,
    /// The input sequence the contest decides; `None` in an arena no binding
    /// drives.
    pub sequence: Option<InputSequenceId>,
    /// The members, in the order the inspector first saw each.
    pub members: Vec<MemberReport>,
    /// Whether the contest stopped admitting members.
    pub closed: bool,
    /// The member that won, once one has.
    pub winner: Option<InspectedMember>,
    /// Why the contest ended with no winner, if it was cancelled.
    pub cancelled: Option<ContestCancel>,
    /// The deadlines the arena polled for this contest, in order.
    pub deadlines: Vec<DeadlineReport>,
}

/// One member of a contest.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MemberReport {
    /// The member.
    pub member: InspectedMember,
    /// Its concrete type name, an annotation for people: never an identity.
    pub type_name: Option<&'static str>,
    /// The compositions it joined under, innermost first; empty when it
    /// joined uncomposed, or joined before the inspector attached.
    pub composition: Vec<CompositionStep>,
    /// Whether it claimed the contest.
    pub claimed: bool,
    /// Its verdict, once the arena committed one.
    pub verdict: Option<Verdict>,
}

/// One composition on a member's path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct CompositionStep {
    /// The node.
    pub node: InspectedComposition,
    /// The relation it is.
    pub kind: CompositionKind,
    /// The branch the member joined through.
    pub branch: CompositionBranch,
}

/// A member's verdict in a contest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// It won.
    Won {
        /// When the arena committed it, on the event timeline.
        at: EventTime,
    },
    /// It was rejected.
    Rejected {
        /// When the arena committed it, on the event timeline.
        at: EventTime,
        /// Why.
        reason: RejectionReason,
    },
}

/// A member's deadline the arena polled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DeadlineReport {
    /// The member polled.
    pub member: InspectedMember,
    /// When the arena polled it, on the event timeline.
    pub at: EventTime,
}

impl fmt::Display for ContestReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "contest for pointer {}", self.pointer.get())?;
        match self.sequence {
            Some(sequence) => write!(f, ", sequence {}", sequence.get())?,
            None => f.write_str(", no sequence")?,
        }
        if !self.closed {
            f.write_str(", open")?;
        }
        match (self.winner, self.cancelled) {
            (Some(winner), _) => writeln!(f, ", won by #{}", winner.get())?,
            (None, Some(cause)) => writeln!(f, ", cancelled ({cause:?})")?,
            (None, None) => writeln!(f, ", undecided")?,
        }
        for member in &self.members {
            write!(
                f,
                "  #{} {}",
                member.member.get(),
                short_type(member.type_name)
            )?;
            for step in &member.composition {
                write!(
                    f,
                    " in {:?}@{} {}",
                    step.kind,
                    step.node.get(),
                    match step.branch {
                        CompositionBranch::First => "first",
                        CompositionBranch::Second => "second",
                    }
                )?;
            }
            if member.claimed {
                f.write_str(", claimed")?;
            }
            match member.verdict {
                Some(Verdict::Won { .. }) => f.write_str(", won")?,
                Some(Verdict::Rejected { reason, .. }) => write!(f, ", rejected ({reason:?})")?,
                None => {}
            }
            let deadlines = self
                .deadlines
                .iter()
                .filter(|deadline| deadline.member == member.member)
                .count();
            if deadlines > 0 {
                write!(f, ", {deadlines} deadline(s) elapsed")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// The last path segment of a type name, for a one-line report.
fn short_type(type_name: Option<&'static str>) -> &'static str {
    type_name.map_or("<retired>", |name| {
        let base = name.split('<').next().unwrap_or(name);
        base.rsplit("::").next().unwrap_or(base)
    })
}

/// A retained contest with the identity observations name it by.
struct Tracked {
    contest: GestureContest,
    report: ContestReport,
}

/// Numbers for identities compared by allocation, minted from 1 and never
/// reissued; at exhaustion nothing new is numbered.
struct Numbering<T> {
    known: Vec<(T, NonZeroU64)>,
    next: Option<NonZeroU64>,
}

impl<T> Numbering<T> {
    fn new() -> Self {
        Self {
            known: Vec::new(),
            next: Some(NonZeroU64::MIN),
        }
    }

    fn number(&mut self, identity: &T, same: impl Fn(&T, &T) -> bool) -> Option<NonZeroU64>
    where
        T: Clone,
    {
        if let Some((_, number)) = self.known.iter().find(|(known, _)| same(known, identity)) {
            return Some(*number);
        }
        let number = self.next?;
        self.next = number.checked_add(1);
        self.known.push((identity.clone(), number));
        Some(number)
    }
}

struct Model {
    capacity: usize,
    contests: VecDeque<Tracked>,
    members: Numbering<ObservedMember>,
    nodes: Numbering<ObservedComposition>,
}

impl Model {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            contests: VecDeque::new(),
            members: Numbering::new(),
            nodes: Numbering::new(),
        }
    }

    fn observe(&mut self, observation: &GestureObservation) {
        match observation {
            GestureObservation::Joined {
                contest,
                member,
                composition,
            } => {
                self.forget_retired();
                let composition = composition
                    .iter()
                    .filter_map(|step| {
                        let node = self.nodes.number(step, ObservedComposition::is_same_node)?;
                        Some(CompositionStep {
                            node: InspectedComposition(node),
                            kind: step.kind(),
                            branch: step.branch(),
                        })
                    })
                    .collect();
                if let Some(report) = self.member(*contest, member) {
                    report.composition = composition;
                }
            }
            GestureObservation::Closed { contest } => {
                if let Some(report) = self.contest(*contest) {
                    report.closed = true;
                }
            }
            GestureObservation::Claimed { contest, member } => {
                if let Some(report) = self.member(*contest, member) {
                    report.claimed = true;
                }
            }
            GestureObservation::Won {
                contest,
                member,
                at,
            } => {
                let Some(report) = self.member(*contest, member) else {
                    return;
                };
                report.verdict = Some(Verdict::Won { at: *at });
                let winner = report.member;
                if let Some(contest) = self.contest(*contest) {
                    contest.winner = Some(winner);
                }
            }
            GestureObservation::Rejected {
                contest,
                member,
                at,
                reason,
            } => {
                if let Some(report) = self.member(*contest, member) {
                    report.verdict = Some(Verdict::Rejected {
                        at: *at,
                        reason: *reason,
                    });
                }
            }
            GestureObservation::Cancelled { contest, cause } => {
                if let Some(report) = self.contest(*contest) {
                    report.cancelled = Some(*cause);
                }
            }
            GestureObservation::DeadlineElapsed {
                contest: Some(contest),
                member,
                now,
            } => {
                let Some(number) = self.members.number(member, PartialEq::eq) else {
                    return;
                };
                if let Some(report) = self.contest(*contest) {
                    report.deadlines.push(DeadlineReport {
                        member: InspectedMember(number),
                        at: *now,
                    });
                }
            }
            // A deadline outside any contest belongs to no report.
            _ => {}
        }
    }

    /// `contest`'s report, opened if this is the first the inspector hears of
    /// it; `None` when the capacity retains no contest.
    fn contest(&mut self, contest: GestureContest) -> Option<&mut ContestReport> {
        if let Some(index) = self
            .contests
            .iter()
            .rposition(|tracked| tracked.contest == contest)
        {
            return Some(&mut self.contests[index].report);
        }
        if self.capacity == 0 {
            return None;
        }
        if self.contests.len() == self.capacity {
            self.contests.pop_front();
        }
        self.contests.push_back(Tracked {
            contest,
            report: ContestReport {
                pointer: contest.pointer(),
                sequence: contest.sequence(),
                members: Vec::new(),
                closed: false,
                winner: None,
                cancelled: None,
                deadlines: Vec::new(),
            },
        });
        self.contests.back_mut().map(|tracked| &mut tracked.report)
    }

    /// `member`'s report in `contest`, added if the inspector has not seen it
    /// there yet.
    fn member(
        &mut self,
        contest: GestureContest,
        member: &ObservedMember,
    ) -> Option<&mut MemberReport> {
        let number = InspectedMember(self.members.number(member, PartialEq::eq)?);
        let report = self.contest(contest)?;
        if let Some(index) = report
            .members
            .iter()
            .position(|known| known.member == number)
        {
            return Some(&mut report.members[index]);
        }
        report.members.push(MemberReport {
            member: number,
            type_name: member.type_name(),
            composition: Vec::new(),
            claimed: false,
            verdict: None,
        });
        report.members.last_mut()
    }

    /// Drop the identities nothing can name again: a retired composition, and
    /// a retired member with no verdict still to come in a retained contest.
    /// Keeping a member's identity keeps its allocation from naming another
    /// member, so one awaiting its verdict stays.
    fn forget_retired(&mut self) {
        let contests = &self.contests;
        self.members.known.retain(|(member, number)| {
            !member.is_retired()
                || contests.iter().any(|tracked| {
                    tracked
                        .report
                        .members
                        .iter()
                        .any(|report| report.member.0 == *number && report.verdict.is_none())
                })
        });
        self.nodes.known.retain(|(node, _)| !node.is_retired());
    }
}
