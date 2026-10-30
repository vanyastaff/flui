# ADR-0264: A platform contract's test double lives in its crate's `testing` kit

- **Status:** Accepted (2026-10-29)
- **Date:** 2026-10-29
- **Supersedes:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §7 only (the
  "test doubles leave the contract" clause: `InMemoryTextStore` and the scripted halves of
  `FakeHaptics` and `InMemoryClipboard` in `flui-testing`) and §6 only (its last sentence, which
  points there)
- **Related:** [ADR-0243](ADR-0243-one-testing-kit-per-layer.md) D2 and D4 (one kit per layer,
  doubles with the contract), [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (the gates that
  decide which crate can hold a double), [ADR-0210](ADR-0210-two-tier-haptics.md) (the haptics
  double's behaviour)

## Context and problem statement

ADR-0151 §7 takes the platform contract's test doubles out of `flui-platform` and puts them in
`flui-testing`, so the Stable contract's API carries no test code. ADR-0243, accepted after it,
says a double for a contract lives in the kit of the crate that owns the contract, behind that
crate's `testing` feature (D1, D4), and that a crate's unit tests in `src/` cannot take its own
types from `flui-testing`, which is built against a second copy of the crate (D2). The two
decisions place the same types in different crates. Where do the platform contract's doubles live?

## Decision drivers

- The Stable contract's default API carries no test code (ADR-0151 §2, §5).
- A double stays next to what it doubles, so a contract change and its double change together,
  and every layer above reuses the one double (ADR-0243 D4).
- The dependency gates (ADR-0081) decide where a double that needs policy from another crate can
  live.
- The headless host is a runtime-selected production host: it cannot depend on a test crate, and
  its receivers must not grow without bound.

## Considered options

1. Each double in the `testing` kit of its contract's crate; a double built on policy that left the
   contract lives in the kit of the crate that holds that policy.
2. ADR-0151 §7 as written: every double in `flui-testing`.
3. Keep the doubles in the contract's default API.

## Decision

Chosen option: 1.

1. A double of a `flui-platform` contract is an item of `flui_platform::testing`, compiled under
   the crate's `testing` feature (and its own unit tests). `FakeHaptics`, with `HapticCall` and
   `HapticPlay`, is `flui_platform::testing::FakeHaptics`; `InMemoryClipboard`'s scripted half
   follows when the capability seam moves it.
2. A double built on policy that is no longer in the contract lives beside that policy:
   `InMemoryTextStore` drives `LockArbiter`, `CompositionLedger` and `EditGeneration`, which
   ADR-0151 §6 moves to `flui-interaction::text_input`, and `flui-platform` cannot depend on
   `flui-interaction`, so it is `flui_interaction::testing::InMemoryTextStore`, moved with that
   policy. The tests that exercise it with the contract's projection and gate move with it.
3. The headless host's receivers are its own production types in `flui-native`, unnamed outside
   it: `HeadlessHaptics` plays every request on the window's actuator and keeps nothing. A test
   that asserts what played installs the kit's double.
4. `flui-testing` enables the kits it builds on and re-exports their doubles where its own API
   hands them out (`TestHost::haptic_actuator` returns `flui_platform::testing::FakeHaptics`).

### Consequences

- Good, because a double lives where its contract changes, and `flui-platform`'s own unit tests
  use it without a second copy of the crate.
- Good, because the default API of the Stable contract still carries no double.
- Bad, because a crate that tests against a double adds the `testing` feature of the double's
  crate to its dev-dependencies.
- Neutral, because the headless host keeps a small receiver of its own beside the double, as
  ADR-0151 §7 already required.

### Confirmation

`cargo heli xtask testing` treats a crate's `src/testing` module as a kit (ADR-0243 D7), so a
`Fake*` in `flui_platform::testing` or `flui_interaction::testing` is not an exception, and one
in `flui_platform::haptics` would be scanned as production code that a test reaches;
`FakeHaptics` compiles only under `flui-platform`'s `testing` feature, so a consumer build that
names it without the feature fails; `stale_haptic_source_plays_on_the_window_actuator` in
`crates/flui-native/src/haptics/degradation/tests.rs` pins the headless receiver.

## Pros and cons of the options

### 1. In the contract's kit, or beside the policy it needs

- Good, because it is ADR-0243's rule, with the one exception the gates force named.
- Bad, because `InMemoryTextStore` is not in the contract's crate, so a reader looks for it in
  `flui-interaction`.

### 2. Every double in `flui-testing`

- Good, because one crate holds every double.
- Bad, because `flui-platform`'s unit tests cannot use them (ADR-0243 D2), and a contract change
  lands in two crates.

### 3. In the contract's default API

- Good, because no feature is needed.
- Bad, because test code ships in the Stable contract, which ADR-0151 removed for good reason.

## More information

- The platform-layer spec's D9 and its LY48 and LY50 tasks follow this ADR.
