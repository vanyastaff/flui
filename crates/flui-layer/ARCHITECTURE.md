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
| `LayerLink`, `LeaderLayer`, `FollowerLayer`, `LeaderClip` | [`src/link.rs`](src/link.rs) `LayerLink` + `resolve_follower_transform`; `leader.rs`; `follower.rs` | The leader index lives on the tree, not on the link (decision 2); the anchor math lives on the follower layer (decision 4). |
| `AnnotatedRegionLayer` | `annotated_region.rs` | Carries an `AnnotationValue`; written by `RenderAnnotatedRegion`, read by `LayerTree::find_annotations` (`src/tree/annotations.rs`); see [Producers](#producers). |
| `SceneBuilder` push/pop | [`src/compositor/builder.rs`](src/compositor/builder.rs) `SceneBuilder` | Hand-authored scenes only; the production frame is built through the tree (decision 5). |
| `Scene` | [`src/scene/mod.rs`](src/scene/mod.rs) `Scene` | A frozen `LayerTree` (decision 6). |
| Retained scene | [`src/scene/`](src/scene/) `SceneOwner`, `SceneVersion`, `BoundaryContent`, `SceneOp` | A persistent version changed by typed patches (decision 9); no producer yet, see the ADR-0304 item of [`docs/gaps.md`](../../docs/gaps.md). |
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
UI runtime had to commit "as one pair" with the tree — duplicated the leader's offset and size off the
`LeaderLayer` and kept a reverse follower index nothing read. Two leaders on one link in one frame
is a widget-tree error; a debug build trips, a release build keeps the later one.

Tests: `tree/layer_tree.rs` tests (`push_child_links_both_sides_in_paint_order`),
`tests/follower.rs` (`linked_across_offset_branches_sums_both_chains`, which finds a leader pushed under a
branch through the index), `flui-engine/src/layer_walk.rs` (`a_deep_chain_survives_a_small_stack`).
That `push_child` under an unknown parent panics, and that a semantics-failure retry submits the
retained tree with its leader index intact: **Unasserted:** no test pins this.

### 3. One `Layer::local_translation`, read by the walk and the resolver

The chain math needs one dispatch point for "this layer translates its children". Flattening `OffsetLayer` into per-variant `offset` fields had lost it: the engine pushed translations for `Offset`, `Transform`, `Opacity`, `ImageFilter`
and `Leader`, while the resolver summed only `Offset` and `Transform` — so a follower nested under
its own leader rendered double-translated. `Layer::local_translation` is now the one place the set
of translating variants is written down. `resolve_follower_transform` walks both chains from the
common ancestor (exclusive) and multiplies, per layer, the `Transform`'s whole matrix, a nested
`Follower`'s own resolved transform, or the translation `local_translation` answers
([ADR-0276](../../docs/adr/ADR-0276-a-follower-takes-its-leaders-full-transform-and-may-hide-with-its-clips.md)).

A follower nested under its own leader resolving to zero, and an `Opacity` offset on the
follower's chain being counted: **Unasserted:** no test pins this.

### 4. Anchors, size and leader-clip policy on `FollowerLayer`, resolution at composite time

Both anchors, the follower's own `size` and its `LeaderClip` live on the layer so
`FollowerLayer::linked_offset` (`Alignment::along_size` on both rectangles, plus
`target_offset`, in the leader's coordinates) and the clip test run from the layer tree alone —
the same transform the GPU walk, the damage producers, the annotation lookup and the follower
hit-test side table all read. Anchors outside `[-1, 1]` are legal off-rectangle pivots. Under
`LeaderClip::Hide` the leader's anchor point is mapped up the leader's chain and tested against
each clip layer on it; clips at or above the common ancestor already clip the follower. A
singular or non-finite chain, a hidden nested follower and nesting past `MAX_FOLLOWER_NESTING`
resolve to `None`, so no consumer sees a degenerate transform. An affine chain is inverted in 2D,
so the result keeps the exact affine shape the GPU painter admits
(`a_scaled_follower_parent_keeps_the_affine_shape`).

Tests: `tests/follower.rs`, run as `follower_resolution` in `tests/main.rs`
(`a_scaled_leader_scales_the_follower`, `a_leader_inside_another_follower_resolves_through_it`,
`hide_follows_the_anchor_through_the_leader_clip`,
`a_singular_transform_on_a_chain_resolves_to_nothing`,
`followers_nested_past_the_limit_resolve_to_nothing`, …).

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
tracker's three-rect merge was removed with the move (the seam's `Region` keeps up to eight disjoint rects, ADR-0304 §5): the one consumer reads `damage_rect()`, the bounding
union, through which the merge was unobservable. The overlay's numbers (a frame-clock reading of the
presentation's frame history) are computed in `flui-runtime`, beside the presentation that knows when a frame was
composited; `PerformanceOverlayLayer::record` composes the numbers it is handed through the
caller's `TextContext` into the display list the layer carries, so the engine replays shaped
labels and shapes nothing (ADR-0092). `record` stays here, beside the layer, because it is the
one recorder of the layer's readout; it holds no state.
`PerformanceOverlayOption`
crosses the engine boundary as itself rather than as a `u32`
(`performance_overlay_readout_rows`).

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
changes pixels under a transparent source damages the whole surface. The damage then closes
over every effect's `read_closure` (`src/damage/closure.rs`, ADR-0304 §6) with `close_over`: a
backdrop filter reads its bounds grown by the kernel's reach (the larger of its local reach scaled
by the transform and the renderer's physical-pixel growth, `ceil(sqrt(3) x sigma)` for a blur) and
writes its bounds; a foreground image filter reads and writes its children's extents moved or
grown the same way, since the engine composites its whole output; a colour or image filter that
paints transparent black has no bound and makes the frame `Full(Unbounded)`. Damage that meets a
closure takes all of it, to a fixed point of at most 64 rounds. The engine's backdrop planning
calls the same two functions. The result is `Full` for an unpairable frame (first frame,
surface size change, unstamped root, root boundary or placement change, a boundary stamped
twice) or above `DamageMode::On::full_above`, `Unchanged` for nothing, and otherwise one
`DamageRect` in whole pixels, rounded outward with a 1 px anti-aliasing margin. `DamageMode::Off`
retains nothing and does not walk.

Why a token rather than `Arc::ptr_eq` on pictures: a boundary that repaints for any reason re-records its inline
pictures, so pointer identity reports it changed on frames
whose content did not. Why an `Arc<()>` rather than a counter: a counter restarts with each
`PipelineOwner`, and the differ, holding the previous frame's clones, makes an allocation
address impossible to reuse while compared. The differ pairs by the boundary `RenderId` and decides "unchanged" by the token, and
the granularity is one boundary's own region, not a layer. `damage_diff` bench: about 200 µs for
1,000 boundaries, 4 ns with the mode off. Tests: `src/damage/tests.rs`,
`scene_snapshot.rs`'s `union_table` and `bounds_round_outward_with_aa_margin`.

### 9. The retained scene is a persistent value — [ADR-0304 §1–§3](../../docs/adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)

A `SceneVersion` is an `Arc` over its nodes, kept in a path-copying table of fanout 32
(`src/scene/table.rs`) written here rather than taken from a persistent-collection crate: the
scene needs lookup, insert, remove and an ordered walk. A write copies its path from the root to
its leaf and shares every other subtree with the version before it; writes into one leaf share
that path; the height grows by one new root per level when a slot passes the capacity, so a
lookup walks two to four levels at scene sizes FLUI meets. Beside the nodes, three tables of the
same shape index link layers (leaders and followers by `LayerLink`), texture users (by
`TextureId`) and nodes whose content holds a backdrop or image filter.

A node is a `SceneNodeId`: its owner's `Lineage` and a generational slot key the `SceneOwner`
mints through `GenSlots`. Minting advances the slot's generation, so an id of a patch never
applied, or of a retired node, never resolves again; the lineage keeps two owners' ids apart,
though both mint the same slot keys, so a cache keyed by a `SceneNodeId` serves one scene. Inside
a version every reference is the bare slot key, and the public id is assembled from the version's
lineage. A boundary's content is one paint-order stream of `Entry::{Effect, Chunk, Slot}` built by
`ContentBuilder`, which makes every entry's parent an earlier effect by construction. A chunk is
one render object's run; its bounds and textures are read from its `DisplayList`.

`SceneOwner::apply` builds the patch on a copy of the version and keeps it only when every
operation is valid: an id of another lineage, an unknown or stale id, a retired node a slot still
names, a node named twice, a slot naming its own ancestor or the root, and a depth past `u32`
refuse the whole patch, and the version stays the one before. Its working sets are slot-indexed
marks the owner keeps across patches, so an apply hashes nothing; a republished content writes
only the children it gains or loses, and leaves the indexes when its link, texture and filter
entries are the old ones. `ContentVersion` and `SceneVersionId` draw from one `IdMint` per owner
under a `Lineage` from a process `AtomicIdMint`, so a replaced or renewed owner never repeats one;
a spent sequence refuses with `VersionsExhausted` and `renew` starts a new lineage, with empty
slots, whose next publication is `Full(Resync(_))`. `SceneVersion::flatten` (feature `testing`) is
the layer-tree oracle a retained frame is compared against.

An apply's damage is the footprint of its operations (ADR-0304 §4, `src/scene/footprint.rs`),
read under the version before and the version after. A node record keeps its subtree bounds in
its own space: `Publish` recomputes them exactly, deepest node first, and every other change
grows its ancestors' bounds by union, so a footprint over-reports and never under-reports. A
republished content pairs its entries by key (a chunk and an effect by owner and ordinal, a slot
by its node); a changed chunk or effect, or an unpaired entry, damages its old and new scope, and
kept entries outside the longest increasing run of their old order damage both positions. `Place`
damages the moved subtree at both placements. After the operations every follower is resolved
again in both versions against its link's last attached leader (a published node no slot
reaches draws nothing and never shadows it), and one whose transform changed damages its subtree
under both. The region then closes over the filtered nodes of both versions; a foreground filter
whose node, an ancestor or a descendant the patch changed seeds it with its whole footprint first, since stored
bounds spread a filter in the boundary's space and a shrinking ancestor shrinks that below the
renderer's device-pixel kernel. A follower under `LeaderClip::Hide` is damaged when the patch
changes a node on its leader's branch. A surface change between applies is
`Full(SurfaceChanged)`, and `renew` makes the next one `Full(Resync(_))`.

Pinned by `layer_it`: `scene_patch_damage`, `patch_damage_covers_diff_damage` (random patches,
whose damage must cover what `LayerDiffer` finds between the flattened versions),
`scene_apply_contract`, `content_version_contract` and `scene_table_cost`
(an apply's copies are at most its operations times the table height, equal at 1 100 and 30 000
nodes, and a republished parent copies as much at 40 children as at 1 000); the table's model
test against `BTreeMap` is `scene::table::tests`.

### Deleted, with the reason

`LayerHandle<T>` (no engine-layer lifecycle exists here); `LayerBounds` trait
(one impl per type, no generic consumer); composition callbacks (see the module map); the `needs_add_to_scene` dirty-bit
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
`ColorFilter`, `ImageFilter`. The other fourteen are built in production, all but one by
`flui-rendering`'s paint composer (`crates/flui-rendering/src/pipeline/owner/paint.rs`):

- `scope_layer` and `clip_layer` turn a recorded `FragmentScope` or `PaintClip` into `Opacity`,
  `ClipRect`, `ClipRRect`, `ClipSuperellipse`, `ClipPath`, `ShaderMask`, `BackdropFilter`,
  `AnnotatedRegion`, `Leader` and `Follower`. `ClipSuperellipse` comes from
  `PaintClip::RSuperellipse`, which `RenderClipRSuperellipse` (`flui-objects`, behind the
  `ClipRSuperellipse` widget) records; `AnnotatedRegion` comes from `PaintCx::with_annotated_region`,
  which `RenderAnnotatedRegion` calls;
- `FragmentComposer` builds `Offset` (the root at a device pixel ratio of 1, and each
  repaint-boundary child in `place_boundary_impl`), `Transform` (the root's device-pixel-ratio
  scale, and a node's own paint transform in `own_effect_layers`) and `Picture` (each sealed
  recording, `seal_picture`);
- `flui-runtime` adds `PerformanceOverlay` (`PerformanceOverlayLayer::record` in
  `crates/flui-runtime/src/presentation.rs`).

`Texture` and `PlatformView` carry real contracts (`freeze`, `hit_test_behavior`) awaiting the
platform layer. External textures reach the GPU without the layer: `RenderTexture`
(`flui-objects`) records a `DrawOp::Texture` into its picture, and the host hands each frame's
`TextureRegistry` updates to the backend before it renders. `Canvas` is a recorder inside the output
vocabulary that `PictureLayer` already covers, built only by tests and benches. `ColorFilter` and
`ImageFilter` are lowered by `flui-engine` (`layer_render.rs`), but no `FragmentScope` produces
them: a widget's color filter reaches the GPU as a draw command (`DrawOp::ImageFiltered` in
`flui-painting`) inside a picture. `AnnotatedRegion` has both halves: the writer above and the
reader `LayerTree::find_annotations` (`src/tree/annotations.rs`, tested in `tests/annotations.rs`),
which no production path calls yet. Whether each unproduced variant is "not wired yet" or
"never" is a roadmap decision, not a crate one — recorded here so it is not re-derived.

---

## Thread safety

| Site | Primitive | Category |
|---|---|---|
| `LayerTree::nodes` | `Vec<LayerNode>` | Owned, single mutator; no lock, no atomics |
| `LayerTree::leaders` | `HashMap<LayerLink, LayerId>` | Owned, filled during the build |
| `Scene`, `SceneSnapshot` | by value | Moved across the raster boundary; `Send + Sync` plain data, pinned by `static_assertions` in `scene_snapshot.rs` |
| `LayerLink::new` | `static AtomicU64`, `Relaxed` | Identity only; the RMW is atomic under every ordering and publishes nothing else |
| `Lineage::fresh` | `static AtomicIdMint<NonZeroU64>` (`counter` grant) | Identity only; minted from every UI thread, never wraps, refuses with `IdExhausted` |
| `SceneVersion` | `Arc` over persistent tables of `Arc` nodes | Immutable once applied; `Send + Sync` by its auto traits (`a_version_is_send_and_sync`), shared by the owner and the raster side |
| `SceneOwner` | owned ledger and `IdMint` | Single owner (the UI runtime); `Send`, not `Sync` |
| `ContentToken` | `Arc<()>`, compared by `Arc::ptr_eq` | Identity only; no data behind the pointer, no `static` |
| `LayerDiffer` | owned records of one frame | Single owner (the host's raster lane); no lock |
| `TextureRegistry` | `Arc` of a `parking_lot::Mutex` over the registry state, plus `Arc<dyn Fn() + Send + Sync>` wake and listeners | Shared by one presentation with frame producers on any thread (`src/texture_registry.rs`) |

No `unsafe` in the crate. Its one lock is `TextureRegistry`'s; its `dyn`s are that registry's
wake and listener callbacks and the `Arc<dyn Any + Send + Sync>` inside `AnnotationValue`
(`src/layer/annotated_region.rs`).
