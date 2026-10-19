# ADR-0243: One testing kit per layer, one switch name, exceptions kept as counted data

- **Status:** Proposed
- **Date:** 2026-10-19
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md), [`docs/testing.md`](../testing.md)

## Context

`docs/testing.md` maps the test support as a stack and says to pick the shallowest tier that can
fail for the reason that matters. Nothing enforces it, and the tree has drifted. The feature that
exposes a crate's test support is `testing` in eight crates, `test-support` in `flui-runtime` and
`test-utils` in `flui-view`. Tests that could use `HeadlessBinding` or `lay_out` carry their own
`pump` or `advance` in `flui-widgets`, `flui-runtime`, `flui-interaction` and `flui-animation`. A view
that counts its builds is written again in more than ten test files while `flui-testing` ships
`ProbeRoot`, two crates each define a `RecordingTextInput`, and `flui-app` and `flui-engine` keep
local doubles beside the headless fakes in `flui-platform`. A fix to the shared kit does not reach
the copies, a reader cannot tell which helper is supported, and a tool that wants to run an existing
test elsewhere has nothing uniform to adopt.

## Decision

**D1 — One switch name.** A crate exposes its test support behind a Cargo feature named `testing`.
`test-support` and `test-utils` are renamed. The feature exposes the crate's kit and nothing else.

**D2 — One kit per layer.** Each layer has one kit, `<crate>::testing`, built on the kit of the layer
below. From the frame layer up it is `flui-testing` (`HeadlessBinding`, `HeadlessHost`, `lay_out`).
A crate below `flui-testing` may use it from `tests/` through a dev-dependency: the layer check
skips dev-dependencies (`tools/xtask/src/workspace.rs`) and `flui-view` already does so. It cannot
pass its own types to `flui-testing` from a unit test inside `src/`, because that test is built
against a second copy of the crate. Unit tests that need the crate's private types keep a small kit
of their own, and the allowlist (D6) names it.

**D3 — Time and frames come from a kit.** A test that needs a clock, a frame or a pump takes it from
its layer's kit. A hand-written `pump`, `advance` or `tick` under `tests/` is an exception (D6).

**D4 — Doubles live with the contract.** A test double for a contract lives in the kit of the crate
that owns the contract, and a crate above reuses it within the dependency gates of ADR-0081 and
ADR-0082. Where a gate forbids the reuse, the crate keeps a local double and lists it. A view or probe
that two test files need moves into the kit.

**D5 — A kit is the headless target of the driver.** A kit does not grow a second finder or wait API
beside the driver's locators; it exposes what the driver adopts.

**D6 — Exceptions are data.** An allowlist names each exception and the change that removes it (an ADR
or a plan step) and counts exactly, as the `markers` and `file-length` gates do
(`tools/xtask/src/ratchet.rs`). A count above the tree is a finding, and so is one below it.

**D7 — The gate.** `cargo heli xtask testing` fails on:

- a Cargo feature named `test-support` or `test-utils`;
- a `fn pump`, `fn advance` or `fn tick` added in test code outside a kit and not listed;
- a type named `Mock*`, `Fake*`, `Scripted*`, `Recording*` or `Probe*` added in test code outside a
  kit and not listed.

Test code is a `tests/` or `benches/` target, a `#[cfg(test)]` module, an item behind the crate's
`testing` feature, or a file only such a module includes. Unconditional production items are not
scanned, so `flui-engine`'s `RecordingBudget`, which `DrawSegment` uses at run time, is never an
exception. The starting allowlist is built from the same scope. The gate joins
`cargo heli xtask checks`.

## Consequences

- The migration is gradual and measured: the allowlist starts at today's count and only goes down.
- Crates below `flui-testing` use it from `tests/` and keep private kits only for unit tests inside
  `src/`; the allowlist shows what remains.
- Renaming `test-support` and `test-utils` also changes normal dependency edges: `flui-testing`
  enables `flui-runtime/test-support` from its `[dependencies]`, and the facade exposes
  `flui-testing` through its optional `testing` feature. The rename step moves those edges in the same
  change and builds the facade with `--features testing` to prove consumer test builds still resolve.
- `docs/testing.md` gains the layer-to-kit table when the first migration lands.

## Alternatives rejected

| Option | Why rejected |
|---|---|
| One test crate used by unit tests of every layer | A unit test inside `src/` of a lower crate would receive a second copy of that crate's types through the dev-dependency cycle, so the private-type tests need a kit inside the crate. |
| Guidance without a gate | `docs/testing.md` already is guidance, and the drift above grew under it. |
| Delete the private helpers at once | Some test the layer below the kit and cannot use it; the allowlist lets each state why. |

## Verification

`testing_gate_fixture_rows` (xtask, run with `cargo heli nextest run -p xtask testing_gate`) walks a
fixture tree in the style of the `ratchet` fixtures. Its rows pin a listed helper passing, an unlisted
helper in `tests/` and in a `#[cfg(test)]` module failing, a feature named `test-support` failing, a
double behind the `testing` feature failing, an unconditional production `Recording*` type ignored,
and an allowlist count that grew or fell. `cargo heli xtask testing` on the tree reports the starting
allowlist with every entry citing this ADR or a plan step. The feature rename is checked with
`cargo heli check -p flui --features testing`. Each migration step lowers a count and keeps
`cargo heli nextest run` green on the changed crates.
