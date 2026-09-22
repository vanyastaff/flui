# FLUI rendering / text / layout-performance audit

Scope: `crates/flui-engine`, `crates/flui-layer`, `crates/flui-painting`, `crates/flui-rendering`,
`crates/flui-animation`, `crates/flui-widgets` (scroll), `docs/BETA.md`. Read-only, no code changes.

## 1. Engine: frame path and primitive coverage

**Frame path.** `flui-painting::display_list` records draw ops (`command.rs`, `command_ops.rs`) →
`flui-layer` (`link.rs`, `scene.rs`, `scene_snapshot.rs`) assembles the layer tree / scene graph →
`flui-engine::layer_walk.rs` walks it, `layer_dispatcher.rs` (62K) turns layer ops into GPU commands,
`layer_compositor.rs` (32K) and `layer_render.rs` (39K) manage compositing/offscreen passes, and
`renderer.rs` (223K, the largest file in the engine) owns the wgpu device/queue/submit and per-primitive
render-pass construction (rects, circles/arcs, glyphs, textures — each a dedicated `*_instanced.wgsl`
shader + pipeline). `raster.rs` and `raster_owner.rs` (198K) own the raster-thread-facing surface
lifecycle (device-loss/surface-recreation retries mentioned in BETA.md).

**GPU-native vs tessellated.** Rects, rounded rects/superellipses (`superellipse.rs`), circles/arcs,
glyphs and images are drawn with dedicated instanced WGSL shaders
(`crates/flui-engine/src/shaders/{rect,circle,arc,glyph,texture}_instanced.wgsl`) — GPU-native, no
CPU tessellation. Arbitrary paths go through `lyon` (`tessellator.rs`, 65K; `lyon = "1.0.16"` in
`flui-engine/Cargo.toml`) into triangle meshes drawn with `shape.wgsl`. **Present** (hybrid, matches
Skia/Impeller's own split), file: `crates/flui-engine/src/tessellator.rs`.

**Anti-aliasing.** Not MSAA: every render-pipeline/pass in `renderer.rs`, `pipeline_set.rs`,
`layer_offscreen.rs`, `headless.rs` is created with `sample_count: 1`, and `ssaa.rs` states explicitly
("Surface / sample-count invariant") that *"`sample_count` stays 1 everywhere. No stencil, no
`resolve_target`."* Instead, arbitrary-path AA is 2× **supersampling** (`ssaa.rs`, 65K): a path is
rendered into a 2×-resolution pooled texture, then box-downsampled 4-tap into the 1× premultiplied
tile (`SsaaDownsamplePipeline`). GPU-native primitives (rect/circle/arc) presumably use analytic
coverage in their own shaders (not verified line-by-line here). `pipeline_cache.rs` does carry unused
`MSAA_4X`/`MSAA_8X` bitflags — dead/aspirational, not wired to any pass. **Present but SSAA-based, not
MSAA and not fully analytic** — a legitimate but less common choice; unclear if it holds up under
heavy path-shape scenes (SSAA is 4x fill-rate + memory per masked path).

**Text rendering.** `cosmic-text 0.19` (`flui-painting/Cargo.toml`) does shaping (`layout.rs`, 47K) and
glyph rasterization (`SwashCache`), with FLUI's own glyph atlas (`flui-engine/src/atlas.rs`,
`glyph_atlas.rs`, 18K) and per-glyph instancing (`glyph_instanced.wgsl`). Font resolution/fallback is
an elaborate custom layer over cosmic-text's `fontdb`/`PlatformFallback`
(`text_layout/font_resolve.rs`, 81K — the single largest file in `flui-painting`), including an
`EmojiForbiddenFallback` wrapper to separate color-emoji fonts from the text fallback chain (implies
color/emoji font support is handled, at least for fallback routing — did not verify COLR/CBDT glyph
drawing itself). No subpixel-positioning or hinting-mode code was found in the sampled files; glyph
placement (`PlacedGlyph`, `glyphs.rs`) looks whole-pixel/atlas-tile based. **Present** (shaping +
atlas + fallback), subpixel-AA/hinting **not confirmed present**.

**Gradients.** `crates/flui-engine/src/effects/gradients/` with dedicated shader + pipeline +
`generated.rs`, plus extensive gradient test suites (`gradient_blend_readback_tests.rs` 30K,
`gradient_image_blend_tests.rs` 57K). **Present.**

**Shadows / blur.** `crates/flui-engine/src/blur/` implements a real separable Gaussian blur
explicitly modeled on Impeller's `gaussian_blur_filter_contents.cc` (√3·sigma kernel radius comment
citing Impeller's `kKernelRadiusPerSigma`), with H/V passes (`mod.rs` docs). This is a genuine blur,
not a faked drop-shadow (e.g., not just an offset dark rect). `effects/shadow.rs` (3.2K) composes it
for box-shadow. **Present, real blur.**

**Backdrop blur.** No `Backdrop`/`backdrop` hits anywhere under `flui-engine`, `flui-layer`, or
`flui-widgets`. **Absent.** (Flutter's `BackdropFilter` / iOS-style frosted glass has no counterpart.)

**Clipping.** `depth_stencil: None` everywhere clip-adjacent code touches pipeline descriptors
(`ssaa.rs`, `layer_offscreen.rs`, `headless.rs`, `renderer.rs`) — **no stencil-buffer clipping.**
`layer_dispatcher.rs` explicitly notes an unimplemented path-clip case: *"Honouring it needs the
machinery an exact path clip needs: a stencil pass... erase the content it asked to keep"* — i.e. exact
arbitrary-path clipping is a known gap; rect/rrect clipping is handled some other way (likely
scissor/coverage), confirmed present by the large `clip_layer_readback_tests.rs` (91K) test file, but
exact-path clip is **partial/absent** per the dispatcher's own comment.

**Transforms/3D.** Not directly audited; `state_stack.rs` (61K) and `layer_state_stack.rs` carry
transform state through the paint walk — 2D affine transform support is clearly present given the
scope of state-stack code; true 3D perspective (`Matrix4`, `Transform.rotate` along X/Y) was not
verified.

**Opacity layers.** `ssaa.rs` explicitly references "Phase B (advanced blend / opacity layers)" as an
already-landed phase sharing its pooled-texture invariant; `advanced_blend.wgsl` (12K) and
`layer_blend_tests.rs` (32K) back this. **Present.**

**Image drawing/sampling/mipmaps.** Every texture creation site found (`atlas.rs`, `headless.rs`,
`layer_dispatcher.rs`, `profiler.rs`) sets `mip_level_count: 1` — **no mipmap generation.** Minification
of large images (e.g. a photo scaled down in an `Image` widget) will alias/shimmer rather than filter
correctly; this is a real, user-visible gap versus Flutter/Skia which mipmaps by default.

**Custom shaders / fragment programs.** No `FragmentShader`/`custom_shader` hits anywhere in
`flui-engine`, `flui-painting`, or `flui-widgets`. **Absent** — no `FragmentProgram`/`AnimatedSampler`
equivalent (Flutter's `dart:ui` custom-shader API has no counterpart).

**Damage / partial repaint (issue #1037 class).** `crates/flui-engine/src/damage.rs` implements a
`DamageTracker` accumulator (single bounding rect, explicitly modeled loosely on Slint's up-to-3-rect
scissor) — but its own doc comment says it plainly: *"the consuming half of ADR-0061... Its producer
does not exist yet — every path calls `mark_full_repaint`... The seam message a producer will send is
`flui_layer::DamageRegion`."* **Scaffolded but not wired — every frame is a full repaint today.** This
is a first-class perf gap: FLUI cannot yet skip re-rendering unchanged screen regions (e.g., a static
app bar while only a list scrolls, or a blinking cursor in one `TextField`).

**Layer caching / raster cache / texture pool.** `texture_cache.rs` (22K), `texture_pool.rs` (19K),
`buffer_pool.rs` (18K), `uniform_pool.rs`, `pipeline_cache.rs` (59K), `path_cache.rs` (15K) all exist —
substantial pooling/caching infrastructure at the GPU-resource level. No evidence of a Flutter-style
*raster cache* (rasterize-once, reuse-as-texture for expensive static subtrees like `RepaintBoundary`
+ `RenderRepaintBoundary.toImage`-style caching) was found in the sampled files — the caching found is
resource-pool-level (buffers/textures/pipelines), not picture/layer-content caching. **Resource pools:
present. Content/raster cache: not found — likely absent or elsewhere; flag for follow-up.**

## 2. Text stack

- **cosmic-text 0.19** (`flui-painting/Cargo.toml:30`), shaping via `Buffer`/`Shaping`, fallback via a
  custom `EmojiForbiddenFallback: cosmic_text::Fallback` wrapper.
- **Global `FontSystem`**: `flui-painting/src/text_layout/layout.rs` defines `SharedFontSystem(Arc<Mutex<FontState>>)`
  guarding `{system: FontSystem, installed_families, scaler: SwashCache, db_generation}` behind one
  `parking_lot::Mutex`. This is a single process-wide lock around all shaping and font-database
  mutation ("the font system is taken per shape, never on the per-command path" — i.e. not held during
  paint dispatch, which mitigates but does not eliminate contention). Comments show real design
  attention to lock-ordering vs the raster thread ("no second lock whose acquisition order could
  invert against the raster thread's"). This matches the shape of the known cosmic-text
  single-`FontSystem`-per-process contention issue class (issue #1133 in the task's framing) — the code
  shows awareness and mitigation (single lock, held only for shape/register, not during raster), but
  the lock is still one mutex serializing all text shaping across the whole app; under concurrent
  text-heavy widgets (e.g. many live `TextField`s or a text-heavy scrolling list) this remains a
  contention point. No sharded/per-thread FontSystem pattern was found.
- **Font discovery / bundled default font**: `font_resolve.rs` (81K) does platform font-database
  scanning (`fontdb::Database`) plus a curated static/variable-face resolution algorithm (weight
  matching, family generic mapping). Whether a truly bundled fallback TTF ships in the binary (so text
  renders even with zero system fonts) was not directly confirmed from file names alone — flagged for
  follow-up (`crates/flui-painting/assets/`).
- **BiDi/RTL**: **no `bidi`/`unicode-bidi` dependency and zero `Bidi` hits anywhere under
  `flui-painting/src`.** `TextDirection` exists as a type (paragraph-level LTR/RTL flag,
  `flui_types::typography::TextDirection`) but there is no evidence of the Unicode Bidirectional
  Algorithm being run to reorder mixed-direction runs within a paragraph. **This corresponds to the
  issues #1080/#1115 class and appears to remain an open gap**: FLUI can label a paragraph RTL but
  likely cannot correctly interleave Arabic/Hebrew with embedded LTR runs (numbers, Latin words) the
  way Flutter (which uses `dart:ui`'s bidi resolver, itself backed by ICU/HarfBuzz-style logic) or
  Parley (which explicitly integrates `unicode-bidi`) do.
- **Grapheme clusters / word boundaries (#1131/#1132 class)**: no `unicode-segmentation`,
  `GraphemeCursor`, or `unicode_words` hits were found in `flui-painting`; `crates/flui-objects/src/text/editable.rs`
  has one incidental `word_boundary`-adjacent match tied to `TextAffinity::Downstream`, not a real
  Unicode word-break implementation. This suggests cursor movement / double-click-to-select-word and
  correct emoji-ZWJ-sequence cursor placement are **not backed by a real Unicode segmentation
  algorithm** — a functional and accessibility-relevant gap versus Flutter (`characters` package) and
  Parley/ICU4X (`unicode-segmentation`/`icu_segmenter`).
- **InlineSpan / WidgetSpan**: **present** — `flui_types::typography::InlineSpan` is used throughout
  `flui-painting/tests/rich_text_example.rs`, and `crates/flui-objects/src/text/paragraph.rs` documents
  "inline `WidgetSpan` children, text selection, semantics" directly, matching Flutter's `RichText`/
  `TextSpan`/`WidgetSpan` model.
- **Text scaling / accessibility scale, selection rendering**: `paragraph.rs` mentions "text
  selection" directly; a dedicated accessibility text-scale factor was not directly confirmed in the
  sampled files (flag for follow-up in `flui-semantics`).
- **Comparison**: FLUI's text stack (cosmic-text + custom fallback/atlas) is architecturally closer to
  Parley (both sit on top of a shaping engine — cosmic-text vs swash/rustybuzz — plus a custom
  layout/fallback layer) than to Flutter's ICU-backed `dart:ui`. The gap versus both is the same:
  **no BiDi reordering, no genuine grapheme/word segmentation** — Parley explicitly wires
  `unicode-bidi` and ICU4X segmentation; FLUI currently does neither, per the search above.

## 3. Layout/pipeline performance

- **Arena**: `crates/flui-rendering/src/pipeline/owner/subtree_arena.rs` exists per the task's target,
  alongside `tree.rs` (68K), `node.rs` (39K), `flags.rs` (38K, likely dirty/relayout-boundary bitflags),
  `sumtree.rs` (81K, a summary-tree structure — likely for efficient subtree-aggregate queries such as
  "any dirty descendant"), and `entry.rs`/`erased.rs` for type-erased render-object storage. This is a
  substantial, purpose-built arena — not a naive `Vec<Box<dyn RenderObject>>` tree.
- **Dirty tracking**: `state/dirty.rs` (18K), `notifier.rs`, `phase.rs`, and `scheduler.rs` (71K, by far
  the largest file in `flui-rendering/src`) implement relayout-boundary / repaint-boundary /
  dirty-phase machinery. The issue numbers named in the task (#1039/#1041/#1042/#1091/#1090) were not
  found as literal code comments (they may be closed/renamed in the tracker rather than left as
  comments) — could not directly verify field-granular inherited-dependency tracking (#1090's specific
  claim) from file names alone; `scheduler.rs`'s size (71K) is consistent with this being a heavily
  evolved, contention-prone area, but a fine-grained field-level `InheritedWidget`-style dependency
  diff was not confirmed present or absent from the sample.
- **Benchmarks**: real `criterion`-style benches exist under `flui-painting/benches`,
  `flui-rendering/benches`, `flui-engine/benches`, `flui-animation/benches`, `flui-interaction/benches`
  (5 files: pointer routing, tap detection, velocity tracking, pointer resampling, gesture arena),
  `flui-types/benches` (geometry/color/conversions), `flui-scheduler/benches`, `flui-testing/benches`,
  `flui-view/benches`. Coverage is broad across the stack, not a token gesture.
- **Recorded frame-time numbers** (`docs/BETA.md`, "Performance and resilience: the representative
  workload — 2026-09-22", `examples/workload_probe.rs`, `just macos-workload`): workload = `Scaffold` +
  `AppBar` + Material `TextField` + a **2,000-row `ListView::builder`**, 900×700 logical, 20 s of
  scroll (18 px/frame, bouncing) + 500 characters typed one/frame + 5 s idle. Host: MacBook Air (M1),
  macOS 27.0, 3440×1440 @ 100 Hz (10.0 ms period), release build.
  - **First run** (swapchain `desired_maximum_frame_latency: 1`, the value carried since ADR-0029):
    every phase pegged at **20.0 ms p50 — exactly half the panel's 100 Hz rate (50 fps)** — scroll p90
    20.3 ms / p99 24.9 ms / max 131.7 ms; typing p50 20.0 / p99 20.6 ms. Root cause: with 1 spare
    drawable, acquiring the next frame's swapchain image waits for the prior one to leave scanout, so
    any frame doing real work misses the next vsync — every frame, once there's real work. Bare platform
    frame pump (no rendering) hit 100.2 fps, isolating the regression to the render path, not input/OS.
  - **Fix**: bump `desired_maximum_frame_latency` to **2** (wgpu's own default) — restored to
    **9.998 ms p50, full 100 Hz panel rate**, both scroll and typing.
  - **Accepted run** (latency 2): scroll p99 10.10 ms (1,967 frames/20 s, 6 frames / 0.31% over budget,
    PASS vs ≤1% budget); typing p99 10.04 ms (500 frames, PASS); idle: 1 frame in 5 s (PASS, ≤5 budget);
    **RSS**: 253.1 → 185.0 MiB over the run (−26.9%, peak 253.6 MiB) — PASS vs ≤10% growth budget
    (measured from a 5 s baseline chosen specifically to exclude the ~200 MiB startup ramp: 75.6→201 MiB,
    +166%, driven by GPU stack init + glyph atlas + first layout, flat within 1% after 2.2 s).
  - Caveats stated in BETA.md itself: one host/display/build; the probe drives `AnimationController`
    ticks directly, not real OS input, so input-translation cost is untested; `PlatformWindow::refresh_period`
    is not exposed to app code (script reads it via CoreGraphics as a workaround) — meaning apps cannot
    query their own refresh rate today.
  - **Verdict**: the swapchain-latency finding is a real, previously-undiscovered halving-of-frame-rate
    defect that BETA.md itself frames as "a finding, not a pass" on first run — now fixed with numeric
    evidence, but only on one Apple-Silicon host at one display refresh rate; Windows/Linux/lower-end
    hardware frame-rate behavior for the same workload is unverified.

## 4. Scrolling

- **Virtualization**: `crates/flui-widgets/src/scroll/sliver_list.rs` — `SliverList` +
  `SliverChildBuilderDelegate` is explicitly documented as "lazy element-built sliver list… only builds
  children visible in the viewport plus a configurable cache margin," with same-frame settling
  (children built between layout passes so a newly-scrolled-in band paints in the same frame it
  appears, without needing a full extra frame — closely modeled on Flutter's lazy-sliver behavior).
  **Present**, file: `crates/flui-widgets/src/scroll/sliver_list.rs:1-40`; also `SliverFixedExtentList`,
  `SliverGrid`.
- **Scroll physics**: `crates/flui-widgets/src/scroll/scroll_physics.rs` implements both
  `ClampingScrollPhysics` (Android hard clamp, the default in `scrollable.rs`) and
  `BouncingScrollPhysics` (iOS-style overscroll + spring-back), explicitly modeled on Flutter's two
  built-ins. **Present.**
- **Overscroll indicators**: not directly located in this pass (no `GlowingOverscrollIndicator` /
  `StretchingOverscrollIndicator`-equivalent file found) — likely **absent or partial**, flag for
  follow-up in `flui-material`.
- **Scrollbar**: `crates/flui-widgets/src/scroll/scrollbar.rs` implements a track+thumb overlay
  explicitly modeled on Flutter's `ScrollbarPainter` (comment cites `widgets/scrollbar.dart` 3.44.0),
  with an explicitly stated gap: "no `ScrollbarTheme` look customization beyond `thumb_color`/
  `thumb_width`." **Present, partial theming.**
- **Keep-alive**: **absent** — `crates/flui-widgets/src/interaction/dismissible.rs` explicitly states
  in its own doc comment: *"No `AutomaticKeepAlive`. The oracle mixes in `AutomaticKeepAliveClientMixin`
  so a mid-flight `Dismissible` is not [torn down when scrolled off]…"* — i.e. FLUI has no
  `AutomaticKeepAlive`/`KeepAliveNotification` mechanism at all. This is a real functional gap: any
  widget needing to preserve state while scrolled out of the lazy-list viewport (video players, form
  fields inside a long list, `PageView` tabs) will be destroyed and lose state in FLUI today.
- **Nested scroll views / scroll-to-index**: not directly confirmed present or absent in this pass
  (`CustomScrollView` + `SliverMainAxisGroup`/`SliverPersistentHeader` exist, which are the building
  blocks Flutter's `NestedScrollView` is built from, but no dedicated `NestedScrollView`-equivalent
  file was found) — flag for follow-up.

## 5. Animation (`flui-animation`)

Comprehensive: `controller.rs` (120K, `AnimationController` — by far the largest file, evidencing deep
Flutter parity work including a 119K test file alongside it), `curve.rs` (49K, standard curve library),
`curved.rs` (17K, `CurvedAnimation`), `tween.rs`/`tween_types.rs` (8K/29K), `spring.rs` (8.3K, physics
spring simulation), `simulation.rs` (49K, general physics simulations — friction/gravity/spring classes
akin to Flutter's `physics` package), `switch.rs` (26K, likely `AnimatedSwitcher`-style transition
management), `vsync.rs` (50K, ticker/vsync provider — the `TickerProvider` equivalent), `compound.rs`
(13K, composed animations), `smoothing.rs`, `reverse.rs`, `status.rs`. This is the most mature-looking
subsystem in the audit — **present, high parity with Flutter's animation package.**

## 6. Memory / threads / async

- **Threading model**: layout/build/paint run on a single **frame thread** by design.
  `flui-scheduler/src/async_driver.rs` states explicitly: *"A single-threaded executor with no runtime,
  no thread pool… Futures are polled on the frame thread, in the gap between a frame's transient
  callbacks (animation ticks) and its persistent callbacks (build → layout → paint) — Flutter's
  `SchedulerPhase.midFrameMicrotasks`."* This is a deliberate Flutter-parity choice (Dart's single
  UI-isolate event loop), not an oversight. Separately, `crates/flui-engine/src/raster.rs` does
  `std::thread::spawn` (line 277) — a raster/GPU-submission thread does exist, distinct from the frame
  thread, matching Flutter's engine-side UI/raster thread split at a coarse level (not verified in
  depth). `async_driver.rs` also spawns worker threads at lines 1240/1953/2038 for other async
  plumbing (likely IO/asset decode offload) — not deeply audited here.
- **Tokio**: `tokio v1.53.1` and `tokio-util v0.7.19` are in the dependency tree of `flui` (confirmed by
  `cargo tree -p flui`), but no `#[tokio::main]`, `Runtime::new()`, or `tokio::spawn` call was found
  under `flui-app`, `flui-assets`, or `flui-hot-reload` in this pass — tokio is present in the tree
  (likely pulled in transitively by `wgpu`/`winit`/a networking or hot-reload dependency) but does not
  appear to be the app's own async runtime; FLUI's actual task driver is the hand-rolled
  single-threaded `AsyncDriver` above. This is good for keeping app logic off a thread pool, but means
  tokio's presence in the binary is largely "along for the ride" — a candidate for feature-trimming if
  it is not load-bearing.
- **Dependency counts**: `cargo tree -p flui -e normal --depth 1` → 13 direct workspace-internal deps
  (`flui-animation`, `flui-app`, `flui-foundation`, `flui-geometry`, `flui-interaction`,
  `flui-macros` (proc-macro), `flui-material`, `flui-painting`, `flui-rendering`, `flui-tree`,
  `flui-types`, `flui-view`, `flui-widgets`). `cargo tree -p flui --prefix none | sort -u | wc -l` →
  **348** total dependency-tree lines (crate@version pairs including duplicated major versions).
  Heaviest external deps by evident scope: **wgpu 30.0.1** (+ `wgpu-core`, `wgpu-hal`,
  `wgpu-naga-bridge`, `wgpu-types`, `wgpu-core-deps-apple` — the GPU stack), **winit 0.30.13**
  (windowing), **cosmic-text 0.19** (text shaping, pulls `fontdb`, `swash`, `rustybuzz` transitively),
  **lyon 1.0** (+ `lyon_path`/`lyon_geom`/`lyon_tessellation`/`lyon_algorithms` — path tessellation),
  **tokio 1.53.1** (+ `tokio-util`, present but apparently not the app's driver, see above).

## 7. Binary size / compile-time proxies

- **Proc-macro crates**: 12 in the `flui` dependency tree (`cargo tree -p flui -e normal | grep
  '(proc-macro)' | sort -u | wc -l`) — modest, not a bloat concern by itself.
- **wgpu features**: `Cargo.toml` (workspace) pins `wgpu = { version = "30.0", default-features =
  false, features = ["wgsl", "naga-ir"] }`; `flui-engine/Cargo.toml` then adds **exactly one** GPU
  backend per platform feature — `dx12` (Windows), `metal` (macOS), `vulkan` (Linux), plus `gles` and
  `webgpu` features defined for other targets — rather than compiling all backends into every binary.
  This is the right pattern for both binary size and compile time (single-backend-per-platform), and is
  explicitly commented as intentional ("Additive pass-throughs to wgpu's own API features; the right
  one for the...").

---

# Executive summary (for the caller)

**Rendering gaps ranked by user visibility** (most visible first):
1. **No damage/partial-repaint** (`flui-engine/src/damage.rs`) — every frame is a full repaint; the
   `DamageTracker` consumer exists but has zero producer wired up (own doc: "producer does not exist
   yet"). This caps power efficiency and worst-case frame cost on any static-mostly UI with one small
   moving part (cursor blink, small badge) — a real perf/battery issue, not just a nice-to-have.
2. **No mipmaps** (`mip_level_count: 1` everywhere) — downscaled images will shimmer/alias; visible
   immediately in any photo grid or avatar list.
3. **No backdrop blur** — frosted-glass UI (iOS-style sheets/nav bars) is simply unbuildable.
4. **Exact-path clipping is unimplemented** (dispatcher's own comment: needs a stencil pass that
   doesn't exist) — only some subset (rect/rrect, coverage-based) clips correctly today.
5. **No custom/fragment shaders** — no `dart:ui` `FragmentProgram` equivalent; any app wanting custom
   GPU effects (shimmer, custom gradients-as-shader, `AnimatedSampler`) cannot.
6. **No `AutomaticKeepAlive`** — state inside lazily-built list items is destroyed on scroll-out; will
   surface as "my video/form resets when I scroll" bugs.
7. No overscroll glow/stretch indicator and no confirmed `NestedScrollView` equivalent (lower
   confidence — not fully verified absent).

**Text-stack verdict**: solid foundation (cosmic-text 0.19 + a genuinely careful custom
font-fallback/atlas layer, real `InlineSpan`/`WidgetSpan` rich-text support, a deliberately-scoped
single font-system mutex not held during raster), but **two real correctness gaps**: no BiDi/RTL
reordering (mixed-direction text will not interleave correctly) and no genuine Unicode
grapheme-cluster/word-boundary segmentation (cursor movement and word-select likely break on emoji
ZWJ sequences and non-Latin scripts). Both gaps are the same shape as what Parley explicitly solves via
`unicode-bidi` + ICU4X segmentation, and what Flutter gets from ICU — FLUI currently has neither. This
is a beta-blocking correctness gap for any non-English/emoji-heavy app, not a performance concern.

**Six perf-architecture items to land before beta**, in priority order:
1. Wire a damage-region producer into `flui_layer::DamageRegion` so `DamageTracker` stops doing full
   repaints every frame (biggest architectural perf debt found).
2. The swapchain `desired_maximum_frame_latency` fix (1→2) is already landed and *numerically proven*
   on one Mac — it must be re-measured on Windows/Linux/lower-end GPUs before beta, since the underlying
   mechanism (drawable-acquire stall) is backend-general, not Metal-specific.
3. Add mipmap generation for image textures (aliasing is a correctness-adjacent perf/quality issue).
4. Verify field-granular inherited-dependency tracking (the #1090-class claim) actually lands in
   `flui-rendering/src/pipeline/owner/scheduler.rs` (71K, largest file in the crate) — could not confirm
   from names alone; a targeted rebuild-count test would settle it.
5. Decide tokio's fate — it's in the `flui` binary's dependency tree but no in-app usage was found;
   either document why it's load-bearing (transitive from wgpu/winit) or drop the feature pulling it in.
6. Expose `PlatformWindow::refresh_period` to application code — BETA.md's own workload script had to
   read it via a CoreGraphics side-channel because the facade doesn't expose it, which blocks apps (and
   FLUI's own future adaptive-refresh logic) from reading real display cadence.
