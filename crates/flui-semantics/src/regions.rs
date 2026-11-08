//! Accessible regions authored by one render object.

use flui_foundation::geometry::{Matrix4, Rect};
use flui_foundation::{
    Marker,
    generation::{GenSlots, Generation},
    id::GenId,
};

use crate::{AccessibilityNodeId, SemanticsConfiguration};

/// Logical identity within one render object's before- or after-child region list.
///
/// Use a domain identity, not a label or a position in the returned list. Reordering
/// a retained key preserves its accessibility identity; removing then reinserting
/// it creates a new identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticsRegionKey(pub u64);

/// One independently addressable accessible part of custom content.
#[derive(Debug, Clone)]
pub struct SemanticsRegion {
    /// Identity scoped to the host and the contribution list.
    pub key: SemanticsRegionKey,
    /// Bounds in the region's local coordinate space.
    pub rect: Rect<f64>,
    /// Local-to-host transform; layout position is applied by the pipeline.
    pub transform: Matrix4,
    /// Existing typed semantics attributes and action handlers.
    pub config: SemanticsConfiguration,
}

impl SemanticsRegion {
    /// Creates a region in the host's coordinate space.
    #[must_use]
    pub fn new(key: SemanticsRegionKey, rect: Rect<f64>, config: SemanticsConfiguration) -> Self {
        Self {
            key,
            rect,
            transform: Matrix4::IDENTITY,
            config,
        }
    }
}

/// Ordered contributions around a render object's actual children.
///
/// Each list has its own key namespace. These are flat region lists; nested
/// accessible structure follows the render children and normal merge policies.
#[derive(Debug, Clone, Default)]
pub struct SemanticsRegions {
    /// Content before the real children in accessibility traversal order.
    pub before_children: Vec<SemanticsRegion>,
    /// Content after the real children in accessibility traversal order.
    pub after_children: Vec<SemanticsRegion>,
}

#[derive(Debug)]
enum SyntheticSemantics {}

impl Marker for SyntheticSemantics {}

/// An allocated identity for content with no separate render object.
///
/// Obtain values from a presentation-owned [`SemanticsIdentitySource`]. The
/// top bit published to accessibility adapters distinguishes this generational
/// token from every render identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticsRegionId(GenId<SyntheticSemantics>);

impl SemanticsRegionId {
    /// The identity published to accessibility adapters.
    #[must_use]
    pub fn accessibility_id(self) -> AccessibilityNodeId {
        AccessibilityNodeId::from_u64(self.0.as_u64() | SYNTHETIC_ID_TAG)
            .expect("BUG: a synthetic generation makes its packed value non-zero")
    }

    fn slot(self) -> u32 {
        self.0.index()
    }
}

/// The domain bit reserved by every packed generational key.
const SYNTHETIC_ID_TAG: u64 = 1_u64 << 63;

/// Presentation-local generational identity authority for synthetic nodes.
///
/// Keep this source alive across semantics-owner replacement and accessibility
/// disable/enable transitions. Released slots return with a new generation;
/// a slot at its terminal generation leaves circulation permanently.
#[derive(Debug, Default)]
pub struct SemanticsIdentitySource {
    generations: GenSlots,
    occupied: Vec<bool>,
    free: Vec<u32>,
    exhausted: bool,
}

/// The presentation has exhausted the reserved synthetic identity space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("synthetic semantics identity space exhausted")]
pub struct SemanticsIdentityExhausted;

impl SemanticsIdentitySource {
    /// Reserves a complete batch or returns an error without consuming any IDs.
    /// Reused slots always advance their generation before publication.
    pub fn allocate(
        &mut self,
        count: usize,
    ) -> Result<Vec<SemanticsRegionId>, SemanticsIdentityExhausted> {
        let recycled = count.min(self.free.len());
        let fresh = count - recycled;
        let slot_count = self
            .generations
            .slot_count()
            .checked_add(fresh)
            .ok_or(SemanticsIdentityExhausted)?;
        if fresh > 0
            && (self.exhausted || slot_count == 0 || u32::try_from(slot_count - 1).is_err())
        {
            return Err(SemanticsIdentityExhausted);
        }

        let mut ids = Vec::with_capacity(count);
        for _ in 0..recycled {
            let slot = self
                .free
                .pop()
                .expect("BUG: the recycled count was reserved");
            let generation = self.generations.mint_recycled(slot);
            self.occupied[slot as usize] = true;
            ids.push(SemanticsRegionId(GenId::new_gen(
                slot,
                generation.as_nonzero(),
            )));
        }
        for _ in 0..fresh {
            let slot = u32::try_from(self.generations.slot_count())
                .expect("BUG: the complete fresh batch passed its index preflight");
            let generation = self.generations.mint_fresh();
            self.occupied.push(true);
            ids.push(SemanticsRegionId(GenId::new_gen(
                slot,
                generation.as_nonzero(),
            )));
        }
        Ok(ids)
    }

    /// Releases the current occupant of an identity's slot.
    ///
    /// Returns `false` for a stale identity or one already released. A released
    /// slot becomes available under its next generation, unless it was the
    /// slot's last generation.
    #[must_use]
    pub fn release(&mut self, id: SemanticsRegionId) -> bool {
        let slot = id.slot();
        let current = self.generations.generation(slot);
        if current.map(Generation::as_nonzero) != Some(id.0.generation())
            || self.occupied.get(slot as usize) != Some(&true)
        {
            return false;
        }
        self.occupied[slot as usize] = false;
        if !self.exhausted && self.generations.peek(slot).is_some() {
            self.free.push(slot);
        }
        true
    }

    /// Refuses all further reservations, so a test reaches exhaustion without
    /// consuming the practical identity space. Existing identities stay valid.
    #[cfg(any(test, feature = "testing"))]
    pub fn exhaust(&mut self) {
        self.exhausted = true;
        self.free.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::RenderId;

    #[test]
    fn synthetic_identity_exhaustion_never_wraps_or_partially_reserves() {
        let mut source = SemanticsIdentitySource::default();
        let existing = source.allocate(1).expect("an existing identity")[0];
        source.exhaust();
        assert_eq!(source.allocate(2), Err(SemanticsIdentityExhausted));
        assert!(source.release(existing));
        assert_eq!(source.allocate(1), Err(SemanticsIdentityExhausted));
        assert_eq!(source.allocate(1), Err(SemanticsIdentityExhausted));
        assert_eq!(
            source.allocate(0).expect("empty reservation"),
            [] as [SemanticsRegionId; 0]
        );
    }

    #[test]
    fn released_slots_advance_and_stale_releases_do_nothing() {
        let mut source = SemanticsIdentitySource::default();
        let first = source.allocate(1).expect("first identity")[0];
        assert!(source.release(first));
        assert!(!source.release(first), "a double release is refused");
        let second = source.allocate(1).expect("recycled identity")[0];
        assert_eq!(first.slot(), second.slot());
        assert_ne!(first, second, "a recycled slot has a new generation");
        assert!(!source.release(first), "a stale release is refused");
        assert!(source.release(second));
    }

    #[test]
    fn a_failed_batch_does_not_consume_a_recycled_slot() {
        let mut source = SemanticsIdentitySource::default();
        let ids = source.allocate(2).expect("two identities");
        assert!(source.release(ids[0]));
        source.exhausted = true;
        assert_eq!(source.allocate(2), Err(SemanticsIdentityExhausted));
        let recycled = source.allocate(1).expect("the free slot remains")[0];
        assert_eq!(recycled.slot(), ids[0].slot());
        assert_ne!(recycled, ids[0]);
    }

    #[test]
    fn a_terminal_slot_never_returns_to_circulation() {
        let mut source = SemanticsIdentitySource::default();
        let first = source.allocate(1).expect("first identity")[0];
        assert!(source.release(first));
        source
            .generations
            .seed_generation(first.slot(), Generation::LAST.as_u32() - 1);
        let last = source.allocate(1).expect("terminal identity")[0];
        assert!(source.release(last));
        let next = source.allocate(1).expect("a fresh slot remains")[0];
        assert_ne!(next.slot(), last.slot());
        assert!(!source.release(last));
    }

    #[test]
    fn tagged_synthetic_ids_cannot_alias_render_ids() {
        let render = RenderId::new_gen(u32::MAX, Generation::LAST.as_nonzero());
        let synthetic = SemanticsRegionId(GenId::new_gen(u32::MAX, Generation::LAST.as_nonzero()));
        let published = synthetic.accessibility_id();
        assert_ne!(published.as_u64(), render.as_u64());
        assert_eq!(published.as_u64() & SYNTHETIC_ID_TAG, SYNTHETIC_ID_TAG);
        assert_eq!(RenderId::from_u64(published.as_u64()), None);
        assert_eq!(render.as_u64() & SYNTHETIC_ID_TAG, 0);
    }
}
