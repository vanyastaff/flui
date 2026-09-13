//! Presentation-local tree revision and commit state.

use std::fmt;

/// Monotonic revision of terminal frame attempts for one presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TreeRevision(u64);

impl TreeRevision {
    /// Revision before any terminal frame attempt has occurred.
    pub(crate) const ZERO: Self = Self(0);

    /// Return the next revision.
    pub(crate) fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("BUG: tree revision space exhausted"),
        )
    }
}

/// Whether the presentation's current tree revision has been acknowledged
/// by a successful submit classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameCommitState {
    /// Every terminal tree revision has been acknowledged.
    Committed,
    /// One or more terminal revisions have not been acknowledged.
    Uncommitted {
        /// Earliest tree revision absent from the acknowledged frame.
        since: TreeRevision,
    },
}

/// Opaque observability snapshot of a presentation's revision state.
pub(crate) struct FrameRevisionSnapshot {
    tree_revision: TreeRevision,
    presented_revision: TreeRevision,
    commit_state: FrameCommitState,
}

impl fmt::Debug for FrameRevisionSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameRevisionSnapshot")
            .field("tree_revision", &self.tree_revision)
            .field("presented_revision", &self.presented_revision)
            .field("commit_state", &self.commit_state)
            .finish()
    }
}

impl FrameRevisionSnapshot {
    pub(super) fn new(
        tree_revision: TreeRevision,
        presented_revision: TreeRevision,
        commit_state: FrameCommitState,
    ) -> Self {
        Self {
            tree_revision,
            presented_revision,
            commit_state,
        }
    }
}
