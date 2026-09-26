# Codebase map: xcut_performance — cross-cutting performance architecture (build → layout → paint → composite → raster, invalidation, allocation/dyn churn, locks, text, GPU, benches) against H2 (100k lists, damage + layer cache, cold start < 300 ms)

_Raw output of the `map:xcut_performance` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

How a frame really runs today (main @ cab06137d). The platform drives production (ADR-0058). `UiRealm::draw_frame_entered` (crates/flui-app/src/app/ui_realm/frame.rs:74-312) ticks each presentation's vsync registry and gesture deadlines. It then asks each presentation's FrameClock whether to run a segment, so an idle app runs no segment. A segment runs in order:
- `WidgetsBinding::draw_frame` under a write lock on `WidgetsBindingInner` (flui-view/src/binding.rs:1240), which drains a depth-ordered dirty heap.
- `run_frame_with_layout_builders` (flui-view/src/owner/layout_builder.rs:411), a layout↔build fixpoint that runs up to 10 passes, 6 of which may service lazy sliver children.
- `PipelineOwner` phases: layout → compositing → paint → semantics.

Paint descends from the root every time anything is paint-dirty. Clean repaint boundaries are 'grafted': their retained capture (a flat `Vec<RetainedNode>` whose `PictureLayer`s share `Arc<DisplayList>`) is cloned into a brand-new `LayerTree` with freshly minted `LayerId`s (paint.rs:1383-1425). Inline content, meaning anything not under a boundary, is re-recorded, and the root boundary is never retained.

The resulting Scene crosses an *inline* raster lane on the same thread (raster_lane.rs, ADR-0045 still Proposed). It always carries `DamageRegion::Full` and the renderer is told `mark_full_repaint()` (raster_lane.rs:354, :486). The engine then walks the whole layer tree, re-batches every DisplayList into Command IR, rewrites pooled buffers and re-rasterises the full surface. The damage consumer (scissor) is written and tested, but nothing produces damage (flui-engine/src/damage.rs:1-13, ADR-0061).

Text is shaped with cosmic-text under one process-global `FONT_SYSTEM` mutex (flui-painting/src/text_layout/layout.rs:124). The engine's glyph-atlas misses rasterise through the same mutex (flui-engine/src/painter/mod.rs:167). The host font scan (`FontSystem::new`) runs synchronously at realm install (flui-app/src/app/runtime.rs:140-147).

What is strong:
- Platform-paced production and an idle realm.
- Typestate pipeline phases over slab arenas, with no lock on per-node render storage.
- A B+-tree virtualizer that makes 100k-item sliver layout O(K log N).
- Composited-layer updates that patch retained captures without promoting nodes (measured 228x on inline content).
- `Scene: Send + Sync`, statically asserted, which is the prerequisite for a threaded raster lane.
- Realm-scoped signals with real rebuild telemetry.

What caps H2 is structural, not local:
1. Frame output has no cross-frame identity: a fresh LayerTree per frame, no damage producer and no engine-side picture cache, so the cost of a presented frame scales with what exists, not with what changed.
2. Several O(tree) steps sit inside frame work: a global render-topology sync after any render-element insert, a whole-slab scan per dirty layout root, invalidation that is not coalesced, and View configs deep-cloned on every rebuild.
3. UI and raster are serialized on one thread and additionally coupled through the global font mutex.
4. Nothing measures the H2 exit criteria: no end-to-end frame bench, no cold-start measurement, no per-PR perf gate. Weekly trend runs skip the feature-gated benches, including the damage baseline.

These need architecture decisions before H3 freezes the render/layer/pipeline API: stable layer identity plus a damage diff, a threaded raster lane with per-lane font/glyph ownership, and one concurrency model for realm-owned state.

## Responsibilities and boundaries

Who owns which per-frame cost:
- **flui-view** — build (dirty heap, reconciliation), the element→render topology sync, and the layout↔build fixpoint.
- **flui-rendering** — dirty scheduling, layout, paint composition into a LayerTree, retained boundary captures and semantics assembly.
- **flui-painting** — DisplayList recording and text shaping, and the owner of the global FONT_SYSTEM.
- **flui-layer** — the per-frame Scene/LayerTree vocabulary and the DamageRegion seam message.
- **flui-engine** — the layer walk, Command IR, GPU replay, glyph atlas, pools and the damage consumer.
- **flui-app** — the frame loop, FrameClock gating, the raster lane adapter and the full-repaint decision.
- **flui-scheduler** — phases, demand and telemetry.

Where the boundaries leak:
1. **Damage has no owner.** The consumer is in flui-engine, the seam type in flui-layer, and the would-be producer (a layer-tree diff) needs identity that flui-rendering mints fresh every frame. flui-app hard-codes `Full`.
2. **Text crosses the UI/raster boundary through a process global.** flui-engine calls `flui_painting::shared_font_system()` to rasterise glyphs, so the raster side is coupled to UI-thread layout through one mutex. That contradicts both principle 3 (no global state) and the planned raster lane.
3. **The element layer reaches into render topology.** `ElementTree::synchronize_render_children` (flui-view) rewrites render-tree parent/child links wholesale inside `pipeline_owner.with_mut`, so flui-view owns a render-tree invariant through a global pass instead of local commits.
4. **The concurrency model is split across crates.** PipelineOwner is `Rc<RefCell>` (owner-affine), but ElementTree/BuildOwner are `Arc<RwLock>` and exposed in public signatures (`ElementBuildContext::tree()`), and gesture recognizers are `Arc<Mutex>` plus a DashMap. Owner-affine data should be `!Send` and lock-free; `Send` should be reserved for the Scene and lane mailboxes.
5. **Perf verification belongs to xtask/CI** (`bench-collect`, weekly), but its target selection silently drops the benches that matter for H2.

## Key types and contracts

- UiRealm::draw_frame_entered / draw_frame_for_presentation (flui-app/src/app/ui_realm/frame.rs:74, :329) — per-presentation segment gated by FrameClock::poll; one Scene per pump
- FramePaintOutcome::{Painted(Scene), Idle, Errored}; SubmitVerdict; FrameSink (DirectSink | RasterLane)
- RasterLane<B> / RasterOwner / SceneSnapshot{FrameStamp, DamageRegion, Scene} — inline mailbox protocol (ADR-0045 Proposed); DamageRegion has only `Full`
- DamageTracker (flui-engine/src/damage.rs, crate-private): mark_dirty/mark_full_repaint/damage_rect → render_scene scissor; Renderer::mark_dirty (renderer.rs:1660) has no production caller
- PipelineOwner<Phase> typestate: run_layout/run_compositing/run_paint/run_semantics; PipelineCell = Rc<RefCell<PipelineOwner>> (owner/cell.rs)
- FragmentComposer + RetainedSubtree{nodes: Vec<RetainedNode>, nested_boundaries, effect_slots} + graft() — per-frame LayerTree assembly with Arc<DisplayList> sharing and freshly minted LayerIds
- LayerNode.render_id: Option<RenderId> (flui-layer/src/tree/layer_tree.rs:38) — stamp on boundary OffsetLayers; the only cross-frame identity carrier
- RenderUpdateImpact (ADR-0046) and PaintKind::{Repaint, LayerUpdate} — property-change → minimal dirtiness
- DirtyTracker::mark_needs_layout (pipeline/scheduler.rs) — non-coalesced ancestor walk (#1042)
- RenderTree::get_subtree_mut → collect_disjoint_mut (storage/tree.rs:82) — whole-slab scan per call (#1041)
- ElementTree::reorder_render_children_after_build / synchronize_render_children (flui-view/src/tree/element_tree.rs:1317, :1428) — one bool triggers a global forest + render-arena pass
- BoxedView(Box<dyn View>) with Clone = dyn_clone deep copy (view/into_view.rs:178-184); View: Downcast + DynClone + 'static (not Send)
- RenderObject<P>: Diagnosticable + Downcast + 'static (traits/render_object.rs:178) — not Send, forecloses parallel layout without a breaking change
- Virtualizer (augmented B+-tree, ADR-0003) + RenderSliverList request strategy; MAX_LAZY_BAND_PASSES = 6, MAX_LAYOUT_BUILD_PASSES = 10 (owner/layout_builder.rs:64,74)
- FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>> (flui-painting/src/text_layout/layout.rs:124); shared_font_system() used by engine glyph rasterisation (painter/mod.rs:167)
- BuildOwner::last_frame_build_report(), PipelineOwner::layout_roots_total — rebuild/layout telemetry; FrameSnapshot telemetry in flui-scheduler
- assert_impl_all!(Layer/LayerTree/Scene: Send, Sync) (flui-layer/src/scene_snapshot.rs:76-78)

## Dependencies

The frame path spans layers from bottom to top. flui-painting (DisplayList, TextLayout on cosmic-text) → flui-layer (Scene, LayerTree, DamageRegion) → flui-rendering (pipeline, retention) → flui-objects (concrete render objects, RenderParagraph owns a TextPainter) → flui-view (build, reconcile, topology sync, fixpoint) → flui-app (frame loop, raster lane) → flui-engine (wgpu, lyon, etagere, glyph atlas).

There is one cross-layer edge that sideslips the layering intent. flui-engine reaches the flui-painting global FONT_SYSTEM for glyph rasterisation, so the renderer depends on a process-global in a lower layer instead of owning a font/glyph service.

External perf-relevant dependencies:
- cosmic-text/swash for shaping and rasterising.
- lyon for tessellation.
- wgpu 30.
- slab arenas.
- parking_lot and dashmap for locks.
- rustc_hash for FxHash.
- dyn_clone, which makes view cloning deep.
- criterion for benches.

Parley (ADR-0077 Proposed) would replace cosmic-text; that is the natural moment to make the font context per-realm and give the raster lane its own glyph cache.

On the consumer side, flui-widgets inserts RepaintBoundary per list/grid item by default (scroll/list_view.rs, grid_view.rs), and modal routes add their own boundary. The retention model's effectiveness therefore depends on catalog conventions, not on the pipeline.

## Fit with the plan

**H0 (beta).** Adequate. Idle = 0 segments works through the FrameClock gate, and the realm model keeps per-window state local. The roadmap's A5 budgets (idle 0 fps; 10k static list p99 < 1 panel period; a single text change repaints only its region) are only partly measurable: no end-to-end bench or per-desktop recipe exists (E5 is still weekly-advisory, and the damage bench is skipped even there).

**H2 (perf/scale)** is where the architecture is the constraint:
- **Damage + layer caching (E1).** Blocked on a design choice that is still open: layer identity across frames. Today every frame mints a new LayerTree, so the engine cannot cache pictures or diff. Boundary OffsetLayers now carry `render_id` (layer_tree.rs:38; graft carries it, paint.rs:1414-1417), which is partly the missing piece ADR-0061 names. So the producer is closer than the ADR text says, but a diff pass and a partial DamageRegion variant still need designing.
- **Raster/IO lanes (E2).** The raster protocol is well prepared (Scene is Send+Sync; mailbox and generation stamping are in place). Overlap is blocked by the shared FONT_SYSTEM mutex used on both sides, and on macOS by the wgpu-hal pinning (#653).
- **100k rows.** The render side is ready (B+-tree virtualizer, O(K log N)). The element/topology side is not: a global topology sync after any render insert, whole-slab scans per dirty root, deep View clones, and multi-pass lazy building.
- **Parallel layout (open question in the plan).** Effectively answered 'no' by the types: RenderObject is not Send, PipelineCell is `Rc<RefCell>`, and FONT_SYSTEM is global. That is fine, but it should be recorded as a decision before H3 rather than left as a spike.
- **Cold start < 300 ms.** Not instrumented anywhere, and the startup path serializes the host font scan, wgpu adapter/device creation and eager pipeline creation on the UI thread.

**H3.** The render/layer/pipeline protocol (RenderBox paint → LayerTree) should not be frozen until layer identity and damage are settled, because both change what `paint` produces.

**H1 / H4 extension points.** External GPU content (a compositor spike in H1, embedding in H2) needs retained layers with stable identity. Without that, an external texture layer is rebuilt every frame and cannot be damage-tracked.

**Principles.** Principle 3 (no global state) is violated on the hottest shared resource (FONT_SYSTEM). Principle 5 (proof, not claims) is weakly served by perf tooling that proves nothing per PR.

## Strengths

- Production is platform-paced (ADR-0058), and a per-presentation FrameClock gate skips segments when nothing is dirty, so an idle app does no build/layout/paint (frame.rs:147-185).
- Typestate pipeline phases over slab arenas with `RenderEntry` owned by value and no lock on per-node render storage; the WAS_REPAINT_BOUNDARY bit moved to atomic flags to avoid a per-paint write lock (flui-rendering/ARCHITECTURE.md:870-890).
- Retained repaint boundaries share `Arc<DisplayList>` across frames; composited-layer updates (opacity/clip/transform) patch the enclosing capture without promoting nodes, a measured, recorded divergence from Flutter (228x on inline content; flui-rendering/ARCHITECTURE.md:223-247).
- Virtualization uses an augmented B+-tree (ADR-0003) with a lazy-sliver bench at 1k/10k/100k showing O(log n) band queries (flui-rendering/benches/virtualizer.rs).
- The raster boundary is designed right: Scene/LayerTree/Layer are statically asserted Send+Sync (flui-layer/src/scene_snapshot.rs:76-78); the mailbox, FrameStamp and SurfaceGeneration discipline already carry every production frame inline, so threading later changes who calls `pump`, not the frame contract.
- The damage consumer (scissor + self-heal) is written, and ADR-0061 records the measured value: 64 layers 2901 µs full vs 56 µs damaged.
- Engine record/replay split (Command IR holds no pooled GPU resources, deterministic replay test), a pow2-bucketed buffer pool, an engine-owned glyph atlas (ADR-0067), and a cached shader-mask painter.
- Rebuild telemetry exists and was used for a real go/no-go: `last_frame_build_report`, `layout_roots_total`, and the signals bench (10k list one-row change: 20003 → 2 elements).
- Allocation-budget tests exist on some hot paths (flui-scheduler/tests/frame_telemetry_allocation.rs, flui-engine/tests/raster_backpressure_allocation.rs, flui-interaction/tests/pointer_route_hot_path.rs).
- Scrolling follows Flutter's model: the viewport listens to its offset directly (flui-objects/src/sliver/viewport.rs:99-115), so scrolling does not rebuild widgets.
- Semantics is incremental: a graft pass, with full reassembly only as fallback (pipeline/owner/semantics.rs:117-133).
- Build profiles are sensible: dependencies at opt-level 3 in dev; release is thin LTO with codegen-units = 1.

## Problems

### Every presented frame re-rasterises the full surface: the damage producer does not exist

- **Kind:** performance · **Severity:** critical
- **Evidence:** crates/flui-app/src/app/raster_lane.rs:354 `SceneSnapshot::new(stamp, DamageRegion::Full, scene)` and :486 `self.renderer.mark_full_repaint()`; crates/flui-engine/src/damage.rs:4-6 'Its producer does not exist yet — every path calls mark_full_repaint'; Renderer::mark_dirty (renderer.rs:1660) has no production caller (grep); ADR-0061 Decision; issue #1037 OPEN. ADR-0061's own measurement at 1920x1080: 64 layers full 2901 µs vs damaged 56 µs.
- **Impact:** This blocks H2 'damage + layer caching' and the roadmap A5 budget that a single text change repaints only its region (E1). A caret blink, spinner or hover highlight costs the GPU the whole scene, which matters most for battery on laptops and mobile (H1) and on weak GPUs (the H2 software-render fallback).
- **Direction:** Treat damage as a pipeline product, not an engine feature. Diff consecutive layer trees keyed by stable layer identity (see next problem), comparing `Arc::ptr_eq` on PictureLayer content plus a transform/opacity/clip compare. Emit `DamageRegion::Partial(rects)` in SceneSnapshot. Keep Full as the fallback when identity is missing. Gate the result with an end-to-end test that counts GPU fragments or scissor area for a single-text change.

### The layer tree is rebuilt with fresh IDs every frame, so the engine cannot cache or diff

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** crates/flui-rendering/src/pipeline/owner/paint.rs:86-189: run_paint builds a new FragmentComposer, descends from the root, and assigns `self.last_layer_tree = Some(layer_tree)`. graft() (paint.rs:1383-1425) clones every retained node into the new tree and 'mints fresh ids'. ARCHITECTURE.md:584-586 says the graft is O(retained layers), and 'patching one still clones the whole retained capture'. The engine walks and re-batches the whole tree each frame (flui-engine/ARCHITECTURE.md:52-75). There is no picture or raster cache in flui-engine: `Arc::ptr_eq` appears only in surface_lease.rs. LayerNode now carries `render_id` (flui-layer/src/tree/layer_tree.rs:38), stamped on boundary OffsetLayers.
- **Impact:** Per-frame CPU (compose + engine record) and GPU cost scale with the total retained content, not with what changed. This caps 100k-row and large-document frames (H2). It also blocks layer caching, external GPU content as a stable layer (H1/H2 extension point), and devtools layer diffs (principle 4).
- **Direction:** Before H3, decide whether the LayerTree is retained (mutated in place, with a stable LayerId per repaint boundary or effect node) or immutable-per-frame with a persistent structural-sharing representation, such as an `Arc` subtree per boundary so a graft is O(1). Either way, make the RenderId → layer mapping the identity key. Let the engine cache Command IR per `Arc<DisplayList>`, and later cache rasterised textures for static boundaries. Record the choice in an ADR that supersedes ADR-0061's 'until that exists' clause.

### Inline content and the root boundary are repainted on every paint pass

- **Kind:** performance · **Severity:** medium
- **Evidence:** flui-rendering/ARCHITECTURE.md:287-293: 'The root boundary is never retained … the frame repaints'. ADR-0061: inline content (anything not under a RepaintBoundary) is re-recorded whenever the frame paints. Boundaries come only from catalog conventions (list/grid items, modal routes: flui-widgets/src/scroll/list_view.rs:91, navigator/modal_route.rs).
- **Impact:** Paint cost for a page body tracks the size of the enclosing boundary's inline content. A single animated or blinking element forces re-recording of all its siblings, including text layout paint. For H2 editors (1 MB) and dense dashboards, this puts app authors in charge of manual RepaintBoundary tuning, a known Flutter pain point.
- **Direction:** Retain the root capture. Consider automatic boundary promotion driven by measured repaint frequency (Compose/Skia-style heuristics), or per-subtree picture caching keyed by render-node paint generation, so retention does not depend on catalog discipline.

### Any render-element insert triggers a global element-forest and render-arena topology pass

- **Kind:** performance · **Severity:** high
- **Evidence:** crates/flui-view/src/tree/element_tree.rs:1107-1114 sets `needs_render_reorder = true` on every render-bearing insert. build_owner.rs:2027 calls reorder_render_children_after_build at the end of every drain. synchronize_render_children (element_tree.rs:1428-1520) walks all roots via `iter_nodes()`, DFSes the whole element forest, builds two HashMaps, collects `render_tree.iter()` ids twice, and rewrites child lists with repeated `remove_child`, which is Vec::remove, so quadratic. Issue #1039 OPEN.
- **Impact:** Mount-heavy frames such as scrolling a lazy list (new rows every frame), route pushes and hero flights pay O(mounted elements + render nodes). With large mounted trees, such as long non-lazy Columns, grids with a wide cache band or several windows, this is per-frame work that grows with the app. It directly threatens the 10k-row p99 budget (E4) and H2 100k.
- **Direction:** Make topology commits local. Record the affected render parents during reconcile or attach, and re-sequence only those parents' child lists by slot order, in O(children) using a single rebuild instead of front removals. Make the global pass a debug-only verifier (it already has debug_assert consistency checks).

### Each dirty layout root pays a scan of the whole render slab

- **Kind:** performance · **Severity:** high
- **Evidence:** crates/flui-rendering/src/storage/tree.rs:82-110 collect_disjoint_mut: builds a HashMap of wanted indices, then `for (idx, node) in nodes.iter_mut()` from slot 0 until all are found. It is reached from run_layout → layout_dirty_root → SubtreeArena::from_tree → get_subtree_mut (issue #1041 OPEN, which says the same helper backs query.rs::acquire_query_slots for intrinsics and dry layout).
- **Impact:** D independent relayout boundaries in an N-node arena cost O(D·N) before any perform_layout runs. With per-row relayout boundaries in a 100k list, or many text fields updating, this becomes quadratic, which is exactly the H2 scale target.
- **Direction:** Replace it with direct disjoint indexing: sort the wanted indices and use `get_many_mut`/`split_at_mut` over the slab's backing storage, or a generational arena with O(1) disjoint borrows. Add a micro-bench that places the dirty leaf late in a large arena.

### Views are deep-cloned on every rebuild, costing O(subtree × depth)

- **Kind:** performance · **Severity:** high
- **Evidence:** crates/flui-view/src/view/into_view.rs:178-184: `impl Clone for BoxedView { BoxedView(dyn_clone::clone_box(&*self.0)) }`, a deep copy. RenderBehavior::build_into_views clones every child view out of the parent config (element/behavior.rs:1069-1073). Proxy elements clone the child (behavior_commons.rs:305). dispatch_view_update clones the incoming config again before storing it (element/dispatch.rs:145). Container views hold `Vec<BoxedView>` (flui-widgets/src/flex/flex.rs:137,249). The journal's setState 10k-list numbers are 20003 elements / 15.3 ms for a one-row change and 34.6 ms for an append (dev profile).
- **Impact:** A rebuild of a container with a large child subtree copies all descendant configuration at each level: allocation churn and cache misses proportional to subtree size times depth. This undermines the 'setState stays as low level' path for Flutter migrants and H2 list or editor scale. Signals mitigate it only where adopted.
- **Direction:** Make view configs cheaply shareable, for example `Rc<dyn View>` or `Arc`-backed children (views are immutable by contract, and Flutter shares widget references), or move child views into the child element instead of cloning. Add an allocation-count test for 'rebuild parent of N children' to pin it.

### UI and raster run serially on one thread (inline lane only)

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** crates/flui-app/src/app/raster_lane.rs:1-13: `RasterMode::Inline` runs `RasterOwner::pump` synchronously on the UI thread. flui-engine/ARCHITECTURE.md:432-436: the threaded lane is 'tested but not wired'. ADR-0045 Status: Proposed. Issue #559 OPEN (latency percentiles and overlap unimplemented). macOS is pinned inline by upstream wgpu-hal (#653).
- **Impact:** Frame time is build + layout + paint + engine record + GPU submit + present-acquire. That leaves the 16 ms (or 8 ms at 120 Hz) budget for H2 with no pipelining, and a GPU-heavy frame stalls input handling. This is the H2 exit criterion 'Raster/IO lanes in production'.
- **Direction:** Finish ADR-0045 as a two-mode contract (inline stays the diagnostic and wasm mode). Prerequisite: remove the shared FONT_SYSTEM coupling (next problem). Measure p50/p95/p99 input-to-present on three desktops as the acceptance criterion.

### UI layout and raster glyph rasterisation contend on one global font mutex, and the host font scan sits on the startup path

- **Kind:** safety · **Severity:** high
- **Evidence:** flui-painting/src/text_layout/layout.rs:124 `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>`, locked for shaping (issue #1133 lists layout.rs:577-660 and measure.rs:80-85). The engine rasterises atlas misses through it: flui-engine/src/painter/mod.rs:167 `flui_painting::shared_font_system()`. `FontSystem::new()` (layout.rs:147) scans host fonts and is forced eagerly at realm install on the UI thread (flui-app/src/app/runtime.rs:140-147). Issue #1133 cites 'ADR-0002 engine-wide threading', which no longer exists in docs/adr.
- **Impact:** (a) A threaded raster lane would block on the UI thread's text layout and the reverse, so the E2 overlap cannot materialise. (b) Parallel layout is foreclosed. (c) It violates principle 3 (no global state) and multi-realm isolation. (d) Cold start: the synchronous system font enumeration is on the critical path to the first frame (hypothesis: typically tens to hundreds of ms on a font-rich Windows host; not measured anywhere), which threatens H2's < 300 ms.
- **Direction:** Fold this into the Parley migration (ADR-0077). Keep an immutable, Arc-shared font collection (loaded once; host scan asynchronous or lazy, with bundled faces available immediately), a per-realm shaping context, and a raster-owned glyph rasteriser or cache keyed by face and glyph with no shared lock. Measure time-to-first-frame with and without the scan.

### Nothing measures the H2 exit criteria, and weekly trend runs skip the key benches

- **Kind:** testing · **Severity:** high
- **Evidence:** tools/xtask/src/bench.rs:36 skips every bench with `required-features`. That skips flui-engine `render_throughput` (Cargo.toml:151-154, home of the `damage_scissor` baseline that ADR-0061 says 'stays as the baseline any producer must beat'), `offscreen_resource_cache`, flui-view `scoped_build_drain`, and flui-widgets `signals_rebuilds`. PR CI only compiles `cargo bench -p flui-rendering --no-run` (.github/workflows/ci.yml:1114). The weekly bench job is `continue-on-error: true` with 'TREND DATA ONLY, never a gate' (weekly.yml:8-12, 97). No bench drives a full frame (build→present) for a large list, and no cold-start or time-to-first-frame measurement exists (grep over crates/tools/docs finds none). No Flutter comparison harness exists either, though the H2 exit is 'bench suite on 3 OSes not worse than Flutter'. Roadmap E5 (per-PR >10% gate) is unimplemented.
- **Impact:** H2 cannot be exited on evidence (principle 5). The regressions most likely to happen (O(N) topology passes, clone churn, font lock) are invisible, and the one measured baseline for damage never runs.
- **Direction:** Add an end-to-end perf harness on flui-testing's virtual clock. Scenarios: 10k and 100k lazy list scroll, one-text change, route push, cold start to first presented frame. Report counts (elements built, layout roots, layers, bytes uploaded, damage area) plus wall time. Gate deterministic counts per PR, since they are noise-free, and keep wall time as a nightly or per-OS trend. Make bench-collect enable the required features instead of skipping them.

### The concurrency model is split: owner-affine realm state behind Arc<RwLock>/Arc<Mutex>, and locks in public signatures

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** PipelineOwner is `Rc<RefCell>` (flui-rendering/src/pipeline/owner/cell.rs:47), but the element tree and build owner are `Arc<RwLock<ElementTree>>` / `Arc<RwLock<BuildOwner>>`. They appear in public API: flui-view/src/context/element_build_context.rs:128-135 `pub fn tree(&self) -> &Arc<RwLock<ElementTree>>`, and lib.rs:130-131. Every frame takes `self.inner.write()` (binding.rs:1241). Each `depend_on` locks a Mutex dep_sink (element_build_context.rs:842). The sliver ChildManager registry is `Arc<Mutex<HashMap<RenderId, Arc<Mutex<dyn ChildManager>>>>>` (element/child_manager.rs:56). Gesture recognizers use `Arc<Mutex<…>>` plus DashMap (flui-interaction/src/recognizers/drag.rs:165-168, arena/team.rs:346). RenderObject is not Send (traits/render_object.rs:178).
- **Impact:** Per-event and per-build lock traffic on data that is owner-affine by ADR-0027 (small but pervasive). It contradicts AGENTS.md ('a lock in a public signature makes callers part of the locking protocol'). It leaves the plan's 'parallel layout inside a realm?' question undecided in API form right before an H3 freeze.
- **Direction:** Write one ADR: realm-owned trees and controllers are `!Send`, with `Rc`/`RefCell` or `&mut` threaded through; `Send` is reserved for Scene, lane mailboxes and IO results. Explicitly record 'no intra-realm parallel layout' (or the conditions to revisit it) so the stable API does not carry Arc/RwLock. Remove the lock types from the public BuildContext surface.

### Lazy sliver children are built between layout passes, not inside layout

- **Kind:** flutter_divergence · **Severity:** medium
- **Evidence:** crates/flui-view/src/owner/layout_builder.rs:411-450: drive_fixpoint reruns `run_layout` after `service_child_requests_between_passes`, with MAX_LAZY_BAND_PASSES = 6 and MAX_LAYOUT_BUILD_PASSES = 10 (:64, :74). When the budget is exhausted, 'the remaining requests are deferred to the next frame' (:441-447). Flutter builds children inside `performLayout` via `invokeLayoutCallback`.
- **Impact:** A fast fling or jump that exposes many new rows costs several full viewport relayouts per frame. Each insert batch then triggers the global topology pass above. Rows can appear a frame late (visible gaps), which works against the 100k-list goal and the p99 budget. Magnitude is a hypothesis: no bench covers the ChildManager path (virtualizer.rs states 'no ChildManager is wired').
- **Direction:** Either allow reentrant build inside sliver layout, as ADR-0003's 'reentrant build' and ADR-0017 seam intend, so each row is built and laid out in one pass, or batch the whole band request in one pass using an extent estimate. Add a fling bench counting layout passes per frame.

### Layout invalidation is not coalesced (quadratic chains)

- **Kind:** performance · **Severity:** medium
- **Evidence:** Issue #1042 OPEN: DirtyTracker::mark_needs_layout (flui-rendering/src/pipeline/scheduler.rs:191-261) always sets the flag, fires on_invalidated, clears the layout cache and walks to the relayout boundary, even when already marked. A chain of N non-boundary nodes marked once each costs N(N+1)/2. The test only checks queue length.
- **Impact:** Bulk updates such as theme or locale changes, text-field IME bursts, or many animated values under one boundary pay depth-proportional work per mark. Mostly an H2 concern.
- **Direction:** Stop the upward walk at the first ancestor already marked in the current invalidation epoch (epoch counter per frame), and pin the fix with a count-based test.

### Info-level logging in the per-element build and mount path, with an 'info' default filter

- **Kind:** performance · **Severity:** medium
- **Evidence:** crates/flui-view/src/element/behavior.rs:1059 `tracing::info!("RenderBehavior::build_into_views START …")` and :1090 `tracing::info!("RenderBehavior::on_mount creating RenderObject")`. flui-log/src/filter.rs sets `DEFAULT_DIRECTIVES = "info,wgpu=warn"`.
- **Impact:** With flui-log installed and default directives, every render-element rebuild and every mount emits an event. Mounting 10k rows produces tens of thousands of log records per frame, which distorts any perf measurement and cold start. It is also a structural gap: nothing enforces log-level discipline on hot paths.
- **Direction:** Demote these to trace. Add a lint or xtask check that forbids info/warn inside designated frame-path modules (or a `#[hot_path]` convention), and document the rule in AGENTS.md's gate table.

### The startup path is unmeasured and serial (fonts, adapter, eager pipelines, no driver pipeline cache)

- **Kind:** missing_capability · **Severity:** medium
- **Evidence:** The font scan is forced at realm install (flui-app/src/app/runtime.rs:140-147). `PipelineSet::new` builds pipeline families eagerly (flui-engine/src/pipeline_set.rs:275-372, with only the SSAA composite lazy). There is no `wgpu::PipelineCache` use (grep over flui-engine/src finds none). The runtime ties `Renderer::new` to window creation. No time-to-first-frame telemetry or test exists.
- **Impact:** The H2 'cold start < 300 ms' exit has no baseline, and the known-heavy steps are sequential on the UI thread. Mobile (H1) makes this worse.
- **Direction:** Instrument the phases from process start to first present (spans, plus an xtask device measurement per OS). Run the font scan and adapter/device request concurrently with window creation. Create pipelines lazily or on a warm-up thread, and adopt a wgpu PipelineCache where the backend supports it. Budget each phase.

### No allocation budget on the build/layout/paint path; per-frame hash sets and a fresh LayerTree

- **Kind:** performance · **Severity:** low
- **Evidence:** run_paint allocates a `FxHashSet` of dirty ids, a `FxHashMap` of layer updates, a `reached`/`visited` set of all visited nodes, and a new LayerTree slab every paint (paint.rs:86-189). graft allocates a map and a vec per call (ARCHITECTURE.md:451). Allocation tests exist only for scheduler telemetry, raster backpressure and pointer routing (crates/*/tests/*allocation*.rs, pointer_route_hot_path.rs).
- **Impact:** This is per-frame allocator churn proportional to the dirty and visited sets. It is not a cap by itself, but nothing pins it, so it grows silently as features land.
- **Direction:** Keep reusable scratch buffers on PipelineOwner. Add counting-allocator tests for a steady-state frame (for example, zero allocations for an unchanged-boundary repaint of N items).

### Two frame-demand carriers bypass the phase gate

- **Kind:** runtime_architecture · **Severity:** low
- **Evidence:** Issue #1172 OPEN: PipelineOwner::request_visual_update goes to the presentation wake callback and window.request_redraw without touching UpdateScheduler::ensure_visual_update's phase switch; wake_action ORs both.
- **Impact:** Extra or duplicated frames from mid-frame visual-update requests. This threatens the 'idle = 0 fps' budget and makes frame-count perf tests unsatisfiable (#1157 criterion dropped).
- **Direction:** Route all demand through one gated entry (FrameClock::mark_demand or ensure_visual_update) and assert frame counts in the perf harness.

### The signals registry scales poorly for wide fan-out

- **Kind:** performance · **Severity:** low
- **Evidence:** Journal (2026-09-22): fan-out costs ~1.3-1.8 µs per reader; the design review names an O(R²) registry; follow-ups #1248-#1254. ADR-0075 effects are not run in the product frame (run_effects is only in the headless harness).
- **Impact:** Wide reactive reads (settings or theme-like values read by thousands of rows) are costlier than InheritedView. Acceptable now, but it matters once signals become the default state layer for H2-scale apps.
- **Direction:** Batched scheduling (schedule_many) and an O(R) registry before signals become the recommended default; wire run_effects into draw_frame_impl under ADR-0075.

## Unwired or dead surface

- Renderer::mark_dirty (crates/flui-engine/src/renderer.rs:1660) and DamageTracker::mark_dirty/damage_rect scissor path: no production producer; DamageRegion has only `Full` (ADR-0061, #1037).
- Threaded raster lane machinery in flui-engine (RasterOwner::run_until_shutdown, InFlightAccounting, ack channel): tested but not driven by flui-app (flui-engine/ARCHITECTURE.md:432-436).
- UiRealm::render_frame_entered / DirectSink on native: `expect(dead_code)` outside wasm and tests (flui-app/src/app/ui_realm/frame.rs, render_frame_entered attributes).
- Feature-gated benches never executed by any automation: flui-engine render_throughput (damage_scissor baseline), offscreen_resource_cache, flui-view scoped_build_drain, flui-widgets signals_rebuilds (tools/xtask/src/bench.rs:36 skips required-features targets).
- ADR-0075 effects: run_effects runs only in the headless harness, not in the product frame (journal 2026-09-22; ADR-0075 Proposed).
- Sliver layout cache: none exists (#1199); sliver intrinsic and dry queries degrade to zero in release.

## Open questions

- Should the LayerTree be retained and mutated in place with stable LayerIds, or stay immutable per frame with structural sharing (an Arc subtree per boundary)? That choice decides damage, engine picture caching and external-texture layers, and it must land before H3 freezes the paint→layer contract.
- Now that boundary OffsetLayers carry `render_id` (flui-layer/src/tree/layer_tree.rs:38, carried through graft), is ADR-0061's 'pairing does not exist' still accurate? A diff-based damage producer may already be feasible for boundary-granular damage.
- Is intra-realm parallel layout ruled out for good? The types say so today (RenderObject not Send, PipelineCell Rc<RefCell>, global FONT_SYSTEM). If yes, record it and remove the Arc/RwLock shapes from realm-owned state; if not, the Send bounds must change before H3.
- What is the actual time-to-first-frame split on Windows, macOS and Linux across host font scan, wgpu adapter/device, pipeline creation, first build/layout and first present? None of it is measured; the host font scan is a hypothesis for the biggest single item.
- What is the real cost of the global topology sync and multi-pass lazy building during a fast fling over a 100k list? No bench wires a ChildManager (virtualizer.rs says so); an end-to-end harness is needed before prioritising #1039 against lazy-in-layout.
- How will the 'not worse than Flutter' comparison in H2's exit be run: a mirrored Flutter app suite with identical scenarios, and on which hardware? No harness or scenario list exists.
- Parley migration (ADR-0077): will the font collection be shared read-only with per-realm shaping contexts and a raster-owned glyph cache? That is the natural fix for the UI↔raster font-lock coupling and should be stated in the ADR's acceptance criteria.
- Should repaint-boundary placement stay a catalog convention, or become automatic (retaining the root, promoting frequently repainted subtrees)? That affects how much tuning burden H2 users carry.

