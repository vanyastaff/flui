[← Polish-pass research](2026-05-24-flui-geometry-polish-pass-research.md) · [← Spike report](2026-05-25-u17-spike-report.md) · [← Tracker](../ROADMAP-TRACKER.md)

# PR 1 Planning — `flui-geometry` Polish Pass

> **Source:** parallel worker subagent output from run `7f766d51` (2026-05-24, ~18 min, $6.89). Worker created 4 separate SDD planning artifacts in worktree which auto-cleaned; this document consolidates the worker's findings for direct use by the apply-phase worker.
>
> **Status:** ready for sdd-apply phase. Advisor (3rd consultation) directed: skip formal SDD ceremony given the scope (~270 reviewer-attention LOC), use this as the constraint sheet, dispatch one worker per atomic commit.

---

## 1. Atomic commit checklist

| # | Commit subject | U-unit(s) | Gross LOC | Reviewer-attention LOC |
|---|---|---|---:|---:|
| 1 | `refactor(geometry): remove cross-type Pixels ops (U2)` | U2 | 70 | 70 |
| 2 | `refactor(geometry): drop Pixels From<scalar> conversions (U1)` | U1 | 33 | 33 |
| 3 | `refactor(geometry): remove dead Float* aliases (U6, SP-4)` | U6 | 15 | 15 |
| 4 | `refactor(geometry): delete ScaledPixels and Scaled* cascade (U6.1, SP-4 zero-consumer)` | U6.1 | ~800 | ~50 (zero-consumer audit) |
| 5 | `fix(geometry): remove Pixels area-as-length operators (U4 + U9 + U10)` | U4+U9+U10 | 30 | 30 |
| 6 | `refactor(geometry): EdgeInsets becomes Edges<Pixels> (U3, 24 sites)` | U3 | 100 | 100 |
| 7 | `refactor(geometry): deprecate raw-scalar device conversions (U5)` | U5 | 32 | 32 |
| 8 | `refactor(geometry): delete broken ScaleFactor::transform_scalar (U7)` | U7 | 35 | 35 |
| 9 (deferred) | `refactor(geometry): explicit lossy integer conversions on Pixels (U11)` | U11 | 50 | 50 |
| 10 | `feat(scripts): install port-check refusal trigger #14 for unit-barrier integrity (U12)` | U12 | 70 | 70 |
| **Total (with U11)** | | | **~1,235** | **~485** |
| **Total (without U11)** | | | **~1,185** | **~435** |

**U6.1 dominates the gross diff** at ~800 LOC, but ~750 of those are mechanical deletion of a self-contained scaffolding type (`ScaledPixels`) with **zero production callers** outside its own crate's tests. Reviewer-attention LOC for this commit is ~50 (the audit anchor: "confirm grep returns zero outside the deletion's own scope").

---

## 2. PR-split decision: Option B (two PRs)

**PR 1a — hygiene + SP-4 (commits 1, 2, 3, 4, 5, 8, 10):**
- ~1,053 gross / ~270 attention LOC
- Under 400-LOC budget

**PR 1b — ripple (commits 6, 7, 9):**
- ~182 gross / ~182 attention LOC
- Under 400-LOC budget

Option B accepted because:
- PR 1a's gross diff dominated by U6.1's zero-attention mechanical deletion
- PR 1b focused on 24-site `EdgeInsets` migration + deprecation cascade — highest-attention work
- Escalation path: if PR 1a review feedback flags U6.1 as too dense, extract to its own PR 1c (Option C in worker's analysis)

---

## 3. First commit: U2 (remove cross-type Pixels ops)

**Why U2 first** (research §IV migration order):
- Cleanest semantic: "cross-type ops should never have existed"
- Smallest fallout: 8 impls deleted + ~20 internal call-site rewrites
- Sets diagnostic tone: every subsequent commit benefits from compiler saying "expected `Pixels`, found `f32`" at call sites U2 closes
- Provides first `compile_fail` doctest of the PR — exercises RED→GREEN→TRIANGULATE→REFACTOR ratchet on non-trivial example

### Pre-U2 sanity check (worker must run BEFORE RED step)

```bash
just ci                        # confirm green baseline
bash scripts/port-check.sh -v  # confirm "all 13 refusal triggers" baseline
git status                     # clean working tree
```

### U2 scope

Remove these impls in `crates/flui-geometry/src/units.rs` (~lines 471–560):

```rust
impl PartialEq<f32> for Pixels { ... }
impl PartialEq<Pixels> for f32 { ... }
impl PartialOrd<f32> for Pixels { ... }
impl PartialOrd<Pixels> for f32 { ... }
impl Add<f32> for Pixels { ... }
impl Add<Pixels> for f32 { ... }
impl Sub<f32> for Pixels { ... }
impl Sub<Pixels> for f32 { ... }
```

**Keep:** `impl Mul<f32> for Pixels` + `Mul<Pixels> for f32` (scaling is dimensionally valid), `Div<f32> for Pixels` (scaling).

### U2 acceptance criteria

- AC-U2-1: 8 impls deleted from `units.rs`.
- AC-U2-2: `compile_fail` doctest on `Pixels::new`:
  ```rust
  /// ```compile_fail
  /// use flui_geometry::px;
  /// let p = px(10.0);
  /// let _ = p == 10.0;  // U2: cross-type PartialEq removed
  /// ```
  ```
- AC-U2-3: Second `compile_fail` doctest:
  ```rust
  /// ```compile_fail
  /// use flui_geometry::px;
  /// let p = px(10.0);
  /// let _ = p + 5.0_f32;  // U2: cross-type Add removed
  /// ```
  ```
- AC-U2-4: Every call site failing to compile is fixed at the boundary with `px(literal)` or `pixels.get()` — NO `.into()` band-aids unless marked `PORT-CHECK-OK-SP3: explicit unit boundary`.
- AC-U2-5: `cargo test -p flui-geometry` green, `cargo build --workspace` green, `cargo clippy --workspace -- -D warnings` green, `bash scripts/port-check.sh -v` green.

### Strict-TDD evidence

Per `openspec/config.yaml strict_tdd: true`:

1. **RED:** add the two `compile_fail` doctests above; verify they FAIL (because the impls still exist).

   Wait — `compile_fail` is RED-by-construction (the test passes when the snippet fails to compile). So RED is: write a NORMAL doctest first using cross-type op, verify it PASSES today.

   ```rust
   /// ```
   /// use flui_geometry::px;
   /// assert_eq!(px(10.0) == 10.0, true);  // RED: currently passes
   /// ```
   ```

   Verify: `cargo test -p flui-geometry --doc` shows this passing.

2. **GREEN:** delete the 8 impls. The normal doctest now fails to compile. Convert it to `compile_fail`:

   ```rust
   /// ```compile_fail
   /// use flui_geometry::px;
   /// assert_eq!(px(10.0) == 10.0, true);  // GREEN: now rejected
   /// ```
   ```

   Verify: `cargo test -p flui-geometry --doc` shows the compile_fail passing (snippet fails to compile, test passes).

3. **TRIANGULATE:** add the second `compile_fail` for `+ 5.0_f32` (different op). Verify it also passes.

4. **REFACTOR:** fix the 20 internal call sites that broke. Use `pixels.get()` or `px(literal)` at the boundary. Add `PORT-CHECK-OK-SP3` markers on any site that legitimately crosses unit boundary (e.g. reading f32 from external API).

5. **Validation:** `just ci` green, port-check green, no `.into()` regression.

---

## 4. Open questions from worker (resolved)

| Q | Worker default | Orchestrator decision |
|---|---|---|
| 1. PR-split shape | Option B (two PRs) | **APPROVED: Option B** |
| 2. Include U11 in PR 1? | Defer to PR 1b or follow-up | **APPROVED: defer to PR 1b** |
| 3. `#[deprecated(since = ...)]` version | `"0.1.0"` per `Cargo.toml:75` | **APPROVED: 0.1.0** |
| 4. Sliver-math `.get()` extraction | Keep `resolve(axis) -> (f32, ...)` extract at accessor | **APPROVED**; add `// PORT-NOTE` marker |
| 5. Tracker update timing | Bundle with commit #10 (port-check #14) | **APPROVED: bundle with #10** |

---

## 5. Discovered surprises (already incorporated into commit plan)

### Surprise #1 — U6.1 ScaledPixels deletion cascade

The actual ripple is **~12 wrapper-cascade methods + 5 macro list entries + ~50 LOC of tests** across the geometry crate:

| File | Method |
|---|---|
| `crates/flui-geometry/src/units.rs:154` | `Pixels::scale(f32) -> ScaledPixels` |
| `crates/flui-geometry/src/units.rs:2206` | `Pixels::to_scaled(f32) -> ScaledPixels` |
| `crates/flui-geometry/src/units.rs:1097` | `DevicePixels::to_scaled_pixels() -> ScaledPixels` |
| `crates/flui-geometry/src/bounds.rs:602` | `Bounds::scale -> Bounds<ScaledPixels>` |
| `crates/flui-geometry/src/point.rs:1115` | `Point::scale -> Point<ScaledPixels>` |
| `crates/flui-geometry/src/size.rs:1012` | `Size::scale -> Size<ScaledPixels>` |
| `crates/flui-geometry/src/edges.rs:366` | `Edges<Pixels>::scale -> Edges<ScaledPixels>` |
| `crates/flui-geometry/src/circle.rs:445` | `Circle::scale_to_scaled` |
| `crates/flui-geometry/src/corners.rs:190` | `Corners::scale -> Corners<ScaledPixels>` |
| `crates/flui-geometry/src/rrect.rs:411` | `RRect::scale` |
| `crates/flui-geometry/src/rsuperellipse.rs:315` | `RSuperellipse::scale` |
| `crates/flui-geometry/src/traits.rs:207,266,339,490,576` | 5 macro lists |
| `crates/flui-types/tests/*` | ~50 LOC of test cases |

All have **zero production callers outside their defining cascade**. Pure SP-4 deletion. All lands in commit #4 (U6.1).

### Surprise #2 — U5 `from_scaled_pixels` collapses into U6.1

`Pixels::from_scaled_pixels(scaled: ScaledPixels, scale_factor: f32) -> Self` (`units.rs:254`) was listed in original U5 scope, but since U6.1 (commit #4) lands before U5 (commit #7), the entire `ScaledPixels` type is already gone. **Function is REMOVED in U6.1, not deprecated in U5.**

---

## 6. Final-pass grep gates (run after PR 1a + PR 1b)

```bash
# U1
rg 'impl From<(f32|f64|i32|u32|usize)> for Pixels' crates/flui-geometry/  # → 0 hits

# U2
rg 'impl (PartialEq|PartialOrd|Add|Sub)<f32> for Pixels' crates/flui-geometry/  # → 0 hits
rg 'impl (PartialEq|Add|Sub)<Pixels> for f32' crates/flui-geometry/  # → 0 hits

# U6 / U6.1
rg 'Float(Point|Vec2|Size|Offset)|Scaled(Pixels|Point|Vec2|Size)|scaled_px' crates/ examples/  # → 0 hits

# U4
rg 'impl Mul<Pixels> for Pixels' crates/flui-geometry/  # → 0 hits
rg 'impl MulAssign<Pixels> for Pixels' crates/flui-geometry/  # → 0 hits
rg 'impl DivAssign<Pixels> for Pixels' crates/flui-geometry/  # → 0 hits

# U7
rg 'transform_scalar' crates/  # → 0 hits

# U12
bash scripts/port-check.sh -v  # → 14 triggers green
```

---

## 7. Dispatch protocol for sdd-apply worker

Each commit gets its own worker spawn. Worker receives:
- This PLANNING.md as the constraint sheet
- The specific commit row from §1 table
- The strict-TDD evidence pattern from §3 (adapted per U-unit)
- Final-pass grep gate from §6 (subset relevant to commit)

Worker MUST:
- Run sanity check from §3 before RED step
- Produce strict-TDD evidence (RED → GREEN → TRIANGULATE → REFACTOR)
- ONE atomic commit with Conventional Commits subject from §1
- NO multi-commit batches per worker spawn
- Return: commit SHA, diff stats, sanity-check + grep-gate output
- Stop if `just ci` fails after the commit — do NOT push

---

[← Polish-pass research](2026-05-24-flui-geometry-polish-pass-research.md) · [← Spike report](2026-05-25-u17-spike-report.md) · [← Tracker](../ROADMAP-TRACKER.md)
