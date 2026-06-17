<!-- Rust Code Studio — task breakdown for the flui-engine-overhaul spec. Each task is implemented via /dev-task. -->

# Tasks: flui-engine architecture overhaul + wgpu-feature adoption

- **Spec:** [`spec.md`](spec.md)   ·   **Updated:** `2026-06-16`

> Decomposition was produced by a code-grounded sizing Workflow (7 parallel investigators →
> synthesis → adversarial completeness-critique). The critique returned **REJECT pending fixes**
> and caught 4 blocking structural defects + several fake-pass risks; **all are folded in below**
> (see *Critique resolutions*). Line/symbol citations were independently verified
> (`painter.rs` = 6410 lines; `SavedLayer` embeds `saved_draw_order`/`saved_segment` at
> painter.rs:88-107; `paint_fragment_snapshot.rs:1` = "Headless … no GPU, no window").

## Task list
*Status: ☐ todo · ◐ in-progress · ☑ done · ⊘ blocked.*

| # | Task (outcome) | Acceptance slice | Owner lead | Blocked by | Status |
|---|----------------|------------------|------------|------------|--------|
| 1 | **GpuProfiler** (`profiler.rs`): wrap `wgpu-profiler` 0.27 behind a `gpu-profiler` cargo feature; `Renderer` owns `Option<GpuProfiler>` created only when adapter exposes `TIMESTAMP_QUERY` **and** feature on; scopes wrap per-frame submits; latest frame = `GpuFrameProfile` via `Diagnosticable`; graceful no-op otherwise. Purely additive. | C6, C8, C9 (incl. feature ON+OFF builds, wasm-off), C10, partial C1 | `systems-perf-lead` | — | ☑ ([#225](https://github.com/vanyastaff/flui/pull/225)) |
| 2 | **GpuResources facade** (`resources.rs`): single owner of `BufferPool`/`TextureCache`/`layer_texture_pool`/`external_texture_registry` (4 painter fields → 1). RAII + `end_frame_maintenance` order verbatim. Does NOT move painter's own `default_sampler` nor OffscreenRenderer's separate `TexturePool`. **Owns `layer_texture_pool`** (task 8 borrows it). | C1, C8, C9, C10, C11-neg | `systems-perf-lead` | — | ☐ |
| 3 | **PipelineSet** (`pipelines.rs`): explicit device-scoped struct owning the 9 named `RenderPipeline` fields + the shape `PipelineCache` (composition) + `viewport_bind_group_layout` (preserve wgpu object **identity**). 10 painter fields → 1. Format-parametric `new` for windowed+offscreen. Does NOT resurrect the deleted prior `pipelines.rs`. | C1, C8, C9, C10 | `systems-perf-lead` | — | ☐ |
| 4 | **Backend runtime override** (C7): `select_backend()` → `resolve_backends(override)` intersecting requested (env `FLUI_BACKEND` and/or ctor arg) with compiled-in backends, per-platform fallback + `tracing::warn`, threaded through all 3 `Instance::new` sites so `new`/`new_offscreen`/`recover` agree. Default byte-identical. **API decision (env-only vs `with_backend`/`RendererConfig`) gated up front** — it decides whether the 4 flui-app sites are in scope. | C7, C8, C9 (native-target compile + **CI per-OS** matrix; one host can't build all 5 backends), C10 | `async-systems-lead` (+`api-design-lead` sign-off) | — | ☐ |
| 5 | **CommandIR data-type lift** (`command_ir.rs`): move `DrawSegment`/`DrawItem`/`draw_order`/`SavedLayer`/`PendingOpacityLayer` **type-defs** out of painter.rs into `command_ir.rs`, baking semantics byte-identical (still stores lowered GPU bytes). Pure type-move + re-export. **Lands BEFORE the compositor** (task 8) so both it and the batcher import shared IR types from one home (no double-touch). | C5-partial, C1, C8 (snapshot unchanged — pure move), C9, C10, C11 | `chief-architect` | — | ☐ |
| 6 | **Characterization readback safety-net** (test-only, LOCAL-GPU): add GPU-readback tests for nested `clip_rrect`/`clip_rsuperellipse` **SDF-clip baking** (currently ZERO pixel coverage) and nested `save+clip+restore` **scissor** behavior; cross-check the scissor conditional-push/pop asymmetry (painter.rs:4061/4079) against `.flutter/` to settle **bug-vs-design** — if a real bug, fix in its own PR FIRST. These pass on current code = the regression net tasks 7/8 prove byte-identity against. | C8-prereq (net for tasks 7/8), C10 | `qa-lead` (impl `test-engineer`) | — | ☐ |
| 7 | **GpuStateStack** (`state_stack.rs`): own the 4 paired transform/scissor/rrect-clip/rsuperellipse-clip stacks; Copy accessors; **port the scissor push/pop asymmetry verbatim** (per task 6 resolution); set-one-clears-other clip invariant preserved; single glam↔`Matrix4` edge (`current_transform_matrix`, painter.rs:1677); depth counter (== `transform_stack.len()`, keeps `save_count()` contract) + `Drop` `debug_assert`. Opacity/layer LEFT for task 8. | C1, C2, C4, C8 (**real anti-cheat gate = task-6 readback diff, not the balance assert alone**), C9, C10 | `chief-architect` sign-off (impl `systems-perf-lead`) | 6 | ☐ |
| 8 | **LayerCompositor** (`layer_compositor.rs`): own `SavedLayer`+`PendingOpacityLayer`+`opacity_stack`/`current_opacity`/`layer_stack`; `save_layer(&mut GpuStateStack, …)` borrow-enforced + own depth+`Drop` assert; `current_opacity` no longer a painter field. **Borrows `layer_texture_pool` from GpuResources** (task 2 owns it). `flush_opacity_layer` seam = state in compositor, GPU-emission stays a painter method (its `texture_batch` use is a KNOWN seam task 9 re-homes). 3 layer readback tests (premul/chroma/z-order) verbatim. | C3, C8 (3 layer readback tests + snapshot), C1, C9, C10, trait byte-identity | `chief-architect` sign-off (impl `systems-perf-lead`) | 7, 5, 2, 3 | ☐ |
| 9 | **DrawBatcher + record-method relocation** (`batches.rs`): own `texture_batch` + InstanceBatch/Vec<Vertex> accumulation + scissor/tess coalescing + non-SrcOver segment-seal contract; **relocate the ~3000 LOC of per-primitive record methods** (rect/rrect/circle/oval/line/draw_image/draw_path) so painter.rs actually reaches the C1 <1500 coordinator target. `batches.rs` imports **glam only** (CI grep-asserts no `Matrix4`). Likely split into sub-PRs at build time. | C5 (record separated), C4 (grep-enforced), C1 (closes the painter<1500 arithmetic), C8, C9, C10, C11 | `chief-architect` | 5, 8, 7 | ☐ |
| 10 | **Replay/submit split — final painter cut** (`renderer`/replay): move `render()`/`flush_segment`/`flush_*` into a replay submitter over `&CommandIR` + `&mut GpuResources` + `&PipelineSet`; `painter.render()` = record-finish + replay. Fixed flush order + non-SrcOver seal preserved exactly; external-image draws carry an **opaque handle id** (wgpu `TextureView` is non-PartialEq) so the IR stays comparable. | C5 (replay is a separate fn/type), C1 (painter drops <1500 here), C8 (full readback incl. Porter-Duff destructive modes + snapshot), C9, C10, C11 | `chief-architect` | 9, 3, 2 | ☐ |
| 11 | **Deterministic-replay test + ARCHITECTURE.md** (C5 gate, hardened): record an IR, **replay to encoder A and B, assert emitted command streams match** (not IR==itself); compile-time proof record holds no `&mut Device/Queue/Encoder`; negative test that replay does not mutate the IR. Document the two-level IR (Scene=`DisplayList`, Command=`CommandIR`). | C5 (non-tautological replay separation), C8, C9 + doc, C10, C11 | `chief-architect` | 10 | ☐ |

## Critical path
`6 → 7 → 8 → 9 → 10 → 11` — the only hard-serial chain (task 5 feeds 8/9). Every link is a real dependency: task 6 lands the safety net before the risky state cut; task 7's `GpuStateStack` is required for task 8's borrow-enforced `save_layer(&mut GpuStateStack)`; the whole 9→10→11 IR sub-program must follow 7+8 because the IR bakes exactly the transform/scissor/clip/opacity state those tasks extract (running it earlier re-edits every record method twice and reopens the borrow-split). **Tasks 1–5 are OFF the critical path (zero deps, parallelizable)** — land them first to bank C1 progress and put task 8's resource/pipeline facades in place.

## Cross-crate ripples
- **flui-app** (task 4 only): `Renderer::new` is called at runner.rs:125/451/668 + direct.rs:97. The `FLUI_BACKEND` env path needs **zero** flui-app edits; a `with_backend`/`RendererConfig` ctor makes those 4 sites **required** (not cosmetic) for the feature to be reachable — the api-design-lead decision in task 4 picks which, *before* build.
- **flui-rendering** (all tasks, read-only): the snapshot harness (`paint_fragment_snapshot.rs`/`pipeline_scenarios.rs`/`dpr_pipeline.rs`) is a **structural, CI** gate consumed unchanged — it does NOT validate GPU compositing.
- **flui-painting** (tasks 5/11, reference-only): the Scene IR (`DisplayList`/`DrawCommand`/`testing::record`) already exists (~80% of C5's Scene half) — referenced + tested against, never modified; the new deterministic-replay test targets the **Command** IR in flui-engine.
- **workspace `Cargo.toml`** (task 1): add `wgpu-profiler = "0.27"` (optional, excluded from the wasm32 target dep set).
- **benches/examples** (task 4, cosmetic-only): `benches/render_throughput.rs` BACKENDS mirror + example renderers may align with the override; not required for correctness.

## Safety-net model (critique fix #2 — load-bearing)
Two nets, **different layers**, do not conflate:
- **PIXEL** (replay correctness): flui-engine GPU-readback serial suite (`cargo test -p flui-engine --features enable-wgpu-tests -- --test-threads 1`). **LOCAL-GPU ONLY — not in CI** (no GPU runner). The implementer of every replay-touching task (6,7,8,9,10) **must run + diff it on a real GPU before merge**, and name the GPU.
- **STRUCTURAL** (scene/record correctness): flui-rendering snapshot harness — headless, CI-runnable, asserts LayerTree/DisplayList structure. Catches record-side regressions, **not** compositing.

## Critique resolutions (traceability)
- **#1 backwards edge** (T7a was blocked_by T6 but T6 owns the IR types) → IR data-type lift is now **task 5, before the compositor (task 8)**.
- **#2 false safety net** → *Safety-net model* above; spec C8 corrected (pixel=local-GPU-only vs structural=CI).
- **#3 no SDF-clip readback** + **scissor-asymmetry bug-or-design** → **task 6** lands the missing readback nets before task 7; cross-checks `.flutter/`.
- **#4a `layer_texture_pool` double-claim** → owned by **task 2 (GpuResources)**; task 8 borrows it.
- **#4b `texture_batch` seam** → owned by **task 9 (DrawBatcher)**; task 8 documents `flush_opacity_layer`'s use as a known re-touch.
- **#5 build matrix** → task 4 C9 scoped to native-target compile + CI per-OS.
- **#6/#8/#7 fake-pass tests** → task 7/8 real gate = task-6 readback diff (not the balance assert); task 11 replay test hardened (A/B streams + compile-time separation + no-mutate); task 1 Diagnosticable test uses a hand-built `Vec<GpuTimerQueryResult>`.
- **#9 C1 math** → **task 9 relocates the ~3000-LOC record methods** into `batches.rs`; C1 closes at task 10's final cut.
- **#10 flui-app scope** → task 4 decides env-only vs ctor up front.
- **#11 backdrop reorder** → task 1 adds a named backdrop-blur readback check; its renderer-wiring is not blindly self-merged.
- **#12 C4 enforcement** → task 9 adds a CI grep assertion (`batches.rs`/`pipelines.rs` import no `Matrix4`).

## Notes
- Per-PR gate for **every** task = the *Safety-net model* (pixel + structural) + C9 (fmt; clippy both modes incl. `--features enable-wgpu-tests`; nextest; `cargo check --release` 0-warn; wasm32 check; doc).
- C11 (immediates DROPPED) is a negative invariant across all tasks — no immediates/push-constant code; task 1's `TIMESTAMP_QUERY` + task 4's backend override are the only new device-init touches, both conditionally gated to keep the no-feature/no-override path byte-identical.
- Sign-off: `chief-architect` on tasks 7-11 (compositing core + IR hot path); `api-design-lead` on task 4 (new `Renderer` pub ctor). Tasks 1/2/3/5/6 self-merge after the per-PR gate.
- Governing decision not yet ADR'd — recommend `/adr` for the C-IR choice before task 7 starts.
