//! Evaluating a wait condition over the published tree (R18).
//!
//! A condition names elements, frames, settledness or events. The tree part is evaluated here,
//! against the revision the owner last published; the runtime supplies what the tree cannot
//! know ([`WaitFacts`]). Evaluation is pure and runs no user code, so the owner can run it after
//! every frame without a borrow outliving the call.

use flui_protocol::{
    Condition, EventKind, Expectation, LocateParams, Locator, Matches, Node, Observation,
};

use super::Placement;
use super::locate::WireLocateError;
use crate::owner::SemanticsOwner;

/// The most matched elements one observation carries; the rest are counted.
const OBSERVED_NODES: usize = 16;

/// The kinds of event a window emitted since a wait began.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventSet(u8);

impl EventSet {
    /// No event.
    pub const EMPTY: Self = Self(0);

    /// The bit of `kind`: its place in the vocabulary.
    fn bit(kind: EventKind) -> u8 {
        let place = EventKind::ALL
            .iter()
            .position(|known| *known == kind)
            .expect("BUG: every event kind is in the vocabulary");
        1 << place
    }

    /// Records that an event of `kind` was emitted.
    pub fn insert(&mut self, kind: EventKind) {
        self.0 |= Self::bit(kind);
    }

    /// Whether an event of `kind` was emitted.
    #[must_use]
    pub fn contains(self, kind: EventKind) -> bool {
        self.0 & Self::bit(kind) != 0
    }
}

/// What a condition depends on besides the tree, measured by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitFacts {
    /// Frames presented since the wait began.
    pub frames: u64,
    /// Whether the window has nothing pending that could still change it.
    pub settled: bool,
    /// The kinds of event the window emitted since the wait began.
    pub events: EventSet,
}

/// Whether a condition held, and what was seen deciding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    /// Whether the condition held.
    pub holds: bool,
    /// The revision evaluated and the elements the condition's locators matched.
    pub observation: Observation,
}

/// Whether `condition` counts frames, so a wait on it must keep frames coming.
#[must_use]
pub fn counts_frames(condition: &Condition) -> bool {
    match condition {
        Condition::Frames(_) => true,
        Condition::All(all) | Condition::Any(all) => all.iter().any(counts_frames),
        _ => false,
    }
}

impl SemanticsOwner {
    /// Evaluates `condition` against the published tree.
    ///
    /// Before the first assembly no element exists: `Appears`, `Gone`, `Matches` and `Count`
    /// then do not hold (a window that has drawn nothing has not made anything disappear), while
    /// `Frames` and `Settled` are decided from `facts` alone. `Matches` holds when the locator
    /// names at least one element and every element it names meets the expectation. An `Event`
    /// holds when `facts` says an event of its kind was emitted since the wait began.
    ///
    /// # Errors
    ///
    /// [`WireLocateError::Malformed`] when the published tree breaks AccessKit's contract.
    pub fn evaluate_wait(
        &self,
        condition: &Condition,
        facts: WaitFacts,
        placement: Placement,
    ) -> Result<Evaluation, WireLocateError> {
        let mut observation = Observation::default();
        observation.revision = self.revision();
        let holds = self.holds(condition, facts, placement, &mut observation)?;
        Ok(Evaluation { holds, observation })
    }

    fn holds(
        &self,
        condition: &Condition,
        facts: WaitFacts,
        placement: Placement,
        seen: &mut Observation,
    ) -> Result<bool, WireLocateError> {
        Ok(match condition {
            Condition::Appears(locator) => self
                .observe(locator, placement, seen)?
                .is_some_and(|matches| !matches.matches.is_empty()),
            Condition::Gone(locator) => self
                .observe(locator, placement, seen)?
                .is_some_and(|matches| matches.matches.is_empty() && matches.truncated == 0),
            Condition::Matches { locator, expect } => self
                .observe(locator, placement, seen)?
                .is_some_and(|matches| expectation_holds(expect, &matches)),
            Condition::Settled => facts.settled,
            Condition::Frames(frames) => facts.frames >= u64::from(*frames),
            Condition::All(all) => {
                // Every part is evaluated, so the observation reports what each saw.
                let mut holds = true;
                for part in all {
                    holds &= self.holds(part, facts, placement, seen)?;
                }
                holds
            }
            Condition::Any(any) => {
                let mut holds = false;
                for part in any {
                    holds |= self.holds(part, facts, placement, seen)?;
                }
                holds
            }
            // An event of another window never reaches this one's wait.
            Condition::Event(filter) => {
                filter
                    .window
                    .is_none_or(|window| window == placement.window)
                    && facts.events.contains(filter.kind)
            }
            // A condition added later: nothing here can see it.
            _ => false,
        })
    }

    /// The elements `locator` matches now, recorded into `seen`; `None` before the first tree.
    fn observe(
        &self,
        locator: &Locator,
        placement: Placement,
        seen: &mut Observation,
    ) -> Result<Option<Matches>, WireLocateError> {
        let matches = match self.locate_wire(&LocateParams::new(locator.clone()), placement) {
            Ok(matches) => matches,
            Err(WireLocateError::NoTree) => return Ok(None),
            Err(error) => return Err(error),
        };
        let total =
            matches.matches.len() + usize::try_from(matches.truncated).unwrap_or(usize::MAX);
        seen.count = seen
            .count
            .saturating_add(u32::try_from(total).unwrap_or(u32::MAX));
        let room = OBSERVED_NODES.saturating_sub(seen.matched.len());
        seen.matched
            .extend(matches.matches.iter().take(room).map(|m| m.node.clone()));
        Ok(Some(matches))
    }
}

fn expectation_holds(expect: &Expectation, matches: &Matches) -> bool {
    if let Expectation::Count(count) = expect {
        let total =
            matches.matches.len() + usize::try_from(matches.truncated).unwrap_or(usize::MAX);
        return usize::try_from(*count).is_ok_and(|count| count == total);
    }
    !matches.matches.is_empty()
        && matches
            .matches
            .iter()
            .all(|matched| meets(expect, &matched.node))
}

fn meets(expect: &Expectation, node: &Node) -> bool {
    match expect {
        Expectation::Name(name) => node.name.as_deref().is_some_and(|text| name.matches(text)),
        Expectation::Value(value) => node
            .value
            .as_deref()
            .is_some_and(|text| value.matches(text)),
        Expectation::State(state) => {
            state
                .checked
                .is_none_or(|checked| node.checked == Some(checked))
                && state
                    .disabled
                    .is_none_or(|disabled| node.disabled == disabled)
                && state.focused.is_none_or(|focused| node.focused == focused)
                && state
                    .expanded
                    .is_none_or(|expanded| node.expanded == Some(expanded))
                && state
                    .selected
                    .is_none_or(|selected| node.selected == Some(selected))
        }
        Expectation::Focused => node.focused,
        // `Count` is decided on the whole list; one added later is not met.
        _ => false,
    }
}
