---
date: 2026-05-20
type: refactor
status: completed
origin: docs/brainstorms/clipcontext-consolidation-requirements.md
depth: standard
target_crates: flui-painting, flui-rendering, flui-engine
flutter_reference: .flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart
predecessor_pr: 81 (flui-rendering Phase 1 zombie cleanup, U5 deferred ClipContext consolidation here)
---

# refactor: ClipContext consolidation + RSuperellipse parity

## Summary

Collapse the two divergent `ClipContext` traits into one Flutter-faithful trait in `flui-painting`, migrate `CanvasContext` onto it, and land the full `RSuperellipse` clip stack (Canvas op + DrawCommand variant + engine wgpu handler + 4th trait method) so the new trait reaches Flutter 4-method parity. Eight atomic commits, scoped so workspace build stays green after each.

## Problem Frame

The Mythos audit ([docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md)) originally listed `ClipContext` deletion in Step 1 "Safe deletions". Round-1 `ce-doc-review` on the cleanup brainstorm uncovered three problems the audit's grep missed: the two `ClipContext` traits have incompatible signatures (different accessor name, different painter callback shape, different `Clip::None` semantics); the `flui-painting/ARCHITECTURE.md` doc-lie at lines 37 and 94 claims `CanvasContext` implements `flui-painting::ClipContext` when in fact it implements `flui-rendering::ClipContext`; and the audit's "missing `clip_superellipse_and_paint` method for Flutter 4-method parity" note has no underlying `Canvas::clip_rsuperellipse_ext` to back it. PR #81 landed Phase 1 cleanup minus the ClipContext work and explicitly deferred it to this plan (see [the audit's Step 1 status annotation block](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md)).

Workspace-wide grep at brainstorm time confirmed zero production callers of `clip_*_and_paint` methods on either trait — only test impls exist today. The migration cost is bounded; the cost of leaving the divergence is that the first widget needing clip-and-paint hits ambiguity and a doc-lie at the same time. Per the user's "how Flutter does it" directive, the single canonical trait shape matches Flutter's `painting/clip.dart` (`canvas` accessor, `FnOnce(&mut Self)` painter callback for `VoidCallback` parity, no `Clip::None` short-circuit, 4 user methods including `clipRSuperellipseAndPaint`).

## Requirements

Carries forward from origin (see [origin: docs/brainstorms/clipcontext-consolidation-requirements.md](../brainstorms/clipcontext-consolidation-requirements.md)). R1-R10 are functional consolidation requirements; R11-R15 are verification gates that apply across all units. AE1 from the origin is exercised by U4's test scenarios.

**Functional requirements (origin):**

- **R1**: `flui-painting::ClipContext` is the single canonical trait. `flui-rendering::context::clip::ClipContext` and its containing file get deleted; module removed from `crates/flui-rendering/src/context/mod.rs`.
- **R2**: Trait accessor renamed `canvas_mut(&mut self) -> &mut Canvas` → `canvas(&mut self) -> &mut Canvas` (Flutter naming parity).
- **R3**: Painter callback stays `F: FnOnce(&mut Self)` (Flutter `VoidCallback` closure-captures-receiver semantics).
- **R4**: `Clip::None` short-circuit in `clip_and_paint_impl` is removed — save/restore always run, matching Flutter ([.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart:21-37](../../.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart)).
- **R5**: New method `clip_rsuperellipse_and_paint(rse: RSuperellipse, clip_behavior: Clip, bounds: Rect<Pixels>, painter: F)` on the trait, completing 4-method Flutter parity.
- **R6**: All four user methods take `Rect<Pixels>` for `bounds` (and `rect` in `clip_rect_and_paint`). FLUI typed-unit retained — Flutter's bare `Rect` has no typed equivalent in Dart.
- **R7**: `impl super::ClipContext for CanvasContext` at [crates/flui-rendering/src/context/canvas.rs:695](../../crates/flui-rendering/src/context/canvas.rs) replaced with `impl flui_painting::ClipContext for CanvasContext { fn canvas(&mut self) -> &mut Canvas { self.canvas() } }`. The inner `self.canvas()` resolves to the inherent method (Rust prefers inherent over trait in method resolution) — verified at plan time, no rename needed (Q3 resolved).
- **R8**: Test impls in deleted `flui-rendering` clip.rs go away with the file; extend `flui-painting::ClipContext` tests to cover `clip_rsuperellipse_and_paint`.
- **R9**: `flui-painting/ARCHITECTURE.md` lines 37 and 94 become truth statements after R7 — verify post-edit, only re-prose if the impl count or surface changes.
- **R10**: Module-level docs on `flui-painting::ClipContext` (mentions a nonexistent `PaintingContext` implementer) updated to name `CanvasContext` as the actual production type.

**Verification gates (origin, apply across U1-U8):**

- **R11**: `cargo build --workspace` passes after each commit.
- **R12**: `cargo test --workspace --lib --tests` passes; the four trait test cases exercise the migrated shape; a new test covers `clip_rsuperellipse_and_paint`.
- **R13**: `cargo clippy --workspace --all-targets -- -D warnings` passes after the final commit.
- **R14**: `bash scripts/port-check.sh -v` reports 7/7 institutional refusal triggers ok after each commit.
- **R15**: Post-cleanup workspace-wide grep returns zero hits for `flui_rendering::context::clip::ClipContext`, `flui_rendering::ClipContext`, or `use flui_rendering::.*ClipContext` outside deletion-diff.

## Output Structure

No new directory hierarchy created. All edits modify existing files. Per-unit `**Files:**` sections are authoritative.

## Key Technical Decisions

- **Eight atomic commits, not four**, because the user's "Land full stack" decision pulled the `RSuperellipse` Canvas + DrawCommand + engine chain into this plan rather than deferring to a follow-up. The brainstorm's "cost-prudence" R5 framing assumed the underlying op existed; plan-time verification ([crates/flui-painting/src/canvas/clipping.rs](../../crates/flui-painting/src/canvas/clipping.rs)) showed it does not. The chain forces sequential commits because each layer's compile gate depends on the previous: DrawCommand variant before Canvas method before engine handler before trait method.
- **Commit-scope tag = `refactor(painting):` / `refactor(rendering):` / `refactor(engine):`** per recent precedent (`refactor(rendering)` used by U1-U3 of PR #81; same shape applies here). U6 + U7 use `docs(painting):` / `docs(plans):` respectively.
- **Engine match arms must update in the same commit as the DrawCommand variant addition (U1)** so `cargo build --workspace` does not break between commits. The engine wgpu handler initially uses a minimal stub (delegate to existing rounded-rect SDF or `panic!("not yet implemented")`); the real superellipse SDF implementation lands in U3 as a focused unit. This preserves the atomic-commit-per-finding shape while keeping every intermediate commit buildable. See `references/synthesis-summary.md`-equivalent rationale in PR #81's U4.5 (clippy baseline catch-up): scope expansion in service of verification-gate honesty.
- **`CanvasContext::canvas()` inherent method is NOT renamed.** Plan-time verification of Rust method resolution rules shows inherent methods always shadow trait methods of the same name with the same signature. The body `self.canvas()` inside `impl flui_painting::ClipContext for CanvasContext` resolves to the inherent (Q3 resolved). Avoids a churn-without-benefit rename across the inherent method's call sites.
- **`flui-painting::ClipContext` module rustdoc renames `PaintingContext` → `CanvasContext`** (Q2 default). The doc currently mentions `PaintingContext` as the intended implementer; `CanvasContext` is the actual production type. Keep the prose specific rather than generic — easier for readers landing in the file cold.
- **Clippy `wrong_self_convention` does NOT fire on the `canvas` rename** (Q2 / Q3 resolved). The lint only fires on `&self -> &mut T` or similar mismatches; `&mut self -> &mut T` without `_mut` suffix is lint-clean (verified by Rust 1.95 clippy reference).
- **Commit message trailer required:** `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>` per CLAUDE.md directive and observed PR #81 precedent.
- **Line-number policy: symbol-based discovery.** Implementer uses `rg` on symbol names at edit time; cited positions are illustrative only.

## Implementation Units

### U1. Add `DrawCommand::ClipRSuperellipse` variant + command_ops coverage

**Goal:** Add the new clip variant to the `DrawCommand` enum and update every per-variant match site so `cargo build --workspace` stays green. Engine wgpu handler gets a minimal stub that compiles; real SDF impl lands in U3.

**Requirements:** R5 (underlying chain).

**Dependencies:** None — leaf change.

**Files:**
- [crates/flui-painting/src/display_list/command.rs](../../crates/flui-painting/src/display_list/command.rs) — add `DrawCommand::ClipRSuperellipse { rsuperellipse, clip_op, clip_behavior, transform }` variant.
- [crates/flui-painting/src/display_list/command_ops.rs](../../crates/flui-painting/src/display_list/command_ops.rs) — add per-variant arms for `with_opacity`, `bounds`, `transform`, `paint`, `kind`, `is_clip`, `apply_transform` (mirror shape of `DrawCommand::ClipRRect` arms; ~7-8 short additions).
- [crates/flui-painting/src/display_list/stats.rs](../../crates/flui-painting/src/display_list/stats.rs) — if the stats struct tracks clip variants, extend it; otherwise no change.
- [crates/flui-engine/src/wgpu/painter.rs](../../crates/flui-engine/src/wgpu/painter.rs) (and any other engine match sites on `DrawCommand`) — add stub arm: delegate to existing rounded-rect SDF clip code path for now, or `tracing::warn!("ClipRSuperellipse stub; real SDF lands in U3"); /* fallthrough to no-op */`. Stub MUST keep workspace compiling.

**Approach:**
- `rg 'match.*cmd.*\{|match cmd \{' crates/` enumerates every site that pattern-matches on `DrawCommand`. ARCHITECTURE.md notes ~1,200 LOC of per-variant arms in `command_ops.rs`; the engine has additional sites.
- For each site, add the new variant arm using the closest existing-variant analog (typically `ClipRRect`).
- Use a stub for the wgpu painter — falling through to `clip_rrect` logic against the superellipse's bounding rect is a reasonable visual approximation that doesn't break tests; the real SDF lands in U3.

**Patterns to follow:**
- Existing `DrawCommand::ClipRRect` variant arms in `command_ops.rs` — copy shape exactly.
- Existing engine handling of `DrawCommand::ClipRRect` in `painter.rs` — `rg 'ClipRRect' crates/flui-engine/src/wgpu/` enumerates the relevant sites.

**Test scenarios:**
- **Happy path:** Construct a `DrawCommand::ClipRSuperellipse`, dispatch through `command_ops` query methods (`bounds`, `kind`, `is_clip`); assertions match the rrect-analog behavior.
- **Edge case:** `DrawCommand::ClipRSuperellipse` with `Clip::None` clip_behavior — exercises the variant-construction path even when no actual clipping happens.
- **Integration:** Add a smoke test in `display_list_tests.rs` (or equivalent) that records a Canvas op-chain hitting the new variant and verifies the DisplayList contains the expected variant count.
- The four preserved `ClipContext` tests in `flui-painting::clip_context::tests` continue to compile (no method-signature break yet — that lands in U4).

**Verification:**
- `cargo check -p flui-painting` clean.
- `cargo build --workspace` clean (stubs in engine match arms hold this gate).
- `bash scripts/port-check.sh -v` reports 7/7 ok.

---

### U2. Add `Canvas::clip_rsuperellipse` + `clip_rsuperellipse_ext` + `ClipShape::RSuperellipse`

**Goal:** Extend the `Canvas` painting-layer surface with the rounded-superellipse clip op, mirroring the rrect/rect/path shape exactly.

**Requirements:** R5 (Canvas underlying ops), R8 (test infrastructure for trait method).

**Dependencies:** U1 (DrawCommand variant must exist).

**Files:**
- [crates/flui-painting/src/canvas/state.rs](../../crates/flui-painting/src/canvas/state.rs) — extend `ClipShape` enum with `RSuperellipse(RSuperellipse)` variant.
- [crates/flui-painting/src/canvas/clipping.rs](../../crates/flui-painting/src/canvas/clipping.rs) — add `pub fn clip_rsuperellipse(&mut self, rse: RSuperellipse)` and `pub fn clip_rsuperellipse_ext(&mut self, rse: RSuperellipse, clip_op: ClipOp, clip_behavior: Clip)` (mirror `clip_rrect` and `clip_rrect_ext` lines 39-47 and 78-86).
- [crates/flui-painting/src/canvas/clipping.rs](../../crates/flui-painting/src/canvas/clipping.rs) — extend `local_clip_bounds` `match clip {}` to handle `ClipShape::RSuperellipse(rse) => Some(rse.bounding_rect())`.

**Approach:**
- `RSuperellipse` already exists at [crates/flui-types/src/geometry/rsuperellipse.rs](../../crates/flui-types/src/geometry/rsuperellipse.rs); import it where `RRect` is imported.
- Both new Canvas methods push `ClipShape::RSuperellipse` onto `self.clip_stack` and emit `DrawCommand::ClipRSuperellipse` onto the display list — exactly parallel to the rrect path.
- `local_clip_bounds` needs `RSuperellipse::bounding_rect()`; verify that method exists on `RSuperellipse` at edit time (if not, add it as part of this unit per in-commit edge-case handling).

**Patterns to follow:**
- [crates/flui-painting/src/canvas/clipping.rs:39-47](../../crates/flui-painting/src/canvas/clipping.rs) — `Canvas::clip_rrect`.
- [crates/flui-painting/src/canvas/clipping.rs:78-86](../../crates/flui-painting/src/canvas/clipping.rs) — `Canvas::clip_rrect_ext`.
- [crates/flui-painting/src/canvas/clipping.rs:113-114](../../crates/flui-painting/src/canvas/clipping.rs) — `local_clip_bounds` rrect arm.

**Test scenarios:**
- **Happy path:** `Canvas::new()`, call `canvas.clip_rsuperellipse(rse)`, assert `display_list.len() == 1` and the recorded command is `DrawCommand::ClipRSuperellipse` with matching fields.
- **Happy path (ext):** Same but for `clip_rsuperellipse_ext` with `ClipOp::Difference` — verify the clip_op flows through to the recorded command.
- **Edge case (local_clip_bounds):** After `clip_rsuperellipse`, `canvas.local_clip_bounds()` returns `Some(rse.bounding_rect())`. After `save() / clip_rsuperellipse / restore()`, returns to whatever was on the stack before.
- **Integration:** Mixed clip stack — `clip_rect / clip_rsuperellipse / clip_path` interleaved with `save/restore`, verify the stack depth and `local_clip_bounds` reflect the most recent clip.

**Verification:**
- `cargo check -p flui-painting` clean.
- `cargo test -p flui-painting --lib` passes; new tests cover the four scenarios above.
- `bash scripts/port-check.sh -v` reports 7/7 ok.

---

### U3. Engine: implement real wgpu SDF handler for `DrawCommand::ClipRSuperellipse`

**Goal:** Replace U1's stub with a real rounded-superellipse SDF clip implementation, reusing the existing pattern from `clip_rrect` and consulting [crates/flui-layer/src/layer/clip_superellipse.rs](../../crates/flui-layer/src/layer/clip_superellipse.rs) for any SDF math already in the workspace.

**Requirements:** R5 (engine consumption).

**Dependencies:** U1 (variant exists), U2 (Canvas op produces the variant).

**Files:**
- [crates/flui-engine/src/wgpu/painter.rs](../../crates/flui-engine/src/wgpu/painter.rs) — replace the U1 stub arm in the `DrawCommand` match with the real SDF clip implementation.
- [crates/flui-engine/src/wgpu/backend.rs](../../crates/flui-engine/src/wgpu/backend.rs) — if `Backend` also dispatches on `DrawCommand`, update there too.
- Possibly: [crates/flui-engine/src/wgpu/layer_render.rs](../../crates/flui-engine/src/wgpu/layer_render.rs) (already mentions `SUPERELLIPSE_CACHE` per audit) — verify whether this unit's wgpu shader can reuse cached SDF coefficients.

**Approach:**
- Re-read [crates/flui-layer/src/layer/clip_superellipse.rs](../../crates/flui-layer/src/layer/clip_superellipse.rs) and any existing SDF helpers (`layer_render.rs::SUPERELLIPSE_CACHE`) to understand the established math.
- Mirror the wgpu shader-injection / scissor-clip pattern from existing `clip_rrect` handling — `rg 'ClipRRect' crates/flui-engine/src/wgpu/` enumerates the call sites.
- If the SDF math is non-trivial and warrants a shared helper, factor it out under [crates/flui-engine/src/wgpu/](../../crates/flui-engine/src/wgpu/); otherwise inline in the painter handler.

**Patterns to follow:**
- Engine's existing `ClipRRect` handler (line numbers float — locate via `rg`).
- `flui-layer::ClipSuperellipseLayer` for the canonical math.

**Test scenarios:**
- **Happy path:** Headless render test that emits a `clip_rsuperellipse` clip followed by a `draw_rect` and verifies the rendered pixels respect the superellipse boundary (corners clipped, center visible).
- **Edge case:** Superellipse with `rx == ry` (rounded square) — should match the equivalent `clip_rrect` output within tolerance.
- **Edge case:** Superellipse with very small radii (near 0) — should approach a `clip_rect` of the bounding rect.
- **Integration:** `clip_rsuperellipse` inside a `save_layer` / `restore` block — verify the clip applies only within the save-layer scope.

**Verification:**
- `cargo check -p flui-engine` clean.
- `cargo test -p flui-engine --lib` passes.
- `cargo build --workspace` clean.
- `bash scripts/port-check.sh -v` reports 7/7 ok.

---

### U4. `flui-painting::ClipContext` Flutter-alignment edits

**Goal:** Apply the three signature/behavior edits that bring the painting-layer trait into Flutter parity: accessor rename, `Clip::None` short-circuit removal, and addition of the 4th method.

**Requirements:** R2, R3 (no change — already on `FnOnce(&mut Self)`), R4, R5 (trait surface), R6 (no change — already `Rect<Pixels>`).

**Dependencies:** U2 (the new trait method's default body calls `self.canvas().clip_rsuperellipse_ext(...)` which requires the underlying op).

**Files:**
- [crates/flui-painting/src/clip_context.rs](../../crates/flui-painting/src/clip_context.rs).

**Approach:**

Three coordinated edits inside the existing trait:

1. **R2 — accessor rename.** Change the required-method signature from `fn canvas_mut(&mut self) -> &mut Canvas` to `fn canvas(&mut self) -> &mut Canvas`. Update every internal call (`self.canvas_mut()` → `self.canvas()`) within the default methods of the trait. The test impl `TestClipContext::canvas_mut` also renames.

2. **R4 — drop `Clip::None` short-circuit.** Remove the early-return at `clip_and_paint_impl` (currently lines 250-253) that skips save/restore when `clip_behavior == Clip::None`. Replace with the Flutter-faithful flow: always `save()`, switch on `clip_behavior` (Clip::None → no canvas-clip call; HardEdge/AntiAlias → `canvas_clip_call(do_anti_alias)`; AntiAliasWithSaveLayer → `canvas_clip_call(true)` then `save_layer`), `painter(self)`, then restore-layer-if-AntiAliasWithSaveLayer + `restore()`. Mirror the [.flutter/.../painting/clip.dart:21-37](../../.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart) exactly.

3. **R5 — add `clip_rsuperellipse_and_paint`.** Insert the 4th method between `clip_rrect_and_paint` and `clip_path_and_paint` (alphabetical order matches Flutter), with the same `FnOnce(&mut Self)` painter shape and same `Rect<Pixels>` bounds parameter. Body delegates to `clip_and_paint_impl` with a closure that calls `canvas.clip_rsuperellipse_ext(rse, ClipOp::Intersect, clip_behavior_inner)` — exact analog to the rrect-method body at lines 147-172.

**Patterns to follow:**
- Flutter source `.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart` lines 11-97 — full trait reference.
- Existing `clip_rrect_and_paint` in the same file — exact shape to mirror for the new method.

**Test scenarios:**
- **Covers AE1 (R4):** `TestClipContext` records canvas `save_count` before, inside, and after a `clip_rect_and_paint(rect, Clip::None, bounds, painter)` call. Assert: inside-painter count is exactly outer-before-count + 1, and after-call count returns to outer-before-count. Locks down the "no short-circuit, save/restore always run" semantics.
- **Happy path (R5):** `TestClipContext::clip_rsuperellipse_and_paint(rse, Clip::AntiAlias, bounds, painter)` — painter callback invoked exactly once; canvas display list contains the expected `ClipRSuperellipse` command.
- **R2 rename:** Test impls update to `fn canvas(&mut self) -> &mut Canvas`; existing `clip_rect_and_paint` / `clip_rrect_and_paint` / `clip_path_and_paint` tests continue to pass with the renamed accessor.
- **All four behaviors with all four `Clip` enum values:** matrix coverage — 4 methods × 4 clip behaviors = 16 cases; pick the corners (each method with `Clip::None` + each method with `Clip::AntiAliasWithSaveLayer`) for explicit tests, leave the rest implicit via the shared `clip_and_paint_impl` path.

**Verification:**
- `cargo check -p flui-painting` clean.
- `cargo test -p flui-painting --lib` passes; AE1 + the new R5 test pass; the four pre-existing `ClipContext` tests continue to pass with the renamed accessor.
- `cargo build --workspace` clean (the rename ripple is contained to the trait file plus its test impls; no production caller exists today).
- `bash scripts/port-check.sh -v` reports 7/7 ok.

---

### U5. Migrate `CanvasContext` impl + delete `flui-rendering::context::clip`

**Goal:** Switch `CanvasContext` from implementing `flui-rendering::ClipContext` to implementing `flui-painting::ClipContext`, then delete the old trait file and its module declaration. Combined into one commit because the workspace compile gate requires both edits together.

**Requirements:** R1, R7, R8 (test impls go away with the deleted file).

**Dependencies:** U4 (the new trait surface must exist before CanvasContext can implement it).

**Files:**
- [crates/flui-rendering/src/context/canvas.rs](../../crates/flui-rendering/src/context/canvas.rs) — replace `impl super::ClipContext for CanvasContext { fn canvas(&mut self) -> &mut Canvas { self.canvas() } }` (around line 695) with `impl flui_painting::ClipContext for CanvasContext { fn canvas(&mut self) -> &mut Canvas { self.canvas() } }`. The body is unchanged — only the trait path swaps. Inherent `self.canvas()` resolves correctly (Rust prefers inherent over trait at the same name + signature; Q3 verified).
- [crates/flui-rendering/src/context/clip.rs](../../crates/flui-rendering/src/context/clip.rs) — delete entire file (~330 lines).
- [crates/flui-rendering/src/context/mod.rs](../../crates/flui-rendering/src/context/mod.rs) — remove `mod clip;` declaration and any `pub use clip::ClipContext` re-export.
- [crates/flui-rendering/src/lib.rs](../../crates/flui-rendering/src/lib.rs) — remove any crate-root `pub use context::clip::ClipContext` re-export if present.

**Approach:**
- `rg 'use.*clip::ClipContext|use.*context::ClipContext|use flui_rendering::.*ClipContext' crates/` to enumerate every external reference; expect zero hits per brainstorm verification.
- Replace the `impl super::ClipContext` block first; verify `cargo check -p flui-rendering` clean.
- Delete the file + mod declaration second; re-run check.
- Both edits must land in the same commit per the workspace-compile invariant. (If the impl block referenced types from the deleted file — it does not, the only required method has signature in terms of `Canvas` from `flui_painting` — the order in the diff doesn't matter for compile.)

**In-commit edge-case handling (per user-memory directive "no defer-with-excuse"):** if any of the above edits surface compilation breakage in code outside the stated files (e.g., an unanticipated in-crate import of the deleted trait), fix the breakage in this commit. Do not defer to a follow-up.

**Patterns to follow:**
- PR #81 U2 (commit `326358b6`, deletion of `IntrinsicProtocol` + `BaselineProtocol`) — same shape: delete trait + sweep re-exports + verify zero grep hits post-edit.

**Test scenarios:**
- **Test expectation: none on new behaviour** — this unit is a trait-swap on the impl plus a file deletion. The existing test coverage for `CanvasContext` (in [crates/flui-rendering/src/context/canvas.rs](../../crates/flui-rendering/src/context/canvas.rs) tests module) continues to pass; the deleted clip.rs test module's coverage is supplanted by U4's tests in [crates/flui-painting/src/clip_context.rs](../../crates/flui-painting/src/clip_context.rs).

**Verification:**
- `cargo check -p flui-rendering` clean.
- `cargo build --workspace` clean.
- `cargo test --workspace --lib --tests` passes.
- `cargo build -p flui-hot-reload --features app-plugin --all-targets` clean (ABI-shape regression check, same gate as PR #81's U3).
- `bash scripts/port-check.sh -v` reports 7/7 ok.
- Post-edit grep audit (R15):
  - `rg 'flui_rendering::context::clip::ClipContext' crates/` returns zero.
  - `rg 'flui_rendering::.*ClipContext' crates/` returns zero (excluding the deletion diff).
  - `rg 'use flui_rendering::.*ClipContext' crates/` returns zero.

---

### U6. ARCHITECTURE.md + module-doc cleanup

**Goal:** Verify the `flui-painting/ARCHITECTURE.md` lines 37 + 94 claims about `CanvasContext` are now true (R9), and update the inline module rustdoc on `flui-painting::ClipContext` to name `CanvasContext` explicitly instead of the nonexistent `PaintingContext` (R10).

**Requirements:** R9, R10.

**Dependencies:** U5 (CanvasContext must already implement the painting trait).

**Files:**
- [crates/flui-painting/ARCHITECTURE.md](../../crates/flui-painting/ARCHITECTURE.md) — re-read lines 37 and 94 post-U5. Edit only if the prose count or surface no longer matches (e.g., if the trait actually has 4 default methods now, not 3, update the count to 4).
- [crates/flui-painting/src/clip_context.rs](../../crates/flui-painting/src/clip_context.rs) — update module-level rustdoc at lines 11-19 (currently mentions `PaintingContext` in the architecture diagram and `TestRecordingPaintingContext` as a sibling impl). Replace `PaintingContext` references with `CanvasContext` and the doc-flutter-equivalent example with a real import path. Drop the `TestRecordingPaintingContext` mention — no such type exists in the workspace.

**Approach:**
- For ARCHITECTURE.md: read lines 37 + 94 post-U5; if they say "3 default methods" while the trait now has 4, update the count. If they say "1 prod impl: CanvasContext in flui-rendering" — that's already accurate post-U5, leave alone.
- For module rustdoc: keep the Flutter Reference block (lines 4-7) — that's the cross-reference to the source. Update the Architecture block (lines 13-18) to reflect actual workspace state.

**Patterns to follow:**
- PR #81 U4 doc reconciliation pattern — verify ground-truth state, only edit where doc claims diverge.

**Test scenarios:**
- **Test expectation: none** — pure documentation update, no behavioral change. The `cargo doc` build must remain clean.

**Verification:**
- `cargo doc -p flui-painting --no-deps` clean.
- `cargo build --workspace` clean (no code impact).
- `bash scripts/port-check.sh -v` reports 7/7 ok.

---

### U7. Final clippy + post-cleanup grep audit

**Goal:** Run the full clippy + grep verification batch and surface any remaining issues. Most issues should already be addressed by U1-U6's per-unit verification; this unit is the consolidated final gate.

**Requirements:** R13, R15.

**Dependencies:** U1, U2, U3, U4, U5, U6.

**Files:**
- No source files modified by default — this unit is a verification pass. If clippy surfaces new errors (e.g., a `missing_docs` on the new `clip_rsuperellipse_and_paint` trait method, or a stale `#[allow]` whose target is removed), fix in-commit per memory directive.

**Approach:**
- `cargo clippy --workspace --all-targets -- -D warnings` → zero errors.
- Workspace-wide grep audit per R15:
  - `rg 'flui_rendering::context::clip::ClipContext' crates/` → 0
  - `rg 'flui_rendering::ClipContext' crates/` → 0 (deletion diff excluded)
  - `rg 'use flui_rendering::.*ClipContext' crates/` → 0
  - `rg 'PaintingContext' crates/flui-painting/src/clip_context.rs` → 0 (module-rustdoc fix verified)
  - `rg 'canvas_mut\s*\(\)' crates/flui-painting/src/clip_context.rs` → 0 (rename verified)
- Any in-place fixups land in this commit; if none needed, commit message is `chore(workspace): final clippy + grep audit for ClipContext consolidation`.

**Test scenarios:**
- **Test expectation: none** — verification-only unit unless fixups required.

**Verification:**
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- All five grep audits above return zero non-deletion-diff hits.

---

### U8. Mythos audit Step 1 annotation update

**Goal:** Update the Mythos audit doc to record that the ClipContext consolidation deferral (originally noted in PR #81's U5 annotation) is now complete, with commit-hash references back to U1-U7.

**Requirements:** Closes the audit-doc loop opened by PR #81's U5.

**Dependencies:** U1, U2, U3, U4, U5, U6, U7 (need their commit hashes).

**Files:**
- [docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md) — extend the Step 1 status block (the one added by PR #81's U5) with a follow-up note: ClipContext consolidation completed in this PR, with commit-hash references; the audit's "missing `clip_superellipse_and_paint` method" item is also resolved as part of this batch.

**Approach:**
- Read the existing Step 1 status block. Append a short paragraph or bullet noting the ClipContext consolidation completion, with the U1-U7 commit hashes filled in at edit time. Preserve all existing prose.
- Optionally update the Step 1 main list item that mentioned ClipContext deletion — annotate "completed via PR #<this PR>".

**Patterns to follow:**
- PR #81 U5 (commit `bb98cb86`) — exact same annotation pattern.

**Test scenarios:**
- **Test expectation: none** — audit-doc update only.

**Verification:**
- `cargo build --workspace` clean (no code impact).
- The Step 1 status block clearly indicates the ClipContext loop is now closed with commit-hash references.

## Verification

The implementer runs the following after each commit. All must pass:

- `cargo check -p <touched crate>` (clean per unit)
- `cargo build --workspace` (clean)
- `cargo test -p <touched crate> --lib --tests` (passing count noted in commit body)
- `bash scripts/port-check.sh -v` (7 triggers ok)

After U3 specifically:
- `cargo build -p flui-engine --all-targets` (ensure wgpu shader/handler builds)

After U5 specifically (per PR #81 precedent):
- `cargo build -p flui-hot-reload --features app-plugin --all-targets` (ABI-shape regression check)

After U7 (final):
- `cargo clippy --workspace --all-targets -- -D warnings` (clean)
- All five workspace-wide grep audits (R15) return zero non-deletion-diff hits.

Post-cleanup (after U8):
- The Mythos audit's Step 1 status block carries the completion annotation referencing U1-U7 commit hashes.

## Scope Boundaries

- **In scope:** the eight units U1-U8 above, executed atomically in order, plus the test-coverage additions in U2 (Canvas op) and U4 (trait method) and AE1 (R4 save/restore semantics lock-in).

### Deferred to Follow-Up Work

- **Renaming `CanvasContext::canvas()` inherent method.** Q3 resolved as "no rename" — Rust method resolution favors inherent over trait, so the trait impl body `self.canvas()` correctly calls the inherent. If a future code reader finds the name collision confusing, a follow-up rename can land independently; deferring it here keeps U5's diff minimal.
- **Audit's `SUPERELLIPSE_CACHE` bounding** ([docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md) Step 5 item 14). U3 may touch the cache in passing but does not bound it; the dedicated stress test and `max_entries` + `last_used_frame` eviction land in a separate plan.
- **Engine performance benchmarking of the new SDF clip.** U3's verification confirms correctness; a Criterion benchmark comparing rsuperellipse-clip vs rrect-clip latency is a follow-up.

### Outside this batch's scope

These items belong to separate brainstorms / plans — do not pull them in:

- **SceneBuilder missing methods** (audit Priority #2).
- **PictureLayer hint fields** (audit Priority #3).
- **RendererBinding redesign** (audit Priority #5).
- **Delegate trait visibility narrowing** (CustomPainter, FlowDelegate, MultiChildLayoutDelegate, SingleChildLayoutDelegate).
- **Lyon tessellation feature-flag move** (audit Step 16).
- **`pipeline.rs` / `pipelines.rs` consolidation** (audit Step 10).
- **`Arc<Mutex<OffscreenRenderer>>` ownership review** (audit Step 12).
- **RenderObject roadmap** (audit Priority #6 — 88% Flutter parity gap).
- **Production integration test for dirty-marking path** (audit Step 4 item 13).

Each gets its own brainstorm / plan iteration.

## Risks & Dependencies

- **R-A1 (Medium):** U1's stub in the engine wgpu match arm may render incorrectly if a `DrawCommand::ClipRSuperellipse` reaches the painter between U1 and U3. **Mitigation:** the stub falls through to existing rrect-handler logic against the superellipse's bounding rect — visually-degraded but not crashing. U2 + U3 land before the trait method (U4) is exposed, so no callers can hit the stub except via direct `Canvas::clip_rsuperellipse_ext` invocation — and there are none in the workspace.
- **R-A2 (Low):** Adding a `DrawCommand` variant may ripple to unexpected `match cmd {}` sites the brainstorm grep missed (e.g., test helpers, debug print formatters). **Mitigation:** `cargo build --workspace` after U1 surfaces every missing arm; fix in-commit.
- **R-A3 (Low):** `RSuperellipse::bounding_rect()` may not exist on the type today. **Mitigation:** U2 verifies at edit time and adds the method as part of the unit if absent. The method shape is trivial (`Rect::from_xywh(self.center.x - self.rx, self.center.y - self.ry, 2.0 * self.rx, 2.0 * self.ry)` or equivalent).
- **R-A4 (Low):** U4's accessor rename (`canvas_mut` → `canvas`) may surface unanticipated callers if any test code outside the trait file references `canvas_mut`. **Mitigation:** `rg 'canvas_mut' crates/flui-painting/` enumerates every site; the brainstorm grep showed only the trait + test impls in `clip_context.rs` itself. Fix in-commit if external callers surface.
- **R-A5 (Low):** Engine wgpu SDF handler in U3 may need a new shader uniform or constant buffer entry for the superellipse parameters. **Mitigation:** `flui-layer::ClipSuperellipseLayer` already lives in the workspace and presumably encodes the math somewhere; reuse its pattern. If a new shader compile path is needed, factor as part of U3 — do not defer.

**Dependencies:** None outside the workspace branch. Land all eight commits on a new feature branch (likely auto-generated worktree name per `ce-work` convention; cycled or created at execution time).

## Outstanding Questions

### Deferred to Implementation

- [Affects U1][Technical] Exact set of `match cmd {}` sites that need the new arm. Implementer runs `rg 'match.*cmd|match cmd' crates/` at edit time; ARCHITECTURE.md says ~1,200 LOC of per-variant arms in `command_ops.rs` plus engine sites.
- [Affects U2][Technical] Whether `RSuperellipse::bounding_rect()` exists. Implementer checks at edit time; adds if missing (R-A3 mitigation).
- [Affects U3][Needs research] Whether `flui-layer::ClipSuperellipseLayer`'s SDF math can be lifted into a shared helper consumed by both the layer and the engine handler, or whether they should stay parallel implementations. Implementer assesses at edit time; the default is to inline the math in the painter handler unless code re-use is obvious.
- [Affects U6][Technical] Exact prose adjustments needed for ARCHITECTURE.md lines 37 + 94 — depends on whether the U5 migration changes any visible counts (default: trait now has 4 default methods not 3; update the count if mentioned).
