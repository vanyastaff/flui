<!-- Rust Code Studio — task breakdown for the gpu-image-filters spec. Each task is implemented via /dev-task. -->

# Tasks: Remaining GPU image/color filters (flui-engine)

- **Spec:** [`spec.md`](spec.md)   ·   **Updated:** `2026-06-21`

> The spec's `## Slicing` section (Slices 0–5) **is** this skeleton — formalized below, not
> re-invented. The seam is split by **bounds-behavior**: bounds-preserving color filters extend
> PR #266's `LayerFilter` chain seam (Half 1); bounds-growing image filters ride a new
> `DrawItem::Filter` (Half 2). Every task is one `/dev-task` ending in `/review` + a **local
> DX12 GPU-readback** serial against a CPU oracle — the readback is **LOCAL-ONLY, not CI** (no
> GPU runner; per spec acceptance §). Each teeth test is red→green and uses a **translucent**
> input wherever a premultiplied contract is claimed (the anti "MVP-as-parity" net).
>
> **`chief-architect` sign-off is mandatory on Task 0 (the `DrawItem::Filter` IR seam + the R1
> flush-order arm) and Task 5 (Compose AST flatten + fold)** — the two structural changes to the
> audited record/replay core. `ce-kieran` is mandatory soundness review on every slice.

## Task list
*Status: ☐ todo · ◐ in-progress · ☑ done · ⊘ blocked.*

| # | Task (outcome) | Acceptance slice | Owner lead | Blocked by | Status |
|---|----------------|------------------|------------|------------|--------|
| 0 | **`DrawItem::Filter` seam + `LayerFilter`→`SmallVec` chain refactor (NO user-visible filter).** Add bounds-aware `pub(crate) DrawItem::Filter(FilterOp)` + its arm in `submit`/`render_layer_to_offscreen` (input → grown intermediate → composite via the existing `OffscreenTexture`/`AdvancedShape` seam; **identity pass only**, no filter math) — **respecting the R1 flush-order invariant**. Convert the layer field `filter: Option<LayerFilter>` → `chain: SmallVec<[LayerFilter;4]>` (on `SavedLayer`/`PendingOpacityLayer`); generalize the single color-matrix `if let` arm at `opacity_layer.rs:451` into the `apply_filter` **fold** (ColorMatrix = sole arm ⇒ pure refactor). **Preserve the no-filter fast-path BIT-FOR-BIT** (empty chain aliases `layer_tex`, zero acquire). Add the **filter-layer A/B deterministic-replay test** (zero coverage today). Extract `pub fn srgb_to_linear/linear_to_srgb` in `flui-types` (one home, from `color.rs:728/762`); add `effects/mod.rs` with `kernel_radius(σ)->u32`. | No-filter fast-path bit-exact; deterministic-replay A/B for filter layers; `LayerFilter` stays `Copy` + additive `pub(crate)` IR; `flui-types` gains only the 2 transfer fns; DRY one-home (`kernel_radius`); PR #266 readback suite stays green | `chief-architect` (IR seam — **sign-off**) | — | ☑ |
| 1 | **Morphology (`Dilate`/`Erode`).** Reshape orphaned `dilate.wgsl`/`erode.wgsl` to synthesized-quad VS + **decal emulation** (in-shader bounds-test ⇒ `vec4(0)` outside the content rect — wgpu has no `AddressMode::Decal`, Fork 3) + **premultiplied** per-channel max/min (dilate init `0`, erode init `1`; **no unpremultiply**, PINNED #1). `MorphologyPipeline` on `PipelineSet`; `ImageFilterPass::Morph{radius,op}` (**one** variant + op field, not two); two-sub-pass (H,V) render inside `DrawItem::Filter` with **grown bounds** from radius. Delete the `backend.rs:1565/1574` `save_layer`+warn fallbacks. | Morphology premul + decal (PINNED #1): checkerboard dilate fills `ceil(radius)` / erode clears; decal-boundary (transparent-black outside); translucent opaque-vs-half-alpha picks the premul max/min; bounds growth (Fork 1) — readback vs CPU premul max/min oracle | `systems-perf-lead` | 0 | ☑ |
| 2 | **`ColorFilter::Mode`.** `Mode` fragment mirroring `Color::blend(self=color, dst=pixel, mode)` (`color.rs:883`, 28 modes) per-pixel; **unpremul → blend → clamp[0,1] → repremul** bracket. `ModePipeline`; `LayerFilter::Mode{color:[f32;4], blend:BlendMode}` (POD, stays `Copy`); one-pass REPLACE fold arm on the Half-1 chain seam; wire the `ColorFilter::Mode` lowering path. | `ColorFilter::Mode` non-separable on translucent: `Modulate` on opaque white = identity (no double-apply); `Multiply` on a translucent layer matches `Color::blend` per-pixel — readback vs `Color::blend` oracle | `systems-perf-lead` | 0 | ☑ |
| 3 | **Gamma (`LinearToSrgbGamma`/`SrgbToLinearGamma`).** `Gamma` fragment applying the sRGB transfer fn per RGB channel (**alpha untouched**; non-linear ⇒ not a 5×4 matrix, its own pass), reusing the extracted `pub srgb_to_linear/linear_to_srgb` as the oracle source. `GammaPipeline`; `LayerFilter::Gamma(GammaDir::{LinearToSrgb,SrgbToLinear})` (POD); one-pass fold arm on the Half-1 chain seam; wire the gamma lowering path. | Gamma round-trip: `SrgbToLinear∘LinearToSrgb ≈ identity` within transfer-fn precision; translucent input → alpha unchanged, RGB transferred (unpremul/repremul brackets the transfer) — readback vs `pub linear_to_srgb`/`srgb_to_linear` oracle | `systems-perf-lead` | 0 | ☑ |
| 4 | **Gaussian blur.** Reshape orphaned **compute** `blur_horizontal.wgsl`/`blur_vertical.wgsl` → **fragment** separable H/V (synthesized-quad VS; `textureSample` legal in fragment; drop storage-texture IO); **exact** separable Gaussian, **premultiplied + sRGB-encoded** (no per-sample unpremul, no sRGB→linear — PINNED #2), **decal**-sampled, **anisotropic** (σx drives H, σy drives V); `kernel_radius=ceil(σ·√3)` (Impeller `kKernelRadiusPerSigma`; the Task-0 one-home helper, now coverage AND sampling extent), running-sum renormalized. `BlurPipeline`; `ImageFilterPass::Blur{sigma_x,sigma_y}`; two-sub-pass ping-pong inside `DrawItem::Filter` with content-bounds-grown intermediate. **Exact only — NO large-σ prefilter / `BlurQuality`** (cut). Delete the `backend.rs:1555` fallback **and** the orphaned compute shaders. | Blur premul + sRGB-encoded + anisotropy (PINNED #2): half-alpha disc on transparent bg has **no dark halo** (premul, not unpremul-first); σx=8/σy=2 spreads horizontal ≫ vertical (sigmas not swapped/averaged); matches CPU separable-Gaussian oracle; bounds growth + intermediate-size assertion — readback vs CPU Gaussian oracle | `systems-perf-lead` | 0, 1 | ☑ |
| 5 | **Compose.** Flatten `Compose(Vec<ImageFilter>)` AST **at record time** in `push_image_filter` (depth-first left-to-right, **inner-first** — `Vec[0]` applied first, PINNED #4); fold over the flattened `ImageFilterPass` chain inside **one** `DrawItem::Filter` (no nested IR, no GPU-side recursion). Delete the `backend.rs:1602` fallback. | Compose order + flatten (PINNED #4): `[Blur,Mode] ≠ [Mode,Blur]` (fold sequences); `Compose([Compose([A]),B]) == [A,B]` (flatten-at-record); deep-chain no pool exhaustion (ping-pong holds 2 live textures) — readback | `chief-architect` (fold/flatten — **sign-off**) | 1, 2, 3, 4 | ☐ |

## Critical path
`0 → (1, 2, 3 parallel) → 4 → 5`

Task 0 is the only hard prerequisite for everything (it lands the `DrawItem::Filter` seam, the
`SmallVec` chain fold, the `flui-types` transfer fns, and the `kernel_radius` one-home). Tasks 1
(morphology, Half 2), 2 (Mode, Half 1) and 3 (Gamma, Half 1) are **mutually independent** and
parallelizable once 0 lands. Task 4 (blur) blocks on **1** as well as 0 — it shares the
`DrawItem::Filter` **grown-bounds machinery** first proven by morphology (two-sub-pass H/V render
into a radius-grown intermediate), so morphology is the trial run for that path. Task 5 (Compose)
is last by definition — it composes filters that must already exist (1–4).

> Tasks 2 and 3 **may co-land in one `/dev-task`** if scoped tight — both are single-pass color
> ops on the same Half-1 chain seam (one-pass REPLACE fold arm, identical pass shape, differing
> only in the fragment). Kept as separate rows for acceptance-criterion traceability; the owning
> lead decides at build time. They do not block each other.

## Acceptance-criterion → task map
*Every spec `## Acceptance criteria` line, mapped to the task(s) that satisfy it. (`Gates` is
per-slice on all tasks.)*

| Spec acceptance criterion | Satisfied by |
|---|---|
| **No fallback left** — every covered filter GPU-renders; `backend.rs:1555/1565/1574/1602` + unwired Mode/gamma fallbacks deleted | 1 (1565/1574 morphology), 2 (Mode), 3 (gamma), 4 (1555 blur), 5 (1602 Compose) |
| **Orphaned compute blur shaders deleted/replaced** — `blur_horizontal/vertical.wgsl` removed, fragment replacements in tree | 4 |
| **Morphology premul + decal (PINNED #1)** | 1 |
| **Blur premul + sRGB-encoded + anisotropy (PINNED #2)** | 4 |
| **ColorFilter::Mode non-separable on translucent** | 2 |
| **Gamma round-trip** | 3 |
| **Compose order + flatten (PINNED #4)** | 5 |
| **Bounds growth (Fork 1)** — spread to grown_bounds (layer-clipped), intermediate sized to grown content | 0 (seam machinery + grown-intermediate sizing), 1 (first real grown filter), 4 (blur halo edge case) |
| **Deterministic-replay A/B for filter layers** (zero coverage today) | 0 |
| **No-filter fast-path bit-exact** | 0 |
| **`LayerFilter` stays `Copy`; new IR additive `pub(crate)`; `flui-types` gains only the 2 transfer fns** | 0 (IR + transfer fns; `Copy` preserved); 2 & 3 add POD `Mode`/`Gamma` variants under the `Copy` invariant |
| **DRY one-home** — one `kernel_radius`, one `srgb_to_linear/linear_to_srgb`; tests cite the same source | 0 (defines both homes); 3 (gamma oracle cites transfer fns); 4 (blur oracle cites `kernel_radius`) |
| **Gates** — fmt; clippy both modes incl. `--features enable-wgpu-tests` `-D warnings`; nextest; doc `-D warnings`; `just ci` green per slice; PR #266 readback stays green | every task (per-slice gate) |

## Cross-crate ripples
*Coordinated by `product-steward` so no leg of a ripple is dropped.*

- **`flui-types`** (Task 0 only): `styling::color` gains `pub fn srgb_to_linear(f32)->f32` +
  `pub fn linear_to_srgb(f32)->f32`, **extracted** from the existing private nested copies at
  `color.rs:728/762` (one home, DRY — must be extracted, not cloned into a shader constant or a
  second test copy). **Additive minor**, 0 other `flui-types` change. The gamma shader reference
  (Task 3) and its oracle both cite these — finish the extraction in Task 0 so Tasks 3/4 import a
  stable home, never a private duplicate. *(Strict-maintainer rejection if cloned instead of
  extracted, or if `kernel_radius` ends up defined twice.)*
- **flui-engine internal** (all tasks): `LayerFilter` and `DrawItem::Filter`/`FilterOp`/
  `ImageFilterPass` are `pub(crate)` within the `wgpu` module — **0 external semver impact**;
  `CommandRenderer`/`LayerStateStack`/`Backend` stay byte-identical. The `DrawItem::Filter` variant
  touches the **R1 flush-order arm-order invariant** in `submit`/`render_layer_to_offscreen` — that
  is why it lands standalone in Task 0 (Fork 2) before any filter logic, and why Task 0 carries
  `chief-architect` sign-off.
- **flui-painting** (Tasks 2/5, reference-only): `ColorFilter` (`command.rs:379`) and `ImageFilter`
  (`flui-types effects.rs`) are the **source AST** lowered by `Backend::push_image_filter` /
  `save_layer_with_filter` — read + lowered, never modified.
- **No `flui-rendering`/`flui-layer` ripple** — filter-bounds growth is **engine-local** (Fork 1):
  `DrawItem::Filter` grows from radius at record time and clips to layer bounds. Threading growth up
  the render/layer tree is explicitly out of scope (spec Non-goals); a follow-up only if a
  clip-interaction bug surfaces.
- **Obsidian vault** (post-Task-0, post-Task-4): capture the durable architecture decision via
  `/remember` after the seam split (Task 0) and the blur color-space contract (Task 4) land — per
  the spec's review discipline.

## Notes
- **Governing ADR not yet written** — spec recommends `/adr` for *"GPU filters seam: bounds-behavior
  split + pinned color-space contracts"* (the two PINNED contracts + the `DrawItem::Filter` vs
  `LayerFilter` split). Recommend writing it **before Task 0 starts** so the seam decision is
  recorded; Task 0 is the change it governs.
- **Review discipline (per slice)** = `rust-builder` (test-first) → `rust-reviewer` + **`ce-kieran`**
  (soundness, mandatory on this audited core) → **`chief-architect` runs the GPU-readback serial on
  DX12** (`--features enable-wgpu-tests -- --test-threads 1`; the only pixel gate, **not CI**).
  Honesty bar: state implemented-vs-deferred per slice; never imply parity not verified against the
  readback oracle.
- **Cut, explicitly (spec Non-goals — do not let creep back in):** large-σ optimization
  (downsample / Dual-Kawase) — `BlurQuality` is an unplumbed free enum, box≈gaussian is a silent
  parity divergence, deferred to a plumbed quality-field task; `TileMode` selection — decal is the
  single honest contract; linear-light blur — Impeller blurs sRGB-encoded; `ImageFilter::Shader`,
  drop-shadow, lighting, displacement; `ColorFilter::Matrix` / `ImageFilter::{Matrix,ColorAdjust}`
  (already shipped in PR #266).
- **No quick wins** — a partial ripple is not done. The `flui-types` extraction (Task 0) and every
  fallback deletion ride **with** their filter slice; nothing is "addressed later".
