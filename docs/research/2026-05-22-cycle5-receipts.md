---
title: "Cycle 5 painting × view — execution receipts"
type: receipts
date: 2026-05-22
plan: docs/plans/2026-05-22-005-refactor-painting-view-cycle5-plan.md
origin: docs/brainstorms/flui-painting-view-cycle5-requirements.md
audit: docs/research/2026-05-22-flui-painting-view-audit.md
branch: feat/painting-view-cycle5
---

# Cycle 5 painting × view — execution receipts

Commit-by-commit ledger of the 15-unit execution (Wave 1 → Wave 8). Mirrors the Cycle 4 Wave 2 receipts format (`docs/research/2026-05-22-cycle4-wave2-receipts.md`).

**Final state:** 29 commits on `feat/painting-view-cycle5` (28 unit/work commits + 1 plan doc commit), +4787/−4145 net across 81 files. `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/port-check.sh -v` (7/7 triggers) all green.

---

## Wave-by-wave commit ledger

### Wave 1 — P0 correctness & parallel-type drift

| U-ID | Finding | Commit | Subject |
|---|---|---|---|
| U1 | P-1 | `588ae74d` + `506bfb12` | drop dead `tessellation` module (+ Cargo.lock sync) |
| U2 | P-2, V-8, P-19 | `802d9698` | drop dead `hit_region` surface |
| U3 | V-1 | `372f5168` | delete the dead flat inherited-element registry |
| U4 | V-12 | `920d99ee` | return `Result` from `attach_root_widget` |
| fix | — | `8a627786` | make build-test fixtures terminate (no self-returning views) |

### Wave 2 — Keyed child reconciliation

| U-ID | Finding | Commit | Subject |
|---|---|---|---|
| U5 | V-2, V-11, V-25 | `945d941d` | hoist keyed child reconciliation into production path |

### Wave 3 — Wire forward-looking half-impls

| U-ID | Finding | Commit | Subject |
|---|---|---|---|
| U6 | R8 | `cf218a6a` | route root-widget bootstrap through `RootRenderView` |
| U7 | R9 | `6e7e0c02` | catch panicking `build()` and substitute `ErrorView` |

### Wave 4 — Genuine-zombie removal (5 atomic commits)

| U-ID | Finding | Commit | Subject |
|---|---|---|---|
| U8 | P-4/P-14/P-15 | `6a97415f` | delete `canvas::sugar` invented ergonomics |
| U8 | P-3 | `9f769a3d` | drop `text_layout::fallback` parallel `TextLayout` |
| U8 | P-9 | `3a44166e` | drop `Picture` type alias for `DisplayList` |
| U8 | V-9 | `20145fc1` | drop parallel Notification dispatch surface |
| U8 | V-10 | `b31f757b` | delete deprecated `SharedWidgetsBinding` |

### Wave 5 — Port-target ledger + rename

| U-ID | Finding | Commit | Subject |
|---|---|---|---|
| U9 | R7 | `d2ba169f` | rename `AnimationBehavior` struct → `AnimatedBehavior` |
| U9 | R3, R6 | `f7c211cc` | mark forward-looking port-targets with `// PORT-TARGET:` ledgers |

### Wave 6 — Hot-path performance

| U-ID | Finding | Commit | Subject |
|---|---|---|---|
| U10 | P-7 | `799735ab` | intern `Paint` behind `Arc` at recording time |
| U11 | P-11 | `566b4212` | walk-and-push `append_display_list_at_offset` |
| U11 | P-6 | `611bbe3d` | use `windows(2)` for `draw_polyline` pair iteration |
| U12 | V-16 | `3ceb4b0c` | iterative O(N) walk for `collect_all_elements` |
| U12 | V-13 (cheap) | `1bf33c65` | cache dummy `ElementBuildContext` in `BuildOwner` |

### Wave 7 — Hygiene + did_change_dependencies wire-up

| U-ID | Finding | Commit | Subject |
|---|---|---|---|
| U13 | V-17 | `65e10831` | mark `Lifecycle` as `#[non_exhaustive]` |
| U13 | V-22 | `597b5709` | mark `RenderSlot` as `#[non_exhaustive]` |
| U13 | P-10 | `be776ee8` | demote `SystemFontsNotifier` to `pub(crate)` |
| U13 | V-21 | `a5ec8a28` | snapshot observers before firing in `handle_*` event handlers (deadlock fix) |
| U13 | P-12, P-16, P-17, P-18, V-24 | `fedb3ccc` | `REMOVE_BY`/`REVIEW_BY` cadence markers + doc fixes |
| U14 | V-19 | `7b5ab623` | fire `did_change_dependencies` on inherited update before rebuild |

### Wave 8 — Cross-crate parallel-type cleanup

| U-ID | Finding | Commit | Subject |
|---|---|---|---|
| U15 | V-14 | `b7738ceb` | delete parallel `Color`, migrate `ColorScheme` to `flui_types::Color` |

---

## Grep gates — all clean post-cycle

```
rg "flui_painting::tessellat"               → 0
rg "HitRegion|add_hit_region"               → 0 (live code; intentional CHANGELOG/audit hits filtered)
rg "register_inherited|inherited_elements"  → 0
rg "draw_pill|canvas::sugar|canvas_sugar"   → 0 (live)
rg "fallback::TextLayout|text_layout::fallback" → 0 (live)
rg "flui_painting::Picture|pub type Picture"   → 0
rg "NotificationNode|NotificationHandler"   → 0
rg "SharedWidgetsBinding|create_shared_binding" → 0
rg "AnimationBehavior" crates/flui-view     → only the deliberate disambiguation prose
rg "flui_app::theme::.*Color|theme::Color"  → 0
```

`scripts/port-check.sh -v` — all 7 institutional refusal triggers clean.

---

## Dropped audit findings (researched, premise empirically wrong against live code)

Three findings were dropped during execution after research showed the audit's framing was factually wrong. The Cycle 4 meta-learning (audit estimates can be 10× off) generalized here to **audit *premises* can be wrong**, not just sizes.

### P-13 — `DrawCommand::kind()` re-shape

**Audit claim:** `kind()` is a 29-arm match; should become a `#[repr(u8)]` discriminant for hot-path filtering.

**Empirical reality (verified at `crates/flui-painting/src/display_list/command_ops.rs:624-639`):** `kind()` is already a compact or-pattern match that LLVM optimizes to a jump table. The audit's 29-arm framing was outdated.

**Disposition:** Dropped per the plan's explicit gating clause ("gate this finding on a before-benchmark; keep the `#[repr(u8)]` change only if `kind()` filtering shows as a measured hot spot, otherwise drop P-13 as already-adequate"). No benchmark harness existed; no measurement possible; finding dropped.

### P-8 — `ClipShape` → `usize` clip-depth counter

**Audit claim:** `ClipShape` variant payloads are "stored but never read"; replace `Vec<ClipShape>` with a `usize` counter.

**Empirical reality (verified at `crates/flui-painting/src/canvas/clipping.rs:148-185`):** The payloads ARE read — by `Canvas::local_clip_bounds()` (matches each variant), `Canvas::device_clip_bounds()`, and `Canvas::would_be_clipped()`. These are public production-API culling helpers documented in `src/lib.rs`, `docs/PERFORMANCE.md`, and `docs/ARCHITECTURE.md`. 4 in-crate tests assert the behavior.

**Disposition:** Dropped. Executing the prescribed fix would silently regress documented culling behavior. The full removal of the culling API is a separate decision, not a constant-factor perf fix — explicitly NOT P-8 scope.

### V-15 — Delete dead `DirtyElement::depth()` / `InactiveElement::depth()` accessors

**Audit claim:** These accessors are `#[allow(dead_code)]`-gated because they have no callers; the `Ord` impl reads fields directly.

**Empirical reality (verified at `crates/flui-view/src/owner/build_owner.rs`):** 
- `InactiveElement::depth()` is called at line 390 in production code: `self.inactive_elements.sort_by_key(|entry| std::cmp::Reverse(entry.depth()));` (`finalize_tree`'s end-of-frame deepest-first sort).
- `DirtyElement::depth()` is called at lines 659/662/665 by the inline test `test_dirty_elements_processed_in_depth_order` to assert the min-heap ordering — the `#[allow(dead_code)]` is the correct escape hatch for a test-only method (since `#[cfg(test)]` would be over-coarse).

**Disposition:** Dropped. Both halves of the audit claim are empirically wrong; deleting the accessors would break production sort + remove a real test.

### V-14 audit-claim precision note (not a drop)

V-14 was executed, but the audit's "zero in-workspace consumers" claim was *imprecise*: the parallel `Color` had two in-crate consumers (via `ColorScheme`), zero external. The plan's secondary branch ("rebuild `ColorScheme` on `flui_types::Color`") covered this; the unit landed on that branch. Worth flagging for the next audit — the precision of "zero consumers" claims matters.

---

## Significant architectural divergences

### U6 — flui-app root-bootstrap is `runner.rs::mount_root`, NOT `attach_root_widget`

The plan assumed `WidgetsBinding::attach_root_widget` is the flui-app production root-bootstrap. **It is not.** Investigation revealed:

- `AppBinding::attach_root_widget` (`crates/flui-app/src/app/binding.rs:179`) is a wrapper around `widgets.attach_root_widget(view)` with **zero callers** in the workspace.
- Production root-bootstrap is `crates/flui-app/src/app/runner.rs::mount_root` (called from 3 sites: lines 132, 395, 558), which hand-rolls its own `RootRenderView` wrap + `RootRenderElement::set_pipeline_owner` + `mount` + `set_root_element`.
- `AppBinding.root_element: Mutex<Option<Box<dyn ElementBase>>>` owns the root element as a **Box**, separate from `WidgetsBinding.element_tree`. `rebuild_root()` calls `perform_build` directly on that owned box, bypassing the `ElementTree`.

**This is a third facet of the V-7 two-ownership-models split.** The runner→`attach_root_widget` consolidation requires deciding where the root element actually lives (`AppBinding` owned-Box vs `WidgetsBinding` `ElementTree` root) — the same ownership decision the deferred Cycle 6 V-7 unification needs to make.

**Disposition:** Runner consolidation **deferred to Cycle 6** with V-7. Both `attach_root_widget` methods now carry `// PORT-TARGET:` ledgers (U9 commit `f7c211cc`) so the deferral does not silently regress. U4 (`Result` signature) and U6 (`RootRenderView` wiring) remain correct port-target improvements waiting on their consumer.

### V-19 — Flutter's actual mechanism (flag-and-fire) replaces the plan's literal "synchronous downcast"

The plan asked for `did_change_dependencies` invocation inside `InheritedBehavior::on_view_updated`. Investigation showed this not executable without expanding `ElementOwner` to carry `ElementTree` access (foundational change beyond U14's ±30 LOC estimate).

**The subagent matched Flutter's actual implementation** (`framework.dart:6117` + `framework.dart:5977-5982`): `StatefulElement` sets `_didChangeDependencies = true` during the update phase; `StatefulElement.performRebuild` consumes the flag and fires `state.didChangeDependencies()` strictly before `super.performRebuild()`. FLUI now mirrors this — `BuildOwner.pending_dependency_changes: HashSet<ElementId>`, set by `InheritedBehavior::on_view_updated`, consumed by `build_scope` before `perform_build` per element. `ElementTree::remove` clears the flag.

The plan's stated **sequencing contract** ("did_change_dependencies fires strictly before the dependent's rebuild — Flutter parity") is honored. The test `fires_typed_hook_exactly_once_before_rebuild` pins the order `[dcd:1, build]`.

---

## Cycle 6 deferred list (carried forward)

Findings explicitly deferred to a future cycle, with consistent rationale (all entangled with the element-ownership unification or substantively related):

- **V-7** — `ElementTree` implements `TreeRead`/`TreeNav`/`TreeWrite` (original plan deferral). Requires deciding the single by-id owner for the element tree.
- **V-1 real fix** — Per-element persistent `_inheritedElements` map for O(1) inherited lookup (Cycle 5 deleted the broken flat registry; the faithful per-element map is V-7-entangled).
- **V-13 real fix** — Threading a live `ElementBuildContext` through every build (Cycle 5 landed the cheap dummy-cache option only).
- **V-20** — `ElementBase` sub-trait split (audit "future wave").
- **V-23** — `WidgetsBindingInner` per-field locks (audit "future wave").
- **U6 discovery — flui-app runner root-bootstrap consolidation** — `runner.rs::mount_root` (3 sites) delegating to `WidgetsBinding::attach_root_widget`, plus reconciling the `AppBinding.root_element` owned-`Box` vs `WidgetsBinding` `ElementTree` root ownership.

**Recommended Cycle 6 scope:** the element-ownership unification + V-20 + V-23 + the U6 runner consolidation. The runner consolidation is naturally the *first* unit of that cycle because the ownership decision it surfaces is the same one V-7 demands.

---

## Pre-existing failures noted (NOT introduced by Cycle 5)

Two pre-existing test failures surfaced during cycle execution. Both reproduce at HEAD before the cycle started; both are out of Cycle 5 scope:

1. **`flui-platform --lib` exits with `STATUS_HEAP_CORRUPTION` (0xc0000374) on Windows.** Real bug, pre-existing. Worth a dedicated bug-investigation task; not on the Cycle 6 deferred list because it is NOT element-ownership-related.

2. **`flui-layer` doctests fail compilation** on `Rect::from_xywh(0.0, …)` / `Offset::new(100.0, 50.0)` — `Pixels` newtype mismatch in doctest snippets. Pre-existing; worth a docs hygiene pass.

Neither failure was introduced by any Cycle 5 commit.

---

## Final verification

- `cargo build --workspace` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `bash scripts/port-check.sh -v` — all 7 institutional refusal triggers clean
- All grep gates clean (live code; intentional CHANGELOG / audit-doc hits filtered)
- 29 commits on `feat/painting-view-cycle5` ready for PR

---

## Notable execution statistics

- **15 plan units** → **23 atomic commits + 4 pre-cycle commits = 27 cycle-content commits** (plus 1 plan doc + 1 test-fixture fix). The plan's R18 "atomic-commit-per-finding" discipline held — multi-finding units (U8 with 5 commits, U9 with 2, U11 with 2, U12 with 2, U13 with 5) each split correctly; single-finding units (U1-U7, U10, U14, U15) each landed as one.
- **3 audit findings dropped** on empirically wrong premises (P-13, P-8, V-15) — 7% drop rate. **1 finding** (V-14) executed with a precision note. **1 finding** (V-19) executed with significant architectural divergence (Flutter actual mechanism). The remaining 40 findings landed as planned.
- **Audit blast-radius accuracy:** U10/P-7 ("±200 cross-cutting") landed at ~150 LOC net non-test — accurate, NOT a 10× miss. U14/V-19 ("±30 LOC") landed at ~70 LOC production — 2× miss, within soft range.
- **U6 plan-model error** surfaced + documented. Single largest divergence of the cycle; resolved by Cycle 6 deferral + port-target ledger.
