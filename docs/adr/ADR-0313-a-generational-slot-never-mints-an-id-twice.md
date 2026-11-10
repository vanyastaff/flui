# ADR-0313: A generational slot never mints an id twice

- **Status:** Accepted (2026-11-04)
- **Date:** 2026-11-04
- **Related:** [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md) §4 (the counter ids'
  issuance stays `flui_foundation`, the `IdMint` family); [ADR-0209](ADR-0209-nested-scroll-connection.md)
  (the input sequence's id and the mint contract it rests on);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (a caught panic's owner
  turn may not re-mint a released identity); [ADR-0098](ADR-0098-owned-f64-geometry-values.md)
  (checked values, the rule the packed keys inherit); the **ID offset** row of `AGENTS.md`
  ("What the compiler and gates enforce"), which this record's first section states as a type;
  [ADR-0204](ADR-0204-synthetic-semantics-nodes.md) and
  [ADR-0223](ADR-0223-authored-semantics-regions.md) (the synthetic semantics identities §4
  packs)
- **Sources:** the workspace on main `1bac2457b` (2026-11-03), whose per-arena bookkeeping this
  record's table lists with its evidence

## Context and problem statement

A generational id packs a slot's index with a non-zero generation, so a stale id fails its
compare instead of addressing the slot's next occupant (ABA). Every tree hand-rolls this
bookkeeping today, and each picked its own exhaustion policy:

| Arena | Generation form | Exhaustion policy | Evidence (main `1bac2457b`) |
|---|---|---|---|
| The reactive signal graph | a bare `u32` per node | the claim's step checked, then a release whose live generation is `u32::MAX` leaves the free list un-restocked | `crates/flui-view/src/reactive/mod.rs:374-400`, `:561-577` |
| The element tree | a raw `NonZeroU32` per slab index | a preflight before teardown, a commit after; exhaustion panics and refuses the removal with the occupant kept | `crates/flui-view/src/tree/element_tree.rs:658-668`, `:1949-1967` |
| The render tree's slab | a raw `NonZeroU32` slot field with a `RETIRED` sentinel | `vacate` steps, or retires the slot into the slab's own free list | `crates/flui-rendering/src/storage/node_slab.rs:198-229` |

Three shapes, three exhaustion outcomes (a free-list omission, a panic, a retirement mark), and
no type: the rule reaches a reviewer only as prose, and the next arena copies whichever shape it
reads first. The identity theme calls this out (`x-invariants-07`: identity "compare by label,
stale generations, counters that wrap" is the recurring defect each copy re-risks). Where does
the one policy live?

## Decision drivers

- "Rules are types": the compiler before a lint, a lint before review (`AGENTS.md`, "Design
  stance"). An ADR that states the policy in words only leaves every next arena to re-decide it.
- No id is ever minted twice for one slot, on any target: a wrapped or seeded-again generation is
  the ABA defect the packing exists to prevent.
- A caught panic between a release's bookkeeping steps must not be able to mint the released
  identity again (ADR-0287's owner turn resumes code that holds foreign handles).
- The frame path is synchronous: the ledger is plain owner-thread state; nothing here crosses a
  thread and nothing here takes a lock.
- A synthetic semantics identity is recycled like any other slot, stays 64-bit at the AccessKit
  boundary (AccessKit's text positions name a run by a bare `NodeId` in the field's own tree, so
  neither a wider id nor a subtree gives runs a namespace) and never aliases a render identity.

## Considered options

1. One typed generation value and one ledger in `flui-foundation`, adopted by the trees.
2. Words only: this record states the policy; every arena keeps its hand-rolled bookkeeping.
3. A storage-owning arena: the ledger carries the arena's `Vec` or slab of nodes and its free
   list, and the trees adopt wholesale.

## Decision — Chosen option 1, because …

The trees' storage is not what repeats; their generation bookkeeping is. Option 2 keeps the
review-time rule the drivers start from, and option 3 rebuilds two storage types that the frame
path and layout touch everywhere, to carry bookkeeping the ledger can carry without owning the
storage.

### §1. A generation is a checked, 1-based, 31-bit value below the reserved domain bit

A packed key is `(generation << 32) | slot` in 64 bits, and its top bit is reserved for a domain
tag (§4), so a generation has 31 bits. `flui_foundation::generation::Generation` is the type;
the ledger of the next section mints it, and a raw value read back from a wire, a trace or a
test fixture enters through one checked conversion:

```rust
pub const DOMAIN_BIT: u64 = 1 << 63;                // no packed key carries it

pub struct Generation(NonZeroU32);
impl Generation {
    pub const FIRST: Self;                          // a fresh slot's first
    pub const LAST: Self;                           // 0x7FFF_FFFF, the last a packed key carries
    pub const fn as_u32(self) -> u32;               // the packed value
    pub const fn as_nonzero(self) -> NonZeroU32;
    pub const fn is_last(self) -> bool;
    pub const fn next(self) -> Option<Generation>;  // the checked step; None past LAST
}
impl TryFrom<NonZeroU32> for Generation { type Error = ReservedGenerationBit; }

impl<T: Marker> GenId<T> {                          // ElementId and PresentationId alike
    pub fn new_gen(index: u32, generation: Generation) -> Self;
    pub fn generation(self) -> Generation;
    pub fn from_u64(packed: u64) -> Option<Self>;   // None for no generation or the domain bit
}
```

- `next()` refuses past `LAST`: no generation wraps, so an id an arena minted cannot be minted
  again (the audit's identity rule, as a type).
- A packed key's constructor takes a `Generation`, so no key carries the domain bit; the refusal
  is the typed outcome of the one raw entry, `Generation::try_from`, which answers
  `ReservedGenerationBit` above `LAST`. Every raw decoder (`from_u64`, `ElementId`'s
  `Deserialize`, the recording wire, the drag-session wire key, the inspector's `NodeRef`
  parser) goes through it and refuses the bit.
- A counter that mints a generation outside a ledger (the UI runtime's incarnation, the native
  data-transfer offer table) steps through `Generation::next` and stops at `LAST`.
- The bare-`u32` and raw-`NonZeroU32` fields the trees kept before this record are the replaced
  forms; they stay nowhere once §3's adoptions land.

### §2. The ledger: `GenSlots`, one bookkeeping, one close

`flui_foundation::generation::GenSlots` carries the live generation of every slot, parallel to
the arena's storage, and nothing else — the storage and the free list stay with the arena:

```rust
pub struct GenSlots { /* the live generation per slot */ }
impl GenSlots {
    pub fn mint_fresh(&mut self) -> Generation;               // the arena grew by one slot
    pub fn mint_recycled(&mut self, slot: u32) -> Generation; // the slot returns: one checked step
    pub fn generation(&self, slot: u32) -> Option<Generation>;
    pub fn peek(&self, slot: u32) -> Option<Release>;
    pub fn commit(&mut self, release: Release);
}
```

- The release's step is *carried*: `peek` mints a `Release` (the slot, the carry it saw, the
  next generation) *before* the release runs user code — an element's unmount hooks — and
  `commit` applies it afterward. A caught panic between the two steps nothing, so the released
  identity cannot be minted from under a partially torn-down occupant, and a release cannot
  commit a step it never peeked (the type, not the convention, carries it).
- An exhausted slot is *closed at its release*: `peek` answers `None`, the arena re-lists
  nothing, and the ledger keeps the final generation so every stale handle keeps refusing the
  compare forever. An arena whose storage owns circulation (the element tree's slab) *refuses
  the release* — before any mutation, the occupant stays — because a slab that will re-serve a
  freed index cannot exclude one the ledger closed.

### §3. The adoption map

Which ledger call each tree uses, and at which point:

| Tree | Step | Close |
|---|---|---|
| The reactive signal graph | `mint_recycled` at the free-list claim | at release: `peek` is `None`, so the release re-lists nothing |
| The element tree | `peek` before teardown, `commit` after the removal's own mutation | `peek`'s `None` refuses the removal before any hook, and the occupant stays |
| The synthetic semantics identity source (§4) | `mint_fresh` on growth, `mint_recycled` at the free-list claim | at release: `peek` is `None`, so the release re-lists nothing |
| The render tree's slab | `Generation::next` on its own slot header | past `LAST` the header takes the slab's `RETIRED` sentinel, above every generation; the ledger's adoption lands with #2241 |

`Generation::FIRST` (one) is every fresh slot's base, and every compare against a live id
checks the ledger.

### §4. The reserved domain bit separates synthetic semantics identities

A synthetic semantics node (an authored region, a text run, an announcer; ADR-0204, ADR-0223)
publishes an `AccessibilityNodeId` beside the render-backed ones, which are packed `RenderId`s.
`SemanticsRegionId` wraps a `GenId` of its own private marker, and its published form sets
`DOMAIN_BIT`. No render id carries the bit (§1), so a synthetic and a render identity never
alias, at any slot and generation, and `RenderId::from_u64` refuses a synthetic value.

`SemanticsIdentitySource` is a slot arena over a `GenSlots` ledger, an occupied flag per slot
and a free list. A batch is all-or-nothing: its fresh slots' indexes are checked before any
mint. `release` accepts only the slot's current, occupied generation, so a stale or second
release answers `false` and lists nothing; a released slot returns under its next generation,
and one whose generation reached `LAST` closes. The pipeline releases a region's or a text
run's identity when its key leaves the retained map; mounted identities survive owner
replacement and accessibility disable/enable, and the announcers' live as long as the
presentation.

### Consequences

- Good, because the exhaustion policy is a type a reviewer reads once, and every later arena
  mints through the ledger instead of copying a shape.
- Good, because the release's preflight/commit pair is the type that pins "check before
  destruction" — the caught panic between the two steps nothing.
- Bad, because the packed generation's base shifts for the signal graph's slots; a trace field
  that recorded a generation before this record does not match one recorded after (release notes
  carry the break, not a migration).
- Bad, because the ledger is one more indirection between the arena's storage and its resolve;
  the compare stays one lookup, and the ledger's `Vec` is as flat as the fields it replaces.
- Bad, because the domain bit spends one generation bit: a slot has `2^31 - 1` generations
  rather than `2^32 - 1`.
- Neutral, because exhausting a generation needs `2^31 - 2` recycles of one slot: the close is a
  policy the code keeps, not a case a run reaches.

### Confirmation

- The compiler: a packed key's constructor takes a `Generation`, and the only constructors of a
  `Generation` from a raw value are `Generation::try_from`, which refuses the domain bit, and
  `seed_generation`, whose doc states it is a test's claim; `Generation::next` is `const` and
  checked, so no arena can compile a wrapping step.
- The tests: `crates/flui-foundation/tests/generation.rs` (`a_fresh_slot_starts_at_its_first_generation`,
  `a_recycled_slot_steps_to_the_generation_it_mints_next`,
  `an_exhausted_slot_has_no_release_step_and_closes_forever`,
  `a_release_commit_applies_only_what_its_peek_minted`,
  `slots_the_bookkeeping_does_not_carry_yet_grow_in_as_fresh_firsts`,
  `the_last_generation_stops_below_the_reserved_domain_bit`,
  `a_raw_generation_that_sets_the_reserved_bit_is_refused`,
  `a_packed_key_never_carries_or_decodes_the_reserved_domain_bit`); the signal graph's
  `reactive_graph_contract_matrix` rows `a_signal_slot_at_its_last_generation_is_retired_not_reused`
  and `unmounting_an_element_releases_what_it_owned_and_its_reads`
  (`crates/flui-view/src/reactive/mod.rs`); the element tree's
  `exhaustion_refusal_matrix` (`crates/flui-view/src/tree/element_tree/tests/exhaustion_refusal_tests.rs`):
  an exhausted removal refuses twice with the occupant live, per removal mode and per subtree
  position, through the public tree, and the parent tree's
  `element_tree_contract_matrix` row `stale_id_after_slot_reuse_resolves_none` still names the
  release's compare.
- §4: `crates/flui-semantics/tests/identity_source.rs`
  (`a_released_identity_returns_its_slot_under_a_new_generation`,
  `a_synthetic_identity_never_aliases_a_render_identity`), the source's own
  `synthetic_identity_exhaustion_never_wraps_or_partially_reserves` and
  `a_slot_at_its_last_generation_closes_at_its_release` (a private seed, since reaching `LAST`
  publicly takes `2^31` releases), and the render-object harness row
  `harness_custom_paint_semantic_regions_keep_order_geometry_identity_and_actions`: an authored
  region publishes the domain bit, and a removed key's slot returns under its next generation.

An accepted record whose every section is built becomes `Implemented` — the gaps entry below is
what holds this record at `Accepted`.

## Pros and cons of the options

### 1. The typed generation and the adopted ledger

- Good, because the policy is a type: the checked step and the carried release compile, and
  review stops re-deciding exhaustion.
- Good, because the trees keep their storage: the frame path's accessors are unchanged calls to
  a flatter `Vec`.
- Bad, because the ledger is cross-crate state the resolve must consult; a debug-only
  peek/commit compare is the only runtime check.

### 2. Words only

- Good, because no public API moves and no tree's internals change.
- Bad, because the three policies stay: the next arena re-derives one of them, which is the
  defect the audit's identity theme names.

### 3. The storage-owning arena

- Good, because one arena type carries storage, circulation and generations together.
- Bad, because both trees' storage is load-bearing in layout, paint and reconciliation;
  replacing it to carry bookkeeping the ledger holds without the storage spends the change on
  the part that has not been wrong.

## More information

- The state this record replaces is the three shapes in the context table, on main
  `1bac2457b`, each its own generation ledger; the render slab's shape
  (`crates/flui-rendering/src/storage/node_slab.rs`) is the one this record's ledger mirrors, and
  #2241 is where it adopts the ledger formally.
- The audit's identity theme (`x-invariants-07`, theme T11, `D:\flui`-reporting set of
  2026-11-03) called for `Generation` with a checked `next` and a `GenSlots` whose exhausted slot
  leaves circulation; this record is that step's record, with the peek/commit split and the two
  close shapes the trees already kept.