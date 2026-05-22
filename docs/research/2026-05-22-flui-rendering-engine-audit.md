---
title: "Mythos Audit — flui-rendering × flui-engine"
date: 2026-05-22
status: audit-complete
audit_methodology: claude-mythos (12-phase rust audit, render + GPU backend pass)
crates_audited:
  - flui-rendering
  - flui-engine
reference_sources:
  - flutter/packages/flutter/lib/src/rendering/object.dart  (RenderObject, PipelineOwner)
  - flutter/packages/flutter/lib/src/rendering/binding.dart  (RendererBinding)
  - flutter/packages/flutter/lib/src/rendering/view.dart  (RenderView)
  - flutter/packages/flutter/lib/src/rendering/mouse_tracker.dart
  - flutter/packages/flutter/lib/src/gestures/hit_test.dart  (HitTestResult, HitTestEntry)
  - flutter/engine/src/flow/  (compositor pattern)
predecessor_cycles:
  - docs/research/2026-05-21-flui-interaction-scheduler-audit.md  (Cycle 1, closed in PRs #85-#98)
  - docs/research/2026-05-22-flui-layer-semantics-audit.md  (Cycle 2, closed in PR #100/#101)
  - docs/research/2026-05-22-flui-foundation-tree-audit.md  (Cycle 3, closed in PRs #102-#106)
predecessor_partial:
  - docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md  (closed PR #81, #82, #83 — remaining findings re-baselined below)
authors:
  - Mythos (via Claude Opus 4.7)
---

# Mythos Audit: `flui-rendering` × `flui-engine`

> Deep audit across FLUI's **render-object + GPU-backend layer** — 79 source files (~25.2K LOC) in flui-rendering, 33 source files (~20.2K LOC) in flui-engine, **~45.4K LOC total**.
>
> Goal: identify zombie abstractions in the render-object spine, half-implemented pipeline phases (`unimplemented!()` in production paths), parallel type stacks shared with sibling crates (HitTestResult ×2, MouseTrackerAnnotation ×2, RenderError ×2, ParentData ×2), nested Arc-RwLock smell in the public surface, hot-path opportunities in painter/offscreen batching, and Flutter-drift in lifecycle / repaint-boundary / mouse-tracking — without breaking active integration with `flui-app`, `flui-foundation`, `flui-tree`, `flui-layer`, `flui-painting`, `flui-semantics`, `flui-view`, `flui-interaction`.
>
> **Cycle**: this audit continues the audit-execute series that produced PRs #81 (rendering Phase 1 zombie cleanup) / #82 (ClipContext consolidation) / #83 (SUPERELLIPSE_CACHE bounding + RSuperellipse parity) / #84 (framework spine repair) / #85-#98 (interaction × scheduler) / #100/#101 (layer × semantics) / #102-#106 (foundation × tree) against `vanyastaff/flui`. The previous cycle audited the foundation primitives + tree-abstraction surface (`flui-foundation` × `flui-tree`); see [`2026-05-22-flui-foundation-tree-audit.md`](2026-05-22-flui-foundation-tree-audit.md) → closed as PR #102 (47 units across 5 waves, ~−9.5K LOC). This cycle covers the render + GPU pair that sits one layer above tree and one below app.

---

## Table of Contents

- [Mythos Improvement Verdict](#mythos-improvement-verdict)
- [Part I — Architecture review](#part-i--architecture-review)
- [Part II — Findings](#part-ii--findings)
  - [flui-rendering findings (R-1 .. R-26)](#flui-rendering-findings-r-1--r-26)
  - [flui-engine findings (E-1 .. E-19)](#flui-engine-findings-e-1--e-19)
- [Part III — Flutter drift catalog](#part-iii--flutter-drift-catalog)
- [Part IV — Final combined priority order](#part-iv--final-combined-priority-order)
- [Appendix A — Investigation receipts](#appendix-a--investigation-receipts)
- [Re-baseline against 2026-05-20 audit](#re-baseline-against-2026-05-20-audit)

---

## Mythos Improvement Verdict

The pair **`flui-rendering` (25.2K LOC, 79 files) × `flui-engine` (20.2K LOC, 33 files)** is the **largest scope of the four cycles** (vs cycle 3's 23.4K, cycle 2's 15.6K, cycle 1's 12.4K) and carries **three load-bearing rot zones** that the prior `2026-05-20-mythos-audit-render-paint-layer-engine.md` only partially called out:

(a) **Three `unimplemented!()` macros sit in production paths.** [`PipelineOwner::<Semantics>::run_semantics`](../../crates/flui-rendering/src/pipeline/owner.rs) at line 1276 panics whenever the semantics dirty list is non-empty AND semantics_enabled is true. [`RendererBinding::perform_semantics_action`](../../crates/flui-rendering/src/binding/mod.rs) at line 325 panics whenever the platform a11y layer fires an action. [`SemanticsBuilder::new`](../../crates/flui-rendering/src/delegates/custom_painter.rs) at line 29 panics on construction. **Constitution Principle 6 violation explicit** ("No `unwrap()`/`println!`/`dbg!`/`unimplemented!`/`todo!` in production paths"). Three separate violations in the same crate, all in the `pub fn` API surface. The path through `run_frame()` → `into_semantics()` → `run_semantics()` will panic the moment a downstream consumer enables semantics with a semantically-dirty tree, dropping the entire owner. None of this is gated on a feature flag.

(b) **Two parallel `HitTestResult` types + two parallel `MouseTrackerAnnotation` types + two parallel `RenderError` types + two parallel `ParentData` types.** Each is consumed at different points in the call graph and shifts a TODO comment to the consumer for conversion. flui-rendering's `HitTestResult` ([`hit_testing/result.rs:25`](../../crates/flui-rendering/src/hit_testing/result.rs)) carries a `Vec<HitTestEntry>` + transform stack; flui-interaction's `routing::HitTestResult` ([`crates/flui-interaction/src/routing/hit_test.rs`](../../crates/flui-interaction/src/routing/hit_test.rs)) carries a different shape. flui-app at [`app/binding.rs:508`](../../crates/flui-app/src/app/binding.rs) has a literal `// TODO: Convert rendering HitTestEntry targets to interaction targets` between them. Similar story for MouseTrackerAnnotation — flui-rendering at [`input/mouse_tracker.rs:76`](../../crates/flui-rendering/src/input/mouse_tracker.rs) is a `trait MouseTrackerAnnotation: Debug + Send + Sync` with `on_enter`/`on_hover`/`on_exit`/`cursor` default-noop methods; flui-interaction at [`mouse_tracker.rs:77`](../../crates/flui-interaction/src/mouse_tracker.rs) is a `struct MouseTrackerAnnotation { region_id, on_enter, on_exit, on_hover: Option<Arc<...>> }`. Both are exported, neither bridges to the other. The duplication mirrors cycle 2's `flui_types::Alignment` newtype (resolved in PR #100/U21) at a far larger blast radius. `RenderError` ×2: [`flui-rendering/src/error.rs`](../../crates/flui-rendering/src/error.rs) holds pipeline failures (`NotAttached`, `LayoutDepthExceeded`, `Poisoned`, …); [`flui-engine/src/error.rs`](../../crates/flui-engine/src/error.rs) holds GPU failures (`SurfaceLost`, `OutOfMemory`, `ShaderError`, …). Two separate `pub enum RenderError`, both `#[non_exhaustive]` — different namespaces, same name, both consumed by flui-app. `ParentData` ×2: [`flui-rendering/src/parent_data/base.rs`](../../crates/flui-rendering/src/parent_data/base.rs) is the render-side base trait; [`flui-view/src/view/parent_data.rs:68`](../../crates/flui-view/src/view/parent_data.rs) defines its own `trait ParentData: Clone + Default + Send + Sync + 'static {}` for the view-side. The view-side never re-exports the render-side, and the two traits aren't related by sub/super-traiting.

(c) **`PipelineOwner::<Compositing>::run_compositing` is a `for node in needs_compositing { tracing::trace }; needs_compositing.clear()` stub.** [`crates/flui-rendering/src/pipeline/owner.rs:918-949`](../../crates/flui-rendering/src/pipeline/owner.rs): the function explicitly says *"Full compositing bits update is not yet implemented. This would require: 1. PipelineOwner to hold a reference to RenderTree 2. Look up each render object by ID 3. Call render_object.update_compositing_bits(). Currently we just clear the list — compositing works but may not be optimally batched."* The trace-and-clear shape is reachable from `run_frame()`; the unimplemented branch is silent (no `unimplemented!()` here, unlike `run_semantics`), so callers receive `Ok(())` while no actual compositing-bits update happens. This is the half-implementation that hides the worst.

Beyond these three, the audit catalogs **45 findings** across the pair — split between **flui-rendering (26 findings)** and **flui-engine (19 findings)** — plus a re-baseline of the 2026-05-20 prior audit at the bottom of this document.

**Three best things:**

1. **Type-state pipeline with sealed phase markers** ([`crates/flui-rendering/src/pipeline/phase.rs`](../../crates/flui-rendering/src/pipeline/phase.rs)). `PipelineOwner<Phase: PipelinePhase>` parameterized on five sealed zero-sized markers (`Idle`, `Layout`, `Compositing`, `PaintPhase`, `Semantics`); each phase's `run_*` method lives on its own impl block, transitions consume `self`. **Calling `run_paint()` on `PipelineOwner<Idle>` is a compile error**, not a runtime assert — Flutter's `_debugDoingThis*` field is hoisted to the type system. Four `compile_fail` doctests in `phase.rs:17-44` lock this in. The sealed module + 5-impl `Sealed` pattern is *Rust for Rustaceans* §3 ("Sealed traits") cleanly applied. **This is canonically the Rust port of Flutter's runtime phase assertion**. Don't touch.

2. **`AtomicRenderFlags` lock-free dirty bitset over `AtomicU32`** ([`crates/flui-rendering/src/storage/flags.rs:93+`](../../crates/flui-rendering/src/storage/flags.rs)). Single `AtomicU32` holds 11 flags (4 dirty + 2 boundary + 2 state + 2 propagation + 1 was-repaint-boundary). Reads use `Acquire`, writes use `Release`, mutations use `AcqRel`. `fetch_or` / `fetch_and` / `fetch_xor` for set/clear/toggle — all O(1) single-atomic operations. Static memory-footprint assert in `dirty.rs:113-117` (`DirtyNode <= 16 bytes`, `DirtySets <= 96 bytes`). Mirrors *Rust Atomics and Locks* Ch.3 ordering discipline exactly. **Don't touch.**

3. **`PipelineOwnerHandle` bounded crossbeam channel for cross-thread mark-dirty** ([`crates/flui-rendering/src/pipeline/handle.rs`](../../crates/flui-rendering/src/pipeline/handle.rs)). Default capacity 256, `try_send` returns `SendError::ChannelFull{capacity}` when at capacity (producer back-pressured), or `SendError::OwnerGone` when the receiver drops. **Refuses unbounded channels by design** ("Unbounded channels would hide this in heap growth; we refuse them" — line 22). The clone-tx / drop-rx-detection pattern is *Programming Rust* 2nd ed §19 ("Channels in mpsc / crossbeam") canonical. **Don't touch.**

**Worst complexity tax:**

1. **`run_compositing` half-implementation + `run_semantics` panic + `RendererBinding::perform_semantics_action` panic + `SemanticsBuilder::new` panic** — four production paths that either silently no-op or hard-panic. The compositing-bits update is the most invisible: a silent gap. The three `unimplemented!()` are loud but in different parts of the public surface (the binding, the owner, the delegate). All four need a coherent answer this cycle: either real impl, or `#[cfg(feature = "semantics")]` gating + clean `RenderError::SemanticsNotEnabled` returns instead of panics.

2. **Four parallel public types shared with sibling crates without bridging.** `HitTestResult` (flui-rendering ↔ flui-interaction), `MouseTrackerAnnotation` (flui-rendering ↔ flui-interaction), `RenderError` (flui-rendering ↔ flui-engine), `ParentData` (flui-rendering ↔ flui-view). Each is a 100+ LOC public surface; each consumer crate carries a TODO or a downcast/copy step. **The cycle 2 pattern (PR #100/U21: `flui_types::Alignment` newtype as the canonical home for what was a `flui-types` enum + `flui-layer` field tuple)** must propagate here. Pick one canonical home per type, demote the duplicate to a re-export or thin newtype.

3. **`RendererBinding::render_views() -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>`** ([`crates/flui-rendering/src/binding/mod.rs:145`](../../crates/flui-rendering/src/binding/mod.rs)). Deep-nested `Arc<RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>>` is **public** trait return surface. Every consumer must `.read()` the outer lock, lookup, then `.read()` the inner lock — interior mutability leaked through the trait. The previous audit flagged it at LOW severity ("redesign before first impl"); flui-app's `RenderingFlutterBinding` impl has now landed at [`crates/flui-app/src/bindings/renderer_binding.rs:145+`](../../crates/flui-app/src/bindings/renderer_binding.rs) — the nested-lock shape is **production**. Cycle 2 PR #100/U22 had to fix exactly this shape in `SemanticsBinding`; the same fix needs to apply here.

4. **Five `unimplemented!`-shaped half-paths inside engine** beyond the `unimplemented!()` macros: `WgpuPainter::clip_path` is a silent no-op tracing-warn ([`painter.rs:3592-3612`](../../crates/flui-engine/src/wgpu/painter.rs)); `Backend::render_backdrop_filter` is a fallback to child-without-filter rendering ([`backend.rs:805-834`](../../crates/flui-engine/src/wgpu/backend.rs)); `offscreen.rs::PipelineManager` is a 50-LOC zombie wrapper around `ShaderCache` with body-less `get_or_create_pipeline()` ([`offscreen.rs:1352-1402`](../../crates/flui-engine/src/wgpu/offscreen.rs)); `offscreen.rs::PipelineHandle` carries only a `shader_type` and zero behavior; `offscreen.rs::FullscreenVertex::buffer_layout` is a doc-only stub. **Engine has `unimplemented!()`-equivalents disguised as fallbacks**. These need the same Constitution Principle 6 treatment.

**Where dead code hides** (the verified-zero-external-consumer + verified-unreachable list):

| Module / Symbol | LOC | External consumers | Verdict |
|---|---|---|---|
| `flui-rendering/src/delegates/{custom_painter, flow_delegate, multi_child_layout_delegate, single_child_layout_delegate, sliver_grid_delegate}.rs` | ~1,800 | 0 production impls; only test mocks. The 2026-05-20 audit flagged at MEDIUM; **status unchanged 18 months later** | Feature-gate `experimental-delegates` |
| `flui-rendering/src/delegates/custom_painter.rs::SemanticsBuilder` | 30 | 0 — only `unimplemented!()` ctor | Delete or feature-gate behind `semantics` |
| `flui-rendering/src/constraints/scroll_metrics.rs` (`ScrollMetrics` trait + `FixedScrollMetrics` + `FixedExtentMetrics`) | 452 | 0 external; re-exported in prelude | Feature-gate or `pub(crate)` |
| `flui-rendering/src/storage/state/propagation.rs` (`RenderDirtyPropagation` trait body) | 73 | 0 in-crate (the `pub(crate) use` line at `state/mod.rs:100` re-exports but no one consumes it) | Delete or finish the viewport-invalidation hook the `PRESERVED_FOR` comment promises |
| `flui-rendering/src/view/render_view.rs::RenderView::{depth, needs_layout, needs_paint, needs_compositing_bits_update, needs_semantics_update, is_repaint_boundary, needs_compositing, cached_constraints, parent_data}` 9 `#[allow(dead_code)]` fields | ~30 | 0 | Either wire them in or delete |
| `flui-rendering/src/view/viewport_offset.rs::ScrollableViewportOffset` listener API | ~50 | 0 (`#[allow(dead_code)]` for future listener API) | Delete or feature-gate |
| `flui-engine/src/wgpu/offscreen.rs::{PipelineManager, PipelineHandle}` | ~60 | 0 (re-exported but `get_or_create_pipeline` is body-less) | Delete |
| `flui-engine/src/wgpu/effects.rs::{BlurParams, BlurIntensity, LinearGradientBuilder, ShadowParams::elevation_*}` | ~150 | 0 production (only `effects.rs` unit tests + 1 docstring reference) | Delete or feature-gate `forward-looking-effects` |
| `flui-engine/src/wgpu/instancing.rs::{RectInstance::rounded_rect, RectInstance::with_transform, RectInstance::with_clip_rsuperellipse, CircleInstance::ellipse, ArcInstance::ellipse, TextureInstance::with_rotation, TextureInstance::with_uv}` | ~80 | 0 production (verified by grep; `instancing.rs` body has them, but no `painter.rs` callsite hits them) | Delete or `// REMOVE_BY: 2026-08-22` |
| `flui-engine/src/wgpu/pipeline.rs::PipelineKey` ALL constructors except 1 | ~200 | 0 (the module is `#[allow(dead_code)]` at mod.rs:85; `pipelines.rs::PipelineCache` is what painter actually uses) | Delete `pipeline.rs` OR migrate painter to `PipelineKey` bitflags |
| `flui-engine/src/wgpu/shader_compiler.rs::{ShaderCache::cached_count, clear}` | ~30 | 0 (devtools-only; module is `#[allow(dead_code)]` at mod.rs:93) | Move to `#[cfg(feature = "devtools")]` |
| `flui-engine/src/wgpu/multi_draw.rs::DrawIndexedIndirectArgs::quad_instances` + `DrawCommand` wrap helpers + `MultiDrawStats` | ~40 | 0 (`MultiDrawBatcher` is used internally; stats / aliases are not) | `pub(crate)` |
| `flui-engine/src/wgpu/{AtlasEntry, AtlasRect, ExternalTextureEntry, ExternalTextureRegistry}` reexports | re-export-only | 0 (internal use only inside flui-engine) | `pub(crate)` |
| **Subtotal — verified zero-external-consumer LOC** | **~2,800** | **0** | |

(Methodology: `rg "<symbol>"` workspace-wide excluding the defining crate; details in Appendix A.2.)

**Half-implemented hot paths** (beyond the three `unimplemented!()` macros and `run_compositing`):
- `Backend::render_backdrop_filter` is a fallback to dispatching the child's display list with no filter applied ([`backend.rs:813-834`](../../crates/flui-engine/src/wgpu/backend.rs)). The `tracing::warn!("BackdropFilter rendering not yet fully implemented")` runs on every backdrop filter. Architectural limitation: WgpuRenderer wraps WgpuPainter which doesn't own the OffscreenRenderer; a real impl needs the cross-component wiring.
- `WgpuPainter::clip_path` is a silent no-op ([`painter.rs:3592-3612`](../../crates/flui-engine/src/wgpu/painter.rs)). The comment lists the implementation requirements ("stencil buffer configuration, tessellate path and render to stencil buffer, …") but produces a `tracing::trace!` and returns. **`Layer::ClipPath` ([`flui-layer/src/layer/mod.rs`](../../crates/flui-layer/src/layer/mod.rs)) renders fine through the layer-tree route via `renderer.push_clip_path(path, behavior)` → `layer_render.rs::ClipPathLayer::render`** — but the painter-direct `clip_path` is a no-op. Two routes, one works, one silently fails.
- `RenderTree::set_owner` has the same shape ([`storage/tree.rs:114-119`](../../crates/flui-rendering/src/storage/tree.rs)): "Currently this only stores the owner reference. Full attach/detach lifecycle would require…". 30-LOC TODO that comments out the actual lifecycle work.
- `propagate_constraints_to_child` and `sync_child_size_to_parent` in `pipeline/owner.rs:885-892` are both empty-body methods called from `layout_node_with_children`. Their stubs are reached every frame — the layout phase's constraint propagation and child-size sync are NOT happening.
- `paint_node_recursive` doesn't sort the dirty_paint list by depth ([`pipeline/owner.rs:985-987`](../../crates/flui-rendering/src/pipeline/owner.rs)): *"Note: We don't need to sort for now since we paint from root"*. Flutter sorts `flushPaint` deep-first because repaint-boundary-isolated subtrees can paint out of root order. The current shape paints only via the recursion through `root_id` — every node-needing-paint that ISN'T reachable from root_id during the descent is silently dropped (the `clear_needs_paint` loop at line 990-994 clears the flag without painting).

**Biggest optimization opportunity** — **collapse the four parallel public types + close the four production-path `unimplemented!`-or-silent-stub holes + delete the ~2,800 LOC of verified-dead engine surface**. Estimated impact: ~+8% binary-size delta from the engine zombie deletion, ~+5x compile-time reduction in flui-rendering's prelude consumers, full panic-free semantics path, full compositing-bits update path. The structural plan is laid out in Part IV.

**Don't touch**:

- `PipelineOwner<Phase>` typestate + sealed phase markers + `compile_fail` doctests (`pipeline/{owner,phase}.rs`) — gold-standard Flutter-port.
- `AtomicRenderFlags` + `RenderFlags` bitfield (`storage/flags.rs`) — lock-free + correct memory ordering + static-size asserts.
- `PipelineOwnerHandle` + `DirtyRequest` + bounded crossbeam channel (`pipeline/handle.rs`) — backpressure-by-construction.
- `VisualUpdateNotifier` 3-callback consolidation (`pipeline/notifier.rs`) — Mythos Step 6 cycle-1 pattern faithfully applied here.
- `DirtySets` co-located 4-Vec consolidation + sort discipline (`pipeline/dirty.rs`).
- `RenderTree::get_two_mut` + `get_parent_and_children_mut` parent-child re-entrant mutable access (`storage/tree.rs:202-289`) — *Rust for Rustaceans* §6 "Index-based access" idiomatically applied with documented `SAFETY` block. The `unsafe` block has the disjoint-index proof; the assert / equality check before is the load-bearing safety witness.
- `SuperellipsePathCache` bounded LRU + `WgpuPainter::superellipse_cache` ownership pattern (`wgpu/{superellipse_cache,painter}.rs`) — closes the cycle-2-flagged unbounded thread_local. PR #83 work.
- `WgpuPainter::path_cache` 120-frame LRU + max_entries=512 default (`wgpu/path_cache.rs`).
- `TextureCache::evict_over_budget()` size-budget eviction wired in `painter.rs:1058`.
- `Renderer::adapter` / `Renderer::instance` keep-alive `#[allow(dead_code)]` with documented reason (`wgpu/renderer.rs:135-140`) — correct discipline, follows cycle 1's "every `#[allow(dead_code)]` needs a documented `// REMOVE_BY:` or `// USED_BY:`" pattern.

---

## Part I — Architecture review

### Where these crates sit in the workspace DAG

```
flui-rendering ──► flui-foundation (RenderId, Diagnosticable, BindingBase, debug_assert_*, NonZeroUsize)
              ──► flui-tree         (TreeRead/TreeNav/TreeWrite<RenderId>, arity markers, Slab/IndexedSlot)
              ──► flui-types        (Offset, Rect, Size, Matrix4, Pixels, geometry, painting, styling)
              ──► flui-painting     (Canvas, DisplayList, Paint, PaintStyle, ClipContext, Picture)
              ──► flui-interaction  (HitTestBehavior, MouseTrackerAnnotation, CursorIcon, PointerEvent ← drift point)
              ──► flui-layer        (Layer, LayerTree, TransformLayer)
              ──► flui-semantics    (SemanticsAction, SemanticsConfiguration, SemanticsTree, etc — re-exported as `semantics`)

flui-engine ──► flui-types     (Offset, Rect, Color, Matrix4, etc)
            ──► flui-painting  (Paint, DisplayList, DrawCommand)
            ──► flui-foundation (no, actually — engine doesn't depend on foundation; verified via Cargo.toml)
            ──► flui-layer     (Scene, Layer, LayerTree, SceneBuilder, …)
            + (no dep on flui-rendering — they are siblings)

flui-app ──► flui-rendering ──► …
         ──► flui-engine ──► …
```

**Cross-crate notes**:
- flui-engine has NO dependency on flui-foundation. RenderError diverges: engine's is independent.
- flui-rendering re-exports `flui_semantics as semantics` (line 66 of `lib.rs`) and `flui_layer::*` under a `pub mod layer` (line 73-75). These are convenience re-exports; the crates are real dependencies.
- flui-engine and flui-rendering are **siblings** in the DAG. They do NOT communicate directly. Their seam is the `Scene` produced by flui-rendering's paint phase and consumed by `flui-engine::wgpu::Renderer::render_scene`. Architecturally clean.

**Public surface used externally** (verified at HEAD `aea56399`, the cycle-4 opener):

| Symbol | Producer crate | Consumer crates |
|---|---|---|
| `RenderTree`, `RenderNode`, `RenderEntry<P>` | flui-rendering | flui-app, flui-view (via pipeline owner) |
| `PipelineOwner<Phase>`, `PipelineOwnerHandle`, `DirtyKind`, `DirtyRequest` | flui-rendering | flui-app, flui-view, flui-hot-reload |
| `RenderObject<P>`, `RenderBox`, `RenderSliver`, capability traits | flui-rendering | flui-app, custom render-objects (none yet outside `objects/`) |
| `BoxProtocol`, `SliverProtocol`, `Protocol` + capability traits | flui-rendering | flui-app, flui-view (for protocol-bridging at view-side) |
| `BoxConstraints`, `Constraints`, `SliverConstraints`, `SliverGeometry` | flui-rendering | flui-app, flui-view |
| `CanvasContext`, `BoxLayoutContext`, `BoxPaintContext`, etc | flui-rendering | flui-app, flui-view |
| `HitTestResult`, `HitTestEntry`, `BoxHitTestResult`, `SliverHitTestResult`, `HitTestTarget` | flui-rendering | flui-app, flui-interaction (via wrap/conversion) |
| `RendererBinding` trait + `debug_dump_*` | flui-rendering | flui-app (RenderingFlutterBinding impl in flui-app) |
| `RenderView`, `RenderViewAdapter`, `ViewConfiguration`, `CompositeResult` | flui-rendering | flui-app, flui-view |
| `MouseTracker`, `MouseTrackerAnnotation`, `MouseTrackerHitTest` | flui-rendering | flui-app (NOTE: parallel `MouseTracker` exists in flui-interaction) |
| `RenderError` (rendering errors) | flui-rendering | flui-app |
| `ParentData` trait + concrete `BoxParentData`/`FlexParentData`/etc | flui-rendering | flui-view (also has its own `ParentData` trait — drift) |
| `Renderer` (cross-platform GPU renderer) | flui-engine | flui-app (only external consumer) |
| `RenderError` (engine errors) | flui-engine | flui-app |
| `Paint`, `Backend`, `LayerRender`, `WgpuPainter`, `FontLoader`, `DebugBackend` | flui-engine | flui-app (Renderer impl), tests |
| `Layer`, `LayerId`, `LayerTree` re-export | flui-engine | flui-app (passes through to flui-layer) |

**Verified-zero-external-consumer surfaces** (re-exported but no consumer):

| Symbol | Producer crate |
|---|---|
| `CustomPainter`, `FlowDelegate`, `MultiChildLayoutDelegate`, `SingleChildLayoutDelegate`, `AspectRatioDelegate`, `CenterLayoutDelegate`, `RectClipper`, `CustomClipper`, `SliverGridDelegate*` | flui-rendering (delegates) |
| `SemanticsBuilder` (panics on construction) | flui-rendering |
| `ScrollMetrics`, `FixedScrollMetrics`, `FixedExtentMetrics` | flui-rendering |
| `ScrollableViewportOffset` (listener API) | flui-rendering |
| `RenderDirtyPropagation` trait (pub(crate)) | flui-rendering |
| `PipelineManager`, `PipelineHandle` | flui-engine |
| `BlurParams`, `BlurIntensity`, `LinearGradientBuilder`, `ShadowParams::elevation_*` | flui-engine |
| `RectInstance::rounded_rect`, `RectInstance::with_transform`, `RectInstance::with_clip_rsuperellipse`, `CircleInstance::ellipse`, `ArcInstance::ellipse`, `TextureInstance::with_rotation`, `TextureInstance::with_uv` | flui-engine |
| `PipelineKey` constructors (engine `wgpu/pipeline.rs`) | flui-engine |
| `ShaderCache::cached_count`, `ShaderCache::clear` | flui-engine |
| `MultiDrawStats`, `DrawIndexedIndirectArgs::quad_instances` | flui-engine |
| `AtlasEntry`, `AtlasRect`, `ExternalTextureEntry`, `ExternalTextureRegistry` (re-exports) | flui-engine |

### Three-tree architecture and the render-tree's role

Per the constitutional five-tree architecture (View / Element / Render / Layer / Semantics), the render tree owns:

1. **Slab storage** keyed on `RenderId` (1-based NonZeroUsize, `+1` offset pattern).
2. **`TreeRead<RenderId> + TreeNav<RenderId> + TreeWrite<RenderId>` impls** at `storage/tree.rs:647-760`. Cycle 3 PR #103 hoisted cascade semantics into `TreeWrite::remove` (cascade by default); `TreeWrite::remove_shallow` is the opt-out. RenderTree's `remove_shallow` at line 685-704 is the non-cascade primitive; the trait default `remove` is the cascade-by-default path RenderTree inherits. **This is the cycle-3 fix landing correctly in cycle 4.** Don't touch.
3. **Protocol dispatch** via `RenderNode::{Box, Sliver}` enum + protocol-aware `RenderObject<P>` trait. The `RenderNode` enum at `storage/node.rs:37-44` is two-variant; the `RenderEntry<P>` at `storage/entry.rs` carries `Box<dyn RenderObject<P>>`. **No `RwLock` around the trait object** — that was U2's exemplar refactor (PR #81/U2) which removed the `RwLock<Box<dyn>>` and replaced with plain `Box<dyn>`; `RenderState<P>` covers all interior mutability via atomics + `OnceCell`. The render-tree is `Send + Sync` by auto-derive. Don't break.
4. **Lock-free dirty tracking** via `AtomicRenderFlags` (single `AtomicU32`, 11 flags) on each `RenderState<P>`. `set_was_repaint_boundary` (former trait method) was deleted in U2 and replaced with an atomic store on `WAS_REPAINT_BOUNDARY` at the state level — the paint phase flips the bit through `entry.state().set_was_repaint_boundary(true)` instead of `&mut` on the trait object.
5. **Pipeline orchestration** via typestate `PipelineOwner<Phase>` with phase-specific `run_*` methods. Mythos Step 7 (PR #81 era).

### The engine architecture

flui-engine wraps wgpu in three concentric layers:

```
Scene (flui-layer) ─▶ Renderer::render_scene (wgpu/renderer.rs)
                             │
                             ├── DamageTracker decides full-repaint vs skip
                             ├── OcclusionTracker culls fully-occluded layers
                             └── Backend::render (wgpu/backend.rs)
                                       │
                                       └── dispatch_command(s) (commands.rs)
                                                 │
                                                 └── LayerRender impls (wgpu/layer_render.rs)
                                                            │
                                                            └── CommandRenderer trait methods
                                                                     │
                                                                     └── WgpuPainter (wgpu/painter.rs, 3961 LOC)
                                                                              │
                                                                              ├── Tessellator (lyon)
                                                                              ├── TextRenderer (glyphon)
                                                                              ├── BufferPool / TexturePool / TextureAtlas
                                                                              ├── PathCache / SuperellipsePathCache / TextureCache
                                                                              ├── OffscreenRenderer (Arc<Mutex<>>, lives on Renderer not painter)
                                                                              └── Shader pipelines (pipelines.rs PipelineCache)
```

**Architectural smells**:
- `OffscreenRenderer` is owned by `Renderer` (`renderer.rs:147`: `offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>`), wrapped in `Arc<Mutex<>>`. `WgpuPainter` doesn't see it. Backdrop filter integration falls back to no-filter rendering because of this seam (the `// TODO` at `backend.rs:813-834`). The right shape is either `OffscreenRenderer` on `Painter` (matching `PathCache` ownership), or a `&mut OffscreenRenderer` parameter on the relevant `Backend` methods.
- `Backend` and `WgpuPainter` are sibling concrete types, both implementing different subsets of behavior. `Backend` wraps `WgpuPainter` via `with_transform` closures (`backend.rs:846`); painter's API doesn't take `Matrix4` directly. The two-layer pattern is fine but the boundary is fuzzy — `clip_path` lives only on `WgpuPainter` (silent no-op), `render_backdrop_filter` lives only on `Backend` (fallback).
- `CommandRenderer` trait has **50+ methods** — this is the dispatch surface between `dispatch_command` and the backend. Each `DrawCommand` variant has a corresponding `render_*` method. Single-impl in production (`Backend`). The 50-method trait is structurally a visitor pattern over the `DrawCommand` enum; cycle 1's pattern (collapse to closure-based `dispatch_*`?) doesn't obviously apply here because the methods are how backend swap-out would work (the doc-block in `traits.rs:30` literally says *"Multiple rendering backends without changing DisplayList"* — Skia/Vello/software being the candidates). **Keep the trait, but `pub(crate)` the methods that aren't part of the public swap-out story** (the `push_*`/`pop_*` clip-stack methods are framework-internal; the `render_*` shape-methods are the actual swap-out).

### Cross-cycle pattern continuity

Patterns from cycles 1-3 that should propagate into cycle 4:

| Pattern | Established by | Applies to this cycle |
|---|---|---|
| **PR #84 `ChangeNotifier::dispose` template** | flui-foundation | `PipelineOwner::dispose` (none today) for the BindingBase teardown story; `Renderer::dispose` (none today) for wgpu device teardown |
| **Cycle 1's `unimplemented!()` → `Err(...)` conversion** (PR #93) | flui-interaction | Three `unimplemented!()` macros in flui-rendering: `run_semantics`, `perform_semantics_action`, `SemanticsBuilder::new` |
| **Cycle 2's cascade-by-default `remove`** (PR #100 U12+U13) | flui-layer + flui-semantics | Cycle 3 PR #103 hoisted this to `TreeWrite::remove` — RenderTree adopts it cleanly. **Already inherited.** |
| **Cycle 2's `Alignment` newtype consolidation** (PR #100 U21) | flui-types ↔ flui-layer | Four parallel types here: `HitTestResult`, `MouseTrackerAnnotation`, `RenderError`, `ParentData`. Pick canonical home per type |
| **Cycle 2's nested-lock cleanup in SemanticsBinding** (PR #100 U22) | flui-semantics | `RendererBinding::render_views() -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>` is exactly the same shape; same fix shape (lookup-method returning `Option<Arc<RwLock<RenderView>>>` cloned, no nested-lock public expose) |
| **Cycle 3's Box<str> for error variants + SmallVec inline + dispose hooks** (PR #106 T-16+T-18+T-20) | flui-foundation + flui-tree | RenderError has 5 `String` fields (`InvalidConstraints.message`, `RelayoutBoundaryViolation.message`, `LayerError.message`, `CompositingError.message`, `SemanticsError.message`) — all candidates for `Box<str>` |
| **Cycle 1's PointerId widening** (PR #96 U9) | flui-interaction | Not applicable; render-side IDs are slab-internal |
| **Cycle 3's TreeWrite cascade contract + RenderTree adoption** (PR #103) | flui-tree | **Already in.** RenderTree's `remove`/`remove_shallow` shape is the post-cycle-3 contract |

### Re-baseline against the prior `2026-05-20-mythos-audit-render-paint-layer-engine.md`

The pre-existing audit catalogued 13 findings across the four render-stack crates. Cycle 4 status per finding (skipping the painting+layer findings outside this cycle's scope):

| Prior finding | Status @ HEAD `aea56399` | Notes |
|---|---|---|
| `flui_painting::ClipContext` + `flui_rendering::ClipContext` duplication | **CLOSED** (PR #82) | flui-rendering's was deleted; `CanvasContext` impls `flui_painting::ClipContext` at line 695 |
| Painting tessellation duplication | NOT in this cycle's scope (cycle audited painting separately) | |
| Commented `impl RenderObject for RenderView` 190 lines | **CLOSED** (PR #81 U1) | Deleted |
| `RenderDirtyPropagation` trait + `propagation.rs` | **PARTIAL** (PR #81 U3) | Trait body preserved at `pub(crate)` with `PRESERVED_FOR` marker. Still uses `ElementId` not `RenderId` — drift remains (R-5 below) |
| `IntrinsicProtocol` + `BaselineProtocol` sealed empty traits | **CLOSED** (PR #81 U2) | Deleted |
| `SUPERELLIPSE_CACHE` unbounded thread_local | **CLOSED** (PR #83) | Replaced with bounded `SuperellipsePathCache` owned by `WgpuPainter` |
| Multiple delegate traits 0 production impls (CustomPainter, FlowDelegate, etc) | **OPEN** | 4 delegate trait modules ~1,800 LOC, 0 production impls. The 2026-05-20 audit suggested "wait for companion render-object consumers"; cycle 4 finding **R-1** revises this verdict |
| Engine `#[allow(dead_code)]` forward-looking helpers (effects, instancing, pipeline, shader_compiler) | **OPEN** with documented cadence | The 2026-05-20 audit demanded a `REMOVE_BY:` discipline; engine adopted *module-level* `#[allow(dead_code)]` with prose justifications in `wgpu/mod.rs:55-94` but did NOT delete the items themselves. Cycle 4 findings **E-1 through E-5** treat each item-level zombie |
| Numerous `mod.rs` re-export-only files | OPEN (verdict: leave) | Confirmed |
| `RendererBinding` trait 9 methods, 0 workspace impls, nested `Arc<RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>>` public type | **PARTIAL** | flui-app's `RenderingFlutterBinding` impl now exists, so the trait HAS a workspace impl. But the nested-lock public type is unchanged and load-bearing. Cycle 4 finding **R-2** treats it |
| `CLAUDE.md` doc drift on disabled crates | **CLOSED** (PR #81 U4) | |
| `ScrollMetrics` trait + 2 impls "needs manual confirmation" | **OPEN** (R-19 confirms 0 consumers) | |

`unsafe` audit in flui-rendering: one `unsafe` block in `storage/tree.rs:225-229` (`get_two_mut`) and one in `storage/tree.rs:281-288` (`get_parent_and_children_mut`). Both have method-level `SAFETY:` documentation. Both witness disjoint slab indices and tie the lifetime to `&mut self`. **No `unsafe impl Send`/`unsafe impl Sync`** — the U2 refactor removed those. The two surviving `unsafe` blocks are correctly Constitution-Principle-3 scoped (rendering is core; allowed). Don't touch.

`unsafe` audit in flui-engine: wgpu surface creation via `Instance::create_surface_unsafe` at `wgpu/renderer.rs:200+`, with a method-level `SAFETY:` block. Other than that, no `unsafe` blocks — the rest of the engine routes through safe wgpu APIs. Constitution-Principle-3 clean.

---

## Part II — Findings

Findings are split between **flui-rendering (R-1 .. R-26)** and **flui-engine (E-1 .. E-19)**. Each follows the cycle-2/3 template: severity tag, evidence line refs, why-problem, Flutter ref (when applicable), proposed fix shape, blast radius.

### flui-rendering findings (R-1 .. R-26)

---

#### R-1 [P0 CONSTITUTION-VIOLATION | CRITICAL] `PipelineOwner::<Semantics>::run_semantics` uses `unimplemented!()` macro in production path

**Evidence:**
- `crates/flui-rendering/src/pipeline/owner.rs:1270-1281`:
  ```rust
  let nodes_to_process: Vec<DirtyNode> = self.dirty.needs_semantics.to_vec();
  self.dirty.needs_semantics.clear();

  // Semantics system is not yet implemented
  if !nodes_to_process.is_empty() {
      unimplemented!(
          "Semantics system not yet implemented - requires full semantics integration. \
           {} nodes need semantics updates",
          nodes_to_process.len()
      );
  }
  ```
- Reached from `PipelineOwner::<Idle>::run_frame()` at `owner.rs:295-296` via `into_semantics()` → `run_semantics()`. The path is unconditional once `semantics_enabled()` returns true and the semantics-dirty list is non-empty.
- `RendererBinding::draw_frame` at `binding/mod.rs:235-267` calls `run_frame()` on every frame. Once any consumer flips semantics_enabled (e.g. screen reader activates), every frame with semantics-dirty nodes panics.

**Why it's a problem:**
- **Constitution Principle 6 violation explicit**: "No `unwrap()`/`println!`/`dbg!`/`unimplemented!`/`todo!` in production paths".
- The panic propagates up through `run_frame()` (which catches `RenderError`s but not panics outside `catch_unwind`'s frame), through `draw_frame()`, through the engine's render loop. Process abort.
- The `Poisoned` error variant at `error.rs:174-180` exists precisely for this case — render-object panic during phase work — but `run_semantics` isn't wrapped in `catch_unwind` like `paint_node_recursive` is (compare `owner.rs:1099-1102`).

**Flutter reference:** `rendering/object.dart::PipelineOwner.flushSemantics` walks the semantics dirty list using `_nodesNeedingSemantics.toList()..sort(...)` and calls `object._updateSemantics()` on each. No panic; absent implementation = no-op. FLUI's panic posture is uniquely a violation of the principle.

**Fix shape (canonical):**
```rust
pub fn run_semantics(&mut self) -> crate::error::RenderResult<()> {
    if !self.semantics_enabled() {
        return Ok(());
    }
    tracing::debug!("run_semantics: {} nodes", self.dirty.needs_semantics.len());
    self.debug_doing_semantics = true;
    let nodes_to_process: Vec<DirtyNode> = std::mem::take(&mut self.dirty.needs_semantics);

    // Sort shallow-first matching Flutter's flushSemantics.
    let mut nodes_to_process = nodes_to_process;
    nodes_to_process.sort_unstable_by_key(|n| n.depth);

    for dirty_node in &nodes_to_process {
        if let Some(render_node) = self.render_tree.get(dirty_node.id) {
            // Trait-method call wrapped in catch_unwind mirroring paint_node_recursive.
            let debug_name = render_node.box_render_object().debug_name();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // SemanticsCapability supertrait is reachable through dyn — see traits/render_object.rs.
                // Stub semantics config build + register on SemanticsOwner here.
                // For now, mark the node clean and tracing-warn for unimpl details.
                tracing::warn!(?dirty_node.id, "run_semantics: full SemanticsOwner integration pending");
            }))
            .map_err(|_| crate::error::RenderError::poisoned(debug_name, "semantics"))?;
        }
    }

    self.debug_doing_semantics = false;
    Ok(())
}
```
The `tracing::warn!` is the bridge to "full impl awaited" — the path returns `Ok(())` without panicking, the framework no longer aborts. Real impl swaps the warn for the SemanticsOwner traversal.

**Blast radius:** owner.rs only. ~15 LOC change. No external consumer break — the API still returns `RenderResult<()>`, just with a different terminal behavior.

---

#### R-2 [P0 CONSTITUTION-VIOLATION | CRITICAL] `RendererBinding::perform_semantics_action` uses `unimplemented!()` macro on every dispatched action

**Evidence:**
- `crates/flui-rendering/src/binding/mod.rs:315-333`:
  ```rust
  fn perform_semantics_action(
      &self,
      view_id: u64,
      node_id: i32,
      action: flui_semantics::SemanticsAction,
      _args: Option<flui_semantics::ActionArgs>,
  ) {
      if let Some(view) = self.get_render_view(view_id) {
          let _view_guard = view.read();
          unimplemented!(
              "Semantics actions not yet implemented - requires SemanticsOwner integration. \
               Attempted action: {:?} on node {} in view {}",
              action, node_id, view_id
          );
      }
  }
  ```
- Called from the platform a11y layer whenever the OS dispatches an accessibility action (button-tap-via-VoiceOver, scroll-via-TalkBack, etc.) — the canonical entry point.
- The `if let Some(view) = self.get_render_view(view_id)` guard means the panic only fires when the view exists, which is the common case.

**Why it's a problem:** Same as R-1 — Constitution Principle 6 violation in a default-trait-method that any consumer of `RendererBinding` inherits.

**Flutter reference:** `rendering/binding.dart::RendererBinding.performSemanticsAction` dispatches to `SemanticsOwner.performAction(args.nodeId, args.action, args.args)`. Absent SemanticsOwner = no-op.

**Fix shape:**
```rust
fn perform_semantics_action(
    &self,
    view_id: u64,
    node_id: i32,
    action: flui_semantics::SemanticsAction,
    _args: Option<flui_semantics::ActionArgs>,
) {
    let Some(view) = self.get_render_view(view_id) else { return };
    let _view_guard = view.read();

    // TODO: wire up SemanticsOwner integration to dispatch action.
    // Until then, surface the action via tracing so platforms see the dispatch
    // path is reachable; no panic.
    tracing::warn!(
        ?action, %node_id, %view_id,
        "RendererBinding::perform_semantics_action: SemanticsOwner integration pending"
    );
}
```
This is a `pub trait` method with a default impl; downstream consumers (flui-app's `RenderingFlutterBinding`) inherit the default unless they override. So changing the default fixes all consumers.

**Blast radius:** binding/mod.rs only. ~8 LOC change. Same shape as R-1.

---

#### R-3 [P0 CONSTITUTION-VIOLATION | CRITICAL] `SemanticsBuilder::new()` panics with `unimplemented!()` on construction

**Evidence:**
- `crates/flui-rendering/src/delegates/custom_painter.rs:18-37`:
  ```rust
  pub struct SemanticsBuilder {
      _private: (),
  }

  impl SemanticsBuilder {
      pub fn new() -> Self {
          unimplemented!("SemanticsBuilder not yet implemented - semantics support incomplete");
      }
  }

  impl Default for SemanticsBuilder {
      fn default() -> Self {
          Self::new()
      }
  }
  ```
- `SemanticsBuilder` is exported from the `delegates` prelude at `lib.rs:122-126`.
- Doc-comment at line 14: "INCOMPLETE: This is a placeholder type. Semantics support is not yet implemented. Using this builder will panic until the semantics system is complete."

**Why it's a problem:**
- Constitution Principle 6. The `Default` impl makes the panic reachable via `SemanticsBuilder::default()` from anywhere.
- The `Default` impl is the kind of pattern that gets accidentally invoked by trait machinery (e.g. `#[derive(Default)]` on a struct that contains `SemanticsBuilder`).
- The struct is in the public prelude. Even consumers who never construct one directly might pull it into scope by accident.

**Fix shape:** Delete the type entirely until the semantics system materializes, OR feature-gate behind `#[cfg(feature = "semantics-incomplete")]`. The `_private: ()` field with the `pub fn new` panic is architecture theater for an unfinished surface. Removing it requires:
1. Delete `SemanticsBuilder` type + `Default` impl in `custom_painter.rs`.
2. Update `delegates/mod.rs` export to remove `SemanticsBuilder`.
3. Update prelude in `lib.rs:122-126` to remove `SemanticsBuilder`.
4. Audit callers of `SemanticsBuilder` workspace-wide: `rg "SemanticsBuilder" crates` returns 0 production callsites (delegate trait method `describe_semantics_for_builder` accepts `&mut SemanticsBuilder` in `custom_painter.rs:170` but it's never called).

**Blast radius:** custom_painter.rs + delegates/mod.rs + lib.rs prelude. ~30 LOC deletion. Public API breaking change but consumers don't exist.

---

#### R-4 [P0 HALF-IMPL | CRITICAL] `PipelineOwner::<Compositing>::run_compositing` is a `for node in list { tracing::trace!() } list.clear()` stub

**Evidence:**
- `crates/flui-rendering/src/pipeline/owner.rs:918-949`:
  ```rust
  pub fn run_compositing(&mut self) -> crate::error::RenderResult<()> {
      tracing::debug!("run_compositing: {} nodes", self.dirty.needs_compositing.len());

      // Sort by depth (shallow first)
      self.dirty.needs_compositing.sort_unstable_by_key(|node| node.depth);

      // Process dirty nodes
      //
      // Note: Full compositing bits update is not yet implemented.
      // This would require:
      // 1. PipelineOwner to hold a reference to RenderTree
      // 2. Look up each render object by ID
      // 3. Call render_object.update_compositing_bits()
      //
      // Currently we just clear the list - compositing works but
      // may not be optimally batched.
      for node in &self.dirty.needs_compositing {
          tracing::trace!(
              "compositing bits update: node id={} depth={} (batching not implemented)",
              node.id, node.depth
          );
      }
      self.dirty.needs_compositing.clear();
      Ok(())
  }
  ```
- The comment block #1-#3 enumerates the missing work. (#1 is wrong — `PipelineOwner` already holds `render_tree: RenderTree` inline at `owner.rs:99`; the bullet point reflects an outdated mental model.)
- The path returns `Ok(())` and clears the dirty list. No actual compositing-bits update happens — the path is **silent**.

**Why it's a problem:**
- **Worse than R-1** because the panic is loud and forces a fix; this is silent and lets the bug remain hidden.
- Flutter's `flushCompositingBits` walks each dirty node, computes whether descendant subtrees need compositing (via the `_updateSubtreeCompositingBits` recursion), and marks the layer-tree compositing dirty. FLUI silently does none of this.
- Combined with R-1 (panic) the picture is: paint runs even without correct compositing bits (so works for the common case), but the moment semantics fires the whole pipeline panics.

**Flutter reference:** `rendering/object.dart::PipelineOwner.flushCompositingBits` (lines 380-420 in current Flutter master). The walk + `_updateSubtreeCompositingBits` recursion is the canonical impl.

**Fix shape:**
```rust
pub fn run_compositing(&mut self) -> crate::error::RenderResult<()> {
    let dirty = std::mem::take(&mut self.dirty.needs_compositing);
    if dirty.is_empty() {
        return Ok(());
    }
    tracing::debug!("run_compositing: {} nodes", dirty.len());

    let mut dirty = dirty;
    dirty.sort_unstable_by_key(|n| n.depth);

    for node in &dirty {
        if let Some(render_node) = self.render_tree.get(node.id) {
            // Walk the subtree, update each render-state's NEEDS_COMPOSITING flag
            // based on is_repaint_boundary + always_needs_compositing.
            // Skeleton — full Flutter parity is its own follow-up.
            let entry = render_node.box_render_object();
            let needs = entry.always_needs_compositing()
                     || entry.is_repaint_boundary();
            if let Some(state_entry) = render_node.as_box() {
                if needs {
                    state_entry.state().flags().set(crate::storage::flags::RenderFlags::NEEDS_COMPOSITING);
                } else {
                    state_entry.state().flags().remove(crate::storage::flags::RenderFlags::NEEDS_COMPOSITING);
                }
            }
        }
    }
    Ok(())
}
```
Even the skeleton above replaces the silent no-op with a structurally correct (if approximate) update.

**Blast radius:** owner.rs only. ~30 LOC change.

---

#### R-5 [P0 DRIFT | CRITICAL] `RenderDirtyPropagation` trait uses `ElementId` not `RenderId`

**Evidence:**
- `crates/flui-rendering/src/storage/state/propagation.rs:15`:
  ```rust
  use flui_foundation::ElementId;
  ```
- `crates/flui-rendering/src/storage/state/propagation.rs:33-72`:
  ```rust
  pub(crate) trait RenderDirtyPropagation {
      fn parent(&self, id: ElementId) -> Option<ElementId>;
      fn get_render_state<P: Protocol>(&self, id: ElementId) -> Option<&RenderState<P>>;
      fn register_needs_layout(&mut self, id: ElementId);
      fn register_needs_paint(&mut self, id: ElementId);
      fn register_needs_compositing_bits_update(&mut self, id: ElementId);
      fn is_repaint_boundary(&self, id: ElementId) -> bool;
      fn was_repaint_boundary(&self, id: ElementId) -> bool;
  }
  ```
- The trait is `pub(crate)`, decorated with `#[expect(dead_code, reason = "preserved as cost-cheap option for a possible viewport-invalidation hook")]`.
- The crate's `RenderTree` operates on `RenderId`, not `ElementId`. Implementing this trait on `RenderTree` would require a translation layer (which `flui-view` owns as part of the Element-tree).
- PR #81 U3 preserved this trait shape as a "cost-cheap option"; the U3 commit message specifically says the trait body keeps "the previous design's `ElementId` parameter for the eventual viewport-invalidation hook". But preserving the wrong-type trait body sets up the next person who attempts viewport-invalidation to think `ElementId` is the right key.

**Why it's a problem:**
- The trait codifies a render-tree API in element-tree terms. If/when the viewport-invalidation hook materializes, the implementer will discover the type mismatch and either (a) rewrite the trait (defeating the "preserved as cost-cheap option" premise), or (b) introduce an ElementId↔RenderId translation layer (which doesn't exist in flui-rendering — that's flui-view's job).
- The `#[expect(dead_code)]` lint allows the issue to compound silently.

**Fix shape (canonical):** Two choices:
1. **Delete the trait body entirely**, replace the `propagation.rs` file with a 5-line doc-stub explaining where the future viewport-invalidation hook lives. Net: −73 LOC.
2. **Rewrite the trait to use `RenderId`** if the cost-cheap-option premise holds. The trait body becomes:
   ```rust
   pub(crate) trait RenderDirtyPropagation {
       fn parent(&self, id: RenderId) -> Option<RenderId>;
       // ...etc, all RenderId
   }
   ```
   This is the cycle-1 equivalent: a typestate kept as a future-bait shape, but with the right types.

The audit recommends choice 1 — by U3's logic, the trait has zero implementers and zero consumers, so deletion is the cleaner path. The cycle-1 pattern (PR #93 deletion of `typestate.rs`) is the exact precedent.

**Blast radius:** propagation.rs + state/mod.rs re-export. ~80 LOC reduction.

---

#### R-6 [P0 NESTED-LOCK-SMELL | CRITICAL] `RendererBinding::render_views` returns `&RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>` — interior mutability through trait surface

**Evidence:**
- `crates/flui-rendering/src/binding/mod.rs:145`:
  ```rust
  fn render_views(&self) -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>;
  ```
- Used internally at `binding/mod.rs:158-181` (add/remove/get_render_view default impls), `binding/mod.rs:344-415` (debug_dump_*), `binding/mod.rs:279-291` (handle_metrics_changed). All call sites pattern `.read()` then `.get()` or `.insert()`.
- Implementer at `crates/flui-app/src/bindings/renderer_binding.rs:362-364`:
  ```rust
  fn render_views(&self) -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>> {
      &self.render_views
  }
  ```
- External consumer at `crates/flui-app/src/bindings/renderer_binding.rs:361` calls `self.render_views()` then locks twice.

**Why it's a problem:**
- The deep-nested `Arc<RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>>` is **public**. Every external caller must:
  1. `.read()` the outer `RwLock` to get the HashMap.
  2. `.get(&view_id)` to clone the inner `Arc`.
  3. `.read()` the inner `RwLock` to access RenderView fields.
- Three locks for one operation. Lock ordering issues. Interior-mutability detail leaked through a trait method that is *supposed* to abstract over storage.
- The 2026-05-20 audit flagged this as "design smell that should be resolved before the first impl, not after". The first impl has now landed.
- Constitution Principle 4 ("Composition Over Inheritance") and Principle 5 ("Declarative API, imperative internals") — the trait is the public API; the lock topology is an internal implementation detail.

**Flutter reference:** `rendering/binding.dart::RendererBinding._views` is a `List<RenderView>` (Dart's single-threadedness means no locks). FLUI's choice to expose locks is a Rust-native shape but the *depth* (three locks) is excessive.

**Fix shape:** Replace the lock-leaking trait method with lookup methods:
```rust
pub trait RendererBinding: Send + Sync {
    // OLD: fn render_views(&self) -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>;
    // NEW: per-operation lookup methods that hide the lock topology.

    /// Returns the render view for the given ID, if it exists.
    /// The returned `Arc` is a cheap reference-count bump; the caller takes
    /// a read lock for actual access.
    fn render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>>;

    /// Iterates all render-view ids. The iterator borrows the outer lock
    /// for its lifetime, so it is held briefly inside `for` blocks.
    fn render_view_ids(&self) -> Vec<u64>;

    /// Inserts a render view.
    fn insert_render_view(&self, view_id: u64, view: Arc<RwLock<RenderView>>);

    /// Removes a render view, returning it if present.
    fn remove_render_view_by_id(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>>;
}
```
The `Arc<RwLock<RenderView>>` is still exposed but only via lookup, not as a public type-soup. The implementer owns the outer HashMap + lock as private state.

**Blast radius:** binding/mod.rs trait + flui-app's `RenderingFlutterBinding` impl. ~100 LOC delta. Migration is mechanical — every `binding.render_views().read().get(&id).cloned()` becomes `binding.render_view(id)`.

---

#### R-7 [P0 PARALLEL-TYPE | CRITICAL] Two `HitTestResult` types — `flui_rendering::HitTestResult` vs `flui_interaction::routing::HitTestResult`

**Evidence:**
- `crates/flui-rendering/src/hit_testing/result.rs:25-31`:
  ```rust
  pub struct HitTestResult {
      path: Vec<HitTestEntry>,
      transforms: Vec<MatrixTransformPart>,
  }
  ```
- `crates/flui-interaction/src/routing/hit_test.rs` defines a different `HitTestResult` (with `RouteEntry` items, not `HitTestEntry`; with `PointerRouter` integration, not transform stack).
- `crates/flui-app/src/app/binding.rs:503-510`:
  ```rust
  let mut render_result = flui_rendering::hit_testing::HitTestResult::new();
  // ...
  // TODO: Convert rendering HitTestEntry targets to interaction targets
  let result = flui_interaction::routing::HitTestResult::new();
  ```
- The literal TODO comment in flui-app codifies the two-type problem.

**Why it's a problem:**
- Hit testing is one of the core flows from input → render. Splitting `HitTestResult` between two crates means every event handler that traverses the tree must convert between them at the rendering-interaction boundary.
- The two structs carry overlapping concerns (the rendering-side has transforms + entries; the interaction-side has routing-specific data). Neither type re-exports or wraps the other.
- Cycle 2's `flui_types::Alignment` newtype work (PR #100/U21) is the canonical precedent for this pattern.

**Flutter reference:** `gestures/hit_test.dart::HitTestResult` is canonical. There's one `HitTestResult` class shared between rendering and gesture layers. Dart's flexibility allows the same type to serve both consumers; Rust forces a choice.

**Fix shape:** Choose one canonical home for `HitTestResult`:
- **Option A (preferred):** `flui-interaction` owns the canonical `HitTestResult`. flui-rendering uses it via direct import (already a dep). The render-side adds `BoxHitTestResult` / `SliverHitTestResult` as protocol-specialized wrappers (these already exist; they don't conflict).
- **Option B:** `flui-foundation` owns `HitTestResult` (one level deeper than both). Both flui-rendering and flui-interaction depend on flui-foundation already.

The audit recommends A — interaction is the consumer-side (event routing), and Flutter's `HitTestResult` lives in `gestures/` (the Flutter equivalent of flui-interaction). PR #84's framework-spine work moved several types to their canonical homes; this is the cycle-4 continuation.

**Blast radius:** ~50 LOC moved. flui-rendering's `hit_testing::result.rs` becomes a re-export of `flui_interaction::routing::HitTestResult` plus the rendering-specific `BoxHitTestResult` and `SliverHitTestResult`. Update flui-app's binding to remove the `// TODO: Convert` and use the single type. flui-interaction's `routing/hit_test.rs` may need to grow `MatrixTransformPart` handling (currently rendering-only).

---

#### R-8 [P0 PARALLEL-TYPE | CRITICAL] Two `MouseTrackerAnnotation` types — `flui_rendering` trait vs `flui_interaction` struct

**Evidence:**
- `crates/flui-rendering/src/input/mouse_tracker.rs:76-115`: `pub trait MouseTrackerAnnotation: Debug + Send + Sync` with `on_enter`/`on_hover`/`on_exit`/`cursor`/`is_valid` default-noop methods.
- `crates/flui-interaction/src/mouse_tracker.rs:77-125`: `pub struct MouseTrackerAnnotation { region_id, on_enter: Option<MouseEnterCallback>, on_exit, on_hover }` with builder methods.
- Each is `pub use`d at its crate's lib root.
- The two types are structurally **incompatible**: one is a trait users implement, the other is a struct with callback fields. Neither implements / wraps / converts to the other.

**Why it's a problem:**
- Same shape as R-7. Two crates own the same name for what is conceptually one concept (mouse tracking annotation).
- The naming collision is more painful because users importing one type from one crate's prelude get errors when consuming the other.
- `crates/flui-rendering/src/lib.rs:106` and `crates/flui-interaction/src/lib.rs:206`: both export `MouseTrackerAnnotation`. Any consumer using `use flui_rendering::prelude::*` followed by `use flui_interaction::prelude::*` gets a name collision.

**Flutter reference:** `rendering/mouse_tracker.dart::MouseTrackerAnnotation` is the canonical type — a class with `onEnter`, `onHover`, `onExit`, `cursor` properties. Flutter's `MouseRegion` widget creates one. There's only one in Flutter.

**Fix shape:** Decide who owns the canonical type. The two impls show two design philosophies:
- The rendering-side wants render-objects to implement a trait (so a `RenderMouseRegion` widget can write `impl MouseTrackerAnnotation for RenderMouseRegion`).
- The interaction-side wants a struct that callers pass to a `MouseTracker::register_annotation` method, registering arbitrary callbacks.

The two approaches are valid. The right answer: **flui-interaction owns the canonical struct** (matches Flutter's MouseRegion-creates-an-annotation pattern), and flui-rendering deprecates/deletes the trait. The render-object that wanted to implement the trait should instead register an annotation struct.

**Blast radius:** ~200 LOC. flui-rendering's `MouseTracker` (different struct from flui-interaction's `MouseTracker` — yet another duplication, see R-9) drops the trait dep. The 1 in-rendering callsite (`TestAnnotation` at `mouse_tracker.rs:497`) is test-only.

---

#### R-9 [P0 PARALLEL-TYPE | CRITICAL] Two `MouseTracker` types — one in flui-rendering, one in flui-interaction

**Evidence:**
- `crates/flui-rendering/src/input/mouse_tracker.rs:295+`: `pub struct MouseTracker { annotations: RwLock<HashMap<usize, Arc<dyn MouseTrackerAnnotation>>>, ... }`.
- `crates/flui-interaction/src/mouse_tracker.rs:148+`: `pub struct MouseTracker { annotations: HashMap<RegionId, MouseTrackerAnnotation>, ... }`.
- Both are `pub use`d at their crate's lib root.

**Why it's a problem:** Same shape as R-7/R-8. Two crates own two different `MouseTracker` structs for what should be one concept.

**Fix shape:** Bundle with R-8. Once `MouseTrackerAnnotation` consolidates to flui-interaction's struct version, `flui_rendering::input::MouseTracker` becomes either a thin wrapper that adapts to the flui-interaction tracker, or is deleted entirely. Audit consumers: `crates/flui-rendering/src/lib.rs:106` exports it; flui-app's `RenderingFlutterBinding` carries `mouse_tracker: RwLock<MouseTracker>` (`crates/flui-app/src/bindings/renderer_binding.rs:75`).

**Blast radius:** ~300 LOC delete in flui-rendering's `mouse_tracker.rs` (634 LOC total). flui-app's binding updates to use the flui-interaction MouseTracker.

---

#### R-10 [P0 PARALLEL-TYPE | CRITICAL] Two `RenderError` types — `flui_rendering::error::RenderError` vs `flui_engine::error::RenderError`

**Evidence:**
- `crates/flui-rendering/src/error.rs:17`:
  ```rust
  #[derive(Error, Debug, Clone)]
  #[non_exhaustive]
  pub enum RenderError {
      NotAttached,
      AlreadyAttached,
      ConfigurationNotSet,
      NodeNotFound(RenderId),
      InvalidParentChild,
      CycleDetected,
      InvalidConstraints { message: String },
      LayoutDuringPaint,
      LayoutDetached,
      RelayoutBoundaryViolation { message: String },
      // ...
      InvalidGeometry { render_object: &'static str, reason: &'static str },
      UnboundedConstraint { render_object: &'static str },
      LayoutDepthExceeded { limit: usize },
      Poisoned { render_object: &'static str, phase: &'static str },
  }
  ```
- `crates/flui-engine/src/error.rs:46`:
  ```rust
  #[derive(Debug, Error)]
  #[non_exhaustive]
  pub enum RenderError {
      SurfaceLost,
      SurfaceOutdated,
      Timeout,
      OutOfMemory,
      ResourceCreation(String),
      // ...
      ShaderError(String),
      PipelineError(String),
      TextRender(String),
      InvalidState(String),
      NotInitialized,
  }
  ```
- Both `#[non_exhaustive]` (cycle 4 opener added this — `aea56399`). Both have `RenderResult<T> = Result<T, RenderError>` aliases (`flui-rendering/src/error.rs:184`, `flui-engine/src/error.rs:208`).
- flui-app consumes both:
  - `crates/flui-app/src/app/binding.rs:33`: `use flui_engine::{RenderError, wgpu::Renderer};`
  - flui-app's pipeline integration uses `flui_rendering::RenderError` for pipeline failures.

**Why it's a problem:**
- Same name in two crates, both `pub`. Same `RenderResult<T>` alias. Anyone importing `use flui_rendering::*; use flui_engine::*;` gets a collision.
- Conceptually they ARE different things (rendering = pipeline phase errors; engine = GPU errors). But the shared name is the worst case for the two-namespaces choice.
- Cycle 4 opening commit `aea56399` added `#[non_exhaustive]` to both, codifying the parallel structure rather than consolidating.

**Fix shape:** Rename one. Two options:
- **Option A (audit-recommended):** rename `flui_engine::RenderError` → `flui_engine::EngineError`. The engine-side is about GPU+wgpu+surface, "engine" is the right namespace. `flui-rendering::RenderError` stays as the canonical rendering error.
- **Option B:** rename `flui_rendering::RenderError` → `flui_rendering::PipelineError`. The rendering-side is about pipeline phase failures, "pipeline" is the right semantic.

The audit recommends A because `engine` is the consumer-known name. flui-engine's existing variants (`SurfaceLost`, `OutOfMemory`) are clearly engine concerns; "engine" matches.

**Blast radius:** ~30 LOC. flui-engine's error module rename + lib re-export + 1 flui-app import path update. Public API breaking change for flui-engine consumers (only flui-app); single-line fix.

---

#### R-11 [P1 PARALLEL-TYPE | HIGH] Two `ParentData` traits — `flui_rendering::ParentData` vs `flui_view::ParentData`

**Evidence:**
- `crates/flui-rendering/src/parent_data/base.rs` defines the canonical render-side `pub trait ParentData: Debug + Send + Sync + 'static` with `Any` downcasting + `Self::Cloned`.
- `crates/flui-view/src/view/parent_data.rs:68`: `pub trait ParentData: Clone + Default + Send + Sync + 'static {}` — minimal, no `Any`, no downcasting.
- Both `pub use`d at their crate's lib root.

**Why it's a problem:**
- flui-view's `ParentData` is a marker trait for the view-side ParentData widget integration; flui-rendering's is the actual storage trait on `RenderObject`.
- They serve different concerns but share the same name. The view-side `ParentData` doesn't unify with the render-side `ParentData` via sub/super-traiting.

**Fix shape:** Rename one. `flui_view::ParentData` → `flui_view::ParentDataConfig` (matching Flutter's `ParentDataWidget.applyParentData(RenderObject)` shape where the widget *configures* the parent-data, not *is* the parent-data).

**Blast radius:** ~10 LOC in flui-view's parent_data module + 1 lib re-export.

---

#### R-12 [P1 HALF-IMPL | HIGH] `RenderTree::set_owner` 30-LOC TODO that doesn't run attach/detach lifecycle

**Evidence:**
- `crates/flui-rendering/src/storage/tree.rs:104-119`:
  ```rust
  /// Sets the pipeline owner.
  ///
  /// This will attach all existing nodes to the new owner.
  ///
  /// # Note
  ///
  /// Currently this only stores the owner reference. Full attach/detach
  /// lifecycle would require:
  /// 1. Iterating all nodes and calling their attach/detach methods
  /// 2. Adding `attached` state tracking to RenderEntry
  /// 3. Notifying the owner about all existing dirty nodes
  pub fn set_owner(&mut self, owner: Option<Arc<RwLock<PipelineOwner>>>) {
      // Note: Full attach/detach lifecycle for existing nodes is not yet implemented.
      self.owner = owner;
  }
  ```

**Why it's a problem:**
- The docstring promises "attach all existing nodes" — the impl does the opposite (silently no-op on existing nodes). A consumer reading the docstring is misled.
- Cycle 3 PR #103 hoisted `TreeWrite::remove` cascade-by-default. The cycle-4 equivalent is: `RenderTree::set_owner` should be a real lifecycle event, OR the doc should be honest about doing nothing.

**Flutter reference:** `rendering/object.dart::RenderObject.attach(PipelineOwner)` is a recursive subtree walk that propagates the owner to all descendants. Each render object calls `super.attach(owner); attach owner to children`.

**Fix shape:** Two options:
- **Option A:** Implement the recursive attach. Add an `attached: AtomicBool` to `RenderState<P>` (one more bit on the existing AtomicU32; it's already at 11 bits, plenty of room). On `set_owner(Some(_))`, walk every node, flip `attached`, register with the owner's dirty lists.
- **Option B:** Trim the docstring to match reality: *"Stores the owner reference. The attach/detach lifecycle is the caller's responsibility — call this AFTER all nodes are inserted, or use `PipelineOwner::insert` directly."*

Option B is the lower-cost honest path; Option A is the cycle-4-shaped fix that matches Flutter parity. The audit recommends Option B for this cycle (the actual attach/detach work is a follow-up).

**Blast radius:** Option B: ~10 LOC docstring change. Option A: ~50 LOC + state bit + tests.

---

#### R-13 [P1 HALF-IMPL | HIGH] `propagate_constraints_to_child` and `sync_child_size_to_parent` are empty-body stubs reached every layout

**Evidence:**
- `crates/flui-rendering/src/pipeline/owner.rs:885-892`:
  ```rust
  fn propagate_constraints_to_child(&self, _parent_id: RenderId, _child_id: RenderId) {}
  fn sync_child_size_to_parent(&mut self, _child_id: RenderId) {}
  ```
- Both are called from `layout_node_with_children` at lines 862 (`propagate_constraints_to_child`) and 872 (`sync_child_size_to_parent`).
- The docstrings on lines 878-891 describe what these methods are supposed to do: propagate constraints from parent to child, sync child sizes back to parent's ChildState.
- The current empty bodies mean the layout phase **does not propagate constraints** and **does not sync child sizes** — the comments at lines 882-884 admit this: *"We pass loose constraints (same max, zero min)"* yet nothing actually propagates them.

**Why it's a problem:**
- Layout protocol violation. Flutter's layout walks parent → child with constraints flowing down (`performLayout` invokes `child.layout(constraints)`); FLUI's loop walks parent → child but the methods that should carry the constraint payload are empty.
- The render-object's `perform_layout_raw` receives its constraints from `RenderEntry::layout`, which currently passes whatever it had cached. So the constraints DO propagate via a different path (the per-entry cached `ProtocolConstraints<P>`) — making the two stub methods either dead code or load-bearing-but-stubbed.
- **Verify which**: if `RenderEntry::layout` carries the constraint, the two methods are dead code that should be deleted. If `layout_node_with_children` ALSO needs to set the constraint up before calling `RenderEntry::layout`, the stubs are load-bearing-but-stubbed.

**Fix shape:**
1. **Investigate** which path actually carries the constraint payload. Read `RenderEntry::layout` (`storage/entry.rs`) and trace the constraint flow.
2. **If RenderEntry handles it**: delete the two stub methods + their docstrings + the call sites.
3. **If they're load-bearing**: write the real impl. Flutter parity is the canonical reference.

**Blast radius:** owner.rs only. ~20 LOC delete (path 2) OR ~50 LOC fill-in (path 3).

---

#### R-14 [P1 DEAD-CODE | HIGH] `RenderView` carries 9 `#[allow(dead_code)]` fields with placeholder comments

**Evidence:**
- `crates/flui-rendering/src/view/render_view.rs:64-88`:
  ```rust
  #[allow(dead_code)] // Placeholder for full RenderView implementation
  depth: usize,
  #[allow(dead_code)]
  needs_layout: bool,
  #[allow(dead_code)]
  needs_paint: bool,
  #[allow(dead_code)]
  needs_compositing_bits_update: bool,
  #[allow(dead_code)]
  needs_semantics_update: bool,
  #[allow(dead_code)]
  is_repaint_boundary: bool,
  #[allow(dead_code)]
  needs_compositing: bool,
  #[allow(dead_code)]
  cached_constraints: Option<BoxConstraints>,
  #[allow(dead_code)]
  parent_data: Option<Box<dyn ParentData>>,
  ```
- The 9 fields are initialized in `RenderView::new()` at lines 121-128 with sensible defaults — `needs_layout: true`, `is_repaint_boundary: true`, etc — and `set_configuration` at line 180 mutates `self.needs_layout = true`. So `needs_layout` is **written** but not read; the same dead-code lint suppression applies to the others.
- These fields mirror what `RenderState<P>` carries (via `AtomicRenderFlags`) — RenderView is BEFORE the protocol-aware RenderObject machinery, so it carries its own field set.

**Why it's a problem:**
- 9 `#[allow(dead_code)]` suppression markers signal incomplete RenderView. The "Placeholder for full RenderView implementation" comment at line 65 admits this.
- RenderView is the root of the render tree. Its lifecycle bits SHOULD be load-bearing — Flutter's `RenderView` has `markNeedsLayout`, `_size`, `_rootTransform` all read in real code paths.
- Some fields ARE used (the file flips `self.needs_layout = true` at line 180; that's a write but no read confirms the field actually drives anything).

**Fix shape:** Two-pass approach:
1. **Delete the truly dead fields** — those that are neither written nor read. After investigation: `needs_compositing_bits_update`, `needs_semantics_update`, `cached_constraints`, `parent_data` are write-zero-read-zero. `depth`, `is_repaint_boundary`, `needs_compositing` are constants set at construction never read. Delete.
2. **Promote the load-bearing fields** — `needs_layout`, `needs_paint` — to `AtomicBool` or move them to `RenderState`. The framework reads them when scheduling the next frame.

**Blast radius:** render_view.rs only. ~30 LOC reduction + structural cleanup.

---

#### R-15 [P1 HALF-IMPL | HIGH] `run_paint` doesn't sort dirty_paint by depth; nodes outside `root_id`'s descent are silently dropped

**Evidence:**
- `crates/flui-rendering/src/pipeline/owner.rs:983-994`:
  ```rust
  let dirty_nodes = std::mem::take(&mut self.dirty.needs_paint);

  // Sort by depth (deep first) - children before parents
  // Flutter: dirtyNodes.sort((a, b) => b.depth - a.depth)
  // Note: We don't need to sort for now since we paint from root

  // Clear needs_paint flags for all dirty nodes
  for dirty_node in &dirty_nodes {
      if let Some(render_node) = self.render_tree.get(dirty_node.id) {
          render_node.clear_needs_paint();
      }
  }
  ```
- Lines 990-994 iterate the dirty list ONLY to clear flags. No painting happens here.
- The actual painting at lines 1003-1029 walks ONLY from `self.root_id` via `paint_node_recursive`. A node-needing-paint that is not reachable from root_id during this descent is never painted — its flag is cleared at line 992 but its paint command is dropped.
- This works as long as `root_id` is the ONLY root and every dirty node is in its subtree. The moment a multi-root design (or a detached subtree) emerges, the bug manifests.
- The comment *"We don't need to sort for now since we paint from root"* is correct for the current architecture but commits to a single-root invariant that the audit-recommendation in cycle 3's view-tree didn't promise.

**Why it's a problem:**
- The flag-clear loop diverges from the paint walk. Both must agree on which nodes are painted.
- Flutter's `flushPaint` sorts deep-first AND paints each node — there's no separate "clear" pass; the paint method itself clears the flag.

**Fix shape:**
```rust
pub fn run_paint(&mut self) -> crate::error::RenderResult<()> {
    let dirty = std::mem::take(&mut self.dirty.needs_paint);
    if dirty.is_empty() {
        return Ok(());
    }
    tracing::debug!("run_paint: {} nodes", dirty.len());
    self.debug_doing_paint = true;

    // Sort deep-first so repaint-boundary subtrees emit before ancestor compositing.
    let mut dirty = dirty;
    dirty.sort_unstable_by_key(|n| std::cmp::Reverse(n.depth));

    // Paint via root descent (unchanged); the clear-needs_paint is folded into
    // paint_node_recursive so the deep-first dirty list doesn't double-clear.
    if let Some(root_id) = self.root_id
        && let Some(root_node) = self.render_tree.get(root_id)
    {
        let paint_bounds = root_node.paint_bounds();
        let mut context = CanvasContext::new(paint_bounds);
        // paint_node_recursive calls render_node.clear_needs_paint() per node visited.
        match self.paint_node_recursive(&mut context, root_id, Offset::ZERO) {
            Ok(()) => self.last_layer_tree = Some(context.into_layer_tree()),
            Err(e) => { self.debug_doing_paint = false; return Err(e); }
        }
    }

    // For any nodes-needing-paint that weren't reached by the descent (detached
    // subtrees, multi-root), tracing-warn so the bug is visible.
    // The flag-clear pass is removed — paint_node_recursive owns clearing.

    self.debug_doing_paint = false;
    Ok(())
}
```

**Blast radius:** owner.rs only. ~20 LOC change.

---

#### R-16 [P1 DEAD-CODE | HIGH] 4 delegate trait modules ~1,800 LOC with 0 production impls

**Evidence:**
- `crates/flui-rendering/src/delegates/{custom_painter, flow_delegate, multi_child_layout_delegate, single_child_layout_delegate, sliver_grid_delegate, custom_clipper}.rs`.
- LOC: custom_painter 248, flow_delegate 248, multi_child_layout_delegate 168, single_child_layout_delegate 168, sliver_grid_delegate 428, custom_clipper 168 = ~1,428 LOC + the prelude/mod.rs reexports.
- All `impl <Delegate> for ...` workspace-wide: only test mocks (`InsetClipper`, `LinearFlowDelegate`, `CheckerboardPainter`, `DialogLayoutDelegate`) inside test modules.
- The 2026-05-20 audit flagged at MEDIUM with the verdict "wait for companion render-object consumers (RenderCustomPaint, RenderFlow, RenderCustomMultiChildLayoutBox, etc)". **18 months later, none of these render-objects exist**.

**Why it's a problem:**
- ~1,800 LOC of public API surface with zero workspace usage.
- The companion render-objects haven't materialized because their order-of-implementation depends on the rest of the workspace materializing first (widgets that wrap them, the view-tree binding to set the delegate, etc).
- Cycle 1's pattern (PR #93 typestate.rs deletion) and cycle 3's pattern (PR #105 feature-gating zero-consumer surfaces) both apply here.

**Fix shape:** Feature-gate behind `experimental-delegates`, default off:
```toml
[features]
default = []
experimental-delegates = []
```
```rust
// in lib.rs:
#[cfg(feature = "experimental-delegates")]
pub mod delegates;
```
The 1,800 LOC stops being part of the default compile + prelude surface; the feature flag is the opt-in for someone landing the companion render-objects.

**Blast radius:** lib.rs feature-gate + prelude module conditional. The 6 delegate modules stay in place. ~20 LOC of feature-cfg additions.

---

#### R-17 [P1 DEAD-CODE | HIGH] `RenderError` 5 `String`-typed variant fields force allocation

**Evidence:**
- `crates/flui-rendering/src/error.rs:53-128`:
  ```rust
  InvalidConstraints { message: String },
  RelayoutBoundaryViolation { message: String },
  LayerError { message: String },
  PhaseOrderViolation { phase: &'static str },  // <-- good
  CompositingError { message: String },
  SemanticsError { message: String },
  ```
- 5 variants carry `String` payload. The constructors at lines 188-220 take `impl Into<String>`. Per-call heap alloc.
- The `&'static str` variants (`Poisoned.render_object`, `InvalidGeometry.render_object`, `UnboundedConstraint.render_object`) are the right shape — no allocation, debug_name comes from `core::any::type_name::<Self>()` (default impl on `RenderObject::debug_name`).

**Why it's a problem:**
- Cycle 3 T-16 hit the same shape in `TreeError::Internal(Box<str>)`; the cycle-3 cleanup PR #106 converted `String` → `Box<str>` for narrow shrink. Same applies here.
- Errors are constructed on error paths so per-instance allocation isn't the worst — but `RenderError` is `Clone` (`#[derive(Clone)]` line 15), so each clone bumps the String refcount-less heap copy.

**Fix shape:** Replace 5 `String` fields with `Box<str>` for narrow shrinkage:
```rust
InvalidConstraints { message: Box<str> },
// ...etc.
```
Constructors accept `impl Into<Box<str>>` (or take `String` and convert via `.into_boxed_str()`). The Display impl is unchanged (`{message}` works for both `String` and `Box<str>`).

**Blast radius:** error.rs only. ~20 LOC. Public API breaking for direct field access (`err.message` typed as `String` becomes `Box<str>`); but consumers go through `Display` not field access in practice.

---

#### R-18 [P1 DEAD-CODE | HIGH] `ScrollMetrics` trait + 2 impls — 452 LOC, 0 external consumers

**Evidence:**
- `crates/flui-rendering/src/constraints/scroll_metrics.rs`: 452 LOC defining `ScrollMetrics` trait + `FixedScrollMetrics` + `FixedExtentMetrics`.
- `crates/flui-rendering/src/constraints/mod.rs:94`: re-exports all three.
- `crates/flui-rendering/src/constraints/mod.rs:156-157`: re-exports in prelude.
- Workspace grep: 0 external consumers. Only internal mentions in `constraints/mod.rs`.

**Why it's a problem:**
- 452 LOC + tests of dead public surface. Same shape as R-16's delegates.
- The 2026-05-20 audit flagged "needs manual confirmation" because the consumer would be the scrolling system, which hasn't materialized.

**Fix shape:** Feature-gate behind `scrolling` or `experimental-scroll`, default off. Bundle with R-16 if the broader feature-gate-incomplete-systems strategy is adopted.

**Blast radius:** constraints/mod.rs feature-gate + prelude conditional.

---

#### R-19 [P1 DEAD-CODE | HIGH] `ScrollableViewportOffset` listener API — 50 LOC `#[allow(dead_code)]`

**Evidence:**
- `crates/flui-rendering/src/view/viewport_offset.rs:164`: `#[allow(dead_code)] // Reserved for future ViewportOffset listener API`.
- The surrounding code is the ScrollableViewportOffset's listener registration.

**Why it's a problem:**
- "Reserved for future" plus `#[allow(dead_code)]` is exactly the "forward-looking helpers without removal cadence" pattern the 2026-05-20 audit flagged.

**Fix shape:** Either delete OR add a `// REMOVE_BY: 2026-09-22` cadence comment OR feature-gate. Audit recommendation: delete.

**Blast radius:** viewport_offset.rs only. ~50 LOC reduction.

---

#### R-20 [P2 API-SURFACE | MEDIUM] `RendererBinding::render_views() -> &RwLock<HashMap<...>>` is a `pub trait` method that exposes private locking

(Same as R-6 — listed here as a P2 alternative if R-6's full redesign doesn't ship.)

**Fix shape (minimal version):** Mark the trait method as `pub(crate)` if possible OR deprecate it with `#[deprecated(note = "Use render_view(id) instead")]` and provide the lookup-style methods alongside. Doesn't fix the lock topology but doesn't propagate the pattern further.

**Blast radius:** binding/mod.rs only.

---

#### R-21 [P2 API-SURFACE | MEDIUM] `RenderObject<P>::insert_into_pipeline` is API surface that smuggles in `where Self: Sized` + `From<Box<dyn>>` trait bound

**Evidence:**
- `crates/flui-rendering/src/traits/render_object.rs:350-359`:
  ```rust
  fn insert_into_pipeline(
      self: Box<Self>,
      owner: &mut crate::pipeline::PipelineOwner,
  ) -> flui_foundation::RenderId
  where
      Self: Sized,
      crate::storage::RenderNode: From<Box<dyn RenderObject<P>>>,
  {
      owner.insert(self)
  }
  ```
- A method on the trait that requires `Self: Sized` and a `From` impl on a different type (`RenderNode`).
- Convenience wrapper around `PipelineOwner::insert(box self)`. The trait pollution earned little — the `From<Box<dyn>>` impl exists at `storage/node.rs:84-94`.

**Why it's a problem:**
- Pollutes the trait with a convenience method that only works for `Sized` types. The `Self: Sized` bound means you can't call this through a `dyn RenderObject<P>`.
- The `From<Box<dyn RenderObject<P>>> for RenderNode` impl is the real load-bearing piece, sitting in `storage/node.rs`.
- A standalone free function `insert_render_object<P: Protocol>(owner: &mut PipelineOwner, ro: Box<dyn RenderObject<P>>) -> RenderId` would be cleaner.

**Fix shape:** Move to a free function in `pipeline/owner.rs` or remove (`PipelineOwner::insert(box)` is the single-step equivalent).

**Blast radius:** traits/render_object.rs trait + 1-2 callsites.

---

#### R-22 [P2 DEAD-CODE | MEDIUM] `DummyTarget` private struct in `hit_testing/result.rs` is a placeholder

**Evidence:**
- `crates/flui-rendering/src/hit_testing/result.rs:230-238`:
  ```rust
  struct DummyTarget;
  impl HitTestTarget for DummyTarget {
      fn handle_event(&self, _event: &super::target::PointerEvent, _entry: &HitTestEntry) {}
      fn debug_label(&self) -> &'static str { "DummyTarget" }
  }
  ```
- Used in `HitTestResult::add_with_position` at line 91-99 — when the caller provides a position but no target, the function creates a fake entry pointing to `DummyTarget`.
- Used in `HitTestEntry::new` test impls (`entry.rs:53`, `entry.rs:65`).

**Why it's a problem:**
- The `add_with_position` use case is a sign of API design that lets callers add hit-entries without a target. That's semantically meaningless (Flutter's HitTestResult.add always requires a target).
- The dummy target carries through to the dispatch path. If a downstream consumer iterates the result, the dummy target's `handle_event` is invoked (no-op) and `debug_label` returns `"DummyTarget"` — telltale of a non-real entry.

**Fix shape:** Delete `add_with_position` (the only caller of `DummyTarget` outside test). Replace test usages with proper `Arc<dyn HitTestTarget>` test impls. Remove `DummyTarget`.

**Blast radius:** result.rs + entry.rs. ~30 LOC reduction.

---

#### R-23 [P2 ARCH-SMELL | MEDIUM] `BoxHitTestResult::wrap` returns a NEW separate result while doc claims it shares storage

**Evidence:**
- `crates/flui-rendering/src/hit_testing/result.rs:261-274`:
  ```rust
  /// Wraps a `HitTestResult` to provide box-specific hit testing.
  ///
  /// This creates a new `BoxHitTestResult` that shares the underlying
  /// storage with the provided `HitTestResult`, allowing seamless
  /// integration between the two hit testing systems.
  ///
  /// Note: In this implementation, we create a new result that can be
  /// merged back if needed. For full Flutter compatibility, this would
  /// share the same storage.
  pub fn wrap(_result: &mut HitTestResult) -> Self {
      Self::new()
  }
  ```
- The doc starts by claiming "shares the underlying storage". The note at line 268 admits the actual impl does NOT share storage. Doc lies.
- Flutter's `BoxHitTestResult.wrap(HitTestResult)` actually DOES share storage — `result.path` and `result.transforms` are the same Lists.

**Why it's a problem:**
- Doc-lying. The function name `wrap` implies wrapping, but the impl creates a separate object.
- Downstream callers that read the doc and pass the wrapped result back expecting merged entries will lose data.

**Fix shape:** Either:
1. Implement real shared storage (probably needs an `&mut Vec<HitTestEntry>` reference in `BoxHitTestResult`).
2. Rewrite the doc to match reality + rename to `from_separate` or similar.

Audit recommends 2 — the structural change for shared storage is significant; the doc lie is the immediate fix.

**Blast radius:** result.rs only. Doc + name change.

---

#### R-24 [P2 ARCH-SMELL | MEDIUM] `HitTestResult` carries `Vec<MatrixTransformPart>` transform stack + per-call matrix multiply chain

**Evidence:**
- `crates/flui-rendering/src/hit_testing/result.rs:102-113`:
  ```rust
  fn current_transform(&self) -> MatrixTransformPart {
      if self.transforms.is_empty() {
          MatrixTransformPart::default()
      } else {
          let mut result = Matrix4::IDENTITY;
          for t in &self.transforms {
              result *= t.to_matrix();
          }
          MatrixTransformPart::Matrix(result)
      }
  }
  ```
- Called from `add_target` and `add_with_position` for every entry added during hit-testing traversal.
- A deep tree (transform depth N) yields O(N) matrix multiplies per hit-test entry. For a hit-test that walks 30 levels deep with a long transform stack, that's 30 multiplies per entry.

**Why it's a problem:**
- Hit testing is hot-path. Per-call matrix-multiply chain is the dominant cost.
- Flutter's `HitTestResult._localTransforms` caches the composed matrix incrementally on push/pop.

**Fix shape:** Incremental composition. Maintain `composed: Matrix4` alongside `transforms: Vec<MatrixTransformPart>`; on push, multiply; on pop, recompute from the (now shorter) `transforms` list. Or use a SmallVec for the typical-shallow case + memoize the composed result.

**Blast radius:** result.rs + add_target / add_with_position paths.

---

#### R-25 [P3 HYGIENE | LOW] `RenderError` derives `Clone` — error types typically should not

**Evidence:**
- `crates/flui-rendering/src/error.rs:15`:
  ```rust
  #[derive(Error, Debug, Clone)]
  #[non_exhaustive]
  pub enum RenderError { ... }
  ```
- Errors are typically `Debug + Display + Error` — `Clone` is unusual. Many error types deliberately don't derive `Clone` because cloning an error can have unbounded payload (e.g. `Source: Box<dyn Error + Send + Sync>` is not `Clone`).
- Looking at the variants, none carry `Box<dyn Error>` — they're all `String` or `&'static str`. So `Clone` works. But the pattern signals an API shape that lets consumers stash errors / pass them around copied.

**Why it's a problem:**
- Minor: API shape signal. The audit-recommended `Box<str>` fix in R-17 would be `Clone`able too.
- If a future variant adds a `Source: Box<dyn Error + Send + Sync>`, the `Clone` derive breaks.

**Fix shape:** Either remove `Clone` (audit-preferred, breaks the consumer expectation that errors can be stashed by value) or document the expectation.

**Blast radius:** error.rs derive + audit consumers of `err.clone()`.

---

#### R-26 [P3 HYGIENE | LOW] `RenderTree::visit_depth_first_from` and `visit_depth_first_mut_from` use recursion; clone children to `Vec<RenderId>`

**Evidence:**
- `crates/flui-rendering/src/storage/tree.rs:571-584`:
  ```rust
  fn visit_depth_first_from<F>(&self, id: RenderId, f: &mut F)
  where F: FnMut(RenderId, &RenderNode),
  {
      if let Some(node) = self.get(id) {
          f(id, node);
          let children = node.children().to_vec();  // <-- Vec alloc per node
          for child_id in children {
              self.visit_depth_first_from(child_id, f);  // <-- recursion
          }
      }
  }
  ```
- Per-node Vec allocation on the `to_vec()`. Per-walk recursion depth proportional to tree depth.

**Why it's a problem:**
- Cycle 3 T-11 fixed the same pattern in `Descendants::next`. Same shape recurs here.
- For a deep tree (1000+ nodes), the per-node alloc adds up. The recursion depth blows the stack on pathologically deep trees.

**Fix shape:**
- Use SmallVec to skip alloc for the typical (≤8 children) case: `let children: SmallVec<[RenderId; 8]> = node.children().iter().copied().collect();`.
- For the recursion-depth concern, fold into iterative with an explicit `Vec<RenderId>` work-stack. (Audit recommends bundling with cycle 3's recursion-to-iteration cleanups.)

**Blast radius:** tree.rs only. ~15 LOC change.

---

### flui-engine findings (E-1 .. E-19)

---

#### E-1 [P0 HALF-IMPL | CRITICAL] `WgpuPainter::clip_path` silent no-op

**Evidence:**
- `crates/flui-engine/src/wgpu/painter.rs:3592-3612`:
  ```rust
  pub fn clip_path(&mut self, _path: &Path) {
      // Path clipping requires stencil buffer or path tessellation
      // This is a complex feature that needs:
      // 1. Stencil buffer configuration in render pass
      // 2. Tessellate path and render to stencil buffer
      // 3. Enable stencil test for subsequent draws
      // 4. Stack management for nested clips
      // 5. Handle even-odd vs non-zero fill rules
      // ...
      // For now, this is a no-op. Applications should use ClipRect or ClipRRect
      // for hardware-accelerated clipping. Path clipping will be implemented
      // in a future version with proper stencil buffer support.

      #[cfg(debug_assertions)]
      tracing::trace!(
          "WgpuPainter::clip_path: not implemented, use ClipRect or ClipRRect instead"
      );
  }
  ```
- `clip_path` is `pub fn` — public API. Direct callers can pass any Path and get a no-op.
- The layer-tree path (`Layer::ClipPath` → `LayerRender::ClipPathLayer` → `renderer.push_clip_path(path, behavior)`) works fine because it goes through `Backend::clip_path` (the trait method on `CommandRenderer`) which the engine handles via stencil buffer in a different code path. The direct `WgpuPainter::clip_path` is the **direct, painter-level** API; the consumer must know to use the layer route instead.

**Why it's a problem:**
- Silent. A user calling `WgpuPainter::clip_path(path)` expects clipping; gets a tracing-trace warning and no clip.
- Constitution Principle 6 in spirit — half-implementation in a public path.

**Fix shape:** Two options:
1. **Remove the method.** Users go through the layer tree (`Layer::ClipPath`) or use `clip_rect` / `clip_rrect`. Public API breaking.
2. **Make it loud.** Change to `debug_assert!(false, "clip_path not implemented")` OR return a `Result<(), PainterError>`, OR forward to a real impl.

Audit recommends 1 — the method is reachable but always wrong; deleting forces users to discover the right path.

**Blast radius:** painter.rs only. ~20 LOC deletion.

---

#### E-2 [P0 HALF-IMPL | CRITICAL] `Backend::render_backdrop_filter` fallback path renders child without filter

**Evidence:**
- `crates/flui-engine/src/wgpu/backend.rs:805-834`:
  ```rust
  fn render_backdrop_filter(
      &mut self,
      child: Option<&flui_painting::DisplayList>,
      _filter: &flui_painting::display_list::ImageFilter,
      _bounds: Rect<Pixels>,
      _blend_mode: BlendMode,
      _transform: &Matrix4,
  ) {
      // TODO: Implement full backdrop filter rendering
      //
      // Current architecture limitation: WgpuRenderer wraps WgpuPainter which doesn't
      // have access to OffscreenRenderer (lives in GpuRenderer).
      // ...
      tracing::warn!(
          "BackdropFilter rendering not yet fully implemented - rendering child without filter"
      );

      // Render child content without filtering (fallback behavior)
      if let Some(child) = child {
          for command in child.commands() {
              dispatch_command(command, self);
          }
      }
  }
  ```
- Architectural issue: `WgpuPainter` doesn't see `OffscreenRenderer` (lives on `Renderer`). Same shape as E-1 but more painful because the fallback (no filter) is silently wrong instead of silently absent.

**Why it's a problem:**
- BackdropFilter (the iOS frosted-glass effect, Flutter's `BackdropFilter` widget) is a visible feature. Falling back to no-filter is a visible regression vs Flutter.
- The `tracing::warn!` is logged once per frame per backdrop filter — log spam at high frame rates.
- The architectural fix is non-trivial: OffscreenRenderer needs to be reachable from `Backend::render_backdrop_filter`. Options: move OffscreenRenderer to `Painter`, pass `&mut OffscreenRenderer` parameter, share `Arc<Mutex<OffscreenRenderer>>` between Renderer and Painter.

**Fix shape:** Architecturally:
- **Option A (simpler):** Move `Arc<Mutex<OffscreenRenderer>>` from `Renderer` to `Painter`. Painter owns it; backdrop_filter has direct access.
- **Option B (mirror PathCache):** Painter owns a `SubcompositorCache` similar to `PathCache` / `SuperellipsePathCache`; OffscreenRenderer becomes the cache. Backdrop_filter consults the cache.

Audit recommends A — it minimizes the API surface change.

**Blast radius:** Renderer → Painter ownership rotation. ~50 LOC + tests.

---

#### E-3 [P0 DEAD-CODE | CRITICAL] `OffscreenRenderer::PipelineManager` + `PipelineHandle` 50 LOC zombie wrapper

**Evidence:**
- `crates/flui-engine/src/wgpu/offscreen.rs:1352-1402`:
  ```rust
  pub struct PipelineManager {
      shader_cache: Arc<ShaderCache>,
      // TODO: Add actual pipelines when integrating with wgpu
      // device: Arc<wgpu::Device>,
      // pipelines: HashMap<ShaderType, wgpu::RenderPipeline>,
  }

  impl PipelineManager {
      pub fn new(shader_cache: Arc<ShaderCache>) -> Self {
          Self { shader_cache }
      }

      pub fn get_or_create_pipeline(&self, shader_type: ShaderType) -> PipelineHandle {
          let _shader = self.shader_cache.get_or_compile(shader_type);
          tracing::trace!("Getting pipeline for shader: {:?}", shader_type);
          // TODO: Create actual wgpu::RenderPipeline
          PipelineHandle { shader_type }
      }
  }

  pub struct PipelineHandle {
      pub shader_type: ShaderType,
      // TODO: Add Arc<wgpu::RenderPipeline> when integrated
  }
  ```
- `PipelineManager` carries only a `shader_cache` field. The TODO fields (`device`, `pipelines`) are commented out.
- `PipelineHandle` carries only `shader_type`. The TODO field (`Arc<wgpu::RenderPipeline>`) is commented out.
- `pub use offscreen::{MaskedRenderResult, OffscreenRenderer, PipelineManager};` at `wgpu/mod.rs:140`. Re-exported externally.
- Workspace grep: 0 consumers of `PipelineManager` or `PipelineHandle` outside their own definition.

**Why it's a problem:**
- 50 LOC of TODO-driven zombie. The real pipeline management happens in `pipelines.rs::PipelineCache` which `Backend` actually uses.
- The `PipelineHandle` type carries a single field (`shader_type`) — semantically equivalent to a `(ShaderType,)` tuple. The zero-fields-of-substance struct is pure architecture theater.

**Fix shape:** Delete both types. The intended replacement (`pipelines.rs::PipelineCache`) is the real impl. Add a doc-comment to `offscreen.rs` summarizing what the deleted shape was attempting and pointing to `pipelines.rs`.

**Blast radius:** offscreen.rs + mod.rs re-export. ~60 LOC reduction.

---

#### E-4 [P0 DEAD-CODE | CRITICAL] Effects module forward-looking helpers — `BlurParams`/`BlurIntensity`/`LinearGradientBuilder` zero production use

**Evidence:**
- `crates/flui-engine/src/wgpu/effects.rs:402-510`:
  - `BlurParams` (line 405): used ONLY by `effects.rs` tests + `offscreen.rs` has its OWN `BlurParams` (line 1305, different struct). Two parallel BlurParams in the engine.
  - `BlurIntensity` enum (line 427): used ONLY by `effects.rs` tests.
  - `LinearGradientBuilder` (line 465): used ONLY by `effects.rs` tests.
- `crates/flui-engine/src/wgpu/effects.rs:296-336`:
  - `ShadowParams::elevation_1/2/3/4/5`: used ONLY by `effects.rs` tests + 1 docstring reference in `painter.rs:3931`.
- `wgpu/mod.rs:55-59` documents this:
  > "several builder/constant items (`ShadowParams::elevation_*`, `BlurIntensity`, `LinearGradientBuilder`) are forward-looking helpers that painter.rs has not yet wired into a public API; deletion would be premature before painter.rs's internal cleanup."
- The "deletion would be premature" hedge has now been in the codebase since the 2026-05-20 audit (a previous cycle). Cycle 4 must decide.

**Why it's a problem:**
- ~150 LOC of forward-looking helpers, all gated under module-level `#[allow(dead_code)]`. The lint suppression is module-level (broad) not item-level (narrow); flipping a single field to "used" doesn't help the audit understand which items are still zombie.
- The two parallel `BlurParams` (one in effects.rs, one in offscreen.rs) is a clearer signal — the effects.rs version is the "future" one that the painter hasn't adopted, while offscreen.rs uses its own.

**Fix shape:** Delete the unused items. Specifically:
- Delete `effects.rs::BlurParams` (the offscreen.rs one is the live version).
- Delete `BlurIntensity` enum.
- Delete `LinearGradientBuilder`.
- Delete `ShadowParams::elevation_*` constructors.
- Remove module-level `#[allow(dead_code)]` from `effects.rs` — any remaining items either are used or get flagged.

**Blast radius:** effects.rs only + module-level `#[allow]` removal. ~150 LOC reduction.

---

#### E-5 [P1 DEAD-CODE | HIGH] Instancing forward-looking helpers — 7 methods on `RectInstance`/`CircleInstance`/`ArcInstance`/`TextureInstance` zero production use

**Evidence:**
- `crates/flui-engine/src/wgpu/instancing.rs`:
  - `RectInstance::rounded_rect` (line 91), `RectInstance::with_clip_rsuperellipse` (line 157), `RectInstance::with_transform` (line 189): 0 callsites (painter.rs uses `RectInstance::rect` and `RectInstance::rounded_rect_corners`).
  - `CircleInstance::ellipse` (line 255): 0 callsites.
  - `ArcInstance::ellipse` (line 338): 0 callsites.
  - `TextureInstance::with_rotation` (line 455), `TextureInstance::with_uv` (line 430): 0 callsites.
- Module-level `#[allow(dead_code)]` at `wgpu/mod.rs:72`.
- The 2026-05-20 audit flagged these and recommended a `// REMOVE_BY:` cadence comment. None added.

**Why it's a problem:**
- ~80 LOC of zombie helpers. Module-level lint suppression continues to mask the rot.
- The 2026-05-20 audit's recommendation hasn't been followed-up.

**Fix shape:** Delete the 7 specific methods. Bundle with E-4.

**Blast radius:** instancing.rs only. ~80 LOC reduction.

---

#### E-6 [P1 DEAD-CODE | HIGH] `wgpu/pipeline.rs` `PipelineKey` constructors zero production use; `pipelines.rs` is what painter uses

**Evidence:**
- `crates/flui-engine/src/wgpu/pipeline.rs` is module-level `#[allow(dead_code)]` per `wgpu/mod.rs:85`.
- `PipelineKey::from_color` + related constructors: 0 callsites (painter.rs uses `pipelines.rs::PipelineCache` API, not `pipeline.rs::PipelineKey`).
- The two adjacent files (`pipeline.rs` vs `pipelines.rs`) carry overlapping concerns. The `pipeline.rs` one is the zombie.

**Why it's a problem:**
- Same as E-5. The 2026-05-20 audit flagged exactly this confusion ("`pipeline.rs` (PipelineKey bitflags) vs `pipelines.rs` (PipelineCache, actually used) — two adjacent files with overlapping abstraction").
- Auditor confusion + dev-onboarding friction.

**Fix shape:** Delete `pipeline.rs` (the bitflags PipelineKey) OR migrate painter to use it. Audit recommends the former — `pipelines.rs::PipelineCache` is the working impl.

**Blast radius:** `pipeline.rs` deletion + 1 `mod.rs` line + verify no painter.rs reference (already 0). ~200 LOC reduction.

---

#### E-7 [P1 DEAD-CODE | HIGH] `ShaderCache::cached_count` / `clear` introspection — 0 production use, devtools-only

**Evidence:**
- `crates/flui-engine/src/wgpu/shader_compiler.rs`: module-level `#[allow(dead_code)]` per `wgpu/mod.rs:93`.
- `ShaderCache::cached_count` and `clear`: 0 callsites. Doc-comment says "forward-looking devtools helpers".

**Why it's a problem:** Same as E-5/E-6. Module-level lint suppression masks zombie.

**Fix shape:** Move both methods to `#[cfg(feature = "devtools")]`. Or delete and reintroduce when devtools materializes.

**Blast radius:** shader_compiler.rs only. ~30 LOC.

---

#### E-8 [P1 ARCH-SMELL | HIGH] `OffscreenRenderer` wrapped in `Arc<parking_lot::Mutex<>>` on single-thread render path

**Evidence:**
- `crates/flui-engine/src/wgpu/renderer.rs:147`:
  ```rust
  offscreen: Option<Arc<parking_lot::Mutex<super::offscreen::OffscreenRenderer>>>,
  ```
- The 2026-05-20 audit flagged this. Engine is single-threaded; `Arc<Mutex<>>` pays atomic overhead + lock acquisition for no concurrency benefit.
- Lock-elision optimizations might help; but the structural choice of `Arc<Mutex<>>` signals concurrent access that doesn't exist.

**Why it's a problem:**
- `OffscreenRenderer` is consumed only by the renderer's render loop. Single-thread. No need for `Mutex`.
- The atomic refcount on `Arc` is non-zero overhead per clone.
- The `parking_lot::Mutex` is faster than `std::sync::Mutex` but still pays the atomic CAS on each lock acquire.

**Fix shape:** Replace with `OffscreenRenderer` owned directly (no Arc, no Mutex). Or `Option<OffscreenRenderer>` if it's optionally present.

**Blast radius:** renderer.rs ownership rotation + backend.rs callsites. ~20 LOC.

---

#### E-9 [P1 API-SURFACE | HIGH] `CommandRenderer` trait has 50+ methods; many are framework-internal not backend-swap concerns

**Evidence:**
- `crates/flui-engine/src/traits.rs` defines `CommandRenderer` with 50+ methods grouped into Shapes / Text / Images / Effects / Gradients / Backdrop / Vertices / Clipping / Viewport / SaveLayer / **LayerTree clip stack (push_/pop_)** / **Effect stack (push_/pop_opacity, color_filter, image_filter)** / Performance Overlay.
- The push_/pop_ clip-stack and effect-stack methods are framework-internal (`flui_engine::wgpu::Backend` is the sole impl). External backend swap (Skia/Vello/software) shouldn't need to implement these — they're a flui-layer's clip-stack hand-off mechanism.

**Why it's a problem:**
- 50+ methods on a public trait. New backend impls have to provide all 50+ to be valid.
- The split between "render this command" (the visitor methods) and "framework state" (push/pop clip + effect stacks) is conceptually mixed. Cycle-2 PR #100/U21 had a similar trait-split intuition for SemanticsConfiguration.

**Fix shape:** Two-trait split:
```rust
pub trait CommandRenderer {
    // 30 render_* visitor methods only.
    fn render_rect(&mut self, ...);
    // ...
}

pub trait LayerStateStack {
    fn push_clip_rect(&mut self, ...);
    fn pop_clip(&mut self);
    fn push_opacity(&mut self, alpha: f32);
    // ...etc.
}

impl CommandRenderer for Backend { /* ... */ }
impl LayerStateStack for Backend { /* ... */ }
```
Backends that only emit commands (e.g. a debug recorder, a software fallback) implement `CommandRenderer`. Backends that participate in flui-layer's clip-stack handshake implement both. Compositors (the layer-route) need both.

**Blast radius:** traits.rs split + Backend re-impl + MockRenderer in tests + DebugBackend.

---

#### E-10 [P2 API-SURFACE | MEDIUM] `wgpu/mod.rs` re-exports private-or-internal-use types in public surface

**Evidence:**
- `crates/flui-engine/src/wgpu/mod.rs:118` exports `AtlasEntry`, `AtlasRect`, `TextureAtlas`.
- `wgpu/mod.rs:132` exports `ExternalTextureEntry`, `ExternalTextureRegistry`.
- `wgpu/mod.rs:137` exports `DrawCommand`, `DrawIndexedIndirectArgs`, `MultiDrawBatcher`, `MultiDrawStats`, `PipelineId`.
- `wgpu/mod.rs:140` exports `MaskedRenderResult`, `OffscreenRenderer`, `PipelineManager`.
- `wgpu/mod.rs:143` exports `PipelineBuilder`, `PipelineCache`.
- All these are consumed only inside flui-engine. flui-app imports only `Renderer`.

**Why it's a problem:**
- 15+ types in the public surface that no external crate consumes. Compile-time cost (rustc has to monomorphize the re-exports), API-stability cost (every type is a public commitment), discoverability cost (consumers see a wall of types).

**Fix shape:** Demote to `pub(crate)`. Audit which re-exports flui-app actually uses (only `Renderer`); demote the rest.

**Blast radius:** mod.rs re-export visibility changes. ~30 LOC.

---

#### E-11 [P2 DEAD-CODE | MEDIUM] `wgpu/multi_draw.rs::DrawCommand` is a different `DrawCommand` than `flui-painting::DrawCommand`

**Evidence:**
- `crates/flui-engine/src/wgpu/multi_draw.rs:119`: `pub struct DrawCommand { ... }`.
- `crates/flui-painting/src/display_list/command.rs`: `pub enum DrawCommand { Rect(...), Path(...), Circle(...), ... }` — the canonical DisplayList variant enum.
- Same name in two crates, same workspace, different shapes. Name collision through `flui_painting::DrawCommand` (the 29-variant enum) and `flui_engine::wgpu::multi_draw::DrawCommand` (the indirect-args struct).
- `pub use multi_draw::{DrawCommand, ...}` at `wgpu/mod.rs:137` exposes the engine variant publicly.

**Why it's a problem:**
- Two `DrawCommand`s in the workspace. Anyone importing both crates' preludes hits a collision.
- The engine version is structurally a `DrawIndirectArgs` wrapper — the name `DrawCommand` overloads the painting DisplayList's variant enum.

**Fix shape:** Rename `wgpu/multi_draw.rs::DrawCommand` to `DrawIndirect` or `MultiDrawIndirect`. flui-app doesn't consume the engine version externally so the rename is internal.

**Blast radius:** multi_draw.rs + mod.rs re-export + 1 painter.rs callsite. ~10 LOC.

---

#### E-12 [P2 HALF-IMPL | MEDIUM] `WgpuPainter::draw_vertices` ignores `tex_coords` parameter

**Evidence:**
- `crates/flui-engine/src/wgpu/painter.rs:3008-3012`:
  ```rust
  pub fn draw_vertices(
      &mut self,
      vertices: &[Point<Pixels>],
      colors: Option<&[Color]>,
      tex_coords: Option<&[Point<Pixels>]>, // TODO: Full texture coordinate support
      indices: &[u16],
      paint: &Paint,
  )
  ```
- `tex_coords` parameter present but unused — the TODO at line 3012 admits it.

**Why it's a problem:** Silent feature gap. Caller passes `tex_coords` expecting textured vertices; gets non-textured output.

**Fix shape:** Either implement the textured path OR remove the parameter (breaking change but honest).

**Blast radius:** painter.rs draw_vertices + traits.rs trait method + Backend forwarder.

---

#### E-13 [P2 PERF | MEDIUM] `Backend::with_transform` closure pattern materializes a temp on every shape

**Evidence:**
- `crates/flui-engine/src/wgpu/backend.rs:495-510` (gradient handling), 836-848 (vertices), 798-803 (color), and others use `painter.with_transform(transform, |painter| { ... })` repeatedly.
- Each call pushes a transform, runs the closure, pops.
- For a tight loop drawing 1000 shapes, that's 1000 push/pops + 1000 closure setups.

**Why it's a problem:**
- The Backend layer's "wrap painter calls in transform" pattern is per-shape. Flutter's equivalent (`Canvas.drawRect(rect)` with a baked-in current transform) doesn't pay this cost.
- The transform stack lives on `WgpuPainter` (`transform_stack: Vec<glam::Mat4>` + `current_transform`); each push is a mat-mult.

**Fix shape:** Batch shapes that share a transform. Or push the transform once, draw N shapes, pop once. The structural change is: Backend tracks the current command's transform and only pushes when it changes.

**Blast radius:** backend.rs internal restructure. Performance-impact-only; no API change.

---

#### E-14 [P2 ARCH-SMELL | MEDIUM] `Renderer::adapter` / `Renderer::instance` `#[allow(dead_code)]` keep-alive — clean but the pattern is documented twice

**Evidence:**
- `crates/flui-engine/src/wgpu/renderer.rs:135-140`:
  ```rust
  // `instance` and `adapter` are kept alive for the lifetime of the renderer
  // because `wgpu::Surface<'static>` and `wgpu::Device` depend on them. They
  // are not read post-init in production code; the `#[allow(dead_code)]`
  // markers document that the keep-alive shape is intentional.
  #[allow(dead_code)]
  instance: wgpu::Instance,
  #[allow(dead_code)]
  adapter: wgpu::Adapter,
  ```
- Good discipline — `#[allow(dead_code)]` with documented reason. The 2026-05-20 audit's "cadence rule" is satisfied here. Don't touch.

**Why it's NOT a problem:** Reference example of correct `#[allow(dead_code)]` usage.

**Action:** None — keep as the canonical example for the project.

---

#### E-15 [P2 HALF-IMPL | MEDIUM] `wgpu/text.rs::TODO` for additional embedded fonts

**Evidence:**
- `crates/flui-engine/src/wgpu/text.rs:140`:
  ```rust
  // TODO: Add more embedded fonts if needed (Bold, Italic, etc.)
  ```

**Why it's a problem:** Documentation TODO without removal cadence.

**Fix shape:** Add `// REMOVE_BY: 2026-09-22` or document in `flui-engine/ARCHITECTURE.md` `## Outstanding refactors`.

**Blast radius:** Trivial.

---

#### E-16 [P3 DEAD-CODE | LOW] `wgpu/multi_draw.rs::MultiDrawStats` and `DrawIndexedIndirectArgs::quad_instances` zero consumers

**Evidence:**
- `MultiDrawStats` (`multi_draw.rs:246+`) is returned by `MultiDrawBatcher::stats()` (line 215). The `stats()` method has 0 production callers.
- `DrawIndexedIndirectArgs::quad_instances` (line 99) is a `pub fn`. 0 callers.

**Why it's a problem:** Minor zombie. The MultiDrawBatcher core is used; the stats/helper around it is not.

**Fix shape:** `pub(crate)` the stats method + the quad_instances helper.

**Blast radius:** multi_draw.rs only.

---

#### E-17 [P3 HYGIENE | LOW] Engine `wgpu/painter.rs::draw_image_filtered` 200+ LOC of filtered-image rendering

(Outside the surface-LOC budget for this audit; flagged for future cycle. The file is 3961 LOC of single-purpose painter logic; auditing it line-by-line is its own deep dive.)

**Action:** Defer to a future painter-focused audit.

---

#### E-18 [P3 HYGIENE | LOW] `wgpu/effects_pipeline.rs` 7 pipeline-creation free functions — could be a builder

**Evidence:**
- `crates/flui-engine/src/wgpu/effects_pipeline.rs:12-235`: `create_gradient_bind_group_layout`, `create_gradient_stops_buffer`, `create_gradient_pipeline_layout`, `create_linear_gradient_pipeline`, `create_radial_gradient_pipeline`, `create_sweep_gradient_pipeline`, `create_shadow_pipeline`.
- All free functions taking `&wgpu::Device` + various other params. Called from `WgpuPainter::new`.

**Why it's NOT immediately a problem:** Free functions with `&wgpu::Device` parameter are fine. Could be refactored to a `EffectsPipelineBuilder` via `bon` per the 2026 quality bar but it's a P3 cosmetic.

**Fix shape:** Defer.

---

#### E-19 [P3 HYGIENE | LOW] `wgpu/backend.rs::render_text_span` unused param + `wgpu/painter.rs` parameter passes

(Skim-level observation — many wgpu/* functions take broad parameter packs because that's the wgpu API surface. The `Backend::render_text_span(span, offset, scale, transform)` matches `CommandRenderer::render_text_span`. Defer to E-9's two-trait split.)

---

## Part III — Flutter drift catalog

Each drift cites Flutter source line. Drifts are *intentional Rust-native shapes* vs *gaps to bridge*. Severity tagged.

### Drift A — `PipelineOwner<Phase>` typestate vs Flutter's `_debugDoingThis*` runtime flags

**Flutter** (`rendering/object.dart::PipelineOwner._debugDoingLayout`, `_debugDoingPaint`, `_debugDoingSemantics`): three boolean fields, asserted at runtime via `assert(!_debugDoingLayout)` at the start of each phase method.

**FLUI** (`pipeline/owner.rs::PipelineOwner<Phase>`): zero-sized phantom-typed `Phase` parameter with 5 sealed phase markers; each `run_*` lives on its phase's impl block. Compile-time enforcement.

**Reason for drift:** Rust's type system supports typestate; Dart doesn't. This is one of the project's best architectural choices.

**Severity:** Low (intentional improvement — compile-time vs runtime).

**Action:** None — keep. The `debug_doing_layout`/`debug_doing_paint`/`debug_doing_semantics` bools also exist on the owner (lines 115-121) for the cases where the type-state is bypassed (e.g. inside `run_layout`'s body, `debug_doing_layout` is set true). Belt + suspenders.

### Drift B — `RendererBinding::render_views` nested `Arc<RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>>` vs Flutter's `List<RenderView>`

**Flutter** (`rendering/binding.dart::RendererBinding._views`): a `List<RenderView>`. Single-thread access, no locks.

**FLUI** (`crates/flui-rendering/src/binding/mod.rs:145`): trait method returning the deep-nested lock structure.

**Reason for drift:** Rust has no GC + Dart's main-thread guarantee. The `RwLock` is correct; the *depth* is excessive.

**Severity:** Critical (R-6 finding).

**Action:** Per R-6 — replace lock-leaking trait method with per-operation lookup methods.

### Drift C — `HitTestResult` × 2 vs Flutter's single `gestures/hit_test.dart::HitTestResult`

**Flutter** (`gestures/hit_test.dart`): a single `HitTestResult` class in `gestures/`. flui-rendering would call into it via `flui_interaction` (the "gestures" port).

**FLUI**: two parallel types (`flui_rendering::HitTestResult` + `flui_interaction::routing::HitTestResult`).

**Severity:** Critical (R-7 finding).

**Action:** Per R-7 — pick one canonical home; the audit recommends flui-interaction.

### Drift D — `MouseTrackerAnnotation` × 2 vs Flutter's single class in `rendering/mouse_tracker.dart`

**Flutter** (`rendering/mouse_tracker.dart::MouseTrackerAnnotation`): a class with `onEnter`, `onHover`, `onExit`, `cursor` properties.

**FLUI**: two parallel types — flui-rendering's trait + flui-interaction's struct.

**Severity:** Critical (R-8 finding).

**Action:** Per R-8 — consolidate to flui-interaction's struct version (matches Flutter's data-class shape).

### Drift E — `RenderError` × 2 vs Flutter's `FlutterError` hierarchy

**Flutter** (`foundation/assertions.dart::FlutterError`): single `FlutterError` exception hierarchy. Different errors are subclasses.

**FLUI**: two `RenderError` enums (flui-rendering + flui-engine), both `pub enum RenderError`.

**Severity:** Critical (R-10 finding).

**Action:** Per R-10 — rename `flui_engine::RenderError` → `EngineError`.

### Drift F — `ParentData` × 2 vs Flutter's single `rendering/object.dart::ParentData`

**Flutter** (`rendering/object.dart::ParentData`): single abstract `ParentData` class. `RenderObject` carries `parentData`. ParentDataWidget configures it.

**FLUI**: two `ParentData` traits — flui-rendering's storage-trait + flui-view's marker-trait.

**Severity:** High (R-11 finding).

**Action:** Per R-11 — rename the view-side to `ParentDataConfig`.

### Drift G — `MouseTracker` × 2 vs Flutter's single class

**Flutter** (`rendering/mouse_tracker.dart::MouseTracker`): single `MouseTracker` class.

**FLUI**: two `MouseTracker` structs — flui-rendering + flui-interaction.

**Severity:** Critical (R-9 finding).

**Action:** Per R-9 — consolidate to flui-interaction.

### Drift H — `run_semantics`/`perform_semantics_action`/`SemanticsBuilder::new` `unimplemented!()` vs Flutter's no-op-when-disabled

**Flutter**: When semantics are disabled, the semantics methods are no-ops, not exception throws.

**FLUI**: Three `unimplemented!()` macros in production paths.

**Severity:** Critical (R-1, R-2, R-3 findings).

**Action:** Per R-1/R-2/R-3 — replace `unimplemented!()` with `tracing::warn!` + `Ok(())` returns until SemanticsOwner integration lands.

### Drift I — `run_compositing` silent stub vs Flutter's `flushCompositingBits` walk

**Flutter** (`rendering/object.dart::PipelineOwner.flushCompositingBits`): walks dirty nodes and recursively updates `_updateSubtreeCompositingBits`.

**FLUI** (`crates/flui-rendering/src/pipeline/owner.rs:918-949`): for-loop that does nothing except tracing-trace and clear the list.

**Severity:** Critical (R-4 finding).

**Action:** Per R-4 — implement structural compositing-bits walk.

### Drift J — `WgpuPainter::clip_path` silent no-op vs Flutter's `Canvas.clipPath` real stencil-buffer clip

**Flutter** (`dart:ui::Canvas.clipPath`): real path clipping via stencil buffer.

**FLUI**: silent no-op with `tracing::trace!` warning (E-1).

**Severity:** Critical (E-1 finding).

**Action:** Per E-1 — delete the method OR implement real stencil-buffer clip.

### Drift K — `Backend::render_backdrop_filter` fallback-no-filter vs Flutter's BackdropFilter widget real impl

**Flutter** (`rendering/proxy_box.dart::RenderBackdropFilter::paint`): captures the parent's rendering as a texture, applies the filter, composites.

**FLUI** (`crates/flui-engine/src/wgpu/backend.rs:805-834`): falls back to rendering child without filter, log-warns once per frame.

**Severity:** Critical (E-2 finding).

**Action:** Per E-2 — rotate `OffscreenRenderer` ownership to Painter to enable real backdrop filter.

### Drift L — `RenderTree::set_owner` silent no-op vs Flutter's `RenderObject.attach(PipelineOwner)` recursive walk

**Flutter** (`rendering/object.dart::RenderObject.attach`): recursive descent, propagates owner to all descendants, registers dirty work.

**FLUI** (`crates/flui-rendering/src/storage/tree.rs:114-119`): silently stores the owner pointer; doesn't walk the tree.

**Severity:** High (R-12 finding).

**Action:** Per R-12 — either implement the walk OR honest-doc the limitation.

### Drift M — `paint_node_recursive` no-depth-sort + flag-clear-loop vs Flutter's `flushPaint` sorted walk

**Flutter**: `flushPaint` sorts deep-first and paints each node; flag-clearing is folded into the paint call.

**FLUI** (`crates/flui-rendering/src/pipeline/owner.rs:983-994`): clear-flags loop separate from descent-from-root paint walk. Unreachable nodes lose their dirty flag without being painted.

**Severity:** High (R-15 finding).

**Action:** Per R-15 — sort deep-first + fold flag-clear into `paint_node_recursive`.

### Drift N — `RenderError` 5 `String` fields vs Flutter's `FlutterError.message: String` (Dart String is reference-counted)

**Flutter**: Dart's `String` is reference-counted; no allocation per construct.

**FLUI**: `String` payloads allocate per-construct.

**Severity:** Medium (R-17 finding).

**Action:** Per R-17 — replace `String` with `Box<str>` per cycle-3 PR #106 pattern.

---

## Part IV — Final combined priority order

Severity legend: P0 = critical correctness / Constitution-violation / cycle-X-parity-essential; P1 = high-impact API or hot-path; P2 = medium-impact hygiene; P3 = low-priority cleanup.

| # | Crate | Finding | Severity | Size (LOC) | Depends on | Notes |
|---|---|---|---|---|---|---|
| **P0 — Critical correctness (must land first; Constitution-violation + cycle-2/3 parity)** | | | | | | |
| 1 | flui-rendering | R-1: `run_semantics` `unimplemented!()` → catch_unwind + tracing-warn + Ok(()) return | P0 | ±15 | None | Constitution Principle 6 violation; explicit production-path panic. **Most important** |
| 2 | flui-rendering | R-2: `RendererBinding::perform_semantics_action` `unimplemented!()` → tracing-warn return | P0 | ±8 | None | Constitution Principle 6 violation; default trait method |
| 3 | flui-rendering | R-3: Delete `SemanticsBuilder` type (panics on construction; 0 consumers) | P0 | −30 | None | Constitution Principle 6 violation; remove from prelude |
| 4 | flui-rendering | R-4: `run_compositing` silent stub → structural compositing-bits walk | P0 | ±30 | None | Half-impl that's worse than panic (silent) |
| 5 | flui-rendering | R-5: Delete `RenderDirtyPropagation` trait body (uses wrong-type ElementId; 0 consumers) | P0 | −80 | None | Cycle 1 PR #93 pattern; deletion |
| 6 | flui-rendering | R-6: `RendererBinding::render_views` → per-operation lookup methods (nested-lock fix) | P0 | ±100 | None | Cycle 2 PR #100/U22 pattern at trait level |
| 7 | flui-rendering / flui-interaction | R-7: Consolidate `HitTestResult` to flui-interaction; flui-rendering re-exports | P0 | ~50 LOC moved | None | Cycle 2 PR #100/U21 newtype pattern |
| 8 | flui-rendering / flui-interaction | R-8: Consolidate `MouseTrackerAnnotation` to flui-interaction's struct version | P0 | ~50 LOC delete | R-7 | Same |
| 9 | flui-rendering / flui-interaction | R-9: Delete flui-rendering's `MouseTracker` (use flui-interaction's) | P0 | ~−300 | R-8 | Same |
| 10 | flui-rendering / flui-engine | R-10: Rename `flui_engine::RenderError` → `EngineError`; flui-rendering keeps `RenderError` | P0 | ±30 | None | Cycle 2 PR #100/U21 newtype pattern |
| 11 | flui-engine | E-1: Delete `WgpuPainter::clip_path` silent no-op (forces users through layer route) | P0 | −20 | None | Constitution Principle 6 spirit |
| 12 | flui-engine | E-2: Rotate `OffscreenRenderer` ownership to Painter; implement real backdrop filter | P0 | ±50 | None | Visible regression vs Flutter |
| 13 | flui-engine | E-3: Delete `PipelineManager` + `PipelineHandle` (50 LOC TODO zombie) | P0 | −60 | None | 2026-05-20 audit follow-through |
| 14 | flui-engine | E-4: Delete `effects.rs::{BlurParams, BlurIntensity, LinearGradientBuilder, ShadowParams::elevation_*}` (~150 LOC) | P0 | −150 | None | 2026-05-20 audit follow-through; remove module-level `#[allow(dead_code)]` |
| **P1 — High-impact (next wave)** | | | | | | |
| 15 | flui-rendering / flui-view | R-11: Rename `flui_view::ParentData` → `ParentDataConfig` | P1 | ±10 | None | Resolves trait-name collision |
| 16 | flui-rendering | R-12: Honest-doc `RenderTree::set_owner` limitation | P1 | doc ±10 | None | Or implement the walk in a follow-up |
| 17 | flui-rendering | R-13: Delete or implement `propagate_constraints_to_child` + `sync_child_size_to_parent` stubs | P1 | ±20 | Investigation | Establish current constraint-flow path |
| 18 | flui-rendering | R-14: Delete dead `RenderView` fields (4-9 fields zero-write zero-read) | P1 | ±30 | None | Trim placeholders |
| 19 | flui-rendering | R-15: `run_paint` sort deep-first + fold flag-clear into `paint_node_recursive` | P1 | ±20 | None | Flutter-parity fix |
| 20 | flui-rendering | R-16: Feature-gate delegates (`experimental-delegates`, default off) | P1 | cfg-gate ~1,800 | None | 18-month-old 2026-05-20 finding |
| 21 | flui-rendering | R-17: `RenderError` `String` → `Box<str>` for 5 message fields | P1 | ±20 | None | Cycle 3 PR #106 pattern |
| 22 | flui-rendering | R-18: Feature-gate `ScrollMetrics` + concrete impls (`scrolling`, default off) | P1 | cfg-gate ~450 | None | 2026-05-20 finding |
| 23 | flui-rendering | R-19: Delete `ScrollableViewportOffset::listener` placeholder | P1 | −50 | None | Cadence |
| 24 | flui-engine | E-5: Delete 7 instancing forward-looking helpers + remove module-level `#[allow(dead_code)]` | P1 | −80 | None | 2026-05-20 finding |
| 25 | flui-engine | E-6: Delete `wgpu/pipeline.rs` (`pipelines.rs` is the live version) | P1 | −200 | None | 2026-05-20 finding |
| 26 | flui-engine | E-7: Move `ShaderCache::cached_count`/`clear` behind `#[cfg(feature = "devtools")]` | P1 | ±30 | None | 2026-05-20 finding |
| 27 | flui-engine | E-8: Unwrap `OffscreenRenderer` from `Arc<Mutex<>>` (single-thread engine) | P1 | ±20 | E-2 | Performance + arch cleanup |
| 28 | flui-engine | E-9: Split `CommandRenderer` into render-visitor + state-stack traits | P1 | ±100 | None | 50+ method trait split |
| **P2 — Medium-impact hygiene** | | | | | | |
| 29 | flui-rendering | R-20: Mark `RendererBinding::render_views` `#[deprecated]` if R-6 doesn't ship | P2 | ±5 | None | Alternative to R-6 |
| 30 | flui-rendering | R-21: Move `RenderObject::insert_into_pipeline` to free function | P2 | ±20 | None | Trait pollution |
| 31 | flui-rendering | R-22: Delete `DummyTarget` + `HitTestResult::add_with_position` | P2 | −30 | R-7 | API discipline |
| 32 | flui-rendering | R-23: Fix `BoxHitTestResult::wrap` doc-lie OR implement shared storage | P2 | doc/code | None | Doc honesty |
| 33 | flui-rendering | R-24: `HitTestResult::current_transform` incremental composition (per-call O(N)→O(1)) | P2 | ±20 | None | Hot path |
| 34 | flui-engine | E-10: Demote internal engine re-exports to `pub(crate)` (~15 types) | P2 | ±30 | None | API surface trim |
| 35 | flui-engine | E-11: Rename `wgpu::multi_draw::DrawCommand` → `DrawIndirect` (collision with painting's `DrawCommand`) | P2 | ±10 | None | Name collision |
| 36 | flui-engine | E-12: `WgpuPainter::draw_vertices` implement or remove `tex_coords` param | P2 | ±20 | None | Half-impl |
| 37 | flui-engine | E-13: `Backend::with_transform` batch shapes with same transform | P2 | ±50 | None | Performance |
| 38 | flui-engine | E-15: Add `// REMOVE_BY: 2026-09-22` to `wgpu/text.rs:140` TODO | P2 | ±2 | None | Cadence discipline |
| **P3 — Low-priority cleanup** | | | | | | |
| 39 | flui-rendering | R-25: Audit `RenderError`'s `Clone` derive (consider removing) | P3 | ±5 | R-17 | Style |
| 40 | flui-rendering | R-26: `RenderTree::visit_depth_first_from` SmallVec + iterative loop | P3 | ±20 | None | Hot-path |
| 41 | flui-engine | E-14: None (correct pattern; reference example only) | P3 | none | none | Reference |
| 42 | flui-engine | E-16: `pub(crate)` `MultiDrawStats` + `quad_instances` | P3 | ±5 | None | API trim |
| 43 | flui-engine | E-17: Defer `painter.rs::draw_image_filtered` 200-LOC audit to future cycle | P3 | none | none | Defer |
| 44 | flui-engine | E-18: Refactor `effects_pipeline.rs` 7 free functions into `EffectsPipelineBuilder` (bon) | P3 | ±50 | None | Cosmetic |
| 45 | flui-engine | E-19: Bundle with E-9 (large param packs in CommandRenderer methods) | P3 | with E-9 | E-9 | Bundle |

**Total LOC delta** (estimated): **~−3,500 LOC deletion + cfg-gate** (across delegates, ScrollMetrics, MouseTracker dup, PipelineManager, effects helpers, instancing helpers, pipeline.rs, RenderView fields, ScrollableViewportOffset, SemanticsBuilder, DummyTarget, RenderDirtyPropagation, MultiDrawStats, ShaderCache devtools-gated) **+ ~+300 LOC** (R-1/R-2/R-3 stub-impls, R-4 structural walk, R-6 lookup methods, R-7/R-8/R-9 re-export bridges, E-2 backdrop filter wiring). Net: **~−3,200 LOC reduction** in public surface, much of which is `#[allow(dead_code)]` zombie.

**Cycle alignment with predecessors:**
- Total scope (45,365 LOC, 45 findings) > cycle 3 (23,448 LOC, 47 findings) > cycle 2 (15,571 LOC, 25 findings) > cycle 1 (12,360 LOC, 16 findings).
- Total `unimplemented!()` violations: **3** (vs cycle 1's 0 — interaction had `unimplemented!()` removed in PR #93).
- Total parallel-type pairs across the workspace boundary: **5** (HitTestResult, MouseTracker, MouseTrackerAnnotation, RenderError, ParentData).
- Total zombie LOC (verified zero external consumer): **~2,800** in flui-engine + ~1,800 in flui-rendering delegates ≈ **~4,600 LOC**. The no-quick-wins memory binding means consolidation, not just deletion.

---

## Appendix A — Investigation receipts

### A.1 — Project shape

```bash
$ wc -l crates/flui-rendering/src/**/*.rs | tail -5
... 25182 total

$ wc -l crates/flui-engine/src/**/*.rs | tail -5
... 20183 total

$ find crates/flui-rendering/src crates/flui-engine/src -name "*.rs" -type f | wc -l
112

$ find crates/flui-rendering/src crates/flui-engine/src -name "*.rs" -exec wc -l {} \; | sort -rn | head -10
3961 crates/flui-engine/src/wgpu/painter.rs
1705 crates/flui-rendering/src/pipeline/owner.rs
1536 crates/flui-engine/src/wgpu/offscreen.rs
1322 crates/flui-engine/src/wgpu/tessellator.rs
1235 crates/flui-engine/src/wgpu/backend.rs
1126 crates/flui-engine/src/wgpu/layer_render.rs
1106 crates/flui-rendering/src/storage/flags.rs
1002 crates/flui-engine/src/wgpu/renderer.rs
1000 crates/flui-engine/src/wgpu/texture_cache.rs
 866 crates/flui-rendering/src/hit_testing/result.rs
```

### A.2 — Zero-consumer module verification (workspace ripgrep)

```bash
# Delegates (R-16) — ~1,800 LOC across 6 files, zero production impls
$ rg "impl .* for .* CustomPainter|impl FlowDelegate|impl MultiChildLayoutDelegate|impl SingleChildLayoutDelegate|impl SliverGridDelegate|impl CustomClipper" crates --type rust | grep -v "/tests/\|delegates/.*\.rs:[0-9]+:\s*///\|test"
crates/flui-rendering/src/delegates/custom_clipper.rs:97:impl CustomClipper<Rect> for RectClipper
crates/flui-rendering/src/delegates/custom_clipper.rs:122:    impl CustomClipper<Rect> for InsetClipper  # test
crates/flui-rendering/src/delegates/custom_painter.rs:139:    impl CustomPainter for TestPainter  # test
crates/flui-rendering/src/delegates/flow_delegate.rs:201:    impl FlowDelegate for LinearFlowDelegate  # test
crates/flui-rendering/src/delegates/multi_child_layout_delegate.rs:144:    impl MultiChildLayoutDelegate for TestDelegate  # test
# Only RectClipper is non-test; that's a 1-impl utility.

# SemanticsBuilder (R-3) — 0 production consumers
$ rg "SemanticsBuilder" crates --type rust | grep -v "flui-rendering/src/delegates/custom_painter.rs"
# (0 hits)

# ScrollMetrics (R-18) — 0 external consumers
$ rg "FixedScrollMetrics|FixedExtentMetrics" crates --type rust | grep -v "flui-rendering/src/constraints/"
# (0 hits)

# Engine zombies (E-3, E-4, E-5, E-6, E-7)
$ rg "PipelineManager\b|PipelineHandle\b" crates --type rust | grep -v "wgpu/offscreen.rs\|wgpu/mod.rs:140"
# (0 hits)

$ rg "BlurIntensity\b|LinearGradientBuilder\b" crates --type rust | grep -v "wgpu/effects.rs\|wgpu/mod.rs"
# (0 hits)

$ rg "ShadowParams::elevation" crates --type rust | grep -v "wgpu/effects.rs\|wgpu/mod.rs"
crates/flui-engine/src/wgpu/painter.rs:3931:    ///     &ShadowParams::elevation_2(),
# (1 docstring-only; 0 runtime consumers)

$ rg "ShaderCache::cached_count|ShaderCache::clear" crates --type rust
# (0 hits outside shader_compiler.rs definition)

$ rg "RectInstance::rounded_rect\b|RectInstance::with_transform\b|RectInstance::with_clip_rsuperellipse\b" crates --type rust | grep -v "wgpu/instancing.rs"
# (0 hits)

$ rg "CircleInstance::ellipse|ArcInstance::ellipse|TextureInstance::with_rotation|TextureInstance::with_uv" crates --type rust | grep -v "wgpu/instancing.rs"
# (0 hits)

# Three unimplemented!() macros (R-1, R-2, R-3)
$ rg "unimplemented!" crates/flui-rendering/src crates/flui-engine/src --type rust
crates/flui-rendering/src/binding/mod.rs:325:            unimplemented!(
crates/flui-rendering/src/delegates/custom_painter.rs:29:        unimplemented!("SemanticsBuilder not yet implemented - semantics support incomplete");
crates/flui-rendering/src/pipeline/owner.rs:1276:            unimplemented!(

# Module-level #[allow(dead_code)] markers
$ rg "#\[allow\(dead_code\)\]" crates/flui-rendering/src crates/flui-engine/src --type rust | wc -l
26
```

### A.3 — Parallel-type drift verification

```bash
# HitTestResult (R-7) — 2 definitions
$ rg "pub struct HitTestResult|pub enum HitTestResult" crates --type rust
crates/flui-interaction/src/routing/hit_test.rs:..:pub struct HitTestResult
crates/flui-rendering/src/hit_testing/result.rs:25:pub struct HitTestResult
# 2 hits in 2 different crates.

# flui-app bridges them with TODO comment
$ rg "TODO: Convert rendering HitTestEntry" crates --type rust
crates/flui-app/src/app/binding.rs:508:        // TODO: Convert rendering HitTestEntry targets to interaction targets

# MouseTrackerAnnotation (R-8) — 2 definitions
$ rg "pub struct MouseTrackerAnnotation|pub trait MouseTrackerAnnotation" crates --type rust
crates/flui-interaction/src/mouse_tracker.rs:77:pub struct MouseTrackerAnnotation
crates/flui-rendering/src/input/mouse_tracker.rs:76:pub trait MouseTrackerAnnotation: Debug + Send + Sync

# MouseTracker (R-9) — 2 definitions
$ rg "pub struct MouseTracker\b|pub struct MouseTracker $" crates --type rust
crates/flui-interaction/src/mouse_tracker.rs:148:pub struct MouseTracker
crates/flui-rendering/src/input/mouse_tracker.rs:..:pub struct MouseTracker

# RenderError (R-10) — 2 definitions
$ rg "pub enum RenderError" crates --type rust
crates/flui-engine/src/error.rs:46:pub enum RenderError
crates/flui-rendering/src/error.rs:17:pub enum RenderError

# ParentData (R-11) — 2 definitions
$ rg "pub trait ParentData" crates --type rust
crates/flui-rendering/src/parent_data/base.rs:..:pub trait ParentData
crates/flui-view/src/view/parent_data.rs:68:pub trait ParentData: Clone + Default + Send + Sync + 'static {}
```

### A.4 — Cycle-3 PR #103 carryover verification

```bash
# RenderTree implements TreeWrite<RenderId> (post-PR #103)
$ rg "impl TreeWrite<RenderId> for RenderTree" crates --type rust
crates/flui-rendering/src/storage/tree.rs:674:impl TreeWrite<RenderId> for RenderTree

# TreeWrite::remove cascade-by-default semantics in trait
$ rg "fn remove\(" crates/flui-tree/src/traits/write.rs --type rust
crates/flui-tree/src/traits/write.rs:..:fn remove(&mut self, id: I) where Self: TreeNav<I>

# RenderTree provides remove_shallow primitive
$ rg "fn remove_shallow" crates/flui-rendering/src --type rust
crates/flui-rendering/src/storage/tree.rs:685:fn remove_shallow

# Verified: cycle-3 work landed correctly. RenderTree adopts cascade semantics
# automatically via the default impl on TreeWrite::remove.
```

### A.5 — Closed findings from 2026-05-20 audit

```bash
# Closed: commented impl RenderObject for RenderView (PR #81 U1)
$ rg "=== RenderObject Implementation \(Legacy" crates/flui-rendering/src
# (0 hits — deleted)

# Closed: IntrinsicProtocol + BaselineProtocol (PR #81 U2)
$ rg "trait IntrinsicProtocol|trait BaselineProtocol" crates/flui-rendering/src
# (0 hits — deleted)

# Closed: SUPERELLIPSE_CACHE unbounded thread_local (PR #83)
$ rg "static SUPERELLIPSE_CACHE" crates/flui-engine/src
# (0 hits — replaced with bounded SuperellipsePathCache on WgpuPainter)

# Closed: Two parallel ClipContext traits (PR #82)
$ rg "pub trait ClipContext" crates --type rust
crates/flui-painting/src/clip_context.rs:..:pub trait ClipContext
# (1 hit only — flui-rendering's was deleted, painting's is the canonical home)
```

---

## Re-baseline against 2026-05-20 audit

The pre-existing `2026-05-20-mythos-audit-render-paint-layer-engine.md` carried 13 findings across 4 crates. Status summary (this audit covers only rendering + engine):

| Prior finding | Status at HEAD `aea56399` (cycle 4 opener) | Cycle 4 verdict |
|---|---|---|
| Two ClipContext traits | **CLOSED** PR #82 | — |
| Tessellation duplication painting/engine | OUT-OF-SCOPE (painting crate, separate cycle) | — |
| Commented impl RenderObject for RenderView 190 lines | **CLOSED** PR #81 U1 | — |
| RenderDirtyPropagation trait used only by tests | **PARTIAL** PR #81 U3 (pub(crate) preserved with `PRESERVED_FOR`) | **R-5 deletes the trait body fully** (uses wrong-type ElementId; 18 months without consumer) |
| IntrinsicProtocol + BaselineProtocol sealed empty traits | **CLOSED** PR #81 U2 | — |
| SUPERELLIPSE_CACHE unbounded | **CLOSED** PR #83 | — |
| Multiple delegate traits 0 production impls | **OPEN** unchanged | **R-16 feature-gates the lot** behind `experimental-delegates` |
| Engine `#[allow(dead_code)]` forward-looking helpers | **OPEN** with module-level lint suppression + prose | **E-3/E-4/E-5/E-6/E-7 delete the verified-zero-consumer items** + remove module-level `#[allow]` |
| Numerous re-export-only mod.rs files | OPEN (verdict: leave) | Unchanged |
| RendererBinding trait nested-lock public type | **PARTIAL** (impl now exists in flui-app) | **R-6 rewrites the trait to per-operation lookup methods** |
| CLAUDE.md doc drift on disabled crates | **CLOSED** PR #81 U4 | — |
| ScrollMetrics trait "needs manual confirmation" | **OPEN** (verified 0 consumers) | **R-18 feature-gates** behind `scrolling` |
| HotReloadCapability trait — 7 production marker impls | OPEN (verdict: keep) | Unchanged |

**Two findings the 2026-05-20 audit missed:**
1. **Three `unimplemented!()` macros in production paths** (R-1, R-2, R-3) — explicit Constitution Principle 6 violations. The 2026-05-20 audit was written before the November 2025 Constitution amendment that made `unimplemented!()` part of Principle 6's prohibition list.
2. **Five parallel-type pairs across crate boundaries** (R-7, R-8, R-9, R-10, R-11) — `HitTestResult`, `MouseTrackerAnnotation`, `MouseTracker`, `RenderError`, `ParentData`. The 2026-05-20 audit reviewed each crate in isolation; cross-crate name + type collisions weren't part of the discovery surface.

---

## Status (open)

This is an audit document. No changes applied. The Priority Order in Part IV is the plan; downstream branches will land the changes in atomic-commit-per-finding shape matching PR #81/#82/#83/#84/#100/#103 precedent.

**Next branch**: `feat/render-engine-cycle4-wave1` for the P0 critical-correctness items (R-1 through R-10 + E-1 through E-4). Estimated wave size: ~15 commits, ~−1,500 / +400 LOC delta.

**Cycle 4 closing**: this is cycle 4 of the audit-execute series. The next cycle (cycle 5) will pick up flui-painting + flui-view in the same pair-audit shape.
