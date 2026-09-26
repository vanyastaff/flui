# Codebase map: view_element: flui-view, the framework spine (View/Element/ViewState, BuildContext/LifecycleContext, reconciliation, keys, inherited data with field masks, signals, BuildOwner, element arena and IDs, WidgetsBinding) and how it composes with flui-rendering, flui-app, flui-hot-reload and the catalog

_Raw output of the `map:view_element` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

flui-view (layer 5, about 54k lines, 2.9k in build_owner.rs outside tests, 2.9k in element_tree.rs outside tests, 2.9k in binding.rs) is a faithful, heavily hardened port of Flutter's widgets/framework.dart, rebuilt on a slab arena.

How it works today:
- **Views.** A view is `View: Downcast + DynClone + 'static`, created by the user each build. Each view type monomorphises an `Element<V, A, B>` (view, arity marker, behavior), boxed behind a closed `ElementKind` enum of trait objects.
- **Storage.** Elements live in a `Slab<ElementNode>` inside `ElementTree`, with a parallel generation vector. `ElementId` is a packed u64 of generation and index. A node owns its `child_ids`, its key, and an `Arc<HashMap<TypeId, ElementId>>` inherited-provider scope, which makes the lookup O(1).
- **Build.** `BuildOwner::build_scope` pops a depth-ordered `BinaryHeap` and takes each dirty element out of its slot by value. It builds the element against a borrowed read-only `BuildCtx` over the live tree, with no locks inside the drain, then puts the element back. It then passes the returned `Vec<Box<dyn View>>` to `reconcile_children_by_id`, a Flutter-shaped top/bottom scan plus a key map that emits typed `ReconcileEvent`s.
- **Cross-thread and listener rebuilds** enter through an `Arc<Mutex<HashMap>>` external inbox. It is drained on every pop, bounded by a per-frame re-entry budget.
- **Inherited dependencies** are field-masked (`FieldMask<D>` from `#[derive(InheritedData)]`) and re-derived on every build.
- **Signals** (feature `signals`, off by default) are a `Reactive` arena owned by each `BuildOwner`. Reads in build register the element as a reader; writes schedule those readers through the same inbox.
- **Capabilities** are split by type: `BuildContext` in build, the sealed `LifecycleContext` in `init_state` and `did_change_dependencies` (ADR-0078).
- **Panics.** Build panics are contained per element (`build_or_recover` produces an `ErrorView`).

Composition:
- `WidgetsBinding` wraps `BuildOwner` and `ElementTree` in `Arc<RwLock<WidgetsBindingInner>>`. flui-app creates one binding per presentation (ADR-0043), and the realm owns only cross-tree services and the `GlobalKeyScope`.
- Render elements adopt their own render objects in `PipelineOwner` at mount ("child-adopts-itself").
- LayoutBuilder, the lazy sliver adaptors, persistent headers and Future/StreamBuilder are elements implemented inside flui-view, because they need crate-private `BuildOwner` registries (the layout-builder registry, the child manager).

Verdict: the build/reconcile core is sound and well tested, and a good spine for a Router (keyed page lists, GlobalKey reparenting, `RouteInformation`). Several shapes around it fit the owner's plan poorly:
- the signal graph belongs to a presentation, not a realm;
- capabilities are a closed sealed method list, so H1 `PlatformCapability` plugins cannot add one;
- custom elements are closed to other crates;
- the element tree has no names or properties, so agents, devtools, the G6 catalog and A2UI cannot read it;
- there is no hot-reload state-migration hook;
- view configs are deep-cloned at every level;
- the public surface is huge and unfrozen: 410 `pub fn`, 39 `pub trait`, re-exported wholesale as `flui::view`, including `ElementTree::insert` and `get_mut`, and test-only contexts.

## Responsibilities and boundaries

What flui-view owns, correctly:
- the View/ViewState/InheritedView/RenderView authoring traits;
- the element arena, lifecycle (mount, activate, deactivate, unmount, finalize), the dirty heap and build scheduling;
- keyed reconciliation, GlobalKey identity and reservations (ADR-0050);
- inherited scopes with field masks, the signal reader registry, the `BuildContext`/`LifecycleContext` split, rebuild reasons and frame build reports, and panic recovery.

Where the boundary leaks:
1. **Catalog widgets in the spine.** LayoutBuilder, SliverList, SliverGrid, SliverFixedExtentList, the persistent-header elements (with `flui_animation::AnimationController`) and FutureBuilder/StreamBuilder are implemented in `flui-view/src/element/*` and re-exported by flui-widgets (`flui-widgets/src/async_builders.rs:1`, `layout/layout_builder.rs:13`, `scroll/sliver_list.rs:17`). That is why flui-view depends on flui-objects (the concrete render catalog: `element/sliver_adaptor.rs:58`, `owner/layout_builder.rs:42`) and on flui-animation. The cause is structural: `ElementOwner`'s fields are all `pub(crate)` (`owner/element_owner.rs:115-150`), so no other crate can write an element that participates in the build-during-layout or lazy-child protocols.
2. **Sliver and parent-data concerns in the base trait.** `ElementBase` has 41 methods, including `child_sliver_slot`, `hosts_sparse_children`, `sliver_slot_for_child`, `parent_data_*` and `set_pipeline_owner`.
3. **The frame driver lives here.** `WidgetsBinding` (Flutter's name, plus lifecycle, observers, predictive-back) is really the per-presentation frame driver and arguably belongs to the app/runtime layer. Keeping it here couples flui-view to app-lifecycle vocabulary.
4. **Porous internal seams.** The internal seam for flui-app and hot reload is a cargo feature (`runtime-internals`). flui-app enables it as a normal dependency (`crates/flui-app/Cargo.toml:90`), and cargo unifies features, so every real app compiles the "internal" API.
5. **Wholesale facade export.** The facade exports the whole crate (`src/lib.rs:151 pub use flui_view as view`), so the arena internals (`ElementTree::insert/remove/get_mut`, `ElementCore`, `ElementBehavior`, `ElementKind`, `test_only_set_global_key_registry`) are public application API.

What belongs elsewhere:
- the concrete lazy-list and layout-builder elements belong in flui-widgets, behind a public element-extension protocol;
- the binding and frame driver belong in flui-app, or in a thin runtime module;
- the error-view factory belongs in realm configuration, not a process static.

## Key types and contracts

- `View: Downcast + DynClone + 'static` { create_element() -> ElementKind; view_type_id; can_update (TypeId + key); should_skip_rebuild (opt-in, Memo<V>); key() } — crates/flui-view/src/view/view.rs:57-160
- `ElementKind` (#[non_exhaustive] enum of Box<dyn *ElementBase> sub-trait objects; RenderLeaf/RenderSingle/RenderOptional never constructed) — element/kind.rs:317-360
- `ElementBase` (41 object-safe methods: lifecycle, update(&dyn View), build_into_views -> Vec<Box<dyn View>>, render/sliver/parent-data wiring, as_inherited, state_as_any) — view/view.rs
- `Element<V, A: ElementArity, B: ElementBehavior<V,A>>` + `ElementCore<V,A>` (per-view-type monomorph; arity A carries no storage) — element/unified.rs, element/generic.rs
- `ElementTree { nodes: Slab<ElementNode>, generations: Vec<NonZeroU32>, ... }`; `ElementNode { kind: Option<ElementKind> (hole during build), parent, depth, slot, key: Option<Box<dyn ViewKey>>, child_ids: Vec<ElementId>, inherited: Arc<HashMap<TypeId, ElementId>> }` — tree/element_tree.rs:61-160, 395-420
- `ElementId(NonZeroU64)` = generation<<32 | slab index (not the 1-based NonZeroUsize that AGENTS.md's ID-offset rule describes) — flui-foundation/src/id.rs:1163
- `BuildOwner` { dirty_elements: BinaryHeap<Reverse<DirtyElement>>, dirty_reasons, global_keys, global_key_reservations, inactive_elements, inherited_dependencies, reactive: Reactive, keep_alive, external_inbox: Arc<Mutex<HashMap<ElementId, RebuildReasons>>>, mid_drain_absorbs_left } — owner/build_owner.rs:376-508
- `ElementOwner<'a>` split-borrow handle into BuildOwner (all fields pub(crate)) — owner/element_owner.rs:115
- `BuildContext: Sealed` (identity, depend_on_inherited[_fields], get_inherited (non-depending), find_ancestor_*, mark_needs_build, dispatch_notification, #[cfg(signals)] reactive/signal_read) and `LifecycleContext: BuildContext` (rebuild_handle, async_driver, post_frame_handle, text_input_handle, hit_test_handle, keep_alive_*, focus_manager, pipeline_owner) — context/build_context.rs:106-565
- Two BuildContext impls: production `BuildCtx<'b>` over `&ElementTree` (context/element_build_context.rs:715) and public `ElementBuildContext` over `Arc<RwLock<ElementTree>>` + `Arc<RwLock<BuildOwner>>` (same file :39), the latter used only by tests
- `StatefulView { type State; create_state }`, `ViewState<V> { init_state(&dyn LifecycleContext), did_change_dependencies, build(&self, &V, &dyn BuildContext) -> impl IntoView, did_update_view, activate/deactivate/dispose }` — no reassemble / state-migration hook — view/stateful.rs:76-178
- `InheritedView { type Data; data; child; update_should_notify; changed_fields }`, `FieldMask<D>` / `FieldSet`, `#[derive(InheritedData)]` — view/inherited.rs
- `Signal<T>` (Copy handle: graph id + index + generation; get/with take &dyn BuildContext; set/update take &Reactive), `Reactive(Rc<RefCell<Inner>>)` one per BuildOwner, `SignalSender` — reactive/mod.rs:161, 648-760
- `RebuildHandle` (Clone + Send + Sync, schedules through the external inbox) — owner/rebuild_handle.rs:90
- `GlobalKey<T>::current_element / with_current_state(&T)` resolved through a thread-local registry stack activated by `UiRealm::enter` — key/global_key.rs:109-160, key/registry.rs:198-212
- `WidgetsBinding { inner: Arc<RwLock<WidgetsBindingInner{build_owner, element_tree, ...}>>, on_need_frame: RwLock<Option<Box<dyn Fn()+Send+Sync>>>, ... }` — binding.rs:473-560
- `RenderView { type Protocol; type RenderObject: RenderObject<P> + Send + Sync; create_render_object; update_render_object -> RenderUpdateImpact; has_children; visit_child_views(&mut dyn FnMut(&dyn View)) }` — view/render.rs:445-497
- `ReconcileEvent` {Mount, Reuse, Reorder, Unmount, Reparent} on tracing target flui::reconcile; `FrameBuildReport` / `RebuildReason` (InitialMount, ParentUpdate, StateChange, DependencyChange, ..., HotReload, SignalChange)

## Dependencies

**Outgoing (normal dependencies):**
- lower layers: flui-foundation, flui-types, flui-tree (used only for the `Arity` markers and `IndexedSlot`; `ElementTree` implements none of flui-tree's `TreeRead`/`TreeNav`/`TreeWrite`, while the render, layer and semantics trees do), flui-macros (the View derives);
- flui-rendering (`PipelineCell`, `RenderObject`, `SliverSlot`, `RenderUpdateImpact`);
- flui-interaction (focus, text input, hit-test, `InteractionDispatchHandle` for `RenderObjectContext`);
- flui-scheduler (`AsyncDriver`, post-frame handles, re-exported);
- flui-animation (only for the persistent header's `AnimationController`);
- flui-objects, the concrete render catalog: RenderLayoutBuilder, `LayoutConstraintsCell`, `BuildDuringLayoutCell`, RenderSliverList/Grid/FixedExtentList, persistent-header render objects, RenderErrorBox;
- slab, parking_lot, smallvec, downcast-rs, dyn-clone, futures-core, thiserror, tracing.

**Dev-dependencies:** a dev-only cycle with flui-testing, which is intentional.

**Incoming:**
- flui-widgets, flui-material, flui-cupertino, flui-localizations (authoring surface);
- flui-testing (`runtime-internals`);
- flui-app (`runtime-internals` and `signals`; one `WidgetsBinding` per presentation at `crates/flui-app/src/app/presentation.rs:501` and `:647`; `UiCommand::SignalWrite` routed to `self.widgets()`, the primary presentation, at `app/ui_realm/commands.rs:450-455`);
- flui-hot-reload (`app-plugin` feature: each dylib plugin owns its own `WidgetsBinding`, `pipeline.rs:1-10`);
- flui-devtools (`TreeObserver`);
- the facade (the whole crate, as `flui::view`).

## Fit with the plan

**Principle 1 (sacred mental model).** Strongly supported. View → Element → Render with keys, lifecycle order and a depth-ordered rebuild is faithfully implemented and pinned by tests.

**Principle 3 (no global state, ambient reach = 0).** Partly violated:
- a process-static `ERROR_VIEW_BUILDER` (`view/error.rs:41`);
- a thread-local GlobalKey registry stack that `GlobalKey::current_*` reads with no context argument (`key/registry.rs:198`);
- a process-wide graph-id counter, which is acceptable.

**Principle 4 (machine-readable, stable IDs).** Only half-met:
- met: typed `ReconcileEvent`s, `FrameBuildReport` and `TreeObserver` exist;
- not met: elements expose only a `TypeId`, with no type name, properties or diagnostics (`ElementBase::debug_description` prints `TypeId`, `view/view.rs:460`). The G6 catalog, the agent inspector and A2UI therefore cannot read the view layer, only semantics.

**H0.**
- *State (A3/A4).* ADR-0074's signals are implemented but feature-gated off. The graph is per `BuildOwner`, which since ADR-0043 means per presentation, not per realm, contrary to the ADR's own wording. Readers are element-granular only, so the plan's open question (element- vs render-level granularity) cannot be answered without new plumbing. ADR-0075 (`Computed`/`Effect`) is Proposed and absent, as are reactive collections (A8).
- *Router (D1).* The spine carries it well: keyed page lists, GlobalKey reparenting with render relocation, `RouteInformation` and observers. Two things are missing. There is no state-restoration concept anywhere in flui-view, which desktop restore and deep-link restore need. And `GlobalKey::with_current_state` hands out only `&T`, so imperative facades (`Navigator::push`, `Form::validate`) force interior mutability into every state.
- *Hot reload via Subsecond (G4).* There is one good hook point (`build_or_recover` is the single choke point every build goes through) and `BuildOwner::reassemble` marks everything dirty. But `ViewState` has no reassemble or migrate hook, there is no state-layout fingerprint, and states live as typed `Box<dyn …>` with vtables minted before a patch (hypothesis about Subsecond semantics). The in-tree hot-reload crate instead runs a separate binding per dlopen'd plugin, so state is not migrated.

**H1.**
- *`PlatformCapability` plugins.* Blocked by design: capabilities are fixed methods on a sealed `LifecycleContext` inside flui-view (ADR-0078; AGENTS.md says "a method on LifecycleContext"), so an out-of-repo plugin cannot add one.
- *A2UI.* Needs dynamic view construction (possible through `BoxedView`), a view catalog with names and schemas (absent), data binding to signals (gated, and per presentation), and custom list elements (closed to other crates).

**H2 (performance and scale).**
- Configs are deep-cloned per level (`dispatch.rs:145`, `behavior.rs:1071-1072`).
- There is a `Box<dyn View>` per child per build and an `Arc<AtomicBool>` dirty flag per element.
- A signal fan-out takes one `Mutex` inbox insert per reader (ADR-0074 §8.1: 600 readers are slower than `setState`).
- Appending one row to a 10k list rebuilds 20k elements unless `Memo` is used.
- These are the known pressure points against the 100k-row goal.

**H3 (freeze by tiers).** Hard with the current surface: the whole crate is re-exported, 410 `pub fn` and 39 `pub trait`, including arena internals and test-only functions. Views, contexts and state must be separated from element and arena machinery before a Stable tier is possible.

**H4 (community element and render extensions).** Render objects are extensible (flui-rendering); elements are not.

## Strengths

- By-value element extraction during build (`ElementNode.kind: Option<ElementKind>` hole + `BuildCtx` over `&ElementTree`) gives build a live, lock-free, read-only view of the real tree. This is safe Rust with no aliasing, and there is no detached fallback context (ARCHITECTURE.md 'Build contexts are live during build').
- Generational `ElementId` (NonZeroU64 generation<<32 | index) with a parallel generations vector: a stale id resolves to None instead of an unrelated recycled element (tree/element_tree.rs:396-407). The niche is asserted at compile time (flui-foundation/src/id.rs:1166).
- The capability split is enforced by the type system: `LifecycleContext` is received only by `init_state`/`did_change_dependencies`, both traits are sealed, and the object-safety asserts are compiled (context/build_context.rs:847-848, ADR-0078).
- Field-precise inherited dependencies with typed `FieldMask<D>` (a foreign mask is a compile_fail doctest), plus reset-on-build dependency re-derivation. This deliberately improves on Flutter's accumulate-until-unmount, and it is recorded and tested (ADR-0074 §5.5).
- O(1) inherited-provider lookup through a per-node `Arc<HashMap<TypeId, ElementId>>`, copy-on-insert only at providers (element_tree.rs:127-157).
- The reconciler is written extract-then-apply with no long-lived slab borrows and no unsafe (tree/id_reconcile.rs:35-58), emits typed `ReconcileEvent`s on the production path, and checks key hash collisions with `key_eq`.
- The rebuild machinery is robust: a depth heap re-keyed to authoritative tree depth, same-drain absorption of mid-drain schedules with a per-frame re-entry budget, per-element panic containment producing an ErrorView, and `RebuildReason` / `FrameBuildReport` that tests and agents can assert on (e.g. the ADR-0074 §8.1 counts).
- The signal design avoids hooks: explicit ownership (element or graph), Copy handles, generation and graph-id checks returning typed `SignalError`, and run-time refusal of writes or creation during build. It reuses the existing scheduler rather than adding a second invalidation path.
- Divergences from Flutter are documented unusually rigorously in ARCHITECTURE.md `## Mapping decisions`, with pinned test names, which makes the spine auditable.

## Problems

### The signal graph is per presentation (BuildOwner), not per realm, so a signal cannot be shared across windows and realm SignalWrite always targets the primary window

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** BuildOwner owns `reactive: Reactive` built in `BuildOwner::new` (crates/flui-view/src/owner/build_owner.rs:444, 722). flui-app creates one WidgetsBinding (and so one BuildOwner) per presentation (crates/flui-app/src/app/presentation.rs:501, 647; ADR-0043). `UiCommand::SignalWrite` applies against `self.widgets()` = `self.presentations.primary().widgets()` (flui-app/src/app/ui_realm/commands.rs:450-455; ui_realm/presentations.rs:357-358). reactive/mod.rs:3 says 'One Reactive graph per BuildOwner (so per realm)', and ADR-0074 §1.2(1)/§5.1 promise realm-owned app state that screens borrow. Slots carry a graph id, so a handle minted in window A and read in window B's build is `SignalError::ForeignGraph`, and a panic through `get`.
- **Impact:** Breaks ADR-0074's core promise (a settings value read on every screen, state that outlives a screen) as soon as a realm has two presentations. Blocks 'multi-window as the norm' (H2) and makes A2UI/app-state models window-local. Readers are stored as per-tree ElementIds scheduled through one tree's inbox, so the registry cannot hold readers from several trees without redesign.
- **Direction:** Move `Reactive` to the realm, next to `GlobalKeyScope`. Key readers by (presentation/owner id, ElementId) and schedule through each owner's inbox. Correct reactive/mod.rs and ADR-0074, or supersede it. Add a two-presentation test that reads one signal in both trees and writes it through SignalWrite.

### Capabilities are a closed method list on a sealed trait, so H1 PlatformCapability plugins have no way in

- **Kind:** extension_point · **Severity:** high
- **Evidence:** `LifecycleContext: BuildContext` is sealed and enumerates rebuild_handle, async_driver, post_frame_handle, text_input_handle, hit_test_handle, keep_alive_*, focus_manager and pipeline_owner as fixed methods (context/build_context.rs:377-565). ADR-0078 §1: 'A new capability is a method on LifecycleContext, never on BuildContext'. AGENTS.md 'Extending FLUI' says the same for a platform capability. plan.md H1: 'PlatformCapability: typed registration of a native capability without string channels; a plugin is an ordinary crate'.
- **Impact:** Every camera, file, geo or notification capability would have to be a new method in flui-view and, per AGENTS.md, a backend in flui-platform. That contradicts the plan's '5 plugins built outside the repo' exit for H1, and makes flui-view a bottleneck crate that knows every OS service. It also grows the Stable surface at every addition.
- **Direction:** Before H1 (ideally before any H3 freeze), add one generic typed lookup, `LifecycleContext::capability::<C: PlatformCapability>() -> Result<C::Handle, Unsupported>`, backed by a realm-owned registry that plugins register into at app build time. Keep the build/lifecycle split, which keeps ADR-0078's guarantee. Migrate the existing handles onto it, or keep them as sugar. Record this in an ADR that supersedes ADR-0078's clause.

### The element extension protocol is closed: layout-time and lazy-list elements must live inside flui-view, dragging the render catalog into the spine

- **Kind:** layering · **Severity:** high
- **Evidence:** No crate outside flui-view implements ElementBase or constructs an ElementKind variant (grep over flui-widgets/material/app src finds only a pattern match at flui-widgets/src/localization/directionality.rs:274). `ElementOwner` fields are all pub(crate) (owner/element_owner.rs:115-150). LayoutBuilder, SliverList/Grid/FixedExtentList (element/sliver_adaptor.rs, 2750 lines), the persistent headers and Future/StreamBuilder live in flui-view/src/element and are re-exported by flui-widgets (async_builders.rs:1, layout/layout_builder.rs:13, scroll/sliver_list.rs:17). This is why flui-view (layer 5) depends on flui-objects for RenderLayoutBuilder/RenderSliverList and on flui-animation (element/sliver_adaptor.rs:58, owner/layout_builder.rs:42-46, element/sliver_persistent_header.rs:58).
- **Impact:** Every new build-during-layout or virtualized widget (a Router transition host, an A2UI list, a table, a community virtualized grid, reactive collections A8) must be written inside the spine by the author. The spine keeps growing with catalog code (54k lines) and cannot be frozen independently. This is the extension-point gap for H1 A2UI and H4 community crates.
- **Direction:** Define a small public element protocol: a stable `ElementBehavior`-like trait with a narrow `ElementOwner` facade (schedule, register layout callback, child-manager hooks). Move the concrete sliver-adaptor, layout-builder and async builders to flui-widgets, and drop the flui-view → flui-objects and flui-animation edges. Alternatively seal the element layer explicitly and state that custom elements are core-only; then list every needed primitive (lazy children, layout callback) as a core concept.

### The spine has no machine-readable identity or properties, so agents, the G6 catalog and A2UI cannot see the view layer

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** An element exposes only `view_type_id: TypeId` (ElementBase::debug_description prints TypeId, view/view.rs:460-467). TreeObserver events carry only `view_type_id: TypeId` (flui-foundation/src/observe.rs:26-43). View has no `type_name`, properties or `Diagnosticable` impl (the Diagnosticable impls in flui-view are test boxes only). ElementTree implements none of flui-tree's TreeRead/TreeNav traits, which the render, layer and semantics trees do.
- **Impact:** Principle 4 ('all machine-readable, stable IDs') and the 2030 view ('one catalog, three consumers: developer, MCP agent, A2UI model') need a catalog keyed by stable names with typed props. TypeId is neither stable across builds nor human-readable. Devtools and the MCP agent must fall back to the semantics tree, which cannot show widget structure, keys or rebuild causes by name.
- **Direction:** Give `View` a derive-generated `const TYPE_NAME` / stable catalog id and an optional `describe(&self, &mut PropertySink)`. Carry the name in TreeObserver events. Implement flui-tree's read/nav traits for ElementTree so the inspector is generic over all trees. The A2UI catalog (G6) then becomes a registry of (name → schema, constructor) built by the same derive.

### View configs are deep-cloned at every tree level on every rebuild (update(&dyn View) plus DynClone)

- **Kind:** performance · **Severity:** medium
- **Evidence:** `ElementBase::update(&mut self, new_view: &dyn View, ...)` takes a borrow, so dispatch must `dyn_clone::clone_box(effective)` before downcasting (element/dispatch.rs:145). A render parent's `build_into_views` first `clone_box`es each child out of its own config (element/behavior.rs:1071-1072), and `Child`/`Children` are `Box`-owned views that clone recursively (child/child.rs, child/children.rs:38-124). Proxy and root views clone the same way (behavior_commons.rs:305, view/root.rs:344). `ElementNode.key: Option<Box<dyn ViewKey>>` is re-cloned at every update (element_tree.rs:81-92). ADR-0074 §8.1: appending one row to a 10k list rebuilds 20,003-20,005 elements in 34-36 ms.
- **Impact:** Rebuild cost grows with config size times depth: a descendant config nested in RenderView chains is cloned about twice per ancestor level (from reading the code; the magnitude is not measured). This is the main H2 risk for 100k rows and a 1 MB editor, and it penalises the natural `Column(children)` authoring style.
- **Direction:** Make reconciliation consume owned child views: `build_into_views` returns `Vec<Box<dyn View>>`, which should be moved into `update(Box<dyn View>)` instead of borrowed. Give RenderView children an owned-drain path (`take_child_views(self)`) or `Rc`-shared child configs so an unchanged subtree is a pointer compare. Measure with the existing `dense_update_reconcile` bench before and after.

### The binding holds a RwLock write guard across all user build code; lock-based APIs re-enter it

- **Kind:** safety · **Severity:** medium
- **Evidence:** `draw_frame_impl` takes `self.inner.write()` and runs `build_owner.build_scope(element_tree)` under it (binding.rs:1240-1297). The production GlobalKey registry closures call `inner.read()` on the same parking_lot RwLock (binding.rs:689-709). A deadlock of exactly this shape already happened: `debug_building_dirty_elements` was hoisted to an atomic to avoid it (binding.rs:497-508). The field doc calls the lock 'the pre-lease interior-mutability shape (C5 endgame removes it)' (binding.rs:476-480). flui-material's `DrawerHandle::open_drawer` resolves `GlobalKey::with_current_state` (flui-material/src/drawer.rs:398-433).
- **Impact:** Hypothesis: any `GlobalKey::current_element()` / `with_current_state` reached synchronously from build, did_update_view or a lifecycle hook during a frame deadlocks on the same thread, instead of returning None or an error. AGENTS.md's own rule says a lock in the frame path is contention and deadlock risk. The Arc<RwLock> also contradicts ADR-0027's `!Send` realm (lib.rs:69 `#![expect(clippy::arc_with_non_send_sync)]`).
- **Direction:** Finish the 'C5 endgame': make WidgetsBinding owner-local (`RefCell`/`&mut` threading from UiRealm) with no RwLock. During a drain, the GlobalKey registry should resolve through the drain's `BuildCtx`, or return a typed `Busy` error. Add a test that calls `with_current_state` from `did_update_view`.

### The public API is the whole crate: arena internals, test-only contexts and test shims are application API

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** The facade does `pub use flui_view as view` (src/lib.rs:151). flui-view has 410 `pub fn`, 80 `pub struct`, 39 `pub trait` and 22 `pub mod`. Among them: `ElementTree::{insert, remove, get_mut, update, deactivate, activate}` (tree/element_tree.rs:803-2218), `ElementCore`, `ElementBehavior`, `ElementKind`, `test_only_set_global_key_registry` (lib.rs:87-117, 194-196), and `ElementBuildContext`/`ElementBuildContextBuilder`, which have no production caller (only tests/ancestor_finders.rs, build_context_tests.rs, inherited_dependency.rs and others). `runtime-internals` is enabled by flui-app's normal dependency (crates/flui-app/Cargo.toml:90), so feature unification makes the 'internal' seam present in every app.
- **Impact:** H3 plans a Stable tier for 'View/State/Element, layout protocol'. With this surface, every arena refactor becomes a semver break, and users can corrupt the tree through `get_mut`. The size also hides the real authoring API (roughly 15 traits) from newcomers and agents.
- **Direction:** Split the crate by visibility: an authoring module (View, StatelessView/StatefulView/ViewState, InheritedView, RenderView, contexts, keys, Signal) re-exported by the facade, and `#[doc(hidden)] pub mod __runtime` for flui-app, flui-testing and hot reload, or a separate internal crate at the same layer. Delete `ElementBuildContext`, or make it `cfg(test)`. Use cargo-semver-checks / public-api snapshots as the gate.

### Two BuildContext implementations; the lock-based one that tests exercise is not the production path

- **Kind:** testing · **Severity:** medium
- **Evidence:** Production builds use `BuildCtx<'b>` over `&ElementTree` (context/element_build_context.rs:715-1050). The public `ElementBuildContext` holds `Arc<RwLock<ElementTree>>` and `Arc<RwLock<BuildOwner>>` and reimplements depend_on_inherited_fields, get_inherited and the ancestor walks with its own lock ordering (element_build_context.rs:39-53, 239-343). ElementBuildContext has no non-test caller; it is used by crates/flui-view/tests/build_context_tests.rs (38 refs), ancestor_finders.rs (17), inherited_dependency.rs (11) and notifications.rs (6).
- **Impact:** Tests of inherited dependencies and ancestor lookups can pass while the production `BuildCtx` diverges: the AGENTS.md 'tests passed both ways' failure mode, built into the structure. The duplicate also doubles the maintenance cost of every BuildContext method (a signals method, field masks, and a future capability lookup).
- **Direction:** Delete ElementBuildContext. Port those tests to drive `BuildOwner::build_scope` (or flui-testing's headless binding) so every context assertion runs through `BuildCtx`.

### Hot-reload state migration has no hook in the spine; the in-tree hot reload does not preserve element state

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** `ViewState` has init_state, did_change_dependencies, build, did_update_view, activate, deactivate and dispose, but no `reassemble`/migrate (view/stateful.rs:106-178). `BuildOwner::reassemble` only marks every element dirty (owner/build_owner.rs:1018-1031). flui-hot-reload loads dylib plugins that each own their own `WidgetsBinding` ('never shares mutable UI state with the host's UiRealm', flui-hot-reload/src/pipeline.rs:1-10). No subsecond dependency exists. The roadmap (G4) calls for 'a subsecond::call point in the build loop and migration of element state on patch'.
- **Impact:** G4/B1 exit ('flui run --hot preserves state') needs three things the spine lacks: (a) routing every `ViewState::build` through a hot-patchable call, (b) detecting that `V::State`'s layout changed, then recreating the state through `create_state` instead of reusing a stale layout, and (c) re-creating elements whose vtables predate the patch. Hypothesis: Subsecond patches calls made through its jump table, not vtables in already-boxed trait objects.
- **Direction:** Add a spine-level hot-reload contract. `build_or_recover`, the single choke point, wraps the build in `subsecond::HotFn` behind a feature. `ViewState` gets an optional `reassemble(&mut self)`. The derive emits a per-state-type layout fingerprint, so `reassemble` can recreate mismatched states and keep matching ones. Spike this before B1, as the roadmap itself flags it as a risk.

### Signals are element-granular only, feature-gated off, and carry no widget-input or collection story

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** `signals = []` is not in default features (crates/flui-view/Cargo.toml:123), and 39 `cfg(feature = "signals")` sites exist, including methods on the sealed BuildContext (context/build_context.rs:131-137). No catalog crate accepts or reads `Signal<T>` (grep over flui-widgets/material/src finds none). The reader registry is `SmallVec<[ElementId; 4]>` per slot (reactive/mod.rs:137) with no render-object or paint readers. A write schedules each reader individually through `Arc<Mutex<HashMap>>` (owner/build_owner.rs:495). ADR-0074 §8.1: 600 readers take 1.77 ms against 1.01 ms for setState. ADR-0075 Computed/Effect is Proposed; A8 reactive collections are absent.
- **Impact:** 'Signals as canon' (A3, H0) is not yet what users get by default. Animation-rate values (scroll offset, drag position) cannot bind to layout or paint without a rebuild and diff. The plan's open question 'element-level or render-level granularity' has no mechanism for its second answer. Lists (the Notes app, A2UI data models) need keyed reactive collections that the spine does not have.
- **Direction:** Decide go/no-go and make signals default in H0. Add a render-level reader kind (a signal bound to a RenderObject property, marking needs_layout/needs_paint directly), batch scheduling per write (#1249), land ADR-0075 with its six tests, and design `SignalVec`/keyed collection diffs feeding the reconciler (A8) before A2UI.

### Documented invariants disagree with the code (ID offset, depth, 'no read path without dependency', UNIFIED_ELEMENT.md)

- **Kind:** docs · **Severity:** medium
- **Evidence:** AGENTS.md's 'ID offset' rule says public IDs are 1-based `NonZeroUsize` (slab_index + 1), but `ElementId` is `NonZeroU64` generation<<32 | index (flui-foundation/src/id.rs:1163-1175). `ElementBase::depth()` is documented as 'depth in the element tree (root = 0)', but ElementCore stores the sibling slot there ('NOT the tree depth', element/generic.rs:77-79, 151), and BuildOwner has to re-key the heap because of it (owner/build_owner.rs:1033-1057). ARCHITECTURE.md says 'There is no read path that does not record a dependency', yet `BuildContext::get_inherited` / `BuildContextExt::get` are public non-depending reads (context/build_context.rs:189-199, 700-715) used in widgets (flui-widgets/src/animated/animated_align.rs:123). UNIFIED_ELEMENT.md still describes `dependents: Vec<ElementId>` and 'Phase 1/2' shapes. ARCHITECTURE.md itself says 'a full crate architecture writeup is deferred'.
- **Impact:** The spine is the crate agents and new contributors must understand first. Wrong invariants in AGENTS.md and ARCHITECTURE.md make reviews and agent-written code confidently wrong, and the duplicated `depth` state is a latent ordering bug class.
- **Direction:** Fix the AGENTS.md ID rule. Remove `ElementCore.depth` (use ElementNode.depth as the single authority) or rename it to `slot`. Either drop `get`/`get_inherited` from build (move them to LifecycleContext) or correct the doc. Replace UNIFIED_ELEMENT.md with a real ARCHITECTURE.md section on the build pipeline (drain → take/put → reconcile → finalize).

### Dead generic dimensions and variants in the element taxonomy (arity parameter, three render variants, animation_listener)

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** `ElementKind::RenderLeaf/RenderSingle/RenderOptional` are never constructed outside a non_exhaustive smoke test (element/kind.rs:342-350; tests/element_kind_non_exhaustive_smoke.rs:48-50). `Stateful { animation_listener: Option<AnimationListener> }` is always None (kind.rs:387-400, 442-456; no `animation_listener: Some` anywhere). `ElementArity` carries no storage (element/arity.rs is marker impls only), and children always live in `ElementNode.child_ids`. The sub-traits `StatelessElementBase` etc. are empty markers, and every call goes through `element()`/`element_mut()` → `&dyn ElementBase` (kind.rs:476-517), so the 'per-position monomorphism' claim in kind.rs:14-19 does not hold. RenderView child arity is run-time (`has_children`/`visit_child_views`, view/render.rs:483-497), contrary to the 'compile-time child arity' stance in AGENTS.md. 138 process markers (Phase N, E3, FR-0xx, Wave N) in flui-view/src break the AGENTS.md rule.
- **Impact:** The machinery makes the element layer look more typed than it is, adds three type parameters to every element monomorph (the file itself notes 130+ instantiations, 13.7% of flui-widgets' LLVM lines, element/generic.rs:62-72, which hurts compile times and hot-reload iteration), and misleads contributors.
- **Direction:** Collapse ElementKind to the variants in use, or to `Box<dyn ElementBase>` plus a behavior tag, and drop the `A` parameter from `Element`. Either make RenderView arity a real associated type that the reconciler uses, or stop claiming it. Strip the process markers.

### Process-global and ambient state in the spine (error-view builder, thread-local GlobalKey registry)

- **Kind:** safety · **Severity:** low
- **Evidence:** `static ERROR_VIEW_BUILDER: RwLock<Option<ErrorViewBuilder>>` with `set_error_view_builder` (view/error.rs:41-58) needs a dedicated test process (Cargo.toml `[[test]] error_view_recovery`). `GlobalKey::current_element()` reads a thread-local `REGISTRY_STACK` activated by `UiRealm::enter` (key/registry.rs:198-212; key/global_key.rs:109-111) and silently returns None outside realm entry. `FlutterError` is a public type name (lib.rs:201).
- **Impact:** Violates principle 3 (the ambient-reach ratchet): two realms cannot have different error views, and GlobalKey lookups depend on invisible thread state. This is small today but becomes Stable API at H3.
- **Direction:** Move the error-view builder into realm or app configuration read through the owner. Give GlobalKey lookups an explicit context (`key.current_state(cx)`, or through `LifecycleContext`) and keep the thread-local only as an internal fallback. Rename the Flutter-branded public types.

### Imperative state access through GlobalKey is read-only, pushing interior mutability into every controller-like state

- **Kind:** api_dx · **Severity:** low
- **Evidence:** `GlobalKey::with_current_state<R>(&self, f: impl FnOnce(&T) -> R)` is the only state accessor, with no `&mut` variant (key/global_key.rs:138-160); `find_state` in BuildContextExt is `&S` too. flui-material's drawer opens through `with_current_state(DrawerControllerState::open)`, where `open(&self)` relies on interior-mutable controllers (flui-material/src/drawer.rs:398-433, 651-661).
- **Impact:** Router's imperative facade (`push/pop`), Form (C1: validate/save/reset) and Scaffold APIs all need mutation. Each state then grows `RefCell`/`Cell` fields and a rebuild-scheduling convention, which is the kind of ad-hoc state model signals are meant to replace.
- **Direction:** Define the imperative-controller pattern once: either a `with_current_state_mut` that schedules a rebuild through the element's RebuildHandle and is refused during build, or a documented rule that controllers are signals or realm-owned handles, not states. Decide it in the Router ADR.

## Unwired or dead surface

- `ElementBuildContext` / `ElementBuildContextBuilder`: public, no production caller (test-only in crates/flui-view/tests/*.rs and element_tree.rs test module at 5408/5463)
- `ElementKind::RenderLeaf`, `RenderSingle`, `RenderOptional`: never constructed (element/kind.rs:342-350)
- `ElementKind::Stateful { animation_listener }`: always None; `AnimationListener` is public but unused (element/kind.rs:241-275, 387-456)
- `ElementArity` type parameter on `Element<V, A, B>` and the `RenderElementBase<A>` arity family: marker-only, children are stored in ElementNode.child_ids
- Predictive-back surface on WidgetsBindingObserver / WidgetsBinding (`handle_*_back_gesture`, `back_gesture_observers`), self-marked `REMOVE_BY: 2026-12-22` because no platform dispatches it (binding.rs:531-541, 1637-1700)
- `test_only_set_global_key_registry` / `test_only_clear_global_key_registry` exported from the production crate root (lib.rs:87-117, 194-196)
- `signals` module (Reactive, Signal, SignalSender) and `UiCommand::SignalWrite`: behind a non-default feature, no catalog consumer; `send_signal_write` is pub(crate) (ADR-0074 §5.8)
- flui-tree's TreeRead/TreeNav/TreeWrite traits are not implemented by ElementTree, so flui-view uses flui-tree only for Arity markers and IndexedSlot
- UNIFIED_ELEMENT.md: describes a superseded element shape (dependents Vec, Phase-1 variants)

## Open questions

- Should `Reactive` (and eventually `Computed`/`Effect`) move to the realm alongside GlobalKeyScope, and how are readers keyed across per-presentation ElementTrees? This decision precedes making signals the default.
- Is the element layer meant to be an extension point (community lazy lists, A2UI hosts, Router transition hosts), or core-only? The answer decides whether sliver_adaptor, layout_builder and the async builders move to flui-widgets behind a public element protocol, or whether flui-view formally owns 'all elements that touch layout'.
- What is the PlatformCapability shape: a generic `cx.capability::<C>()` on LifecycleContext backed by a realm registry, or inherited providers in the tree? Either one supersedes ADR-0078's 'one method per capability' clause.
- Hypothesis to verify: does calling `GlobalKey::with_current_state` from `did_update_view`/`init_state` during a frame deadlock on `WidgetsBinding.inner` (write held in draw_frame_impl, read taken by the registry closure)? No test was run (read-only review).
- Hypothesis to verify with the G4 spike: do Subsecond patches reach code through vtables of `Box<dyn ElementBase>` created before the patch, or must reassemble rebuild elements, and what happens to a `ViewState` whose struct layout changed?
- Magnitude of the per-level deep clone of view configs: run `cargo bench -p flui-view --bench dense_update_reconcile` with an owned-move variant to size the H2 win (not measured here).
- Should WidgetsBinding (frame driver, app lifecycle, observers) stay in the spine, or move to flui-app now that flui-app owns one binding per presentation and all runners?
- For H3 tiers: which subset of the 410 pub fns / 39 traits is the 'View/State/Element' Stable surface, and where do the arena internals go (a `#[doc(hidden)] __runtime` module or a separate internal crate at layer 5)?

