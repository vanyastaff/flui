//! Slot identity: the generational key a presentation's slot carries, and the
//! mint the owner issues it from (ADR-0305 §3).

use flui_foundation::{Marker, generation::GenSlots, id::GenId};

use crate::error::{EngineError, EngineResult};

/// The slot domain of [`SlotId`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "an uninhabited slot domain marker has no variants"
)]
pub enum SlotDomain {}

impl Marker for SlotDomain {}

/// A presentation's slot in the raster executor: a generational key, so an id
/// kept after its slot detached never names the slot's next occupant.
///
/// The owner mints it ([`mint_slot`](super::RasterSender::mint_slot)),
/// so attaching never waits for the raster side.
pub type SlotId = GenId<SlotDomain>;

/// Issues slot ids: a generational key per index, an index reused only once
/// its id is released, and an index whose generation is spent retired.
#[derive(Debug, Default)]
pub(crate) struct SlotMint {
    generations: GenSlots,
    /// Whether each index's current id is out, parallel to `generations`.
    issued: Vec<bool>,
    /// Released indexes whose generation can still advance, taken before a
    /// fresh index.
    free: Vec<u32>,
}

impl SlotMint {
    /// A new id: a released index at its next generation, else a fresh index.
    ///
    /// # Errors
    /// [`EngineError::SlotIdentityExhausted`] when no index is left.
    pub(crate) fn mint(&mut self) -> EngineResult<SlotId> {
        if let Some(index) = self.free.pop() {
            let generation = self.generations.mint_recycled(index);
            self.issued[index as usize] = true;
            return Ok(SlotId::new_gen(index, generation));
        }
        let index = u32::try_from(self.generations.slot_count())
            .map_err(|_overflow| EngineError::SlotIdentityExhausted)?;
        let generation = self.generations.mint_fresh();
        self.issued.push(true);
        Ok(SlotId::new_gen(index, generation))
    }

    /// Whether `id` is out: minted and not released since.
    pub(crate) fn is_issued(&self, id: SlotId) -> bool {
        self.generations.generation(id.index()) == Some(id.generation())
            && self.issued.get(id.index() as usize).copied() == Some(true)
    }

    /// Give `id`'s index back. An index whose generation cannot advance is
    /// retired, never reused; a stale id or one already released changes
    /// nothing.
    pub(crate) fn release(&mut self, id: SlotId) {
        if !self.is_issued(id) {
            return;
        }
        self.issued[id.index() as usize] = false;
        if self.generations.peek(id.index()).is_some() {
            self.free.push(id.index());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SlotMint;

    #[test]
    fn a_released_index_comes_back_under_a_new_id_and_a_spent_one_is_retired() {
        let mut mint = SlotMint::default();
        let first = mint.mint().expect("an index is left");
        mint.release(first);
        assert!(!mint.is_issued(first), "a released id is stale");
        let second = mint.mint().expect("an index is left");
        assert_eq!(
            second.index(),
            first.index(),
            "the released index is reused"
        );
        assert_ne!(second, first, "under a new generation");

        mint.generations
            .seed_generation(second.index(), 0x7FFF_FFFF);
        let last = flui_foundation::id::GenId::new_gen(
            second.index(),
            mint.generations
                .generation(second.index())
                .expect("the index is carried"),
        );
        mint.release(last);
        let third = mint.mint().expect("a fresh index is left");
        assert_ne!(third.index(), last.index(), "a spent index is retired");
    }
}
