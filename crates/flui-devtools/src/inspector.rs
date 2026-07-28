//! Counting/logging tree inspector — the ADR-0040 slice-1 consumer.
//!
//! Proves the dependency-inverted observation seam end to end with only
//! `flui-foundation` on the dependency list: install an
//! [`InspectorCounters`] via `WidgetsBinding::install_tree_observer` and it
//! tallies mounts, moves, rebuilds (with per-cause counts), and unmounts as
//! the tree mutates. No tree access, no locks in the public surface.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use flui_foundation::RebuildReason;
use flui_foundation::observe::{
    ElementMounted, ElementMoved, ElementRebuilt, ElementUnmounted, TreeObserver,
};

/// Number of [`RebuildReason`] variants tracked in the per-cause tallies.
const REASON_COUNT: usize = 10;

/// Counting/logging [`TreeObserver`] — the slice-1 inspector.
///
/// Interior state is private atomics; nothing here exposes a lock (SP-6).
/// All counter updates use `Relaxed`: bare tallies, no data is published
/// through them, and the observation contract already serializes emissions
/// per realm (owner-thread, synchronous).
#[derive(Debug, Default)]
pub struct InspectorCounters {
    mounts: AtomicU64,
    unmounts: AtomicU64,
    rebuilds: AtomicU64,
    moves: AtomicU64,
    per_reason: [AtomicU64; REASON_COUNT],
    detached: AtomicBool,
}

impl InspectorCounters {
    /// Plain constructor — callers wrap in `Arc` themselves
    /// (`Arc::new(InspectorCounters::new())`), consistent with the
    /// `Default` derive rather than competing with it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot for display. Each counter is individually monotonic, but
    /// the snapshot reads independent atomics without a lock, so
    /// cross-counter consistency is NOT guaranteed (a snapshot taken
    /// mid-frame may show a mount whose paired rebuild is not yet counted).
    /// Per-counter monotonicity is the only invariant. Clones values out,
    /// returns no guard (SP-6).
    #[must_use]
    pub fn snapshot(&self) -> InspectorSnapshot {
        let mut per_reason = [0u64; REASON_COUNT];
        for (slot, counter) in per_reason.iter_mut().zip(&self.per_reason) {
            *slot = counter.load(Ordering::Relaxed);
        }
        InspectorSnapshot {
            mounts: self.mounts.load(Ordering::Relaxed),
            unmounts: self.unmounts.load(Ordering::Relaxed),
            rebuilds: self.rebuilds.load(Ordering::Relaxed),
            moves: self.moves.load(Ordering::Relaxed),
            per_reason,
            detached: self.detached.load(Ordering::Relaxed),
        }
    }
}

impl TreeObserver for InspectorCounters {
    fn element_mounted(&self, event: &ElementMounted) {
        self.mounts.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(element = ?event.element, parent = ?event.parent, slot = event.slot, "inspector: mounted");
    }

    fn element_moved(&self, event: &ElementMoved) {
        self.moves.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(element = ?event.element, parent = ?event.parent, slot = event.slot, "inspector: moved");
    }

    fn element_rebuilt(&self, event: &ElementRebuilt) {
        self.rebuilds.fetch_add(1, Ordering::Relaxed);
        for reason in event.reasons.iter() {
            if let Some(counter) = self.per_reason.get(reason as u8 as usize) {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }
        tracing::debug!(element = ?event.element, reasons = %event.reasons, "inspector: rebuilt");
    }

    fn element_unmounted(&self, event: &ElementUnmounted) {
        self.unmounts.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(element = ?event.element, "inspector: unmounted");
    }

    fn detached(&self) {
        self.detached.store(true, Ordering::Relaxed);
        tracing::debug!("inspector: stream detached");
    }
}

/// Point-in-time counters. `#[non_exhaustive]` — fields append.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectorSnapshot {
    /// Elements mounted since attach (including replay-seeded mounts).
    pub mounts: u64,
    /// Elements unmounted since attach.
    pub unmounts: u64,
    /// Completed builds since attach (first builds included — see
    /// `ElementRebuilt`'s docs).
    pub rebuilds: u64,
    /// Moves (reparents + keyed reorders) since attach.
    pub moves: u64,
    /// Per-reason rebuild tallies; read via [`Self::rebuilds_for`].
    per_reason: [u64; REASON_COUNT],
    /// Whether the stream ended (`detached()` fired) before this snapshot.
    detached: bool,
}

impl InspectorSnapshot {
    /// Rebuilds whose cause set contained `reason`.
    ///
    /// A build accumulating several causes counts once per contained
    /// cause, so per-reason tallies can sum to more than
    /// [`rebuilds`](Self::rebuilds).
    #[must_use]
    pub fn rebuilds_for(&self, reason: RebuildReason) -> u64 {
        self.per_reason
            .get(reason as u8 as usize)
            .copied()
            .unwrap_or(0)
    }

    /// Whether the observation stream has ended (`detached()` fired).
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.detached
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use flui_foundation::{ElementId, RebuildReasons};

    use super::*;

    #[test]
    fn counters_tally_events_and_reasons() {
        let counters = InspectorCounters::new();
        let observer: &dyn TreeObserver = &counters;
        let id = ElementId::new(1);

        observer.element_mounted(&ElementMounted::new(id, None, 0, TypeId::of::<()>()));
        let mut reasons = RebuildReasons::from_reason(RebuildReason::InitialMount);
        reasons.insert(RebuildReason::StateChange);
        observer.element_rebuilt(&ElementRebuilt::new(id, TypeId::of::<()>(), reasons));
        observer.element_moved(&ElementMoved::new(id, id, 2));
        observer.element_unmounted(&ElementUnmounted::new(id));
        observer.detached();

        let snapshot = counters.snapshot();
        assert_eq!(snapshot.mounts, 1);
        assert_eq!(snapshot.rebuilds, 1);
        assert_eq!(snapshot.moves, 1);
        assert_eq!(snapshot.unmounts, 1);
        assert_eq!(snapshot.rebuilds_for(RebuildReason::InitialMount), 1);
        assert_eq!(snapshot.rebuilds_for(RebuildReason::StateChange), 1);
        assert_eq!(snapshot.rebuilds_for(RebuildReason::AnimationTick), 0);
        assert!(snapshot.is_final());
    }
}
