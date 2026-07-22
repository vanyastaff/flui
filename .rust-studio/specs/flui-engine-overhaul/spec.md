<!-- Rust Code Studio — feature spec. Acceptance criteria are what /spec-verify checks. -->

# Spec: flui-engine architecture overhaul + wgpu-feature adoption

- **Status:** Draft
- **Slug:** `flui-engine-overhaul`   ·   **Date:** `2026-06-16`   ·   **Owner:** `chief-architect`
- **Governing ADR:** none yet (recommend `/adr` — the research-backed **C-IR** decision)

## Problem

`WgpuPainter` (`crates/flui-engine/src/wgpu/painter.rs`, **6410 lines, ~58 fields**) is a god-object holding ALL per-frame state: pipelines, caches, invariant-free `Vec` state stacks (transform / scissor / rrect-clip / rsuperellipse-clip / opacity at painter.rs:395-441) and compositor state (`SavedLayer`). After 5 rounds of correctness audits (see memory `flui-engine-wgpu29-audit-improve`) the *behavior* is solid, but the *structure* blocks maintainability, testability, and the capabilities every serious renderer has. **Market research established that flui is the only serious renderer — vs Impeller, vello, WebRender, Bevy, egui/epaint, GPUI — with no record/replay IR seam.** Separately, wgpu v20→v29 added `TIMESTAMP_QUERY` and the engine has zero GPU instrumentation.

## Goals / Non-goals

**Goals**
- Decompose `WgpuPainter` into ~6 cohesive **concrete** components (the market-validated seams).
- Establish an explicit **record/replay Scene/Command IR as data** (incrementally formalize the implicit `DrawSegment`→`flush_segment`), keeping wgpu concrete.
- Enforce save/restore **balance** structurally: depth-counter + `Drop` `debug_assert` on the two real nesting structures (state stack + compositor).
- Reduce `glam::Mat4` ↔ `flui_geometry::Matrix4` divergence to **one structural conversion edge** (the trait boundary); glam stays internal, comparison/tolerance untouched.
- Adopt `TIMESTAMP_QUERY` GPU profiling by **reusing `wgpu-profiler` 0.27** (bridge into `Diagnosticable`, feature-gated, graceful no-op).
- Finish runtime backend feature-flag wiring (`select_backend` override; ~80% already in Cargo.toml).
- **Preserve behavior exactly** (5-round-audited paths); **zero** public-trait semver change.

**Non-goals (explicitly out of scope)**
- ❌ `dyn GpuBackend` / device-trait abstraction (unearned; vello/Bevy stay wgpu-concrete with no regret).
- ❌ A 2nd GPU backend (skia/vello/software remain "Future").
- ❌ **immediates / push-constants for per-draw uniforms — DROPPED** (wgpu-28 rename; default `max_immediate_size` = 64 B, too tight for transform+paint; WebGPU-web pending → dual-path; market uses instancing + dynamic-offset/storage).
- ❌ wgpu on-disk `PipelineCache` as a cross-backend strategy (Vulkan-only in 29).
- ❌ Any change to paint output / behavior "improvements" (loyalty to audited behavior).
- ❌ Parallel command recording (the IR seam *enables* it later; not built now).
- ❌ A bespoke GPU profiler.

## Approach (C-IR)

`WgpuPainter` becomes a thin coordinator over cohesive concrete components:

| Component (module) | Responsibility | Reuse mandate |
|---|---|---|
| `GpuStateStack` (`state_stack.rs`) | transform / scissor / rrect-clip / rsuperellipse-clip stacks; depth + `Drop` assert; glam internal | convert to/from `Matrix4` ONLY at the trait boundary |
| `LayerCompositor` (`layer_compositor.rs`) | `SavedLayer` + **opacity stack** (compositor-cluster state, snapshotted in `save_layer` — painter.rs:93-96); explicit `save_layer(&mut GpuStateStack, …)` (borrow-enforced snapshot) | — |
| `PipelineSet` (`pipelines.rs`) | explicit in-memory `PipelineKey→RenderPipeline` cache | reuse `PipelineKey` + `blend_state_for()`; **cohesion only, NOT a "device-loss bug fix"** (refuted) |
| `GpuResources` (`resources.rs`) | facade over existing `TexturePool`/`BufferPool`/`TextureCache` | reuse as-is, RAII preserved |
| `CommandIR` + `DrawBatcher` (`command_ir.rs`/`batches.rs`) | explicit record→replay; clip/transform/opacity **baked into IR primitives at record time** (market pattern) | formalizes the existing `DrawSegment` |
| `GpuProfiler` (`profiler.rs`) | wrapper over `wgpu-profiler` 0.27 → frame/pass summary via `Diagnosticable`; feature-gated | reuse the crate, do not build |

**Data flow:** Scene (flui-layer) → `Backend` (`CommandRenderer`/`LayerStateStack`, **byte-identical**) → record into Command IR (state baked via `GpuStateStack` + `LayerCompositor`) → `DrawBatcher` batches → replay submits to wgpu via `PipelineSet` + `GpuResources`.

This is what both research lenses (Flutter/Impeller reference + Rust ecosystem) and the in-repo `.flutter/` Impeller source independently endorsed: keep the IR at the boundaries (Impeller: DisplayList in, flat Command vector out; it *deleted* its retained EntityPass tree in 2024), decompose into ~6 cohesive components, and treat wgpu as the portability layer (no device trait).

### Alternatives considered
| Option | Trade-off | Why not chosen |
|--------|-----------|----------------|
| B — generic-parameterize `WgpuPainter` | future-proof type seam | re-typing 6410 audited lines for a non-existent 2nd consumer (premature-abstraction-against-delicate-code) |
| ALT-1 + device trait | full HAL | "over-builds the half the market rejects"; HAL not earned (no 2nd backend) |
| ALT-2 — freeze the core | ~0 regression risk | "no respected Rust renderer resembles it"; the god-object survives |
| Pure-C' (no IR) | simpler | "insufficient — leaves flui the only serious renderer without batching/retention/parallel-encode capacity" |

## Public surface & semver impact

Internal to flui-engine. Public traits `CommandRenderer` / `LayerStateStack` / `Backend<'frame>` are **byte-identical → 0 semver**. New `pub` types are crate-internal (within the `wgpu` module). New dependency: `wgpu-profiler` (feature-gated, optional). Breaking changes are confined to flui-engine internals (active-dev, no external consumers).

## Acceptance criteria

*The checklist `/spec-verify` will prove.*
- [ ] `WgpuPainter` reduced to a thin coordinator; the ~6 components exist as separate modules, each ≲ 1500 lines. *(verified by: line counts + module map)*
- [ ] `GpuStateStack` enforces balance: `Drop` `debug_assert` on non-zero depth; a test proves an unbalanced save/restore panics in debug. *(verified by: test that fails without the assert)*
- [ ] `LayerCompositor` owns `SavedLayer` + opacity; `save_layer` takes `&mut GpuStateStack` (borrow-enforced snapshot); opacity is no longer a free field on the painter. *(verified by: API shape + test)*
- [ ] glam↔Matrix4 conversion occurs at exactly one structural edge; `batches.rs`/`pipelines.rs` do not import `Matrix4`. *(verified by: grep)*
- [ ] Explicit Command/Scene IR: record and replay are separated; a test records an IR and replays it deterministically. *(verified by: test)*
- [ ] `GpuProfiler` wraps `wgpu-profiler`, feature-gated, emits via `Diagnosticable`, no-ops gracefully when `TIMESTAMP_QUERY` is unsupported. *(verified by: test + feature-off build)*
- [ ] Runtime `select_backend` override works. *(verified by: test or doc)*
- [ ] **BEHAVIOR PRESERVED — two distinct nets, different layers:** (a) **PIXEL correctness** = the flui-engine GPU-readback serial suite (`enable-wgpu-tests`, `--test-threads 1`) — the round 1-5 value-bug gate; **LOCAL-GPU ONLY, not run in CI** (no GPU runner), so the implementer must run + diff it on a real GPU before merge for every replay-touching task (T7-T11). (b) **STRUCTURAL/scene correctness** = the flui-rendering snapshot harness (`paint_fragment_snapshot.rs` — *headless, no-GPU*, asserts LayerTree/DisplayList structure / `pipeline_scenarios.rs` / `dpr_pipeline.rs`), CI-runnable, but it does **NOT** validate GPU compositing. Risky cuts that lack readback coverage (nested `clip_rrect`/`clip_rsuperellipse` SDF baking, nested save+clip+restore scissor) get a characterization readback test landed **before** the cut. *(verified by: local GPU readback diff + CI snapshot)*
- [ ] Gates: fmt; clippy both modes (incl. `--features enable-wgpu-tests`) `-D warnings`; nextest; `cargo check --release` 0 warnings; wasm32 check; doc.
- [ ] Each item ships as its own PR; a failing-before test where behavior is added; per-PR characterization via the snapshot harness.
- [ ] item #6 (immediates) explicitly DROPPED with rationale recorded; no immediates code added.

## Phasing (recommended PR sequence — additive/mechanical first, risky state/compositor cut last)

1. **PR-1 `GpuProfiler`** (reuse `wgpu-profiler`) — additive, zero regression risk, demonstrates wgpu-feature adoption, builds confidence.
2. **PR-2 `GpuResources` facade** — mechanical extraction of pool/cache ownership.
3. **PR-3 `PipelineSet`** — explicit pipeline cache (cohesion).
4. **PR-4 Backend feature-flags** runtime override (~80% done) — small, contained.
5. **PR-5 `GpuStateStack`** + balance assertions — risky (hot paths), but the file is already smaller.
6. **PR-6 `LayerCompositor`** (`SavedLayer` + opacity) + explicit `save_layer` — riskiest (compositor coupling).
7. **PR-7 Command/Scene IR** formalization + `DrawBatcher` — the central seam, on top of now-clean components.

> Caveat (Flutter lens): the exact "formalize the IR" effort (PR-7) needs a code-level sizing pass before `/spec-tasks` estimates; the 5↔7 ordering may be refined.

## Risks & open questions
- IR effort unsized → code-level pass before `/spec-tasks`.
- Regression risk on audited paths → mitigation: GPU-readback serial + snapshot harness **per PR**; behavior-preserving extractions; additive-first sequencing.
- `wgpu-profiler` version-lag on future wgpu majors → isolated behind the `GpuProfiler` wrapper, feature-gated.
- glam↔Matrix4 → keep glam internal, do NOT touch identity/tolerance comparisons (avoid re-introducing a round-4/5-class transform bug).
- Borrow-split contention during extraction could force `Rc<RefCell>` → watch; if components share too much mutable state, re-evaluate the cut (do NOT paper over with interior mutability — the spirit of port-check applies).

## Pre-code maintainer verdict
**ACCEPTABLE** (after harsh-critic reshape). Concept owner = flui-engine. Reused: `TexturePool` / `BufferPool` / `TextureCache` / `PipelineKey` / `blend_state_for` / `Diagnosticable` / `wgpu-profiler`. Reinvented: nothing. Breaking changes confined to flui-engine internals. Strict-maintainer objections resolved: the device-loss bug-claim was removed (refuted — `recover()` rebuilds the painter), opacity sits in the compositor cluster, the per-PR gate is the real GPU-readback + snapshot harness (not a toy in-crate test), and the design is IR-in / device-trait-out.

## Links
- Memory: `flui-engine-overhaul-spec-cir.md` (this decision), `flui-engine-wgpu29-audit-improve.md` (the 5 audit rounds), `flui-future-proof-over-yagni.md` (the philosophy this scopes), `render-harness-2.0-design.md` (the snapshot gate).
- Research inputs: 3 market-research reports (Flutter/Impeller, Rust ecosystem, wgpu-tooling) produced 2026-06-16.
- Prior architect note: `.claude/agent-memory/rust-studio-chief-architect/adr-flui-engine-decomposition.md` (superseded — it predates the IR-in research flip).
