# Changelog

All notable changes to `flui-engine` (the `wgpu`-backed GPU rendering engine) will be
documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

`flui-engine` is in active pre-1.0 development; breaking changes land freely until a
0.1.0 release is cut. The entries below capture the engine's evolution since the
**wgpu 25 → 29 migration** (the `0.1.0`-dev baseline).

### Deleted (same pass)

- **`wgpu/multi_draw.rs`** (`MultiDrawBatcher`, `PipelineId`, `MultiDrawStats`).
  A pair of counters feeding one `debug_assertions` trace line; `PipelineId`
  was never read and three of `add_quad_draw`'s four parameters were
  discarded. `replay/flush.rs` logs the same numbers from the batches it
  already holds.
- **`EngineError::NoAdapter`** — not constructed anywhere; `AdapterRequest`
  replaced it and carries the wgpu diagnostic.
- **`GpuCapabilities`' per-question `bool`s** (and with them the
  `clippy::struct_field_names` suppression they required). The struct now
  holds the adapter's `Features`/`Limits` tables and answers
  `supports_push_constants()` / `supports_timestamp_queries()` /
  `supports_dual_source_blending()` / `supports_hdr()` as methods; four of
  the old fields were never read at all.

### Renamed (standard-term pass)

Names now follow the vocabulary the rest of the workspace already uses
(`flui_painting::Canvas`'s `draw_*`, `flui_types::painting::Clip`), and each
one states what the thing is without opening the file:

- **`wgpu/backend.rs`'s `Backend` → `layer_dispatcher.rs`'s `LayerDispatcher`.**
  The old name collided with `RasterBackend` (the real backend swap point)
  and `DebugBackend`, and described a per-frame `CommandRenderer` adapter as
  if it were a backend. Module renamed with the type.
- **`wgpu::texture_cache::TextureId` → `TextureKey`.** The crate had two
  `TextureId`s — this cache key and `flui_types::painting::TextureId` (an
  external texture's identity) — and `command_ir.rs` had to alias the latter
  to keep them apart. The cache key is now named for what it is.
- **`WgpuPainter`'s drawing methods all take `draw_`.** Fifteen took it and
  fifteen did not (`rect`/`rrect`/`circle`/`oval`/`line`/`text`/`rich_text`
  beside `draw_path`/`draw_image`/…). `Canvas` in `flui-painting` already
  spells all of them `draw_*`; `DrawBatcher` follows. `WgpuPainter::texture`
  is deleted rather than renamed — it duplicated `draw_texture` (same
  registry lookup, no `src` rect) and had no caller, and
  `DrawBatcher::texture` with it.
- **Gradient/shadow painters say what they draw**:
  `gradient_rect` → `draw_gradient_rect`, `radial_gradient_rect` →
  `draw_radial_gradient_rect`, `sweep_gradient_rect` →
  `draw_sweep_gradient_rect`, `shadow_rect` → `draw_shadow_rect`.
- **`WgpuPainter::clip_*` take `flui_types::painting::Clip`**, not a bare
  `bool`. On `clip_rect` the parameter was also dead (`let _ = hard;`); the
  enum is now the caller's contract and the painter's `debug_assert` states
  what it may not receive.
- **`RasterBackend`'s query methods are `#[must_use]`** (`is_device_lost`,
  `has_damage`, `size`), with the `Renderer` getters that mirror them.
  Discarding a device-lost or damage answer is always a bug.
- **Module names that described a fraction of their contents**:
  `traits.rs` → `command_renderer.rs` + `layer_state_stack.rs` (one trait
  each), `commands.rs` → `dispatch.rs`, `pipeline.rs` → `pipeline_cache.rs`
  and `pipelines.rs` → `pipeline_set.rs` (singular/plural was the only
  difference), `opacity_layer.rs` → `layer_offscreen.rs` (it owns all
  layer-to-texture rendering, not just opacity), `effects.rs` →
  `effects/{mod,gradient,shadow,blur}.rs`.

### Removed / Changed (public-surface review, 2026-09-17)

- **The public surface was narrowed to the entry points.** `CommandRenderer`,
  `LayerStateStack`, `LayerRender`, `Backend`, `dispatch_command`/
  `dispatch_commands`, `DebugBackend`, and `FontLoader` are no longer exported
  from the crate root or `flui_engine::wgpu`; the `flui_layer`/`flui_painting`
  re-exports (`Scene`, `Layer`, `LayerTree`, `Paint`, …) are gone with them.
  No consumer outside the crate named any of them — `flui-app` reaches the
  renderer through `RasterBackend`, and the layer walk/command dispatch is
  internal machinery. The embedder surface is `Renderer`, `WgpuPainter`,
  `HeadlessRenderer`, `WindowTarget`, `RasterBackend`, the `raster_owner` type
  family, `EngineError`, and the embedded `fonts` bytes.
- **`GpuServices` and `Renderer::from_offscreen_services` deleted** (ADR-0045
  decision 2). The offscreen-only value type had no production consumer — its
  every reader was its own test — and its windowed half cannot exist before
  `ReplaceServices` does. `Renderer::new` is no longer `#[doc(hidden)]`;
  recovery stays per-renderer until the shared stack and its owner-thread
  re-pointing step land together.
- **`RasterOptions` deleted** (ADR-0045 decision 6). `RasterOwner` stored it
  and never read it; the type shipped a `1..=255` range whose reachable values
  were `1..=2`. `PipelineDepth` replaces it with the threaded lane.
- **`EngineError::NoAdapter` and `SharedServicesNotRecoverable` deleted.**
  Neither was constructed anywhere in the workspace.
- **Dead public helpers removed**: `TextureCache::{get_or_load,
  with_memory_budget, set_max_memory_bytes, max_memory_bytes, stats,
  atlas_image_count, atlas_utilization, contains}`, `TexturePool::{release,
  stats, clear}`, `OffscreenRenderer::{with_caches, warmup,
  texture_pool_stats, clear_texture_pool}`, `ShaderCache::precompile_all`,
  `TextRenderer::cache_stats`, `Backend::{save_count, restore}`,
  `CommandRenderer::viewport_bounds`, `TextureAtlas::image_count`,
  `GradientStop`'s gradient-instance convenience constructors, and the
  `images`/`assets` features (which gated nothing after the texture-loading
  path they served was removed).
- **`CachedTexture` no longer keeps a second `Texture` handle** or its
  `width`/`height`: `wgpu::TextureView` already holds the texture alive, and
  nothing read the copies. The 1×1 atlas placeholder texture they required is
  gone with them.
- **The test-only `StubCache` eviction suite is gone.** It exercised a copy of
  `TextureCache`'s eviction logic rather than the logic itself; the real
  GPU readback tests remain.

### Fixed

- **`HeadlessRenderer::new` is `async`.** It was the crate's only production
  `pollster::block_on`: a blocking constructor beside `Renderer::new`, which
  is `async` and awaited. An embedder calling it from an async context stalled
  the executor thread for the whole acquisition. Both now have one shape, and
  `pollster` leaves `[dependencies]` for `[dev-dependencies]` — the library no
  longer picks a blocking strategy on the caller's behalf.
- **`GradientStop`'s `padding` field is private.** It is GPU alignment: the
  three floats are uploaded verbatim as the shader's `_pad0.._pad2`, so a
  caller could write arbitrary values into data the fragment shader reads.
  Construction goes through `new` / `from_rgba`, which zero it;
  `from_rgba` covers callers holding a channel array.
- **A `ShaderMaskLayer`'s blend mode is now honoured.** `render_masked`
  accepted a `blend_mode`, never used it, and every masked layer composited
  `SrcOver` — the accept-and-discard contract violation mapping decision 8
  names. The mode now rides on `DrawItem::OffscreenTexture` and selects the
  exact per-mode composite pipeline; `BackdropFilterLayer::blend_mode()` is
  threaded the same way instead of being dropped. Pinned by
  `an_offscreen_result_composites_with_its_own_blend_mode`.
- **`RasterOwner`'s `Drop` signals shutdown completion**, so a consumer parked
  in `run_until_shutdown` is released when the owner is dropped without an
  explicit shutdown instead of waiting on a lane that no longer exists.
- **In-flight and device-lost atomics use the orderings they argue for**:
  `Release` on the ticket decrement (the one that publishes retire work),
  `Relaxed` on the increment and on the device-lost flag (which carries no
  data), `Acquire` on the reads paired with the decrement.

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
- **Error model reshaped to typed `#[source]` variants.** `EngineError::TextRender` is
  now `TextRender(#[source] Box<dyn Error + Send + Sync + 'static>)` (was
  `TextRender(String)`); a new `TextPrepare(#[source] Box<dyn Error + Send + Sync +
  'static>)` variant boxes `glyphon::PrepareError`. The `text_render` constructor now
  takes `E: Error + Send + Sync + 'static` (was `Into<String>`); a new `text_prepare`
  constructor mirrors it. `Renderer::new`'s `window_handle()`/`display_handle()` errors
  are now boxed directly as `raw_window_handle::HandleError` (was stringified through
  `std::io::Error::other`), preserving the original error type in the source chain.
  *(breaking — pre-1.0; no shims/aliases per active-dev policy)*
- **`Recoverability` enum replaces `is_recoverable` / `is_fatal`.** The new
  `EngineError::recoverability() -> Recoverability` classifier is an exhaustive
  internal `match`, so a future variant cannot compile without a classification arm —
  closing the silent-third-bucket hole the two `bool` methods had (six variants fell
  into an undocumented bucket). `Recoverability` is `#[non_exhaustive]` and re-exported
  from the crate root. The single classifier consumer (`flui-app` direct mode) was
  updated to `e.recoverability() == Recoverability::Recoverable`.
- **New `SurfaceValidation` variant** for `wgpu::CurrentSurfaceTexture::Validation`.
  Previously mapped to `SurfaceLost` (Recoverable), which caused an **infinite retry
  loop** on a surface misconfig — `get_current_texture` kept returning `Validation`.
  Now classified `Unrecoverable`: `flui-app`'s `render_frame` drops the frame and logs
  at `error` level ("surface misconfig; external reconfigure required") instead of
  retrying. Reconfiguration is **not automatic** — `render_scene` only reconfigures in
  the `Outdated`/`Lost` arm, so a `SurfaceValidation` without an external trigger
  (window resize / surface recreate) drops + error-logs every frame until that trigger
  arrives. This stops the infinite retry; it is not a self-heal. *(breaking — pre-1.0)*
- **`raw-window-handle` `std` feature enabled** in `crates/flui-engine/Cargo.toml` so
  `HandleError` impls `std::error::Error` (the impl is `#[cfg(feature = "std")]` in
  raw-window-handle 0.6.2), allowing direct boxing into `SurfaceCreation`.
- **Hygiene:** `glam`/`bytemuck` pinned once in `[workspace.dependencies]`;
  `GpuFrameProfile`/`PassTiming` are `#[non_exhaustive]` (future telemetry fields stay
  additive); `wgpu-profiler` `compile_error!`-guarded on wasm32; `docs.rs` all-features
  metadata + `readme`; `#[allow(unsafe_code)]` → `#[expect(…)]` in `buffer_pool`.

### Removed

- **Four dead `String`-carrying `EngineError` variants** with zero production call
  sites: `ResourceCreation(String)`, `ShaderError(String)`, `PipelineError(String)`,
  `InvalidState(String)` — and their constructors `resource`, `shader`, `pipeline`,
  `invalid_state`. wgpu shader/pipeline creation is infallible at runtime (validation
  surfaces via `on_uncaptured_error`, not a `Result`); there was no typed error to wrap.
  *(breaking — pre-1.0)*
- **`EngineError::is_recoverable` and `EngineError::is_fatal`** — replaced by the
  exhaustive `Recoverability` classifier (see Changed). No deprecated aliases.

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
