# ADR-0314: Synthetic semantics ids use a tagged generational slot

- **Status:** Implemented (2026-11-07)
- **Date:** 2026-11-07
- **Supersedes:** [ADR-0204](ADR-0204-synthetic-semantics-nodes.md) §1 for the synthetic id encoding and release policy; [ADR-0223](ADR-0223-authored-semantics-regions.md) for the identity-source paragraphs; [ADR-0313](ADR-0313-a-generational-slot-never-mints-an-id-twice.md) §1 for the packed generation's upper bound
- **Related:** [ADR-0313](ADR-0313-a-generational-slot-never-mints-an-id-twice.md) (the shared slot ledger); [ADR-0223](ADR-0223-authored-semantics-regions.md) (presentation ownership and retained keys)

## Context and problem statement

Render-backed accessibility ids are generational, but synthetic regions, text runs and announcers shared a monotonic pool of only `1..=u32::MAX`. Text edits and removed regions permanently spent that pool, while a render id could in principle use any non-zero 32-bit generation. Synthetic ids need bounded reuse without ABA, must remain 64-bit at the AccessKit boundary, and cannot alias any render-backed id.

## Decision drivers

- A stale platform action must never resolve to a new occupant of a recycled slot.
- `AccessibilityNodeId` and AccessKit's `NodeId` remain 64-bit.
- Generation bookkeeping and terminal refusal belong to `flui_foundation::generation`, not another allocator-specific policy.
- The frame path is synchronous and the source is presentation-local.

## Considered options

1. Reserve the top bit as a domain tag and use `GenSlots` below it.
2. Keep the monotonic low-32-bit pool and shorten text-run identity churn only.
3. Give synthetic nodes a separate public id width or an AccessKit subtree.

## Decision

Chosen option: 1, because it preserves the platform width, proves domain separation in the packed type and reuses the existing ABA policy.

### §1. One tagged 64-bit layout

A packed generational key reserves its top bit. Its remaining high 31 bits are a non-zero generation and its low 32 bits are a zero-based slot. `Generation::LAST` is `0x7FFF_FFFF`; `GenSlots::next` closes a slot there. `GenId::new_gen` refuses a larger generation, and `GenId::from_u64` rejects both a zero generation and a set domain bit.

A render-backed `AccessibilityNodeId` is the unchanged packed `RenderId`, whose domain bit is zero. `SemanticsRegionId` privately wraps `GenId<SyntheticSemantics>` and sets the domain bit only when converting to `AccessibilityNodeId`. Equal slot and generation values in the two domains therefore publish different raw ids, including at their terminal boundaries.

### §2. Synthetic slots recycle only with a new generation

`SemanticsIdentitySource` owns a `GenSlots` ledger, an occupied bit per slot and a free list. Batch allocation preflights fresh-slot capacity and is all-or-nothing. A recycled allocation calls `mint_recycled`, so it advances the generation before publication. `release` accepts only the slot's current occupied generation; stale and double releases return `false` and cannot duplicate a free-list entry. Releasing the terminal generation closes the slot rather than listing it.

The pipeline releases a region or text-run identity when its key leaves the corresponding retained map. Mounted identities survive owner replacement and accessibility disable/enable. Announcer identities live for the presentation. Exhaustion still refuses only the affected reservation and never wraps, partially commits or reissues an old packed value.

### Consequences

- Good, because ordinary region and text-run churn has bounded slot consumption and stale ids remain distinguishable.
- Good, because synthetic and render identities cannot alias by construction, including through raw-id decoding.
- Bad, because packed generations have `2^31 - 1` values rather than `2^32 - 1`; terminal retirement remains the explicit policy.
- Neutral, because independently constructed presentation-local sources can still mint the same raw values; foreign synthetic trees remain rejected at owner admission.

### Confirmation

`tagged_synthetic_ids_cannot_alias_render_ids` checks the terminal slot/generation boundary and raw `RenderId` decoding. `released_slots_advance_and_stale_releases_do_nothing`, `a_failed_batch_does_not_consume_a_recycled_slot`, `a_terminal_slot_never_returns_to_circulation` and `synthetic_identity_exhaustion_never_wraps_or_partially_reserves` cover reuse, ABA, atomic refusal and closure. The existing custom-paint region harness checks removal/reinsertion, owner replacement and disable/enable through the public pipeline.

## Pros and cons of the options

### 1. Tagged generational slots

- Good, because reuse is bounded and ABA-safe with one shared ledger.
- Bad, because the domain tag spends one generation bit.

### 2. Monotonic low-32-bit ids

- Good, because the existing encoding is small.
- Bad, because ordinary editing eventually causes permanent refusal and the pool cannot be recycled.

### 3. Another id width or subtree

- Good, because domains could have independent numeric spaces.
- Bad, because AccessKit text positions carry a bare node id in the field's own tree, so a subtree does not provide a usable text-run namespace and a wider public id does not fit the adapter contract.
