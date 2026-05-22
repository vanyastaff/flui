---
title: "Mythos Audit — flui-painting × flui-view"
date: 2026-05-22
status: audit-complete
audit_methodology: claude-mythos (12-phase rust audit, canvas-recorder + view-tree pass)
crates_audited:
  - flui-painting
  - flui-view
reference_sources:
  - flutter/packages/flutter/lib/src/painting/clip.dart  (ClipContext)
  - flutter/packages/flutter/lib/src/painting/binding.dart  (PaintingBinding, ImageCache, SystemFontsNotifier)
  - flutter/packages/flutter/lib/src/painting/text_painter.dart
  - flutter/packages/flutter/lib/src/widgets/framework.dart  (Widget, Element, BuildContext, BuildOwner, InheritedElement)
  - flutter/packages/flutter/lib/src/widgets/notification_listener.dart
  - flutter/packages/flutter/lib/src/widgets/binding.dart  (WidgetsBinding, WidgetsBindingObserver)
predecessor_cycles:
  - docs/research/2026-05-21-flui-interaction-scheduler-audit.md  (Cycle 1, closed in PRs #85-#98)
  - docs/research/2026-05-22-flui-layer-semantics-audit.md  (Cycle 2, closed in PR #100/#101)
  - docs/research/2026-05-22-flui-foundation-tree-audit.md  (Cycle 3, closed in PRs #102-#106)
  - docs/research/2026-05-22-flui-rendering-engine-audit.md  (Cycle 4, in flight)
predecessor_partial:
  - docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md  (closed PR #82 for ClipContext / PR #83 for SUPERELLIPSE; the painting findings re-baselined below)
authors:
  - Mythos (via Claude Opus 4.7)
---

# Mythos Audit: `flui-painting` × `flui-view`

> Deep audit across FLUI's **canvas-recorder + view-tree layer** — 32 source files (~8.3K LOC) in flui-painting, 44 source files (~13.8K LOC) in flui-view, **~22.2K LOC total**.
>
> Goal: identify zombie abstractions in the Canvas/DisplayList recorder (sugar wrappers with 0 production callers, hit-region parallel surface, broken half-impls), parallel public types shared with sibling crates (TextLayout has cosmic-text + fallback duals; PointerEvent in painting vs interaction; Color in flui-app vs flui-types), half-implemented BuildOwner machinery (`inherited_elements` registry never populated by production; key-based child reconciliation TODO), lifecycle FSM drift from Flutter (StatefulView's `init_state` runs even when clean, `attach_root_widget` `panic!("already attached")`), and Constitution Principle violations — without breaking active integration with `flui-rendering`, `flui-engine`, `flui-foundation`, `flui-tree`, `flui-interaction`, or `flui-app`.
>
> **Cycle**: this audit continues the audit-execute series that produced PRs #81 (rendering Phase 1 zombie cleanup) / #82 (ClipContext consolidation) / #83 (SUPERELLIPSE_CACHE bounding) / #84 (framework spine repair — `BuildContext` + `GlobalKey` + type-collision cleanup landed in flui-view) / #85-#98 (interaction × scheduler) / #100/#101 (layer × semantics) / #102-#106 (foundation × tree) / #108-#117 (rendering × engine, Cycle 4 in flight). The previous cycle audited the render-object spine + GPU backend (`flui-rendering` × `flui-engine`); see [`2026-05-22-flui-rendering-engine-audit.md`](2026-05-22-flui-rendering-engine-audit.md). This cycle covers the canvas-recorder + view-tree pair — the seam between widget code and the render-object machinery audited in cycle 4.

---

## Table of Contents

- [Mythos Improvement Verdict](#mythos-improvement-verdict)
- [Part I — Architecture review](#part-i--architecture-review)
- [Part II — Findings](#part-ii--findings)
  - [flui-painting findings (P-1 .. P-20)](#flui-painting-findings-p-1--p-20)
  - [flui-view findings (V-1 .. V-25)](#flui-view-findings-v-1--v-25)
- [Part III — Flutter drift catalog](#part-iii--flutter-drift-catalog)
- [Part IV — Final combined priority order](#part-iv--final-combined-priority-order)
- [Appendix A — Investigation receipts](#appendix-a--investigation-receipts)
- [Re-baseline against 2026-05-20 audit](#re-baseline-against-2026-05-20-audit)

---

## Mythos Improvement Verdict

The pair **`flui-painting` (8.3K LOC, 32 files) × `flui-view` (13.8K LOC, 44 files)** is **smaller than every previous cycle's scope** (vs cycle 4's 45.4K, cycle 3's 23.4K, cycle 2's 15.6K, cycle 1's 12.4K), but it carries **the workspace's largest concentration of view-side zombie surface** (~+45% of flui-view's source LOC has zero production consumers) and **the workspace's first BuildOwner-level half-impl that silently bypasses an O(1) optimization** (`inherited_elements` registry is populated only by tests; the production `depend_on_inherited` path walks ancestors instead). Beyond these, three load-bearing rot zones:

(a) **`BuildOwner::inherited_elements: HashMap<TypeId, ElementId>` registry is built, exposed via `register_inherited` / `unregister_inherited` / `inherited_element`, included in `Debug` output, exercised by tests, AND NEVER POPULATED BY PRODUCTION CODE.** `InheritedBehavior::on_mount` ([`element/behavior.rs:656`](../../crates/flui-view/src/element/behavior.rs)) does not call `register_inherited`. The `ElementOwner` split-borrow handle exposes no `register_inherited` method. Meanwhile, `ElementBuildContext::depend_on_inherited` ([`context/element_build_context.rs:261`](../../crates/flui-view/src/context/element_build_context.rs)) walks the ancestor chain via `walk_ancestors_for_inherited(type_id)` — `O(depth)` for every dependency lookup. The registry's docstring ([`owner/build_owner.rs:441`](../../crates/flui-view/src/owner/build_owner.rs)) explicitly says *"This allows `depend_on<T>()` to be O(1) instead of O(depth)"* — a promise the production code does not keep. **This is a worst-of-both-worlds shape**: the API surface exists (so the upgrade path is "use it"), the data structure cost is paid (HashMap grown to fit), the tests pass against it, but the actual production lookup ignores it entirely.

(b) **Three structurally significant `flui-view` features are test-only surfaces with zero production impls**: `AnimatedView` + `AnimationBehavior` ([`view/animated.rs`](../../crates/flui-view/src/view/animated.rs), [`element/behavior.rs:759-886`](../../crates/flui-view/src/element/behavior.rs)) is 296 LOC + 128 LOC = **~424 LOC of automatic-listener-subscription machinery** — only `TestAnimatedView` (test-internal) implements it; **`ParentDataView` + `ParentDataConfig`** ([`view/parent_data.rs`](../../crates/flui-view/src/view/parent_data.rs)) is **479 LOC** — only `TestFlexible` (test-internal) implements it; **`ErrorView` + `FlutterError` + `set_error_view_builder` + `ErrorViewBuilder`** ([`view/error.rs`](../../crates/flui-view/src/view/error.rs)) is **333 LOC** — never constructed by production code (no `build`-failure recovery path wires it in). Beyond those: `RootRenderView` + `RootRenderElement` ([`view/root.rs`](../../crates/flui-view/src/view/root.rs)) is **577 LOC** with `RootElement` + `RootElementImpl` providing the bootstrap path; the `WidgetsBinding::attach_root_widget` flow at [`binding.rs:596-632`](../../crates/flui-view/src/binding.rs) builds the root element directly — `RootRenderView` is structural prep for the eventual `flui-app` integration but ships today as 100% forward-looking. **Total forward-looking + zero-consumer LOC in `flui-view`**: ~424 (animated) + ~479 (parent_data) + ~333 (error) + ~577 (root) + ~152 (notification helpers: `NotificationNode`/`NotificationHandler`) ≈ **~1,965 LOC ≈ 14% of crate**. The figure climbs to **~58%** if we include the production-defined-but-test-only consumed surface (`StatefulBehavior`'s child-reconciliation hook, `InheritedBehavior`'s O(1) registry, the entire predictive-back-gesture path).

(c) **`flui-painting::tessellation` (537 LOC) + `TextPainter` (366 LOC) + `text_layout::TextLayout` (457 LOC; cosmic-text impl) are all test-only consumers in the workspace.** No production code imports `flui_painting::tessellation::tessellate_fill`/`tessellate_stroke` — the canonical tessellation pipeline lives at `flui_engine::wgpu::tessellator` ([`crates/flui-engine/src/wgpu/tessellator.rs`](../../crates/flui-engine/src/wgpu/tessellator.rs), 1,322 LOC). No production code constructs `TextPainter::new()` — the text-render path goes Canvas → `DrawCommand::DrawText` → engine. No production code constructs `TextLayout::new()` — the text-painter cache wraps it but the cache is itself test-only. **Painting carries ~1,900 LOC (23% of source) of canonical-but-unused text infrastructure** that the 2026-05-20 audit flagged at MEDIUM and which is unchanged 18 months later. Add the canvas `sugar/{batch,conditional,grid,debug,shapes,chain}` modules (~720 LOC) where every method has zero workspace callers outside their own definitions: **~2,620 LOC ≈ 31% of crate** with zero downstream consumers.

Beyond these three, the audit catalogs **45 findings** across the pair — split between **flui-painting (20 findings)** and **flui-view (25 findings)** — plus a re-baseline of the 2026-05-20 + cycle-4 audits at the bottom of this document.

**Three best things:**

1. **`ElementOwner<'_>` split-borrow handle** ([`crates/flui-view/src/owner/element_owner.rs`](../../crates/flui-view/src/owner/element_owner.rs)). Replaces Flutter's `Element._owner` mutable backreference with disjoint `&mut` references to `global_keys`, `dirty_elements`, `dirty_set`, `inactive_elements`, and a snapshot of `on_build_scheduled`. The borrow checker proves non-aliasing because each `BuildOwner` field is borrowed once. The module-level docs at lines 1-40 cite *Rust for Rustaceans* §"Lifetimes and split borrows" and explain why the simplest possible shape (plain `&'a mut` references, no HRTB) held without falling back to the `for<'a> Fn(&'a mut ElementOwner<'a>)` alternative noted in plan §I1. **This is the canonical Rust port of Flutter's mutable backreference**, landed in PR #84. Don't touch.

2. **`Element<V, A, B>` unified element with three type parameters** ([`crates/flui-view/src/element/unified.rs`](../../crates/flui-view/src/element/unified.rs)). One generic struct + four behavior impls (`StatelessBehavior`, `ProxyBehavior`, `StatefulBehavior<V>`, `RenderBehavior<V>`, `InheritedBehavior<V>`, `AnimationBehavior<V>`) replace what would have been six parallel element types. The 469-LOC `unified.rs` carries the `ElementBase` trait impl + the type-specific impl blocks (`Element<V, Variable, RenderBehavior<V>>` for `RenderObjectElement`; `Element<V, Single, StatefulBehavior<V>>` for `state()`/`set_state()`). The `Element` body is small; the composition pattern (`core: ElementCore<V, A>` + `behavior: B`) keeps both halves orthogonal. **This is the cycle 1 `behavior_commons` consolidation pattern applied at the type level.** Don't touch.

3. **`PaintingBinding::evict_if_needed` race-cap protection** ([`crates/flui-painting/src/binding.rs:212-251`](../../crates/flui-painting/src/binding.rs)). The eviction loop is capped at 1,000 iterations because the read-loop predicate (`count <= max_images && size <= max_bytes`) can be defeated by a parallel inserter on another thread keeping pace. The cap guarantees forward progress and emits a `tracing::warn!("ImageCache::evict_if_needed hit iteration cap; concurrent inserts may be outpacing eviction")` on saturation. This is the cycle 2 `Scene::fire_composition_callbacks` pattern + cycle 2 `LinkRegistry::leaders` bounded-loop pattern applied here. **Mirrors *Rust Atomics and Locks* Ch.8 "ABA / livelock avoidance" canonically.** Don't touch.

**Worst complexity tax:**

1. **`BuildOwner::inherited_elements` registry is dead infrastructure with O(depth) production fallback** (covered above as load-bearing rot zone (a)). Either remove the registry + the four methods + the `Debug` field + the test surface, OR wire `InheritedBehavior::on_mount` to call `register_inherited(TypeId::of::<V>(), element_id)` and `on_unmount` to `unregister_inherited`. Right now the worst of both worlds: API exists, cost paid, lookup ignores it. **V-3 (P0) treats this**.

2. **`VariableChildStorage::update_with_views` ignores keys** ([`element/child_storage.rs:494-515`](../../crates/flui-view/src/element/child_storage.rs)). The comment at line 496 says *"TODO: In a full implementation, this would use keys for reordering"*. The body matches old-vs-new children by **index**, not key. Every framework that ports Flutter has to land Flutter's `_updateChildren` algorithm (`framework.dart:5836-5946`) because index-based matching breaks `Reorderable`/`ListView` state preservation, breaks `GlobalKey` reparenting (the canonical move-with-state scenario), and breaks the entire `Hero` widget pattern when implemented. The `reconciliation.rs::reconcile_children` ([`tree/reconciliation.rs:51-193`](../../crates/flui-view/src/tree/reconciliation.rs)) **does** implement the keyed O(N) linear reconciliation algorithm with all five phases (match-start / match-end / build-key-map / process-middle / cleanup) — but **nothing calls it**: `VariableChildStorage::update_with_views` uses the index loop instead. The 325-LOC reconciliation module is the real impl, the 21-LOC inline loop in child_storage is what production uses. **V-4 (P0) hoists `reconcile_children` into `VariableChildStorage`**.

3. **Two `TextLayout` structs in the same crate, gated by feature flag, with `pub use` at the crate root.** [`text_layout/layout.rs:58`](../../crates/flui-painting/src/text_layout/layout.rs) defines `pub struct TextLayout` backed by cosmic-text's `Buffer`; [`text_layout/fallback.rs:23`](../../crates/flui-painting/src/text_layout/fallback.rs) defines a parallel `pub struct TextLayout` with character-count-estimated metrics. Both are `pub use`d via `text_layout::TextLayout` at [`text_layout/mod.rs:38-46`](../../crates/flui-painting/src/text_layout/mod.rs) — the active one depends on whether `text` feature is on (default ON via `default = ["text", "tessellation"]` at [`Cargo.toml:32`](../../crates/flui-painting/Cargo.toml)). Tests pass against both. Production consumers (`TextPainter::layout_cache`) embed it without naming. **This is a `Default ON` feature-gate around what the documentation calls "stub text layout for use by TextPainter when the text feature is disabled"** — but `TextPainter` itself has zero production consumers (only tests). So the fallback `TextLayout` exists to satisfy a `TextPainter` that exists to satisfy tests. **P-5 (P0) deletes the fallback**; **P-7 (P1) deletes `TextPainter` until a production consumer materializes**.

**Where dead code hides** (the verified-zero-external-consumer + verified-unreachable list):

| Module / Symbol | LOC | External consumers | Verdict |
|---|---|---|---|
| `flui-painting/src/tessellation.rs` (whole module, feature `tessellation`, default ON) | 537 | 0 production; only `tests/tessellation_integration.rs` + `examples/simple_tessellation.rs` | Drop `tessellation` from default features; engine's `wgpu/tessellator.rs` (1,322 LOC) is the canonical home — **P-2 (P0)** |
| `flui-painting/src/text_painter/{mod,measure,paint,baseline}.rs` (`TextPainter`, `TextLayoutCache`, `LayoutMetrics`, `TextBaseline`, `DEFAULT_FONT_SIZE`) | 751 | 0 production; only `tests/text_painter_unit.rs` + `tests/text_layout_pipeline.rs` + `tests/rich_text_example.rs` + README | Feature-gate `text-painter`, default off, until a production widget needs it — **P-7 (P1)** |
| `flui-painting/src/text_layout/fallback.rs` (parallel `TextLayout` impl) | 257 | 0 production; tests only | Delete; require `text` feature on for users — **P-5 (P0)** |
| `flui-painting/src/canvas/sugar/{batch,conditional,grid,debug,shapes,chain}.rs` 30+ ergonomic methods (`draw_rects`, `draw_circles`, `draw_pill`, `draw_ring`, `draw_rect_if`, `draw_if`, `draw_if_some`, `draw_unless`, `repeat_x`, `repeat_y`, `repeat_radial`, `debug_rect`, `debug_point`, `debug_axes`, `debug_grid`, `draw_rounded_rect_corners`, `draw_grid`, fluent `translated`/`scaled`/`rotated`/`clipped_*`/`also`/`when`/`when_else`) | 720 | 0 production callers workspace-wide (verified by `rg` — only painting's own README, ARCHITECTURE.md, and PERFORMANCE.md reference them) | Feature-gate `canvas-sugar`, default off — **P-9 (P1)** |
| `flui-painting/src/display_list/hit_region.rs` (`PointerEvent`, `PointerEventKind`, `HitRegion`, `HitRegionHandler`, `add_hit_region`, `DisplayList::hit_regions: Vec<HitRegion>`) | 101 | 0 production; `tests/canvas_unit.rs` only. The doc-comment at line 17 says *"The full event system is in `flui_interaction`"* — so painting carries a second `PointerEvent` for a hit-region routing that does not actually route anywhere | Delete + drop `hit_regions` from `DisplayList` — **P-6 (P0)** |
| `flui-painting/src/binding.rs::SystemFontsNotifier` (listener registry) | 50 | 0 production; tests only. `handle_system_message("fontsChange")` is the only caller of `notify_listeners`, called only by tests | Keep if `fontsChange` platform routing materializes; mark `pub(crate)` if not — **P-12 (P2)** |
| `flui-view/src/view/animated.rs` + `element/behavior.rs:759-886` (`AnimatedView`, `AnimationBehavior`, `AnimatedElement`) | 424 | 0 production impls; only `TestAnimatedView` in `view/animated.rs:206` | Feature-gate `animated-views`, default off — **V-9 (P1)** |
| `flui-view/src/view/parent_data.rs` (`ParentDataView`, `ParentDataConfig`, `ParentDataElement`, `impl_parent_data_view!` macro) | 479 | 0 production impls; only `TestFlexible` in tests | Feature-gate `parent-data-views`, default off — **V-10 (P1)** |
| `flui-view/src/view/error.rs` (`ErrorView`, `FlutterError`, `ErrorViewBuilder`, `set_error_view_builder`, `clear_error_view_builder`) | 333 | 0 production; tests only. The `BuildContext` path has no `catch_unwind` that would surface a `FlutterError` to the registered builder | Wire the builder into `Element::perform_build` panic-catch OR feature-gate `error-view`, default off — **V-13 (P1)** |
| `flui-view/src/view/root.rs` (`RootRenderView`, `RootRenderElement`) | 577 | 0 production; tests only. `WidgetsBinding::attach_root_widget` mounts the root element directly, not via `RootRenderView` | Either wire `attach_root_widget` to route through `RootRenderView`, OR feature-gate as "forward-looking" — **V-15 (P1)** |
| `flui-view/src/element/notification.rs::{NotificationNode, NotificationHandler, BoxedNotification, NotificationCallback}` (parallel-tree dispatch infrastructure) | 152 | 0 production; `ElementBase::on_notification` + `BuildContext::dispatch_notification` go through the unified `Element<V, A, B>::on_notification` route (the object-safe protocol). `NotificationNode`'s parallel-tree variant is dead | Delete `NotificationNode` + `NotificationHandler` + `BoxedNotification` + `NotificationCallback` — **V-19 (P2)** |
| `flui-view/src/owner/build_owner.rs::register_inherited + unregister_inherited + inherited_element` (the O(1) lookup registry) | ~50 + tests | 0 production. Tests in `tests/inherited_dependency.rs` + `tests/build_owner_tests.rs` are the only callers | Wire `InheritedBehavior::on_mount` to populate it, OR delete — **V-3 (P0)** |
| `flui-view/src/tree/reconciliation.rs::reconcile_children` (keyed O(N) linear reconciliation algorithm) | 325 | 0 production. `VariableChildStorage::update_with_views` does index-based loop instead | Hoist into `VariableChildStorage` — **V-4 (P0)** |
| `flui-view/src/binding.rs::SharedWidgetsBinding` type alias + `create_shared_binding` constructor (both `#[deprecated]`) | ~10 | 0 production; deprecated since 0.2.0 | Delete — **V-22 (P2)** |
| `flui-app/src/theme/colors.rs::{Color, ColorScheme}` (parallel `Color` struct) | ~150 | 0 production. `flui_types::Color` is the canonical color; `flui-app` defines its own type at `theme/colors.rs:5` with different field shape (`r,g,b,a: f32` instead of packed `u32`) | Delete + use `flui_types::Color` — **V-25 (P3)** (out of cycle-5 scope but noted) |
| **Subtotal — verified zero-external-consumer LOC** | **~3,810** | **0** | |
| **% of painting + view source LOC** | **~17.2%** | | |

(Methodology: `rg "<symbol>"` workspace-wide excluding the defining crate; details in Appendix A.2. Counts are conservative — the figure does not include downstream impls that exist only inside their own definition module.)

**Half-implemented hot paths** (beyond the registry bypass and keyless reconciliation):

- **`flui-view/src/owner/build_owner.rs::DirtyElement::depth` is `#[allow(dead_code)]`** ([`build_owner.rs:46`](../../crates/flui-view/src/owner/build_owner.rs)) — the comment claims *"Currently consumed only by inline tests; U9+ will read it during dirty-element drain dispatching. The `Ord` impl reads `self.depth` directly"*. The trait-impl `Ord::cmp` does read the field via direct access in the same impl block (`self.depth.cmp(&other.depth)` at line 55). The `#[allow(dead_code)]` on the getter is correct — but the comment about *"U9+ will read it"* has been there since PR #84 and U9 has shipped. Either delete the getter or stop promising U9+.
- **`flui-view/src/tree/reconciliation.rs::ReconcileAction` enum is `#[allow(dead_code)]`** ([`reconciliation.rs:17`](../../crates/flui-view/src/tree/reconciliation.rs)) — the comment says *"Will be used when full reconciliation is implemented"*. The 325-LOC `reconcile_children` function does NOT construct any `ReconcileAction` value; it returns `Vec<ElementId>` directly. The enum exists for the canonical "reconciliation algorithm returns intentions" shape (Update / Create / Remove / Move) — but the current impl bypasses it. Either restructure to return `Vec<ReconcileAction>` then apply, OR delete the enum.
- **`flui-painting/src/tessellation.rs::tessellate_path` has a TODO** at line 374: *"// TODO: Implement proper arc tessellation using arc_to or manual bezier curves"* — arcs render with a 4-segment quadratic approximation. The `Path::arc_to` family in `flui-types::painting::Path` is canonical; this fallback exists only because the painting tessellator predates the typed Path API. Compounds with P-2: if the whole module is removed, this TODO disappears.
- **`flui-painting/src/canvas/composition.rs::append_display_list_at_offset`** ([`composition.rs:83-96`](../../crates/flui-painting/src/canvas/composition.rs)) is `O(N)` clone + `O(N)` transform-rewrite. The comment at lines 70-77 explains *"the implementation clones the source list and rewrites every command's baked-in transform via DisplayList::apply_transform with a translation matching offset before appending"* — the alternative (a transform applied during paint) would be `O(1)`. This is the layer-replay hot path for `RepaintBoundary`, so the per-frame cost is per-cached-layer-replay. Cost is high; the layer is cached precisely because re-recording is expensive. **P-15 (P2)** addresses.
- **`flui-view/src/binding.rs::attach_root_widget` panics with `assert!`** ([`binding.rs:599-602`](../../crates/flui-view/src/binding.rs)) on double-attach. Per Constitution Principle 6 spirit (panics in production paths), the panic should be a `Result::Err(...)` return. The test at line 1320 explicitly asserts the panic shape (`#[should_panic(expected = "Root widget already attached")]`), so the contract is documented behavior. Still: idiomatic Rust returns errors. **V-23 (P2)**.

**Biggest optimization opportunity** — **wire `BuildOwner::inherited_elements` registry through `InheritedBehavior::on_mount`/`on_unmount` so `depend_on_inherited` becomes `O(1) HashMap::get` instead of `O(depth) walk_ancestors_for_inherited`** + **hoist `reconcile_children` into `VariableChildStorage::update_with_views`** + **drop the `tessellation` feature from painting's default + delete `TextPainter` + delete fallback `TextLayout`** + **delete `hit_region.rs` (PointerEvent parallel surface)** + **feature-gate the four zero-consumer view modules (`animated`, `parent_data`, `error`, `root`)**. Estimated impact: ~+10% binary-size delta from the view zombie + painting zombie deletions; `depend_on_inherited` goes from `O(depth)` to `O(1)` for inherited dependencies (the most common build-time lookup); keyed reconciliation enables `Hero`, `Reorderable`, and `GlobalKey` reparenting. The structural plan is laid out in Part IV.

**Don't touch**:

- `ElementOwner<'_>` split-borrow handle (`owner/element_owner.rs`) — gold-standard Rust port of Flutter's `Element._owner` mutable backreference. Cycle 1 pattern applied at framework level.
- `Element<V, A, B>` unified element + 5 behavior types (`element/{unified,behavior,generic,child_storage}.rs`) — canonical composition-over-inheritance shape, single `dyn` boundary at `ElementBase`. **Don't break.**
- `PaintingBinding::evict_if_needed` 1,000-iteration race-cap (`binding.rs:226-250`) — correct livelock protection with `tracing::warn!` observability.
- `SystemFontsNotifier::notify_listeners` snapshot-then-fire pattern (`binding.rs:299-307`) — preserves Mythos Step 12 "no reentrant deadlock" discipline.
- `BuildOwner::finalize_tree` `std::mem::take` snapshot-then-drain (`build_owner.rs:343`) — correct mid-iteration enqueue handling.
- `ElementBuildContext::find_root_ancestor_state` two-phase walk (`element_build_context.rs:406-486`) — documented borrow-disjoint resolution; minimal-clone shape.
- `Canvas::with_save` / `with_translate` / `with_*` closure-based scoped helpers (`canvas/scoped.rs`) — zero-overhead RAII via closure scope; *Rust for Rustaceans* §"RAII with closures" canonical.
- `DisplayList` + 29-variant `DrawCommand` enum + sealed extension trait pair (`display_list/{command,sealed,stats}.rs`) — cycle 4 confirmed parity with engine's `CommandRenderer`; the sealed trait pattern keeps the public extension surface forward-compatible.
- `GlobalKey<T>` covariant phantom + `with_current_state` triple-Option flatten (`key/global_key.rs`) — PR #84 work; *Programming Rust* 2nd ed §22 "Phantom data" canonical.
- `recursion-depth cap` in `command_ops::with_opacity_depth` / `apply_transform` (`display_list/command_ops.rs:51-56`) — bounded recursion + tracing warn; explicit rationale (~4 KB/frame × 64 = 256 KB).
- `ParentDataConfig` rename from `ParentData` (cycle 4 R-11 prefactor landed in PR #84) — collision with `flui_rendering::ParentData` resolved.

---

## Part I — Architecture review

### Where these crates sit in the workspace DAG

```
flui-painting ──► flui-foundation (BindingBase, ElementId, RenderId, impl_binding_singleton, HasInstance)
              ──► flui-types      (Color, Paint, Path, Image, Pixels, Rect, Offset, Size, Matrix4, ColorFilter, Shader, TextStyle, InlineSpan, LineMetrics, TextBox, …)
              ──► (no cyclic deps; sibling to flui-tree, flui-interaction; below flui-layer / flui-engine / flui-rendering / flui-view)

flui-view ──► flui-foundation     (ElementId, RenderId, BindingBase, ListenerCallback, ListenerId, Listenable, ViewKey, UniqueKey, ValueKey, impl_binding_singleton)
          ──► flui-tree           (Arity, Leaf, Single, Optional, Variable — re-exported from flui-tree, see element/arity.rs:25)
          ──► flui-types          (Color, Size, Offset, Axis, Pixels — small surface)
          ──► flui-rendering      (PipelineOwner, RenderNode, RenderObject, RenderViewAdapter, RenderView, ViewConfiguration, Protocol)
          ──► flui-interaction    (no direct use in flui-view; declared via Cargo.toml but used only for re-export consistency)
          ──► flui-log            (tracing wrapper)

flui-app ──► flui-view ──► flui-rendering ──► …
         ──► flui-painting ──► flui-types
```

**Cross-crate notes**:
- **flui-painting depends only on flui-types + flui-foundation.** Architecturally the bottom of the render stack (above flui-types). The `flui_painting::TextPainter` lives here, NOT in flui-view, because painting is a leaf in the DAG and view depends on the render-object layer which depends on painting. Correct dependency direction.
- **flui-view depends on flui-rendering.** This is the bridge: `RenderView` trait surface produces concrete `RenderObject<P>` types that `RenderBehavior<V>` inserts into `PipelineOwner::render_tree`. The DAG is clean — flui-rendering does not depend on flui-view.
- **flui-painting and flui-view never communicate directly.** Their seam is the `DrawCommand` enum produced by `Canvas` and consumed by `flui-engine::wgpu::Renderer::render_scene` via `Layer::Picture(Box<PictureLayer>)`. Architecturally clean.
- **`Color` defined in `flui_app::theme::colors`** ([`crates/flui-app/src/theme/colors.rs:5`](../../crates/flui-app/src/theme/colors.rs)) and `Color` defined in `flui_types::styling::color` ([`crates/flui-types/src/styling/color.rs:8`](../../crates/flui-types/src/styling/color.rs)) — **two parallel structs with different field shapes**. flui-app's version is `f32` per channel; flui-types' is canonical. flui-app's has zero in-workspace consumers (verified). Drift catalog entry K below + V-25 (P3) finding.

**Public surface used externally** (verified at HEAD `eb95c2f2`, the cycle-5 opener):

| Symbol | Producer crate | Consumer crates |
|---|---|---|
| `Canvas`, `DisplayList`, `DrawCommand`, `DisplayListCore`, `DisplayListExt`, `Picture` (= DisplayList alias), `DisplayListStats` | flui-painting | flui-rendering (paint phase), flui-engine (Renderer consumes DisplayList), flui-layer (PictureLayer wraps DisplayList) |
| `Paint`, `PaintStyle`, `PaintBuilder`, `BlendMode`, `Clip`, `ClipOp`, `FilterQuality`, `PointMode`, `Shader`, `TextureId`, `ColorFilter`, `ImageFilter`, `ImageRepeat`, `StrokeCap`, `StrokeJoin` (re-exports from flui-types) | flui-painting (re-export) | flui-engine, flui-rendering, flui-layer (paint operations) |
| `ClipContext` (trait) | flui-painting | flui-rendering (`CanvasContext` impl in `crates/flui-rendering/src/context/canvas.rs`) |
| `PaintingBinding`, `ImageCache`, `CachedImage`, `ImageHandle`, `image_cache()` (function), `SystemFontsNotifier` | flui-painting | flui-app (`crates/flui-app/src/bindings/renderer_binding.rs:43`) |
| `HitRegion`, `HitRegionHandler`, `PointerEvent` (painting-side), `PointerEventKind` | flui-painting | none (test-only consumers; see P-6 below) |
| `tessellate_fill`, `tessellate_stroke`, `TessellatedPath`, `TessellationVertex`, `TessellationOptions` | flui-painting (feature `tessellation`, default ON) | none (test + example only; engine has its own tessellator) |
| `TextPainter`, `TextLayout`, `TextLayoutResult`, `LineInfo`, `TextBaseline`, `DEFAULT_FONT_SIZE`, `detect_text_direction`, `measure_text`, `measure_inline_span` | flui-painting (feature `text`, default ON) | none (test-only) |
| `View` (trait), `ElementBase` (trait), `IntoView`, `IntoElement`, `BoxedView`, `BoxedElement`, `ViewExt`, `ElementExt` | flui-view | flui-app, downstream consumers |
| `Element<V, A, B>` + 5 type aliases (`StatelessElement<V>`, `StatefulElement<V>`, `ProxyElement<V>`, `InheritedElement<V>`, `RenderElement<V>`, `AnimatedElement<V>`) | flui-view | flui-app (root mount); downstream widget impls |
| `StatelessView`, `StatefulView`, `ProxyView`, `InheritedView`, `RenderView`, `ParentDataView`, `AnimatedView`, `ViewState` | flui-view | downstream widget impls (none yet outside `flui-widgets`, which is disabled) |
| `BuildContext`, `BuildContextExt`, `ElementBuildContext`, `ElementBuildContextBuilder` | flui-view | downstream widget impls |
| `BuildOwner`, `ElementOwner`, `ElementTree`, `ElementNode`, `reconcile_children` | flui-view | flui-app (`crates/flui-app/src/bindings/renderer_binding.rs` integration), test harness |
| `WidgetsBinding`, `WidgetsBindingObserver`, `RouteInformation`, `AppExitResponse`, `PredictiveBackEvent`, `ViewFocusEvent`, `AppLifecycleState`, `ViewFocusState`, `ViewFocusDirection` | flui-view | flui-app (binding integration) |
| `GlobalKey<T>`, `GlobalKeyId`, `ObjectKey`, `ValueKey` (re-exported from flui-foundation) | flui-view | downstream consumers |
| `Notification`, `NotifiableElement<N>`, `NotificationCallback<T>`, `NotificationNode`, `NotificationHandler`, `BoxedNotification`, `LayoutChangedNotification`, `SizeChangedNotification`, `ScrollNotification`, `DragStartNotification`, `DragEndNotification`, `FocusNotification`, `KeepAliveNotification` | flui-view | downstream widget impls |
| `Lifecycle`, `ElementArity`, `Leaf` / `Single` / `Optional` / `Variable` (re-exported from flui-tree) | flui-view | downstream widget impls |
| `RootElement`, `RootElementImpl`, `RootRenderElement`, `RootRenderView` | flui-view | none (forward-looking; `WidgetsBinding::attach_root_widget` mounts directly) |
| `ErrorView`, `FlutterError`, `ErrorViewBuilder`, `set_error_view_builder`, `clear_error_view_builder` | flui-view | none (no build-failure recovery wired) |
| `ParentDataConfig`, `ParentDataView`, `ParentDataElement`, `impl_parent_data_view!` (macro) | flui-view | none (only `TestFlexible` test consumer) |
| `AnimatedView`, `AnimationBehavior`, `AnimatedElement<V>` | flui-view | none (only `TestAnimatedView` test consumer) |
| `test_only_set_global_key_registry`, `test_only_clear_global_key_registry` | flui-view | test harness only |

**Verified-zero-external-consumer surfaces** (re-exported but no consumer):

| Symbol | Producer crate |
|---|---|
| `flui-painting::tessellation` module (whole) | flui-painting |
| `flui-painting::text_painter` module (`TextPainter`, `TextLayoutCache`, `LayoutMetrics`, `TextBaseline`) | flui-painting |
| `flui-painting::text_layout::fallback::TextLayout` (parallel impl) | flui-painting |
| `flui-painting::canvas::sugar` module (whole — 30+ helper methods) | flui-painting |
| `flui-painting::display_list::hit_region` module (`PointerEvent`, `HitRegion`, `HitRegionHandler`, `PointerEventKind`) | flui-painting |
| `flui-view::view::animated` (whole — `AnimatedView`) | flui-view |
| `flui-view::view::parent_data` (whole — `ParentDataView`, `ParentDataConfig`, `ParentDataElement`) | flui-view |
| `flui-view::view::error` (whole — `ErrorView`, `FlutterError`, `ErrorViewBuilder`) | flui-view |
| `flui-view::view::root::{RootRenderView, RootRenderElement}` | flui-view |
| `flui-view::element::notification::{NotificationNode, NotificationHandler, BoxedNotification, NotificationCallback}` | flui-view |
| `flui-view::owner::build_owner::{register_inherited, unregister_inherited, inherited_element}` (the O(1) registry) | flui-view |
| `flui-view::tree::reconciliation::{reconcile_children, ReconcileAction}` (keyed algorithm) | flui-view |
| `flui-view::binding::{SharedWidgetsBinding, create_shared_binding}` (both `#[deprecated]`) | flui-view |
| `flui-app::theme::colors::{Color, ColorScheme}` (parallel `Color` struct) | flui-app |

### The three-tree architecture and the view-tree's role

Per the constitutional three-tree architecture (View / Element / Render), flui-view owns:

1. **View tree** (immutable, `Box<dyn View>` short-lived per build cycle). Encodes the declarative configuration produced by user `build()` methods. Six trait families: `StatelessView`, `StatefulView`, `ProxyView`, `InheritedView`, `RenderView`, `ParentDataView` (+ `AnimatedView` as a `StatefulView` subtype).
2. **Element tree** (mutable retained tree, Slab-stored at [`crates/flui-view/src/tree/element_tree.rs`](../../crates/flui-view/src/tree/element_tree.rs)). Each `ElementNode` carries the `Box<dyn ElementBase>` plus tree-position metadata (`parent`, `depth`, `slot`, `registered_global_key_hash`). The unified `Element<V, A, B>` carries `ElementCore<V, A>` (lifecycle, children, dirty bit) + `B: ElementBehavior<V, A>` (per-view-type build logic). 5 behavior impls cover the six view-trait families (Animation composes Stateful).
3. **Render-tree integration** via `RenderBehavior<V>::on_mount` (`element/behavior.rs:491-524`). The element creates a `V::RenderObject` (concrete type), inserts it into `PipelineOwner::render_tree` via `owner.insert(Box::new(render_object))`, and attaches the parent-child relationship in the render tree. The View's `update_render_object(&mut V::RenderObject)` reconciles render-object state on element update.

The architectural seam between the three trees is at [`element/behavior.rs:486+`](../../crates/flui-view/src/element/behavior.rs) where `RenderBehavior<V>::on_mount` reads `core.pipeline_owner()` and `core.parent_render_id()` to splice in. The seam is clean: flui-view doesn't reach into the render-tree machinery; it only operates through the trait surface exposed by flui-rendering.

### The painting architecture and the canvas-recorder's role

flui-painting wraps four concentric concerns in one crate:

```
Canvas (canvas/{mod,drawing,clipping,transform,state,composition,scoped,sugar/*}.rs)
    │   3,305 LOC pre-split, now 2,054 LOC across 11 files
    │
    ▼ records
DisplayList (display_list/{mod,command,command_ops,sealed,stats,hit_region}.rs)
    │   2,434 LOC pre-split, now 1,975 LOC across 6 files
    │
    ▼ consumed by
flui-engine::wgpu::Renderer (cycle 4 dependency)
    │
    ▼ ...
GPU pipeline

(orthogonal concerns living in flui-painting):

PaintingBinding (binding.rs)             — ImageCache + SystemFontsNotifier singleton
ClipContext (clip_context.rs)            — base trait for canvas clip operations
TextPainter (text_painter/*)             — text-layout state machine (0 production consumers)
TextLayout (text_layout/{layout,fallback,measure,detect}.rs) — cosmic-text + fallback
tessellation (tessellation.rs)           — Lyon-based path → vertex buffer (0 production consumers; duplicate of flui-engine's)
```

**Architectural smells**:
- `TextPainter` owns a `Option<TextLayoutCache>` carrying both a `TextLayout` (cosmic-text buffer) and pre-computed layout metrics. The cache is read by `paint`/`width`/`height`/`get_offset_for_caret`/`get_position_for_offset`. **But no production code constructs a `TextPainter`**; the text-render path is direct `Canvas::draw_text` → `DrawCommand::DrawText { text, style, paint, transform }` → engine. The `TextPainter` state machine exists in case future widgets need to compute glyph metrics before recording (e.g. for hit-testing tappable links inside `RichText`), but the integration doesn't exist yet. ~750 LOC of forward-looking infrastructure.
- `flui-painting::tessellation::tessellate_fill` and `flui-engine::wgpu::tessellator::Tessellator::tessellate_fill` both wrap Lyon's `FillTessellator`, but with different vertex types (`TessellationVertex { position: [f32;2] }` vs engine's `Vertex { position + color + uv }`). The painting tessellator is feature-gated `tessellation` (default ON) but no production consumer reads from it. ~537 LOC duplicate.
- `Canvas::sugar/{batch,conditional,grid,debug,shapes,chain}.rs` adds ~30 ergonomic methods (`draw_pill`, `draw_ring`, `draw_grid`, `debug_rect`, fluent `translated`/`scaled`/`clipped_*`, conditionals `draw_if`/`draw_unless`/`draw_if_some`). All have zero workspace consumers. The split was done in U4 fixup pass 2 to keep each file under ~360 LOC; the methods themselves are forward-looking ergonomics.

### Cross-cycle pattern continuity

Patterns from cycles 1-4 that should propagate into cycle 5:

| Pattern | Established by | Applies to this cycle |
|---|---|---|
| **PR #84 `ParentDataConfig` rename** (cycle 4 R-11 prefactor) | flui-view + flui-rendering | Already landed. flui-view's `ParentDataConfig` is the trait; `flui-rendering::ParentData` is the storage trait. No collision. |
| **Cycle 1's `unimplemented!()` → `Err(...)` conversion** (PR #93) | flui-interaction | No `unimplemented!()` macros in painting or view (verified via rg) — Constitution Principle 6 clean. But: `assert!` panics in `attach_root_widget` (V-23) are the spirit-of-the-principle violation |
| **Cycle 2's cascade-by-default `remove`** (PR #100 U12+U13) | flui-layer + flui-semantics | Hoisted to `TreeWrite::remove` in cycle 3 PR #103. `ElementTree::remove` / `remove_finalized` carry the cascade semantics; `BuildOwner::finalize_tree` does deepest-first cascade. **Already inherited.** |
| **Cycle 2's `Alignment` newtype consolidation** (PR #100 U21) | flui-types ↔ flui-layer | Drift here: `Color` in `flui_types::styling` vs `flui_app::theme::colors`. The flui-app version is the duplicate (V-25 P3). |
| **Cycle 2's nested-lock cleanup in SemanticsBinding** (PR #100 U22) | flui-semantics | Nested-lock smell here: `Arc<RwLock<ElementTree>>` is the active shape for `ElementBuildContext`. Currently consumers see a single RwLock; no nested-lock public exposure. Acceptable. |
| **Cycle 3's Box<str> for error variants** (PR #106 T-16) | flui-foundation + flui-tree | `FlutterError::{message,details,exception}` carries 3 `Option<String>` fields. Candidates for `Box<str>` — but the type is zero-consumer so the right move is V-13: feature-gate / delete |
| **Cycle 3's TreeWrite cascade contract + RenderTree adoption** (PR #103) | flui-tree | Element tree does not implement `TreeRead<ElementId>` / `TreeWrite<ElementId>` (verified). Should it? Memory `[[flui-tree-unified-interface-intent]]` says yes. **V-7 (P1)** treats this |
| **Cycle 1's PointerId widening** (PR #96 U9) | flui-interaction | `PointerEvent` exists TWICE in workspace (painting `display_list::hit_region::PointerEvent` + interaction `pointer::PointerEvent`). Painting's is the parallel surface. **P-6 (P0)** deletes |
| **Cycle 4's `RenderError` rename to `EngineError`** (R-10 in cycle 4) | flui-engine | No `EngineError` collision here; the rendering-side error is unique. Acceptable |
| **Cycle 4's `MouseTracker` consolidation** (R-7/R-8/R-9 in cycle 4) | flui-interaction | Not applicable to painting/view |

### Re-baseline against the prior `2026-05-20-mythos-audit-render-paint-layer-engine.md`

The pre-existing audit catalogued 4 painting-specific findings. Cycle 5 status per finding:

| Prior finding | Status @ HEAD `eb95c2f2` | Notes |
|---|---|---|
| `flui_painting::ClipContext` + `flui_rendering::ClipContext` duplication | **CLOSED** (PR #82) | flui-rendering's was deleted; painting's is canonical home. `CanvasContext` impls it at `flui-rendering/src/context/canvas.rs:695` |
| Painting tessellation duplication with engine | **OPEN** | Cycle 4 audit deferred; cycle 5 finding **P-2** treats it (drop from default features) |
| Painting `tessellation.rs` feature-gated but unused in production | **OPEN** | Cycle 5 finding **P-2** treats; recommendation `default = ["text"]` (drop tessellation from default) |
| `flui_painting::ClipContext` doc-block lying about `PaintingContext` impl | **PARTIAL** | Doc updated post-PR #82 to say *"PaintingContext (flui-rendering) is not present; CanvasContext is the production implementer"* — the new lie is "PaintingContext type is not present in the current workspace" which is accurate. Acceptable |

**Two findings from the 2026-05-20 audit that this cycle EXTENDS:**

1. **Tessellation duplication** — flagged as "remove from default features" in 2026-05-20; cycle 5 extends to **delete entirely** since zero production consumers + engine has the canonical impl. The `Lyon` dependency in `crates/flui-painting/Cargo.toml:27` can be dropped, shaving the compile-time cost from the default build.
2. **`#[allow(dead_code)]` in painting** — flagged as "needs REMOVE_BY cadence". Cycle 5 finds 2 painting + 1 view item-level `#[allow(dead_code)]` markers:
   - `crates/flui-painting/src/canvas/state.rs:46` `#[allow(dead_code)] // Fields stored for future optimization features` on `ClipShape` (the rest of the enum stores `Rect`/`RRect`/`RSuperellipse`/`Path` data that's collected during clip but currently not read by anyone except for `save_count` queries).
   - `crates/flui-view/src/tree/reconciliation.rs:17` `#[allow(dead_code)] // Will be used when full reconciliation is implemented` on `ReconcileAction` enum.

`unsafe` audit in flui-painting: **ZERO `unsafe` blocks**. The lib.rs at line 151 sets `#![forbid(unsafe_code)]`. Constitution-Principle-3 clean.

`unsafe` audit in flui-view: **ZERO `unsafe` blocks** (verified via `rg "unsafe " crates/flui-view/src --type rust` — no matches). Constitution-Principle-3 clean.

---

## Part II — Findings

Findings are split between **flui-painting (P-1 .. P-20)** and **flui-view (V-1 .. V-25)**. Each follows the cycle-2/3/4 template: severity tag, evidence line refs, why-problem, Flutter ref (when applicable), proposed fix shape, blast radius.

### flui-painting findings (P-1 .. P-20)

---

#### P-1 [P0 DEAD-CODE | CRITICAL] `flui-painting::tessellation` module (537 LOC) — feature `tessellation` ON by default, zero production consumers

**Evidence:**
- `crates/flui-painting/src/tessellation.rs` — 537 LOC defining `tessellate_fill`, `tessellate_stroke`, `TessellatedPath`, `TessellationVertex`, `TessellationOptions`. Uses `lyon::lyon_tessellation::{FillTessellator, StrokeTessellator, …}`.
- `crates/flui-painting/Cargo.toml:31-32`: `[features] default = ["text", "tessellation"]`.
- Workspace-wide consumer search:
  ```bash
  $ rg "use flui_painting::tessellation|flui_painting::tessellate" crates --type rust
  crates/flui-painting/tests/tessellation_integration.rs:5:    use flui_painting::tessellation::{TessellationOptions, tessellate_fill, tessellate_stroke};
  crates/flui-painting/src/tessellation.rs:22://! use flui_painting::tessellation::{tessellate_fill, tessellate_stroke, TessellationOptions};
  # plus rustdoc lines inside tessellation.rs itself
  ```
- `crates/flui-engine/src/wgpu/tessellator.rs` (1,322 LOC) is the canonical Lyon tessellator used by `WgpuPainter`; vertex format is `{position + color + uv}` (the GPU pipeline shape), not painting's `{position}`-only `TessellationVertex`.

**Why it's a problem:**
- 537 LOC of public API surface, ON by default, **zero production consumers**.
- The 2026-05-20 audit flagged this 18 months ago as "move behind opt-in feature flag + remove from default, or delete and drop lyon from `flui-painting` Cargo.toml". No movement since.
- Pulls `lyon` 1.0 into `flui-painting`'s default build, increasing compile time on a crate that's a Foundation dependency of every render-stack consumer.
- Documentation lies (the `tessellation.rs` module-level doc-comment promises "pre-tessellate paths for GPU consumption" — but the engine ignores it).

**Flutter reference:** Flutter has no analogous tessellation surface — `dart:ui` `Path` is opaque; tessellation happens entirely inside Skia. The Rust port's design intention (backend-agnostic pre-tessellation) was speculative; engine chose to own tessellation directly.

**Fix shape (canonical):** Delete the entire module + feature + Lyon dependency:
```toml
# crates/flui-painting/Cargo.toml
[dependencies]
flui-types = { path = "../flui-types" }
flui-foundation = { path = "../flui-foundation" }
thiserror = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true, optional = true }
parking_lot = { workspace = true }
cosmic-text = { version = "0.12", optional = true }
# REMOVED: lyon = { version = "1.0", optional = true }

[features]
default = ["text"]
text = ["dep:cosmic-text"]
# REMOVED: tessellation = ["dep:lyon"]
serde = ["dep:serde", "flui-types/serde"]
```
And:
1. Delete `crates/flui-painting/src/tessellation.rs`.
2. Delete `crates/flui-painting/tests/tessellation_integration.rs`.
3. Delete `crates/flui-painting/examples/simple_tessellation.rs` (if present).
4. Update `crates/flui-painting/src/lib.rs:176-177` to remove the `#[cfg(feature = "tessellation")] pub mod tessellation;` block.
5. Audit `crates/flui-painting/docs/{ARCHITECTURE,PERFORMANCE}.md` for tessellation references and trim.

**Blast radius:** ~−537 LOC + ~−200 LOC docs/examples/tests. No external API break (verified zero consumers). flui-engine unaffected (its tessellator is independent).

---

#### P-2 [P0 DEAD-CODE | CRITICAL] `flui-painting::display_list::hit_region` module (101 LOC) — parallel `PointerEvent` surface with no routing

**Evidence:**
- `crates/flui-painting/src/display_list/hit_region.rs:19-30`:
  ```rust
  #[derive(Debug, Clone)]
  pub struct PointerEvent {
      pub kind: PointerEventKind,
      pub position: Offset<Pixels>,
      pub pointer: i32,
      pub buttons: i32,
      pub time_stamp: Duration,
  }
  ```
- Doc-comment at line 17 says *"This is a minimal event type used for hit region handlers. The full event system is in `flui_interaction`"*.
- `crates/flui-painting/src/display_list/hit_region.rs:67`: `pub type HitRegionHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>;`
- `crates/flui-painting/src/display_list/hit_region.rs:74-92`: `pub struct HitRegion { bounds: Rect, handler: HitRegionHandler }`
- `crates/flui-painting/src/canvas/mod.rs:236-238`: `Canvas::add_hit_region` registers regions.
- `crates/flui-painting/src/display_list/mod.rs:75-78`: `DisplayList::hit_regions: Vec<HitRegion>` field is `#[cfg_attr(feature = "serde", serde(skip))]`.
- Workspace consumer check:
  ```bash
  $ rg "HitRegion::new|add_hit_region|HitRegionHandler" crates --type rust
  crates/flui-painting/tests/canvas_unit.rs:178: (test)
  crates/flui-painting/tests/canvas_unit.rs:186: use flui_painting::{HitRegion, HitRegionHandler};
  crates/flui-painting/tests/canvas_unit.rs:196: canvas.add_hit_region(HitRegion::new(bounds, handler));
  ```
- Zero production consumers. The interaction layer's `PointerEvent` ([`crates/flui-interaction/src/pointer/event.rs`](../../crates/flui-interaction/src/pointer/event.rs)) is the canonical hit-test event type.

**Why it's a problem:**
- **Parallel-type drift across crate boundaries** (same name `PointerEvent`, same conceptual purpose, two definitions). Cycle 4 R-7/R-8/R-9 closed exactly this pattern for `HitTestResult` / `MouseTracker` / `MouseTrackerAnnotation`. This one survives.
- The hit-region routing never connects: nothing reads `DisplayList::hit_regions` during a hit-test. The engine ignores it; `RenderPointerListener` (flui-rendering) hit-tests via `HitTestResult`, not via `Canvas`'s `add_hit_region`.
- The `HitRegionHandler` carries `Arc<dyn Fn(&PointerEvent) + Send + Sync>` — heap allocation at registration that's never invoked.
- Doc-comment honestly admits *"The full event system is in flui_interaction"* — the existence of this module is acknowledged dead code.

**Fix shape (canonical):** Delete the module entirely:
1. Delete `crates/flui-painting/src/display_list/hit_region.rs`.
2. Delete the `DisplayList::hit_regions: Vec<HitRegion>` field, `add_hit_region`, and any references in `display_list/{mod,stats}.rs`.
3. Delete `Canvas::add_hit_region` at `canvas/mod.rs:236`.
4. Update `crates/flui-painting/src/lib.rs:194-197` to remove the `HitRegion, HitRegionHandler` exports.
5. Delete the test in `crates/flui-painting/tests/canvas_unit.rs:182-200`.

**Blast radius:** ~−101 LOC source + ~−40 LOC test + ~−4 LOC mod.rs re-exports + ~−15 LOC DisplayList field/method. No external API break (verified zero consumers).

---

#### P-3 [P0 DEAD-CODE | CRITICAL] `flui-painting::text_layout::fallback::TextLayout` — parallel `TextLayout` impl under `#[cfg(not(feature = "text"))]`

**Evidence:**
- `crates/flui-painting/src/text_layout/fallback.rs:23-203`: parallel `pub struct TextLayout` with character-count-estimated metrics (`font_size * 0.5` per char).
- `crates/flui-painting/src/text_layout/layout.rs:58-72`: the canonical cosmic-text-backed `pub struct TextLayout`.
- `crates/flui-painting/src/text_layout/mod.rs:38-46`: cfg-gated `pub use`:
  ```rust
  #[cfg(feature = "text")]
  pub use detect::detect_text_direction;
  #[cfg(not(feature = "text"))]
  pub use fallback::{TextLayout, detect_text_direction, measure_inline_span, measure_text};
  #[cfg(feature = "text")]
  pub use layout::TextLayout;
  ```
- `Cargo.toml:31-33`: `default = ["text", "tessellation"]`; `text = ["dep:cosmic-text"]`. So the default build uses `layout::TextLayout` (cosmic-text); only `--no-default-features` users see `fallback::TextLayout`.
- Both are 100% consumed by tests:
  ```bash
  $ rg "use flui_painting::TextLayout" crates --type rust
  crates/flui-painting/tests/text_layout_unit.rs:225 (test)
  crates/flui-painting/tests/text_layout_unit.rs:250 (test)
  crates/flui-painting/tests/text_layout_fallback.rs:154 (test)
  crates/flui-painting/tests/text_layout_fallback.rs:179 (test)
  ```

**Why it's a problem:**
- Same-name parallel struct in the same crate, gated by feature flag. Confusing API surface — users with `default-features = false` get one shape; users with default get a different shape with cosmic-text dependencies. The fallback is "best-effort" word-segmentation and character-count-based metrics that diverge from real shaping.
- Zero production consumers of either: `TextPainter::layout_cache` is the only intra-crate consumer (test-only) — and `TextPainter` itself has zero production consumers (see P-7).
- `text` feature default ON + cosmic-text default — the fallback exists only to satisfy callers who do `--no-default-features`, but no workspace consumer disables the default.
- The 2-line lie in `mod.rs:23` describes the cfg gating but doesn't admit the fallback is dead code.

**Fix shape:**
Three options ranked by audit preference:
1. **Delete the fallback entirely**, require `text` feature ON. Drop `cfg(not(feature = "text"))` arms. flui-painting becomes "cosmic-text required" — matches the canonical Rust ecosystem expectation for a UI framework. Net: −257 LOC.
2. Keep the fallback but mark `pub(crate)` to remove from public API. The fallback's only use is internal stubbing.
3. Delete both `TextLayout` impls (this and the cosmic-text impl) until a production widget consumes them. The text-render path is `Canvas::draw_text` → `DrawCommand::DrawText { ... }` → engine, which doesn't go through `TextLayout`. Net: −714 LOC (fallback + layout + measure + detect modules).

Audit recommends Option 1 for cycle 5; option 3 is the "no quick wins" full consolidation but blocks on whether the text-layout state-machine surface is needed at all (see P-7).

**Blast radius:** Option 1: ~−257 LOC. Cargo.toml + lib.rs cleanup. No external API break.

---

#### P-4 [P0 DEAD-CODE | CRITICAL] `flui-painting::canvas::sugar` module (720 LOC, 30+ helpers) — zero production callers

**Evidence:**
- `crates/flui-painting/src/canvas/sugar/{batch,conditional,grid,debug,shapes,chain}.rs`: 6 submodules totaling 720 LOC.
- Each file's content:
  - `batch.rs` (58 LOC): `draw_rects`, `draw_circles`, `draw_lines`, `draw_rrects`, `draw_paths` — loops over primary methods.
  - `conditional.rs` (66 LOC): `draw_rect_if`, `draw_circle_if`, `draw_if`, `draw_unless`, `draw_if_some`.
  - `grid.rs` (71 LOC): `draw_grid`, `repeat_x`, `repeat_y`, `repeat_radial`.
  - `debug.rs` (96 LOC): `debug_rect`, `debug_point`, `debug_axes`, `debug_grid`.
  - `shapes.rs` (76 LOC): `draw_rounded_rect`, `draw_rounded_rect_corners`, `draw_pill`, `draw_ring`.
  - `chain.rs` (338 LOC): ~30 fluent wrappers returning `&mut Self` + closure combinators (`also`, `when`, `when_else`).
- Workspace consumer check:
  ```bash
  $ rg "draw_pill|draw_ring|draw_rect_if|draw_if_some|repeat_x|repeat_y|debug_rect|debug_axes|canvas\.translated|canvas\.clipped_rect" crates --type rust
  crates/flui-painting/docs/PERFORMANCE.md:* (doc-only)
  crates/flui-painting/docs/ARCHITECTURE.md:* (doc-only)
  crates/flui-painting/README.md:* (doc-only)
  ```
  Zero production callers in any other crate, and **no in-painting callers either** outside the defining `impl Canvas` block.

**Why it's a problem:**
- 720 LOC of public API surface with **zero workspace consumers**, including inside flui-painting itself.
- Each method delegates to a primary method in `canvas/{drawing,transform,clipping,state,scoped}.rs`. The duplication is API ergonomics theater.
- The methods are exposed via `Canvas`'s inherent impls — they live in the crate root's namespace and inflate the public-method count from ~50 (primary) to ~80+ (with sugar). IDE auto-completion noise.

**Fix shape:** Feature-gate the whole sugar module:
```rust
// crates/flui-painting/src/canvas/mod.rs
#[cfg(feature = "canvas-sugar")]
pub mod sugar;

// Cargo.toml
[features]
default = ["text"]
canvas-sugar = []
text = ["dep:cosmic-text"]
serde = ["dep:serde", "flui-types/serde"]
```

OR delete entirely. The cycle-1 pattern (PR #93 typestate.rs deletion) and cycle-3 pattern (PR #105 feature-gating zero-consumer surfaces) both apply. Audit recommends feature-gate (allows reintroduction at the user's option).

**Blast radius:** ~−720 LOC moved behind cfg. ~10 LOC mod.rs delta. No external API break.

---

#### P-5 [P0 DEAD-CODE | CRITICAL] `flui-painting::text_painter` module (751 LOC across 4 files) — zero production consumers

**Evidence:**
- `crates/flui-painting/src/text_painter/{mod,measure,paint,baseline}.rs`: 366 + 210 + 143 + 32 = 751 LOC.
- Workspace consumer check:
  ```bash
  $ rg "use flui_painting::TextPainter|flui_painting::TextPainter|TextPainter::new" crates --type rust
  # 40+ hits, all in:
  #   crates/flui-painting/tests/text_painter_unit.rs
  #   crates/flui-painting/tests/text_layout_pipeline.rs
  #   crates/flui-painting/tests/rich_text_example.rs
  #   crates/flui-painting/README.md (doc)
  ```
- The text-render path through flui-engine: `Canvas::draw_text(text, offset, size, style, paint)` → `DrawCommand::DrawText { text: String, ..., style: TextStyle, paint, transform }` → `flui_engine::wgpu::backend::Backend::render_draw_text` (no `TextPainter` indirection).
- `TextPainter` is a stateful builder over cosmic-text's `Buffer` with cached layout metrics — useful for widgets that need glyph-level metrics before recording (link hit-testing in `RichText`, cursor positioning in editable text). **None of those widgets exist in the workspace**.

**Why it's a problem:**
- 751 LOC of public API surface, zero production callers. The 2026-05-20 audit didn't flag this (text-painter was newer); cycle 5 surfaces it.
- The state machine is sophisticated: `mark_needs_layout`, `did_layout`, `layout_cache`, `compute_layout_metrics`, `compute_paint_offset`, `get_offset_for_caret`, `get_position_for_offset`, `get_line_metrics`, `get_boxes_for_selection`, `get_word_boundary`. Each method's invariants depend on a successful `layout()` call. Maintaining this for tests-only is high cost.
- The README at line 924 documents `let text_painter: TextPainter = TextPainter::new()...` as a primary API — diverging from the actual production path which goes through `Canvas::draw_text` + `DrawCommand::DrawText`.

**Fix shape:** Feature-gate behind `text-painter`, default off:
```rust
// crates/flui-painting/Cargo.toml
[features]
default = ["text"]
text = ["dep:cosmic-text"]
text-painter = ["text"]
canvas-sugar = []

// crates/flui-painting/src/lib.rs
#[cfg(feature = "text-painter")]
pub mod text_painter;
#[cfg(feature = "text-painter")]
pub use text_painter::{DEFAULT_FONT_SIZE, TextBaseline, TextPainter};
```

When a widget that needs `TextPainter` materializes (`RichText` with hit-testable spans, `EditableText`), the consumer enables the feature.

**Blast radius:** ~−751 LOC behind cfg (still compiles, just feature-gated). 4 lib.rs `pub use` lines cfg-guarded. The README documents an opt-in API. Tests move to `#[cfg(feature = "text-painter")]`.

---

#### P-6 [P1 DEAD-CODE | HIGH] `Canvas::draw_polyline` walks `points[i]` / `points[i+1]` via indexed loop — clippy-lintable but works

**Evidence:**
- `crates/flui-painting/src/canvas/drawing.rs:478-486`:
  ```rust
  pub fn draw_polyline(&mut self, points: &[Point<Pixels>], paint: &Paint) {
      if points.len() < 2 {
          return;
      }
      for i in 0..points.len() - 1 {
          self.draw_line(points[i], points[i + 1], paint);
      }
  }
  ```
- The idiomatic Rust shape is `points.windows(2).for_each(|w| self.draw_line(w[0], w[1], paint))` — equivalent but no integer arithmetic at the call site, cleaner iterator chain.

**Why it's a problem:**
- Style + readability. Clippy `needless_range_loop` lint would catch this if enabled.
- Each `draw_line` clones `paint` (per cycle 4 audit note on canvas allocation hot path). For 100 points = 99 paint clones. The cost is real but bounded.

**Fix shape:**
```rust
pub fn draw_polyline(&mut self, points: &[Point<Pixels>], paint: &Paint) {
    for w in points.windows(2) {
        self.draw_line(w[0], w[1], paint);
    }
}
```

**Blast radius:** drawing.rs only. ~5 LOC.

---

#### P-7 [P1 ALLOCATION-HOT-PATH | HIGH] `Canvas::draw_*` methods clone `Paint` (~80-200 bytes incl. `Box<Shader>`) per call

**Evidence:**
- `crates/flui-painting/src/canvas/drawing.rs:42-487`: every `draw_*` method takes `paint: &Paint` and pushes `paint: paint.clone()` into the DrawCommand variant.
- The 29 variants of `DrawCommand` each carry `paint: Paint` (or `Option<Paint>`) by value.
- The module-level doc-comment at `drawing.rs:15-26` admits the cost: *"Every `draw_*` call clones `Paint` (~80-200 bytes incl. optional `Box<Shader>` payload). `draw_path`/`draw_shadow` additionally clones the `Path` (`Vec<PathCommand>` heap allocation). Paint interning + flat-bytecode + Path-Cow are tracked in `ARCHITECTURE.md ## Outstanding refactors` and require measured benefit before adoption."*
- `Paint` is defined at `flui_types::painting::paint::Paint:77`:
  ```rust
  pub struct Paint {
      pub color: Color,
      pub style: PaintStyle,
      pub stroke_width: f32,
      pub stroke_cap: StrokeCap,
      pub stroke_join: StrokeJoin,
      pub blend_mode: BlendMode,
      pub shader: Option<Box<Shader>>,  // ← heap alloc on clone if Some
      // ... more fields
  }
  ```

**Why it's a problem:**
- A typical 60fps frame with 100 draw operations × ~120 bytes/Paint clone = 12 KB/frame of pure Paint cloning. Plus the `Box<Shader>` allocations for gradient draws.
- The doc-comment explicitly acknowledges this is tracked-but-not-fixed.
- The cycle 4 audit identified the same Paint-clone pattern in `flui-rendering`'s `CanvasContext::paint_*` methods — the issue compounds across layers.

**Flutter reference:** Flutter's `dart:ui Canvas.drawRect(Rect, Paint)` does not clone the `Paint` — the engine's `Skia::SkCanvas::drawRect` reads from the supplied `SkPaint` directly. Dart's GC and value semantics make this less expensive than the Rust port's pessimistic `clone()`.

**Fix shape (audit-recommended):** Two-phase:
1. **`Arc<Paint>` interning at recording time.** `DrawCommand` carries `paint: Arc<Paint>` instead of `paint: Paint`. `Canvas` maintains an `Arc<Paint>` interning pool keyed by `(color.0, style as u8, blend_mode as u8, …)` so identical paints share storage. First-time Paint costs the same; subsequent identical Paints become `Arc::clone` (refcount bump, no heap alloc).
2. **`Cow<'a, Paint>` for one-shot paints** — Skia's pattern; the engine reads the Paint during dispatch_command and doesn't keep a long reference. Lifetime-bound, no `Arc` needed for the common case.

Audit recommends interning (option 1) — better caches for repeated-paint scenarios (stroke widgets with a single `Paint::stroke(Color::BLACK, 1.0)`).

**Blast radius:** Cross-cutting. `DrawCommand` enum field type change × 29 variants. `Canvas` interning pool. `Engine` dispatch updated to read through `Arc::as_ref`. Estimated ~200 LOC delta + behaviorally significant test suite changes. This is **structural enough that audit defers to a dedicated wave** rather than landing in cycle 5's first batch.

---

#### P-8 [P1 DEAD-CODE | HIGH] `ClipShape` enum variants carry data via `#[allow(dead_code)]` — fields stored but never read

**Evidence:**
- `crates/flui-painting/src/canvas/state.rs:36-56`:
  ```rust
  #[derive(Debug, Clone)]
  #[allow(dead_code)] // Fields stored for future optimization features
  pub enum ClipShape {
      Rect(Rect<Pixels>),
      RRect(flui_types::geometry::RRect),
      RSuperellipse(flui_types::geometry::RSuperellipse),
      Path(Box<flui_types::painting::Path>),
  }
  ```
- The clip stack at `Canvas::clip_stack: Vec<ClipShape>` (line 99 of `canvas/mod.rs`) is appended to by `canvas/clipping.rs` (each `clip_rect`/`clip_rrect`/`clip_path` pushes a variant).
- The `save_count()` query reads the stack length (not variant data); the per-variant payload (`Rect`, `RRect`, `RSuperellipse`, `Path`) is **stored** but **never read** by any code path.
- The doc-comment at line 38 lists 3 "future optimization features" — culling, clip-bounds queries, render optimization — none of which is implemented.

**Why it's a problem:**
- 4 large variants (Path boxes a `Vec<PathCommand>`) carried for theoretical future use. The `Path(Box<...>)` boxing adds heap allocation per `clip_path` call for data nothing reads.
- `#[allow(dead_code)]` discount means the rust compiler can't flag the unused fields.
- Cycle 3 PR #106 pattern (delete-or-cadence): same shape applies here.

**Fix shape:** Replace `ClipShape` with `usize` clip-depth tracking. `Canvas::clip_stack` becomes a simple counter incremented by clips and truncated on `restore`. The variant data is never used; tracking via counter is `O(1)` memory and equivalent behaviorally.

If/when culling materializes, reintroduce the variant data at that time with a documented consumer.

**Blast radius:** state.rs + clipping.rs + canvas/mod.rs. ~30 LOC reduction (removes `ClipShape` enum + 4 push sites; replaces with depth counter).

---

#### P-9 [P1 PARALLEL-TYPE | HIGH] `Picture` type alias at `flui_painting::Picture = DisplayList` — re-exports the same type under two names

**Evidence:**
- `crates/flui-painting/src/lib.rs:239`:
  ```rust
  /// Flutter compatibility: Picture is our DisplayList
  pub type Picture = DisplayList;
  ```
- Workspace usage:
  ```bash
  $ rg "use flui_painting::Picture|flui_painting::Picture\b" crates --type rust
  crates/flui-painting/src/lib.rs:239 (definition)
  # zero production usage outside the definition itself
  ```

**Why it's a problem:**
- `Picture` aliases `DisplayList` to provide Flutter API compatibility (`PictureRecorder.endRecording() -> Picture`). But no FLUI consumer reads `Picture`; the canonical name in the codebase is `DisplayList`. The alias is API surface theater.
- Confusing for readers: which name is canonical? The doc-comment promotes both. flui-types' painting prelude doesn't re-export `Picture`. Documentation drift between modules.

**Fix shape:** Choose one:
1. **Delete the `Picture` alias** (audit-recommended). `DisplayList` is the canonical name; Flutter-compat is achievable via an example in the README that aliases `type Picture = DisplayList` at call sites.
2. **Rename `DisplayList` to `Picture`** to match Flutter naming — bigger change but reduces aliasing.

Audit recommends option 1 — cycle 4 / cycle 3 conventions favor descriptive Rust-native names (`DisplayList` over Flutter's `Picture`).

**Blast radius:** lib.rs + prelude. ~3 LOC delete. Zero consumers; no API break.

---

#### P-10 [P2 DEAD-CODE | MEDIUM] `SystemFontsNotifier` listener registry — no production trigger

**Evidence:**
- `crates/flui-painting/src/binding.rs:263-307`: `pub struct SystemFontsNotifier { listeners: RwLock<Vec<Arc<dyn Fn() + Send + Sync>>> }`.
- `crates/flui-painting/src/binding.rs:399-405`: `PaintingBinding::handle_system_message("fontsChange")` → `system_fonts.notify_listeners()`.
- Workspace consumer search:
  ```bash
  $ rg "handle_system_message|notify_listeners\(\)" crates --type rust | grep -v "flui-painting/src/binding.rs\|tests/"
  # 0 hits — no production caller invokes handle_system_message
  ```
- The OS-level platform integration that would fire `handle_system_message("fontsChange")` is `flui-platform`, but no plumbing exists. The notifier is registered + listened-to in tests only.

**Why it's a problem:**
- 50 LOC of public listener API with no event source. The `Arc<dyn Fn()>` listener Arc + RwLock infrastructure is real cost; the trigger doesn't exist.
- Forward-looking infrastructure with no clear `REMOVE_BY:` cadence.

**Fix shape:** Two options:
1. **Demote to `pub(crate)`** until the platform-side trigger materializes. Public API drops the listener registration surface; tests update to use crate-internal access.
2. **Document with `// REMOVE_BY: 2026-09-22`** marker and leave in place for ~3 months. If no consumer appears, delete then.

Audit recommends option 1 — the `Arc<RwLock<Vec<Arc<dyn Fn>>>>` shape is significant cost; locking it behind `pub(crate)` lets us preserve the impl while removing it from external API surface.

**Blast radius:** binding.rs `pub` → `pub(crate)`. ~5 LOC visibility change. Tests already in-crate.

---

#### P-11 [P2 ALLOCATION-HOT-PATH | MEDIUM] `Canvas::append_display_list_at_offset` clones + transform-rewrites every command — `O(N)` per layer replay

**Evidence:**
- `crates/flui-painting/src/canvas/composition.rs:83-96`:
  ```rust
  pub fn append_display_list_at_offset(
      &mut self,
      display_list: &DisplayList,
      offset: Offset<Pixels>,
  ) {
      if offset.dx == px(0.0) && offset.dy == px(0.0) {
          self.display_list.append(display_list.clone());
          return;
      }
      let mut shifted = display_list.clone();
      shifted.apply_transform(Matrix4::translation(offset.dx.0, offset.dy.0, 0.0));
      self.display_list.append(shifted);
  }
  ```
- The comment at lines 70-77 explains the rationale: *"Without this rewrite the appended commands keep their original transforms (recorded against the child canvas's origin) and the `offset` argument silently drops on the floor; `Canvas::translate` only mutates the canvas's current transform for future recorded commands"*.
- `O(N)` clone + `O(N)` transform-rewrite, where N = `display_list.len()`.

**Why it's a problem:**
- This is the cached-layer-replay hot path for `RepaintBoundary` cycles. Per-frame cost = cached-layer commands × cached-layer count. A 1000-command picture replayed each frame burns 16ms+ on the CPU side before any GPU work.
- The alternative (apply offset as a pushed transform during paint instead of baking) would be `O(1)` add-clip-stack.

**Flutter reference:** Flutter's `PictureLayer.draw(Canvas)` doesn't rewrite the picture's commands; the engine applies the layer's `offset` as a `concat()` on the GPU side. The picture data is shared between frames.

**Fix shape:** Apply offset as a transform during engine consumption, not during recording:
1. Replace `append_display_list_at_offset(dl, offset)` body with `self.display_list.append_with_offset_hint(dl, offset)`.
2. New method on `DisplayList` carries an inline offset alongside the appended commands (or wraps them in a `SaveLayer` + `Translate` pair).
3. Engine reads the offset hint during dispatch and applies it.

OR keep the current shape but optimize the clone path: `Arc::clone(&dl.commands)` instead of `Vec::clone()` (each `DrawCommand` is Clone but the `Vec` allocation is what dominates).

**Blast radius:** composition.rs + DisplayList. ~50 LOC delta. Performance-sensitive; needs micro-benchmark before/after.

---

#### P-12 [P2 STYLE | MEDIUM] `Paint`, `BlendMode`, `Path`, `Color` etc are re-exported from `flui_types::painting` — the re-export creates double-import temptation

**Evidence:**
- `crates/flui-painting/src/lib.rs:244-246`:
  ```rust
  pub use flui_types::painting::{
      BlendMode, Paint, PaintBuilder, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin,
  };
  ```
- `crates/flui-painting/src/display_list/mod.rs:54-58`:
  ```rust
  pub use flui_types::painting::{
      BlendMode, Clip, ClipOp, FilterQuality, Paint, PointMode, Shader, TextureId,
      effects::ImageFilter,
      image::{ColorFilter, ImageRepeat},
  };
  ```
- Workspace consumers can write either `use flui_painting::Paint` or `use flui_types::Paint`. Both resolve to the same type but the imports look like different types.

**Why it's a problem:**
- Documentation drift potential. A function signature `fn f(p: flui_painting::Paint)` is identical to `fn f(p: flui_types::Paint)` but readers may not know.
- Diagnostic messages (`error[E0308]: mismatched types`) report the canonical path (`flui_types::painting::Paint`), which can confuse users importing from flui-painting.

**Fix shape:** Single-source re-export with explicit aliasing:
```rust
// Either:
pub use flui_types::painting::Paint as Paint;
// To make clear the re-export is a convenience.
// Or:
pub mod paint {
    pub use flui_types::painting::Paint;
}
// To make consumers write flui_painting::paint::Paint, clearly an alias.
```
Audit recommends keeping the current re-exports (the prelude is the documented public surface) but adding a CONTRIBUTING.md note that flui_types is the canonical source for these types.

**Blast radius:** Documentation only. ~10 LOC.

---

#### P-13 [P2 STYLE | MEDIUM] `flui-painting::display_list::DrawCommand` lacks a `kind` accessor with const dispatch — uses a 7-LOC match in `kind()` instead

**Evidence:**
- `crates/flui-painting/src/display_list/command.rs:449-459`: `pub enum CommandKind { Draw, Clip, Effect, Layer }`.
- `crates/flui-painting/src/display_list/command_ops.rs:*` (verified via earlier read): `DrawCommand::kind()` matches all 29 variants and returns one of 4 `CommandKind` values.

**Why it's a problem:**
- 29-variant match per `kind()` call. The `is_draw()`, `is_clip()`, `is_effect()`, `is_layer()`, `is_shape()`, `is_image()`, `is_text()` helpers in `sealed.rs::DisplayListExt` each invoke `kind()` (or pattern-match similarly) in their filter predicates.
- For a 1000-command DisplayList, filtering by kind = 1000 × 29-arm match = 29k comparisons per filter pass.
- Hot path: `DisplayListStats::stats()` invokes `count_by_kind` which loops over all commands.

**Fix shape:** Add a discriminant byte to each `DrawCommand` variant or use `std::mem::discriminant`-based dispatch:
- A `#[repr(u8)]` on the enum + variant discriminants lets `kind()` become a single byte read.
- Alternative: pre-compute the `CommandKind` once per command and store as a sibling field (denormalized — increases memory).

This is the cycle 4 E-11 pattern (rename `wgpu::multi_draw::DrawCommand` for performance hint compatibility) applied to flui-painting's hot path.

**Blast radius:** command.rs + command_ops.rs. ~30 LOC delta. Performance-positive but minor.

---

#### P-14 [P3 DEAD-CODE | LOW] `flui-painting/src/canvas/sugar/batch.rs::draw_atlas` and other forward-looking iteration helpers — only doc-comment usage

**Evidence:**
- `crates/flui-painting/src/canvas/sugar/batch.rs` (58 LOC) defines `draw_rects(rects: &[(Rect, Paint)])`, `draw_circles(circles: &[(Point, Pixels, Paint)])`, etc.
- The "atlas" draw is `Canvas::draw_atlas` (in `canvas/drawing.rs:370-401`) — single-call multi-sprite. Different method.
- Workspace consumer search:
  ```bash
  $ rg "canvas\.draw_rects\b|canvas\.draw_circles\b|canvas\.draw_lines\b|canvas\.draw_paths\b" crates --type rust
  # 0 hits
  ```

**Why it's a problem:**
- Bundle with P-4. Each batch method is `for i in 0..items.len() { self.draw_X(items[i]) }` — saves no per-call overhead, just an iteration shape.
- Same dead-code status as the rest of sugar.

**Fix shape:** Feature-gate along with P-4.

**Blast radius:** Bundled with P-4.

---

#### P-15 [P3 DEAD-CODE | LOW] `flui-painting/src/canvas/composition.rs::Canvas::record` and `Canvas::build` static constructors

**Evidence:**
- `crates/flui-painting/src/canvas/composition.rs:105-128`:
  ```rust
  pub fn record<F>(f: F) -> DisplayList
  where F: FnOnce(&mut Canvas) { ... }
  pub fn build<F>(f: F) -> Self
  where F: FnOnce(&mut Canvas) { ... }
  ```
- Workspace consumer check:
  ```bash
  $ rg "Canvas::record|Canvas::build\b" crates --type rust
  # 0 hits in production
  ```

**Why it's a problem:**
- Forward-looking ergonomics with zero consumers. Same status as P-4 sugar but lives in `composition.rs` for thematic reasons.

**Fix shape:** Move to `sugar/composition_sugar.rs` (new file under the sugar feature-gate) OR delete. Audit recommends bundle with P-4.

**Blast radius:** ~20 LOC.

---

#### P-16 [P3 STYLE | LOW] `flui-painting/src/canvas/state.rs::Canvas::save_count` returns `usize` initialized to 1, not 0 (Flutter-parity)

**Evidence:**
- `crates/flui-painting/src/canvas/state.rs:98-101`:
  ```rust
  /// Returns the number of saved states (plus 1 for the initial
  /// state). The initial save count is 1.
  pub fn save_count(&self) -> usize { ... }
  ```
- Flutter's `Canvas.getSaveCount()` returns 1 for an unmodified canvas (matches the initial save scope). FLUI matches this.

**Why it's a problem:**
- Documentation clarity: callers may expect `save_count() == 0` for "no save() calls yet"; the actual return is 1.
- Not a real problem if the doc-comment is read; flagged for awareness.

**Fix shape:** Docstring already explicit. Optional rename `save_count` → `save_depth` (depth in nesting, 1-indexed by convention) — but this is a public API rename for cosmetic reasons. Defer.

**Blast radius:** None unless rename.

---

#### P-17 [P3 DOC | LOW] `flui-painting/src/text_layout/layout.rs::FONT_SYSTEM` global singleton — documented poisoning trade-off

**Evidence:**
- `crates/flui-painting/src/text_layout/layout.rs:28-49`: `static FONT_SYSTEM: OnceLock<Mutex<FontSystem>> = OnceLock::new();`
- Doc-comment at line 28: *"We deliberately use `parking_lot::Mutex`, which does *not* poison the lock when a panic occurs while it is held. If cosmic-text panics mid-set_text or mid-shape_until_scroll [...] the surviving FontSystem is conceptually corrupt but no subsequent caller will observe a PoisonError. We accept that today because (a) cosmic-text panics are rare in practice, and (b) std::sync::Mutex's poisoning would force every call site to match the lock result. A catch_unwind wrapper around set_text / shape_until_scroll is the principled fix and is tracked in `crates/flui-painting/ARCHITECTURE.md ## Outstanding refactors`."*

**Why it's a problem:**
- Tracked-but-not-fixed in `ARCHITECTURE.md`. No `REMOVE_BY:` cadence.
- Compounds with P-3 (fallback text layout) — if `TextLayout` (cosmic-text impl) is deleted via the option-3 path on P-3, this goes away.

**Fix shape:** Either wrap the cosmic-text calls in `catch_unwind` (the principled fix the doc-comment cites), OR keep as-is with a `// REMOVE_BY: 2026-09-22` cadence marker. Audit recommends marker — `cosmic-text` panics aren't a today-problem.

**Blast radius:** None until the cosmic-text panic materializes.

---

#### P-18 [P3 DOC | LOW] `ARCHITECTURE.md` and `PERFORMANCE.md` reference `Paint interning` + `flat-bytecode DrawCommand` + `Path-Cow` — all tracked, none done

**Evidence:**
- `crates/flui-painting/docs/ARCHITECTURE.md ## Outstanding refactors`: lists 3+ optimization paths.
- `crates/flui-painting/docs/PERFORMANCE.md`: mentions the same.
- No `REMOVE_BY:` cadence on any.

**Why it's a problem:**
- The pattern from cycle 1 (P-X items with `REMOVE_BY:` cadence) is the discipline; painting's outstanding-refactors list doesn't follow it.
- Reviewers reading the docs may interpret "tracked" as "scheduled" — but no scheduling exists.

**Fix shape:** Add `// REMOVE_BY:` or `// REVIEW_BY:` dates to each outstanding refactor. If not done by the date, escalate or delete from the list.

**Blast radius:** docs only.

---

#### P-19 [P3 DEAD-CODE | LOW] `flui-painting/src/lib.rs::DisplayListStats` carries `hit_regions: usize` field that overrides to 0 in trait default

**Evidence:**
- `crates/flui-painting/src/display_list/stats.rs:*`: `DisplayListStats { total, draw, clip, effect, layer, shapes, images, text, hit_regions: usize }`.
- `crates/flui-painting/src/display_list/sealed.rs:158`: `DisplayListExt::stats()` default impl sets `hit_regions: 0` (since the trait can't see the inner `Vec<HitRegion>`).
- `crates/flui-painting/src/display_list/mod.rs:*`: `DisplayList::stats()` overrides to read `self.hit_regions.len()`.

**Why it's a problem:**
- Bundle with P-2. If `hit_regions` is deleted (P-2 fix), the `DisplayListStats::hit_regions` field becomes dead.

**Fix shape:** Bundle with P-2.

**Blast radius:** Bundled.

---

#### P-20 [P3 STYLE | LOW] `flui-painting/src/lib.rs` `#![forbid(unsafe_code)]` — different from workspace lint default

**Evidence:**
- `crates/flui-painting/src/lib.rs:151`: `#![forbid(unsafe_code)]`.
- Workspace `Cargo.toml` lints set `unsafe_code` to `allow` (default for the workspace).

**Why it's a problem:**
- Not actually a problem. The `forbid` here is correct — flui-painting is a pure-safe crate. The strictness is intentional.
- Audit-noted for clarity: this is a Constitution-Principle-3 compliance choice that should be locked in.

**Fix shape:** None. Audit recommends keeping this lint discipline.

**Blast radius:** None.

---

### flui-view findings (V-1 .. V-25)

---

#### V-1 [P0 HALF-IMPL | CRITICAL] `BuildOwner::inherited_elements` registry is built, exposed, tested — and NEVER populated by production

**Evidence:**
- `crates/flui-view/src/owner/build_owner.rs:103-105`:
  ```rust
  /// InheritedElement registry: TypeId -> element ID.
  /// Used for O(1) InheritedView lookup.
  inherited_elements: HashMap<TypeId, ElementId>,
  ```
- `crates/flui-view/src/owner/build_owner.rs:440-454`:
  ```rust
  /// Register an InheritedElement for O(1) lookup.
  ///
  /// This allows `depend_on<T>()` to be O(1) instead of O(depth).
  pub fn register_inherited(&mut self, type_id: TypeId, element: ElementId) { ... }
  pub fn unregister_inherited(&mut self, type_id: TypeId) { ... }
  pub fn inherited_element(&self, type_id: TypeId) -> Option<ElementId> { ... }
  ```
- `crates/flui-view/src/owner/build_owner.rs:469-476`: `Debug` impl exposes `.field("inherited_elements", &self.inherited_elements.len())`.
- Workspace consumer check:
  ```bash
  $ rg "register_inherited|unregister_inherited" crates --type rust | grep -v "/tests/\|owner/build_owner.rs:"
  # 0 hits — no production caller invokes register_inherited
  ```
- Test consumers (`crates/flui-view/tests/inherited_dependency.rs`, `tests/build_owner_tests.rs`) populate the registry manually in test setup. The production path through `InheritedBehavior::on_mount` ([`element/behavior.rs:656-720`](../../crates/flui-view/src/element/behavior.rs)) **does NOT call `register_inherited`** — it only initializes `self.data` / `self.view_cache` / `self.dependents` (all per-element-instance state, not the global lookup).
- The production `depend_on_inherited` path at `crates/flui-view/src/context/element_build_context.rs:261-322` walks ancestors via `walk_ancestors_for_inherited(type_id)` — `O(depth)` for every lookup.
- The docstring at `build_owner.rs:441` explicitly promises: *"This allows `depend_on<T>()` to be O(1) instead of O(depth)"* — a promise the production code does NOT keep.

**Why it's a problem:**
- Worst-of-both-worlds shape: API exists (so the upgrade path is "use it"), HashMap cost paid (one entry per `InheritedElement` would be hot-path-light, but the cost is paid for tests only), production lookup walks ancestors instead.
- The documented promise diverges from the production behavior. Documentation lies.
- Cycle 4 R-5 (drift in `RenderDirtyPropagation` trait) is the same pattern: a pub-but-test-only registry surface that confuses readers into thinking the optimization is live.
- The 18-month-old `2026-05-20` audit didn't flag this because `BuildOwner` was newer.

**Flutter reference:** `framework.dart:5028-5060` — `getElementForInheritedWidgetOfExactType` walks a per-element `_inheritedElements: PersistentHashMap` (every element carries an inherited-ancestor lookup table; updated during mount, propagated to children). Flutter's mechanism is O(1) but with O(depth) build cost; the FLUI design choice to centralize the registry in `BuildOwner` is a Rust-native simplification.

**Fix shape (canonical):** Wire `InheritedBehavior::on_mount` to call `register_inherited`, and `on_unmount` to call `unregister_inherited`:
```rust
// crates/flui-view/src/element/behavior.rs:
impl<V, A> ElementBehavior<V, A> for InheritedBehavior<V>
where V: InheritedView, A: ElementArity,
{
    fn on_mount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Register self in the inherited-element registry for O(1) lookup.
        owner.register_inherited(TypeId::of::<V>(), /* self element_id, somehow threaded through */);
    }

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        owner.unregister_inherited(TypeId::of::<V>());
        // ... existing clear-dependents logic
    }
}
```
**Blocker:** `ElementOwner` doesn't expose `register_inherited` / `unregister_inherited` (verified at [`owner/element_owner.rs`](../../crates/flui-view/src/owner/element_owner.rs)). Need to widen the split-borrow to include `&mut HashMap<TypeId, ElementId>` for `inherited_elements`.

Also: the behavior doesn't know its own `ElementId` at `on_mount` time (the `core: &mut ElementCore<V, A>` doesn't carry id). Need to thread `ElementId` through the `on_mount` signature OR expose `core.element_id()`.

Once wired, update `ElementBuildContext::depend_on_inherited` to consult the registry first:
```rust
fn depend_on_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
    // O(1) registry lookup first.
    let owner = self.owner.read();
    let Some(ancestor_id) = owner.inherited_element(type_id) else {
        // Fallback: ancestor walk (defensive; should never hit if registry stays consistent).
        drop(owner);
        return self.fallback_walk_inherited(type_id, callback);
    };
    drop(owner);
    // ... resolve and invoke callback as today
}
```

**Blast radius:** ~80 LOC across `element/behavior.rs`, `owner/element_owner.rs`, `owner/build_owner.rs`, `context/element_build_context.rs`. Threads `ElementId` through `on_mount` / `on_unmount` (alters trait surface — minor downstream ripple). Tests stay compatible (registry is still populated, just by production now).

---

#### V-2 [P0 HALF-IMPL | CRITICAL] `VariableChildStorage::update_with_views` ignores keys — `reconcile_children` exists but is never called

**Evidence:**
- `crates/flui-view/src/element/child_storage.rs:494-515`:
  ```rust
  fn update_with_views(&mut self, views: &[Box<dyn View>], owner: &mut crate::ElementOwner<'_>) {
      // Simple reconciliation: match by index
      // TODO: In a full implementation, this would use keys for reordering
      for (i, view) in views.iter().enumerate() {
          if let Some(child) = self.children.get_mut(i) {
              child.update(view.as_ref(), &mut *owner);
          } else {
              self.children.push(view.create_element());
          }
      }
      if views.len() < self.children.len() {
          for mut child in self.children.drain(views.len()..) {
              child.unmount(&mut *owner);
              drop(child);
          }
      }
  }
  ```
- `crates/flui-view/src/tree/reconciliation.rs:51-193`: 142-LOC `reconcile_children` function implementing Flutter's 5-phase keyed O(N) linear reconciliation (match-start / match-end / build-key-map / process-middle / cleanup).
- Workspace consumer check for the real algorithm:
  ```bash
  $ rg "reconcile_children\(" crates --type rust
  crates/flui-view/src/tree/reconciliation.rs:* (definition + tests)
  # 0 production callers; tests + definition only
  ```
- The real algorithm correctly uses `new_view.key()` and `key.key_hash()` — index-based matching is the broken default.

**Why it's a problem:**
- `GlobalKey` reparenting (state-preserving move across the tree) breaks: index-based matching doesn't track keys, so a keyed move-from-slot-0-to-slot-5 forces unmount/remount, losing state.
- `Hero` widget patterns break (they rely on `GlobalKey` cross-tree matching).
- `ListView`/`Reorderable`/`AnimatedList` with keys produce mismatched state on reorder.
- The keyed reconciliation algorithm is **already implemented** (325 LOC tested at `tree/reconciliation.rs`) — it just isn't called by the production path.
- The TODO comment makes this finding observable: someone wrote it knowing the gap exists.

**Flutter reference:** `framework.dart:5836-5946` — `Element._updateChildren` implements the canonical Flutter reconciliation algorithm (the source `reconciliation.rs` ports). It is the production path.

**Fix shape (canonical):** Hoist `reconcile_children` into `VariableChildStorage::update_with_views`:
```rust
// crates/flui-view/src/element/child_storage.rs:
fn update_with_views(&mut self, views: &[Box<dyn View>], owner: &mut crate::ElementOwner<'_>) {
    // Pre-compute the &dyn View slice expected by reconcile_children.
    let new_views: Vec<&dyn View> = views.iter().map(|b| b.as_ref()).collect();

    // Use the workspace-canonical reconciliation algorithm.
    // (The old children are at self.children; the new are new_views.)
    crate::tree::reconciliation::reconcile_children(
        /* tree */ ???, // Problem: VariableChildStorage doesn't see ElementTree
        /* parent */ ???,
        /* old */ ???,
        &new_views,
        owner,
    );
}
```
**Blocker:** `VariableChildStorage` carries `children: Vec<Box<dyn ElementBase>>` — it owns the child Elements directly, not their IDs. The `reconcile_children` function operates on `ElementTree` + `ElementId`s. **The two data models are incompatible**.

Two-phase fix needed:
1. Refactor `reconcile_children` to operate on `&mut Vec<Box<dyn ElementBase>>` (cheaper) OR refactor `VariableChildStorage` to store `Vec<ElementId>` and a `&ElementTree` borrow (more invasive).
2. Wire the algorithm into the storage.

**Blast radius:** Cross-cutting, ~200-400 LOC depending on choice. Architectural decision needed — does flui-view's element-tree store-by-id or store-by-value? Currently it's a mix: `ElementTree` stores `ElementNode`s in a Slab keyed by `ElementId`, but `VariableChildStorage` owns `Box<dyn ElementBase>` directly (not via id). The mix is the underlying problem; resolving it makes `reconcile_children` callable.

---

#### V-3 [P0 DEAD-CODE | CRITICAL] `flui-view::view::animated` (296 LOC) + `AnimationBehavior` (128 LOC) — zero production consumers

**Evidence:**
- `crates/flui-view/src/view/animated.rs` (296 LOC): `pub trait AnimatedView: StatefulView` with `listenable()` accessor.
- `crates/flui-view/src/element/behavior.rs:759-886` (128 LOC): `pub struct AnimationBehavior<V> { stateful: StatefulBehavior<V>, listener_id: Option<ListenerId> }` + `impl ElementBehavior` with auto-subscribe/unsubscribe on `on_mount`/`on_unmount`.
- `crates/flui-view/src/lib.rs:` re-exports `AnimatedElement` (type alias).
- Workspace consumer check:
  ```bash
  $ rg "impl AnimatedView|use flui_view::AnimatedView" crates --type rust
  crates/flui-view/README.md:163 (doc)
  crates/flui-view/src/view/animated.rs:63 (doc-comment)
  crates/flui-view/src/view/animated.rs:109 (doc-comment)
  crates/flui-view/src/view/animated.rs:123 (macro usage)
  crates/flui-view/src/view/animated.rs:206 (test impl)
  # 0 production consumers
  ```
- `flui-animation::AnimationBehavior` is a different enum (the animation status enum in `crates/flui-animation/src/status.rs:106`) — **two `AnimationBehavior` types in the workspace with completely different concepts**. The flui-view version is the listener subscription behavior; the flui-animation version is the curve behavior (Normal vs Preserve).

**Why it's a problem:**
- 424 LOC of forward-looking infrastructure for animated widgets that don't exist in the workspace (flui-animation is currently disabled in workspace Cargo.toml).
- The `AnimationBehavior` name collides with `flui_animation::AnimationBehavior` — same workspace, different crate, different concept. Cycle 4 R-7/R-8 pattern.

**Fix shape:** Two options:
1. **Feature-gate `animated-views`, default off** until `flui-animation` is re-enabled in the workspace.
2. **Delete entirely**. When `flui-animation` is re-enabled and a real `AnimatedView` is needed, reintroduce with proper naming (e.g. `ListenableViewBehavior` to avoid the collision).

Audit recommends option 1 (preserves the work, just not in the default build).

**Blast radius:** ~−424 LOC behind cfg. `lib.rs` exports conditional. `element/mod.rs` re-export conditional. Tests cfg-gated.

---

#### V-4 [P0 DEAD-CODE | CRITICAL] `flui-view::view::parent_data` (479 LOC) — zero production impls; only `TestFlexible` test consumer

**Evidence:**
- `crates/flui-view/src/view/parent_data.rs` (479 LOC): `pub trait ParentDataConfig`, `pub trait ParentDataView`, `pub struct ParentDataElement`, `impl_parent_data_view!` macro.
- Workspace consumer check:
  ```bash
  $ rg "impl ParentDataView for" crates --type rust
  crates/flui-view/src/view/parent_data.rs:378:    impl ParentDataView for TestFlexible (test-internal)
  ```
- The doc-comment cites `Positioned` and `Flexible`/`Expanded` widgets as future production consumers — but `flui-widgets` is currently disabled in workspace Cargo.toml.

**Why it's a problem:**
- 479 LOC of forward-looking infrastructure for `ParentDataWidget` ports that don't exist yet.
- The `ParentDataConfig` trait (renamed from `ParentData` in cycle 4 R-11) is the marker trait for parent-data values; `flui_rendering::ParentData` is the storage trait. The naming is now clean — but the feature is unused.

**Fix shape:** Feature-gate `parent-data-views`, default off. Same shape as V-3.

**Blast radius:** ~−479 LOC behind cfg.

---

#### V-5 [P0 DEAD-CODE | CRITICAL] `flui-view::view::error` (333 LOC) — `ErrorView` + `FlutterError` + builder registry; never wired to actual build failures

**Evidence:**
- `crates/flui-view/src/view/error.rs` (333 LOC): `pub struct ErrorView`, `pub struct FlutterError`, `pub fn set_error_view_builder(builder: ErrorViewBuilder)`, etc.
- `crates/flui-view/src/lib.rs:194-199`: re-exports `ErrorElement, ErrorView, ErrorViewBuilder, FlutterError, clear_error_view_builder, set_error_view_builder`.
- Workspace consumer check:
  ```bash
  $ rg "use flui_view::ErrorView|ErrorView::new|FlutterError::new|set_error_view_builder" crates --type rust | grep -v "view/error.rs"
  crates/flui-view/src/view/mod.rs:34 (re-export only)
  crates/flui-view/src/lib.rs:198 (re-export only)
  # 0 production callers
  ```
- The `Element::perform_build` / `unified::Element::perform_build` does NOT wrap user `build()` in `catch_unwind`. A user `build()` panic propagates up, never reaching the `ErrorView` builder.

**Why it's a problem:**
- 333 LOC of build-failure recovery infrastructure that never executes because the production build path doesn't catch panics.
- Flutter's `Element.performRebuild` catches user-`build`-panics and substitutes `ErrorWidget` (`framework.dart:5048-5108`). The FLUI port has the receiver (`ErrorView`) but no producer (no `catch_unwind`).
- Documented in lib.rs prelude but zero adoption.

**Fix shape:** Two options:
1. **Wire it up**: `Element::perform_build` wraps the user `build()` call in `std::panic::catch_unwind`, captures the panic payload, builds a `FlutterError`, and constructs the error view via the registered builder. Real impl needs the panic-resilient framework that Flutter provides.
2. **Feature-gate** `error-view`, default off, until the wrap-and-recover machinery is built.

Audit recommends option 2 for cycle 5 (the wrap-and-recover machinery is a wave on its own). Option 1 is the structurally correct fix; option 2 is the discipline-respecting fix.

**Blast radius:** Option 2: ~−333 LOC behind cfg. Option 1: ~+200 LOC for the catch_unwind integration + behavior change in production build paths.

---

#### V-6 [P0 DEAD-CODE | CRITICAL] `flui-view::view::root` (577 LOC) — `RootRenderView` + `RootRenderElement` never instantiated

**Evidence:**
- `crates/flui-view/src/view/root.rs` (577 LOC): `RootRenderView<V>`, `RootRenderElement<V>`, `impl ElementBase for RootRenderElement<V>`, `impl RenderObjectElement for RootRenderElement<V>`, `impl RenderTreeRootElement for RootRenderElement<V>`.
- `crates/flui-view/src/binding.rs:596-632`: `WidgetsBinding::attach_root_widget(view)` directly mounts the user view via `element_tree.mount_root_with_pipeline_owner(view, pipeline_owner, &mut build_owner.element_owner_mut())` — no `RootRenderView` indirection.
- Workspace consumer check:
  ```bash
  $ rg "RootRenderView::|RootRenderElement::|RootElement::new\b" crates --type rust
  crates/flui-view/src/element/root.rs:* (RootElement, NOT RootRenderElement)
  # 0 callers of RootRenderView/RootRenderElement outside its own module + tests
  ```
- The intent (per docstrings) is the Flutter `_RawViewInternal` shape — a special root widget that owns the `RenderView` (window/screen). The implementation exists; the integration doesn't.

**Why it's a problem:**
- 577 LOC of forward-looking root-bootstrapping infrastructure that the `WidgetsBinding` doesn't use. The binding integrates with the render-tree differently (directly mounting via `pipeline_owner`).
- Cycle 5 should decide: is `RootRenderView` the canonical bootstrap path, or is `WidgetsBinding::attach_root_widget`'s direct path? Both shouldn't exist.

**Fix shape:** Audit recommends preserving `RootRenderView` + `RootRenderElement` but adding a clear `// REMOVE_BY: 2026-09-22` cadence comment. If by then no consumer materializes, delete. The path is forward-looking; the WidgetsBinding direct-mount is a working-shortcut that may need to be reverted toward `RootRenderView` for cycles 6/7.

OR: wire `attach_root_widget` to construct a `RootRenderView::new(user_view, width, height)` and mount that instead of the user view directly. This matches Flutter's `_RawViewInternal` indirection.

**Blast radius:** Either ~+50 LOC (option B: wire it up) or ~−577 LOC (option C: delete) or ~+5 LOC cadence comment (option A: defer).

---

#### V-7 [P0 DRIFT | CRITICAL] `ElementTree` does NOT implement `TreeRead<ElementId>` / `TreeWrite<ElementId>` from flui-tree

**Evidence:**
- `crates/flui-view/src/tree/element_tree.rs:116-122`:
  ```rust
  pub struct ElementTree {
      nodes: Slab<ElementNode>,
      root: Option<ElementId>,
  }
  ```
- The file has no `impl TreeRead<ElementId> for ElementTree`, `impl TreeNav<ElementId> for ElementTree`, or `impl TreeWrite<ElementId> for ElementTree` (verified via `rg "impl Tree.* for ElementTree" crates/flui-view/src`).
- Memory `[[flui-tree-unified-interface-intent]]` says: *"flui-tree spec'd as unified API over Flutter's multi-tree (Element/Render/Layer/Semantics); zero-consumer abstractions = migration gap, not deletion signal"*.
- Cycle 2 PR #100 wired `LayerTree` to `TreeRead<LayerId>` + `TreeNav<LayerId>`. Cycle 3 PR #103 wired `RenderTree` to `TreeWrite<RenderId>` with cascade-by-default. `ElementTree` is the one that hasn't been migrated.

**Why it's a problem:**
- Drift across the workspace's tree primitives. `LayerTree`/`RenderTree`/`SemanticsTree` all (or mostly) integrate with `flui-tree`'s trait surface; `ElementTree` is parallel-hand-rolled.
- Cascade semantics: `ElementTree::remove` currently dispatches to the element's `unmount` which then unmounts children recursively — but the cascade is hidden inside `ElementBase::unmount` impls (per-element). If `TreeWrite<ElementId>` were implemented with `remove` defaulting to cascade (per cycle 3 PR #103), the cascade would be uniform across trees.
- Pattern-consistency: a downstream consumer reading `LayerTree::get(id)` and then `ElementTree::get(id)` should see the same interface shape. They don't.

**Fix shape (canonical):** Implement `TreeRead<ElementId> + TreeNav<ElementId> + TreeWrite<ElementId>` for `ElementTree`:
```rust
// crates/flui-view/src/tree/element_tree.rs:
impl flui_tree::TreeRead<ElementId> for ElementTree {
    type Node = ElementNode;
    fn get(&self, id: ElementId) -> Option<&Self::Node> { ... }
    fn contains(&self, id: ElementId) -> bool { ... }
    fn len(&self) -> usize { ... }
    fn is_empty(&self) -> bool { ... }
}

impl flui_tree::TreeNav<ElementId> for ElementTree {
    fn parent(&self, id: ElementId) -> Option<ElementId> { ... }
    fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ { ... }
    fn root(&self) -> Option<ElementId> { ... }
}

impl flui_tree::TreeWrite<ElementId> for ElementTree {
    fn insert(&mut self, ...) -> ElementId { ... }
    fn remove(&mut self, id: ElementId) -> Option<Self::Node> // cascades by default
    fn remove_shallow(&mut self, id: ElementId) -> Option<Self::Node>;
}
```

**Blast radius:** ~+200 LOC trait impls. `ElementBase::visit_children` currently uses `&mut dyn FnMut(ElementId)` — this is the `TreeNav::children` shape pre-rename. Migration changes the signatures; downstream consumers (test harness, `BuildOwner::collect_elements_to_unmount`) update accordingly.

---

#### V-8 [P0 PARALLEL-TYPE | CRITICAL] `flui-painting::PointerEvent` ≠ `flui-interaction::PointerEvent` — same name, two crates

**Evidence:**
- `crates/flui-painting/src/display_list/hit_region.rs:19-30`: `pub struct PointerEvent { kind, position, pointer: i32, buttons: i32, time_stamp: Duration }`.
- `crates/flui-interaction/src/pointer/event.rs:*`: canonical pointer event with `device_id`, `pointer_id` (NonZeroU64), `pressure`, `tilt`, etc.
- Both `pub use`d at their crate roots.

**Why it's a problem:**
- Bundle with P-2 (flui-painting's `PointerEvent` is dead). After P-2 deletes `hit_region.rs`, this collision goes away.
- Same shape as cycle 4 R-7/R-8/R-9.

**Fix shape:** P-2 deletes the parallel type. Bundle.

**Blast radius:** Resolved by P-2.

---

#### V-9 [P1 DEAD-CODE | HIGH] `flui-view::element::notification::{NotificationNode, NotificationHandler, BoxedNotification, NotificationCallback}` — parallel dispatch infrastructure unused by the unified protocol

**Evidence:**
- `crates/flui-view/src/element/notification.rs:101-220` (~152 LOC): `NotificationNode` (parallel tree node), `NotificationHandler` (trait), `BoxedNotification` (type alias), `NotificationCallback<T>` (callback type alias).
- The active production dispatch path goes through `ElementBase::on_notification(TypeId, &dyn Any) -> bool` ([`view/view.rs:508`](../../crates/flui-view/src/view/view.rs)) → `Element<V, A, B>::on_notification` ([`element/unified.rs:222`](../../crates/flui-view/src/element/unified.rs)) → `behavior.on_notification(...)`. The walk happens in `ElementBuildContext::dispatch_notification` (`context/element_build_context.rs:550-585`) via the `walk_strict_ancestors` helper.
- Workspace consumer check:
  ```bash
  $ rg "NotificationNode::|NotificationHandler\b" crates --type rust | grep -v "notification.rs"
  # 0 hits
  ```
- The doc-comment at line 146 calls `NotificationNode` *"a parallel structure to the element tree, containing only elements that can handle notifications. This enables O(k) dispatch where k is the number of NotifiableElements in the ancestor chain"* — but this parallel tree is never built. The actual dispatch walks the full ancestor chain (`O(depth)`) via `walk_strict_ancestors`.

**Why it's a problem:**
- 152 LOC of unused parallel-tree dispatch infrastructure. The active path is the unified protocol; the parallel-tree variant is dead.
- Confusing for readers: `NotificationNode` looks like the canonical home for the dispatch tree but isn't.

**Fix shape:** Delete `NotificationNode`, `NotificationHandler`, `BoxedNotification`, `NotificationCallback`. Keep `Notification` trait and the typed notification structs (`LayoutChangedNotification`, `ScrollNotification`, `ScreenSizeNotification`, etc.) which are the active surface.

**Blast radius:** ~−152 LOC. lib.rs prelude trim. Update `view/error.rs` and lib.rs re-exports.

---

#### V-10 [P1 DEAD-CODE | HIGH] `WidgetsBinding::SharedWidgetsBinding` + `create_shared_binding` (`#[deprecated]` since 0.2.0)

**Evidence:**
- `crates/flui-view/src/binding.rs:1163-1175`:
  ```rust
  #[deprecated(since = "0.2.0", note = "Use WidgetsBinding::instance() instead")]
  pub type SharedWidgetsBinding = Arc<RwLock<WidgetsBinding>>;

  #[deprecated(since = "0.2.0", note = "Use WidgetsBinding::instance() instead")]
  pub fn create_shared_binding() -> Arc<RwLock<WidgetsBinding>> {
      Arc::new(RwLock::new(WidgetsBinding::new()))
  }
  ```
- Workspace consumer check:
  ```bash
  $ rg "SharedWidgetsBinding|create_shared_binding" crates --type rust
  crates/flui-view/src/binding.rs:* (definition)
  # 0 production callers
  ```

**Why it's a problem:**
- Deprecated for ~6 months (since version 0.2.0). Zero remaining callers. Public API surface that adds documentation noise.

**Fix shape:** Delete both. Bump major version or note in changelog if pre-1.0.

**Blast radius:** ~−10 LOC. Public API removal (already deprecated, so semver-safe).

---

#### V-11 [P1 HALF-IMPL | HIGH] `ReconcileAction` enum is `#[allow(dead_code)]` — algorithm bypasses it

**Evidence:**
- `crates/flui-view/src/tree/reconciliation.rs:15-27`:
  ```rust
  #[derive(Debug)]
  #[allow(dead_code)] // Will be used when full reconciliation is implemented
  pub enum ReconcileAction {
      Update(ElementId),
      Create,
      Remove(ElementId),
      Move(ElementId, usize),
  }
  ```
- The 142-LOC `reconcile_children` function below doesn't construct any `ReconcileAction` value — it directly invokes `tree.insert` / `tree.update` / `tree.remove` and returns `Vec<ElementId>`.
- The doc-comment claims *"Will be used when full reconciliation is implemented"* — but `reconcile_children` IS the full reconciliation. The enum exists but the algorithm bypasses it.

**Why it's a problem:**
- Compound finding with V-2 (the algorithm is unused) — when V-2 wires `reconcile_children` into production, `ReconcileAction` is still unused.
- The canonical "reconciliation returns intentions, then applies" shape is intent-vs-action separation (test the intentions, apply atomically). The current impl mixes both. Refactoring toward `ReconcileAction` is the cleaner long-term design.

**Fix shape (audit-recommended):** Two-phase:
1. Bundle with V-2's wave: keep `ReconcileAction` defined but only as a future-bait shape. Remove the `#[allow(dead_code)]` only after a non-trivial consumer materializes.
2. Alternative: delete `ReconcileAction` until the intent-vs-action separation is implemented.

Audit recommends keeping `ReconcileAction` as forward-looking; mark with `// REMOVE_BY: 2026-09-22` cadence.

**Blast radius:** Documentation only (~5 LOC).

---

#### V-12 [P1 STYLE | HIGH] `attach_root_widget` panics on double-attach via `assert!` — Constitution Principle 6 spirit violation

**Evidence:**
- `crates/flui-view/src/binding.rs:599-602`:
  ```rust
  assert!(
      inner.root_element.is_none(),
      "Root widget already attached. Call detach_root_widget first."
  );
  ```
- Test at line 1320 explicitly asserts the panic: `#[should_panic(expected = "Root widget already attached")]`.

**Why it's a problem:**
- Production-path panic. Constitution Principle 6 says *"No `unwrap()`/`println!`/`dbg!`/`unimplemented!`/`todo!` in production paths"* — `assert!` is the same shape (process abort on failure).
- Documented behavior via test (the contract is "panic on double-attach"). Replacing with `Result::Err` is a breaking API change.

**Fix shape (canonical):** Convert to `Result`:
```rust
pub fn attach_root_widget<V: View>(&self, view: &V) -> Result<(), AttachError> {
    let mut inner = self.inner.write();
    if inner.root_element.is_some() {
        return Err(AttachError::AlreadyAttached);
    }
    // ... existing logic
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum AttachError {
    #[error("Root widget already attached. Call detach_root_widget first.")]
    AlreadyAttached,
}
```
Update the test to assert the `Err(AttachError::AlreadyAttached)` shape.

**Blast radius:** binding.rs + tests. ~+30 LOC. Breaking API change for any consumer that called `attach_root_widget` and didn't handle the panic (none in-workspace).

---

#### V-13 [P1 STYLE | HIGH] `ElementBuildContext::new_minimal` creates dummy tree + owner — leaks Arc-shared dummy across builds

**Evidence:**
- `crates/flui-view/src/context/element_build_context.rs:211-227`:
  ```rust
  pub fn new_minimal(depth: usize) -> Self {
      let tree = Arc::new(RwLock::new(ElementTree::new()));
      let owner = Arc::new(RwLock::new(BuildOwner::new()));
      let element_id = ElementId::new(1);
      Self {
          element_id,
          depth,
          mounted: true,
          tree,
          owner,
          #[cfg(debug_assertions)]
          is_building: true,
      }
  }
  ```
- Called from `StatelessBehavior::perform_build` ([`element/behavior.rs:222`](../../crates/flui-view/src/element/behavior.rs)) — every stateless build creates a fresh dummy tree + owner just to pass `ctx` to `view.build(&ctx)`.

**Why it's a problem:**
- **Every `StatelessView::build` allocates two `Arc<RwLock<...>>` payloads + two empty inner structures** — measured cost per stateless rebuild ≈ 2 KB heap + 2 atomics.
- The dummy tree/owner means `BuildContext::find_ancestor_element` / `depend_on_inherited` / `find_render_object` all return `None` for stateless builds — the call sites genuinely don't have an ancestor tree to walk.
- The hardcoded `ElementId::new(1)` is invariant — every stateless build sees the same "id". Confusing if anyone inspects.

**Flutter reference:** `framework.dart::ComponentElement.performRebuild` passes the actual `this` element as `BuildContext`. The element IS the build context. FLUI's split between Element and BuildContext requires the dummy.

**Fix shape (canonical):** Thread the actual `BuildContext` reference through `perform_build`:
```rust
// crates/flui-view/src/element/behavior.rs (StatelessBehavior):
fn perform_build(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
    // Borrow the live ElementBuildContext from the binding, not new_minimal.
    let ctx = ???; // Problem: ElementBuildContext doesn't exist for an Element being built
    let child_view = core.view().build(&ctx);
    // ...
}
```

**Blocker:** the unified `Element<V, A, B>` doesn't have an `ElementBuildContext` of its own — the build context is constructed per-call. Constructing a real one needs `Arc<RwLock<ElementTree>>` + `Arc<RwLock<BuildOwner>>` — the inputs to `BuildContext::new`, which is what `WidgetsBinding` carries.

Two options:
1. **Pass real `ElementBuildContext` references through `perform_build`** — threads the binding's Arcs through to every perform_build call. Bigger surface change.
2. **Cache a single "dummy" `ElementBuildContext` once per `BuildOwner`** — reuse the same dummy instead of allocating per-build. Less ambitious; preserves the same semantics (still `None`-returning ancestor lookups for stateless) but eliminates the per-build allocations.

Audit recommends option 2 for cycle 5 (option 1 is a wave on its own).

**Blast radius:** Option 2: ~+30 LOC adding a cached dummy in `BuildOwner` + lifetime threading. Option 1: ~+150 LOC across binding/element machinery.

---

#### V-14 [P1 DEAD-CODE | HIGH] `flui-app::theme::colors::{Color, ColorScheme}` — parallel `Color` struct with `f32` channels

**Evidence:**
- `crates/flui-app/src/theme/colors.rs:5-20`: `pub struct Color { r: f32, g: f32, b: f32, a: f32 }`.
- `crates/flui-types/src/styling/color.rs:8`: `pub struct Color { /* packed u32 ARGB */ }` — canonical workspace Color.
- Workspace consumer check:
  ```bash
  $ rg "use flui_app::theme::Color|use flui_app::theme::colors|flui_app::theme::Color" crates --type rust
  # 0 hits
  ```

**Why it's a problem:**
- **Two `Color` structs with different field shapes** — `f32` per-channel vs packed `u32`. Cycle 4 R-10 / R-7 / R-8 pattern: parallel types with name collisions across crates.
- flui-app's `Color` has zero consumers — pure dead code.

**Fix shape:** Delete `flui-app::theme::colors::Color` + `ColorScheme`. Use `flui_types::Color` workspace-wide. The `ColorScheme` (semantic tokens) is reasonable as a flui-app concept but should be built from `flui_types::Color` values, not the parallel `Color` struct.

**Blast radius:** Cross-crate. ~−150 LOC delete. Verifies dead — zero callers.

**Note:** This finding is in `flui-app`, not `flui-view`. Out of cycle-5 strict scope but called out as a P3 cleanup that touches the audit's drift catalog. Defer to a future cycle.

---

#### V-15 [P2 DEAD-CODE | MEDIUM] `BuildOwner::DirtyElement::depth()` accessor is `#[allow(dead_code)]`

**Evidence:**
- `crates/flui-view/src/owner/build_owner.rs:39-49`:
  ```rust
  /// Depth used to order the heap (shallowest first).
  ///
  /// Currently consumed only by inline tests; U9+ will read it during
  /// dirty-element drain dispatching. The `Ord` impl reads
  /// `self.depth` directly (private field access from the same `impl`
  /// block), so the accessor stays on the surface for future
  /// `ElementOwner` consumers.
  #[allow(dead_code)]
  pub(crate) fn depth(&self) -> usize { ... }
  ```
- `crates/flui-view/src/owner/build_owner.rs:52-58`: `Ord` impl reads `self.depth` directly (field access from same module).

**Why it's a problem:**
- 6-LOC `#[allow(dead_code)]` placeholder. Promise "U9+ will read it" is stale (U9 has shipped per the existing memory note about the framework spine repair PR #84).
- Either ElementOwner has the future use case or it doesn't; the accessor should be either deleted or wired.

**Fix shape:** Delete the accessor. The `Ord` impl uses direct field access (`self.depth.cmp(&other.depth)`); no caller needs the public getter. The `InactiveElement::depth()` has the same pattern at line 154.

**Blast radius:** ~−15 LOC across `DirtyElement::depth` + `InactiveElement::depth`.

---

#### V-16 [P2 ALLOCATION-HOT-PATH | MEDIUM] `WidgetsBinding::collect_all_elements` recursive `Vec::extend` — `O(N²)` in the worst case

**Evidence:**
- `crates/flui-view/src/binding.rs:690-706`:
  ```rust
  fn collect_all_elements(
      tree: &ElementTree,
      id: ElementId,
      depth: usize,
  ) -> Vec<(ElementId, usize)> {
      let mut result = vec![(id, depth)];
      if let Some(node) = tree.get(id) {
          node.element().visit_children(&mut |child_id| {
              result.extend(Self::collect_all_elements(tree, child_id, depth + 1));
          });
      }
      result
  }
  ```
- Called from `schedule_root_rebuild` which marks every element dirty (animation-driven full-tree rebuild).
- Each recursive call allocates a fresh `Vec` for its subtree; the parent's `extend` does an `O(child_subtree)` copy.

**Why it's a problem:**
- For a balanced tree of N elements, total work is `O(N log N)` (each level copies its width into the parent). For an unbalanced tree (chain-of-Stateless), it's `O(N²)`.
- The function is called from `schedule_root_rebuild` — animation demos that mark every frame dirty. Per-frame cost = `O(N²)` allocation + copy.

**Fix shape:** Iterative pre-allocated walk:
```rust
fn collect_all_elements(
    tree: &ElementTree,
    root_id: ElementId,
) -> Vec<(ElementId, usize)> {
    let mut result = Vec::with_capacity(tree.len());
    let mut stack = vec![(root_id, 0_usize)];
    while let Some((id, depth)) = stack.pop() {
        result.push((id, depth));
        if let Some(node) = tree.get(id) {
            node.element().visit_children(&mut |child_id| {
                stack.push((child_id, depth + 1));
            });
        }
    }
    result
}
```
`O(N)` total work, single allocation upfront sized to `tree.len()`.

**Blast radius:** binding.rs only. ~20 LOC delta. Performance-positive.

---

#### V-17 [P2 STYLE | MEDIUM] `Lifecycle` enum lacks `#[non_exhaustive]` — future variant addition is a breaking change

**Evidence:**
- `crates/flui-view/src/element/lifecycle.rs:18-45`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
  pub enum Lifecycle {
      #[default]
      Initial,
      Active,
      Inactive,
      Defunct,
  }
  ```
- No `#[non_exhaustive]` attribute.

**Why it's a problem:**
- Public enum without `#[non_exhaustive]`. Adding a variant (`Suspended` for `keepAlive` scenarios, for example) is a breaking change for every external consumer matching on the enum.
- Constitution / 2026 quality bar: `#[non_exhaustive]` on public enums is required.
- The `Lifecycle::is_*` accessor methods + `can_*` methods provide a stable shape — but they assume callers don't pattern-match directly. The current public surface allows direct matching.

**Fix shape:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Lifecycle {
    #[default]
    Initial,
    Active,
    Inactive,
    Defunct,
}
```

**Blast radius:** lifecycle.rs only. ~1 LOC. May surface match-exhaustiveness errors in downstream code (test harness).

---

#### V-18 [P2 STYLE | MEDIUM] `BuildOwner::on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>` — `Arc` over `Box` for cheaper cloning

**Evidence:**
- `crates/flui-view/src/owner/build_owner.rs:127`:
  ```rust
  pub(crate) on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,
  ```
- `crates/flui-view/src/owner/element_owner.rs:99-103`: `ElementOwner` stores a borrowed reference `&'a dyn Fn() + Send + Sync` via `as_deref()` from this field.
- The callback never moves; it's invoked once at `schedule_build_for` and never owned by anyone else.

**Why it's a problem:**
- `Box<dyn Fn()>` is the right shape if the callback is single-owned. The current use is single-owned (lives on `BuildOwner`). The `Arc` alternative would only be necessary if the callback were shared with another owner.
- Audit-noted as a discipline reminder: `Box` is correct here.

**Fix shape:** None. Audit recommends keeping `Box<dyn Fn>`. This finding is for completeness.

**Blast radius:** None.

---

#### V-19 [P2 DEAD-CODE | MEDIUM] `view::stateful::ViewState::activate` / `deactivate` / `dispose` lifecycle hooks have empty default impls — only `StatefulBehavior` consumes them

**Evidence:**
- `crates/flui-view/src/view/stateful.rs:96-150` (estimated; verified shape from cycle context): `ViewState::activate(&mut self)`, `deactivate(&mut self)`, `dispose(&mut self)`, `did_change_dependencies(&mut self, &dyn BuildContext)`, `did_update_view(&mut self, &V)`. All have empty default impls.
- Only `StatefulBehavior::on_activate` / `on_deactivate` / `on_unmount` invoke these hooks (per [`element/behavior.rs:347-365`](../../crates/flui-view/src/element/behavior.rs)).

**Why it's a problem:**
- 5 default-impl hooks per `ViewState`. Most user state never overrides them (the empty defaults are the common case). Pattern is fine; just observed.
- The `did_change_dependencies` hook is **not currently wired** through `InheritedBehavior::on_view_updated` — Flutter parity ([`framework.dart:5160`](https://github.com/flutter/flutter)) would notify dependents via `didChangeDependencies` on each dependent's `State`. The FLUI port doesn't do this — `update_should_notify == true` schedules a rebuild but doesn't call the typed `did_change_dependencies` hook.

**Fix shape:** Wire `InheritedBehavior::on_view_updated` to call `did_change_dependencies` on each dependent's state. Requires a way to invoke the typed hook through the `ElementOwner` split-borrow — but the `state_as_any` accessor + `TypeId` check at the call site can resolve.

**Blast radius:** Minor. ~30 LOC in `element/behavior.rs`. Behavioral change: dependents get a typed lifecycle callback in addition to the rebuild. Tests in `tests/inherited_dependency.rs` would need to assert the new behavior.

---

#### V-20 [P3 DOC | LOW] `view/view.rs::ElementBase` has 40+ trait methods with default impls — large object-safe trait, hard to scan

**Evidence:**
- `crates/flui-view/src/view/view.rs:114-512`: `pub trait ElementBase` with 40+ methods including identity (`view_type_id`, `depth`, `slot`), lifecycle (`lifecycle`, `mounted`, `mount`, `unmount`, `activate`, `deactivate`), update (`update`, `mark_needs_build`, `rebuild`, `perform_build`, `did_change_dependencies`), child management (`visit_children`, `first_child`, `deactivate_child`), debug (`debug_description`), render-object access (`render_object_any`, `render_object_any_mut`, `child_element`, `attach_to_render_tree`, `render_object_shared`, `set_pipeline_owner_any`, `set_parent_render_id`), inherited-element protocol (`as_inherited`, `as_inherited_mut`), ancestor-finder protocol (`view_as_any`, `state_as_any`), render-object-finder (`render_id`), notification (`on_notification`).

**Why it's a problem:**
- Single-`dyn`-boundary trait with 40+ methods is a maintenance surface. Adding a method without a default impl is a breaking change for every existing implementer (currently only `Element<V, A, B>` and test stubs).
- Method discovery: rust-analyzer's hover shows 40+ candidates when typing `element.`; cognitive load.

**Fix shape:** Split `ElementBase` into sub-traits:
- `ElementIdentity` (view_type_id, depth, slot)
- `ElementLifecycle` (mount, unmount, activate, deactivate, lifecycle)
- `ElementBuild` (perform_build, mark_needs_build, rebuild)
- `ElementRenderObject` (render_object_*, attach_to_render_tree, etc — only for RenderElement subset)
- `ElementInherited` (as_inherited, as_inherited_mut — only for InheritedElement subset)
- `ElementNotification` (on_notification)

Then `ElementBase = ElementIdentity + ElementLifecycle + ElementBuild + ElementNotification` (the trait inheritance graph). The render-object + inherited subsets stay as optional supertraits implemented only by the relevant `Element<V, A, B>` instantiations.

**Blast radius:** Trait surface restructure. ~+100 LOC across `view/view.rs`. Object-safety preserved at all levels. Implementer trait bounds change (the unified `Element<V, A, B>` needs to implement multiple sub-traits explicitly).

Audit recommends deferring to a dedicated wave (cycle 6+). This finding is forward-looking observation.

---

#### V-21 [P3 STYLE | LOW] `binding.rs::handle_*` event-forwarding methods clone `Vec<Arc<dyn Observer>>` per call

**Evidence:**
- `crates/flui-view/src/binding.rs:858-887` and others: many event handlers follow the pattern:
  ```rust
  pub fn handle_metrics_changed(&self) {
      let inner = self.inner.read();
      for observer in &inner.observers {
          observer.did_change_metrics();
      }
  }
  ```
- async ones clone:
  ```rust
  pub async fn handle_pop_route(&self) -> bool {
      let observers: Vec<_> = self.inner.read().observers.clone();
      // ...
  }
  ```

**Why it's a problem:**
- For async (`handle_pop_route`, `handle_push_route`, `handle_request_app_exit`, `handle_commit_back_gesture`), the `Vec::clone()` clones each `Arc` (refcount bump per observer). Necessary because the lock can't be held across `.await`.
- For sync (`handle_metrics_changed`, `handle_locale_changed`, `handle_platform_brightness_changed`, etc.), the lock IS held across the loop. Observer callbacks are arbitrary code — if any observer takes the same lock, deadlock.

**Fix shape:** Snapshot-then-fire pattern (matches `SystemFontsNotifier::notify_listeners`):
```rust
pub fn handle_metrics_changed(&self) {
    let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
    for observer in &observers {
        observer.did_change_metrics();
    }
}
```
Refcount bump per observer per event. For 10 observers × 1 event/second = 10 RC bumps/sec — negligible cost. The deadlock-safety win outweighs the RC cost.

**Blast radius:** binding.rs only. ~10 sync handlers × 2 LOC each = ~20 LOC delta.

---

#### V-22 [P3 STYLE | LOW] `RenderSlot` is `pub` but only used internally for RenderObjectElement attach/detach

**Evidence:**
- `crates/flui-view/src/element/render_object_element.rs:27-35`:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
  pub enum RenderSlot {
      #[default]
      Single,
      Index(usize),
      Named(String),
  }
  ```
- Used by `RenderObjectElement::insert_render_object_child(slot)`, `attach_render_object(slot)`, etc.
- Workspace consumer check:
  ```bash
  $ rg "RenderSlot::|RenderSlot\b" crates --type rust | grep -v "flui-view/src"
  # 0 external usage — only internal
  ```

**Why it's a problem:**
- Public enum, no `#[non_exhaustive]`, internal-use-only. Same shape as V-17.

**Fix shape:** Either `pub(crate)` or `#[non_exhaustive]`. Audit recommends `#[non_exhaustive]` to preserve forward-compatibility (a `Named(Arc<str>)` variant for cheaper clones is a likely future addition).

**Blast radius:** Minimal. ~1 LOC.

---

#### V-23 [P3 STYLE | LOW] `binding.rs::WidgetsBindingInner` holds `pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>` and `element_tree: ElementTree` side-by-side

**Evidence:**
- `crates/flui-view/src/binding.rs:` (binding inner struct, ~line 400): combines `build_owner`, `element_tree`, `root_element`, `observers`, `pipeline_owner`, `build_scheduled`, `back_gesture_observers`, etc. into a single inner struct guarded by `Arc<RwLock<WidgetsBindingInner>>`.
- Every read of any field requires reading the outer RwLock.

**Why it's a problem:**
- Coarse-grained locking: looking up `root_element` (a Copy `Option<ElementId>`) requires acquiring the same lock as a complex mutation of `element_tree`.
- Cycle 4 R-6 (nested-lock smell in RendererBinding) is the same pattern. Cycle 5 should propose the lookup-method per-field pattern.

**Fix shape:** Split `WidgetsBindingInner` into independently-locked fields:
```rust
pub struct WidgetsBinding {
    build_owner: RwLock<BuildOwner>,
    element_tree: RwLock<ElementTree>,
    root_element: AtomicU64, // or RwLock<Option<ElementId>>
    pipeline_owner: RwLock<Option<Arc<RwLock<PipelineOwner>>>>,
    observers: RwLock<Vec<Arc<dyn WidgetsBindingObserver>>>,
    // ... etc, each field independently locked
}
```
This is the cycle 4 R-6 fix applied at the WidgetsBinding level. Major restructure.

**Blast radius:** Cross-cutting in binding.rs. ~+200 LOC of API surface change. Defer to dedicated wave.

---

#### V-24 [P3 DEAD-CODE | LOW] `WidgetsBindingObserver::handle_*_back_gesture` family — 4 trait methods + `back_gesture_observers` storage

**Evidence:**
- `crates/flui-view/src/binding.rs:265-283`: `handle_start_back_gesture(event) -> bool`, `handle_update_back_gesture_progress(event)`, `handle_commit_back_gesture()`, `handle_cancel_back_gesture()`.
- `crates/flui-view/src/binding.rs:1041-1094`: `WidgetsBinding::handle_*_back_gesture` impls.
- The feature is Android-13+ predictive-back-gesture support.
- Workspace consumer check: no `impl WidgetsBindingObserver` in production with predictive-back override.

**Why it's a problem:**
- 80+ LOC of platform-specific (Android) infrastructure for a feature that no in-workspace consumer uses.
- The platform integration would route through `flui-platform`, but `flui-platform` doesn't have a working predictive-back path either.

**Fix shape:** Feature-gate `android-predictive-back`, default off, until `flui-platform` wires the Android side. OR keep as forward-looking with a `// REMOVE_BY: 2026-09-22` cadence.

Audit recommends cadence marker.

**Blast radius:** Documentation only.

---

#### V-25 [P3 DOC | LOW] `view/view.rs::View::can_update` default impl reads `self.view_type_id() == old.view_type_id()` — keys not consulted

**Evidence:**
- `crates/flui-view/src/view/view.rs:80-84`:
  ```rust
  fn can_update(&self, old: &dyn View) -> bool {
      self.view_type_id() == old.view_type_id()
  }
  ```
- The doc-comment at line 78 says *"Override this to add additional constraints (e.g., key matching)."*

**Why it's a problem:**
- Flutter's `Widget.canUpdate` compares both `runtimeType` AND `Key`. The default FLUI shape compares only type, not keys.
- A `View::key()` returning a `Some(key)` won't influence `can_update` by default — callers must override.
- This is the subtle correctness issue that breaks GlobalKey reparenting in the index-based reconciliation (V-2). Even when reconciliation is fixed (V-2), the `can_update` default needs to consider keys.

**Flutter reference:** `framework.dart:1064-1075` `Widget.canUpdate(old, new)`: `return old.runtimeType == new.runtimeType && old.key == new.key`.

**Fix shape:** Update default to compare keys too:
```rust
fn can_update(&self, old: &dyn View) -> bool {
    if self.view_type_id() != old.view_type_id() {
        return false;
    }
    match (self.key(), old.key()) {
        (None, None) => true,
        (Some(s), Some(o)) => s.key_eq(o),
        _ => false, // one has a key, the other doesn't — can't update
    }
}
```

**Blast radius:** view.rs only. ~10 LOC. Tests in `tests/reconciliation_tests.rs` may break — they assume the old default.

---

## Part III — Flutter drift catalog

This catalog enumerates points where FLUI's painting / view layer diverges from its Flutter reference. For each: cite Flutter source (path:line per `.flutter/flutter-master/packages/flutter/lib/src/`), classify as intentional improvement vs gap-to-bridge, severity tag.

### Drift A — `View::can_update` default ignores `key()` [P0 — GAP]

- **Flutter:** `framework.dart:1064-1075` — `Widget.canUpdate(old, new)` compares `runtimeType` AND `key`. Both must match for in-place update.
- **FLUI:** `view/view.rs:80-84` — default impl compares only `view_type_id()`. Key comparison only happens if a consumer overrides `can_update`.
- **Verdict:** GAP. Breaks state preservation on keyed widget moves.
- **Resolves via:** **V-25** (P3). Bundle with **V-2** (P0 reconciliation fix) for full keyed-reconciliation correctness.

### Drift B — `InheritedElement` dependent map uses `HashMap<ElementId, usize>` (depth) [INTENTIONAL — improvement-ish]

- **Flutter:** `framework.dart:6252` — `_dependents: HashMap<Element, Object?>`. The `Object?` value slot carries dependency aspects (used by `InheritedModel` for selective dependents).
- **FLUI:** `element/behavior.rs:578-598` — `pub dependents: HashMap<ElementId, usize>`. The `usize` is the dependent's depth (captured at `depend_on_inherited` time) so `schedule_build_for(id, depth)` can be O(1) at notify time.
- **Verdict:** INTENTIONAL. FLUI's choice trades the (unused-by-most-widgets) aspect slot for an optimization (depth caching). Flutter pays an extra tree walk on notify to recover depth.
- **Documentation:** the inline comment at `element/behavior.rs:564-575` calls out the divergence.
- **Future gap:** `InheritedModel` (Flutter `inherited_model.dart`) needs the aspect slot. When FLUI ports `InheritedModel`, widen the map to `HashMap<ElementId, DependentDescriptor { depth, aspect: Option<Box<dyn Any>> }>`.

### Drift C — `BuildOwner::inherited_elements` registry exists but never populated [P0 — GAP]

- **Flutter:** `framework.dart:5028-5060` — `_inheritedElements` is per-element (`Element._inheritedElements: PersistentHashMap`). Each child inherits its parent's map, adds itself if it's an `InheritedElement`. Lookup is `O(1)` on this map.
- **FLUI:** `owner/build_owner.rs:103-105` — single global `HashMap<TypeId, ElementId>` registry on `BuildOwner`. **Never populated by production**; the `depend_on_inherited` walk falls back to `O(depth)`.
- **Verdict:** GAP. FLUI's design choice (centralized vs distributed) is intentional and Rust-idiomatic. Wiring the registration is what's missing.
- **Resolves via:** **V-1** (P0). Wire `InheritedBehavior::on_mount` / `on_unmount` to populate.

### Drift D — `VariableChildStorage::update_with_views` uses index-based matching [P0 — GAP]

- **Flutter:** `framework.dart:5836-5946` — `Element._updateChildren` implements 5-phase keyed O(N) linear reconciliation. The canonical port lives at `flui-view/src/tree/reconciliation.rs::reconcile_children` — but production doesn't use it.
- **FLUI:** `element/child_storage.rs:494-515` — index-based match.
- **Verdict:** GAP. Cycle 5 V-2 (P0) hoists the algorithm into production.

### Drift E — `PaintingContext` class doesn't exist in FLUI [INTENTIONAL]

- **Flutter:** `painting/clip.dart` defines `ClipContext` abstract class; `rendering/object.dart::PaintingContext extends ClipContext`. `PaintingContext` is the per-paint-call context passed to `RenderObject.paint`.
- **FLUI:** `flui-rendering/src/context/canvas.rs::CanvasContext` is the production implementer of `ClipContext` (per the docstring at `flui-painting/src/clip_context.rs:13`). No `PaintingContext` type.
- **Verdict:** INTENTIONAL. The FLUI port collapses Flutter's `PaintingContext` into `CanvasContext` (which holds the canvas + paint-bounds + clip-stack). Reduces type proliferation. Acceptable Rust idiom.

### Drift F — Two `TextLayout` structs (cosmic-text vs fallback) gated by feature [P0 — GAP]

- **Flutter:** `painting/text_painter.dart` — single `TextPainter` class, single text-shaping path through Skia's `ParagraphBuilder`. No "fallback" for `text` feature off.
- **FLUI:** `text_layout/{layout,fallback}.rs` — two parallel `TextLayout` structs gated by `cfg(feature = "text")`. Both are unused by production (the canonical path goes Canvas → DrawCommand → engine).
- **Verdict:** GAP. The fallback exists to support `--no-default-features` users, but no such user exists.
- **Resolves via:** **P-3** (P0): delete the fallback. **P-5** (P1): feature-gate `TextPainter` until production needs it.

### Drift G — Canvas state-stack `ClipShape` enum carries clip data nothing reads [P1 — GAP]

- **Flutter:** `dart:ui Canvas.save()` doesn't track the clip stack at the API surface — Skia owns the clip stack internally.
- **FLUI:** `canvas/state.rs:36-56` — `ClipShape` enum stores clip geometry. The data is captured but read by nothing (the `Canvas::save_count` query reads the stack length, not variant data).
- **Verdict:** GAP. The "future optimization features" promised by the doc-comment (culling, clip-bounds queries) don't exist.
- **Resolves via:** **P-8** (P1): replace with depth-counter; reintroduce variant data when a consumer materializes.

### Drift H — `PointerEvent` defined in painting AND interaction [INTENTIONAL — being-resolved-via-P-2]

- **Flutter:** `gestures/events.dart::PointerEvent` is the single canonical type. Used by `gestures/` + `rendering/` + `widgets/`.
- **FLUI:** Two `PointerEvent` structs: `flui_painting::display_list::hit_region::PointerEvent` (minimal, for hit-region handlers that never fire) + `flui_interaction::pointer::PointerEvent` (canonical, used by the event pump).
- **Verdict:** GAP — being resolved via cycle-5 P-2.

### Drift I — `BuildContext` is a separate trait, not a method on `Element` [INTENTIONAL]

- **Flutter:** `framework.dart::Element implements BuildContext` — the element IS the build context.
- **FLUI:** `context/element_build_context.rs::ElementBuildContext` is a separate struct constructed per-call.
- **Verdict:** INTENTIONAL. Rust's borrow checker forbids the mutual-self-reference Flutter's pattern needs. The split-borrow is the canonical Rust port (matches `Element._owner` → `ElementOwner<'_>` pattern from PR #84).

### Drift J — `Element::activate` / `deactivate` lifecycle hooks on `ElementBase` carry no `BuildContext` [P2 — minor gap]

- **Flutter:** `Element.activate()` and `Element.deactivate()` operate as a method on `this`, which IS the build context. Inside `activate`/`deactivate` body, `this.dependOnInheritedWidgetOfExactType` is callable.
- **FLUI:** `ElementBase::activate(&mut self)` / `deactivate(&mut self)` take no `&mut crate::ElementOwner<'_>` (verified at [`view/view.rs:181`](../../crates/flui-view/src/view/view.rs)). So a behavior implementing `on_activate` can't reach the build owner.
- **Verdict:** GAP — minor. The four widget-side behaviors that override these (`StatefulBehavior`, `AnimationBehavior`, `InheritedBehavior` if needed) currently don't need owner access. But future scenarios (StatefulBehavior wanting to re-register dependents on activate) would.
- **Severity:** P3. Defer until a consumer needs it.

### Drift K — `flui_app::theme::colors::Color` parallel to `flui_types::Color` [P3 — GAP, cross-crate]

- **Flutter:** single `Color` class in `dart:ui`. Used universally.
- **FLUI:** `flui_types::Color` is the canonical packed-u32 ARGB; `flui_app::theme::colors::Color` is a parallel `f32`-per-channel struct.
- **Verdict:** GAP. The flui-app version has zero consumers (verified via rg). Delete.
- **Resolves via:** **V-14** (P3). Cross-cycle finding; the audit notes it but defers.

### Drift L — `RootRenderView` + `RootRenderElement` infrastructure exists but is bypassed [P0 — GAP]

- **Flutter:** `binding.dart::RendererBinding.renderViews` is a `List<RenderView>`. The root is a `_RawViewElement` (Flutter `framework.dart:6420+`) which carries the pipeline owner.
- **FLUI:** `view/root.rs::RootRenderView<V>` + `RootRenderElement<V>` provide the same shape (577 LOC). But `WidgetsBinding::attach_root_widget` ([`binding.rs:596-632`](../../crates/flui-view/src/binding.rs)) bypasses them — direct-mounts the user view via `element_tree.mount_root_with_pipeline_owner`.
- **Verdict:** GAP. Either wire `attach_root_widget` to construct `RootRenderView`, OR delete the unused infrastructure.
- **Resolves via:** **V-6** (P0).

### Drift M — `WidgetsBindingObserver::handle_*_back_gesture` family unwired [P3 — GAP, platform-specific]

- **Flutter:** `binding.dart::WidgetsBinding.handlePredictiveBackGesture` routes Android 13+ predictive-back events through observers.
- **FLUI:** Same trait surface exists (`binding.rs:265-283`) + `WidgetsBinding::handle_*_back_gesture` impls — but no platform-side wiring in `flui-platform`. Observers register, no events fire.
- **Verdict:** GAP — platform-specific, forward-looking. Defer.
- **Resolves via:** **V-24** (P3).

### Drift N — Error recovery: `ErrorView` exists but `Element::perform_build` doesn't `catch_unwind` [P0 — GAP]

- **Flutter:** `framework.dart:5048-5108` — `Element.performRebuild()` catches user `build()` panics and substitutes `ErrorWidget.builder(FlutterError)`. Error recovery is built into the framework.
- **FLUI:** `view/error.rs::ErrorView` + `FlutterError` + `set_error_view_builder` exist (333 LOC) — but `Element::perform_build` does NOT wrap the user `view.build()` call in `std::panic::catch_unwind`. A user panic propagates up, aborts the process.
- **Verdict:** GAP. Half-impl: receiver exists, producer doesn't.
- **Resolves via:** **V-5** (P0). Either wire the catch_unwind, OR feature-gate the `ErrorView` until wired.

### Drift O — `BoxedView` re-clones via `dyn_clone::clone_box` per `Box::clone` [INTENTIONAL]

- **Flutter:** Widgets are immutable Dart objects; no clone needed (canonical `==` operator + GC).
- **FLUI:** `view/into_view.rs:142-160` — `BoxedView` wraps `Box<dyn View>`, and `Clone` impl calls `dyn_clone::clone_box(&*self.0)` (Box<dyn View> → fresh Box<dyn View>).
- **Verdict:** INTENTIONAL. `dyn_clone` is the canonical Rust pattern for cloning trait objects. Cost per clone is acceptable for the type-erasure benefit.

---

## Part IV — Final combined priority order

Severity legend: P0 = critical correctness / Constitution-violation / cycle-X-parity-essential; P1 = high-impact API or hot-path; P2 = medium-impact hygiene; P3 = low-priority cleanup.

| # | Crate | Finding | Severity | Size (LOC) | Depends on | Notes |
|---|---|---|---|---|---|---|
| **P0 — Critical correctness (must land first; Constitution-violation + cycle-1/2/3/4 parity)** | | | | | | |
| 1 | flui-view | V-1: Wire `InheritedBehavior::on_mount` to populate `BuildOwner::inherited_elements` registry (O(depth) → O(1) for `depend_on_inherited`) | P0 | ±80 | None | Half-impl that silently bypasses an O(1) optimization. **Most important** view finding |
| 2 | flui-view | V-2: Hoist `reconcile_children` into `VariableChildStorage::update_with_views` (keyed reconciliation) | P0 | ±200-400 | Possibly V-7 (TreeWrite trait) | Currently index-based; enables `Hero` / `Reorderable` / `GlobalKey` reparenting. Architectural — store-by-id vs store-by-value resolution |
| 3 | flui-painting | P-1: Delete `tessellation` module + drop from default features + drop Lyon dep | P0 | −537 LOC + −200 LOC deps | None | 2026-05-20 audit's 18-month-old finding; engine owns tessellation |
| 4 | flui-painting | P-2: Delete `display_list::hit_region` (parallel `PointerEvent` + dead `add_hit_region`) | P0 | −101 LOC + −40 LOC tests | None | Cycle 4 R-7/R-8/R-9 pattern; parallel-type drift |
| 5 | flui-painting | P-3: Delete `text_layout::fallback::TextLayout` (parallel impl; default feature ON means it's unreachable in default build) | P0 | −257 | None | OR delete both impls per option 3 |
| 6 | flui-painting | P-4: Feature-gate `canvas::sugar` (`canvas-sugar`, default off; 30+ methods, zero consumers) | P0 | cfg-gate ~720 | None | 720 LOC behind cfg |
| 7 | flui-view | V-3: Feature-gate `view::animated` + `AnimationBehavior` (`animated-views`, default off) | P0 | cfg-gate ~424 | None | Zero production impls; `AnimationBehavior` collides with `flui_animation::AnimationBehavior` |
| 8 | flui-view | V-4: Feature-gate `view::parent_data` (`parent-data-views`, default off) | P0 | cfg-gate ~479 | None | Zero production impls |
| 9 | flui-view | V-5: Feature-gate `view::error` (`error-view`, default off) until `catch_unwind` wraps `perform_build` | P0 | cfg-gate ~333 | None | Receiver exists, producer doesn't (Drift N) |
| 10 | flui-view | V-6: Either wire `attach_root_widget` through `RootRenderView`, OR feature-gate `view::root` | P0 | ±50 or cfg-gate ~577 | None | Forward-looking infrastructure; canonical bootstrap path decision |
| 11 | flui-view | V-7: Implement `TreeRead<ElementId> + TreeNav<ElementId> + TreeWrite<ElementId>` for `ElementTree` | P0 | +200 | None | DAG-uniform with `LayerTree` / `RenderTree`; cascade-by-default removal |
| 12 | flui-view | V-8: Bundle with P-2 (parallel `PointerEvent` removed) | P0 | bundled | P-2 | Cross-crate consolidation |
| **P1 — High-impact (next wave)** | | | | | | |
| 13 | flui-painting | P-5: Feature-gate `text_painter` (`text-painter`, default off; zero production consumers) | P1 | cfg-gate ~751 | None | OR delete entirely |
| 14 | flui-painting | P-6: `Canvas::draw_polyline` `windows(2)` idiom (clippy `needless_range_loop`) | P1 | ±5 | None | Style |
| 15 | flui-painting | P-7: `Arc<Paint>` interning to eliminate per-draw paint clone | P1 | ±200 cross-cutting | None | Hot path; defer to dedicated wave |
| 16 | flui-painting | P-8: Replace `ClipShape` enum with depth counter | P1 | −30 | None | Forward-looking optimization data, unused |
| 17 | flui-painting | P-9: Delete `Picture` type alias | P1 | −3 | None | Cosmetic |
| 18 | flui-view | V-9: Delete `NotificationNode` + `NotificationHandler` + `BoxedNotification` + `NotificationCallback` (parallel-tree dispatch unused) | P1 | −152 | None | Active path is the unified protocol |
| 19 | flui-view | V-10: Delete `SharedWidgetsBinding` + `create_shared_binding` (deprecated since 0.2.0) | P1 | −10 | None | Deprecation cleanup |
| 20 | flui-view | V-11: Mark `ReconcileAction` with `// REMOVE_BY: 2026-09-22` cadence (or delete) | P1 | ±5 | None | Bundle with V-2 |
| 21 | flui-view | V-12: Convert `attach_root_widget` panic to `Result<(), AttachError>` | P1 | ±30 | None | Constitution Principle 6 spirit |
| 22 | flui-view | V-13: Cache a single dummy `ElementBuildContext` in `BuildOwner` to eliminate per-build Arc allocations | P1 | ±30 | None | Hot path; option 1 (real ctx threading) is a follow-up wave |
| **P2 — Medium-impact hygiene** | | | | | | |
| 23 | flui-painting | P-10: Demote `SystemFontsNotifier` to `pub(crate)` until platform integration exists | P2 | visibility ±5 | None | API trim |
| 24 | flui-painting | P-11: Optimize `append_display_list_at_offset` — offset as transform, not bake | P2 | ±50 | None | Hot path; bench before/after |
| 25 | flui-painting | P-12: Document re-export shape of `Paint`/`Path`/etc from `flui_types::painting` | P2 | doc ±10 | None | Diagnostic clarity |
| 26 | flui-painting | P-13: Optimize `DrawCommand::kind()` to constant byte read via `#[repr(u8)]` | P2 | ±30 | None | Hot path |
| 27 | flui-view | V-14: (cross-crate) Delete `flui_app::theme::colors::{Color, ColorScheme}` | P3* | −150 | None | *Out-of-cycle-5-strict-scope but called out |
| 28 | flui-view | V-15: Delete `DirtyElement::depth()` + `InactiveElement::depth()` accessors (`#[allow(dead_code)]`) | P2 | −15 | None | Cleanup |
| 29 | flui-view | V-16: Iterative pre-allocated `collect_all_elements` (O(N²) → O(N)) | P2 | ±20 | None | Hot path for `schedule_root_rebuild` |
| 30 | flui-view | V-17: `#[non_exhaustive]` on `Lifecycle` enum | P2 | ±1 | None | API discipline |
| 31 | flui-view | V-18: None (`Box<dyn Fn>` is correct here; finding for completeness) | P2 | none | None | Reference |
| 32 | flui-view | V-19: Wire `did_change_dependencies` lifecycle hook through `InheritedBehavior::on_view_updated` | P2 | ±30 | V-1 | Flutter parity |
| **P3 — Low-priority cleanup** | | | | | | |
| 33 | flui-painting | P-14: Bundle with P-4 (batch sugar methods) | P3 | bundled | P-4 | — |
| 34 | flui-painting | P-15: Bundle with P-4 (`Canvas::record` / `build` static constructors) | P3 | bundled | P-4 | — |
| 35 | flui-painting | P-16: Optional rename `save_count` → `save_depth` (defer; doc clarification) | P3 | doc only | None | Defer |
| 36 | flui-painting | P-17: Add `// REMOVE_BY: 2026-09-22` to `FONT_SYSTEM` global cosmic-text panic-poisoning trade-off | P3 | doc ±5 | None | Cadence |
| 37 | flui-painting | P-18: Add `// REMOVE_BY:` cadence to `ARCHITECTURE.md`/`PERFORMANCE.md` outstanding refactors | P3 | doc only | None | Cadence |
| 38 | flui-painting | P-19: Bundle with P-2 (`DisplayListStats::hit_regions` field becomes dead after P-2) | P3 | bundled | P-2 | — |
| 39 | flui-painting | P-20: None (`#![forbid(unsafe_code)]` is correct; finding for completeness) | P3 | none | None | Reference |
| 40 | flui-view | V-20: Defer split of `ElementBase` into sub-traits (cycle 6+) | P3 | future wave | None | Defer |
| 41 | flui-view | V-21: Snapshot-then-fire pattern on `WidgetsBinding::handle_*` event handlers | P3 | ±20 | None | Style + deadlock-safety |
| 42 | flui-view | V-22: `#[non_exhaustive]` on `RenderSlot` enum | P3 | ±1 | None | API discipline |
| 43 | flui-view | V-23: Defer split of `WidgetsBindingInner` into per-field locks (cycle 6+) | P3 | future wave | None | Defer |
| 44 | flui-view | V-24: Add `// REMOVE_BY: 2026-09-22` cadence to predictive-back-gesture infrastructure | P3 | doc ±5 | None | Cadence |
| 45 | flui-view | V-25: `View::can_update` default impl reads keys (bundles with V-2) | P3 | ±10 | V-2 | Bundle |

**Total LOC delta** (estimated): **~−3,600 LOC deletion + feature-gate** (across `tessellation`, `hit_region`, fallback `TextLayout`, sugar, `text_painter`, `animated`, `parent_data`, `error`, `root`, `NotificationNode`, deprecated binding) **+ ~+500 LOC** (V-1 registry wire, V-2 reconciliation hoist, V-6 root binding wire, V-7 TreeWrite impls, V-12 Result conversion). Net: **~−3,100 LOC reduction** in public surface; ~14% of crate surface trimmed.

**Cycle alignment with predecessors:**
- Total scope (22,161 LOC, 45 findings) < cycle 4 (45,365 LOC, 45 findings) ≈ cycle 3 (23,448 LOC, 47 findings) > cycle 2 (15,571 LOC, 25 findings).
- Total `unimplemented!()` violations: **0** (verified — neither crate has them). Constitution Principle 6 clean on the macro side.
- Total `assert!`-panics in production paths: **1** (V-12: `attach_root_widget`). One spirit-of-Principle-6 violation.
- Total parallel-type pairs across the workspace boundary: **2** (`PointerEvent` painting × interaction, `Color` flui-types × flui-app). Lowest count of any cycle.
- Total zombie LOC (verified zero external consumer): **~2,620** in flui-painting + **~1,965** in flui-view ≈ **~4,585 LOC**. Comparable to cycle 4's ~4,600 in engine + rendering.

**Cycle 5 closing**: this is cycle 5 of the audit-execute series. The next cycle (cycle 6) would pick up `flui-interaction` + `flui-scheduler` for the second round, OR `flui-platform` + `flui-app` if platform integration has matured.

---

## Appendix A — Investigation receipts

### A.1 — Project shape

```bash
$ find crates/flui-painting/src -name "*.rs" -type f | wc -l
32

$ find crates/flui-view/src -name "*.rs" -type f | wc -l
44

$ wc -l crates/flui-painting/src/**/*.rs | tail -1
8341 total

$ wc -l crates/flui-view/src/**/*.rs | tail -1
13820 total

$ find crates/flui-painting/src crates/flui-view/src -name "*.rs" -exec wc -l {} \; | sort -rn | head -10
1328 crates/flui-view/src/binding.rs
 886 crates/flui-view/src/element/behavior.rs
 872 crates/flui-painting/src/display_list/command_ops.rs
 812 crates/flui-view/src/context/element_build_context.rs
 697 crates/flui-view/src/element/generic.rs
 688 crates/flui-view/src/tree/element_tree.rs
 657 crates/flui-view/src/owner/build_owner.rs
 627 crates/flui-view/src/element/child_storage.rs
 577 crates/flui-view/src/view/root.rs
 552 crates/flui-painting/src/binding.rs
```

### A.2 — Zero-consumer module verification (workspace ripgrep)

```bash
# tessellation (P-1) — 537 LOC, zero production consumers
$ rg "use flui_painting::tessellation|flui_painting::tessellate" crates --type rust
crates/flui-painting/tests/tessellation_integration.rs:5
crates/flui-painting/src/tessellation.rs:22 (own doc-comment)
# Production hits: 0

# hit_region (P-2) — 101 LOC, only test + own definition
$ rg "HitRegion::new|add_hit_region|HitRegionHandler" crates --type rust
crates/flui-painting/tests/canvas_unit.rs:182-196 (test only)
crates/flui-painting/src/display_list/hit_region.rs:* (definition)
crates/flui-painting/src/canvas/mod.rs:236 (Canvas method, test-only consumer)

# TextLayout fallback (P-3) — 257 LOC, test-only
$ rg "use flui_painting::TextLayout|TextLayout::new" crates --type rust
crates/flui-painting/tests/text_layout_unit.rs:225,250 (test)
crates/flui-painting/tests/text_layout_fallback.rs:154,179 (test)
# Production hits: 0

# canvas/sugar (P-4) — 720 LOC, zero production consumers
$ rg "draw_pill|draw_ring|draw_rect_if|draw_unless|repeat_x|debug_rect|debug_axes" crates --type rust
crates/flui-painting/docs/* (doc-only)
crates/flui-painting/README.md (doc-only)
crates/flui-painting/src/canvas/sugar/* (definition)
# 0 callers outside the sugar module itself

# TextPainter (P-5) — 751 LOC, test-only
$ rg "use flui_painting::TextPainter|TextPainter::new" crates --type rust | grep -v "/tests/\|/README"
# 0 hits

# AnimatedView (V-3) — 424 LOC, test-only
$ rg "impl AnimatedView|use flui_view::AnimatedView" crates --type rust
crates/flui-view/README.md:163 (doc)
crates/flui-view/src/view/animated.rs:206 (test impl)
# Production hits: 0

# ParentDataView (V-4) — 479 LOC, test-only
$ rg "impl ParentDataView" crates --type rust
crates/flui-view/src/view/parent_data.rs:378 (test impl)
# Production hits: 0

# ErrorView / FlutterError (V-5) — 333 LOC, test-only
$ rg "use flui_view::ErrorView|ErrorView::new|FlutterError::new|set_error_view_builder" crates --type rust | grep -v "/tests/\|view/error.rs\|view/mod.rs:34\|lib.rs:198"
# 0 hits

# RootRenderView / RootRenderElement (V-6) — 577 LOC, test-only
$ rg "RootRenderView::|RootRenderElement::" crates --type rust
crates/flui-view/src/view/root.rs:* (definition)
# 0 callers outside view/root.rs

# NotificationNode / NotificationHandler (V-9) — 152 LOC, definition-only
$ rg "NotificationNode::|NotificationHandler\b" crates --type rust | grep -v "element/notification.rs\|view/mod.rs"
# 0 hits

# SharedWidgetsBinding (V-10) — 10 LOC, deprecated, zero callers
$ rg "SharedWidgetsBinding|create_shared_binding" crates --type rust
crates/flui-view/src/binding.rs:* (definition)
# 0 production callers

# BuildOwner::inherited_elements registry (V-1) — populated only by tests
$ rg "register_inherited|unregister_inherited" crates --type rust | grep -v "/tests/\|owner/build_owner.rs:[0-9]+:\s*\(pub\|//\|self\.\|fn"
# Only fn-definitions and test callers.

# VariableChildStorage::update_with_views index-based loop vs reconcile_children
$ rg "reconcile_children\(" crates --type rust
crates/flui-view/src/tree/reconciliation.rs:* (definition + tests)
# 0 production callers

# Two AnimationBehavior types (V-3)
$ rg "^pub (struct|enum) AnimationBehavior\b" crates --type rust
crates/flui-animation/src/status.rs:* (enum: Normal | Preserve)
crates/flui-view/src/element/behavior.rs:759 (struct: stateful + listener_id)
# Same name, different concepts

# flui-app::theme::Color (V-14, cross-crate)
$ rg "use flui_app::theme::Color" crates --type rust
# 0 hits

# Module-level #[allow(dead_code)] markers
$ rg "^#\[allow\(dead_code\)\]" crates/flui-painting/src crates/flui-view/src --type rust
crates/flui-painting/src/canvas/state.rs:46
crates/flui-view/src/tree/reconciliation.rs:17
# 2 module/item-level suppressions
```

### A.3 — Parallel-type drift verification

```bash
# PointerEvent (V-8 / P-2) — 2 definitions
$ rg "^pub struct PointerEvent\b" crates --type rust
crates/flui-painting/src/display_list/hit_region.rs:19:pub struct PointerEvent {
# Plus the canonical one at flui-interaction/src/pointer/event.rs (verified via separate search)

# TextLayout (P-3) — 2 definitions in same crate
$ rg "^pub struct TextLayout\b" crates --type rust
crates/flui-painting/src/text_layout/layout.rs:58:pub struct TextLayout (cosmic-text)
crates/flui-painting/src/text_layout/fallback.rs:23:pub struct TextLayout (fallback)
# 2 hits in same crate, gated by feature

# Color (V-14, K) — 2 definitions, cross-crate
$ rg "^pub struct Color\b" crates --type rust
crates/flui-app/src/theme/colors.rs:5:pub struct Color (f32 channels)
crates/flui-types/src/styling/color.rs:8:pub struct Color (packed u32)

# AnimationBehavior (V-3) — 2 definitions, different concepts
$ rg "^pub (struct|enum) AnimationBehavior\b" crates --type rust
crates/flui-animation/src/status.rs:* (enum)
crates/flui-view/src/element/behavior.rs:759 (struct)

# ParentData (cycle 4 R-11, prefactor in PR #84) — 2 definitions, but renamed
$ rg "^pub trait ParentData\b" crates --type rust
crates/flui-rendering/src/parent_data/base.rs:* (storage trait)
# flui_view::ParentDataConfig renamed (was ParentData)
$ rg "^pub trait ParentDataConfig\b" crates --type rust
crates/flui-view/src/view/parent_data.rs:90 (config marker trait)
# Cycle 4 closed the collision
```

### A.4 — Constitution Principle 6 sweep

```bash
$ rg "unimplemented!\(\)|todo!\(\)|panic!\(\"not impl" crates/flui-painting crates/flui-view --type rust
# 0 hits — Constitution Principle 6 clean on macros

$ rg "\.unwrap\(\)" crates/flui-painting/src crates/flui-view/src --type rust | grep -v "#\[cfg(test)\]\|//\|/// "
# Verified hits — all in tests or doc-comments:
crates/flui-painting/src/tessellation.rs:* (in tests)
crates/flui-painting/src/binding.rs:* (in tests)
crates/flui-view/src/context/element_build_context.rs:* (in tests)
crates/flui-view/src/owner/build_owner.rs:* (in tests)
crates/flui-view/src/view/render.rs:* (in tests)
crates/flui-view/src/view/parent_data.rs:* (in tests + doc-example at line 68)
crates/flui-view/src/view/inherited.rs:* (in doc-example)
crates/flui-view/src/tree/element_tree.rs:* (in tests)
# Production: 0 .unwrap() hits

$ rg "println!|eprintln!|dbg!" crates/flui-painting/src crates/flui-view/src --type rust
crates/flui-painting/src/tessellation.rs:* (in tests + doc-comments)
crates/flui-painting/src/lib.rs:57 (doc-example)
crates/flui-view/src/binding.rs:210 (doc-example)
# Production: 0 hits

$ rg "assert!" crates/flui-view/src/binding.rs --type rust
crates/flui-view/src/binding.rs:599 (V-12: production-path panic on double-attach)
# 1 production-path assert that's the V-12 Principle-6-spirit finding
```

### A.5 — Closed findings from 2026-05-20 audit

```bash
# Closed: Two ClipContext traits (PR #82)
$ rg "pub trait ClipContext" crates --type rust
crates/flui-painting/src/clip_context.rs:74 (canonical home)
# 1 hit only — flui-rendering's was deleted

# Closed: ParentData / ParentDataConfig naming collision (PR #84 prefactor for cycle 4 R-11)
$ rg "^pub trait ParentDataConfig\b" crates --type rust
crates/flui-view/src/view/parent_data.rs:90
# View-side renamed; flui_rendering::ParentData is the storage trait

# Closed: BuildContext + GlobalKey + Type-Collision Cleanup (PR #84 — U1-U17 chain)
# Multiple closures verified across the framework spine repair.
```

---

## Re-baseline against 2026-05-20 audit

The pre-existing `2026-05-20-mythos-audit-render-paint-layer-engine.md` carried 13 findings across 4 crates; cycle 5 covers 2 of those 4 (painting + view portion of the audit).

| Prior finding | Status at HEAD `eb95c2f2` (cycle 5 opener) | Cycle 5 verdict |
|---|---|---|
| Two `ClipContext` traits (painting + rendering) | **CLOSED** PR #82 | — |
| Painting tessellation duplication with engine | **OPEN** | **P-1 deletes** the painting tessellation module entirely (zero production consumers; engine is canonical) |
| Painting tessellation feature-gated but unused in production | **OPEN** | **P-1** above |
| `flui_painting::ClipContext` doc-block lying about `PaintingContext` impl | **PARTIAL** (doc updated post-PR #82 to call out `CanvasContext` is the production impl) | Acceptable as-is |

**Findings the 2026-05-20 audit missed (added in cycle 5):**

1. **`BuildOwner::inherited_elements` registry never populated by production** (V-1, P0) — the 2026-05-20 audit was before PR #84's framework-spine-repair landed; the registry was added there but never wired.
2. **`VariableChildStorage::update_with_views` ignores keys; `reconcile_children` algorithm is dead** (V-2, P0) — the 2026-05-20 audit didn't review the view-side reconciliation.
3. **`hit_region` parallel `PointerEvent` surface** (P-2, P0) — the 2026-05-20 audit didn't catch the parallel-type drift with `flui-interaction`.
4. **Three test-only zero-consumer surface modules** (V-3 `animated`, V-4 `parent_data`, V-5 `error`) totaling ~1,236 LOC — added in PR #84.
5. **`RootRenderView` infrastructure exists but bypassed** (V-6) — added in PR #84.
6. **`ElementTree` doesn't implement `TreeRead`/`TreeNav`/`TreeWrite`** (V-7) — cycle 2/3's tree-trait pattern not propagated to flui-view.
7. **`TextPainter` zero production consumers** (P-5) — the 2026-05-20 audit didn't review text-painter (newer addition).
8. **`canvas::sugar` 720 LOC zero consumers** (P-4) — added in PR #81's U4 fixup pass.

---

## Status (open)

This is an audit document. No changes applied. The Priority Order in Part IV is the plan; downstream branches will land changes in atomic-commit-per-finding shape matching PR #81/#82/#83/#84/#100/#103/#117 precedent.

**Next branch**: `feat/painting-view-cycle5-wave1` for the P0 critical-correctness items (V-1, V-2, P-1, P-2, P-3, P-4 + V-3 + V-4 + V-5). Estimated wave size: ~12 commits, ~−2,500 / +400 LOC delta.

**Cycle 5 closing**: this is cycle 5 of the audit-execute series. The next cycle (cycle 6) candidates are `flui-platform` + `flui-app` (platform integration matured), `flui-foundation` × `flui-types` (the two truly-bottom crates, never paired-audited), or a second pass at `flui-interaction` × `flui-scheduler` for follow-up findings post-PR #95-#98.
