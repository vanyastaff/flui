# ADR-0153: Stable crates do not depend on train-versioned crates

- **Status:** Proposed (2026-10-06). Awaiting the owner's approval; nothing in this ADR is
  implemented.
- **Date:** 2026-10-06
- **Amends, on acceptance:** [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (the
  train guard no longer reaches crates that depend only on the contract);
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) §3 (a new kind rule).
- **Related:** [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §1 ("the promise
  follows the item"), [ADR-0098](ADR-0098-owned-f64-geometry-values.md),
  [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md),
  [ADR-0154](ADR-0154-capability-crates.md)

## Context

The contract crate is `stable`, but its manifest pins `flui-foundation = "=0.2.0-dev"`, and
`flui-foundation` carries the train guard (`links = "flui_train"`, ADR-0088). Two consequences:

- Under ADR-0089 §1 every foundation type in a contract signature (`Size`, `Point`, `Offset`,
  `Bounds`, `EdgeInsets`, `DeviceSize`, `DataTransferId`) is already part of the Stable promise,
  while foundation itself is `internal` and changes with every release.
- Any crate that depends only on the contract (a plugin interface today, a capability crate under
  ADR-0154) is tied to the release train: each FLUI release forces a release of every such crate,
  and two of them built against different trains cannot be linked together. ADR-0088 notes that
  the guard "also ties platform plugins to the train". This is the ecosystem failure the
  capability crates must avoid.

The contract uses from foundation: the geometry values, `DataTransferId` (data-transfer
vocabulary, which ADR-0151 moves to the backend), `ClaimSlot` (backend machinery behind
`OfferTable`, also moving), and a panic-containment helper used by the text-store machinery
(whose home is decided after the Win32 text-services work).

## Decision

1. **A `stable` crate's normal dependencies are `stable` crates or external crates admitted by
   ADR-0089 §2.** No `internal`, `evolving` or train-guarded crate. `cargo xtask workspace`
   checks this as a kind rule, with a `--self-test` case that plants the violation.
2. **The geometry values used in Stable signatures move to a new `stable` crate,
   `flui-geometry`** (tier V, no workspace dependencies, no train guard, optional `serde`).
   `flui-foundation` re-exports it at the current paths, so no call site changes. The logical
   and device unit types of ADR-0098 move together; nothing about their arithmetic changes.
3. **The contract drops `flui-foundation`, `parking_lot` and `tracing`** once the items that
   need them have left (ADR-0151 §2) or the text-store machinery's home is decided.
4. **Timing:** before the first capability crate (ADR-0154), so no published capability crate
   ever depends on the train.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Declare `flui-foundation` Stable | It holds ID counters, claim slots, containment helpers and diagnostics that change every release; the promise would freeze machinery |
| Copy geometry types into the contract | Two `Size` types with conversions at every boundary; the trybuild geometry suite would have to cover both |
| Put geometry in `flui-protocol` | `flui-protocol` is the wire and semantics vocabulary; mixing geometry in gives it two reasons to change |
| Keep the train link and release every capability crate with each train | The Bevy ecosystem's recurring cost: every core release breaks every plugin |

## Consequences

- A capability crate built against `flui-platform 1.x` links with any FLUI 1.x.
- One more crate in tier V; geometry stops changing with the train and gets semver review.
- `flui-geometry` is free on crates.io (checked 2026-10-06).
