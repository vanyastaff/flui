# ADR-0304: The scene is retained, and damage is its patch's footprint

- **Status:** Accepted (2026-11-03)
- **Date:** 2026-11-03
- **Supersedes:** [ADR-0061](ADR-0061-damage-needs-layer-identity.md) for its Decision and its
  2026-09-29 amendment only (damage from comparing consecutive layer trees through boundary
  stamps); [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md) §3 only (the whole-scene
  differ as the producer, `ContentToken`, the one-rectangle `Partial`, the off switch that retains
  nothing, and the amended sentence that `run_paint` always descends from the root);
  [ADR-0253](ADR-0253-retained-paint-captures-hold-slots-for-nested-boundaries.md) §2 and §6 only
  (a replay that mints every clean boundary's nodes into a new tree each frame, and that per-frame
  tree with its eviction of boundaries holding a follower);
  [ADR-0068](ADR-0068-frame-disposition-replaces-the-presented-bit.md) §3's repaint half only
  (`mark_needs_full_repaint_for` as a `NotShown` retry's work; the wake half stands)
- **Amends:** [ADR-0006](ADR-0006-c-ir-record-replay-seam.md) §1 and §6 (a chunk's IR outlives the
  frame under a recording key; replay determinism becomes load-bearing and gains its test);
  [ADR-0276](ADR-0276-a-follower-takes-its-leaders-full-transform-and-may-hide-with-its-clips.md)
  §1 (resolution reads the retained scene and finds the common ancestor by stored depth; the rule
  is unchanged)
- **Related:**
  [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md) (the
  executor that holds each presentation's rastered version and contains a raster panic);
  [ADR-0306](ADR-0306-display-list-commands-hold-local-geometry-under-interned-transforms.md) (the
  command representation a chunk holds and the recording cache keys);
  [ADR-0045](ADR-0045-raster-lane.md) §1 (one owned value crosses down, `Copy` facts cross up);
  [ADR-0062](ADR-0062-the-paint-queue-is-the-cross-pass-record.md) (a failed paint pass commits
  nothing and the queue survives);
  [ADR-0068](ADR-0068-frame-disposition-replaces-the-presented-bit.md) §2 (an exhaustive outcome
  enum);
  [ADR-0100](ADR-0100-prepared-gpu-work-and-retained-frame-commit.md) (known damage remains owed;
  a candidate is promoted after a successful submission);
  [ADR-0233](ADR-0233-external-texture-registry.md) (texture frames reach the raster side with a
  scene); [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (a replaced value
  is released after the owner's state commits); [ADR-0098](ADR-0098-owned-f64-geometry-values.md)
  §4 and §6 (float cache keys by canonical bits; covering bounds round outward);
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (the lineage mint is a granted `counter`)

## Context and problem statement

Paths and lines are at `e5ba8fe8f`. Since ADR-0253 a clean boundary is replayed whatever is nested
in it and the root is retained, so paint already knows which boundaries changed. Everything after
paint still works per frame and per scene:

| Fact | Evidence |
|------|----------|
| Each paint pass builds a new `LayerTree` and pushes a clone of every layer of every clean boundary into it | `crates/flui-rendering/src/pipeline/owner/paint.rs:55-61`, `:197`, `:1128-1131`, `:1469-1471` |
| The tree is append-only, its ids follow insertion order, and it lives for one frame | `crates/flui-layer/src/tree/layer_tree.rs:1-11`, `:218-237`; `crates/flui-layer/ARCHITECTURE.md:46-57` |
| Damage is a walk of the whole frozen scene against the previous one, about 200 µs per 1,000 boundaries | `crates/flui-app/src/app/raster_lane.rs:359`; `crates/flui-layer/src/damage/differ.rs:92-108`, `:221`; `crates/flui-layer/ARCHITECTURE.md:172-173` |
| Damage is one rectangle: a caret and a clock in opposite corners repaint everything between them | `crates/flui-layer/src/scene_snapshot.rs:20-29`; `crates/flui-engine/src/damage.rs:11-14`, `:29-33` |
| Content under a follower, textures, canvas layers, platform views and the performance overlay are damaged on every frame they exist | `differ.rs:230-239`, `:326-341`, `:477-478` |
| A boundary that holds a `Leader` or `Follower` is never retained, because its capture does not carry the follower's render object | `paint.rs:1603-1609`, `:1650-1652` |
| One picture run holds the output of several render objects | `paint.rs:1500-1503` |
| The engine records every layer of the scene into its IR every frame, a partial frame included | `crates/flui-engine/src/renderer.rs:2652` |
| Two rules say what a filter reads: the differ joins a backdrop's output bounds, the engine joins its input with another radius and treats every other filter as full | `differ.rs:480-507`, `:655-678`, `:738-748`; `crates/flui-engine/src/frame_protocol.rs:258-275`, `:283-299` |
| A non-finite damage rectangle is reported as `Unchanged` | `scene_snapshot.rs:71-76`; `differ.rs:509-511` |
| The content certificate is an allocation because an owner-local counter restarts with each `PipelineOwner` | `crates/flui-layer/src/tree/stamp.rs:23-30` |

ADR-0061 rejected damage taken from paint because inline content repainted from the root on every
frame; since ADR-0253 §3 and §4 that premise no longer holds. What does the owner hand the raster
side each frame, and where does damage come from, so that a frame costs what changed rather than
what exists?

## Decision drivers

- A frame's paint, damage and recording follow what changed: a caret blink in a list of 10,000
  items costs what it costs in a list of 100.
- One footprint rule: the pixels a frame redraws, the damage it reports and what a filter reads
  come from one function.
- The frame path is synchronous and the owner never blocks on the raster side; one owned value
  crosses down and only `Copy` facts cross up (ADR-0045 §1).
- Accepted damage is never lost: a superseded, refused, failed or unshown frame leaves its damage
  owed (ADR-0100), and an older acknowledgement cannot clear a newer obligation.
- Identity is a typed ID with a generation; no counter wraps.
- No non-finite geometry is published; a rectangle that cannot be computed widens damage, never
  narrows it.
- A panic in paint changes no published state (ADR-0062).
- The full comparison stays available as an oracle for the incremental one.

## Considered options

1. **A new tree each frame, a full diff and a multi-rect region.** Today's pipeline with
   `Partial` widened to several rectangles.
2. **Raster-side subtrees grafted by command.** The raster side keeps each boundary's layers, and
   the owner sends graft, replace and remove commands per boundary.
3. **A retained scene changed by typed operations.** The owner keeps a persistent, versioned
   scene; paint writes a patch of operations; damage is the patch's footprint, computed when it is
   applied.
4. **No retention.** Repaint, re-record and redraw everything each frame, and delete damage.

## Decision

Chosen option: 3, because it is the only one in which paint, damage and recording are each
proportional to the patch, while the raster side reads an immutable value and needs nothing from
the owner but that value and a base.

§1. **The scene is a persistent value.** `flui-layer` holds:

```rust
pub struct SceneVersion(Arc<VersionInner>);                // Clone is one reference-count increment
pub struct SceneVersionId { lineage: Lineage, seq: NonZeroU64 }   // Copy, Eq
pub struct SceneNodeId { lineage: Lineage, key: NodeKey }  // Copy; Eq and Hash on both
struct NodeKey(GenId<SceneNodeMarker>);                    // u32 slot, NonZeroU32 generation
struct NodeRecord {
    key: NodeKey,
    parent: Option<NodeKey>,
    depth: u32,
    placement: Placement,                                  // in the parent's slot space
    content: Arc<BoundaryContent>,
    subtree_bounds: Bounds,                                // §4
}
pub struct SceneNode<'a> { lineage: Lineage, record: &'a NodeRecord }   // read through a version
```

`VersionInner` keeps the nodes in a path-copying table, branches of 32 over leaves of 8 (a leaf
holds its values inline and a write copies the whole leaf, so a narrow leaf keeps one record
change to a few cache lines), written in `flui-layer`
itself: no persistent-collection crate is in the workspace, and the scene needs only lookup,
insert, remove and an ordered walk, which do not justify a dependency and its review. Beside it are
three persistent indexes: links (`LayerLink` to the leader and followers that name it), texture users
(`TextureId` to the chunks that draw it) and filtered nodes (the nodes whose content holds an
effect with a read footprint, §6). Applying a patch copies the paths it touches; every other node
is shared with the version before. `SceneVersion` is `Send + Sync` by its auto traits: it holds
the layers a `SceneSnapshot` holds today, which is plain `Send + Sync` data
(`scene_snapshot.rs:155-159`; an annotation value is `Any + Send + Sync`,
`crates/flui-layer/src/layer/annotated_region.rs:129`), so no `unsafe impl` is added. A node id
is its owner's lineage and a slot key minted with `new_gen(slot, generation)`
(`crates/flui-foundation/src/id.rs:807-825`), and replaces, for every reader that keeps an id
across frames, the per-frame `LayerId` and its scene number (`layer_tree.rs:185-197`). The
lineage is part of the id because two owners mint the same slot keys (each root is slot 0 of the
first generation) and one `GpuContext` serves every window (ADR-0305), so a cache or an inspector
link keyed by a slot key alone would mix two scenes' nodes. Inside a version every reference (a
node's key and parent, the table keys, the three indexes) is the bare `NodeKey`, since one version
has one lineage; the public id is assembled from the version's lineage where the API hands one
out, and `SceneVersion::node` answers `None` for an id of another lineage. A slot entry keeps the
whole `SceneNodeId`, because an `Entry`'s size is set by its `Layer` variant and the id lets
`apply` refuse a slot of another owner.

§2. **Content belongs to one boundary, and a chunk to one render object.**

```rust
pub struct BoundaryContent {
    entries: Box<[Entry]>,
    bounds: Bounds,
    filters: Box<[FilterFootprint]>,                       // §6, in the content's own space
}
pub enum Entry {                                           // one paint-order stream (ADR-0253 §1)
    Effect { layer: Layer, owner: RenderId, ordinal: u32, parent: Option<EntryIndex> },
    Chunk { chunk: Arc<Chunk>, parent: Option<EntryIndex> },
    Slot { node: SceneNodeId, parent: Option<EntryIndex> },
}
pub struct Chunk {
    owner: RenderId,
    ordinal: u32,
    version: ContentVersion,
    commands: DisplayList,                                 // representation: ADR-0306
    bounds: Bounds,
    textures: SmallVec<[TextureId; 1]>,
    reads_destination: bool,
}
pub struct ContentVersion { lineage: Lineage, seq: NonZeroU64 }
pub enum Bounds { Finite(Rect<f64>), Unbounded }
```

A chunk has one parent entry, so it never straddles an effect scope, and one owner, so a render
object that repaints changes only its own chunks: the composer seals the open run whenever the
painting render object changes, which replaces the merge at `paint.rs:1500-1503`. An effect names
the render object that pushed it, which is the follower correlation a capture lacks today
(`paint.rs:1603-1609`), so a boundary holding a link layer is retained like any other. A slot names
its child node, so attaching, detaching or moving a boundary between parents is a new publication
of the parents' content, which their paint already produces. Bounds that are not finite are
`Bounds::Unbounded`.

A chunk keeps its `ContentVersion` while its render object owes no paint work (it is absent from
the paint queue, ADR-0062) and its recording inputs, the device pixel ratio and its origin in the
boundary, are unchanged; otherwise it gets the next one. A composited-layer update republishes the
boundary's content with the new effect and the same chunks. `seq` comes from the scene owner's
`IdMint<NonZeroU64>` (`crates/flui-foundation/src/sequence.rs:118-186`), which never wraps, and
`SceneVersionId` draws from the same mint. `Lineage(NonZeroU64)` comes from a non-`pub`
`static AtomicIdMint<NonZeroU64>`, because every UI thread builds scene owners; it never wraps,
refuses with `IdExhausted` once spent, and is a `counter` grant in the `globals` of `flui-layer`.
A new scene owner takes a new lineage, so a restarted `seq` never equals an old one, which is what
`ContentToken` was an allocation for (`stamp.rs:23-30`); `ContentToken` and `BoundaryStamp` are
deleted. `renew` keeps the owner's mint running, so within one owner a `seq`, which an inspection
handle names its scene by, is issued once; only a spent mint starts over, under the new lineage.

§3. **Paint writes a patch of typed operations, applied all or nothing.**

```rust
pub enum SceneOp {
    Publish { node: SceneNodeId, content: Arc<BoundaryContent> },
    Place { node: SceneNodeId, placement: Placement },
    Retire { node: SceneNodeId },
}
impl Placement {
    pub fn try_new(offset: Offset) -> Result<Placement, NonFinite>;
}
pub enum SceneError {
    ForeignNode(SceneNodeId), UnknownNode(SceneNodeId), StaleNode(SceneNodeId),
    RetiredInUse(SceneNodeId),
    TwoParents(SceneNodeId), OwnAncestor(SceneNodeId), DepthOverflow(SceneNodeId),
    VersionsExhausted,
}
impl SceneOwner {
    pub fn writer(&mut self) -> SceneWriter<'_>;           // clears the reused operation buffer
    pub fn apply(&mut self, surface: SurfaceKey) -> Result<Applied, SceneError>;
}
pub struct Applied { pub id: SceneVersionId, pub released: ReleasedContents }
impl SceneWriter<'_> {
    pub fn mint_node(&mut self) -> SceneNodeId;
    pub fn next_content_version(&mut self) -> Result<ContentVersion, SceneError>;
    pub fn push(&mut self, op: SceneOp);
    pub fn current(&self) -> &SceneVersion;
    // a composited-layer update: `content`'s stream shared, the named effects replaced
    pub fn with_effects(&mut self, content: &BoundaryContent,
        replaced: impl IntoIterator<Item = (EntryIndex, Layer)>)
        -> Result<Arc<BoundaryContent>, EntryIndex>;
}
```

`Publish` on an id the owner minted and the version does not hold creates the node, unattached
until a parent's slot names it. `Place` moves a node in its parent's slot space without
republishing either content. `Retire` frees a node no slot names. `mint_node` advances the slot's
generation when it issues the id, so an id from a patch that is never applied is stale forever; a
slot whose generation would overflow is retired, and running out of the 2³² slots panics with
`BUG:`, as `scenes_painted` does for its `u64` (`paint.rs:372-377`). `apply` consumes the
operations the writer buffered. It validates each of them against the version and the rest of
the patch before it builds anything, and refuses an id of another lineage (before its slot key is
read), an unknown or stale id, a retired node a slot still names, a node named by two slots, a
slot that names its own ancestor (found by walking up from the parent, at most as many nodes as
the version holds) and a depth past `u32`. `apply` runs no code the scene does not own and hashes
nothing: its working sets are slot-indexed, kept by the owner and cleared per patch. A republished
content costs the slots it gains or loses, since a child named before and after keeps its parent
unwritten. On `Ok` it hands back, beside the new id, the version it replaced and the contents a
fold of the pending log let go of (§7), to drop once the owner is free. On `Err` the version and
the pending log are as they were, and the node ids and content
versions minted for the refused patch stay spent.

A node stores its parent and depth, so the common ancestor of a leader and a follower is found by
depth in O(depth), not by id order (`layer_tree.rs:305-326`). A node attached under a new parent has
the depths below it recomputed, counted as `scene_depth_updates`. The indexes change on a
`Publish` whose link, texture or filter entries differ from the old content's (the old ones leave,
the new ones enter) and on `Retire`, and on nothing else; an unattached node's links resolve to nothing. Two leaders on one link resolve to the
one indexed last, and a debug build asserts, as `LayerTree::push` does today
(`layer_tree.rs:226-234`). `flui_layer::resolve_follower_transform` takes the version and the
follower's address (node and entry) and keeps ADR-0276's rule.

A later operation that writes one effect property without republishing content is a new `SceneOp`
variant: §4 already stores the subtree bounds and §6 the footprints its damage needs.

§4. **Damage is the footprint of the applied patch.** `apply` computes, in device pixels under the
version before and after it:

- Only nodes the root reaches contribute; a change to an unattached node damages nothing.
- `Publish` pairs old and new entries by key: a chunk by its `(owner, ordinal)`, an effect by its
  own `(owner, ordinal)`, a slot by its node. A chunk with a new version, or an effect whose layer
  changed, damages its old and new bounds; an unpaired entry damages its bounds; kept entries whose
  paint order changed damage both positions, by the longest-increasing-subsequence rule the differ
  uses (`differ.rs:532-566`); a slot under a changed effect chain damages its child's subtree
  footprint under the old and the new chain.
- `Place` damages the node's subtree footprint at the old and the new placement.
- `Retire` damages nothing, since only a node no slot names can be retired.
- After the operations every follower is resolved again (O(followers × depth), counted as
  `followers_resolved`); one whose transform changed damages its subtree footprint under the old and
  the new transform, and so does one under `LeaderClip::Hide` whose leader's branch the patch
  changed, or whose chains pass through such a follower. A follower is no longer damaged every
  frame. The followers of a republished
  boundary pair with its old ones in one pass over each side's links (`follower_pairing_reads`).

A subtree's footprint is its `subtree_bounds` mapped through the chain to the surface, grown by
its kernel and rounded outward (ADR-0098 §6). A node's `subtree_bounds`, in its own space, covers
its content's bounds and each child's `subtree_bounds` mapped through the child's placement and the
effects above its slot. Its kernel is how far, in device pixels, the filters in the subtree spread
past those bounds once mapped: the renderer takes a blur or morphology kernel in physical pixels
whatever the transform, so a bound in the boundary's space, which a shrinking ancestor shrinks,
cannot hold it; a magnifier scales the kernel it takes, and a transform filter that is not a uniform
scale has no kernel bound inside its boundary. A filter adds its own kernel to the maximum of its
children's, so kernels sum along a chain and take the maximum across siblings; inside a boundary a
filter moves what it takes and leaves its spread to the kernel, and a clip over a scope with a kernel
bounds it by the clip, its kernel spent. `Publish`
recomputes both exactly from the children (O(children)); every other change grows the ancestors'
bounds by union and their kernel by maximum
up to the root (O(depth)) and never shrinks them, so a footprint can over-report and never
under-report. The engine culls with the same bounds. A chain, placement or
bound that is not finite makes the frame `Full(NonFinite)`; a singular chain draws nothing and
contributes only its old footprint.

§5. **The damage type.**

```rust
pub enum DamageRegion { Unchanged, Region(Region), Full(FullReason) }      // exhaustive
#[derive(Clone, Copy)]
pub struct Region { rects: [DamageRect; Region::MAX], len: NonZeroU8 }     // MAX = 8
pub enum FullReason {
    FirstFrame, SurfaceChanged, AboveThreshold, Unbounded, NonFinite, DamageOff,
    Resync(ResyncReason),
}
pub enum ResyncReason { BaseMismatch, RasterReset, Rebuilt }
impl DamageRect {
    pub fn try_covering(rect: Rect<f64>, surface: (u32, u32))
        -> Result<Option<DamageRect>, NonFinite>;
}
```

The rects of a region are non-empty and pairwise disjoint. Inserting a rect merges it with every
rect it overlaps into their bounding box, and merging repeats until none overlap; a ninth disjoint
rect merges the pair whose bounding box adds the least area, under the same repetition. A region's
area is a `u64` that cannot overflow, since disjoint rects inside a surface cover at most its area;
above `full_above` of the surface (`DamageMode`, one half by default) the region becomes
`Full(AboveThreshold)`. `union` keeps the first `Full` reason. `try_covering` refuses a
non-finite rect with `NonFinite`, which becomes `Full(NonFinite)`; `Ok(None)` means empty or off
the surface, nothing else. `DamageRegion` is exhaustive, as `PresentDisposition` is (ADR-0068 §2):
the wildcard arm that turns an unknown variant into a full repaint (`raster_owner.rs:1690-1697`) is
deleted, and a new variant is a compile error in every consumer. `Region::MAX` and `full_above`
are hypotheses the perf scenario under Confirmation measures.

With damage off (`FLUI_DAMAGE=off`, `raster_lane.rs:163-183`) every publication is
`Full(DamageOff)` and the raster side records every chunk afresh instead of reading the recording
cache (§9), so the switch stays the field fallback for stale pixels it is today
(`raster_lane.rs:163-166`). The scene stays retained: it is the frame's input, not an optimisation
of it.

Every sink applies the publication's damage, the web's `DirectSink` included: it marks a full
repaint every frame today (`raster_lane.rs:550-561`), and the region policy lives at the seam, so
a backend that ignores it is the defect this section removes, not a platform choice.

§6. **One function says what an effect reads.**

```rust
pub fn read_closure(effect: &Layer, ctm: &Matrix4, content: Rect<f64>) -> Closure;
pub enum Closure { None, Bounded { reads: Rect<f64>, writes: Rect<f64> }, Unbounded }
```

`reads` is what the effect samples (for a blur, the kernel's reach as the renderer implements it,
under the norm of the CTM) and `writes` what it changes; a chunk with `reads_destination` reads and
writes its own bounds. Damage that meets an entry's `reads ∪ writes`, unless one rect of the
region already contains it, joins `reads ∪ writes`. The joins and the region's merges repeat to a
fixed point, at most 64 rounds (the cap of `frame_protocol.rs:285-299`), past which the frame is
`Full(Unbounded)`; damage that meets a `Closure::Unbounded` is `Full(Unbounded)`. A
foreground image filter is read only once the region meets its node's subtree footprint, which holds
all it writes, and then with the node's other filters in one fold of its content; a backdrop or colour
filter and a reading chunk cost their chain. The filtered-node index and each content's list of its
closure entries keep the closure at O(filtered × depth), never a pass over a node's other chunks,
plus one fold of each filtered node the damage reaches. The
scene owner's `apply` and the raster side's texture
damage (§9) call this function; the differ's rule (`differ.rs:480-507`) and the engine's
(`frame_protocol.rs:215-300`) are deleted. At the fixed point every footprint the region meets lies
inside one of its rects, so each rect can be redrawn alone (§9).

§7. **The owner publishes a version and a base.**

```rust
pub struct SceneOwner {
    lineage: Lineage,
    mint: IdMint<NonZeroU64>,
    current: SceneVersion,
    acked: Option<SceneVersionId>,
    pending: PendingLog,                                   // at most 16 entries
}
pub struct ScenePublication {
    pub base: Option<SceneVersionId>,
    pub scene: SceneVersion,
    pub damage: DamageRegion,
}
pub struct SurfaceKey { pub size: (u32, u32), pub generation: SurfaceGeneration }
impl SceneOwner {
    pub fn publication(&self) -> ScenePublication;
    pub fn completed(&mut self, rastered: Option<SceneVersionId>) -> ReleasedContents;
    pub fn renew(&mut self, reason: ResyncReason) -> ReleasedContents;
    pub fn restock(&mut self, reclaimed: Reclaimed);   // what `reclaim` kept, emptied
}
impl ReleasedContents {
    pub fn reclaim(self) -> Reclaimed;                 // runs the destructors, keeps the buffers
}
// flui-runtime; `surface_size` is unchanged
pub trait FrameSink { fn submit(&mut self, publication: ScenePublication) -> SubmitVerdict; }
// flui-engine
pub struct RasterCompletion { /* … */ pub rastered: Option<SceneVersionId> }
```

The UI runtime holds one `SceneOwner` per presentation. The pending log keeps, per applied version
not yet acknowledged, its id, its damage and the contents it replaced. A
publication's base is `acked` and its damage is the union of the log: `Full(FirstFrame)` without a
base, `Full(SurfaceChanged)` when the log spans two surface keys, since a version applied for another
key than the one before carries that damage itself. A seventeenth entry folds the two
oldest into one (the union of their damage, the newer id, the newer contents; the older contents
go back with the apply's `Applied::released`), counted as `pending_log_folds`: the
damage stays relative to the same base and only over-reports, so a long raster stall costs a larger
region, never a lost one, and never more than sixteen entries of buffers. `renew` takes a new
lineage and empties the version, the slots and the
log; every node id of the old lineage is refused as `ForeignNode`, and `renew` makes
the next publication `Full(Resync(reason))`. Raster-side state and inspector links keyed by a
node are keyed by its `SceneNodeId` (with the `ContentVersion` where a recording matters), which
carries the lineage, so no separate lineage pair is kept.

`SceneSnapshot` carries `base` and `scene: SceneVersion` (`scene_snapshot.rs:172-180`). It still
moves by value; what it shares with the owner is immutable, and its doc's "ownership transfer, not
reference counting" (`scene_snapshot.rs:155-159`) now means that the snapshot is moved and its
nodes are shared. The raster mailbox stays latest-wins (`raster_owner.rs:3-8`); a superseded
frame's damage folds into the survivor as it does now (`raster_owner.rs:791-797`), and the survivor
takes the older base.

The raster side holds, per presentation, the version whose image it last committed and the
`SurfaceKey` that image is on (`RasterBase`). A publication for another key, a surface recreated at
the same size included, is `Full(SurfaceChanged)`, and a lost device or lost GPU resources forget
both, since the image went with them. It accepts a
publication's damage when that version and the base share a lineage and `base.seq <= held.seq`,
because damage from an older base covers every change since a newer one; otherwise the frame is
`Full(Resync(BaseMismatch))`, or `Full(Resync(RasterReset))` when it holds nothing. After a
successful submission (ADR-0100) it holds the new version; a failed or unshown frame leaves the held
version as it was, so its damage stays owed through the base, not through an accumulator a reset can
clear. `RasterCompletion::rastered` reports the held version through the reliable coalesced slot
(`raster_owner.rs:88-109`, `:147`).

`completed` moves `acked` forward only, ignores an id of another lineage, drops the log entries at
or below `acked`, and hands back the contents they replaced as `ReleasedContents`, which a caller
holding the owner borrowed passes to an `Outbox` (ADR-0287), so their destructors (an annotation's
value is user code) run once the owner is free. `apply` keeps the version it replaces among the
two it retires and hands each back the same way once no other holder (the raster side, a
pipeline, a probe) reads it, or when a newer one pushes it out; `renew` hands back its version and
every retiring one. The caller frees what it is handed with `ReleasedContents::reclaim`, which runs
those destructors and keeps, emptied, the version records, table nodes and contents the owner held
alone; `SceneOwner::restock` takes them back, the next apply copies a shared table node into an
emptied one, and a composited-layer update (`SceneWriter::with_effects`) writes into an emptied
content, so a frame that moves a layer allocates nothing in the scene. A recording buffer the
owner holds alone goes back to the recorder's pool (`Arc::try_unwrap`), any other is dropped.
Buffers come back with the completion while only the `Copy` id crosses up (ADR-0045 §1). A refused
submit changes nothing: the next publication's damage still starts at `acked`.

A frame that was not shown (`PresentDisposition::NotShown`, ADR-0068) is retried by publishing the
version the owner already holds: `acked` did not move, so the publication's damage is still every
change since the last shown frame, and the raster side finds every chunk in its recording cache.
The retry re-records nothing and needs no repaint, so `mark_needs_full_repaint_for` leaves
ADR-0068 §3's retry; the wake half (`wake_frame()`) stays, because the retry pump still needs a
frame to run. A surface reconfigured in between makes that publication `Full(SurfaceChanged)`, as
any change of `SurfaceKey` does.

§8. **Paint starts at each owed boundary.** `PipelineOwner::run_frame` takes a `SceneWriter`. It
paints the boundaries in the paint queue, deepest first (the order `paint.rs:127` already sorts),
so a child's node exists before its parent's slot names it, and no other: a clean boundary emits
nothing, and a clean boundary whose position changed emits `Place`. The owner keeps per boundary
a `BoundaryRecord { node, content: Arc<BoundaryContent>, origin, offset_override, parent }`, the
values ADR-0253 §2's replay carried down. A boundary painted alone receives the paint anchor a walk
from the root would have given it by walking its records' `parent` chain, O(depth) per painted
boundary: an ancestor that moved without repainting changed only its own record, so the derived
anchor cannot go stale, where a stored anchor per record would be a second copy to invalidate on
every `Place`. A disposed boundary's node is retired. The runtime applies
the patch where it now freezes the per-frame tree
(`crates/flui-runtime/src/ui_runtime/frame.rs:625-639`), with its own node for the performance
overlay, republished when the overlay's numbers change; the overlay-style probe, input regions,
leader watches (`paint.rs:370`) and follower hit-test placement (`paint.rs:326-352`) read the
applied version. A device-pixel-ratio change re-records every chunk,
as the captures are dropped today (`paint.rs:116-120`), and changes the root placement:
`Full(SurfaceChanged)`.

§9. **The raster side retains recordings and redraws a region rect by rect.** The engine walks the
held version and culls nodes by their subtree footprint. For a `Region` it plans:

```rust
enum RegionPlan { Bounds(DamageRect), Rects(Region) }
```

`Bounds` clears and redraws what meets the region's bounding box under one scissor; `Rects` clears
and redraws each rect under its own scissor, culling against that rect, at most `Region::MAX`
passes, counted as `region_passes`. `Bounds` is planned for one rect, or when the bounding box is at
most twice the region's area (a hypothesis to measure). Both keep one invariant: no pixel outside
the plan's rects is written, and every chunk that meets a written pixel is drawn there in paint
order. One scissor around the region with culling per rect breaks it: the pixels between the rects
are cleared and not redrawn, or drawn and blended twice.

A chunk's IR outlives the frame in a recording cache:

```rust
struct RecordingKey {
    content: ContentVersion,
    raster: RasterKey,
    ambient: AmbientKey,
    epoch: ResourceEpoch,
}
```

`RasterKey` is the canonical bits (ADR-0098 §4) of the linear part of the chunk's device transform
and of the fractional part of its translation; the integer part is applied at replay, so a chunk
moved by whole pixels replays without recording. `AmbientKey` is whatever the IR bakes from outside
the chunk, in the chunk's own pixel frame, which today is each batch's scissor and SDF clip
(`crates/flui-engine/src/command_ir.rs:479-498`, `:560-572`): a chunk scrolled under a fixed clip
misses until the representation applies that state at replay, which ADR-0306 decides. A chunk
under a projective transform is not cached. `ResourceEpoch` advances on a glyph-atlas reset or
eviction, device recovery, a font-source switch and a surface-format change. An entry is inserted only after its recording completed; eviction is
least recently rastered first under a byte budget (a hypothesis), and a presentation's entries go
when its slot detaches. Counters: `recording_cache_bytes`, `recording_cache_hits`,
`recording_cache_evictions`. A chunk has one parent entry, so it never straddles an effect scope,
and the renderer's straddle self-heal (`crates/flui-engine/src/renderer.rs:2654-2675`) becomes a
debug assertion that the producer kept that rule.

A texture frame drained with a publication (`raster_lane.rs:350-358`) damages the device
footprints of that texture's users in the held version, closed under §6; a texture no chunk draws
damages nothing. A platform view is damaged on every frame it is drawn, because the platform gives
no signal when its content changes; a per-view frame signal that damages it the way a texture
frame does is a follow-up, not part of this decision. A canvas layer's display
list and the performance overlay are versioned content, and no longer damaged every frame.

§10. **Failure paths.**

| Point | What holds after it |
|-------|---------------------|
| A panic in one boundary's paint | The pass commits nothing (ADR-0062): no patch is applied, the version and log are unchanged, the paint queue keeps the boundary, and the next publication's damage still starts at `acked`. Ids and content versions minted for the failed pass stay spent and are never reissued. |
| A refused patch (`SceneError`) | A defect in the producer: a debug build panics with `BUG:`. A release build calls `renew(Rebuilt)`, the pipeline owner drops its boundary records and marks the root for a full paint, the frame is not presented, and a frame is requested; that frame publishes `Full(Resync(Rebuilt))`. A second refusal in a row is a defect the rebuild did not cure: that frame fails without asking for a retry, and the next frame waits for a change (`a_second_refusal_in_a_row_asks_for_no_retry`). |
| `seq` exhausted | `IdMint` refuses with `IdExhausted` (it never wraps), `apply` returns `VersionsExhausted`, and the refused-patch path renews the lineage, so `seq` starts over where no old version can equal a new one; a renewal with sequence left runs it on. |
| Owner retirement | Published versions stay valid on the raster side until the presentation's slot detaches; a new owner has a new lineage, so its first publication is `Full(FirstFrame)` and no recording of the old lineage is replayed for it. |
| A hot reload (`DevReloadHook`) | Code changed under the recordings and content versions, so the reload calls `renew(Rebuilt)` on every presentation's scene owner: the next frame is `Full(Resync(Rebuilt))` and no recording made by the old code is replayed. |
| A raster-side panic or device loss | The raster side holds no version for that presentation and drops its recordings; the next frame is `Full(Resync(RasterReset))`. The owner's `acked` does not move, so nothing it owes is dropped. ADR-0305 says where the panic is contained. |
| A base mismatch | `Full(Resync(BaseMismatch))`. |
| A lost or coalesced completion | `acked` lags; the log folds and the damage over-reports; nothing is lost, and a completion older than `acked` is ignored. |
| Arithmetic | Depth is checked (`DepthOverflow`); a region's area is bounded by the surface; a generation that would overflow retires its slot. |
| A non-finite rect, placement or chain | `Placement::try_new` refuses a non-finite offset at the producer; a non-finite footprint is `Full(NonFinite)`, never `Unchanged`. |
| User code | `paint_raw` runs before any operation exists, and `apply` runs none. The contents a version replaced (an annotation's `Drop` is user code) are released after `completed` has committed the owner's state, as ADR-0287 §1 releases a replaced value; on the raster side a version is dropped where a `Scene` is dropped now. Tracing events are emitted after the state commits. |

§11. **The full diff stays as the oracle.** `LayerDiffer` moves behind `flui-layer`'s `testing`
feature and compares `SceneVersion::flatten()` (a `LayerTree` built from a version, test-only) of
consecutive versions. The patch damage must cover the diff damage.

### Consequences

- Good, because a frame costs O(patch) in paint, damage and recording, plus O(followers × depth)
  for follower resolution and O(filtered × depth) for the closure; nothing walks the whole scene.
- Good, because two distant changes repaint two rects, and a boundary holding a follower, a canvas
  layer and the performance overlay stop being repainted or damaged every frame.
- Good, because the pixels redrawn, the damage reported and the filter reads come from one function.
- Good, because owed damage survives superseded, refused, failed and unshown frames through the
  base, by construction.
- Bad, because the scene is a persistent structure: path copying, atomic reference counts shared by
  the owner and raster threads, and a table `flui-layer` has to write and test itself.
- Bad, because the contract spans `flui-layer`, `flui-rendering`, `flui-runtime`, `flui-app` and
  `flui-engine` and changes as one.
- Bad, because grow-only bounds and a folded log over-report, which costs pixels, and the recording
  cache costs memory under a budget still to measure.
- Good, because a retry after an unshown frame republishes the held version and re-records
  nothing (§7), where ADR-0068 §3 repainted the whole tree.
- Neutral, because the inspector's layer link (`Link::Layer`,
  `crates/flui-layer/src/inspect.rs:144-152`) compares scene numbers today and names a node by
  its `SceneNodeId` instead, whose lineage makes a link into an old lineage resolve to nothing.

### Confirmation

- Compiler: `DamageRegion`, `FullReason` and `ResyncReason` are exhaustive and matched without a
  wildcard; `SceneOp` carries `SceneNodeId`, never an index; a placement exists only through
  `Placement::try_new`.
- Gates: `cargo xtask globals` passes with the lineage mint under its `counter` grant;
  `cargo xtask send-impls` lists no new `unsafe impl`; no author-written `Send` or `Sync` bound is
  added, so `cargo xtask send-positions` is unchanged.
- `flui-layer` (`tests/main.rs` modules, to be added): the `scene_apply_contract` table
  (`a_slot_naming_its_own_ancestor_is_refused`, `a_stale_generation_is_refused`,
  `a_node_of_another_owner_is_refused`,
  `a_node_named_by_two_slots_is_refused`, `a_refused_patch_leaves_the_version_and_log_unchanged`,
  `a_place_damages_the_subtree_under_both_placements`,
  `an_effect_chain_change_damages_every_slot_under_it`,
  `a_follower_resolves_after_its_leaders_boundary_moves_parents`); the `region_contract` table
  (`a_ninth_rect_merges_the_cheapest_pair`, `merged_rects_stay_pairwise_disjoint`,
  `a_non_finite_rect_escalates_to_full`, `full_keeps_its_first_reason`); the
  `content_version_contract` table (`a_replaced_owner_never_reports_unchanged`,
  `an_exhausted_mint_renews_the_lineage_and_publishes_full`, through `IdMint::starting_after`); the
  `pending_log_contract` table (`a_full_log_folds_its_two_oldest_entries`,
  `damage_from_an_older_base_covers_the_newer_one`, `a_base_from_another_lineage_renders_full`,
  `a_lost_completion_over_reports_and_never_under_reports`,
  `a_surface_change_in_the_log_publishes_full`); and the property test
  `patch_damage_covers_diff_damage`, which runs random trees, marks, moves and followers against the
  §11 oracle and fails on a planted patch that drops one operation's footprint. The existing
  `union_table` and `bounds_round_outward_with_aa_margin` (`scene_snapshot.rs:213`, `:227`) are
  rewritten for `Region`.
- `flui-rendering` (to be added): `a_clean_boundary_emits_no_operation`,
  `a_paint_panic_applies_no_patch_and_keeps_the_presented_version`,
  `a_boundary_holding_a_leader_is_retained`,
  `a_boundary_painted_alone_reads_the_anchor_its_parent_gave_it`. The existing
  `retained_frames_match_a_full_repaint_over_random_trees_and_marks`
  (`crates/flui-rendering/tests/nested_boundary_capture.rs:549`) compares flattened versions, and
  `pipeline_frames_diff_to_the_changed_boundary_rect`
  (`crates/flui-rendering/tests/boundary_contents.rs:251`) asserts the patch damage.
- `flui-engine` (to be added): `a_cached_chunk_replays_the_command_stream_it_recorded` (ADR-0006
  §6), `a_chunk_moved_by_whole_pixels_hits_the_recording_cache`,
  `an_atlas_reset_misses_every_cached_recording`, `the_renderer_kernel_stays_inside_read_closure`,
  `a_texture_frame_damages_only_its_users`, `a_base_mismatch_renders_full`,
  `a_raster_panic_renders_the_next_frame_in_full`, and the readback row
  `region_frames_match_a_full_frame`, whose two damaged rects sit in opposite corners with
  translucent content between them, so a plan that clears the gap without redrawing it, or blends
  it twice, fails. The existing
  `superseded_damage_folds_into_the_survivor` (`raster_owner.rs:2324`) and
  `a_moved_box_repaints_its_old_and_new_positions_only`
  (`crates/flui-engine/src/damage_readback_tests.rs:209`) stay.
- `flui-runtime` (to be added): `a_refused_submit_keeps_its_damage_owed`,
  `a_refused_patch_rebuilds_under_a_new_lineage`,
  `a_frame_not_shown_is_retried_from_the_held_version` (the retry records no chunk and paints no
  boundary) and `a_hot_reload_renews_every_scene_lineage`.
- `cargo xtask perf` gains the scenario `caret_blink_in_10k_list` (to be added to
  `crates/flui-widgets/tests/perf.rs`, counts in `crates/flui-widgets/perf/baseline/`), whose
  `nodes_painted`, `scene_ops` and `chunks_recorded` per blink are equal at 1,000 and 10,000
  items.

## Pros and cons of the options

### 1. A new tree each frame, a full diff and a multi-rect region

- Good, because it changes only the region type and the engine's scissor loop.
- Bad, because paint still re-composes O(layers) every frame (ADR-0253 §6), the differ still walks
  every node, and the engine still records the whole scene.
- Bad, because followers, textures and canvas layers are still damaged every frame, and the two
  footprint rules stay.

### 2. Raster-side subtrees grafted by command

- Good, because the raster side keeps recordings per boundary and the owner sends only what changed.
- Bad, because the structure lives on the raster side, so the owner cannot compute the footprint of
  a move or a follower without a mirror of it, a second copy of state to keep in sync.
- Bad, because a graft in a frame the latest-wins mailbox supersedes is lost unless commands queue,
  which turns the slot into an unbounded queue or needs a resynchronisation protocol of its own.
- Bad, because the raster side would mint and check the identity of owner-side objects.

### 3. A retained scene changed by typed operations (chosen)

- Good, because paint, damage and recording are each O(patch), and the raster side needs only the
  version and its base.
- Good, because footprints are computed where the structure is, before the hand-over, and the full
  diff stays as an oracle for them.
- Bad, because it adds a persistent structure and its invariants (parent, depth, links, bounds) to
  `flui-layer`, and every producer of the scene changes.

### 4. No retention

- Good, because nothing retained can go stale, and the code is the smallest.
- Bad, because every frame costs O(scene): `damage_scissor` measured 2901 µs for a full frame of 64
  layers against 56 µs for a damaged one (ADR-0061, "What it is worth").

## More information

- The measurements cited are `flui-engine`'s `damage_scissor` bench (ADR-0061) and `flui-layer`'s
  `damage_diff` bench (`crates/flui-layer/ARCHITECTURE.md:172-173`).
- Revisit `Region::MAX`, the `Bounds` plan's factor of two, the pending log's 16 entries and the
  recording cache's budget when the `caret_blink_in_10k_list` counts or a readback bench show
  another value wins; each is a constant, not a contract.
