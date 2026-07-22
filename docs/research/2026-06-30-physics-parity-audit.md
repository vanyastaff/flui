# Physics Parity Audit — Core.0 Deliverable N13

**Date**: 2026-06-30
**Auditor**: rust-builder (automated)
**Scope**: `crates/flui-types/src/physics/` vs Flutter reference at
`/home/vanyastaff/dev/flutter/packages/flutter/lib/src/physics/`

---

## Context

The `flui-types::physics` module is a *value-physics* layer: simulations that return
position and velocity scalars without `Send + Sync` or async requirements. It is
distinct from the *animation-physics* layer in `flui-animation::simulation`, which
the scroll-physics pipeline (`flui-widgets`) actually uses. Both layers are audited
here; changes are limited to `crates/flui-types`.

---

## Simulation 1 — Spring

### Flutter formula and classifier (spring_simulation.dart:285–295)

Flutter resolves the spring ODE variant by computing the discriminant
`Δ = c² − 4mk`:

```dart
return switch (spring.damping * spring.damping - 4 * spring.mass * spring.stiffness) {
  > 0.0 => _OverdampedSolution(...),
  < 0.0 => _UnderdampedSolution(...),
  _     => _CriticalSolution(...),   // exact zero only
};
```

The three closed-form solutions are mathematically continuous at the boundary.

### FLUI impl before fix (spring.rs:79–89)

```rust
let damping_ratio = self.damping / critical_damping;
if (damping_ratio - 1.0).abs() < 0.001 {   // ±0.001 tolerance band
    SpringType::Critical
} else if damping_ratio < 1.0 {
    SpringType::Underdamped
} else {
    SpringType::Overdamped
}
```

The 0.001 band around ζ = 1 misclassified springs with damping ratios in
`[0.999, 1.001]` as `Critical`, where Flutter classifies them as
under/overdamped. The discriminant identity `Δ = 4mk(ζ²−1)` makes the
equivalence precise: Flutter's `Δ > 0` is exactly `ζ > 1`.

Because all three solution families are continuous at the critical-damping
boundary, the observable position and velocity output difference is tiny for
|ζ − 1| = 5e-4. The classification tests use ζ = 1 ± 5e-4 because this offset
sits robustly INSIDE the old ±0.001 tolerance band (|5e-4| < 0.001), so the old
code would misclassify both as Critical and the tests would fail without the fix.
A 1e-3 offset (as used in the continuity tests) lies at the very edge of the band
and in practice falls outside it due to f32 rounding, making it a weak regression
guard. Flutter's regression test "SpringSimulation results are continuous near
critical damping" (`spring_simulation_test.dart:36`) explicitly verifies that
ζ = 1 ± 1e-3 produces the same output as ζ = 1 exactly — those continuity tests
use 1e-3 to verify the formula is continuous, not to verify the classifier fix.

### Verdict: DIVERGENCE-bug

### Action taken

Replaced the tolerance band with the exact discriminant check in
`SpringDescription::spring_type()`:

```rust
let discriminant = self.damping * self.damping - 4.0 * self.mass * self.stiffness;
if discriminant > 0.0 { SpringType::Overdamped }
else if discriminant < 0.0 { SpringType::Underdamped }
else { SpringType::Critical }
```

`flui-animation::SpringSolution::new()` (simulation.rs:393–415) already used
the discriminant check; this fix brings `flui-types` into alignment with both
Flutter and the animation layer.

### Tests added (spring.rs)

| Test | What it proves |
|---|---|
| `spring_type_critical_exact` | Exact `with_critical_damping` → `Critical` |
| `spring_type_underdamped` | ζ < 1 → `Underdamped` |
| `spring_type_overdamped` | ζ > 1 → `Overdamped` |
| `spring_type_slightly_underdamped_is_not_critical` | ζ = 0.9995 (5e-4 inside old band) → `Underdamped`; **fails on old code** |
| `spring_type_slightly_overdamped_is_not_critical` | ζ = 1.0005 (5e-4 inside old band) → `Overdamped`; **fails on old code** |
| `spring_critical_position_at_t0_4` | x(0.4) ≈ 0.0616 vs Flutter `spring_simulation_test.dart:47` |
| `spring_critical_velocity_at_t0_4` | v(0.4) ≈ 0.2681 vs Flutter test line 48 |
| `spring_slightly_underdamped_continuous_with_critical` | x(0.4) ≈ 0.0616 after classifier fix (continuity) |
| `spring_slightly_overdamped_continuous_with_critical` | x(0.4) ≈ 0.0616 after classifier fix (continuity) |
| `spring_underdamped_oscillates` | Underdamped spring overshoots target |
| `spring_overdamped_does_not_oscillate` | Overdamped spring does not overshoot |
| `spring_is_done_when_position_and_velocity_within_tolerance` | Simulation terminates |

---

## Simulation 2 — FrictionSimulation

### Flutter formula (friction_simulation.dart:40–58, 117–133)

Flutter uses a drag-coefficient `cₓ ∈ (0, 1)`:

```dart
FrictionSimulation(double drag, ...)
dx(t) = v₀ · drag^t
x(t)  = x₀ + v₀ · drag^t / ln(drag) − v₀ / ln(drag)
finalX = x₀ − v₀ / ln(drag)
```

`drag^t = e^(t · ln drag)` — same exponential curve family.

### FLUI impl (friction.rs:26–33, 114–135)

`flui-types::FrictionSimulation` uses a *decay rate* `k > 0`:

```rust
FrictionSimulation::new(decay_rate: f32, ...)   // `decay_rate` is rate k
velocity(t) = v₀ · e^(−k·t)
position(t) = x₀ + v₀ · (1 − e^(−k·t)) / k
final_position = x₀ + v₀ / k
```

The two forms are identical curves with the parameter mapping `k = −ln(cₓ)`.
Flutter's `cₓ = 0.135` → `k ≈ 2.0`.

### Would porting Flutter code break?

If a caller copies Flutter's `drag = 0.135` and passes it to
`flui-types::FrictionSimulation::new(0.135, ...)`, the simulation would be
**nearly frictionless** (k = 0.135 → very slow decay). This is a latent
parameter hazard. However:

- No callers outside `flui-types` exist (confirmed by workspace grep).
- Scroll physics uses `flui_animation::FrictionSimulation` with the Flutter
  `cₓ` convention, not this type.
- The type documents the convention since this audit.

### Verdict: DIVERGENCE-intentional-leapfrog

Rationale: the decay-rate form (`k > 0`) is the standard physics convention
(`F = −kv`) and avoids the counter-intuitive `drag ∈ (0, 1)` domain. Both
layers are internally consistent within FLUI.

### Action taken

Renamed the field and constructor parameter from `drag` to `decay_rate` throughout
`FrictionSimulation` (field, getter `decay_rate()`, all internal `self.decay_rate`
references, and the `BoundedFrictionSimulation::new` passthrough parameter). Added
explicit parameter-convention documentation to `FrictionSimulation::new` and to the
`decay_rate` field, citing the Flutter source file and the conversion formula
`k = −ln(cₓ)`. No callers outside `crates/flui-types` exist (confirmed by workspace
grep), so the public rename has no ripple.

### Tests added (friction.rs)

| Test | What it proves |
|---|---|
| `friction_position_at_t0_is_start` | x(0) = x₀ |
| `friction_velocity_at_t0_is_initial` | v(0) = v₀ |
| `friction_position_decays_over_time` | x(0.1), x(0.5), x(2.0) vs formula |
| `friction_velocity_decays_exponentially` | v(0.5), v(2.0) vs formula |
| `friction_final_position_formula` | x₀ + v₀/k = 150 |
| `friction_is_done_when_velocity_below_tolerance` | done threshold correct |
| `friction_negative_velocity_moves_in_negative_direction` | negative-direction case |
| `friction_time_to_velocity_round_trip` | v(time_to_velocity(50)) ≈ 50 |

---

## Simulation 3 — BoundedFrictionSimulation

### Flutter impl (friction_simulation.dart:172–201)

```dart
BoundedFrictionSimulation(drag, position, velocity, minX, maxX)
// x(t) = clamp(super.x(t), minX, maxX)
// dx() NOT overridden — velocity continues unbounded past boundary
// isDone: base done OR position within tolerance of minX or maxX
```

Two key properties:
1. **Two bounds** (`minX` and `maxX`)
2. **Velocity is NOT zeroed** at the boundary

### FLUI impl (friction.rs:138–238)

```rust
BoundedFrictionSimulation::new(drag, position, velocity, boundary: f32)
// Single directional boundary; direction inferred from velocity sign
// velocity() → 0.0 once boundary is reached
```

### Divergences

**A — API shape (DIVERGENCE-bug, not fixed):**
Single boundary vs Flutter's min+max. The `flui_animation::BoundedFrictionSimulation`
already uses the correct min+max API and is what scroll physics uses. Fixing
`flui-types` would require a breaking API change with no active callers outside
`flui-types`. Documented; tracked for consolidation once the two physics layers
merge (SP3).

**B — Velocity at boundary (DIVERGENCE-intentional):**
Both `flui-types` and `flui-animation` zero velocity once the boundary is reached,
with the explicit rationale: "a controller sampling a pinned simulation should see
zero velocity, not the still-decaying friction velocity." This is a consistent,
documented FLUI design choice across both physics layers.

### Verdict: API-shape is DIVERGENCE-bug (documented, not fixed); velocity-at-boundary is DIVERGENCE-intentional

### Tests added (friction.rs)

| Test | What it proves |
|---|---|
| `bounded_friction_clamps_position_at_boundary` | position never exceeds boundary |
| `bounded_friction_zeroes_velocity_at_boundary` | intentional divergence is exercised |
| `bounded_friction_is_done_at_boundary` | done once boundary reached |
| `bounded_friction_negative_direction` | negative-direction boundary |

---

## Simulation 4 — GravitySimulation

### Flutter formula (gravity_simulation.dart:71–90)

```dart
GravitySimulation(double acceleration, double distance, double endDistance, double velocity)
// endDistance >= 0 (assert)
// isDone(t) = x(t).abs() >= endDistance    // magnitude threshold, both directions
```

The `endDistance` is a **non-negative magnitude threshold**: done when
`|x(t)| ≥ endDistance` in either direction. Flutter's test confirms:
`GravitySimulation(-10, 0.0, 6.0, 10.0)` with `isDone(2.0) = false` and
`isDone(3.0) = true` (positions 0.0 and −15.0 respectively).

### FLUI impl (gravity.rs:26–119)

```rust
GravitySimulation::new(acceleration: f32, start: f32, end: f32, velocity: f32)
// end is the SIGNED TARGET POSITION
// is_done: directional — acceleration > 0 → pos >= end; acceleration < 0 → pos <= end
```

`flui-animation::GravitySimulation` uses the same signed-target API.

### Mapping

To replicate Flutter's `GravitySimulation(a, x₀, endDist, v₀)`:
- If `a < 0` (falling negative): set `end = −endDist`
- If `a > 0` (falling positive): set `end = +endDist`

The parity tests in this audit use `GravitySimulation::new(-10.0, 0.0, -6.0, 10.0)`,
producing x(1)=5, x(2)=0, x(3)=−15, `is_done(2.0)=false`, `is_done(3.0)=true` —
all matching Flutter's test (`gravity_simulation_test.dart:14`).

### Verdict: DIVERGENCE-intentional-leapfrog

A signed target position is more explicit than a magnitude threshold. Both FLUI
physics layers are consistent. Migration note added to `GravitySimulation::new` doc.

### Tests added (gravity.rs)

| Test | What it proves |
|---|---|
| `gravity_position_at_t0/t1/t2/t3` | x(t) values match Flutter's test line 19–30 |
| `gravity_velocity_at_t0/t1/t2/t3` | v(t) values match |
| `gravity_not_done_at_t0/t2` | `is_done` false before threshold |
| `gravity_done_at_t3` | `is_done` true after threshold (matches Flutter isDone(3.0)=true) |
| `gravity_positive_acceleration_example` | x(10) ≈ 500.5 (Flutter test line 11) |

---

## Simulation 5 — Tolerance defaults

### Flutter (tolerance.dart:14–18)

```dart
const Tolerance({
  this.distance = _epsilonDefault,   // 1e-3
  this.time     = _epsilonDefault,   // 1e-3
  this.velocity = _epsilonDefault,   // 1e-3
});
```

All three defaults are `1e-3 = 0.001`.

### FLUI impl before fix (tolerance.rs:28–32)

```rust
pub const DEFAULT: Self = Self {
    distance: 0.001,
    velocity: 0.01,    // 10× Flutter's value — BUG
    time: 0.001,
};
```

The `velocity: 0.01` default caused simulations to stop prematurely: when
velocity dropped below 0.01 px/s (but was still > 0.001), FLUI reported done
while Flutter would not. The `flui-animation::Tolerance::DEFAULT` already used
`velocity: 1e-3`; this was a `flui-types`-only bug.

### Verdict: DIVERGENCE-bug

### Action taken

Changed `velocity: 0.01` to `velocity: 0.001` in `Tolerance::DEFAULT`.

### Tests added (tolerance.rs)

| Test | What it proves |
|---|---|
| `tolerance_default_distance_matches_flutter` | distance = 0.001 |
| `tolerance_default_velocity_matches_flutter` | velocity = 0.001 (regression guard) |
| `tolerance_default_time_matches_flutter` | time = 0.001 |
| `tolerance_default_via_default_trait` | `Default::default()` == `DEFAULT` |

---

## Downstream Caller Check

Grep for all callers of `flui-types::physics` types outside `crates/flui-types`:

```
grep -rn "FrictionSimulation|BoundedFrictionSimulation|SpringSimulation|GravitySimulation" \
  crates/ --include="*.rs" | grep -v "flui-types" | grep -v target
```

Results: `flui-animation` and `flui-widgets` reference only their **own**
`FrictionSimulation` / `BoundedFrictionSimulation` / `SpringSimulation` /
`GravitySimulation` (declared in `flui-animation::simulation`). Zero callers
reference `flui_types::physics` types from outside `crates/flui-types`.

**Conclusion**: both fixes (spring classifier, tolerance velocity) are safe to
apply without ripple. No downstream build changes required.

---

## Gate Results

```
cargo test -p flui-types                                    118 passed, 0 failed
cargo clippy -p flui-types --all-targets -- -D warnings    Finished (no warnings)
cargo fmt -p flui-types -- --check                         Clean
```

---

## Summary

| # | Simulation | Flutter source | Verdict | Action |
|---|---|---|---|---|
| 1 | Spring classifier | spring_simulation.dart:291 | DIVERGENCE-bug | **Fixed**: discriminant check replaces tolerance band |
| 2 | Friction parameters | friction_simulation.dart:40 | DIVERGENCE-intentional | Renamed `drag` → `decay_rate` (field, getter, ctor param); documented: decay-rate k vs drag-coefficient cₓ |
| 3a | BoundedFriction API shape | friction_simulation.dart:181 | DIVERGENCE-bug | Documented (not fixed: no callers, flui-animation already correct) |
| 3b | BoundedFriction velocity at boundary | friction_simulation.dart:170 note | DIVERGENCE-intentional | Documented: consistent FLUI design choice |
| 4 | Gravity is_done / API | gravity_simulation.dart:71,90 | DIVERGENCE-intentional | Documented: signed target vs magnitude threshold |
| 5 | Tolerance velocity default | tolerance.dart:15 | DIVERGENCE-bug | **Fixed**: 0.01 → 0.001 |

Tests added: 40 new unit tests across spring.rs, friction.rs, gravity.rs, tolerance.rs.
