# Test-suite minimization: inventory (Phase 1)

Read-only survey of the workspace's ~10.2k tests. Mutation score (`cargo-mutants`), not test
count or line coverage, is the acceptance bar for any consolidation done in later phases — this
document only ranks where consolidation looks promising and cheap to verify.

## Method

- Test counts: `cargo nextest list --workspace --message-format json`, aggregated per crate
  (unit-test binary + its integration-test binary, e.g. `flui-widgets` + `flui-widgets::widgets_it`).
- Runtime: `cargo nextest run --workspace -E 'not group(nested-cargo)'` (excludes the
  `nested-cargo` group per `.config/nextest.toml` — trybuild/template/facade-consumer tests that
  each shell out to a real `cargo`, 21 tests that otherwise dominate wall-clock). Wall clock for
  that run: **88.1s** for 10,221 tests on this 32-core host. Per-crate figures below are summed
  per-test CPU-seconds from the same run (tests execute in parallel, so these don't add up to
  88.1s — they're a relative weight, not a serial-runtime estimate).
- Redundancy clusters: read-only sampling of the six largest crates by three parallel
  Explore-agent passes (flui-widgets+flui-material, flui-objects+flui-rendering,
  flui-view+flui-types), each told to report concrete file:line clusters and explicitly flag
  false positives (tests that look repetitive but cover genuinely distinct behavior).

## Per-crate test counts and weight

| Crate | Tests | Summed CPU-s (parallel run) |
|---|---:|---:|
| flui-widgets | 1503 | 295.4 |
| flui-objects | 996 | 307.1 |
| flui-rendering | 952 | 210.3 |
| flui-view | 811 | 159.4 |
| flui-material | 736 | 155.1 |
| flui-types | 656 | 144.1 |
| flui-app | 609 | 188.9 |
| flui-interaction | 527 | 135.1 |
| flui-scheduler | 524 | 99.7 |
| flui-engine | 368 | 162.6 |
| flui-animation | 308 | 99.6 |
| flui-cli | 244 | 266.0 |
| flui-platform | 271 | 59.8 |
| flui-geometry | 245 | 70.6 |
| flui-foundation | 238 | 52.7 |
| flui-semantics | 216 | 74.2 |
| flui-painting | 184 | 55.3 |
| xtask | 174 | 49.5 |
| flui-tree | 126 | 39.8 |
| flui-layer | 114 | 27.8 |
| flui-cupertino | 95 | 16.8 |
| flui-log | 85 | 27.0 |
| flui-assets | 85 | 23.4 |
| flui-testing | 75 | 22.9 |
| flui (facade) | 43 | 6.4 |
| flui-devtools | 34 | 4.6 |
| flui-hot-reload | 17 | 5.0 |
| flui-macros | 8 | 2.5 |
| flui-localizations | 8 | 2.0 |

`flui-cli`'s CPU-seconds looks high relative to its (non-nested-cargo) test count because several
of its tests still shell out to `cargo` outside the `nested-cargo` group's filter — worth
rechecking during that crate's own baseline pass, not a minimization target by itself.

## Redundancy clusters found (ranked by tests-removable × confidence)

### 1. flui-widgets (1503 tests) — high-confidence, largest removable count

- **Widget field-wiring boilerplate** (`layout/intrinsic_width.rs`, `fractionally_sized_box.rs`,
  `custom_multi_child_layout.rs`, `stack/stack.rs`, ~10-15 widgets × 6-10 tests ≈ 100 tests): the
  same `new_defaults_to_none` / `with_x_overrides_default` / `create_render_object_wires_x_and_y`
  /`update_render_object_applies_changed_x` skeleton repeats per widget.
  → `rstest` `#[case]` per widget. Est. ~100 → ~25-30. Confidence: medium (each widget's
  `RenderUpdateImpact` return value still needs an explicit per-case assertion).
- **`scroll.rs` numeric input-literal variants** (thumb-fraction, clamping-physics, ~6 tests):
  pure functions tested with 3 literal cases each.
  → `rstest` or `proptest` (clamp invariant: output always within `[min, max]`). Est. 6 → 2.
  Confidence: high.
- **Navigator/route/hero lifecycle scenarios** (`crates/flui-widgets/tests/navigator.rs` and siblings, ~223 tests):
  superficially repetitive names, but each verb (push/pop/remove/replace) drives a genuinely
  distinct state transition. **False positive** — flag for shared-helper extraction only
  (boilerplate, not behavior, is duplicated); do not parameterize away the distinct scenarios.

### 2. flui-material (736 tests) — high-confidence, second-largest removable count

- **Per-component "resolve default color for `WidgetState` combo" tests**, repeated identically
  across `checkbox.rs` (~18), `switch.rs` (~20), `chip.rs` (~14), `radio.rs` (~10),
  `data_table.rs` (~9) ≈ 71 tests, plus a parallel ~25 tests re-proving the same
  widget-override > theme > default cascade per component.
  → `rstest` `#[case(state, expected_color)]` per component (71 → ~8-10), cascade tests
  → 5 `rstest` cases or one shared property if resolver signatures unify (25 → 5).
  Confidence: high for the per-component collapse; medium for generalizing the cascade check
  across components (`data_table`'s cascade has extra fallthrough semantics — verify before
  merging).
- **`theme_data.rs` `copy_with` slot tests** (8 theme slots × 2 tests = 16): identical
  `Option::or`/replace semantics per slot.
  → `rstest` `#[case(setter, getter)]` or a macro. Est. 16 → 2. Confidence: high.
- **Large-struct field dumps** (chip painter geometry, data-table cell padding, ~10-12 tests):
  3-6 `assert_eq!` per test on one computed struct.
  → `insta` snapshot. Same test count, stronger regression diagnostics (this doesn't reduce
  count but is the direct answer to the "many `assert_eq!` on one output" pattern named in
  scope). Confidence: medium.

### 3. flui-types (656 tests) — high-confidence, small/pure-function crate

- **`Color::from_hex` family + roundtrip** (`color_operations_tests.rs`, ~15 tests): hex-parsing
  literal variants + an explicit `to_hex`/`from_hex` roundtrip test.
  → `rstest` table (~7 → 1) + one `proptest` roundtrip (`from_hex(to_hex(c)) == c`). Keep
  `_invalid_*` and zero/one/black/white boundary tests explicit. Est. 15 → 4. Confidence: high.
- **`Rect`/`Point`/`Vec2` invariant tests** (`geometric_calculations_tests.rs`, ~8 tests): already
  named as invariants (`_commutative`, `_symmetric`, `_associative`) but pinned to one hard-coded
  input each — textbook `proptest` candidates that would *increase* fault-detection (many random
  inputs instead of one). Est. 8 → 5. Confidence: high.
- **`Pixels`/`DevicePixels` scale-conversion ladder** (`unit_conversions_tests.rs`, ~20 tests):
  one function tested across scale factors (1x/1.5x/2x/retina/fractional/rounding/zero/negative),
  plus a "real-world use case" section that duplicates the same function under cosmetic names.
  → `rstest` table (forward + reverse) + one `proptest` roundtrip-within-tolerance. Est. 20 → 3.
  Confidence: high.
- **`BoxFit` variants** (`image.rs`, ~11 tests): one function, one `#[case]` per fit mode.
  → `rstest` table. Est. 11 → 1 (+1 kept explicit: degenerate-input edge case). Confidence: high.
- `Image::try_from_rgba8`/`from_rgba8` overflow/panic tests: **do not merge** — exactly the
  distinct edge cases (usize overflow, u32 wrap, panic message) the brief protects.

### 4. flui-view (811 tests) — medium confidence, mixed

- **Element lifecycle single-transition tests** (`lifecycle_tests.rs`, ~7 tests): one test per
  transition (mount→active, deactivate→inactive, …), duplicated again under a "characterization"
  naming scheme lower in the same file.
  → `rstest` `#[case(action, expected_state)]`, or better, one sequential-invariant test
  asserting the full `Initial→Active→Inactive→Active→Defunct` chain. Est. 7 → 1-2.
  Confidence: high (keep the existing multi-cycle repetition test separate — it targets ordering
  under repeated cycles, a distinct guarantee).
- **Depth/slot/parent bookkeeping on tree insert**, duplicated across `lifecycle_tests.rs` and
  `element_tree_tests.rs` (~9 tests total checking one field each on the same one-child-insert
  fixture). → one combined multi-field assertion test per file. Est. 9 → 2. Confidence: medium.
- **`sparse_children`/`reconcile_tests.rs` panic-recovery scenarios**: **do not merge** — each
  targets a different panic-injection point and recovery guarantee, exactly the class of test the
  brief wants kept explicit.
- **`build_owner.rs` `mid_drain_*` family** (~13 tests): superficially similar naming, but each
  covers a genuinely different scheduling/ordering scenario. **Do not consolidate** without a much
  closer read — flagged low-confidence.

### 5. flui-objects (996 tests) — mostly boilerplate reduction, not count reduction

AGENTS.md requires a `harness_*` test per concrete `RenderBox`/`RenderSliver`; most of the 996
tests are 1:1 required conformance coverage, not literal-varying duplicates, and should not
shrink in count.

- **Clip-family harness quartet** (`render_object_harness.rs`, 4 tests: rect/rrect/oval/path):
  byte-for-byte identical structure, differing only in the type under test.
  → `rstest` `#[case]`, one type per case (keeps all 4 required per-type harness assertions,
  removes the duplication). Est. 4 tests → 1 fn/4 cases (no net test-count change, this is a
  boilerplate win, not a count reduction). Confidence: high.
- **`Oval::contains`/`ClipGeometry::contains` cluster** (`clip.rs`, 6 tests): center/bbox-corner/
  far-outside/degenerate point checks.
  → `rstest` table, keeping boundary case names explicit. Est. 6 → 1 fn/~5 cases. Confidence:
  medium (must preserve documented boundary semantics as named cases, not silently drop them).
- **`RenderSliverPadding` clamp-math tests** (`sliver_padding.rs`, ~5 tests): literal-only
  variations of the same offset-clamp math.
  → `rstest` for exact boundary numbers + a supplementary `proptest` invariant
  (`paint_offset ⊆ [0, requested_len]`), not a replacement for the numeric cases. Est. 5 → 2 fns
  (~7-8 cases total). Confidence: medium-high.
- The 432-test harness suite as a whole, and `flui-rendering`'s
  `sliver_direction_matrix`/`sliver_hit_direction_matrix` tests (which already use a
  case-matrix-in-one-function shape): **not candidates** — cited as the target shape other
  clusters should move toward.

### flui-rendering (952 tests) — smallest removable count of the six sampled crates

- **`SliverGeometry::validation_error` cluster** (`sliver_geometry.rs`, 3 tests/5 sub-cases):
  each builds `SliverGeometry { field: bad_value, ..SliverGeometry::ZERO }` and checks one error
  string. → `rstest` `#[case(field_override, expected_error)]`, keeping every field-override
  explicit (AGENTS.md flags `..SliverGeometry::ZERO` masking missing fields as a real bug
  pattern — a snapshot of the whole struct would reintroduce exactly that risk). Est. 3→1 fn/
  5-6 cases. Confidence: high.
- `BoxConstraints` unit-test block: internally checks several derived values per test; flagged
  **low confidence** for snapshotting — constraint math is exactly the kind of "wrong but
  plausible number" a snapshot can silently accept. Recommend leaving as explicit asserts.
- `retained_boundary_layers.rs`, `semantics_assembly.rs`, `hit_test_pipeline.rs`: **not
  candidates** — each test targets a structurally distinct scenario. `proptest` is already a
  dev-dependency of this crate but currently unused; the clamp-math functions above are the
  strongest fit for it.

## Top 5 target modules for Phase 2 (ranked by tests-removable × confidence)

1. **flui-material** — highest-confidence, largest single removable count (~96 of 736 tests
   collapse into ~15-20), all pure resolver functions.
2. **flui-types** — highest confidence overall (pure functions, several already-named
   invariants), ~54 of 656 tests collapse into ~13, plus genuine coverage gains from
   proptest roundtrips.
3. **flui-widgets** — largest absolute test count and largest raw removable count (~100 widget
   field-wiring tests), but medium confidence (per-widget return-value assertions must survive
   parameterization) — do the smaller, high-confidence `scroll.rs` cluster first as a warm-up.
4. **flui-view** — medium confidence, smaller removable count (~16 of 811), but low risk
   (lifecycle transition and tree-bookkeeping tests are mechanical).
5. **flui-objects** — lowest removable *count* (harness coverage is mandated 1:1), but the
   clip-family and clamp-math clusters are good boilerplate-reduction pilots before touching the
   larger, riskier crates.

`flui-rendering` is deliberately excluded from the top 5: sampling found only one clean cluster
(~5-6 tests) and one place (`BoxConstraints`) where the "obvious" strategy (snapshotting) would
actively risk hiding regressions — not a good ROI target for this phase.

## What Phase 0 already set up

- `.cargo/mutants.toml`: `cargo-mutants` configured to use `nextest` with the workspace's own
  `default` profile, and a 60s minimum per-mutant test timeout. Verified working:
  `cargo mutants -p flui-types -f crates/flui-types/src/painting/image.rs --list` enumerates
  mutants correctly.
- `proptest` and `rstest` added to `[workspace.dependencies]` in the root `Cargo.toml`, alongside
  the existing `insta = "1"` workspace dependency. Not yet added to any crate's own
  `[dev-dependencies]` — that happens per-module in Phase 3, at the point of use, per
  `cargo xtask deps`'s "nothing inherits" check (an unused workspace dependency is only warned
  about, not an error, but this avoids the warning until a crate actually needs it).
- `cargo-llvm-cov` and `cargo-insta` installed (`cargo-nextest` and `cargo-mutants` were already
  present on this host).
