# flui-layer Architecture

The per-crate record for `flui-layer`: the module map, each design decision with its reason and
the tests that pin it, the thread-safety surface, and what is deliberately not here.

Citations below name symbols, not line numbers.

---

## Module map

| Layer | FLUI | Notes |
|---|---|---|
| `Layer` and its container behaviour | [`src/layer/mod.rs`](src/layer/mod.rs) `enum Layer` | Closed enum, 15 variants; no `Box<dyn Layer>` (decision 1). |
| `PictureLayer` | [`src/layer/picture.rs`](src/layer/picture.rs) | `Arc<DisplayList>`; the leaf the paint walk emits. |
| `Effect` | [`src/layer/effect.rs`](src/layer/effect.rs) | Every layer but a picture: what `ContentBuilder::push_effect` takes, so an effect of a content never draws; what a content draws is a `Chunk`. |
| `OffsetLayer` (and the `offset` field on `ImageFilterLayer`, `TransformLayer`, `OpacityLayer`) | `offset.rs`, and an `offset` field on `image_filter.rs`, `opacity.rs`, `leader.rs` | Translation is a per-variant field; the shared query is `Layer::local_translation` (decision 3). |
| `ClipRectLayer`, `ClipRRectLayer`, `ClipSuperellipseLayer`, `ClipPathLayer` | `clip_rect.rs`, `clip_rrect.rs`, `clip_superellipse.rs`, `clip_path.rs` | `Clip::None` is accepted by all four and lowered as "no clip" by the engine. |
| `ColorFilterLayer`, `ImageFilterLayer`, `ShaderMaskLayer`, `BackdropFilterLayer` | `color_filter.rs`, `image_filter.rs`, `shader_mask.rs`, `backdrop_filter.rs` | |
| `LayerLink`, `LeaderLayer`, `FollowerLayer`, `LeaderClip` | [`src/link.rs`](src/link.rs) `LayerLink`; `SceneVersion::follower_transform` resolves a follower (`src/scene/geometry.rs`), and `resolve_follower_transform` over a hand-built tree (`src/tree/follower.rs`, feature `testing`) is its oracle; `leader.rs`; `follower.rs` | The leader index lives on the tree, not on the link (decision 2); the anchor math lives on the follower layer (decision 4). |
| `AnnotatedRegionLayer` | `annotated_region.rs` | Carries an `AnnotationValue`; written by `RenderAnnotatedRegion`, read by `SceneVersion::find_annotation`, whose oracle over a hand-built tree is `LayerTree::find_annotations` (`src/tree/annotations.rs`, feature `testing`); see [Producers](#producers). |
| `SceneBuilder` push/pop, `build` to a `Scene` | [`src/compositor/builder.rs`](src/compositor/builder.rs) `SceneBuilder` | Hand-authored scenes only; the production frame is a scene patch (decision 5). |
| `Scene` | [`src/scene/mod.rs`](src/scene/mod.rs) `Scene` | One `SceneVersion`; `Scene::new` converts a hand-built `LayerTree` (decision 6). |
| Retained scene | [`src/scene/`](src/scene/) `SceneOwner`, `SceneVersion`, `BoundaryContent`, `SceneOp` | A persistent version changed by typed patches (decision 9), which `flui-rendering`'s paint writes; what ADR-0304 still owes is its item of [`docs/gaps.md`](../../docs/gaps.md). |
| Retained layers across frames | the retained scene (decision 9) | A clean repaint boundary keeps its `BoundaryContent` in the next version and costs no operation; the engine walks the version in place and skips each boundary and each chunk a partial frame's damage cannot reach (`SceneVersion::subtree_cull`). Cross-frame damage is the footprint of the patch (decision 9), not a comparison of frames. |
| Composition callbacks | none | They would fire after a scene-add step this architecture does not have. |
| `TextureLayer`, `PlatformViewLayer` | none | A texture is a `DrawOp::Texture` in a picture (ADR-0233), so it sits under the clips and transforms of its chunk and in its recording; a native view inside the UI has no scene form yet ([`docs/gaps.md`](../../docs/gaps.md), "No native views inside the UI"). |

---

## Mapping decisions

Each decision names the shape chosen and why, and the tests that pin it.

### 1. Closed `Layer` enum, not a `Layer` class hierarchy

`Layer` is a closed enum and deliberately **not** `#[non_exhaustive]`: compile-time exhaustiveness
is the contract with `flui-engine`, whose wgpu backend matches every variant. A new variant breaks
that match and lands its GPU lowering in the same change. There is no third-party `impl Layer`
extension point, and plugin authors cannot define layer types — the accepted trade-off.

Every payload is inline: `Picture` (`Arc` + `Rect`) and `ClipPath` (`Arc<Path>`) are small, so a
sealed picture run costs no heap allocation beyond its `Arc`. The budget is
128 B; re-measure before boxing anything. **Unasserted:** no test pins this.

### 2. Append-only `LayerTree` with a leader index, not a mutable tree plus a side registry

A tree is built whole and never edited (a frame retains the scene instead, §9), so it needs one structural
primitive: `LayerTree::push_child(parent, node)`, which mints the child's id in the call that links
it. A fresh id has no descendants, so a cycle or a doubly-parented node cannot be expressed; there
is no `remove`, no re-parenting, no node-level link setter, no `&mut` reach into a node, and no
generic tree-write trait. Consequences:
`flui-engine`'s walk has no cycle guard, inserts are O(1) with no ancestor check, and ids never
alias (nothing is ever freed). A parent is always pushed before its child, so its id is smaller;
`LayerTree::lowest_common_ancestor` steps the larger id up until the two meet, in O(depth) with no
allocation. `LayerId` stays a plain 1-based index; a generational id is only
needed if a holder ever outlives the frame that minted it (none does today).

`LayerLink` is a `Copy` token, so the tree holds `leaders: FxHashMap<LayerLink, LayerId>`, filled in `push_child` when the layer is
a `Leader`. The earlier shape — a `LinkRegistry` beside the tree that the composer filled and the
UI runtime had to commit "as one pair" with the tree — duplicated the leader's offset and size off the
`LeaderLayer` and kept a reverse follower index nothing read. Two leaders on one link in one frame
is a widget-tree error; a debug build trips, a release build keeps the later one.

Tests: `tree/layer_tree.rs` tests (`push_child_links_both_sides_in_paint_order`),
`tests/follower.rs` (`linked_across_offset_branches_sums_both_chains`, which finds a leader pushed under a
branch through the index), `flui-engine/src/layers/walk.rs` (`a_deep_chain_survives_a_small_stack`).
That `push_child` under an unknown parent panics: **Unasserted:** no test pins this.

### 3. One `Layer::local_translation`, read by the walk and the resolver

The chain math needs one dispatch point for "this layer translates its children". Flattening `OffsetLayer` into per-variant `offset` fields had lost it: the engine pushed translations for `Offset`, `Transform`, `Opacity`, `ImageFilter`
and `Leader`, while the resolver summed only `Offset` and `Transform` — so a follower nested under
its own leader rendered double-translated. `Layer::local_translation` is now the one place the set
of translating variants is written down. `SceneVersion::follower_transform`, and its oracle over a
hand-built tree `resolve_follower_transform`, walk both chains from the
common ancestor (exclusive) and multiply, per layer, the `Transform`'s whole matrix, a nested
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

### 5. `SceneBuilder` is secondary; the composer writes the frame into the retained scene

`flui-rendering`'s `FragmentComposer` writes each repainted boundary's content through
`ContentBuilder` into a `SceneWriter` patch (decision 9), and the engine walks the applied version in place
(`flui-engine`'s `layers::walk`). `SceneBuilder` is the push/pop shape for a hand-authored scene
(hot-reload plugins, examples, the GPU readback tests), as `dart:ui`'s `SceneBuilder` is: `push`,
`add` and `pop` return the builder for chaining and no layer ids, since `build` hands back a
`Scene` whose version names its own nodes; a caller that keeps ids builds a `LayerTree` with
`push_child`. A `pop` with nothing open is the caller's bug, which a debug build asserts, so the
crate has no error type. The builder's typed `push_*`/`add_*` are
one-line conveniences over `push(impl Into<Layer>)` / `add(..)`; a variant without one is reached
through `push(SomeLayer::new(..))`.

### 6. `Scene` holds one version; a hand-built tree becomes one

A `Scene` holds a retained `SceneVersion`, which the engine walks in place; it adds one fact over
the version: no `&mut` path back. A frame's paint publishes its version (`Scene::of_version`). A
scene authored by hand (a hot-reload plugin's image, an example, a GPU readback test) is built as a
`LayerTree` and converted when the scene is made: `Scene::new` runs `SceneVersion::from_tree`,
one node holding the tree, each picture without children a chunk of its recording and every other
layer an effect over its children. The engine therefore walks one form, closes damage over one
form (`SceneVersion::close_damage`) and keeps no tree walk, no tree-side backdrop closure and no
straddle self-heal for trees whose damage carries no such closure (ADR-0304 §9). A version from
`from_tree` comes from an owner of its own, so its lineage pairs with no other scene's and its
first frame on any target is full. The conversion's equivalence is pinned by
`flui-engine`'s `layers::walk::tests::a_version_walks_as_its_flattened_tree` (a version's walk
equals the walk of its flattened tree's version) and by `SceneVersion::from_tree`'s doctest.

Its former `size`, `root` and `frame_number` fields had no reader that did not already hold the
value (`root` was a copy of `LayerTree::root` that eleven of thirteen callers left disagreeing;
`frame_number` is the presentation's counter, read back by its writer; `size` had no reader).
`flui-hot-reload` moves a `Scene` across its dlopen boundary and hashes `size_of::<Scene>` into
its ABI token, which pins that the type is nameable without an engine dependency — not its field
set.

### 7. Runtime state lives with its owner

`DamageTracker` (the renderer's per-frame dirty accumulator, ADR-0061's consuming half) lives in
`flui-engine/src/frame/damage.rs`; the seam message `DamageRegion` and its producer, the footprint of a scene patch
(decision 9), are defined here. The tracker keeps the seam's `Region` (up to eight disjoint rects,
ADR-0304 §5), merged as `Region::insert` merges, and the engine redraws it rect by rect or under its bounding box (ADR-0304 §9, the engine's
`ARCHITECTURE.md`). The performance overlay has no layer of its own: `flui-runtime`, beside the
presentation that knows when a frame was composited, samples its numbers from the frame clock,
records the readout through the UI runtime's `TextContext` and publishes it as one chunk under a
clip at a scene node of its own (`performance_overlay_matrix`), so the engine replays shaped labels
and shapes nothing (ADR-0092).

### 8. The stamp differ is the damage oracle — [ADR-0304 §4](../../docs/adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)

Production damage is the footprint of a scene patch (decision 9). `LayerDiffer`, `BoundaryStamp`
and `ContentToken` stay behind the `testing` feature as the independent answer the footprint is
checked against: a frame's damage must cover what the differ finds between two layer trees. A
stamped layer carries a `BoundaryStamp`: a `RenderId`, which pairs the layer with the previous
tree's (every tree's `LayerId`s are fresh), and a `ContentToken`, kept while the content is the
same and minted otherwise. `LayerDiffer::diff` walks the
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
closure takes all of it, and damage that meets one member of a backdrop group takes the rect over every
member's (a `Closures` set keys them by `BackdropGroupKey`), to a fixed point of at most 64 rounds. The engine's backdrop planning
reaches the same two functions through `SceneVersion::close_damage`, and the crate exports them
only under `testing`, for the oracle's tests. The result is `Full` for an unpairable frame (first frame,
surface size change, unstamped root, root boundary or placement change, a boundary stamped
twice) or above `DamageMode::On::full_above`, `Unchanged` for nothing, and otherwise one
`DamageRect` in whole pixels, rounded outward with a 1 px anti-aliasing margin. `DamageMode::Off`
retains nothing and does not walk.

Why the differ stays: the footprint reads only the patch's operations, so a footprint rule that
misses a change under-reports silently; the differ derives damage from the two whole trees, an
answer that shares none of the footprint's bookkeeping (`patch_damage_covers_diff_damage`). It is
not on the frame path: a walk of every layer each frame is the cost the retained scene removed
(`damage_diff` bench: about 200 µs for 1,000 boundaries). Tests: `src/damage/tests.rs`,
`damage/region.rs`'s `union_table` and `bounds_round_outward_with_aa_margin`.

### 9. The retained scene is a persistent value — [ADR-0304 §1–§3](../../docs/adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)

A `SceneVersion` is an `Arc` over its nodes, kept in a path-copying table of branches of 32 over
leaves of 8
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
one render object's run; its bounds and textures are read from its `DisplayList`. An effect's
layer is held beside the stream, one per effect in paint order (`BoundaryContent::effect_layer`);
the stream sits in an `Arc` with everything that depends on the entries alone: each effect's
entry, the effects and slots in order, each effect's scope end and the union of its direct
chunks' bounds, the union of the top-level chunks' bounds, the slots by node, and the chunks'
texture and closure entries. `SceneWriter::with_effects`, the composited-layer update, copies the
layers into a content the owner reclaimed when it holds one, rebuilds the indexes they feed (links,
texture layers, filter closures) and shares the stream, so it costs the content's effects and never
its chunks. A refused update (an index that names no effect, or names one twice) hands the
layers it was given to the owner, which drops them when the next patch opens, as it does a
discarded content.

`SceneOwner::apply` builds the patch on a copy of the version and keeps it only when every
operation is valid: an id of another lineage, an unknown or stale id, a retired node a slot still
names, a node named twice, a slot naming its own ancestor or the root, and a depth past `u32`
refuse the whole patch, and the version stays the one before. Its working sets are slot-indexed
marks the owner keeps across patches, so an apply hashes nothing; a republished content writes
only the children it gains or loses, and leaves the indexes when its link, texture and filter
entries are the old ones. `ContentVersion` and `SceneVersionId` draw from one `IdMint` per owner
under a `Lineage` from a process `AtomicIdMint`, so a replaced or renewed owner never repeats one;
a spent sequence refuses with `VersionsExhausted`; `renew` starts a new lineage, runs the
sequence on unless it is spent, and starts with empty
slots, whose next publication is `Full(Resync(_))`. Nothing in production builds a layer tree from
a version: the engine walks it, and the inspector reads it in place as an `InspectTree`
(`src/scene/inspect.rs`), naming a boundary by its slot, generation and lineage and an effect or
chunk by its position in the boundary's content, so an id survives every frame that keeps the
boundary. `SceneVersion::flatten`, behind the `testing` feature, is the oracle both are compared
against (ADR-0304 §11).

An apply's damage is the footprint of its operations (ADR-0304 §4, `src/scene/footprint.rs`),
read under the version before and the version after. A node record keeps its subtree bounds in
its own space: `Publish` recomputes them exactly, deepest node first, and every other change
grows its ancestors' bounds by union, so a footprint over-reports and never under-reports. A
republished content pairs its entries by key (a chunk and an effect by owner and ordinal, a slot
by its node); a changed chunk or effect, or an unpaired entry, damages its old and new scope, and
kept entries outside the longest increasing run of their old order damage both positions. A
content that shares its predecessor's stream skips the pairing: each changed effect none of whose
ancestors changed damages its scope under both versions. `Place`
damages the moved subtree at both placements. After the operations every follower is resolved
again in both versions against its link's last attached leader (a published node no slot
reaches draws nothing and never shadows it), and one whose transform changed damages its subtree
under both. Both chains of a follower stop at the lowest common ancestor of it and its leader, as
`SceneVersion::follower_transform` walks them, so nested followers cost their unshared chains. A picture
effect changes its display list's damage extent. A subtree carries, beside its local
bounds, the device-pixel kernel its filters spread by (the renderer takes a kernel in physical
pixels, which bounds in a boundary's space would shrink under a shrinking ancestor; a filter adds
its own to the maximum of its children's, a sum along a chain), and a
footprint adds it once mapped to the surface. The region then closes over the filtered nodes of
both versions. A follower under `LeaderClip::Hide` is damaged when the patch changes a node on its
leader's branch, and so is every follower whose chains pass through it, since hidden it hides
them. A scope's footprint folds only its effects and slots, from the last back and without
recursion, each effect starting from its direct chunks' union; a clip over
filters with a kernel bounds their scope by the clip, and a clip that cuts its scope away whole
keeps the strip of itself within one device pixel of it (`edge_of`), judged by the least scale of
the maps between the clip and the surface that the fold knows (a map out of the plane keeps the
whole clip), since an anti-aliased edge shows
content in a pixel the two share ([ADR-0304](../../docs/adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md) §4). A node's foreground filter inputs are read
only once the damage meets its subtree footprint, in one fold; until then only the content's
closure entries are visited, never its other chunks. A surface
change between applies is
`Full(SurfaceChanged)`, and `renew` makes the next one `Full(Resync(_))`.

Each applied version enters a pending log (`src/scene/pending.rs`) until the raster side reports
holding it or a later one through `completed`. `publication` hands over the current version with
the acknowledged one as its base and the union of the log as its damage, full without a base. The
log holds sixteen entries; a seventeenth folds the two oldest under the newer id and lets go of the
older contents, so a raster stall over-reports damage and never holds more than sixteen entries of
buffers. An acknowledgement moves forward only and ignores another lineage or a version never
applied. `apply` (in `Applied::released`), `completed` and `renew` return what they release as
`ReleasedContents`, which a caller holding the owner in a `RefCell` passes to an `Outbox`: a
released annotation value's destructor is user code and finds the owner free. An apply keeps the
version it replaced among two retiring ones and releases it once no other holder reads it (an
`Arc` count of one) or a newer one pushes it out; a fold, a completion or a renewal releases
contents. `ReleasedContents::reclaim`, run with no borrow held, drops what was released and keeps,
emptied, the version records, table nodes (`table.rs` `Spares`), contents and their chunks the
owner held alone; `SceneOwner::restock` takes them back under small caps (a chunk only while its
command list's room is at most `KEPT_CHUNK_ROOM`). A write that copies a shared table node
copies it into an emptied one, an apply's version takes an emptied record, `with_effects`
writes into an emptied content, and `SceneWriter::chunk` writes a repainted run into an emptied
chunk and hands back its emptied command list for the next recording, so a steady layer update
and a steady repaint reuse the same buffers each frame. A
painter that gives up contents it recorded for a patch never applied hands them to
`SceneWriter::discard`; the owner drops them when `writer` opens the next patch or `renew` returns
them, both outside the painter's borrows.

Pinned by `layer_it`: `pending_log_contract`, `scene_patch_damage`, `patch_damage_covers_diff_damage` (random patches,
whose damage must cover what `LayerDiffer` finds between the flattened versions),
`scene_apply_contract`, `content_version_contract` and `scene_table_cost`
(an apply's copies are at most its operations times the table height, equal at 1 100 and 8 000
nodes, and a republished parent copies as much at 40 children as at 200); the table's model
test against `BTreeMap` is `scene::table::tests`. `scene_reclaim`: a version someone holds keeps
what it was applied with while the owner reuses what it held alone
(`reclaimed_buffers_never_change_a_version_someone_holds`, against an owner that reclaims nothing),
an update takes what a completion handed back (`an_update_writes_into_what_the_owner_reclaimed`),
a repaint writes into a reclaimed chunk and never into a held one
(`a_repaint_records_into_the_chunks_the_owner_reclaimed`), and a refused update drops none of its layers while the writer borrows the owner
(`a_refused_effect_update_drops_its_layers_outside_the_writer`).

### Deleted, with the reason

`LayerHandle<T>` (no engine-layer lifecycle exists here); `LayerBounds` trait
(one impl per type, no generic consumer); composition callbacks (see the module map); the `needs_add_to_scene` dirty-bit
protocol (it fed retained-layer reuse, which this architecture does not have); `LayerNode::offset` (a second
way to say "translate" that no production tree ever set); the 57 generated `is_*/as_*/as_*_mut`
accessors bar the two with callers (`as_leader`, `as_follower`); the
`f32` sugar constructors (`OffsetLayer::from_xy`, `FollowerLayer::below/above/..`,
`Clip*Layer::circular/circle/oval`), second spellings of the geometry constructors; the `testing`
feature's `LayerSpec`/`LayerTester` DSL (a third builder with no consumer); `prelude` and
`VERSION` (no importer); `PerformanceOverlayLayer` and its `PerformanceOverlayOption` (a layer
that drew beside the chunks that draw, and options no row honoured: the readout is a chunk
`flui-runtime` records).

---

## Producers

Three variants have no production producer today: `Picture`, `ColorFilter` and `ImageFilter`.
The other twelve are written into the retained scene as effect entries of a boundary's content
(`ContentBuilder::push_effect`), by `flui-rendering`'s paint composer
(`crates/flui-rendering/src/pipeline/owner/paint/composer.rs`, `push_layer`) and, for the performance
overlay, by `flui-runtime`:

- `scope_layer` and `clip_layer` turn a recorded `FragmentScope` or `PaintClip` into `Opacity`,
  `ClipRect`, `ClipRRect`, `ClipSuperellipse`, `ClipPath`, `ShaderMask`, `BackdropFilter`,
  `AnnotatedRegion`, `Leader` and `Follower`. `ClipSuperellipse` comes from
  `PaintClip::RSuperellipse`, which `RenderClipRSuperellipse` (`flui-objects`, behind the
  `ClipRSuperellipse` widget) records; `AnnotatedRegion` comes from `PaintContext::with_annotated_region`,
  which `RenderAnnotatedRegion` calls;
- `FragmentComposer` writes `Offset` (the root at a device pixel ratio of 1) and `Transform` (the
  root's device-pixel-ratio scale, and a node's own paint transform in `own_effect_layers`). A
  sealed recording is a chunk, not a `Picture` (`seal_picture`), and a repaint-boundary child is a
  slot (`place_boundary_impl`);
- `flui-runtime`'s presentation scene writes a `ClipRect`, under a `Transform` at a device pixel
  ratio other than 1, over the performance overlay's chunk (`presentation_scene.rs`).

An external texture is not a layer: `RenderTexture` (`flui-objects`) records a `DrawOp::Texture`
into its picture (ADR-0233), the version indexes each chunk's texture draws, and a texture frame
damages those users (`SceneVersion::texture_damage`), while the host hands each frame's
`TextureRegistry` updates to the backend before it renders. A native view inside the UI has no
form in the scene: the ADR that brings one decides its compositing, hit-testing and semantics
([`docs/gaps.md`](../../docs/gaps.md)). `Picture` is what a hand-built tree and
`SceneVersion::flatten` draw a chunk as. `ColorFilter` and
`ImageFilter` are lowered by `flui-engine` (`layers/render.rs`), but no `FragmentScope` produces
them ([`docs/gaps.md`](../../docs/gaps.md), "No widget filters its subtree's colours or pixels"): a
widget's color filter reaches the GPU as a draw command (`DrawOp::ImageFiltered` in
`flui-painting`) inside a picture. `AnnotatedRegion` has both halves: the writer above and the
reader `SceneVersion::find_annotation`, which `flui-runtime`'s `overlay_style.rs` asks for the
system UI overlay style under a probe point; `LayerTree::find_annotations`
(`src/tree/annotations.rs`, feature `testing`, tested in `tests/annotations.rs`) answers the same for a hand-built
tree, the oracle of the version's read. Whether each unproduced variant is "not wired yet" or
"never" is a roadmap decision, not a crate one — recorded here so it is not re-derived.

---

## Thread safety

| Site | Primitive | Category |
|---|---|---|
| `LayerTree::nodes` | `Vec<LayerNode>` | Owned, single mutator; no lock, no atomics |
| `LayerTree::leaders` | `FxHashMap<LayerLink, LayerId>` | Owned, filled during the build |
| `Scene`, `SceneSnapshot` | by value | Moved across the raster boundary; `Send + Sync` plain data, pinned by a compile-time `Send + Sync` check in `scene_snapshot.rs` |
| `LayerLink::new` | `static AtomicIdMint<NonZeroU64>` | Identity only; minted from every UI thread, never wraps, panics after 2^64 - 1 links |
| `Lineage::fresh` | `static AtomicIdMint<NonZeroU64>` (`counter` grant) | Identity only; minted from every UI thread, never wraps, refuses with `IdExhausted` |
| `SceneVersion` | `Arc` over persistent tables of `Arc` nodes | Immutable once applied; `Send + Sync` by its auto traits (`a_version_is_send_and_sync`), shared by the owner and the raster side |
| `SceneOwner` | owned ledger and `IdMint` | Single owner (the UI runtime); `Send`, not `Sync` |
| `ContentToken` (feature `testing`) | `Arc<()>`, compared by `Arc::ptr_eq` | Identity only; no data behind the pointer, no `static` |
| `LayerDiffer` (feature `testing`) | owned records of one tree | Single owner (the test that compares); no lock |
| `TextureRegistry` | `Arc` of a `parking_lot::Mutex` over the registry state, plus an `Arc<dyn Fn() + Send + Sync>` wake | Shared by one presentation with frame producers on any thread (`src/texture_registry.rs`) |

No `unsafe` in the crate. Its one lock is `TextureRegistry`'s; its `dyn`s are that registry's
wake callback and the `Arc<dyn Any + Send + Sync>` inside `AnnotationValue`
(`src/layer/annotated_region.rs`).
