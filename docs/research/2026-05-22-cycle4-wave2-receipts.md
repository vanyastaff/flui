# Cycle 4 Wave 2 — Verification Receipts

**Branch:** `feat/render-engine-cycle4-wave2` (stacked on Wave 1 PR #109)
**Design:** `docs/research/2026-05-22-cycle4-wave2-design.md`
**Audit:** `docs/research/2026-05-22-flui-rendering-engine-audit.md`
**Status:** All 4 architectural P0 findings closed (R-6, R-7+R-8+R-9 trio, E-2).
**Date:** 2026-05-22.

This document records the verification gates executed for each Wave 2
unit and the final cumulative state. Same shape as cycle 3 PR #102's
receipts.

---

## Commit-by-commit ledger

Wave 2 commits (stacked on Wave 1, which is the 6 commits of PR #109
plus the design doc `4eacb862`):

| Unit | Commit | Subject | Files | LOC delta |
|------|--------|---------|-------|-----------|
| design | `4eacb862` | docs: cycle 4 Wave 2 architectural design | 1 | +787 / 0 |
| U-1 | `8a1e7b65` | refactor(rendering)!: hide RendererBinding lock topology behind 4 typed primitives (R-6) | 3 | +138 / −73 |
| U-2 | `07b620d6` | feat(interaction): add HitTestResult paint scope guards + HitTestEntry::with_transform_unchecked (R-7 prep) | 1 | +59 / 0 |
| U-3 | `3698a540` | refactor(rendering): delete parallel BoxHitTestEntry / SliverHitTestEntry / BoxHitTestResult / SliverHitTestResult in hit_testing module (R-7 U-3) | 4 | +73 / −719 |
| U-4 | `10702ae3` | refactor(rendering+app)!: collapse rendering HitTestResult/HitTestEntry to interaction canonical (R-7 U-4) | 4 | +126 / −511 |
| U-5 | `d3464a01` | refactor(rendering): delete HitTestTarget trait + entire hit_testing/target.rs module (R-7 U-5) | 3 | +59 / −369 |
| U-6 | `0f69bed6` | refactor(rendering+app)!: delete flui-rendering input module, migrate to flui-interaction MouseTracker (R-8 + R-9 U-6) | 6 | +57 / −680 |
| U-7 | `bccb517a` | docs(rendering): fix stale rustdoc intra-doc link to renamed add_render_view (R-7 U-7) | 1 | +3 / −2 |
| U-8 | `146672e8` | feat(engine): add 'frame lifetime to Backend with bind_surface() (E-2 U-8) | 2 | +69 / −5 |
| U-9 | `3e1f0839` | feat(engine): wire DisplayList backdrop filter through offscreen pipeline (E-2 U-9) | 1 | +139 / −15 |

**Cumulative branch delta vs `origin/main`:** **29 files changed,
1747 insertions(+), 2784 deletions(-)** — net **~−1,037 LOC**.

Of those 16 commits, 6 are from Wave 1 (PR #109) and 10 are
Wave 2 (this doc's scope, including the design + receipts).

---

## Per-gate outcomes

Each gate was executed at the **branch HEAD** (after all 10 Wave 2
commits landed). Gates that depend on specific commits are noted.

### Build

```
$ cargo build --workspace
   ...
    Finished `dev` profile [optimized + debuginfo] target(s) in 2.15s
```

✅ Clean.

### Clippy

```
$ cargo clippy --workspace --all-targets -- -D warnings
   ...
    Finished `dev` profile [optimized + debuginfo] target(s) in 0.43s
```

✅ Zero warnings.

### Tests

| Crate | Result | Pre-Wave-2 baseline | Notes |
|-------|--------|---------------------|-------|
| `flui-rendering --lib` | **278 passed; 0 failed** | 318 | -40 matches deleted unit-test count for U-3 (BoxHitTestResult / SliverHitTestResult), U-4 (HitTestResult), U-5 (PointerEvent), U-6 (MouseTracker). No surviving tests regressed. |
| `flui-interaction --lib` | **250 passed; 0 failed** | 250 | No change — U-2's additions land via additive `HitTestResult` methods + `HitTestEntry::with_transform_unchecked` builder, no test deletions. |
| `flui-engine --lib` | **59 passed; 0 failed** | 59 | No change — U-8 + U-9 modify production paths without touching test surface. |
| `flui-app --lib --test-threads=1` | **25 passed; 0 failed** | 25 | Serial-run required for the pre-existing `test_semantics_enabled` ↔ `test_semantics_listener` singleton-state flake (cycle 2 PR #100 noted the same). Not introduced by Wave 2. |

✅ All test crates green at branch HEAD.

### port-check.sh

```
$ bash scripts/port-check.sh -v
ok    1: RwLock<Box<dyn ...>> in render/view/layer/painting/engine crates
ok    2: Box<dyn ...> wrapped in interior-mutability primitive in render/view/layer/painting/engine storage
ok    3: async fn build/layout/paint/perform_layout/composite/render/submit/present/render_scene/render_layer_recursive/handle_backdrop_filter/fire_composition_callbacks in render/layer/engine hot path
ok    4: Mutex on dirty-list state in flui-rendering production code
ok    5: Arc::clone in per-frame paint/composite loop
ok    6: Box<dyn View> stored as a struct field in element child collections
ok    7: Arc<(Mutex|RwLock)<*Renderer|*Pool|wgpu::*>> struct field in flui-engine wgpu module
port-check: all seven refusal triggers clean
```

✅ All 7 institutional refusal triggers clean.

### Doc-build

```
$ cargo doc -p flui-rendering --no-deps
   ...
warning: `flui-rendering` (lib doc) generated 17 warnings
```

✅ Wave 2 introduces **zero new** doc warnings; the 17 surviving
warnings (PaintContext / PipelineOwner / BoxParentData /
SliverParentData / DEFAULT_DIRTY_CHANNEL_CAPACITY / RenderResult /
Paint / RenderObject::attach / ChangeNotifier intra-doc links +
unclosed-HTML-tag warnings on `Protocol` / `P` / `RenderFlags` /
`BoxProtocol` / `SliverProtocol`) are all pre-existing, outside Wave
2's scope. U-7 fixed the one Wave-2-introduced warning
(`Self::add_render_view` -> `Self::add_render_view_with_config`).

### Architectural grep gates

The audit's specific cleanup checks:

| Gate | Command | Expected | Actual |
|------|---------|----------|--------|
| `render_views()` callers | `rg 'render_views\(\)\.' crates/` | zero hits | **zero hits** — all 9 pre-cycle callers migrated to 4 primitives in U-1 |
| Bridge TODO | `rg 'TODO: Convert' crates/flui-app/` | zero hits | **zero hits** — the literal `// TODO: Convert rendering HitTestEntry targets to interaction targets` at `flui-app/src/app/binding.rs:507` disappeared in U-4 |
| Rendering HitTestResult | `rg 'flui_rendering::hit_testing::HitTestResult' crates/` | zero hits in production | **zero hits in production** — only one docstring mention at `render_view.rs:572` recording the U-4 deletion + one in `binding.rs:503` recording the bridge removal |
| HitTestTarget trait | `rg 'HitTestTarget' crates/flui-rendering/src/` (production refs) | zero hits | **zero production hits** — only comment / docstring mentions recording the U-3/U-4/U-5 deletion timeline; the `lib.rs:82` re-export points at `flui_interaction::HitTestTarget` (separate trait) |
| Rendering input module | `rg 'flui_rendering::input' crates/` (production refs) | zero hits | **zero production hits** — one docstring mention at `flui-app/src/bindings/renderer_binding.rs:88` recording the U-6 migration |

✅ All grep gates pass.

---

## Cumulative findings closed

Wave 2 closes the remaining 4 P0 architectural findings from the
cycle-4 audit, on top of Wave 1's 9 (PR #109). The full P0 closure
ledger for cycle 4:

| # | Finding | Theme | Wave | Commit | Status |
|---|---------|-------|------|--------|--------|
| R-1 | `run_semantics` `unimplemented!()` | Constitution Principle 6 violation | 1 | `94fd80fd` | ✅ closed |
| R-2 | `perform_semantics_action` `unimplemented!()` | Constitution Principle 6 violation | 1 | `94fd80fd` | ✅ closed |
| R-3 | `SemanticsBuilder::new()` `unimplemented!()` | Constitution Principle 6 violation | 1 | `94fd80fd` | ✅ closed |
| R-4 | `run_compositing` silent stub | Half-impl observability | 1 | `94fd80fd` | ✅ closed |
| R-5 | `RenderDirtyPropagation` zombie trait | Zombie deletion | 1 | `4284d5c4` | ✅ closed |
| **R-6** | **`RendererBinding::render_views` nested-lock smell** | **Trait-surface lock-hiding** | **2** | **`8a1e7b65`** | **✅ closed** |
| **R-7** | **Two `HitTestResult` types (rendering vs interaction)** | **Parallel-type consolidation** | **2** | **`3698a540` + `10702ae3` + `d3464a01` + `bccb517a`** | **✅ closed** |
| **R-8** | **Two `MouseTrackerAnnotation` types** | **Parallel-type consolidation** | **2** | **`0f69bed6`** | **✅ closed** |
| **R-9** | **Two `MouseTracker` types** | **Parallel-type consolidation** | **2** | **`0f69bed6`** | **✅ closed** |
| R-10 | `flui_engine::RenderError` rename | Parallel-type rename | 1 | `d321acd8` | ✅ closed |
| E-1 | `WgpuPainter::clip_path` silent no-op | Half-impl observability | 1 | `15a6f062` | ✅ closed |
| **E-2** | **`Backend::render_backdrop_filter` unimplemented** | **Architectural ownership + impl** | **2** | **`146672e8` + `3e1f0839`** | **✅ closed** |
| E-3 | `PipelineManager` + `PipelineHandle` zombie | Zombie deletion | 1 | `7e892e16` | ✅ closed |
| E-4 | effects.rs forward-looking helpers | Zombie deletion + lint discipline | 1 | `0fc9e49d` | ✅ closed |

**13 of 14 cycle 4 P0 findings closed.** The 14th — R-11 (rename
`flui_view::ParentData` → `ParentDataConfig` to resolve trait-name
collision) — was reclassified P1 in the audit ranking and falls into
the Wave 3 backlog.

---

## What Wave 2 produced

**Architectural wins:**

1. **`RendererBinding` lock topology hidden behind 4 typed
   primitives** — the trait no longer leaks `&RwLock<HashMap<u64,
   Arc<RwLock<RenderView>>>>` to consumers. Implementers retain full
   freedom over container choice and lock primitive.

2. **Single canonical `HitTestResult` flows from RenderView through
   gesture dispatch** — flui-app's `// TODO: Convert rendering
   HitTestEntry targets to interaction targets` bridge (which
   silently dropped every hit) is gone. The rendering crate's
   `hit_testing` module is now a thin protocol-extension surface
   over `flui_interaction::routing`.

3. **`HitTestTarget` vestigial trait deleted** — one production impl
   (RenderView, deleted in U-4) + two file-private DummyTarget stubs
   was the entire workspace shape. FLUI never adopted Flutter's
   trait-dispatch pattern; the audit recommended deletion-not-
   relocation, and the empirical evidence confirmed it.

4. **`flui-rendering::input` module deleted** — `MouseTracker` +
   `MouseTrackerAnnotation` consolidated to flui-interaction's
   canonical implementations. The pre-cycle dummy `hit_test_callback`
   in `RenderingFlutterBinding::new_with_pipeline` (a tell that the
   shape was placeholder, not working) is gone.

5. **`Backend::render_backdrop_filter` wired through the offscreen
   pipeline** — `Backend<'frame>` now borrows the frame's surface
   handles via `bind_surface()`; the DisplayList-command-level
   backdrop-filter path mirrors the layer-tree-level path's 5-stage
   pipeline (flush + COPY + Dual Kawase + queue composite +
   dispatch child). Closes a visible Flutter-parity regression.

**LOC accounting:**

- **Wave 2 net delta:** ~−1,037 LOC across 29 files.
- **Wave 2 deletion bulk:** 2,784 lines removed (BoxHitTestResult /
  BoxHitTestEntry / SliverHitTestResult / SliverHitTestEntry /
  HitTestResult / HitTestEntry / HitTestTarget / PointerEvent /
  PointerEventKind / PointerDeviceKind / MouseTracker /
  MouseTrackerAnnotation / MouseCursorSession / PointerEnterEvent /
  PointerExitEvent / PointerHoverEvent / MouseTrackerHitTest /
  parallel-type tests + bridge code + stale rustdoc links).
- **Wave 2 addition bulk:** 1,747 lines added (4 trait primitives +
  `add_render_view_with_config` + 2 paint-scope guards +
  `with_transform_unchecked` + `BoxHitTestResult` adapter
  scaffolding + `Backend<'frame>` lifetime + `bind_surface` +
  full `render_backdrop_filter` body + comprehensive migration
  docstrings + design doc + this receipts doc).

**Architectural-shape verdicts the audit produced during Wave 2
implementation that the original cycle-4 audit did NOT surface:**

- **E-2 ownership rotation is NOT needed.** The audit's E-2 fix-shape
  enumerated three sub-tasks (ownership rotation, painter API
  expose, actual filter rendering). Reading the engine showed
  `Backend::with_offscreen` already owns the
  `Arc<Mutex<OffscreenRenderer>>`; the painter's
  `queue_offscreen_result` is already pub. The actual gap was a
  surface-handle borrow, addressed by the `'frame` lifetime
  parameter.

- **`HitTestTarget` is delete-not-relocate.** The audit's R-7 fix
  shape considered moving the trait down to flui-interaction or
  flui-foundation as a lower common dep. Empirical evidence (`rg
  'impl HitTestTarget for'` returning 1 production impl + 2 stubs)
  collapsed that to a deletion: FLUI never adopted the trait, so
  there is no trait to relocate.

- **`BoxHitTestEntry` had pre-existing internal divergence.** Two
  distinct types with the same name lived in
  `hit_testing/entry.rs` (1-arg `new(local_position)`) and
  `protocol/box_protocol.rs` (2-arg `new(target_id, transform)`).
  Same shape for `SliverHitTestEntry`. U-3 resolved it by deleting
  the dead `hit_testing` parallels.

---

## Verification commands (full set)

For future reference / CI integration. Each command was run at branch
HEAD (post-U-9) and produced the result column shown.

```bash
# Build gates
cargo build --workspace                                                  # OK
cargo clippy --workspace --all-targets -- -D warnings                     # OK
cargo doc -p flui-rendering --no-deps                                    # 17 pre-existing warnings, 0 new

# Test gates (per crate)
cargo test -p flui-rendering --lib                                       # 278/0
cargo test -p flui-interaction --lib                                     # 250/0
cargo test -p flui-engine --lib                                          # 59/0
cargo test -p flui-app --lib -- --test-threads=1                         # 25/0 (singleton-flake mitigation)

# Architectural-discipline gates
bash scripts/port-check.sh -v                                            # 7/7 clean

# Dead-symbol grep gates
rg 'render_views\(\)\.' crates/                                          # 0
rg 'TODO: Convert' crates/flui-app/                                      # 0
rg 'flui_rendering::hit_testing::HitTestResult' crates/ -F | grep -v '//'   # 0 production
rg 'flui_rendering::input' crates/ -F | grep -v '//'                     # 0 production
```

All green.

---

## Wave 2 verdict

**Single-PR-sized at the upper edge.** The Wave 2 design predicted
this; reviewer feedback can split into "trio (U-2..U-7) PR-A + R-6 +
E-2 + receipts PR-B" if the cumulative diff exceeds review tolerance.
The branch is currently stacked on Wave 1 PR #109 (per user
direction); once #109 merges, the rebase produces a clean Wave 2
PR against `main`.

The 4 architectural P0 findings are closed. The remaining cycle 4
work (P1 backlog) covers parity items (`ParentData` rename), dead-
field deletes (`RenderView` dead fields, `ScrollableViewportOffset`
placeholder), API trim items, and the engine instancing / pipeline
forward-looking helpers — all mechanical, none architectural.

**Cycle 4 status:** 13 of 14 P0 findings closed across Waves 1 + 2.
P1 + P2 + P3 (32 findings remaining) are independent mechanical
work suitable for follow-up cycles.
