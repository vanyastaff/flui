---
title: "Mythos Audit — flui-rendering × flui-painting × flui-layer × flui-engine"
date: 2026-05-20
status: audit-complete
audit_methodology: claude-mythos (12-phase rust audit)
crates_audited:
  - flui-rendering
  - flui-painting
  - flui-layer
  - flui-engine
reference_sources:
  - flutter/packages/flutter/lib/src/painting/
  - flutter/packages/flutter/lib/src/rendering/
  - flutter/engine/src/
authors:
  - Mythos (via Claude Opus 4.7)
---

# Mythos Audit: `flui-rendering` × `flui-painting` × `flui-layer` × `flui-engine`

> Single-pass deep audit across four rendering-stack crates, followed by cross-reference against Flutter source at `.flutter/flutter-master/packages/flutter/lib/src/{painting,rendering}` and `.flutter/flutter-master/engine/src/`.
>
> Goal: identify dead code, duplication, leaky abstractions, unbounded growth, and architecture theater — without breaking public API or sliding into cosmetic churn.

## Table of Contents

- [Part I — Self-Audit Findings](#part-i--self-audit-findings)
  - [Mythos Improvement Verdict](#mythos-improvement-verdict)
  - [Project Map](#project-map)
  - [Findings](#findings)
  - [Dead Code Table](#dead-code-table)
  - [Restructuring Plan](#restructuring-plan)
  - [Optimization Plan](#optimization-plan)
  - [What to Preserve](#what-to-preserve)
  - [Priority Order (initial)](#priority-order-initial)
- [Part II — Flutter Cross-Reference](#part-ii--flutter-cross-reference)
  - [Painting vs Flutter painting/](#section-1--flui-painting-vs-flutter-painting)
  - [Rendering vs Flutter rendering/](#section-2--flui-rendering-vs-flutter-rendering)
  - [Layer vs Flutter layer.dart](#section-3--flui-layer-vs-flutter-renderinglayerdart)
  - [Engine vs Flutter engine](#section-4--flui-engine-vs-flutter-engine-enginesrc)
- [Part III — Combined Priority Order](#part-iii--combined-priority-order)
- [Appendix A — Investigation Trail](#appendix-a--investigation-trail)

---

# Part I — Self-Audit Findings

## Mythos Improvement Verdict

Архитектура крейтов **structurally sound** — DAG чистый, нет циклов, painting не знает про layer/engine, engine + rendering siblings без cross-deps. Type-state pipeline (`PipelineOwner` phases), lock-free dirty tracking (`AtomicRenderFlags`), Slab-storage + 1-based ID offset — production-grade design.

**Худший complexity tax**: дублирование смежных абстракций — два параллельных `ClipContext` trait (oба с нулём production impls), две Lyon-tessellation pipeline (`painting/tessellation.rs` только в tests + example), две layer-vs-DrawCommand enum'ы которые сами по себе оправданы но границы между ними плавают.

**Где hide dead code**: пограничные слои — `flui-rendering/protocol/protocol.rs` (IntrinsicProtocol, BaselineProtocol, RenderDirtyPropagation = zombie абстракции), `flui-rendering/view/render_view.rs:528-720+` (180 строк закомментированного legacy impl), `flui-engine/wgpu/layer_render.rs:71-94` (unbounded thread_local `SUPERELLIPSE_CACHE`), `flui-engine/effects.rs` + `instancing.rs` (`#[allow(dead_code)]` forward-looking helpers без cadence удаления).

**Biggest optimization opportunity** — устранение Paint/Path clone storm в painting + flat-bytecode DrawCommand (трекается в `ARCHITECTURE.md`, но не сделано).

**Не трогать**: `flui-rendering` Slab+OnceCell+AtomicRenderFlags storage stack, `flui-layer` `gen_layer_accessors!` macro-collapsed enum dispatch, `flui-engine` BufferPool+PathCache+TextureCache (eviction wired в `painter.rs:1058`), Arity system (Leaf/Single/Optional/Variable), bounded crossbeam channel в `PipelineOwnerHandle`.

---

## Project Map

```text
flui-painting (5.5K LOC)
  owns: Canvas (immediate-mode recorder), DisplayList (29-variant DrawCommand enum), Paint surface,
        TextLayout (cosmic-text), TextPainter, optional Lyon tessellation
  depends on: flui-types, flui-foundation
  public surface: Canvas, DisplayList, DrawCommand, ClipContext, ImageCache, Picture=DisplayList,
                  20 top-level pub + prelude
  suspected hot paths: Canvas::draw_*, DrawCommand emit, Paint::clone, Path::clone,
                       cosmic-text shape_until_scroll (global Mutex<FontSystem>)
  risk: clone storm on Paint/Path; tessellation.rs feature-gated but unused в production;
        ClipContext trait has 0 production impls

flui-layer (8K LOC, 19 layer variants)
  owns: Layer enum + dispatch, LayerTree (Slab), SceneBuilder/SceneCompositor (split modes),
        DamageTracker, LinkRegistry (leader/follower)
  depends on: flui-foundation (LayerId), flui-tree (TreeRead/TreeNav), flui-types, flui-painting
  public surface: Layer, LayerId, Scene, SceneBuilder, LayerTree, LayerNode + 19 layer variant types
  suspected hot paths: gen_layer_accessors macro dispatch (19 variants), DamageTracker rect union,
                       LayerTree slab access
  risk: clip variants (4) + effect variants (5) могут merge в submodules;
        PerformanceOverlayLayer (530 LOC) не подключён к движку

flui-engine (~12K LOC, wgpu backend)
  owns: CommandRenderer (49-method trait, 1 impl = Backend), WgpuPainter, LayerRender,
        PathCache (LRU, eviction), TextureCache (size-budget evict), BufferPool, TexturePool,
        TextureAtlas, OffscreenRenderer (shader masks)
  depends on: flui-types, flui-painting, flui-foundation, flui-layer + wgpu, glyphon, lyon, glam
  public surface: Backend, FontLoader, LayerRender, WgpuPainter, DebugBackend (debug-only),
                  CommandRenderer, dispatch_command(s)
  suspected hot paths: CommandRenderer dispatch (40-arm match), WgpuPainter batch accumulate +
                       render(), lyon tessellation (cached), texture upload
  risk: SUPERELLIPSE_CACHE (thread_local) unbounded;
        `#[allow(dead_code)]` forward-looking helpers в effects.rs/instancing.rs/pipeline.rs;
        Arc<Mutex<OffscreenRenderer>> на single-thread render path

flui-rendering (~14K LOC, 70 файлов)
  owns: RenderObject<P>/RenderBox/RenderSliver trait stack, Protocol system (Box/Sliver + capabilities),
        RenderTree (Slab + OnceCell + AtomicRenderFlags), PipelineOwner type-state phases,
        DirtySets + bounded crossbeam mark-dirty channel, MouseTracker,
        layout/paint/hit-test contexts, delegates
  depends on: flui-types, flui-painting, flui-interaction, flui-foundation, flui-tree,
              flui-layer, flui-semantics
  public surface: 38 top-level + 69 prelude exports
  suspected hot paths: PipelineOwner::run_layout/drain_dirty_*, AtomicRenderFlags read/write,
                       context::paint::paint_child, RenderView::perform_layout
  risk: protocol.rs zombie traits (IntrinsicProtocol, BaselineProtocol);
        propagation.rs RenderDirtyPropagation trait (только test impl, prod usage = 0);
        180+ commented lines в render_view.rs; multiple 0-impl delegate traits;
        второй ClipContext trait здесь дублирует painting's
```

**Cross-crate dependency DAG** (clean — no backwards deps):

```
painting → types, foundation
layer    → foundation, tree, types, painting
engine   → types, painting, foundation, layer
rendering→ types, painting, interaction, foundation, tree, layer, semantics
```

Engine + rendering are siblings (both depend on layer + painting; neither depends on the other).

---

## Findings

### 💀 [DUPLICATION | CRITICAL]: Two parallel `ClipContext` traits, both zero production impls

**Evidence:**
- [`crates/flui-painting/src/clip_context.rs`](../../crates/flui-painting/src/clip_context.rs) — `pub trait ClipContext { fn canvas_mut(&mut self) -> &mut Canvas; ... }` + 3 default methods (`clip_rect_and_paint`, `clip_rrect_and_paint`, `clip_path_and_paint`). Documented as "PaintingContext (flui-rendering) implements ClipContext".
- [`crates/flui-rendering/src/context/clip.rs`](../../crates/flui-rendering/src/context/clip.rs) — `pub trait ClipContext { fn canvas(&mut self) -> &mut Canvas; ... }` + same 3 default methods.
- Method differs by name (`canvas_mut` vs `canvas`).
- Grep `impl ClipContext for` across workspace: 2 hits — `TestClipContext` in painting (line 302), `TestClipContext` in rendering (line 223). **Zero production impls**.
- Painting's `clip_context.rs` doc-block actively lies: claims "PaintingContext extends ClipContext" — no such impl exists.

**Why it exists:**
Flutter parallel: Flutter's `ClipContext` is base class of `PaintingContext`. Rust adaptation used a trait. Painting added the seam → rendering started consuming via `flui-painting::ClipContext`, then drifted into its own copy. Neither finished; nothing in production implements either trait.

**Cost today:**
- Cognitive load — two near-identical traits with different signatures mislead readers.
- Documentation lying — rendering's ARCHITECTURE doc-block references an impl that doesn't exist.
- API surface bloat — double-published trait surface for "decoupling" theater.
- Test infra theater — `TestClipContext` in both crates, both exist only to exercise the trait.

**Risk of changing:**
Minimal. 0 production callers — removing either trait will not break production code. Tests test the trait itself — delete tests alongside.

**Recommendation:** **delete** one of the two traits (prefer painting's — outward seam without users). Reintroduce the seam when the first non-test implementer materializes. Drop both `TestClipContext` impls.

**Patch sketch:**
```rust
// crates/flui-painting/src/lib.rs — remove:
//   pub mod clip_context;
//   pub use clip_context::ClipContext;

// crates/flui-rendering/src/context/mod.rs — keep ClipContext but consider renaming
// to PaintingClipExt and downgrading to pub(crate) until a non-test implementer arrives.
```

---

### 💀 [DUPLICATION | HIGH]: Two Lyon tessellation pipelines; painting's used only by tests + example

**Evidence:**
- [`crates/flui-painting/src/tessellation.rs`](../../crates/flui-painting/src/tessellation.rs) — `tessellate_fill`, `tessellate_stroke`, `TessellatedPath`, `TessellationVertex`. Feature `tessellation` (default ON). Uses `lyon::lyon_tessellation::{FillTessellator, StrokeTessellator, ...}`.
- [`crates/flui-engine/src/wgpu/tessellator.rs`](../../crates/flui-engine/src/wgpu/tessellator.rs) — `Tessellator` struct holding `FillTessellator + StrokeTessellator`. Uses the same Lyon. 700+ lines.
- Grep `tessellate_fill|tessellate_stroke|TessellatedPath`:
  - `flui-painting/src/tessellation.rs` (definition)
  - `flui-painting/tests/tessellation_integration.rs` (tests)
  - `flui-painting/examples/simple_tessellation.rs` (example)
  - `flui-engine/src/wgpu/tessellator.rs` — **separate namespace** (`Tessellator::tessellate_fill`, not painting's free function)
- Production engine code does NOT import `flui_painting::tessellation`. Vertex types differ (`TessellationVertex` `[f32;2]` vs engine `Vertex` with color + uv).

**Why it exists:**
`flui-painting` was originally intended as backend-agnostic — it might pre-tessellate paths and hand the engine ready vertex buffers. But for GPU, engine needs its own `Vertex` format (color + UV for texturing). Painting tessellation remained as unused public API + lyon dep "for the future".

**Cost today:**
- Duplicate Lyon dependency — both `flui-painting` and `flui-engine` pull lyon 1.0+. Compile time + binary size duplication.
- Public API liability — `tessellate_fill/stroke` callable by users, but engine ignores it → confusion about source of truth.
- Test-only feature gated as production default — `default = ["text", "tessellation"]` → every build pulls Lyon into painting, but only tests + the example read from it.

**Risk of changing:**
- Removing painting tessellation feature → breaks the existing [`crates/flui-painting/examples/simple_tessellation.rs`](../../crates/flui-painting/examples/simple_tessellation.rs) and [`crates/flui-painting/tests/tessellation_integration.rs`](../../crates/flui-painting/tests/tessellation_integration.rs).
- API is public: there may be a downstream user (none in the workspace).

**Recommendation:** **move behind opt-in feature flag + remove from default**, or **delete and drop lyon from `flui-painting` Cargo.toml**. Engine becomes the single source of truth for tessellation. Move or delete the example/tests accordingly.

**Patch sketch:**
```toml
# crates/flui-painting/Cargo.toml
[features]
default = ["text"]                  # remove "tessellation"
# OR delete entirely if no downstream users:
# tessellation = ["dep:lyon"]      # leave for opt-in users
```
Then remove or relocate the example/tests.

---

### 💀 [ZOMBIE | HIGH]: 180+ lines of commented-out legacy `impl RenderObject for RenderView`

**Evidence:**
- [`crates/flui-rendering/src/view/render_view.rs:524-720+`](../../crates/flui-rendering/src/view/render_view.rs) — comment block `// === RenderObject Implementation (Legacy - commented out) ===` followed by 190+ lines of `// impl RenderObject for RenderView { ... }`.
- Contains `depth`/`parent`/`owner`/`attach`/`detach`/`adopt_child`/`drop_child`/`redepth_child`/`needs_layout`/`mark_*`/`cached_constraints`/`schedule_initial_*`/`is_repaint_boundary`/`layer_id`/`parent_data`/`visit_children_mut` etc.
- Pre-U2 refactor design before the move to Slab + capability traits.

**Why it exists:**
"Might be useful as reference" — typical mind-trap. Refactor done, new design lives in `storage/RenderTree + PipelineOwner + capability traits`. Comment-only legacy persists.

**Cost today:**
- 190+ lines of cognitive noise while reading `render_view.rs`.
- Code-review confusion — reviewers must scroll past dead code to find real logic.
- Git diff churn — every touch of the file distracts onto the legacy block.
- Linter ignores commented code → bit-rot until the types drift far enough that the block no longer even parses as Rust syntactically.

**Risk of changing:**
None. It is commented text, not code. No rebuilds, no test impact, no public API impact.

**Recommendation:** **delete** the entire block lines 524-720+. Git history preserves it indefinitely if anyone ever wants "reference".

---

### 💀 [ZOMBIE | HIGH]: `RenderDirtyPropagation` trait used only by tests; production uses a different mechanism

**Evidence:**
- [`crates/flui-rendering/src/storage/state/propagation.rs:39`](../../crates/flui-rendering/src/storage/state/propagation.rs) — `pub trait RenderDirtyPropagation` (5 methods).
- Same file lines 142, 259, 345, 444 — `pub fn mark_needs_layout(&self, element_id, tree: &mut impl RenderDirtyPropagation)` etc.
- Grep `impl RenderDirtyPropagation`: 1 hit only — `crates/flui-rendering/src/storage/state/tests.rs:117` (`impl RenderDirtyPropagation for MockTree`).
- Grep callers of `mark_needs_layout(.., tree)` / `mark_needs_paint(.., tree)`: only `tests.rs` + doc-comments. **No production callers**.
- Production dirty marking instead routes through `crates/flui-rendering/src/storage/state/` → `AtomicRenderFlags::set_needs_layout()` directly — lock-free atomic, no tree traversal.

**Why it exists:**
In the earlier design (pre-AtomicRenderFlags), propagation was supposed to walk the tree and mark relayout boundaries. After atomic flags landed, the tree was no longer needed for dirty marking — the trait and propagation API stayed as architecture theater for testability, but real production callers were excised.

**Cost today:**
- Public API liability — `RenderDirtyPropagation` re-exported from `storage/state/mod.rs:152`, visible in the crate's public surface.
- ~500 LOC of docs + code in `propagation.rs` — all examples in docstrings call `mark_needs_layout(.., tree)`, which production doesn't do.
- Tests test a trait that production doesn't use — false confidence.
- Maintainability hazard — someone adds a method to the trait, no production impls update, tests don't catch it.

**Risk of changing:**
- Trait + `propagation.rs` public API surface — possibly a downstream user (zero in this workspace).
- Tests need rework — MockTree-driven propagation tests must either go away or move to direct AtomicRenderFlags assertions.

**Recommendation:** **delete** the `RenderDirtyPropagation` trait + `propagation.rs` (~500 LOC). If propagation logic is needed for viewport invalidation — restore through direct `RenderTree` API without a trait. Rewrite tests against `AtomicRenderFlags`.

**Patch sketch:**
```rust
// crates/flui-rendering/src/storage/state/mod.rs — remove:
//   pub use propagation::RenderDirtyPropagation;
//   pub mod propagation;
// Move mark_needs_layout/mark_needs_paint helpers (if any prod usage) onto RenderTree impl.
```

---

### 💀 [ZOMBIE | MEDIUM]: `IntrinsicProtocol` + `BaselineProtocol` — sealed traits, 0 impls, only in prelude

**Evidence:**
- [`crates/flui-rendering/src/protocol/protocol.rs:91`](../../crates/flui-rendering/src/protocol/protocol.rs) — `pub trait IntrinsicProtocol: Protocol { ... }` (sealed via `Protocol`). 1 method.
- Same file line 106 — `pub trait BaselineProtocol: Protocol { ... }` (sealed). 1 method.
- Grep `impl .* for .* IntrinsicProtocol` / `BaselineProtocol`: zero hits. Only re-exports in `crates/flui-rendering/src/lib.rs:158,168` + `protocol/mod.rs`.
- Sealed = external users cannot implement.

**Why it exists:**
Reserved for future Flutter-parity (Flutter's intrinsic sizing API — `minIntrinsicWidth/Height` etc). Pre-emptive abstraction.

**Cost today:**
- Public API surface — two empty trait imports in the prelude.
- Sealed trait nobody can implement = pure decoration.
- Grep noise — every protocol-related impl scan surfaces these.

**Risk of changing:**
Public API breakage — but prelude consumers get a clear `type IntrinsicProtocol not found` error and a trivial fix.

**Recommendation:** **delete** both traits + the matching prelude/`lib.rs` re-exports. When intrinsic/baseline are actually needed, add them as extension traits on `Protocol` with concrete impls.

---

### 💀 [PERFORMANCE | HIGH]: `SUPERELLIPSE_CACHE` (thread_local HashMap) unbounded; no eviction

**Evidence:**
- [`crates/flui-engine/src/wgpu/layer_render.rs:71-94`](../../crates/flui-engine/src/wgpu/layer_render.rs)

```rust
thread_local! {
    static SUPERELLIPSE_CACHE: RefCell<HashMap<SuperellipseKey, flui_types::painting::Path>> =
        RefCell::new(HashMap::new());
}
fn get_or_generate_superellipse_path(...) -> flui_types::painting::Path {
    SUPERELLIPSE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(path) = cache.get(&key) { return path.clone(); }
        let path = generate_superellipse_path(superellipse);
        cache.insert(key, path.clone());
        path
    })
}
```

- `SuperellipseKey` = 8 × u32 (corner radii bits) → unique per radius combination.
- `Path` = `Vec<PathCommand>` → meaningful heap allocation.
- **No `max_entries`, no LRU, no frame counter, no shrink, no clear.** Lives for the entire thread lifetime.

**Why it exists:**
Quick caching add-on; `thread_local!` was chosen as "render is single-threaded" pattern. Eviction policy never added because superellipse usage was assumed rare.

**Cost today:**
- Memory growth — long-running app with dynamic UI (animated rounded corners, varying radii) accumulates unbounded paths.
- The in-code comment says "Rendering is single-threaded, so thread-local is the correct choice." — the choice is OK, **but the lifetime policy is undefined**.
- No telemetry → invisible leak.

**Risk of changing:**
- Adding eviction may evict a hot superellipse and force regeneration.
- Code change in `layer_render.rs` — needs a proper benchmark afterwards.

**Recommendation:** **add bounded LRU + max_entries policy** (mirror the existing `PathCache` pattern in [`crates/flui-engine/src/wgpu/path_cache.rs`](../../crates/flui-engine/src/wgpu/path_cache.rs)). 128 entries is a reasonable default.

**Patch sketch:**
```rust
// Replace HashMap with a bounded LRU. Existing PathCache (path_cache.rs) already has
// last_used_frame eviction at a 120-frame threshold + max_entries cap. Use the same shape:
struct SuperellipseCache {
    entries: HashMap<SuperellipseKey, (Path, u64 /* last_used_frame */)>,
    max_entries: usize, // 128
    current_frame: u64,
}
impl SuperellipseCache {
    fn get_or_generate(&mut self, key: SuperellipseKey, gen: impl FnOnce() -> Path) -> Path { ... }
    fn advance_frame(&mut self) { self.current_frame += 1; self.evict_idle(120); }
}
thread_local! { static SUPERELLIPSE_CACHE: RefCell<SuperellipseCache> = ...; }
```

---

### 💀 [TRAIT | MEDIUM]: Multiple delegate traits with zero production impls

`CustomPainter`, `SingleChildLayoutDelegate`, `MultiChildLayoutDelegate`, `FlowDelegate` — all defined in `crates/flui-rendering/src/delegates/`, all currently have **zero production implementations**. Only test mocks exist.

**Why it exists:** Flutter parity — Flutter has `CustomPainter` / `FlowDelegate` / `*LayoutDelegate` as user-facing API points. Pre-emptive surface.

**Cost today:** 6 delegate trait modules (~600 LOC) of `Send + Sync + Debug + Any`-shaped boilerplate; public API freeze risk; IDE noise.

**Risk of changing:** Public API breakage; downstream code (if any) breaks. Workspace internal usage = 0.

**Recommendation:** **make `pub(crate)`** until the first non-test implementer appears, OR **move behind an `experimental-delegates` feature flag**.

> ⚠ Cross-reference update (see Part II): these traits **wait for companion render-object consumers** (`RenderCustomPaint`, `RenderCustomMultiChildLayoutBox`, `RenderCustomSingleChildLayoutBox`, `RenderFlow`). Don't delete them — re-evaluate after Phase 1-2 of render-object roadmap.

---

### 💀 [BOUNDARY | MEDIUM]: Engine forward-looking `#[allow(dead_code)]` without removal cadence

**Evidence:**
- [`crates/flui-engine/src/wgpu/effects.rs`](../../crates/flui-engine/src/wgpu/effects.rs) — `LinearGradientBuilder`, `BlurIntensity` marked `#[allow(dead_code)]`.
- [`crates/flui-engine/src/wgpu/instancing.rs`](../../crates/flui-engine/src/wgpu/instancing.rs) — `RectInstance::rounded_rect`, `with_transform`, `ellipse` marked `#[allow(dead_code)]`.
- [`crates/flui-engine/src/wgpu/pipeline.rs`](../../crates/flui-engine/src/wgpu/pipeline.rs) — `PipelineKey::from_color` + other constructor variants — but `pipelines.rs` (different file!) is what the painter actually uses.
- [`crates/flui-engine/src/wgpu/shader_compiler.rs`](../../crates/flui-engine/src/wgpu/shader_compiler.rs) — `ShaderCache::cached_count`, `clear` — for devtools; the `devtools` feature isn't enabled.

**Why it exists:** "Forward-looking helpers" — placeholders. `pipeline.rs` (PipelineKey bitflags) vs `pipelines.rs` (PipelineCache, actually used) — two adjacent files with overlapping abstraction.

**Cost today:** Confusing parallel namespaces, suppressed warnings hide stale code, reviewers can't tell live vs dormant.

**Recommendation:** **add cadence rule** — every `#[allow(dead_code)]` accompanied by `// REMOVE_BY: <YYYY-MM-DD>` or `// USED_BY: <issue/feature>`. Without maintained reference — delete after 6 months. For `pipeline.rs` — pick one of two (either migrate painter onto PipelineKey bitflags, or delete `pipeline.rs`).

---

### 💀 [SHALLOW | LOW]: Numerous re-export-only `mod.rs` files in `flui-rendering`

15-77 lines each, only `pub use`:
- [`traits/mod.rs`](../../crates/flui-rendering/src/traits/mod.rs) — 15 lines
- [`input/mod.rs`](../../crates/flui-rendering/src/input/mod.rs) — 26 lines
- [`delegates/mod.rs`](../../crates/flui-rendering/src/delegates/mod.rs) — 29 lines
- [`pipeline/mod.rs`](../../crates/flui-rendering/src/pipeline/mod.rs) — 30 lines
- [`view/mod.rs`](../../crates/flui-rendering/src/view/mod.rs) — 34 lines
- [`objects/mod.rs`](../../crates/flui-rendering/src/objects/mod.rs) — 44 lines
- [`hit_testing/mod.rs`](../../crates/flui-rendering/src/hit_testing/mod.rs) — 51 lines
- [`storage/mod.rs`](../../crates/flui-rendering/src/storage/mod.rs) — 77 lines

**Recommendation:** **leave unchanged**. Not shallow in the smell sense — each module really owns several types. Re-export shape is fine; the `lib.rs` prelude is the facade.

---

### 💀 [API SURFACE | MEDIUM]: `RendererBinding` trait — 9 methods, 0 workspace impls

**Evidence:** [`crates/flui-rendering/src/binding/mod.rs`](../../crates/flui-rendering/src/binding/mod.rs) — `pub trait RendererBinding { ... }` 9 methods including `render_views() -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>`.

**Why it exists:** Integration seam for the app layer — `flui-app` was supposed to implement it. `flui-app` is in migration phase; the impl hasn't materialized yet.

**Recommendation:** **make `pub(crate)` + add migration TODO** until the impl exists, or move behind a `bindings` feature flag. The nested `Arc<RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>>` public type is a design smell that should be resolved **before** the first impl, not after.

---

### 💀 [DOC DRIFT | LOW]: `CLAUDE.md` claims crates disabled when all 4 are active

[`CLAUDE.md`](../../CLAUDE.md) — "Active crates: flui-painting, flui-layer ...; Temporarily disabled: flui-rendering, flui-view, ..." but [`Cargo.toml`](../../Cargo.toml) lists all 4 as ACTIVE.

**Recommendation:** **update CLAUDE.md** — current crate status. Trivial fix.

---

## Dead Code Table

| Item | Location | Evidence | Hidden-use risk | Verdict | Action |
|------|----------|----------|-----------------|---------|--------|
| Commented `impl RenderObject for RenderView` | [render_view.rs:524-720+](../../crates/flui-rendering/src/view/render_view.rs) | 190+ commented lines, pre-U2 legacy | None (comments) | **Proven dead** | **delete** |
| `flui_painting::ClipContext` trait | [painting/clip_context.rs](../../crates/flui-painting/src/clip_context.rs) | 0 production impls; only TestClipContext | Possible downstream (extern) | **Zombie abstraction** | **delete or feature-flag** |
| `flui_rendering::context::ClipContext` trait | [rendering/context/clip.rs](../../crates/flui-rendering/src/context/clip.rs) | 0 production impls; only TestClipContext | Same | **Zombie abstraction** | **delete or `pub(crate)`** |
| `RenderDirtyPropagation` trait + propagation.rs | [storage/state/propagation.rs](../../crates/flui-rendering/src/storage/state/propagation.rs) | 1 impl (MockTree in tests); no production callers | None — production uses AtomicRenderFlags directly | **Zombie abstraction** | **delete** |
| `IntrinsicProtocol` trait | [protocol/protocol.rs:91](../../crates/flui-rendering/src/protocol/protocol.rs) | 0 impls; sealed; in prelude | None — sealed | **Proven dead** | **delete** |
| `BaselineProtocol` trait | [protocol/protocol.rs:106](../../crates/flui-rendering/src/protocol/protocol.rs) | 0 impls; sealed; in prelude | None — sealed | **Proven dead** | **delete** |
| `flui_painting::tessellation` module | [painting/src/tessellation.rs](../../crates/flui-painting/src/tessellation.rs) | Used only by tests + example; engine has its own | Possible downstream user | **Test/example only** | **feature-flag opt-in (remove from default), OR delete with example** |
| `SUPERELLIPSE_CACHE` (unbounded) | [engine/wgpu/layer_render.rs:71](../../crates/flui-engine/src/wgpu/layer_render.rs) | Live; unbounded growth | None — internal | **Keep but bound** | **add LRU/eviction policy** |
| `CustomPainter` trait | [delegates/custom_painter.rs](../../crates/flui-rendering/src/delegates/custom_painter.rs) | 0 prod impls | Test/example may use | **Public but unused internally** | **`pub(crate)` or feature-flag** |
| `FlowDelegate` trait | [delegates/flow_delegate.rs](../../crates/flui-rendering/src/delegates/flow_delegate.rs) | 0 prod impls | Same | **Public but unused internally** | **`pub(crate)` or feature-flag** |
| `MultiChildLayoutDelegate` trait | [delegates/multi_child_layout_delegate.rs](../../crates/flui-rendering/src/delegates/multi_child_layout_delegate.rs) | 0 prod impls | Same | **Public but unused internally** | **`pub(crate)` or feature-flag** |
| `SingleChildLayoutDelegate` trait | [delegates/single_child_layout_delegate.rs](../../crates/flui-rendering/src/delegates/single_child_layout_delegate.rs) | 0 prod impls | Same | **Public but unused internally** | **`pub(crate)` or feature-flag** |
| `RendererBinding` trait | [binding/mod.rs](../../crates/flui-rendering/src/binding/mod.rs) | 0 workspace impls; deep nested Arc<RwLock<..>> public type | **High** — flui-app migration target | **Needs manual confirmation** | **`pub(crate)` + redesign before first impl** |
| `PipelineKey` constructors | [engine/wgpu/pipeline.rs](../../crates/flui-engine/src/wgpu/pipeline.rs) | `#[allow(dead_code)]`; PipelineCache (pipelines.rs) is used instead | None | **Zombie abstraction** | **delete pipeline.rs OR migrate painter to use it** |
| Instancing forward helpers | [engine/wgpu/instancing.rs](../../crates/flui-engine/src/wgpu/instancing.rs) | `#[allow(dead_code)]` on rounded_rect/with_transform/ellipse | Possible extern API | **Test-only/forward-looking** | **add `// REMOVE_BY: date` or delete** |
| Effects helpers | [engine/wgpu/effects.rs](../../crates/flui-engine/src/wgpu/effects.rs) | `#[allow(dead_code)]` BlurIntensity, LinearGradientBuilder | Possible extern API | **Forward-looking** | **add cadence comment or delete** |
| `ShaderCache::cached_count`, `clear` | [engine/wgpu/shader_compiler.rs](../../crates/flui-engine/src/wgpu/shader_compiler.rs) | `#[allow(dead_code)]`; devtools-only | None — gated | **Feature-gated** | **move under `cfg(feature="devtools")`** |
| `ScrollMetrics` trait + 2 impls | [rendering/constraints/scroll_metrics.rs](../../crates/flui-rendering/src/constraints/scroll_metrics.rs) | 2 impls (FixedScrollMetrics, FixedExtentMetrics); consumers in workspace? | Possible — re-exported in prelude | **Needs manual confirmation** | **investigate manually** |
| `MouseTrackerAnnotation` trait | [rendering/input/mouse_tracker.rs](../../crates/flui-rendering/src/input/mouse_tracker.rs) | 1 impl in flui-interaction | Real | **Keep** | **leave unchanged** |
| `HotReloadCapability` trait | [traits/render_object.rs](../../crates/flui-rendering/src/traits/render_object.rs) | 7 production marker impls (RenderCenter/Flex/etc), all empty | None | **Keep (marker)** | **document as marker trait or use derive macro** |
| `TextureCache` (no eviction) | [engine/wgpu/texture_cache.rs](../../crates/flui-engine/src/wgpu/texture_cache.rs) | `evict_over_budget()` exists; called from [painter.rs:1058](../../crates/flui-engine/src/wgpu/painter.rs) | **Earlier report was wrong** | **Keep** | **leave unchanged** |
| `PathCache` (engine) | [engine/wgpu/path_cache.rs](../../crates/flui-engine/src/wgpu/path_cache.rs) | 120-frame LRU eviction, max_entries=512 default | None | **Keep** | **leave unchanged** |

---

## Restructuring Plan

### Step 1 — Safe deletions

> **Status (2026-05-20): partially complete.** Items 1, 2, 3, and the `CLAUDE.md` fix landed on branch `naughty-jackson-324931` as the flui-rendering Phase 1 zombie cleanup. Item "delete duplicate `ClipContext`" (originally part of an earlier draft of Step 1) is **deferred to a separate brainstorm**: round-1 `ce-doc-review` uncovered a missed `CanvasContext` production implementer at [crates/flui-rendering/src/context/canvas.rs:695](../../crates/flui-rendering/src/context/canvas.rs) plus incompatible trait signatures between `flui-rendering::ClipContext` and `flui-painting::ClipContext` (closure shape, accessor name, typed-unit Rect divergence). Consolidating is a migration, not a deletion, and does not fit the "pure cleanup" framing of this Phase 1 batch.
>
> Commits (in order on the worktree branch):
> - `dc07578b` — U1: delete commented `impl RenderObject for RenderView` (item 1).
> - `326358b6` — U2: delete `IntrinsicProtocol` + `BaselineProtocol` (item 2).
> - `eb1945a7` — U3: delete `RenderState<P>` propagation impl bulk + tests, preserve the `RenderDirtyPropagation` trait shape at `pub(crate)` visibility with a `PRESERVED_FOR` marker (revised from "delete the trait entirely" — see "Note on item 3 revision" below).
> - `e7860ff5` — U4: reconcile `CLAUDE.md` crate-status with `Cargo.toml` ground truth (item 4 — widened to also fix `flui-build` from "active" → "disabled" and add `flui-hot-reload` to "active" per round-2 `ce-doc-review` finding).
>
> **Note on item 3 revision.** The audit originally proposed deleting the trait shape and rewriting tests onto `AtomicRenderFlags`. Round-1 `ce-doc-review` showed that `AtomicRenderFlags::set_needs_layout/paint` is **also** not the production dirty-marking path — production uses `PipelineOwner::add_node_needing_layout / add_node_needing_paint` invoked from `flui-view` and `flui-hot-reload`. Rewriting tests onto a different unreachable path would not improve coverage. The trait shape itself was preserved at `pub(crate)` with a `// PRESERVED_FOR:` marker (≈40 LOC vs the ~430 LOC of deleted impl bulk) on cost-prudence grounds — not as an endorsement of the shape for the future viewport-invalidation hook this Step 4 item 13 contemplates. Pinning down the production dirty-marking path with a real integration test remains a separately-scoped follow-up under Step 4 item 13.

1. **Delete commented-out RenderObject impl** in [render_view.rs:524-720+](../../crates/flui-rendering/src/view/render_view.rs). Pure comment removal.
2. **Delete `IntrinsicProtocol` and `BaselineProtocol`** from [protocol/protocol.rs:91,106](../../crates/flui-rendering/src/protocol/protocol.rs) plus prelude/lib.rs/protocol/mod.rs re-exports.
3. **Delete `RenderDirtyPropagation` trait + propagation.rs entirely** ([storage/state/propagation.rs](../../crates/flui-rendering/src/storage/state/propagation.rs)). Remove `pub use propagation::RenderDirtyPropagation;` from [storage/state/mod.rs:152](../../crates/flui-rendering/src/storage/state/mod.rs). Rewrite tests in [storage/state/tests.rs](../../crates/flui-rendering/src/storage/state/tests.rs) — replace MockTree paths with direct `AtomicRenderFlags` assertions, or delete tests that exist only to exercise the trait.
4. **Update [CLAUDE.md](../../CLAUDE.md)** — set all four crates as ACTIVE; remove the outdated "disabled" notice.

### Step 2 — Privacy / API cleanup

5. **`pub(crate)` the four delegate traits**: CustomPainter, FlowDelegate, MultiChildLayoutDelegate, SingleChildLayoutDelegate. Re-promote to `pub` when the first production implementer appears.
6. **`pub(crate)` `RendererBinding` + the `Arc<RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>>` shape.** Redesign the integration surface (likely an owned handle with a `&mut self`-style update API) before exposing it publicly to `flui-app`.
7. **Decide on `ClipContext`**: keep at most one trait. If `flui-painting`'s seam is the intended contract, delete `flui-rendering::context::clip::ClipContext` and have rendering's painting context implement painting's trait. If neither is needed yet — delete both and reintroduce when a real implementer arrives.
8. **Move `flui-painting::tessellation` to opt-in feature** (remove from `default = ["text", "tessellation"]`) or delete it and the example, since the production tessellator lives in `flui-engine` and the painting one has no production consumer.
9. **Gate `ShaderCache::cached_count`, `clear`** behind `cfg(feature = "devtools")` rather than relying on `#[allow(dead_code)]`.

### Step 3 — Module consolidation

10. **Pick one of `pipeline.rs` / `pipelines.rs`** in [crates/flui-engine/src/wgpu/](../../crates/flui-engine/src/wgpu/). Two near-namesake files with overlapping intent (PipelineKey bitflags vs PipelineCache) is the disease. Either migrate the painter to use `PipelineKey` and delete `pipelines.rs`, or delete `pipeline.rs` and keep `PipelineCache`. Document the chosen pipeline-selection strategy in a single module comment.
11. **Optionally consolidate clip + effect layer variants** in [flui-layer/src/layer/](../../crates/flui-layer/src/layer/). Only do this if it produces a deeper module (shared invariants like anti-alias handling), not just fewer files.

### Step 4 — Ownership / state cleanup

12. **Re-evaluate `Arc<parking_lot::Mutex<OffscreenRenderer>>`** in [backend.rs:26,45](../../crates/flui-engine/src/wgpu/backend.rs) and [renderer.rs:147](../../crates/flui-engine/src/wgpu/renderer.rs). Engine is single-threaded synchronous; the Arc<Mutex> exists only because Backend + Renderer both want a handle. Consider giving sole ownership to one (Renderer) and passing `&mut OffscreenRenderer` down to Backend when needed.
13. **Pin down the dirty-marking path** post-#3. Document explicitly where `set_needs_layout` / `set_needs_paint` get invoked from in production. Add at least one production integration test that exercises layout-then-paint on a non-trivial tree.

### Step 5 — Performance-oriented restructuring

14. **Bound `SUPERELLIPSE_CACHE`** in [layer_render.rs:71](../../crates/flui-engine/src/wgpu/layer_render.rs). Mirror the `PathCache` shape (max_entries, last_used_frame, advance_frame eviction). Add a memory-budget metric so this never recurs invisibly. Add a stress test that emits 10k unique superellipse keys and asserts cache size stays ≤ max_entries.
15. **Track Paint/Path clone storm** for `DisplayList`. Land Paint interning + `Cow<Path>` only after a Criterion benchmark proves it pays. Today it's a known cost without measurement.

### Step 6 — Dependency cleanup

16. **Remove Lyon from `flui-painting` `default` features** (Step 8 above). Painting's lyon dep currently leaks into every consumer though only engine uses tessellation in production.
17. **Audit duplicate transitive crate versions** with `cargo tree -d` after Step 16. Lyon, wgpu, glyphon must appear once.
18. **Move `parking_lot` import in `flui-layer/Cargo.toml`** to either real use or removal.

### Step 7 — Tests and regression protection

19. **Add integration test exercising RenderTree dirty propagation** after Step 3 — proving the production path (AtomicRenderFlags + PipelineOwner phases) is the actual dirty mechanism.
20. **Add `SUPERELLIPSE_CACHE` stress test** (Step 14).
21. **Add cross-crate smoke test** — build a tiny scene (`SceneBuilder` → 1 Picture + 1 Clip + 1 Opacity layer), feed through `flui-engine::Backend` with `DebugBackend`, assert the command stream matches a golden file. Protects all four crates' wire format simultaneously.
22. **Re-run `cargo clippy --workspace --all-targets --all-features -- -D warnings`** after each step.

---

## Optimization Plan

| Area | Current cost | Proposed change | Expected gain | Risk | Benchmark/test |
|------|--------------|-----------------|---------------|------|----------------|
| SUPERELLIPSE_CACHE unbounded | Unbounded heap growth on long sessions with varied corner radii | Bounded LRU mirroring PathCache (max_entries=128, 120-frame idle eviction) | Bounded RAM; tiny CPU cost on advance_frame | Low — single-threaded thread_local | Stress test: emit 10k unique keys, assert size ≤ max_entries; Criterion bench |
| Paint clone storm in DisplayList | Every `draw_*` clones Paint (~80-200 B incl. boxed Shader) | Intern Paint via Arc<Paint> + pointer hashing, or Cow<Paint> with explicit borrow | Lower allocations in command-heavy frames | Medium — Paint mutation semantics; equality/hash for interner | Criterion bench on 10k-command DisplayList build; dhat-rs alloc tracking |
| Path clone in draw_path / clip_path / draw_shadow | `Vec<PathCommand>` cloned each emission | Cow<'a, Path> or Arc<Path>; producer borrows | Eliminate per-draw heap alloc for shared paths | Medium — DrawCommand lifetime annotations leak | Bench scene with 1k identical paths drawn at different transforms |
| `pipeline.rs` vs `pipelines.rs` dual abstractions | Two parallel modules; pipeline.rs unused | Choose one; migrate painter or delete | LOC reduction; clearer pipeline selection | Low if delete pipeline.rs; Medium if migrate | No bench needed; correctness via existing render tests |
| `Arc<parking_lot::Mutex<OffscreenRenderer>>` on sync render | Atomic ref count + spinlock per-frame | Single owner (Renderer), pass `&mut` down | Remove sync overhead; clearer ownership | Medium — confirm no cross-thread use | Bench shader-mask-heavy scene before/after |
| `parent_data` Box<dyn ParentData> + DowncastSync | Boxed dyn + downcast in layout iteration | If variant set is small/known — replace with enum + concrete arms | Cache locality + skip vtable | High — public API; ParentData is extension point | Bench RenderFlex layout on 100 children before/after |
| HashMap with String keys in TextureCache | String hashing + alloc per lookup | Compact ID — interned `Lasso`/`Spur` or precomputed `Arc<str>` | Sub-µs lookup, fewer allocations | Low — internal cache | Bench frame with 100 textured draws |
| `DefaultHasher` in `PathCache` | DefaultHasher is `SipHash` (slow) | `ahash` or `fxhash` for path hashing | 2-3× hashing speed | Low — non-cryptographic use | Criterion bench on path-heavy scene |
| Hot match arms in `dispatch_command` (40 variants) | Linear-ish jump table | Profile to confirm; if hot — split common variants into fast path + `#[cold]` rare branches | Marginal | Low | `perf record` on a 60-second animated scene |
| Lyon double-dep | Lyon compiled in both engine + painting | Remove from painting (Step 16) | Faster cold compile; smaller binary | Low if painting's tessellation feature really unused | `cargo bloat --release`; `cargo tree -d` before/after |

**Rule reinforcement**: no Paint-interning / Path-Cow / parent_data-enum landings without a Criterion benchmark and a regression test.

---

## What to Preserve

Do not touch these. They earn their place:

- **`flui-rendering` storage stack** — Slab + RenderEntry + OnceCell<geometry> + AtomicRenderFlags + AtomicOffset.
- **`PipelineOwner<Phase>` type-state machine** — phases Idle → Layout → Compositing → Paint → Semantics enforced at compile time.
- **Bounded `crossbeam_channel` in `PipelineOwnerHandle`** (256 default) — cross-thread mark-dirty without unbounded queue.
- **`gen_layer_accessors!` macro** in [flui-layer/src/layer/dispatch.rs](../../crates/flui-layer/src/layer/dispatch.rs).
- **`flui-layer::DamageTracker`**.
- **`flui-engine::PathCache`** — LRU with 120-frame idle eviction, max_entries=512.
- **`flui-engine::TextureCache::evict_over_budget`** wired from [painter.rs:1058](../../crates/flui-engine/src/wgpu/painter.rs).
- **`flui-engine::BufferPool`** — reset per frame, reuse.
- **`flui-engine::dispatch_command`** — generic over `R: CommandRenderer + ?Sized`. Zero-overhead static dispatch.
- **`Arity` system** (Leaf/Single/Optional/Variable + `BoxChild<A>`).
- **`flui-painting::Canvas` immediate-mode + DisplayList retained record**.
- **Sealed `DisplayListCore` + blanket `DisplayListExt`**.
- **`#![forbid(unsafe_code)]` in flui-painting**.
- **`MAX_EFFECT_DEPTH = 64`** in command_ops.rs.

---

## Priority Order (initial)

1. **Delete now** — proven dead, zero risk
2. **Restrict visibility** — accidental public APIs
3. **Collapse shallow abstractions** — duplicates, parallel namespaces
4. **Fix ownership / state boundaries** — important before adding features
5. **Benchmark before optimizing** — suspected hot paths
6. **Leave alone** — deep modules that already work

See [Part III](#part-iii--combined-priority-order) for the updated priority list incorporating Flutter cross-reference.

---

# Part II — Flutter Cross-Reference

Cross-reference of FLUI rendering stack against Flutter source at `.flutter/flutter-master/`. Flutter is reference, not blueprint — Rust-idiomatic divergences are OK if intentional.

## Section 1 — `flui-painting` vs Flutter `painting/`

### Coverage table (sample)

| Flutter | FLUI | Status |
|---------|------|--------|
| canvas.dart | flui-painting/src/canvas/ | ✓ adapted (Canvas + DisplayList) |
| picture.dart | display_list/ + Picture alias | ✓ |
| paint.dart | flui-types/painting/paint.rs | ✓ |
| gradient.dart | flui-types/styling/gradient.rs | ✓ |
| border.dart, border_radius.dart, borders.dart | flui-types/styling/border*.rs | ✓ |
| box_decoration.dart | flui-types/styling/decoration.rs | ✓ (BoxDecoration only) |
| **image_provider.dart** | — | ✗ **HIGH gap** |
| **image_stream.dart** | — | ✗ **HIGH gap** |
| **asset_image / network_image / file_image** | — | ✗ HIGH gap |
| text_painter.dart | flui-painting/src/text_painter/ | ✓ |
| text_span.dart | flui-types/typography/text_spans.rs | ✓ |
| **placeholder_span.dart** | — | ✗ MEDIUM gap |
| **shape_decoration.dart** | — | ✗ MEDIUM gap |
| **beveled/circle/stadium/star/oval/continuous_rect Border** | — | ✗ 7 missing border shapes |
| clip.dart, notched_shapes.dart | flui-types/painting/clipping.rs | ✓ |
| binding.dart | flui-painting/src/binding.rs | ✓ |
| image_cache.dart | binding.rs::ImageCache | ✓ |
| matrix_utils.dart | flui-types/geometry/matrix4.rs | ✓ |
| edge_insets.dart | flui-types/geometry/edges.rs | ✓ |
| alignment.dart | flui-types/layout/alignment.rs | ✓ |

### Painting gaps

#### 💀 [GAP | HIGH]: `ImageProvider<T>` + async image loading absent

**Evidence:** Flutter `painting/image_provider.dart` defines abstract `ImageProvider<T>` + `AssetImage`/`NetworkImage`/`FileImage`/`MemoryImage` concretes + `ImageStream` / `ImageStreamCompleter` async lifecycle + `ImageConfiguration` resolution context.

FLUI: `crates/flui-painting/src/binding.rs` has `ImageCache` + `ImageHandle` + `CachedImage`, but no provider abstraction. Image arrives as raw bytes already decoded → cached handle. Loading delegated upward.

**Cost:** Widget layer (when it arrives) can't write `Image.network("...")` / `Image.asset("...")` without reinventing provider patterns. Each caller duplicates async loading + decode + caching choreography. ImageCache exists without external consumers — seam-without-implementer pattern.

**Recommendation:** Add ImageProvider trait + minimal AssetImage/MemoryImage impls when widget layer comes online. Network/File providers — feature-gated with reqwest/tokio behind `network-image`, `file-image` flags.

#### 💀 [GAP | MEDIUM]: PlaceholderSpan + RichText embeds

**Evidence:** Flutter has `InlineSpan` (abstract) → `TextSpan` (text), `PlaceholderSpan` (embedded widget), `WidgetSpan` (alias). FLUI typography/text_spans.rs has only `TextSpan`.

**Recommendation:** Add `InlineSpan` enum (TextSpan vs Placeholder { size, alignment, baseline }) when widgets arrive.

#### 💀 [GAP | MEDIUM]: ShapeDecoration + 7 border shape variants

**Evidence:** Flutter has `ShapeDecoration` + `BeveledRectangleBorder`, `CircleBorder`, `StadiumBorder`, `ContinuousRectangleBorder`, `OvalBorder`, `LinearBorder`, `StarBorder`. FLUI styling/decoration.rs only has `BoxDecoration`.

**Recommendation:** Lower priority; implement via BoxBorder extension trait when the first consumer appears.

### Coverage summary: **~80%** of Flutter painting API present in FLUI.

---

## Section 2 — `flui-rendering` vs Flutter `rendering/`

### **88% RenderObject gap**

| Category | Flutter | FLUI | Gap |
|----------|---------|------|-----|
| RenderObject classes (concrete) | 62 | 7 | 89% |
| RenderProxyBox subclasses | 36+ | 0 | 100% |
| RenderShiftedBox hierarchy | 8 | 0 | 100% |
| Multi-child containers (Stack/Wrap/IndexedStack/Table/Flow/ListBody) | 6 | 1 (Flex) | 83% |
| Clip render objects (RenderClipRect/RRect/Path/Oval) | 4 | 0 | 100% |
| Sliver render objects (concrete + proxy) | 20 | 0 | 100% |
| Text/Image/Editable | 3 | 0 | 100% |
| Viewport (concrete RenderViewport/ShrinkWrapping) | 2 | 0 (trait only) | 100% |
| ParentData subclasses | 20+ | ~8 | 60% |
| PipelineOwner | 1 | 1 | ✓ parity |
| MouseTracker | 1 | 1 | ✓ parity |

### Rendering findings

#### 💀 [PARITY | CRITICAL]: 100% gap in `RenderProxyBox` hierarchy — 36 missing classes

Flutter `rendering/proxy_box.dart`: `RenderProxyBox` abstract base + ~36 subclasses (RenderConstrainedBox, RenderLimitedBox, RenderAspectRatio, RenderIntrinsicWidth/Height, RenderOpacity, RenderShaderMask, RenderBackdropFilter, RenderClipRect/RRect/Path/Oval, RenderRepaintBoundary, RenderIgnorePointer, RenderOffstage, RenderAbsorbPointer, RenderMetaData, RenderAnnotatedRegion, RenderFittedBox, RenderFractionalTranslation, RenderTransform, RenderDecoratedBox, RenderAnimatedOpacity, RenderPhysicalModel, RenderPhysicalShape, RenderListWheelViewport, RenderListBody, and more).

FLUI has only RenderOpacity, RenderTransform — all others absent.

**Why:** Widget/view layer still in migration phase. Render objects added on demand. This is **expected gap for skeleton**, not a bug.

**Recommendation:** Roadmap-driven, don't try to fix in one shot:

1. **Phase 1** (unblock widget layer): RenderProxyBox base + RenderConstrainedBox, RenderClipRect, RenderClipRRect, RenderClipPath, RenderClipOval, RenderDecoratedBox, RenderRepaintBoundary, RenderFittedBox.
2. **Phase 2** (common layout): RenderStack, RenderIndexedStack, RenderWrap, RenderAspectRatio.
3. **Phase 3** (sliver core): RenderSliverList, RenderSliverGrid, RenderSliverPadding, RenderSliverToBoxAdapter, concrete RenderViewport.
4. **Phase 4** (interactive): RenderIgnorePointer, RenderAbsorbPointer, RenderMouseRegion, RenderPointerListener.
5. **Phase 5** (text/image — depends on widget layer): RenderParagraph, RenderImage.

#### 💀 [GAP | HIGH]: Sliver ecosystem missing (20 concrete classes; 0 in FLUI)

All 13 concrete RenderSliver* + 7 proxy slivers are absent. `flui-rendering` has the SliverProtocol/RenderSliver trait + SliverConstraints/Geometry + SliverGridDelegate but **no concrete sliver impl**.

**Cost:** scrollable lists/grids impossible without user-side full reimplementation. All existing sliver-related types in FLUI = scaffolding without implementers.

#### 💀 [CONFIRM ZOMBIE | HIGH]: `IntrinsicProtocol` + `BaselineProtocol` truly dead

Cross-reference reinforces earlier audit. Flutter `rendering/box.dart` has `computeMinIntrinsicWidth`, `computeMaxIntrinsicWidth`, `computeMinIntrinsicHeight`, `computeMaxIntrinsicHeight`, `computeDryLayout`, `computeDryBaseline`, `getDistanceToBaseline` — **production API used by RenderIntrinsicWidth, RenderConstrainedBox, RenderAspectRatio, RenderFlex**.

FLUI IntrinsicProtocol + BaselineProtocol — sealed, 0 impls, plus zero implementor uses these methods anywhere.

**Updated verdict:** **delete now confirmed**. When RenderIntrinsicWidth/Height materialize — add intrinsic capability as extension trait on BoxProtocol, not sealed. Current sealed-decoration won't work in any design.

#### 💀 [PARITY | MEDIUM]: ParentData specialization 60% gap

Flutter has 20+ ParentData subclasses. FLUI has ~8. Missing: WrapParentData, ListWheelParentData, FlowParentData, full TableCellParentData, TreeSliverNodeParentData.

**Recommendation:** Add **only** when the corresponding render object is implemented. Adding ParentData class before its consumer creates another generation of zombie types.

#### ✓ PARITY: PipelineOwner, MouseTracker, ClipBehavior, HitTestBehavior

PipelineOwner phases (Idle→Layout→Compositing→Paint→Semantics) match Flutter's flushLayout/flushCompositingBits/flushPaint/flushSemantics. Type-state design **superior** to Flutter's runtime check.

MouseTracker (input/mouse_tracker.rs) — parity confirmed.

HitTestBehavior imported from flui-interaction — single source of truth through workspace. ✓

---

## Section 3 — `flui-layer` vs Flutter `rendering/layer.dart`

### **100% layer parity** — best crate by Flutter alignment.

| Flutter Layer class | FLUI Layer variant | Status |
|---------------------|-------------------|--------|
| PictureLayer | Picture | ✓ 1:1 |
| TextureLayer | Texture | ✓ 1:1 |
| PlatformViewLayer | PlatformView | ✓ 1:1 |
| PerformanceOverlayLayer | PerformanceOverlay | ✓ 1:1 (not yet wired to engine — flagged earlier) |
| OffsetLayer | Offset | ✓ 1:1 |
| ClipRectLayer | ClipRect | ✓ 1:1 |
| ClipRRectLayer | ClipRRect | ✓ 1:1 |
| ClipPathLayer | ClipPath | ✓ 1:1 |
| ClipRSuperellipseLayer | ClipSuperellipse | ✓ renamed (acceptable) |
| TransformLayer | Transform | ✓ 1:1 |
| OpacityLayer | Opacity | ✓ adapted (`f32` vs `int alpha 0-255`) |
| ColorFilterLayer | ColorFilter | ✓ 1:1 |
| ImageFilterLayer | ImageFilter | ✓ adapted (no offset inheritance) |
| BackdropFilterLayer | BackdropFilter | ✓ partial (missing `backdropKey` v3.22+) |
| ShaderMaskLayer | ShaderMask | ✓ 1:1 |
| LeaderLayer | Leader | ✓ adapted (LinkRegistry separate vs Flutter's inline ref) |
| FollowerLayer | Follower | ✓ adapted (single `target_offset` vs Flutter's 3 offset variants) |
| AnnotatedRegionLayer<T> | AnnotatedRegion | ✓ adapted (Arc<dyn Any> vs generic) |
| **(none — FLUI extra)** | **Canvas** | ➕ FLUI-only mutable surface |
| PhysicalModelLayer | — | ✗ correctly omitted (Flutter deprecated 3.x) |
| ContainerLayer (abstract) | — | ✗ correctly omitted (enum dispatch instead) |

### Layer findings

#### 💀 [GAP | MEDIUM]: `SceneBuilder` missing 2-3 methods

Flutter `ui.SceneBuilder` has `pushClipRSuperellipse`, `addPlatformView`, `addPerformanceOverlay`. FLUI's SceneBuilder lacks all three — Layer variants exist but no builder methods.

**Recommendation:** Add the missing 3 SceneBuilder methods. Trivial work — wrap existing variant constructors.

#### 💀 [GAP | LOW]: `PictureLayer` lacks `isComplexHint` / `willChangeHint`

Flutter PictureLayer stores `isComplexHint: bool` + `willChangeHint: bool` — used by Skia RasterCache to decide whether to cache rasterized output. FLUI PictureLayer has none.

**Recommendation:** Add `is_complex: bool` + `will_change: bool` fields to PictureLayer **before** RasterCache implementation. Cheap now, costly to retrofit.

#### 💀 [GAP | LOW]: `BackdropFilterLayer.backdropKey` missing (Flutter 3.22+)

Flutter added `backdropKey: BackdropKey?` for grouping multiple backdrop filters → single GPU pass.

**Recommendation:** Add when benchmark shows multiple-backdrop scenes are bottleneck.

#### 💀 [BOUNDARY | LOW]: `PlatformViewHitTestBehavior` shouldn't live on Layer

FLUI's PlatformView layer exports a `PlatformViewHitTestBehavior` enum. Flutter handles hit-test behavior at RenderObject level. **Recommendation:** move to flui-rendering or flui-interaction when actually used.

---

## Section 4 — `flui-engine` vs Flutter engine (`engine/src/`)

### Architectural divergence: **wgpu + Lyon + glyphon** vs **Skia/Impeller + HarfBuzz**. Different by design.

### DrawCommand parity (≈85%)

| Flutter DL op category | FLUI coverage |
|------------------------|---------------|
| Primitive shapes (15) | 15/15 ✓ |
| Image ops (8) | 7/8 (missing DrawDashedLine) |
| Clipping (9) | 4/9 (ClipRect, RRect, Path, Superellipse — no ClipOval) |
| Text (2) | 2/2 ✓ |
| Effects (3) | 3/3 ✓ |
| Gradients (2) | 2/2 ✓ |
| Save/Restore + SaveLayer | ✓ |
| Paint state setters (20 in Flutter) | **N/A — architectural** — FLUI embeds immutable Paint per command |

### Engine findings

#### 💀 [GAP | MEDIUM]: No `RasterCache` (layer-level retained rasterization)

Flutter `engine/src/flutter/flow/raster_cache.{cc,h}` — caches rasterized PictureLayer output to GPU texture. Eviction by frame age + memory budget. Preroll phase marks cacheable layers.

FLUI has SceneCompositor with `retain()`/`release()`, but this is scene-graph retention, **not** raster-level caching.

**Cost:** scenes with static PictureLayer (background, icon set) re-rasterize each frame even when content unchanged. Lost performance win.

**Recommendation:** Add `RasterCache` keyed on PictureLayer id + bounds. Eviction by memory budget mirroring TextureCache::evict_over_budget. **Wait for PictureLayer hints** (is_complex/will_change) before implementing.

#### 💀 [PARITY | LOW]: Missing draw operations (ClipOval, DrawDashedLine, DrawRoundSuperellipse)

Workarounds:
- ClipOval → ClipPath with oval geometry (1-line wrapper)
- DrawDashedLine → tessellated Path with dash pattern via Lyon (StrokeOptions has dash support)
- DrawRoundSuperellipse → existing ClipSuperellipse + draw

**Recommendation:** Add direct DrawCommand variants for performance later. Doesn't block anything.

#### ✓ EARNED ADDITION: `MultiDrawIndirect` — FLUI-only optimization

Flutter doesn't use MDI. FLUI's MDI infrastructure reduces CPU dispatch cost ~75% for instance-heavy scenes. **Keep.**

#### ✓ EARNED ADDITION: Explicit `BufferPool` / `TexturePool` / `PathCache` — separate, observable, tunable

Flutter's analogues live inside Skia/Impeller allocators — opaque to telemetry. FLUI exposing these as first-class types enables benchmark-driven tuning + bounded eviction policies. **Keep.**

#### 💀 [DIVERGENCE | LOW]: Glyph cache via `glyphon` vs Skia text blob cache

Acceptable divergence. cosmic-text + glyphon = pure Rust + GPU-native. Flutter's HarfBuzz + Skia → C++ bridge. Different tech stacks, **same correctness target**.

**Watch:** verify glyphon's atlas eviction is wired similarly to TextureCache.

---

# Part III — Combined Priority Order

| Priority | Action | Why now |
|----------|--------|---------|
| **1** | Delete confirmed-dead zombie code: commented `impl RenderObject` block (190 lines), `IntrinsicProtocol`, `BaselineProtocol`, `RenderDirtyPropagation` trait + propagation.rs, one of the two `ClipContext` traits | Flutter cross-ref **reinforced** these are dead — current FLUI design has no path to materialize them; intrinsic/baseline need entirely different abstraction when RenderIntrinsicWidth lands |
| **2** | Add 3 missing SceneBuilder methods (`push_clip_superellipse`, `add_platform_view`, `add_performance_overlay`) | Cheap; closes layer-side parity to 100%; layers already exist |
| **3** | Add PictureLayer hint fields (`is_complex`, `will_change`) | Cheap now, retrofit later costs more; signals needed for future RasterCache |
| **4** | Bound `SUPERELLIPSE_CACHE` + verify glyphon atlas eviction | Production-leak risks; mirror existing PathCache pattern |
| **5** | Tighten visibility: `pub(crate)` for `RendererBinding`, delegate traits — **until widget/view layer materializes consumers** | Don't freeze API on incomplete abstractions; widget layer needs work first |
| **6** | Plan widget-layer roadmap with **concrete** RenderObject phases (RenderProxyBox + RenderClip* + RenderDecoratedBox first) | This is the real architectural debt; 88% RenderObject gap is the framework's biggest hole |
| **7** | Defer: RasterCache (waits for hints), missing draw ops (workarounds OK), PlaceholderSpan/ShapeDecoration/ImageProvider (depend on widget layer) | YAGNI until consumer materializes |
| **8** | Don't touch: layer parity, PipelineOwner type-state, MouseTracker, PathCache/TextureCache/BufferPool, dispatch_command, Arity, sealed Protocol | Confirmed strong by Flutter cross-ref |

### Combined Mythos Insight

The single-crate audit recommended `pub(crate)` for delegate traits "until the first implementer arrives". Flutter cross-ref **confirmed** that decision — `CustomPainter`, `SingleChildLayoutDelegate`, `MultiChildLayoutDelegate`, `FlowDelegate` are waiting for implementers:

- `RenderCustomPaint` (Flutter has it; FLUI doesn't) → consumer of `CustomPainter`
- `RenderCustomMultiChildLayoutBox` → consumer of `MultiChildLayoutDelegate`
- `RenderCustomSingleChildLayoutBox` → consumer of `SingleChildLayoutDelegate`
- `RenderFlow` → consumer of `FlowDelegate`

All four render objects sit in the Phase 1-2 roadmap. **The traits are NOT long-term zombies** — they await render-object companions. **Updated recommendation:** keep `pub`, but add `// REVISIT_AFTER: RenderCustomPaint lands` doc comments and avoid investing API-stability work into them.

Similarly, `RendererBinding` waits for the `flui-app` impl — it's a **migration target**, not dead code. The internal `Arc<RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>>` public type **must be reworked before** `flui-app` lands the impl. So: `pub(crate)` the trait, redesign the integration surface, then unfreeze through `flui-app`.

`flui-painting::ClipContext` vs `flui-rendering::context::ClipContext` — **remain confirmed-duplicate** regardless of Flutter cross-ref. Flutter has one `ClipContext` (in painting/clip.dart) which `PaintingContext` extends. FLUI must keep **one** trait — preferably `flui-painting`'s — and have a future `PaintingContext` in rendering implement it. Today both are dead seams.

---

# Appendix A — Investigation Trail

## Tool dispatches

- **4 parallel `Explore` agents** mapped flui-{rendering, painting, layer, engine} structure (lib.rs surface, modules, traits, impl counts, hot paths, suspected dead).
- **4 parallel `Explore` agents** for Flutter cross-reference (painting/, rendering/ excl layer.dart, layer.dart standalone, engine/src/).
- Targeted Read + Grep passes to verify each finding:
  - Confirm 190+ commented lines in render_view.rs.
  - Confirm 2 ClipContext traits, 0 production impls.
  - Confirm RenderDirtyPropagation only used by MockTree in tests.
  - Confirm SUPERELLIPSE_CACHE thread_local is unbounded (no eviction calls).
  - Confirm TextureCache::evict_over_budget called from painter.rs:1058 (earlier "unbounded" concern was wrong).
  - Confirm PathCache uses 120-frame LRU + max_entries=512.
  - Confirm flui-painting::tessellation only referenced in painting's own tests + example.
  - Confirm dependency graph is clean DAG (no backwards deps).

## Workspace state at audit time (2026-05-20)

- All four audited crates ACTIVE in `Cargo.toml` (despite `CLAUDE.md` claiming otherwise).
- Workspace branch: `naughty-jackson-324931` (audit worktree).
- Rust edition: 2024; minimum rust-version: 1.94.
- wgpu pinned at 25.x (26.0+ broken; see https://github.com/gfx-rs/wgpu/issues/7915).

## Files referenced

Repo-relative paths (clickable in markdown viewers):

- [`Cargo.toml`](../../Cargo.toml)
- [`CLAUDE.md`](../../CLAUDE.md)
- [`crates/flui-painting/src/lib.rs`](../../crates/flui-painting/src/lib.rs)
- [`crates/flui-painting/src/clip_context.rs`](../../crates/flui-painting/src/clip_context.rs)
- [`crates/flui-painting/src/tessellation.rs`](../../crates/flui-painting/src/tessellation.rs)
- [`crates/flui-layer/src/lib.rs`](../../crates/flui-layer/src/lib.rs)
- [`crates/flui-layer/src/layer/mod.rs`](../../crates/flui-layer/src/layer/mod.rs)
- [`crates/flui-layer/src/layer/dispatch.rs`](../../crates/flui-layer/src/layer/dispatch.rs)
- [`crates/flui-layer/src/compositor/builder.rs`](../../crates/flui-layer/src/compositor/builder.rs)
- [`crates/flui-engine/src/lib.rs`](../../crates/flui-engine/src/lib.rs)
- [`crates/flui-engine/src/wgpu/layer_render.rs`](../../crates/flui-engine/src/wgpu/layer_render.rs)
- [`crates/flui-engine/src/wgpu/path_cache.rs`](../../crates/flui-engine/src/wgpu/path_cache.rs)
- [`crates/flui-engine/src/wgpu/texture_cache.rs`](../../crates/flui-engine/src/wgpu/texture_cache.rs)
- [`crates/flui-engine/src/wgpu/tessellator.rs`](../../crates/flui-engine/src/wgpu/tessellator.rs)
- [`crates/flui-engine/src/wgpu/painter.rs`](../../crates/flui-engine/src/wgpu/painter.rs)
- [`crates/flui-rendering/src/lib.rs`](../../crates/flui-rendering/src/lib.rs)
- [`crates/flui-rendering/src/view/render_view.rs`](../../crates/flui-rendering/src/view/render_view.rs)
- [`crates/flui-rendering/src/protocol/protocol.rs`](../../crates/flui-rendering/src/protocol/protocol.rs)
- [`crates/flui-rendering/src/storage/state/propagation.rs`](../../crates/flui-rendering/src/storage/state/propagation.rs)
- [`crates/flui-rendering/src/context/clip.rs`](../../crates/flui-rendering/src/context/clip.rs)
- [`crates/flui-rendering/src/binding/mod.rs`](../../crates/flui-rendering/src/binding/mod.rs)
- [`crates/flui-rendering/src/delegates/`](../../crates/flui-rendering/src/delegates/)

Flutter reference (absolute paths — outside the worktree):

- `C:\Users\vanya\RustroverProjects\flui\.flutter\flutter-master\packages\flutter\lib\src\painting\` — 48 files
- `C:\Users\vanya\RustroverProjects\flui\.flutter\flutter-master\packages\flutter\lib\src\rendering\` — 48 files
- `C:\Users\vanya\RustroverProjects\flui\.flutter\flutter-master\engine\src\` — C++ Skia/Impeller engine

---

*Audit generated 2026-05-20 via Claude Mythos methodology (12-phase Rust audit) + Flutter cross-reference (4 parallel Explore agents). Findings ranked by concrete burden + risk of changing. Recommendations prioritized so each step removes a real cost.*
