//! The versions an owner applied that the raster side has not acknowledged,
//! with their damage and the contents they replaced (ADR-0304 §7).

use std::collections::VecDeque;
use std::sync::Arc;

use super::content::BoundaryContent;
use super::id::SceneVersionId;
use crate::DamageRegion;

/// The most versions the log holds; one more folds the two oldest.
pub(crate) const PENDING_CAP: usize = 16;

/// One applied version the raster side has not acknowledged.
#[derive(Debug)]
struct PendingEntry {
    id: SceneVersionId,
    /// Its damage against the version applied before it.
    damage: DamageRegion,
    /// The contents its patch replaced or retired.
    replaced: Vec<Arc<BoundaryContent>>,
}

/// The unacknowledged versions, oldest first.
///
/// A version applied for another surface than the one before it carries
/// `Full(SurfaceChanged)` as its own damage, so a log that spans two surface
/// keys publishes full through the union and keeps no key of its own.
#[derive(Debug, Default)]
pub(crate) struct PendingLog {
    entries: VecDeque<PendingEntry>,
    folds: u64,
}
impl PendingLog {
    /// Records an applied version; at capacity the two oldest fold first,
    /// and the older one's contents come back to drop once the owner's state
    /// is committed: a stalled raster side costs a larger region, never
    /// memory past the log's capacity.
    pub(crate) fn push(
        &mut self,
        id: SceneVersionId,
        damage: DamageRegion,
        replaced: Vec<Arc<BoundaryContent>>,
    ) -> Vec<Arc<BoundaryContent>> {
        let released = if self.entries.len() == PENDING_CAP {
            self.fold_oldest()
        } else {
            Vec::new()
        };
        self.entries.push_back(PendingEntry {
            id,
            damage,
            replaced,
        });
        released
    }

    /// Folds the two oldest entries into one with the newer id and the union
    /// of their damage: it stays relative to the same base and only
    /// over-reports. Returns the older entry's contents.
    fn fold_oldest(&mut self) -> Vec<Arc<BoundaryContent>> {
        let (Some(older), Some(newer)) = (self.entries.pop_front(), self.entries.pop_front())
        else {
            unreachable!("BUG: the log folds only at capacity, which is more than one entry");
        };
        self.entries.push_front(PendingEntry {
            id: newer.id,
            damage: older.damage.union(newer.damage),
            replaced: newer.replaced,
        });
        self.folds += 1;
        older.replaced
    }

    /// Every change since the acknowledged version: the union of the log,
    /// `Unchanged` when it is empty.
    pub(crate) fn damage(&self) -> DamageRegion {
        self.entries
            .iter()
            .fold(DamageRegion::Unchanged, |damage, entry| {
                damage.union(entry.damage)
            })
    }

    /// Drops the entries at or below `acked` and hands back the contents they
    /// replaced.
    pub(crate) fn acknowledge(&mut self, acked: SceneVersionId) -> Vec<Arc<BoundaryContent>> {
        let mut released = Vec::new();
        while self
            .entries
            .front()
            .is_some_and(|entry| entry.id.seq() <= acked.seq())
        {
            let entry = self
                .entries
                .pop_front()
                .expect("BUG: the front entry was just seen");
            released.extend(entry.replaced);
        }
        released
    }

    /// Empties the log and hands back every content it held.
    pub(crate) fn clear(&mut self) -> Vec<Arc<BoundaryContent>> {
        self.entries
            .drain(..)
            .flat_map(|entry| entry.replaced)
            .collect::<Vec<Arc<BoundaryContent>>>()
    }

    /// Entries held now.
    #[cfg(feature = "testing")]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Folds so far.
    #[cfg(feature = "testing")]
    pub(crate) fn folds(&self) -> u64 {
        self.folds
    }
}
