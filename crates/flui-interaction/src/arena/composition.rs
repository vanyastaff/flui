use std::rc::Rc;

use super::GestureArena;

/// How the two branches of an arena competition relate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GestureCompetition {
    /// Both branches compete for the presentation arena's single winner.
    Exclusive,
    /// The second branch may win only after every first-branch member fails.
    RequireFirstFailure,
}

/// Two owner-local arena handles sharing one presentation's lifecycle.
///
/// Build preferred recognizers with the first handle and fallback recognizers
/// with the second handle for [`GestureCompetition::RequireFirstFailure`].
#[must_use]
pub struct GestureBranches {
    first: GestureArena,
    second: GestureArena,
}

impl GestureBranches {
    /// Consume the pair into its ordered recognizer-construction handles.
    #[must_use]
    pub fn into_branches(self) -> (GestureArena, GestureArena) {
        (self.first, self.second)
    }
}

/// A composition cannot introduce another relation inside an existing branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CompositionError {
    /// Nested branch composition is not supported.
    #[error("cannot compose an arena branch again")]
    AlreadyComposed,
}

#[derive(Clone)]
pub(super) struct CompositionBranch {
    pub(super) competition: Rc<GestureCompetition>,
    pub(super) first: bool,
}

impl CompositionBranch {
    pub(super) fn blocks(&self, other: &Self) -> bool {
        !self.first
            && other.first
            && *self.competition == GestureCompetition::RequireFirstFailure
            && Rc::ptr_eq(&self.competition, &other.competition)
    }
}

impl GestureArena {
    /// Construct a binary recognizer competition in this presentation arena.
    ///
    /// Membership inherits the branch on every admission, including recognizers
    /// that create a new arena member for each contact. The handles share the
    /// clock, owner lifetime and exact-generation close/sweep lifecycle.
    ///
    /// # Errors
    /// Returns [`CompositionError::AlreadyComposed`] for a branch handle.
    pub fn compose(
        &self,
        competition: GestureCompetition,
    ) -> Result<GestureBranches, CompositionError> {
        if self.branch.is_some() {
            return Err(CompositionError::AlreadyComposed);
        }
        let competition = Rc::new(competition);
        let mut first = self.clone();
        first.branch = Some(CompositionBranch {
            competition: Rc::clone(&competition),
            first: true,
        });
        let mut second = self.clone();
        second.branch = Some(CompositionBranch {
            competition,
            first: false,
        });
        Ok(GestureBranches { first, second })
    }
}
