# ADR-0198: Animation admission errors preserve typed causes

- **Status:** Accepted
- **Date:** 2026-10-11
- **Supersedes:** [ADR-0186](ADR-0186-typed-animation-rest-thresholds.md), its threshold-error variant only; converter ownership and rest distances remain unchanged.
- **Related:** [ADR-0177](ADR-0177-controller-ordered-delivery.md), [ADR-0179](ADR-0179-controller-registration-ownership.md), [ADR-0197](ADR-0197-explicit-animation-completion.md).

## Context

An application admitting animation configuration must distinguish invalid data,
missing finite bounds and unrepresentable arithmetic. String-bearing errors
require message parsing and erase physical-construction causes. An error from
creating a value range also cannot meaningfully describe controller disposal.

## Decision

`ValueRange::try_new` returns `RangeDefect`: non-finite endpoints, inverted or empty
endpoints, or an overflowing span. Its checks retain that order. A controller
builder accepts only a validated `ValueRange`; no unused controller bounds-error
variant duplicates the value constructor's error domain.

`AnimationError` reports input kinds through `AnimationInput`, scalar values,
repeat endpoints and controller bounds, span endpoints, and gesture conversion
inputs. A finite input whose arithmetic overflows is distinct from an originally
non-finite input. Bounded `target` and `from` infinities retain clamping; an
unbounded endpoint refuses. Retarget and custom simulation inputs retain their
own finite-input requirements.

Scalar physical construction preserves `SimulationError`. Vector construction
preserves the same cause plus the zero-based component index. An oscillating
bounded fling reports its admitted damping ratio; it is not conflated with an
invalid rest threshold or coefficient overflow. Error variants are
`#[non_exhaustive]` and contain no user-owned callbacks or formatted strings.

Every refusal occurs before admission, preserving the preceding displayed value,
run and future. Existing diagnostic paths keep the operation name and release
controller borrows before invoking tracing subscribers. Range-shape validation
continues to return its error directly. Lifecycle failure priority and panic
containment are unchanged.

Finite repeat endpoints also require a finite span. This matches value-range and
ordinary transition admission: an overflowing difference must not reach repeat
sampling, even when both original endpoints are finite.

## Alternatives and migration

Retaining strings with a classifier would duplicate error state and require
consumers to trust prose. A single generic construction error would erase the
difference between missing bounds, arithmetic overflow and a converter's invalid
component. Adding `From<RangeDefect>` to the controller error would either invent
unavailable numeric endpoints or introduce an unused controller error variant.
Applications can retain their own aggregate error at the configuration boundary.

This intentionally changes pre-1.0 Rust and optional serde APIs. Migrate
`ValueRange::try_new` callers to `RangeDefect` and match structured `AnimationError`
variants instead of the former three string variants. The SDK exposes the same
types through `flui_sdk::animation`; it creates no copied error layer. Optional
serde preserves finite diagnostic values as numbers. Human-readable formats use
the exact strings `NaN`, `Infinity` and `-Infinity` for non-finite values, including
nested simulation causes. Decoding rejects null and other strings. NaN sign and
payload bits are canonicalized; classification and infinity signs are preserved.
Non-human-readable formats retain native `f64` encoding. The adapter is private
and shared by the two error domains; the public fields remain ordinary `f64`.

## Verification

`bounds_rejects_non_finite_endpoints` requires the narrow constructor result and
checks each defect. `typed_refusals_preserve_the_original_run` verifies input
fields, bounded infinity clamping, refusal state and original-future outcomes.
`overflowing_repeat_span_preserves_the_run` fails without the span-admission check.
The animated-value invalid-threshold contracts preserve the component's typed
distance failure while checking grouped refusal and subsequent progress.
`admission_error_wire_values` exercises actual refused controller admissions and
their human-readable wire values. The invalid-threshold rows also round-trip
actual nested simulation causes with the optional serde feature.
Physical overflow and interruption contracts continue to verify recovery.
