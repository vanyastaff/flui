# flui-painting Architecture

This document is the per-crate template instance for `flui-painting` as defined by [`docs/PORT.md`](../../docs/PORT.md). It records the Flutter / Skia → Rust mapping for this crate, the divergence decisions taken during the Mythos chain (PR opened 2026-05-20, commits `25f48fcc` through `ddd89e9a`), the current thread-safety surface, the known friction not yet refactored, and the planned cleanups that the methodology will pick up next.

The deeper Mythos design verdict lives at [`docs/designs/2026-05-20-mythos-flui-painting-redesign.md`](../../docs/designs/2026-05-20-mythos-flui-painting-redesign.md). The implementation plan lives at [`docs/plans/2026-05-20-004-feat-flui-painting-mythos-redesign-plan.md`](../../docs/plans/2026-05-20-004-feat-flui-painting-mythos-redesign-plan.md). The requirements brainstorm lives at [`docs/brainstorms/flui-painting-mythos-redesign-requirements.md`](../../docs/brainstorms/flui-painting-mythos-redesign-requirements.md). The allocation hot-path audit lives at [`docs/research/2026-05-20-flui-painting-alloc-audit.md`](../../docs/research/2026-05-20-flui-painting-alloc-audit.md).

Companion deep-dive docs live in [`docs/`](docs/) and are kept alongside this template:

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) -- pre-template architecture deep-dive (Command Pattern walkthrough, transform/clip stack design, integration points). Predates the Mythos chain; retained as reference companion.
- [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md) -- perf guidance and benchmark targets (no actual benches landed yet; targets recorded for future criterion harness).
- [`docs/MIGRATION.md`](docs/MIGRATION.md) -- stubbed in Mythos chain Step 3 (the original described migrations between non-existent crate versions).
- [`docs/README.md`](docs/README.md) -- Q&A landing page; retained as companion.

---

## Flutter source mapping

The Flutter `Canvas` API is split between `dart:ui` (the engine binding) and `package:flutter/src/painting/` (decoration/text/clip helpers). The `Skia SkCanvas` is the semantic reference for the recording API; cosmic-text + lyon are the Rust crates used in place of Skia's text shaping + path tessellation.

| Flutter / Skia source | FLUI module | Notes |
|---|---|---|
| `dart:ui` `Canvas` + `PictureRecorder` (Flutter engine) | [`src/canvas/mod.rs`](src/canvas/mod.rs) -- `Canvas` struct | FLUI conflates recorder + canvas into one `Canvas` value. `Canvas::finish(self) -> DisplayList` consumes the canvas (equivalent to `recorder.endRecording()`). |
| `dart:ui` `Canvas.translate`/`scale`/`rotate`/`skew`/`transform`/`setMatrix` | [`src/canvas/transform.rs`](src/canvas/transform.rs) | Transform stack operations; baked into emitted `DrawCommand::*` via `transform: Matrix4` field. |
| `dart:ui` `Canvas.save`/`restore`/`saveLayer`/`getSaveCount`/`restoreToCount` | [`src/canvas/state.rs`](src/canvas/state.rs) -- `CanvasState`, `ClipShape`, save/restore/save_layer family | State stack with `(transform, clip_depth, is_layer)` tuples. |
| `dart:ui` `Canvas.clipRect`/`clipRRect`/`clipPath` + bounds queries | [`src/canvas/clipping.rs`](src/canvas/clipping.rs) | 6 clip methods + 3 bounds queries. |
| `dart:ui` `Canvas.draw*` (lines, rects, paths, text, image, atlas, ...) | [`src/canvas/drawing.rs`](src/canvas/drawing.rs) | 29 primary `draw_*` methods, each emitting one `DrawCommand` variant. |
| Skia `SkCanvas` `save_layer` / `saveLayerAlpha` / `saveLayerOpacity` (extension) | [`src/canvas/state.rs`](src/canvas/state.rs) -- `save_layer_alpha`, `save_layer_opacity`, `save_layer_blend` | FLUI convenience overloads matching Flutter's `Canvas.saveLayer` shorthand variants. |
| `dart:ui` scoped wrappers (Flutter-side ergonomic patterns; no engine analog) | [`src/canvas/scoped.rs`](src/canvas/scoped.rs) -- 12 `with_*` helpers | Closure-based save/restore wrappers; zero-cost. |
| Multi-canvas composition (Flutter `PaintingContext.canvas` flow) | [`src/canvas/composition.rs`](src/canvas/composition.rs) -- `extend_from`/`merge`/`append_*` + `record`/`build` | First-child append uses `Vec::mem::swap` for O(1). |
| Caller-side ergonomic sugar (chaining, batch, conditional, grid, debug viz) | [`src/canvas/sugar.rs`](src/canvas/sugar.rs) | All caller-side wrappers; no `DrawCommand` emission directly. |
| `dart:ui` `Picture` (immutable recording) | [`src/lib.rs`](src/lib.rs) `pub type Picture = DisplayList` | Flutter-parity alias. |
| `dart:ui` `Canvas.drawX` underlying engine command vocabulary | [`src/display_list/command.rs`](src/display_list/command.rs) -- `DrawCommand` enum (29 variants) | Closed enum (no `Box<dyn Drawable>` plugin trait). Same shape as `flui-layer::Layer` enum. |
| `dart:ui` engine command dispatch | [`src/display_list/command_ops.rs`](src/display_list/command_ops.rs) -- `with_opacity`, `bounds`, `transform`, `paint`, `kind`, `is_*`, `apply_transform` | ~1,200 LOC of per-variant pattern matches. |
| Sealed-extension-trait pattern (FLUI-side; no Flutter analog) | [`src/display_list/sealed.rs`](src/display_list/sealed.rs) -- `DisplayListCore` + `DisplayListExt` + 4 blanket impls | Cross-crate seam consumed by `flui-layer::Layer::Picture` (stores `Picture = DisplayList` by value today; the `Arc<DisplayList>` blanket impl is a forward-compatible shape for future retained-layer sharing) and `flui-engine`'s wgpu backend. |
| `DisplayListStats` (FLUI invention for command-count stats) | [`src/display_list/stats.rs`](src/display_list/stats.rs) | |
| Flutter `dart:ui` `PointerEvent` -- minimal subset | [`src/display_list/hit_region.rs`](src/display_list/hit_region.rs) | Hit-region recording; full event system in `flui-interaction`. |
| `painting/clip.dart` `ClipContext` abstract class | [`src/clip_context.rs`](src/clip_context.rs) | Cross-crate seam (1 prod impl: `CanvasContext` in `flui-rendering`). 3 default `clip_*_and_paint` methods. |
| `painting/binding.dart` `PaintingBinding` mixin | [`src/binding.rs`](src/binding.rs) -- `PaintingBinding` singleton | Trimmed surface; `ShaderWarmUp` subsystem deleted in U2. |
| `painting/image_cache.dart` `ImageCache` | [`src/binding.rs`](src/binding.rs) -- `ImageCache` struct | `RwLock<HashMap>` for cache + live_images; off the per-command hot path. |
| `painting/binding.dart` `SystemFontsNotifier` | [`src/binding.rs`](src/binding.rs) -- `SystemFontsNotifier` struct | `RwLock<Vec<Arc<dyn Fn>>>` listener registry; setup-phase only. |
| `painting/text_painter.dart` `TextPainter` | [`src/text_painter/*`](src/text_painter/) -- 4 files | TextPainter struct + builder + getters + setters + layout + measure + paint + cursor. Split in Mythos chain Step 7. |
| `painting/text_painter.dart` `TextBaseline` enum | [`src/text_painter/baseline.rs`](src/text_painter/baseline.rs) | |
| cosmic-text 0.12 `Buffer` + `FontSystem` shaping API | [`src/text_layout/*`](src/text_layout/) -- 5 files | cosmic-text-backed text shaping. Split in Mythos chain Step 6; `mod inner` cfg indirection flattened. |
| Flutter `TextDirection` detection (Unicode bidi) | [`src/text_layout/detect.rs`](src/text_layout/detect.rs) | Strong-LTR / strong-RTL / neutral Unicode codepoint ranges. |
| lyon 1.0 `FillTessellator` + `StrokeTessellator` | [`src/tessellation.rs`](src/tessellation.rs) | Path → GPU triangle conversion. Untouched in this chain (already clean). |
| `painting/shader_warm_up.dart` `ShaderWarmUp` abstract class | -- | **Deleted in Mythos chain Step 2.** Decorative subsystem; `execute()` was a stub. Real offscreen-canvas-backed warm-up tracked in Outstanding refactors. |

---

## Mapping decisions

This section records places where the Rust shape diverges from the Dart/Skia shape and why. Each entry follows the "Accepted trade-offs" format established by [`docs/plans/2026-03-31-custom-render-callback-design.md`](../../docs/plans/2026-03-31-custom-render-callback-design.md).

### 1. Closed `DrawCommand` enum, not a `Box<dyn Drawable>` plugin trait

**Rule:** [`docs/PORT.md`](../../docs/PORT.md) Mapping rule "Compile-time over runtime"; constitution Anti-Patterns ("Prefer generics and enum dispatch over `dyn` trait objects"); strategy clause "Behavior loyal, structure Rust-native".

**Choice:** `DrawCommand` is a closed `enum` with 29 concrete variants ([`src/display_list/command.rs`](src/display_list/command.rs)). The closed enum is **the** trust boundary with `flui-engine`'s wgpu backend -- the backend pattern-matches every variant exhaustively for GPU lowering. Adding a 30th variant is a coordinated change in `flui-painting` + `flui-engine` (+ optionally `flui-rendering` if a render-object should emit it).

**Alternatives:**
- `Box<dyn Drawable + Send + Sync>` mirroring Flutter's Dart class hierarchy -- rejected. The GPU backend cannot lower an arbitrary `dyn Drawable` to wgpu draw calls; every variant needs a hand-written translation. Closed-enum gives compile-time match-exhaustiveness; trait-object loses that. **Deliberately the same shape as `flui-layer::Layer` enum** (see [`docs/designs/2026-05-20-mythos-flui-layer-redesign.md`](../../docs/designs/2026-05-20-mythos-flui-layer-redesign.md) Mapping decisions #1).
- Sealed-trait-with-private-impl-marker -- rejected. Same result as closed enum but with vtable dispatch on the hot path.

**Accepted trade-off:** Plugin authors cannot define their own `DrawCommand` variants. The 29 variants must cover every compositor draw primitive forever (the rendering crate emits only these). When a new compositor primitive appears (e.g. mesh shaders, future WebGPU features), it lands as a new variant in a coordinated change. Verdict §12 rejected design #1.

### 2. Sealed `DisplayListCore` / `DisplayListExt` extension-trait pair stays

**Rule:** Verdict §12 rejected design #10; precedent: `flui-rendering`'s extension-trait split (commit `d0e53c63`).

**Choice:** `DisplayListCore` is sealed via the `private::Sealed` marker trait; `DisplayListExt` is a blanket-implemented superset of helpers (filter iterators, count stats). Four blanket impls: `DisplayList`, `Arc<DisplayList>`, `Box<DisplayList>`, `&DisplayList`. `flui-layer::Layer::Picture` currently stores `Picture = DisplayList` by value (see [`crates/flui-layer/src/layer/picture.rs`](../flui-layer/src/layer/picture.rs)); the `Arc<DisplayList>` blanket impl is kept as a forward-compatible shape for retained-layer sharing across frames.

**Alternatives:**
- Demote `DisplayListCore` to `pub(crate)` and expose only `DisplayList` directly -- rejected. The trait is the cross-crate seam that lets `flui-engine`'s wgpu backend accept any of the four supported wrappers via `display_list.commands()`; demoting would force callers into explicit `.deref()` at every call site.
- Replace the trait pair with inherent methods on `DisplayList` -- rejected. The blanket-on-smart-pointer pattern is what makes `Arc<DisplayList>` ergonomic for the eventual retained-layer use case; inherent methods would not generalise.

**Accepted trade-off:** External callers must import `DisplayListCore` (or use the prelude) to access `.commands()` / `.bounds()` / `.len()` on `DisplayList`. The flui-rendering chain hit this friction and resolved it via prelude import; flui-engine's wgpu backend does the same.

### 3. `WarmUpCanvas` + `ShaderWarmUp` subsystem deletion (decorative)

**Rule:** Strategy clause "Every dyn, every Arc, every RwLock must defend its existence in writing"; verdict §12 rejected design #10 ("Convert `WarmUpCanvas` to a closed enum vocabulary -- rejected; deletion is the answer").

**Choice:** In Mythos chain Step 1, the `WarmUpCanvas` trait (4 abstract methods, 0 production impls) was deleted from [`src/binding.rs`](src/binding.rs). In Step 2, the entire `ShaderWarmUp` trait + `DefaultShaderWarmUp` struct + `shader_warm_up: Option<Box<dyn ShaderWarmUp>>` field on `PaintingBinding` + `with_shader_warm_up` constructor variant + `set_shader_warm_up` setter + `BindingBase::init_instances` warm-up branch all went with it.

**Alternatives:**
- Convert `WarmUpCanvas` to a closed enum vocabulary `WarmUpCommand` -- rejected. Salvages the API but perpetuates the lie that warm-up actually does something. The `execute()` body of the original `DefaultShaderWarmUp` literally documented "in a real implementation, we'd create an offscreen canvas here".
- Keep `ShaderWarmUp` as a trait with a stub impl, file real implementation as future work -- rejected. Future code lands cleaner on a deleted-then-rebuilt slate than on a stub-with-1-impl.

**Accepted trade-off:** If a future Mythos chain needs real shader warm-up, the right shape is an offscreen canvas wired through `flui-engine`'s wgpu surface API (which does not yet exist in the workspace). Rebuilding will be ~50 LOC; the deleted ~75 LOC of trait + struct + binding plumbing was hostile to that rebuild. Tracked in `## Outstanding refactors` below.

### 4. `ClipContext` retention as cross-crate seam (1 prod impl)

**Rule:** Strategy clause "Composition over inheritance" but with awareness of cross-crate ergonomics.

**Choice:** [`src/clip_context.rs`](src/clip_context.rs) carries the `ClipContext` trait with 3 default `clip_*_and_paint` methods. The trait's only required method is `canvas_mut(&mut self) -> &mut Canvas`. The trait has exactly 1 production impl (`CanvasContext` in `flui-rendering::context::canvas`) + 2 test impls.

**Alternatives:**
- Demote the trait to `pub(crate)` and re-implement the 3 default methods as free functions taking `&mut Canvas` -- rejected. Would force `flui-rendering::CanvasContext` to call free functions awkwardly; the trait method dispatch is more ergonomic.
- Inline the 3 default methods into `flui-rendering::CanvasContext` directly -- rejected. Would duplicate ~80 LOC of clip dispatch logic at the only caller site; the trait's default-method pattern is the legitimate boilerplate-saving mechanism.

**Accepted trade-off:** The trait stays despite having only 1 production impl. The 3 default methods provide real value at the caller site; the seal (`canvas_mut` as the only required method) keeps the surface narrow. Documented as a legitimate single-impl trait for ergonomic cross-crate dispatch.

### 5. `Canvas::finish(self) -> DisplayList` stays infallible

**Rule:** Strategy clause "Behavior loyal, structure Rust-native" -- Flutter parity for the common case.

**Choice:** `Canvas::finish(self)` returns `DisplayList` directly (not `Result<DisplayList, PaintingError>`). On unrestored `save()` calls, it fires `debug_assert!` (Mythos chain Step 10) to catch the bug during tests, and `tracing::warn!` for release-build observability. Flutter's `PictureRecorder.endRecording()` does the same -- silent finalisation with debug-time sanity checks.

**Alternatives:**
- Change to `finish(self) -> Result<DisplayList, PaintingError::SaveRestoreImbalance>` -- rejected. Massive caller-side ripple (every paint phase call site has to handle `Result`). Flutter parity matters more than the bug-class catching in release; debug builds already catch via the new `debug_assert!`.
- Add `try_finish(self) -> Result<...>` companion method -- rejected. Adds API surface duplication without solving the caller-side problem; if callers want explicit error handling, they check `save_count() == 1` before calling `finish()`.

**Accepted trade-off:** Release builds silently log the imbalance via tracing rather than surface it as an error. Developer-facing test builds catch the imbalance via panic. The trade matches Flutter's behaviour and avoids the workspace ripple. Verdict §12 rejected design "Make `Canvas::finish()` fallible".

### 6. `Paint.clone()` per `Canvas::draw_*` deferred (measured benefit needed)

**Rule:** No-quick-wins memo's "concrete-blocker-with-named-dependency" exception; verdict §9 (Data-Oriented Notes) and §12 (Rejected Designs entry "Paint interning at construction").

**Choice:** Every `Canvas::draw_*` method clones the `Paint` parameter into the emitted `DrawCommand`. For 1,000+ commands per frame with reused `Paint`, this is measurable allocation churn (~80-200 bytes per clone). The Mythos chain Step 9 audit documented the cost but deferred the optimisation -- Paint interning requires `Paint: Hash + Eq` (Paint contains `f32` colour components; not `Eq`), a per-canvas interning table, engine-side handle resolution, and a measured benchmark on realistic workloads.

**Alternatives:**
- Implement Paint interning now -- rejected. Real optimisation, but the blocker chain is named (`Paint: Hash + Eq` requires either bit-pattern hashing of `f32` or `ordered-float` wrapping; per-canvas table is new state on `Canvas`; engine handle resolution is a wgpu-backend change; measured benchmark requires a criterion harness). Each blocker is concrete external work.
- Use `Cow<'a, Paint>` borrowing in `DrawCommand` -- rejected. Adds lifetime complexity to every `DrawCommand` variant; would force the `DisplayList` to hold the source borrows for its lifetime; ripples into `Arc<DisplayList>` retained-layer use cases where the borrow source is gone.

**Accepted trade-off:** Per-`draw_*` Paint allocation cost is paid by every recorded command today. Filed as Outstanding refactor; tracked in [`docs/research/2026-05-20-flui-painting-alloc-audit.md`](../../docs/research/2026-05-20-flui-painting-alloc-audit.md) with named blockers. Verdict §12 rejected design "Paint interning at construction".

### Net unsafe delta: 0

The crate is `#[forbid(unsafe_code)]` at [`src/lib.rs:151`](src/lib.rs) before and after the chain. Zero `unsafe` blocks introduced; zero removed. Distinct from the `flui-layer` chain's -39 net delta (flui-layer had 39 cargo-cult `unsafe impl Send + Sync` blocks to delete; flui-painting never had them).

---

## Thread safety

`flui-painting` runs in the paint phase (scene construction) and the engine phase (GPU lowering). Per strategy clause "sync hot path", neither is multi-threaded within a single canvas/scene. Cross-thread movement is by value (`Canvas: Send`, `DisplayList: Send + Sync`).

| Site | Primitive | Category | Notes |
|---|---|---|---|
| `Canvas` ([`src/canvas/mod.rs`](src/canvas/mod.rs)) | Owned struct | Auto-`Send` + auto-`!Sync` | No interior mutability on Canvas itself. Recording is single-threaded by design. |
| `DisplayList` ([`src/display_list/mod.rs`](src/display_list/mod.rs)) | Owned struct | Auto-`Send + Sync` | Consumed-once value; immutable from public API after `Canvas::finish()`. |
| `Arc<DisplayList>` blanket impl ([`src/display_list/sealed.rs`](src/display_list/sealed.rs)) | `Arc<>` wrap | Send + Sync via `Arc` | Forward-compatible blanket impl for future retained-layer caching across frames. `flui-layer::Layer::Picture` stores `Picture = DisplayList` by value today. Read-only via `DisplayListCore`. |
| `HitRegionHandler` = `Arc<dyn Fn(&PointerEvent) + Send + Sync>` ([`src/display_list/hit_region.rs`](src/display_list/hit_region.rs)) | `Arc<dyn Fn>` | Per-hit-region heap allocation | Recording-time only; off per-command hot path. |
| `Paint`, `Path`, `Shader`, `Image` etc. (re-exported from `flui_types::painting`) | Owned values | Auto-`Send + Sync` | Validated at construction in `flui-types`. |
| `binding::ImageCache::cache` ([`src/binding.rs`](src/binding.rs)) | `RwLock<HashMap<String, CachedImage>>` | Shared infrastructure | Off per-command hot path per [`docs/PORT.md`](../../docs/PORT.md) lock-decision table. |
| `binding::ImageCache::live_images` ([`src/binding.rs`](src/binding.rs)) | `RwLock<HashMap<String, CachedImage>>` | Shared infrastructure | Same; off per-command hot path. |
| `binding::ImageCache::current_size_bytes` / `max_images` / `max_size_bytes` | `AtomicUsize` (3 sites) | Lock-free atomics | Set/get counters; no contention. |
| `binding::SystemFontsNotifier::listeners` ([`src/binding.rs`](src/binding.rs)) | `RwLock<Vec<Arc<dyn Fn() + Send + Sync>>>` | Setup-phase registry | System font change notifications are rare; off per-command hot path. |
| `binding::PaintingBinding` singleton | Process-wide via `impl_binding_singleton!` macro | `Send + Sync` | Single static instance; access serialised by the binding's own RwLock. |
| `text_layout::layout::FONT_SYSTEM` ([`src/text_layout/layout.rs`](src/text_layout/layout.rs)) | `static OnceLock<Mutex<FontSystem>>` (feature-gated) | Lazy init + per-shape lock | cosmic-text font system; held during `Buffer::set_text` + `Buffer::shape_until_scroll`. Off per-command hot path; per-text-layout-creation. Lock contention on multi-text-widget workloads -- filed in Outstanding refactors as cosmic-text 0.13+ upgrade blocker. |
| `SceneBuilder::stack` (per-Canvas owned) | Owned `Vec<...>` | Single-mutator | Borrow checker enforces single-writer-during-build at compile time. |
| `tessellation` module ([`src/tessellation.rs`](src/tessellation.rs), feature-gated) | Stateless functions | -- | `lyon::FillTessellator`/`StrokeTessellator` instances are created per call; no shared state. |

**Zero `unsafe impl Send/Sync` blocks anywhere in the crate.** `#[forbid(unsafe_code)]` at [`src/lib.rs:151`](src/lib.rs). Net unsafe delta for this chain: **0**.

---

## Friction log

Known sites that do not yet match the methodology but are not violations of the current refusal triggers. Each entry names the site and the next planned step.

### Allocation hot path: `Paint.clone()` per draw_*

**Site:** Every `Canvas::draw_*` method in [`src/canvas/drawing.rs`](src/canvas/drawing.rs).

**Cost:** ~80-200 bytes per draw call. For 1000+ commands per frame with reused `Paint`, measurable. Documented in [`docs/research/2026-05-20-flui-painting-alloc-audit.md`](../../docs/research/2026-05-20-flui-painting-alloc-audit.md) finding F1.

**Status:** Deferred to Outstanding refactor "Paint interning at construction" (named blockers: `Paint: Hash + Eq`, per-canvas table, engine handle resolution, measured benchmark).

### Allocation hot path: per-`DrawCommand` 64-byte `Matrix4` baking

**Site:** [`src/display_list/command.rs`](src/display_list/command.rs) -- every variant carries `transform: Matrix4`.

**Cost:** 64 bytes per command, 64 KB for a 1000-command frame. If transform invariant across most commands, redundant. Audit finding F2.

**Status:** Deferred to Outstanding refactor "Flat-bytecode DisplayList representation" (very high blast radius; named blockers: bytecode encoder/decoder, operation re-shape, measured benefit).

### Allocation hot path: `Path.clone()` + `Box::new(Path::clone())`

**Sites:**
- `Canvas::draw_path` -- [`src/canvas/drawing.rs:107`](src/canvas/drawing.rs) -- clones `Path` (Vec<PathCommand>).
- `Canvas::draw_shadow` -- [`src/canvas/drawing.rs`](src/canvas/drawing.rs) -- same.
- `Canvas::clip_path` / `clip_path_ext` -- [`src/canvas/clipping.rs`](src/canvas/clipping.rs) -- additional `Box::new()` for `ClipShape::Path` variant uniformity.

**Cost:** O(N) per path command + heap allocation; double allocation per `clip_path`. Audit findings F3 + F4.

**Status:** Deferred to Outstanding refactors "Path Clone-on-Write" (`flui-types` breaking change blocker) and the smaller `ClipShape::Path(Path)` un-box (bundled with the same Path-CoW change).

### cosmic-text 0.12 `FontSystem` mutex contention

**Site:** `text_layout::layout::FONT_SYSTEM` at [`src/text_layout/layout.rs`](src/text_layout/layout.rs).

**Cost:** Per-shape lock held during `Buffer::set_text` + `Buffer::shape_until_scroll` (1-10ms for complex text). Multi-text-widget workloads serialise. Audit finding F5.

**Status:** Deferred to Outstanding refactor "Per-thread cosmic-text FontSystem (cosmic-text 0.13+)" -- named blocker: cosmic-text version bump (current pin 0.12 vs glyphon's 0.14 in flui-engine = duplicate-version situation).

### Per-`draw_*` `tracing::instrument` -- NOT added by design

**Sites:** All 29 `Canvas::draw_*` methods.

**Status:** Verdict §13 Step 9 explicitly declined per-draw spans (Span allocation + thread-local lookup overhead non-trivial for 1000+ draws/frame). Existing tracing spans live on `Canvas::save_layer`, `Canvas::finish`, `Canvas::extend_from`, `Canvas::append_display_list_at_offset`, `DisplayList::append`, `DisplayList::to_opacity`, `PaintingBinding::handle_*` -- all coarser-than-per-draw. Audit finding F6.

### Companion docs predate the template

**Sites:** [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md), [`docs/README.md`](docs/README.md).

**Status:** Retained as reference companions per the [`docs/PORT.md`](../../docs/PORT.md) graft instructions ("existing port-flavoured docs are integrated into the template in-place; do not rewrite"). This templated `ARCHITECTURE.md` at crate root is the authoritative document; the `docs/*.md` files are deeper companions linked from this file's Flutter source mapping section.

[`docs/MIGRATION.md`](docs/MIGRATION.md) was stubbed in Mythos chain Step 3 (the original described migrations between non-existent crate versions).

### Doctests use the canvas/drawing API correctly post-split

**Sites:** ~20 doc examples across the post-split submodule files.

**Status:** No verified breakage during the chain. If doctest failures surface in a future audit, they get tracked here.

### CLAUDE.md drift (workspace-wide)

**Site:** [`CLAUDE.md`](../../CLAUDE.md) "Current Development Focus" section.

**Status:** Pre-existing per `flui-rendering` and `flui-layer` chains' deferred lists; not addressed here. Workspace-level housekeeping PR will reconcile CLAUDE.md vs AGENTS.md vs `docs/crates.md`.

---

## Outstanding refactors

Concrete cleanups visible from `flui-painting` outward, sized for an `/aif-implement` dispatch. Each entry names a file/site and what would need to change. Named blockers are flagged per the no-quick-wins memo's "concrete-blocker-with-named-dependency" exception.

### "Paint interning at construction"

**Goal:** Replace per-`Canvas::draw_*` `Paint::clone()` with a per-canvas interning table; `DrawCommand` variants carry `PaintHandle(NonZeroU32)` instead of `Paint`. Engine resolves handle to `&Paint` at GPU lowering.

**Files:** [`src/canvas/drawing.rs`](src/canvas/drawing.rs), [`src/display_list/command.rs`](src/display_list/command.rs), [`src/display_list/command_ops.rs`](src/display_list/command_ops.rs), [`src/canvas/mod.rs`](src/canvas/mod.rs); ripples into `flui-engine`'s wgpu backend.

**Named blockers:**
- `Paint: Hash + Eq` -- Paint contains `f32` color components; not `Eq`. Requires bit-pattern hashing of `f32` or `ordered-float` wrapping.
- Per-canvas interning table -- new state on `Canvas`; must be cleared on `reset()`.
- Engine-side handle resolution in `flui-engine`'s wgpu backend.
- Measured benchmark via criterion harness for a 1,000-`draw_rect` synthetic workload.

**Reference:** [`docs/research/2026-05-20-flui-painting-alloc-audit.md`](../../docs/research/2026-05-20-flui-painting-alloc-audit.md) finding F1.

### "Flat-bytecode `Vec<u8>` `DisplayList` representation"

**Goal:** Replace `Vec<DrawCommand>` with a byte buffer + opcode tags (Skia `SkRecord` shape). The engine decodes opcode + payload at GPU lowering. Dedups invariant transforms across runs.

**Files:** [`src/display_list/mod.rs`](src/display_list/mod.rs), [`src/display_list/command.rs`](src/display_list/command.rs), [`src/display_list/command_ops.rs`](src/display_list/command_ops.rs); ripples into engine + retained-layer consumers.

**Named blockers:**
- Bytecode encoder per `DrawCommand` variant.
- Bytecode decoder on `flui-engine` side.
- Re-shape `with_opacity`, `apply_transform`, `bounds`, `filter`, `map`, `to_opacity` to work over bytecode (significant refactor of `command_ops.rs`).
- Loss of `serde` derive ergonomics on `DrawCommand`; would need hand-rolled serde for the byte buffer.
- Measured benchmark.

**Reference:** Audit F2.

### "`Path` Clone-on-Write (`Arc<[PathCommand]>`)"

**Goal:** Change `Path` interior from `Vec<PathCommand>` to `Arc<[PathCommand]>`. `Path::clone()` becomes `Arc::clone()` (one atomic increment). Eliminates per-`draw_path` heap allocation.

**Files:** `flui-types::painting::Path` (workspace breaking change).

**Named blockers:**
- `Path` lives in `flui-types::painting`. Requires a `flui-types` breaking change.
- Existing `Path::push` / `Path::move_to` callers compile unchanged via `Arc::make_mut` but pay one-time cost on first mutation.
- Measured benchmark on N-repeated-icon-paths workload.

**Reference:** Audit F3 + F4.

### "Per-thread cosmic-text `FontSystem` (cosmic-text 0.13+ upgrade)"

**Goal:** Eliminate the global `Mutex<FontSystem>` contention by using cosmic-text 0.13+'s thread-local font system support.

**Files:** [`src/text_layout/layout.rs`](src/text_layout/layout.rs), [`src/text_layout/measure.rs`](src/text_layout/measure.rs), [`Cargo.toml`](Cargo.toml).

**Named blockers:**
- cosmic-text 0.12 → 0.13+ upgrade. API surface changes may ripple into `text_layout/*` and `text_painter/measure.rs`.
- glyphon (in flui-engine) currently uses cosmic-text 0.14; upgrading flui-painting to 0.14 would deduplicate the two cosmic-text versions in the workspace (per cargo tree -d).
- Measured benchmark on concurrent text-shape workload.

**Reference:** Audit F5.

### "Typed `NonNegativePixels` wrapper for radius/elevation"

**Goal:** Replace `debug_assert!` panics on negative `radius` / `elevation` in `Canvas::draw_circle` / `Canvas::draw_shadow` with type-level enforcement via `NonNegativePixels(Pixels)`.

**Files:** `flui-types::geometry::Pixels` (new wrapper type); ripples into [`src/canvas/drawing.rs`](src/canvas/drawing.rs) `draw_circle`, `draw_shadow`, `draw_point` signatures.

**Named blockers:**
- `flui-types` breaking change to add `NonNegativePixels`.
- Ripples into every geometric API in the workspace that uses radius/elevation.

### "Real offscreen-canvas-backed shader warm-up"

**Goal:** Re-implement the deleted (Mythos chain Step 2) shader warm-up as an offscreen-canvas system that actually bootstraps shader compilation.

**Files:** New `crates/flui-painting/src/warm_up/`; integrates with `flui-engine`'s wgpu surface API.

**Named blockers:**
- wgpu surface API for offscreen canvases does not yet exist in the workspace; needs `flui-engine`-side groundwork.
- Measured frame-time benefit (the original Flutter rationale was "shader compilation can cause jank during animations"; jank measurement requires a real workload + GPU profiler).

### "`gen_command_accessors!` macro for `DrawCommand` 29-variant operations"

**Goal:** Apply the `flui-layer` Step 4 hand-written `macro_rules!` pattern to collapse [`src/display_list/command_ops.rs`](src/display_list/command_ops.rs) (~1,200 LOC of per-variant pattern matches) via a single macro invocation.

**Files:** New `src/display_list/dispatch.rs`; modifies `command_ops.rs`.

**Named blockers:**
- The macro must mirror `DrawCommand`'s per-variant payload shapes 1:1, but each of the 29 variants has a distinct named-field set (`rect+paint+transform` vs. `path+paint+transform` vs. `child+filter+bounds+blend_mode+transform`). A bare `macro_rules!` arm cannot synthesise the field tokens from a single variant name, so the choices are: (a) hand-list every variant's payload tuple at the macro invocation site (defeats the LOC win) or (b) introduce the [`paste`](https://crates.io/crates/paste) crate as a workspace dependency so the macro can stamp out `transform_mut`/`transform`/`apply_transform` arms from the variant name alone.
- Pick the `paste` adoption path. Coordinate workspace-wide: `flui-layer`'s Step 4 macro and `flui-engine`'s dispatch table would benefit from the same dep, so dep introduction should be a workspace-level decision rather than a per-crate one.

**Reference:** `flui-layer` Step 4 commit `366f6c10`.

### "Property tests for canvas/display_list invariants"

**Goal:** Add `proptest`-based tests for:
- For any sequence of `(draw_*, save, restore, save_layer, restore)`, `Canvas::finish()` produces a consistent DisplayList.
- For any non-empty Canvas, `canvas.bounds()` contains the union of `cmd.bounds()` for all commands.
- `to_opacity(1.0)` produces a byte-equivalent DisplayList modulo Paint Cow shape.

**Files:** New `crates/flui-painting/tests/proptest_canvas.rs`, new `proptest` dev-dep.

**Named blockers:**
- `proptest` dev-dep decision.

**Reference:** Mirrors `flui-rendering` and `flui-layer` chain Outstanding entries.

### "Mutation testing with `cargo-mutants`"

**Goal:** Run `cargo-mutants` against `flui-painting` to surface untested mutation paths.

**Files:** New CI config; no source changes.

**Named blockers:**
- CI infra extension.

### "Doctest sweep" (completed during code-review fixup pass 2)

**Goal:** Verify all ~20 doc examples in the post-split files compile cleanly.

**Status:** Completed during code-review fixup pass 2. `cargo test -p flui-painting --doc` ran clean: 19 doctests detected, 18 marked `rust,ignore` by design (they show idiomatic usage that depends on `prelude::*` imports we deliberately keep out of doc-test isolation), 1 executable doctest passed, 0 failures.

---

## Notes

- **Net unsafe delta for this chain: 0.** The crate is and stays `#[forbid(unsafe_code)]` at [`src/lib.rs:151`](src/lib.rs).
- **Net LOC delta in `src/` production code:** approximately -2,000 LOC across the 4 god-module splits + dead-code deletions (canvas.rs 3305 → canvas/ 2187; display_list.rs 2434 → display_list/ 2016; text_layout.rs 1243 → text_layout/ 1161; text_painter.rs 990 → text_painter/ 873; binding.rs trimmed; WarmUpCanvas + ShaderWarmUp + DefaultShaderWarmUp deletions). New `tests/` integration files add ~580 LOC (4 files extracted from inline test blocks).
- **Post-fixup test counts** (after code-review fixup pass 2): `cargo test -p flui-painting --lib` → 23 passing; `cargo test -p flui-painting --tests` → 141 passing across 13 integration files with default features; `cargo test --no-default-features -p flui-painting --tests` → 75 passing (cosmic-text-shape-dependent tests cfg-gated out, fallback tests cfg-gated in). Combined lib+tests under default features: 164; combined lib+tests under no-default features: 93.
- **`port-check.sh` re-confirmed in Mythos chain Step 13** to cover the post-split `crates/flui-painting/src/` subdirectories (Triggers 1, 2, 3).
- **Companion docs preserved.** [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) + [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md) + [`docs/README.md`](docs/README.md) stay alongside this templated file per the [`docs/PORT.md`](../../docs/PORT.md) graft instructions.
- **CLAUDE.md drift** noted in `## Friction log`. Not addressed by this chain.
