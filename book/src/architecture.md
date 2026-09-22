# Architecture

The deep architectural material lives in the repository's own `docs/`, where it is checked
against the source tree by CI (`port-check`, `runtime-conformance-check`, `inventory-check`) — this
page is an index into it, not a restatement, so it can't drift out of sync with what those gates
actually enforce.

## Start here

- [`docs/architecture.md`](https://github.com/vanyastaff/flui/blob/main/docs/architecture.md) —
  the three-tree pipeline overview (`View` → `Element` → `RenderObject` → `Layer` →
  `flui-engine` → `wgpu`).
- [`docs/FOUNDATIONS.md`](https://github.com/vanyastaff/flui/blob/main/docs/FOUNDATIONS.md) — the
  architecture contract and the locked contracts (C1–C9).
- [`docs/PORT.md`](https://github.com/vanyastaff/flui/blob/main/docs/PORT.md) — the Flutter
  translation rules, the 23 refusal triggers `port-check` enforces, and the type map.

## Per-crate architecture

Each framework-layer crate carries its own `ARCHITECTURE.md` for the design decisions local to it,
including a `## Mapping decisions` section recording any deliberate Flutter divergence:
`crates/flui-{engine,foundation,layer,painting,platform,rendering,scheduler,widgets}/ARCHITECTURE.md`.

## Architecture Decision Records

Protocol-level decisions — ones that change a contract more than one crate depends on — are
recorded as ADRs under
[`docs/adr/`](https://github.com/vanyastaff/flui/tree/main/docs/adr). There is no single ADR index
page yet; browse the directory by filename (`ADR-NNNN-<slug>.md`) or search the repository for the
number cited by the code or comment you're reading. An ADR that revises an earlier one carries an
explicit `Supersedes:`/`Superseded-by:` pair — see AGENTS.md's ADR Policy.

## Runtime contract

[`docs/runtime-contract.toml`](https://github.com/vanyastaff/flui/blob/main/docs/runtime-contract.toml)
is the checked registry of FLUI's public shipped and planned runtime contracts — it is the
machine-readable source of truth `runtime-conformance-check` verifies against, and deliberately
does not depend on the design records above.
