//! Per-child track policy for cross-axis sliver groups.

use std::num::NonZeroU32;

/// How a cross-axis sliver group assigns one child's track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliverCrossAxisAllocation {
    /// Claim the child's explicitly reported cross-axis extent first.
    Content,
    /// Share the remaining extent proportionally, with a positive weight.
    Flex(NonZeroU32),
}

impl Default for SliverCrossAxisAllocation {
    fn default() -> Self {
        Self::Flex(NonZeroU32::MIN)
    }
}

/// Authored track policy. Committed positions stay in the render tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SliverCrossAxisParentData {
    /// Track allocation; ordinary children default to one flexible share.
    pub allocation: SliverCrossAxisAllocation,
}

impl super::ParentData for SliverCrossAxisParentData {}
