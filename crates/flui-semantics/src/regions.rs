//! Accessible regions authored by one render object.

use flui_foundation::Marker;
use flui_foundation::generation::{DOMAIN_BIT, GenSlots};
use flui_foundation::geometry::{Matrix4, Rect};
use flui_foundation::id::GenId;

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
/// The slot domain of synthetic semantics identities.
#[derive(Debug)]
enum SyntheticSemantics {}

impl Marker for SyntheticSemantics {}

/// An allocated identity for content with no separate render object.
///
/// Obtain values from a presentation-owned [`SemanticsIdentitySource`]. It is
/// a generational slot key whose published form sets the packed key's
/// reserved [`DOMAIN_BIT`], which no render identity carries, so the two
/// never alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticsRegionId(GenId<SyntheticSemantics>);

impl SemanticsRegionId {
    /// The identity published to accessibility adapters.
    #[must_use]
    pub fn accessibility_id(self) -> AccessibilityNodeId {
        AccessibilityNodeId::from_u64(self.0.as_u64() | DOMAIN_BIT)
            .expect("BUG: the domain bit makes the published value non-zero")
    }
}

/// Presentation-local, generational identity authority for synthetic nodes.
///
/// Keep this source alive across semantics-owner replacement and accessibility
/// disable/enable transitions. A released identity's slot returns under its
/// next generation, so a stale identity never names the slot's next occupant;
/// a slot whose generation reached [`Generation::LAST`] closes at its release
/// and never returns.
///
/// [`Generation::LAST`]: flui_foundation::generation::Generation::LAST
#[derive(Debug, Default)]
pub struct SemanticsIdentitySource {
    /// The live generation of every slot the source has minted.
    generations: GenSlots,
    /// Whether each slot's current generation is handed out.
    occupied: Vec<bool>,
    /// Released slots with a next generation, taken before a fresh slot.
    free: Vec<u32>,
    /// Refuse every reservation that needs a slot (a test's exhaustion).
    exhausted: bool,
}

/// The presentation has exhausted the reserved synthetic identity space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("synthetic semantics identity space exhausted")]
pub struct SemanticsIdentityExhausted;

impl SemanticsIdentitySource {
    /// Reserves a complete batch or returns an error without consuming any IDs.
    /// A released slot is taken again only under its next generation, and
    /// exhaustion never wraps or reissues an earlier identity.
    pub fn allocate(
        &mut self,
        count: usize,
    ) -> Result<Vec<SemanticsRegionId>, SemanticsIdentityExhausted> {
        if count == 0 {
            return Ok(Vec::new());
        }
        if self.exhausted {
            return Err(SemanticsIdentityExhausted);
        }
        let recycled = count.min(self.free.len());
        let fresh = count - recycled;
        // Every fresh slot's index must fit the packed key's 32-bit slot.
        let slots = self
            .generations
            .slot_count()
            .checked_add(fresh)
            .ok_or(SemanticsIdentityExhausted)?;
        // Compared in u64: on a 32-bit target `u32::MAX + 1` does not fit a usize.
        if !u64::try_from(slots).is_ok_and(|slots| slots <= u64::from(u32::MAX) + 1) {
            return Err(SemanticsIdentityExhausted);
        }

        let mut ids = Vec::with_capacity(count);
        for _ in 0..recycled {
            let slot = self
                .free
                .pop()
                .expect("BUG: the recycled count is at most the free list's length");
            let generation = self.generations.mint_recycled(slot);
            self.occupied[slot as usize] = true;
            ids.push(SemanticsRegionId(GenId::new_gen(slot, generation)));
        }
        for _ in 0..fresh {
            let slot = u32::try_from(self.generations.slot_count())
                .expect("BUG: the fresh batch passed its slot-index preflight");
            let generation = self.generations.mint_fresh();
            self.occupied.push(true);
            ids.push(SemanticsRegionId(GenId::new_gen(slot, generation)));
        }
        Ok(ids)
    }

    /// Returns `id` to the source once its content left the tree.
    ///
    /// `false`, with nothing changed, for an identity this source does not
    /// hold out: a stale one, or one already released. A released slot is
    /// handed out again under its next generation, unless `id` carried the
    /// slot's last generation, which closes the slot.
    #[must_use]
    pub fn release(&mut self, id: SemanticsRegionId) -> bool {
        let slot = id.0.index();
        let held = self.generations.generation(slot) == Some(id.0.generation())
            && self.occupied.get(slot as usize) == Some(&true);
        if !held {
            return false;
        }
        self.occupied[slot as usize] = false;
        if !self.exhausted && self.generations.peek(slot).is_some() {
            self.free.push(slot);
        }
        true
    }

    /// Refuses every further reservation, so a test reaches exhaustion, which
    /// no practical run does. Identities already handed out stay valid.
    #[cfg(any(test, feature = "testing"))]
    pub fn exhaust(&mut self) {
        self.exhausted = true;
        self.free.clear();
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::generation::Generation;

    use super::*;

    #[test]
    fn synthetic_identity_exhaustion_never_wraps_or_partially_reserves() {
        let mut source = SemanticsIdentitySource::default();
        let existing = source.allocate(1).expect("an existing identity")[0];
        source.exhaust();
        assert_eq!(source.allocate(2), Err(SemanticsIdentityExhausted));
        assert!(source.release(existing), "a held identity stays releasable");
        assert_eq!(
            source.allocate(1),
            Err(SemanticsIdentityExhausted),
            "a release after exhaustion lists nothing"
        );
        assert_eq!(source.generations.slot_count(), 1, "no slot was reserved");
        assert_eq!(
            source.allocate(0).expect("empty reservation"),
            [] as [SemanticsRegionId; 0]
        );
    }

    // Reaching a slot's last generation takes 2^31 releases through the
    // public API; only the seed is private.
    #[test]
    fn a_slot_at_its_last_generation_closes_at_its_release() {
        let mut source = SemanticsIdentitySource::default();
        let first = source.allocate(1).expect("a first identity")[0];
        assert!(source.release(first));
        source
            .generations
            .seed_generation(first.0.index(), Generation::LAST.as_u32() - 1);
        let last = source.allocate(1).expect("the slot's last identity")[0];
        assert_eq!(last.0.index(), first.0.index());
        assert_eq!(last.0.generation(), Generation::LAST);
        assert!(source.release(last), "the last identity is released");
        assert!(!source.release(last), "and only once");
        let next = source.allocate(1).expect("a fresh slot")[0];
        assert_ne!(
            next.0.index(),
            last.0.index(),
            "the closed slot never returns"
        );
        assert_ne!(next.accessibility_id(), last.accessibility_id());
    }
}
