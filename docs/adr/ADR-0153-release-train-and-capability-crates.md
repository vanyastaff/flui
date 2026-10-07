# ADR-0153: The release train and capability crates — question for 0.3

- **Status:** Proposed as an open question (2026-10-06). It records evidence and decides nothing
  for 0.2.0; the decision is taken with the first capability crate (0.3). A new crate for
  geometry is not on the table: ADR-0098 dissolved `flui-geometry` and `flui-types`.
- **Date:** 2026-10-06
- **Related:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md) (geometry is f64 values in
  `flui_foundation::geometry`, no units),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (the train guard),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) §3 (kinds; only `evolving` sets its own
  version), [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md),
  [ADR-0154](ADR-0154-capability-crates.md)

## Question

A capability crate (ADR-0154) has its own version and depends on the contract crate. The
contract depends on `flui-foundation`, which carries the train guard (`links = "flui_train"`),
and pins it exactly (`flui-foundation = "=0.2.0-dev"`, `crates/flui-platform-api/Cargo.toml:27`).
Does that tie every capability crate to every FLUI release, and if so, what is the narrowest
change that unties it?

Until 0.3 nothing changes: in 0.2.0 the contract depends on `flui-foundation` as it does today.

## Evidence

### What the contract names from `flui-foundation`

Read on `main` @ `d56188c14`, `rg 'flui_foundation::' crates/flui-platform-api/src`:

| Item | Where in a public signature | After ADR-0151 |
|---|---|---|
| `geometry::Point` | `PlatformWindow` (positions), `window.rs`, `text_store::session` | stays |
| `geometry::Size` | `PlatformWindow` (sizes), `window.rs` | stays |
| `geometry::Bounds` | `PlatformWindow`, `PlatformTextInput` (cursor area), `text_store::session` | stays |
| `geometry::EdgeInsets` | `PlatformWindow::safe_area_insets` and its callback | stays |
| `geometry::Offset` | `offset_from_coords`, `delta_offset_from_coords` only | deleted (no consumer) |
| `DataTransferId` | `DragDropEvent` (part of `PlatformInput`) | stays |
| `ClaimSlot`, `ClaimHandle`, `ClaimOutcome`, `claim_slot` | data-transfer request machinery | moves to the core host with the rest of data transfer |
| `panic::retain_opaque_payload` | text-store machinery, not in signatures | follows the machinery (open question Q5 of the spec) |

So after ADR-0151 the contract's signatures name four geometry values (`Point`, `Size`,
`Bounds`, `EdgeInsets`, all f64 per ADR-0098) and one identifier (`DataTransferId`).

### Where the train breaks a capability crate

Three cargo experiments in a scratch workspace (rustc 1.99.0; the transcript is in the pull
request). A stand-in `foundation` with `links = "flui_train"`, a `platform` contract that pins it
exactly and re-exports `Size`, a capability crate `location` built against one contract version,
and an application on another.

| Case | Setup | Result |
|---|---|---|
| A | The contract's version moves with the train: `location` needs `platform ^0.2` (foundation `=0.2.0`), the app uses `platform 0.3` (foundation `=0.3.0`) | Resolution fails: "package `foundation` links to the native library `flui_train`, but it conflicts with a previous package" |
| B | The contract keeps its own version: `platform 0.2.1` depends on foundation `=0.3.0`; `location` needs `platform ^0.2`; the app is on train 0.3 | Builds and runs; `location` reads the app's window through the 0.2.1 contract |
| C | No `links` key at all, contract 0.2 vs 0.3 | Fails anyway: E0277, the app's window implements the 0.3 `Window`, `location` takes the 0.2 one |

What breaks a capability crate is **the contract's own version changing incompatibly**, not the
geometry crate as such. Case C shows that removing the train guard would not help: two contract
versions are two trait identities. Case B shows that the train is harmless while the contract's
version stays semver-compatible across trains.

The geometry matters in one way only: an incompatible change to one of the four values above (or
to `DataTransferId`) in a train release forces an incompatible contract release, and with it
every capability crate.

## Options for 0.3

1. **The contract gets its own version** (like `evolving` crates today): it releases a
   compatible version on each train and bumps its own major or 0.N only when its own surface, or
   one of the five items above, changes incompatibly. Needs a change to the kind rule that makes
   every non-`evolving` crate inherit the workspace version, and `cargo-semver-checks` on the
   contract and on those five items in each release. No new crate.
2. **A narrow split by an amendment to ADR-0098** (`Supersedes` ADR-0098 in part): only the
   values the contract names (today `Point`, `Size`, `Bounds`, `EdgeInsets`), f64, no units, in
   the smallest home that does not ride the train. Only if option 1 proves insufficient.
3. **Capability crates ride the train**: each is released with every FLUI release. Simple; it is
   the cost Bevy's ecosystem pays on every release.

## How the decision is taken in 0.3

With the first capability crate: reproduce cases A and B with the real crates; count incompatible
changes to the five items across the 0.2 trains (`cargo-semver-checks` against each published
train); choose option 1 if there were none or they were avoidable, option 2 only with that
evidence, by an amendment to ADR-0098. The decision is a revision of this ADR.

## Consequences

- 0.2.0 ships with the contract on `flui-foundation`, unchanged.
- ADR-0154's capability crates depend on the contract (and, through it, `flui-foundation`) until
  this question is decided.
