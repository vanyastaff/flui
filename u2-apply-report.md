# PR 1a commit #1 — U2 apply report

**Worker role:** `worker` (implementation subagent).
**Task:** Remove cross-type `Pixels` ↔ `f32` ops from `flui-geometry`,
strict-TDD, ONE atomic commit.
**Date:** 2026-05-25.
**Time consumed:** ~50 min wall clock.

---

## 1. Commit SHA + branch

- **Branch:** `pr1-u2-cross-type-pixels-ops` (created here on the main repo
  because cwd was NOT a worktree — see §3 “Environment discrepancy”).
- **Parent commit:** `0fdd0f65` (main HEAD as instructed).
- **U2 commit:** `35db8a1607996687d3339f71758172d1e713f842`.
- **Subject:** `refactor(geometry): remove cross-type Pixels ops (U2)` (exact).
- **Not pushed.** Local branch only, per `Do NOT push to main`.

```
35db8a16 refactor(geometry): remove cross-type Pixels ops (U2)
0fdd0f65 docs(geometry): U17 spike outcome + PR 1 planning consolidation
```

---

## 2. `git diff --stat HEAD~1..HEAD`

```
 crates/flui-app/src/app/config.rs                  |  4 +-
 crates/flui-app/src/app/runner.rs                  |  3 +-
 crates/flui-geometry/src/bounds.rs                 |  4 +-
 crates/flui-geometry/src/point.rs                  |  4 +-
 crates/flui-geometry/src/rect.rs                   |  6 +-
 crates/flui-geometry/src/units.rs                  | 95 ++++++++--------------
 crates/flui-geometry/src/vector.rs                 |  4 +-
 crates/flui-layer/src/layer/clip_rect.rs           |  2 +-
 crates/flui-layer/src/layer/clip_rrect.rs          |  2 +-
 crates/flui-layer/src/layer/clip_superellipse.rs   |  2 +-
 crates/flui-layer/src/layer/offset.rs              |  4 +-
 crates/flui-painting/tests/canvas_composition.rs   |  8 +-
 crates/flui-painting/tests/rich_text_example.rs    |  4 +-
 crates/flui-painting/tests/text_layout_pipeline.rs |  9 +-
 .../src/constraints/box_constraints.rs             |  8 +-
 crates/flui-rendering/src/context/hit_test.rs      |  2 +-
 crates/flui-rendering/src/hit_testing/transform.rs | 16 ++--
 crates/flui-rendering/src/objects/transform.rs     |  6 +-
 .../src/parent_data/box_parent_data.rs             | 10 +--
 .../flui-rendering/src/parent_data/box_variants.rs |  2 +-
 .../src/parent_data/sliver_variants.rs             |  2 +-
 crates/flui-rendering/src/protocol/box_protocol.rs |  2 +-
 crates/flui-rendering/src/view/configuration.rs    | 18 ++--
 crates/flui-semantics/src/node.rs                  |  2 +-
 crates/flui-types/src/gestures/velocity.rs         |  5 +-
 .../tests/compile_fail/mixed_units.stderr          | 18 ++--
 crates/flui-types/tests/geometry_property_tests.rs |  9 +-
 27 files changed, 114 insertions(+), 137 deletions(-)
```

Net: **-23 LOC** (deletions dominate, as expected — the 8 impls were
larger than their corresponding `Pixels::ZERO` / `px(literal)` boundary
fixes). Reviewer-attention LOC stays well within the planning doc’s
~70-LOC budget for U2: the deletion in `units.rs` is the only block that
needs careful semantic review; everything else is mechanical boundary
fixes.

---

## 3. Sanity-check baseline (BEFORE any code change)

Run on inherited environment, **before** the RED step.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✓ green (exit 0) |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✓ green (exit 0) |
| `bash scripts/port-check.sh -v` | ✓ green: **all 13 refusal triggers + FR-033** clean, no marker budget violations |
| `git status` (clean working tree) | **partial**: short status was clean, but `git status -uall openspec/` revealed **untracked** pre-existing planning files under `openspec/changes/core-0a-foundation-parity-to-flutter/` (mtime `May 25 00:31`, predates this task). Not mine — excluded from commit. |
| `git worktree list` | cwd is the **main repo on branch `main`** (HEAD `0fdd0f65`), NOT a worktree as the task assumed. Two unrelated worktrees exist (`jolly-mayer-273837`, `romantic-shannon-792425`). Created branch `pr1-u2-cross-type-pixels-ops` in the main repo as the closest safe interpretation. |
| `cargo test --workspace` (default parallelism) | **1 pre-existing flaky failure** in `flui-app::bindings::renderer_binding::tests::test_semantics_enabled`. Singleton state pollution from another test in the same module setting `set_semantics_enabled(true)` and not resetting it. **Passes** in isolation (`cargo test -p flui-app --lib test_semantics_enabled`) and **passes** under `--test-threads=1`. Unrelated to U2 (no `Pixels` code path). |
| `cargo test --workspace --no-run` | **stale-cache LNK1120** under sccache, on `flui_rendering-c8fc...rcgu.o` referencing missing LLVM anon symbols. Resolved by `cargo clean -p flui-rendering`; build then succeeds. This is a known Windows + sccache + Rust 1.95 interaction, not a code issue. |

**Escalation attempts:** two `contact_supervisor` calls (one for the
worktree-vs-main-repo discrepancy, one for the baseline flake decision)
both returned `Intercom not connected: Broker failed to start within
timeout`. Generic `intercom` was also unavailable. Proceeded with the
safest documented interpretation:

- created feature branch in the main repo, did NOT push;
- treated `flui-app` flake as pre-existing, out-of-U2-scope, and called
  it out prominently here rather than expanding scope to fix singleton
  pollution in a crate outside the task’s explicit allow list.

---

## 4. AC checklist (planning doc §3)

| AC | Status | Evidence |
| --- | --- | --- |
| **AC-U2-1** 8 impls deleted from `units.rs` | ✓ | `crates/flui-geometry/src/units.rs` lines 471–533 replaced by a 5-line refusal-trigger comment block; final grep below shows 0 hits. |
| **AC-U2-2** `compile_fail` doctest pinning `Pixels == f32` rejection | ✓ | `units.rs` `pub struct Pixels` doc, “Comparison rejected” block: `let _ = px(10.0) == 10.0_f32;` runs as `compile_fail ... ok`. |
| **AC-U2-3** `compile_fail` doctest pinning `Pixels + f32` rejection | ✓ | Same doc block, “Arithmetic rejected”: `let _ = px(10.0) + 5.0_f32;` runs as `compile_fail ... ok`. Also added a positive `Mul<f32>` scaling doctest so a future regression that re-adds `Add<f32>` cannot silently sail past as “scaling is fine”. |
| **AC-U2-4** Every call site fixed at the boundary; no `.into()` band-aids | ✓ | All fixes are one boundary-cross wide: `Pixels::ZERO` for `== 0.0` / `<= 0.0` / `>= 0.0`, `px(N.N)` for non-zero literal comparisons, `.get()` only where the comparison is genuinely unitless (epsilon vs `1e-6`/`1e-5`). **0 `.into()` band-aids added.** **0 `PORT-CHECK-OK-SP3` markers needed.** |
| **AC-U2-5** Validation suite green | ✓ | See §6 below. |
| **AC-U2-6** Final-pass grep gate | ✓ | `rg 'impl (PartialEq\|PartialOrd\|Add\|Sub)<f32> for Pixels' crates/flui-geometry/` → **0 hits**. `rg 'impl (PartialEq\|Add\|Sub)<Pixels> for f32' crates/flui-geometry/` → **0 hits**. Also verified against entire `crates/` tree — 0 hits anywhere. |

---

## 5. REFACTOR call-site fixes — by crate

**37 call sites + 1 stderr snapshot refresh, 7 crates.**

The planning doc estimated “~20 internal call sites”; actual was
~1.85× higher, driven mostly by Pixels-typed `.dx` / `.dy` / `.width`
/ `.height` assertions in test files that the planning doc’s call-site
audit (focused on production code) didn’t enumerate.

| Crate | Sites | Files |
| --- | ---: | --- |
| `flui-geometry` | 4 (all doctests) | `bounds.rs` (line 38 doctest), `point.rs` (`Point::cast` doctest), `rect.rs` (`Rect` struct doctest), `vector.rs` (`Vec2::from_radians` doctest) |
| `flui-rendering` | 18 | **prod (10):** `constraints/box_constraints.rs` (4 `Pixels::ZERO` swaps in `has_loose_*`/`is_normalized`), `context/hit_test.rs` (2 in `is_within_size`), `hit_testing/transform.rs` (2 in `is_identity` + `Pixels` import), `objects/transform.rs` (2 in hit-test bounds check). **test (8):** `parent_data/box_parent_data.rs` (5), `parent_data/box_variants.rs` (1), `parent_data/sliver_variants.rs` (1), `protocol/box_protocol.rs` (1), `view/configuration.rs` (8 — split: 4 `assert_eq!` to `px(literal)`, 4 epsilon-tolerance to `.get()`) |
| `flui-painting` | 3 (all integration tests) | `tests/canvas_composition.rs` (4 `bounds.left()` etc → `px(0.0)`/`px(200.0)`), `tests/rich_text_example.rs` (2 selection-box dims), `tests/text_layout_pipeline.rs` (3 caret/selection box dims) |
| `flui-layer` | 4 (all doctests) | `layer/clip_rect.rs`, `layer/clip_rrect.rs`, `layer/clip_superellipse.rs`, `layer/offset.rs` (each: trailing `assert_eq!(layer.X().width(), 100.0)` → `px(100.0)`) |
| `flui-types` | 5 + 1 snapshot | `src/gestures/velocity.rs` (1 doctest, used `.get()` because epsilon tolerance is unitless); `tests/geometry_property_tests.rs` (4 proptest sites — switched to explicit `epsilon: f32 = 1e-N` because `Point::distance()` returns `f32`, not `Pixels`); `tests/compile_fail/mixed_units.stderr` (snapshot refresh — see §7) |
| `flui-semantics` | 1 | `src/node.rs` (1 test assert in `test_semantics_node_absorb`) |
| `flui-app` | 2 | `src/app/config.rs` (1 test assert in `test_builder_pattern`), `src/app/runner.rs` (1 test assert in `test_config_creation` + a `use flui_types::geometry::px;` in the test module) |
| **Total** | **37 + 1 stderr** | **27 files** |

### Scope-vs-allowed-list note

The task explicitly listed `flui-geometry`, `flui-rendering`,
`flui-painting`, `flui-view` as “any downstream callers that fail
compile after the deletion”. The deletion **also** broke compile in
`flui-layer`, `flui-types`, `flui-semantics`, `flui-app`. Per task’s
hard constraints, only `flui-engine` and `flui-widgets` are explicitly
disallowed; the others were not mentioned. I fixed them at the unit
boundary using the same `Pixels::ZERO` / `px(literal)` / `.get()`
pattern so the workspace builds and AC-U2-5 passes. **`flui-view` was
NOT touched** — no compile breakage surfaced there.

If you want the scope sharper next time (e.g. “only listed crates
allowed”), I’d need either pre-task knowledge that those other call
sites exist (so they can be on the allow list) or permission to leave
the workspace red and surface a list of forced expansion candidates
for orchestrator approval before continuing.

---

## 6. AC-U2-5 validation suite (post-commit, on `35db8a16`)

| Check | Result |
| --- | --- |
| `cargo test -p flui-geometry` | ✓ green |
| `cargo test -p flui-geometry --doc` | ✓ green — including the 2 new U2 `compile_fail` doctests and the pre-existing `transform2d` `compile_fail` |
| `cargo build --workspace` | ✓ green |
| `cargo build --workspace --all-targets` | ✓ green |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✓ green |
| `cargo fmt --all -- --check` | ✓ green |
| `cargo test --workspace --doc` | ✓ green |
| `cargo test --workspace -- --test-threads=1` | ✓ green (118 `test result: ok` lines, EXIT=0, including all trybuild compile-fail tests) |
| `cargo test --workspace` (default parallelism) | **EXIT=101** — same pre-existing `flui-app::bindings::renderer_binding::tests::test_semantics_enabled` flake observed at baseline. **No new failures introduced by U2.** |
| `bash scripts/port-check.sh -v` | ✓ green — all 13 refusal triggers + FR-033 clean, marker budget empty |
| `rg 'impl (PartialEq\|PartialOrd\|Add\|Sub)<f32> for Pixels' crates/flui-geometry/` | 0 hits |
| `rg 'impl (PartialEq\|Add\|Sub)<Pixels> for f32' crates/flui-geometry/` | 0 hits |

`just ci` itself fails on this Windows machine for an **unrelated**
reason: `justfile` uses `set windows-shell := ["powershell.exe", ...]`
and the version backtick (`git rev-parse --short HEAD || echo "unknown"`)
uses POSIX `||`, which PowerShell 7 here parses as an invalid statement
separator. I ran the equivalent recipes manually (`cargo fmt --all --
--check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace -- --test-threads=1`) and reported each above.
This is a pre-existing justfile/Windows interaction; it is **not** a
post-commit regression.

---

## 7. `PORT-CHECK-OK-SP3` markers added

**NONE.** All 37 fixes are one boundary-cross wide and self-justifying:

- `Pixels::ZERO` for `== 0.0` / `<= 0.0` / `>= 0.0` checks — the
  literal `0.0` was unambiguously zero pixels;
- `px(N.N)` for non-zero literal comparisons — the literal `N.N` was
  unambiguously `N.N` pixels;
- `.get()` only where the comparison is genuinely unitless (e.g.
  `(width - 1920.0).abs() < 1e-6`, where the epsilon `1e-6` is a
  floating-point tolerance, not a pixel measurement, and dropping the
  unit at the boundary is the more honest translation). Sites using
  `.get()` are tagged with a short comment `// Epsilon comparison is
  unitless: drop Pixels at the boundary.` so future readers know it
  isn’t a band-aid.

No site needed `.into()`. No site needed an SP3-marker justification.

---

## 8. Surprises encountered

1. **`flui-app::test_semantics_enabled` is flaky at baseline.**
   Singleton-state pollution from sibling tests setting
   `set_semantics_enabled(true)`. Out of U2 scope but flagged here so
   you can decide whether to (a) accept the commit despite a pre-
   existing parallel-mode flake, (b) require a separate
   single-threaded test attribute, or (c) require a fresh fix commit
   first.

2. **sccache + Windows + Rust 1.95 LNK1120 on stale objects.** Hit
   before my work even started, on `flui_rendering`’s lib-test binary,
   citing missing `anon.X.llvm.Y` symbols. Resolved by
   `cargo clean -p flui-rendering`. Recurred later during incremental
   builds with `STATUS_ACCESS_VIOLATION` from sccache itself on
   `flui-types` `geometry_property_tests`. Same recovery pattern works
   (`cargo clean -p flui-types`). This is a known sccache 0.14.0
   limitation, not anything U2 introduced; worth a note in
   `docs/getting-started.md` if you start seeing it bite other workers
   on Windows.

3. **REFACTOR fanout was ~1.85× the planning doc estimate.** Planning
   said “~20 internal call sites”; actual was 37. The delta sits in
   test files (`.dx`/`.dy`/`.width()`/`.height()` assertions that
   previously relied on `Pixels == f32` to look ergonomic). None of
   these are dangerous; they just inflate the diff a bit. The new
   call-site shape is uniformly `Pixels::ZERO` / `px(literal)` /
   `.get()`-with-comment, which sets a clear pattern for the rest of
   PR 1a.

4. **`trybuild` snapshot churn.** Removing `impl Add<f32> for Pixels`
   shortened the compiler diagnostic for
   `crates/flui-types/tests/compile_fail/mixed_units.rs`: it now emits
   a clean E0308 instead of an E0277 that listed the deleted impl as a
   “help”. The `.stderr` snapshot was updated to match the new (and
   more informative) error. The actual U2 invariant is unchanged: the
   test still rejects `Pixels + DevicePixels` at compile time, which
   is the whole point.

5. **`openspec/changes/core-0a-foundation-parity-to-flutter/` is
   untracked at baseline.** Pre-existing planning files from a
   different workstream; I left them untracked and excluded from the
   commit (task explicitly said “do not modify `openspec/`”).
   Mentioning here so you know they aren’t mine.

6. **The orchestrator’s “you are on a worktree branch” assumption was
   inaccurate.** cwd is the main repo on `main`. Created
   `pr1-u2-cross-type-pixels-ops` as a local branch in the main repo —
   functionally equivalent for the no-push constraint, but if you
   intended an isolated worktree for write-isolation reasons,
   subsequent commits should be set up that way first.

---

## 9. Recommended next U-unit

Per planning doc §1, **commit #2 = U1 — `refactor(geometry): drop
Pixels From<scalar> conversions (U1)`** (33 LOC, single-crate).

Why it’s safe to do next:

- U2 already deleted the cross-type ops, so U1’s deletion of
  `impl From<f32> for Pixels` and siblings no longer has a competing
  ergonomic escape hatch — call sites that previously relied on
  `pixels: Pixels = 10.0.into()` would now ALSO have been broken by
  U2 if they then tried to combine the result with a scalar. So U1
  cleanups will surface as failed compiles only where the call site
  was using `From` purely as a literal-construction shortcut, which
  is the exact target of U1.
- U1 carries the same RED→GREEN→TRIANGULATE→REFACTOR pattern, with
  one extra TRIANGULATE doctest covering `impl From<f64>` (the
  precision-loss path) — straightforward extension of the protocol
  established by U2.
- After U1, U6 (dead `Float*` aliases, 15 LOC) is a trivial cleanup
  that doesn’t depend on either.

Suggested dispatch shape (matches the orchestrator’s working pattern):
spawn one fresh `worker` with this report + planning §3-equivalent
adapted to U1, allow list = same 7 crates I had to touch here
(`flui-geometry`, `flui-rendering`, `flui-painting`, `flui-layer`,
`flui-types`, `flui-semantics`, `flui-app`), explicit disallow =
`flui-engine`, `flui-widgets`, `docs/`, `openspec/`.

---

## 10. Open risks / questions

- **The `flui-app` pre-existing flake.** Decide whether to accept the
  commit as-is (parallel `cargo test --workspace` will be red) or
  block on a separate fix. Recommend a tiny `#[serial_test]` or
  per-test reset in a dedicated commit before merging PR 1a.
- **Scope expansion to non-explicitly-allowed crates** (`flui-layer`,
  `flui-types`, `flui-semantics`, `flui-app`). If you want the next
  worker to refuse scope expansion and instead surface a list for
  approval, say so explicitly in the next dispatch prompt. Today’s
  call was “fix at the boundary, document forcibly” because intercom
  was down and breaking compile would have prevented AC-U2-5
  validation.
- **Branch lifecycle.** Branch `pr1-u2-cross-type-pixels-ops` lives
  only in the main repo; not pushed, no remote tracking. If you want
  it on a worktree, do `git worktree add` from a clean main first;
  otherwise the next U-unit commit can stack directly on this branch.
