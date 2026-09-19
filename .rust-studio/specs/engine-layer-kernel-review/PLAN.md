# Kernel-style review of flui-layer, flui-engine (then flui-painting) — working plan

Started 2026-09-18. Lenses: SLOP / PATTERNS / entropy / conceptual integrity / pre-publication /
soundness / cancellation / orderings, plus four optics (dtolnay API surface, Matsakis types-as-
guarantees, Ralf Jung unsafe, withoutboats "which problem does this module solve").
Standing decision (memory `wgpu-is-the-engine-no-backend-swap-planned`): no Vello/Skia/software
backend; abstractions that exist only for a backend swap are deletion candidates.

## flui-layer — landed
Append-only `LayerTree` (`insert_root`/`push_child`), leader index on the tree, one translation
source (`Layer::local_translation`), `link.rs` follower resolution proven against Flutter 3.44.0
`_establishTransform`; `Scene{tree}`; `SceneBuilder<'a>`; `DamageTracker` → engine,
`PerformanceStats` → flui-app. Mapping decisions 1–7 in `crates/flui-layer/ARCHITECTURE.md`.

## flui-engine — Tier 1 (apply in this pass)
- [x] E1 `readback_dump`: no `env::set_var`; explicit `dir` parameter.
- [x] E2 `render_scene_content -> EngineResult`, end-frame maintenance on the error path.
- [x] E3 `GpuStackOrigin` gone; `Renderer{lease, config, painter, offscreen}` non-Option;
      `new_offscreen` + getters deleted.
- [x] E4 `SurfaceLease::release() -> Released` token consumed by `replace_surface`.
- [x] E5 `WakeGuard` publishes completion and retires the frame on unwind; `Drop for RasterOwner`
      breaks the hook cycle (Miri: 36 passed, 0 leaks).
- [x] E6 `RasterOwner::resize -> Option<SurfaceGeneration>` (zero axis mints nothing).
- [x] E7 `StateStack::concat` / `WgpuPainter::transform` — no decompose-then-recompose.
- [x] E8 `#[must_use]` on `WgpuPainter::size` / `viewport_bounds`.
- [x] E9 `GradientStop`/`ShadowParams` private fields; `Recoverability` not `non_exhaustive`;
      restore-underflow warn unconditional; warn-once on >8 gradient stops.
- [x] E10 `wgpu-backend` feature deleted; per-target wgpu entries non-optional (they were
      `optional = true`, which left the crate with NO backend → every GPU test panicked in
      `Instance::new`; caught by nextest, not by `cargo hack check`).
- [x] E13 shader-mask painter cache moved from `LayerDispatcher` (per frame) to
      `OffscreenRenderer::mask_painter` (cross-frame).
- [x] E12 `LayerTree::new(root)`, `root() -> LayerId`, `SceneBuilder` owns its tree,
      `Scene::default` = blank root. Landed across 25 files; workspace green.
- [x] E11 docs (ARCHITECTURE.md rewritten as current-state; markers removed; headless gaps
      stated; RasterOptions/FontLoader references fixed; RasterBackend restated as a test seam): ARCHITECTURE.md unsafe table, CONTRIBUTING claims, headless doc (what it does
      not render), runtime-contract.toml prose, ROADMAP FontLoader, platform `RasterOptions`,
      process markers (`wave 2`, `PR #110 F-W2-4`, `DECISION N`, `GO/NO-GO`, Mythos).
- [x] E17 "IR-purity witness" withdrawn (wgpu 30 handles derive Clone); consts on Clone kept
      as the `PooledTexture` bar only, docs say so.
- [x] E18 `wgpu` module flattened into the crate root; `pub use ::wgpu`; `effects`/`path_cache`
      private with root re-exports; external paths ported (flui-app, examples, benches, fixtures).
- [x] E19 `enable-wgpu-tests` → `testing` (workspace convention).
- [x] E20 `#![cfg_attr(not(test), deny(unsafe_code))]`; stale `SAFETY` markers on safe code removed.
- [x] Gates green 2026-09-18: fmt, clippy ×2, nextest workspace (10457), doctests, compile-fail,
      port-check, runtime-conformance, panic-policy, inventory, text-check, doc-strict workspace,
      hack each-feature, wasm32 + windows + macOS cross-checks.

## flui-engine — Tier 2 (needs a spec + user answers)
ALT-2 ordered IR with per-run gradient stop offsets; ALT-1 surface out of `Renderer`, one layer
walker; ALT-3 single lowering path; ALT-4 raster protocol to the consumer / drop the threaded
half (hinges on ADR-0045: threaded lane yes/no); `bind_surface` ctor param; `InFlightAccounting` family.
Open questions for the user: ADR-0045 threaded lane; headless as the canonical oracle;
`WgpuPainter` public draw API intent.

## flui-painting — next
Step 0 done: 28 files, ~11k LOC, `forbid(unsafe_code)`, `FONT_SYSTEM` global, `binding.rs`
RwLock caches + observer closures, public-api 2453 lines. Same lenses + optics.

## flui-painting — review landed 2026-09-18 (two lenses, claims verified by rg/compile)
Tier 1 (apply now, no design decision needed):
- [x] P1 delete `binding.rs` (ImageCache/CachedImage/ImageHandle/SystemFontsNotifier/PaintingBinding);
      `register_font` → free fn beside `shared_font_system()`; `error.rs` keeps one variant.
- [x] P2 delete `clip_context.rs` + flui-rendering re-export.
- [x] P3 `DisplayListCore`/`DisplayListExt`/`stats.rs` → inherent `len/is_empty/bounds/commands`
      (`bounds -> Option<Rect>`); delete `&mut` surface (iter_mut/IndexMut/AsMut/filter/map/
      with_opacity*/kind/CommandKind/is_*/has_paint/paint()).
- [~] P4 `#[non_exhaustive]` dropped, engine matches exhaustively (no `_ =>`), the dead
      `ShaderMask`/`BackdropFilter` command variants and the engine's second lowering for them are
      gone. The `DrawCommand { transform, op }` split is folded into Tier 2 (c) — doing it before the
      Path/Paint representation change would be double churn.
- [x] P5 delete the 36 zero-consumer Canvas methods (+ tests), one private `push_clip`,
      `measure.rs`/`detect.rs`/`LineInfo`/`TextLayout::new|with_overflow`, 4 ignored TextPainter
      settings, `did_layout`, testing wrappers, `prelude`.
- [x] P6 `RRect::contains` in flui-geometry (3 copies deleted); `Paint::eq` includes `shader`
      (delete `paints_equal`).
- [x] P7 docs: lib.rs front page rewritten; Sync claims fixed; ignored examples compile or go;
      scoped.rs panic claim; process markers; PERFORMANCE.md "Actual" column; MIGRATION.md;
      ARCHITECTURE.md current-state.
- [x] P8 gamed tests: save_layer_blend_paint_is_opaque, test_canvas_not_sync, contract_freeze count.
Tier 1 landed 2026-09-18: flui-painting 35 files, +730/−4746 (src 11.0k → 7.0k LOC); public-api
2453 → 1155 lines; workspace gates green.

Tier 2 (needs the user's yes): (c) IR ≤128 B (`Arc<[PathCommand]>` Path, Paint inline, one state
model) → (a) shaped-IR (DrawTextSpan carries glyph runs; engine stops shaping; ellipsis reaches
GPU) → (b) `FontContext` handle + `Shaper` scope, no OnceLock, `db_generation` as cache key.

## Tier 2 — architect ruling (chief-architect, 2026-09-18) and landing order
ADR-0065 (R1+R2, painting owns shaping; three font doors) — `docs/adr/ADR-0065-*.md`.
ADR-0066 (R3+R4, `DrawCommand { transform, op }`, COW `Path`, ≤192 B budget, gradients fold) — `docs/adr/ADR-0066-*.md`.
- [x] PR A (fonts): `shape`/`register_font`/`generation`; `with_mut`/`resolve_font(s)` gone; bundled
      faces in `flui_painting::fonts` (`bundled-fonts`, default on); engine never touches `db_mut`;
      caches key on generation; tests in `tests/font_registration.rs` + `fonts.rs`. Gates green.
- [x] PR B (wire, landed 2026-09-18): `Paragraph { Arc<TextLayout>, offset, color }`, `Canvas::draw_paragraph`,
      engine reads `TextLayout::buffer()`, engine shaper + caches deleted; the 7 named tests.
- [x] PR C (IR, landed 2026-09-18): `DrawCommand { transform, op: DrawOp }` (unit Save/Restore),
      COW `Path` in flui-types, `draw_command_fits_its_budget` (DrawOp 128 / DrawCommand 192 — was 560),
      `draw_picture` back, gradients folded into shader paints (engine `render_gradient*`, painter
      `draw_*_gradient_rect`/`draw_shadow_rect`, `GradientStop`/`ShadowParams` exports deleted; the
      fold is also a fix: per-corner radii, blend mode, transform), `display_list_record` bench,
      six replacement tests (per-corner readback test mutation-verified); ADR-0066.
- [x] PR D (landed 2026-09-18, ADR-0067): glyphon gone; `TextLayout::placed_glyphs` + `SharedFontSystem::rasterize`
      (painting names cosmic-text alone); engine `GlyphAtlas` (etagere pages, frame-stamped eviction) +
      `DrawSegment::glyph_batch`/`Phase::Glyph`; text ranges/claimed-text/gap passes/`seal_text_tail` deleted;
      `text_throughput` bench 470→390 µs steady, 762→656 µs cold; rounded-clip and mid-tone-colour readback fixes.
Parallel agent's landed fixes (verified in tree): `append_isolated` + composer, leader index checked
before mutation, `DisplayList` bounds recomputed on deserialize, `SavedState`, `clip_paths -> Vec<&Path>`,
`ShaderCache` deleted. PR C supersedes the `Save{IDENTITY}` payload and the custom `Deserialize`.

