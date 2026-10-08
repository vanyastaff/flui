# Architecture

FLUI has two maps: the tree pipeline explains how UI becomes a frame, and the workspace layers
explain which crates may depend on which. Start with the [concept overview](concepts/overview.md)
for the trees, then use this page to find the design records behind a particular subsystem.

The architectural source lives in the repository's own documentation. Crate manifests declare
the dependency rules, enforced by `cargo xtask workspace` and `cargo xtask reach`;
`cargo xtask docs-links` checks repository documentation links. These checks do not establish
that every described behavior is implemented. For a decision's implementation and migration
state, read its status and the architecture file of the owning crate.

## Start here

- [`docs/architecture.md`](https://github.com/vanyastaff/flui/blob/main/docs/architecture.md) —
  the pipeline overview (`View` → `Element` → `RenderObject` → `Layer` → `flui-engine` →
  `wgpu`).
- [`docs/FOUNDATIONS.md`](https://github.com/vanyastaff/flui/blob/main/docs/FOUNDATIONS.md) — the
  architecture contract and the locked contracts (C1–C9).
- [`AGENTS.md`](https://github.com/vanyastaff/flui/blob/main/AGENTS.md) — the design stance
  and the rules the compiler and gates enforce.
- [`docs/crates.md`](https://github.com/vanyastaff/flui/blob/main/docs/crates.md) — crate
  responsibilities and the dependency map.

## Follow a subsystem

| Question | Design record | Implementation guide |
|---|---|---|
| Who owns UI state, and how does work cross threads? | [ADR-0027: owner-affine UI runtimes](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0027-owner-affine-ui-realms.md), [ADR-0171: UI runtime and host vocabulary](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0171-ui-runtime-and-host-vocabulary.md) | [Runtime architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-runtime/ARCHITECTURE.md) |
| What is one frame, and where is a failed presentation contained? | [ADR-0048: frame transaction boundary](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0048-frame-transaction-boundary.md), [ADR-0083: frame runtime](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) | [Runtime architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-runtime/ARCHITECTURE.md) |
| Why are capabilities acquired outside `build`? | [ADR-0078: rules in types and lints](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0078-rules-live-in-types-and-lints.md) | [View architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-view/ARCHITECTURE.md), [lifecycle](concepts/lifecycle.md) |
| Where do platform contracts end and OS backends begin? | [ADR-0082: platform contract crate](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0082-platform-api-contract-crate.md) | [Platform API architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-platform-api/ARCHITECTURE.md) |
| How do paint commands become a composited scene? | [Pipeline overview](https://github.com/vanyastaff/flui/blob/main/docs/architecture.md) | [Rendering architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-rendering/ARCHITECTURE.md), [layer architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-layer/ARCHITECTURE.md), [engine architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-engine/ARCHITECTURE.md) |
| How are text measurement, carets and painted glyphs related? | [ADR-0092: per-runtime text over Parley](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0092-per-realm-text-over-parley.md) | [Painting architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-painting/ARCHITECTURE.md) |
| How do official packages use the framework? | [ADR-0088: packages, SDK and facade](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0088-official-packages-sdk-and-facade.md) | [SDK architecture](https://github.com/vanyastaff/flui/blob/main/crates/flui-sdk/ARCHITECTURE.md) |

Use the implementation guide to find the invariants and named tests for a decision. A record
may describe a migration with only some moves landed; the presence of an ADR is not a promise
that its whole target is available through the public API.

## Per-crate architecture

Most crates carry their own `crates/<crate>/ARCHITECTURE.md` for the design decisions local to
it, including a `## Mapping decisions` section recording those decisions.

## Architecture Decision Records

Protocol-level decisions — ones that change a contract more than one crate depends on — are
recorded as ADRs under
[`docs/adr/`](https://github.com/vanyastaff/flui/tree/main/docs/adr). Browse the directory by
filename (`ADR-NNNN-<slug>.md`) or search the repository for the number cited by the code or
comment you're reading. Read the record's `Status` first, then follow its related decisions
and migration notes. An ADR that revises an earlier one carries an explicit
`Supersedes:`/`Superseded-by:` pair — see AGENTS.md's ADR Policy.
