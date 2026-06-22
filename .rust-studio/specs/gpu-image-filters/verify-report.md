<!-- Rust Code Studio — spec verification report. Each criterion → pass/fail + evidence. -->

# Verify report: `gpu-image-filters`

- **Verified:** 2026-06-22 · **Verdict:** **COMPLETE** (with two documented follow-ups)
- **PRs:** #267 (seam+chain), #268 (morphology), #269 (Mode/Gamma), #270 (blur), #273 (Compose), #274 (grown-bounds sizing)
- **Pixel gate:** local DX12 GPU readback `cargo test -p flui-engine --features enable-wgpu-tests -- --test-threads 1` → **432 passed / 0 failed / 7 ignored** (NOT in CI; CI runs `--lib` without the feature).

## Gates (ground-truthed on final main, all green)
| Gate | Result |
|------|--------|
| `cargo clippy -p flui-engine -p flui-types --all-targets -- -D warnings` (no-feature) | 0 |
| `… --features enable-wgpu-tests -- -D warnings` | 0 |
| `cargo check --workspace --exclude flui-platform` | 0 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-engine --no-deps --document-private-items` | 0 |
| `cargo fmt --check` · `typos` · `scripts/port-check.sh` | 0 / 0 / 0 |
| GPU readback (local DX12) | 432 / 0 / 7 ignored |

## Criterion-by-criterion

| # | Criterion | Verdict | Evidence |
|---|-----------|---------|----------|
| 1 | No fallback left | **PASS** + follow-up | `ImageFilter::{Blur,Dilate,Erode,Compose}` `save_layer`+warn fallbacks deleted (grep clean); all GPU-render. Mode/Gamma GPU passes exist + oracle-verified. **Follow-up:** layer-seam producer wiring (`push_color_filter` takes only `&ColorMatrix`) deferred, documented in `command_ir.rs`; image-path `ColorFilter::Mode`/gamma IS wired (CPU-baked, `batches/images.rs`). |
| 2 | Orphaned compute blur shaders deleted | **PASS** | `blur_horizontal.wgsl`/`blur_vertical.wgsl` gone; `blur.wgsl` fragment replacement in tree (Glob confirmed). |
| 3 | Morphology premul + decal (PINNED #1) | **PASS** | `morphology_filter_tests.rs` M1–M6: premul-direct max/min oracle (`morph_oracle_premul`, independent CPU), decal=vec4(0) both ops, translucent discriminator M4. |
| 4 | Blur premul + sRGB + anisotropy (PINNED #2) | **PASS** | `blur_filter_tests.rs` B1 (no dark halo), B2 (σx=8/σy=2 anisotropy), B3 (independent `blur_oracle_premul` ±3 LSB), B4 (zero-σ identity). |
| 5 | ColorFilter::Mode non-separable on translucent | **PASS** (pass) + follow-up | `mode_filter_tests.rs` MO1–MO7 vs `Color::blend` oracle. **Follow-up:** layer-seam producer wiring (see #1). |
| 6 | Gamma round-trip | **PASS** (pass) + follow-up | `gamma_filter_tests.rs` GA1–GA6 vs `flui_types::styling::color::{srgb_to_linear,linear_to_srgb}` (one-home oracle). **Follow-up:** layer-seam producer wiring (see #1). |
| 7 | Compose order + flatten (PINNED #4) | **PASS** | `compose_filter_tests.rs` C1 (order discriminator alpha→R∘Blur ≠ Blur∘alpha→R), C2/C5 drive production `flatten_compose` (`pub(crate)`, no test duplicate), C3 6-pass heap-spill oracle. Inner-first verified vs `.flutter` `dl_compose_image_filter.cc:33-51`. |
| 8 | Bounds growth (Fork 1) | **PASS** + follow-up | Task 6 (#274): `content_aabb` conservative producer + integer-grid composite; B5/B8 halo extent, B7 sub-viewport `fb_dim`, B9 circle radius, B10 gradient→viewport fallback, B11 rect-only still sub-viewport, B12 clipped-rect rebased scissor. **Follow-up:** repositioning shadow/gradient/image inside the grown intermediate (currently full-viewport fallback — correct, no VRAM win). |
| 9 | Deterministic-replay A/B for filter layers | **PASS** + follow-up | `deterministic_replay_tests.rs` `filter_layer_identity_replay_is_deterministic_and_faithful` (byte-identical A/B + IR purity). **Follow-up:** A/B covers the Identity pass; Blur/Morph get implicit determinism from their own readbacks, not a dedicated A/B case. |
| 10 | No-filter fast-path bit-exact | **PASS** | `fold_layer_filter_chain` empty-chain returns `input_tex` directly (no acquire); color-matrix readback F1/F6 + B4 green. |
| 11 | `LayerFilter` stays `Copy`; new IR additive `pub(crate)` | **PASS** | `LayerFilter` `#[derive(…Copy…)]` (command_ir.rs:76); `DrawItem::Filter` + `FilterOp` additive `pub(crate)`; `flui-types` gained only `pub srgb_to_linear`/`linear_to_srgb`. (`ImageFilterSpec` dropped `Copy` in Task 5 — internal, not `LayerFilter`.) |
| 12 | DRY one-home | **PASS** | exactly one `kernel_radius` (effects.rs:407), one `srgb_to_linear`/`linear_to_srgb` (color.rs:1000/1025); gamma oracle cites the flui-types fns directly. |
| 13 | Gates | **PASS** | see Gates table. |

## Soundness review
Every slice carried a mandatory `ce-kieran` adversarial soundness review. Notable catches the standard gates missed: Task 1 (3 morphology parity bugs), Task 2+3 (132 readback failures from broken WGSL the builder never ran), Task 4 (orphaned compute shaders), Task 5 (C2/C5 tested a `flatten_compose` duplicate — fixed to drive production), Task 6 (façade producer → `content_aabb`; circle radius drop; P0 shadow/gradient/image mis-position regression → gated to viewport fallback). The recurring failure mode (builder reports "complete" against un-run readbacks / narrowed tests) was caught each time by orchestrator ground-truthing (cargo + DX12 readback) + adversarial review.

## Tracked follow-ups (out of scope, honestly deferred)
1. **Mode/Gamma layer-seam producer wiring** — route a layer-level `ColorFilter::Mode`/gamma through `LayerFilter::Mode`/`Gamma` (`push_color_filter` currently takes only `&ColorMatrix`). Passes + oracles exist; only the producer is unwired. (#1/#5/#6)
2. **Grown-intermediate repositioning of shadow/gradient/image** — Task 6 repositions vertices/rect/circle/arc; shadow/gradient/image filter content falls back to full-viewport (correct, no VRAM win). Extend `render_segment_to_grown_offscreen` to those kinds. (#8)
3. **Test strengthening (minor):** full-29-mode coverage for Mode; dedicated A/B replay for Blur/Morph; per-axis grown_bounds. (non-blocking)
