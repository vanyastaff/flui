//! Presentation-local tree revision and commit state.

/// Monotonic revision of terminal frame attempts for one presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TreeRevision(u64);

impl TreeRevision {
    /// Revision before any terminal frame attempt has occurred.
    pub const ZERO: Self = Self(0);

    /// Return the next revision.
    ///
    /// # Panics
    ///
    /// Panics if the revision space is exhausted, which a presentation cannot
    /// reach at any frame rate.
    #[must_use]
    pub fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("BUG: tree revision space exhausted"),
        )
    }

    /// Numeric field value for structured tracing.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Whether the presentation's current tree revision has been acknowledged
/// by a successful submit classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameCommitState {
    /// Every terminal tree revision has been acknowledged.
    Committed,
    /// One or more terminal revisions have not been acknowledged.
    Uncommitted {
        /// Earliest tree revision absent from the acknowledged frame.
        since: TreeRevision,
    },
}
