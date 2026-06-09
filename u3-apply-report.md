# U3 Apply Report — EdgeInsets becomes Edges<Pixels>

**Task:** PR 1a commit #6 — U3: Migrate `EdgeInsets` type alias from `Edges<f32>` to `Edges<Pixels>`  
**Branch:** `pr1-u2-cross-type-pixels-ops`  
**Commit:** `aa31cb0f`  
**Parent:** `274cd59e` (U4+U9+U10)  
**Date:** 2026-05-25

## Executive Summary

Changed the type alias in `crates/flui-geometry/src/lib.rs` from `pub type EdgeInsets = Edges<f32>` to `pub type EdgeInsets = Edges<Pixels>`. Fixed ~49 call sites across 9 files (production code + integration tests). Added a compile_fail doctest showing f32 rejection. All acceptance criteria met.

Single atomic commit, full validation suite green.

## Commit Details

```
Commit:  aa31cb0f
Parent:  274cd59e (U4+U9+U10 — kill Pixels × Pixels semantic bug)
Branch:  pr1-u2-cross-type-pixels-ops
Subject: refactor(geometry): EdgeInsets becomes Edges<Pixels> (U3)
Date:    2026-05-25
```

### Git Log (HEAD~7)

```
aa31cb0f refactor(geometry): EdgeInsets becomes Edges<Pixels> (U3)
274cd59e fix(geometry): kill Pixels × Pixels semantic bug (U4 + U9 + U10)
6b726ee2 refactor(geometry): delete ScaledPixels and Scaled* cascade (U6.1, SP-4 zero-consumer)
a322e35b refactor(geometry): remove dead Float* aliases (U6, SP-4)
87740bed refactor(geometry): drop Pixels From<scalar> conversions (U1)
35db8a16 refactor(geometry): remove cross-type Pixels ops (U2)
0fdd0f65 docs(geometry): U17 spike outcome + PR 1 planning consolidation
```

### Diff Stats

```
9 files changed, 118 insertions(+), 102 deletions(-)
```

## Changes Made

### Type Alias Change (lib.rs:311)

```rust
// Before
pub type EdgeInsets = Edges<f32>;

// After
pub type EdgeInsets = Edges<Pixels>;
```

### Files Changed (9 total)

| File | Sites | Nature |
|------|-------|--------|
| `crates/flui-geometry/src/lib.rs` | 1 | Type alias + compile_fail doctest |
| `crates/flui-rendering/src/constraints/box_constraints.rs` | — | deflate/inflate fixed |
| `crates/flui-rendering/src/objects/padding.rs` | — | Constructors migrated to Pixels params |
| `crates/flui-rendering/src/objects/sliver_padding.rs` | — | Constructors + resolve() uses `.0` extraction |
| `crates/flui-rendering/tests/u19_bridge.rs` | 1 | Call site |
| `crates/flui-rendering/tests/u20_layout_dirty_root.rs` | 9 | Call sites |
| `crates/flui-rendering/tests/u21_layout_cycle.rs` | 4 | Call sites |
| `crates/flui-rendering/tests/u23_run_layout_wiring.rs` | 5 | Call sites |
| `crates/flui-rendering/tests/u34_compositing_bits_walk.rs` | 6 | Call sites |

## Acceptance Criteria — ALL MET ✓

| ID | Criterion | Status |
|----|-----------|--------|
| **AC-U3-1** | Type alias changed to `Edges<Pixels>` | ✅ PASS |
| **AC-U3-2** | `rg "Edges<f32>" crates/` returns 0 hits | ✅ PASS |
| **AC-U3-3** | All call sites migrated to `px()` wrapper | ✅ PASS |
| **AC-U3-4** | compile_fail doctest added showing f32 rejection | ✅ PASS |
| **AC-U3-5** | Full validation suite green | ✅ PASS |
| **AC-U3-6** | Commit created with specified message | ✅ PASS |

## Verification Results

### Full Validation Suite (all green)

| Check | Result |
|-------|--------|
| `cargo build --workspace` | ✅ green |
| `cargo test -p flui-geometry --doc` | ✅ green (118 passed, 13 compile_fail, 21 ignored) |
| `cargo test -p flui-rendering` | ✅ green (443 unit tests + 25 doc-tests + 4 compile_fail) |
| `cargo test --workspace -- --test-threads=1` | ✅ green |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ green (zero warnings) |
| `cargo fmt --all -- --check` | ✅ green (compliant) |
| `rg "Edges<f32>" crates/` | ✅ 0 hits |

## Surprises

1. **Call-site count was ~49 (not 24 as estimated)** — the extra ~25 were in integration test files (`u19_bridge`, `u20_layout_dirty_root`, `u21_layout_cycle`, `u23_run_layout_wiring`, `u34_compositing_bits_walk`).

2. **`padding.rs` got cleaner after migration** — `horizontal_total()` directly returns `Pixels`, eliminating wrapping that was previously needed.

3. **`sliver_padding.rs` `resolve()` extracts `.0`** to interface with sliver protocol's `f32` fields — a thin boundary where typed units meet the legacy float API.

## Next Recommended Step

Per the ROADMAP-TRACKER N-geom section:

- **U5** — Deprecate `to_device_pixels(f32)` + wrapper cascade
- **U7** — Delete `ScaleFactor::transform_scalar<T>`
- **U12** — Install `port-check.sh` trigger #14 (unit-barrier regression guard)

---

**Report completed:** 2026-05-25  
**Branch:** `pr1-u2-cross-type-pixels-ops`  
**Status:** COMPLETE — Ready for next U-unit or orchestrator handoff.
