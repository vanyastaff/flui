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
- `flui-material` clusters.

## flui-types — Color hex/lighten/darken, unit conversions, Corners

Mutation runs from here on are scoped with `-F` to the functions the rewritten tests exercise
(a whole-file run of `color.rs` is ~900 mutants, almost all in blend/SIMD code these tests never
touch); the baseline side is the same regex applied to the Phase 2 baseline lists.

**Color** (`tests/color_operations_tests.rs` → new `tests/color_property_tests.rs`): nine example
tests (hex without `#`/lowercase/mixed case, `to_hex` roundtrip, `lighten_basic`/`_red`,
`darken_basic`/`_blue`, `lighten_darken_effect`) replaced by six properties and one exact
midpoint test. The anchor cases stay explicit: `#RRGGBB`, `#AARRGGBB`, white/black, the four
invalid-format errors, already-white/already-black.

| Scope: `Color::{from_hex,from_argb,to_hex,lighten,darken}` | Before | After |
|---|---:|---:|
| Caught | 38 | 50 |
| Missed | 13 | 1 |
| Unviable | 1 | 1 |

Newly caught: all nine `lighten` arithmetic mutants (the old tests only checked direction, which
a blend that saturates straight to white also satisfies), three `to_hex` nibble mutants. The
generator puts `a == 255` in half the cases so `to_hex`'s opaque branch is exercised; uniform
alpha reached it 1 time in 256, and the first run lost the `to_hex:405` kill because of it.

Remaining missed: `from_hex:142 | → ^` is an equivalent mutant — `(0xFF << 24) | rgb` has
disjoint bits, so OR and XOR agree for every input.

**Unit conversions** (`tests/unit_conversions_tests.rs`): 40 → 32 tests. Three `rstest` tables
(`to_device_pixels`, `DevicePixels::to_pixels`, `Rems::to_pixels`) absorb the literal ladders
and the "real-world" duplicates (retina/mdpi/xxhdpi/125%, rem font sizes); four DevicePixels
roundtrip examples → one property with a rounding + f32-ULP tolerance; two rem roundtrips → one
property. The two proportionality tests stay as examples: rounding makes proportionality false
for arbitrary floats (`px(0.3)`/`px(0.6)` at 1x round to 0 and 1).

**Corners** (`tests/corners_tests.rs`): `all/top/bottom/left/right` → one `rstest` table (count
unchanged, five copies of the same body removed).

| Scope: units + corners constructors/conversions | Before | After |
|---|---:|---:|
| Caught | 16 | 16 |
| Missed | 0 | 0 |

**Not a candidate after review:** `BoxFit` in `painting/image.rs`. Phase 1 proposed folding its
tests into one table; read closely, each test targets a different branch of Flutter's
`applyBoxFit` (cover crops height vs width, fit-width/fit-height contain vs cover branch,
`None` crop vs no crop, `ScaleDown`'s two-step shrink) and carries the parity note for it. A
case table would hide which branch a regression broke.

**`types_it`:** 415 → 406 tests, all passing.
