# Test-suite minimization: report (Phase 4)

One row per completed module/slice. Mutation score is measured with `cargo mutants`
(`.cargo/mutants.toml`, nextest tool), scoped to the exact source files each slice touches;
`caught`/`missed`/`unviable` counts and mutation score are all before-vs-after on that same file
scope, not the whole crate.

## flui-types — Point/Rect duplicate-of-property removal (pilot slice)

**What:** `crates/flui-types/tests/geometric_calculations_tests.rs` had six single-example tests
that turned out to be exact duplicates of properties `tests/geometry_property_tests.rs` already
proves for arbitrary inputs (this file existed before this effort; the duplication was discovered
during Phase 2 baseline review, not predicted in the Phase 1 inventory):

| Removed example test | Superseded by (already existing) property |
|---|---|
| `test_point_distance_symmetric` | `prop_point_distance_symmetric` |
| `test_point_distance_to_self_is_zero` | `prop_point_distance_self_is_zero` |
| `test_point_distance_non_negative` | `prop_point_distance_non_negative` |
| `test_point_triangle_inequality` | `prop_point_triangle_inequality` |
| `test_rect_intersect_commutative` | `prop_rect_intersection_commutative` |
| `test_rect_union_contains_both` | `prop_rect_union_contains_both` |

No new tests were written for this slice — the replacement already existed and already passed,
so the "never delete before the replacement exists and passes" rule was satisfied trivially.

**Tests:** 44 → 38 in `geometric_calculations_tests.rs` (6 removed, 0 added); `types_it` suite:
421 → 415, still 100% pass (`cargo nextest run -p flui-types --test types_it`).

**Runtime:** negligible change (`types_it` runtime is dominated by other files; 6 fewer
sub-millisecond example tests).

**Mutation score** (scope: `crates/flui-geometry/src/{point,rect}.rs`, tested via
`--test-package flui-geometry --test-package flui-types` since the properties exercise
`flui_types::geometry`'s re-exported `flui-geometry` types):

| | Before | After |
|---|---:|---:|
| Caught | 250 | 250 |
| Missed | 85 | 85 |
| Unviable | 156 | 156 |
| Total | 491 | 491 |
| Score (caught / (caught+missed)) | 74.6% | 74.6% |

**Identical, mutant-for-mutant.** The removed examples contributed zero unique fault detection;
the existing property tests already covered everything they covered, and nothing they didn't.

**Remaining missed mutants** (85, pre-existing, not touched by this slice — see
[baseline/flui-geometry.txt](baseline/flui-geometry.txt) for the full list): concentrated in
`Rect::abs`/`ceil`/`floor`/pixel-snapping helpers and in accessor/derive-adjacent code
(`Debug`/`Default` replacements) that the existing property and example tests don't assert
value-for-value. Not part of this slice's scope; noted for a future targeted pass, not fixed
here since fixing pre-existing gaps was not requested and would be a separate, larger change.

**Not touched in this slice** (per the original Phase 1 inventory, still open):
- `test_rect_intersect_self`, `test_rect_union_commutative`, `test_rect_union_self`,
  `test_vec2_addition_associative` — no matching property test exists yet; each is either a
  genuine one-off (`_self` identity checks) or a good *new* property to add later, not a
  duplicate to remove now.
- `Color::from_hex` family, `Pixels`/`DevicePixels` scale-conversion ladder, `BoxFit` variants,
  `flui-material` clusters — scoped for later slices of this module.
