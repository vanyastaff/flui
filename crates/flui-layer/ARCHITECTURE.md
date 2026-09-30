# flui-layer Architecture

The per-crate record for `flui-layer`: the module map, each design decision with its reason and
the tests that pin it, the thread-safety surface, and what is deliberately not here.

Citations below name symbols, not line numbers.

---

## Module map

| Layer | FLUI | Notes |
|---|---|---|
| `Layer` and its container behaviour | [`src/layer/mod.rs`](src/layer/mod.rs) `enum Layer` | Closed enum, 19 variants; no `Box<dyn Layer>` (decision 1). |
| `PictureLayer` | [`src/layer/picture.rs`](src/layer/picture.rs) | `Arc<DisplayList>`; the leaf the paint walk emits. |
| `TextureLayer`, `PlatformViewLayer`, `PerformanceOverlayLayer` | `texture.rs`, `platform_view.rs`, `performance_overlay.rs` | Present as vocabulary; see [Producers](#producers). |
| `OffsetLayer` (and the `offset` field on `ImageFilterLayer`, `TransformLayer`, `OpacityLayer`) | `offset.rs`, and an `offset` field on `image_filter.rs`, `opacity.rs`, `leader.rs` | Translation is a per-variant field; the shared query is `Layer::local_translation` (decision 3). |
| `ClipRectLayer`, `ClipRRectLayer`, `ClipRSuperellipseLayer`, `ClipPathLayer` | `clip_rect.rs`, `clip_rrect.rs`, `clip_superellipse.rs`, `clip_path.rs` | `Clip::None` is accepted by all four and lowered as "no clip" by the engine. |
| `ColorFilterLayer`, `ImageFilterLayer`, `ShaderMaskLayer`, `BackdropFilterLayer` | `color_filter.rs`, `image_filter.rs`, `shader_mask.rs`, `backdrop_filter.rs` | |
| `LayerLink`, `LeaderLayer`, `FollowerLayer` | [`src/link.rs`](src/link.rs) `LayerLink` + `resolve_follower_offset`; `leader.rs`; `follower.rs` | The leader index lives on the tree, not on the link (decision 2); the anchor math lives on the follower layer (decision 4). |
| `AnnotatedRegionLayer<T>` | `annotated_region.rs` | Writer half only; see [Producers](#producers). |
| `SceneBuilder` push/pop | [`src/compositor/builder.rs`](src/compositor/builder.rs) `SceneBuilder` | Hand-authored scenes only; the production frame is built through the tree (decision 5). |
| `Scene` | [`src/scene.rs`](src/scene.rs) `Scene` | A frozen `LayerTree` (decision 6). |
| Retained layers across frames | none | The engine rebuilds every frame from the tree. Cross-frame reuse of painted output is `flui-rendering`'s (`FragmentComposer::capture`/`graft`, keyed on the boundary's `RenderId`); cross-frame damage is `LayerDiffer`'s comparison of boundary stamps (`RenderId` plus `ContentToken`, decision 8), not a retained engine layer. |
| Composition callbacks | none | They would fire after a scene-add step this architecture does not have. |
| `Layer::Canvas(CanvasLayer)` | `Layer::Canvas(CanvasLayer)` | A live recorder inside the tree; see [Producers](#producers). |

---

## Mapping decisions

Each decision names the shape chosen and why, and the tests that pin it.

### 1. Closed `Layer` enum, not a `Layer` class hierarchy

`Layer` is a closed enum and deliberately **not** `#[non_exhaustive]`: compile-time exhaustiveness
is the contract with `flui-engine`, whose wgpu backend matches every variant. A 20th variant breaks
that match and lands its GPU lowering in the same change. There is no third-party `impl Layer`
extension point, and plugin authors cannot define layer types — the accepted trade-off.

Payloads are inline unless the variant would dominate the enum (`Canvas` carries a live recorder,
~184 B); `Picture` (`Arc` + `Rect`), `ClipPath` (`Arc<Path>`) and `PerformanceOverlay` are small
and unboxed, so a sealed picture run costs no heap allocation beyond its `Arc`. The budget is
128 B; re-measure before boxing anything. **Unasserted:** no test pins this.

### 2. Append-only `LayerTree` with a leader index, not a mutable tree plus a side registry

The tree is not retained across frames: a fresh one is built per frame, so it needs exactly one structural
primitive: `LayerTree::push_child(parent, node)`, which mints the child's id in the call that links
it. A fresh id has no descendants, so a cycle or a doubly-parented node cannot be expressed; there
is no `remove`, no re-parenting, no node-level link setter, no `&mut` reach into a node, and no
generic tree-write trait. Consequences:
`flui-engine`'s walk has no cycle guard, inserts are O(1) with no ancestor check, and ids never
alias (nothing is ever freed). A parent is always pushed before its child, so its id is smaller;
`LayerTree::lowest_common_ancestor` steps the larger id up until the two meet, in O(depth) with no
allocation. `LayerId` stays a plain 1-based index; a generational id is only
needed if a holder ever outlives the frame that minted it (none does today).

`LayerLink` is a `Copy` token, so the tree holds `leaders: HashMap<LayerLink, LayerId>`, filled in `push_child` when the layer is
a `Leader`. The earlier shape — a `LinkRegistry` beside the tree that the composer filled and the
realm had to commit "as one pair" with the tree — duplicated the leader's offset and size off the
`LeaderLayer` and kept a reverse follower index nothing read. Two leaders on one link in one frame
is a widget-tree error; a debug build trips, a release build keeps the later one.

Tests: `tree/layer_tree.rs` tests (`push_child_links_both_sides_in_paint_order`),
`link.rs` (`linked_across_offset_branches_sums_both_chains`, which finds a leader pushed under a
branch through the index), `flui-engine/src/layer_walk.rs` (`a_deep_chain_survives_a_small_stack`).
That `push_child` under an unknown parent panics, and that a semantics-failure retry submits the
retained tree with its leader index intact: **Unasserted:** no test pins this.

### 3. One `Layer::local_translation`, read by the walk and the resolver

The chain math needs one dispatch point for "this layer translates its children". Flattening `OffsetLayer` into per-variant `offset` fields had lost it: the engine pushed translations for `Offset`, `Transform`, `Opacity`, `ImageFilter`
and `Leader`, while the resolver summed only `Offset` and `Transform` — so a follower nested under
its own leader rendered double-translated. `Layer::local_translation` is now the one place the set
of translating variants is written down; `resolve_follower_offset` sums it along both chains
inclusive of the common ancestor (which cancels). Known limitations, named rather than silent: a `Transform` contributes only its
translation (the follower system is offset-only), and a follower on another follower's chain
contributes zero.

A follower nested under its own leader resolving to zero, and an `Opacity` offset on the
follower's chain being counted: **Unasserted:** no test pins this.

### 4. Anchors and size on `FollowerLayer`, resolution at composite time

Both anchors and the follower's own `size` live on the layer so
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
`flui-engine/src/damage.rs`; the seam message `DamageRegion` and its producer, `LayerDiffer`
(decision 8), are defined here. Its
three-rect merge was removed with the move: the one consumer reads `damage_rect()`, the bounding
union, through which the merge was unobservable. `PerformanceStats` (a clock-bearing frame-time
window) moved to `flui-app`; `PerformanceOverlayLayer::update_stats(fps, frame_time_ms,
total_frames)` takes the numbers, and `PerformanceOverlayOption` crosses the engine boundary as
itself rather than as a `u32`.

### 8. Damage is a diff of boundary stamps — [ADR-0087 §3](../../docs/adr/ADR-0087-raster-contract-and-cpu-backend.md)

A repaint boundary's layer carries a `BoundaryStamp`: its `RenderId`, which pairs the layer with
the previous frame's (every frame's `LayerId`s are fresh), and a `ContentToken`, which the paint
pass keeps while the boundary is clean and mints otherwise (`flui-rendering`'s
`ARCHITECTURE.md`, "Paint certifies a boundary's content token"). `LayerDiffer::diff` walks the
frozen tree once with an explicit stack, recording per stamp its token, its placement (the
accumulated transform and the effect layers above it, compared with `Layer::same_effect`) and its
own region in surface pixels (its subtree minus nested boundaries' subtrees, from each picture's
`DisplayList::damage_extent` and each shader mask's whole bounds, mapped with its isotropic shadow spread scaled by the transform's
largest stretch, and clipped; content under an image filter or a perspective transform takes the
clip). A changed token or placement damages the old and new regions, as does a kept boundary whose
paint order changed relative to the other kept ones (those outside a longest increasing run of
their previous order), an added or removed boundary its one region; textures (a texture layer, or a picture's texture draws through
`DisplayList::volatile_extent`), platform views, canvases, overlays and anything under a follower
are damaged every frame; a shader mask or backdrop filter's bounds are not clipped by the clips
above it (the engine composites the offscreen unscissored), and an opacity layer whose blend
changes pixels under a transparent source, or a colour filter that paints transparent black,
damages the whole surface; damage that meets a foreground image filter's footprint (its
children's extents grown by the filter's reach: the larger of its local reach scaled by the
transform and the renderer's physical-pixel kernel growth, `ceil(sqrt(3) x sigma)` for a blur)
joins that footprint, since the engine re-renders
the filter from children recorded under the damage scissor; a backdrop filter whose blur-widened bounds meet the damage
joins it until nothing more does. The result is `Full` for an unpairable frame (first frame,
surface size change, unstamped root, root boundary or placement change, a boundary stamped
twice) or above `DamageMode::On::full_above`, `Unchanged` for nothing, and otherwise one
`DamageRect` in whole pixels, rounded outward with a 1 px anti-aliasing margin. `DamageMode::Off`
retains nothing and does not walk.

Why a token rather than `Arc::ptr_eq` on pictures: an outer boundary re-records its inline
pictures whenever a nested boundary is dirty, so pointer identity reports it changed on frames
whose content did not. Why an `Arc<()>` rather than a counter: a counter restarts with each
`PipelineOwner`, and the differ, holding the previous frame's clones, makes an allocation
address impossible to reuse while compared. The differ pairs by the boundary `RenderId` and decides "unchanged" by the token, and
the granularity is one boundary's own region, not a layer. `damage_diff` bench: about 200 µs for
1,000 boundaries, 4 ns with the mode off. Tests: `src/damage/tests.rs`,
`scene_snapshot.rs`'s `union_table` and `bounds_round_outward_with_aa_margin`.

### Deleted, with the reason

`LayerHandle<T>` (no engine-layer lifecycle exists here); `LayerBounds` trait
(one impl per type, no generic consumer); annotation search (no reader after the writer lost its
producer); composition callbacks (see the module map); the `needs_add_to_scene` dirty-bit
protocol (it fed retained-layer reuse, which this architecture does not have); `LayerNode::offset` (a second
way to say "translate" that no production tree ever set); the 57 generated `is_*/as_*/as_*_mut`
accessors bar the four with callers (`as_leader`, `as_follower`, `as_performance_overlay`); the
`f32` sugar constructors (`OffsetLayer::from_xy`, `FollowerLayer::below/above/..`,
`Clip*Layer::circular/circle/oval`), second spellings of the geometry constructors; the `testing`
feature's `LayerSpec`/`LayerTester` DSL (a third builder with no consumer); `prelude` and
`VERSION` (no importer).

---

## Producers

Five variants have no production producer today: `Canvas`, `Texture`, `PlatformView`,
`ClipSuperellipse`, `AnnotatedRegion` (the composer emits the other fourteen; `flui-app` adds
`PerformanceOverlay`). `Texture` and `PlatformView` carry real contracts (`freeze`,
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
| `ContentToken` | `Arc<()>`, compared by `Arc::ptr_eq` | Identity only; no data behind the pointer, no `static` |
| `LayerDiffer` | owned records of one frame | Single owner (the host's raster lane); no lock |

No `unsafe`, no `Arc<Mutex<_>>`, no interior mutability in the crate. The one `dyn` is
`AnnotationValue = Arc<dyn Any + Send + Sync>`.
