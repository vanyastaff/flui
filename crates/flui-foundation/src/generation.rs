//! A slot's generation, and the one bookkeeping for a slot arena's
//! generations ([`GenSlots`]).
//!
//! A generational slot's id packs the slot index with a non-zero generation:
//! the owning tree steps the slot's generation across its release, so an id
//! minted against the released occupant stops resolving, and no id minted
//! later for the same slot can equal one minted earlier (ABA). `Generation`
//! is that value as a type, and [`GenSlots`]
//! the bookkeeping every slot arena keeps: the live generation per slot,
//! fresh and recycled mints, and the exhaustion policy — a slot whose
//! generation space is exhausted is closed, and its final generation never
//! comes back as a live id.
//!
//! An arena closes a slot at its release: where the arena owns its
//! circulation (a free list it re-lists at release), the closed slot stays
//! off the list; where the storage owns circulation (the element tree's
//! slab), the release is refused before any mutation and the occupant stays.
//!
//! The peek and the commit are separate: the release's preflight
//! ([`GenSlots::peek`]) happens before the release runs user code — an
//! element's unmount hooks — and only the commit ([`GenSlots::commit`])
//! mutates; a caught panic between the two steps nothing, so the released
//! identity cannot be minted from under a partially torn-down occupant.
//!
//! A packed key is `(generation << 32) | slot` in 64 bits, and its top bit is
//! reserved for a domain tag (a synthetic semantics identity sets it), so a
//! generation is 31 bits wide: [`Generation::LAST`] is `0x7FFF_FFFF`. A raw
//! generation read back from a wire or a trace enters through
//! `Generation::try_from`, which refuses the reserved bit.

use std::num::NonZeroU32;

/// The top bit of a packed generational key, reserved for a domain tag. No
/// [`Generation`] reaches it, so no `GenId`, `ElementId` or `PresentationId`
/// carries it, and their raw decoders refuse a value that sets it. A domain
/// whose published ids sit beside another domain's packed keys sets it on its
/// own published form, so the two never alias.
pub const DOMAIN_BIT: u64 = 1 << 63;

/// The typed generation of one generational slot: 1-based, increasing, never
/// wrapped, at most [`Generation::LAST`]. A [`GenSlots`] mints it; a raw value
/// read back from a wire enters through `TryFrom<NonZeroU32>`. `as_u32` is the
/// packed value a generational key carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(NonZeroU32);

/// A raw generation a packed key cannot carry: it sets the top bit that every
/// packed generational key reserves for its domain tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("generation {0} sets the reserved domain bit of a packed key")]
pub struct ReservedGenerationBit(pub NonZeroU32);

impl Generation {
    /// A fresh slot's first generation.
    pub const FIRST: Self = Self(NonZeroU32::MIN);

    /// The last generation a packed key carries. The bit above it is the
    /// packed key's reserved domain bit; a slot at `LAST` closes at its release.
    pub const LAST: Self =
        Self(NonZeroU32::new(0x7FFF_FFFF).expect("BUG: the last packed generation is non-zero"));

    /// The packed value every generational key carries for this generation.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0.get()
    }

    /// The generation as the constructor form a packed-key mint takes.
    #[must_use]
    pub const fn as_nonzero(self) -> NonZeroU32 {
        self.0
    }

    /// Whether this is the slot's last generation: its release then closes
    /// the slot ([`GenSlots::peek`] answers `None`).
    #[must_use]
    pub const fn is_last(self) -> bool {
        self.0.get() == Self::LAST.0.get()
    }

    /// The generation after this one, or `None` past [`Generation::LAST`].
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        if self.is_last() {
            return None;
        }
        match self.0.checked_add(1) {
            Some(generation) => Some(Self(generation)),
            None => None,
        }
    }
}

impl TryFrom<NonZeroU32> for Generation {
    type Error = ReservedGenerationBit;

    /// The generation `raw` names, or the refusal when `raw` exceeds
    /// [`Generation::LAST`]. For a value read back from a wire, a trace or a
    /// test fixture; a live slot's generation comes from its [`GenSlots`].
    fn try_from(raw: NonZeroU32) -> Result<Self, Self::Error> {
        if raw.get() > Self::LAST.0.get() {
            return Err(ReservedGenerationBit(raw));
        }
        Ok(Self(raw))
    }
}

impl core::fmt::Display for Generation {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The step one release commits, as [`GenSlots::peek`] minted it: a release
/// applies only a step it peeked, against the slot as the peek carried it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    slot: u32,
    at: Generation,
    to: Generation,
}

/// The slot generations of one arena — the one exhaustion policy.
///
/// The arena's storage and circulation stay with the arena; this is the
/// generation ledger under every key it packs: a fresh slot's
/// [`mint_fresh`](Self::mint_fresh), a recycled slot's
/// [`mint_recycled`](Self::mint_recycled), the values a resolve checks a
/// handle against ([`generation`](Self::generation)), and the release
/// discipline: peek before user code runs, commit after the release's own
/// mutation is done.
///
/// A closed slot's entry keeps its last generation: the stale handles
/// against it keep refusing the compare forever, and the slot never mints
/// again.
#[derive(Debug, Clone, Default)]
pub struct GenSlots {
    /// The live generation per slot, parallel to the arena's storage. A slot
    /// whose release closed it keeps its last generation forever.
    live: Vec<Generation>,
}

impl GenSlots {
    /// Bookkeeping for an arena with no slots yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { live: Vec::new() }
    }

    /// A fresh slot's first generation; the arena's storage grew to hand
    /// this slot out, and the bookkeeping grows with it.
    pub fn mint_fresh(&mut self) -> Generation {
        let first = Generation::FIRST;
        self.live.push(first);
        first
    }

    /// The slot's next generation, committed as the arena takes it back
    /// from circulation.
    ///
    /// # Panics
    ///
    /// A listed slot is releasable by construction: an arena that carries it
    /// in circulation closed every exhausted slot at release
    /// ([`GenSlots::peek`] answers `None`), so reaching the refusal names a
    /// broken arena invariant.
    pub fn mint_recycled(&mut self, slot: u32) -> Generation {
        let release = self.peek(slot).expect(
            "BUG: an arena lists only slots whose release step exists; \
                 it closes a slot whose generation space is exhausted",
        );
        self.commit(release);
        release.to
    }

    /// The generation the slot's live ids carry, or `None` past the arena's
    /// end. A closed slot's value is its last generation: a stale handle
    /// against it refuses the compare, and nothing that release can mint.
    #[must_use]
    pub fn generation(&self, slot: u32) -> Option<Generation> {
        self.live.get(slot as usize).copied()
    }

    /// How many slots the bookkeeping carries — the diagnostic twin of the
    /// arena's slot count, for the growth assertions that keep the arena's
    /// storage and this ledger parallel.
    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.live.len()
    }

    /// The step a release of `slot` commits: peeked before the release runs
    /// user code, committed by [`GenSlots::commit`] once the release's own
    /// mutation is done. A caught panic between the two steps nothing, so the
    /// released identity cannot be minted from under a partially torn-down
    /// occupant.
    ///
    /// # Errors
    ///
    /// `None` once the slot's live generation is its last: the release
    /// cannot advance it and the slot's identity is closed now — the arena
    /// keeps the release's occupant, and every handle against the slot
    /// refuses forever.
    #[must_use]
    pub fn peek(&self, slot: u32) -> Option<Release> {
        let at = self.generation(slot)?;
        Some(Release {
            slot,
            at,
            to: at.next()?,
        })
    }

    /// Apply the peeked step, against the slot as its peek carried it.
    ///
    /// # Panics
    ///
    /// The commit lands only on a slot the bookkeeping carries: through the
    /// peek/commit pair the peek itself names a carried slot, so the refusal
    /// here names a broken invariant, and the debug compare pins the
    /// peek/commit pair.
    pub fn commit(&mut self, release: Release) {
        let live = self
            .live
            .get_mut(release.slot as usize)
            .expect("BUG: commit applies a step the bookkeeping's peek minted");
        debug_assert_eq!(
            *live, release.at,
            "BUG: a release commits against the slot as its peek carried it"
        );
        *live = release.to;
    }

    /// The live generation of `slot`, for a test reaching the arena's
    /// exhaustion boundary without reciting its release history. Production
    /// generations mint through [`GenSlots::mint_fresh`] and
    /// [`GenSlots::mint_recycled`]; a seeded value is what the arena's
    /// resolve checks handles against — a claim, not proof of a live occupant.
    ///
    /// Slots the bookkeeping does not carry yet grow in as fresh firsts.
    ///
    /// # Panics
    ///
    /// A seed names a generation a packed key carries: zero names no
    /// generation and a value above [`Generation::LAST`] sets the reserved
    /// domain bit; either names a broken test fixture.
    pub fn seed_generation(&mut self, slot: u32, generation: u32) {
        let generation =
            NonZeroU32::new(generation).expect("BUG: a generation is named by a non-zero u32");
        let generation = Generation::try_from(generation)
            .expect("BUG: a seeded generation stays below the packed key's reserved bit");
        if slot as usize >= self.live.len() {
            self.live.resize(slot as usize + 1, Generation::FIRST);
        }
        self.live[slot as usize] = generation;
    }
}
