---
title: "Mythos design verdict — flui-layer redesign"
status: design
date: 2026-05-20
author: Claude Mythos
applies-to: crates/flui-layer
---

# Mythos Design Verdict

## What `flui-layer` should be

A **single-owner, closed-enum layer tree + a stack-based scene builder + a damage tracker**. Three concerns, one crate. Holds the composited output from `flui-rendering` for downstream GPU consumption by `flui-engine`. Nothing else.

## What it must not become

A second copy of `PaintingContext` (which is the duplicate `push_*` API today). A second `Mutex`-wrapped registry crate for callbacks no caller subscribes to. A `Box<dyn Layer>` plugin boundary the GPU backend cannot accept. An owner of `LayerHandle<T>` ceremony with 17 type aliases and `Arc<AtomicUsize>` ref-counts that no external caller ever reads. A wrapper that tells its users to put it inside an `Arc<RwLock<>>` because the doc-writer panicked at the Send-bound boundary.

## Main state owner

`Scene` owns the `LayerTree` plus the `LinkRegistry` plus a frame number. `LayerTree` owns the `Slab<LayerNode>` arena. Exactly one mutable instance at a time, passed by value into the engine for rendering, dropped after consumption. No `Arc<RwLock<LayerTree>>` anywhere in the workspace. No "shared layer trees across threads"; cross-thread layer construction happens before `Scene` finalises, and `Scene: Send` carries it across the render-thread boundary as a single value.

## Main trust boundary

**`Layer` is a closed concrete enum, not a plugin trait.** The 18 variants are the entire vocabulary of compositor layers; adding a 19th is a coordinated change in `flui-layer` + `flui-engine` + (sometimes) `flui-rendering`. This is **deliberately the opposite shape** from `RenderObject<P>`. The reason: layers map directly to GPU operations in `flui-engine` — arbitrary user-defined layers would force a `Box<dyn LayerOps>` boundary the wgpu backend cannot translate. There is no "third-party plugin" trust boundary here because there are no third-party plugins. The boundary is `flui-engine`'s `match layer { … }` site, and the trust contract is "every variant in this enum has a documented GPU lowering."

## Main async risk

Zero. There is no `async fn` in any layer method, no `.await` in tree mutation, no `.await` in scene composition, no `.await` in damage tracking. Composition callbacks (currently `Arc<Mutex<Vec<Box<dyn Fn() + Send + Sync>>>>`) are synchronous `Fn()` invocations and can be inlined onto the `Scene`-build path.

## Main simplification principle

**Every layer of indirection in this crate must justify its presence in writing.** A non-exhaustive list of indirection that does not justify itself today:

- `LayerHandle<T>` (467 LOC) has zero external callers in the workspace. `Arc<AtomicUsize>` ref-counts are bumped and decremented but never read by any consumer to gate resource release. **Delete the type and its 17 type aliases.**
- `CompositionCallbackRegistry` with `Arc<Mutex<Vec<(Id, Box<dyn Fn() + Send + Sync>)>>>` has zero `impl HasCompositionCallbacks` outside its own tests. Either fold the callback list into `Scene` as a plain `Vec<…>` and skip the lock, or delete entirely.
- The 19 × 3 = 57 `is_*` / `as_*` / `as_*_mut` methods on `Layer` enum are pattern-match boilerplate. Either macro-collapse to a derive (`enum_dispatch`, custom `paste!` macro) or delete; the engine matches the enum directly without needing the helpers.
- 39 `unsafe impl Send + Sync` blocks across 18 layer files + `scene.rs` + `handle.rs` (one body even has the comment "contains only owned, Send types" right above its unsafe impl). **Delete all 39.** The auto-derived bounds are correct; the unsafe blocks are cargo-cult from Dart's threading model.
- `Scene::dispose(self)` calls `drop(self)`. **Delete.** Rust users know what `drop` does.
- `LayerTree::push_clip_rect`, `push_clip_rrect`, `push_clip_path`, `push_transform`, `push_opacity` (5 methods on `LayerTree`) duplicate `SceneBuilder::push_clip_rect`, `push_clip_rrect`, …, `push_opacity` (30 methods on `SceneBuilder`). Same operations, two impls, two test suites. **Pick one.** The `SceneBuilder` is the public scene-construction API; the `LayerTree::push_*` helpers are an unused short-cut and should be deleted.

This is not architecture. It is the visible cost of porting Dart class shapes into Rust without paying attention to what Rust gives you for free. The auto-derived `Send + Sync`, the closed enum dispatch, the `Drop` trait, the `Option<T>` enum, the `&mut self` borrow checker — every one of those subsumes ceremony the current code re-implements by hand.

---

## 1. Problem Definition

**Responsibility.** Store and mutate a tree of compositor layers; provide a stack-based scene builder for `PaintingContext` to construct that tree; emit a `Scene` value containing the tree + frame metadata for `flui-engine` to consume. Track damage regions for incremental rendering.

**Non-responsibility.**
- Render-object layout, paint, or hit-testing (lives in `flui-rendering`).
- GPU surface management, render-pass scheduling, draw-call submission (lives in `flui-engine`).
- Repaint-boundary policy decisions — when to allocate a new layer vs reuse one (lives in `flui-rendering`; this crate just stores the result).
- Painting primitives (Canvas, DisplayList, Picture — live in `flui-painting`).
- Display-list recording (lives in `flui-painting::Canvas`).
- Persistent layer caching across app sessions (not implemented; would live higher).

**Callers.** Four crates consume `flui-layer`:
- `flui-rendering` (`pipeline/owner.rs`, `context/canvas.rs`, `view/render_view.rs`) — paint phase emits layers into `LayerTree` and packages them as `Scene`.
- `flui-engine` (`wgpu/layer_render.rs`) — consumes `Scene` and lowers each `Layer` variant to wgpu draw calls.
- `flui-app` (`binding.rs`, `direct.rs`) — owns the frame loop and threads `Scene` from `flui-rendering` to `flui-engine`.
- `flui-hot-reload` — preserves the most-recent `Scene` across hot-reload events.

`SceneBuilder` is consumed by `flui-rendering`'s paint context; the other crates only touch the finished `Scene`.

**Lifecycle.** A `Scene` is constructed once per frame, consumed once by the engine, dropped. `LayerTree` inside `Scene` is mutable only during construction (when `SceneBuilder` holds `&mut LayerTree`) and read-only during rendering. There is no "long-lived layer tree across frames" — Flutter's retained-layer optimisation will be modelled later via a separate retention store on `flui-rendering`'s side (currently a stub `SceneCompositor::retained`).

**Key invariants.**
1. **Single-writer-during-build.** While a `SceneBuilder` holds `&mut LayerTree`, no other code can touch the tree. Compiler-enforced.
2. **ID stability per scene.** A `LayerId` issued during scene construction is valid for the lifetime of that `Scene`. Across scenes, IDs are not portable.
3. **Layer enum is exhaustive.** Every `Layer` variant has a corresponding `flui-engine::WgpuBackend::render_layer` arm. Adding a variant is a coordinated change.
4. **Closed tree shape.** A `LayerNode` parent points to its children (`Vec<LayerId>`); each child points back to its parent (`Option<LayerId>`). Insertion and `add_child` maintain both directions; the only way to violate is mutating the slab directly (no public API).
5. **No mutation across scenes.** Once `Scene::build()` returns, the `LayerTree` is frozen behind `&Scene` accessors. Mutation requires consuming the `Scene` (currently the engine does not, but the API allows it).
6. **No `Arc<RwLock<Scene>>`.** Cross-thread access is by value-move; the engine receives a `Scene` and renders it.

**Failure modes — normal, not exceptional.**
- A layer's bounds returns `None` (legitimate for `OffsetLayer`, `OpacityLayer`, etc. that have no intrinsic bounds) — the engine handles it via the parent's clip. Not an error.
- A `LayerLink` registered with no follower — handled by `LinkRegistry::remove_orphaned_followers`. Not an error.
- A `LayerId` from a previous scene used against the current scene — returns `None` from `LayerTree::get`; the caller decides.
- A composition callback panics during `fire()` — currently the panic propagates and brings the frame down. Mythos plan: wrap in `catch_unwind`, surface as `LayerError::CallbackPoisoned`, drop the frame.

---

## 2. Architecture Overview

```text
flui-rendering (paint phase)
  │  builder.push_offset(...); builder.push_opacity(...); builder.add_picture(...)
  ▼
SceneBuilder<'a>             ◄── stack-based; holds &mut LayerTree
  │  push_layer / add_leaf / pop / build
  ▼
LayerTree                    ◄── single mutable owner; arena
  │  Slab<LayerNode>
  ▼
LayerNode { parent: Option<LayerId>, children: Vec<LayerId>, layer: Layer, … }
  │
  ▼
Layer (closed enum)          ◄── 18 variants, exhaustive match in flui-engine
  │
  ▼
Scene { size, layer_tree, root, links, frame }
  │  Send; moved to render thread by value
  ▼
flui-engine::WgpuBackend::render(&Scene)
```

No `Arc<RwLock<…>>` on the diagram. No `Box<dyn Layer>`. No `LayerHandle<T>`. No `CompositionCallbackRegistry` (folded into `Scene` or deleted).

**What goes away from current code:**
- `handle.rs` (467 LOC + 17 type aliases) — deleted. Zero external callers.
- `LayerHandle<T>` references in `lib.rs` re-exports and prelude — deleted.
- `composition_callback.rs` `Arc<Mutex<…>>` storage — replaced by a plain `Vec<CompositionCallback>` field on `Scene`, fired synchronously after build. `HasCompositionCallbacks` trait deleted (0 impls).
- `Layer::is_*` / `as_*` / `as_*_mut` triplet (57 methods) — macro-collapsed to a single `paste!`-generated impl block, or deleted entirely once callers switch to `match` (most already do; the helpers are barely used). Mythos plan keeps the macro form for ergonomics, audits external usage first.
- `LayerTree::push_clip_rect` / `push_clip_rrect` / `push_clip_path` / `push_transform` / `push_opacity` (5 helpers) — deleted. Callers use `SceneBuilder`.
- `Scene::dispose(self)` — deleted. `drop(scene)` is the idiom.
- `LayerNode::get_layer` / `get_layer_mut` (alongside `layer` / `layer_mut`) — duplicates deleted; pick `layer` / `layer_mut`.
- `Scene::new` vs `Scene::with_links` — collapsed via builder.
- 39 × `unsafe impl Send + Sync` blocks — deleted; auto-derived bounds suffice.
- LayerTree doc-comment "use `Arc<RwLock<LayerTree>>` for multi-threaded access" — replaced with "construct on one thread, move `Scene` by value to the render thread."
- `parallel = ["rayon"]` feature with unused `rayon` dependency — either wired up to a real parallel traversal or deleted with the dep.

**What earns its place:**
- `tree/layer_tree.rs` — Slab-based arena + LayerNode storage. Without it, no tree. Stays after extracting the 720 LOC test suite and the 5 deleted `push_*` helpers.
- `tree/tree_traits.rs` — `TreeRead<LayerId>` and `TreeNav<LayerId>` impls for `LayerTree`. ~100 LOC of production code + 200 LOC tests. Stays.
- `layer/` directory with 18 concrete layer struct files — each is one Flutter Layer subclass and one GPU lowering. Stays.
- `layer/mod.rs` — `Layer` enum + `LayerBounds` trait. Trimmed from 1075 LOC to ~300 LOC after collapsing the `is_*`/`as_*` boilerplate and moving `LayerBounds` to its own file.
- `compositor.rs::SceneBuilder` — the public scene-construction API. ~600 LOC of `push_*` / `add_*` / `pop` methods. Stays, but `SceneCompositor` (retained-layer manager) moves to its own file.
- `scene.rs` — the public scene value. ~150 LOC after deletions. Stays.
- `link_registry.rs` — leader-follower bookkeeping. ~330 LOC after extracting 290 LOC tests. Stays.
- `damage.rs` — damage region tracking. ~110 LOC. Untouched; already clean.

---

## 3. Core Types

```rust
// ───────────────────────────────────────────────────────────────
// Layer enum — closed, exhaustive, dispatched by match
// ───────────────────────────────────────────────────────────────

/// The compositor's layer vocabulary.
///
/// Every variant has a documented GPU lowering in `flui-engine::WgpuBackend`.
/// Adding a variant is a coordinated change in `flui-layer` + `flui-engine`,
/// not a third-party extension point.
#[derive(Debug)]
#[non_exhaustive]   // future-proofing for internal additions; external code must still match all variants today
pub enum Layer {
    Canvas(CanvasLayer),
    Picture(PictureLayer),
    Texture(TextureLayer),
    PlatformView(PlatformViewLayer),
    PerformanceOverlay(PerformanceOverlayLayer),
    ClipRect(ClipRectLayer),
    ClipRRect(ClipRRectLayer),
    ClipPath(ClipPathLayer),
    ClipSuperellipse(ClipSuperellipseLayer),
    Offset(OffsetLayer),
    Transform(TransformLayer),
    Opacity(OpacityLayer),
    ColorFilter(ColorFilterLayer),
    ImageFilter(ImageFilterLayer),
    ShaderMask(ShaderMaskLayer),
    BackdropFilter(BackdropFilterLayer),
    Leader(LeaderLayer),
    Follower(FollowerLayer),
    AnnotatedRegion(AnnotatedRegionLayer),
}

// `Layer::bounds()`, `Layer::needs_compositing()`, `Layer::is_opaque()` stay as
// methods on the enum — three semantic queries the engine consumes. They are
// not the boilerplate that's being deleted.

// ───────────────────────────────────────────────────────────────
// LayerId — already correct in flui-foundation; no change
// ───────────────────────────────────────────────────────────────

pub use flui_foundation::LayerId;   // NonZeroUsize-backed, +1/-1 offset pattern

// ───────────────────────────────────────────────────────────────
// LayerTree — single-owner arena; no locks anywhere
// ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct LayerTree {
    nodes: Slab<LayerNode>,
    root: Option<LayerId>,
}

/// A node in `LayerTree`. Concrete `Layer` enum is stored inline; no `Box<dyn>`.
#[derive(Debug)]
pub struct LayerNode {
    parent: Option<LayerId>,
    children: Vec<LayerId>,           // small-vec candidate; AVG_CHILDREN=4 today
    layer: Layer,
    needs_compositing: bool,
    offset: Option<Offset<Pixels>>,
    element_id: Option<ElementId>,
}

// Public surface on LayerNode is `layer()` / `layer_mut()` / `parent()` /
// `children()` / `set_parent()` / accessor methods on metadata. The duplicate
// `get_layer()` / `get_layer_mut()` accessors are deleted.

// ───────────────────────────────────────────────────────────────
// Scene — owns the LayerTree + LinkRegistry + frame metadata
// ───────────────────────────────────────────────────────────────

/// A composited scene ready for rendering. `Send`; moved to render thread.
///
/// Auto-derived `Send` and `Sync` — no `unsafe impl`.
#[derive(Debug, Default)]
pub struct Scene {
    size: Size<Pixels>,
    layer_tree: LayerTree,
    root: Option<LayerId>,
    link_registry: LinkRegistry,
    composition_callbacks: Vec<CompositionCallback>,   // folded in from old registry
    frame_number: u64,
}

/// A boxed `Fn()` callback fired once when `Scene::fire_composition_callbacks()`
/// is called by the engine. No `Arc<Mutex<>>` — the Scene owns the list.
pub struct CompositionCallback(Box<dyn FnOnce() + Send + 'static>);
//                                  ^^^^^^^^ - one-shot; FnOnce subsumes Fn
//                                                       and removes the
//                                                       lifetime ambiguity
//                                                       that forced Mutex.

// ───────────────────────────────────────────────────────────────
// SceneBuilder — stack-based public scene construction
// ───────────────────────────────────────────────────────────────

pub struct SceneBuilder<'a> {
    tree: &'a mut LayerTree,
    stack: Vec<LayerId>,         // typical depth: 4-12; Vec is fine, smallvec optional
    root: Option<LayerId>,
}

// push_* / add_* / pop / build remain on SceneBuilder; the duplicate
// LayerTree::push_clip_rect / push_clip_rrect / push_clip_path /
// push_transform / push_opacity disappear.

// ───────────────────────────────────────────────────────────────
// LinkRegistry — leader-follower bookkeeping; unchanged HashMap pair
// ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct LinkRegistry {
    leaders: HashMap<LayerLink, LeaderInfo>,
    followers: HashMap<LayerId, LayerLink>,
}

// LeaderInfo, register_leader, register_follower, followers_for_link, etc.
// stay as-is. The registry is touched during scene build, not during render;
// HashMap is fine here (setup-phase, not per-frame walk).

// ───────────────────────────────────────────────────────────────
// DamageTracker — already clean; no change
// ───────────────────────────────────────────────────────────────

pub struct DamageTracker {
    regions: Vec<Rect<Pixels>>,
    full_repaint: bool,
}

// ───────────────────────────────────────────────────────────────
// Errors — narrow, structured
// ───────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum LayerError {
    #[error("unknown layer id {id:?} in tree")]
    UnknownLayerId { id: LayerId },

    #[error("scene builder stack underflow; pop called with empty stack")]
    BuilderStackUnderflow,

    #[error("composition callback panicked: {panic_type}")]
    CallbackPoisoned { panic_type: &'static str },

    #[error("leader {link:?} has no registered follower")]
    OrphanedLeader { link: LayerLink },

    #[error("follower {follower:?} references unregistered leader {link:?}")]
    OrphanedFollower { follower: LayerId, link: LayerLink },
}

pub type LayerResult<T> = Result<T, LayerError>;
```

`SceneCompositor` (the retained-layer manager) **does not appear in the core types**. It is a forward-looking helper that lives in its own file (`compositor/retained.rs`) and is consumed by `flui-rendering` once Flutter's retained-layer optimisation lands. Today it has 80 LOC and 5 callers, none of which are production code. Stays but is separate from `SceneBuilder`.

---

## 4. State Machine

Two state machines: one for the **scene-construction phase**, one for the **layer lifecycle**.

### Scene construction

```text
LayerTree::new()
  │ Scene::new() / Scene::empty()
  ▼
Scene::empty
  │ scene.builder() -> SceneBuilder<'_>
  ▼
SceneBuilder<'_>                  ◄── holds &mut LayerTree
  │ push_offset / push_opacity / push_clip_rect / … push_*
  │ add_canvas / add_picture / add_texture / add_layer / add_retained
  │ pop / try_pop / pop_all / pop_to_depth
  ▼
SceneBuilder::build(self) -> Option<LayerId>
  │ (consumes builder; releases &mut)
  ▼
Scene { layer_tree, root, … }     ◄── ready for engine
  │ engine.render(&scene)
  ▼
drop(scene)
```

Today's `Scene::dispose(self)` is folded into `drop`. The flow is otherwise unchanged.

### Layer lifecycle

Layers in `flui-layer` are **stateless data containers**. There is no insert/attach/detach state machine — a `Layer` is constructed, inserted into the tree, and lives until the tree drops. The `needs_compositing` flag on `LayerNode` is queried by the engine but is not state to transition through.

This is intentionally different from `RenderObject`'s `Unattached → Attached → LaidOut → Painted → Detached` cycle. Render objects have layout/paint phases that mutate state; layers do not — they are emitted-once, consumed-once.

---

## 5. Public API

The crate's public surface — every other type is implementation detail and lives behind `pub(crate)` or further.

```rust
// Construction
LayerTree::new() -> LayerTree
LayerTree::with_capacity(usize) -> LayerTree
Scene::empty(size: Size<Pixels>) -> Scene
Scene::from_layer(size, Layer, frame: u64) -> Scene    // convenience for 1-layer scenes

// Scene mutation via builder
Scene::builder<'a>(&'a mut self) -> SceneBuilder<'a>   // new — replaces today's SceneBuilder::new(&mut tree)
SceneBuilder<'_>::push_* / add_* / pop / build         // same surface as today, minus the duplicated LayerTree::push_*

// Tree inspection (any time)
LayerTree::root() -> Option<LayerId>
LayerTree::len() -> usize
LayerTree::is_empty() -> bool
LayerTree::contains(id) -> bool
LayerTree::get(id) -> Option<&LayerNode>
LayerTree::get_mut(id) -> Option<&mut LayerNode>
LayerTree::parent(id) -> Option<LayerId>
LayerTree::children(id) -> Option<&[LayerId]>
LayerTree::iter() -> impl Iterator<Item = (LayerId, &LayerNode)>
LayerTree::layer_ids() -> impl Iterator<Item = LayerId>

// TreeRead / TreeNav trait impls (already present)
// These give engine and devtools a generic tree-walk surface.

// Scene-level
Scene::size() -> Size<Pixels>
Scene::layer_tree() -> &LayerTree
Scene::root() -> Option<LayerId>
Scene::root_layer() -> Option<&Layer>
Scene::link_registry() -> &LinkRegistry
Scene::has_content() -> bool
Scene::layer_count() -> usize
Scene::frame_number() -> u64
Scene::add_composition_callback(FnOnce() + Send + 'static)
Scene::fire_composition_callbacks(&mut self)            // consumes the list; one-shot

// LinkRegistry (unchanged)
LinkRegistry::new() / register_leader / register_follower / ...

// DamageTracker (unchanged)
DamageTracker::new() / mark_dirty / damage_rect / ...
```

That is the public API. Compared to today, the following surface **disappears**:

| Today | Replacement / disposition |
|---|---|
| `LayerHandle<T>`, 17 type aliases, `AnyLayerHandle` | Deleted. No external callers. |
| `CompositionCallbackRegistry`, `CompositionCallbackHandle`, `CompositionCallbackId`, `HasCompositionCallbacks` | Folded: `Scene` owns a `Vec<CompositionCallback>` directly. Registry's `Arc<Mutex<>>` storage and 0-impl trait disappear. |
| `LayerTree::push_clip_rect`, `push_clip_rrect`, `push_clip_path`, `push_transform`, `push_opacity` | Deleted. Callers use `SceneBuilder`. |
| `LayerNode::get_layer`, `get_layer_mut` | Deleted (duplicates of `layer`, `layer_mut`). |
| `Scene::dispose(self)` | Deleted; `drop(scene)` is the idiom. |
| `Scene::with_links(…)` | Folded into `Scene::builder()` flow; the registry attaches as the builder publishes its links. |
| `Layer::is_canvas`, `is_picture`, …, 19 × 3 helpers | Macro-collapsed to a derive (kept for ergonomics) OR deleted. See §12 "Rejected designs" — the macro form wins because external callers do use `is_picture()`. |
| `SceneCompositor::retain` / `release` / `clear_retained` | Moved to `compositor/retained.rs`. Stays but stops being mixed in with `SceneBuilder`. |
| `unsafe impl Send` / `Sync` blocks (39) | Deleted. Auto-derived from the field types. |

---

## 6. Internal Modules

```text
crates/flui-layer/src/
  lib.rs                      — re-exports, prelude, version
  error.rs                    — LayerError + LayerResult
  layer/                      — concrete layer struct files + closed enum
    mod.rs                    — Layer enum + bounds() / needs_compositing() /
                                  is_opaque() (3 semantic methods; not 57)
    bounds.rs                 — LayerBounds trait (extracted from mod.rs)
    dispatch.rs               — generated/macro is_*/as_*/as_*_mut helpers
                                  (or deleted; decision in Mythos Step 4)
    canvas.rs                 — CanvasLayer
    picture.rs                — PictureLayer
    texture.rs                — TextureLayer
    platform_view.rs          — PlatformViewLayer
    performance_overlay.rs    — PerformanceOverlayLayer
    clip_rect.rs              — ClipRectLayer
    clip_rrect.rs             — ClipRRectLayer
    clip_path.rs              — ClipPathLayer
    clip_superellipse.rs      — ClipSuperellipseLayer
    offset.rs                 — OffsetLayer
    transform.rs              — TransformLayer
    opacity.rs                — OpacityLayer
    color_filter.rs           — ColorFilterLayer
    image_filter.rs           — ImageFilterLayer
    shader_mask.rs            — ShaderMaskLayer
    backdrop_filter.rs        — BackdropFilterLayer
    leader.rs                 — LeaderLayer + LayerLink
    follower.rs               — FollowerLayer
    annotated_region.rs       — AnnotatedRegionLayer + AnnotationValue + SemanticLabel + SystemUiOverlayStyle
    annotation/               — annotation search system
      mod.rs
      entry.rs                — AnnotationEntry / AnnotationResult / AnnotationSearchOptions
  tree/                       — Slab arena + tree trait impls
    mod.rs
    layer_tree.rs             — LayerTree + LayerNode (post-split: ~250 LOC after extracting tests)
    tree_traits.rs            — TreeRead<LayerId> + TreeNav<LayerId>
  compositor/                 — scene building + retention
    mod.rs                    — re-exports
    builder.rs                — SceneBuilder<'a> (extracted from compositor.rs)
    retained.rs               — SceneCompositor + CompositorStats (extracted from compositor.rs)
  scene.rs                    — Scene (post-fold-in composition callbacks)
  link_registry.rs            — LinkRegistry + LeaderInfo
  damage.rs                   — DamageTracker (untouched)
  tests/                      — integration tests pulled out of the .rs files
    layer_tree.rs             — extracted 720 LOC test suite
    scene_builder.rs          — extracted ~300 LOC test suite
    link_registry.rs          — already in tests/ idiom; just extract
```

**What `handle.rs` and `composition_callback.rs` become.** Both files are deleted. The 467 LOC of `LayerHandle<T>` + 17 type aliases evaporate. The 358 LOC of `CompositionCallbackRegistry` reduces to a ~30 LOC `CompositionCallback` newtype on `Scene` plus its tests folded into `scene.rs`'s test module.

**What `lib.rs` re-exports.** Today 291 lines, including a `prelude` module. After the cut: `LayerId`, `Layer`, `LayerNode`, `LayerTree`, `Scene`, `SceneBuilder`, `LinkRegistry`, `LeaderInfo`, `LayerLink`, `LayerBounds` trait, `DamageTracker`, the 18 concrete layer types, plus `AnnotationEntry` / `AnnotationResult` / `AnnotationSearchOptions` / `SemanticLabel` / `SystemUiOverlayStyle` / `PerformanceOverlayOption` / `PerformanceStats` / `PlatformViewId` / `PlatformViewHitTestBehavior`. Estimated 35-40 names total. The `prelude` module shrinks to the same set with no `LayerHandle` or `CompositionCallback*` entries.

---

## 7. Async & Failure Semantics

**Task ownership.** Zero. The crate runs synchronously inside the paint phase + render phase. No `spawn`, no `tokio::task`, no `JoinHandle`.

**Cancellation.** Not applicable. A `Scene` is either fully constructed and consumed, or dropped mid-construction (which is just dropping the `LayerTree` it was building inside).

**Retry.** Not applicable at the layer level. The frame loop above is responsible for retry.

**Idempotency.** Tree mutation is naturally idempotent at the `add_child` boundary (the underlying `Vec::push` is not — calling `add_child(p, c)` twice gives two child entries). The current code has this property today and we preserve it.

**Backpressure.** Not applicable — no channel, no queue, no buffer.

**Shutdown.** `Scene::drop` releases the `LayerTree` (which releases the `Slab` of `LayerNode`s), the `LinkRegistry` HashMaps, and the `Vec<CompositionCallback>` (running any unfired `FnOnce`s during `Drop` is **rejected** — that would surprise callers; we drop without firing). The composition callback list is normally drained explicitly by `Scene::fire_composition_callbacks()` before drop.

**Partial failure recovery.** A composition callback that panics during `fire_composition_callbacks()` is caught via `std::panic::catch_unwind` (wrapping `AssertUnwindSafe`), surfaced as `LayerError::CallbackPoisoned { panic_type: type_name }`, and **does not prevent subsequent callbacks from firing**. The `fire_composition_callbacks` method collects all errors into a `Vec<LayerError>` and returns them at once. This mirrors the `flui-rendering` `Poisoned` model (Mythos Step 12 commit `dc0fa1ad`).

**Two-phase commits.** Not needed; everything is in-memory.

---

## 8. Security Model

`flui-layer` is a library, not a service. It does not handle credentials, secrets, or network input. Its trust boundaries are:

**Trusted inputs.**
- `Layer` variants — closed enum, all variants are crate-defined.
- `LayerId` values — issued by `LayerTree::insert`, validated on `get` (returns `None` on miss).
- `Size<Pixels>`, `Rect<Pixels>`, `Offset<Pixels>` — from `flui-types`, validated at their boundary (NaN check, finite-value check).
- `Picture` — recorded DisplayList from `flui-painting`; trusted because `flui-painting` is in the same workspace and validates at its boundary.

**Untrusted inputs.**
- Composition callbacks (`FnOnce() + Send + 'static`) — third-party closures. Can panic (caught), can run for unbounded time (not detected; the engine's frame budget will time out elsewhere), can allocate unbounded memory (not detected; OS will OOM).
- Layer configurations (e.g. `ShaderMaskLayer::new(shader, blend_mode, bounds)`) — the shader's GPU effect is `flui-engine`'s concern; this crate only stores the bytes.

**Capabilities.** None. The crate does not mediate authority. The composition callbacks run with the privileges of the host process.

**Secret handling.** Not applicable. `Layer::Debug` impls may print configuration data; documenting "do not embed secrets in layer configurations" mirrors the rendering crate's guidance.

**Logging rules.** No layer configuration logged at info-level. The crate is currently a `tracing` dep but does not emit any spans; that is acceptable for now (no diagnostics yet). When spans are added (`tracing::instrument` on `SceneBuilder::push_*`), they will use `LayerId`, not layer contents.

**Serialization.** `Scene` is not serializable in this crate. Devtools-flavoured serialization lives in `flui-devtools` (currently disabled) with explicit redaction.

**Plugin/user input rules.** No plugin surface — the enum is closed. The only third-party input is the composition callback closure body, which is sandboxed by `catch_unwind` for panics but not for resource consumption.

---

## 9. Data-Oriented Notes

**Hot data.** Touched every frame during render:
- `LayerNode::layer` (the `Layer` enum tag + payload) — ~64 bytes typical, ~200 bytes worst-case (`PerformanceOverlayLayer` has the largest payload).
- `LayerNode::children` (`Vec<LayerId>` — 24-byte header + heap allocation). The `TreeNav::AVG_CHILDREN = 4` constant suggests `SmallVec<[LayerId; 4]>` would inline most cases, but that change is a separate post-Mythos optimisation (see Mythos Step 13).
- `LayerNode::parent` (`Option<LayerId>` — 8 bytes via NonZeroUsize niche).
- `LayerNode::offset` (`Option<Offset<Pixels>>` — 24 bytes for the wrapping Option).

**Cold data.**
- `LayerNode::element_id` (`Option<ElementId>`) — debug/devtools tracing only.
- `LayerNode::needs_compositing` — boolean; rarely read at render time.
- `LinkRegistry` HashMaps — touched during build, occasionally read during render (when resolving follower positions). Off the per-layer hot walk.

**Allocation strategy.**
- `Slab<LayerNode>` arena: O(1) insert/delete, dense reuse, ID stability. Same pattern as `RenderTree`. Already in place.
- `Vec<LayerId>` per node's children: amortised; small allocations bound by tree fan-out (typically 1-8 children per node).
- `Vec<CompositionCallback>` on `Scene`: small (typically 0-2 callbacks per frame; most scenes have none).
- `HashMap<LayerLink, LeaderInfo>` + `HashMap<LayerId, LayerLink>` in `LinkRegistry`: small (most apps have 0-3 leader-follower pairs).
- `Vec<Rect<Pixels>>` in `DamageTracker`: bounded by dirty-region count per frame.

**Forbidden allocations.**
- No `Arc::clone` inside the engine's per-layer walk (Trigger 5, extended to flui-layer via the port-check extension).
- No `HashMap` lookup on the per-frame layer walk (only on initial `LinkRegistry` query for follower positioning, which is a one-time-per-follower lookup).
- No `Box<dyn Layer>` allocation per layer — the enum is inline.
- No `Arc<Layer>` cloning — layers are owned by their `LayerNode`.

**Cache locality.**
- `LayerNode` is ~120 bytes (estimate; pending audit). One cache line per node is the target.
- `Slab<LayerNode>` stores nodes contiguously; sequential traversal in insertion order is cache-friendly.
- The `Vec<LayerId>` children pointer breaks locality on its first access; the SmallVec optimisation referenced above would put 4 children inline (32 bytes) and keep the parent + first 4 children on the same cache line.

**Where `Arc`/`Mutex`/`HashMap`/`Box`/`dyn Trait` are acceptable.**
- `LinkRegistry`'s two `HashMap`s — sparse, setup-phase.
- `Box<dyn FnOnce() + Send + 'static>` inside `CompositionCallback` — necessary because the callback type is heterogeneous and erased at the boundary. One allocation per callback registered; typically 0-2 per scene.
- No `Arc` in production layer code (audit will confirm; today there is `Arc<AtomicUsize>` inside `LayerHandle` which is being deleted, and `Arc<Mutex<CallbackStorage>>` inside `CompositionCallbackRegistry` which is being folded into `Scene`).
- No `Mutex` in production layer code after the composition-callback refactor.

**Where they are forbidden.**
- `Arc<RwLock<LayerTree>>` — never. The tree has one owner.
- `Mutex<HashMap<LayerId, _>>` — never. State lives on `LayerNode`, not in side tables.
- `Arc<Mutex<Vec<Box<dyn Fn>>>>` — the composition-callback shape today. Deleted.
- `unsafe impl Send + Sync` on layer types — deleted across all 18 layer files + scene.rs + handle.rs.

---

## 10. Error Model

```rust
#[derive(Debug, thiserror::Error)]
pub enum LayerError {
    // ── Programmer error / structural ──
    #[error("unknown layer id {id:?} in tree")]
    UnknownLayerId { id: LayerId },

    #[error("scene builder stack underflow; pop called with empty stack")]
    BuilderStackUnderflow,

    // ── LinkRegistry consistency ──
    #[error("leader {link:?} has no registered follower")]
    OrphanedLeader { link: LayerLink },

    #[error("follower {follower:?} references unregistered leader {link:?}")]
    OrphanedFollower { follower: LayerId, link: LayerLink },

    // ── Composition callback poison ──
    #[error("composition callback panicked: {panic_type}")]
    CallbackPoisoned { panic_type: &'static str },
}

pub type LayerResult<T> = Result<T, LayerError>;
```

**Retryable** — `CallbackPoisoned`. The frame containing the panicking callback is dropped; subsequent frames may not re-register the broken callback (caller's choice).

**Terminal for this frame** — `CallbackPoisoned` (when there are no further callbacks to fire) and `BuilderStackUnderflow` (programmer error, panic equivalent).

**User-facing** — `BuilderStackUnderflow` is the canonical programmer-error signal; a panic in a `SceneBuilder` user is a bug in `flui-rendering`'s paint phase.

**Internal only** — `UnknownLayerId`, `OrphanedLeader`, `OrphanedFollower`. These should not reach an end-user; they signal tree inconsistency the framework should never construct.

**Security-sensitive** — none.

`anyhow::Error` is **never** returned from this crate's public API. Internally, `anyhow::Context` may wrap diagnostics inside test code, but the public surface is `LayerError` only.

**Today's panic-based error paths** — `SceneBuilder::pop` panics on empty stack today. The Mythos plan replaces this with `try_pop` as the default and `pop` becomes a `Result`-returning wrapper. Programmer error stays panic-flavoured at the boundary; user code gets `Result`.

---

## 11. Tests Required

Each test must prove a design guarantee.

**Invariants on `LayerTree`.**
- `tree.insert(layer).get(id)` round-trips.
- IDs are 1-based via NonZeroUsize.
- Removing a node updates `root` if the removed node was root.
- `add_child(p, c)` updates both `p.children` and `c.parent`.
- `clear_children(p)` clears `p.children` AND nulls `parent` on each child.
- `get_two_mut` (a future addition if Mythos plan needs disjoint-borrow primitives) returns disjoint borrows.

**Invariants on `Scene`.**
- A `Scene` constructed via `Scene::builder()` and then `build()` has `layer_count == scene.layer_tree().len()`.
- Composition callbacks registered via `Scene::add_composition_callback` fire exactly once per `fire_composition_callbacks` call.
- A panicking callback returns `LayerError::CallbackPoisoned` but does not prevent the remaining callbacks from firing.

**Invariants on `SceneBuilder`.**
- Push N layers, pop N times: `depth() == 0`, `root == first_pushed_id`.
- `build()` returns the root.
- `pop` on empty stack returns `Err(BuilderStackUnderflow)` (changed from today's panic).

**Phase invariants.** None (the crate is sync and has no phase typestate; the SceneBuilder borrow checker enforces single-writer).

**Cancellation.** Not applicable.

**Retry / idempotency.**
- A fresh `Scene::builder()` after `Scene::dispose` (now `drop`) produces an equivalent scene from the same script.

**Authorization.** Not applicable.

**Malformed input.**
- `LayerId::new(0)` is a compile/test error via `NonZeroUsize`.
- `Rect::from_xywh(..., NaN, ...)` propagates via the `flui-types` validation layer (not this crate's concern).
- `Layer::Picture(PictureLayer::new(empty_picture))` is valid; an empty Picture is legal (zero-byte DisplayList).

**Concurrency.**
- `Scene` is `Send`: move it across a thread boundary and consume it on the other side. Compile-test via `fn assert_send<T: Send>()`.
- `Scene` is **not** `Sync`. We do not promise read-from-multiple-threads; that would require `Arc<Scene>` ceremony that has no current consumer.
- No loom test needed; there are no concurrent mutation paths to interleave.

**Property tests.**
- For any sequence of `(insert, add_child, remove)`, the tree is consistent: every reachable `LayerId` has a parent (except root), every parent's `children` contains its actual children, no cycles.
- For any tree of depth ≤ `TreeNav::MAX_DEPTH = 32`, `descendants(root)` terminates and visits every node exactly once.
- Filed as deferred test class, mirroring `flui-rendering` `Outstanding refactors`.

**Loom tests.** Not applicable (no concurrent state in the crate after the composition-callback fold-in).

**Miri tests.** Run `cargo +nightly miri test -p flui-layer` to verify the Slab access patterns. If a disjoint-borrow primitive is added in a future step (none planned in the current Mythos chain), it gets a miri gate.

**Integration tests.**
- End-to-end scene: builder pushes Offset → Opacity → Clip → Canvas, builds, asserts the resulting tree shape and the `Scene::root_layer()` identity.
- Damage-tracking: mark dirty rect, query damage, verify intersection with frame bounds.
- LinkRegistry: register leader, register follower, unregister leader (orphans follower), verify `remove_orphaned_followers` cleans up.

---

## 12. Rejected Designs

For each rejected design: what it was, why it was tempting, why it is wrong here.

### `Box<dyn Layer>` plugin trait

**What:** Replace the closed `Layer` enum with a trait object: `Box<dyn Layer + Send + Sync>`. Each layer type implements the trait and the engine matches by `Any::type_id` or via a vtable method.

**Why tempting:** Mirrors Flutter's Dart class hierarchy directly (`abstract class Layer`, 18 concrete subclasses). Open-set extension point for downstream crates.

**Why wrong:** The GPU backend in `flui-engine` cannot lower an arbitrary `dyn Layer` to wgpu draw calls — every variant needs a hand-written translation. Either every "third-party" layer ships with its own wgpu shader pack (massive extension surface) or the trait is closed by convention (in which case the enum is cheaper and more honest). Rust's borrow checker also gives the closed enum match-exhaustiveness checks that catch missing translations at compile time; the trait object loses that.

### `Arc<RwLock<LayerTree>>` shared layer tree

**What:** Today's documentation literally recommends `Arc<RwLock<LayerTree>>` for "multi-threaded access". Make that the default shape: every `Scene` carries an `Arc<RwLock<LayerTree>>` so background work can mutate it.

**Why tempting:** Allows async layer construction (e.g. background image decoders feeding `Layer::Texture` updates into a live `LayerTree`). Mirrors Flutter's `addToScene` Dart pattern where layers can be appended on different microtasks.

**Why wrong:** Lock contention on the render-thread → painter-thread boundary, plus the lock guards an arena that has no actual cross-thread mutation today. The single-owner shape with `Scene: Send` and value-moving across threads gives the same flexibility (background workers build their own subtrees and emit them as values; the merge happens on the render thread in O(subtree-size)). The `Arc<RwLock<>>` shape is a tax with no payback.

### `LayerHandle<T>` as today, kept "for future GPU lifecycle"

**What:** Keep the 467 LOC `LayerHandle<T>` and its 17 type aliases. Argument: "Flutter has `LayerHandle<T extends Layer>` to manage GPU resource lifecycles. We will need it eventually."

**Why tempting:** Avoids the discomfort of deleting working code. Insurance against a future need.

**Why wrong:** No external caller in the workspace reads `ref_count`. The `Arc<AtomicUsize>` is bumped and decremented but no consumer ever sees the count reach zero and acts. The "GPU resource release" hook the docs promise does not exist in `flui-engine`. The handle is fake-ownership ceremony — it owns `Option<T>` directly, which is just `Option<T>`. **Delete it.** If a future Mythos chain needs GPU lifecycle management, the right shape is to attach it to the `flui-engine` resource registry, not to a wrapper in `flui-layer`. Rebuilding from scratch will be ~50 LOC; the current 467 LOC is hostile to that rebuild.

### `CompositionCallbackRegistry` as a shared `Arc<Mutex<>>` registry

**What:** Keep today's shape — `Arc<Mutex<Vec<(Id, Box<dyn Fn() + Send + Sync>)>>>` with a `Clone` impl that shares the storage and a separate `HasCompositionCallbacks` trait for layers to implement.

**Why tempting:** Mirrors Flutter's `Layer.addCompositionCallback` pattern where each container layer carries its own callback list and they bubble up at composite time.

**Why wrong:** `HasCompositionCallbacks` has zero impls today; the trait is dead. The registry is a shared `Arc<Mutex<>>` that no caller actually shares across threads (no `Clone` in production code). The `Box<dyn Fn() + Send + Sync>` heap allocation per callback is paid for callbacks that may never fire. The correct shape is `Scene::add_composition_callback(FnOnce() + Send + 'static)` storing them in a plain `Vec<CompositionCallback>` on `Scene` and firing them once at scene-finalisation. One ownership chain, no lock, `FnOnce` not `Fn` (one-shot semantics match the callback's actual use), no `Arc`.

### `enum_dispatch` crate for the `is_*`/`as_*` boilerplate

**What:** Pull in the `enum_dispatch` proc-macro crate to auto-generate `is_*` and `as_*` accessors on the `Layer` enum.

**Why tempting:** Eliminates 57 lines of boilerplate. Macro generates them cleanly.

**Why wrong:** New dependency for a small win; the macro produces methods that callers cannot inspect easily. The hand-written `paste!`-style macro inside `layer/dispatch.rs` (or even a tiny `macro_rules!` `gen_layer_accessors!`) gives the same output with no new crate. Adopted in the Mythos plan; `enum_dispatch` rejected.

### Make `Scene` `Sync` via `Arc<Scene>` wrapping

**What:** Implement `Sync` on `Scene` so multiple render threads can hold `&Scene` simultaneously (e.g. one thread renders to backbuffer A while another renders to backbuffer B from the same scene).

**Why tempting:** Multi-output rendering (mirror to second monitor, screenshot at same time as live render). Sounds useful.

**Why wrong:** The wgpu backend in `flui-engine` is single-threaded per `Surface`; multiple surfaces have their own `Scene` instances. Adding `Sync` would force every layer's internal mutability to be `Sync` (most are today via auto-derive; the change wouldn't break anything immediately), but it would invite `Arc<Scene>` ceremony at every caller with no concrete need. Today's `Send`-only `Scene` is correct.

### Per-layer `unsafe impl Send + Sync` retained because "of course it's safe"

**What:** Keep the 39 `unsafe impl Send` / `Sync` blocks because they compile and don't do harm.

**Why tempting:** Doesn't hurt anything. Status quo. Mass deletion is risky if some layer secretly holds a `Cell<T>` somewhere.

**Why wrong:** The unsafe is a lie. `BackdropFilterLayer` has the comment "contains only owned, Send types" directly above its `unsafe impl Send` — the comment proves the unsafe is unnecessary. Every layer's fields are auto-Send/Sync (audit will confirm). The unsafe blocks pollute every layer file with a maintenance burden that adds nothing. Delete them and let the compiler verify. If a future layer adds a `Cell<T>` or `Rc<T>` field, the compile error will surface immediately — and that surface is the right place to think about thread-safety, not a copy-pasted unsafe block.

### Keep the `tracing` dep but emit nothing

**What:** `tracing` is in `Cargo.toml` but no `tracing::info!` / `span!` calls exist in the production code. Argument: leave the dep, add spans later.

**Why tempting:** Avoids a Cargo.toml shuffle.

**Why wrong:** Dead dependency = compile-time tax + audit surface that doesn't earn its keep. Either add spans now (`tracing::instrument` on `SceneBuilder::push_*` and `Scene::fire_composition_callbacks`) or remove the dep. Mythos plan: add minimal spans on builder ops + scene finalisation, document in the ARCHITECTURE.md `Thread safety` section.

### Keep the `parallel = ["rayon"]` feature with no rayon use

**What:** Keep the optional `rayon` dependency behind the `parallel` feature flag, intending to wire up a parallel tree-walk later.

**Why tempting:** "We will need it eventually" — premature parallelism.

**Why wrong:** Feature flag with no implementation is a lie. Rayon is heavy (transitive deps). Delete both the feature and the dep. When parallel layer-tree traversal becomes a measured need, re-add with the actual implementation; this is days of work, not a feature flag.

### Helper submodules `compositor/helpers.rs`, `tree/helpers.rs`

**What:** Group utility functions in `helpers.rs` to "avoid cluttering the main module".

**Why tempting:** Quick way to extract code from a big file.

**Why wrong:** "Helper" is a naming smell. If a function is genuinely shared, it belongs on the type it manipulates or in a named submodule about its concern (e.g. `tree/iter.rs` if it's iteration helpers, `compositor/builder.rs` if it's builder helpers). Reject the name; require functional names for any extraction.

---

## 13. Implementation Plan

Ordered. Each step lands as a reviewable commit. Each step compiles and passes tests independently. Steps are numbered to map onto the `flui-rendering` Mythos plan's step format for cross-reference.

### Step 1 — Delete dead surface: `LayerHandle<T>` + type aliases

- Delete `crates/flui-layer/src/handle.rs` (467 LOC).
- Remove `handle::*` re-exports from `lib.rs` and `prelude`.
- Verify zero external callers (`grep -r "LayerHandle\|AnyLayerHandle" crates/`).
- Update workspace docs and CLAUDE.md if any reference it.

**Verifies:** `cargo build --workspace` clean; no other crate referenced `LayerHandle`.

### Step 2 — Delete dead trait: `HasCompositionCallbacks` + fold registry

- Delete `HasCompositionCallbacks` trait (0 impls).
- Replace `CompositionCallbackRegistry` (`Arc<Mutex<Vec<(Id, Box<dyn Fn>)>>`) with `Vec<CompositionCallback>` on `Scene` where `CompositionCallback(Box<dyn FnOnce() + Send + 'static>)`.
- Add `Scene::add_composition_callback`, `Scene::fire_composition_callbacks(&mut self) -> Vec<LayerError>`.
- Delete `composition_callback.rs` (358 LOC) — relocate the type definitions to `scene.rs` (~30 LOC).
- Remove `composition_callback::*` re-exports.

**Verifies:** the only test (`test_callback_id_uniqueness` and friends) ports to the new shape and passes.

### Step 3 — Delete 39 `unsafe impl Send/Sync` blocks

- Across `layer/*.rs` (18 files × 2 = 36 blocks), `scene.rs` (1), `handle.rs` (2 — already deleted in Step 1).
- Rely on auto-derivation. If any compile error surfaces, it identifies a layer with a non-Send field that needs separate attention (e.g. `Rc<T>`, `Cell<T>`, raw pointer).

**Verifies:** `cargo build --workspace` + `cargo test -p flui-layer --lib` green; no soundness regression.

### Step 4 — Macro-collapse `is_*`/`as_*` dispatch on `Layer`

- Create `layer/dispatch.rs` with a `macro_rules! gen_layer_accessors!` macro (or `paste!`-based) that expands the 19 variants × 3 methods.
- Replace the 600 LOC of hand-written boilerplate in `layer/mod.rs` with a single macro invocation.
- Add audit comment listing what's generated.

**Verifies:** the existing tests for `is_*` / `as_*` still pass. `layer/mod.rs` drops from 1075 LOC to ~300 LOC.

### Step 5 — Delete `LayerTree::push_*` helpers (duplicates of `SceneBuilder::push_*`)

- Delete `LayerTree::push_clip_rect`, `push_clip_rrect`, `push_clip_path`, `push_transform`, `push_opacity` (5 methods, ~120 LOC).
- Verify callers — should be zero in production code; the helpers are exercised only by tests in `layer_tree.rs`.
- Delete the corresponding test methods (`test_push_clip_rect`, `test_push_clip_rrect`, etc.). The same scenarios are covered by `SceneBuilder` tests.

**Verifies:** the test count drops but each remaining test exercises the canonical `SceneBuilder` API.

### Step 6 — Split god module `tree/layer_tree.rs`

- Extract the 720 LOC test suite to `tests/layer_tree.rs` (integration test crate).
- Keep `layer_tree.rs` to ~250 LOC of production code: `LayerNode` struct + `LayerTree` struct + core methods.
- Move `LayerNode::get_layer` / `get_layer_mut` deletions into this step (they duplicate `layer` / `layer_mut`).

**Verifies:** `cargo test -p flui-layer --tests` still runs the same test names; `layer_tree.rs` LOC drops from 1660 to ~250.

### Step 7 — Split god module `compositor.rs`

- Move `SceneCompositor` + `CompositorStats` (~80 LOC of impl + 70 LOC of tests) to `compositor/retained.rs`.
- Keep `SceneBuilder<'a>` in `compositor/builder.rs` (~600 LOC).
- Extract the ~300 LOC of `SceneBuilder` tests to `tests/scene_builder.rs`.
- Re-export both from `compositor/mod.rs`.

**Verifies:** `compositor.rs` (now `mod.rs`) drops to ~20 LOC of re-exports.

### Step 8 — Simplify `Scene` construction

- Delete `Scene::dispose(self)`. Update callers (engine, hot-reload) to use `drop(scene)`.
- Collapse `Scene::new(size, tree, root, frame)` + `Scene::with_links(size, tree, root, links, frame)` into a single typed builder OR a `Scene::new_with_links(...)` variant — pick the simpler. (Recommend: keep `Scene::new` for the common path and `Scene::new_with_links` for the link-aware path; both fully-typed, no builder needed.)
- Add `Scene::builder(&mut self) -> SceneBuilder<'_>` as an alternative to `SceneBuilder::new(&mut tree)` for ergonomics.

**Verifies:** the existing `Scene` tests adapt; external callers (binding, direct, hot-reload) only need cosmetic changes.

### Step 9 — `LayerTree` doc-comment correction

- Replace the line "Use `Arc<RwLock<LayerTree>>` for multi-threaded access" with "Construct on one thread, move `Scene` by value to the render thread."
- Update the `## Thread Safety` section of the docstring to match the new model.

**Verifies:** doc-comment is no longer giving anti-pattern advice.

### Step 10 — Error model

- Add `crates/flui-layer/src/error.rs` with `LayerError` + `LayerResult`.
- Replace `SceneBuilder::pop`'s panic with `Result<LayerId, LayerError::BuilderStackUnderflow>`. Keep `try_pop` as the panic-free alternative.
- Wrap `Scene::fire_composition_callbacks` in `catch_unwind` per callback; return `Vec<LayerError>` of poisoned ones.

**Verifies:** `cargo test -p flui-layer` covers the new error paths. Callers (paint phase in `flui-rendering`) handle the new `Result`.

### Step 11 — Drop unused dependencies + features

- Audit `parallel = ["rayon"]` feature. Either implement parallel tree-walk (out of scope) or delete both the feature and the rayon dep.
- Audit `tracing` dep. Either add `#[tracing::instrument]` on `SceneBuilder::push_*` and `Scene::fire_composition_callbacks` (cheap; align with `flui-rendering` convention) or delete the dep.
- Recommend: keep `tracing`, add minimal instrumentation; delete `rayon`+feature.

**Verifies:** `cargo build --workspace` clean; `cargo tree --workspace -e features` smaller.

### Step 12 — Per-crate `ARCHITECTURE.md`

- Create `crates/flui-layer/ARCHITECTURE.md` per the `docs/PORT.md` template:
  - `## Flutter source mapping` (table: layer.dart class → `flui-layer/src/layer/*.rs`)
  - `## Mapping decisions` (Accepted trade-offs for: closed enum vs `Box<dyn>`, `Vec<CompositionCallback>` vs `Arc<Mutex<>>`, single-owner `LayerTree` vs `Arc<RwLock<>>`, delete `LayerHandle`)
  - `## Thread safety` (table: `Scene` Send-only, `LayerTree` no locks, `LinkRegistry` HashMaps off hot path)
  - `## Friction log` (anything not yet refactored at end of chain)
  - `## Outstanding refactors` (SmallVec for `LayerNode::children`, optional Mythos Step 13 / 14 carryovers, ARCHITECTURE.md grafts on other crates that consume `flui-layer`)

**Verifies:** `docs/PORT.md` `## Index` flips `flui-layer` from "Not yet templated" to "Templated 2026-05-20".

### Step 13 — Extend `scripts/port-check.sh`

- Add `crates/flui-layer/src` to Trigger 1 and Trigger 2 path globs.
- Add Trigger 3 to scan `crates/flui-layer/src` for `async fn build|layout|paint|composite`.
- Add Trigger 5 for `Arc::clone(` in `crates/flui-engine/src/wgpu/layer_render.rs` (the per-frame layer walk). Forward-looking; should match nothing today.
- Run `bash scripts/port-check.sh -v` and verify all triggers stay clean.

**Verifies:** `port-check` exits 0; the methodology now covers `flui-layer`.

### Step 14 — Tests pass + clippy clean

- Run `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`.
- Address any new clippy warnings introduced by the refactor (deletions sometimes uncover dead-code lints on adjacent imports).
- The deferred test classes (property tests, miri, parallel-tree-walk benches) are documented in `Outstanding refactors`, not landed in this chain.

**Verifies:** the workspace is fully green; the refactor is mergeable.

---

## Self-check

- **Did I start from data, not traits?** Yes. `LayerTree`/`LayerNode`/`Layer` enum/`Scene` are the spine. There is no `Box<dyn Layer>` plugin trait — that is explicitly rejected.
- **Did every module earn its existence?** Two modules (`handle.rs`, `composition_callback.rs`) flagged for deletion. Three modules (`tree/layer_tree.rs`, `layer/mod.rs`, `compositor.rs`) flagged for split because their responsibilities are distinct.
- **Did I identify the state owner?** Yes. `Scene` owns `LayerTree` owns `Slab<LayerNode>`. Exactly one mutable instance per scene; moved across threads by value.
- **Did I define cancellation behavior?** Yes. The crate is sync; cancellation is not applicable. Dropping a `Scene` in mid-construction is safe (drops the slab) and produces no side effects.
- **Did I define trust boundaries?** Yes. The `Layer` enum is closed; the GPU lowering in `flui-engine` is the only consumer. Composition callbacks are sandboxed via `catch_unwind` for panics.
- **Did I avoid fake extensibility?** Yes. `HasCompositionCallbacks` trait (0 impls) and `LayerHandle<T>` (0 external callers) are slated for deletion. The closed enum keeps the layer vocabulary explicit.
- **Did I avoid Quick Win architecture?** The plan executes 14 steps including dead-code deletion (`LayerHandle`, `CompositionCallbackRegistry`), god-module splits (`layer_tree.rs`, `layer/mod.rs`, `compositor.rs`), unsafe deletions (39 blocks), API surface trimming (`LayerTree::push_*` helpers, `Scene::dispose`, duplicate `get_layer`/`layer` accessors), and methodology extension (`port-check.sh` paths). Ripples land in `flui-rendering` and `flui-engine` (`Scene::dispose` → `drop`, `SceneBuilder::pop` → `Result`); both are executed in-band, not deferred.
- **Did I encode invariants in types where possible?** Yes. `LayerId` is `NonZeroUsize`; the closed enum gives exhaustive-match compile-time checks; `Scene::add_composition_callback` takes `FnOnce` (one-shot, single fire semantics). The `BuilderStackUnderflow` error replaces a runtime panic with a `Result` at the boundary.
- **Did I reject bad alternatives?** Eight rejected designs documented in Section 12.
- **Could a Rust developer implement this design without guessing?** Yes, given the implementation plan in Section 13 and the type sketches in Section 3.
