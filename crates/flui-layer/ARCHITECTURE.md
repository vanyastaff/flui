# flui-layer Architecture

The per-crate record for `flui-layer`: the
Flutter → Rust mapping, every divergence from `layer.dart` with the reason and the test that
replaces the Flutter one, the thread-safety surface, and what is deliberately not here.

The behavioural reference is `.flutter/packages/flutter/lib/src/rendering/layer.dart` at the
pinned tag (`git -C .flutter describe --tags` must print `3.44.0`). Citations below name symbols,
not line numbers.

---

## Flutter source mapping

| Flutter (`layer.dart`) | FLUI | Notes |
|---|---|---|
| `abstract class Layer` + `ContainerLayer` | [`src/layer/mod.rs`](src/layer/mod.rs) `enum Layer` | Closed enum, 19 variants; no `Box<dyn Layer>` (decision 1). |
| `PictureLayer` | [`src/layer/picture.rs`](src/layer/picture.rs) | `Arc<DisplayList>`; the leaf the paint walk emits. |
| `TextureLayer`, `PlatformViewLayer`, `PerformanceOverlayLayer` | `texture.rs`, `platform_view.rs`, `performance_overlay.rs` | Present as vocabulary; see [Producers](#producers). |
| `OffsetLayer` (base of `ImageFilterLayer`, `TransformLayer`, `OpacityLayer`) | `offset.rs`, and an `offset` field on `image_filter.rs`, `opacity.rs`, `leader.rs` | The inheritance is flattened; the one virtual it carried (`applyTransform`) is `Layer::local_translation` (decision 3). |
| `ClipRectLayer`, `ClipRRectLayer`, `ClipRSuperellipseLayer`, `ClipPathLayer` | `clip_rect.rs`, `clip_rrect.rs`, `clip_superellipse.rs`, `clip_path.rs` | `Clip::None` is accepted by all four and lowered as "no clip" by the engine. |
| `ColorFilterLayer`, `ImageFilterLayer`, `ShaderMaskLayer`, `BackdropFilterLayer` | `color_filter.rs`, `image_filter.rs`, `shader_mask.rs`, `backdrop_filter.rs` | |
| `LayerLink` (`link.dart`), `LeaderLayer`, `FollowerLayer`, `FollowerLayer._establishTransform` | [`src/link.rs`](src/link.rs) `LayerLink` + `resolve_follower_offset`; `leader.rs`; `follower.rs` | The leader index lives on the tree, not on the link (decision 2); the anchor math lives on the follower layer (decision 4). |
| `AnnotatedRegionLayer<T>` | `annotated_region.rs` | Writer half only; see [Producers](#producers). |
| `ui.SceneBuilder` push/pop | [`src/compositor/builder.rs`](src/compositor/builder.rs) `SceneBuilder` | Hand-authored scenes only; the production frame is built through the tree (decision 5). |
| `ui.Scene` | [`src/scene.rs`](src/scene.rs) `Scene` | A frozen `LayerTree` (decision 6). |
| retained rendering: `_needsAddToScene`, `addRetained`, `EngineLayer`, `Layer.dispose` | none | The engine rebuilds every frame from the tree; cross-frame reuse is keyed on `LayerNode::render_id` (ADR-0061) and `Arc` identity of `PictureLayer` content, consumed by `flui-rendering`'s `FragmentComposer::capture`/`graft`. |
| `Layer.addCompositionCallback` | none | Fires after `addToScene`, a step this architecture does not have. |
| no analog | `Layer::Canvas(CanvasLayer)` | A live recorder inside the tree; see [Producers](#producers). |

---

## Mapping decisions

Each decision names what is better than the reference and why, and the FLUI test that replaces
the Flutter coverage it displaces.

### 1. Closed `Layer` enum, not a `Layer` class hierarchy

`Layer` is a closed enum and deliberately **not** `#[non_exhaustive]`: compile-time exhaustiveness
is the contract with `flui-engine`, whose wgpu backend matches every variant. A 20th variant breaks
that match and lands its GPU lowering in the same change. There is no third-party `impl Layer`
extension point, and plugin authors cannot define layer types — the accepted trade-off.

Payloads are inline unless the variant would dominate the enum (`Canvas` carries a live recorder,
~184 B); `Picture` (`Arc` + `Rect`), `ClipPath` (`Arc<Path>`) and `PerformanceOverlay` are small
and unboxed, so a sealed picture run costs no heap allocation beyond its `Arc`. `layer/mod.rs`'s
`layer_fits_the_inline_budget` pins the 128 B budget; re-measure before boxing anything.

### 2. Append-only `LayerTree` with a leader index, not a mutable tree plus a side registry

Flutter's tree is a retained, mutable object graph (`append`, `remove`, `dispose`) because it is
reused across frames. FLUI builds a fresh tree per frame, so the tree needs exactly one structural
primitive: `LayerTree::push_child(parent, node)`, which mints the child's id in the call that links
it. A fresh id has no descendants, so a cycle or a doubly-parented node cannot be expressed; there
is no `remove`, no re-parenting, no node-level link setter, no `&mut` reach into a node, and no
generic tree-write trait. Consequences:
`flui-engine`'s walk has no cycle guard, inserts are O(1) with no ancestor check, and ids never
alias (nothing is ever freed). A parent is always pushed before its child, so its id is smaller;
`LayerTree::lowest_common_ancestor` steps the larger id up until the two meet, in O(depth) with no
allocation. `LayerId` stays a plain 1-based index; a generational id is only
needed if a holder ever outlives the frame that minted it (none does today).

Flutter keeps "which leader has this link" on `LayerLink.leader`. FLUI's link is a `Copy` token,
so the tree holds `leaders: HashMap<LayerLink, LayerId>`, filled in `push_child` when the layer is
a `Leader`. The earlier shape — a `LinkRegistry` beside the tree that the composer filled and the
realm had to commit "as one pair" with the tree — duplicated the leader's offset and size off the
`LeaderLayer` and kept a reverse follower index nothing read. Two leaders on one link in one frame
is a widget-tree error (Flutter asserts); a debug build trips, a release build keeps the later one.

Replacement coverage: `tree/layer_tree.rs` tests (`push_child_links_both_sides_in_paint_order`,
`leaders_are_indexed_at_insertion`, `push_child_under_an_unknown_parent_is_a_bug`),
`flui-engine/src/layer_walk.rs` (`a_deep_chain_survives_a_small_stack`),
`flui-app`'s `semantics_failure_retry_submits_the_retained_linked_tree`.

### 3. One `Layer::local_translation`, read by the walk and the resolver

Flutter's chain math (`_collectTransformForLayerChain`) calls one virtual, `applyTransform`, on
every container. Flattening `OffsetLayer` into per-variant `offset` fields had lost that single
dispatch point: the engine pushed translations for `Offset`, `Transform`, `Opacity`, `ImageFilter`
and `Leader`, while the resolver summed only `Offset` and `Transform` — so a follower nested under
its own leader rendered double-translated. `Layer::local_translation` is now the one place the set
of translating variants is written down; `resolve_follower_offset` sums it along both chains
inclusive of the common ancestor (which cancels), exactly as `_establishTransform` does, and
`flui-engine`'s `every_variant_pushes_exactly_its_local_translation` pins that the walk's pushes
equal it. Known limitations, named rather than silent: a `Transform` contributes only its
translation (the follower system is offset-only), and a follower on another follower's chain
contributes zero.

Replacement coverage: `link.rs` tests, including `follower_nested_under_its_leader_resolves_to_zero`
(Flutter's `_establishTransform` as oracle) and `linked_through_an_opacity_offset_counts_it`.

### 4. Anchors and size on `FollowerLayer`, resolution at composite time

Flutter keeps `leaderAnchor`/`followerAnchor` on `RenderFollowerLayer` and stores only
`linkedOffset` on the layer. FLUI moves both anchors and the follower's own `size` onto the layer so
`FollowerLayer::calculate_offset` (`Alignment::along_size` on both rectangles, plus
`target_offset`) runs from the layer tree alone — the same value the GPU walk and the follower
hit-test side table both read. Anchors outside `[-1, 1]` are legal off-rectangle pivots.

### 5. `SceneBuilder` is secondary; the composer builds the frame through the tree

`flui-rendering`'s `FragmentComposer` inserts straight through `insert_root`/`push_child`, and
`SceneBuilder` is the push/pop shape for hand-authored scenes (`flui_app::run_direct`, examples,
the GPU readback tests). `pop` returns `Option<LayerId>` — an empty stack is absence, the
`Vec::pop` contract — so the crate has no error type. The builder's typed `push_*`/`add_*` are
one-line conveniences over `push(impl Into<Layer>)` / `add(..)`; a variant without one is reached
through `push(SomeLayer::new(..))`.

### 6. `Scene` is a frozen tree

`Scene::new(tree)` adds one fact over the tree: no `&mut` path back to it. Its former `size`,
`root` and `frame_number` fields had no reader that did not already hold the value (`root` was a
copy of `LayerTree::root` that eleven of thirteen callers left disagreeing; `frame_number` is the
presentation's counter, read back by its writer; `size` had no reader). `flui-hot-reload` moves a
`Scene` across its dlopen boundary and hashes `size_of::<Scene>` into its ABI token, which pins
that the type is nameable without an engine dependency — not its field set.

### 7. Runtime state lives with its owner

`DamageTracker` (the renderer's per-frame dirty accumulator, ADR-0061's consuming half) moved to
`flui-engine/src/damage.rs`; only the seam message `DamageRegion` is defined here. Its
three-rect merge was removed with the move: the one consumer reads `damage_rect()`, the bounding
union, through which the merge was unobservable. `PerformanceStats` (a clock-bearing frame-time
window) moved to `flui-app`; `PerformanceOverlayLayer::update_stats(fps, frame_time_ms,
total_frames)` takes the numbers, and `PerformanceOverlayOption` crosses the engine boundary as
itself rather than as a `u32`.

### Deleted, with the reason

`LayerHandle<T>` (Flutter's `EngineLayer` lifecycle has no counterpart here); `LayerBounds` trait
(one impl per type, no generic consumer); annotation search (no reader after the writer lost its
producer); composition callbacks (see the mapping table); the `needs_add_to_scene` dirty-bit
protocol (feeds `addRetained`, which this architecture never calls); `LayerNode::offset` (a second
way to say "translate" that no production tree ever set); the 57 generated `is_*/as_*/as_*_mut`
accessors bar the four with callers (`as_leader`, `as_follower`, `as_performance_overlay`); the
`f32` sugar constructors (`OffsetLayer::from_xy`, `FollowerLayer::below/above/..`,
`Clip*Layer::circular/circle/oval`) that bypassed the `Pixels` unit barrier; the `testing`
feature's `LayerSpec`/`LayerTester` DSL (a third builder with no consumer); `prelude` and
`VERSION` (no importer).

---

## Producers

Five variants have no production producer today: `Canvas`, `Texture`, `PlatformView`,
`ClipSuperellipse`, `AnnotatedRegion` (the composer emits the other fourteen; `flui-app` adds
`PerformanceOverlay`). `Texture` and `PlatformView` carry real Flutter contracts (`freeze`,
`hit_test_behavior`) awaiting the platform layer; `AnnotatedRegion`'s reader half was deleted
with annotation search and a producer would bring it back as one change; `Canvas` is a recorder
inside the output vocabulary that `PictureLayer` already covers. Whether each is "not wired yet" or
"never" is a roadmap decision, not a crate one — recorded here so it is not re-derived.

---

## Thread safety

| Site | Primitive | Category |
|---|---|---|
| `LayerTree::nodes` | `Vec<LayerNode>` | Owned, single mutator; no lock, no atomics |
| `LayerTree::leaders` | `HashMap<LayerLink, LayerId>` | Owned, filled during the build |
| `Scene`, `SceneSnapshot` | by value | Moved across the raster boundary; `Send + Sync` plain data, pinned by `static_assertions` in `scene_snapshot.rs` |
| `LayerLink::new` | `static AtomicU64`, `Relaxed` | Identity only; the RMW is atomic under every ordering and publishes nothing else |

No `unsafe`, no `Arc<Mutex<_>>`, no interior mutability in the crate. The one `dyn` is
`AnnotationValue = Arc<dyn Any + Send + Sync>`.
