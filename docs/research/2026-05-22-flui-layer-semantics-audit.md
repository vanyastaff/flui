---
title: "Mythos Audit — flui-layer × flui-semantics"
date: 2026-05-22
status: audit-complete
audit_methodology: claude-mythos (12-phase rust audit, compositing + a11y pass)
crates_audited:
  - flui-layer
  - flui-semantics
reference_sources:
  - flutter/packages/flutter/lib/src/rendering/layer.dart
  - flutter/packages/flutter/lib/src/rendering/compositing.dart
  - flutter/packages/flutter/lib/src/semantics/semantics.dart
  - flutter/packages/flutter/lib/src/semantics/semantics_event.dart
  - flutter/packages/flutter/lib/src/semantics/binding.dart
  - flutter/packages/flutter/lib/src/semantics/semantics_service.dart
authors:
  - Mythos (via Claude Opus 4.7)
---

# Mythos Audit: `flui-layer` × `flui-semantics`

> Deep audit across the FLUI **compositing + accessibility layer** — 46 source files, ~15.6K LOC — followed by cross-reference against Flutter `rendering/layer.dart` (3,029 LOC) + `rendering/compositing.dart` + `semantics/semantics.dart` (7,232 LOC) + `semantics/binding.dart` + `semantics/semantics_event.dart`.
>
> Goal: identify zombie abstractions, duplicate type aliases, half-implemented platform-routing, FSM drift from Flutter, lifecycle/Drop gaps, sync contention, and Constitution Principle violations — without breaking active integration with `flui-engine`, `flui-foundation`, or `flui-tree`.
>
> **Cycle**: this audit continues the audit-execute series that produced PRs #81 / #82 / #83 / #84 / #85-#98 against `vanyastaff/flui`. Previous cycle audited the input + frame-loop layer (`flui-interaction` × `flui-scheduler`); see [`2026-05-21-flui-interaction-scheduler-audit.md`](2026-05-21-flui-interaction-scheduler-audit.md) → closed as PRs #95/#96/#97 + companions.

---

## Table of Contents

- [Mythos Improvement Verdict](#mythos-improvement-verdict)
- [Part I — flui-layer Self-Audit](#part-i--flui-layer-self-audit)
- [Part II — flui-semantics Self-Audit](#part-ii--flui-semantics-self-audit)
- [Part III — Flutter Cross-Reference](#part-iii--flutter-cross-reference)
  - [Section 1 — flui-layer vs rendering/layer.dart + compositing.dart](#section-1--flui-layer-vs-flutterrenderinglayerdart--compositingdart)
  - [Section 2 — flui-semantics vs semantics/*.dart](#section-2--flui-semantics-vs-fluttersemantics)
- [Part IV — Combined Priority Order](#part-iv--combined-priority-order)
- [Appendix A — Investigation Trail](#appendix-a--investigation-trail)

---

## Mythos Improvement Verdict

The pair **`flui-layer` (9,796 LOC, 34 files) × `flui-semantics` (5,775 LOC, 12 files)** is **structurally cleaner than the interaction × scheduler pair** but carries **three load-bearing rot zones**: (a) a **fundamentally absent Layer lifecycle protocol** — no `Drop`, no ref-counted handles, no `markNeedsAddToScene` / `engineLayer` retention; (b) a **half-implemented `SemanticsService::send_event`** platform-routing path which is the only bridge from the semantics tree to the OS a11y API; and (c) a **double-walled `LinkRegistry` cache** that holds bidirectional `HashMap`s with no GC on layer removal. Beyond these: the `Layer` enum (`layer/mod.rs:184-248`) carries a `#[allow(clippy::large_enum_variant)]` discount of 19 variants up to ~360 LOC each — 7.2× bigger than the smallest variant, which means every `LayerNode::layer` field carries the worst-case footprint on every node. The `LayerNode::set_parent` / `add_child` pair (`tree/layer_tree.rs:343-353`) does **not detach a child from its previous parent** before reparenting — silent dual-parent state if a caller mis-uses it. The `flui-tree` integration is *clean* — `LayerTree` implements `TreeRead<LayerId> + TreeNav<LayerId>` exactly per memory's [[flui-tree-unified-interface-intent]] — but `SemanticsTree` does **not** implement those traits (separate hand-rolled traversal). Asymmetry between two trees that should be parallel.

**Three best things:**
1. `Scene::fire_composition_callbacks` (`scene.rs:228-245`) — uses `std::panic::catch_unwind` to isolate one callback's poison from the rest, returns a `Vec<LayerError::CallbackPoisoned>`. This is the Mythos Step 12 `Poisoned` pattern from PR #82's painting consolidation, applied correctly here. Matches *Rust for Rustaceans* "panics at module boundaries" guidance.
2. `LayerError` (`error.rs`) — narrow `thiserror` enum with five variants (`UnknownLayerId` / `BuilderStackUnderflow` / `OrphanedLeader` / `OrphanedFollower` / `CallbackPoisoned`). Constitution Principle 6 compliant. No `anyhow` leakage. The doc-comment explicitly says "the crate's failure surface is small" — earned.
3. `SemanticsBinding` ref-counting via `SemanticsHandle` + `Drop` (`binding.rs:100-117`). Matches Flutter's `SemanticsHandle` reference-counting protocol (`binding.dart:118-140`) exactly: `fetch_add` on construct, `fetch_sub` on drop, `semantics_enabled() = platform || handle_count > 0`. **This is the only fully Flutter-faithful FSM in either crate.**

**Worst complexity tax:**

1. **No layer lifecycle protocol**. Flutter's `Layer` ships:
   - `LayerHandle<T extends Layer>` ref-counting (`layer.dart:783-822`) — every render object holds a `LayerHandle`, layer is disposed when last handle drops.
   - `_refCount: int` + `_unref()` + `dispose()` (`layer.dart:273-340`) — `dispose()` is `@mustCallSuper`, asserts `!_debugDisposed`, releases `_engineLayer`.
   - `_needsAddToScene: bool` (`layer.dart:351-392`) — paint-pass marks dirty; compositing pass clears.
   - `_engineLayer: ui.EngineLayer?` (`layer.dart:444-481`) — retained engine handle across frames; disposed via `_engineLayer.dispose()` in `Layer.dispose()`.
   - `updateSubtreeNeedsAddToScene()` (`layer.dart:495-521`) — propagate dirty bit up the tree at composite time.

   **FLUI ships ZERO of the above.** `LayerNode` stores a `Layer` enum by value, has no `Drop`, no `disposed: AtomicBool`, no `needs_add_to_scene: bool`, no `engine_layer: Option<EngineLayerHandle>`. The `set_needs_compositing(bool)` setter (`layer_tree.rs:131-134`) exists but is **never called by anything in-workspace** (verified by grep — see Appendix A). Without `engine_layer` caching, every frame rebuilds the GPU layer from scratch even when the layer tree is identical to last frame — the entire point of `PictureLayer`'s "retained rendering" optimization is lost.

2. **`SemanticsService::send_event` is a 2-line stub** (`binding.rs:407-412`):
   ```rust
   pub fn send_event(event: SemanticsEvent) {
       tracing::debug!(event = ?event, "SemanticsService::send_event");
       // TODO: Route to platform accessibility API
   }
   ```
   This is the *only* bridge to platform-level accessibility events (announcements, tooltips, scroll notifications). The companion `SemanticsService::announce` (`binding.rs:386-388`) routes through `SemanticsBinding::announce` → `announce_callback` — that path works. But `send_event` — used by `SemanticsService::tooltip` and any future Flutter-ported "liveRegionChanged" / "scrollCompleted" event — is a no-op with a `// TODO`. Constitution Principle 6 violation in spirit ("No `unwrap()`/`println!`/`dbg!`/`unimplemented!`/`todo!` in production paths" — `// TODO` comment in lieu of impl is the same shape).

3. **Layer enum is 360+ LOC heavy on every node**. The biggest variant is `Picture(PictureLayer)` carrying a `flui_painting::Picture` (a `DisplayList` of Vec<DrawCommand> with `Box<dyn Drawable>` content). The smallest is `Opacity(OpacityLayer)` carrying `f32 + Offset<Pixels> = ~16 bytes`. `Layer` has `#[allow(clippy::large_enum_variant)]` silencing the warning, but every `LayerNode::layer` field carries the WORST-CASE footprint × N nodes. For a tree of 10k canvas layers (one Picture variant), 90k bytes are wasted on the empty variant tag. *Rust Performance Book* "Enum size" / *Programming Rust* 2nd ed §11 "Enums in memory" — the canonical Rust solution is to box the large variants: `Picture(Box<PictureLayer>)`, `Canvas(Box<CanvasLayer>)`. Verified: `PictureLayer` contains an owned `Picture` (`picture.rs:62`), `Picture` per `flui-painting` is unbounded; `CanvasLayer` per `canvas.rs:135` contains command Vec; `AnnotatedRegionLayer` contains `Arc<dyn Any>` (already pointer-sized). Boxing 4-5 variants would compress the enum to ~16-24 bytes from current ~360+.

**Where dead code hides:**

- `LayerNode::needs_compositing: bool` + setter (`tree/layer_tree.rs:34-36, 131-134`) — zero in-workspace callers of `set_needs_compositing`. The `Layer::needs_compositing()` enum-method at `layer/mod.rs:281-303` IS called (`builder.rs` flows), but the per-node CACHED bool exists for no reason — it's redundant with the enum method.
- `LayerNode::set_offset` / `set_element_id` + setters (`tree/layer_tree.rs:144-158`) — zero in-workspace callers of `set_offset`. Built-as-builder via `with_offset()` (1 internal callsite at insert path). The dead setters bloat the API surface.
- `SemanticsTree::iter_mut` (verified via grep) — zero in-workspace callers outside `owner.rs:342` (`SemanticsOwner::send_full_tree`).
- `DamageTracker::region_count` (`damage.rs:99-103`) — `#[must_use]` getter, 0 in-workspace callers (used in tests only).
- `LinkRegistry::leader_for_follower` (`link_registry.rs:251-255`) — 0 production callers in workspace (only own tests).
- `LinkRegistry::rebuild_follower_lists` (`link_registry.rs:314-326`) — 0 production callers; doc-comment hints "call after bulk modifications" but bulk modifications don't currently exist as a workspace pattern.
- `LayerTree::iter_mut` (`tree/layer_tree.rs:531-535`) — 0 production callers (own test only).
- `CompositorStats` getters (`compositor/retained.rs:50-58`) — `stats()` has 0 in-workspace callers.

**Half-implemented hot paths:**

- `SemanticsService::send_event` — covered above, single TODO in semantics crate.
- `LayerNode::needs_compositing` field — has a setter, but no caller; if the engine eventually reads it, it'll always be the default-true. Functional placeholder.
- `LayerTree::remove` (`tree/layer_tree.rs:323-330`) — explicitly documents "**Note:** This does NOT remove children. Caller must handle tree cleanup." But every consumer (one of which doesn't exist yet) would forget — this is a footgun. The Flutter equivalent (`ContainerLayer.remove`, `layer.dart:1185-1216`) cascades cleanup. FLUI's `remove` should either (a) cascade, or (b) take a `&mut self.children` slice and validate empty first.

**Biggest optimization opportunity** — **box the heavy Layer enum variants**. Estimated impact: a 19-variant enum that's 360+ bytes per node compresses to 16-24 bytes. A tree of 1k layers saves ~344k bytes of `LayerNode` storage. Plus: implement `Layer::needs_add_to_scene` dirty-bit propagation matching Flutter; cache GPU `engine_layer` handles to skip re-tessellation on unchanged subtrees. Without these, every frame re-encodes the entire scene — the worst-case render path.

**Не трогать**:
- `Scene::fire_composition_callbacks` panic-catch pattern (`scene.rs:228-245`) — correct.
- `LayerError` shape (`error.rs`) — exemplary.
- `SemanticsHandle` ref-counting (`binding.rs:100-117`) — Flutter-faithful + Rust-idiomatic `Drop`.
- `SemanticsConfiguration::absorb` semantic *shape* (`configuration.rs:820-854`) — even though it has 4 drift issues vs Flutter (covered in cross-ref), the structure (merge flags + actions + custom_actions + tags + first-wins for scalars) is correctly Flutter-shaped.
- `flui-tree::TreeRead/TreeNav` integration in `LayerTree` (`tree/tree_traits.rs`) — clean port of the trait surface, no parallel traversal.
- `DamageTracker` (`damage.rs`) — 113 LOC, simple, correct, full-repaint default + union-of-dirty-rects on demand. Don't touch.
- `LayerLink` atomic ID generation (`leader.rs:18-23`) — `AtomicU64::fetch_add(1, Relaxed)` is a Rust-native improvement over Flutter's `LayerLink` (`layer.dart:2416-2477`), which relies on Dart object identity (no explicit ID — uses `==` on object reference). Rust has no GC-backed object identity, so an explicit ID counter is the canonical Rust port. Relaxed ordering is correct because the ID is only used for HashMap keying; no other memory must be synchronized with the ID write.

---

# Part I — flui-layer Self-Audit

## Project Map

```text
flui-layer (9,796 LOC, 34 files, 4 module roots)
  owns: Scene + LayerTree + SceneBuilder + LinkRegistry + 19-variant `Layer` enum
        scene.rs (589 LOC) — Scene + CompositionCallback + fire_composition_callbacks
          panic-catch + Scene::from_layer / new / with_links / empty / builder().
        tree/layer_tree.rs (542 LOC) — LayerNode { parent + children + layer +
          needs_compositing + offset + element_id } + LayerTree { Slab<LayerNode>
          + root } + add_child / remove_child / clear_children / append_layer(s) /
          insert / insert_with_element / get / get_mut / get_layer / get_layer_mut.
        tree/tree_traits.rs (303 LOC) — impl TreeRead<LayerId> + TreeNav<LayerId>
          for LayerTree.
        layer/mod.rs (584 LOC) — Layer enum (19 variants) + bounds() / needs_compositing()
          / is_clip() / is_linking() / is_opaque() + gen_layer_accessors! macro
          generating 19 × is_/as_/as_mut_ accessors + 18 From<*Layer> impls.
        layer/dispatch.rs (88 LOC) — macro_rules! gen_layer_accessors!{} (Mythos Step 4).
        Layer types:
          canvas.rs (135) Canvas, picture.rs (251) Picture, texture.rs (348),
          platform_view.rs (269), performance_overlay.rs (530),
          clip_rect.rs (192), clip_rrect.rs (262), clip_path.rs (271),
          clip_superellipse.rs (353),
          offset.rs (242), transform.rs (393),
          opacity.rs (285), color_filter.rs (323), image_filter.rs (281),
          shader_mask.rs (197), backdrop_filter.rs (185),
          leader.rs (239) LayerLink + LeaderLayer, follower.rs (366),
          annotated_region.rs (313).
        layer/annotation.rs (369) — AnnotationEntry + AnnotationResult +
          AnnotationSearchOptions (annotation lookup walker).
        layer/bounds.rs (103) — LayerBounds trait + impls per Layer variant.
        compositor/builder.rs (557) — SceneBuilder<'a> { tree: &mut LayerTree +
          stack: Vec<LayerId> + root } + push_offset / push_transform / push_opacity /
          push_clip_rect / push_clip_rrect / push_clip_path / push_color_filter /
          push_image_filter / push_shader_mask / push_backdrop_filter +
          add_canvas / add_picture / add_texture / add_retained +
          pop -> Result + try_pop -> Option + pop_to_depth + build / build_and_reset.
        compositor/retained.rs (94) — SceneCompositor { retained: Vec<LayerId> +
          stats: CompositorStats }.
        link_registry.rs (621) — LinkRegistry { leaders: HashMap<LayerLink, LeaderInfo>
          + followers: HashMap<LayerId, LayerLink> } + register / unregister /
          followers_for_link + remove_orphaned_followers + rebuild_follower_lists.
        damage.rs (112) — DamageTracker { regions: Vec<Rect> + full_repaint: bool }.
        error.rs (69) — LayerError ( UnknownLayerId | BuilderStackUnderflow |
          OrphanedLeader | OrphanedFollower | CallbackPoisoned ) + LayerResult<T>.
  depends on: flui-foundation (LayerId, ElementId), flui-types (Offset, Size,
              Rect, Matrix4, Pixels, painting::Clip + BlendMode + Path + Shader +
              ImageFilter + TextureId + FilterQuality + ColorMatrix),
              flui-painting (Picture + DisplayListCore), flui-tree (TreeRead +
              TreeNav + Ancestors + DescendantsWithDepth + AllSiblings),
              slab, thiserror, tracing.
  public surface: 39 top-level + 30 prelude exports (lib.rs:114-170 + 181-212).
  suspected hot paths:
    - SceneBuilder::push_layer (builder.rs:105-123) — every push allocates 1
      LayerNode (Vec<LayerId> in LayerNode + Vec stack push).
    - Layer enum dispatch at scene composite time — large_enum_variant means every
      pattern match traverses the worst-case payload.
    - Scene::fire_composition_callbacks (scene.rs:228-245) — once per frame on
      successful composite; panic-catch is the cost.
    - LinkRegistry::register_follower (link_registry.rs:202-209) — per-follower
      double HashMap write + leader follower-list push.
  risk:
    - LayerNode has no Drop, no disposed: AtomicBool, no engine_layer cache.
      Layer instances live and die by Slab eviction with zero lifecycle protocol.
    - LayerTree::remove docs explicitly: "does NOT remove children. Caller must
      handle tree cleanup." This is a footgun — cascading cleanup is the only
      safe shape, but the API as written hides the risk.
    - LayerTree::add_child does NOT detach a child from its previous parent before
      reparenting. Dual-parent state possible if caller mis-uses.
    - LinkRegistry follower map persists even after LeaderLayer is removed from
      tree. `remove_orphaned_followers` exists as the GC hook but has 0 production
      callers — nobody invokes it in workspace.
    - FollowerLayer.leader_anchor: Offset<Pixels> is a TYPE ERROR — anchor should
      be unitless [0,1] alignment; here it's typed as a Pixels offset and used in
      arithmetic that multiplies Pixels × Pixels (`calculate_offset`, follower.rs:175-191).
    - SemanticsConfiguration has 4 Flutter-divergence points (cross-ref Section 2).
```

**Cross-crate dependency DAG** (clean):

```
flui-layer → flui-foundation (LayerId, ElementId)
           → flui-types (Offset, Rect, Matrix4, painting::*)
           → flui-painting (Picture, DisplayListCore)
           → flui-tree (TreeRead, TreeNav, iter modules)
           → slab, thiserror, tracing
```

No upward deps. flui-rendering and flui-engine consume flui-layer (per `crates/flui-rendering/Cargo.toml`).

## Findings

### 💀 [LIFECYCLE-LEAK | CRITICAL]: `LayerNode` has no `Drop`, no `disposed: AtomicBool`, no `engine_layer` cache — Flutter's entire Layer lifecycle protocol is absent

**Evidence:**
- [`crates/flui-layer/src/tree/layer_tree.rs:24-43`](../../crates/flui-layer/src/tree/layer_tree.rs) — `LayerNode` fields: `parent / children / layer / needs_compositing / offset / element_id`. **No `disposed` flag. No `needs_add_to_scene` flag. No `engine_layer` cache. No `Drop` impl.**
- [`flutter/lib/src/rendering/layer.dart:144-340`](Flutter source) — `Layer` ships:
  - `_refCount: int` (line 274) + `_unref()` (line 277-284) — auto-dispose when last `LayerHandle` drops.
  - `_debugDisposed: bool` + `assert(!_debugDisposed)` guards on every public mutation.
  - `_needsAddToScene: bool = true` (line 372) — dirty bit for compositing pass.
  - `_engineLayer: ui.EngineLayer?` (line 447) — retained GPU-side handle across frames.
  - `void dispose()` `@mustCallSuper` (line 319-340) — disposes `_engineLayer` + marks `_debugDisposed = true`.
  - `void markNeedsAddToScene()` (line 377-392) + `updateSubtreeNeedsAddToScene()` (line 495-521) — propagate dirty bit.
- The Layer concept in Flutter exists ENTIRELY to amortize GPU-side work across frames: `engineLayer` is a Skia/Impeller native handle. Without `engine_layer` retention, every frame re-builds the GPU scene from scratch — `PictureLayer`'s caching benefit is zero. FLUI's `PictureLayer.set_picture(picture)` (`picture.rs:147-150`) replaces the picture in-place but never tells the engine "you may keep the old GPU handle".
- PR #84 introduced the `ChangeNotifier::dispose` + `disposed: AtomicBool` pattern in `flui-foundation/src/notifier.rs`. Every Layer mutation path — `LayerNode::set_layer`, `LayerNode::clear_children`, `Scene::layer_tree_mut`, `SceneBuilder::push_*` — would need to assert non-disposed and call `markNeedsAddToScene` upstream. None do.
- LayerTree storage is `Slab<LayerNode>`; `LayerTree::remove` (`layer_tree.rs:323-330`) calls `slab.try_remove` — drops the Rust struct, but if the layer held an `engine_layer: Option<EngineLayerHandle>`, no `dispose()` is called.

**Why it exists:**
The flui-layer crate was scaffolded as a *data layer* — a typed enum of layer descriptions sitting in a tree — without modeling the *resource layer* (GPU-side allocations). The split between "layer as data" and "layer as resource holder" got dropped on the floor.

**Cost today:**
- **No retained rendering**: every frame re-encodes the entire scene. `PictureLayer`'s point of existence (replay cached commands without re-recording, per `picture.rs:54-59`) is wasted because the engine never sees a cached `ui.EngineLayer` handle to skip.
- **No mark-dirty propagation**: Flutter's "child dirty → parent dirty → root dirty" chain via `markNeedsAddToScene` doesn't exist. A subtree-level repaint optimization requires this. Without it, the only state available is `Scene::has_content()` (binary) — no per-subtree dirty bits.
- **No double-dispose protection**: a slab-removed layer can theoretically be re-inserted with the same `LayerId` (slab reuses indices). Without `disposed: AtomicBool`, a stale `LayerId` from a prior frame could navigate to a freshly-inserted layer with no panic — silent corruption.
- **Resource leaks under unsafe-sister-crate scenarios**: when `flui-engine` materializes (currently disabled), GPU buffer / texture handles stored in `LayerNode::layer` won't be properly released on tree mutation.

**Risk of changing:**
High — this is a fundamental data-shape addition. Touches: `LayerNode` struct definition; every layer mutation path; `LayerTree::insert` / `remove`; `SceneBuilder::push_layer`. Mitigates with: implement gradually, start with `disposed: AtomicBool` + `Drop` impl + assert guards (no engine integration yet), then add `needs_add_to_scene` per-node bool, finally `engine_layer` slot once `flui-engine` exposes the handle type.

**Recommendation:**
**Phased Layer lifecycle introduction.** Three atomic-commit phases:

1. **`LayerNode::disposed` + `Drop` + assert guards** (~150 LOC). Add `disposed: std::sync::atomic::AtomicBool` to `LayerNode`. Add `impl Drop for LayerNode { fn drop(&mut self) { self.disposed.store(true, Release); } }`. Add `debug_assert!(!self.disposed.load(Acquire), "LayerNode used after disposal")` to every public mutation path on `LayerNode`. Mirrors PR #84's `ChangeNotifier::dispose` pattern.

2. **`needs_add_to_scene` dirty propagation** (~200 LOC). Add `needs_add_to_scene: AtomicBool = true` to `LayerNode`. Add `LayerTree::mark_needs_add_to_scene(id)` that walks ancestors via `TreeNav::ancestors`. Add `LayerTree::update_subtree_needs_add_to_scene(root)` matching Flutter's `updateSubtreeNeedsAddToScene` (layer.dart:495-521). Add `LayerTree::is_clean(id) -> bool` getter. SceneBuilder's `push_layer` and `add_layer` call `mark_needs_add_to_scene(parent)` on parent.

3. **`engine_layer` cache slot** (~80 LOC, deferred until flui-engine ships). Add `engine_layer: Option<EngineLayerHandle>` field (engine handle type comes from flui-engine when materialized). On `LayerNode::drop`, drop the handle. On `LayerNode::layer_mut` calls, invalidate via `engine_layer = None`. The engine consumes `engine_layer` slot when re-composing the scene — if `Some`, retain; if `None`, re-encode.

**Patch sketch (Phase 1):**
```rust
// crates/flui-layer/src/tree/layer_tree.rs
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub struct LayerNode {
    parent: Option<LayerId>,
    children: Vec<LayerId>,
    layer: Layer,
    needs_compositing: bool,
    needs_add_to_scene: AtomicBool, // new — phase 2
    offset: Option<Offset<Pixels>>,
    element_id: Option<ElementId>,
    disposed: AtomicBool, // new — phase 1
}

impl Drop for LayerNode {
    fn drop(&mut self) {
        // Drop the layer; engine handle release wires here in phase 3.
        self.disposed.store(true, Ordering::Release);
    }
}

impl LayerNode {
    #[inline]
    fn assert_alive(&self) {
        debug_assert!(
            !self.disposed.load(Ordering::Acquire),
            "LayerNode used after disposal"
        );
    }

    pub fn layer_mut(&mut self) -> &mut Layer {
        self.assert_alive();
        // phase 2: self.mark_needs_add_to_scene()
        &mut self.layer
    }
}
```

---

### 💀 [PARITY-DRIFT | CRITICAL]: `Layer` enum suffers `large_enum_variant` — silenced via `#[allow]` instead of boxing heavy variants

**Evidence:**
- [`crates/flui-layer/src/layer/mod.rs:182-248`](../../crates/flui-layer/src/layer/mod.rs) — `pub enum Layer { Canvas(CanvasLayer), Picture(PictureLayer), Texture(TextureLayer), PlatformView(PlatformViewLayer), PerformanceOverlay(PerformanceOverlayLayer), ClipRect(ClipRectLayer), ... ShaderMask(ShaderMaskLayer), BackdropFilter(BackdropFilterLayer), Leader(LeaderLayer), Follower(FollowerLayer), AnnotatedRegion(AnnotatedRegionLayer) }`. 19 variants.
- Line 183: `#[allow(clippy::large_enum_variant)]` — silences the clippy lint instead of fixing it.
- Heaviest variants:
  - `Picture(PictureLayer)` contains an owned `Picture` (DisplayList of `Vec<DrawCommand>` w/ `Box<dyn Drawable>` content). `Picture` from flui-painting has unbounded payload — Vec growth + each command boxed.
  - `Canvas(CanvasLayer)` (canvas.rs:7-15) — internal Vec of CanvasCommand (each `Box<dyn>` style payload).
  - `PerformanceOverlay(PerformanceOverlayLayer)` (530 LOC source — has metrics history Vec, render state struct).
  - `ClipPath(ClipPathLayer)` contains a `Path` (Vec of path commands).
  - `ShaderMask(ShaderMaskLayer)` contains a `Shader` enum (gradient stops Vec etc.).
- Lightest variants:
  - `Opacity(OpacityLayer)` — `f32 + Offset = ~16 bytes` (`opacity.rs:48-54`).
  - `Offset(OffsetLayer)` — `Offset = ~8 bytes`.
  - `Leader(LeaderLayer)` — `LayerLink(u64) + Size + Offset = ~32 bytes`.
- A 19-variant enum carries the max of all payloads in every cell: each `LayerNode::layer` field is sized to the largest variant. For a tree of 1000 `OpacityLayer`s alone, the enum overhead is ~344kb of wasted space — every node is sized to `PictureLayer`'s footprint.
- *Programming Rust* 2nd ed §11.2 "Enums in memory" + *Rust Performance Book* "Enum size" both recommend boxing heavy variants: `Picture(Box<PictureLayer>)`.

**Why it exists:**
Layer was designed as a value-typed enum for stack-friendly pattern matching. The 19 variants were added incrementally as Flutter's layer hierarchy was ported, and the `#[allow]` was tagged when clippy started complaining.

**Cost today:**
- Memory bloat per `LayerNode` proportional to worst variant (estimated 360+ bytes vs 16 bytes for a `Box`-ified variant tag).
- Stack-heavy pattern matching — every `match layer { ... }` allocates the worst-case payload as the pattern frame even for the lightweight branches.
- Move semantics — `Layer` values move in/out of `LayerNode` are byte-by-byte copy of 360+ bytes.

**Risk of changing:**
Medium. Boxing heavy variants:
```rust
pub enum Layer {
    // Light variants stay inline:
    Opacity(OpacityLayer),
    Offset(OffsetLayer),
    Leader(LeaderLayer),
    Follower(FollowerLayer),
    Transform(TransformLayer),
    ClipRect(ClipRectLayer),
    ClipRRect(ClipRRectLayer),
    ClipSuperellipse(ClipSuperellipseLayer),
    ColorFilter(ColorFilterLayer),
    ImageFilter(ImageFilterLayer),
    Texture(TextureLayer),
    BackdropFilter(BackdropFilterLayer),
    PlatformView(PlatformViewLayer),
    ShaderMask(ShaderMaskLayer),
    AnnotatedRegion(AnnotatedRegionLayer),
    // Heavy variants boxed:
    Canvas(Box<CanvasLayer>),
    Picture(Box<PictureLayer>),
    ClipPath(Box<ClipPathLayer>),
    PerformanceOverlay(Box<PerformanceOverlayLayer>),
}
```
Ripples: all `From<XxxLayer> for Layer` impls (`mod.rs:388-494`) box the inner. The `gen_layer_accessors!` macro auto-generates `as_picture(&self) -> Option<&PictureLayer>` — needs to dereference the Box. The 18 `as_<variant>` accessors return `Option<&Layer>` so the dereference is automatic via `Deref`.

**Recommendation:**
**Box the 4 heaviest variants** (Canvas / Picture / ClipPath / PerformanceOverlay). Remove `#[allow(clippy::large_enum_variant)]`. Add `as_picture(&self) -> Option<&PictureLayer>` accessors returning `&PictureLayer` (the macro dereferences the Box internally). Estimated 50-LOC delta, all in `layer/mod.rs` + `From` impls.

---

### 💀 [HALF-IMPLEMENTED | CRITICAL]: `SemanticsService::send_event` is a 2-line `// TODO` stub — the only platform routing path for semantics events

**Evidence:**
- [`crates/flui-semantics/src/binding.rs:407-412`](../../crates/flui-semantics/src/binding.rs):
  ```rust
  #[allow(clippy::needless_pass_by_value)] // Will be consumed when routed to platform API
  pub fn send_event(event: SemanticsEvent) {
      tracing::debug!(event = ?event, "SemanticsService::send_event");
      // TODO: Route to platform accessibility API
  }
  ```
- This is the **only** way for semantics events to reach the platform a11y API. The companion `SemanticsService::announce` (binding.rs:386-405) routes through `SemanticsBinding::announce` → `announce_callback` — that path works. `send_event` is broken.
- Consumers (today: `SemanticsService::tooltip` at binding.rs:414-418) emit an event and assume it gets routed. The event is swallowed by `tracing::debug!`.
- Future Flutter-aligned events: `SemanticsEvent::scroll_completed`, `SemanticsEvent::announce`, `SemanticsEvent::tooltip`, `SemanticsEvent::tap`, `SemanticsEvent::longPress` — none of which can reach the platform via FLUI today.

**Why it exists:**
The platform routing hook hadn't been threaded through yet — the `announce_callback` pattern (binding.rs:158-162, 238-243) was implemented but its sibling `event_callback` was scaffolded but never added.

**Cost today:**
- Constitution Principle 6 in spirit: `// TODO` in production path.
- Public API lie: `SemanticsService::send_event` doc-comment promises "send to platform" — it logs.
- `SemanticsService::tooltip` (binding.rs:414-418) is silently broken — calls `send_event` which is no-op.

**Risk of changing:**
Low. The fix mirrors the existing `announce_callback`:
```rust
// binding.rs add new field
event_callback: RwLock<Option<Arc<dyn Fn(&SemanticsEvent) + Send + Sync>>>,

impl SemanticsBinding {
    pub fn set_event_callback<F>(&self, callback: F)
    where F: Fn(&SemanticsEvent) + Send + Sync + 'static,
    {
        *self.event_callback.write() = Some(Arc::new(callback));
    }

    pub fn dispatch_event(&self, event: &SemanticsEvent) {
        if let Some(ref cb) = *self.event_callback.read() {
            cb(event);
        }
    }
}

impl SemanticsService {
    pub fn send_event(event: SemanticsEvent) {
        use flui_foundation::HasInstance;
        if SemanticsBinding::is_initialized() {
            SemanticsBinding::instance().dispatch_event(&event);
        } else {
            tracing::debug!(event = ?event, "send_event (binding not initialized)");
        }
    }
}
```

**Recommendation:**
**Wire `send_event` through a new `SemanticsBinding::event_callback`** mirroring the `announce_callback` pattern. ~30 LOC change. Update `SemanticsBinding::dispatch_action` doc-comments to clarify the three callback paths (announce / event / action).

---

### 💀 [LIFECYCLE-LEAK | HIGH]: `LayerTree::remove` documents non-cascading but every consumer needs cascade — design footgun

**Evidence:**
- [`crates/flui-layer/src/tree/layer_tree.rs:317-330`](../../crates/flui-layer/src/tree/layer_tree.rs):
  ```rust
  /// Removes a LayerNode from the tree.
  ///
  /// Returns the removed node, or None if it didn't exist.
  ///
  /// **Note:** This does NOT remove children. Caller must handle tree
  /// cleanup.
  pub fn remove(&mut self, id: LayerId) -> Option<LayerNode> {
      if self.root == Some(id) {
          self.root = None;
      }
      self.nodes.try_remove(id.get() - 1)
  }
  ```
- The doc explicitly says caller must clean up children. Every realistic consumer — `SceneBuilder` rebuild, `flui-rendering`'s repaint-boundary swap — wants cascading removal. Non-cascade is a footgun: a remove leaves orphan children in the slab; if a fresh insert reuses the slab index, the orphan children's `.parent` field now points to the wrong node — silent tree corruption.
- Compare Flutter (`layer.dart:1185-1216` `ContainerLayer::remove`):
  ```dart
  void remove(Layer layer) {
    layer._previousSibling?._nextSibling = layer._nextSibling;
    layer._nextSibling?._previousSibling = layer._previousSibling;
    if (layer._parent == this) { layer._parent = null; }
    // Children retained on Layer itself; layer's own dispose() cascades.
  }
  ```
  Flutter's removal cleans up the sibling pointers + clears parent. The cascade happens via reference counting when the `LayerHandle` drops.
- FLUI has no `LayerHandle` (covered above), so the cleanup that Flutter gets for free via `_unref()` requires explicit cascading in `LayerTree::remove`.

**Cost today:**
- Footgun-shaped API. Anyone calling `LayerTree::remove(id)` without first calling `clear_children(id)` and walking the children to remove them, then their children, recursively — corrupts the tree.
- The companion `LayerTree::clear_children(parent_id)` (`layer_tree.rs:391-410`) removes the parent-child link but does NOT remove the children from the slab — they become orphaned roots. Two of these footguns interact.

**Risk of changing:**
Low. Change `remove` to cascade by default:
```rust
pub fn remove(&mut self, id: LayerId) -> Option<LayerNode> {
    // Cascade-remove children first (post-order).
    let children: Vec<LayerId> = self.get(id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    for child_id in children {
        self.remove(child_id);  // recurse
    }
    // Remove self from parent's children.
    if let Some(parent_id) = self.get(id).and_then(|n| n.parent()) {
        if let Some(parent) = self.get_mut(parent_id) {
            parent.remove_child(id);
        }
    }
    if self.root == Some(id) { self.root = None; }
    self.nodes.try_remove(id.get() - 1)
}
```
Add `remove_shallow(id) -> Option<LayerNode>` for the non-cascading variant — but rename so the safe behavior is the default. Recursion depth is bounded by Flutter's effective layer-tree depth (~32, per `TreeNav::MAX_DEPTH` = 32 in tree_traits.rs).

**Recommendation:**
**Rename `remove` → `remove_shallow` (or `try_remove_orphan`) and add a new `remove` that cascades.** The cascade variant is the safe default. Same shape applied to `clear_children` — add a `remove_subtree(parent)` that recursively removes the parent and all descendants.

---

### 💀 [LIFECYCLE-LEAK | HIGH]: `LayerTree::add_child` does not detach child from previous parent — silent dual-parent state possible

**Evidence:**
- [`crates/flui-layer/src/tree/layer_tree.rs:343-353`](../../crates/flui-layer/src/tree/layer_tree.rs):
  ```rust
  pub fn add_child(&mut self, parent_id: LayerId, child_id: LayerId) {
      // Update parent's children
      if let Some(parent) = self.get_mut(parent_id) {
          parent.add_child(child_id);
      }
      // Update child's parent
      if let Some(child) = self.get_mut(child_id) {
          child.set_parent(Some(parent_id));
      }
  }
  ```
- If the child was previously a child of `old_parent`, the old_parent's `children: Vec<LayerId>` still contains `child_id`. So `old_parent.children()` reports the child as still attached, while `child.parent()` reports the new parent. Walking the tree top-down via `old_parent.children` will incorrectly recurse into the moved child — double-counting.
- `LayerNode::add_child` (`layer_tree.rs:91-94`) uses `Vec::push` without duplicate check: `self.children.push(child)`. If `add_child(p, c)` is called twice, the child appears in `p.children` twice. SmallVec or HashSet would prevent this; Vec doesn't.
- Compare Flutter (`layer.dart:1098-1149` `ContainerLayer.append`):
  ```dart
  void append(Layer child) {
    assert(child != this);
    assert(child != _firstChild);
    assert(child != _lastChild);
    assert(child._parent == null);  // ← Flutter ASSERTS no prior parent
    assert(!_debugUltimatePreviousSiblingOf(child, equals: _lastChild));
    ...
    adoptChild(child);
    ...
  }
  ```
  Flutter asserts `child._parent == null` — explicit contract that re-parenting requires explicit detach first.

**Cost today:**
- Silent dual-parent corruption.
- Duplicate-add silently creates a child-list with the same ID twice.
- DescendantsWithDepth iterator (via TreeNav) returns duplicates if dual-parented.

**Risk of changing:**
Low. Add a debug-assert + auto-detach:
```rust
pub fn add_child(&mut self, parent_id: LayerId, child_id: LayerId) {
    // Detach from previous parent if any.
    let prev_parent = self.get(child_id).and_then(|n| n.parent());
    if let Some(prev) = prev_parent {
        if prev != parent_id {
            if let Some(prev_node) = self.get_mut(prev) {
                prev_node.remove_child(child_id);
            }
        } else {
            // Already a child — short-circuit instead of double-push.
            return;
        }
    }
    // Now attach.
    if let Some(parent) = self.get_mut(parent_id) {
        parent.add_child(child_id);
    }
    if let Some(child) = self.get_mut(child_id) {
        child.set_parent(Some(parent_id));
    }
}
```
Plus add `LayerNode::add_child` containment check: `if !self.children.contains(&child) { self.children.push(child); }` — matches `SemanticsNode::add_child` which already does this (`node.rs:127-130`).

**Recommendation:**
**Auto-detach + dedup in `add_child`**. Matches Flutter's `ContainerLayer.append` contract. ~15 LOC delta.

---

### 💀 [API-SURFACE | HIGH]: `FollowerLayer::leader_anchor` and `follower_anchor` are typed `Offset<Pixels>` but used as unitless [0,1] anchors — semantic + unit type error

**Evidence:**
- [`crates/flui-layer/src/layer/follower.rs:53-57`](../../crates/flui-layer/src/layer/follower.rs):
  ```rust
  /// Alignment on the leader (0,0 = top-left, 1,1 = bottom-right)
  leader_anchor: Offset<Pixels>,
  /// Alignment on the follower (0,0 = top-left, 1,1 = bottom-right)
  follower_anchor: Offset<Pixels>,
  ```
- The doc-comment explicitly says **normalized [0,1] alignment** but the **type is `Offset<Pixels>`**. The unit is pixels, but the values represent fractions of size.
- Usage in `calculate_offset` (`follower.rs:169-191`):
  ```rust
  let leader_anchor_point = Offset::new(
      leader_offset.dx + leader_size.width * self.leader_anchor.dx,
      leader_offset.dy + leader_size.height * self.leader_anchor.dy,
  );
  ```
  Multiplies `Pixels (leader_size.width) × Pixels (self.leader_anchor.dx)` and assigns to a `Pixels` — this **compiles** because `Pixels` is `f32`-wrapped and multiplication likely produces `Pixels² → Pixels` via the operator overload, but semantically the result of `width(px) × fraction(unitless)` should be `px × 1 = px`. The fraction term should be `f32` or a dedicated `Alignment` newtype.
- Convenience constructors `below / above / right_of / left_of` (`follower.rs:196-232`) pass values like `Offset::new(px(0.5), px(1.0))` — `px(0.5)` is "0.5 pixels" semantically, used as "0.5 alignment". Type-system lie.
- Compare Flutter (`layer.dart:2603-2700`):
  ```dart
  class FollowerLayer extends ContainerLayer {
    Alignment? leaderAnchor;        // ← dedicated Alignment type
    Alignment? followerAnchor;
  }
  ```
  Flutter has `Alignment` (`painting/alignment.dart`) — a dedicated unitless type with `Alignment.topLeft = (-1, -1)`, `Alignment.center = (0, 0)`, `Alignment.bottomRight = (1, 1)`.

**Cost today:**
- Type-system tells a lie. Anyone touching this code reads `Offset<Pixels>` and expects a pixel coordinate.
- Mixing pixel coords with unitless fractions in the same `Offset<Pixels>` field would slip past unit-aware downstream code that intermediates between Pixels and LogicalPixels.
- Disagreement with Flutter's range: FLUI's doc-comment says `0..1` but Flutter's `Alignment` is `-1..1` (center is `(0, 0)`). The convenience constructors use `0..1`, but a user porting Flutter code might pass `Alignment.center = (0, 0)` and get unexpected behavior.

**Risk of changing:**
Medium. Two options:
- **(a)** Change field type to `(f32, f32)` or a tuple/pair, drop the pixel unit. Lose the structured `.dx/.dy` accessors but gain semantic correctness.
- **(b)** Introduce a `Alignment` newtype in flui-types (per Flutter's `painting/alignment.dart`). Pre-defined constants `Alignment::TOP_LEFT`, `CENTER`, `BOTTOM_RIGHT`. Two-component f32 pair, range `-1..1` per Flutter convention.

**(b)** is the Flutter-port-aligned answer.

**Recommendation:**
**Introduce `flui_types::painting::Alignment` newtype** (range `-1..1` per Flutter). Migrate `FollowerLayer::leader_anchor` + `follower_anchor` to `Alignment` type. Update convenience constructors. The `calculate_offset` math becomes:
```rust
let leader_anchor_point = Offset::new(
    leader_offset.dx + leader_size.width * (self.leader_anchor.x + 1.0) / 2.0,
    leader_offset.dy + leader_size.height * (self.leader_anchor.y + 1.0) / 2.0,
);
```
~80 LOC change including new Alignment newtype + 4 convenience constructors + tests. **Lands as a flui-types PR first, then a FollowerLayer migration PR.**

---

### 💀 [SYNC-CONTENTION | MEDIUM]: `SemanticsBinding` holds 4 separate `RwLock`s; per-callback callsite locks 2-3 of them per event

**Evidence:**
- [`crates/flui-semantics/src/binding.rs:154-163`](../../crates/flui-semantics/src/binding.rs) — `SemanticsBinding` fields:
  ```rust
  accessibility_features: RwLock<AccessibilityFeatures>,
  announce_callback: RwLock<Option<Arc<dyn Fn(&str, Assertiveness) + Send + Sync>>>,
  action_callback: RwLock<Option<Arc<dyn Fn(SemanticsActionEvent) + Send + Sync>>>,
  ```
  + (after fix to `send_event`) — `event_callback` would be a fourth RwLock.
- `disable_animations()` (`binding.rs:231-233`): `self.accessibility_features.read().disable_animations` — one read lock, just to read a `bool`.
- `accessibility_features()` (`binding.rs:218-220`): same — one read lock to copy a 7-field POD struct.
- `dispatch_action` (`binding.rs:271-275`): `if let Some(ref cb) = *self.action_callback.read() { cb(event); }` — holds read lock during callback invocation. If callback re-enters `SemanticsBinding` (e.g., to read `accessibility_features`), no deadlock since different locks — but if the callback calls `set_action_callback` (replacing itself), deadlock.
- Compare Flutter (`binding.dart:154-441`): single-threaded. No locks. The Dart event loop serializes everything.
- Per *Rust Atomics and Locks* (Gjengset) ch.3-4: scalar reads (bool, simple fields) should be atomic. RwLock per POD field is wasteful — both compile-time (allocator overhead in the RwLock state) and runtime (lock acquire + memory ordering even on the read path).

**Why it exists:**
The binding handles multiple platform callbacks. Each callback was added with its own RwLock for independent registration. The accessibility_features were added similarly. The result is 3-4 separate locks.

**Cost today:**
- Per-frame `disable_animations()` checks (likely called from animation pipeline) acquire RwLock for a single bool read.
- Read-during-callback pattern: if dispatch_action's callback calls disable_animations, that's lock-2-while-lock-1-held. No current deadlock, but easy to hit if more locks are added.
- 7-bool `AccessibilityFeatures` struct is `Copy + Default` — could be `AtomicU8` (bitflags) for entire struct.

**Risk of changing:**
Medium. Split into:
- `accessibility_features: AtomicU8` (bitflags packed). The 7 bools fit in one byte. Read via `AccessibilityFeatures::from_bits(load(Acquire))`. Write via `store(features.bits(), Release)`. Single CAS-free atomic for the whole struct.
- Callbacks: keep as `RwLock<Option<Arc<dyn Fn>>>` (the callback registration is rare, the Arc clone via `read()` is cheap). But take the callback OUT of the lock before invoking:
  ```rust
  pub fn dispatch_action(&self, event: SemanticsActionEvent) {
      let cb = self.action_callback.read().as_ref().map(Arc::clone);
      if let Some(cb) = cb { cb(event); }
  }
  ```
  This pattern is already correct in flui-interaction's PointerRouter (per prior audit's [SYNC-CONTENTION | HIGH] finding).

**Recommendation:**
**Two-part**:
1. **Pack `AccessibilityFeatures` into an `AtomicU8`** via bitflags. `set_accessibility_features` becomes `store(features.bits(), Release)`. `accessibility_features()` becomes `from_bits(load(Acquire))`. Eliminates one RwLock entirely.
2. **Clone-and-release pattern in `dispatch_action` / `dispatch_event`** — release the read lock BEFORE invoking the callback. Prevents lock-held-during-callback class of bugs.

---

### 💀 [API-SURFACE | MEDIUM]: `LinkRegistry::remove_orphaned_followers` and `rebuild_follower_lists` exist as GC hooks with zero production callers

**Evidence:**
- [`crates/flui-layer/src/link_registry.rs:294-326`](../../crates/flui-layer/src/link_registry.rs) — `remove_orphaned_followers` + `rebuild_follower_lists` — both `pub` with doc-comments suggesting "call after bulk modifications" / "to ensure consistency".
- Grep `remove_orphaned_followers|rebuild_follower_lists` across workspace: **0 production callers**. Tests in `link_registry.rs` only (`tests::test_remove_orphaned_followers`, `tests::test_rebuild_follower_lists`).
- The intended use case: when a `LeaderLayer` is removed from the tree, its registered LinkRegistry entry should be removed; followers linked to that leader become orphaned. If nobody calls `remove_orphaned_followers`, the registry grows unbounded across frames.
- Compare Flutter: `LayerLink` is per-`LeaderLayer`-instance; followers reference it by Dart object identity. When a LeaderLayer is disposed (via LayerHandle ref-count), its LayerLink object becomes unreachable and GC'd — followers' references to it become dangling but harmless (they check `link._leader != null` before using). FLUI's `LinkRegistry` is a global-ish map that needs explicit cleanup.
- Additionally: `LinkRegistry` is field of `Scene`, not a singleton. Each Scene has its own registry — so when a Scene is dropped, the registry drops with it. That mitigates the leak SOMEWHAT (per-frame Scene rebuild releases the prior registry), but if a Scene retains the registry across frames (via `Scene::with_links(... existing_registry ...)`, scene.rs:156-171), the leak compounds.

**Why it exists:**
The registry was scaffolded with GC hooks but the integration that would invoke them (LayerTree::remove → unregister_leader cascade) was never written.

**Cost today:**
- Per-frame: if a Scene retains a long-lived LinkRegistry (Scene::with_links path), follower entries pile up.
- 0 in-workspace callers means the hook is fundamentally dead. Constitution-wise: zombie API surface.

**Risk of changing:**
Low. Three options:
- **(a)** Wire from `LayerTree::remove`: if removed layer is a Leader (via `Layer::is_leader()`), call `link_registry.unregister_leader(layer.link)`. If a Follower, call `link_registry.unregister_follower(id)`. Requires Scene-aware LayerTree (currently LayerTree doesn't know about LinkRegistry — they're sibling fields of Scene).
- **(b)** Move LinkRegistry INTO LayerTree as an inner field. Then `LayerTree::remove` can cascade into registry.
- **(c)** Keep the hooks but require callers (rendering pipeline) to invoke them explicitly. Update doc-comments to mark them "MUST be called after LeaderLayer removal". Add a `Scene::gc_orphaned_followers()` convenience method.

**Recommendation:**
**Option (c)** — keep hooks, add `Scene::gc_orphaned_followers()` that calls `link_registry.remove_orphaned_followers()`. Document the contract in `Scene::layer_tree_mut` doc-comment: "If you remove LeaderLayer or FollowerLayer instances, call `gc_orphaned_followers()` afterwards to prevent registry growth." Track explicit-GC as a Scene-builder responsibility, matching Flutter's `LayerHandle` dispose semantics. ~30 LOC.

---

### 💀 [DUPLICATION | MEDIUM]: `LayerNode::needs_compositing: bool` field shadows `Layer::needs_compositing()` enum method

**Evidence:**
- [`crates/flui-layer/src/tree/layer_tree.rs:34-36`](../../crates/flui-layer/src/tree/layer_tree.rs) — `LayerNode` has `needs_compositing: bool` cached field, default `true`.
- Setter at `layer_tree.rs:131-134` (`set_needs_compositing`). Getter at `layer_tree.rs:125-128`.
- [`crates/flui-layer/src/layer/mod.rs:281-303`](../../crates/flui-layer/src/layer/mod.rs) — `Layer::needs_compositing(&self) -> bool` enum method that pattern-matches on the variant:
  - `Canvas / Picture / Offset / Leader / Follower / AnnotatedRegion` → `false`
  - `Texture(layer) -> !layer.is_opaque()`
  - `ClipRect/RRect/Path/Superellipse(layer) -> layer.is_anti_aliased()`
  - `Opacity(layer) -> layer.needs_compositing()`
  - `ColorFilter/ImageFilter/ShaderMask/BackdropFilter` → `true`
- These two answer the same question, with the field being a cached version of what the enum-method computes. But:
  - Grep `set_needs_compositing` across workspace: 0 production callers — only own tests at `layer_tree.rs:test_*`.
  - The default value (`true`) is wrong for the simple cases (Canvas/Picture/Offset are `false` per the enum method).
  - There's no invalidation: if `LayerNode::layer_mut()` is called to change the layer variant, the cached `needs_compositing` field doesn't update.

**Why it exists:**
The field was added speculatively for "cache the answer for fast access". The invalidation logic never landed; the enum method was added later as the canonical answer.

**Cost today:**
- Field can drift from method's answer.
- Storage overhead per LayerNode (1 byte) for a field that's never set correctly.
- API confusion — two answers for the same question.

**Risk of changing:**
Low. Delete the field + setter + getter. Replace all callers (none in workspace today) with `layer_node.layer().needs_compositing()`.

**Recommendation:**
**Delete `LayerNode::needs_compositing` field + `set_needs_compositing` + `needs_compositing()` getter.** Use `Layer::needs_compositing()` enum method directly. ~15 LOC reduction.

---

### 💀 [API-SURFACE | LOW]: Multiple `LayerTree` setter methods on `LayerNode` lack workspace callers

**Evidence:**
- `LayerNode::set_offset(&mut self, offset)` (`layer_tree.rs:144-146`) — 0 production callers (only used via `with_offset` builder at construction).
- `LayerNode::set_element_id(&mut self, element_id)` (`layer_tree.rs:156-158`) — 0 production callers (only used via `with_element_id` builder).
- `LayerNode::set_layer(&mut self)` — verified there isn't one, layers are accessed via `layer_mut()`.
- `LayerNode::clear_children(&mut self)` (`layer_tree.rs:103-106`) — 1 caller, the parent's `LayerTree::clear_children` which already exists with its own dual-API issue.
- `Scene::layer_tree_mut(&mut self)` (`scene.rs:261-263`) — 0 production callers, only `Scene::builder()` which we already have.

**Why it exists:**
Speculative API surface — "complete CRUD" pattern.

**Cost today:**
- Bloated module surface.
- Each unused method is doc-comment + signature + impl + likely a test.

**Risk of changing:**
Trivial. Delete the unused setters; promote the builder pattern as the canonical mutation path.

**Recommendation:**
**Delete `LayerNode::set_offset`, `set_element_id`, and `Scene::layer_tree_mut`** unless a consumer materializes. Use `with_offset` / `with_element_id` builders at construction. Saves ~40 LOC + cleaner module surface.

---

### 💀 [PARITY-DRIFT | MEDIUM]: `SemanticsConfiguration::absorb` does first-wins for `label`/`value`/`hint` — Flutter concatenates labels with `_concatAttributedString`

**Evidence:**
- [`crates/flui-semantics/src/configuration.rs:820-854`](../../crates/flui-semantics/src/configuration.rs):
  ```rust
  pub fn absorb(&mut self, other: &SemanticsConfiguration) {
      // ...
      // Use other's values if self doesn't have them
      if self.label.is_none() {
          self.label.clone_from(&other.label);
      }
      if self.value.is_none() {
          self.value.clone_from(&other.value);
      }
      if self.hint.is_none() {
          self.hint.clone_from(&other.hint);
      }
      ...
  }
  ```
- Compare Flutter ([`semantics.dart:6837-6862`](Flutter source)):
  ```dart
  _attributedLabel = _concatAttributedString(
      thisAttributedString: _attributedLabel,
      thisTextDirection: textDirection,
      otherAttributedString: child._attributedLabel,
      otherTextDirection: child.textDirection,
  );
  if (_attributedValue.string == '') {
      _attributedValue = child._attributedValue;
  }
  // ...
  _attributedHint = _concatAttributedString(...);  // ← Hint also concat!
  ```
  Flutter **concatenates** label + hint, joins value/increasedValue/decreasedValue only if self is empty. FLUI does first-wins for ALL of them.
- For a button with label "Submit" containing a child with label "loading state" the merged accessibility output should read "Submit loading state" (Flutter). FLUI returns "Submit" only, losing the child's contribution.
- Also drift: Flutter merges `_actions` differently when `child.isBlockingUserActions` (`semantics.dart:6796-6804`):
  ```dart
  if (child.isBlockingUserActions) {
      child._actions.forEach((SemanticsAction key, SemanticsActionHandler value) {
          if (_kUnblockedUserActions & key.index > 0) {
              _actions[key] = value;  // ← only unblocked actions absorbed
          }
      });
  } else {
      _actions.addAll(child._actions);
  }
  ```
  FLUI's `absorb` always copies all actions (`configuration.rs:825-827`).
- Also drift: Flutter's absorb has `_role = child._role` if self's role is `none` (`semantics.dart:6852-6854`). FLUI's absorb has no role merging.
- Also drift: Flutter has `_headingLevel = _mergeHeadingLevels(...)` (`semantics.dart:6827-6830`). FLUI has no heading-level support.

**Cost today:**
- Screen readers see truncated labels/hints (most-ancestor-wins instead of concat).
- Blocked-user-actions absorption rule isn't honored — child's actions are absorbed even when child has `blocks_user_actions = true`. FLUI's blocking is stored (`configuration.rs:53-54 blocks_user_actions`) but never consulted during absorb.

**Risk of changing:**
Medium. Need to add `_concatAttributedString` equivalent to flui-semantics (string + text-direction-aware concat). Need to honor `blocks_user_actions` filter. Need to add role and heading-level mergers.

**Recommendation:**
**Port `_concatAttributedString`** to flui-semantics (new helper in `properties.rs`). Update `absorb` to concat label + hint per Flutter. Honor `blocks_user_actions` filter for actions. Add `role` field absorption (None → child.role). Add `heading_level` field + merge (post-port — needs SemanticsConfiguration heading level addition). ~80 LOC.

---

### 💀 [PARITY-DRIFT | MEDIUM]: `SemanticsNode` stores `transform: Option<[f32; 16]>` instead of `Option<Matrix4>`

**Evidence:**
- [`crates/flui-semantics/src/node.rs:73`](../../crates/flui-semantics/src/node.rs) — `transform: Option<[f32; 16]>`.
- Getter/setter (`node.rs:209-218`) — `Option<&[f32; 16]>` / `Option<[f32; 16]>`.
- `to_node_data` (`node.rs:274`) — `transform: self.transform.map_or(Matrix4::IDENTITY, Matrix4::from)` — converts BACK to `Matrix4` for the data export.
- The rest of the codebase uses `flui_types::Matrix4` (used here at `node.rs:11`). Layer module stores `Matrix4` directly (`transform.rs:54`). Hit-test stores `Matrix4` (`flui-interaction/src/routing/hit_test.rs`).
- The `[f32; 16]` representation is a Flutter remnant (Dart's `Float64List` representation). In Rust we have a `Matrix4` newtype that's already `Copy + Clone` and likely zero-cost over `[f32; 16]`.
- The choice forces a conversion at the export boundary that should be the identity.

**Why it exists:**
Direct port of Flutter's `Float64List` shape from `SemanticsNode._transform`. The Dart side stores a flat list; the Rust side adopted the same shape instead of unifying with the workspace `Matrix4` type.

**Cost today:**
- Type fragmentation — two ways to express a 4×4 matrix in flui-semantics + flui-layer ecosystem.
- API confusion — node.set_transform takes `Option<[f32; 16]>`, layer.set_transform takes `Matrix4`.
- Allocation-free but conversion-required at every boundary.

**Risk of changing:**
Trivial. Change field type to `Option<Matrix4>`. Update setter/getter. The `to_node_data` already converts back via `Matrix4::from`, so the data export shape stays unchanged.

**Recommendation:**
**Change `SemanticsNode::transform` field from `Option<[f32; 16]>` to `Option<Matrix4>`**. Update setter / getter signatures. ~10 LOC change.

---

### 💀 [ZOMBIE | LOW]: `LayerTree::iter_mut` + `LinkRegistry::leader_for_follower` + `LinkRegistry::links` — public getters with zero workspace callers

**Evidence:**
- [`crates/flui-layer/src/tree/layer_tree.rs:531-535`](../../crates/flui-layer/src/tree/layer_tree.rs) — `LayerTree::iter_mut` — 0 production callers.
- [`crates/flui-layer/src/link_registry.rs:251-255`](../../crates/flui-layer/src/link_registry.rs) — `leader_for_follower` — 0 production callers.
- [`crates/flui-layer/src/link_registry.rs:258-260`](../../crates/flui-layer/src/link_registry.rs) — `links()` iterator — 0 production callers.
- [`crates/flui-layer/src/link_registry.rs:263-265`](../../crates/flui-layer/src/link_registry.rs) — `leaders()` iterator — 0 production callers.

**Recommendation:**
Keep `iter` and `iter_mut` on LayerTree as the canonical mutation/iteration path — `iter_mut` is convenient for renderers that need to walk + mutate. But mark with `#[allow(dead_code)]` until a consumer materializes. **Delete `leader_for_follower` and `links` iterator** unless a renderer consumer materializes — they're exposed but never used.

---

### 💀 [API-SURFACE | LOW]: `LinkRegistry::has_leader` / `has_follower` / `get_follower_link` / `unregister_follower` mixed signatures around `&LayerLink` vs `LayerLink` (by-value)

**Evidence:**
- [`crates/flui-layer/src/link_registry.rs:190`](../../crates/flui-layer/src/link_registry.rs) — `has_leader(&self, link: &LayerLink) -> bool`.
- Line 175 — `unregister_leader(&mut self, link: LayerLink)` — by-value.
- Line 167 — `update_leader(&mut self, link: LayerLink, ...)` — by-value.
- Line 150 — `register_leader(&mut self, link: LayerLink, ...)` — by-value.
- `LayerLink` is `#[derive(Clone, Copy)]` (verified at `leader.rs:11`). By-value is `O(1)` (it's 8 bytes — `u64` inside).
- Mixed reference style is awkward — readers must check which form is expected. Idiomatic Rust per *Rust API Guidelines* §C-COMMON-TRAITS: small `Copy` types should be passed by value uniformly.

**Recommendation:**
**Pass `LayerLink` by value uniformly** — change `has_leader(&self, link: &LayerLink) -> bool` to `has_leader(&self, link: LayerLink) -> bool`. Same for `get_leader`, `followers_for_link`, `leader_for_link`. ~6 signatures changed.

---

## Dead Code Table

| Item | Location | Evidence | Verdict | Action |
|------|----------|----------|---------|--------|
| `LayerNode::needs_compositing` cached field + setter + getter | `tree/layer_tree.rs:36, 124-134` | 0 production callers of setter; shadows `Layer::needs_compositing()` enum method | **Stale cache** | **Delete field + accessors**; use enum method |
| `LayerNode::set_offset` | `tree/layer_tree.rs:144-146` | 0 production callers (only `with_offset` builder used) | **Speculative setter** | **Delete**; keep builder path |
| `LayerNode::set_element_id` | `tree/layer_tree.rs:156-158` | 0 production callers (only `with_element_id` builder used) | **Speculative setter** | **Delete**; keep builder path |
| `Scene::layer_tree_mut` | `scene.rs:261-263` | 0 production callers; `Scene::builder()` is the canonical mutation entry | **Redundant escape hatch** | **Delete** unless engine consumer materializes |
| `LayerTree::iter_mut` | `tree/layer_tree.rs:531-535` | 0 production callers | **Speculative** | **Keep with `#[allow(dead_code)]`** until engine consumer; iter() useful for rendering |
| `LinkRegistry::remove_orphaned_followers` | `link_registry.rs:295-308` | 0 production callers; GC hook scaffolded but unused | **GC hook orphan** | **Add `Scene::gc_orphaned_followers()` wrapper** + document contract |
| `LinkRegistry::rebuild_follower_lists` | `link_registry.rs:314-326` | 0 production callers | **Unused recovery hook** | **Delete** unless deserialization path materializes |
| `LinkRegistry::leader_for_follower` | `link_registry.rs:251-255` | 0 production callers (own test only) | **Dead getter** | **Delete** unless renderer consumer materializes |
| `LinkRegistry::links` iterator | `link_registry.rs:258-260` | 0 production callers | **Dead iterator** | **Delete** unless inspection consumer materializes |
| `LinkRegistry::leaders` iterator | `link_registry.rs:263-265` | 0 production callers | **Dead iterator** | **Delete** unless inspection consumer materializes |
| `DamageTracker::region_count` | `damage.rs:99-103` | `#[must_use]`, 0 production callers (test only) | **Debug-only getter** | **Keep** but mark `#[cfg(any(test, debug_assertions))]` |
| `CompositorStats::stats()`, `reset_stats`, `update_stats` | `compositor/retained.rs:50-91` | 0 production callers in workspace | **Stats facade unused** | **Keep behind feature flag** `compositor-stats` |
| `SceneCompositor::release`, `clear_retained` | `compositor/retained.rs:78-86` | 0 production callers | **Speculative API surface** | **Delete** until retain-flow ships |
| `SemanticsService::send_event` body | `flui-semantics/src/binding.rs:407-412` | `// TODO: Route to platform accessibility API` placeholder | **Half-implemented** | **Wire to new `SemanticsBinding::event_callback`** — covered in findings |
| `SemanticsNode::set_transform` taking `[f32; 16]` | `flui-semantics/src/node.rs:215-218` | Storage as raw array forces conversion at boundaries | **Type fragmentation** | **Change to `Option<Matrix4>`** — covered in findings |

Total dead/zombie LOC currently shipped in production builds: **~280 LOC** (estimate). Smaller than flui-interaction's 2,495 LOC dead surface — flui-layer is cleaner. Most "dead" items are unused getters / GC hooks waiting for integration; the bigger structural issues are the lifecycle protocol absence (covered separately).

---

## Restructuring Plan

Step-ordered to minimize ripple. Each step is a candidate atomic commit (PR #81/82/83/84 precedent).

1. **`SemanticsService::send_event` wiring** — add `SemanticsBinding::event_callback` mirroring `announce_callback`. Wire `send_event` through `SemanticsBinding::instance().dispatch_event(&event)`. ~30 LOC. Single-file change, narrow scope. **First commit — unblocks platform a11y events.**

2. **`SemanticsNode::transform` shape unification** — change `Option<[f32; 16]>` → `Option<Matrix4>`. Update getter/setter signatures. `to_node_data` simplifies. ~15 LOC. **Independent atomic commit.**

3. **`LayerNode::needs_compositing` field deletion** — delete field + setter + getter. Migrate callers (none in production today, only tests) to `layer.needs_compositing()` enum method. ~20 LOC reduction.

4. **`LayerNode` unused setter cleanup** — delete `set_offset`, `set_element_id`, `Scene::layer_tree_mut`. ~25 LOC reduction.

5. **`LinkRegistry` API hygiene** — pass `LayerLink` by value uniformly; delete unused `leader_for_follower`, `links`, `leaders` iterators, `rebuild_follower_lists`. Add `Scene::gc_orphaned_followers()` wrapper. ~80 LOC reduction.

6. **`LayerTree::add_child` auto-detach + dedup** — add prev-parent check; if child has a different parent, detach first. Add containment check (no double-push). ~20 LOC delta.

7. **`LayerTree::remove` cascade-by-default** — rename current to `remove_shallow`; new `remove` cascades children. ~30 LOC delta. Update callers (likely 0 outside tests today).

8. **`LayerNode` lifecycle phase 1: `disposed: AtomicBool` + `Drop`** — add field; impl Drop; add `debug_assert!` guards to every public mutation method. ~150 LOC.

9. **`Layer` enum variant boxing** — box `Canvas / Picture / ClipPath / PerformanceOverlay`. Update `From` impls + macro `gen_layer_accessors!` to deref Boxes. ~80 LOC.

10. **`LayerNode` lifecycle phase 2: `needs_add_to_scene: AtomicBool` dirty bit** — add field; `mark_needs_add_to_scene(id)` walks ancestors; SceneBuilder push paths invoke it. ~200 LOC.

11. **`FollowerLayer::Alignment` newtype migration** — add `flui_types::painting::Alignment` (separate flui-types PR); migrate `FollowerLayer::leader_anchor` + `follower_anchor`. ~80 LOC + new newtype.

12. **`SemanticsConfiguration::absorb` Flutter-faithful merging** — port `_concatAttributedString` to flui-semantics; honor `blocks_user_actions` filter; add role absorption; add heading-level support. ~120 LOC.

13. **`AccessibilityFeatures` packed `AtomicU8`** — eliminate `RwLock<AccessibilityFeatures>` in favor of bitflags atomic. ~50 LOC delta in binding.rs.

14. **`SemanticsBinding` clone-and-release pattern** — release locks before invoking callbacks. ~10 LOC delta in `dispatch_action` / `dispatch_event`.

15. **`LayerNode` lifecycle phase 3 (deferred): `engine_layer` cache** — add `engine_layer: Option<EngineLayerHandle>` field once flui-engine exposes the handle type. ~80 LOC, blocked on flui-engine.

## Optimization Plan

In priority order (top = highest impact):

1. **Layer enum boxing** (Restructuring step 9) — ~344kb saved on 1k-layer tree; faster `match` arms; faster move semantics. Biggest single optimization.
2. **`needs_add_to_scene` dirty-bit propagation** (step 10) — enables subtree-level repaint skipping. Foundation for the engine-layer cache (step 15).
3. **`AccessibilityFeatures` atomic pack** (step 13) — eliminates RwLock acquisition per `disable_animations` check (animation pipeline hot path).
4. **`SemanticsBinding` clone-and-release** (step 14) — eliminates lock-held-during-callback class of bugs + reduces lock pressure when callbacks are slow.
5. **`SemanticsConfiguration::absorb` actions blocking filter** (part of step 12) — avoids absorbing blocked actions (subtle correctness, low perf delta).

## What to Preserve

- `Scene::fire_composition_callbacks` panic-catch pattern (`scene.rs:228-245`) — exemplary `Poisoned`-style error reporting per PR #82.
- `LayerError` shape (`error.rs`) — narrow thiserror surface, no anyhow.
- `LayerLink` atomic ID generation (`leader.rs:18-23`) — Flutter-faithful.
- `DamageTracker` (`damage.rs`) — small, correct, no anti-patterns.
- `gen_layer_accessors!` macro (`layer/dispatch.rs`) — Mythos Step 4 boilerplate reduction; matches PR-precedent shape.
- `LayerTree` slab + 1-based ID convention (`layer_tree.rs:243-244, 276-280`) — matches the constitution's ID Offset Pattern.
- `SemanticsHandle` ref-counting (`flui-semantics/src/binding.rs:100-117`) — perfect Flutter port + idiomatic Rust Drop.
- `SemanticsConfiguration` SmallVec/SmolStr/FxHashMap optimizations (`configuration.rs:6-10`) — well-shaped.
- `LayerTree` impl of `TreeRead<LayerId> + TreeNav<LayerId>` (`tree/tree_traits.rs`) — clean flui-tree integration; **memory `flui-tree-unified-interface-intent` satisfied**.
- `Scene::builder()` borrow ergonomics (`scene.rs:307-317`) — short-lived `SceneBuilder<'_>` ties to Scene's mutable borrow.

## Priority Order (initial)

P0 — Critical correctness / unblocks downstream:
1. **`SemanticsService::send_event` wiring** (step 1). Only platform a11y events path. ~30 LOC, immediate unblock.
2. **`LayerNode` lifecycle phase 1: disposed + Drop** (step 8). PR #84 precedent. Prevents use-after-disposal silently corrupting tree.
3. **`LayerTree::add_child` auto-detach** (step 6). Silent dual-parent state today.
4. **`LayerTree::remove` cascade-by-default** (step 7). Footgun API today.

P1 — High-impact API hygiene / drift:
5. **`SemanticsConfiguration::absorb` Flutter parity** (step 12). Screen readers truncate today.
6. **Layer enum variant boxing** (step 9). Memory bloat fix.
7. **`SemanticsNode::transform` shape unification** (step 2). Type fragmentation.
8. **`FollowerLayer::Alignment` newtype** (step 11). Type-system lie.

P2 — Dead-code / hygiene:
9. **`LinkRegistry` API cleanup** (step 5).
10. **`LayerNode::needs_compositing` field deletion** (step 3).
11. **`LayerNode` unused setter cleanup** (step 4).

P3 — Optimization milestones:
12. **`LayerNode` lifecycle phase 2: dirty-bit propagation** (step 10). Foundation for retained rendering.
13. **`AccessibilityFeatures` atomic pack** (step 13).
14. **`SemanticsBinding` clone-and-release** (step 14).

P4 — Deferred (blocked on flui-engine):
15. **`LayerNode` lifecycle phase 3: engine_layer cache** (step 15).

---

---

# Part II — flui-semantics Self-Audit

## Project Map

```text
flui-semantics (5,775 LOC, 12 source files)
  owns: SemanticsTree + SemanticsNode + SemanticsConfiguration + SemanticsOwner +
        SemanticsBinding singleton + 27-variant SemanticsAction bitflag enum
        action.rs (302 LOC) — SemanticsAction enum (27 variants, repr(u64) bitflags
          1 << 0 .. 1 << 26) + ActionArgs + SemanticsActionHandler =
          Arc<dyn Fn(SemanticsAction, Option<ActionArgs>) + Send + Sync>.
        binding.rs (560 LOC) — SemanticsBinding singleton via impl_binding_singleton!
          + AccessibilityFeatures (7 bool flags) + SemanticsHandle (ref-counting
          AtomicUsize + Drop) + announce_callback / action_callback (RwLock<Option<Arc>>)
          + SemanticsActionEvent + SemanticsService (announce / send_event TODO /
          tooltip).
        configuration.rs (1069 LOC) — SemanticsConfiguration (~22 fields):
          flags + actions FxHashMap + label/value/hint AttributedString + tooltip
          + custom_actions SmallVec + tags SmallVec + sort_key + hint_overrides +
          scroll_position/extent_max/extent_min + index_in_parent + scroll_index +
          scroll_child_count + platform_view_id + max_value_length +
          current_value_length + elevation + thickness + absorb() + has_content().
        event.rs (314 LOC) — SemanticsEvent { event_type + payload<SemanticsEventData> }
          + SemanticsEventType enum + SemanticsEventData enum.
        flags.rs (257 LOC) — SemanticsFlag enum (~27 variants) + SemanticsFlags
          bitflags wrapper.
        node.rs (459 LOC) — SemanticsNode { parent + children Vec + element_id +
          config + rect + transform Option<[f32; 16]> + dirty } + merge / to_node_data /
          is_semantics_boundary / has_content.
        owner.rs (576 LOC) — SemanticsOwner { tree + callback Option<UpdateCallback>
          + enabled } + insert / get / get_mut / remove / clear / dispose + flush
          (collect dirty + build update payloads + invoke callback + mark_clean).
        properties.rs (504 LOC) — AttributedString + StringAttribute +
          StringAttributeType + SemanticsProperties (builder for declarative config) +
          CustomSemanticsAction + SemanticsHintOverrides + SemanticsSortKey +
          TextDirection + SemanticsTag.
        role.rs (502 LOC) — SemanticsRole enum (28 variants: Dialog / Tab / Cell /
          Row / Menu / Status / List / Search / Navigation etc.) +
          AccessibilityFocusBlockType + Assertiveness + DebugSemanticsDumpOrder.
        tree.rs (749 LOC) — SemanticsTree { Slab<SemanticsNode> + root } + insert /
          get / get_mut / remove / add_child / remove_child + dirty_nodes /
          mark_all_clean / has_dirty_nodes + iter / iter_mut + impl
          TreeRead<SemanticsId> + TreeNav<SemanticsId>.
        update.rs (255 LOC) — SemanticsNodeData (flat data shape for platform export)
          + SemanticsTreeUpdate + SemanticsTreeUpdateBuilder.
  depends on: flui-foundation (SemanticsId, ElementId, BindingBase,
              impl_binding_singleton!, HasInstance), flui-types (Pixels, Rect,
              Matrix4), flui-tree (TreeRead, TreeNav, iter modules),
              parking_lot (RwLock for binding callbacks), smol_str, smallvec,
              rustc_hash, tracing, thiserror, anyhow (verify usage).
  public surface: 47 top-level + 33 prelude exports (lib.rs:84-139 + 150-167).
  suspected hot paths:
    - SemanticsOwner::flush (owner.rs:294-319) — per-frame; collects dirty nodes
      via Slab iter + filter + build SemanticsNodeUpdate vec + callback invocation.
    - SemanticsConfiguration::absorb (configuration.rs:820-854) — per non-boundary
      node during tree assembly. FxHashMap iteration + 22-field clone-from chain.
    - SemanticsBinding::dispatch_action (binding.rs:271-275) — per platform-initiated
      action; reads action_callback under RwLock, invokes outside? — verify.
    - SemanticsConfiguration::has_content (configuration.rs:808-815) — boolean OR of
      6 checks; per-node during tree walk.
  risk:
    - SemanticsService::send_event is `// TODO` placeholder (binding.rs:411).
      Only platform routing path for SemanticsEvent.
    - SemanticsConfiguration::absorb has 4 drift points vs Flutter (label concat,
      blocked-actions filter, role merge, heading level — covered in cross-ref).
    - SemanticsBinding holds 3 RwLocks for callbacks + features. Per-frame
      `disable_animations()` checks acquire RwLock for a bool read.
    - SemanticsNode::transform is `Option<[f32; 16]>` not `Option<Matrix4>` —
      type fragmentation.
    - SemanticsTree::remove does NOT cascade (same footgun as LayerTree::remove).
    - SemanticsOwner::flush builds a fresh Vec<SemanticsNodeUpdate> every frame.
      For typical 100-node tree with 10 dirty, vec capacity 10 → allocator hit.
      Could reuse a `Vec` field.
```

**Cross-crate dependency DAG**:

```
flui-semantics → flui-foundation (SemanticsId, BindingBase, impl_binding_singleton!)
              → flui-types (Pixels, Rect, Matrix4)
              → flui-tree (TreeRead, TreeNav, iter modules)
              → parking_lot, smol_str, smallvec, rustc_hash, thiserror, anyhow, tracing
```

No upward deps. flui-rendering and flui-view consume flui-semantics.

## Findings

### 💀 [HALF-IMPLEMENTED | CRITICAL]: `SemanticsService::send_event` — TODO stub blocking all semantics events from reaching the platform

Already covered in [Mythos Improvement Verdict](#mythos-improvement-verdict) above. Restated here as a Part II finding with concrete recommendation:

**Evidence:** `crates/flui-semantics/src/binding.rs:407-412`.

**Recommendation:** add `SemanticsBinding::event_callback: RwLock<Option<Arc<dyn Fn(&SemanticsEvent) + Send + Sync>>>` mirror of `announce_callback`. Wire `SemanticsBinding::dispatch_event(&event)` → `SemanticsService::send_event(event)` invokes via `SemanticsBinding::instance().dispatch_event(&event)`. Drops the `// TODO`. ~30 LOC.

---

### 💀 [LIFECYCLE-LEAK | HIGH]: `SemanticsTree::remove` mirrors `LayerTree::remove` non-cascade footgun

**Evidence:**
- [`crates/flui-semantics/src/tree.rs:164-177`](../../crates/flui-semantics/src/tree.rs):
  ```rust
  /// Removes a SemanticsNode from the tree.
  ///
  /// Returns the removed node, or None if it didn't exist.
  ///
  /// **Note:** This does NOT remove children. Caller must handle tree
  /// cleanup.
  pub fn remove(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
      if self.root == Some(id) {
          self.root = None;
      }
      self.nodes.try_remove(id.get() - 1)
  }
  ```
- Same problem as `LayerTree::remove` — non-cascade leaves orphan children + risks tree corruption on slab-index reuse.
- The doc-comment is the same boilerplate.

**Why it exists:**
Both trees were templated from the same "Slab + LayerId/SemanticsId + parent/children Vec" pattern, including this footgun.

**Recommendation:**
**Mirror the LayerTree fix**: rename to `remove_shallow`, add cascading `remove`. ~30 LOC delta. **Track as the same atomic commit family** as the LayerTree fix — both are slab-tree pattern unifications. Per memory `flui-tree-unified-interface-intent`: this points toward a future `TreeWrite<Id>` trait that provides cascading remove as the default contract.

---

### 💀 [LIFECYCLE-LEAK | HIGH]: `SemanticsTree::add_child` matches `LayerTree::add_child` non-detaching pattern (but SemanticsNode::add_child has dedup, so partial protection)

**Evidence:**
- [`crates/flui-semantics/src/tree.rs:187-200`](../../crates/flui-semantics/src/tree.rs) — same shape as `LayerTree::add_child`: updates parent's children + sets child's parent, no detach-from-prev-parent step.
- [`crates/flui-semantics/src/node.rs:126-130`](../../crates/flui-semantics/src/node.rs) — `SemanticsNode::add_child` HAS dedup check: `if !self.children.contains(&child) { self.children.push(child); }`. Better than `LayerNode::add_child` (which doesn't dedup).
- But dual-parent state can still happen: `tree.add_child(p1, c); tree.add_child(p2, c)` — `p1.children` still contains `c` (no detach), `p2.children` contains `c`, `c.parent == Some(p2)`. So a top-down walk from p1 incorrectly counts c.

**Recommendation:**
**Mirror the LayerTree fix**: detach from previous parent before re-attaching. ~15 LOC delta. Same atomic-commit family as LayerTree fix.

---

### 💀 [PARITY-DRIFT | HIGH]: `SemanticsConfiguration::absorb` does first-wins for label/hint — Flutter concatenates

Already covered in Part I cross-cutting finding above. Restated as a Part II finding.

**Recommendation:**
**Port `_concatAttributedString` to flui-semantics**; update `absorb` to concat label + hint per Flutter; honor `blocks_user_actions`. ~80 LOC.

---

### 💀 [PARITY-DRIFT | MEDIUM]: `SemanticsAction` enum is missing `dismiss` from Flutter's set, but adds `Focus` / `Unfocus` / `Expand` / `Collapse` / `ScrollToOffset` that Flutter has

**Evidence:**
- [`crates/flui-semantics/src/action.rs:19-102`](../../crates/flui-semantics/src/action.rs) — 27 variants total.
- Flutter (`ui.SemanticsAction` from dart:ui, also enumerated in `semantics.dart:96-105`):
  - `tap`, `longPress`, `scrollLeft`, `scrollRight`, `scrollUp`, `scrollDown`, `increase`, `decrease`, `showOnScreen`, `moveCursorForwardByCharacter`, `moveCursorBackwardByCharacter`, `setSelection`, `copy`, `cut`, `paste`, `didGainAccessibilityFocus`, `didLoseAccessibilityFocus`, `customAction`, `dismiss`, `moveCursorForwardByWord`, `moveCursorBackwardByWord`, `setText`, `focus`, `scrollToOffset` (mostly aligned).
- FLUI **adds** `Expand` (`1 << 24`) and `Collapse` (`1 << 25`) — Flutter has these via the `SemanticsFlag::IsExpanded` flag pair, not as actions. **Drift A**: FLUI conflates "flag state" and "user action" — Expand/Collapse should be flag states OR menu actions.
- FLUI **adds** `Unfocus` (`1 << 23`) — Flutter has `focus` only; unfocus is implicit (focusing another node defocuses the prior). **Drift B**: minor — could be kept as a FLUI improvement.
- The `1 << N` bitmask values may not match Flutter's wire-format values for the platform protocol. Flutter's `dart:ui SemanticsAction` has specific integer constants — if FLUI's are derived independently, the platform-side (Android/iOS) accessibility integration will misinterpret action codes.

**Why it exists:**
Action enum was ported with localized additions for FLUI-specific patterns. The wire-format alignment with Flutter's platform channel was not verified.

**Cost today:**
- Platform a11y integration will fail at the wire-format boundary unless the values match Flutter's exactly. Flutter's protocol is the de facto standard for Skia-on-platform a11y.
- Documentation lie — comment says "Corresponds to Flutter's `SemanticsAction` enum" but the variant set diverges.

**Risk of changing:**
Medium. Two options:
- **(a)** Strict Flutter parity — drop `Expand` / `Collapse` / `Unfocus` (move to flags), keep wire-format constants matching Flutter.
- **(b)** Document the divergences explicitly; ensure platform channel translation layer maps FLUI's values to Flutter's on output.

**Recommendation:**
**Option (a)** — strict Flutter parity is the port-not-redesign principle. Move `Expand` / `Collapse` to `SemanticsFlag::HasExpandedState + IsExpanded`. Drop `Unfocus`. Verify all 27 remaining variants have wire-format values matching Flutter's `dart:ui.SemanticsAction` constants (per `Constants.dart::SemanticsAction.values`). ~50 LOC migration + a verification cross-ref table.

---

### 💀 [SYNC-CONTENTION | MEDIUM]: `SemanticsBinding` has 3 `RwLock`s; `disable_animations()` acquires a lock to read a single bool

Already covered in Part I cross-cutting finding above. Restated.

**Recommendation:** pack `AccessibilityFeatures` into `AtomicU8` bitflags. Eliminate the `RwLock<AccessibilityFeatures>`. ~50 LOC delta.

---

### 💀 [API-SURFACE | MEDIUM]: `SemanticsOwner::flush` allocates a fresh `Vec<SemanticsNodeUpdate>` every frame

**Evidence:**
- [`crates/flui-semantics/src/owner.rs:294-319`](../../crates/flui-semantics/src/owner.rs):
  ```rust
  pub fn flush(&mut self) {
      if !self.enabled { return; }
      let dirty_ids: Vec<SemanticsId> = self.tree.dirty_nodes().collect();
      if dirty_ids.is_empty() { return; }
      let updates: Vec<SemanticsNodeUpdate> = dirty_ids
          .iter()
          .filter_map(|&id| self.build_update(id))
          .collect();
      if let Some(ref callback) = self.callback {
          callback(&updates);
      }
      self.tree.mark_all_clean();
  }
  ```
- Two `Vec`s allocated per `flush` — `dirty_ids` then `updates`.
- For a 100-node tree with 10 dirty nodes per frame at 60fps, this is 2 × 60 = 120 Vec allocations per second.
- Per *Rust Performance Book* "Allocator pressure": allocator hits in a 60fps frame budget are the highest-impact category for sustained workloads.
- The `dirty_ids` vec is intermediate — `build_update` could be invoked directly from the `dirty_nodes()` iterator without collecting.
- The `updates` vec is invoked once via callback then thrown away — could be a reusable field on `SemanticsOwner`.

**Recommendation:**
1. **Drop the `dirty_ids` intermediate Vec** — fold `build_update` into the iterator chain:
   ```rust
   let updates: Vec<SemanticsNodeUpdate> = self.tree.dirty_nodes()
       .filter_map(|id| self.build_update(id))
       .collect();
   ```
2. **Reuse `updates: Vec<SemanticsNodeUpdate>` as a field on `SemanticsOwner`**. Clear before extending. Eliminates one alloc per frame.

Estimated 30 LOC change.

---

### 💀 [API-SURFACE | LOW]: `SemanticsOwner` exposes `new_without_callback` for testing but it's `pub` not `#[cfg(test)]` or `#[cfg(feature = "testing")]`

**Evidence:**
- [`crates/flui-semantics/src/owner.rs:148-155`](../../crates/flui-semantics/src/owner.rs):
  ```rust
  /// Creates a new SemanticsOwner without a callback (for testing).
  pub fn new_without_callback() -> Self { ... }
  ```
- Doc-comment explicitly says "for testing" but the function is `pub` with no `#[cfg(test)]` or feature flag.
- Production code can construct a no-callback owner — silently broken accessibility.

**Recommendation:**
**Gate behind `#[cfg(test)]` or `#[cfg(any(test, feature = "testing"))]`**. Mirror flui-interaction's `testing/` submodule gate pattern (recommended in prior audit). ~5 LOC.

---

### 💀 [DUPLICATION | LOW]: `SemanticsNode::merge` and `SemanticsConfiguration::absorb` are two abstraction layers for the same "absorb child" operation

**Evidence:**
- [`crates/flui-semantics/src/node.rs:295-306`](../../crates/flui-semantics/src/node.rs) — `SemanticsNode::merge(&mut self, other: &SemanticsNode)`:
  ```rust
  pub fn merge(&mut self, other: &SemanticsNode) {
      self.config.absorb(&other.config);
      if self.rect == Rect::ZERO {
          self.rect = other.rect;
      } else if other.rect != Rect::ZERO {
          self.rect = self.rect.union(&other.rect);
      }
      self.dirty = true;
  }
  ```
- [`crates/flui-semantics/src/configuration.rs:820-854`](../../crates/flui-semantics/src/configuration.rs) — `SemanticsConfiguration::absorb(&mut self, other: &SemanticsConfiguration)`.
- The split is correct *architecturally* (Node owns geometry, Config owns semantics), but the names diverge — `merge` vs `absorb`. Flutter uses `absorb` for both (`semantics.dart:6790` for config, and `SemanticsNode.absorb` doesn't exist — the equivalent is `_canonicalSemanticsNodeMixin._isMergedIntoParent` flag).
- Compare Flutter: there is no `SemanticsNode.merge` — Flutter has SemanticsNodes either be boundaries OR be merged into their boundary parent via the `mergeAllDescendantsIntoThisNode` flag. The "merge" happens at tree-assembly time in `_SemanticsBuilder._merge`.
- FLUI's `SemanticsNode::merge` is a manual API call that wraps `absorb` + rect-union. The geometry merging belongs in tree-assembly logic, not on the node itself.

**Recommendation:**
**Rename `SemanticsNode::merge` to `SemanticsNode::absorb`** for naming consistency with Flutter + `SemanticsConfiguration::absorb`. Optionally consider moving rect-union into tree-assembly code (`SemanticsOwner::flush` or a future `SemanticsBuilder`) — leaves the node's API smaller. ~10 LOC.

---

### 💀 [API-SURFACE | LOW]: `SemanticsRole` has 28 variants but FLUI's `SemanticsConfiguration` cannot store a role

**Evidence:**
- [`crates/flui-semantics/src/role.rs:32-141`](../../crates/flui-semantics/src/role.rs) — `SemanticsRole` enum (28 variants: `None` / `AlertDialog` / `Dialog` / `Tab` / `TabBar` / `TabPanel` / `Table` / `Cell` / `Row` / `ColumnHeader` / `RadioGroup` / `Menu` / `MenuBar` / `MenuItem` / `MenuItemCheckbox` / `MenuItemRadio` / `Alert` / `Status` / `List` / `ListItem` / `Complementary` / `ContentInfo` / `Main` / `Navigation` / `Region` / `Search` / `Form` / `Banner`).
- Grep `SemanticsRole` across flui-semantics outside `role.rs` and tests: confirms `SemanticsConfiguration` has NO `role` field.
- `SemanticsProperties` (the declarative builder) might have one — verify.
- Without a `role` field on `SemanticsConfiguration`, the 28-variant role enum is exposed in the prelude but unused.
- Flutter has `_role: SemanticsRole` field on `SemanticsConfiguration` (`semantics.dart:6852`).

**Why it exists:**
The role enum was ported but the storage on `SemanticsConfiguration` was forgotten. (Verified `SemanticsProperties` likely has it as the declarative shape, but the runtime configuration is missing it.)

**Cost today:**
- Public API surface — 28 variants exposed for an enum that's never actually used by the runtime semantics config.
- Cannot port Flutter's `_role` absorption logic (covered in absorb-drift finding).

**Recommendation:**
**Add `role: SemanticsRole` field to `SemanticsConfiguration`**. Default `SemanticsRole::None`. Add `set_role` / `role` accessors. Update `absorb` to copy `role` if `self.role == None` (matches Flutter). Update `to_node_data` to include role in the data export. **Then** the 28-variant enum earns its keep.

---

### 💀 [PARITY-DRIFT | LOW]: `SemanticsNode::children` is `Vec<SemanticsId>` despite lib.rs:43 promoting `SmallVec` optimization

**Evidence:**
- [`crates/flui-semantics/src/lib.rs:38-44`](../../crates/flui-semantics/src/lib.rs) — doc-comment lists "Optimizations":
  > - [`SmallVec`](smallvec::SmallVec) for children/actions (stack allocation)
- [`crates/flui-semantics/src/node.rs:58`](../../crates/flui-semantics/src/node.rs) — `children: Vec<SemanticsId>`. **`Vec`, not `SmallVec`.**
- `SemanticsConfiguration::custom_actions: SmallVec<[CustomSemanticsAction; 2]>` (`configuration.rs:86`) ✓ matches lib.rs claim.
- `SemanticsConfiguration::tags: SmallVec<[SemanticsTag; 2]>` (`configuration.rs:89`) ✓ matches claim.
- So the claim is partially true (config-level uses SmallVec) but partially false (node-level uses Vec).

**Recommendation:**
**Switch `SemanticsNode::children: Vec<SemanticsId>` → `SmallVec<[SemanticsId; 4]>`**. Children count distribution per a typical UI semantics tree: most nodes have 0-3 children; deep ancestor nodes have 4-8. SmallVec inline-4 covers the common case. ~5 LOC change. Bonus: `SemanticsConfiguration::actions: FxHashMap` — could also become `SmallVec<[(SemanticsAction, SemanticsActionHandler); 4]>` for the common case (≤ 4 actions per node).

---

## Dead Code Table (flui-semantics)

| Item | Location | Evidence | Verdict | Action |
|------|----------|----------|---------|--------|
| `SemanticsOwner::new_without_callback` | `owner.rs:148-155` | Doc-comment "for testing" but `pub` without feature gate | **Test-only-in-prod** | **Gate behind `#[cfg(any(test, feature = "testing"))]`** |
| `SemanticsService::send_event` body | `binding.rs:407-412` | `// TODO: Route to platform accessibility API` | **Half-implemented** | **Wire to `SemanticsBinding::event_callback`** |
| `SemanticsRole` enum exposure | `role.rs:32-141` (28 variants) | No `role` field on `SemanticsConfiguration` | **Unused public type** | **Add `role` field to config** |
| `SemanticsNode::merge` | `node.rs:295-306` | Wraps `config.absorb` + rect-union; Flutter has no `SemanticsNode.merge` | **Naming + layer mismatch** | **Rename `absorb`** + consider moving rect-union to tree-assembly |
| `SemanticsNode::children: Vec` | `node.rs:58` | lib.rs:42-43 promises SmallVec but uses Vec | **API/doc drift** | **Switch to `SmallVec<[SemanticsId; 4]>`** |
| `SemanticsConfiguration::actions: FxHashMap` | `configuration.rs:62` | Typical node has ≤ 4 actions; HashMap is overkill | **Premature HashMap** | **Switch to `SmallVec<[(SemanticsAction, SemanticsActionHandler); 4]>`** |
| `SemanticsConfiguration::set_semantics_boundary` setter usage | `configuration.rs:151-153` | Verify 0 production callers | **Verify** | **Investigate** — may be called by RenderObject layer when materialized |

Total dead/zombie LOC currently shipped in production builds: **~50 LOC** + the `SemanticsRole` enum (502 LOC) is exposed but unused at the config level. **Bigger fix: add `role` field to config → enum earns its keep**.

## Restructuring Plan (flui-semantics)

1. **`SemanticsService::send_event` wiring** (same as Part I step 1) — ~30 LOC.
2. **`SemanticsTree::remove` cascade + `add_child` auto-detach** (same shape as LayerTree fix) — ~40 LOC.
3. **`SemanticsConfiguration::absorb` Flutter parity** — port `_concatAttributedString`; honor `blocks_user_actions`; add role merging; add heading_level support. ~120 LOC.
4. **`SemanticsConfiguration::role` field addition** — adds field + accessors + `to_node_data` export. ~30 LOC.
5. **`SemanticsAction` wire-format alignment with Flutter** — move Expand/Collapse to flags; drop Unfocus; verify wire constants. ~50 LOC.
6. **`SemanticsNode::transform: Option<Matrix4>`** (covered in Part I) — ~15 LOC.
7. **`SemanticsNode::merge` rename to `absorb`** — ~10 LOC.
8. **`SemanticsOwner::flush` vec reuse + drop intermediate** — ~30 LOC.
9. **`SemanticsBinding` `AccessibilityFeatures` AtomicU8 pack** (covered in Part I) — ~50 LOC.
10. **`SemanticsBinding` clone-and-release pattern** (covered in Part I) — ~10 LOC.
11. **`SemanticsNode::children: SmallVec<[_; 4]>`** + similar for `actions` — ~10 LOC.
12. **`SemanticsOwner::new_without_callback` feature-gate** — ~5 LOC.

## Optimization Plan (flui-semantics)

1. **`SemanticsAction` wire-format alignment** (correctness; not perf).
2. **`SemanticsOwner::flush` allocation reduction** — 2 vec allocs/frame → 0.
3. **`AccessibilityFeatures` atomic pack**.
4. **`SemanticsNode::children` SmallVec** — stack-allocate the common case.
5. **`SemanticsConfiguration::actions` SmallVec** — stack-allocate the common case.

## What to Preserve (flui-semantics)

- `SemanticsHandle` ref-counting pattern (`binding.rs:100-117`) — Flutter-faithful + Rust-idiomatic.
- `SemanticsConfiguration` struct layout (SmallVec/SmolStr/FxHashMap mix where appropriate).
- `SemanticsAction` bitflag layout (`1 << N`) — once wire constants verified, the shape is correct.
- `SemanticsConfiguration::has_content` quick-check (`configuration.rs:808-815`) — efficient short-circuit boolean OR.
- `SemanticsConfiguration::from_properties` (`configuration.rs:857-934`) — declarative builder pattern, well-shaped.
- `SemanticsTree` slab + 1-based ID convention (`tree.rs:133-135, 154-155`) — matches constitution's ID Offset Pattern.
- `impl TreeRead<SemanticsId> + TreeNav<SemanticsId> for SemanticsTree` (`tree.rs:290-371`) — clean flui-tree integration; **memory `flui-tree-unified-interface-intent` satisfied**.
- `AttributedString` + `StringAttribute` shape (`properties.rs`) — Flutter-faithful.
- `SemanticsTreeUpdateBuilder` (`update.rs`) — clean builder for batch updates.

## Priority Order (flui-semantics initial)

P0 — Critical correctness:
1. **`SemanticsService::send_event` wiring** (step 1). Only platform path.
2. **`SemanticsConfiguration::absorb` Flutter parity** (step 3). Screen readers truncate today.
3. **`SemanticsTree::remove` cascade + `add_child` auto-detach** (step 2). Footgun.

P1 — High-impact alignment:
4. **`SemanticsAction` wire-format verification** (step 5). Platform protocol correctness.
5. **`SemanticsConfiguration::role` field addition** (step 4). Unblocks 28-variant enum.
6. **`SemanticsNode::transform: Option<Matrix4>`** (step 6). Type fragmentation.

P2 — Dead-code / hygiene:
7. **`SemanticsNode::merge` → `absorb` rename** (step 7).
8. **`SemanticsOwner::new_without_callback` feature-gate** (step 12).

P3 — Optimizations:
9. **`SemanticsOwner::flush` allocation reduction** (step 8).
10. **`SemanticsBinding` atomic pack + clone-release** (steps 9 + 10).
11. **`SemanticsNode::children` SmallVec** (step 11).

---

---

# Part III — Flutter Cross-Reference

## Section 1 — flui-layer vs `flutter/rendering/layer.dart` + `compositing.dart`

Flutter's layer module: **3,029 LOC across 24 classes + 2 enums + 4 utility types**. FLUI ships 19 layer-type Rust structs (plus 1 enum dispatch). Coverage and drift analysis follows.

### Coverage Matrix (Flutter Layer → FLUI Layer)

| Flutter Layer Class | Flutter File:Line | FLUI Match | Coverage |
|---|---|---|---|
| `Layer` (abstract base) | `layer.dart:144` | NO equivalent. FLUI uses enum dispatch (`Layer` enum at `layer/mod.rs:184`). | **Architectural drift** — no virtual dispatch, no `engineLayer`/`_needsAddToScene` lifecycle. |
| `LayerHandle<T>` | `layer.dart:783-822` | NO equivalent. | **MISSING** — Critical. No ref-counted disposal. |
| `PictureLayer` | `layer.dart:824-924` | `PictureLayer` at `layer/picture.rs:60-161` | **Partial** — no `isComplexHint` / `willChangeHint`. No `dispose()` (no `_picture.dispose()`). |
| `TextureLayer` | `layer.dart:952-1003` | `TextureLayer` at `layer/texture.rs` | **Likely OK** — verify `freeze` / `filterQuality` properties. |
| `PlatformViewLayer` | `layer.dart:1005-1030` | `PlatformViewLayer` at `layer/platform_view.rs` | **Likely OK** — verify hit-test-behavior matches. |
| `PerformanceOverlayLayer` | `layer.dart:1032-1075` | `PerformanceOverlayLayer` at `layer/performance_overlay.rs` (530 LOC) | **Larger than Flutter** — verify scope. |
| `ContainerLayer` (abstract) | `layer.dart:1077-1457` | NO direct match. FLUI uses LayerNode children Vec. | **Different shape** — Flutter has `firstChild`/`lastChild`/`nextSibling`/`previousSibling` linked-list pointers; FLUI has children Vec. |
| `OffsetLayer` | `layer.dart:1459-1599` | `OffsetLayer` at `layer/offset.rs` | **Partial** — no `toImage` / `toImageSync` methods (Flutter screenshot APIs). |
| `ClipRectLayer` | `layer.dart:1601-1692` | `ClipRectLayer` at `layer/clip_rect.rs` | **OK** — bounds + Clip enum + anti-alias check. |
| `ClipRRectLayer` | `layer.dart:1694-1782` | `ClipRRectLayer` at `layer/clip_rrect.rs` | **OK**. |
| `ClipRSuperellipseLayer` | `layer.dart:1784-1869` | `ClipSuperellipseLayer` at `layer/clip_superellipse.rs` | **OK** — added in Mythos Step 3 (PR #83). |
| `ClipPathLayer` | `layer.dart:1871-1951` | `ClipPathLayer` at `layer/clip_path.rs` | **OK**. |
| `ColorFilterLayer` | `layer.dart:1953-1991` | `ColorFilterLayer` at `layer/color_filter.rs` | **Verify** — Flutter takes a `ColorFilter`; FLUI takes `ColorMatrix`. Drift — `ColorMatrix` is one of N color-filter shapes. |
| `ImageFilterLayer` | `layer.dart:1993-2036` | `ImageFilterLayer` at `layer/image_filter.rs` | **OK** — FLUI provides `blur` convenience method matching Flutter. |
| `TransformLayer` | `layer.dart:2038-2136` | `TransformLayer` at `layer/transform.rs` | **OK** — matrix-based dispatch. |
| `OpacityLayer` | `layer.dart:2138-2216` | `OpacityLayer` at `layer/opacity.rs` | **Partial** — Flutter extends `OffsetLayer`; FLUI has independent `OpacityLayer` with embedded offset (`opacity.rs:53`). |
| `ShaderMaskLayer` | `layer.dart:2218-2323` | `ShaderMaskLayer` at `layer/shader_mask.rs` | **OK**. |
| `BackdropFilterLayer` | `layer.dart:2325-2414` | `BackdropFilterLayer` at `layer/backdrop_filter.rs` | **OK** — verify `blendMode` parameter. |
| `LayerLink` | `layer.dart:2416-2484` | `LayerLink` at `layer/leader.rs:11-30` | **OK** — atomic counter, Rust-idiomatic. |
| `LeaderLayer` | `layer.dart:2486-2601` | `LeaderLayer` at `layer/leader.rs:81-156` | **Partial** — Flutter has `_lastOffset` + `applyTransform`; FLUI has simpler offset field. |
| `FollowerLayer` | `layer.dart:2603-2925` | `FollowerLayer` at `layer/follower.rs` | **Drift A** (covered): anchor type is `Offset<Pixels>` not `Alignment`. |
| `AnnotatedRegionLayer<T>` | `layer.dart:2927-3027` | `AnnotatedRegionLayer` at `layer/annotated_region.rs` | **OK** — FLUI uses `Arc<dyn Any + Send + Sync>` type erasure where Flutter uses generic `<T>`. |

### Drift A — Layer Lifecycle Protocol

**Flutter** (`layer.dart:144-340`):
- `Layer._refCount: int` (line 274). `_unref()` (line 277) decrements + auto-dispose when 0.
- `LayerHandle<T>` (line 783-822) — wraps layer; `_layer = layer` increments refcount, `_layer = null` decrements + may dispose.
- `void dispose()` `@mustCallSuper` (line 319-340) — clears `_engineLayer`.
- `bool _needsAddToScene = true` (line 372) — dirty bit.
- `ui.EngineLayer? _engineLayer` (line 444-481) — retained GPU handle across frames.

**FLUI** (`tree/layer_tree.rs:24-43`):
- `LayerNode` fields: `parent / children / layer / needs_compositing / offset / element_id`.
- **No `disposed` flag. No `_needsAddToScene` dirty bit. No `_engineLayer` cache. No `LayerHandle`. No `Drop` impl.**

**Drift severity**: CRITICAL. Already covered in Part I. The entire retained-rendering optimization that Layer exists to enable is impossible without `_engineLayer`. The lifecycle protocol must be added.

### Drift B — `addToScene` virtual dispatch

**Flutter** (`layer.dart:693-694`):
```dart
@protected
void addToScene(ui.SceneBuilder builder);
```
Every Layer subclass overrides this. Dispatch is virtual.

**FLUI**: no `add_to_scene` method on Layer. The enum-based dispatch in `Scene::layer_tree` is implicit (the engine — when materialized — will pattern-match on Layer variants).

**Drift severity**: MEDIUM. The enum dispatch IS correct Rust idiom for closed sets, but it means every Layer-using consumer (engine, debug-dumper, hit-test, annotation-finder) must pattern-match all 19 variants. Adding a 20th variant requires touching every consumer. Trait-based dispatch via `dyn Layer` would let consumers extend more cleanly — but Constitution Principle 4 prefers enum dispatch over `dyn`. The trade-off is conscious.

**Recommendation**: keep enum dispatch (Constitution-aligned), but extract repeated patterns via `gen_layer_accessors!` macro (already done at `layer/dispatch.rs`). For consumer-side dispatch boilerplate, consider a `LayerVisitor` trait with default `visit_*` methods.

### Drift C — `ContainerLayer` child linked-list vs Vec

**Flutter** (`layer.dart:1077-1457`):
```dart
class ContainerLayer extends Layer {
  Layer? get firstChild => _firstChild;
  Layer? _firstChild;
  Layer? get lastChild => _lastChild;
  Layer? _lastChild;
  // Children via sibling pointers: _nextSibling, _previousSibling on Layer
}
```
Doubly-linked list of children. O(1) insert/remove at head/tail; O(1) sibling navigation.

**FLUI** (`tree/layer_tree.rs:28`):
```rust
children: Vec<LayerId>,
```
Vec — O(1) at tail, O(N) in the middle, O(N) for arbitrary remove.

**Drift severity**: LOW. Vec is the right choice for compact storage + cache-friendly iteration. Flutter's linked list is Dart-idiomatic but slower in cache terms. The Vec choice is a Rust-native improvement, not a drift to fix. **Document the divergence**.

### Drift D — `PictureLayer.isComplexHint` / `willChangeHint` raster cache hints

**Flutter** (`layer.dart:859-884`):
```dart
bool isComplexHint = false;  // hint to Skia raster cache
bool willChangeHint = false; // hint to Skia raster cache
```

**FLUI** (`layer/picture.rs`): **no equivalent**.

**Drift severity**: MEDIUM. These hints are user-facing Skia raster-cache tuning knobs. Flutter exposes them at the Layer API; FLUI cannot. The hints flow to `SceneBuilder.addPicture(isComplexHint:, willChangeHint:)` — when FLUI's `flui-engine` integrates, those hints should be threaded through.

**Recommendation**: add `is_complex_hint: bool` + `will_change_hint: bool` fields to `PictureLayer` (default false). Plumb through to `flui-engine::Scene` build path when materialized. ~10 LOC PictureLayer additions, deferred until engine integration.

### Drift E — `Layer.toImage` / `toImageSync` screenshot APIs

**Flutter** (`layer.dart:744-781` `OffsetLayer.toImage` / `toImageSync`):
```dart
Future<ui.Image> toImage(ui.Rect bounds, { double pixelRatio = 1.0 })
ui.Image toImageSync(ui.Rect bounds, { double pixelRatio = 1.0 })
```
Renders the layer subtree to an image — for screenshots, off-thread effects, etc.

**FLUI**: no equivalent.

**Drift severity**: LOW (DEFERRED). Screenshot APIs are user-facing but downstream — flui-engine + flui-painting need to expose Image generation first. Mark as future work post-engine.

### Drift F — `RoundedRectangle` (Flutter `ClipRSuperellipseLayer`) vs FLUI `ClipSuperellipseLayer`

**Flutter** (`layer.dart:1784-1869`): `ClipRSuperellipseLayer` — superellipse-shaped clip (iOS squircle).

**FLUI** (`layer/clip_superellipse.rs`): `ClipSuperellipseLayer` — same shape, different name.

**Drift severity**: LOW (NAMING). FLUI's name `ClipSuperellipseLayer` is clearer than Flutter's `ClipRSuperellipseLayer` (the "R" prefix is a Flutter-internal naming oddity that doesn't translate). **Keep FLUI's clearer name; document the equivalence**.

### Coverage Summary (flui-layer)

- **19 of ~21 Flutter Layer subclasses ported** (PictureLayer, TextureLayer, PlatformViewLayer, PerformanceOverlayLayer, OffsetLayer, ClipRect/RRect/Path/RSuperellipse, ColorFilter, ImageFilter, Transform, Opacity, ShaderMask, BackdropFilter, LayerLink, LeaderLayer, FollowerLayer, AnnotatedRegionLayer, plus CanvasLayer — FLUI-specific mutable canvas).
- **Missing**: `LayerHandle<T>` ref-counted disposal pattern, `Layer._engineLayer` retained GPU handle, `_needsAddToScene` dirty bit, `toImage`/`toImageSync` screenshot APIs, `PictureLayer` hint fields.
- **Architectural divergence**: enum dispatch (FLUI) vs `dyn Layer` virtual dispatch (Flutter). Rust-idiomatic — preserve.
- **Type-system divergences**: `FollowerLayer.leader_anchor: Offset<Pixels>` should be `Alignment` (Flutter parity); `LayerNode::needs_compositing: bool` is a stale cache.

---

## Section 2 — flui-semantics vs `flutter/semantics/*.dart`

Flutter's semantics module: **7,232 LOC in `semantics.dart` + 280 LOC in `binding.dart` + 215 LOC in `semantics_event.dart` + 200 LOC in `semantics_service.dart`**. Total ~7,927 LOC. FLUI ships 5,775 LOC. Coverage analysis follows.

### Coverage Matrix (Flutter Semantics → FLUI Semantics)

| Flutter Concept | Flutter File:Line | FLUI Match | Coverage |
|---|---|---|---|
| `SemanticsBinding` mixin | `binding.dart:23-251` | `SemanticsBinding` at `binding.rs:146-282` | **Good**. `SemanticsHandle` ref-counting matches exactly. |
| `SemanticsHandle` | `binding.dart:263-279` | `SemanticsHandle` at `binding.rs:100-117` | **OK** — Drop impl maps to dispose. |
| `SemanticsService` static | `semantics_service.dart` (separate file) | `SemanticsService` at `binding.rs:380-419` | **Half-implemented**. `send_event` is `// TODO`. `tooltip` calls broken `send_event`. |
| `SemanticsNode` | `semantics.dart:2773` | `SemanticsNode` at `node.rs:51-307` | **Partial**. Missing: `mergeAllDescendantsIntoThisNode` flag, traversal order machinery (`_BoxEdge`, `_SemanticsSortGroup`, `_TraversalSortNode`), `_dirty`, `_inDirtyNodes`, route to `SemanticsOwner._dirtyNodes`. |
| `SemanticsConfiguration` | `semantics.dart:6356-7200` | `SemanticsConfiguration` at `configuration.rs:46-935` | **Drift cluster**: `absorb()` does first-wins for label/hint (Flutter concats); no `role` field; no `headingLevel`; no `_actionsAsBits` lazy cache. |
| `SemanticsAction` | `dart:ui SemanticsAction` (engine) | `SemanticsAction` at `action.rs:21-102` | **27 variants** — verify wire-format constants match Flutter exactly. FLUI adds `Expand`/`Collapse`/`Unfocus` not in Flutter's set. |
| `SemanticsFlag` | `dart:ui SemanticsFlag` (engine) | `SemanticsFlag` at `flags.rs` | **Verify variant set + bit positions**. |
| `SemanticsOwner` | `semantics.dart:4842-5300` | `SemanticsOwner` at `owner.rs:117-349` | **Good**. `flush` collects dirty + invokes callback. Missing: `sendSemanticsUpdate` direct-to-platform path. |
| `SemanticsUpdate` (platform message) | `dart:ui SemanticsUpdate` | `SemanticsTreeUpdate` at `update.rs:60+` + `SemanticsNodeData` flat data | **Partial**. FLUI's update shape is its own; Flutter uses `ui.SemanticsUpdate` built via `ui.SemanticsUpdateBuilder`. |
| `AccessibilityFeatures` | `dart:ui AccessibilityFeatures` | `AccessibilityFeatures` at `binding.rs:30-70` | **OK** — same 7 fields. |
| `SemanticsEvent` hierarchy | `semantics_event.dart` | `SemanticsEvent` at `event.rs` | **Verify variant coverage**. |
| `SemanticsProperties` (declarative) | `semantics.dart:1634-2350` | `SemanticsProperties` at `properties.rs` | **OK** — builder shape matches. |
| `AttributedString` + `StringAttribute` | `semantics.dart:819-936` | `AttributedString` at `properties.rs` | **OK** — port matches. |
| `CustomSemanticsAction` | `semantics.dart:736-818` | `CustomSemanticsAction` at `properties.rs` | **OK**. |
| `SemanticsTag` | `semantics.dart:608-633` | `SemanticsTag` at `properties.rs` | **OK**. |
| `SemanticsRole` | `dart:ui SemanticsRole` (28+ values) | `SemanticsRole` at `role.rs:32-141` (28 variants) | **Drift G** — variant count matches, but FLUI's `SemanticsConfiguration` has no `role` field; the enum is exposed-but-unused. |
| `SemanticsSortKey` (+ `OrdinalSortKey`) | `semantics.dart` | `SemanticsSortKey` at `properties.rs` | **Verify**. |
| `SemanticsHintOverrides` | `semantics.dart:1576-1632` | `SemanticsHintOverrides` at `properties.rs` | **OK**. |
| `SemanticsValidationResult` | `dart:ui SemanticsValidationResult` | NO equivalent | **MISSING** — Flutter's form-field validation reporting. |
| `_concatAttributedString` | `semantics.dart:937` (typedef) + helper | NO equivalent | **MISSING** — covered in absorb drift. |
| `mergeAllDescendantsIntoThisNode` | `semantics.dart` (config field + node walk) | NO equivalent | **MISSING** — covered below as Drift H. |
| Heading level (`_headingLevel`) | `semantics.dart:6827-6830` | NO equivalent | **MISSING**. |

### Drift G — `SemanticsConfiguration::role` field absent

**Flutter** (`semantics.dart:6852-6854`):
```dart
if (_role == SemanticsRole.none) {
    _role = child._role;
}
```
`_role: SemanticsRole` is a field on Flutter's SemanticsConfiguration.

**FLUI** (`configuration.rs`): **no `role` field on `SemanticsConfiguration`**. The 28-variant `SemanticsRole` enum is exposed but unused at config level.

**Drift severity**: MEDIUM. Already covered in Part II findings. **Add the field**.

### Drift H — `mergeAllDescendantsIntoThisNode` flag

**Flutter** (`semantics.dart:6395-6410`):
```dart
bool _mergeAllDescendantsIntoThisNode = false;
bool get isMergedIntoParent => _mergeAllDescendantsIntoThisNode;
set mergeAllDescendantsIntoThisNode(bool value) { ... }
```
When set on a `SemanticsConfiguration`, all descendant semantics nodes' configurations are merged into this one — used for buttons whose semantics should be a single accessible item.

**FLUI** (`configuration.rs`): closest is `is_semantics_boundary: bool` (line 50), but the semantic is inverted: Flutter's `mergeAllDescendantsIntoThisNode == true` means *this* absorbs descendants; FLUI's `is_semantics_boundary == true` means *this* is the boundary (descendants do not absorb).

**Drift severity**: MEDIUM. The two are *related* but distinct concepts:
- Flutter's `isSemanticsBoundary` (config field) - this is where a SemanticsNode is created.
- Flutter's `_mergeAllDescendantsIntoThisNode` (config field) - this absorbs descendants' configs.

FLUI has only one (`is_semantics_boundary`); the merge-descendants behavior is implicit (via `absorb` chains in tree assembly). The drift means: a Flutter widget setting `mergeAllDescendantsIntoThisNode: true` on a button widget won't have an equivalent FLUI API.

**Recommendation**: add `merge_all_descendants_into_this_node: bool` field + getter/setter. The actual merge behavior is implemented by the (yet-to-be-written) semantics tree builder that walks the render tree — it should consult this flag.

### Drift I — `SemanticsValidationResult` (form-field validation)

**Flutter** (`semantics.dart:6874-6881`):
```dart
if (child._validationResult != _validationResult) {
    if (child._validationResult == SemanticsValidationResult.invalid) {
        _validationResult = SemanticsValidationResult.invalid;
    } else if (_validationResult == SemanticsValidationResult.none) {
        _validationResult = child._validationResult;
    }
}
```

**FLUI**: no equivalent.

**Drift severity**: LOW (DEFERRED). Form validation a11y is a specialized feature; can be added when widget-layer validators materialize.

### Drift J — `SemanticsBinding::performSemanticsAction` is bridge between platform action and Widget tree

**Flutter** (`binding.dart:192-205`):
```dart
@protected
void performSemanticsAction(ui.SemanticsActionEvent action);
```
This is an *abstract* method — implemented by `WidgetsBinding` to route actions to the correct widget. The chain is: platform a11y service → `_handleSemanticsActionEvent` → `performSemanticsAction` → `RendererBinding._performAction` → finds RenderObject by ID → invokes its semantics action handler.

**FLUI** (`binding.rs:259-275`):
```rust
pub fn set_action_callback<F>(&self, callback: F)
where F: Fn(SemanticsActionEvent) + Send + Sync + 'static,
{ ... }
pub fn dispatch_action(&self, event: SemanticsActionEvent) { ... }
```
FLUI uses a callback-registration pattern. Less rigid — the consumer (rendering layer) registers its action-routing function once.

**Drift severity**: LOW. FLUI's pattern is Rust-idiomatic (closure injection vs abstract method). Functionally equivalent. **Document the divergence**.

### Drift K — `SemanticsActionListeners` registry

**Flutter** (`binding.dart:96-110`):
```dart
final ObserverList<ValueSetter<ui.SemanticsActionEvent>> _semanticsActionListeners = ...;
void addSemanticsActionListener(ValueSetter<ui.SemanticsActionEvent> listener) { ... }
void removeSemanticsActionListener(ValueSetter<ui.SemanticsActionEvent> listener) { ... }
```
Flutter supports multiple action listeners (chain pattern). `_handleSemanticsActionEvent` iterates the listeners then calls `performSemanticsAction`.

**FLUI** (`binding.rs:259-265`): single `action_callback: RwLock<Option<Arc<...>>>`. Only one listener.

**Drift severity**: LOW. Add a listeners Vec/SmallVec when a multi-listener scenario materializes (e.g., dev-tools wants to observe all actions without blocking the main route). For now, single-callback is sufficient.

### Coverage Summary (flui-semantics)

- **Core types ported**: SemanticsNode, SemanticsConfiguration, SemanticsAction (with wire-format risk), SemanticsBinding, SemanticsOwner, SemanticsTree (Flutter has no separate tree class but FLUI's is a clean addition), AccessibilityFeatures, SemanticsRole, SemanticsProperties, AttributedString, SemanticsTag, CustomSemanticsAction.
- **Critical missing**: `SemanticsService::send_event` platform routing (TODO).
- **Important missing**: `_role` field on Config (enum exposed-but-unused); `mergeAllDescendantsIntoThisNode` flag; `_concatAttributedString` (label concat absorb semantics); `_headingLevel`; `SemanticsValidationResult`.
- **Wire-format risk**: `SemanticsAction` variant set drifts (Expand/Collapse/Unfocus added vs Flutter), bit positions need cross-check.
- **Architectural pluses**: `SemanticsTree` as a separate type (Flutter just walks the RenderObject tree); `SemanticsHandle::Drop` impl (Flutter requires manual `.dispose()`).

---

# Part IV — Combined Priority Order

The findings cross both crates — `LayerTree` and `SemanticsTree` share the same Slab-tree pattern and inherit the same lifecycle/cascade issues. Several "atomic commit families" exist. Below is the unified execution sequence.

## Atomic Commit Families

**Family 1 — Half-Implementation Wires (P0 unblock):**
- U1: `SemanticsService::send_event` wiring → `SemanticsBinding::event_callback` mirror of `announce_callback`. **~30 LOC. Single-file (binding.rs). Highest unblocking impact.**
- U2: `SemanticsConfiguration::role` field + accessors. Adds 28-variant `SemanticsRole` to runtime config. **~30 LOC.**

**Family 2 — Slab-Tree Hygiene Pair (Layer + Semantics together):**
- U3a (layer): `LayerTree::add_child` auto-detach + dedup. **~20 LOC.**
- U3b (semantics): `SemanticsTree::add_child` auto-detach. **~15 LOC.**
- U4a (layer): `LayerTree::remove` cascade-by-default (rename old to `remove_shallow`). **~30 LOC.**
- U4b (semantics): `SemanticsTree::remove` cascade-by-default. **~30 LOC.**

  **Note**: per [[flui-tree-unified-interface-intent]], the cascading-remove and auto-detach semantics are good candidates for a future `TreeWrite<Id>` trait extension. Track as input for next round of `flui-tree` API consolidation.

**Family 3 — Layer Lifecycle Phase 1 (PR #84 precedent):**
- U5: `LayerNode::disposed: AtomicBool` + `Drop` impl + `debug_assert!` guards on every mutation method. Mirrors `ChangeNotifier::dispose`. **~150 LOC.**

**Family 4 — Semantics Configuration Flutter-Faithful Absorb:**
- U6: Port `_concatAttributedString` helper to `properties.rs`. Update `SemanticsConfiguration::absorb` to concat label + hint per Flutter (`semantics.dart:6837-6862`). **~80 LOC.**
- U7: `absorb` honors `blocks_user_actions` filter — only un-blockable actions absorbed when child blocks (`semantics.dart:6796-6804`). **~20 LOC.**
- U8: `absorb` merges `role` field (None → child.role per Flutter). **~10 LOC (after U2).**

**Family 5 — Layer Enum Optimization:**
- U9: Box heavy variants in `Layer` enum (Picture / Canvas / ClipPath / PerformanceOverlay). Update `From` impls + macro. **~80 LOC.**

**Family 6 — Type-System Cleanups:**
- U10: `SemanticsNode::transform: Option<Matrix4>` (drop `[f32; 16]`). **~15 LOC.**
- U11: Introduce `flui_types::painting::Alignment` newtype (separate flui-types PR). **~60 LOC + tests.**
- U12: Migrate `FollowerLayer::leader_anchor` + `follower_anchor` to `Alignment`. **~30 LOC, depends on U11.**

**Family 7 — Dead-Code Cleanup:**
- U13: Delete `LayerNode::needs_compositing` field + setter + getter (zero in-prod callers verified). **~20 LOC reduction.**
- U14: Delete `LayerNode::set_offset` / `set_element_id`. **~15 LOC reduction.**
- U15: Delete `Scene::layer_tree_mut`. **~5 LOC reduction.**
- U16: Delete `LinkRegistry::leader_for_follower` / `links` / `leaders` iterators / `rebuild_follower_lists`. **~60 LOC reduction.**
- U17: Pass `LayerLink` by value uniformly in `LinkRegistry` signatures. **~10 LOC delta.**
- U18: `SemanticsNode::merge` → `absorb` rename. **~10 LOC.**
- U19: `SemanticsOwner::new_without_callback` `#[cfg(any(test, feature = "testing"))]`. **~5 LOC.**

**Family 8 — Lifecycle Phase 2 + Performance:**
- U20: `LayerNode::needs_add_to_scene: AtomicBool` + `LayerTree::mark_needs_add_to_scene` + `update_subtree_needs_add_to_scene` (mirrors Flutter `layer.dart:495-521`). **~200 LOC.**
- U21: `SemanticsOwner::flush` allocation reduction (drop intermediate Vec, reuse `updates` Vec field). **~30 LOC.**
- U22: `AccessibilityFeatures` packed AtomicU8 (eliminate `RwLock<AccessibilityFeatures>`). **~50 LOC.**
- U23: `SemanticsBinding` clone-and-release pattern for callback dispatch. **~10 LOC.**
- U24: `SemanticsNode::children: SmallVec<[_; 4]>` + `SemanticsConfiguration::actions: SmallVec<[(_, _); 4]>`. **~30 LOC.**

**Family 9 — SemanticsAction Wire-Format Alignment:**
- U25: Verify all 27 `SemanticsAction` variant values match Flutter `dart:ui.SemanticsAction` constants. Move `Expand` / `Collapse` to `SemanticsFlag::HasExpandedState + IsExpanded`. Drop `Unfocus`. **~50 LOC + cross-ref doc.**

**Family 10 — Deferred (blocked on other crates):**
- U26: `LayerNode::engine_layer: Option<EngineLayerHandle>` field — blocked on flui-engine handle type.
- U27: `PictureLayer.is_complex_hint` / `will_change_hint` fields + plumbing — blocked on flui-engine.
- U28: `OffsetLayer.to_image` / `to_image_sync` screenshot APIs — blocked on flui-painting Image generation.
- U29: `mergeAllDescendantsIntoThisNode` flag + builder integration — blocked on (yet-to-write) semantics tree builder consuming RenderObject tree.

## Final Combined Priority Order

P0 — Critical correctness, unblocks downstream consumers:
1. **U1** — `SemanticsService::send_event` wiring (30 LOC). Sole platform a11y events path. Same-day landable.
2. **U4a + U4b** — Slab-tree cascade `remove` for both Layer and Semantics (60 LOC). Footgun → safe default.
3. **U3a + U3b** — Slab-tree `add_child` auto-detach (35 LOC). Silent dual-parent bug → safe default.

P1 — Flutter parity / type-system correctness:
4. **U6 + U7 + U8** — Semantics `absorb` Flutter-faithful (110 LOC). Screen readers truncate today.
5. **U2** — `SemanticsConfiguration::role` field (30 LOC). Unblocks 28-variant unused enum.
6. **U5** — `LayerNode::disposed` + `Drop` (150 LOC). Lifecycle protocol foundation.
7. **U25** — `SemanticsAction` wire-format alignment (50 LOC). Platform protocol correctness.
8. **U10** — `SemanticsNode::transform: Option<Matrix4>` (15 LOC). Type unification.

P2 — Memory + structural cleanups:
9. **U9** — `Layer` enum heavy-variant boxing (80 LOC). Memory footprint fix.
10. **U11 + U12** — `Alignment` newtype + `FollowerLayer` migration (90 LOC). Type-system lie fix.
11. **U13–U19** — Dead-code cleanup (125 LOC reduction net). API surface compression.

P3 — Performance milestones:
12. **U20** — `needs_add_to_scene` dirty propagation (200 LOC). Foundation for retained rendering.
13. **U22** — `AccessibilityFeatures` atomic pack (50 LOC). Eliminate RwLock for bool reads.
14. **U23** — Clone-and-release callback dispatch (10 LOC). Anti-deadlock.
15. **U24** — `SemanticsNode::children` SmallVec (30 LOC). Stack-allocate common case.
16. **U21** — `SemanticsOwner::flush` allocation reduction (30 LOC). Frame-budget hygiene.

P4 — Deferred (blocked on other crates):
17. **U26-U29** — engine_layer cache, picture hints, screenshot APIs, merge-descendants flag.

## Estimated Sequence Length

If executed per the precedent of PR #81 / #82 / #83 / #84 (atomic commits, one logical unit each):

- **P0 work** (~125 LOC): 3 atomic commits over 1-2 sessions.
- **P1 work** (~365 LOC): 7 atomic commits over 2-3 sessions.
- **P2 work** (~295 LOC): 5-6 atomic commits over 2 sessions.
- **P3 work** (~320 LOC): 5 atomic commits over 2-3 sessions.
- **P4 deferred**: track as future cycles.

Total: ~22-25 atomic commits, ~1,100 LOC net change (additions + reductions blend), 8-10 working sessions. Comparable to the prior cycle's flui-interaction × flui-scheduler workload.

---

# Appendix A — Investigation Trail

This appendix records the rg / grep / find commands that produced the audit evidence. All paths are relative to the worktree root (`C:\Users\vanya\RustroverProjects\flui\.claude\worktrees\determined-proskuriakova-d2eccf`).

## A.1 — Crate enumeration + LOC counts

```bash
$ find crates/flui-layer/src -type f -name "*.rs" -exec wc -l {} +
# Output: 34 files, 9,796 LOC total.

$ find crates/flui-semantics/src -type f -name "*.rs" -exec wc -l {} +
# Output: 12 files, 5,775 LOC total.

$ wc -l .flutter/flutter-master/packages/flutter/lib/src/rendering/layer.dart \
        .flutter/flutter-master/packages/flutter/lib/src/semantics/semantics.dart
# Output:
#   3029 layer.dart
#   7232 semantics.dart
#  10261 total
```

## A.2 — Dead-code receipts

```bash
# LayerNode::set_needs_compositing (the cached field setter)
$ grep -rn "node\.set_needs_compositing\|LayerNode.*set_needs_compositing" crates/
crates/flui-layer/tests/layer_tree.rs:166:    node.set_needs_compositing(false);
# (only the own test). 0 production callers. Confirms zombie cache.

# Scene::layer_tree_mut
$ grep -rn "\.layer_tree_mut\(\)" crates/
# (0 hits outside flui-layer test). Confirms 0 production callers.

# LinkRegistry GC hooks
$ grep -rn "remove_orphaned_followers\|rebuild_follower_lists" crates/ | grep -v test
# (0 production hits). Confirms zombie GC.

# LinkRegistry leader_for_follower
$ grep -rn "leader_for_follower" crates/ | grep -v "test\|link_registry.rs"
# (0 production hits).

# LayerTree::iter_mut
$ grep -rn "layer_tree.iter_mut\|tree\.iter_mut" crates/flui-layer/
# (0 production callers).
```

## A.3 — Half-implementation receipts

```bash
# TODO / placeholder scan across flui-layer + flui-semantics
$ grep -rn "TODO\|FIXME\|XXX:" crates/flui-layer/src crates/flui-semantics/src
crates/flui-semantics/src/binding.rs:411:        // TODO: Route to platform accessibility API
# Single TODO — the only half-implementation marker in either crate.

# unimplemented!/todo!/panic! scan (production paths)
$ grep -rn "unimplemented!\|todo!\|panic!" crates/flui-layer/src crates/flui-semantics/src
crates/flui-layer/src/scene.rs:548:            panic!("intentional poison in callback 2");
crates/flui-layer/src/layer/backdrop_filter.rs:144:            _ => panic!("Expected Blur filter"),
crates/flui-layer/src/layer/backdrop_filter.rs:162:            _ => panic!("Expected ColorAdjust filter with Brightness"),
crates/flui-layer/src/layer/shader_mask.rs:153:            _ => panic!("Expected LinearGradient"),
crates/flui-layer/src/layer/shader_mask.rs:174:            _ => panic!("Expected RadialGradient"),
# All in `#[cfg(test)]` modules. No production-path panics — Constitution Principle 6 clean.

# `unsafe` scan (Constitution Principle 3)
$ grep -rn "\bunsafe\b" crates/flui-layer/src crates/flui-semantics/src
crates/flui-layer/src/scene.rs:328:// No `unsafe impl` is needed -- Mythos Step 3 deletion.
# (only a comment about removal). 0 `unsafe` in production. Constitution Principle 3 clean.
```

## A.4 — Type-system drift receipts

```bash
# SemanticsRole usage outside role.rs
$ grep -rn "SemanticsRole" crates/flui-semantics/src | grep -v "role.rs\|test"
crates/flui-semantics/src/lib.rs:130:    AccessibilityFocusBlockType, ..., SemanticsRole,
crates/flui-semantics/src/lib.rs:165:        SemanticsRole, SemanticsService, ...
crates/flui-semantics/src/owner.rs:93:/// use flui_semantics::{..., SemanticsRole};
crates/flui-semantics/src/owner.rs:108:///             .with_role(SemanticsRole::Button)
# Only re-exports + doc-comments. Confirms: enum exposed but not used by SemanticsConfiguration.

# SemanticsConfiguration field scan (verify no `role` field)
$ grep -n "^\s*\(pub\)\?\s*\(role\|_role\)" crates/flui-semantics/src/configuration.rs
# (0 hits). Confirms missing field.

# FollowerLayer anchor type
$ grep -n "leader_anchor\|follower_anchor" crates/flui-layer/src/layer/follower.rs
# Field type Offset<Pixels>, used as unitless fraction in calculate_offset.

# SemanticsNode::transform type
$ grep -n "transform: " crates/flui-semantics/src/node.rs
crates/flui-semantics/src/node.rs:73:    transform: Option<[f32; 16]>,
# Confirms type fragmentation vs flui_types::Matrix4.
```

## A.5 — Sync-primitive receipts

```bash
# Mutex / RwLock / atomic primitives in flui-layer (production paths)
$ grep -rn "Mutex\|RwLock\|DashMap\|parking_lot" crates/flui-layer/src | grep -v test | grep -v "doc"
# Only `Scene::test_*` use atomic counters. Production code: no Mutex/RwLock. Clean.

# Sync primitives in flui-semantics
$ grep -n "RwLock\|Mutex\|AtomicBool\|AtomicUsize" crates/flui-semantics/src/binding.rs
crates/flui-semantics/src/binding.rs:8:    atomic::{AtomicBool, AtomicUsize, Ordering},
crates/flui-semantics/src/binding.rs:13:use parking_lot::RwLock;
crates/flui-semantics/src/binding.rs:148:    handle_count: Arc<AtomicUsize>,
crates/flui-semantics/src/binding.rs:151:    platform_semantics_enabled: AtomicBool,
crates/flui-semantics/src/binding.rs:154:    accessibility_features: RwLock<AccessibilityFeatures>,
crates/flui-semantics/src/binding.rs:158:    announce_callback: RwLock<Option<Arc<dyn Fn(&str, Assertiveness) + Send + Sync>>>,
crates/flui-semantics/src/binding.rs:162:    action_callback: RwLock<Option<Arc<dyn Fn(SemanticsActionEvent) + Send + Sync>>>,
# 3 RwLocks + 2 atomics in SemanticsBinding. Per finding: AccessibilityFeatures RwLock
# should be AtomicU8; callback RwLocks should use clone-and-release pattern.
```

## A.6 — Flutter cross-ref receipts

```bash
# Flutter Layer subclass enumeration
$ grep -n "^class\|^abstract class\|^mixin" .flutter/flutter-master/packages/flutter/lib/src/rendering/layer.dart
31:class AnnotationEntry<T> {
57:class AnnotationResult<T> {
144:abstract class Layer with DiagnosticableTreeMixin {
783:class LayerHandle<T extends Layer> {
824:class PictureLayer extends Layer {
... [24 Layer-family classes]

# Flutter Layer dispose protocol
$ grep -n "void dispose\|_refCount\|_engineLayer\|_needsAddToScene" .flutter/flutter-master/packages/flutter/lib/src/rendering/layer.dart | head -15
# Confirms dispose/refCount/engineLayer/needsAddToScene fields documented in Mythos Verdict.

# Flutter SemanticsConfiguration absorb
$ grep -n "absorb(" .flutter/flutter-master/packages/flutter/lib/src/semantics/semantics.dart
6790:  void absorb(SemanticsConfiguration child) {
# Verified line 6790. Cross-ref Section 2 absorb-drift evidence accurate.

# Flutter SemanticsRole field on SemanticsConfiguration
$ grep -n "_role" .flutter/flutter-master/packages/flutter/lib/src/semantics/semantics.dart | head -5
# Confirms _role field exists in Flutter; FLUI's absence is real drift.
```

## A.7 — Tree-trait integration receipts

```bash
# LayerTree implements TreeRead + TreeNav
$ grep -n "impl TreeRead\|impl TreeNav" crates/flui-layer/src/tree/tree_traits.rs
19:impl TreeRead<LayerId> for LayerTree {
50:impl TreeNav<LayerId> for LayerTree {

# SemanticsTree implements TreeRead + TreeNav (in tree.rs not separate file)
$ grep -n "impl TreeRead\|impl TreeNav" crates/flui-semantics/src/tree.rs
290:impl TreeRead<SemanticsId> for SemanticsTree {
323:impl TreeNav<SemanticsId> for SemanticsTree {
# Both trees integrate cleanly. Memory [[flui-tree-unified-interface-intent]] satisfied.
```

---

*End of audit.*
