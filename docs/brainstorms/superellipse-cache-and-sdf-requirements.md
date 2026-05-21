---
date: 2026-05-21
topic: superellipse-cache-and-sdf
audit_source: docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md
audit_items: "Priority #4, Step 5 item 14"
predecessor_pr: "82 (ClipContext consolidation + RSuperellipse parity)"
scope: deep-feature
---

# SUPERELLIPSE_CACHE bounding + real iOS-squircle SDF clip

## Summary

Bound the unbounded `SUPERELLIPSE_CACHE` by mirroring the `PathCache` shape (`max_entries` cap, `last_used_frame` LRU-by-frame eviction, `advance_frame` call site) and migrate it from a thread-local static onto the `Painter` struct so the eviction call site is unambiguous. In the same batch, write the real iOS-squircle SDF clip in the wgpu fragment shader so `CommandRenderer::clip_rsuperellipse` produces pixel-perfect superellipse clipping instead of the current `clip_rrect` approximation fallback — closing the PR #82 follow-up gap. Workspace memory becomes bounded, SDF parity reaches Flutter.

## Problem Frame

PR #82 ([refactor(painting): ClipContext consolidation + RSuperellipse parity](https://github.com/vanyastaff/flui/pull/82), merged commit `a6094c00`) landed the `RSuperellipse` clip stack — `DrawCommand::ClipRSuperellipse` variant, `Canvas::clip_rsuperellipse` op, `CommandRenderer::clip_rsuperellipse` trait method — but left two open obligations explicitly tied to audit Step 5 item 14:

1. **Unbounded cache.** [crates/flui-engine/src/wgpu/layer_render.rs:71](../../crates/flui-engine/src/wgpu/layer_render.rs) declares `SUPERELLIPSE_CACHE` as `thread_local! { static SUPERELLIPSE_CACHE: RefCell<HashMap<SuperellipseKey, Path>> = RefCell::new(HashMap::new()); }`. Comments admit "rendering is single-threaded, so thread-local is the correct choice," but the `HashMap` is never evicted — entries accumulate as long as the application produces fresh superellipse keys. Under animation or user-driven variation (each frame producing new corner radii), this is a memory-leak vector: cache size grows monotonically over the application's lifetime. The audit's Priority #4 mandate is verbatim: "Mirror the `PathCache` shape (max_entries, last_used_frame, advance_frame eviction). Add a memory-budget metric so this never recurs invisibly. Add a stress test that emits 10k unique superellipse keys and asserts cache size stays ≤ max_entries."

2. **Approximation-only SDF.** `CommandRenderer::clip_rsuperellipse` ([crates/flui-engine/src/traits.rs](../../crates/flui-engine/src/traits.rs)) currently uses a default-implementation fallback that re-routes the clip request to `clip_rrect` against the superellipse's outer rect plus per-corner radii. The result is visually wrong for superellipses with strong corner curvature: a rounded-rect's elliptical-arc corners differ from an iOS-squircle's parametric `|x/a|^n + |y/b|^n = 1` curve (Flutter uses `n = 4`). The brainstorm prompt's own follow-up gap call-out and PR #82 doc-clarification commit (`85b3aa26`) both name this as the deferred work; the `flui-engine::wgpu::layer_render` already carries the squircle math used by `ClipSuperellipseLayer::render` (parametric form with 64 samples per corner), but that path tessellates to a `Path` for stencil-style clipping — it does not produce a pixel-perfect SDF clip.

The cost of leaving the divergence: the workspace ships an `RSuperellipse` API that, under the current default engine, renders identically to `RRect` — meaning the `clip_rsuperellipse` trait method is documentation theater for the immediate-mode call path. Worse, the cache memory leak is an unobserved long-running cost — bug-class invisible until profiling surfaces it.

## Requirements

**Line-number policy:** brainstorm cites paths and symbol names; line numbers are illustrative only. Implementer uses `grep` / `rg` on symbol names at edit time and ignores any cited line position.

**Cache bounding (mirror PathCache)**

- **R1**: Replace the thread-local-static `SUPERELLIPSE_CACHE` with a bounded `SuperellipsePathCache` struct owned by the `Painter` (or equivalent owning context — see Key Decisions). The struct shape mirrors `PathCache` in [crates/flui-engine/src/wgpu/path_cache.rs](../../crates/flui-engine/src/wgpu/path_cache.rs): `HashMap<SuperellipseKey, CachedSuperellipsePath>` + `max_entries: usize` + `current_frame: u64` + `hits/misses` counters + `EVICTION_THRESHOLD: u64` const matching PathCache's `120`. Cache entries carry a `last_used_frame: u64` field.

- **R2**: `SuperellipsePathCache::advance_frame` increments the frame counter and evicts entries whose `last_used_frame < current_frame - EVICTION_THRESHOLD`. Called once per frame from the Painter's render loop, matching the existing `PathCache::advance_frame` call site.

- **R3**: `SuperellipsePathCache::insert` evicts the LRU entry (by `last_used_frame`) when at `max_entries` capacity, matching `PathCache::insert` logic exactly.

- **R4**: Default `max_entries` of 256 (smaller than PathCache's 512 because typical UIs use fewer distinct superellipse shapes than distinct paths). Configurable via `SuperellipsePathCache::new(max_entries: usize)`.

- **R5**: `SuperellipsePathCache::stats() -> (u64, u64, usize)` returns `(hits, misses, current_entries)` matching `PathCache::stats`. Enables the memory-budget metric the audit asks for; observability is via `tracing::debug!` on eviction (same pattern as PathCache).

- **R6**: Stress test asserts 10k unique superellipse keys inserted in sequence leave cache size at exactly `max_entries` (256 default) — verifies the LRU-by-capacity eviction path works under the audit's stated stress scenario.

**SDF shader (real iOS-squircle clip)**

- **R7**: Write a WGSL fragment-shader SDF for the rounded-superellipse curve. Algorithm: per-pixel, compute `(|dx/rx|^n + |dy/ry|^n)^(1/n)` in the corner regions where `n = 4` (Flutter's iOS squircle); discard or alpha-attenuate pixels where the SDF value exceeds 1.0. Edge and center regions match the existing `clip_rrect` SDF (axis-aligned interior).

- **R8**: Add a shader uniform / constant-buffer slot for the superellipse parameters: outer rect (4 floats), per-corner radii (8 floats — x/y per corner), exponent `n` (1 float). Mirror the existing `current_rrect_clip: [f32; 8]` storage shape in `Painter` ([crates/flui-engine/src/wgpu/painter.rs](../../crates/flui-engine/src/wgpu/painter.rs)) — likely add `current_rsuperellipse_clip: [f32; 13]` or equivalent.

- **R9**: Override `CommandRenderer::clip_rsuperellipse` in `Backend` ([crates/flui-engine/src/wgpu/backend.rs](../../crates/flui-engine/src/wgpu/backend.rs)) to populate the uniform and apply the superellipse SDF clip via `Painter::clip_rsuperellipse` (new method, parallel to `Painter::clip_rrect`). The default fallback in the trait stays as the cross-backend safety net for backends that haven't implemented the shader override.

- **R10**: SDF clip stack: `rsuperellipse_clip_stack: Vec<[f32; N]>` for save/restore semantics, matching the existing `rrect_clip_stack` shape. `save()` / `restore()` push/pop entries; `clear` resets to `[0.0; N]`.

- **R11**: Visual smoke test: render a square-ish superellipse with significant corner curvature (e.g., `Rect { w: 200, h: 200 }`, radius 80) and assert that the resulting framebuffer pixels at a known "corner" sample location (e.g., 30px inside the corner) differ measurably from a `clip_rrect` of the same outer-rect-plus-radii. Lock in that the SDF is actually doing different math than the rrect fallback. Full golden-image regression infrastructure is out of scope (see Scope Boundaries).

**Layer-tree path preservation**

- **R12**: `ClipSuperellipseLayer::render` keeps using the path-tessellation route (`get_or_generate_superellipse_path` → `renderer.push_clip_path`). The layer-tree path is independent of the immediate-mode SDF override — both code paths coexist post-implementation. The bounded cache (R1-R6) serves the layer-tree path; the SDF shader (R7-R11) serves the immediate-mode dispatch path.

- **R13**: Existing `generate_superellipse_path` math at [crates/flui-engine/src/wgpu/layer_render.rs](../../crates/flui-engine/src/wgpu/layer_render.rs) (`n = 4`, 64 sample points per corner) stays the reference implementation; the new SDF shader produces visually equivalent output within fragment-shader sampling tolerance.

**Verification gates**

- **R14**: `cargo build --workspace` passes after each commit.
- **R15**: `cargo test --workspace --lib --tests` passes; the stress test (R6) and visual smoke test (R11) live in `flui-engine`'s test suite and pass.
- **R16**: `cargo clippy --workspace --all-targets -- -D warnings` passes after the final commit.
- **R17**: `bash scripts/port-check.sh -v` reports 7/7 institutional refusal triggers ok after each commit. (The bounded cache may need attention from Trigger 4 — "Mutex on dirty-list state in flui-rendering production code" — verify by reading the script. The cache is in flui-engine, not flui-rendering, so should be unaffected.)

## Acceptance Examples

- **AE1** (Covers R3): When `SuperellipsePathCache::insert` is called with the cache at `max_entries` capacity and the inserted key is new, the LRU entry (the one with the lowest `last_used_frame`) is removed before the new entry is added. After insertion, cache size equals `max_entries` exactly.

- **AE2** (Covers R6): When a test loops 10,000 times calling `cache.insert(unique_key, path)` with monotonically distinct keys, the cache size at the end equals `max_entries` (256 default). No `OutOfMemory` panic. No allocation growth beyond `max_entries * sizeof(entry)`.

- **AE3** (Covers R2): When 121 frames pass via `cache.advance_frame()` without re-accessing a previously-inserted entry, that entry is evicted (it does not appear in `cache.stats().2`). Matches PathCache's EVICTION_THRESHOLD behavior exactly.

- **AE4** (Covers R7-R9): When the immediate-mode dispatcher hits `DrawCommand::ClipRSuperellipse`, the wgpu pipeline binds the SDF shader uniform with the superellipse parameters and the fragment shader rejects pixels outside the iOS-squircle curve. The resulting framebuffer pixels at a known corner-curvature sample location differ measurably (e.g., RGB delta >5 on at least one channel) from the same sample location rendered via the `clip_rrect` fallback.

## Success Criteria

- `SUPERELLIPSE_CACHE` is bounded; long-running applications no longer exhibit monotonic cache growth.
- 10k-key stress test passes — audit Step 5 item 14's explicit ask.
- `CommandRenderer::clip_rsuperellipse` no longer routes to `clip_rrect` approximation in the wgpu backend; the SDF shader override fires and produces pixel-perfect superellipse clipping at fragment-shader resolution.
- The smoke test (R11) locks in that the SDF override is actually different math than the rrect fallback, so a regression to the rrect fallback would surface immediately.
- Mythos audit Step 5 item 14 + Priority #4 marked complete in the audit document's Step 5 / Priority list with commit-hash references — same annotation pattern as PR #82 U8.
- Existing `ClipSuperellipseLayer::render` continues to render correctly via the bounded layer-tree path.

## Scope Boundaries

**In scope:**

- The 17 requirements above. Bounded cache + SDF shader + stack management + override + tests + audit annotation.

**Deferred to Follow-Up Work:**

- **Full golden-image regression test infrastructure.** R11's smoke test asserts SDF produces *different* pixels than rrect; a comprehensive golden-image suite comparing rendered output against fixture PNGs is a separate infrastructure investment. Out of scope for this batch.
- **Cross-backend SDF parity.** The override lands only in `Backend` (wgpu); other backends (`DebugBackend`, `MockRenderer`, future Skia/Vello backends) keep using the default-impl fallback. Each backend's SDF override is a follow-up if/when that backend ships.
- **Performance benchmarking SDF vs path-tessellation.** R11 covers correctness; a Criterion benchmark comparing per-frame cost of SDF clip vs the layer-tree path-tessellation route is a follow-up.
- **PathCache improvements** beyond serving as the reference shape for `SuperellipsePathCache`. Any general-purpose cache enhancements land in their own scope.
- **Removing the layer-tree path-tessellation route.** Both paths (immediate-mode SDF + layer-tree tessellation) coexist post-implementation per R12. Unifying them later — if profiling shows the SDF route is universally better — is a separate decision.

**Outside this batch's scope:**

- **Other Mythos audit items.** SceneBuilder methods, PictureLayer hint fields, RendererBinding redesign, delegate trait visibility narrowing, Lyon tessellation feature-flag move, `pipeline.rs` / `pipelines.rs` consolidation, `Arc<Mutex<OffscreenRenderer>>` ownership review, RenderObject roadmap, production integration test for dirty-marking path. Each has its own brainstorm / plan iteration.
- **Adding a `n != 4` superellipse parameter.** The exponent `n` ships hardcoded to 4 (Flutter's iOS-squircle constant). Configurable per-corner exponents are a separate feature, not in this batch.
- **SDF for other primitives.** This batch only adds the rounded-superellipse SDF. SDFs for arbitrary paths, blob shapes, etc. are unrelated work.

## Key Decisions

- **Cache moves from thread_local-static onto an owning struct.** Mirror `PathCache` ownership exactly. The thread_local-static approach in the current code works because rendering is single-threaded, but the `advance_frame` call site is awkward without an owner — `PathCache` lives on `Painter` and is advanced in `Painter::render`; the bounded `SuperellipsePathCache` follows the same pattern. Eliminates a static-mutable smell while preserving thread-safety (each Painter has its own cache, same as PathCache today). Plumbing for `ClipSuperellipseLayer::render` to access the cache goes through a new `CommandRenderer::superellipse_path(rse) -> Path` trait method with a default impl that calls the existing `get_or_generate_superellipse_path` free function for backward compatibility — backends that own a `SuperellipsePathCache` override the method to consult their own cache.

- **Default `max_entries = 256`, not 512.** PathCache uses 512 because typical UIs produce many distinct paths (text glyphs, decorative shapes). Superellipses are much rarer in typical UIs (mostly platform-shaped icons or specific design-system surfaces); 256 entries comfortably covers most app's working set. Bound is configurable via `SuperellipsePathCache::new(max_entries)` for power-user tuning.

- **`EVICTION_THRESHOLD = 120` frames matches PathCache.** No reason to diverge; the same per-second budget (2 seconds at 60fps) applies to superellipse path lifetimes. If profiling later shows superellipse paths benefit from a different threshold, tune in a follow-up.

- **SDF shader uses `n = 4` hardcoded.** Matches Flutter's iOS-squircle constant. Configurable exponent would be a future API extension; today's scope is parity, not generalization.

- **SDF override lives in `Backend::clip_rsuperellipse`**, not in the trait default. The default-impl fallback in `CommandRenderer::clip_rsuperellipse` (currently routes to `clip_rrect`) stays unchanged — that's the cross-backend safety net. `Backend` overrides for wgpu. This preserves the contract for `DebugBackend` and `MockRenderer` (which want the simpler fallback) without forcing them to implement the SDF shader.

- **Layer-tree and immediate-mode paths stay separate.** `ClipSuperellipseLayer::render` keeps using `push_clip_path` against a tessellated path (with the now-bounded cache). The immediate-mode `Backend::clip_rsuperellipse` uses the SDF shader. Both produce visually equivalent output (within sampling tolerance) but via different mechanisms. Unifying is deferred — the layer-tree path's `push_clip_path` semantics integrate with stencil-style clipping that the SDF approach doesn't fully replace.

- **`SuperellipseKey` struct stays as-is** (12-field f32-bits representation). Already designed to be Hash + Eq + Clone; works as the cache key. Only the cache's ownership and bounding shape changes.

- **Atomic-commit-per-finding shape.** Same precedent as PR #81 (U1-U5) and PR #82 (U1-U8). Likely commit boundaries: (a) introduce `SuperellipsePathCache` struct + tests, (b) plumb the cache onto `Painter`, (c) migrate `get_or_generate_superellipse_path` callers, (d) delete the thread_local-static, (e) write WGSL SDF shader + wire uniform, (f) override `Backend::clip_rsuperellipse`, (g) SDF smoke test, (h) Mythos audit annotation. Final commit count likely 10-12 atomic commits.

## Dependencies / Assumptions

- **PathCache shape is the canonical reference.** Verified at brainstorm time — [crates/flui-engine/src/wgpu/path_cache.rs](../../crates/flui-engine/src/wgpu/path_cache.rs) carries `EVICTION_THRESHOLD = 120`, `max_entries`, `hits`/`misses` counters, `current_frame`, `last_used_frame`, `advance_frame` with retain-by-threshold and LRU-by-capacity eviction, and `stats()` accessor. `SuperellipsePathCache` mirrors this shape unchanged.

- **`SuperellipseKey` is already Hash + Eq.** The existing struct at [crates/flui-engine/src/wgpu/layer_render.rs:27](../../crates/flui-engine/src/wgpu/layer_render.rs) derives `Hash, PartialEq, Eq` with f32-to-bits representation. Works as a HashMap key under the new bounded cache.

- **wgpu 25.x is workspace-pinned** per CLAUDE.md ("Stay on 25.x, 26.0+ broken"). WGSL syntax targets 25.x; if a later wgpu adds a syntax change, this plan stays compatible. SDF shader is plain WGSL with no version-specific features.

- **`Painter::current_rrect_clip` and `rrect_clip_stack` patterns are the model for superellipse clip storage.** Verified at brainstorm time — Painter exposes both `current_rrect_clip: [f32; 8]` and `rrect_clip_stack: Vec<[f32; 8]>`. The superellipse equivalents follow the same shape with a wider tuple (rect + per-corner radii + exponent).

- **`flui-engine::wgpu::layer_render::generate_superellipse_path` math is correct.** No verification of the iOS-squircle parametric form is in scope here — the existing implementation is the reference. SDF shader implements the same `n = 4` curve at fragment-shader resolution.

- **`CommandRenderer::clip_rsuperellipse` default-impl fallback stays.** R9 explicitly preserves the trait default; only `Backend` overrides. This avoids forcing `DebugBackend` and `MockRenderer` to implement the SDF shader.

- **No external consumers depend on `SUPERELLIPSE_CACHE` being thread_local-static.** Verified by workspace-wide grep: only `flui-engine::wgpu::layer_render` references the cache directly. The migration to Painter ownership is internally-scoped.

## Outstanding Questions

### Deferred to Planning

- **[Affects R10][Technical]:** Exact tuple shape for `current_rsuperellipse_clip` — `[f32; 13]` (4 rect + 8 radii + 1 exponent) vs a typed struct `RSuperellipseClipUniform { ... }`. Planner decides at edit time based on how Painter's other uniform shapes look. Tuple is simpler; struct is more readable.

- **[Affects R9][Technical]:** Whether `Backend::clip_rsuperellipse` override populates the uniform and ALSO applies an outer-rect scissor clip for early rasterizer rejection (matching how `Painter::clip_rrect` calls `self.clip_rect(rrect.rect)` after setting the SDF uniform). Default: yes, same pattern as rrect — saves fragment-shader work outside the bounding box.

- **[Affects R11][Technical]:** Where the visual smoke test runs — inline in `flui-engine`'s test suite (CPU-side comparison of `DrawCommand` output, not real GPU rendering) vs as an integration test requiring a wgpu device. Planner verifies which test infrastructure already exists in flui-engine (the `tests/` directory) and routes accordingly. Default: inline + CPU-side, requiring a wgpu device for tests is out of scope.

- **[Affects R7][Needs research]:** Whether the SDF shader can share code with the existing `clip_rrect` SDF (parameterized by exponent: `n=2` is the rrect ellipse, `n=4` is the squircle) or needs a separate shader pipeline. Planner reads the existing `clip_rrect` SDF shader and assesses code-share feasibility. Default: separate shader pipeline if the existing rrect SDF is hardcoded for `n=2`; shared otherwise.
