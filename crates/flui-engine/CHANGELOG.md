# Changelog

All notable changes to `flui-engine` (the `wgpu`-backed GPU rendering engine) will be
documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

`flui-engine` is in active pre-1.0 development; breaking changes land freely until a
0.1.0 release is cut. The entries below capture the engine's evolution since the
**wgpu 25 → 29 migration** (the `0.1.0`-dev baseline).

### Added

- **GPU image filters** on the bounds-**growing** `DrawItem::Filter` seam
  (`ImageFilter::{Blur, Dilate, Erode, Compose}`): separable **anisotropic** Gaussian
  blur (premultiplied, sRGB-encoded, √3·σ kernel); premultiplied morphology with
  in-shader decal; record-time `Compose` flatten into one ordered pass chain;
  intermediates sized to the **grown content bounds** on an integer grid, not the full
  viewport. (#267, #268, #270, #273, #274)
- **GPU color filters** on the bounds-**preserving** `LayerFilter` chain
  (`ColorFilter::{Mode, Gamma, Matrix}`): the full 28-mode Porter-Duff/advanced blend
  for `Mode`, sRGB↔linear transfer for `Gamma`, 5×4 matrix for `Matrix`. The complete
  `ColorFilter` is reachable through the `push_color_filter` producer path. (#266, #269,
  #277, #278)
- **Tessellated anti-aliasing**: affine-SDF reroute for rect/rrect/circle/oval/arc with
  `fwidth`-based coverage, plus an **SSAA-offscreen-tile** path for arbitrary fills —
  including non-`SrcOver` shapes/paths. (#258, #259, #260, #262, #263, #265)
- **Advanced (dst-read) blend modes**: a dst-read compositor applying 15 advanced
  Porter-Duff/separable modes at shape / `saveLayer` / layer / gradient / image level,
  plus a **`COPY_SRC`-less present path** so advanced blend works on adapters whose
  surface lacks `COPY_SRC`. Per-draw `Paint.blendMode` for shapes (Porter-Duff). (#224,
  #251, #252, #254, #255, #256, #257)
- **C-IR record/replay architecture**: a `GpuReplay::submit` record/replay split,
  `command_ir` IR types, a **deterministic-replay A/B gate** + IR-purity witness, and a
  GPU timestamp profiler (`wgpu-profiler` 0.27). (#225, #242–#249)
- **Device-loss detection + recovery** on the renderer. (#217)
- Windowed GPU filter demos `examples/filter_demo.rs` and `examples/color_filter_demo.rs`
  (built via `SceneBuilder`'s programmatic filter producers). (#276, #278)

### Changed

- **Migrated to `wgpu` 29** (from 25), Rust 1.96, and a unified `cosmic-text`. (#174)
- **Decomposed the `WgpuPainter` god-object** into focused units: `PipelineSet`,
  `GpuResources`, `GpuStateStack`, `LayerCompositor`, and `DrawBatcher` (with a
  `batches/` submodule split). (#227, #228, #229, #231, #232, #233–#238)
- **`push_color_filter` now takes `&ColorFilter`** (was `&ColorMatrix`) and dispatches
  all variants to the layer-filter chain; `ColorFilterLayer` carries the full
  `ColorFilter`; `ColorFilter::Matrix` wraps the `ColorMatrix` newtype and `ColorFilter`
  is `#[non_exhaustive]`; `ColorMatrix` is now `Copy`. *(breaking — internal signatures
  only, no serialized-format change)* (#277)

### Fixed

- **Single source of truth for the W3C blend helpers + epsilon drift.** The
  non-separable blend leaf helpers (`hard_light`/`lum`/`clip_color`/`set_lum`/
  `sat`/`set_sat`) were duplicated across `mode.wgsl` and `advanced_blend.wgsl`,
  and had drifted: `mode.wgsl`'s `clip_color` used `1e-7` where the CPU oracle
  `Color::blend` + `advanced_blend.wgsl` use `f32::EPSILON`. Extracted them to one
  `blend_helpers.wgsl`, prepended via `concat!(include_str!(…))` at both pipeline
  sites, aligned the epsilon to the oracle. Adds a non-separable-on-translucent
  GPU-vs-oracle test (MO8); the 433-test readback baseline is unchanged.
- **~28 render-correctness bugs** surfaced by the wgpu-29 deep-audit rounds: sRGB
  encoding, group-opacity premultiplication, `ColorFilter` chroma, HiDPI offscreen
  (backdrop-filter + shader-mask resolved in device space), tessellation fill-rule /
  scale-aware tolerance / atlas gutter / size-overflow, and baking `current_transform`
  into tessellated geometry. (#218, #219, #220, #221, #222, #223)
- **Removed an unsound back-to-front occlusion cull** that blanked any layer drawn on
  top of opaque content (e.g. a filtered layer over an opaque background). (#276)
- `ColorFilter::Mode` is now composited **over** the image as a per-pixel blend, fixing
  a flush-bucket-order bug where the tint drew under the opaque image. (#241)
- **Advanced (dst-read) shapes/layers no longer leave stale pixels under partial damage.**
  An advanced-blend shape, SSAA path, or `saveLayer` whose `device_bounds` straddle a
  partial-damage scissor edge was clipped to the damage rect on its foreground but
  blended over its full bounds with `LoadOp::Load` + no scissor — writing the prior
  frame's backdrop in the out-of-damage slice (a damage-completeness gap: a dst-read
  shape's true dirty region is its full bounds). The renderer now detects such a
  straddle (`has_advanced_shape_straddling`) and schedules a full repaint on the next
  frame (1-frame self-heal; partial damage is currently unused so the transient is
  unobservable today).

### Performance

- Trimmed dev `debuginfo` (`debug = 1`) with an opt-in `dbg` profile for full type
  info. (#253)
- Smooth Win32 live-resize alongside the L2 AA-norm SSAA path. (#265)

[Unreleased]: https://github.com/flui-org/flui/compare/v0.1.0...HEAD
