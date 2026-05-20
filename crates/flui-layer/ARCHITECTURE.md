# flui-layer Architecture

This document is the per-crate template instance for `flui-layer` as defined by [`docs/PORT.md`](../../docs/PORT.md). It records the Flutter → Rust mapping for this crate, the divergence decisions taken during the Mythos chain (PR opened 2026-05-20, commits `fa7cbe6a` through `6abaa7df`), the current thread-safety surface, the known friction not yet refactored, and the planned cleanups that the methodology will pick up next.

The deeper Mythos design verdict lives at [`docs/designs/2026-05-20-mythos-flui-layer-redesign.md`](../../docs/designs/2026-05-20-mythos-flui-layer-redesign.md). The implementation plan lives at [`docs/plans/2026-05-20-002-feat-flui-layer-mythos-redesign-plan.md`](../../docs/plans/2026-05-20-002-feat-flui-layer-mythos-redesign-plan.md). The requirements brainstorm lives at [`docs/brainstorms/flui-layer-mythos-redesign-requirements.md`](../../docs/brainstorms/flui-layer-mythos-redesign-requirements.md).

---

## Flutter source mapping

The Flutter compositor `Layer` hierarchy lives in [`.flutter/flutter-master/packages/flutter/lib/src/rendering/layer.dart`](../../.flutter/flutter-master/packages/flutter/lib/src/rendering/layer.dart) (3,029 LOC, `abstract class Layer with DiagnosticableTreeMixin` at line 144). FLUI flattens the inheritance graph to a closed `Layer` enum — a deliberate divergence recorded in [Mapping decisions](#mapping-decisions).

| Flutter source (`layer.dart`) | FLUI module | Notes |
|---|---|---|
| `abstract class Layer` (line 144) + `abstract class ContainerLayer` (line 1077) | [`src/layer/mod.rs`](src/layer/mod.rs) | Closed `enum Layer { Canvas(CanvasLayer), Picture(PictureLayer), … }` with 19 variants. No `Box<dyn Layer>` plugin trait; see Mapping decisions #1. |
| `class PictureLayer extends Layer` (line 824) | [`src/layer/picture.rs`](src/layer/picture.rs) | Wraps `flui_painting::Picture` (a recorded `DisplayList`). |
| `class TextureLayer extends Layer` (line 952) | [`src/layer/texture.rs`](src/layer/texture.rs) | External GPU texture. `is_opaque()` controls compositing fast-path. |
| `class PlatformViewLayer extends Layer` (line 1005) | [`src/layer/platform_view.rs`](src/layer/platform_view.rs) | Native view embedding (Android View, iOS UIView, etc.). |
| `class PerformanceOverlayLayer extends Layer` (line 1032) | [`src/layer/performance_overlay.rs`](src/layer/performance_overlay.rs) | FLUI implementation is larger (530 LOC) than Flutter's; provides graph rendering directly rather than delegating to a SkPerformanceOverlay primitive. |
| `class OffsetLayer extends ContainerLayer` (line 1459) | [`src/layer/offset.rs`](src/layer/offset.rs) | Simple translation; base class for repaint boundaries in Flutter. FLUI keeps it as a flat enum variant. |
| `class ClipRectLayer extends ContainerLayer` (line 1601) | [`src/layer/clip_rect.rs`](src/layer/clip_rect.rs) | |
| `class ClipRRectLayer extends ContainerLayer` (line 1694) | [`src/layer/clip_rrect.rs`](src/layer/clip_rrect.rs) | |
| `class ClipRSuperellipseLayer extends ContainerLayer` (line 1784) | [`src/layer/clip_superellipse.rs`](src/layer/clip_superellipse.rs) | iOS-style squircle clipping. FLUI variant: `Layer::ClipSuperellipse`. |
| `class ClipPathLayer extends ContainerLayer` (line 1871) | [`src/layer/clip_path.rs`](src/layer/clip_path.rs) | |
| `class ColorFilterLayer extends ContainerLayer` (line 1953) | [`src/layer/color_filter.rs`](src/layer/color_filter.rs) | Wraps a `ColorMatrix` via `flui_types::painting::effects::ColorMatrix`. |
| `class ImageFilterLayer extends OffsetLayer` (line 1993) | [`src/layer/image_filter.rs`](src/layer/image_filter.rs) | Blur / dilate / erode. Flutter inherits from `OffsetLayer`; FLUI flattens. |
| `class TransformLayer extends OffsetLayer` (line 2038) | [`src/layer/transform.rs`](src/layer/transform.rs) | Full 4×4 matrix. Flutter inherits from `OffsetLayer`; FLUI flattens. |
| `class OpacityLayer extends OffsetLayer` (line 2138) | [`src/layer/opacity.rs`](src/layer/opacity.rs) | Alpha blending. Flutter inherits from `OffsetLayer`; FLUI flattens. |
| `class ShaderMaskLayer extends ContainerLayer` (line 2218) | [`src/layer/shader_mask.rs`](src/layer/shader_mask.rs) | GPU shader masking. |
| `class BackdropFilterLayer extends ContainerLayer` (line 2325) | [`src/layer/backdrop_filter.rs`](src/layer/backdrop_filter.rs) | Frosted-glass effect; captures backdrop, applies filter. |
| `class LeaderLayer extends ContainerLayer` (line 2486) + `class LayerLink` (separate file `link.dart`) | [`src/layer/leader.rs`](src/layer/leader.rs) | FLUI co-locates `LayerLink` and `LeaderLayer` in one file. |
| `class FollowerLayer extends ContainerLayer` (line 2603) | [`src/layer/follower.rs`](src/layer/follower.rs) | Position-following layer; resolves leader position via `LinkRegistry`. |
| `class AnnotatedRegionLayer<T extends Object> extends ContainerLayer` (line 2927) | [`src/layer/annotated_region.rs`](src/layer/annotated_region.rs) | Metadata regions for system UI integration. |
| `Layer.addCompositionCallback(VoidCallback)` method on `ContainerLayer` (line 1077+) | [`src/scene.rs`](src/scene.rs) `Scene::add_composition_callback` + `CompositionCallback` newtype | Folded into `Scene` in Mythos Step 2; see Mapping decisions #2. |
| no Flutter analog (FLUI invention) | `Layer::Canvas(CanvasLayer)` -- [`src/layer/canvas.rs`](src/layer/canvas.rs) | Mutable Canvas layer for pre-finish drawing. Flutter records `Picture` only after canvas finalisation; FLUI keeps the mutable form addressable until paint completes. |
| `PaintingContext.pushClipRect` / `pushClipRRect` / `pushClipPath` / `pushTransform` / `pushOpacity` (Dart `PaintingContext` outside `layer.dart`) | [`src/compositor/builder.rs`](src/compositor/builder.rs) `SceneBuilder::push_clip_rect` / `push_clip_rrect` / … | Stack-based scene-construction API. The duplicate `LayerTree::push_*` helpers (5 methods) were deleted in Mythos Step 5; SceneBuilder is the canonical path. |
| LayerTree implementation backing `RenderView.compositeFrame` | [`src/tree/layer_tree.rs`](src/tree/layer_tree.rs) | `Slab<LayerNode>` arena keyed by `LayerId` (NonZeroUsize, +1/-1 offset pattern). Implements `TreeRead<LayerId>` + `TreeNav<LayerId>` in [`src/tree/tree_traits.rs`](src/tree/tree_traits.rs). |

---

## Mapping decisions

This section records places where the Rust shape diverges from the Dart shape and why. Each entry follows the "Accepted trade-offs" format established by [`docs/plans/2026-03-31-custom-render-callback-design.md`](../../docs/plans/2026-03-31-custom-render-callback-design.md).

### 1. Closed `Layer` enum, not a `Box<dyn Layer>` plugin trait

**Rule:** [`docs/PORT.md`](../../docs/PORT.md) Mapping rule "Compile-time over runtime"; constitution Anti-Patterns ("Prefer generics and enum dispatch over `dyn` trait objects"); strategy clause "Behavior loyal, structure Rust-native".

**Choice:** `Layer` is a `#[non_exhaustive] enum` with 19 concrete variants ([`src/layer/mod.rs`](src/layer/mod.rs)). The closed enum is **the** trust boundary -- adding a 20th variant is a coordinated change in `flui-layer` + `flui-engine` (whose wgpu backend pattern-matches every variant to GPU draw calls). There is no third-party `impl Layer` extension point.

**Alternatives:**
- `Box<dyn Layer + Send + Sync>` mirroring Flutter's inheritance hierarchy -- rejected. The GPU backend cannot lower an arbitrary `dyn Layer` to wgpu draw calls; every variant needs a hand-written translation. Closed-enum gives compile-time match-exhaustiveness; trait-object loses that.
- Sealed-trait-with-private-impl-marker -- rejected. Same result as closed enum but with vtable dispatch on the hot path.

**Accepted trade-off:** Plugin authors cannot define their own layer types. The 19 variants must cover every compositor primitive forever (the rendering crate emits only these). When a new compositor primitive appears (e.g. WebGPU compute layers, future Skia features), it lands as a new variant in a coordinated change. Mythos verdict S12 rejected design #1.

### 2. `Vec<CompositionCallback>` on `Scene`, not `Arc<Mutex<Registry>>`

**Rule:** [`docs/PORT.md`](../../docs/PORT.md) Refusal trigger 4 (`Mutex` on dirty-list-shaped state in the render hot path); verdict S12 #4.

**Choice:** `Scene` owns a `Vec<CompositionCallback>` field where `CompositionCallback(Box<dyn FnOnce() + Send + 'static>)`. `Scene::add_composition_callback` pushes onto the vec. `Scene::fire_composition_callbacks(&mut self) -> Vec<LayerError>` drains the vec, wrapping each fire in `std::panic::catch_unwind` and accumulating poisoned-callback errors.

**Alternatives:**
- `CompositionCallbackRegistry { storage: Arc<Mutex<Vec<(Id, Box<dyn Fn() + Send + Sync>)>>> }` (Flutter parity via mutable registry shared across threads) -- rejected. Zero cross-thread consumers in the workspace; the lock and `Arc` were pure ceremony. The `Box<dyn Fn() + Send + Sync>` allocation per callback was paid even for callbacks that never fired.
- `HasCompositionCallbacks` trait on each container layer -- rejected. Zero impls in the workspace; the trait was dead.
- Per-container-layer callback lists (Flutter shape) -- rejected. FLUI's `Layer` is a closed enum, not a container hierarchy.

**Accepted trade-off:** Callbacks are scene-scoped, not layer-scoped. A future need for per-layer callbacks (none today) would require a different model; the current `FnOnce() + Send + 'static` shape carries owned state and one-shot semantics with no lock. Mythos verdict S12 rejected design #4.

### 3. Single-owner `LayerTree` + `Scene: Send`, not `Arc<RwLock<LayerTree>>`

**Rule:** [`docs/PORT.md`](../../docs/PORT.md) Refusal trigger 1 (`RwLock` field on a type used inside the render hot path); strategy clause "sync hot path, async at edges".

**Choice:** `LayerTree` owns its `Slab<LayerNode>` directly; `Scene` owns the `LayerTree` directly. Cross-thread movement is by value-move (`Scene: Send`). No `Arc<RwLock<>>` anywhere in production code.

The doc-comment on `LayerTree` previously recommended `Arc<RwLock<LayerTree>>` for "multi-threaded access" -- corrected in Mythos Step 9 (commit chain). The corrected advice is "construct on one thread, move `Scene` by value to the render thread."

**Alternatives:**
- `Arc<RwLock<LayerTree>>` (Flutter-style shared mutability via `addToScene` Dart microtasks) -- rejected. Lock contention on the render-thread → painter-thread boundary; the lock guards an arena with no actual cross-thread mutation. Background workers build their own subtrees and emit them as values.

**Accepted trade-off:** Single mutable owner during construction (the `SceneBuilder` holds `&mut LayerTree`). Multi-output rendering (e.g. mirror display) requires multiple `Scene` instances, not a shared scene. Mythos verdict S12 rejected design #2.

### 4. `LayerHandle<T>` deletion, not retained for future GPU lifecycle

**Rule:** Mythos verdict S12 rejected design #3; strategy clause "Every dyn, every Arc, every RwLock must defend its existence in writing".

**Choice:** `crates/flui-layer/src/handle.rs` (467 LOC) was deleted entirely in Mythos Step 1 (commit `702e8751`). The 17 type aliases (`CanvasLayerHandle`, `OpacityLayerHandle`, …, `AnyLayerHandle`) are gone. The lib.rs re-exports and prelude entries are removed.

**Alternatives:**
- Retain `LayerHandle<T>` as fake-ownership ceremony "for future GPU lifecycle hooks" -- rejected. Zero external callers in the workspace at the time of deletion (verified by `grep -rn "LayerHandle\|AnyLayerHandle" crates/`). The `Arc<AtomicUsize>` ref-count was bumped and decremented but no consumer ever read it to gate GPU resource release. The handle owned `Option<T>` directly, not a reference into `LayerTree` -- the architecture diagram in the file lied about the implementation.

**Accepted trade-off:** If a future Mythos chain needs per-layer GPU resource lifecycle management, the right shape is in `flui-engine`'s resource registry (which has direct access to wgpu textures and buffers), not a wrapper in `flui-layer`. Rebuilding from scratch will be ~50 LOC; the deleted 467 LOC was hostile to that rebuild. Mythos verdict S12 rejected design #3.

### 5. Macro-collapsed `is_*`/`as_*`/`as_*_mut` dispatch via local `macro_rules!`

**Rule:** Verdict S12 #5; strategy clause "Compile-time over runtime".

**Choice:** A single `gen_layer_accessors!` `macro_rules!` macro in [`src/layer/dispatch.rs`](src/layer/dispatch.rs) emits all 57 methods (19 variants × `is_<name>` + `as_<name>` + `as_<name>_mut`) for the `Layer` enum. The composite predicates (`is_clip`, `is_linking`) and the semantic methods (`bounds`, `needs_compositing`, `is_opaque`) stay hand-written in `layer/mod.rs` because they pattern-match across multiple variants.

**Alternatives:**
- `enum_dispatch` crate -- rejected. Adds a new proc-macro dep for a small win; output identical.
- Hand-written boilerplate (the pre-Mythos shape) -- rejected. 600 LOC of pattern-match noise.

**Accepted trade-off:** The macro form is a single-source-of-truth for accessor shapes; adding a new variant requires updating two places (the enum + the macro invocation) instead of three (enum + 3 hand-written methods × 1 = three places). Mythos verdict S12 rejected design #5.

### Net unsafe delta: -39

Mythos Step 1 + Step 3 deleted 39 `unsafe impl Send + Sync` blocks across `handle.rs` (2), `scene.rs` (1), and 17 layer files (34 total -- 17 files × 2 blocks each). Every layer's fields are auto-Send/Sync; the unsafe blocks were cargo-cult from Dart's threading model. The `BackdropFilterLayer` even carried the comment "contains only owned, Send types" directly above its `unsafe impl Send` -- proof the unsafe was unnecessary.

Zero new `unsafe` blocks were introduced by the chain.

---

## Thread safety

`flui-layer` runs in the paint phase (scene construction) and the render phase (engine consumption); per strategy clause "sync hot path", neither is multi-threaded within a single scene. Cross-thread movement is by value (`Scene: Send`).

| Site | Primitive | Category | Notes |
|---|---|---|---|
| `LayerTree::nodes` ([`src/tree/layer_tree.rs`](src/tree/layer_tree.rs)) | `Slab<LayerNode>` | Owned, single-mutator | Auto-derived `Send + Sync`. No lock. |
| `Scene` ([`src/scene.rs`](src/scene.rs)) | Owned by-value | `Send`-only (auto-derived) | Moved across threads by value-move. Not `Sync` -- no `Arc<Scene>` consumer exists; adding `Sync` would invite ceremony with no current need. |
| `Scene::composition_callbacks` | `Vec<CompositionCallback>` where `CompositionCallback(Box<dyn FnOnce() + Send + 'static>)` | Owned, single-mutator | Scene-scoped; fired once in `fire_composition_callbacks`. `FnOnce` matches one-shot semantics. No lock. |
| `LinkRegistry::leaders` / `LinkRegistry::followers` ([`src/link_registry.rs`](src/link_registry.rs)) | `HashMap<LayerLink, LeaderInfo>` + `HashMap<LayerId, LayerLink>` | Owned, setup-phase | Touched during scene build, occasionally during render (follower position lookup). Off the per-layer walk. Auto-derived `Send + Sync` for the HashMap pair. |
| `DamageTracker::regions` ([`src/damage.rs`](src/damage.rs)) | `Vec<Rect<Pixels>>` | Owned, frame-scoped | No lock. |
| `SceneBuilder::stack` ([`src/compositor/builder.rs`](src/compositor/builder.rs)) | `Vec<LayerId>` | Owned by `SceneBuilder<'a>` | Single-mutator during build; the `&mut LayerTree` borrow enforces single-writer-per-build at compile time. |
| `SceneCompositor::retained` ([`src/compositor/retained.rs`](src/compositor/retained.rs)) | `Vec<LayerId>` | Owned, setup-phase | Today no production consumer; awaiting Flutter retained-layer optimisation in `flui-rendering`. |

No `unsafe impl Send/Sync` anywhere in the crate after Mythos Step 3. No `Arc<>` / `Mutex<>` / `RwLock<>` in production code after Step 2. No interior-mutability primitive on any layer type's storage.

---

## Friction log

Known sites that do not yet match the methodology but are not violations of the current refusal triggers. Each entry names the site and the next planned step.

### Doctest examples use pre-Pixels-wrap `Offset::new(f32, f32)` shape

**Sites:** [`src/compositor/builder.rs`](src/compositor/builder.rs) lines 14-37 (module doc), 77-93 (`SceneBuilder` doc), 184-194 (`push_offset` doc), 219-236 (`push_opacity` doc), 521 (`pop` doc), and ~15 similar examples in layer-type docstrings (`layer/clip_rect.rs:25`, `layer/clip_rrect.rs:28`, etc.).

**Violation:** none of the six refusal triggers; pre-existing doctest breakage from a `flui_types::Offset::new` signature change (now requires `Pixels` wrapping). `cargo test -p flui-layer --doc` reports 21 failures across these examples.

**Next planned step:** mechanical sweep wrapping `100.0` → `px(100.0)` (or `Pixels(100.0)`) in every affected doctest. Out of scope for the Mythos chain (was tracked as a deferred concern in Step 7's commit message). A standalone `chore(flui-layer): fix doctest pixels wrapping` PR resolves it; estimated 1-2 hours.

### `Layer` enum still carries 19 `impl From<XxxLayer> for Layer` blocks (~95 LOC)

**Site:** [`src/layer/mod.rs`](src/layer/mod.rs) lines ~360-490 (post-Mythos Step 6 layout).

**Violation:** none. The 19 `From` impls are mechanical (each is `fn from(layer: XxxLayer) -> Self { Layer::Xxx(layer) }`) and similar in shape to the `is_*`/`as_*` dispatch that Mythos Step 4 collapsed via macro.

**Next planned step:** add a `gen_layer_from_impls!` macro to `layer/dispatch.rs` and replace the hand-written `From` impls with a single 19-line invocation. Estimated -75 LOC; trivial. Tracked in Outstanding refactors below.

### `link_registry.rs` carries ~290 LOC of inline tests

**Site:** [`src/link_registry.rs`](src/link_registry.rs) lines ~332-621.

**Violation:** none. The tests are clean and focused. The Mythos chain extracted `layer_tree.rs` and `compositor.rs` tests to integration tests but left `link_registry.rs` inline (the file's other concerns are smaller and the test extraction would be cosmetic).

**Next planned step:** extract to `tests/link_registry.rs` if/when the file is touched for another reason. Not blocking.

### CLAUDE.md still lists `flui-layer` as disabled / mid-integration

**Site:** [`CLAUDE.md`](../../CLAUDE.md) "Current Development Focus" section.

**Violation:** none. CLAUDE.md lists `flui-rendering`, `flui-view`, `flui-app`, `flui-hot-reload` as "Temporarily disabled" while `AGENTS.md` and [`docs/crates.md`](../../docs/crates.md) correctly mark them active. The `flui-rendering` chain (PR #77) also called out this drift as deferred to a separate housekeeping PR.

**Next planned step:** sync CLAUDE.md with `AGENTS.md`/`docs/crates.md` in a follow-up housekeeping PR. Not blocking.

---

## Outstanding refactors

Concrete cleanups visible from `flui-layer` outward, sized for an `/aif-implement` dispatch. Each entry names a file and what would need to change.

### `gen_layer_from_impls!` macro for the 19 `impl From<XxxLayer> for Layer` blocks

**File:** [`src/layer/mod.rs`](src/layer/mod.rs); add to [`src/layer/dispatch.rs`](src/layer/dispatch.rs).

**Goal:** apply the same macro pattern Mythos Step 4 used for `is_*`/`as_*` dispatch to the 19 `From` impls. The new macro accepts `(Variant => Type)` pairs and emits one `impl From<Type> for Layer` per pair. Saves ~75 LOC of mechanical boilerplate.

**Shape:** add `gen_layer_from_impls!` next to `gen_layer_accessors!`; invoke it from `mod.rs` after the existing accessor invocation. External callers of `Layer::from(canvas_layer)` etc. compile unchanged.

**Dependencies:** none. Mechanical extension of the dispatch.rs macro file.

### `SmallVec<[LayerId; 4]>` for `LayerNode::children`

**File:** [`src/tree/layer_tree.rs`](src/tree/layer_tree.rs); add `smallvec` dev-dep first, then promote to runtime dep.

**Goal:** typical layer-tree fan-out is 1-8 children per node (the `TreeNav::AVG_CHILDREN = 4` constant codifies this). Inlining the first 4 child IDs in a `SmallVec` saves a heap allocation per leaf node and improves cache locality (`LayerNode` body + first 4 children on the same cache line).

**Shape:** change `LayerNode::children: Vec<LayerId>` to `SmallVec<[LayerId; 4]>`; update `add_child` / `remove_child` / `clear_children` / `children() -> &[LayerId]` API. Iterator types in the `TreeNav` impl may need adjustment.

**Dependencies:** `smallvec = "1"` workspace dep decision. Measured-benefit verification via Criterion benchmark (post-`flui-rendering` Mythos Step 14 benchmark infrastructure).

### Property tests for `LayerTree` consistency

**Files:** new `crates/flui-layer/tests/proptest_layer_tree.rs`; add `proptest` dev-dep.

**Goal:** invariants that hold over any sequence of `(insert, add_child, remove)` operations -- every reachable `LayerId` has a parent (except root), every parent's `children` contains its actual children, no cycles. Mirrors the rendering crate's deferred property test class.

**Shape:** standard `proptest!` macro with a state-machine strategy that maintains an oracle `BTreeMap<LayerId, (Option<parent>, Vec<children>)>` alongside the real tree.

**Dependencies:** `proptest` dev-dep decision (mirrors the rendering crate's open Outstanding refactor).

### Miri gate for the disjoint-borrow patterns (if added)

**File:** new CI config; new `crates/flui-layer/tests/miri_disjoint.rs` if a primitive is added.

**Goal:** today no `unsafe` block lives in `crates/flui-layer/src/` after Mythos Step 3 (-39 net delta). If a future step adds a disjoint-borrow primitive on `LayerTree` (e.g. `get_two_mut(parent, child)` for parent-and-child concurrent mutation during a future compositor optimisation), the primitive's safety invariant gets a miri-checked unit test.

**Shape:** invoke `cargo +nightly miri test -p flui-layer` from CI when the unsafe block lands. No work needed today.

**Dependencies:** no unsafe blocks currently exist; the gate is forward-looking.

### Fix the pre-existing doctest breakage

**Files:** ~20 doc examples across `src/compositor/builder.rs`, `src/scene.rs`, `src/layer/*.rs`.

**Goal:** every doctest currently uses `Offset::new(100.0, 50.0)` which fails to compile because `Offset<Pixels>::new` requires `Pixels`-wrapped arguments. The breakage predates the Mythos chain; tracked in Friction log above.

**Shape:** mechanical sweep of `Offset::new(<float>, <float>)` → `Offset::new(px(<float>), px(<float>))` plus an explicit `use flui_types::geometry::px;` in each affected doc example. Estimated 1-2 hours of mechanical edits.

**Dependencies:** none. `flui_types::geometry::px` is already available.

### Per-variant GPU lowering documentation in `flui-engine`

**File:** in `crates/flui-engine/docs/` -- a `LAYER_LOWERING.md` that documents the wgpu translation for every `Layer` variant.

**Goal:** the verdict S2 architecture diagram says "every variant in this enum has a documented GPU lowering." Today the documentation is implicit in the wgpu backend's match arms. Making it explicit prevents drift when a new variant is added.

**Shape:** one section per variant; each describes the GPU pipeline state, shader entry point, draw call shape, and any platform-specific path (texture / hybrid / virtual display for `PlatformViewLayer`).

**Dependencies:** lives in the engine crate, not this one. Recorded here because the contract is bidirectional.

### Apply Mythos to `flui-engine` and `flui-app` next

**Files:** TBD by future brainstorms.

**Goal:** continue the chain through the remaining active crates. `flui-engine` consumes `Scene` from this crate; `flui-app` owns the frame loop. Both have their own god-module candidates and refusal-trigger violations that the lens will surface.

**Shape:** one brainstorm + verdict + plan + chain per crate, following the precedent of `flui-rendering` (PR #77) and `flui-layer` (this chain).

**Dependencies:** standalone planning effort. Not blocking any work in this crate.

---

## Notes

- **Net unsafe delta for this chain: -39.** Every `unsafe impl Send + Sync` block in `flui-layer` was unjustified and is gone. Zero new `unsafe` blocks were added.
- **Net LOC reduction for this chain: ~3,000 LOC across the touched .rs files.** Three god modules split, two dead files deleted, one duplicate API removed, 57 boilerplate methods collapsed to one macro invocation.
- **`port-check.sh` extended in Mythos Step 13 (U13)** to cover `crates/flui-layer/src/` paths in Triggers 1, 2, 3 and to flag `Arc::clone` in `crates/flui-engine/src/wgpu/layer_render.rs` (Trigger 5, forward-looking).
- **Doctest breakage tracked but not fixed by this chain.** See Friction log + Outstanding refactors. 21 doctests fail with the pre-Pixels-wrap signature; the lib + integration test surfaces are fully green (229 lib + 45 integration tests pass after U10).
