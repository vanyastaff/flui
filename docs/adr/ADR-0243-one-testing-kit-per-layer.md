# ADR-0243: One testing kit per layer, one switch name, exceptions kept as counted data

- **Status:** Proposed
- **Date:** 2026-10-19
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (layers and the
  dependency-direction gate), [ADR-0082](ADR-0082-platform-api-contract-crate.md) (which crates
  may name `flui-platform`), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (the frame
  transaction the frame kit drives), [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md)
  (input recording, which the headless target replays), [`docs/testing.md`](../testing.md) (the tier
  map this ADR makes binding)

## Context

`docs/testing.md` describes the test support as a stack and says to pick the shallowest tier that
can fail for the reason that matters. Nothing enforces it, and the tree has drifted:

- The switch that exposes a crate's test support is `testing` in eight crates (rendering, objects,
  semantics, layer, painting, scheduler, interaction, engine), `test-support` in `flui-runtime` and
  `test-utils` in `flui-view`.
- Tests that could use `HeadlessBinding` or `lay_out` carry their own frame or clock helpers:
  `flui-widgets` (`tests/text_store_kit.rs`, `editable_text.rs`, `perf.rs`, `transitions.rs`,
  `frame_partitions.rs`), `flui-runtime` (`tests/contracts/frame_rate_demand.rs`), `flui-interaction`
  (`tests/multi_pointer_recognizers.rs`) and `flui-animation` (`tests/support/motion_trace.rs`).
- A view that counts its builds is written again in at least eleven test files across `flui-widgets`,
  `flui-material`, `flui-cupertino`, `flui-view` and `flui-scheduler`, while `flui-testing` already
  ships `ProbeRoot` and `ProbeSignals`.
- Two crates each define a `RecordingTextInput` (`flui-interaction` and `flui-testing`), and
  `flui-app` and `flui-engine` hold local `Scripted*`, `Fake*` and `Recording*` backends beside the
  headless platform fakes in `flui-platform`.

These counts come from a search by type and function name, so each hit is a candidate. The drift
costs in three ways. A fix to the shared kit does not reach the copies. A reader cannot tell which
helper is the supported one. And any tool that wants to run an existing test elsewhere, such as the
planned driver's headless target, has nothing uniform to adopt.

## Decision

1. **One switch name.** A crate exposes its test support behind a Cargo feature named `testing`.
   `test-support` and `test-utils` are renamed. The feature exposes the crate's kit and nothing else.
2. **One kit per layer.** Each layer has one kit, `<crate>::testing`, built on the kit of the layer
   below. From the frame layer up the kit is `flui-testing` (`HeadlessBinding`, `HeadlessHost`,
   `lay_out`). A crate below `flui-testing` may use it from `tests/` through a dev-dependency: the
   layer check skips dev-dependencies (`tools/xtask/src/workspace.rs`) and `flui-view` already does
   so. What such a crate cannot do is pass its own types to `flui-testing` from a unit test inside
   `src/`, because that test is built against a second copy of the crate. Unit tests that need the
   crate's private types keep a small kit of their own, and the allowlist (6) names it.
3. **Time and frames come from a kit.** A test that needs a clock, a frame or a pump takes them from
   its layer's kit. A hand-written `pump`, `advance` or `tick` under `tests/` is an exception (6).
4. **Doubles live with the contract.** A test double for a contract lives in the kit of the crate
   that owns the contract, and a crate above reuses it, within the dependency gates of ADR-0081 and
   ADR-0082. Where a gate forbids the reuse, the crate keeps a local double and lists it as an
   exception. A view or probe that two test files need moves into the kit.
5. **A scenario written for a kit is the headless target of a scenario written for the driver.** A
   kit does not grow a second finder or wait API beside the driver's locators; it exposes what the
   driver adopts.
6. **Exceptions are data.** An allowlist names each exception and the change that removes it (an ADR
   or a plan step), and counts exactly, as the `markers` and `file-length` gates do
   (`tools/xtask/src/ratchet.rs`). A count above the tree is a finding, and so is one below it.
7. **The gate.** `cargo xtask testing` fails on:
   - a Cargo feature named `test-support` or `test-utils`;
   - a `fn pump`, `fn advance` or `fn tick` added under `tests/` outside a kit and not listed;
   - a type named `Mock*`, `Fake*`, `Scripted*`, `Recording*` or `Probe*` added in test code outside a
     kit and not listed.

   Test code is a `tests/` or `benches/` target, a `#[cfg(test)]` module or a file only such a module
   includes. Production items are not scanned, so a type such as `flui-engine`'s `RecordingBudget`,
   which `DrawSegment` uses at run time, is never an exception. The starting allowlist is built from the
   same scope. The gate joins `cargo xtask checks`.

## Consequences

- The migration is gradual and measured: the allowlist starts at today's count and only goes down.
- Crates below `flui-testing` use it from `tests/` and keep small private kits only for unit tests
  inside `src/`. That duplication is deliberate, and the allowlist shows what remains.
- Renaming `test-support` and `test-utils` changes the dev-dependency feature lists of their users
  and nothing in a consumer's build, because the features are test-only.
- `docs/testing.md` gains the layer-to-kit table when the first migration lands.

## Alternatives considered

- **One test crate for the whole workspace, used by unit tests too.** Rejected: a unit test inside
  `src/` of a lower crate would receive a second copy of that crate's types through the dev-dependency
  cycle, so the private-type tests need a kit inside the crate.
- **Guidance without a gate.** Rejected: `docs/testing.md` already is guidance, and the drift above
  grew under it.
- **Delete the private helpers at once.** Rejected: some of them test the layer below the kit and
  cannot use it. The allowlist lets each exception state why.

## Verification

- `cargo nextest run -p xtask testing`: a fixture tree with a listed and an unlisted helper, a renamed
  feature and a count that grew or fell, in the style of the `ratchet` fixtures.
- `cargo xtask testing` on the tree reports the starting allowlist with every entry citing this ADR
  or a plan step.
- Each migration step lowers a count and keeps `cargo nextest run` green on the changed crates.
