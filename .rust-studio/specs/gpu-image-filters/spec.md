<!-- Rust Code Studio — feature spec. Acceptance criteria are what /spec-verify checks. -->

# Spec: Remaining GPU image/color filters (flui-engine)

- **Status:** Done (verified 2026-06-22 — see `verify-report.md`; PRs #267–#270, #273, #274)
- **Slug:** `gpu-image-filters`   ·   **Date:** `2026-06-21`   ·   **Owner:** `chief-architect`
- **Governing ADR:** recommend `/adr` — "GPU filters seam: bounds-behavior split + pinned color-space contracts" (the two PINNED contracts + the `DrawItem::Filter` vs `LayerFilter` split). PR #266 (color-matrix `LayerFilter` seam) is the predecessor.

## Problem

PR #266 landed the **bounds-preserving** color-matrix filter seam: a `LayerFilter::ColorMatrix([f32;20])` carried on the offscreen layer, applied as a ping-pong pass in `flush_opacity_layer` (`opacity_layer.rs:451`) between `render_layer_to_offscreen` and the composite, with a verified premultiplied unpremul→matrix→clamp→repremul contract (`color_matrix.wgsl`). `ImageFilter::{Matrix, ColorAdjust}` already route through it (`backend.rs:1583/1592`).

Everything else still **silently degrades to a `save_layer(WHITE)` passthrough + `tracing` warn**:

| Filter | Fallback site | Behavior today |
|---|---|---|
| `ImageFilter::Blur{sigma_x,sigma_y}` | `backend.rs:1555` | passthrough — no blur |
| `ImageFilter::Dilate{radius}` | `backend.rs:1565` | passthrough — no dilation |
| `ImageFilter::Erode{radius}` | `backend.rs:1574` | passthrough — no erosion |
| `ImageFilter::Compose(Vec<ImageFilter>)` | `backend.rs:1602` | passthrough — chain dropped |
| `ColorFilter::Mode{color,blend_mode}` | not wired to GPU pass | constant-color blend missing |
| `ColorFilter::{LinearToSrgbGamma, SrgbToLinearGamma}` | not wired to GPU pass | gamma transfer missing |

This is an audited GPU render core. "MVP reported as parity" is the recurring failure mode (see `docs/`/memory). Each filter must match the **observable Flutter/Impeller behavior** (Prime Directive #1), verified by GPU-readback against a CPU oracle, with translucent inputs as the teeth case — not a green "AA present"-style vacuous gate.

The orphaned shaders in `shaders/effects/` (`blur_horizontal.wgsl`, `blur_vertical.wgsl`, `dilate.wgsl`, `erode.wgsl`) are **math references, not drop-in passes**: the blur pair are **compute** shaders calling `textureSample` (illegal in a compute stage — needs implicit derivatives) and writing storage textures; the morphology pair are fragment shaders but sample **ClampToEdge** (Impeller uses **Decal** — see PINNED below) and carry a vertex-buffer layout instead of the synthesized-quad VS the seam uses.

## Goals / Non-goals

**Goals**
- GPU-implement `ImageFilter::{Blur, Dilate, Erode, Compose}` and `ColorFilter::{Mode, LinearToSrgbGamma, SrgbToLinearGamma}`, deleting every `save_layer`+warn fallback above.
- **Split the filter seam by bounds-behavior** (the reshaped architecture): bounds-preserving color filters stay on the `LayerFilter` seam; bounds-growing image filters ride a new `DrawItem::Filter`.
- Match the PINNED Flutter/Impeller color-space contracts exactly (morphology premultiplied + decal; blur premultiplied + sRGB-encoded; compose inner-first).
- Per-filter GPU-readback teeth tests with CPU oracles; deterministic-replay A/B coverage for filter layers (zero today).
- Delete/replace the orphaned compute blur shaders; reshape the morphology shaders to the seam.

**Non-goals (explicitly out of scope)**
- ❌ **Large-σ optimization (downsample prefilter / Dual-Kawase)** — `BlurQuality{Low,Medium,High}` is a free enum (`effects.rs:17`), **not a field on `ImageFilter::Blur`**, and nothing plumbs it from the widget/layer layer. box→gaussian≈gaussian under one API is a **silent parity divergence** — forbidden in an audited core. Ship **exact separable Gaussian only**; defer the optimization to a future *plumbed quality-field* task.
- ❌ **`TileMode` selection** — there is no `tile_mode` field on `ImageFilter::Blur`/morphology today. Impeller defaults `ImageFilter.blur` to **decal**; flui ships **decal as the single honest contract**, documented. Adding `TileMode` is a typed-API extension deferred to its own task.
- ❌ **Cross-crate filter-bounds growth into the render/layer tree** — bounds growth is **engine-local** (`DrawItem::Filter` grows from radius at record time, clips to layer bounds like Flutter). Threading filter-bounds-growth up into `flui-rendering`/`flui-layer` is a follow-up only if a clip-interaction bug surfaces. *(User-chosen Fork 1.)*
- ❌ Linear-light blur (Impeller blurs sRGB-encoded — see PINNED). No "make it better" color-management divergence.
- ❌ `ImageFilter::Shader` (runtime FragmentShader filter), drop-shadow, lighting, displacement — future filters; the seam is designed to admit them additively.
- ❌ `ColorFilter::Matrix` / `ImageFilter::{Matrix, ColorAdjust}` — already done in PR #266.

## PINNED contracts (read from the `.flutter/` Impeller reference — not assumed)

`.flutter/` is a symlink to `/c/Users/vanya/RustroverProjects/.flutter`; Impeller **is** present at `flutter/engine/src/flutter/impeller/entity/contents/filters/`. The following were read from source this session and are load-bearing.

1. **Morphology = PREMULTIPLIED, per-channel max/min, DECAL padding.**
   `impeller/entity/shaders/filters/morphology_filter.frag`: samples the texture directly (Impeller textures are premultiplied) and does `result = max(color, result)` (dilate) / `min(color, result)` (erode) with **no unpremultiply**. Dilate inits `f16vec4(0.0)`, erode inits `f16vec4(1.0)`. `morphology_filter_contents.cc` sets `SamplerAddressMode::kDecal` (transparent-black outside) and `BlendMode::kSrc`, separable via a `uv_offset` direction. Per-channel max on premultiplied **can** produce a pixel with `RGB > alpha` (an "invalid" premultiplied color) at a boundary — **Impeller does not guard it; flui matches** (loyalty to observable behavior). The orphaned `dilate.wgsl`/`erode.wgsl` use ClampToEdge → **WRONG**, must emulate decal (Fork 3, owned at build).

2. **Blur = exact separable Gaussian on PREMULTIPLIED, sRGB-ENCODED (NOT linear light).**
   `impeller/entity/shaders/filters/gaussian.frag`: `total_color += coefficient * texture(...)` — a coefficient-weighted sum of **premultiplied** texels, **no per-sample unpremultiply, no sRGB→linear**. `apply_unpremultiply` is a *final-pass-only conditional* (true only when `bounds_.has_value()`, an internal blur-of-opaque nicety), never per-sample. The slight midtone darkening from blurring gamma-encoded values **is** the Flutter behavior; flui ships it, documented. Anisotropy is free: `sigma_x` drives the H pass uniform, `sigma_y` the V pass.

3. **Filters GROW bounds.**
   `gaussian_blur_filter_contents.cc:799` `GetFilterCoverage` returns `input_coverage.Expand(padding)`; `morphology_filter_contents.cc:185` dilate `origin -= r; size += 2r` (erode shrinks). A full-viewport-clamped layer-filter pass structurally **cannot** represent spread beyond the layer rect → a blur halo would clip at the edge. This is the reason for the seam split.

4. **Compose = inner-first.**
   Dart `lib/ui/painting.dart:4404`: `ImageFilter.compose({outer, inner})` ⇒ `result = outer(inner(source))`. flui-types `ImageFilter::Compose(Vec<ImageFilter>)` is an *unlabeled ordered Vec*: pin the convention **`Vec[0]` is applied first (= innermost)**; a left-to-right fold (`acc = source; for f in vec { acc = f(acc) }`) is then correct. Flatten-at-record is depth-first left-to-right: `Compose([Compose([A,B]),C]) → [A,B,C]`.

## Approach

**Split the seam by bounds-behavior, not by filter kind.**

### Half 1 — Color filters stay on the bounds-PRESERVING `LayerFilter` seam

`ColorFilter::{Mode, LinearToSrgbGamma, SrgbToLinearGamma}` are per-pixel: output bounds == input bounds. They extend PR #266's seam additively.

- `LayerFilter` (`command_ir.rs:47`) **stays `#[derive(Copy)]`** — gains `Mode{color:[f32;4], blend:BlendMode}` and `Gamma(GammaDir)` (`GammaDir::{LinearToSrgb, SrgbToLinear}`), both POD.
- The layer field `filter: Option<LayerFilter>` (on `SavedLayer:140` / `PendingOpacityLayer:437`) becomes `chain: SmallVec<[LayerFilter; 4]>` (inline ≤4, the common case; heap-spills beyond). The single `if let Some(LayerFilter::ColorMatrix(..))` arm at `opacity_layer.rs:451` becomes a **fold**: `acc = layer_tex; for f in chain { acc = apply_filter(f, acc) }`. Empty chain → alias `layer_tex` directly (zero acquire, **bit-exact** with today's no-filter fast-path).
- Each color filter = **one** full-viewport REPLACE pass mirroring `apply_color_matrix` (`color_matrix/mod.rs:56`): acquire pooled tex → synthesized-quad VS → fragment → return. Premul contract: **unpremul → op → clamp[0,1] → repremul** (the color-matrix bracket verbatim; transparent-pixel `select(vec3(0), t.rgb/t.a, t.a>0)` guard).
  - **Mode** fragment mirrors `Color::blend(self=color, dst=pixel, mode)` (`color.rs:883`) per-pixel — the straight sRGB-encoded `[0,1]` blend equation for the 28 modes.
  - **Gamma** fragment applies the sRGB transfer fn per RGB channel (alpha untouched); non-linear, so **not** a 5×4 matrix — its own pass.

### Half 2 — Image filters ride a new bounds-GROWING `DrawItem::Filter`

`ImageFilter::{Blur, Dilate, Erode, Compose}` grow bounds. A dedicated top-level draw-order item, isolated at record time, rendered to a **content-bounds-sized (grown)** pooled intermediate, composited at its grown rect via the **existing** `DrawItem::OffscreenTexture` / `AdvancedShape` seam (`opacity_layer.rs:208`) — so the composite arm is unchanged (same "ride the existing seam" discipline as PR-3/4/5 advanced-blend).

- New `pub(crate)` IR: `DrawItem::Filter(FilterOp)` where `FilterOp { input: DrawSegment, filter: SmallVec<[ImageFilterPass; 4]>, content_bounds: Rect<Pixels>, grown_bounds: Rect<Pixels> }`. `ImageFilterPass` is the lowered, flattened form (`Blur{sigma_x,sigma_y}` / `Morph{radius, op}`). All POD + `Clone` (T11 purity witness — every IR neighbor derives `Clone`, none `Copy`).
- **Engine-local bounds growth (Fork 1):** at record time in `push_image_filter`, grow `content_bounds` by the filter radius (`grown_bounds = content_bounds ⊕ radius`), clipped to the layer bounds — matching Flutter's layer-clip behavior. Intermediate sized `ceil(grown_bounds.{w,h})`.
- **Blur** = two sub-passes ping-pong (H with `sigma_x`, V with `sigma_y`), exact separable Gaussian, premultiplied, sRGB-encoded, decal-sampled. Reshape the orphaned **compute** shaders to **fragment** (synthesized-quad VS, `textureSample` legal in fragment; `kernel_radius = ceil(3σ)` from a one-home helper; weight-normalized). Delete the compute originals.
- **Morphology** = two sub-passes (H, V), per-channel max/min, premultiplied, decal. Reshape the orphaned fragment shaders: synthesized-quad VS + decal emulation (Fork 3). `MorphOp::{Dilate,Erode}` is **one** `ImageFilterPass::Morph` variant + an op field (they differ only by max/min + init constant).
- **Compose** = fold the flattened chain inside one `DrawItem::Filter` (no nested IR, no GPU-side recursion). Inner-first per PINNED #4.

**Data flow:** `Backend::push_image_filter` (`backend.rs`) lowers the `ImageFilter` AST → for color filters, `save_layer_with_filter` pushing onto the `LayerFilter` chain (Half 1); for image filters, isolates the subtree into `DrawItem::Filter` with grown bounds (Half 2). Replay: color-filter chain folds in `flush_opacity_layer`; `DrawItem::Filter` renders the input to a grown intermediate, folds its passes, composites via the existing offscreen seam.

**Shared infra (both halves):** one pipeline struct per shader on `PipelineSet` (`BlurPipeline`, `MorphologyPipeline`, `ModePipeline`, `GammaPipeline`); `TexturePool::acquire` + RAII ping-pong; the synthesized-quad VS; the `color_matrix_filter_tests.rs` readback harness (extended per filter). **DRY one-home:** `kernel_radius(sigma)->u32` in a new `effects/mod.rs` (shared by the blur pass AND its CPU oracle); `pub fn srgb_to_linear/linear_to_srgb` extracted from the private nested copies in `color.rs:728/762` (shared by the gamma shader's reference AND its oracle).

### Reuse vs reinvent

| Reuse (sibling owns it) | New (must build) |
|---|---|
| `apply_color_matrix` pass shape (acquire→REPLACE quad→return) | `apply_filter` fold + `DrawItem::Filter` render path |
| Synthesized-quad VS (`color_matrix.wgsl` `vs_main`) | Mode fragment (mirror `Color::blend` 28 modes); Gamma fragment (transfer fn) |
| `TexturePool::acquire` + RAII ping-pong | content-bounds-grown intermediate sizing |
| `Color::blend` (Mode oracle), color transfer fn (gamma oracle) | `pub srgb_to_linear/linear_to_srgb` extraction; `kernel_radius` one-home |
| Gaussian math (`gaussian_weight`, 3σ) — from orphaned compute shader | **fragment** blur H/V (reshape from compute; fix `textureSample`; drop storage IO) |
| Dilate/erode max/min math — from orphaned fragment shaders | synthesized-quad VS + **decal emulation** + premul (no unpremul) |
| existing `OffscreenTexture`/`AdvancedShape` composite seam | `DrawItem::Filter` draw-order arm in `submit` + `render_layer_to_offscreen` |
| `color_matrix_filter_tests.rs` GPU-readback harness | per-filter readback teeth tests + filter-layer A/B replay test |
| `PipelineSet` registration pattern | `BlurPipeline`/`MorphologyPipeline`/`ModePipeline`/`GammaPipeline` |

### Alternatives considered

| Option | Trade-off | Why not chosen |
|--------|-----------|----------------|
| **All filters on the `LayerFilter` layer seam** (the original Approach 1) | one seam, max reuse, no new IR | **Rejected — cannot grow bounds.** `flush_opacity_layer` is full-viewport-clamped to the layer rect; blur/morphology `GetFilterCoverage` expand coverage by radius (PINNED #3) → halo clips at the edge. A bounds-preserving seam structurally can't carry a bounds-growing op. |
| **Dual-Kawase blur** (downsample/upsample mip-chain) | ~O(1)/level at large σ | **Deferred.** Approximate (not exact Gaussian → weakens the readback oracle, a parity divergence); anisotropy is a shader gap (Kawase is isotropic — `sigma_x≠sigma_y` needs non-square steps the orphaned shaders don't parameterize). Kept as a *future option behind a plumbed quality field*, not day-one. |
| **Nested-enum Compose** `LayerFilter::Compose(Box<[LayerFilter]>)` | matches the source AST 1:1 | **Rejected.** Forces `Copy`→`Clone` and pushes **recursion into the replay hot path**; pool-depth bound becomes AST-nesting-dependent. Flatten-at-record to a linear chain keeps `Copy`, keeps "IR = lowered linear data", and bounds the chain trivially. |
| **Nested save_layers for Compose** (one offscreen per filter) | simplest IR, no enum change | **Rejected.** N offscreens + N composites where the fold needs 2 ping-pong textures and 0 extra composites; defeats the lowered-linear-data principle; per-filter composite overhead. |

## Public surface & semver impact

Engine-internal — **0 external semver impact**. `LayerFilter` and the new IR are `pub(crate)` within the `wgpu` module. Public traits `CommandRenderer`/`LayerStateStack`/`Backend` are byte-identical.

- `LayerFilter` **stays `#[derive(Copy)]`** (Mode/Gamma variants are POD). **No `Copy`→`Clone` break anywhere.**
- New `DrawItem::Filter(FilterOp)` variant — **additive `pub(crate)`** (the `DrawItem` enum is engine-internal; not `#[non_exhaustive]`-relevant externally).
- `flui-types::styling::color` gains `pub fn srgb_to_linear(f32)->f32` + `pub fn linear_to_srgb(f32)->f32` — **additive minor** (extracted from existing private nested fns; one home, DRY). No other `flui-types` change.
- Breaking changes confined to flui-engine internals (active-dev, no external consumers) — **allowed**.

## Pre-code maintainer verdict: **ACCEPTABLE** (reshaped)

- **Crate ownership** — correct. GPU passes over GPU textures belong in flui-engine; the *type* + *oracle* live in flui-types/flui-painting. The one thing that could cross into `flui-rendering` (render-tree bounds growth) is explicitly **out of scope** (Fork 1 = engine-local).
- **Sibling reuse** — strong; the split *increases* reuse (color filters keep the whole PR #266 path; `DrawItem::Filter` rides the existing offscreen-composite seam). Strict-maintainer rejection if: `srgb_to_linear` is cloned into a shader constant instead of extracted (DRY); `kernel_radius` defined twice (pass vs test).
- **Preempted rejections** — no silent box≈gaussian (cut); no ClampToEdge where Impeller uses Decal (PINNED); no unpremultiply morphology (PINNED premul); no full-viewport clip of grown blur (split to bounds-aware item); no untested seam refactor (A/B replay test required); morphology is one variant + op field, not two.
- **Allowed breaking changes** — none needed (`LayerFilter` stays `Copy`).

## Acceptance criteria

*The checklist `/spec-verify` will prove. GPU-readback is local DX12 only (`--features enable-wgpu-tests -- --test-threads 1`), not CI — the implementer runs + diffs on a real GPU before merge for every slice. Each teeth test must fail without the change (red→green) and use a translucent input where a premul contract is claimed.*

- [x] **No fallback left** — the `ImageFilter::{Blur,Dilate,Erode,Compose}` `save_layer`+warn fallbacks are deleted; all four GPU-render. `ColorFilter::{Mode,LinearToSrgbGamma,SrgbToLinearGamma}` GPU passes (`LayerFilter::Mode`/`Gamma`) + fold arms exist and are oracle-verified. *(verified by: grep absence of warn strings + readback. **Follow-up:** the layer-seam **producer wiring** — `push_color_filter` accepts only `&ColorMatrix`; routing a layer-level `ColorFilter::Mode`/gamma through the new passes is deferred and documented in `command_ir.rs`. The image-path `ColorFilter::Mode`/gamma IS wired, CPU-baked.)*
- [x] **Orphaned compute blur shaders deleted/replaced** — `blur_horizontal.wgsl`/`blur_vertical.wgsl` (compute, illegal `textureSample`) removed; fragment replacements in tree. *(verified by: grep + file list)*
- [x] **Morphology premul + decal (PINNED #1)** — readback: a checkerboard of opaque + transparent → dilate fills by exactly `ceil(radius)` texels, erode clears; **decal boundary** test (transparent-black outside, not edge-clamped); **translucent** opaque-vs-half-alpha adjacency picks the premultiplied per-channel max/min (straight-color morphology diverges here). *(verified by: GPU readback vs CPU premul max/min oracle)*
- [x] **Blur premul + sRGB-encoded + anisotropy (PINNED #2)** — readback: half-alpha disc on transparent background has **no dark halo ring** (proves premul, not unpremul-before-blur); **anisotropic** σx=8,σy=2 spreads horizontally ≫ vertically (proves the two sigmas are not swapped/averaged); result matches a CPU separable-Gaussian oracle using the same `kernel_radius`/`gaussian_weight` source. *(verified by: GPU readback vs CPU Gaussian oracle)*
- [x] **ColorFilter::Mode non-separable on translucent** — `Modulate` on opaque white = identity (no double-apply); a non-separable mode (e.g. `Multiply`) on a **translucent** layer matches `Color::blend(color, pixel, mode)` per pixel. *(verified by: GPU readback vs `Color::blend` oracle — the pass is correct. **Follow-up:** layer-seam producer wiring, see "No fallback left".)*
- [x] **Gamma round-trip** — `SrgbToLinear` then `LinearToSrgb` ≈ identity within transfer-fn precision; translucent input → alpha unchanged, RGB transferred (proves unpremul/repremul brackets the transfer). *(verified by: GPU readback vs `pub linear_to_srgb`/`srgb_to_linear` oracle — the pass is correct. **Follow-up:** layer-seam producer wiring, see "No fallback left".)*
- [x] **Compose order + flatten (PINNED #4)** — `[Blur,Mode] ≠ [Mode,Blur]` (non-commuting, proves the fold sequences); `Compose([Compose([A]),B]) == [A,B]` (flatten-at-record); inner-first matches `Vec[0]` applied first. *(verified by: GPU readback)*
- [x] **Bounds growth (Fork 1)** — a blur near the layer edge spreads to `grown_bounds` (clipped to layer bounds, Flutter-like), not clipped to the unguarded content rect; intermediate sized to the **integer-aligned** grown content bounds (`floor(grown.min)→ceil(grown.max)`), not full viewport. *(Task 6, PR #274: `content_aabb` producer + integer-grid composite; verified by readback of edge-adjacent blur B5/B8 + sub-viewport `fb_dim` assertion B7. **Follow-up:** repositioning shadow/gradient/image content inside the grown intermediate — those kinds currently fall back to the full-viewport path, correct but no VRAM win.)*
- [x] **Deterministic-replay A/B for filter layers** — snapshot the IR before `GpuReplay::submit`, run, assert IR unchanged (T11 purity); two runs produce identical pixels for a filtered layer. *(verified by: A/B replay test on a `Filter([Identity])` layer. **Follow-up:** the A/B test covers the Identity pass; content-modifying passes (Blur/Morph) get implicit determinism coverage from their own GPU readbacks but not a dedicated A/B case.)*
- [x] **No-filter fast-path bit-exact** — the empty-chain branch in `flush_opacity_layer` remains a direct alias of `layer_tex` (no acquire, no pass); existing non-filter readback unchanged. *(verified by: diff-level assertion + existing readback green)*
- [x] **`LayerFilter` stays `Copy`; new IR is additive `pub(crate)`** — no `Copy`→`Clone`; `DrawItem::Filter` added; `flui-types` gains only the two `pub` transfer fns. *(verified by: API shape + grep)*
- [x] **DRY one-home** — exactly one `kernel_radius` and one `srgb_to_linear/linear_to_srgb`; tests cite the same source as the passes. *(verified by: grep)*
- [x] **Gates** — fmt; clippy both modes incl. `--features enable-wgpu-tests` `-D warnings`; nextest; doc (`-D warnings`); port-check; typos; PR #266 color-matrix readback suite stays green. *(ground-truthed per slice + on final main; local DX12 readback 432/0.)*

## Risks & open questions

- **Premul / color-space** — PINNED from Impeller source (morphology premul+decal, blur premul+sRGB). Residual risk is transcription error in the WGSL; the translucent-input teeth tests are the net.
- **Decal emulation (Fork 3, owned at build)** — wgpu has no `AddressMode::Decal`. Plan: bounds-test in the shader (`sample → vec4(0)` if uv outside the content rect), mirroring Impeller's `IPHalfSampleDecal` fallback for backends lacking decal. Decided at build time; tactical, not strategic.
- **Pool / VRAM peak** — content-bounds sizing bounds the area; ping-pong holds 2 live textures (blur, and the Compose fold, regardless of chain length); the dominant multiplier is **layer-nesting depth** (each nested filtered layer holds its own intermediate up the recursion). Pool (~16 slots) cannot exhaust by count; VRAM peak ≈ `2 × grown_area × max_nesting_depth`, **logged** at acquire (depth + area). No silent truncation; `debug_assert` + `tracing::warn` on chain overflow.
- **Slice-0 seam refactor** — touches the just-merged-and-verified color-matrix path, which has **zero** deterministic-replay coverage for filter layers. Mitigated by: bit-exact no-filter fast-path preservation + the new filter-layer A/B replay test + `LayerFilter` staying `Copy` (no purity-witness churn).
- **`DrawItem::Filter` arm order** — adding a draw-order variant touches the R1 arm-order invariant in `submit`/`render_layer_to_offscreen`. Mitigated by landing it as a **standalone seam slice** (Slice 0) before any filter logic (Fork 2).

## Slicing (the /spec-tasks plan — in order, each an independent /dev-task ending in /review + local GPU-readback)

0. **Slice 0 — the `DrawItem::Filter` seam + color-filter chain refactor (no user-visible filter).** Add `DrawItem::Filter(FilterOp)` (additive `pub(crate)`) + its arm in `submit`/`render_layer_to_offscreen` (renders input → grown intermediate → composites via existing offscreen seam; no filter math yet — identity pass). Convert the layer field `Option<LayerFilter>` → `SmallVec` chain; generalize the single color-matrix arm into the `apply_filter` fold (ColorMatrix the sole arm → pure refactor, PR #266 tests stay green). Preserve the no-filter fast-path **bit-for-bit**. Add the **filter-layer A/B deterministic-replay test**. Extract `pub srgb_to_linear/linear_to_srgb` in flui-types; add `effects/mod.rs` with `kernel_radius`. *(Riskiest structural change, isolated — the T9 standalone-pure-move discipline. Fork 2.)*
1. **Slice 1 — Morphology (`Dilate`/`Erode`).** Reshape the two fragment shaders (synthesized-quad VS, premul, **decal emulation** — Fork 3), `MorphologyPipeline`, `ImageFilterPass::Morph{radius,op}`, two-sub-pass render in `DrawItem::Filter`, grown bounds from radius. Decal-boundary + translucent teeth tests. Delete `backend.rs:1565/1574` fallbacks. *(Simplest ready shaders; first real filter through the new seam.)*
2. **Slice 2 — `ColorFilter::Mode`.** Mode fragment mirroring `Color::blend` (28 modes), `ModePipeline`, `LayerFilter::Mode`, one-pass fold arm, oracle teeth tests, wire the `ColorFilter::Mode` path. *(Bounds-preserving — Half 1 seam.)*
3. **Slice 3 — Gamma (`LinearToSrgbGamma`/`SrgbToLinearGamma`).** Gamma fragment (per-channel transfer, alpha untouched), `GammaPipeline`, `LayerFilter::Gamma(GammaDir)`, one-pass fold arm, round-trip teeth test. *(May merge with Slice 2 if scoped tight — both 1-pass color ops on the same seam.)*
4. **Slice 4 — Gaussian blur.** Reshape compute→fragment H/V, `BlurPipeline`, `ImageFilterPass::Blur{sigma_x,sigma_y}`, two-sub-pass render in `DrawItem::Filter`, exact separable Gaussian premul+sRGB+decal+anisotropic. Halo + anisotropy + oracle teeth tests. Delete `backend.rs:1555` fallback + the orphaned compute shaders. *(Largest single slice.)*
5. **Slice 5 — Compose.** Record-time AST flattening in `push_image_filter` (inner-first, depth-first); the fold over the `ImageFilterPass` chain inside one `DrawItem::Filter`. Order non-commuting + flatten + depth teeth tests. Delete `backend.rs:1602` fallback. *(Last — composes filters that must already exist.)*

## Review discipline (per slice)

`rust-builder` (test-first) → `rust-reviewer` + **`ce-kieran`** (soundness, mandatory on this audited core) → **chief-architect runs the GPU-readback serial on DX12** (the only pixel gate; not in CI). Capture the durable architecture decision to the Obsidian vault via `/remember` after Slice 0 + Slice 4 (blur color-space) land. Honesty bar: state implemented-vs-deferred per slice; never imply parity not verified against the readback oracle (anti "MVP reported as parity").

## Links

- **Predecessor:** PR #266 (color-matrix `LayerFilter` seam) — `color_matrix/mod.rs`, `color_matrix.wgsl`, `opacity_layer.rs:451`.
- **Reference (read this session):** Impeller `morphology_filter.frag`, `gaussian.frag`, `morphology_filter_contents.cc`, `gaussian_blur_filter_contents.cc`, Dart `painting.dart:4404` (under `/c/Users/vanya/RustroverProjects/.flutter/flutter/engine/src/flutter/`).
- **Sibling seams:** `.rust-studio/specs/tessellated-aa/SPEC.md` (the SSAA-tile → OffscreenTexture composite seam this rides), `.rust-studio/specs/flui-engine-overhaul/spec.md` (the C-IR record/replay seam).
- **Vault notes:** wgsl `mat4x4` column-major transpose (color-matrix uniform packing); tile-composite premultiplied contract; viewport-uniform-cant-change-mid-encoder (full-screen-quad passes only); advanced-blend dst-read compositor at the renderer layer.
- **Types:** `flui-types/src/painting/effects.rs` (`ImageFilter`, `BlurQuality`), `flui-painting/src/display_list/command.rs:379` (`ColorFilter`), `flui-types/src/styling/color.rs:883/728/762` (`Color::blend`, transfer fns).
