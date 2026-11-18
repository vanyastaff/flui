# ADR-0381: Animation errors are not serialized

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0198](ADR-0198-typed-animation-admission-errors.md), its serde wire format
  only (the paragraph on non-finite tokens and the private number adapter); the typed causes,
  their variants and fields stand
- **Related:** [ADR-0198](ADR-0198-typed-animation-admission-errors.md)

## Context and problem statement

ADR-0198 gave `AnimationError`, `AnimationInput`, `RangeDefect`, `SimulationError` and
`SimulationParameter` serde derives under the crate's `serde` feature. It also gave them a private
adapter that writes a non-finite number as the string `NaN`, `Infinity` or `-Infinity` in a
human-readable format. Nothing in the workspace serializes an animation error: a refusal is
matched where it happens, logged through `Display`, or wrapped in the application's own error.
The format is a public contract with no caller. Every new variant has to keep it, and the
adapter is a second encoding of `f64` that the rest of the framework does not use.

## Decision drivers

- A public contract has a caller (AGENTS.md, "a new `pub` item that no production path reaches").
- An error type says why an operation failed; persisting or sending it is the application's
  boundary, in the application's format.
- A feature flag widens the API only for data an application stores: curves, tweens and
  statuses.

## Considered options

1. **Keep the serde derives and the adapter.**
2. **Remove serde from the error types and delete the adapter.**

## Decision

Chosen option: 2. The five error types keep `Debug`, `Display`, `Clone`, `PartialEq` and
`thiserror::Error` and lose `Serialize` and `Deserialize`. The `diagnostic_number` adapter is
deleted. The `serde` feature keeps the value types an application stores: curves, `Tween`,
`IntTween`, `Rounding`, `AnimationStatus` and `AnimationBehavior`.

### Confirmation

`cargo check -p flui-animation --all-features` builds with no serde implementation on the
error types, and no test or production path serializes one.

## Pros and cons of the options

### 1. Keep them

- Good, because an application that logs errors as JSON gets the format for free.
- Bad, because the format binds every later variant and field, with no caller to say it is
  right.

### 2. Remove them

- Good, because the API states only what the framework uses, and new variants stay free.
- Bad, because an application that wants to send an error converts it at its own boundary,
  usually through `Display`.
