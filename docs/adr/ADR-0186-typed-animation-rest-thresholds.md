# ADR-0186: Spring rest distances belong to the value converter

- **Status:** Accepted
- **Date:** 2026-10-10
- **Related:** [ADR-0176](ADR-0176-presentation-animation-playback.md),
  [ADR-0183](ADR-0183-coordinated-property-motion-admission.md).

## Context

A fixed scalar tolerance treats logical pixels, normalized alignment and
premultiplied color components as the same unit. A nested value can contain all
of them in one vector. Configuring one threshold on its motion description
cannot retain those distinctions; asking every widget to configure component
thresholds repeats knowledge already owned by its converter.

## Decision

`TwoWayConverter::rest_thresholds()` returns positive finite distances in the
converter's fixed-width vector representation. Its return type has the same
component count as the motion vector. Manual implementations explicitly choose
their distances. The derive concatenates each field's thresholds in declaration
order, including nested fields, without passing distances through `to_vector`.
Nonlinear converters do not generally preserve errors under value conversion.

Offset, size and insets use 0.01 logical pixels. Scalar, alignment and
premultiplied Oklab components use 0.001 in their representation's units. These
are conservative type defaults, not a device-pixel visibility guarantee. A
converter for a different unit or precision selects its own distances. The
existing Hermite rest transition bounds its remaining displacement by
`31 / 27` times the selected distance, then reaches the exact authored target
with zero velocity. Geometry's bound is consequently below 0.012 logical pixels.

The spring derives each speed limit from its natural rate and the component's
distance through the existing `Tolerance` contract. Rest timing remains
independent of observing frame partitions. Curve motion and direct scalar
controller motion retain their existing contracts; the latter has no typed
converter from which to infer value units.

Threshold calls run during preparation outside state guards. Spring admission
validates every distance before replacing the previous run. An invalid distance
returns `AnimationError::InvalidSpring`; neither cancellation nor a prefix of a
coordinated update is admitted. The existing seam validation rejects or retries
preparation invalidated by converter reentry. Fixed arrays require no heap
storage for thresholds, and sampling invokes no threshold conversion.

All spring components in one animated value share the latest native rest start.
Their common Hermite interval uses the shortest permitted duration at that
start, preserving each displacement bound. Independently ending components
would distort relationships in a premultiplied color: a fade can otherwise
change hue on its last visible frame. A common interval preserves the analytic
vector trajectory and proportional components through exact arrival. Separate
property owners retain independent rest timing.

## Alternatives

A shared default leaves mixed-unit vectors unresolved. A threshold configured
only on `MotionSpec` makes widget authors reconstruct the vector's units and
field order. Converter-owned thresholds keep that knowledge beside conversion
and let nested derives preserve it. Requiring the method on manual converters
is an intentional pre-1.0 migration instead of guessing units for custom values.

## Evidence

`owning_animated_value_contract` observes geometry reaching exact rest before
scalar motion under the same spring. It checks zero, negative, NaN and infinite
threshold refusal, old-run progression and coordinated admission refusal.
`two_way_converter_derive_contract` observes geometry retaining its threshold
inside a nested registered value. The color fade case in
`tolerance_constructors_validate_and_scale_with_dpr` observes visible hue through
the rest interval. Built-in implicit geometry consumers use the same
`AnimatedValue` path; no separate widget threshold protocol is introduced.
