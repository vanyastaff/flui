# Test-suite minimization: report (Phase 4)

One row per completed module/slice. The early slices were measured with `cargo mutants` (nextest
tool) and the later ones with `cargo gamma` (`gamma.toml`, see docs/testing.md's "Mutation
Testing"). Counts are before-vs-after on the source files the slice
touches, not the whole crate.

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

## flui-types — mutation-driven pass over the rest of the crate

From here on every slice follows the same loop: run `cargo mutants` over the source files, read
the surviving mutants, replace example tests with properties, tables or oracle checks that kill
them, delete what the new tests subsume, and rerun. Where a surviving mutant turned out to be a
production bug rather than a missing assertion, the bug was fixed in its own commit with a test
that fails without the fix. Flutter parity claims were checked against the 3.44.0 sources.

### Production bugs found and fixed

| Commit | Defect |
|---|---|
| `62aea6d30` | SIMD `Color::lerp` truncated where the scalar path rounds |
| `080827d0a` | `Color::blend` truncated on un-premultiply: `Src`/`Dst` were not identities (engine CPU image-filter path) |
| `256244ce7` | `Color::blend_over` differed from Flutter's `alphaBlend`; SIMD and scalar disagreed by one |
| `334f2dfc7` | `BoxConstraints::normalize` could return `max < min` |
| `a678d8924` | `CircularNotchedRectangle`'s notch was drawn upside down |
| `262d5417c` | `TextRange::overlaps` said an empty range overlapped |
| `b860c549f` | `BorderRadius::horizontal` swapped the bottom corners |
| `85209157c` | `BorderSide::lerp` darkened toward black and flipped style at 0.5 (Flutter fades) |
| `673338ca5` | `Border::symmetric` put `vertical` on top/bottom, the reverse of Flutter |
| `124e00aee` | `Paint::is_opaque` ignored the shader |
| `59e392a26` | `BoxDecoration::lerp` faded one-sided fields in a V and ended wrong at `t = 1` |
| `e1d415353` | HSL/HSV → `Color` truncated, so the roundtrip was not exact |
| `7a241df41` | `FontWeight::from_css` mapped negative weights to `W900` |
| `4a03e1264` | `Locale` tags dropped the script; Pashto was not RTL |
| `a5f0fdef6` | Shadow/BoxShadow `lerp_list` faded unpaired shadows' colour (Flutter scales geometry) |
| `20f23f914` | `ColorFilter::invert` used 255 offsets on normalized channels: every image turned white |
| `5e15330bd` | `Color::approx_eq` rejected some one-unit differences (f32 subtraction of normalized channels) |

### Mutation results per slice

| Slice (commit) | Scope | Before caught / missed | After caught / missed |
|---|---|---:|---:|
| 16 files: gestures, layout enums, BoxFit, simulations, platform (`b7db1024b`) | those files | 111 / 313 | 446 / 12 |
| Color blend + examples (`07fe2d078`) | `styling/color.rs` | 582 / 298 | 807 / 51 |
| ColorMatrix (`c1a7aef67`) | `painting/effects.rs` | 150 missed, 94 in `hue_rotate` | directional checks replaced by position/composition/W3C matrix |
| Path containment (`add1025b6`) | `painting/path.rs` | 138 missed | containment checked against barycentric/geometric oracles |
| Physics, typography, platform (`aa7bfdee8`) | 8 files, 775 mutants | 459 / 296 | 738 / 17 |
| Corners/Edges moved to flui-geometry (`a46d1391e`, `659cf6fc1`) | `corners.rs`, `corner.rs`, `edges.rs` | 53 / 59 | 102 / 10 |
| Layout and RTL integration files removed (`ca68e2d48`) | `layout/*.rs`, cargo-gamma | 201 killed / 7 survived | 201 / 7, 0 uncovered |
| Geometry integration files removed (`1b7c1d4b0`) | 9 flui-geometry files, cargo-gamma | 1005 killed (both crates' tests) | 1014 killed (flui-geometry alone) |
| Path contour semantics, bounds cache, chord count (`687b543fb`) | `painting/path.rs`, cargo-gamma | 143 survived | 43 survived (all equivalent or unspecified) |
| Crate-wide survivors (`e7894e7f1`) | all of flui-types, 5278 mutants, cargo-gamma | 4230 killed / 376 survived (79.9%) | 4464 / 154 (84.6%) |

Every mutant the baseline caught stayed caught in each slice (compared by position). Remaining
survivors in `color.rs` are equivalent (`|` vs `^` on disjoint bits, `<` vs `<=` at unreachable
boundaries, NEON code not compiled on x86_64) or gaps in `Color::blend`'s composite path and
`clip_color`'s guards.

Files that had no tests at all (`gradient.rs`, `text_spans.rs`, `text_decoration.rs`,
`text_style.rs`, `hsl_hsv.rs`, `paint.rs`, `border.rs`, `border_radius.rs`, `box_border.rs`,
`table_border.rs`, `shader.rs`, `image.rs`) gained tests, so the crate's test count rose while
the mutation score rose much faster. Removing the example files that turned out to be
subsumed then brought it down: `flui-types` went from 656 tests (235 in-source, 421 in
`types_it`) to 423 (345 and 78), and `flui-geometry` from 245 to 272, 901 to 695 together.

### Equivalent mutants (not worth a test)

- `|` → `^` in `from_hex` / `to_argb`: the OR'd bit ranges are disjoint.
- `Simulation::tolerance`'s default body.
- `>` → `>=` in `BoxFit::apply` / `cover_source` at equal aspect ratios: both branches agree.
- `blend_over_factors`' `out > 0` guard; the NEON twins on x86_64.

### Known limitation

nextest does not run doctests, so a mutant killed only by a doctest is reported as missed.

### Unique-killer checks before deleting a test file

A test file is deleted only after a run shows it kills nothing the remaining tests miss. Two
runs over the same mutants, one with the file's tests and one without (`-- --skip <module>::`
for a module of `types_it`, or dropping `--test-package flui-types` for tests of re-exported
`flui-geometry` code), compared mutant by mutant. Anything killed only with the file present is
covered by a new targeted test first, and the pair is rerun. This is how `Alignment`'s `Add`/
`Neg` and `BoxConstraints::deflate` (reached only from `layout_tests.rs`) and 105 `flui-geometry`
mutants (reached only from the eight geometry files) were found and covered before those files
went.

### cargo-gamma pilot

[cargo-gamma](https://crates.io/crates/cargo-gamma) 0.2.1 (Microsoft, MIT) compiles every mutant
into one instrumented build and switches them at run time, instead of rebuilding per mutant.

| Scope | cargo-mutants | cargo-gamma |
|---|---|---|
| `corners.rs`, `corner.rs`, `edges.rs`, flui-geometry tests | 28 min, 256 mutants (112 viable), 102 caught / 10 missed | 86 s, 278 generated (134 viable), 122 killed / 12 survived |
| 9 flui-geometry files, flui-geometry tests | not run (estimated hours) | 133 s, 1961 mutants |

The cargo-mutants time was measured while other runs shared the machine; the gap is still an
order of magnitude. Differences worth knowing:

- gamma found two real test gaps cargo-mutants cannot: `cond.always_true` on
  `Edges::clamp_non_negative`'s top and bottom checks (fixed in `659cf6fc1`).
- gamma does not mutate `const fn` bodies (none in `corner.rs`), where the run-time switch
  cannot go; cargo-mutants does.
- gamma reports `NoCoverage` separately from survivors, which is what made the unique-killer
  checks above cheap: a mutant no remaining test reaches shows up without being run.
- trybuild tests fail its baseline (they compile examples inside the instrumented tree);
  `gamma.toml` skips them, with the other nested-cargo tests, by name.
- It is three weeks old. It replaced cargo-mutants as the workspace tool (`gamma.toml`; the
  `.cargo/mutants.toml` it superseded is gone) after the scopes above agreed.
