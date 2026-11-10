# ADR-0320: Packed keys reserve their top bit, and synthetic semantics ids set it

- **Status:** Accepted (2026-11-10)
- **Date:** 2026-11-10
- **Supersedes:** [ADR-0313](ADR-0313-a-generational-slot-never-mints-an-id-twice.md) §1 only (a
  generation's range, `u32` up to `u32::MAX`, and "minted only by the ledger": a generation is
  31 bits and a raw value enters through one checked conversion);
  [ADR-0204](ADR-0204-synthetic-semantics-nodes.md) §1 only (synthetic ids in `1..2^32`, a
  released value never reissued); [ADR-0223](ADR-0223-authored-semantics-regions.md) only for the
  identity source's monotonic `1..=u32::MAX` range and its never-returned values
- **Related:** [ADR-0313](ADR-0313-a-generational-slot-never-mints-an-id-twice.md) §2–§3 (the
  `GenSlots` ledger and its adoption map, in force),
  [ADR-0098](ADR-0098-owned-f64-geometry-values.md) (checked values, the rule the packed keys
  inherit), the **ID offset** row of `AGENTS.md` ("What the compiler and gates enforce")

## Context and problem statement

A packed generational key (`ElementId`, `RenderId`, `UiRuntimeId`, `PresentationId`,
`DataTransferId`) is `(generation << 32) | slot` in a `NonZeroU64`, with any non-zero `u32`
generation. Accessibility identities share one 64-bit space at the AccessKit boundary: a
render-backed `AccessibilityNodeId` is the packed `RenderId`, and a synthetic node (an authored
region, a text run, an announcer) has no render object. The synthetic source kept those apart by
staying below `2^32` (render ids always have a non-zero high half), so it could never reuse a
value: a released id was spent, ordinary region and text-run churn consumed a finite pool, and
exhaustion was permanent. Three counters minted generations outside the ledger, each up to
`u32::MAX` (the UI runtime's incarnation, the native data-transfer offer table, the render slab
header), and the raw decoders (`from_u64`, `ElementId`'s `Deserialize`, the recording wire, the
drag-session key, the inspector's `NodeRef` parser) admitted any. How do synthetic ids recycle slots without
aliasing a render id, and where is a generation's range checked?

## Decision drivers

- A stale platform action must never resolve to a new occupant of a recycled slot (ABA), on any
  target and after any number of releases.
- `AccessibilityNodeId` and AccessKit's `NodeId` stay 64-bit: AccessKit's text positions name a
  run by a bare `NodeId` in the field's own tree, so neither a wider id nor a subtree gives runs a
  namespace (ADR-0204, Context).
- "Rules are types": the range of a generation is a type, not a check each constructor repeats.
- Generation bookkeeping and its exhaustion policy stay in `flui_foundation::generation`
  (ADR-0313 §2); no allocator-specific policy.
- No intermediate arithmetic overflows on a 32-bit `usize` (wasm32).

## Considered options

1. Reserve the packed key's top bit as a domain tag; a generation is 31 bits; synthetic ids are
   `GenSlots` keys whose published form sets the bit.
2. Keep synthetic ids below `2^32` and the monotonic pool; reduce text-run churn only.
3. Give synthetic nodes their own id width or an AccessKit subtree.

## Decision

Chosen option: 1, because it keeps the platform width, makes domain separation a property of the
packed type, and reuses the one ABA policy.

### §1. A generation is 31 bits, and a raw value enters through one conversion

```rust
// flui_foundation::generation
pub const DOMAIN_BIT: u64 = 1 << 63;                // no packed key carries it

pub struct Generation(NonZeroU32);
impl Generation {
    pub const FIRST: Self;
    pub const LAST: Self;                           // 0x7FFF_FFFF
    pub const fn next(self) -> Option<Generation>;  // None past LAST
    pub const fn is_last(self) -> bool;
}
impl TryFrom<NonZeroU32> for Generation { type Error = ReservedGenerationBit; }
pub struct ReservedGenerationBit(pub NonZeroU32);

// flui_foundation::id, alike for GenId<T>, ElementId and PresentationId
impl<T: Marker> GenId<T> {
    pub fn new_gen(index: u32, generation: Generation) -> Self;
    pub fn generation(self) -> Generation;
    pub fn from_u64(packed: u64) -> Option<Self>;   // None for no generation or DOMAIN_BIT
}
```

- A packed key's constructor takes a `Generation`, so no key carries `DOMAIN_BIT`, and nothing
  wraps: the refusal is the typed outcome of the one raw entry, `Generation::try_from`, which
  answers `ReservedGenerationBit` above `LAST`.
- Every raw decoder goes through it: `from_u64`, `ElementId`'s `Deserialize`, the recording
  wire's `DataTransferId`, the drag-session wire key and the inspector's `NodeRef` parser.
- A counter that mints a generation outside a `GenSlots` (the UI runtime's incarnation, the native
  offer table, the render slab's header) steps through `Generation::next` and stops at `LAST`; the
  slab's `RETIRED` sentinel stays above every generation.

### §2. Synthetic semantics ids are tagged `GenSlots` keys

```rust
// flui_semantics::regions
pub struct SemanticsRegionId(GenId<SyntheticSemantics>);   // private marker
impl SemanticsRegionId {
    pub fn accessibility_id(self) -> AccessibilityNodeId;  // packed value | DOMAIN_BIT
}
pub struct SemanticsIdentitySource { /* GenSlots, occupied flags, free list */ }
impl SemanticsIdentitySource {
    pub fn allocate(&mut self, count: usize)
        -> Result<Vec<SemanticsRegionId>, SemanticsIdentityExhausted>;
    #[must_use]
    pub fn release(&mut self, id: SemanticsRegionId) -> bool;
}
```

- No render id carries `DOMAIN_BIT` (§1), so a synthetic and a render identity never alias at any
  slot and generation, and `RenderId::from_u64` refuses a synthetic value.
- A batch is all-or-nothing: the fresh slots' indexes are checked, in `u64`, before any mint. A
  recycled slot mints through `GenSlots::mint_recycled`, so its id differs from every earlier one.
- `release` accepts only the slot's current, occupied generation: a stale or second release
  answers `false` and lists nothing. A released slot returns under its next generation; a slot at
  `LAST` closes.
- The pipeline releases a region's or a text run's identity when its key leaves the retained map.
  Mounted identities survive owner replacement and accessibility disable/enable; the announcers'
  live as long as the presentation. A text run keeps the identity keyed by its text (ADR-0204 §1).

### Consequences

- Good, because region and text-run churn reuses slots, and a stale id stays distinguishable.
- Good, because domain separation is a property of the packed type, including through raw decoding.
- Bad, because a slot has `2^31 - 1` generations rather than `2^32 - 1`; closing at `LAST` stays
  the policy.
- Bad, because `new_gen` takes a `Generation`: a fixture with a raw generation spells
  `Generation::try_from`.
- Neutral, because two independently constructed sources can mint the same raw values; a foreign
  synthetic tree is still rejected at owner admission (ADR-0223).

### Confirmation

- The compiler: a packed key's constructor takes a `Generation`; the only raw constructors of a
  `Generation` are `Generation::try_from` and the test claim `GenSlots::seed_generation`, which
  refuses above `LAST`.
- `crates/flui-foundation/tests/generation.rs`: `the_last_generation_stops_below_the_reserved_domain_bit`,
  `a_raw_generation_that_sets_the_reserved_bit_is_refused`,
  `a_packed_key_never_carries_or_decodes_the_reserved_domain_bit`.
- `crates/flui-semantics/tests/identity_source.rs`:
  `a_released_identity_returns_its_slot_under_a_new_generation`,
  `a_synthetic_identity_never_aliases_a_render_identity`; the source's own
  `synthetic_identity_exhaustion_never_wraps_or_partially_reserves` and
  `a_slot_at_its_last_generation_closes_at_its_release`, through a private seed because reaching
  `LAST` publicly takes `2^31` releases.
- The render-object harness row
  `harness_custom_paint_semantic_regions_keep_order_geometry_identity_and_actions`: an authored
  region publishes `DOMAIN_BIT`, and a removed key's slot returns under its next generation.
- CI's wasm32 check compiles the source's bound on a 32-bit `usize`.

## Pros and cons of the options

### 1. The reserved domain bit and tagged slots

- Good, because reuse is bounded and ABA-safe with the one shared ledger.
- Good, because the range of a generation becomes a type every packed key shares.
- Bad, because the tag spends one generation bit.

### 2. The monotonic pool below `2^32`

- Good, because the encoding and the API stay as they are.
- Bad, because ordinary editing and region churn eventually exhaust the pool, permanently.

### 3. Another width or an AccessKit subtree

- Good, because the domains get independent numeric spaces.
- Bad, because AccessKit's `TextPosition` names a run by a bare `NodeId` resolved in the field's
  own tree, so runs in a subtree are unusable, and a wider id does not fit the adapter contract.

## More information

Revisit if AccessKit gains a per-subtree namespace for text positions, or if a second domain
needs a tag: a second domain splits the reserved bit's space, which needs its own record.
