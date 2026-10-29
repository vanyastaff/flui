# ADR-0184: Coordinate mapping preserves admitted pointer readings

- **Status:** Accepted
- **Date:** 2026-10-10
- **Related:** [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md),
  [ADR-0098](ADR-0098-owned-f64-geometry-values.md)

## Context

`PointerMove` admits measured and predicted histories through constructors that
order readings and exclude exact copies of the current source sample. Interaction
routes then convert source positions into each receiving node's coordinates.
Distinct readings with a shared coarse timestamp and equal sensors can round to
the same local position under a large finite affine translation. Reconstructing
the localized movement through its admission constructors then removes accepted
readings, changing history cardinality solely because the coordinate space changed.

## Decision

`flui-platform::PointerMove::try_map_positions` converts only checked positions
and returns an owned movement. It preserves pointer metadata, buttons, modifiers,
sample times, sensors, and measured/predicted membership and ordering. A mapper
error returns no partial movement and leaves the borrowed source unchanged.
Coordinate conversion never repeats admission filtering, even when distinct
source readings become equal local samples. Mapping cannot alter time or sensors.

`flui-interaction` uses this operation for actual pointer-route localization. A
position that cannot project into a finite checked logical coordinate refuses
the local delivery; the original global event remains available unchanged.
Source-event construction continues to enforce ADR-0143's admission rules.

## Evidence

The public `input_vocabulary_contract` row
`position_mapping_preserves_readings_and_refuses_partial_output` covers mapping
failure at current, measured and predicted positions, source preservation, and
successful recovery retaining all readings. The actual public hit-route
`hit_test_transform_admission` row
`localization_preserves_distinct_readings_that_round_to_the_same_position`
pins global source preservation and local history cardinality, positions,
timestamps and sensors under an admitted affine transform.
