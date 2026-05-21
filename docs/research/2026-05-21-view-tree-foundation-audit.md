---
title: "Mythos Audit — flui-view × flui-tree × flui-foundation"
date: 2026-05-21
status: audit-complete
audit_methodology: claude-mythos (12-phase rust audit, framework-spine pass)
crates_audited:
  - flui-view
  - flui-tree
  - flui-foundation
reference_sources:
  - flutter/packages/flutter/lib/src/widgets/framework.dart
  - flutter/packages/flutter/lib/src/widgets/binding.dart
  - flutter/packages/flutter/lib/src/widgets/notification_listener.dart
  - flutter/packages/flutter/lib/src/foundation/change_notifier.dart
  - flutter/packages/flutter/lib/src/foundation/key.dart
  - flutter/packages/flutter/lib/src/foundation/observer_list.dart
  - flutter/packages/flutter/lib/src/foundation/diagnostics.dart
authors:
  - Mythos (via Claude Opus 4.7)
---

# Mythos Audit: `flui-view` × `flui-tree` × `flui-foundation`

> Single-pass deep audit across the FLUI framework-spine crates, followed by cross-reference against Flutter `widgets/framework.dart` (7,455 LOC) + `foundation/`.
>
> Goal: identify zombie abstractions, parallel type systems, half-implemented hooks, and seams without callers — without breaking active integration with `flui-rendering` or sliding into cosmetic churn.

---

## Post-audit correction (2026-05-21)

> **`flui-tree` is intentional unified-tree infrastructure, not zombie code.**
>
> The auditor classified `flui-tree`'s `Depth`/`AtomicDepth`, `Mountable`/`Unmountable` typestate, `TreeVisitor`/`TreeCursor`/`TreePath`/`ChildDiff`/`Node` traits as deletable (Finding #4, ~10K LOC). That recommendation contradicts the crate's design intent and is **inverted** for execution planning.
>
> Per [`STRATEGY.md`](../../STRATEGY.md) "Behavior loyal, structure Rust-native": Flutter has four parallel tree implementations (Element / RenderObject / Layer / Semantics) each with its own bespoke traversal. `flui-tree` exists as **one unified Rust trait API** (`TreeRead`/`TreeNav`/`TreeWrite` + Arity system + typestate + visitors + cursors) that all four trees should build on top of. The crate was deliberately created by @vanyastaff as Rust-native consolidation of Flutter's multi-tree problem.
>
> **Implication for the priority order below**: Finding #4 stays in the doc as a record of the auditor's observation but the *action* changes from "delete ~10K LOC" to "migrate production consumers (`flui-rendering`, `flui-layer`, `flui-semantics`, `flui-view`) TO the unified `flui-tree` API". Zero-consumer = migration gap, not deletion signal. Concrete abstractions that turn out to be wrong-shaped get redesigned, not removed.
>
> Findings #1, #2, #3, #5, and #6-onwards remain valid as written.

## Table of Contents

- [Part I — Self-Audit Findings](#part-i--self-audit-findings)
  - [Mythos Improvement Verdict](#mythos-improvement-verdict)
  - [Project Map](#project-map)
  - [Findings](#findings)
  - [Dead Code Table](#dead-code-table)
  - [Restructuring Plan](#restructuring-plan)
  - [Optimization Plan](#optimization-plan)
  - [What to Preserve](#what-to-preserve)
  - [Priority Order (initial)](#priority-order-initial)
- [Part II — Flutter Cross-Reference](#part-ii--flutter-cross-reference)
  - [Section 1 — flui-view vs widgets/framework.dart](#section-1--flui-view-vs-flutter-widgetsframeworkdart)
  - [Section 2 — flui-tree vs Flutter tree helpers](#section-2--flui-tree-vs-flutter-tree-helpers)
  - [Section 3 — flui-foundation vs foundation/](#section-3--flui-foundation-vs-flutter-foundation)
- [Part III — Combined Priority Order](#part-iii--combined-priority-order)
- [Appendix A — Investigation Trail](#appendix-a--investigation-trail)

---

# Part I — Self-Audit Findings

## Mythos Improvement Verdict

Архитектура крейтов **structurally promising но недожатая** — DAG чистый (`foundation → tree → view`), Cargo manifests honest, unified Element design (`Element<V, A, B>`) и behavior delegation — production-grade ambitions. Pipeline owner propagation through `set_pipeline_owner_any` уже работает на root mount. `BindingBase + impl_binding_singleton!` mirrors Flutter's `BindingBase.checkInstance` correctly.

**Худший complexity tax**: **дублированные типы между crates**. Два `ViewKey` trait (`flui_foundation::ViewKey` vs `flui_view::view::view::ViewKey`) с разными method signatures — `View::key()` returns the view-local one but `GlobalKey`/`ValueKey`/`UniqueKey`/`ObjectKey` all impl the foundation one. Два `IndexedSlot` (`flui_tree::IndexedSlot<I>` vs `flui_view::IndexedSlot<T>`) с одинаковым именем — prelude glob-import collision risk. Два `TargetPlatform` enum в `flui_foundation::platform` vs `flui_types::platform::target_platform` (different variants — foundation has `Unknown`, types has `Fuchsia`).

**Где hide dead code**: вся `flui-tree`'s sophisticated abstraction layer (`Depth`/`AtomicDepth`/`MAX_TREE_DEPTH` 1143 LOC, `TreePath`/`IndexPath` 1150 LOC, `TreeVisitor`/`TreeVisitorMut` 1264 LOC, `TreeCursor` 1057 LOC, `ChildDiff`/`DiffOp` 1234 LOC, `Mountable`/`Unmountable` typestate 616 LOC) — sum ≈ 6,400 LOC of architecture theater. Production consumers (`flui-rendering`, `flui-layer`, `flui-semantics`) use only `Arity` types + `Identifier` + `Ancestors`/`DescendantsWithDepth`/`AllSiblings` iterators. The `Depth` type isn't even imported — `flui-rendering/src/pipeline/owner.rs:513` uses raw `usize: let child_depth = parent_depth + 1`.

**`BuildContext` half-implemented hot path**: `ElementBuildContext::depend_on_inherited` (line 189), `get_inherited` (line 213), `find_ancestor_view` (line 243), `find_ancestor_state` (line 249), `find_root_ancestor_state` (line 254), `find_render_object` (line 259), `dispatch_notification` (line 302) all return `None` or no-op with `// Placeholder - needs architectural solution` comments. **The single most-used user-facing API of any UI framework is unimplemented**. Tests pass because they exercise `find_ancestor_element` (impl OK) and `mark_needs_build` (impl OK) only.

**`GlobalKey` is decoration**: `GlobalKey<T>::current_element()` (line 78) is `// TODO: Implement via GlobalKeyRegistry`. `current_state()` (line 91) same. The registry exists in `BuildOwner::global_keys: HashMap<u64, ElementId>` (build_owner.rs:65), wired via `register_global_key`/`unregister_global_key` — but nothing calls register, and `GlobalKey::current_element` doesn't read it. This is the **second most-used API** after `BuildContext` and it's a stub.

**Biggest optimization opportunity** — collapse the unused flui-tree machinery to ≤ 30% of current LOC and lock the framework-spine type system on one Key, one ViewKey, one Slot, one IndexedSlot abstraction.

**Не трогать**: `Element<V, A, B>` unified design + `ElementBehavior` trait (генеричность спасает от вылетов), `ElementCore<V, A>::dirty: Arc<AtomicBool>` (animation behavior depends on lock-free dirty mark), `BuildOwner::dirty_elements: BinaryHeap<Reverse<DirtyElement>>` (Flutter parity, correct depth-ordering), `ChangeNotifier::notify_listeners` snapshot-then-fire (correctly handles reentrancy at notifier.rs:158-163), `impl_binding_singleton!` macro, `Key` (NonZeroU64 niche optimization).

---

## Project Map

```text
flui-foundation (6.2K LOC, 13 modules)
  owns: Key/ViewKey/UniqueKey/ValueKey/ObjectKey trait stack, 30+ ID types (ViewId/ElementId/
        RenderId/LayerId/SemanticsId + Animation/Frame/Task/Ticker/...), ChangeNotifier+
        ValueNotifier+MergedListenable+Listenable, ObserverList/HashedObserverList,
        BindingBase+HasInstance+impl_binding_singleton!, DiagnosticsNode/Property/Builder,
        FluiError + FoundationError (two error types), TargetPlatform, Wasm shims,
        DEBUG_MODE/RELEASE_MODE/IS_WEB/IS_MOBILE consts, callbacks aliases
  depends on: bitflags, dashmap, parking_lot, thiserror, tracing
  public surface: ~60 top-level + 40+ prelude exports
  suspected hot paths: ChangeNotifier::notify_listeners (Mutex<HashMap<ListenerId, Arc<dyn Fn()>>>),
                       ValueNotifier::set_value, ObserverList add/remove (HashMap<ObserverId, usize>
                       index + VecDeque slots), HashedObserverList (DashMap)
  risk: TWO error types (FluiError + FoundationError, both zero workspace consumers);
        TargetPlatform duplicates flui-types::TargetPlatform with different variants;
        ViewKey trait duplicates flui-view's view-local ViewKey;
        HashedObserverList + MergedListenable + ValueNotifier — zero workspace consumers

flui-tree (18.0K LOC, 8 modules; 30+ files)
  owns: Arity types (Leaf/Single/Optional/Variable) + ArityStorage + ChildrenStorage,
        Depth/AtomicDepth/MAX_TREE_DEPTH (1143 LOC), TreeRead/TreeNav/TreeWrite/TreeWriteNav
        trait stack, Mountable/Unmountable/Mounted/Unmounted typestate, NodeState marker trait,
        Node/NodeExt/NodeVisitor/NodePredicate, TreeCursor (1057 LOC), TreePath/IndexPath
        (1150 LOC), ChildDiff/TreeDiff/DiffOp (1234 LOC), TreeVisitor/TreeVisitorMut (1264 LOC),
        Ancestors/Descendants/Siblings/BreadthFirst/DepthFirst iterators (~2300 LOC),
        IndexedSlot/Slot/SlotBuilder/SlotIter (slot.rs 911 LOC)
  depends on: flui-foundation, thiserror, tracing, smallvec, ambassador
  public surface: ~80 top-level + 50 prelude exports
  suspected hot paths: TreeNav::ancestors, DescendantsWithDepth iteration, ChildrenStorage access
  risk: **the most over-engineered crate in the workspace**. Outside flui-tree's own examples
        + the Arity types + a handful of iterators (Ancestors, DescendantsWithDepth, AllSiblings),
        NONE of: Depth/AtomicDepth, TreePath/IndexPath, TreeCursor, ChildDiff/DiffOp,
        TreeVisitor, Mountable/Unmountable typestate, NodeState — is consumed by any production
        crate. Estimated 60-70% of LOC is zombie abstraction with zero callers.

flui-view (11.5K LOC, 8 modules)
  owns: View+StatelessView+StatefulView+InheritedView+RenderView+ProxyView+ParentDataView+
        AnimatedView trait stack, ElementBase trait (object-safe), Element<V, A, B> unified +
        ElementCore<V, A> + ElementBehavior<V, A> behavior trait (Stateless/Stateful/Inherited/
        Render/Proxy/Animation behaviors), BuildOwner (dirty BinaryHeap + global_keys +
        inherited_elements HashMap), BuildContext trait + ElementBuildContext concrete,
        WidgetsBinding (singleton, RwLock<WidgetsBindingInner>, 1265 LOC), ElementTree (Slab),
        reconcile_children (O(N) linear), GlobalKey<T>, ObjectKey, ValueKey re-export, Notification
        bubbling, Lifecycle (Initial/Active/Inactive/Defunct), ElementSlot = IndexedSlot<Option<ElementId>>,
        ChildStorage variants (No/Single/Optional/Variable)
  depends on: flui-foundation, flui-tree (only Arity types), flui-types, flui-rendering,
              flui-interaction, flui-log, downcast-rs, dyn-clone, parking_lot, slab
  public surface: ~50 top-level + 35 prelude exports
  suspected hot paths: BuildOwner::build_scope (BinaryHeap pop), ElementTree::get (slab index),
                       WidgetsBinding::draw_frame (RwLock<inner>::write)
  risk: ElementBuildContext returns None for depend_on_inherited / get_inherited /
        find_ancestor_view / find_ancestor_state / find_root_ancestor_state /
        find_render_object / dispatch_notification — the entire dependency-injection
        + ancestor-lookup + render-object-finder API surface is stubs.
        GlobalKey::current_element + current_state are TODO stubs.
        Two unimplemented!() calls in view/root.rs:487, :494.
        Local ViewKey trait shadows foundation's ViewKey.
        Local IndexedSlot shadows flui-tree's IndexedSlot.
```

**Cross-crate dependency DAG** (clean):

```
foundation → (nothing)
tree       → foundation
view       → foundation, tree (Arity only), types, rendering, interaction, log
```

No backwards or circular deps. View depends on rendering, but the crate is in the framework layer per `CLAUDE.md`, so this is expected.

---

## Findings

### 💀 [DUPLICATION | CRITICAL]: Two `ViewKey` traits in workspace — type system collision

**Evidence:**
- [`crates/flui-foundation/src/key.rs:309`](../../crates/flui-foundation/src/key.rs) — `pub trait ViewKey: Send + Sync + 'static { fn as_any(&self) -> &dyn Any; fn key_eq(&self, other: &dyn ViewKey) -> bool; fn key_hash(&self) -> u64; fn clone_key(&self) -> Box<dyn ViewKey>; fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result; }`. Implementations: `ValueKey<T>` (line 405), `UniqueKey` (line 486), `ObjectKey` (in flui-view), `GlobalKey<T>` (in flui-view at `key/global_key.rs:125`).
- [`crates/flui-view/src/view/view.rs:103`](../../crates/flui-view/src/view/view.rs) — `pub trait ViewKey: Send + Sync + std::fmt::Debug { fn key_type_id(&self) -> TypeId; fn key_eq(&self, other: &dyn ViewKey) -> bool; fn key_hash(&self) -> u64; }`. **Zero implementations in the workspace** — the only impl is the empty default in `View::key() -> Option<&dyn ViewKey> { None }` at line 92.
- [`crates/flui-view/src/lib.rs:142`](../../crates/flui-view/src/lib.rs) re-exports `ViewKey` from `view::view::ViewKey` (the local empty trait).
- [`crates/flui-foundation/src/lib.rs:229`](../../crates/flui-foundation/src/lib.rs) re-exports `ViewKey` from `key::ViewKey` (the populated trait).
- `use flui_view::prelude::*; use flui_foundation::prelude::*;` → `ViewKey` becomes ambiguous; the user picks one trait, every concrete key implements the other.

**Why it exists:**
The Foundation-side ViewKey is the working one (4 production impls). The View-side `ViewKey` was scaffolded for `View::key()` to return a borrow, but no concrete key type ever migrated to it. The local trait has `key_type_id() -> TypeId` instead of `as_any() -> &dyn Any`, suggesting a separately-evolved API surface that drifted from foundation.

**Cost today:**
- `GlobalKey<T>` impls `flui_foundation::ViewKey` but `View::key()` returns `Option<&dyn flui_view::view::view::ViewKey>` → these are different types. **A `GlobalKey<T>` instance is statically not assignable to the slot `View::key()` advertises.**
- Reading `into_view.rs:171` confirms: `fn key(&self) -> Option<&dyn super::view::ViewKey>` (the local empty trait). Any user who reads documentation and tries `view.with_key(GlobalKey::new())` will get a trait-bound mismatch.
- API surface theater — two parallel namespaces, both `pub`, neither functionally compatible.
- Documentation lies — `key/object_key.rs:9,21` reference `flui_foundation::ViewKey` while the lib.rs prelude advertises the view-local one.

**Risk of changing:**
Low. **The local `ViewKey` trait has zero workspace impls** beyond the default `None` return — deleting it and rewiring `View::key()` to return `Option<&dyn flui_foundation::ViewKey>` is a 2-line code change + a trait re-export removal. The hidden risk is downstream consumers of `flui_view::ViewKey` (none in the workspace).

**Recommendation:** **delete** `flui_view::view::view::ViewKey`. Change `View::key()` to `fn key(&self) -> Option<&dyn flui_foundation::ViewKey>`. Remove `pub use view::ViewKey` from `flui-view/src/lib.rs:142` and prelude. All concrete keys already impl the foundation trait — no migration needed.

**Patch sketch:**
```rust
// crates/flui-view/src/view/view.rs — replace:
//   pub trait ViewKey { ... }  // delete entire trait
// with:
//   use flui_foundation::ViewKey;
// and the View::key signature becomes:
//   fn key(&self) -> Option<&dyn ViewKey> { None }
//
// crates/flui-view/src/lib.rs — remove from re-exports + prelude:
//   ViewKey  (the local re-export)
```

---

### 💀 [DUPLICATION | CRITICAL]: Two `IndexedSlot` types — prelude glob-import shadowing

**Evidence:**
- [`crates/flui-tree/src/iter/slot.rs:422`](../../crates/flui-tree/src/iter/slot.rs) — `pub struct IndexedSlot<I: Identifier> { index: usize, previous_sibling: Option<I> }`. Re-exported via `flui_tree::IndexedSlot` (lib.rs:174) AND in prelude (lib.rs:234).
- [`crates/flui-view/src/element/slot.rs:53`](../../crates/flui-view/src/element/slot.rs) — `pub struct IndexedSlot<T = Option<ElementId>> { index: usize, value: T }`. Re-exported via `flui_view::IndexedSlot` (lib.rs:121) AND in prelude (lib.rs:169).
- Different type-parameter shape: `IndexedSlot<I: Identifier>` (concrete `Option<I>` inside) vs `IndexedSlot<T = Option<ElementId>>` (generic value).
- Different field name: `previous_sibling: Option<I>` vs `value: T`.
- `flui-view::ElementSlot = IndexedSlot<Option<ElementId>>` is a type alias for the view-local one.

**Why it exists:**
`flui-tree` introduced `IndexedSlot<I>` as a generic helper alongside `SlotBuilder` and `SlotIter` (911 LOC in `iter/slot.rs`). Later `flui-view` re-introduced `IndexedSlot` with subtly different semantics for `ElementSlot`. Neither consumes the other; both exist as parallel solutions to the same Flutter pattern (`IndexedSlot<T>` in widgets/framework.dart).

**Cost today:**
- `use flui_view::prelude::*; use flui_tree::prelude::*;` resolves to the LAST one imported (compiler-dependent / ambiguous in some glob orderings).
- Two near-namesake types with similar fields is a maintenance trap — bug fixes to one don't propagate.
- `flui-tree::IndexedSlot` has 0 production consumers outside flui-tree (grep confirmed).

**Risk of changing:**
Low. flui-tree's `IndexedSlot<I>` has zero consumers. Either: (a) delete `flui-tree::IndexedSlot` + `Slot` + `SlotBuilder` + `SlotIter` (911 LOC) and keep flui-view's as canonical; or (b) make flui-view's `ElementSlot` a thin alias over the tree-level type.

**Recommendation:** **delete** `flui-tree::iter::slot` (entire submodule, 911 LOC) including `IndexedSlot<I>`, `Slot`, `SlotBuilder`, `SlotIter`. Keep `flui-view::IndexedSlot` + `ElementSlot` as the single canonical types. Remove the flui-tree re-exports from `lib.rs:174` + prelude.

---

### 💀 [DUPLICATION | HIGH]: Two `TargetPlatform` enums in flui-foundation + flui-types

**Evidence:**
- [`crates/flui-foundation/src/platform.rs:25`](../../crates/flui-foundation/src/platform.rs) — `pub enum TargetPlatform { Android, IOS, Windows, MacOS, Linux, Web, Unknown }`. Method `current()` uses `cfg!(target_os)` to detect at runtime.
- [`crates/flui-types/src/platform/target_platform.rs:4`](../../crates/flui-types/src/platform/target_platform.rs) — `pub enum TargetPlatform { Android, iOS, MacOS, Linux, Windows, Fuchsia, Web }`. No `Unknown`, has `Fuchsia`.
- Variants disagree: foundation has `IOS` (UPPER), types has `iOS` (camelCase); foundation has `Unknown`, types has `Fuchsia`.
- Grep `use flui_foundation::TargetPlatform`: 0 workspace consumers; only README + the platform.rs doc-block reference it.
- Grep `use flui_types::TargetPlatform`: 0 workspace consumers in the production crates I sampled.

**Why it exists:**
Pre-workspace-split artifact. When `flui-types` and `flui-foundation` were separated, `TargetPlatform` ended up in both because both needed it for examples or doc-tests. Neither got removed. The foundation one was later updated with `Unknown` for completeness; the types one acquired `Fuchsia`.

**Cost today:**
- Public API ambiguity — which to import?
- Zero production consumers in either crate.
- The shape divergence (`IOS` vs `iOS`, `Unknown` vs `Fuchsia`) makes them not interconvertible without a `match`.

**Risk of changing:**
Low. Pick one canonical location (likely flui-types per "primitive types" mandate). Delete the other. Both have zero workspace consumers.

**Recommendation:** **delete** `flui-foundation::platform::TargetPlatform`. Keep `flui-types::TargetPlatform` as canonical. Update `flui-foundation::IS_DESKTOP/IS_MOBILE/IS_WEB` const fns in `consts.rs` to continue using `cfg!` — they don't need the enum. Remove the re-export from `flui-foundation/src/lib.rs:237`.

---

### 💀 [ZOMBIE | CRITICAL]: `flui-tree` typestate machinery (`Mountable`/`Unmountable`/`NodeState`) — zero consumers outside lib

**Evidence:**
- [`crates/flui-tree/src/state.rs`](../../crates/flui-tree/src/state.rs) — 616 LOC. `NodeState` marker trait, `Mounted`/`Unmounted` PhantomData markers, `Mountable` trait (`type Mounted; fn mount(self, parent: Option<Self::Id>, parent_depth: Depth) -> Self::Mounted`), `Unmountable` trait, `MountableExt`. Documented as "Typestate Pattern" with extensive doc examples.
- Grep `impl Mountable for|impl Unmountable for` across the workspace: 0 hits in production crates. Tree's own doc-tests and `lib.rs:36-97` doc-test are the only usages.
- `flui-rendering::storage::RenderTree`, `flui-view::tree::ElementTree`, `flui-layer::LayerTree`, `flui-semantics::SemanticsTree`: NONE use `Mountable`/`Unmountable`. Each crate implements its own mount/unmount lifecycle directly.
- `flui-view::Element<V, A, B>::mount(&mut self, parent: Option<ElementId>, slot: usize)` takes `&mut self`, not `self by-value` — irreconcilable with `Mountable::mount(self, ...) -> Self::Mounted`.

**Why it exists:**
Architectural ambition — encode mount state at the type level so misuse is impossible. Sound idea in isolation, but it conflicts with the **existing mutable-by-default lifecycle** every production tree already uses. The typestate forces by-value mounting, which is incompatible with Slab storage (you can't move a node out of slab for mounting without invalidating its ID).

**Cost today:**
- 616 LOC of trait machinery + 280 LOC `arity/runtime.rs` + 180 LOC `arity/traits.rs` documenting a pattern nobody uses.
- Cognitive load on contributors — the docs front-load typestate, but reading any actual production tree shows mutable `mount(&mut self, ...)`.
- Public API freeze — `Mountable`, `Unmountable`, `Mounted`, `Unmounted`, `NodeState`, `MountableExt` are all re-exported from `lib.rs:178` + prelude. Any change is breaking.

**Risk of changing:**
Medium. The typestate is documented as the headline pattern of the crate (`lib.rs:8-21` ASCII diagram). Deleting it requires explaining the new architecture in `lib.rs` docs.

**Recommendation:** **demote to `pub(crate)` + add `// REVISIT_AFTER: real consumer materializes`** rather than outright delete. If/when an actual mountable typestate user appears (Slab + Mountable would need a major rework), the trait exists. Drop the headline doc emphasis — replace lib.rs:8-21 description with the actual `TreeRead/TreeNav/TreeWrite` story.

---

### 💀 [ZOMBIE | CRITICAL]: `Depth` + `AtomicDepth` + `MAX_TREE_DEPTH` (1143 LOC) — zero production consumers

**Evidence:**
- [`crates/flui-tree/src/depth.rs`](../../crates/flui-tree/src/depth.rs) — 1143 LOC. `Depth(usize)` newtype with 18 const methods (root, child_depth, parent_depth, distance_to, is_deeper_than, etc.). `AtomicDepth(AtomicUsize)` with `set`/`get`/`from_depth`/`is_root` etc. `MAX_TREE_DEPTH: usize = 256`. `DepthAware` trait.
- Grep `use flui_tree::Depth|use flui_tree::AtomicDepth|use flui_tree::MAX_TREE_DEPTH|DepthAware` across workspace (excluding flui-tree/src + flui-tree/examples): **0 hits in production**.
- [`crates/flui-rendering/src/pipeline/owner.rs:513`](../../crates/flui-rendering/src/pipeline/owner.rs) (production): `let child_depth = parent_depth + 1; self.add_node_needing_layout(child_id, child_depth as usize);` — uses raw `usize`.
- [`crates/flui-rendering/src/storage/tree.rs:10`](../../crates/flui-rendering/src/storage/tree.rs) imports `Ancestors, AllSiblings, DescendantsWithDepth` from flui-tree but NOT `Depth`.
- Every production tree stores `depth: usize` directly (`ElementNode.depth: usize` at `flui-view/src/tree/element_tree.rs:24`).

**Why it exists:**
"Type-safe depth" intent — semantic clarity, validation, max-depth enforcement. The doc-block at depth.rs:18-23 lists virtues that production code does not exercise.

**Cost today:**
- 1143 LOC of code + tests for unused machinery.
- Public API of the crate is bloated — `lib.rs:143` re-exports 6 items from depth module, all unused outside flui-tree.
- `MAX_TREE_DEPTH = 256` is hardcoded; flui-rendering's `RenderError::LayoutDepthExceeded` uses its own `usize` limit at `flui-rendering/src/error.rs:153`. Two depth-limit systems.

**Risk of changing:**
Low. Zero production callers — `Depth` could be moved to `pub(crate)` or deleted entirely. The compile-time docs would need a rewrite (currently use Depth in examples), but the rest of flui-tree just needs to swap `Depth` → `usize` in its own internals.

**Recommendation:** **demote to `pub(crate)` + add cadence rule**. Mark `// REMOVE_BY: 2026-12-31 unless a production tree adopts Depth`. If no migration begins by Q4 2026, delete the module. Alternative: shrink `depth.rs` to ≤ 200 LOC (just `Depth` + `child_depth`/`parent_depth`/`get`) and remove `AtomicDepth`, `DepthAware`, `try_*` variants, all the comparison helpers. None of those are used.

---

### 💀 [ZOMBIE | HIGH]: `TreeVisitor`/`TreeVisitorMut`/`CollectVisitor`/`CountVisitor`/`FindVisitor`/`ForEachVisitor`/`MaxDepthVisitor` (1264 LOC) — only tree's own examples use them

**Evidence:**
- [`crates/flui-tree/src/visitor/mod.rs`](../../crates/flui-tree/src/visitor/mod.rs) — 1264 LOC. `visit_depth_first`, `visit_breadth_first`, `collect_all`, `count_all`, `find_first`, `for_each`, `max_depth` free functions + 5 concrete visitor structs.
- [`crates/flui-tree/src/visitor/composition.rs`](../../crates/flui-tree/src/visitor/composition.rs) — 648 LOC of visitor combinators.
- [`crates/flui-tree/src/visitor/fallible.rs`](../../crates/flui-tree/src/visitor/fallible.rs) — 638 LOC of fallible visitors.
- Grep for `TreeVisitor|TreeVisitorMut|visit_depth_first|visit_breadth_first` outside `flui-tree/src`: 1 match in `flui-tree/examples/basic_traversal.rs`; 0 in any production crate.
- [`crates/flui-rendering/src/storage/tree.rs:553`](../../crates/flui-rendering/src/storage/tree.rs) implements its own `pub fn visit_depth_first<F>(&self, mut f: F)` (line 553) using direct slab+recursion — NOT TreeVisitor.

**Why it exists:**
Generic visitor combinators looked useful pre-construction. Production crates wrote their own bespoke traversals (5-10 LOC each) because:
1. flui-tree's visitor takes a `&dyn TreeRead<I>` trait object — production trees prefer concrete `&RenderTree`.
2. Composition combinators add allocation overhead.
3. Fallible visitors propagate errors but the trees don't have error-propagating traversals.

**Cost today:**
- 2550 LOC across 3 visitor files, all forwarded through `pub use visitor::{...}` in lib.rs:191.
- 7 traits/structs in public API surface (TreeVisitor, TreeVisitorMut, CollectVisitor, CountVisitor, FindVisitor, ForEachVisitor, MaxDepthVisitor) — all `pub`, all unused.

**Risk of changing:**
Low. Zero production consumers. Examples can be rewritten to use direct iterators.

**Recommendation:** **delete** the entire `visitor/` submodule (2550 LOC) and the re-exports. If a generic visitor becomes necessary, reintroduce a single `TreeVisitor` trait with one method and a concrete `visit_dfs<T: TreeRead<I>>(tree: &T, root: I, &mut impl FnMut(I))` free function. The combinator suite (composition + fallible) is YAGNI for the actual call sites.

---

### 💀 [ZOMBIE | HIGH]: `TreeCursor` (1057 LOC) — no production consumer

**Evidence:**
- [`crates/flui-tree/src/iter/cursor.rs`](../../crates/flui-tree/src/iter/cursor.rs) — 1057 LOC. `TreeCursor<'a, I, T>` with `goto_parent`, `goto_first_child`, `goto_next_sibling`, save/restore stack, path tracking.
- Re-exported via `lib.rs:159` (top-level) + prelude (`lib.rs:253`).
- Grep `TreeCursor` outside `flui-tree/src`: 0 hits.

**Why it exists:**
Cursor-based traversal pattern (rust-tree-sitter, ego-tree) inspires this. Production trees handle traversal via iterators (DepthFirstIter, etc.) and direct lookups.

**Cost today:**
- 1057 LOC of unused state-machine code.
- Public API liability — `TreeCursor` is in the prelude, will appear in IDE autocomplete.

**Recommendation:** **delete** `iter/cursor.rs` entirely. Remove from re-exports.

---

### 💀 [ZOMBIE | HIGH]: `TreePath`/`IndexPath`/`TreeNavPathExt` (1150 LOC) — no production consumer

**Evidence:**
- [`crates/flui-tree/src/iter/path.rs`](../../crates/flui-tree/src/iter/path.rs) — 1150 LOC. `TreePath<I>`, `IndexPath` (slot-based path), `TreeNavPathExt` trait extension.
- Re-exported via `lib.rs:170`.
- Grep `TreePath|IndexPath|TreeNavPathExt` outside `flui-tree/src`: 0 hits in production crates.

**Why it exists:**
Address nodes by path (sequence of indices from root) — useful for serialization, hot-reload, dev-tools. None of those exist in production yet.

**Cost today:**
- 1150 LOC of unused code.
- Public API liability.

**Recommendation:** **delete** or **`pub(crate)`** — keep at least the `IndexPath` type as the framework will likely want it for hot-reload, but `TreePath` (the long-cumulative path type) and `TreeNavPathExt` (extension methods on TreeNav) can go. **Re-evaluate after flui-hot-reload integration lands.**

---

### 💀 [ZOMBIE | HIGH]: `ChildDiff`/`TreeDiff`/`DiffOp` (1234 LOC) — production uses bespoke O(N) reconciliation

**Evidence:**
- [`crates/flui-tree/src/diff.rs`](../../crates/flui-tree/src/diff.rs) — 1234 LOC. `DiffOp::{Insert, Remove, Move, Update}`, `ChildDiff<I>`, `ChildOp::{Keep, Insert, Remove, Reorder}`, `TreeDiff<I>`, `DiffStats`.
- Re-exported via `lib.rs:147` + prelude (`lib.rs:218`).
- [`crates/flui-view/src/tree/reconciliation.rs:51`](../../crates/flui-view/src/tree/reconciliation.rs) implements its own `pub fn reconcile_children(tree: &mut ElementTree, parent: ElementId, old_children: &[ElementId], new_views: &[&dyn View]) -> Vec<ElementId>` using 5-phase algorithm (match start, match end, build keyed map, process middle, remove unused). 200+ LOC of bespoke reconciliation. Doc comment line 1-5: "Flutter insight: 'Contrary to popular belief, Flutter does not employ a tree-diffing algorithm'".
- Grep `ChildDiff|TreeDiff|DiffOp|ChildOp` outside `flui-tree/src`: 0 hits in production.
- The reconciliation algorithm explicitly cites Flutter's choice to NOT diff trees, but flui-tree still ships a diff API.

**Why it exists:**
Tree diffing is a generic CS textbook problem. The view layer immediately chose to NOT use it because Flutter doesn't. Diff machinery never migrated.

**Cost today:**
- 1234 LOC unused.
- Confusing — the lib.rs docs advertise diffing as a core feature; the actual production reconciliation algorithm rejects diffing entirely.

**Recommendation:** **delete** `diff.rs` + the re-exports. The bespoke `reconcile_children` lives in flui-view; if other crates need reconciliation later, they should follow the same O(N) pattern (Flutter's approach), not a tree-diff.

---

### 💀 [ZOMBIE | HIGH]: `Node`/`NodeExt`/`NodePredicate`/`NodeTypeInfo`/`NodeVisitor` (305 LOC) — no production consumers

**Evidence:**
- [`crates/flui-tree/src/traits/node.rs`](../../crates/flui-tree/src/traits/node.rs) — 305 LOC. `pub trait Node`, `NodeExt`, `NodeTypeInfo`, `NodeVisitor`, plus `collect_matching_nodes`/`count_matching_nodes` free fns.
- Grep `flui_tree::Node\b|NodeExt|NodeTypeInfo|NodeVisitor` outside flui-tree: 0 hits.
- Production trees use bespoke node types (`ElementNode`, `RenderNode`, `LayerNode`, `SemanticsNode`) — none implement `flui_tree::Node`.

**Recommendation:** **delete** the entire `traits/node.rs` if no production consumer materializes in 6 months. Add `// REMOVE_BY: 2026-11-21` marker. Keep `TreeRead`/`TreeNav`/`TreeWrite`/`TreeWriteNav` — those ARE used.

---

### 💀 [HALF-IMPLEMENTED | CRITICAL]: `ElementBuildContext` returns `None`/no-op for 7 of 10 BuildContext methods

**Evidence:** [`crates/flui-view/src/context/element_build_context.rs`](../../crates/flui-view/src/context/element_build_context.rs)
- `depend_on_inherited` (line 189) — `let _ = node; None // Placeholder - needs architectural solution`
- `get_inherited` (line 213) — `None // Placeholder - needs architectural solution`
- `find_ancestor_view` (line 243) — `let _ = type_id; None`
- `find_ancestor_state` (line 249) — `let _ = type_id; None`
- `find_root_ancestor_state` (line 254) — `let _ = type_id; None`
- `find_render_object` (line 259) — `None // requires RenderElement integration`
- `dispatch_notification` (line 302) — bubbles up but doesn't call any handler: `// Check if this element handles the notification\n// This requires NotifiableElement trait check\n// For now, just walk up\nlet _ = notification;`
- `owner` (line 182) — `None // callers should use build_owner() method instead`

**Working:** `element_id`, `depth`, `mounted`, `is_building`, `find_ancestor_element` (line 225), `visit_ancestor_elements` (line 265), `visit_child_elements` (line 282), `mark_needs_build` (line 297).

**Why it exists:**
Lifetime issue: `RwLock<ElementTree>` guard can't outlive the `&dyn Any` returned reference. The architectural solution is either (a) make `InheritedElement` cache data in a longer-lived store (e.g. `Arc<dyn Any>` in BuildOwner per InheritedElement), or (b) return `Result<Box<dyn Any>>` and clone, or (c) callback API `with_inherited<T>(&self, |&T|)`. The author identified the issue but didn't pick a solution.

**Cost today:**
- **The single most-used API surface of any UI framework — `BuildContext::depend_on::<T>()`, `BuildContext::dispatch_notification`, `BuildContext::find_render_object` — is a stub**. This is not "future work"; this is "the framework does not work for its declared use case".
- `BuildContextExt::depend_on<T>` (line 223) downcasts `None` → still `None`. The typed extension method is also broken.
- Tests pass because they don't exercise these methods.

**Risk of changing:**
High to fix correctly — needs an architectural decision (callback API vs `Arc<dyn Any>` per InheritedElement vs `Result<Cow<dyn Any>>`). Low to mark explicit — replace `None` with `unimplemented!("ElementBuildContext::depend_on_inherited - tracked at #XXX")` so callers fail fast.

**Recommendation:** **stop pretending the API works.** Pick ONE of:
1. **Callback API** (cleanest): `fn with_inherited<T: 'static, R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R`. The callback holds the read lock for its scope; no lifetime escape.
2. **Owned-return**: `fn depend_on_owned<T: Clone + 'static>(&self) -> Option<T>` — clone the data out.
3. **Per-InheritedElement Arc<dyn Any>**: `InheritedBehavior::data: Arc<V::Data>`, ElementBuildContext clones the Arc.

Pick #1 (matches Flutter's pattern most closely). Mark the current `depend_on_inherited` etc. as `#[doc(hidden)] #[deprecated(note = "Use with_inherited instead, see #XXX")]` until removed.

---

### 💀 [HALF-IMPLEMENTED | CRITICAL]: `GlobalKey<T>::current_element` + `current_state` are TODO stubs

**Evidence:**
- [`crates/flui-view/src/key/global_key.rs:78`](../../crates/flui-view/src/key/global_key.rs) — `pub const fn current_element(&self) -> Option<ElementId> { // TODO: Implement via GlobalKeyRegistry\n        None\n    }`
- Line 91 — `pub const fn current_state(&self) -> Option<Arc<T>> where T: Send + Sync { // TODO: Implement via GlobalKeyRegistry\n        None\n    }`
- [`crates/flui-view/src/owner/build_owner.rs:65`](../../crates/flui-view/src/owner/build_owner.rs) — `global_keys: HashMap<u64, ElementId>` exists.
- Same file lines 296-308 — `register_global_key(&mut self, key_hash: u64, element: ElementId)`, `unregister_global_key`, `element_for_global_key(&self, key_hash: u64) -> Option<ElementId>` — registry API exists.
- Grep `register_global_key|unregister_global_key|element_for_global_key` across workspace: 0 callers outside the BuildOwner tests at lines 502-513.
- **Nothing in the framework actually registers a GlobalKey when an element is mounted.** The element lifecycle paths (`Element::mount`, `behavior::on_mount`) do NOT register the key.

**Why it exists:**
The Flutter pattern requires the Element's `mount()` and `unmount()` to call `BuildOwner::_registerGlobalKey` and `_unregisterGlobalKey` respectively (see `framework.dart:3178-3201`). FLUI's `Element::mount` doesn't have access to the BuildOwner — the BuildOwner is owned by WidgetsBinding. The plumbing was never finished.

**Cost today:**
- GlobalKey is **public API that does nothing** — `GlobalKey::new()` works, registration is silent, lookup returns None.
- `currentState/currentContext` (Flutter's most-used GlobalKey methods) are stubs.
- Test `test_global_key_registry` (build_owner.rs:502) confirms the registry works — but no production code wires it up.

**Risk of changing:**
Medium. Wiring Element::mount → BuildOwner.register_global_key requires either:
(a) Passing BuildOwner reference through Element mounting (large surface area change), OR
(b) Storing pending GlobalKey registrations on the Element + flushing them in `WidgetsBinding::attach_root_widget`/`finalize_tree`, OR
(c) Static GlobalKey registry (`OnceLock<Mutex<HashMap<u64, ElementId>>>`), Flutter-incompatible but Rust-idiomatic.

**Recommendation:** Pick **(a)**. Plumb the BuildOwner into `ElementBase::mount(&mut self, parent: Option<ElementId>, slot: usize, owner: &mut BuildOwner)`. Mark `GlobalKey::current_element` / `current_state` as `unimplemented!("requires Element::mount BuildOwner plumbing — tracked at #XXX")` until the plumbing lands.

---

### 💀 [HALF-IMPLEMENTED | HIGH]: `view/root.rs:487,494` `unimplemented!()` in production code path

**Evidence:**
- [`crates/flui-view/src/view/root.rs:487`](../../crates/flui-view/src/view/root.rs) — `unimplemented!("attach_to_pipeline_owner needs migration to RenderTree/RenderId")`
- Same file line 494 — `unimplemented!("detach_from_pipeline_owner needs migration to RenderTree/RenderId")`
- Both methods are reached during the application setup phase (root mounting attaches to pipeline owner).

**Cost today:**
- Any production code that exercises the full root → pipeline owner → render tree integration will panic.
- The constitution forbids `unwrap()`/`unimplemented!()` panics in production paths (CLAUDE.md "No unwrap()/println!/dbg!"). This violates that rule.

**Recommendation:** Implement the migration to RenderTree/RenderId per the inline TODO, OR mark these as `pub(crate)` and route the actual root-mount through `WidgetsBinding::attach_root_widget` (which has working PipelineOwner plumbing at `binding.rs:563-571`). Most likely the methods are stale and the binding path is the real one — confirm and delete the stale `unimplemented!()` versions.

---

### 💀 [DUPLICATION | MEDIUM]: Two binding-related modules with overlap (`flui-foundation::binding` + `flui-view::binding`)

**Evidence:**
- [`crates/flui-foundation/src/binding.rs`](../../crates/flui-foundation/src/binding.rs) — 272 LOC. `BindingBase` trait, `HasInstance` marker, `impl_binding_singleton!` macro, `check_instance<B>()` helper.
- [`crates/flui-view/src/binding.rs`](../../crates/flui-view/src/binding.rs) — 1265 LOC. `WidgetsBinding` singleton (uses `impl_binding_singleton!`), `WidgetsBindingObserver` trait, lifecycle/navigation/predictive-back/exit handling.
- These are correctly layered: foundation provides the singleton machinery, view consumes it. **Not a duplication** — the file-name reuse misled me on first scan. **Verified non-issue.**

**Status:** No action. Mark in audit as "checked, not a problem".

---

### 💀 [ZOMBIE | MEDIUM]: `MergedListenable` + `HashedObserverList` + `SyncObserverList` — no production consumers

**Evidence:**
- [`crates/flui-foundation/src/notifier.rs:404`](../../crates/flui-foundation/src/notifier.rs) — `pub struct MergedListenable { listenables: Vec<Box<dyn Listenable + Send>>, notifier: ChangeNotifier, source_listener_ids: Vec<ListenerId> }`. The `source_listener_ids` field is `#[allow(dead_code)]` at line 408.
- [`crates/flui-foundation/src/observer.rs:303`](../../crates/flui-foundation/src/observer.rs) — `pub struct HashedObserverList<T> { observers: dashmap::DashMap<ObserverId, T>, ... }`.
- Grep `MergedListenable` outside `flui-foundation`: 0 hits.
- Grep `HashedObserverList|SyncObserverList` outside `flui-foundation`: 0 hits.
- The `dashmap` workspace dep at `flui-foundation/Cargo.toml:20` exists **solely** for HashedObserverList — a single struct with no users.

**Why it exists:**
Flutter's `Listenable.merge()` is a foundation primitive (`change_notifier.dart:495` `_MergingListenable`). The hash-based observer list is a perf optimization for "many unique observers" patterns. Neither pattern has emerged in the workspace.

**Cost today:**
- dashmap dependency in foundation just for HashedObserverList.
- `#[allow(dead_code)]` on MergedListenable.source_listener_ids smells.
- Two observer-list types where one would suffice.

**Recommendation:** **`pub(crate)`** all three (`MergedListenable`, `HashedObserverList`, `SyncObserverList`) until first consumer. Drop the `dashmap` dependency from `Cargo.toml` (saves a transitive dep). Re-promote to `pub` when needed.

---

### 💀 [DUPLICATION | LOW]: Two error types — `FluiError` (assert.rs) + `FoundationError` (error.rs)

**Evidence:**
- [`crates/flui-foundation/src/assert.rs:41`](../../crates/flui-foundation/src/assert.rs) — `pub struct FluiError { summary: String, message: String, context: Vec<String>, library: Option<String> }`. Rich diagnostic error with builder pattern.
- [`crates/flui-foundation/src/error.rs:30`](../../crates/flui-foundation/src/error.rs) — `pub enum FoundationError { InvalidId, InvalidKey, ListenerError, DiagnosticsError, NotificationError, AtomicError, SerializationError, Generic }`. thiserror-based domain error.
- Grep `use flui_foundation::FluiError|use flui_foundation::FoundationError` workspace: 0 production hits.
- Both are `pub` and re-exported (lib.rs:162, lib.rs:181).

**Why it exists:**
Two evolved error patterns. `FluiError` mirrors Flutter's `FlutterError` (rich, builder-style). `FoundationError` mirrors thiserror/Rust idioms (variants per failure mode). Neither got picked as the canonical workspace error.

**Cost today:**
- API surface duplication.
- Doc confusion — `crates/flui-foundation/README.md:?` lists both.
- Zero workspace consumers — either is removable.

**Recommendation:** **delete `FluiError`** (the more Flutter-pattern one). Keep `FoundationError` — thiserror enum is Rust-idiomatic and Constitution Principle 6 prefers thiserror. Re-evaluate when downstream consumers materialize.

---

### 💀 [TRAIT | MEDIUM]: `RenderObjectElement` + `RenderSlot` + `RenderTreeRootElement` — only RenderElement uses them

**Evidence:**
- [`crates/flui-view/src/element/render_object_element.rs`](../../crates/flui-view/src/element/render_object_element.rs) — 183 LOC. `pub trait RenderObjectElement`, `RenderSlot`, `RenderTreeRootElement`.
- Grep `impl RenderObjectElement for` workspace: 1 hit at `crates/flui-view/src/element/unified.rs:208` (`impl<V> RenderObjectElement for Element<V, Variable, RenderBehavior<V>>`). This is parameterized on `V: RenderView`, so it covers the single Element variant used.
- `RenderTreeRootElement` — 0 impls.
- `RenderSlot` — used in the one RenderObjectElement impl.

**Why it exists:**
Flutter parity — `RenderObjectElement` is an abstract base class. Rust uses a trait. Since the unified Element design covers it with the `Variable + RenderBehavior` specialization, the trait has only one impl by design.

**Risk of changing:**
The trait is genuinely used through dynamic dispatch in `flui-rendering`'s element-render integration. Keep. `RenderTreeRootElement` may be deletable — confirm.

**Recommendation:** **keep `RenderObjectElement` + `RenderSlot`**. Investigate `RenderTreeRootElement` — if it has 0 impls and 0 callers, delete.

---

### 💀 [API SURFACE | LOW]: `dashmap` dep used by one zero-consumer struct

**Evidence:** [`crates/flui-foundation/Cargo.toml:20`](../../crates/flui-foundation/Cargo.toml) — `dashmap = { workspace = true }`. The only use site is [`crates/flui-foundation/src/observer.rs:303`](../../crates/flui-foundation/src/observer.rs) in `HashedObserverList`. HashedObserverList has 0 consumers.

**Recommendation:** Drop `dashmap` from flui-foundation `Cargo.toml`. Either delete `HashedObserverList` or fall back to `RwLock<HashMap>` until a concurrent-heavy consumer arrives.

---

### 💀 [SHALLOW | LOW]: `view/animated.rs` (294 LOC) ties to non-existent `flui-animation`

**Evidence:**
- [`crates/flui-view/src/view/animated.rs`](../../crates/flui-view/src/view/animated.rs) — 294 LOC. `AnimatedView` trait, `AnimationBehavior`, listener subscription.
- `flui-animation` is **disabled in workspace** (`Cargo.toml: # "crates/flui-animation"`).
- The AnimatedView trait depends on `flui_foundation::Listenable` — which exists. So AnimatedView is functional in isolation but the user-facing animation crate it's designed for doesn't compile.

**Cost:** AnimatedView is a public API tied to a disabled subsystem. Not broken, but not exercised either.

**Recommendation:** **keep with `// REVISIT_AFTER: flui-animation re-enables` doc comment**. The dependency-target alignment is fine.

---

## Dead Code Table

| Item | Location | Evidence | Hidden-use risk | Verdict | Action |
|------|----------|----------|-----------------|---------|--------|
| `flui_view::view::view::ViewKey` trait | [view/view.rs:103](../../crates/flui-view/src/view/view.rs) | 0 impls; `View::key()` returns `Option<&dyn` this `>` but all concrete keys impl `flui_foundation::ViewKey` | None — incompatible with concrete keys | **Zombie trait — type-system bug** | **delete + retype View::key to flui_foundation::ViewKey** |
| `flui_tree::IndexedSlot<I>` + `Slot` + `SlotBuilder` + `SlotIter` | [iter/slot.rs](../../crates/flui-tree/src/iter/slot.rs) | 911 LOC; 0 production callers; collides with flui-view::IndexedSlot | None | **Zombie + collision** | **delete entire slot.rs module** |
| `flui-foundation::TargetPlatform` | [platform.rs](../../crates/flui-foundation/src/platform.rs) | 0 workspace consumers; duplicates flui-types::TargetPlatform with different variants | None | **Duplicate** | **delete, keep flui-types one as canonical** |
| `flui-tree::state::{Mountable, Unmountable, NodeState, Mounted, Unmounted}` | [state.rs](../../crates/flui-tree/src/state.rs) | 616 LOC; 0 impls outside flui-tree's own doc-tests | None | **Architecture theater** | **`pub(crate)` + REVISIT_AFTER marker** |
| `flui-tree::depth::{Depth, AtomicDepth, MAX_TREE_DEPTH, DepthAware}` | [depth.rs](../../crates/flui-tree/src/depth.rs) | 1143 LOC; 0 production consumers; flui-rendering uses raw `usize` | None | **Zombie type system** | **`pub(crate)` or shrink ≤200 LOC** |
| `flui-tree::visitor::*` (TreeVisitor/TreeVisitorMut + 5 concrete visitors + composition + fallible) | [visitor/](../../crates/flui-tree/src/visitor/) | 2550 LOC; flui-rendering implements its own visit_depth_first | None | **Proven dead** | **delete entire visitor/ submodule** |
| `flui-tree::iter::cursor::TreeCursor` | [iter/cursor.rs](../../crates/flui-tree/src/iter/cursor.rs) | 1057 LOC; 0 production consumers | None | **Proven dead** | **delete** |
| `flui-tree::iter::path::{TreePath, IndexPath, TreeNavPathExt}` | [iter/path.rs](../../crates/flui-tree/src/iter/path.rs) | 1150 LOC; 0 production consumers | **High** — hot-reload integration may need IndexPath | **Pending consumer** | **`pub(crate)` IndexPath, delete TreePath + TreeNavPathExt** |
| `flui-tree::diff::{ChildDiff, TreeDiff, DiffOp}` | [diff.rs](../../crates/flui-tree/src/diff.rs) | 1234 LOC; flui-view explicitly rejects tree-diff for O(N) reconciliation | None | **Proven dead** | **delete diff.rs entirely** |
| `flui-tree::traits::node::{Node, NodeExt, NodePredicate, NodeTypeInfo, NodeVisitor}` | [traits/node.rs](../../crates/flui-tree/src/traits/node.rs) | 305 LOC; 0 production consumers | None | **Proven dead** | **delete or `pub(crate)`** |
| `ElementBuildContext::depend_on_inherited` + `get_inherited` + `find_ancestor_view` + `find_ancestor_state` + `find_root_ancestor_state` + `find_render_object` + `dispatch_notification` (no handler) | [context/element_build_context.rs:189-318](../../crates/flui-view/src/context/element_build_context.rs) | 7 of 10 BuildContext methods return None / no-op with `// Placeholder` | **Critical** — the most-used user-facing API of any UI framework | **Half-implemented hot path** | **architectural decision + unimplemented!() until fixed** |
| `GlobalKey<T>::current_element` + `current_state` | [key/global_key.rs:78,91](../../crates/flui-view/src/key/global_key.rs) | `// TODO: Implement via GlobalKeyRegistry`; registry exists in BuildOwner but no production caller invokes register_global_key | **Critical** | **Stub** | **wire Element::mount → BuildOwner::register_global_key** |
| `view/root.rs:487,494` `unimplemented!()` | [view/root.rs](../../crates/flui-view/src/view/root.rs) | `attach_to_pipeline_owner` / `detach_from_pipeline_owner` panic; binding has working pipeline owner plumbing | None — alternate path works | **Stale legacy** | **delete or wire to RenderTree** |
| `MergedListenable` | [notifier.rs:404](../../crates/flui-foundation/src/notifier.rs) | 0 consumers; field `source_listener_ids` is `#[allow(dead_code)]` | None | **Forward-looking** | **`pub(crate)` + REVISIT_AFTER** |
| `HashedObserverList` + `SyncObserverList` | [observer.rs:303](../../crates/flui-foundation/src/observer.rs) | 0 consumers; entire `dashmap` dep exists for this | None | **Forward-looking** | **`pub(crate)` + drop dashmap dep** |
| `FluiError` | [assert.rs:41](../../crates/flui-foundation/src/assert.rs) | 0 consumers; duplicates FoundationError | None | **Duplicate error type** | **delete, keep FoundationError** |
| `RenderTreeRootElement` | [element/render_object_element.rs](../../crates/flui-view/src/element/render_object_element.rs) | 0 impls (only RenderObjectElement is implemented) | None | **Unused trait** | **investigate; likely delete** |
| `crate_summary()` + `VERSION` | [flui-tree/src/lib.rs:286,291](../../crates/flui-tree/src/lib.rs) | Cosmetic public API; only exercised by self-tests | None | **Decoration** | **delete or `pub(crate)`** |
| `tree/reconciliation.rs::ReconcileAction` enum | [tree/reconciliation.rs:18](../../crates/flui-view/src/tree/reconciliation.rs) | `#[allow(dead_code)]`; "Will be used when full reconciliation is implemented" | Real — TODO | **Forward-looking** | **leave with `// USED_BY:` marker or delete** |
| `flui-tree::error::TreeError` variants used only by flui-tree | [error.rs](../../crates/flui-tree/src/error.rs) | 378 LOC of error variants; most variants only used internally | Possible | **Needs manual confirmation** | **investigate variant-by-variant** |

---

## Restructuring Plan

### Step 1 — Type-system unification (highest priority)

1. **Delete `flui_view::view::view::ViewKey` trait**. Change `View::key()` signature to return `Option<&dyn flui_foundation::ViewKey>`. Remove view-local `ViewKey` re-exports from `flui-view/src/lib.rs:142` + prelude. All concrete keys already impl the foundation trait — zero migration burden.
2. **Delete `flui-tree::iter::slot` submodule** (911 LOC: `IndexedSlot<I>`, `Slot`, `SlotBuilder`, `SlotIter`). Remove from `flui-tree/src/lib.rs:174` + prelude. `flui-view::IndexedSlot<T>` + `ElementSlot` become the single canonical types.
3. **Delete `flui-foundation::platform::TargetPlatform`**. Keep `flui-types::TargetPlatform`. Update `flui-foundation::consts::{IS_DESKTOP, IS_MOBILE, IS_WEB}` to use `cfg!` directly (already do). Remove the re-export from `flui-foundation/src/lib.rs:237`.

### Step 2 — Half-implemented hot paths

4. **Resolve `ElementBuildContext` lifetime issue.** Pick the callback API: `fn with_inherited<T: 'static, R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R`. Mark current `depend_on_inherited`/`get_inherited`/`find_ancestor_view`/`find_ancestor_state`/`find_root_ancestor_state` as `#[deprecated]` for one release cycle, then remove. `find_render_object` needs RenderElement integration (separate issue).
5. **Wire `GlobalKey::current_element` + `current_state` to BuildOwner registry.** Either (a) plumb BuildOwner into `ElementBase::mount`, OR (b) use static GlobalKey registry (`OnceLock<Mutex<HashMap<u64, ElementId>>>`). Pick (a) for Flutter parity. Until landed, mark `current_element`/`current_state` as `unimplemented!()` to fail fast.
6. **Implement `dispatch_notification` actual bubbling.** The walk-up in `element_build_context.rs:302` doesn't check `NotifiableElement::on_notification`. Add the check.
7. **Fix or delete `view/root.rs:487,494` `unimplemented!()`.** Likely stale — `WidgetsBinding::attach_root_widget` (`binding.rs:563-571`) has working pipeline owner plumbing. Confirm and delete the stale path.

### Step 3 — flui-tree compression (the biggest LOC win)

8. **Delete `flui-tree::visitor/` submodule** (2550 LOC: TreeVisitor/TreeVisitorMut + 5 concrete visitors + composition + fallible). Production crates implement their own depth-first traversals (e.g., `flui-rendering::storage::tree::visit_depth_first`).
9. **Delete `flui-tree::iter::cursor::TreeCursor`** (1057 LOC). 0 consumers.
10. **Delete `flui-tree::diff` module** (1234 LOC). Production reconciliation in flui-view explicitly rejects tree diffing.
11. **`pub(crate)` `flui-tree::depth::{AtomicDepth, DepthAware}`** + shrink `Depth` to ≤ 200 LOC (just `Depth(usize)` + `root`/`child_depth`/`parent_depth`/`get`/`is_root`). Remove `try_*`, `saturating_*`, `distance_to`, `is_deeper_than`, etc.
12. **`pub(crate)` flui-tree's `state` module** (`Mountable`, `Unmountable`, `Mounted`, `Unmounted`, `NodeState`). Mark `// REVISIT_AFTER: real consumer materializes`.
13. **Delete `flui-tree::iter::path::{TreePath, TreeNavPathExt}`**. Keep `IndexPath` at `pub(crate)` until flui-hot-reload integration.
14. **Delete `flui-tree::traits::node` module** (305 LOC). 0 consumers.
15. **Update `flui-tree/src/lib.rs`** — strip ~50% of the re-exports + prelude entries. Rewrite the lib doc-comment to emphasize the actually-used surface (TreeRead/TreeNav/TreeWrite + Arity types + Ancestors/Descendants/Siblings iterators).

### Step 4 — flui-foundation cleanup

16. **`pub(crate)` `MergedListenable` + `HashedObserverList` + `SyncObserverList`.** Drop `dashmap` workspace dep from `flui-foundation/Cargo.toml:20`.
17. **Delete `FluiError`** (assert.rs:41). Keep `FoundationError`. Remove re-export from `flui-foundation/src/lib.rs:162`.
18. **Audit ID type usage.** Identify which of the 30+ ID types in `id.rs` are actually used (ViewId, ElementId, RenderId, LayerId, SemanticsId are core; AnimationId, FrameCallbackId, FrameId, TaskId, TickerId belong to flui-scheduler which has its own Handle<T> system). `pub(crate)` the unused ones.

### Step 5 — Element behavior cleanup

19. **Investigate `RenderTreeRootElement`** ([element/render_object_element.rs](../../crates/flui-view/src/element/render_object_element.rs)). 0 impls — likely delete.
20. **Investigate `reconciliation::ReconcileAction`** enum ([tree/reconciliation.rs:18](../../crates/flui-view/src/tree/reconciliation.rs)) — `#[allow(dead_code)]` but documented as TODO. Add `// USED_BY: full reconciliation implementation` or delete.
21. **Address `InheritedBehavior::on_update` TODO** ([element/behavior.rs:541](../../crates/flui-view/src/element/behavior.rs)) — "Mark all dependents as needing rebuild if update_should_notify returns true". This is core InheritedWidget semantics; cannot ship without it. Wire to `BuildOwner::schedule_build_for(dep_id, dep_depth)` for each dependent.

### Step 6 — Tests and regression protection

22. **Add integration test exercising `BuildContext::depend_on::<T>()`** post-Step 4. Currently no test catches the broken implementation because no test calls it.
23. **Add integration test for `GlobalKey::current_state` round-trip** post-Step 5.
24. **Add integration test for `Notification` bubbling that asserts the handler is invoked** — current dispatch_notification stub passes any test that just verifies bubbling stops.
25. **Re-run `cargo clippy --workspace --all-targets --all-features -- -D warnings`** after each step.

---

## Optimization Plan

| Area | Current cost | Proposed change | Expected gain | Risk | Benchmark/test |
|------|--------------|-----------------|---------------|------|----------------|
| flui-tree LOC | 18K LOC; ~60-70% zombie | Delete visitor/cursor/diff/path/node; pub(crate) state/depth | -10K LOC (≈55%); faster cold compile; smaller doc surface | Low — 0 production consumers of deleted items | Compile time before/after; `cargo bloat` |
| `ChangeNotifier::notify_listeners` allocation | `Vec<ListenerCallback> = self.listeners.lock().values().cloned().collect()` — per-notify Vec alloc + N Arc clones | Flutter pattern: fixed-size `_listeners: List<VoidCallback?>` with null tombstones during reentrant remove (change_notifier.dart:140-149) | Eliminate per-notify Vec alloc; bounded slot reuse | Medium — locking around `_count + _notificationCallStackDepth` is subtle | Criterion bench on 1000 notifications with 10 listeners |
| `BuildOwner::dirty_elements` BinaryHeap | `BinaryHeap<Reverse<DirtyElement>>` + `HashSet<ElementId>` for dedup; two structures kept in sync | Use a single `BTreeMap<usize, ElementId>` keyed by depth (preserves order + dedup) | Less memory + atomic insertion | Low — ordering semantics identical | Bench scheduling 1000 elements at varied depths |
| ChangeNotifier `Arc<Mutex<HashMap<ListenerId, Arc<dyn Fn()>>>>` | Mutex over HashMap; per-listener Arc | Match Flutter shape — `Vec<Option<Callback>>` with id→index map; halves indirection on iterate | 2-3× notify throughput | Medium — listener registration semantics differ | Criterion bench |
| `ObserverList<T>` triple-structure | VecDeque<Option<(ObserverId, T)>> + HashMap<ObserverId, usize> + free_slots Vec | Slab<T> + secondary `HashMap<ObserverId, usize>` (slab handles free slots) | Simpler + faster | Low | Bench add/remove churn |
| `WidgetsBinding::with_element_tree_mut` borrows | `f(&mut self.inner.write().element_tree)` — full write lock for any tree mutation | Split `inner` into separate `RwLock<ElementTree>` + `RwLock<BuildOwner>` so read-only ElementTree access doesn't block dirty-marking | Less contention in concurrent scenarios | Medium — borrow shapes change | Bench dirty-marking under concurrent reads |
| `ElementBuildContext` per-element allocation | `Arc<RwLock<ElementTree>>` + `Arc<RwLock<BuildOwner>>` cloned per build | Pass `&BuildContext` by reference; only clone for stored callbacks | Lower per-element overhead | Medium — lifetime gymnastics | Criterion bench 10k element rebuild |

---

## What to Preserve

Do not touch these. They earn their place:

- **`Element<V, A, B>` unified design** + `ElementBehavior<V, A>` trait — replaces ~5 separate Element variants with one generic struct + 6 behavior impls. Saves ~500 LOC of boilerplate.
- **`ElementCore<V, A>::dirty: Arc<AtomicBool>`** — lock-free dirty-mark from listener callbacks. Animation behavior requires this.
- **`BuildOwner::dirty_elements: BinaryHeap<Reverse<DirtyElement>>` + `dirty_set: HashSet<ElementId>`** for dedup + depth-ordered processing. Flutter parity (`framework.dart:2918 _scheduledFlushDirtyElements` + `BuildScope._dirtyElements`).
- **`ChangeNotifier::notify_listeners` snapshot-then-fire** ([notifier.rs:158-163](../../crates/flui-foundation/src/notifier.rs)) — correctly handles reentrant `add_listener`/`remove_listener` from within callbacks. The doc-comment explicitly mentions matching Flutter's re-entrancy semantics. The implementation differs from Flutter's null-tombstone approach but is correct.
- **`Key(NonZeroU64)` + niche optimization** — `Option<Key>` = 8 bytes. Compile-time `from_str("name")` via FNV-1a.
- **`impl_binding_singleton!` macro** — clean composition pattern for Flutter's mixin-based bindings.
- **`Lifecycle` enum** (Initial/Active/Inactive/Defunct) — direct Flutter parity, simple, correct.
- **`Arity` types** (Leaf/Single/Optional/Variable) + `ChildrenStorage` + `ArityStorage` — the one part of flui-tree that is genuinely used by all production trees.
- **`Ancestors`/`Descendants`/`DescendantsWithDepth`/`AllSiblings` iterators** — used by flui-rendering/storage/tree.rs, flui-layer/tree/tree_traits.rs, flui-semantics/tree.rs.
- **`TreeRead`/`TreeNav`/`TreeWrite`/`TreeWriteNav` traits** — the actual API surface that production tree types implement.
- **`reconcile_children` O(N) algorithm** ([tree/reconciliation.rs:51](../../crates/flui-view/src/tree/reconciliation.rs)) — Flutter-aligned approach (explicitly avoids tree-diff per Flutter's design note). Correct skeleton even if `view.key()` is broken.
- **`WidgetsBinding::draw_frame` phase orchestration** — build → finalize_tree → first_frame_reporting. Matches Flutter's `binding.dart` drawFrame structure.
- **`PipelineOwner` propagation via `set_pipeline_owner_any`** at root mount ([tree/element_tree.rs:171-200](../../crates/flui-view/src/tree/element_tree.rs)). Type-erased Arc pattern enables RenderObjectElement to create RenderObjects without a generics leak.

---

## Priority Order (initial)

1. **Type-system unification** (ViewKey, IndexedSlot, TargetPlatform) — collisions are framework-blocking
2. **Half-implemented hot paths** (BuildContext, GlobalKey, dispatch_notification) — public API does nothing
3. **flui-tree compression** — delete confirmed-dead modules; ~10K LOC win
4. **flui-foundation cleanup** — pub(crate) zero-consumer types; drop dashmap
5. **Element behavior follow-ups** — `RenderTreeRootElement`, `ReconcileAction`, InheritedBehavior dependent notification
6. **Performance** — only after Steps 1-5 land

See [Part III](#part-iii--combined-priority-order) for the updated priority list incorporating Flutter cross-reference.

---

# Part II — Flutter Cross-Reference

Cross-reference of FLUI framework-spine against Flutter source at `flutter/packages/flutter/lib/src/{widgets,foundation}/`. Flutter is reference, not blueprint — Rust-idiomatic divergences are OK if intentional.

## Section 1 — `flui-view` vs Flutter `widgets/framework.dart`

Flutter `framework.dart` is 7,455 LOC and contains: Widget, Element, ComponentElement, StatelessElement, StatefulElement, State, ProxyElement, InheritedElement, ParentDataElement, RenderObjectElement, BuildOwner, BuildScope, BuildContext, GlobalKey/GlobalObjectKey, InheritedNotifier helpers, error widgets.

### Coverage table (sampled by symbol)

| Flutter symbol | Location | FLUI equivalent | Status |
|---------------|----------|-----------------|--------|
| `Widget` | framework.dart | `View` trait (view/view.rs:49) | ✓ adapted; ImpL via `Box<dyn ElementBase>` |
| `Element` | framework.dart:3557 | `Element<V, A, B>` (element/unified.rs:52) + `ElementBase` (view.rs:128) | ✓ unified design — Rust improvement |
| `ComponentElement` | framework.dart | `StatelessBehavior` + `StatefulBehavior` (behavior.rs) | ✓ |
| `StatelessElement` | framework.dart:5884 | `StatelessElement<V> = Element<V, Single, StatelessBehavior>` | ✓ |
| `StatefulElement` | framework.dart:5900 | `StatefulElement<V> = Element<V, Single, StatefulBehavior<V>>` | ✓ |
| `State<T>` | framework.dart | `<V as StatefulView>::State` associated type + `ViewState<V>` trait | ✓ adapted to associated types |
| `ProxyElement` | framework.dart | `ProxyElement<V> = Element<V, Single, ProxyBehavior>` | ✓ |
| `InheritedElement` | framework.dart:6252 | `InheritedElement<V> = Element<V, Single, InheritedBehavior<V>>` | ✓ shell present |
| `InheritedWidget` | framework.dart | `InheritedView` trait (view/inherited.rs:72) | ✓ |
| `ParentDataElement` | framework.dart | `ParentDataElement` + `ParentDataView` (view/parent_data.rs) | ✓ |
| `RenderObjectElement` | framework.dart:6611 | `RenderObjectElement` trait + `Element<V, Variable, RenderBehavior<V>>` impl | ✓ |
| `BuildOwner` | framework.dart:2901 | `BuildOwner` (owner/build_owner.rs:57) | ✓ skeleton; misses BuildScope nesting |
| `BuildScope` | framework.dart | — | ✗ MEDIUM gap |
| `BuildContext` | framework.dart | `BuildContext` trait + `ElementBuildContext` concrete | ⚠ **stub for 7/10 methods** |
| `GlobalKey<T>` | framework.dart:159 | `GlobalKey<T>` (key/global_key.rs:52) | ⚠ **current_element/current_state are TODOs** |
| `LabeledGlobalKey<T>` | framework.dart:203 | — | ✗ LOW gap |
| `GlobalObjectKey<T>` | framework.dart:245 | `ObjectKey` (key/object_key.rs) — different semantics | ⚠ partial |
| `Key`/`LocalKey`/`ValueKey<T>` | foundation/key.dart | `Key` (NonZeroU64), `ValueKey<T>` | ✓ |
| `UniqueKey` | foundation/key.dart | `UniqueKey` | ✓ |
| `ChangeNotifier` | foundation/change_notifier.dart:139 | `ChangeNotifier` (notifier.rs:115) | ✓ adapted |
| `ValueNotifier<T>` | foundation/change_notifier.dart:542 | `ValueNotifier<T>` | ✓ |
| `Listenable` | foundation/change_notifier.dart | `Listenable` trait (notifier.rs:68) | ✓ |
| `Listenable.merge` | foundation/change_notifier.dart:495 | `MergedListenable` | ✓ but 0 consumers |
| `Notification` | widgets/notification_listener.dart:56 | `Notification` trait (element/notification.rs:45) | ⚠ dispatch broken |
| `NotificationListener<T>` | widgets/notification_listener.dart:100 | — | ✗ HIGH gap (no widget side yet) |
| `WidgetsBinding` | widgets/binding.dart | `WidgetsBinding` (binding.rs:374) | ✓ |
| `WidgetsBindingObserver` | widgets/binding.dart | `WidgetsBindingObserver` (binding.rs:221) | ✓ |
| `BindingBase` | foundation/binding.dart | `BindingBase` (foundation/binding.rs:106) | ✓ |
| `FocusManager` | widgets/focus_manager.dart | — | ✗ HIGH gap (referenced in BuildOwner test but no impl) |
| `InheritedNotifier` / `InheritedModel` | widgets/inherited_notifier.dart | — | ✗ MEDIUM gap |

### View findings

#### 💀 [PARITY | CRITICAL]: BuildContext API surface — 7 of 10 methods are stubs

Cross-reference reinforces the [self-audit finding](#-half-implemented--critical-elementbuildcontext-returns-nonenop-for-7-of-10-buildcontext-methods). Flutter's `Element` (which implements `BuildContext`) has these working methods (framework.dart:3557+):
- `dependOnInheritedWidgetOfExactType<T>()` → calls `_dependents.add(this); inheritFromElement(...)` (~framework.dart:4470+)
- `getInheritedWidgetOfExactType<T>()` → `_inheritedElements?[T]?.widget` lookup
- `findAncestorWidgetOfExactType<T>()` → `_findAncestorOfType` walks `_parent` chain
- `findAncestorStateOfType<T>()` → similar walk testing for StatefulElement
- `findRootAncestorStateOfType<T>()` → walks to root
- `findRenderObject()` → `_findRenderObject()` walks child chain
- `dispatchNotification(Notification)` → walks ancestors via `_parent`, calls `_notificationTree?.dispatchNotification`

**Updated verdict:** FLUI's `ElementBuildContext` advertises the same API as Flutter but **does not implement it**. This must be fixed before any non-trivial widget tree can be built.

#### 💀 [PARITY | CRITICAL]: `GlobalKey` registry not wired

Flutter's pattern (framework.dart:3148, :3178-3201):
- `BuildOwner._globalKeyRegistry: Map<GlobalKey, Element>`
- `Element.mount()` → if `widget.key is GlobalKey`, calls `owner._registerGlobalKey(key, this)` (framework.dart:_registerGlobalKey)
- `Element.unmount()` → calls `owner._unregisterGlobalKey(key, this)` (framework.dart:_unregisterGlobalKey)
- `GlobalKey._currentElement` getter: `_globalKeyRegistry[this]` (framework.dart:173)

FLUI:
- `BuildOwner.global_keys: HashMap<u64, ElementId>` (build_owner.rs:65) — registry exists.
- `register_global_key(&mut self, key_hash, element)` exists (line 296).
- **`Element::mount()` does NOT call `register_global_key`.** Trace: `unified.rs:182 fn mount(&mut self, parent, slot) { self.core.mount(parent, slot); self.behavior.on_mount(&mut self.core); }`. Neither `core.mount` nor any `behavior.on_mount` reaches the BuildOwner.
- `GlobalKey::current_element` doesn't read the registry (returns `None`).

**Updated verdict:** Wire Element::mount → BuildOwner registration. Requires plumbing BuildOwner reference through ElementBase::mount (matches Flutter's `Element._owner: BuildOwner?` field at framework.dart).

#### 💀 [PARITY | HIGH]: `BuildScope` nesting not implemented

Flutter has `BuildScope` (framework.dart) — a per-`LookupBoundary`/`PageRoute` build scope with its own `_dirtyElements: List<Element>`, `_building: bool` flag, and `_dirtyElementsNeedsResorting: bool`. Allows independent build batches.

FLUI has a single global `BuildOwner.dirty_elements`. No BuildScope concept.

**Cost:** Without BuildScope, the entire tree is one batch — cannot rebuild a subtree in isolation. Acceptable for current development phase; will matter for `PageRoute` / lazy widget tree work.

**Recommendation:** Defer until route widgets land. Add `// REVISIT_AFTER: PageRoute support` marker on BuildOwner.

#### 💀 [PARITY | HIGH]: `Element.activate/deactivate` lifecycle — `_InactiveElements` finalization

Flutter (framework.dart):
- `BuildOwner._inactiveElements: _InactiveElements` is a dedicated class with `_unmount(Element)` recursive walker.
- `Element.deactivate()` → calls `super.deactivate()` + clears dependencies from inherited widgets.
- `Element.activate()` → reattaches to parent, re-registers global key.
- `finalizeTree()` → walks inactive elements, calls `_unmount`, fires `dispose` on each.

FLUI:
- `BuildOwner.inactive_elements: Vec<InactiveElement>` (build_owner.rs:73).
- `BuildOwner::finalize_tree` (line 223) sorts by depth (deepest first), collects elements_to_unmount, calls `node.element_mut().unmount()`, removes from tree.
- `BuildOwner::collect_elements_to_unmount` (line 262) recursively walks children.

**Verdict:** Working but `Element::activate/deactivate` paths don't clear inherited dependencies (line 542 InheritedBehavior::on_update has the TODO for it). Update inherited dependents on activate too.

#### 💀 [GAP | HIGH]: No `NotificationListener<T>` widget

Flutter has `NotificationListener<T>` (notification_listener.dart:100) — the actual widget that intercepts bubbling Notifications. FLUI has the `Notification` trait + `NotifiableElement` trait + dispatch code, but **no widget** that creates a `NotifiableElement`.

**Cost:** Notification bubbling has no possible handler. Notifications dispatch into the void.

**Recommendation:** Add `NotificationListener<T>` widget when widget layer matures. Until then, document the gap.

#### 💀 [PARITY | MEDIUM]: `BuildOwner.lockState` callback scope missing

Flutter `BuildOwner.lockState(VoidCallback callback)` (framework.dart:3013) — increments `_debugStateLockLevel`, runs callback, decrements. Used by `State.dispose` to prevent setState during disposal.

FLUI: `BuildOwner` has `building: bool` (debug-only) + `scope_depth: usize` (debug-only) but no `lock_state` API. `BuildScopeGuard` exists but is for build scope, not state lock.

**Recommendation:** Add `BuildOwner::lock_state<F: FnOnce()>(&mut self, callback: F)` matching Flutter. Will be needed when `State::dispose` materializes.

#### 💀 [GAP | MEDIUM]: `InheritedNotifier` + `InheritedModel` patterns absent

Flutter has `InheritedNotifier<T extends Listenable>` (rebuilds dependents when notifier fires) and `InheritedModel<T>` (aspect-based dependency tracking). FLUI's `InheritedView` is the most basic form only.

**Cost:** Common Flutter pattern (`Provider`, `Riverpod`) leans on InheritedNotifier. Adding it later costs little.

**Recommendation:** Defer until first consumer.

#### ✓ EARNED ADDITION: Unified `Element<V, A, B>` design

Flutter has 6+ Element subclasses (StatelessElement, StatefulElement, ProxyElement, InheritedElement, ParentDataElement, RenderObjectElement, plus _NotificationElement, _RawViewElement, etc.) with significant code duplication.

FLUI consolidates into one struct `Element<V: View, A: Arity, B: ElementBehavior<V, A>>` + 6 behavior impls. Saves ~500 LOC of lifecycle boilerplate. **Keep.**

#### ✓ EARNED ADDITION: `ElementCore<V, A>` composition

Lifecycle/depth/dirty/pipeline_owner state lives in `ElementCore`, behaviors compose over it. Flutter mixes these into Element directly. FLUI's separation is cleaner.

#### Coverage summary: **~75%** of Flutter widget framework API present in FLUI; the 25% gap is concentrated in `BuildContext` dependency-injection methods (CRITICAL), GlobalKey wiring (CRITICAL), and NotificationListener (HIGH).

---

## Section 2 — `flui-tree` vs Flutter tree helpers

Flutter has **no separate "tree" crate**. Tree traversal lives in:
- `widgets/framework.dart` — `Element.visitChildren`, `Element.visitChildElements`, `Element.findAncestorElementOfType`, `Element.findRenderObject`.
- `rendering/object.dart` — `RenderObject.visitChildren`, `RenderObject.attach`, `RenderObject.detach`.

Each tree has its own traversal — Flutter does not abstract a generic "tree" API.

### Tree-side findings

#### 💀 [DIVERGENCE | CRITICAL]: flui-tree is a **speculative abstraction layer** with 60-70% LOC unused

The combined `flui-tree` machinery (18K LOC) is intended to provide a generic tree API consumed by 4 production trees (Element, Render, Layer, Semantics). In practice:

| flui-tree subsystem | Used by production? | Status |
|---------------------|----------------------|--------|
| `Arity` types (Leaf/Single/Optional/Variable) | YES — all 4 production trees | **Keep** |
| `ArityStorage` / `ChildrenStorage` | YES — flui-view + flui-rendering | **Keep** |
| `TreeRead` / `TreeNav` / `TreeWrite` traits | YES — flui-rendering, flui-layer, flui-semantics impl them | **Keep** |
| `Ancestors` / `DescendantsWithDepth` / `AllSiblings` iterators | YES — flui-rendering, flui-layer, flui-semantics use these | **Keep** |
| `Identifier` trait | YES — re-exported from flui-foundation | **Keep** |
| `Depth` / `AtomicDepth` / `MAX_TREE_DEPTH` | **NO** — production uses raw `usize` | **Zombie** |
| `Mountable` / `Unmountable` / `NodeState` typestate | **NO** — incompatible with Slab storage | **Zombie** |
| `TreeVisitor` / `TreeVisitorMut` + 5 visitors + composition + fallible | **NO** — production writes its own visit_dfs | **Zombie** |
| `TreeCursor` | **NO** | **Zombie** |
| `TreePath` / `IndexPath` / `TreeNavPathExt` | **NO** | **Zombie (defer IndexPath for hot-reload)** |
| `ChildDiff` / `TreeDiff` / `DiffOp` / `ChildOp` | **NO** — production reconciliation rejects tree diffing per Flutter design | **Zombie** |
| `Node` / `NodeExt` / `NodeTypeInfo` / `NodeVisitor` | **NO** | **Zombie** |
| `IndexedSlot` / `Slot` / `SlotBuilder` / `SlotIter` | **NO** — collides with flui-view::IndexedSlot | **Zombie + collision** |

**Updated verdict:** flui-tree should be **compressed to ≤ 6K LOC** — the Arity+Storage system + the TreeRead/TreeNav/TreeWrite traits + the 3 actually-used iterators. Delete or `pub(crate)` everything else.

#### 💀 [PARITY | LOW]: No Flutter equivalent for the typestate `Mountable`/`Unmountable`

Flutter's Elements have `_lifecycleState: _ElementLifecycle` enum (initial, active, inactive, defunct) at runtime. Same approach FLUI's `Lifecycle` enum already uses. The typestate `Mountable`/`Unmountable` is an over-engineered alternative that doesn't fit Slab-storage trees.

**Recommendation:** Confirmed — demote to `pub(crate)`.

#### ✓ EARNED ADDITION: `Arity` system

Flutter has no compile-time child-count safety. `RenderObjectWithChildMixin` (single child) vs `ContainerRenderObjectMixin` (variable children) are separate mixins with runtime checks. FLUI's `Leaf`/`Single`/`Optional`/`Variable` traits + `ChildrenStorage` give the same guarantees at compile time. **Keep.**

---

## Section 3 — `flui-foundation` vs Flutter `foundation/`

Flutter `foundation/` contains: `key.dart` (Key/LocalKey/ValueKey/UniqueKey/ObjectKey), `change_notifier.dart` (ChangeNotifier/ValueNotifier/Listenable/_MergingListenable), `observer_list.dart` (ObserverList<T> + HashedObserverList<T>), `diagnostics.dart`, `binding.dart` (BindingBase), `assertions.dart` (FlutterError), `platform.dart` (TargetPlatform).

### Coverage table (sampled)

| Flutter | Location | FLUI | Status |
|---------|----------|------|--------|
| `Key` abstract base | foundation/key.dart:8 | — (FLUI has concrete `Key` only) | ⚠ flattened |
| `LocalKey` | foundation/key.dart | — | ✓ correctly omitted (no LocalKey vs GlobalKey distinction; types differ structurally) |
| `ValueKey<T>` | foundation/key.dart | `ValueKey<T>` (key.rs:377) | ✓ |
| `UniqueKey` | foundation/key.dart | `UniqueKey` (key.rs:455) | ✓ |
| `ObjectKey` | foundation/key.dart | `ObjectKey` (in flui-view/key/object_key.rs) | ✓ correctly located per Flutter convention |
| `ChangeNotifier` mixin | foundation/change_notifier.dart:139 | `ChangeNotifier` struct (notifier.rs:115) | ✓ adapted |
| `Listenable` interface | foundation/change_notifier.dart | `Listenable` trait | ✓ |
| `ValueListenable<T>` | foundation/change_notifier.dart | `ValueListenable<T>` trait | ✓ |
| `ValueNotifier<T>` | foundation/change_notifier.dart:542 | `ValueNotifier<T>` | ✓ + `value_mut`, `replace`, `take`, `update` extensions |
| `_MergingListenable` (internal) | foundation/change_notifier.dart:495 | `MergedListenable` (pub) | ✓ but pub when Flutter has it private |
| `Listenable.merge()` factory | foundation/change_notifier.dart | `MergedListenable::new(Vec<Box<dyn Listenable>>)` | ✓ |
| `dispose()` lifecycle | foundation/change_notifier.dart:376 | — | ✗ MEDIUM gap |
| `debugAssertNotDisposed` | foundation/change_notifier.dart:181 | — | ✗ LOW gap |
| `ObserverList<T>` | foundation/observer_list.dart | `ObserverList<T>` | ✓ adapted with id-based remove |
| `HashedObserverList<T>` | foundation/observer_list.dart | `HashedObserverList<T>` (dashmap) | ✓ but 0 consumers |
| `BindingBase` | foundation/binding.dart | `BindingBase` (foundation/binding.rs:106) | ✓ |
| `BindingBase.checkInstance` | foundation/binding.dart | `check_instance::<B>()` (binding.rs:210) | ✓ |
| `Diagnostics*` types | foundation/diagnostics.dart | `DiagnosticsNode` + `DiagnosticsProperty` + `DiagnosticsBuilder` | ✓ |
| `Diagnosticable` mixin | foundation/diagnostics.dart | `Diagnosticable` trait | ✓ used by flui-rendering (objects/center.rs:68) |
| `FlutterError` | foundation/assertions.dart | `FluiError` (assert.rs) + `FoundationError` (error.rs) | ⚠ two error types |
| `TargetPlatform` | foundation/platform.dart | `TargetPlatform` (foundation/platform.rs) + `TargetPlatform` (flui-types/platform/target_platform.rs) | ⚠ two enums |
| `defaultTargetPlatform` | foundation/platform.dart | `TargetPlatform::current()` | ✓ |
| `kDebugMode` / `kReleaseMode` / `kIsWeb` | foundation/constants.dart | `DEBUG_MODE` / `RELEASE_MODE` / `IS_WEB` (consts.rs) | ✓ |
| `precisionErrorTolerance` etc | foundation/math.dart | `EPSILON` / `EPSILON_F32` (consts.rs:80,83) | ✓ |
| `VoidCallback` | foundation/basic_types.dart | `VoidCallback` (callbacks.rs) | ✓ |
| `ValueChanged<T>` | foundation/basic_types.dart | `ValueChanged<T>` | ✓ |

### Foundation findings

#### 💀 [PARITY | HIGH]: `ChangeNotifier::dispose` + reentrancy guards missing

Flutter's `ChangeNotifier.dispose()` (change_notifier.dart:376-393):
- Asserts not yet disposed.
- Asserts not currently inside `notifyListeners()` (line 380 — "The dispose() method was called during notifyListeners()").
- Sets `_listeners = _emptyListeners; _count = 0`.

Flutter's `ChangeNotifier.addListener` / `removeListener` use `_notificationCallStackDepth` + `_reentrantlyRemovedListeners` to allow safe reentrant removal during notification iteration (lines 348-362, 430-491). This is **non-trivial** correctness work — the comment block at line 419-491 explains why.

FLUI `notifier.rs:158-163`:
```rust
pub fn notify_listeners(&self) {
    let callbacks: Vec<ListenerCallback> = self.listeners.lock().values().cloned().collect();
    for callback in &callbacks {
        callback();
    }
}
```
Snapshot-then-fire correctly handles `add_listener`/`remove_listener` during notification (the new listeners are in the snapshot or not, the removed ones are still in the snapshot but the underlying map is updated). **Correctness verified.**

But:
- No `dispose()` method — listeners just dropped when ChangeNotifier is dropped.
- No "must not be called after dispose" assertion.
- The `cloned().collect()` allocates a Vec per notify — Flutter uses tombstone-marking in the same backing list to avoid allocation.

**Recommendation:**
1. **Add `ChangeNotifier::dispose()`** — clears listeners, sets a `disposed: AtomicBool`. Subsequent `add_listener` panics in debug.
2. **Add debug assertion** in `add_listener`/`remove_listener`/`notify_listeners` that not yet disposed.
3. **(Optional perf)** Replace `HashMap<ListenerId, Arc<Fn>>` snapshot with Flutter's `Vec<Option<Arc<Fn>>>` tombstone pattern. Wait for benchmark evidence.

#### 💀 [DIVERGENCE | MEDIUM]: FLUI has `MergedListenable` as public, Flutter as private `_MergingListenable`

Flutter's `_MergingListenable` (change_notifier.dart:495) is private. The public API is the factory `Listenable.merge([a, b, c])`.

FLUI has `MergedListenable` as `pub struct` with a `new(Vec<Box<dyn Listenable + Send>>)` constructor — 0 consumers, accidentally public.

**Recommendation:** **`pub(crate)`** `MergedListenable`. Add a `Listenable::merge<I: IntoIterator<Item = Box<dyn Listenable + Send>>>(iter: I) -> Box<dyn Listenable>` static-style factory.

#### ✓ PARITY: `BindingBase` + `checkInstance` + singleton pattern

The Rust idioms are different (OnceLock + atomic flag vs Dart's static initialization), but the contract matches:
- `BindingBase::init_instances()` ↔ `BindingBase.initInstances()`.
- `check_instance<B: HasInstance>()` ↔ `BindingBase.checkInstance<T>(T? instance)`.
- The `impl_binding_singleton!` macro neatly handles the OnceLock + AtomicBool boilerplate.

Verdict: **clean adaptation**. Keep.

#### ✓ PARITY: `Key` + `ValueKey` + `UniqueKey`

Flutter:
- `Key` abstract base with const constructor (key.dart:30).
- `LocalKey` abstract intermediate (key.dart:54).
- `ValueKey<T>` (key.dart:67).
- `UniqueKey` (key.dart:113).
- `ObjectKey` (key.dart:144).
- `GlobalKey<T>` (in framework.dart:159).

FLUI:
- `Key(NonZeroU64)` — concrete; no LocalKey distinction.
- `ValueKey<T>` — same.
- `UniqueKey` — same.
- `ObjectKey` — in flui-view (correct location per Flutter, see foundation/key.rs:18-20 doc).
- `GlobalKey<T>` — in flui-view (correct location).

**The "Key abstract base" flattening** — Flutter has `Key`/`LocalKey`/`GlobalKey` as a 3-level hierarchy. FLUI flattens because LocalKey/GlobalKey are structurally different types (LocalKey is a `Copy` u64-ish thing, GlobalKey is an `Arc`-backed registry entry). Rust-idiomatic.

Verdict: **clean adaptation**. The only issue is the `ViewKey` trait duplication (covered in [Findings](#-duplication--critical-two-viewkey-traits-in-workspace--type-system-collision)).

#### ✓ EARNED ADDITION: `Key::from_str` const-time FNV-1a hash

Flutter has no compile-time `Key("name")`. FLUI's `const fn from_str(s: &str) -> Key` (key.rs:105) gives zero-cost compile-time keys: `const HEADER: Key = Key::from_str("header")`. **Keep.**

#### ✓ EARNED ADDITION: 30+ typed ID types via `Id<Marker>` system

Flutter uses `int` for IDs in many places (FrameCallbackId, ObserverId, etc.). FLUI's `Id<T: Marker>` system (id.rs) + the generated typed IDs (ViewId, ElementId, RenderId, LayerId, SemanticsId, AnimationId, FrameId, TaskId, TickerId, PointerId, GestureId, etc.) provide type-safe distinction. **Keep — but audit which are actually used** (flui-scheduler reimplemented with `Handle<T>` instead of using these).

#### Coverage summary: **~90%** of Flutter foundation API present in FLUI. Gaps concentrated in ChangeNotifier lifecycle (dispose + reentrancy guards) and InheritedNotifier patterns.

---

# Part III — Combined Priority Order

## Completion status (2026-05-21)

Findings **#1, #2, #3, #5, #6, #7, #8** of Part III closed by [`docs/plans/2026-05-21-002-feat-framework-spine-repair-plan.md`](../plans/2026-05-21-002-feat-framework-spine-repair-plan.md) — atomic-commit-per-unit shape across 17 units on branch `feat/framework-spine-repair`:

| Finding | Unit | Commit |
|---------|------|--------|
| #3 TargetPlatform collision (one of three) | U1 | `a740b28a` |
| #3 ViewKey collision (one of three) | U2 | `64e438bf` |
| #3 IndexedSlot collision (one of three) | U3 | `de4c8265` |
| #5 Listenable/Observer bloat + dashmap drop | U4 | `c009156a` |
| #5 FluiError deletion | U5 | `2cdf792b` |
| #6 ChangeNotifier::dispose + disposed-state assert | U6 | `b0c914bf` |
| #5 ID-type audit (17 deletions, 11 kept) | U7 | `67afd624` |
| #2 ElementOwner split-borrow + Element lifecycle plumbing | U8 | `e05ff86c` |
| #1 BuildContext::depend_on_inherited + InheritedBehavior::on_view_updated | U9 | `318540e4` |
| #1 BuildContext::get_inherited (non-recording) | U10 | `109c81d1` |
| #1 BuildContext::find_ancestor_view/state/root_state | U11 | `613ebeef` |
| #1 BuildContext::find_render_object | U12 | `1f627e4b` |
| #1 BuildContext::dispatch_notification + NotifiableElement | U13 | `e3c7ac2c` |
| #2 GlobalKey register/unregister + state migration + lookup | U14 | `0b99e247` |
| #7 unimplemented!() removal in view/root.rs | U15 | `f884b6ac` |
| Behavior commons extraction | U16 | `b3ecb45c` |
| Audit annotation + plan status flip | U17 | (this commit) |

Finding **#4** (full `flui-tree` migration) deferred to follow-up multi-PR series per [post-audit correction](#post-audit-correction-2026-05-21). One tiny precursor (IndexedSlot, U3) landed; the full consumer migration of `flui-rendering` / `flui-layer` / `flui-semantics` / remaining `flui-view` paths is planned separately.

Findings **#9** (integration tests for the formerly-stubbed APIs) and **#10** (defer items) remain open as written.

---

| Priority | Action | Why now |
|----------|--------|---------|
| **1** | **Fix BuildContext stubs** — pick callback API (`with_inherited<T, R>`), mark current `depend_on_inherited`/etc. as `#[deprecated]` + actually-working under new name. Wire `dispatch_notification` to check `NotifiableElement::on_notification`. | The single most-used user-facing API of any UI framework is non-functional. Flutter cross-ref confirms 5+ critical methods are stubs. |
| **2** | **Wire `GlobalKey` registry** — plumb `&mut BuildOwner` through `ElementBase::mount/unmount`. Make `GlobalKey::current_element` read the registry. | Second most-used API. Flutter pattern at framework.dart:3148+ is well-defined; FLUI has the registry + the methods, just no wiring. |
| **3** | **Resolve type-system collisions** — delete `flui_view::view::view::ViewKey` (retype `View::key` → `flui_foundation::ViewKey`). Delete `flui_tree::iter::slot` (911 LOC; collides with flui-view::IndexedSlot). Delete `flui-foundation::TargetPlatform` (keep flui-types). | Public API ambiguity blocks downstream code. ViewKey collision means GlobalKey can't be returned from View::key. |
| **4** (inverted per [post-audit correction](#post-audit-correction-2026-05-21)) | **Migrate production crates TO `flui-tree` unified API.** Pick one consumer (likely `flui-rendering::pipeline::owner` — already uses raw `usize` for depth at `pipeline/owner.rs:513`) and migrate it to use `flui-tree::Depth`/`TreeRead`/`TreeNav`. Repeat for `flui-layer`, `flui-semantics`, `flui-view`. Redesign any `flui-tree` abstraction that turns out incompatible with real use cases — do not delete by default. | Closes the "zero production consumers" gap the auditor flagged. The unified-tree API is the architectural ставка; production crates writing bespoke traversals is the bug, not `flui-tree`'s existence. Largest scope item on this list. |
| **5** | **flui-foundation cleanup** — `pub(crate)` `MergedListenable` + `HashedObserverList` + `SyncObserverList`. Drop `dashmap` dep. Delete `FluiError` (keep `FoundationError`). Audit unused ID types (scheduler reinvents Handle<T>). | Removes API surface ambiguity, removes one transitive dep, removes one of two error types. |
| **6** | **Add `ChangeNotifier::dispose` + disposed-state assertion** matching Flutter (change_notifier.dart:181, 376). | Production listeners can outlive the notifier silently today — no diagnostic. Flutter has explicit asserts. |
| **7** | **Implement `view/root.rs:487,494` `unimplemented!()`** OR delete the stale path. WidgetsBinding::attach_root_widget has working pipeline owner plumbing at binding.rs:563-571 — likely the legacy paths are dead. | Constitution forbids `unimplemented!()` in production paths. |
| **8** | **Implement `InheritedBehavior::on_update` dependent notification** — when `update_should_notify` returns true, call `BuildOwner::schedule_build_for(dep_id, dep_depth)` for each dependent. | Core InheritedWidget semantics; cannot ship widget layer without it. TODO at behavior.rs:541. |
| **9** | **Add integration tests** — `BuildContext::depend_on::<T>()` round-trip; `GlobalKey::current_state` round-trip; `Notification` bubbling to a `NotifiableElement` handler. | Today no test exercises these (because they're broken/stubbed). Add the tests BEFORE fixing, to ensure regressions are caught. |
| **10** | **Defer** — `BuildScope` nesting, `NotificationListener<T>` widget, `InheritedNotifier`/`InheritedModel`, `FocusManager`, `LabeledGlobalKey`/`GlobalObjectKey` distinct types | Wait for widget layer + route layer materialization. Don't pre-bake APIs without consumers. |
| **11** | **Don't touch** — `Element<V, A, B>` unified design, `ElementCore<V, A>` composition, `BuildOwner::dirty_elements BinaryHeap`, `ChangeNotifier::notify_listeners` reentrancy semantics, `Key(NonZeroU64)`, `impl_binding_singleton!`, `Lifecycle` enum, `Arity` system, `TreeRead/TreeNav/TreeWrite` traits, `Ancestors/Descendants/AllSiblings` iterators, `reconcile_children` O(N) algorithm, `WidgetsBinding::draw_frame` orchestration, `PipelineOwner` propagation via `set_pipeline_owner_any` | Confirmed strong by Flutter cross-ref. These are the load-bearing parts. |

### Combined Mythos Insight

**The framework spine has a confidence problem.** Public APIs advertise functionality (`BuildContext::depend_on::<T>()`, `GlobalKey::current_state`, `dispatch_notification`) that production code does not implement. Tests pass because they don't exercise the unimplemented paths. The Flutter cross-reference shows the gaps are not "missing features" — they're **stubbed-out public surface where Flutter has the real impl in framework.dart**. Step 1 + Step 2 of Part III are not new work; they're **finishing the existing API**.

**flui-tree is the biggest LOC waste.** Six subsystems (visitor, cursor, path, diff, state, depth, plus node traits) accounting for ~10K LOC have **zero production consumers**. They are pre-emptive abstractions. The Flutter cross-reference confirms Flutter does not have an equivalent generic tree layer — Flutter's trees implement their own traversal directly. FLUI's experiment with "a single tree API for all four trees" failed silently — production trees wrote their own traversals because the trait objects had unwanted overhead and the typestate was incompatible with Slab storage. Compress to ≤ 30% of current LOC; revisit if a real consumer materializes.

**flui-foundation is mostly right but bloated.** The `Key`/`ValueKey`/`UniqueKey`/`ChangeNotifier`/`ValueNotifier`/`BindingBase`/`Diagnosticable` quartet maps cleanly to Flutter and is used in production. The bloat is in MergedListenable + HashedObserverList + 30+ ID types where only ~8 IDs are actually used + two error types. Cleanup is low-risk LOC removal.

**Type-system collisions are the only hard-to-roll-back mistake.** `ViewKey` + `IndexedSlot` + `TargetPlatform` duplications can be fixed now with zero production breakage (because no production code uses the duplicate side). Fix them before they ossify.

---

# Appendix A — Investigation Trail

## Tool dispatches

- **Sequential Read/Grep passes** mapped flui-view/flui-tree/flui-foundation structure: lib.rs surface, modules, traits, impl counts, hot paths.
- **Cross-reference** against Flutter source at `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/{widgets,foundation}/` via direct Read on:
  - `widgets/framework.dart` (7,455 LOC) — Widget/Element/BuildOwner/BuildScope/GlobalKey/StatelessElement/StatefulElement/InheritedElement/RenderObjectElement.
  - `widgets/notification_listener.dart` — Notification/NotificationListener.
  - `foundation/change_notifier.dart` (568 LOC) — ChangeNotifier mixin + reentrancy + ValueNotifier + _MergingListenable.
  - `foundation/key.dart` (116 LOC) — Key/LocalKey/ValueKey/UniqueKey/ObjectKey/GlobalKey relationships.
- **Targeted grep passes** to verify each finding:
  - Confirm 2 `ViewKey` traits, only flui-foundation one has impls.
  - Confirm 2 `IndexedSlot` types, only flui-view one has prelude consumers.
  - Confirm 2 `TargetPlatform` enums with different variants.
  - Confirm 7/10 `ElementBuildContext` methods return None/no-op.
  - Confirm `GlobalKey::current_element` + `current_state` are TODOs.
  - Confirm `unimplemented!()` in `view/root.rs:487,494`.
  - Confirm `flui-tree::Depth`/`Mountable`/`TreeVisitor`/`TreeCursor`/`TreePath`/`ChildDiff`/`Node` have zero production consumers via workspace-wide grep.
  - Confirm `dashmap` is used only by `HashedObserverList` which has 0 consumers.
  - Confirm `MergedListenable` has 0 consumers; the `source_listener_ids` field is `#[allow(dead_code)]`.
  - Confirm `FluiError` + `FoundationError` both have 0 workspace consumers.
  - Confirm `reconcile_children` in flui-view doesn't use `flui-tree::ChildDiff`.

## Workspace state at audit time (2026-05-21)

- All three audited crates ACTIVE in `Cargo.toml` workspace members.
- flui-view depends on flui-tree (only for Arity types — confirmed via grep `use flui_tree`) + flui-rendering + flui-foundation + flui-interaction + flui-types + flui-log.
- flui-tree depends on flui-foundation + ambassador (for arity storage delegation) + thiserror + tracing + smallvec.
- flui-foundation depends on bitflags + dashmap + parking_lot + thiserror + tracing.
- Workspace branch: `naughty-jackson-324931` (audit worktree).
- Rust edition: 2024; minimum rust-version: 1.94.

## Files referenced

Repo-relative paths (clickable in markdown viewers):

- [`Cargo.toml`](../../Cargo.toml)
- [`CLAUDE.md`](../../CLAUDE.md)
- [`crates/flui-foundation/src/lib.rs`](../../crates/flui-foundation/src/lib.rs)
- [`crates/flui-foundation/src/key.rs`](../../crates/flui-foundation/src/key.rs)
- [`crates/flui-foundation/src/notifier.rs`](../../crates/flui-foundation/src/notifier.rs)
- [`crates/flui-foundation/src/observer.rs`](../../crates/flui-foundation/src/observer.rs)
- [`crates/flui-foundation/src/binding.rs`](../../crates/flui-foundation/src/binding.rs)
- [`crates/flui-foundation/src/platform.rs`](../../crates/flui-foundation/src/platform.rs)
- [`crates/flui-foundation/src/error.rs`](../../crates/flui-foundation/src/error.rs)
- [`crates/flui-foundation/src/assert.rs`](../../crates/flui-foundation/src/assert.rs)
- [`crates/flui-foundation/src/id.rs`](../../crates/flui-foundation/src/id.rs)
- [`crates/flui-foundation/src/debug.rs`](../../crates/flui-foundation/src/debug.rs)
- [`crates/flui-foundation/Cargo.toml`](../../crates/flui-foundation/Cargo.toml)
- [`crates/flui-tree/src/lib.rs`](../../crates/flui-tree/src/lib.rs)
- [`crates/flui-tree/src/depth.rs`](../../crates/flui-tree/src/depth.rs)
- [`crates/flui-tree/src/diff.rs`](../../crates/flui-tree/src/diff.rs)
- [`crates/flui-tree/src/state.rs`](../../crates/flui-tree/src/state.rs)
- [`crates/flui-tree/src/iter/slot.rs`](../../crates/flui-tree/src/iter/slot.rs)
- [`crates/flui-tree/src/iter/cursor.rs`](../../crates/flui-tree/src/iter/cursor.rs)
- [`crates/flui-tree/src/iter/path.rs`](../../crates/flui-tree/src/iter/path.rs)
- [`crates/flui-tree/src/visitor/mod.rs`](../../crates/flui-tree/src/visitor/mod.rs)
- [`crates/flui-tree/src/traits/node.rs`](../../crates/flui-tree/src/traits/node.rs)
- [`crates/flui-tree/src/arity/mod.rs`](../../crates/flui-tree/src/arity/mod.rs)
- [`crates/flui-tree/Cargo.toml`](../../crates/flui-tree/Cargo.toml)
- [`crates/flui-view/src/lib.rs`](../../crates/flui-view/src/lib.rs)
- [`crates/flui-view/src/binding.rs`](../../crates/flui-view/src/binding.rs)
- [`crates/flui-view/src/view/view.rs`](../../crates/flui-view/src/view/view.rs)
- [`crates/flui-view/src/view/inherited.rs`](../../crates/flui-view/src/view/inherited.rs)
- [`crates/flui-view/src/view/root.rs`](../../crates/flui-view/src/view/root.rs)
- [`crates/flui-view/src/key/global_key.rs`](../../crates/flui-view/src/key/global_key.rs)
- [`crates/flui-view/src/owner/build_owner.rs`](../../crates/flui-view/src/owner/build_owner.rs)
- [`crates/flui-view/src/context/build_context.rs`](../../crates/flui-view/src/context/build_context.rs)
- [`crates/flui-view/src/context/element_build_context.rs`](../../crates/flui-view/src/context/element_build_context.rs)
- [`crates/flui-view/src/tree/element_tree.rs`](../../crates/flui-view/src/tree/element_tree.rs)
- [`crates/flui-view/src/tree/reconciliation.rs`](../../crates/flui-view/src/tree/reconciliation.rs)
- [`crates/flui-view/src/element/unified.rs`](../../crates/flui-view/src/element/unified.rs)
- [`crates/flui-view/src/element/generic.rs`](../../crates/flui-view/src/element/generic.rs)
- [`crates/flui-view/src/element/behavior.rs`](../../crates/flui-view/src/element/behavior.rs)
- [`crates/flui-view/src/element/lifecycle.rs`](../../crates/flui-view/src/element/lifecycle.rs)
- [`crates/flui-view/src/element/slot.rs`](../../crates/flui-view/src/element/slot.rs)
- [`crates/flui-view/src/element/notification.rs`](../../crates/flui-view/src/element/notification.rs)
- [`crates/flui-view/src/element/render_object_element.rs`](../../crates/flui-view/src/element/render_object_element.rs)
- [`crates/flui-view/Cargo.toml`](../../crates/flui-view/Cargo.toml)

Flutter reference (absolute paths — outside the worktree, at main repo root):

- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/widgets/framework.dart` — 7,455 LOC
- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/widgets/binding.dart`
- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/widgets/notification_listener.dart`
- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/foundation/change_notifier.dart` — 568 LOC
- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/foundation/key.dart` — 116 LOC

---
