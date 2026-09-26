# Codebase map: rendering_objects_layer — flui-rendering (protocol + PipelineOwner), flui-objects (catalog), flui-layer (layer vocabulary)

_Raw output of the `map:rendering_objects_layer` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

How it works today (main @ cab06137d). flui-rendering (~55.8k LOC, layer 4) owns the render-object protocol and the whole per-realm frame machine. Render objects implement `RenderBox` or `RenderSliver`, typed by `Arity` and `ParentData`. Blanket impls erase them into `RenderObject<P>` for the sealed `Protocol` pair Box/Sliver (protocol/protocol.rs:54). Nodes are stored by value in a `Slab<RenderNode>` (`enum RenderNode { Box, Sliver }`, storage/node.rs:52) keyed by a generational `RenderId` (flui-foundation id.rs:723). `PipelineOwner<Phase>` is a typestate machine (Idle→Layout→Compositing→Paint→Semantics), shared per presentation through a `!Send` `PipelineCell(Rc<RefCell<_>>)` (owner/cell.rs).

Layout walks a dirty root through an unsafe `SubtreeArena`. For every pass the arena gathers every node id in the dirty subtree and builds a `HashMap<RenderId, NodePtr>` (subtree_arena.rs:161,523-543). A layout, paint or layer-update panic is caught with `catch_unwind` and turned into poisoning. A placed-generation stamp keeps unlaid children out of paint, hit-test and semantics, and keep-alive depends on it (ADR-0056). Build-during-layout (LayoutBuilder, lazy slivers, persistent headers) is not re-entrant. The render object writes into a mailbox cell, or asks for children with `request_child_build`. The binding in flui-view then runs a bounded layout↔build fixpoint: at most 10 passes, and at most 6 for lazy bands (flui-view owner/layout_builder.rs:64-74, ADR-0017/0003).

Paint records `PaintCx` fragments into a fresh `LayerTree` every frame. Clean repaint boundaries are grafted from CPU-only `retained_boundaries` captures (owner/mod.rs:78). Alpha and clip changes patch the enclosing capture instead of promoting the node to a boundary. Semantics assembly also lives here.

flui-objects (~38.3k LOC, also layer 4) holds 84 concrete objects plus a 15.9k-line harness file (424 `harness_*` tests; RENDER_OBJECT_TYPES is a test-local string list). flui-layer (~5k LOC, layer 3) is a closed 19-variant `Layer` enum in an append-only per-frame `LayerTree`, frozen into `Scene`/`SceneSnapshot` for the raster lane. Damage exists only as `DamageRegion::Full`, so every frame is a full repaint.

Verdict. The internal mechanics are unusually well engineered and tested: typestate phases, poisoning, the stamp, the capture patching, and a sum-tree virtualizer. The weak part is the boundary. The protocol crate does not own its own topology: flui-view edits parent/child links directly and runs whole-tree scans. Catalog knowledge leaks down into the protocol and up into the spine. The public surface is about 1,400 items with no tier. The layer vocabulary is closed. The H0 promise of "a stable public protocol for third-party render objects and layers" therefore holds only for leaf and proxy boxes written against the curated `flui::rendering` facade module. Custom viewports, lazy slivers, layers or GPU content either require the raw crate or are impossible. The H2 items (damage, layer caching, 100k rows) run into two structural costs: layout cost grows with subtree size on every fixpoint pass, and no layer identity or diff producer exists yet.

## Responsibilities and boundaries

What it owns, as intended:
- **flui-rendering** is the protocol: traits, constraints, parent data, contexts, the pipeline and its phases, the retained paint captures, semantics assembly, and the hit-test walk.
- **flui-objects** is the first-party catalog.
- **flui-layer** is the output vocabulary handed to flui-engine.
- flui-engine depends only on types, painting, foundation and layer (verified), so the backend is cleanly decoupled from the render tree. That is a good boundary.

Where the boundaries leak:
1. **The spine edits the render tree directly.** `PipelineOwner::render_tree_mut` (owner/accessors.rs:348) is public, and so are the `RenderNode` mutators `set_parent`, `set_depth`, `add_child`, `insert_child`, `remove_child` and `set_parent_data` (storage/node.rs:297-343,631). flui-view calls `render_tree_mut()` at 26 non-test sites. For example, element_tree.rs:1402-1415 rewires children by hand, and element_tree.rs:1486-1487 collects every render id in the tree. So render-tree topology invariants (depth, arity, dirty queues, eviction of retained captures) are maintained by a different crate.
2. **Catalog knowledge sits in the protocol crate:**
   - the semantics pass downcasts to `SliverMultiBoxAdaptorParentData` (owner/semantics.rs:1003-1008);
   - `context/intrinsics.rs:13,316` special-cases `FlexParentData`;
   - the parent-data catalog (Flex/Stack/Table/Wrap/ListWheel/TreeSliver/Text), the grid delegate, the flow/custom-layout delegates, and `ScrollPosition` with a flui-scheduler `PostFrameHandle` (view/scroll_position.rs:28) all live in flui-rendering.
3. **Protocol-level seams sit in the catalog, and the spine depends on the catalog in production:**
   - `LayoutConstraintsCell`, `HeaderShrinkCell` and `BuildDuringLayoutCell` live in flui-objects. Their module doc says they are "public so flui-view and flui-objects can share the same cell" (layout_constraints_cell.rs:31-35).
   - flui-view imports `RenderSliverList`, `RenderSliverFixedExtentList`, `RenderSliverGrid` and `RenderErrorBox` in production code (element/sliver_adaptor.rs:58, view/error.rs).
   - Because of the orphan rule, the `LazyMultiBoxRender` impls for catalog types must live in the spine.

What belongs elsewhere:
- The build-during-layout cells and the lazy-child contract belong in flui-rendering, next to `request_child_build`.
- Catalog parent data, delegates and `ScrollPosition` belong in flui-objects or the widgets layer.
- Topology edits belong behind a small owner API (`attach_child`, `move_subtree`, `set_children`) that enforces arity and depth.
- Test seeding (`ParentDataSeed`, `semantics_error_once_for_test`) belongs outside production types.

## Key types and contracts

- `trait RenderBox: RenderObject<BoxProtocol> + Diagnosticable { type Arity; type ParentData; fn perform_layout(&mut self, &mut BoxLayoutContext<'_, Arity, ParentData>) -> Size; fn paint(&self, &mut PaintCx<'_, Arity>); ... }`, plus about 30 defaulted hooks: intrinsics, dry layout, baselines, `paint_effects`, `pointer_target`/`scroll_target`/`pan_zoom_target`, `mouse_cursor`, `metadata`, semantics, `attach(RenderInvalidationHandle)` (traits/render_box.rs:83-606).
- `trait RenderObject<P: Protocol>`: the erased form with `perform_layout_raw`, `paint_raw`, `hit_test_raw` and `intrinsic_raw`. It is filled by blanket impls for `RenderBox`/`RenderSliver` (render_box.rs:651, render_sliver.rs:539). It is not sealed; `Protocol` is sealed (protocol.rs:54), so the only protocols are Box and Sliver.
- `PipelineOwner<Phase: PipelinePhase = Idle>`: typestate phases, where `run_paint` cannot be named on `Idle`. It holds `RenderTree`, the dirty tracker, `LayoutPoison`, `retained_boundaries: FxHashMap<RenderId, RetainedSubtree>`, `last_layer_tree`, follower side tables, and pending child-request and retain-band sinks (owner/mod.rs:116-260).
- `PipelineCell(Rc<RefCell<PipelineOwner>>)`: `!Send`, closure-scoped (owner/cell.rs).
- `RenderInvalidationHandle`: a weak, generational, least-privilege dirty-marking handle over a bounded crossbeam channel of capacity 256 (pipeline/handle.rs). ADR-0013.
- `RenderUpdateImpact`: the phases a View→render update invalidates, applied once by the owner (ADR-0046).
- The placed-generation stamp (`layout_generation` and `placed_by` identity): the pipeline-enforced 'laid out this pass' gate for paint, hit-test and semantics. Keep-alive depends on it (ARCHITECTURE.md 'A layout stamps the children it laid out'; ADR-0056).
- Build-during-layout contract: the render object publishes into `LayoutConstraintsCell`/`HeaderShrinkCell` or calls `SliverLayoutContext::request_child_build(index)`; the binding services the fixpoint (ADR-0017, ADR-0003, flui-view owner/layout_builder.rs).
- `RenderError::Poisoned { render_object, phase: PoisonPhase::{Layout, Paint, LayerUpdate} }`; phases return `RenderResult<()>`.
- `enum Layer`: closed, 19 variants, deliberately not `#[non_exhaustive]`; the engine match is exhaustive (flui-layer ARCHITECTURE decision 1). `LayerTree::push_child` is append-only; `LayerNode::render_id` gives cross-frame identity for boundaries only.
- `SceneSnapshot { stamp, damage: DamageRegion, scene }`: `Send + Sync` handoff to the raster lane. `DamageRegion::Full` is the only variant (scene_snapshot.rs:12-18).
- `flui::rendering` (src/rendering.rs): the curated facade authoring module for render objects: traits, contexts, parent data, `RenderInvalidationHandle`, the `forward_single_child_*` macros and arity types. `flui::testing::rendering` exposes `RenderTester`/`Probe`.
- `flui_rendering::virtualization`: a protocol-agnostic sum-tree `Virtualizer` with `ItemExtent::{Unmeasured, Measured}` and item-identity anchor correction (ADR-0003).

## Dependencies

**Inbound (production).**
- flui-rendering depends on flui-types, flui-painting, flui-interaction, flui-foundation, flui-tree, flui-layer, flui-semantics and flui-scheduler. The scheduler edge exists only because `ScrollPosition` names `PostFrameHandle`.
- Its external crates are parking_lot, crossbeam-channel, slab, stacker, downcast-rs, dyn-clone and smallvec.
- flui-objects depends on flui-rendering, flui-painting, flui-types, flui-foundation, flui-tree and flui-animation, plus parking_lot.
- flui-layer depends on flui-foundation, flui-tree, flui-types and flui-painting.

**Outbound.**
- flui-view (layer 5) depends on both flui-rendering and flui-objects in production.
- flui-widgets, flui-app, flui-hot-reload and flui-engine use flui-layer.
- flui-app implements `RendererBinding` (bindings/renderer_binding.rs:550) and builds `SceneSnapshot` with `DamageRegion::Full` (app/raster_lane.rs:354).
- The facade re-exports a curated subset as `flui::rendering`.

**Dev and feature edges.**
- flui-rendering has a dev-dependency on flui-objects (the tests import the catalog).
- The `testing` feature of flui-rendering and flui-layer is enabled by flui-objects, view, widgets, app, material, cupertino and flui-testing (as dev-dependencies), and by the facade's `testing` feature (Cargo.toml:592). Under feature unification, those dependents' test builds compile the testing-augmented `PipelineOwner`.

**Layering.**
- flui-rendering, flui-objects and flui-engine all declare layer 4. Protocol → catalog → backend ordering inside that layer is therefore not checked by `cargo xtask workspace`.

## Fit with the plan

**H0 extension point: "custom render objects and layers with harness tests for third-party catalogs".**
- Partially met for boxes. The curated `flui::rendering` module exists, and `tests/facade_consumer.rs` builds an out-of-workspace crate from `tests/fixtures/facade_extensions.rs` that implements `RenderBox`, mounts it through a `RenderView` and drives `RenderTester`. That is a real proof.
- Not met for:
  - **slivers:** the fixture has zero `RenderSliver` impls;
  - **viewports and lazy lists:** they need `ViewportOffset`, `ScrollPosition`, `LayerLink` and `virtualization`, which are absent from the facade, and they need the spine-side `LazyMultiBoxRender` glue;
  - **layers:** the enum is closed by design;
  - **GPU content:** `Texture` and `PlatformView` have no producer;
  - **conformance testing:** there is no reusable suite; the "harness" is a test-local string list.

**H1 (external GPU content, WebView/video compositor spike).**
- Blocked on a producer for `Texture`/`PlatformView` and on a decision about the closed enum.

**H2 (damage, layer caching, 100k rows, raster/IO lanes).**
- The raster-lane handoff (`SceneSnapshot`) and the boundary identity (`render_id` on `LayerNode`, generational `RenderId`) are good groundwork.
- Four things block it:
  - no layer-diff producer exists (ADR-0061 accepted, unimplemented);
  - captures are CPU-only, with no GPU raster cache;
  - the per-pass O(subtree) arena construction is multiplied by up to 6-10 fixpoint passes;
  - the spine's whole-tree scans (element_tree.rs:1487) remain. These are exactly the roadmap's E1/E4 items (#1037, #1039, #1041), and they are structural, not tuning.

**H3 (API freeze by tier).**
- About 1,421 `pub` items in flui-rendering, 869 in flui-objects and 268 in flui-layer, with storage, pipeline internals and topology mutators all public and no `doc(hidden)`/`unstable` split. The facade's curated module is the natural "Stable" tier, but nothing marks the rest "Experimental".
- cargo-semver-checks would pin everything.

**Delivery layers.**
- The catalog is core, which is right, but the spine's hard dependency on flui-objects prevents treating the catalog as replaceable.

**Principles.**
- Principle 3 (no globals) holds here: the per-owner `PIPELINE_ID_COUNTER` and `LayerLink::new`'s `AtomicU64` are identity-only.
- Principle 5 (proof, not claims) is violated by roadmap line 183 ("sliver protocol with nested scroll"): no `NestedScroll` exists anywhere in `crates/`.

## Strengths

- Phase ordering is a type: `run_paint` is callable only on `PipelineOwner<PaintPhase>`, pinned by compile_fail doctests and a trybuild suite (Cargo.toml `[[test]] compile_fail`). The one remaining runtime gate (`PaintBeforeLayout`) is justified in writing.
- Render objects are stored by value in a slab with a generational `RenderId` (ABA-safe caches); there is no per-node lock or `Arc<RwLock>` tree. The owner is `!Send`, and `static_assertions` pins that it stays so.
- Per-node fault isolation for layout and paint, with bounded-retry poisoning and a distinguishable degraded state (`geometry_degraded`, ADR-0054: a degraded viewport pass does not publish scroll dimensions).
- The placed-generation stamp turns a per-object discipline (Flutter's `RenderSliverMultiBoxAdaptor` only paints the children it laid out) into a pipeline property for every multi-child object. Its red-without-change tests need two frames (tests/placed_generation_gate.rs).
- Composited-layer updates patch the enclosing capture instead of promoting the node to a boundary. It is measured: 228x faster on inline content for an opacity alpha change (ARCHITECTURE.md 'A composited-layer update patches ...').
- `RenderUpdateImpact` (ADR-0046) carries delegate `should_relayout`/`should_repaint` decisions across the View→Render boundary.
- Windowing core `virtualization`: a protocol-agnostic sum tree with O(log n) seek and edit, type-level measured/estimated extents and an item-identity anchor. This is better than Flutter's `RenderSliverList`, and it is proptested against a Vec oracle.
- flui-layer is lean and honest: an append-only per-frame tree without cycles, one `local_translation` shared by the GPU walk and the follower resolver, an inline-size budget test, and an explicit 'Producers' table naming the variants nothing emits.
- The engine is decoupled from the render tree: flui-engine depends only on types, painting, foundation and layer.
- A real out-of-workspace extension proof: tests/facade_consumer.rs compiles a consumer crate against the facade (with a `flui`/`ui` alias) that implements `RenderBox` + `RenderView`, forwarding macros, and tests through `flui::testing::rendering`.
- Text is isolated behind flui-painting's `TextPainter`: flui-objects has no direct cosmic-text or FONT_SYSTEM reference, so the Parley swap (ADR-0077) should not ripple into the catalog's structure.
- Divergences from Flutter are recorded in considerable depth (Mapping decisions, each with a named replacement test that was verified red).

## Problems

### The spine performs raw render-tree topology edits; the protocol crate does not own its own tree invariants

- **Kind:** layering · **Severity:** high
- **Evidence:** `PipelineOwner::render_tree_mut` is public (crates/flui-rendering/src/pipeline/owner/accessors.rs:348). `RenderNode::{set_parent, set_depth, add_child, insert_child, remove_child, set_parent_data}` are public (storage/node.rs:297-343,631). flui-view has 26 non-test `render_tree_mut()` sites, e.g. crates/flui-view/src/tree/element_tree.rs:1402-1415 (a manual child rewire) and :1486-1487 (`render_tree.iter()` collecting every render id in the tree). Arity is not enforced at topology level: flui-rendering ARCHITECTURE.md records 'a `Single`-arity object mounted with two children' in the merge harness.
- **Impact:** Depth, arity, dirty-queue membership and retained-capture eviction are maintained by convention across two crates. Each spine change risks a stale capture or a mis-depth. It blocks E4 (local topology commits #1039, no whole-arena scans #1041, both H2), and it makes the whole storage module de-facto Stable API at H3.
- **Direction:** Give `PipelineOwner<Idle>` a small topology transaction API (`set_children(parent, &[RenderId])`, `move_subtree`, `attach_root`) that enforces arity at runtime (compile time where possible), keeps depth, marks dirty and evicts captures. Make `RenderNode`'s link mutators `pub(crate)`, remove `render_tree_mut`, and let flui-view submit per-parent diffs instead of scanning.

### Layout arena cost grows with subtree size on every pass, multiplied by the layout↔build fixpoint

- **Kind:** performance · **Severity:** high
- **Evidence:** `SubtreeArena::from_tree` calls `collect_subtree_ids` (a std HashSet over the whole subtree, storage/tree.rs:696-721) and then builds `by_id: HashMap<RenderId, (NodePtr, AtomicBool)>` over all of it (pipeline/owner/subtree_arena.rs:161,279,523-543) before any node lays out. The fixpoint re-drives layout up to `MAX_LAYOUT_BUILD_PASSES = 10` / `MAX_LAZY_BAND_PASSES = 6` (flui-view/src/owner/layout_builder.rs:64-74).
- **Impact:** Scrolling a lazy list whose viewport is the dirty root pays roughly passes × materialized-subtree SipHash inserts per frame, regardless of how few nodes actually relayout. This directly threatens the H2 exit ('100k rows', 'static 10k-row list p99 < 1 frame period') and E4/E5.
- **Direction:** Make the arena lazy (reborrow children on demand from `&mut Slab`, with the in-flight bitset in `RenderState` flags instead of a map), or reuse a per-owner arena across passes, with FxHash at minimum. Measure the lazy-band fixpoint pass count on the 10k/100k benches, and consider servicing lazy child requests within one pass per sliver.

### No tiered public surface: about 1,400 pub items, storage and pipeline internals included; the curated facade module covers only box leaves and proxies

- **Kind:** api_dx · **Severity:** high
- **Evidence:** grep counts `pub` items: flui-rendering 1421, flui-objects 869, flui-layer 268, with 2 `doc(hidden)` in total. The prelude exports `RenderTree`, `RenderNode`, `PipelineOwner` and `PipelineCell` (flui-rendering/src/lib.rs:118-139). owner/accessors.rs alone has 61 pub fns. src/rendering.rs (the facade) omits `ViewportOffset`, `ScrollPosition`, `LayerLink`, the delegates and `virtualization`, all of which flui-objects' viewport, leader and grid depend on. tests/fixtures/facade_extensions.rs has 0 `RenderSliver` impls.
- **Impact:** The H0 extension point is real only for boxes. A third-party lazy list or viewport must depend on the raw crate. At H3, semver-checks would freeze internals, or the freeze would be meaningless.
- **Direction:** Declare `flui::rendering` the Stable authoring tier and grow it deliberately: the sliver authoring set, the viewport-offset trait and `LayerLink`. Move storage, pipeline and binding behind `#[doc(hidden)]` or an `unstable-internals` feature used by flui-view and flui-app. Add a facade-consumer fixture with a custom `RenderSliver` and a custom lazy sliver.

### Catalog knowledge leaks into the protocol crate, and protocol seams live in the catalog (the spine depends on flui-objects)

- **Kind:** layering · **Severity:** medium
- **Evidence:** The pipeline downcasts to `SliverMultiBoxAdaptorParentData` for set-position semantics (flui-rendering/src/pipeline/owner/semantics.rs:1003-1008). `IntrinsicsCtx` special-cases `FlexParentData` (context/intrinsics.rs:13,316). Catalog parent data (Table, Wrap, ListWheel, TreeSliver, Text) sits in flui-rendering parent_data/, as does `ScrollPosition` with a scheduler edge (view/scroll_position.rs:28). In the other direction, `LayoutConstraintsCell`/`HeaderShrinkCell`/`BuildDuringLayoutCell` live in flui-objects 'so flui-view and flui-objects can share' (layout_constraints_cell.rs:31-35), and flui-view imports concrete catalog types in production (element/sliver_adaptor.rs:58, view/error.rs RenderErrorBox).
- **Impact:** A third-party lazy sliver with its own parent data gets no set-position semantics. A third-party build-during-layout object must depend on the first-party catalog. The catalog cannot be treated as a replaceable package, and every new catalog object with a special contract pulls code into the protocol or the spine.
- **Direction:** Turn the special cases into protocol hooks: a `logical_index()` on the ParentData trait or a sliver hook for semantics index, and a generic build-during-layout cell type in flui-rendering. Move catalog parent data, delegates and `ScrollPosition` up into flui-objects/widgets. Move the `LazyMultiBoxRender` impls into flui-widgets by moving the trait down into flui-rendering.

### The closed `Layer` enum contradicts the plan's 'custom layers' and external-GPU-content extension points

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** flui-layer ARCHITECTURE.md decision 1: 'There is no third-party `impl Layer` extension point, and plugin authors cannot define layer types — the accepted trade-off'. Its Producers section says `Texture`, `PlatformView`, `Canvas`, `ClipSuperellipse` and `AnnotatedRegion` have no production producer. The plan ('Extension points before 1.0') lists 'Custom render objects и слои (H0)' and 'External GPU content (spike H1, insertion H2)'; roadmap E3 wants custom fragment shaders.
- **Impact:** Either the plan promises something the architecture explicitly refuses, or H1/H2 will force a late enum redesign after consumers exist. Custom shaders, video and WebView all need a sanctioned seam.
- **Direction:** Decide explicitly in an ADR. Keep the enum closed, but add one open variant carrying an engine-registered id: `Layer::External { id: ExternalContentId, rect }` backed by a typed registry in flui-engine (textures, custom wgpu passes, shaders). Wire `Texture` as its first producer. Then amend the plan so it says 'custom content via registry', not 'custom layers'.

### Damage and layer caching have no producer; the whole scene is rebuilt and repainted every frame

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** `DamageRegion` has one variant, `Full` (flui-layer/src/scene_snapshot.rs:12-18). flui-app always sends `DamageRegion::Full` (app/raster_lane.rs:354). `DamageTracker::mark_dirty` is `pub(crate)` with only test callers (flui-engine/src/damage.rs:38). ADR-0061 (Accepted) says damage must come from diffing consecutive layer trees, but nothing diffs them. Retained captures are CPU-only by design (flui-rendering ARCHITECTURE.md 'A retained capture holds no GPU resource'). `LayerNode::render_id` is set only for boundaries.
- **Impact:** Idle-but-animating UIs pay full-surface GPU cost; ADR-0061's bench shows ratios of 0.02-0.18 left on the table. This is on the critical path of H2's exit ('damage + layer caching', 'changing one text repaints only its region').
- **Direction:** Implement ADR-0061's differ on `render_id`-stamped boundary subtrees, keyed by `Arc` identity of pictures, and emit `DamageRegion::Rects`. Add a GPU raster cache for stable boundaries in flui-engine keyed by (RenderId, content hash), with explicit device-loss invalidation. Once the differ exists, the 'captures hold no GPU resource' invariant changes, so record that.

### The render-object trait surface is fat and triplicated, and it mixes input/cursor concerns into the layout protocol

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** `RenderObject<P>` declares about 30 hooks (traits/render_object.rs:232-860). `RenderBox` redeclares them (render_box.rs:135-606), and the blanket impl forwards each one again (render_box.rs:651-909). `RenderSliver` repeats the pattern (render_sliver.rs:539+). The hooks include `pointer_target`, `scroll_target`, `pan_zoom_target`, `mouse_cursor`, `mouse_tracker_annotation` and `metadata() -> Option<Arc<dyn Any + Send + Sync>>`.
- **Impact:** Every protocol hook change touches at least 5 places and becomes Stable API at H3. Agents and contributors face a 60-method trait. Input routing is welded to layout objects, so interaction features (the H1 plugin model, the agent protocol) cannot evolve independently.
- **Direction:** Split optional capabilities into small opt-in traits or a returned descriptor, as `paint_effects` already does: `fn interaction(&self) -> InteractionDescriptor`, `fn semantics(...)`. Generate the forwarding with a macro, or drop the RenderObject redeclaration by having the pipeline call typed hooks through one erased vtable. Freeze only the minimal core.

### Fault isolation is uneven: hit-test, intrinsics queries and semantics hooks run third-party code unguarded, and a paint poison stalls the whole frame

- **Kind:** safety · **Severity:** medium
- **Evidence:** `catch_unwind` wraps layout (storage/entry.rs:420, subtree_arena.rs:1347,2016) and paint/layer-update (owner/paint.rs:513,703). There is no wrap at `hit_test_raw` (owner/accessors.rs:710,1038), at the query-path `intrinsic_raw` (owner/query.rs:491) or at `describe_semantics_configuration` (owner/semantics.rs:941,946). ARCHITECTURE.md: a paint or `LayerUpdate` panic 'discards the WHOLE frame' and 'a node stuck panicking blocks the whole frame from completing'. It also says 'The catch_unwind helper around hit_test will land when hit testing is wired through the pipeline'. Hypothesis: on wasm32 (the default panic=abort) none of this isolation applies.
- **Impact:** Third-party render objects (the H0 extension point) can take down event dispatch or the a11y publish path, and one bad paint freezes the UI indefinitely. That weakens the 'trusted runtime for humans and agents' positioning.
- **Direction:** One `guarded_call(node, phase, f)` helper used for every trait call site, with the phase enum extended to HitTest, Intrinsics and Semantics. On repeated paint poison, substitute an error-box picture for that node's subtree instead of dropping frames. Document the wasm behaviour.

### Send+Sync residue and locks on the layout hot path contradict the single-writer realm model

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** `trait ViewportOffset: Debug + Send + Sync` (flui-rendering/src/view/viewport_offset.rs:57) forces `ScrollPosition`'s `parking_lot::Mutex<State>` (scroll_position.rs:28). That lock is taken inside `RenderViewport::perform_layout` (flui-objects/src/sliver/viewport.rs:1057,1104). `pending_child_requests: &parking_lot::Mutex<Vec<..>>` sits inside the `!Send` arena (protocol/sliver_protocol.rs:862; subtree_arena.rs:299-300). The notifier is `Arc<parking_lot::RwLock<VisualUpdateNotifier>>` (owner/mod.rs:138). The Mutex-based cells are in header_shrink_cell.rs:55, layout_constraints_cell.rs:96 and subtree_anchor.rs:70. Meanwhile flui-objects ARCHITECTURE.md says 'No locks', and flui-rendering's claims 'No primitive sits inside perform_layout / paint on a per-node basis'.
- **Impact:** Locks on the per-frame path are cost and noise that AGENTS.md's frame-path rule forbids. The public `Send + Sync` bounds make callers part of a locking protocol the `!Send` owner never needs, and they complicate the H2 question of parallel layout inside a realm.
- **Direction:** Align the bounds with the realm: drop `Send + Sync` from `ViewportOffset` and the cells. Use `Rc<Cell/RefCell>` owner-local state, and cross-thread wakes only through `RenderInvalidationHandle`. Replace the arena Mutexes with `RefCell`. Fix both ARCHITECTURE thread-safety tables.

### Test scaffolding is compiled into production types behind a unified `testing` feature

- **Kind:** testing · **Severity:** low
- **Evidence:** 57 `cfg(any(test, feature = "testing"))` sites in flui-rendering production modules (grep). They include `PipelineOwner` fields `parent_data_seeds` and `semantics_error_once_for_test` (owner/mod.rs:251-257) and an extra `SubtreeArena::from_tree` parameter (subtree_arena.rs:523-528). The feature is enabled by 8 dependents' dev-deps and by the facade `testing` feature (Cargo.toml:592).
- **Impact:** Integration tests of every dependent run a differently-shaped pipeline than release builds. A frame-path test can pass on code that ships differently, which is the 'prove, don't claim' principle in reverse. It also inflates the H3 API.
- **Direction:** Move the seeding and failure injection behind a trait object or a hook registry installed by the harness (no cfg in owner fields). Keep `testing` as a pure additive module that only adds new types. Add a CI check that `cargo check -p flui-rendering` with and without `testing` has an identical `PipelineOwner` layout, via `size_of` or static_assertions.

### The catalog 'harness contract' is a string grep in one 15.9k-line test file, and no conformance kit exists for third-party catalogs

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** RENDER_OBJECT_TYPES is a test-local `&[&str]` (crates/flui-objects/tests/render_object_harness.rs:151). `catalog_covers_every_render_object_name` passes if a `harness_*` test chunk merely contains the type name as text (render_object_harness.rs:14440-14450). `render_object_types_match_exports` parses lib.rs `pub use` lines as text (:14453-14485). The file is 15,915 lines with 424 tests. flui-rendering/src/testing has no generic invariant checks: no hits for conformance or invariant helpers.
- **Impact:** The plan's 'stable protocol with harness tests for third-party catalogs' has no artifact a third party can run. Coverage means 'mentioned', not 'behaviour checked'. The single giant file is hostile to agents and contributors.
- **Direction:** Ship `flui::testing::rendering::conformance::check_box::<T>(factory)` / `check_sliver`, which run protocol invariants: dry layout equals layout, intrinsics monotone and finite, the baseline within size, hit-test within bounds, idempotent relayout, no paint outside the paint clip unless declared, and semantics stable across two frames. The first-party catalog runs it for every object. Replace the string registry with a typed registry (an `inventory`-free const list of factories) and split the harness file by family.

### Architecture docs and ADRs have drifted from the code they govern

- **Kind:** docs · **Severity:** medium
- **Evidence:** flui-rendering ARCHITECTURE.md contradicts the code in several places:
- its 'Render-tree storage' entry says `RenderId` is a `NonZeroUsize` newtype, while it is a generational `GenId` (flui-foundation/src/id.rs:715-723,759-787);
- its Thread-safety table lists `RenderTree::owner Option<Arc<RwLock<PipelineOwner>>>` and `Weak<RwLock<PipelineOwner>>` back-references, which cell.rs says were deleted;
- its Friction log cites `Box<dyn ContainerLayer>`, but layers are a closed enum.

Other drift:
- ADR-0017's Context still describes an `Arc<RwLock<…>>` owner.
- ADR-0056 says the facade 'deliberately does not re-export flui-rendering', while src/rendering.rs does.
- flui-rendering/src/lib.rs's crate example shows a non-existent API (`perform_layout(&mut self, constraints) -> Size`, `CanvasContext`).
- Rustdoc narrates history ('Signature evolution 1./2./3.', render_object.rs:198-223) despite AGENTS.md's no-history rule.
- There are about 8.4k lines of companion docs, including a per-crate docs/ROADMAP.md.
- **Impact:** With a bus factor of 1 plus AI agents, docs are the working memory. Stale invariants lead agents to reintroduce deleted shapes or to distrust correct ones, and they undermine the H3 exit criterion that an external contributor can finish a feature without the author.
- **Direction:** Rewrite the ARCHITECTURE thread-safety and storage sections from the code. Supersede or amend ADR-0017's context and ADR-0056's facade claim. Replace the lib.rs example with a compiled doctest. Archive migration/ and the per-crate ROADMAP.md into docs/archive. Add a doctest or `docs-links`-style check that ARCHITECTURE symbol references resolve.

### Layer 4 conflates protocol, catalog and GPU backend, so the within-layer direction is unchecked

- **Kind:** workspace_topology · **Severity:** low
- **Evidence:** `[package.metadata.flui] layer = 4` for flui-rendering, flui-objects and flui-engine. The root Cargo.toml's layers list has 'Render machine + render catalog' as #4. xtask allows same-layer edges. flui-rendering already has a dev-dependency on flui-objects.
- **Impact:** Nothing mechanically prevents flui-rendering from depending on flui-objects, or flui-objects on flui-engine. The protocol/catalog split that the H0 extension point relies on is enforced by convention only.
- **Direction:** Give the protocol (flui-rendering) its own layer below the catalog (flui-objects) and keep flui-engine a sibling backend layer, or add an intra-layer order to `cargo xtask workspace`. This is consistent with the 'crates are layers' decision already taken.

### Keep-alive is built in the pipeline but has no user-facing widget, and the plan and ADR disagree on its shape

- **Kind:** plan_misfit · **Severity:** low
- **Evidence:** ADR-0056 implements keep-alive as an element-side RAII lease (`LifecycleContext::keep_alive_lease`, crates/flui-view/src/owner/keep_alive.rs:177-249), and the placed-generation stamp became load-bearing for it. flui-widgets has no `KeepAlive`/`AutomaticKeepAlive` consumer; grep finds only a doc comment in dismissible.rs:64. The roadmap's design corrections row says 'Keep-alive as a property of the sliver protocol, not a mixin', while the ADR explicitly puts the decision element-side, not in the sliver protocol. Separately, ARCHITECTURE.md records that stamp exclusion is history-dependent: two identical trees can expose different a11y trees.
- **Impact:** C5 (list and scroll) is not done despite the infrastructure. The plan text will mislead the next implementer.
- **Direction:** Add the thin widget (`KeepAlive` / a builder option on lazy lists) with a test that fails without the lease. Update the roadmap row to match ADR-0056.

### The roadmap's 'nested scroll' differentiator does not exist in code

- **Kind:** plan_misfit · **Severity:** low
- **Evidence:** Roadmap line 183 claims a 'Sliver scrolling protocol with nested scroll and pinned/floating headers' as unique in the field. `grep -rl NestedScroll crates` returns nothing. The persistent headers exist (flui-objects/src/sliver/sliver_persistent_header.rs).
- **Impact:** It violates principle 5 (proof, not claims) and hides a real protocol gap: coordinated inner/outer scroll positions need `ScrollPosition` ownership decisions that currently live in the wrong crate (see the catalog-leak problem).
- **Direction:** Reword the roadmap claim to what exists, and open an epic for nested scroll that first settles where scroll position state lives (widgets layer, owner-local, not `Send + Sync`).

## Unwired or dead surface

- `DamageRegion`: only `Full` exists, and no producer ever sends anything else (flui-layer/src/scene_snapshot.rs:12-18; flui-app/src/app/raster_lane.rs:354).
- flui-engine `DamageTracker::mark_dirty`: `pub(crate)` with only test callers (flui-engine/src/damage.rs:38,113-134).
- Layer variants with no production producer: `Canvas`, `Texture`, `PlatformView`, `ClipSuperellipse`, `AnnotatedRegion` (flui-layer ARCHITECTURE.md 'Producers').
- `RenderError::{LayoutDuringPaint, LayoutDetached, PaintDetached, PhaseOrderViolation}`: deliberately unconstructible variants kept for future seams (flui-rendering ARCHITECTURE.md 'Phase ORDERING').
- `experimental-delegates` feature / `CustomClipper`/`RectClipper`: a delegate with no companion render object (flui-rendering/Cargo.toml features; lib.rs prelude cfg).
- `LifecycleContext::keep_alive_lease` / `KeepAliveLease`: no widget in flui-widgets consumes it.
- `hit_test_raw` catch_unwind: documented as 'will land when hit testing is wired through the pipeline'.
- `flui_rendering::virtualization` is described as 'extractable once a 2nd consumer appears'; its only consumers are the flui-objects slivers.
- `LayerTree`/`SceneBuilder` hand-authored path: used only by `flui_app::run_direct`, examples and readback tests (flui-layer ARCHITECTURE decision 5). Not dead, but not on the production frame path.
- flui-rendering companion docs: `docs/ROADMAP.md` (178 lines), `migration/*.md` (3.1k lines) and `flutter-rendering-hierarchy.md` (1352 lines). Their status as current guidance is unclear.

## Open questions

- Should third parties be able to add a new layout protocol? `Protocol` is sealed (protocol.rs:54), so only Box and Sliver exist. Flutter allows custom protocols in principle; is Box plus Sliver the permanent contract to freeze at H3?
- Where does scroll position state belong? It currently lives in flui-rendering as `Send + Sync` behind a Mutex, while Flutter keeps it in widgets. This decides nested scroll, the Router's scroll restoration and parallel layout.
- Is the lazy-band fixpoint pass count bounded in practice on a 10k/100k list during fast flings? No bench was found that reports passes per frame. Hypothesis: 2-3 passes are common. Needs measurement under E4.
- What is the ADR-0061 differ's granularity: boundary subtrees only (`render_id` set only on boundaries) or every picture layer? Does a GPU raster cache come with it, and who owns device-loss invalidation then?
- Should the curated facade module `flui::rendering` be formally declared the Stable tier now, so that everything else in flui-rendering can be marked unstable before crates.io publication (roadmap H1 publish pipeline)?
- Hypothesis to verify: on wasm32 (panic=abort by default) the whole poisoning/catch_unwind contract is inert. Is that acceptable for the web target in H0's exit?
- Should the paint-poison policy stay 'drop the whole frame until the node recovers', or move to Flutter-like per-node isolation with an error-box substitute? The current choice can freeze the UI for good because of one faulty third-party object.

