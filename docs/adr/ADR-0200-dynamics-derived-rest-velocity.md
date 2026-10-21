# ADR-0200: Displacement rest tolerances derive their speed criterion from the motion

- **Status:** Accepted
- **Date:** 2026-10-11
- **Supersedes:** [ADR-0155](ADR-0155-ballistic-rest-tolerance-from-device-pixel-ratio.md),
  decision 1's absence of a speed criterion and its corresponding consequence only.
  Presentation DPR ownership, release-time acquisition, refusal and missing-owner fallback remain in force.
- **Related:** [ADR-0186](ADR-0186-typed-animation-rest-thresholds.md), converter-owned distances;
  the approved [physics requirements](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/plans/specs/flui-animation/physics/requirements.md),
  conservative rest timing and displacement-derived tolerance.

## Context

ADR-0155 made scroll rest distance half a device pixel. Its original description
said that this tolerance had no velocity limit. The accepted physics contract
instead derives a speed criterion from the motion's dynamics when a tolerance
specifies displacement alone. The shipped simulations already follow that
contract; this record reconciles the older statement without changing their code.

A distance bound alone does not express the remaining motion of a fast spring
passing close to its endpoint. An unrelated fixed logical-pixel speed would also
lose the presentation density and the simulation's natural time scale.

## Decision

`Tolerance::for_device_pixel_ratio(r)` supplies distance `0.5 / r` logical pixels
and a dynamics-derived speed criterion. The ratio and resulting distance must be
finite and positive. The tolerance's infinite speed value selects derivation;
it does not mean that speed is ignored.

A spring derives its speed limit as distance times its natural angular frequency.
Its conservative rest time bounds both displacement and speed for the remaining
analytic tail. Friction bounds the remaining glide by
`abs(velocity / ln(drag)) <= distance`; a separately configured finite speed limit
also constrains that motion. These criteria depend on the admitted dynamics,
not on display refresh rate or which frame first observes rest.

The presentation's DPR is still acquired at release time, with the existing
`1.0` fallback when its pipeline is unavailable. Invalid admission still creates
no ballistic simulation. This decision does not alter controller interruption,
the spring run's Hermite rest transition, or converter-owned component distances.

## Alternatives

Ignoring speed would disagree with the approved conservative-rest contract.
A fixed speed unrelated to the dynamics would require authors to choose a second
unit-sensitive threshold. Derivation keeps a displacement tolerance meaningful
for both a spring and a remaining friction glide without making frame cadence
part of physical rest.

## Consequences and verification

The old no-speed interpretation is superseded explicitly; the existing tests and
simulation behavior remain authoritative. The distance remains a visibility
tolerance, while rest also accounts for motion still capable of leaving it.

`rest_time_is_conservative_and_tight` in animation's spring contract family pins
the independent displacement/speed bounds and timing. The public
`tolerance_constructors_validate_and_scale_with_dpr` and
`friction_preserves_small_decay_and_position_time_roundtrips` families pin valid
construction, density scaling and remaining glide.
`scroll_fling_rest_scales_with_device_pixel_ratio` pins the actual mounted scroll
producer. Their existing execution evidence is unchanged by this documentation
reconciliation; no new native or performance result is claimed.
