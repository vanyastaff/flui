//! Typed causes for element rebuilds.
//!
//! Rebuild scheduling is an architectural event, not merely a dirty bit. Every
//! scheduling edge names why it invalidated an element; when several edges
//! target the same element before the next build, their causes are combined.
//! This makes the build pipeline explainable without reconstructing intent from
//! log messages after the fact.

use std::fmt;

/// Why an element was scheduled to rebuild.
///
/// The variants describe the invalidation mechanism rather than widget-domain
/// details, so framework tooling can compare causes across all widget crates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[repr(u8)]
pub enum RebuildReason {
    /// The element has just entered the tree and needs its first build.
    InitialMount,
    /// Its parent reconciled a new configuration into the existing element.
    ParentUpdate,
    /// State owned by the element or one of its imperative controllers changed.
    StateChange,
    /// An inherited dependency notified this dependent.
    DependencyChange,
    /// An animation advanced to a new value.
    AnimationTick,
    /// An asynchronous computation or stream produced a new value.
    AsyncCompletion,
    /// Layout constraints or layout-driven child requirements changed.
    LayoutChange,
    /// A lazy child collection inserted, removed, or retained children.
    ChildListChange,
    /// The presentation root was replaced or reconfigured.
    RootChange,
    /// Hot reload requested that every live element rebuild in place.
    HotReload,
}

impl RebuildReason {
    pub(crate) const ALL: [Self; 10] = [
        Self::InitialMount,
        Self::ParentUpdate,
        Self::StateChange,
        Self::DependencyChange,
        Self::AnimationTick,
        Self::AsyncCompletion,
        Self::LayoutChange,
        Self::ChildListChange,
        Self::RootChange,
        Self::HotReload,
    ];

    const fn bit(self) -> u16 {
        1 << self as u8
    }

    /// Stable diagnostic name for this cause.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InitialMount => "initial_mount",
            Self::ParentUpdate => "parent_update",
            Self::StateChange => "state_change",
            Self::DependencyChange => "dependency_change",
            Self::AnimationTick => "animation_tick",
            Self::AsyncCompletion => "async_completion",
            Self::LayoutChange => "layout_change",
            Self::ChildListChange => "child_list_change",
            Self::RootChange => "root_change",
            Self::HotReload => "hot_reload",
        }
    }
}

impl fmt::Display for RebuildReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Compact set of causes accumulated for one pending element rebuild.
///
/// Scheduling the same element several times still produces one build, but it
/// does not erase causality: every distinct reason remains in this set. Values
/// returned by [`BuildOwner::pending_rebuild_reasons`](super::BuildOwner::pending_rebuild_reasons)
/// are snapshots and never expose the owner's internal queue or lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RebuildReasons(u16);

impl RebuildReasons {
    /// Create a set containing exactly one reason.
    #[must_use]
    pub const fn from_reason(reason: RebuildReason) -> Self {
        Self(reason.bit())
    }

    pub(crate) const fn one(reason: RebuildReason) -> Self {
        Self::from_reason(reason)
    }

    pub(crate) fn insert(&mut self, reason: RebuildReason) {
        self.0 |= reason.bit();
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Whether this set contains `reason`.
    #[must_use]
    pub const fn contains(self, reason: RebuildReason) -> bool {
        self.0 & reason.bit() != 0
    }

    /// Number of distinct causes in this set.
    #[must_use]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// Whether this set contains no causes.
    ///
    /// Framework-produced values are non-empty. This method makes generic
    /// diagnostic consumers robust without exposing a public empty
    /// constructor.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Iterate over causes in stable diagnostic order.
    pub fn iter(self) -> impl Iterator<Item = RebuildReason> {
        RebuildReason::ALL
            .into_iter()
            .filter(move |reason| self.0 & reason.bit() != 0)
    }
}

impl fmt::Display for RebuildReasons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut separator = "";
        for reason in self.iter() {
            f.write_str(separator)?;
            f.write_str(reason.as_str())?;
            separator = "|";
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{RebuildReason, RebuildReasons};

    #[test]
    fn accumulated_reasons_are_unique_and_stably_ordered() {
        let mut reasons = RebuildReasons::one(RebuildReason::AnimationTick);
        reasons.insert(RebuildReason::DependencyChange);
        reasons.insert(RebuildReason::AnimationTick);

        assert_eq!(
            reasons.iter().collect::<Vec<_>>(),
            [
                RebuildReason::DependencyChange,
                RebuildReason::AnimationTick,
            ],
        );
        assert_eq!(reasons.to_string(), "dependency_change|animation_tick");
    }
}
