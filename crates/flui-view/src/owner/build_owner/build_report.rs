//! Per-frame rebuild counts and the report the last build pass left.

use super::BuildOwner;
use crate::owner::RebuildReason;
use flui_foundation::RebuildReasons;

/// Per-cause rebuild counters for one `build_scope`, indexed by the
/// `RebuildReason` discriminant (`#[repr(u8)]`), sized by `RebuildReason::COUNT`
/// so a new variant grows the table with it.
#[derive(Debug, Default)]
pub(super) struct FrameBuildCounts {
    /// Which causes appeared this frame — the bitset is also the stable
    /// iteration order for the report.
    seen: Option<RebuildReasons>,
    counts: [usize; RebuildReason::COUNT],
    /// Every completed build this frame, re-entries included.
    pub(super) builds_run: usize,
}

impl FrameBuildCounts {
    fn clear(&mut self) {
        self.seen = None;
        self.counts = [0; RebuildReason::COUNT];
        self.builds_run = 0;
    }

    pub(super) fn record(&mut self, reason: RebuildReason) {
        let index = reason as u8 as usize;
        debug_assert!(
            index < self.counts.len(),
            "BUG: RebuildReason outgrew the counter table"
        );
        if let Some(slot) = self.counts.get_mut(index) {
            *slot += 1;
        }
        match &mut self.seen {
            Some(seen) => seen.insert(reason),
            None => self.seen = Some(RebuildReasons::from_reason(reason)),
        }
    }

    fn by_reason(&self) -> Vec<(RebuildReason, usize)> {
        self.seen
            .into_iter()
            .flat_map(RebuildReasons::iter)
            .map(|reason| {
                (
                    reason,
                    self.counts.get(reason as u8 as usize).copied().unwrap_or(0),
                )
            })
            .collect()
    }
}

/// Per-frame rebuild telemetry, see [`BuildOwner::last_frame_build_report`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct FrameBuildReport {
    /// Distinct elements rebuilt in the most recent widget draw frame or
    /// explicit `build_scope`.
    pub elements_built: usize,
    /// Every build run this frame, re-entries included.
    ///
    /// Equal to [`elements_built`](Self::elements_built) unless an element
    /// built more than once in the frame — a self-rescheduling build or an
    /// A↔B ping-pong, which the distinct count cannot show.
    pub builds_run: usize,
    /// Rebuilt-element count per cause, in stable diagnostic-name order.
    pub by_reason: Vec<(RebuildReason, usize)>,
}

impl FrameBuildReport {
    /// How many rebuilt elements carried `reason`.
    #[must_use]
    pub fn count(&self, reason: RebuildReason) -> usize {
        self.by_reason
            .iter()
            .find(|(candidate, _)| *candidate == reason)
            .map_or(0, |(_, count)| *count)
    }
}

impl BuildOwner {
    /// What the most recent widget draw frame or explicit `build_scope`
    /// rebuilt: the number of distinct elements built this frame and, per
    /// [`RebuildReason`], how many of them
    /// carried that cause (an element scheduled for two reasons counts once in
    /// `elements_built` and once under each reason). Reset at every
    /// `build_scope` and every binding draw frame, including a clean one.
    /// A pump that produces no draw frame leaves the previous report intact.
    pub fn last_frame_build_report(&self) -> FrameBuildReport {
        FrameBuildReport {
            elements_built: self.built_this_frame.len(),
            builds_run: self.frame_builds.builds_run,
            by_reason: self.frame_builds.by_reason(),
        }
    }

    /// Start a fresh telemetry report without routing or draining build work.
    /// A clean binding frame still owns an empty report; lazy child service
    /// later in that frame may add its builds to it.
    pub(crate) fn clear_frame_build_report(&mut self) {
        self.built_this_frame.clear();
        self.frame_builds.clear();
    }

    /// Start a fresh telemetry report, for a test kit that reports one frame:
    /// cleared before the frame, the report afterwards holds nothing the
    /// frames before it built.
    #[cfg(any(test, feature = "testing"))]
    pub fn clear_frame_build_report_for_test(&mut self) {
        self.clear_frame_build_report();
    }
}
