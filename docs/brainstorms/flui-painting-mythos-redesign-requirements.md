---
date: 2026-05-20
topic: flui-painting-mythos-redesign
origin: docs/designs/2026-05-20-mythos-flui-painting-redesign.md
---

# flui-painting Mythos Redesign — Requirements

## Summary

Apply the Mythos architectural lens + the full 14-step refactor methodology established by the `flui-rendering` chain (PR #77 on `main`, merged commit `03774584`) and the `flui-layer` chain (PR #78 on `main`, merged commit `a78cdd69`) to `crates/flui-painting/`. The crate is ~12,321 LOC and sits between `flui-rendering` (whose paint phase calls `Canvas` methods to record drawing operations) and both `flui-engine` (which pattern-matches every `DrawCommand` variant for GPU lowering via wgpu) and `flui-layer` (which wraps `DisplayList` in `Layer::Picture` and `Canvas` in `Layer::Canvas`). Phase 1 investigation surfaced one fully dead trait (`WarmUpCanvas`, 0 impls, only used as a parameter type for a 1-impl trait method that does nothing real), one decorative subsystem (`ShaderWarmUp` whose `execute()` body literally documents "in a real implementation, we'd create an offscreen canvas here"), four god modules totalling 7,972 LOC (`canvas.rs` 3,305, `display_list.rs` 2,434, `text_layout.rs` 1,243, `text_painter.rs` 990), one obsolete companion doc (`docs/MIGRATION.md` migrating between non-existent crate versions), and one unnecessary `#[cfg(feature = "text")] mod inner` indirection inside `text_layout.rs`. The crate is `#[forbid(unsafe_code)]` at `lib.rs:151` so the net unsafe delta is **0** -- distinct from the `flui-layer` chain's −39 net delta. The design verdict at `docs/designs/2026-05-20-mythos-flui-painting-redesign.md` resolves these into a 14-step implementation plan; this brainstorm encodes the user-story / requirements layer that drives that plan.

---

## Problem Frame

The Mythos chain on `flui-rendering` (PR #77) and `flui-layer` (PR #78) has established the methodology, the verdict shape, the plan template, the per-crate `ARCHITECTURE.md` instance, and the `scripts/port-check.sh` enforcement. Today the methodology covers `flui-foundation` (grafted), `flui-rendering` (templated 2026-05-20, exemplar), and `flui-layer` (templated 2026-05-20). The next crate that earns the same treatment is `flui-painting`, because it (a) is the Canvas-recording crate that `flui-rendering`'s paint phase emits into, (b) is the source of every `DrawCommand` variant that `flui-engine`'s wgpu backend exhaustively pattern-matches for GPU lowering, (c) is wrapped by `flui-layer`'s `Layer::Picture` and `Layer::Canvas` for compositing, and (d) carries the same kind of dead-trait + god-module + cargo-cult-port shape that motivated Mythos in the first place.

Without the Mythos pass, the crate carries:

- A `WarmUpCanvas` trait (4 abstract methods, 0 production implementations, exists only as the parameter type for `ShaderWarmUp::warm_up_on_canvas(&self, canvas: &mut dyn WarmUpCanvas)`). Pure dead code surface.
- A `ShaderWarmUp` trait with 1 production impl (`DefaultShaderWarmUp`) whose entire purpose is to bootstrap shader compilation to avoid jank, but whose `execute()` body documents "in a real implementation, we'd create an offscreen canvas here." The subsystem is decorative -- present, exported, plumbed through `PaintingBinding::with_shader_warm_up` and `set_shader_warm_up`, but does no real work.
- A 3,305-LOC `canvas.rs` mixing the `Canvas` struct + 7 transform methods + save/restore/save_layer state stack + 6 clip methods + 4 clip query helpers + 29 `draw_*` primitive methods + 12 `with_*` scoped operations + 5 multi-canvas composition methods + finalization + reset/clear + query helpers. Eight distinct concerns in one file.
- A 2,434-LOC `display_list.rs` mixing `PointerEvent` + `HitRegion` + `HitRegionHandler` + sealed-trait module + `DisplayListCore` + `DisplayListExt` + 4 blanket impls + `DisplayList` struct + `DisplayListStats` + 29-variant `DrawCommand` enum + the 240-LOC `with_opacity` pattern match + the 250-LOC `bounds` pattern match + accessor methods + `apply_transform` + `CommandKind` enum. Same 8-concerns problem.
- A 1,243-LOC `text_layout.rs` whose entire body is wrapped in an unnecessary `#[cfg(feature = "text")] mod inner { … }` indirection (the cfg should sit on the mod declaration in `lib.rs`). Mixes RTL/LTR detection + `TextLayoutResult` + `LineInfo` + `TextLayout` wrapping cosmic-text + measurement helpers + the FontSystem global.
- A 990-LOC `text_painter.rs` mixing `TextPainter` + `TextBaseline` + paint integration + measurement + the `DEFAULT_FONT_SIZE` constant.
- A `docs/MIGRATION.md` documenting migration "from 0.0.x to 0.1.x" of a crate that never had a 0.0.x release. Obsolete documentation.
- A `restore()` that silently no-ops on empty save stack AND a `finish()` that silently `tracing::warn!`s about unrestored saves. Neither path catches the programmer error during test runs.
- `Paint::clone()` per `Canvas::draw_*` call (~80-200 bytes), per-`DrawCommand` 64-byte `Matrix4` baking, additional `Path::clone()` per `draw_path` / `clip_path` / `draw_shadow` (Vec<PathCommand> heap alloc), plus `Box::new(Path::clone())` for `clip_path` variant uniformity. Documented allocation hot spots; deferred to Outstanding refactors per the no-quick-wins memo (real benefit needs measurement first).

The shape is exactly what Mythos was designed to catch: a Flutter / Skia-inspired Canvas + DisplayList + Picture API translated 1:1 into Rust where (i) dead surface accumulates because no one removes it, (ii) god modules grow because no one splits them, (iii) cargo-cult abstractions persist because deletion feels risky, and (iv) "we'll need it later" stubs survive long enough to look load-bearing.

The cost shape is recurring: every new feature on top of `flui-painting` (e.g. when `flui-animation` re-enables and needs to record interpolated draw commands, when `flui-devtools` re-enables and wants to dump DisplayLists for inspection, when text rendering matures) will inherit and possibly extend the same maintenance debt unless cleaned first. The Mythos pass front-loads the cleanup.

The chain also coordinates with the parallel `flui-engine` Mythos chain currently in flight (worktree `flamboyant-varahamihira-2347ef` on branch `feat/flui-engine-mythos-redesign`, plan NNN 003). Both chains touch `docs/PORT.md` `## Index` and `scripts/port-check.sh`; rebase conflicts on those two files are trivial (one row per crate; union of trigger path globs).

---

## Actors

- **A1. Solo maintainer (`vanyastaff`)** -- runs the Mythos refactor by hand following the 14-step plan; primary author of the resulting commit chain and PR. Mythos rules are non-negotiable per `~/.claude/projects/.../memory/no-quick-wins-vanyastaff.md`: "execute full migration including breaking ripples." Maintainer is the consumer of the resulting `crates/flui-painting/ARCHITECTURE.md` template instance.

- **A2. Implementation agent (Claude Code / `/aif-implement` / `implement-coordinator`)** -- consumes the resulting `crates/flui-painting/ARCHITECTURE.md` `## Outstanding refactors` section when picking up follow-up work (e.g. Paint interning, flat-bytecode DisplayList, per-thread cosmic-text FontSystem, typed `NonNegativePixels` wrapper, shader warm-up backed by real offscreen canvas, `enum_dispatch`-style macro for the 29-variant DrawCommand operations). Not a primary author of the Mythos pass itself; downstream reader.

- **A3. Downstream crates as consumers** -- `flui-rendering` (paint phase: `Canvas` + `ClipContext::CanvasContext` impl + `Paint` re-export), `flui-engine` (wgpu backend: exhaustive `DrawCommand` pattern match across `wgpu/backend.rs`, `wgpu/layer_render.rs`, `wgpu/debug.rs`; consumes `Paint`/`PaintStyle`/`BlendMode`/`Shader`/`PointMode`/`StrokeCap`/`StrokeJoin`/`ImageFilter`/`ColorFilter`/`ImageRepeat`/`DisplayListCore`), `flui-layer` (`layer/canvas.rs` for `Layer::Canvas(CanvasLayer)`, `layer/picture.rs` for `Layer::Picture(PictureLayer)`), `flui-app` (`bindings/renderer_binding.rs` for the `PaintingBinding` singleton). Each must continue to compile and behave identically after the refactor.

  The `WarmUpCanvas` / `ShaderWarmUp` / `DefaultShaderWarmUp` deletions ripple **nowhere** outside `flui-painting` itself -- the deleted surface has no external callers (verified by `grep -r "WarmUpCanvas\|ShaderWarmUp\|DefaultShaderWarmUp" crates/`).

  The `PaintingBinding::with_shader_warm_up` and `set_shader_warm_up` deletions ripple to `flui-app::bindings::renderer_binding` if it calls them today (Phase 1 investigation confirms it does not).

  The `DisplayList::commands_mut` demotion from `pub` to `pub(crate)` ripples to any external callers that mutate commands after `finish()`. Phase 1 investigation confirms zero such callers in production code.

  The companion `docs/MIGRATION.md` deletion / stub-down ripples nowhere -- it is a documentation file.

  The god-module splits (canvas.rs, display_list.rs, text_layout.rs, text_painter.rs) preserve the public API surface; the `lib.rs` re-export block keeps the same symbol set (minus the WarmUpCanvas / ShaderWarmUp / DefaultShaderWarmUp deletions). External callers see no breaking change from the splits themselves.

---

## Key Flows

- **F1. Author the Mythos design verdict for `flui-painting`**
  - **Trigger:** the next crate in line after `flui-rendering` + `flui-layer` needs the Mythos lens applied.
  - **Actors:** A1.
  - **Steps:** Investigate the current shape (Phase 1: trait impl count audit, refusal-trigger scan, god-module identification, allocation hot-spot audit, companion-doc audit, dependency-feature audit) → identify dead surface and concern-mixed god modules → write the 13-section design verdict at `docs/designs/2026-05-20-mythos-flui-painting-redesign.md` matching the `flui-rendering` and `flui-layer` template → publish.
  - **Outcome:** A reviewable design verdict exists that the implementation chain can be sourced from. The verdict is the source of truth for the rest of the chain.
  - **Covered by:** R1, R2, R3.

- **F2. Execute the 14-step Mythos refactor chain**
  - **Trigger:** the verdict is published and the implementation plan is approved.
  - **Actors:** A1; agent A2 may pick up individual Outstanding refactors after the chain.
  - **Steps:** Branch off `origin/main` (NOT off local `main` which is at a divergent commit; NOT off any feature branch) on `feat/flui-painting-mythos-redesign` → execute each Mythos step as a commit → after each step, `cargo check --workspace`, `cargo test -p flui-painting --lib`, `cargo test -p flui-painting --tests`, `bash scripts/port-check.sh` (extended) all green or no commit → land breaking ripples in `flui-app` in-band per the no-quick-wins memo (anticipated ripple count: 1-2 commits for `PaintingBinding` API trim; remaining steps preserve public API).
  - **Outcome:** All 14 steps committed, all gates green, the PR is mergeable into `main` without remaining Mythos-blocked violations except those explicitly logged as concrete-blocker-with-named-dependency in `## Outstanding refactors`.
  - **Covered by:** R4-R20.

- **F3. Extend `scripts/port-check.sh` to cover `crates/flui-painting/src/`**
  - **Trigger:** the refactor lands and the methodology should now refuse the same patterns on next introduction in `flui-painting`.
  - **Actors:** A1.
  - **Steps:** Add `crates/flui-painting/src/` to the relevant trigger globs (Trigger 1: `RwLock<Box<dyn>>` on storage-shaped types -- forward-looking; Trigger 2: `Box<dyn>` wrapped in interior-mutability -- forward-looking; Trigger 3: `async fn` on `build|layout|paint|perform_layout|composite|render|fire_composition_callbacks` -- forward-looking) → run `bash scripts/port-check.sh -v` and verify all triggers stay clean post-refactor.

    Note: Trigger 3 for `flui-painting` was already in scope per `docs/PORT.md` (Mythos Step 13 of the `flui-layer` chain explicitly named `flui-painting` in the trigger's scope-extension note). This chain re-confirms the scope is genuinely covering `flui-painting/src/` after the U4-U7 god-module splits create new files inside subdirectories.
  - **Outcome:** Future re-introductions of any of the six refusal-trigger patterns inside `flui-painting` are caught at port-check time, not at next-quarter cleanup time.
  - **Covered by:** R17, R18.

- **F4. Templated `ARCHITECTURE.md` for `flui-painting`**
  - **Trigger:** the chain lands and the methodology requires a per-crate template instance for the touched crate.
  - **Actors:** A1.
  - **Steps:** Create `crates/flui-painting/ARCHITECTURE.md` at crate root following the five-section template in `docs/PORT.md` → graft from the existing `crates/flui-painting/docs/ARCHITECTURE.md` (companion deep-dive, kept in place per the docs/PORT.md graft instructions; not rewritten) → fill `## Flutter source mapping` (Flutter `painting/painting.dart` / `painting/canvas.dart` / `painting/clip.dart` / `painting/binding.dart` / `painting/image_cache.dart` / `painting/shader_warm_up.dart` / Skia `SkCanvas` etc. → FLUI file table), `## Mapping decisions` (Accepted trade-offs for the closed `DrawCommand` enum, sealed `DisplayListCore`/`DisplayListExt` pair, `WarmUpCanvas` + `ShaderWarmUp` deletions, `ClipContext` retention as cross-crate seam, `finish(self)` infallibility, deferred Paint interning), `## Thread safety` (post-refactor: ImageCache RwLocks + SystemFontsNotifier listener vec + FontSystem Mutex; all off hot path), `## Friction log` (companion docs/MIGRATION.md obsolete; allocation hot-path documented from U9), `## Outstanding refactors` (Paint interning, flat-bytecode, Path-Cow, per-thread FontSystem, typed NonNegativePixels, enum_dispatch-style macro, shader warm-up real implementation, optional doctest fix sweep) → update `docs/PORT.md` `## Index` to flip `flui-painting` from "`crates/flui-painting/docs/ARCHITECTURE.md` (pre-template)" to "Templated 2026-05-20 (Mythos chain)".
  - **Outcome:** The per-crate template instance for `flui-painting` exists at the crate root; companion docs in `crates/flui-painting/docs/` stay in place and are linked from the templated file as appropriate; the methodology's coverage advances by one active crate.
  - **Covered by:** R19, R20.

---

## Requirements

### Design verdict authorship

- **R1.** The design verdict at `docs/designs/2026-05-20-mythos-flui-painting-redesign.md` follows the 13-section structure established by `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` and `docs/designs/2026-05-20-mythos-flui-layer-redesign.md`: Problem Definition, Architecture Overview, Core Types, State Machine, Public API, Internal Modules, Async/Failure Semantics, Security Model, Data-Oriented Notes, Error Model, Tests Required, Rejected Designs, Implementation Plan. Sections that fold to "N/A" for `flui-painting` (e.g. no concurrent mutation paths) are present with a one-sentence justification, not omitted.

- **R2.** The verdict names the main state owner (`Canvas` owns inner `DisplayList` during recording; `Canvas::finish(self) -> DisplayList` consumes the canvas; the consumed DisplayList is moved into engine for GPU lowering or wrapped by flui-layer's `Layer::Picture`/`Layer::Canvas`), the main trust boundary (closed `DrawCommand` enum -- deliberately the same shape as `flui-layer::Layer` enum; no third-party `Box<dyn Drawable>` plugin trait), the main async risk (zero -- no `async fn` anywhere), and the main simplification principle (every dead trait, every fake plugin seam, every fluff method must justify its presence in writing).

- **R3.** Rejected designs in §12 of the verdict cover at least eleven alternatives explicitly considered and discarded: `Box<dyn Drawable>` plugin trait, `Arc<RwLock<Canvas>>` shared recording, fallible `Canvas::finish()`, fallible every `draw_*` method, Paint interning at construction, flat bytecode `Vec<u8>` instead of `Vec<DrawCommand>`, `RecordedCanvas`/`MutableCanvas` typestate distinction, `enum_dispatch` crate for the 29-variant `DrawCommand` operations, `async fn draw_*` for streaming display lists, converting `WarmUpCanvas` to closed enum vocabulary, demoting `DisplayListCore` to `pub(crate)` + sealing extensions, and helper submodule naming. The rejection of each names the temptation and the concrete reason it is wrong for FLUI.

### Refactor scope — dead surface deletion

- **R4.** The `WarmUpCanvas` trait (declaration at `crates/flui-painting/src/binding.rs` lines ~281-293, 4 abstract methods) is deleted. Verified by `grep -r "WarmUpCanvas" crates/` returning zero matches after the chain. The `&mut dyn WarmUpCanvas` parameter on the `ShaderWarmUp::warm_up_on_canvas` method is also gone (because R5 deletes the entire `ShaderWarmUp` trait); if a temporary intermediate exists between U1 and U2, the trait method signature is changed to drop the WarmUpCanvas reference but the trait body stays dead.

- **R5.** The `ShaderWarmUp` trait + `DefaultShaderWarmUp` struct + the `impl ShaderWarmUp for DefaultShaderWarmUp` block are deleted from `binding.rs` (lines ~250-319). The `shader_warm_up: Option<Box<dyn ShaderWarmUp>>` field on `PaintingBinding` is deleted (line 386). The `PaintingBinding::with_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>)` constructor variant (lines ~421-429) is deleted. The `PaintingBinding::set_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>)` setter (lines ~449-451) is deleted. The warm-up execution path in `BindingBase::init_instances` (lines ~474-481) is deleted. `ShaderWarmUp` + `DefaultShaderWarmUp` are removed from `lib.rs` re-exports. The `Debug` impl on `PaintingBinding` is updated to remove the `has_shader_warm_up` field. The deletion is documented in `crates/flui-painting/ARCHITECTURE.md` `## Mapping decisions` with a forward-looking note: "Shader warm-up subsystem deleted; track real offscreen-canvas-backed warm-up in Outstanding refactors."

- **R6.** The companion `docs/MIGRATION.md` (which documents migration "from 0.0.x to 0.1.x" of a crate that never had a 0.0.x release) is either deleted or stubbed to a 1-line note pointing at the templated `ARCHITECTURE.md` at the crate root.

### Refactor scope — god module splits

- **R7.** `crates/flui-painting/src/canvas.rs` (3,305 LOC) splits into a `canvas/` directory with:
  - `canvas/mod.rs` (~200 LOC) — `Canvas` struct + `Default`/`Clone`/`Debug`/`AsRef<DisplayList>` impls + `finish` + `display_list()` + `reset` + `clear_commands` + `is_empty`/`len`/`bounds` + `add_hit_region`.
  - `canvas/state.rs` (~400 LOC) — `CanvasState` + `ClipShape` + `save`/`restore`/`save_count`/`restore_to_count` + `save_layer`/`save_layer_alpha`/`save_layer_opacity`/`save_layer_blend`.
  - `canvas/transform.rs` (~400 LOC) — `translate`/`scale_uniform`/`scale_xy`/`rotate`/`rotate_around`/`skew`/`transform`/`set_transform`/`transform_matrix`.
  - `canvas/clipping.rs` (~400 LOC) — `clip_rect`/`clip_rrect`/`clip_path` + `*_ext` variants + `local_clip_bounds`/`device_clip_bounds`/`would_be_clipped`.
  - `canvas/drawing.rs` (~900 LOC) — all 29 `draw_*` primitive methods.
  - `canvas/scoped.rs` (~400 LOC) — 12 `with_*` scoped helpers.
  - `canvas/composition.rs` (~300 LOC) — `extend_from`/`extend`/`merge`/`append_display_list`/`append_display_list_at_offset`.
  - Inline `#[cfg(test)] mod tests` blocks stay in the new files for U7; they are extracted to `tests/` in U8.

- **R8.** `crates/flui-painting/src/display_list.rs` (2,434 LOC) splits into a `display_list/` directory with:
  - `display_list/mod.rs` (~250 LOC) — `DisplayList` struct + `Default` + `iter`/`iter_mut` + `apply_transform`/`filter`/`map`/`to_opacity`/`clear` + `commands_mut` (demoted to `pub(crate)` in U10) + `Display for stats`.
  - `display_list/command.rs` (~600 LOC) — 29-variant `DrawCommand` enum + `CommandKind` enum.
  - `display_list/command_ops.rs` (~1,200 LOC) — `DrawCommand` impl block: `with_opacity` (240 LOC pattern match), `bounds` (250 LOC pattern match), `transform`, `transform_mut`, `paint`, `has_paint`, `kind`, `is_*` accessors, `apply_transform`.
  - `display_list/sealed.rs` (~200 LOC) — `private::Sealed` module + `DisplayListCore` trait + `DisplayListExt` trait + 4 blanket impls (for `DisplayList`, `Arc<DisplayList>`, `Box<DisplayList>`, `&DisplayList`).
  - `display_list/stats.rs` (~150 LOC) — `DisplayListStats` struct + `zero()` + `new()` + `Display` impl.
  - `display_list/hit_region.rs` (~120 LOC) — `PointerEvent` + `PointerEventKind` + `HitRegion` + `HitRegionHandler`.

- **R9.** `crates/flui-painting/src/text_layout.rs` (1,243 LOC) splits into a `text_layout/` directory with the `#[cfg(feature = "text")] mod inner` indirection flattened (cfg moves from inside the file to the `pub mod text_layout;` declaration in `lib.rs`):
  - `text_layout/mod.rs` (~150 LOC) — re-exports + module-level docs + `static FONT_SYSTEM: OnceLock<Mutex<FontSystem>>` + `font_system()` accessor.
  - `text_layout/detect.rs` (~150 LOC) — `detect_text_direction` + `is_rtl_char` + `is_ltr_char`.
  - `text_layout/layout.rs` (~600 LOC) — `TextLayout` struct + `new` + `metrics` + cursor + hit-test methods.
  - `text_layout/line_info.rs` (~100 LOC) — `LineInfo` struct + `TextLayoutResult` struct.
  - `text_layout/measure.rs` (~250 LOC) — `measure_text` + `measure_inline_span` + `style_to_attrs` helpers.

- **R10.** `crates/flui-painting/src/text_painter.rs` (990 LOC) splits into a `text_painter/` directory with concern-based files (final boundary determined during implementation):
  - `text_painter/mod.rs` — re-exports + `TextPainter` struct + `DEFAULT_FONT_SIZE` constant.
  - `text_painter/baseline.rs` — `TextBaseline` enum + baseline math.
  - `text_painter/paint.rs` — paint integration + canvas drawing + glyph emission.
  - `text_painter/measure.rs` — measurement-side helpers (if separable; otherwise folded into mod.rs).

- **R11.** Inline `#[cfg(test)] mod tests` blocks across the new submodules from U4-U7 are extracted to integration test files under `crates/flui-painting/tests/`. New test file names: `canvas_state.rs`, `canvas_transform_unit.rs`, `canvas_drawing.rs`, `canvas_scoped_unit.rs`, `canvas_composition_unit.rs`, `display_list_command.rs`, `display_list_sealed.rs`, `display_list_stats.rs`, `display_list_hit_region.rs`, `text_layout_detect.rs`, `text_layout_layout.rs`, `text_painter_basic.rs`. Test names are preserved so the pre/post diff is reviewable as a move, not a rewrite. The existing 7 integration tests in `crates/flui-painting/tests/` (`canvas_composition.rs`, `canvas_scoped.rs`, `canvas_transform.rs`, `thread_safety.rs`, `text_layout_pipeline.rs`, `rich_text_example.rs`, `tessellation_integration.rs`) stay in place.

### Refactor scope — allocation audit + error model

- **R12.** The allocation hot-path audit lands as **documentation only**, not as code changes. Per the no-quick-wins memo's "concrete-blocker-with-named-dependency" exception, premature optimisation without measured benefit is filed as Outstanding refactors. The four documented sites:
  - `Paint::clone()` per `Canvas::draw_*` call (~80-200 bytes including optional `Box<Shader>` payload).
  - Per-`DrawCommand` 64-byte `Matrix4` baking on every variant.
  - `Path::clone()` per `draw_path` / `clip_path` / `draw_shadow` call (Vec<PathCommand> heap allocation).
  - `Box::new(Path::clone())` per `clip_path` call (additional heap indirection for ClipShape variant uniformity).

  Filed Outstanding refactors: "Paint interning at construction", "Flat-bytecode DisplayList representation", "Path-Cow on draw_path / clip_path / draw_shadow", "Per-thread cosmic-text FontSystem (cosmic-text 0.13+)".

- **R13.** `DisplayList::commands_mut` is demoted from `pub` to `pub(crate)`. The existing internal caller (`apply_transform`) continues to work. External callers that needed post-finish command mutation must migrate to the existing `apply_transform`/`filter`/`map`/`to_opacity` API. Phase 1 investigation confirms zero such external callers exist today.

- **R14.** `Canvas::finish(self)` adds `debug_assert!(self.save_stack.is_empty(), "Canvas finished with {} unrestored save() calls", self.save_stack.len());` before the existing `tracing::warn!(unrestored_saves = self.save_stack.len(), …)` line. Release-build behaviour is preserved (Flutter parity). Test builds catch the imbalance bug during development. The `finish()` signature stays `(self) -> DisplayList` (not `Result`); rationale documented in verdict §12 ("Make `Canvas::finish()` fallible" rejected design).

- **R15.** `PaintingError` 5 variants stay as-is. No new variants are added in this chain. `RecordingFinished`, `SaveRestoreImbalance`, `InvalidGeometry`, `PathBoundsExceeded` are filed in Outstanding refactors as the typed-wrapper companion work in `flui-types` (e.g. `NonNegativePixels`, `BoundedSaveDepth`) lands.

### Methodology extension

- **R16.** `PaintingBinding` surface is audited and trimmed alongside R5. Methods retained: `new()`, `default()`, `image_cache()`, `image_cache_mut()`, `system_fonts()`, `handle_memory_pressure()`, `handle_system_message(message_type)`, `evict(asset: &str)`, `instance()` (via `impl_binding_singleton!` macro). Methods deleted: `with_shader_warm_up`, `set_shader_warm_up`. The audit also adds `tracing::instrument` spans where missing (only on `handle_memory_pressure` and `handle_system_message`).

- **R17.** `scripts/port-check.sh` Trigger 1, Trigger 2, Trigger 3 path globs are confirmed to cover `crates/flui-painting/src/` after the U4-U7 god-module splits create new files in subdirectories. The relevant trigger entries in `docs/PORT.md` already name `flui-painting` for Trigger 3 (since the `flui-layer` chain's Step 13); this chain re-confirms the scope is genuinely covering all painting subdirectories post-split.

- **R18.** After the refactor chain lands, `bash scripts/port-check.sh -v` exits 0 and reports each trigger as "ok". Any violation that cannot be resolved at chain time is documented in `crates/flui-painting/ARCHITECTURE.md` `## Outstanding refactors` with concrete-blocker language (named external dependency, not "would touch X").

### Per-crate `ARCHITECTURE.md` instance

- **R19.** `crates/flui-painting/ARCHITECTURE.md` is created at crate root (per `AGENTS.md` naming convention; mirrors `crates/flui-rendering/ARCHITECTURE.md` + `crates/flui-layer/ARCHITECTURE.md` precedents) following the `docs/PORT.md` template specification (five fixed sections: `## Flutter source mapping`, `## Mapping decisions`, `## Thread safety`, `## Friction log`, `## Outstanding refactors`). Optional sections may be added (e.g. `## Exception ledger` if accepted trade-offs accumulate).

  Companion docs at `crates/flui-painting/docs/{ARCHITECTURE.md, MIGRATION.md, PERFORMANCE.md, README.md}` stay in place per the `docs/PORT.md` graft instructions. The new templated file references them where relevant.

- **R20.** `docs/PORT.md` `## Index` table flips `flui-painting` from "`crates/flui-painting/docs/ARCHITECTURE.md` (pre-template)" to "Templated 2026-05-20 (Mythos chain)" in the same commit that ships `crates/flui-painting/ARCHITECTURE.md`.

### Mythos rules (non-negotiable, sourced from no-quick-wins memo)

- **R21.** Breaking ripples in adjacent crates (`flui-app` for the PaintingBinding surface trim; potentially `flui-rendering`/`flui-engine`/`flui-layer` if any public API breaks during the splits, which is not anticipated) are executed in-band, not deferred. The only legitimate deferrals are concrete-blocker-with-named-dependency: external dependency needed (proptest dev-dep, loom dev-dep, miri CI infra, `cosmic-text 0.13+` migration, `flui-types::NonNegativePixels` typed wrapper, derive-macro feature) explicitly named in `## Outstanding refactors`. "Mechanical busywork" and "would touch flui-engine" are NOT legitimate deferrals.

- **R22.** No new `unsafe` block is introduced. The crate is `#[forbid(unsafe_code)]` at `lib.rs:151`. Net unsafe delta for `flui-painting`: **0**. Distinct from `flui-layer`'s −39 delta because flui-painting never had cargo-cult unsafe to begin with.

- **R23.** The hot path (`Canvas::draw_*`, `DrawCommand` variant construction, `DisplayList::push`, `DisplayList::commands()` iteration consumed by the engine) remains synchronous after the chain. No `async fn` may be introduced on any `Canvas` method or `DisplayList` accessor.

---

## Acceptance Examples

- **AE1. Covers R4.** Given the `WarmUpCanvas` trait has zero external callers in the workspace at the start of the chain (verified by `grep -r "WarmUpCanvas" crates/` returning matches only inside `crates/flui-painting/src/binding.rs` and `crates/flui-painting/src/lib.rs`), when the Mythos chain lands, then the trait declaration does not exist in `binding.rs`, `WarmUpCanvas` is not re-exported from `lib.rs`, and a fresh `grep -r "WarmUpCanvas" crates/` returns zero matches.

- **AE2. Covers R5.** Given `PaintingBinding::with_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>)` and `PaintingBinding::set_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>)` exist today (verified by `grep -n "shader_warm_up\|ShaderWarmUp" crates/flui-painting/src/binding.rs`), when the Mythos chain lands, then `ShaderWarmUp` + `DefaultShaderWarmUp` traits/structs are deleted, `PaintingBinding::shader_warm_up` field does not exist, the two constructor/setter methods do not exist, `BindingBase::init_instances` no longer references warm-up, the lib.rs re-export block no longer mentions `ShaderWarmUp` or `DefaultShaderWarmUp`, and `cargo test -p flui-painting` passes. A fresh `grep -r "ShaderWarmUp\|DefaultShaderWarmUp" crates/` returns zero matches.

- **AE3. Covers R7.** Given `canvas.rs` is 3,305 LOC with 8 distinct concerns today (verified by reading the file), when the Mythos chain lands, then `crates/flui-painting/src/canvas/mod.rs` is ≤ 250 LOC, `canvas/state.rs` is 300-500 LOC, `canvas/transform.rs` is 300-500 LOC, `canvas/clipping.rs` is 300-500 LOC, `canvas/drawing.rs` is the largest at 800-1000 LOC, `canvas/scoped.rs` is 300-500 LOC, `canvas/composition.rs` is 200-400 LOC, the public API surface of `Canvas` (29 `draw_*` + 12 `with_*` + 5 composition + transform/clip/state methods) compiles unchanged for all callers, and `cargo test -p flui-painting` passes.

- **AE4. Covers R8.** Given `display_list.rs` is 2,434 LOC mixing 8 distinct concerns today, when the Mythos chain lands, then `display_list/mod.rs` is ≤ 300 LOC, `display_list/command.rs` is 500-700 LOC, `display_list/command_ops.rs` is the largest at 1,000-1,300 LOC, `display_list/sealed.rs` is 150-250 LOC, `display_list/stats.rs` is 100-200 LOC, `display_list/hit_region.rs` is 100-150 LOC. The sealed `DisplayListCore`/`DisplayListExt` pair survives intact with all 4 blanket impls. The `DrawCommand` enum's 29 variants and the `DrawCommand` impl block (with_opacity, bounds, transform, transform_mut, paint, has_paint, kind, is_*, apply_transform) all compile unchanged for callers in `flui-engine`'s wgpu backend.

- **AE5. Covers R9.** Given `text_layout.rs` is 1,243 LOC wrapped in `#[cfg(feature = "text")] mod inner { … }` today, when the Mythos chain lands, then the cfg attribute lives on `pub mod text_layout;` in `lib.rs` (no `mod inner` indirection inside `text_layout/`), `text_layout/mod.rs` holds the FontSystem global, `text_layout/detect.rs` holds RTL/LTR detection, `text_layout/layout.rs` holds the cosmic-text `Buffer` wrapper, `text_layout/line_info.rs` holds the line metric types, `text_layout/measure.rs` holds the measurement helpers, `cargo build --no-default-features -p flui-painting` is clean (text feature genuinely off), and `cargo test -p flui-painting --features text` is green.

- **AE6. Covers R12, R13.** Given `Canvas::draw_*` clones Paint per call today (verified by reading drawing.rs after U7 split), when the Mythos chain lands, then the Paint-clone allocation pattern is documented in `crates/flui-painting/ARCHITECTURE.md` `## Friction log`, "Paint interning at construction" is filed in `## Outstanding refactors` with the named-blocker "requires `Paint: Hash + Eq` + per-canvas interning table + engine-side handle resolution + measured benchmark on realistic workloads", and `DisplayList::commands_mut` is `pub(crate)` (a compile-test demonstrating external use fails). No new code-side optimisation lands in this chain.

- **AE7. Covers R14.** Given `Canvas::finish(self)` calls `tracing::warn!(unrestored_saves = N)` today (verified by reading canvas.rs line ~1808-1822), when the Mythos chain lands, then `finish()` additionally calls `debug_assert!(self.save_stack.is_empty(), "Canvas finished with {} unrestored save() calls", self.save_stack.len())` before the tracing line, the `finish()` signature is still `(self) -> DisplayList` (not Result), and a unit test creates a Canvas with a save() but no restore() and verifies finish() panics in debug builds (cfg-gated test).

- **AE8. Covers R17, R18.** Given `docs/PORT.md` Trigger 3 already mentions `flui-painting` in its scope-extension note from the flui-layer chain, when the flui-painting chain lands, then `scripts/port-check.sh` Triggers 1, 2, 3 path globs explicitly include `crates/flui-painting/src/` for the post-split subdirectories (canvas/, display_list/, text_layout/, text_painter/), `bash scripts/port-check.sh -v` reports "ok" for all six triggers, and the docs/PORT.md trigger entries are updated to reflect the re-confirmed scope (text mentions "Re-confirmed in Mythos Step 13 of the `flui-painting` chain for post-split subdirectories").

- **AE9. Covers R19, R20.** Given `docs/PORT.md` `## Index` lists `flui-painting` as "`crates/flui-painting/docs/ARCHITECTURE.md` (pre-template)" today, when the Mythos chain lands, then `crates/flui-painting/ARCHITECTURE.md` exists at crate root with the five fixed template sections populated, the existing `crates/flui-painting/docs/ARCHITECTURE.md` + `MIGRATION.md` + `PERFORMANCE.md` + `README.md` stay in place (linked from the templated file as appropriate), `docs/PORT.md` `## Index` lists `flui-painting` as "Templated 2026-05-20 (Mythos chain)", and the `## Mapping decisions` section includes "Accepted trade-offs" entries for: closed `DrawCommand` enum (vs `Box<dyn Drawable>`), sealed `DisplayListCore`/`DisplayListExt` pair, `WarmUpCanvas` + `ShaderWarmUp` deletions, `ClipContext` retention as cross-crate seam, `finish(self)` infallibility, deferred Paint interning.

- **AE10. Covers R21 (Mythos rules).** Given the chain trims `PaintingBinding`'s surface (`with_shader_warm_up`, `set_shader_warm_up` deletions) and these may be called by `flui-app::bindings::renderer_binding`, when the chain lands, then any caller-side updates in `flui-app` are commits inside the same PR (not a follow-up), no "TODO: migrate callers of `with_shader_warm_up`" comment exists anywhere, and `cargo build --workspace` is clean.

- **AE11. Covers R22.** Given `#[forbid(unsafe_code)]` is set at `crates/flui-painting/src/lib.rs:151` today, when the Mythos chain lands, then the attribute is still in place, `rg "^unsafe " crates/flui-painting/src/` returns zero matches, and `cargo build -p flui-painting` is clean. The chain adds zero unsafe blocks.

---

## Success Criteria

- The Mythos refactor chain merges as a feature branch off `main` (`feat/flui-painting-mythos-redesign`) in a single PR with 14 reviewable commits, each commit passing `cargo check --workspace`, `cargo test -p flui-painting --lib`, `cargo test -p flui-painting --tests`, and `bash scripts/port-check.sh` (extended). No commit lands with broken tests or red CI.

- Net unsafe delta for `flui-painting`: **0**. The crate is and stays `#[forbid(unsafe_code)]`.

- Net LOC delta for the touched .rs files: targeted reduction from dead surface deletions (~250 LOC for `WarmUpCanvas` + `ShaderWarmUp` + `DefaultShaderWarmUp` + `PaintingBinding` trim) + concern-based splits (~7,972 LOC redistributed across new submodules + extracted to `tests/`). Total .rs file size in `src/` may grow slightly due to per-file boilerplate (mod headers, module-level docs), but no concern is mixed across boundaries.

- `crates/flui-painting/ARCHITECTURE.md` exists at crate root and matches the template. `docs/PORT.md` `## Index` shows `flui-painting` as "Templated 2026-05-20 (Mythos chain)".

- `scripts/port-check.sh` covers `crates/flui-painting/src/` (Triggers 1, 2, 3) for the post-split subdirectories. Running `bash scripts/port-check.sh -v` exits 0 and prints six "ok" lines.

- The PR description follows the shape of PR #77 + PR #78 (Track A: dead-surface deletions; Track B: god-module splits; Methodology extension; Key decisions; Verification; Pre-existing issues NOT addressed; Quick-wins-track callouts listing temptations the maintainer caught and rejected during the chain).

- A2 (a downstream agent) can pick up any entry from `crates/flui-painting/ARCHITECTURE.md` `## Outstanding refactors` and produce a follow-up PR without a fresh brainstorm or out-of-band clarification.

---

## Scope Boundaries

- **Out of scope: Paint interning at construction.** The verdict mentions this as the largest measurable allocation win; it is recorded as an Outstanding refactor but not landed in this chain because it requires `Paint: Hash + Eq` (additional trait impls; Paint contains `f32` colour which is not `Eq`), a per-canvas interning table, engine-side handle resolution, AND measured benefit on realistic workloads. Premature optimisation without measurement is filed as deferred per the no-quick-wins memo's "concrete-blocker-with-named-dependency" exception.

- **Out of scope: Flat-bytecode `Vec<u8>` `DisplayList`.** Same shape as Paint interning -- requires encoder + decoder + operation re-shape + measured benefit. Filed as Outstanding.

- **Out of scope: Per-thread cosmic-text FontSystem.** Requires cosmic-text 0.13+ adoption; current pin is 0.12. Filed as Outstanding.

- **Out of scope: Typed `NonNegativePixels` wrapper for radius/elevation arguments.** Requires `flui-types` change that ripples into every geometric API in the workspace. Filed as Outstanding with the named-blocker "flui-types breaking change".

- **Out of scope: Real offscreen-canvas-backed shader warm-up.** The decorative `ShaderWarmUp` subsystem is deleted in U2; the future real implementation requires a wgpu surface API that does not yet exist in the workspace. Filed as Outstanding.

- **Out of scope: `enum_dispatch`-style macro for the 29-variant `DrawCommand` operations.** The verdict mentions this as a possible reduction of the 1,200-LOC `command_ops.rs` boilerplate, but requires either a new proc-macro dep (`enum_dispatch`) or a hand-written `macro_rules!` extension. Filed as Outstanding; not blocking the chain because `command_ops.rs` is structurally clean (pure pattern match, low cognitive load even at 1,200 LOC).

- **Out of scope: Property tests, miri gate, loom tests for `flui-painting`.** Same shape as the `flui-rendering` and `flui-layer` carry-overs (the previous chains deferred these to Outstanding refactors because they require `proptest`/`loom` dev-deps and CI infra changes that exceed the chain's scope). The crate's `#[forbid(unsafe_code)]` makes the miri gate unnecessary; loom tests are unnecessary because there are no concurrent mutation paths in the crate.

- **Out of scope: `flui-engine`'s wgpu backend changes.** This chain only touches `flui-painting/src/`. The engine consumes `DrawCommand` via exhaustive pattern match; the chain preserves the variant shapes (no DrawCommand variant added/removed/restructured). The parallel `flui-engine` Mythos chain (worktree `flamboyant-varahamihira-2347ef`) handles engine-side cleanups separately; coordination at PR open time via rebase against `origin/main` for `docs/PORT.md` Index + `scripts/port-check.sh` shared file changes.

- **Out of scope: Re-enabling `flui-animation`, `flui-devtools`, `flui-cli`.** Disabled crates are not in the chain's blast radius. They may inherit Mythos-clean shapes when re-enabled in future chains.

- **Out of scope: Cross-crate dependency-graph audit.** This chain only touches the consumers of `flui-painting`'s public API where R21 (in-band breaking ripples) requires it. A workspace-wide audit of `Arc<RwLock<>>` sites in non-`flui-painting` crates is a separate brainstorm.

- **Out of scope: Building a third-party `Box<dyn Drawable>` plugin boundary.** The verdict explicitly rejects this as Rejected Design #1; the chain enforces the closed-enum `DrawCommand` shape.

- **Out of scope: Making `Canvas::finish()` fallible.** The verdict explicitly rejects this; the chain adds `debug_assert!` for the imbalance check + keeps `tracing::warn!` for release-build observability.

---

## Key Decisions

- **Closed `DrawCommand` enum over `Box<dyn Drawable>` plugin trait.** The GPU backend cannot lower arbitrary trait-object commands to wgpu draw calls; every variant is a coordinated change in `flui-painting` + `flui-engine`. The closed enum gives exhaustive-match compile-time checks; the trait object loses that. **Deliberately the same shape as `flui-layer::Layer` enum** (see flui-layer verdict Mapping decisions #1).

- **Sealed `DisplayListCore`/`DisplayListExt` pair stays.** The blanket `DisplayListCore for Arc<DisplayList>` impl is load-bearing: `flui-layer::Layer::Picture` carries `Arc<DisplayList>` and the engine consumes it via `display_list.commands()`. Demoting the pair to `pub(crate)` would force `flui-engine` into explicit Arc-deref at every call site.

- **Delete `WarmUpCanvas` + `ShaderWarmUp` subsystem over retain-for-future.** Zero impls of `WarmUpCanvas`; one stub impl of `ShaderWarmUp` whose `execute()` is decorative. The subsystem produces no measurable benefit; deleting it now means the future real implementation lands on a clean slate.

- **Retain `ClipContext` trait over inline into `flui-rendering`.** 1 production impl (`CanvasContext` in flui-rendering) is the legitimate cross-crate seam; the 3 default `clip_*_and_paint` methods save real boilerplate at the caller. Sealing it would force flui-rendering into an awkward concrete-type position.

- **Single-owner `Canvas` + consumed-once `DisplayList` over `Arc<RwLock<Canvas>>`.** Recording is fundamentally single-threaded; cross-thread workflows build their own Canvases and emit them as values (via `extend_from`/`merge`). The `Arc<RwLock<>>` shape is ceremony with no payback. Same shape as `flui-layer`'s rejection of `Arc<RwLock<LayerTree>>` (verdict S12 #2).

- **Keep `Canvas::finish(self) -> DisplayList` infallible.** Flutter parity: `PictureRecorder.endRecording()` does not return an error. Massive caller-side ripple if changed. The honest middle ground: `debug_assert!` in debug builds catches the bug class; `tracing::warn!` provides release-build observability.

- **Demote `DisplayList::commands_mut` from `pub` to `pub(crate)`.** External callers should go through `apply_transform`/`filter`/`map`/`to_opacity`. Phase 1 investigation confirms zero external callers.

- **Document allocation hot path; defer optimisation to Outstanding refactors.** Per the no-quick-wins memo's "concrete-blocker-with-named-dependency" exception, Paint interning + flat-bytecode + Path-Cow + per-thread FontSystem require external work (dev-deps, benchmarks, upstream version bumps, typed wrappers). Premature optimisation without measurement wastes effort.

- **Macro-collapse `is_*`/`as_*`/`as_*_mut` deferred to Outstanding refactor.** The 1,200-LOC `command_ops.rs` is structurally clean despite size (pure pattern match per variant). The macro-collapse adopts the `flui-layer` Step 4 hand-written `macro_rules!` pattern but lands as a separate cleanup PR; not blocking the chain.

- **Companion docs stay; obsolete `MIGRATION.md` stubs or deletes.** `docs/ARCHITECTURE.md` (architecture deep-dive), `docs/PERFORMANCE.md` (perf guidance), `docs/README.md` (Q&A) all stay and are linked from the new templated `ARCHITECTURE.md` at crate root. `docs/MIGRATION.md` is obsolete (migration between non-existent versions) and gets a stub or deletion.

- **Land breaking ripples in-band over deferred.** Per the no-quick-wins memo. The chain is 14 steps; `flui-app` ripple (PaintingBinding API trim) lands in steps 2-3, not as a follow-up PR. No "TODO: migrate caller of `with_shader_warm_up`" comment exists anywhere.

---

## Open Questions

### Resolved during planning

- "Is `Canvas::finish` fallible?" — resolved: stays infallible per Flutter parity + caller-side ripple cost. `debug_assert!` + `tracing::warn!` covers the bug class. (See verdict §12 rejected design "Make Canvas::finish() fallible".)

- "Are there any real consumers of `WarmUpCanvas`?" — resolved: zero. Pure dead code.

- "Is `ShaderWarmUp::execute()` doing anything?" — resolved: no. The body documents "in a real implementation, we'd create an offscreen canvas here." Decorative subsystem.

- "How many production impls of `ShaderWarmUp`?" — resolved: one (`DefaultShaderWarmUp`). Plus zero outside the dead `Option<Box<dyn ShaderWarmUp>>` field on PaintingBinding.

- "How many production impls of `ClipContext`?" — resolved: one (`CanvasContext` in `flui-rendering`). Plus 2 test impls. Legitimate cross-crate seam.

- "Are there any `unsafe` blocks in `flui-painting`?" — resolved: zero. `#[forbid(unsafe_code)]` is set. Net unsafe delta for this chain: 0.

- "Does any external caller use `DisplayList::commands_mut`?" — resolved: zero in production code. Demotion to `pub(crate)` is safe.

- "Does the `parallel` feature flag exist on flui-painting?" — resolved: no (that was flui-layer). flui-painting has `text` + `tessellation` + `serde` features, all wired correctly.

### Deferred to implementation

- **Final concern-boundary split inside `canvas/drawing.rs`.** The verdict proposes one big `drawing.rs` for all 29 `draw_*` methods. If after the U4 move the file exceeds ~2,000 LOC, a follow-up sub-split by variant group (shapes, text, image, gradient, effects, atlas, color, layer) is at the maintainer's discretion. The chain does NOT pre-commit to a specific sub-split.

- **Final concern-boundary split inside `text_painter/`.** The verdict proposes `mod`, `baseline`, `paint`, `measure`. The actual file content may collapse `measure` into `paint` or `mod` if measurement-side helpers are too thin to justify their own file. Final boundary determined during U7 implementation.

- **Whether `docs/MIGRATION.md` is fully deleted or stubbed.** Stub is gentler on the git history; deletion is more honest. Recommend stub.

- **Whether `cargo-mutants` is run as part of U14 verification.** Out of scope for the chain (CI infra concern). Filed in Outstanding if a future chain wants mutation testing.

- **Whether `tracing::instrument` spans are added to `Canvas::draw_*` methods (29 methods).** The verdict mentions this as "may be added if cheap"; the actual decision is at U3 / U10 implementation. Per-call span overhead is non-zero (struct allocation + thread-local lookup); 29 spans on the hot path may be too much. Recommend NOT adding `draw_*` spans by default; keep existing spans on `save_layer`, `extend_from`, `append_display_list_at_offset`, `finish`.

- **Whether `text_painter/` sub-split mirrors `text_layout/` exactly.** Depends on `text_painter.rs` actual internal concern boundaries. Read the file at U7 implementation; if concerns don't split cleanly, reduce to `mod` + `baseline` + `paint` (drop the separate `measure.rs`).

---

## Related Work

- `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` — the precedent verdict (rendering chain). The structure, the rejected-designs format, and the 14-step implementation plan template are sourced from this document.
- `docs/designs/2026-05-20-mythos-flui-layer-redesign.md` — the second precedent verdict (layer chain). The closed-enum rationale, dead-trait deletion methodology, god-module split discipline, and per-crate ARCHITECTURE.md template instance are sourced from this document.
- `docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md` — the methodology plan that established `docs/PORT.md`, the per-crate `ARCHITECTURE.md` template, and `scripts/port-check.sh`. Status: completed (merged via PR #77).
- `docs/plans/2026-05-20-002-feat-flui-layer-mythos-redesign-plan.md` — the layer chain's implementation plan. Status: completed (merged via PR #78).
- `crates/flui-rendering/ARCHITECTURE.md` — the first per-crate template instance.
- `crates/flui-layer/ARCHITECTURE.md` — the second per-crate template instance. The `flui-painting/ARCHITECTURE.md` instance R19 produces mirrors its shape.
- `~/.claude/projects/.../memory/no-quick-wins-vanyastaff.md` — the no-deferred-ripples rule. R21 codifies this for the chain.
- Reference commits on `main` (exemplars for the chain steps):
  - `907a7787` — full delete + rewire (analog: Mythos Step 1, `WarmUpCanvas` deletion).
  - `702e8751` — flui-layer Mythos Step 1 (analog: Mythos Step 2, `ShaderWarmUp` deletion).
  - `4d05efc5` — god-module split (analog: Mythos Steps 4, 5, 6, 7 — canvas.rs, display_list.rs, text_layout.rs, text_painter.rs splits).
  - `dc0fa1ad` — `catch_unwind` plumbing (analog reference; not directly applicable to flui-painting which has no third-party callback panics to catch).
  - `d0e53c63` — extension-trait split (analog: relevant if the sealed pair is ever further split; not in this chain).
- Parallel `flui-engine` Mythos chain (worktree `flamboyant-varahamihira-2347ef` on `feat/flui-engine-mythos-redesign`, plan NNN 003). Coordination at PR open time via rebase against `origin/main` for `docs/PORT.md` Index + `scripts/port-check.sh` shared file changes. Anticipated conflicts: trivial (one row per crate; union of trigger path globs).
