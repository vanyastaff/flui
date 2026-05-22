---
title: "Mythos Audit — flui-foundation × flui-tree"
date: 2026-05-22
status: audit-complete
audit_methodology: claude-mythos (12-phase rust audit, foundation + tree-traits pass)
crates_audited:
  - flui-foundation
  - flui-tree
reference_sources:
  - flutter/packages/flutter/lib/src/foundation/change_notifier.dart
  - flutter/packages/flutter/lib/src/foundation/key.dart
  - flutter/packages/flutter/lib/src/foundation/diagnostics.dart
  - flutter/packages/flutter/lib/src/foundation/binding.dart
  - flutter/packages/flutter/lib/src/widgets/framework.dart  (GlobalKey, Element lifecycle)
predecessor_cycles:
  - docs/research/2026-05-21-flui-interaction-scheduler-audit.md (Cycle 1, closed in PRs #85-#98)
  - docs/research/2026-05-22-flui-layer-semantics-audit.md (Cycle 2, closed in PR #100/#101)
authors:
  - Mythos (via Claude Opus 4.7)
---

# Mythos Audit: `flui-foundation` × `flui-tree`

> Deep audit across FLUI's **foundation primitives + tree-abstraction surface** — 12 source files (~5.4K LOC) in flui-foundation, 30 source files (~18.0K LOC) in flui-tree, **~23.4K LOC total**.
>
> Goal: identify zombie abstractions in the unified-tree spine, depth-constant drift, parallel sibling-tree mutation APIs that should consolidate into `TreeWrite`, lifecycle/Drop gaps, hot-path allocation hazards, drift from Flutter `foundation/*.dart` + `framework.dart::GlobalKey`, and Constitution Principle violations — without breaking active integration with `flui-rendering`, `flui-view`, `flui-layer`, `flui-semantics`, `flui-scheduler`, `flui-app`.
>
> **Cycle**: this audit continues the audit-execute series that produced PRs #81 / #82 / #83 / #84 / #85-#98 / #100 / #101 against `vanyastaff/flui`. Previous cycle audited the compositing + a11y layer (`flui-layer` × `flui-semantics`); see [`2026-05-22-flui-layer-semantics-audit.md`](2026-05-22-flui-layer-semantics-audit.md) → closed as PR #100 (24 units, +1.5kLOC delta) + PR #101 (4 followup fixes). Cycle 1 before that audited the input + frame-loop layer (`flui-interaction` × `flui-scheduler`); see [`2026-05-21-flui-interaction-scheduler-audit.md`](2026-05-21-flui-interaction-scheduler-audit.md) → closed as PRs #95/#96/#97 + companions.

---

## Table of Contents

- [Mythos Improvement Verdict](#mythos-improvement-verdict)
- [Part I — Architecture review](#part-i--architecture-review)
- [Part II — Findings](#part-ii--findings)
  - [Foundation findings (I-1 .. I-22)](#foundation-findings-i-1--i-22)
  - [Tree findings (T-1 .. T-25)](#tree-findings-t-1--t-25)
- [Part III — Flutter drift catalog](#part-iii--flutter-drift-catalog)
- [Part IV — Final combined priority order](#part-iv--final-combined-priority-order)
- [Appendix A — Investigation receipts](#appendix-a--investigation-receipts)
- [Status (closed)](#status-closed)

---

## Mythos Improvement Verdict

The pair **`flui-foundation` (5,424 LOC, 12 files) × `flui-tree` (18,024 LOC, 30 files)** sits at the absolute bottom of the workspace DAG and is **structurally more divergent from its consumer base than either cycle 1 or cycle 2 pair**. Two structural problems dominate:

(a) **flui-tree is ~55% zombie surface against the unified-tree intent**. Per memory [[flui-tree-unified-interface-intent]], the crate was specced as the canonical home for tree primitives — `TreeRead` / `TreeNav` / `TreeWrite` traits, slot positions, iterators, visitors, diff, arity storage, the typestate `Mountable`/`Unmountable` lifecycle. **In practice only `TreeRead<I>` + `TreeNav<I>` + the arity *markers* (`Leaf` / `Single` / `Optional` / `Variable` zero-sized types) + `IndexedSlot<I>` are consumed by downstream crates**. Counting zero-external-consumer modules: `visitor/{mod,composition,fallible}.rs` (2,550 LOC), `diff.rs` (1,234), `iter/cursor.rs` (1,057), `iter/path.rs` (1,150), `iter/{breadth,depth}_first.rs` (655), `arity/{storage,arity_storage,accessors}.rs` (3,051), `state.rs` (616), `traits/node.rs` (305). **~10,600 LOC = ~58% of flui-tree ships with no external consumers** (Appendix A.2 for greps). This is not a "delete signal" — memory says it's migration gap — but the audit MUST flag that the gap has grown rather than narrowed since cycle 2's PR #100 added per-tree cascading `add_child`/`remove` to LayerTree and SemanticsTree *outside* the TreeWrite trait.

(b) **`TreeWrite::remove` documents non-cascade as the default semantic** — the same footgun cycle 2 fixed for `LayerTree::remove` and `SemanticsTree::remove` in PR #100 U12+U13. The `TreeWrite<I>` trait at [`crates/flui-tree/src/traits/write.rs:64-81`](../../crates/flui-tree/src/traits/write.rs) says *"This removes only the specified node. Children handling depends on the implementation (may be orphaned or removed)"*. The current sole implementer in the workspace (`RenderTree` via `flui-rendering/src/storage/tree.rs:677-691`) does NOT cascade — it removes only the specified node and updates the parent's children list, leaving descendants orphaned in the slab. Memory [[flui-tree-unified-interface-intent]] is correct that flui-tree should be the canonical surface, but the trait contract today *codifies* the footgun rather than mandating cascade or providing `remove`/`remove_shallow` pair as PR #100/U12 did at the LayerTree level. **This is the most important finding in this cycle** — the same lifecycle correctness work cycle 2 did for the data-trees has to land at the trait-contract level.

**Three best things:**

1. **`ChangeNotifier::dispose` + `disposed: AtomicBool` + `check_disposed` + reentrancy-safe snapshot-then-fire** (`crates/flui-foundation/src/notifier.rs:151-303`). This is the **canonical Flutter-faithful + Rust-idiomatic** lifecycle template adopted across the workspace in PR #84 — Cycle 1 mirrored it on `Ticker` (PR #95 U24), cycle 2 mirrored it on `LayerNode` (PR #100 U8). The dispose pattern correctly handles three reentrancy modes: (i) idempotent second-dispose (line 200 `swap(true, AcqRel)` no-op), (ii) snapshot-then-fire iteration immune to mid-flight dispose (line 252 collect-then-iterate), (iii) debug-panic / release-warn degrade for use-after-dispose (line 218 `check_disposed`). Mirrors Flutter's `change_notifier.dart:181` `_debugAssertNotDisposed` + `:376` `dispose` exactly. Don't touch.

2. **`Id<T: Marker>` generic ID system with `#[repr(transparent)]` over `NonZeroUsize` + niche optimization + `markers::*` module + blanket `Identifier` impl** (`crates/flui-foundation/src/id.rs:81-573`). This is **architecturally cleaner than any of cycle 1's PointerId widening or cycle 2's SemanticsId** — a single generic carrying type discipline for 9 ID types (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`, `ListenerId`, `ObserverId`, `FrameId`, `FrameCallbackId`, `TaskId`, `TickerId`) with the macro `ids!` declaring marker enum types in `pub mod markers` for orthogonal IdGenerator-style use (flui-scheduler exploits this at `id.rs:73` `pub struct IdGenerator<M: Marker>`). The niche-optimization invariant is enforced at compile-time via `const _: () = { assert!(size_of::<RawId>() == size_of::<Option<RawId>>()); };` (line 59-62). This is the constitutional **ID Offset Pattern** as a generic — every consumer gets the +1/−1 slab offset for free. Don't touch.

3. **`BindingBase` + `HasInstance` + `impl_binding_singleton!` macro with double-init guard via `AtomicBool::store(true, Release)` inside `OnceLock::get_or_init`** (`crates/flui-foundation/src/binding.rs:106-194`). Adopted uniformly across workspace bindings: `SchedulerBinding`, `RendererBinding`, `GestureBinding`, `WidgetsBinding`, `SemanticsBinding`. Three-phase contract: (i) `BindingBase::init_instances` runs once, (ii) `HasInstance::INITIALIZED: &'static AtomicBool` flips on first `instance()`, (iii) `check_instance<B>()` panics if accessed before init. **The macro composes well** — each binding writes a one-line `impl_binding_singleton!(MyBinding);` and gets the full singleton protocol. Mirrors Flutter's `BindingBase` mixin chain (`binding.dart:79-180`) with zero dyn-dispatch. Don't touch.

**Worst complexity tax:**

1. **Quadruple depth-constant drift** — four different constants encode "tree depth" with no documented relationship:
   - `MAX_TREE_DEPTH = 256` in `crates/flui-tree/src/depth.rs:68` — global cap, used by `Depth::new_checked` validation.
   - `TreeNav::MAX_DEPTH = 32` at `crates/flui-tree/src/traits/nav.rs:42` — "for stack allocation optimization". Iterators read this for size_hint upper bound (`iter/ancestors.rs:101`).
   - `TreeVisitor::MAX_STACK_DEPTH = 64` at `crates/flui-tree/src/visitor/mod.rs:137` — "Expected maximum tree depth for stack allocation".
   - `TreeVisitorMut::STACK_SIZE = 48` at `crates/flui-tree/src/visitor/mod.rs:178` — "Stack allocation size hint".
   - Plus `DescendantStack = SmallVec<[Id; 32]>` hard-coded inline-32 at `iter/descendants.rs:12` — independent of all four.
   - Plus `Children<[I; 8]>` hard-coded inline-8 used by `Descendants::next` at `iter/descendants.rs:108` and by `TreeNavExt::path_to_node` `SmallVec<[I; 8]>` at `iter/path.rs:105` — independent of `TreeNav::INLINE_CHILDREN_THRESHOLD = 8`.

   **Cycle 2 PR #101 review hit exactly this** — Codex flagged that `MARK_PROPAGATION_MAX_DEPTH = 32` in flui-layer/flui-semantics was too shallow for the cascade walk; the fix was to remove the hard cap. The same fragmentation now lives in flui-tree's iterator stacks — when a real consumer needs to walk deeper than 32, every iterator and visitor needs an independent fix. **One constant should authority** — `MAX_TREE_DEPTH` from depth.rs as the upper bound, `DEFAULT_INLINE_DEPTH` (~32-64) as the SmallVec sizing hint, no other depth constants.

2. **Massive zero-consumer trait surface across `visitor/`, `diff.rs`, `cursor.rs`, `path.rs`, `breadth_first.rs`, `depth_first.rs`, `arity/{storage,arity_storage,accessors}.rs`, `state.rs`, `traits/node.rs`**. Per memory [[flui-tree-unified-interface-intent]] these are migration-gap not deletion-signal, BUT:
   - `StatefulVisitor<State, Data>` + `states::{Initial, Started, Finished}` typestate machinery (`visitor/mod.rs:659-840`, ~180 LOC) is the **exact same shape as cycle 1's deleted `typestate.rs`** (PR #93). Three-state typestate, zero external consumers, never likely to materialize because the closure-based `ForEachVisitor` already covers the use case more ergonomically. Same cycle-1 verdict applies.
   - `TypedVisitor<I, T>` + `Item<'a>` + `Collection<'a>` GAT machinery (`visitor/mod.rs:182-213`, +`visit_depth_first_typed` impl at :347-390, ~50 LOC). Zero consumers, structurally identical to `CollectVisitor` + `TreeVisitor` already present. Pure speculation.
   - `Mountable`/`Unmountable`/`Mounted`/`Unmounted` typestate lifecycle (`state.rs` 616 LOC entirely). flui-view has its own Element FSM (`view/element/lifecycle.rs`) and does NOT use this — the typestate was speccd for an Element-side migration that never happened. Spec-the-canonical-surface or delete; do not let it sit as scaffolding.
   - `Node` trait + `NodeExt` + `NodeTypeInfo` (`traits/node.rs` 305 LOC) — 0 external impls. flui-view's `Element` does not implement it. flui-layer's `LayerNode` does not implement it. Only test mocks use it.
   - `ChildrenStorage` + `ChildrenStorageExt` traits (`arity/storage.rs` 794 LOC + `arity_storage.rs` 858 LOC) + 7 typed accessors (`accessors.rs` 1,399 LOC) — 0 external consumers. The arity *markers* (`Leaf`/`Single`/`Optional`/`Variable` zero-sized types) ARE used (flui-rendering's render-objects), but the *storage* machinery is unused.

3. **`TreeWrite::remove` codifies non-cascade footgun + provides `remove_subtree` as opt-in cascade** (`crates/flui-tree/src/traits/write.rs:64-112`). Quoting the doc: *"This removes only the specified node. Children handling depends on the implementation (may be orphaned or removed). … Implementations should update parent's children list when removing a node."* The phrasing makes orphaning *acceptable*. Cycle 2 PR #100 explicitly fixed this footgun at the impl level in flui-layer (U12) and flui-semantics (U13), but the trait contract was never updated. **`RenderTree::remove` (the only TreeWrite consumer in-workspace, at `flui-rendering/src/storage/tree.rs:677-691`) inherits this footgun** — it orphans children today. The right shape is cycle 2's `remove` (cascade default) + `remove_shallow` (explicit non-cascade) pair, hoisted into the TreeWrite trait as `remove` (cascade default with `where Self: TreeNav<I>`) + `remove_shallow` (default impl that does the old behavior).

**Where dead code hides** (the verified-zero-external-consumer list):

| Module | LOC | External consumers |
|---|---|---|
| `flui-tree/src/visitor/mod.rs` (TreeVisitor/TypedVisitor/StatefulVisitor + 6 built-in visitors) | 1,264 | 0 |
| `flui-tree/src/visitor/composition.rs` (`ComposedVisitor` / `ConditionalVisitor` / `DynVisitor` / `MappedVisitor` / `TripleComposedVisitor` / `VisitorExt` / `VisitorVec`) | 648 | 0 |
| `flui-tree/src/visitor/fallible.rs` (`FallibleVisitor` / `DepthLimitVisitor` / `TryCollectVisitor` / `TryForEachVisitor` + errors) | 638 | 0 |
| `flui-tree/src/diff.rs` (`TreeDiff` / `DiffOp` / `ChildDiff` / `ChildOp` / `DiffStats`) | 1,234 | 0 |
| `flui-tree/src/iter/cursor.rs` (`TreeCursor`) | 1,057 | 0 |
| `flui-tree/src/iter/path.rs` (`TreePath` / `IndexPath` / `TreeNavPathExt`) | 1,150 | 0 |
| `flui-tree/src/iter/breadth_first.rs` (`BreadthFirstIter`) | 317 | 0 |
| `flui-tree/src/iter/depth_first.rs` (`DepthFirstIter` / `DepthFirstOrder`) | 338 | 0 |
| `flui-tree/src/arity/storage.rs` + `arity_storage.rs` + `accessors.rs` (`ChildrenStorage`/`ArityStorage`/7 accessors) | 3,051 | 0 |
| `flui-tree/src/state.rs` (`Mountable` / `Unmountable` / `Mounted`/`Unmounted` typestate) | 616 | 0 |
| `flui-tree/src/traits/node.rs` (`Node` / `NodeExt` / `NodeTypeInfo`) | 305 | 0 |
| `flui-tree/src/traits/{read,nav}.rs::TreeReadExt`/`TreeNavExt` extension methods | ~250 | 0 |
| `flui-tree/src/iter/siblings.rs::Siblings` directional iterator | 554 (partial) | 0 (`siblings()` core fn used; `Siblings` struct only) |
| `flui-foundation/src/observer.rs` (`ObserverList`) | 271 | 0 (only examples) |
| `flui-foundation/src/error.rs` (`FoundationError` + `ErrorContext`) | 335 | 0 (only examples; flui-cli uses `anyhow::Context` not this) |
| `flui-foundation/src/debug.rs::ParseDiagnosticLevelError`/`ParseDiagnosticsTreeStyleError` `FromStr` errors | ~20 | 0 |
| `flui-foundation/src/id.rs::Id::from_raw` / `Id::zip_unchecked` / `Id::new_unchecked` unsafe escape hatches | ~30 | 0 |
| **Subtotal — zero-consumer LOC** | **~11,400** | **0** |

(Methodology: ripgrep across the workspace excluding `flui-foundation/` and `flui-tree/` themselves; details in Appendix A.2.)

**Half-implemented hot paths:**

- `TreeWrite::remove` documentation explicitly delegates cascade decision to impl — same shape as cycle 2's LayerTree footgun, but at the trait-contract level. RenderTree, the only consumer, orphans descendants today. Untested, never exercised, but if/when flui-rendering tree mutates a subtree, the slab becomes corrupted.
- `TreeNav::ancestors` doesn't bound iteration — `Ancestors::next` walks until `parent()` returns `None`. A corrupted tree with a 2-node cycle (`p.parent = c, c.parent = p`) loops forever. No max-depth guard. Same in `Descendants::next` (line 99-114, `iter/descendants.rs`), which uses recursion `self.next()` on miss-and-retry — could blow stack on a malformed tree. Cycle 2's PR #101 had to handle this in flui-layer's `add_child` cycle-rejection; flui-tree's iterators have no equivalent.
- `Descendants::next` recurses via `return self.next()` (line 104, descendants.rs) on missing child — Rust does NOT guarantee tail-call optimization, so an iteration starting from a heavily-orphaned subtree can blow the stack. Should be a loop with `continue`.
- `Siblings::new` (`iter/siblings.rs:99-118`) collects `Vec<I>` of all siblings at construction time. Per-call allocation. For a tree where Siblings is created per-frame in a layout pipeline, this would be the per-frame hot path. Allocation discipline.

**Biggest optimization opportunity** — **single-source-of-truth for tree depth + tree-write trait cascade contract**. Estimated impact: ~10 hot-path consumers (RenderTree mutation, LayerTree mark propagation, ancestors/descendants iterators, visitors, slot computation) all currently fragment their own depth assumption. The structural fix is two changes: (1) hoist cycle 2's cascade-by-default `remove` from LayerTree/SemanticsTree into `TreeWrite::remove` (with `remove_shallow` opt-out) — RenderTree adopts automatically. (2) unify the four depth constants behind a single `MAX_TREE_DEPTH` cap in `flui-tree::depth` + a `MAX_INLINE_DEPTH = 64` SmallVec sizing hint, derived from it.

**Don't touch**:

- `ChangeNotifier::dispose` + `check_disposed` + snapshot-then-fire pattern (`crates/flui-foundation/src/notifier.rs:151-303`) — gold standard. PR #84 canonical, mirrored across workspace.
- `Id<T: Marker>` + `RawId(NonZeroUsize)` + `markers::*` + `Identifier` blanket (`crates/flui-foundation/src/id.rs`) — generic discipline beyond what cycle 1 needed. Two compile-time `const _: ()` asserts enforce niche-optimization invariants. Don't fragment.
- `Key` const FNV-1a hash via `Key::from_str("name")` enabling **compile-time-constant keys with no runtime cost** (`crates/flui-foundation/src/key.rs:104-111`). `ViewKey::is_global_key()` cheap-skip optimization (line 347) for GlobalKey downcast avoidance — used at `flui-view/src/tree/element_tree.rs:497`. Both well-shaped.
- `BindingBase` + `HasInstance` + `impl_binding_singleton!` macro composition (`crates/flui-foundation/src/binding.rs`) — minimal, composable, no `dyn` boundary. Constitution Principle 4 clean.
- `Depth` + `AtomicDepth` (`crates/flui-tree/src/depth.rs`) — `#[repr(transparent)]` over `usize`, `AtomicDepth` exposes `Acquire`/`Release` correctly, saturating + checked + try variants per *Rust Atomics and Locks* Ch.3. Don't touch beyond unifying `MAX_TREE_DEPTH` constant usage (T-2 finding).
- `TreeRead<I>` trait shape (`traits/read.rs`) — minimal, RPITIT for iterators, blanket impls for `&T` / `&mut T` / `Box<T>`. Don't break.
- `TreeNav<I>` *core* methods (`parent`, `children`, `ancestors`, `descendants`, `siblings`, `slot`) — well-shaped. Don't break, but consider the `Slot::with_siblings` default impl's O(children.collect-and-find) cost in the audit (T-9).
- `arity::Arity` trait + `Leaf`/`Single`/`Optional`/`Variable` zero-sized markers (`arity/types.rs:38-333`) — actually consumed by flui-rendering render-objects. The *storage* machinery is the zombie part; the markers are the load-bearing piece.
- `IndexedSlot<I>` (`iter/slot.rs`) — consumed by flui-view as `ElementSlot = IndexedSlot<ElementId>`. The crate doc declares it canonical home per memory; this consumer relationship was added in the framework-spine repair (PR #84).

---

## Part I — Architecture review

### Where these crates sit in the workspace DAG

```
flui-foundation (no in-workspace deps) ─────┬─► flui-tree (depends only on flui-foundation)
                                            │
                                            ├─► flui-types
                                            │
                                            ├─► flui-scheduler
                                            │     uses: ChangeNotifier (Listenable), Id<T>, markers::Frame, BindingBase
                                            │
                                            ├─► flui-interaction
                                            │     uses: BindingBase, HasInstance, ChangeNotifier
                                            │
                                            ├─► flui-rendering  ─────► flui-tree
                                            │     uses: Id<T>, BindingBase, Diagnosticable, ChangeNotifier
                                            │     impl TreeRead/Nav/Write<RenderId> for RenderTree
                                            │     uses arity markers (Leaf/Single/Optional/Variable)
                                            │
                                            ├─► flui-layer  ─────────► flui-tree
                                            │     uses: LayerId, ElementId
                                            │     impl TreeRead/Nav<LayerId> for LayerTree (NO TreeWrite — parallel)
                                            │
                                            ├─► flui-semantics  ─────► flui-tree
                                            │     uses: SemanticsId, BindingBase, impl_binding_singleton!
                                            │     impl TreeRead/Nav<SemanticsId> for SemanticsTree (NO TreeWrite — parallel)
                                            │
                                            ├─► flui-painting (downstream of flui-types only)
                                            │
                                            └─► flui-view  ──────────► flui-tree
                                                  uses: Key, KeyRef, Keyed, ValueKey, UniqueKey, WithKey, ViewKey
                                                  re-exports IndexedSlot as ElementSlot
                                                  has its own Element FSM (NOT using state.rs typestate)
```

**Foundation surface used** (verified at HEAD `64e5ec30`):

| Symbol | Producer | Consumer crates |
|---|---|---|
| `ElementId`, `RenderId`, `LayerId`, `SemanticsId`, `ViewId` | id.rs | flui-rendering, flui-layer, flui-semantics, flui-view, flui-tree (re-export) |
| `Id<T: Marker>` + `Marker` + `markers::*` (generic ID system) | id.rs | flui-scheduler (`IdGenerator<M: Marker>`) |
| `FrameId`, `FrameCallbackId`, `TaskId`, `TickerId` | id.rs | flui-scheduler |
| `ListenerId`, `ObserverId` | id.rs | flui-foundation::notifier (ListenerId), examples (ObserverId) |
| `ChangeNotifier` + `Listenable` + `ValueNotifier` + `ValueListenable` + `ListenerCallback` | notifier.rs | flui-animation (8 files use it as the canonical Listenable shape — disabled but real) |
| `BindingBase` + `HasInstance` + `check_instance` + `impl_binding_singleton!` | binding.rs | flui-app (RendererBinding etc.), flui-rendering, flui-interaction (GestureBinding), flui-semantics (SemanticsBinding), flui-scheduler |
| `Key` + `KeyRef` + `Keyed` + `WithKey` + `ValueKey` + `UniqueKey` + `ViewKey` | key.rs | flui-view (key system); `ViewKey::is_global_key()` consumed by GlobalKey downcast skip |
| `DiagnosticLevel` + `DiagnosticsTreeStyle` + `DiagnosticsProperty` + `DiagnosticsNode` + `DiagnosticsBuilder` + `Diagnosticable` | debug.rs | flui-rendering (10+ Diagnosticable impls on RenderObjects) |
| `VoidCallback` / `ValueChanged` / `ValueGetter` / `ValueSetter` / `Predicate` / `ValueTransformer` / `FallibleCallback` | callbacks.rs | flui-rendering, flui-view, flui-platform, flui-interaction, flui-animation |
| `EPSILON` + `EPSILON_F32` + `approx_equal` + `approx_equal_f32` + `is_near_zero*` + `DEBUG_MODE` + `RELEASE_MODE` + `IS_*` | consts.rs | various |
| `WasmNotSend` + `WasmNotSendSync` | wasm.rs | flui-foundation (Marker trait bound), flui-types |
| `debug_assert_valid!` + `debug_assert_range!` + `debug_assert_finite!` + `debug_assert_not_nan!` + `report_error!` + `report_warning!` | assert.rs | various |
| `RawId` + `Index` + `Id::from_raw` + `Id::zip_unchecked` + `Id::new_unchecked` | id.rs | **NONE** (zero external consumers) |
| `FoundationError` + `ErrorContext` | error.rs | **NONE** (only flui-foundation examples; flui-cli uses anyhow's `with_context`) |
| `ObserverList<T>` | observer.rs | **NONE** (only flui-foundation examples) |

**Tree surface used**:

| Symbol | Producer | Consumer crates |
|---|---|---|
| `TreeRead<I>` (core methods only) | traits/read.rs | flui-rendering, flui-layer, flui-semantics |
| `TreeNav<I>` (core methods only) | traits/nav.rs | same — three trees implement it; iterator consumers via the trait |
| `TreeWrite<I>` + `TreeWriteNav<I>` | traits/write.rs | **flui-rendering only** (RenderTree, with non-cascade `remove` footgun) |
| `Ancestors` + `DescendantsWithDepth` + `Descendants` iterators | iter/{ancestors,descendants}.rs | used INSIDE TreeNav impls (flui-layer, flui-semantics, flui-rendering) |
| `Arity` trait + `Leaf`/`Single`/`Optional`/`Variable`/`Exact<N>` zero-sized markers | arity/{types,traits}.rs | flui-rendering (10+ render-objects bind `RenderBox<Single>`, `RenderBox<Leaf>`, etc.) |
| `IndexedSlot<I>` | iter/slot.rs | flui-view (as `ElementSlot = IndexedSlot<ElementId>`) |
| `Depth` + `AtomicDepth` + `MAX_TREE_DEPTH` + `ROOT_DEPTH` + `DepthError` + `DepthAware` | depth.rs | conceptually used everywhere, but only flui-tree itself imports |
| `TreeError` + `TreeResult` | error.rs | flui-rendering (uses `TreeWriteNav::set_parent` error type), flui-tree internal |
| `Node` + `NodeExt` + `NodeTypeInfo` | traits/node.rs | **NONE** (zero external impls) |
| `Mountable` + `Unmountable` + `Mounted` + `Unmounted` + `NodeState` + `MountableExt` | state.rs | **NONE** (flui-view has its own Element FSM) |
| `TreeReadExt` + `TreeNavExt` extension traits | traits/{read,nav}.rs | **NONE** (the core trait methods cover all in-workspace use cases) |
| `TreeCursor` + `TreePath` + `IndexPath` + `TreeNavPathExt` + `Slot` (the non-IndexedSlot one) + `SlotBuilder` + `SlotIter` | iter/{cursor,path,slot}.rs | **NONE** for all except `IndexedSlot` |
| `BreadthFirstIter` + `DepthFirstIter` + `DepthFirstOrder` | iter/{breadth,depth}_first.rs | **NONE** |
| `Siblings` (directional) + `SiblingsDirection` + `AllSiblings` | iter/siblings.rs | **NONE** (the `TreeNav::siblings()` core method is used, but as a `flat_map` not the iterator struct) |
| `TreeVisitor` + `TreeVisitorMut` + `TypedVisitor` + `StatefulVisitor` + all built-in visitors + composition + fallible | visitor/* | **NONE** |
| `TreeDiff` + `DiffOp` + `ChildDiff` + `ChildOp` + `DiffStats` | diff.rs | **NONE** |
| `ArityStorage<T, A>` + `ChildrenStorage` + `ChildrenStorageExt` + 7 accessors + storage aliases | arity/{storage,arity_storage,accessors,aliases}.rs | **NONE** for storage; arity markers (`Leaf`/`Single` etc. ZSTs) are used |

### The unified-tree intent vs reality

Per memory [[flui-tree-unified-interface-intent]]:

> flui-tree spec'd as unified API over Flutter's multi-tree (Element/Render/Layer/Semantics); zero-consumer abstractions = migration gap, not deletion signal

This audit corroborates that intent strongly for some pieces and weakly for others:

**Strong-intent pieces** (migration gap is real and worth bridging now):
- `TreeWrite<I>` trait should be the canonical mutation surface. Today only RenderTree implements it; LayerTree (cycle 2) and SemanticsTree (cycle 2) have their own parallel `add_child`/`remove`/`remove_shallow` methods. The cycle-2 cascade work needs to land at the trait level so RenderTree inherits the fix.
- `IndexedSlot` is canonical home (cycle 2 work confirmed this — flui-view re-exports as ElementSlot). Pattern works.
- `Arity` markers (`Leaf`/`Single`/`Optional`/`Variable`) are canonical. flui-rendering exercises them.
- `Depth` + `AtomicDepth` canonical. Limited current use but the shape is right.

**Weak-intent pieces** (spec'd but no near-term consumer, structurally similar to cycle 1's deleted `typestate.rs`):
- `StatefulVisitor` typestate machinery — same shape as PR #93 deletion.
- `TypedVisitor` GAT machinery — speculation; closure-based `ForEachVisitor` covers the use case.
- `Mountable`/`Unmountable`/`Mounted`/`Unmounted` typestate — flui-view's actual Element FSM is `Initial`/`Active`/`Inactive`/`Defunct` per Flutter; the four-state Mountable/Unmountable distinction doesn't match Flutter parity and is unused.
- `Node` trait + `NodeExt` + `NodeTypeInfo` — minimal, but no consumer impls it. Could keep as a trait alias for `Identifier`-relating types or delete.
- `ChildrenStorage` + `ArityStorage` + 7 accessors — the storage layer was speccd for a generic `RenderObject::children: ArityStorage<Box<dyn RenderBox>, A>` pattern that flui-rendering ended up doing differently (per-arity-type fields like `child: BoxChild<Single>`). The actual flui-rendering shape diverged.

**Migration-gap pieces** (consumer will come from devtools or hot-reload):
- `TreeDiff` + `DiffOp` — speculation for future reconciliation, but Element-side reconciliation in flui-view doesn't use it.
- `TreeCursor` — speculation for devtools.
- `TreePath` + `IndexPath` — speculation for selection / serialization / devtools.
- `BreadthFirstIter` + `DepthFirstIter` — generic alternatives to the `TreeNav::descendants` impl.

**Audit recommendation**: distinguish in Part IV between "delete" (the StatefulVisitor/TypedVisitor/Node trait shapes that mirror cycle 1's typestate.rs) and "keep but feature-gate" (the devtools-shaped pieces — diff/cursor/path), behind a `unstable-devtools` Cargo feature defaulted off.

### Three-tree architecture and ID flow

The constitutional five-tree architecture (View / Element / Render / Layer / Semantics) routes IDs through flui-foundation's `Id<T>` system. Each tree-implementer crate:

1. Owns its slab storage in private fields (`Slab<XxxNode>` keyed on 0-based slab index).
2. Mints IDs as `XxxId::new(slab_index + 1)` (the **+1 ID Offset Pattern** the Constitution declares).
3. Exposes `TreeRead<XxxId> + TreeNav<XxxId>` for read access.
4. Should expose `TreeWrite<XxxId>` for write access — but only flui-rendering does today.
5. Provides domain-specific mutation methods (`set_parent`, `add_child`, `remove`) outside the TreeWrite contract, with implementation-specific cascade semantics.

The 5 IDs (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`) are produced by the `ids!` macro at `id.rs:500-572`. Each gets a marker enum in `pub mod markers`, then a `pub type XxxId = Id<markers::Marker>` alias. `Option<XxxId>` is 8 bytes via niche optimization. **Two compile-time asserts at `id.rs:54-62` enforce the invariant** that `RawId == size_of::<usize>()` and `Option<RawId> == size_of::<RawId>()`.

**Drift point** — `PointerId` is NOT in flui-foundation. Cycle 1 PR #96 (U9) widened the local `PointerId(i32)` in flui-interaction to `ui_events::pointer::PointerId(NonZeroU64)` from the `ui-events` crate. This was a *deliberate* divergence to align with the W3C wire format. PointerId remains a flui-interaction concern; not a foundation concern. **No action needed**, but the audit should note the pattern: IDs for **internal slab storage** belong in flui-foundation; IDs for **external wire formats** belong in their consumer crates.

### BindingBase singleton pattern

Five workspace bindings adopt the BindingBase pattern via `impl_binding_singleton!`:
- `SchedulerBinding` (flui-scheduler)
- `GestureBinding` (flui-interaction)
- `RendererBinding` (flui-app `bindings/renderer_binding.rs`)
- `SemanticsBinding` (flui-semantics, cycle 2 work)
- `WidgetsBinding` (flui-app `bindings/`)

Each follows the protocol: (a) `impl BindingBase for Self { fn init_instances(&mut self) { … } }`, (b) `Self::new()` calls `init_instances` exactly once, (c) `impl_binding_singleton!(Self)` macro emits `OnceLock<Self>` + `AtomicBool::store(true, Release)` first-init signaling. No double-init hazard.

**Subtle observation**: the macro at `binding.rs:177-195` uses `OnceLock::get_or_init` which is fine — but `INITIALIZED.store(true, Release)` fires **inside** the init closure, BEFORE `<Self>::new()` returns. If `new()` panics, the OnceLock stays uninitialized but `INITIALIZED` reads `true`. Subsequent `is_initialized() → true` + `instance()` call would attempt re-init (per OnceLock semantics — re-init after panic). Cycle 1's view-tree audit hit a similar protocol gap. Should be fixed.

### Cross-cycle pattern continuity

Patterns cycles 1 + 2 established that should propagate into this cycle's plan:

| Pattern | Established by | Should propagate to |
|---|---|---|
| **PR #84 `ChangeNotifier::dispose` template** | flui-foundation (this crate) | RenderTree (TreeWrite::clear / drop guard), maybe BindingBase teardown |
| **Cycle 1's typestate.rs deletion** (PR #93) | flui-interaction | `state.rs` (Mountable/Unmountable typestate), `visitor/mod.rs::StatefulVisitor` |
| **Cycle 2's cascade-by-default `remove`** (PR #100 U12+U13) | flui-layer + flui-semantics | `TreeWrite::remove` trait contract + RenderTree |
| **Cycle 2's `MARK_PROPAGATION_MAX_DEPTH` cap removal** (PR #101) | flui-layer mark propagation | `TreeNav::MAX_DEPTH` + visitor stack-size consts unification |
| **Cycle 1's PointerId widening** (PR #96 U9) | flui-interaction | Not applicable — PointerId is consumer-side; foundation IDs are slab-internal |
| **Cycle 2's clone-and-release lock pattern** (PR #100 U14, U22) | flui-semantics | BindingBase's OnceLock+AtomicBool synchronization (per re-init-after-panic hazard) |

---

## Part II — Findings

Findings are split between **flui-foundation (I-1 .. I-22)** and **flui-tree (T-1 .. T-25)**. Each finding follows the cycle-2 template: severity tag, evidence line refs, why-problem, Flutter ref (when applicable), proposed fix shape, blast radius.

### Foundation findings (I-1 .. I-22)

---

#### I-1 [P0 ZOMBIE | CRITICAL] `ObserverList<T>` ships with zero production consumers

**Evidence:**
- `crates/flui-foundation/src/observer.rs` — entire 271-LOC module.
- Grep `ObserverList` across workspace excluding flui-foundation tests and examples: 0 hits.
- Examples reference it: `crates/flui-foundation/examples/observer_pattern.rs` only.
- The doc-comment promises "O(1) add/remove via `HashMap` index" — useful primitive — but `ChangeNotifier` (notifier.rs) already provides the same semantic with `HashMap<ListenerId, ListenerCallback>` at `notifier.rs:132`, and IS the workspace-wide observer pattern.

**Why it's a problem:**
- 271 LOC + tests in release-mode public surface for a type nobody uses.
- ObserverList has 4 fields (`observers: VecDeque<Option<(ObserverId, T)>>` + `id_to_index: HashMap<ObserverId, usize>` + `len`, `free_slots: Vec<usize>`, `next_id: usize`) — its data model is more complex than ChangeNotifier's `HashMap<ListenerId, Arc<dyn Fn()>>`.
- The `compact()` method (lines 157-167) suggests a use case (high churn) that no consumer has.

**Fix shape:** Move to `#[cfg(feature = "observer-list")]` or delete entirely. Constitution Principle 4 — "Composition Over Inheritance" — doesn't justify keeping speculative collections without consumers.

```toml
[features]
default = []
observer-list = []  # legacy collection, not used by workspace consumers
```

**Blast radius:** Trivial. Update `lib.rs` re-exports + prelude conditionally; the workspace has no consumers.

---

#### I-2 [P0 ZOMBIE | CRITICAL] `FoundationError` + `ErrorContext` ship with zero production consumers

**Evidence:**
- `crates/flui-foundation/src/error.rs` — entire 335-LOC module.
- Grep `FoundationError` across workspace excluding flui-foundation src/tests/examples: 0 hits.
- Grep `ErrorContext` / `with_context`: only flui-cli uses `with_context` — but it imports it from **anyhow** (`crates/flui-cli/src/error.rs:359-372`), NOT from flui-foundation.
- The `FoundationError` enum has 8 variants (`InvalidId`, `InvalidKey`, `ListenerError`, `DiagnosticsError`, `NotificationError`, `AtomicError`, `SerializationError`, `Generic`) — each takes a `String` context, all use `thiserror`, none are actually emitted by any flui-foundation code (the dispose, key, id, callback modules use `panic!` / `debug_assert!` / `Result<T, E>` with E ≠ FoundationError).

**Why it's a problem:**
- 335 LOC + tests of dead surface.
- The `ErrorContext` trait shadows `anyhow::Context` — if a consumer brings both into scope, they'll collide on `with_context`. Documented in cycle 1's audit too, but never fixed.
- `is_recoverable()` / `is_structural()` / `category()` are all `const fn` decorations over an enum nobody uses.

**Fix shape:** Delete the module. Foundation operations either succeed (no error needed: ID construction is panic-on-zero, listener registration is infallible) or return their own narrow error (notifier returns `Option` / `()`; debug returns `ParseDiagnosticLevelError`). There's no shape that fits a unified `FoundationError`.

**Blast radius:** Trivial. Update `lib.rs` re-exports. Workspace has no consumers.

---

#### I-3 [P0 PARITY-DRIFT | CRITICAL] `BindingBase` `OnceLock::get_or_init` + `INITIALIZED.store(true, Release)` re-init-after-panic hazard

**Evidence:**
- `crates/flui-foundation/src/binding.rs:177-195`:
  ```rust
  #[macro_export]
  macro_rules! impl_binding_singleton {
      ($binding:ty) => {
          impl $crate::HasInstance for $binding {
              const INITIALIZED: &'static std::sync::atomic::AtomicBool = {
                  static INIT: std::sync::atomic::AtomicBool = AtomicBool::new(false);
                  &INIT
              };
              fn instance() -> &'static Self {
                  static INSTANCE: std::sync::OnceLock<$binding> = std::sync::OnceLock::new();
                  INSTANCE.get_or_init(|| {
                      Self::INITIALIZED.store(true, std::sync::atomic::Ordering::Release);
                      <$binding>::new()
                  })
              }
          }
      };
  }
  ```
- The `INITIALIZED.store(true, Release)` fires **before** `<Self>::new()` returns. If `new()` panics:
  - `OnceLock::get_or_init` propagates the panic but leaves the OnceLock un-initialized (Rust docs: "If this function panics, the cell is unchanged").
  - But `INITIALIZED` has already been flipped to `true`.
  - A subsequent caller invoking `is_initialized() → true` then `instance()` sees the panic propagate again from a fresh init attempt — or worse, on the rare contention path, gets a wrong answer.
- `check_instance` (line 210-218) uses `is_initialized()` as the only gate before `instance()`, so the false-positive is reachable from public API.

**Why it's a problem:**
- Init logic for any binding (RendererBinding, SemanticsBinding, etc.) can panic during construction — wgpu init, OS resource acquisition, dependency-binding cycle. Flutter's binding hierarchy explicitly tolerates partial init (and Flutter framework reports useful diagnostics on init failure).
- FLUI's macro silently produces incoherent state on init panic.
- Cycle 1 mentioned in passing that flui-app's BindingBase pattern is the "real production singleton" — meaning this macro is in the hot path for app startup.

**Flutter reference:** `binding.dart:79-180` — Flutter's `BindingBase` ctor calls `initInstances()` and either succeeds or throws `FlutterError`. Dart's single-threadedness means there's no parallel-instance race, but the ctor-throws-on-failure pattern is documented.

**Fix shape:** Flip the store AFTER `<Self>::new()` returns:
```rust
fn instance() -> &'static Self {
    static INSTANCE: std::sync::OnceLock<$binding> = std::sync::OnceLock::new();
    let inst = INSTANCE.get_or_init(<$binding>::new);
    // Mark initialized AFTER new() returns successfully.
    Self::INITIALIZED.store(true, std::sync::atomic::Ordering::Release);
    inst
}
```
Note: on the steady state (after first init), the `store(true, Release)` becomes a redundant atomic write per call. Mitigate via `INITIALIZED.compare_exchange(false, true, Release, Acquire).ok()` or a one-shot `init_once: AtomicBool` pattern.

**Blast radius:** 1 macro change. All 5 binding consumers (`SchedulerBinding`, `GestureBinding`, `RendererBinding`, `SemanticsBinding`, `WidgetsBinding`) get the fix automatically.

---

#### I-4 [P1 SYNC-CONTENTION | HIGH] `ChangeNotifier::notify_listeners` cloning entire callback set under lock per notify

**Evidence:**
- `crates/flui-foundation/src/notifier.rs:252`:
  ```rust
  let callbacks: Vec<ListenerCallback> = self.listeners.lock().values().cloned().collect();
  for callback in &callbacks {
      callback();
  }
  ```
- For a notifier with N listeners, the per-notify cost is:
  - `Mutex::lock` — 1
  - `Vec` allocation of size N — 1
  - `N × Arc::clone` (refcount bump) — N
  - N invocations + N refcount drops at end-of-scope.
- The Arc clone is cheap (1 atomic increment per listener) but the Vec allocation is per-notify-per-N. Flutter's `ChangeNotifier.notifyListeners()` uses a fixed-size `_listeners._listeners` array (no per-notify allocation).
- For a hot-path notifier (e.g., scroll position or animation tick), this is a frame-rate allocation pressure.

**Why it's a problem:**
- Per *Rust Performance Book* "Allocator pressure" — repeated short-lived Vec allocations at frame rate are the highest-impact category. Worse, for ChangeNotifier with rare add_listener / remove_listener but frequent notify, the Vec is the dominant cost.
- The snapshot-then-fire pattern is correct (necessary for reentrancy safety per the dispose-during-notify test at line 800-838), but the snapshot can reuse buffer.

**Flutter reference:** `change_notifier.dart:425-465` — `notifyListeners` iterates `_listeners` directly. The `_count` field tracks valid entries. No per-call allocation.

**Fix shape:**
- Cache a reusable `Vec<ListenerCallback>` field on `ChangeNotifier` (under the same mutex; clear-then-extend per notify):
  ```rust
  pub struct ChangeNotifier {
      listeners: Arc<Mutex<HashMap<ListenerId, ListenerCallback>>>,
      // Reused per-notify snapshot buffer; cleared on each notify.
      snapshot_buffer: Arc<Mutex<Vec<ListenerCallback>>>,
      next_id: Arc<AtomicUsize>,
      is_disposed: Arc<AtomicBool>,
  }
  ```
  But this complicates the reentrancy story — a callback that calls notify_listeners recursively would clobber the buffer. Need separate buffer per `notify_listeners` call.
- Better: use `SmallVec<[ListenerCallback; 4]>` since most notifiers have ≤4 listeners (per UI patterns). Stack allocation for the common case, heap fallback:
  ```rust
  use smallvec::SmallVec;
  // ...
  let mut buf: SmallVec<[ListenerCallback; 4]> = SmallVec::new();
  buf.extend(self.listeners.lock().values().cloned());
  for callback in &buf {
      callback();
  }
  ```
  Adds `smallvec` to flui-foundation dependencies. ~5 LOC change.

**Blast radius:** notifier.rs only. The change is invisible to consumers — pure performance fix.

---

#### I-5 [P1 PARITY-DRIFT | HIGH] `Key::default()` returns a fresh unique key — surprising for a `Default` impl

**Evidence:**
- `crates/flui-foundation/src/key.rs:205-213`:
  ```rust
  impl Default for Key {
      /// Default key is generated uniquely
      ///
      /// Same as calling `Key::new()`.
      #[inline]
      fn default() -> Self {
          Self::new()
      }
  }
  ```
- The `Default` trait by convention returns a *deterministic, identity-like* value (e.g., `0`, `String::new()`, `Vec::new()`). `Key::default()` calling `Key::new()` returns a *fresh atomic-counter-incremented unique value* every time — different from every other default.
- `UniqueKey::default()` (lines 496-500) — same pattern.
- The test at line 766-772 explicitly asserts `let k1 = Key::default(); let k2 = Key::default(); assert_ne!(k1, k2);` — codifies the surprising behavior.

**Why it's a problem:**
- Surprising `Default` semantics violate Rust API guidelines (`API-DEFAULT`: "If a type has a default value that is determined by some constant value, implement `Default`").
- Anyone deriving `Default` on a struct containing `Key` field will get **a different key on every construction** — breaks `Eq`/`Hash` invariants for derived structs.
- The `Default` impl exists nowhere in Flutter (Flutter uses Dart constructors; no Default trait).

**Flutter reference:** `key.dart:33-50` — `Key.empty()` constructor pattern when "no specific key" semantics needed. No Dart equivalent of Rust's `Default` trait.

**Fix shape:** Remove the `Default` impl. Force callers to choose `Key::new()` (unique) or `Key::from_u64(n)` (explicit) or `Key::from_str("name")` (compile-time const). Same for `UniqueKey::default()`.

```rust
// DELETE:
// impl Default for Key { fn default() -> Self { Self::new() } }
// impl Default for UniqueKey { fn default() -> Self { Self::new() } }
```

**Blast radius:** Audit `#[derive(Default)]` structs across the workspace that contain `Key` / `UniqueKey` — likely none today, but the audit must verify. (Grep returned no production callers of `Key::default()`.)

---

#### I-6 [P1 PARITY-DRIFT | HIGH] `Key::from_str` `const fn` returns `Key(1)` on hash-collision-with-zero — silent collision

**Evidence:**
- `crates/flui-foundation/src/key.rs:104-111`:
  ```rust
  #[inline]
  pub const fn from_str(s: &str) -> Self {
      let hash = const_fnv1a_hash(s.as_bytes());
      // Ensure non-zero (use 1 if hash is 0, which is extremely rare)
      let non_zero = if hash == 0 { 1 } else { hash };
      // SAFETY: We just ensured non_zero != 0
      Self(unsafe { NonZeroU64::new_unchecked(non_zero) })
  }
  ```
- If `const_fnv1a_hash(s.as_bytes()) == 0`, the function silently returns `Key(1)` — same value as `Key::from_str(s')` for any other string `s'` whose hash is `0`, AND same value as `Key::from_str("any-string-that-hashes-to-1")`.
- FNV-1a's offset basis `14_695_981_039_346_656_037` means the empty string `""` hashes to `14_695_981_039_346_656_037` (non-zero), so the empty-string-zero case is fine. But for arbitrary input there is no proof the collision is impossible — only "extremely rare" per the comment.
- The "extremely rare" claim isn't supportable — FNV-1a is a non-cryptographic hash with 2^64 output space. Probability of `== 0` is ~2^-64 per random string, but adversarial input or pathological strings can target it.

**Why it's a problem:**
- Silent collision violates the `Key` contract (compile-time constant uniqueness).
- The fallback `Key(1)` would also collide with `Key::from_str(s')` for any other string s' that hashed to 1.
- No way to detect at compile time — `const_assert!(const_fnv1a_hash(b"my-string") != 0)` would have to be written per call site, which is silly.

**Fix shape:** Make `from_str` return `Option<Key>` — caller must handle the zero-hash case. Or rotate the bits (XOR with a non-zero salt) before storing. Or return a different non-zero value for the zero case (e.g., `u64::MAX` instead of `1`):
```rust
const ZERO_FALLBACK: u64 = u64::MAX; // unambiguous fallback
pub const fn from_str(s: &str) -> Self {
    let hash = const_fnv1a_hash(s.as_bytes());
    let non_zero = if hash == 0 { ZERO_FALLBACK } else { hash };
    Self(unsafe { NonZeroU64::new_unchecked(non_zero) })
}
```
Still collides with anyone who hashes to `u64::MAX`, but the fallback value is more recognizable.

A cleaner fix: change the FNV implementation to **rotate-XOR** that mathematically cannot produce zero:
```rust
const fn const_fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(1_099_511_628_211);
        i += 1;
    }
    // Set top bit unconditionally — guaranteed non-zero.
    hash | (1 << 63)
}
```
This loses 1 bit of entropy (63-bit effective), but guarantees non-zero output. ~3 LOC change.

**Blast radius:** key.rs only. Compile-time hashes change values (compile-time API break for any caller relying on specific hash values — none exist in workspace per grep). Worst case: re-compile.

---

#### I-7 [P1 API-SURFACE | HIGH] `Key::new()` `assert!(id != u64::MAX, ...)` panics in release mode

**Evidence:**
- `crates/flui-foundation/src/key.rs:138-153`:
  ```rust
  #[inline]
  pub fn new() -> Self {
      static COUNTER: AtomicU64 = AtomicU64::new(1);
      let id = COUNTER.fetch_add(1, Ordering::Relaxed);

      // Always check for overflow, even in release mode
      // UB is never acceptable, even in "impossible" cases
      assert!(
          id != u64::MAX,
          "Key counter overflow! Created {} keys. \
           This should never happen in practice, but UB is never acceptable.",
          u64::MAX
      );

      // SAFETY: We just verified id != u64::MAX, and counter starts at 1
      Self(unsafe { NonZeroU64::new_unchecked(id) })
  }
  ```
- Uses `assert!` (release-mode panic) for the overflow case, not `debug_assert!`. The comment justifies: "UB is never acceptable, even in 'impossible' cases".
- Counter is `AtomicU64`. To exhaust, you need 2^64-1 ≈ 1.8e19 calls. At 1ns per call → ~584 years.
- BUT: the `fetch_add` returns the **pre-increment** value. So when COUNTER is `u64::MAX`, `fetch_add(1)` returns `u64::MAX` and wraps the counter to `0`. The check `id != u64::MAX` catches the wrap-around. **Correct, but…** the next call returns `0` — which is forbidden by NonZeroU64. The assert would panic on the call AFTER overflow, when `id == 0`. So the assert is checking the wrong value.

Actually wait — re-reading: when counter is at u64::MAX, fetch_add(1) returns u64::MAX. The assert catches it BEFORE the call returns 0. Subsequent call: counter has wrapped to 0, fetch_add(1) returns 0. NonZeroU64::new_unchecked(0) is UB. The assert doesn't catch it (the check is `id != u64::MAX`).

**Why it's a problem:**
- The assert has off-by-one logic. After counter wraps once, subsequent calls return 0, 1, 2, … — the 0 case is UB. Should also check `id != 0`.
- More fundamentally: `assert!` in release breaks the Constitution Principle 6 spirit (no production panics) — the runtime path can panic even in release. Use `Result<Key, KeyOverflow>` instead.

**Fix shape:**
```rust
#[inline]
pub fn try_new() -> Result<Self, KeyOverflow> {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    NonZeroU64::new(id).map(Self).ok_or(KeyOverflow)
}

#[inline]
pub fn new() -> Self {
    Self::try_new().expect("Key counter exhausted: 2^64 keys created")
}
```
This shifts the panic from `assert!` to `expect`, which is identical in failure mode but cleaner. The `try_new` path provides a Result for callers that want recovery.

**Blast radius:** key.rs only. `Key::new` keeps its panic semantic; new `try_new` is opt-in.

---

#### I-8 [P1 PARITY-DRIFT | HIGH] `ViewKey::is_global_key()` default-`false` allows GlobalKey impls to forget to override

**Evidence:**
- `crates/flui-foundation/src/key.rs:329-349`:
  ```rust
  fn is_global_key(&self) -> bool {
      false
  }
  ```
- Default impl returns `false`. The intention (from the long doc-comment) is that `GlobalKey<T>` in flui-view overrides to `true`, and the framework uses this for cheap-skip on non-global keys without `Any::downcast`.
- `crates/flui-view/src/key/global_key.rs:217:` — `fn is_global_key(&self) -> bool { true }` — correctly overridden.
- **But**: anyone writing a new key type that should be a GlobalKey-equivalent will forget to override `is_global_key`. The default-false means missing override = silently NOT registered in the global-key registry = silently broken reparenting.
- Cycle 1 framework-spine repair (PR #84) introduced the GlobalKey registry; this method is the cheap-skip optimization for it. Per `flui-view/src/tree/element_tree.rs:497` `if key.is_global_key() { … register … }` — the registration is gated on this method.

**Why it's a problem:**
- Subtle. The Rust idiom for marker-trait-ish behavior is: don't have a default — make it abstract.
- But `ViewKey` is object-safe (`fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;` requires it), so it can't be a marker trait with associated const.
- A `GlobalKey` extension trait `pub trait GlobalKey: ViewKey { fn ... }` + `ViewKey::is_global_key(&self) -> bool { false }` *with* a debug-assert that "if any field is named global_key_id, return true" — no, that's the wrong tool too.

**Fix shape:** Two options:
- **(a)** Make `is_global_key()` not a default — make it abstract, force every implementer to think about it:
  ```rust
  pub trait ViewKey: Send + Sync + 'static {
      // ...
      fn is_global_key(&self) -> bool;  // NO default
  }
  ```
  Forces flui-view's `ValueKey`/`UniqueKey`/`ObjectKey` impls to explicitly return `false`. Mild API churn — `flui-foundation::ValueKey` impl needs the line.
- **(b)** Add a `#[must_use]`-style lint via a procedural macro / clippy::missing_trait_methods.

Option (a) is the simpler Rust idiom — sealed by abstract method.

**Blast radius:** flui-view's `GlobalKey` impl (already returns true). flui-foundation's `ValueKey` and `UniqueKey` impls need to add `fn is_global_key(&self) -> bool { false }` (one line each). Cycle 2's `flui-view/src/key/object_key.rs` likely similar.

---

#### I-9 [P2 API-SURFACE | MEDIUM] `Id<T>::from_raw` / `Id<T>::zip_unchecked` / `Id<T>::new_unchecked` unsafe escape hatches with zero in-workspace consumers

**Evidence:**
- `crates/flui-foundation/src/id.rs:215-294`: three `unsafe` constructors that bypass non-zero check.
- Grep across workspace: `from_raw` callers are tests-only (line 740) plus the `serde_impl` deserialize path (line 608). `zip_unchecked` / `new_unchecked` — zero callers in production code.
- The `Id::zip_unchecked` is also the safety-justifying call for the workspace's only other unsafe in foundation: `Key::new_unchecked` at key.rs:153, which is consumer-side.
- The unsafe is well-documented: "SAFETY: Caller guarantees index is non-zero" — but there's no caller to justify the escape hatch.

**Why it's a problem:**
- Unsafe surface area without consumer justification is technical debt. Constitution Principle 3 mandates `unsafe` only in `flui-platform`, `flui-painting`, `flui-engine` (this is flui-foundation, so the unsafe should be minimized to the documented `NonZeroUsize` invariant only).
- Encourages future drift — someone could discover the unsafe constructor and use it speculatively.

**Fix shape:** Mark `pub(crate)` instead of `pub`. The serde deserialize path needs it internally; no external consumer needs it.

```rust
// id.rs line 215:
pub(crate) const unsafe fn from_raw(raw: RawId) -> Self { ... }
// line 248:
pub(crate) const unsafe fn zip_unchecked(index: Index) -> Self { ... }
// line 291:
pub(crate) const unsafe fn new_unchecked(index: Index) -> Self { ... }
```

**Blast radius:** Trivial. Visibility downgrade. No external consumer breaks.

---

#### I-10 [P2 ZOMBIE | MEDIUM] `RawId` + `Index` type aliases — exposed but zero external consumers

**Evidence:**
- `crates/flui-foundation/src/id.rs:71` `pub type Index = usize;`
- `crates/flui-foundation/src/id.rs:81-122` `pub struct RawId(NonZeroUsize);` + impls.
- Grep `flui_foundation::Index` / `flui_foundation::RawId` external consumers: 0.
- flui-scheduler's `IdGenerator<M: Marker>` at id.rs:73 imports `flui_foundation::{Id, Identifier, Index, Marker, RawId, ...}` — but inspection shows only `Id<M>` and `Marker` are referenced in the impl. `Index` and `RawId` are imported-and-discarded.

**Why it's a problem:**
- `Index` is just a `usize` alias adding zero type safety — meaningless niche.
- `RawId` is a useful internal — `NonZeroUsize` wrapper with `zip`/`unzip` — but no external code needs to construct or destructure it.
- Public re-exports waste surface.

**Fix shape:** Make `RawId` and `Index` `pub(crate)`. Update flui-scheduler import to remove unused. Public surface stays clean: `Id<T>` is the public type, `Identifier` trait is the public bound.

**Blast radius:** id.rs + flui-scheduler/src/id.rs import. Trivial.

---

#### I-11 [P2 API-SURFACE | MEDIUM] `DiagnosticsTreeStyle::ErrorProperty` + `::Shallow` variants — no external consumers

**Evidence:**
- `crates/flui-foundation/src/debug.rs:144-157`: 5 variants total — `Sparse`, `Shallow`, `Dense`, `SingleLine`, `ErrorProperty`.
- Grep external usage of variants (excluding examples):
  - `Sparse`: default, used internally
  - `Dense`: used in examples
  - `SingleLine`: used in flui-foundation's own `Display` impls
  - `Shallow`: zero external consumers
  - `ErrorProperty`: zero external consumers
- The enum is `#[derive(... )]`, exhaustive (no `#[non_exhaustive]`).

**Why it's a problem:**
- Two of 5 variants exist for completeness with Flutter (`ErrorProperty` corresponds to Flutter's diagnostic-error rendering; `Shallow` is Flutter's shorter object-property rendering) but neither is consumed.
- Enum is not `#[non_exhaustive]`, so any add/remove is a public API break.

**Fix shape:**
- Mark `#[non_exhaustive]` on `DiagnosticsTreeStyle` and `DiagnosticLevel` enums — protects against future additions.
- Keep all variants for Flutter-parity reasons but document they are aspirational.

**Blast radius:** debug.rs — `#[non_exhaustive]` is the only change. May break exhaustive-match callers; none exist in workspace.

---

#### I-12 [P2 PARITY-DRIFT | MEDIUM] `FoundationError`/`Diagnosticable`/`debug.rs` mention "Flutter parity" but don't trace to specific Flutter source

**Evidence:**
- `crates/flui-foundation/src/debug.rs:9-11`: "Similar to Flutter's `DiagnosticLevel`". No line ref.
- `crates/flui-foundation/src/notifier.rs:113`: "Similar to Flutter's `ChangeNotifier`". Cycle 1 added explicit `change_notifier.dart:181, :376` refs (PR #84 work). The rest of the crate uses casual "similar to" without traceable refs.
- `crates/flui-foundation/src/key.rs:319-349` — the `is_global_key()` long doc has detailed Flutter refs (`framework.dart:3148`). Good shape. Other methods in key.rs have generic "Flutter's Key" mentions without line refs.

**Why it's a problem:**
- Cycle 2 standardized the practice of citing exact Flutter file:line refs (`semantics.dart:6790`, `layer.dart:1185-1216`) in audit and code comments. The audit-execute cycle's correctness depends on these refs being load-bearing.
- Without refs, future maintainers can't validate parity.

**Fix shape:** Sweep through flui-foundation doc-comments. Cycle 2's PR #100 set the standard:
  - "Mirrors Flutter `change_notifier.dart:181, :376`" not "Similar to Flutter's ChangeNotifier".
  - Apply to `key.rs` (Key/ValueKey/UniqueKey/ViewKey), `binding.rs` (BindingBase), `debug.rs` (DiagnosticLevel/DiagnosticsTreeStyle/DiagnosticsNode/Diagnosticable), `notifier.rs` (already done for ChangeNotifier; add for ValueNotifier).
  - Sometimes "no Flutter equivalent" is the right comment (e.g., `KeyRef` is Rust-specific).

~50-LOC doc churn. No functional change.

**Blast radius:** Doc-only changes across 4-5 files in flui-foundation.

---

#### I-13 [P2 DEAD-CODE | MEDIUM] `consts.rs::approx_equal`, `is_near_zero` `const fn` with `EPSILON` — zero workspace consumers

**Evidence:**
- `crates/flui-foundation/src/consts.rs:96-128`: 4 `const fn` helpers (`approx_equal`, `approx_equal_f32`, `is_near_zero`, `is_near_zero_f32`).
- Grep workspace excluding flui-foundation tests/examples: 0 hits for `approx_equal` / `is_near_zero` / `EPSILON`.
- `EPSILON_F32` is also unused (`EPSILON` is the only usage internally — in approx_equal definition).
- flui-types `Pixels`/`f32` math has its own near-zero logic (`flui_types/src/numeric.rs::approx_eq` etc.).

**Why it's a problem:**
- Speculative API surface. The constants/functions look like they "should" be used but no consumer depends on them.

**Fix shape:** Two options:
- **(a)** Delete — flui-types already provides its own.
- **(b)** Mark all `#[cfg(any(test, feature = "math-helpers"))]` to gate on opt-in.

(a) is cleaner. Move EPSILON / approx_equal etc. to flui-types where they belong, behind a feature.

**Blast radius:** consts.rs only. No production consumer breaks.

---

#### I-14 [P2 DEAD-CODE | MEDIUM] `assert.rs::report_error!` / `report_warning!` macros — zero workspace consumers

**Evidence:**
- `crates/flui-foundation/src/assert.rs:149-166`: two macros.
- Grep `report_error!\|report_warning!` workspace: 0 hits outside flui-foundation tests/examples.
- Both wrap `tracing::error!` / `tracing::warn!` with a `cfg!(debug_assertions)` gate — release-mode no-op.

**Why it's a problem:**
- `tracing::error!` / `tracing::warn!` already exist. The wrappers add only the debug-only-gate, which is rarely the right semantic.
- API surface bloat with no consumer.

**Fix shape:** Delete the macros. Callers needing debug-only diagnostics can write `if cfg!(debug_assertions) { tracing::error!(...) }` explicitly. ~25 LOC reduction.

**Blast radius:** assert.rs only. No external consumer.

---

#### I-15 [P3 HOT-PATH | LOW] `ChangeNotifier::has_listeners` / `is_empty` / `len` lock the mutex for trivial reads

**Evidence:**
- `crates/flui-foundation/src/notifier.rs:259-277` (3 methods).
- Each locks `self.listeners.lock()` to read a count. If the notifier is queried frequently (e.g., from a debug overlay or stats path), per-call lock contention.

**Why it's a problem:**
- For trivial reads (existence of any listener, count), a separate `AtomicUsize` counter could avoid the lock entirely.
- Per *Rust Atomics and Locks* §3 — separate atomic for cheap-read semantic, mutex for compound operations.

**Fix shape:**
```rust
pub struct ChangeNotifier {
    listeners: Arc<Mutex<HashMap<ListenerId, ListenerCallback>>>,
    listener_count: Arc<AtomicUsize>, // new: lock-free read
    next_id: Arc<AtomicUsize>,
    is_disposed: Arc<AtomicBool>,
}

// add_listener: fetch_add(1, Release) after insert
// remove_listener: fetch_sub(1, Release) after remove (only if removed)
// remove_all_listeners: load + store(0, Release) after clear
// has_listeners / is_empty / len: read AtomicUsize::load(Acquire)
```

Maintains consistency by sequencing the count update after the map mutation (Acquire/Release pair).

**Blast radius:** notifier.rs only. Backward-compatible — same external API, internal optimization.

---

#### I-16 [P3 API-SURFACE | LOW] `ListenerCallback = Arc<dyn Fn() + Send + Sync>` — could be `Arc<dyn Fn() + Send + Sync + 'static>`

**Evidence:**
- `crates/flui-foundation/src/notifier.rs:43`: `pub type ListenerCallback = Arc<dyn Fn() + Send + Sync>;`
- Without `+ 'static`, the type is `dyn Fn() + Send + Sync + '?` — `'static` is inferred for `dyn Trait`, but explicit is clearer.

**Fix shape:** Add `+ 'static`:
```rust
pub type ListenerCallback = Arc<dyn Fn() + Send + Sync + 'static>;
```
Same for `VoidCallback`, `ValueChanged<T>`, etc. in `callbacks.rs`. Doc-comment lift.

**Blast radius:** callbacks.rs + notifier.rs. No functional change (inferred lifetime is `'static`); doc clarity.

---

#### I-17 [P3 DEAD-CODE | LOW] `ValueNotifier::take` / `replace` / `value_mut` — single-test-only callers

**Evidence:**
- `crates/flui-foundation/src/notifier.rs:362-410`: 4 mutation methods on ValueNotifier.
- Grep external consumers: 0 (only own tests + animation crate uses set_value).
- The methods are well-shaped, but `take()` requires `T: Default`, `replace()` requires `T: PartialEq`, `value_mut()` returns `&mut T` and the caller must remember to call `notify()`.

**Why it's a problem:**
- API surface for a notifier that's used in animation crate primarily as a value holder, not a mutation target. flui-animation uses `set_value` (the safe path); the others sit unused.

**Fix shape:** Keep — they're not harmful and might attract consumers. But flag with `// TODO: verify consumers materialize` comment. Or mark `#[cfg(feature = "extra-value-ops")]`.

(Lowest priority — judgment call.)

**Blast radius:** Doc-only or feature-gate.

---

#### I-18 [P3 API-SURFACE | LOW] `Marker` trait bound `Debug` requires a useless impl on zero-sized marker enums

**Evidence:**
- `crates/flui-foundation/src/id.rs:173`: `pub trait Marker: 'static + WasmNotSendSync + Debug {}`
- Marker types in the `markers::*` module (lines 484-491) are `pub enum Marker { }` (uninhabited) with `#[derive(Debug)]`.
- Debug for an empty enum is a vestigial impl — formatting an uninhabited type is unreachable code.

**Why it's a problem:**
- Aesthetic. The `Debug` bound exists so that `Id<T>` debug format can print the marker name (line 311: `let marker_name = type_name.rsplit("::").next()...`) — but `type_name::<T>()` works without the Debug bound.
- One unnecessary supertrait bound on Marker.

**Fix shape:** Remove the `+ Debug` requirement from Marker trait. The `Id::fmt` uses `type_name::<T>()` which doesn't need `T: Debug`. Update macro to drop `#[derive(Debug)]` on marker types.

**Blast radius:** id.rs only. Minor cleanup.

---

#### I-19 [P3 PARITY-DRIFT | LOW] `ParseDiagnosticLevelError` and `ParseDiagnosticsTreeStyleError` accept already-allocated `String` — could be `Box<str>`

**Evidence:**
- `crates/flui-foundation/src/debug.rs:118-127, 217-226`: two error structs with `String` payload.
- The `String` holds the invalid input — once stored, it's immutable. `Box<str>` would save the capacity field (~8 bytes per error instance).

**Fix shape:** `pub struct ParseDiagnosticLevelError(Box<str>);` — minor optimization. ~5 LOC change.

**Blast radius:** debug.rs only. Error struct is unused externally per I-11 finding.

---

#### I-20 [P3 DEAD-CODE | LOW] `ValueNotifier::new` `notifier_count` invariant — `into_value` consumes notifier dropping listeners silently

**Evidence:**
- `crates/flui-foundation/src/notifier.rs:341-346`:
  ```rust
  pub fn into_value(self) -> T {
      self.value
  }
  ```
- Consuming the ValueNotifier silently drops `self.notifier: ChangeNotifier`, which contains an `Arc<Mutex<HashMap<ListenerId, ListenerCallback>>>`. The HashMap's listener callbacks may still be in flight elsewhere (other clones of the Arc), and the Drop is a no-op.

**Why it's a problem:**
- Subtle: if a listener has a side-effect that depends on the notifier being alive, `into_value` deactivates it silently.
- Per the dispose protocol (PR #84 template), the pattern is to call `notifier.dispose()` before drop — but `into_value` doesn't do that.

**Fix shape:**
```rust
pub fn into_value(self) -> T {
    self.notifier.dispose(); // mirror PR #84 explicit dispose
    self.value
}
```
Same for `take` if it transitions ownership.

**Blast radius:** notifier.rs only.

---

#### I-21 [P3 API-SURFACE | LOW] `KeyRef::new` `pub const fn` accepts already-existing Key — `From<Key>` already provides this

**Evidence:**
- `crates/flui-foundation/src/key.rs:243-249`: `pub const fn new(key: Key) -> Self { Self(key) }` 
- Line 265-269: `impl From<Key> for KeyRef { fn from(k: Key) -> Self { Self(k) } }`
- Two equivalent constructors. The `new` is `const fn` (more flexible for const contexts), the `From` is the idiomatic conversion.

**Fix shape:** Keep `From` impl; deprecate `KeyRef::new` in favor of `Key.into()`. ~5 LOC change.

**Blast radius:** key.rs only. No external consumers (per grep).

---

#### I-22 [P3 PARITY-DRIFT | LOW] `wasm.rs::WasmNotSend` exposed but unused

**Evidence:**
- `crates/flui-foundation/src/wasm.rs:55-67`: `WasmNotSend` trait.
- Grep external consumers: 0 (only `WasmNotSendSync` is used — as a supertrait bound on `Marker`).
- `WasmNotSend` is `Send`-only without `Sync`, parallel to `WasmNotSendSync`. Speculation for a future single-threaded-on-wasm pattern.

**Fix shape:** Delete `WasmNotSend`. Keep `WasmNotSendSync` (load-bearing).

**Blast radius:** wasm.rs only.

---

### Tree findings (T-1 .. T-25)

---

#### T-1 [P0 PARITY-DRIFT | CRITICAL] `TreeWrite::remove` codifies non-cascade footgun — same as cycle 2 LayerTree/SemanticsTree pre-PR#100 state

**Evidence:**
- `crates/flui-tree/src/traits/write.rs:64-81`:
  ```rust
  /// Removes a node from the tree.
  ///
  /// This removes only the specified node. Children handling
  /// depends on the implementation (may be orphaned or removed).
  ///
  /// # Arguments
  /// * `id` - The unique identifier of the node to remove
  ///
  /// # Returns
  /// `Some(node)` if the node existed and was removed, `None` otherwise.
  ///
  /// # Note
  /// Implementations should update parent's children list when
  /// removing a node.
  fn remove(&mut self, id: I) -> Option<Self::Node>;
  ```
- The phrasing "Children handling depends on the implementation (may be orphaned or removed)" is the codified footgun.
- The only in-workspace implementer, `RenderTree::remove` at `crates/flui-rendering/src/storage/tree.rs:677-691`:
  ```rust
  fn remove(&mut self, id: RenderId) -> Option<Self::Node> {
      // Update root if removing root
      if self.root == Some(id) { self.root = None; }
      // Get parent and remove from parent's children
      if let Some(parent_id) = self.get(id).and_then(|n| n.parent())
          && let Some(parent) = self.nodes.get_mut(parent_id.get() - 1)
      {
          parent.remove_child(id);
      }
      self.nodes.try_remove(id.get() - 1)
  }
  ```
  Does NOT cascade — only removes the specified node and unlinks from parent. Descendants become orphans in the slab.
- Cycle 2 PR #100 U12 (LayerTree::remove) and U13 (SemanticsTree::remove) explicitly fixed this same footgun shape with cascade-by-default + `remove_shallow` opt-out. That work was done at the impl level for those two trees but never hoisted to TreeWrite.
- `TreeWrite::remove_subtree` (lines 97-112) provides cascade as opt-in via a separate method. Hierarchy wrong — non-cascade is the default footgun, cascade is opt-in.

**Why it's a problem:**
- Same orphaning-slab corruption issue cycle 2 fixed at the impl level. The TreeWrite contract codifies it.
- RenderTree (the only consumer) currently inherits the footgun.
- LayerTree and SemanticsTree don't even implement TreeWrite (T-2 finding) — their cascade-by-default `remove` is parallel to the trait. The trait contract should be the inverse: cascade by default, opt-out via `remove_shallow`.

**Flutter reference:** `framework.dart::Element::deactivateChild` (the equivalent for Element tree) recursively deactivates the subtree. `rendering/layer.dart::ContainerLayer::remove` + `LayerHandle._unref` cascade (cited in cycle 2 audit).

**Fix shape:**
```rust
pub trait TreeWrite<I: Identifier>: TreeRead<I> {
    fn get_mut(&mut self, id: I) -> Option<&mut Self::Node>;
    fn insert(&mut self, node: Self::Node) -> I;

    /// Removes a node and ALL its descendants.
    ///
    /// Mirrors cycle 2's PR #100 `LayerTree::remove` / `SemanticsTree::remove`
    /// cascade-by-default contract. Walk is post-order — children dispose
    /// before parent.
    ///
    /// For non-cascading removal (reparenting workflow), use
    /// [`remove_shallow`](TreeWrite::remove_shallow).
    fn remove(&mut self, id: I) -> Option<Self::Node>
    where
        Self: super::TreeNav<I> + Sized,
    {
        // Default impl: post-order cascade.
        let children: Vec<I> = self.children(id).collect();
        for child_id in children {
            let _ = self.remove(child_id);
        }
        self.remove_shallow(id)
    }

    /// Removes only the specified node WITHOUT cascading to descendants.
    ///
    /// Children become orphaned in the storage. Use only for reparenting
    /// workflows that immediately re-attach the removed subtree elsewhere.
    fn remove_shallow(&mut self, id: I) -> Option<Self::Node>;

    fn clear(&mut self) { /* ... */ }
    fn reserve(&mut self, additional: usize) { let _ = additional; }
}
```

**Blast radius:** Medium.
- `RenderTree::remove` at `flui-rendering/src/storage/tree.rs:677-691` becomes `RenderTree::remove_shallow` (rename); the trait default impl provides cascade. ~30 LOC change in flui-rendering.
- Once trait shape is fixed, LayerTree and SemanticsTree can implement TreeWrite — their cycle-2 cascading `remove` is the trait's `remove`, their `remove_shallow` is the trait's `remove_shallow`. ~100 LOC of parallel API can be removed (T-3 finding).

---

#### T-2 [P0 PARITY-DRIFT | CRITICAL] `LayerTree` + `SemanticsTree` do not implement `TreeWrite<I>` — parallel `add_child`/`remove`/`remove_shallow` outside the unified trait

**Evidence:**
- `crates/flui-layer/src/tree/tree_traits.rs`: implements `TreeRead<LayerId>` + `TreeNav<LayerId>` only.
- `crates/flui-semantics/src/tree.rs:424-457`: same — `TreeRead<SemanticsId>` + `TreeNav<SemanticsId>` only.
- Both crates have parallel mutation APIs:
  - `LayerTree::add_child(&mut self, parent_id: LayerId, child_id: LayerId)` at layer_tree.rs:559 (66 LOC including doc + cycle-detection)
  - `LayerTree::remove(&mut self, id: LayerId) -> Option<LayerNode>` at layer_tree.rs:477 (35 LOC including cascade walk)
  - `LayerTree::remove_shallow` at layer_tree.rs:523 (5 LOC)
  - `LayerTree::remove_child(&mut self, parent_id, child_id)` at layer_tree.rs:646 (~30 LOC)
  - `LayerTree::insert(&mut self, layer: Layer) -> LayerId` at layer_tree.rs:417 (5 LOC)
  - Plus `LayerTree::get_mut`, `clear`, etc.
- Same shape repeated in `SemanticsTree` (`flui-semantics/src/tree.rs`):
  - `add_child` at line 240
  - `remove` at line 170 (cascade-by-default, mirror of LayerTree)
  - `remove_shallow` at line 210
  - `remove_child` at line 324
- These two parallel APIs replicate ~250 LOC of mutation logic that should live in TreeWrite/TreeWriteNav.

**Why it's a problem:**
- Memory [[flui-tree-unified-interface-intent]] is explicit that flui-tree should be the canonical mutation home. Cycle 2 fixed the cascade semantics but didn't consolidate to the trait — instead duplicating the fix in two crates.
- Three parallel implementations of the same pattern (LayerTree, SemanticsTree, eventually RenderTree) is the **"no quick wins"** pattern memory binding forbids.
- New trees (when flui-element materializes its own ElementTree, when ViewTree gets a write API) will continue the pattern.

**Fix shape:** Once T-1 is fixed (trait redefines `remove` as cascade-by-default), implement `TreeWrite<LayerId> for LayerTree` and `TreeWrite<SemanticsId> for SemanticsTree`:
```rust
// crates/flui-layer/src/tree/tree_traits.rs (extension)
impl TreeWrite<LayerId> for LayerTree {
    fn get_mut(&mut self, id: LayerId) -> Option<&mut Self::Node> {
        LayerTree::get_mut(self, id)
    }
    fn insert(&mut self, layer: LayerNode) -> LayerId {
        // Note: trait `insert` takes a `Self::Node` (LayerNode), not bare Layer.
        // Map through the LayerTree's existing path.
        ...
    }
    fn remove_shallow(&mut self, id: LayerId) -> Option<LayerNode> {
        LayerTree::remove_shallow(self, id)
    }
}
impl TreeWriteNav<LayerId> for LayerTree { /* ... */ }
```
The trait's default `remove` (with `where Self: TreeNav<I>`) provides the cascade automatically — LayerTree's current `remove` becomes redundant (deletable).

**Blast radius:** Medium. ~30 LOC trait impls in flui-layer + flui-semantics. Removes ~200 LOC of duplicated logic across both crates.

---

#### T-3 [P0 ZOMBIE | CRITICAL] `state.rs` — `Mountable`/`Unmountable`/`Mounted`/`Unmounted` typestate machinery: 616 LOC with zero external consumers

**Evidence:**
- `crates/flui-tree/src/state.rs` — entire 616-LOC file.
- Grep `Mountable\|Unmountable\|Mounted\|Unmounted\|NodeState\|MountableExt` external consumers:
  - flui-view's `Element` FSM at `crates/flui-view/src/element/lifecycle.rs:14` has its own state machine (Initial/Active/Inactive/Defunct per Flutter `Element._lifecycleState`), NOT using this typestate.
  - Zero other external consumers.
- The typestate is two-state (Mounted/Unmounted) — Flutter's actual Element FSM is four-state. Even the conceptual basis is misaligned.

**Why it's a problem:**
- Same shape as cycle 1's deleted `typestate.rs` (PR #93) — 232-LOC typestate machinery with 0 consumers.
- 616 LOC of public surface + tests for an abstraction nobody uses.
- The trait machinery (`Mountable::mount(self, parent, parent_depth) -> Mounted`) doesn't compose well with the Slab-storage pattern every tree actually uses (the Slab holds owned nodes, not typestate-transitioning ones — typestate would force ownership transfer through the Slab boundary).

**Fix shape:** Delete entire module. Same atomic-commit shape as cycle 1's PR #93. Per memory [[flui-tree-unified-interface-intent]], if a future consumer needs typestate, it can be reintroduced — but the current 616-LOC scaffolding has no migration target.

**Blast radius:** Trivial. lib.rs re-exports + prelude. No external consumers.

---

#### T-4 [P0 ZOMBIE | CRITICAL] `visitor/mod.rs::StatefulVisitor` typestate machinery + `visitor/composition.rs` + `visitor/fallible.rs` — 2,550 LOC, zero external consumers

**Evidence:**
- `crates/flui-tree/src/visitor/mod.rs` (1,264 LOC), `composition.rs` (648 LOC), `fallible.rs` (638 LOC).
- Grep `TreeVisitor\|TreeVisitorMut\|CollectVisitor\|CountVisitor\|FindVisitor\|MaxDepthVisitor\|ForEachVisitor\|StatefulVisitor\|TypedVisitor\|ComposedVisitor\|...` external consumers: 0.
- `StatefulVisitor<State, Data>` + `states::{Initial, Started, Finished}` (lines 659-840 of `visitor/mod.rs`) — ~180 LOC of typestate machinery identical in shape to cycle 1's deleted typestate.rs (PR #93).
- `TypedVisitor` GAT machinery (lines 182-213, +`visit_depth_first_typed` impl) — 50 LOC of GAT speculation that `CollectVisitor` already covers.
- 6 built-in visitors (`CollectVisitor`, `CountVisitor`, `FindVisitor`, `MaxDepthVisitor`, `ForEachVisitor`, plus `StatefulVisitor`) — none used externally.
- Composition (`ComposedVisitor`, `TripleComposedVisitor`, `MappedVisitor`, `ConditionalVisitor`, `DynVisitor`, `VisitorVec`, `VisitorExt`) — 648 LOC of combinator surface, 0 consumers.
- Fallible (`FallibleVisitor`, `DepthLimitVisitor`, `TryCollectVisitor`, `TryForEachVisitor`, `VisitorError`) — 638 LOC, 0 consumers.

**Why it's a problem:**
- 2,550 LOC of public + tested API surface for a system nobody uses.
- The visitor pattern is the **classical OO design pattern** — Rust closures cover most of the use cases more ergonomically. `tree.descendants(root).filter(|(id, _)| predicate(id)).collect::<Vec<_>>()` replaces `CollectVisitor` + `visit_depth_first`.
- `StatefulVisitor`'s typestate (Initial → Started → Finished) is unused noise.

**Fix shape:**
- **Phase 1 — Delete StatefulVisitor + TypedVisitor** (~250 LOC). Same shape as cycle 1's PR #93.
- **Phase 2 — Move remaining visitor surface behind `#[cfg(feature = "visitors")]`** — defaulted off. Per memory [[flui-tree-unified-interface-intent]] keep for future migration target.
- **Phase 3 — Add doc comment marking the visitor surface as "speculative; awaiting devtools consumer"** so future maintainers don't develop more.

**Blast radius:** Phase 1 trivial. Phase 2 needs `Cargo.toml` feature flag + cfg-gating in `lib.rs` re-exports + prelude.

---

#### T-5 [P0 ZOMBIE | CRITICAL] `diff.rs::TreeDiff` + 5 supporting types — 1,234 LOC, zero external consumers

**Evidence:**
- `crates/flui-tree/src/diff.rs` — entire 1,234-LOC file: `TreeDiff<I>`, `DiffOp<I>`, `ChildDiff<I>`, `ChildOp<I>`, `DiffStats`, internal `TreeDiffer<'a, I, T>`.
- Grep `TreeDiff\|DiffOp\|ChildDiff\|ChildOp\|DiffStats` external consumers: 0.
- The intended consumer is reconciliation (View → Element tree diff), but flui-view's reconciliation (in `crates/flui-view/src/element/` + `tree/element_tree.rs`) uses its own logic — key-based child reconciliation, NOT this generic diff.

**Why it's a problem:**
- 1,234 LOC of surface for a system that has a real consumer use case (devtools / hot-reload tree diff) but no current consumer.
- Per memory [[flui-tree-unified-interface-intent]], keep — but flag clearly as awaiting consumer.

**Fix shape:** Move behind `#[cfg(feature = "tree-diff")]` defaulted off. Same shape as T-4 phase 2.

**Blast radius:** lib.rs re-exports + prelude. No production consumer.

---

#### T-6 [P0 ZOMBIE | CRITICAL] `iter/cursor.rs::TreeCursor` + `iter/path.rs::{TreePath, IndexPath, TreeNavPathExt}` + `iter/{breadth,depth}_first.rs` — 3,812 LOC, zero external consumers

**Evidence:**
- `iter/cursor.rs` 1,057 LOC. Grep `TreeCursor` external: 0.
- `iter/path.rs` 1,150 LOC. Grep `TreePath\|IndexPath\|TreeNavPathExt` external: 0.
- `iter/breadth_first.rs` 317 LOC. Grep `BreadthFirstIter` external: 0.
- `iter/depth_first.rs` 338 LOC. Grep `DepthFirstIter` external: 0.
- The `iter/siblings.rs::Siblings` directional iterator (554 LOC) — `Siblings` struct unused externally; `TreeNav::siblings()` method is used as `flat_map`.

**Why it's a problem:**
- 3,812 LOC of iterator surface that's not consumed.
- Cursor / Path / IndexPath were specced for devtools (memory [[flui-tree-unified-interface-intent]] — devtools is the consumer). BreadthFirst / DepthFirst are alternative ordering primitives that `TreeNav::descendants` already covers internally.

**Fix shape:** Same as T-5 — feature-gate `#[cfg(feature = "tree-iter-advanced")]`. Keep `Ancestors`, `Descendants`, `DescendantsWithDepth`, `AncestorsWithDepth` (used by all three TreeNav impls — flui-layer, flui-semantics, flui-rendering).

**Blast radius:** lib.rs re-exports + prelude. No production consumer.

---

#### T-7 [P0 ZOMBIE | CRITICAL] `arity/{storage,arity_storage,accessors}.rs` — 3,051 LOC, zero external consumers

**Evidence:**
- `arity/storage.rs` 794 LOC (`ChildrenStorage` trait + `ChildrenStorageExt`).
- `arity/arity_storage.rs` 858 LOC (`ArityStorage<T, A>` enum).
- `arity/accessors.rs` 1,399 LOC (`ChildrenAccess` trait + 7 accessors: `NoChildren`, `OptionalChild`, `FixedChildren`, `SliceChildren`, `BoundedChildren`, `SmartChildren`, `TypedChildren`, `NeverAccessor`, `Copied`).
- Grep `ArityStorage\|ChildrenStorage\|ChildrenAccess\|ChildrenStorageExt\|FixedChildren\|SliceChildren\|NeverAccessor\|...` external consumers: 0.
- The arity *markers* (`Leaf`, `Single`, `Optional`, `Variable`, zero-sized types from `arity/types.rs`) ARE consumed — flui-rendering's render-objects use them as type-level binding tags (`RenderBox<Single>`, `RenderBox<Leaf>` etc.). But the *storage* machinery (the runtime data structures) is not.

**Why it's a problem:**
- 3,051 LOC of public + tested surface, 0 consumers.
- flui-rendering chose a different pattern: per-arity-type field on the render object struct (`pub struct RenderPadding { child: BoxChild<Single> }`). The `ArityStorage` enum-based approach was speccd but the actual implementation diverged.
- The `ChildrenStorage` trait is ambassador-delegatable (`#[delegatable_trait]`), which would be useful for the flui-rendering shape — but flui-rendering doesn't delegate through this trait.

**Fix shape:** Two-stage:
- **Phase 1 — Move storage machinery behind `#[cfg(feature = "arity-storage")]`** defaulted off. ~3,000 LOC out of release builds.
- **Phase 2 — Add doc note in `arity/mod.rs`** clarifying that the markers are the load-bearing public surface; the storage machinery is opt-in for consumers who want it.

**Blast radius:** Substantial cfg gating in `arity/mod.rs` re-exports + `lib.rs` re-exports + prelude. No consumer breaks.

---

#### T-8 [P0 ZOMBIE | CRITICAL] `traits/node.rs::Node` + `NodeExt` + `NodeTypeInfo` — 305 LOC, zero external impls

**Evidence:**
- `crates/flui-tree/src/traits/node.rs` — 305 LOC.
- Grep `impl Node for` / `implements Node` workspace-wide: 0 external impls.
- The trait is `pub trait Node: Sized + Send + Sync + 'static { type Id: Identifier; }` — minimal but unused.
- `NodeExt` provides `type_name()` and `id_type_name()` via `std::any::type_name::<Self>()` — both methods callable on any type without the trait.
- `NodeTypeInfo` packages debug info — useful for diagnostics but no consumer.

**Why it's a problem:**
- 305 LOC of trait + extensions + tests for a trait with no impls outside the file's own tests.
- `Node` provides only `type Id: Identifier` — that's just a type alias. flui-view's `Element`, flui-layer's `LayerNode`, etc. don't implement `Node` — they just have an `Id` type without claiming the trait.

**Fix shape:**
- **Option A — Delete** — the `type Id: Identifier` constraint isn't doing useful work.
- **Option B — Keep but flag as marker-trait scaffolding** awaiting future use.
- Per memory [[flui-tree-unified-interface-intent]] option B is safer.

**Blast radius:** If delete: lib.rs re-exports + prelude. No external consumers.

---

#### T-9 [P1 HOT-PATH | HIGH] `TreeNav::slot` default impl `Vec<I>` allocation per call

**Evidence:**
- `crates/flui-tree/src/traits/nav.rs:149-170`:
  ```rust
  fn slot(&self, id: I) -> Option<Slot<I>> {
      let parent = self.parent(id)?;
      let children: Vec<I> = self.children(parent).collect();  // ← per-call alloc
      let index = children.iter().position(|&c| c == id)?;
      let depth = Depth::new(self.depth(id));

      let previous_sibling = if index > 0 { Some(children[index - 1]) } else { None };
      let next_sibling = children.get(index + 1).copied();

      Some(Slot::with_siblings(parent, index, depth, previous_sibling, next_sibling))
  }
  ```
- Per call: 1 `Vec<I>` allocation (size = parent.children().count()), 1 linear scan for position, plus `depth()` which itself allocates via `ancestors().count()`.
- For a 5-tree with 100s of slot queries per frame, this is allocator pressure.

**Why it's a problem:**
- Default impl is the "free" fallback every TreeNav impl gets. Per *Rust Performance Book*, default impls are silent perf traps when the actual hot path goes through them.
- The Slab-storage trees can do this in O(1) — they have the children Vec directly accessible, no need to re-collect.

**Fix shape:** Either:
- **(a)** Keep default impl, document the perf caveat — Slab consumers override.
- **(b)** Move slot to `TreeNavExt` (extension trait) so the default doesn't masquerade as the canonical answer:
  ```rust
  pub trait TreeNavExt<I: Identifier>: TreeNav<I> {
      fn slot(&self, id: I) -> Option<Slot<I>> {
          let parent = self.parent(id)?;
          let mut iter = self.children(parent);
          let mut prev = None;
          let mut index = 0;
          for child in iter.by_ref() {
              if child == id {
                  let next_sibling = iter.next();
                  return Some(Slot::with_siblings(parent, index, ..., prev, next_sibling));
              }
              prev = Some(child);
              index += 1;
          }
          None
      }
  }
  ```
  Uses streaming iteration — no Vec alloc. Iterates parent's children at most twice (once to find id, once to peek next sibling).

Option (b) is the clean fix.

**Blast radius:** nav.rs only. Backward-compatible — same semantic, better implementation.

---

#### T-10 [P1 PARITY-DRIFT | HIGH] Four-way depth constant drift: `MAX_TREE_DEPTH=256`, `TreeNav::MAX_DEPTH=32`, `TreeVisitor::MAX_STACK_DEPTH=64`, `TreeVisitorMut::STACK_SIZE=48`

**Evidence:**
- `crates/flui-tree/src/depth.rs:68` `pub const MAX_TREE_DEPTH: usize = 256;` — global cap.
- `crates/flui-tree/src/traits/nav.rs:42` `const MAX_DEPTH: usize = 32;` — TreeNav stack-allocation hint.
- `crates/flui-tree/src/visitor/mod.rs:137` `const MAX_STACK_DEPTH: usize = 64;` — TreeVisitor stack hint.
- `crates/flui-tree/src/visitor/mod.rs:178` `const STACK_SIZE: usize = 48;` — TreeVisitorMut stack hint.
- Plus hard-coded inline-32 in `DescendantStack = SmallVec<[Id; 32]>` at `iter/descendants.rs:12`.
- Plus hard-coded inline-8 in `TreeNavExt::path_to_node` `SmallVec<[I; 8]>` at `iter/path.rs:105`.

**Why it's a problem:**
- Four different declarations of "tree depth" with no documented relationship. When a real consumer needs to walk deeper than 32 (e.g., cycle 2 PR #101 fixed a similar issue in flui-layer's mark propagation), every constant needs an independent fix.
- The shallowest value (`TreeNav::MAX_DEPTH = 32`) is the iterator size_hint upper bound — anyone consuming a `size_hint()` for buffer pre-allocation gets 32, even though `MAX_TREE_DEPTH` says 256 is the real cap.
- Cycle 1 (PR #95) had the same pattern with three Ticker shapes; cycle 2 (PR #100) had four AccessibilityFeatures locks. Same shape repeating.

**Fix shape:** Single source of truth in `depth.rs`:
```rust
// crates/flui-tree/src/depth.rs

/// Maximum allowed tree depth (validation cap).
pub const MAX_TREE_DEPTH: usize = 256;

/// Default SmallVec inline capacity for tree-depth-bounded operations.
///
/// Sized for the typical UI tree (most consumers see depths 4-16).
/// Heap fallback beyond this; the upper bound is `MAX_TREE_DEPTH`.
pub const INLINE_TREE_DEPTH: usize = 64;
```

Then everywhere else:
- `TreeNav::MAX_DEPTH = depth::INLINE_TREE_DEPTH` (or remove the const entirely — let consumers reference `depth::INLINE_TREE_DEPTH` directly).
- `TreeVisitor::MAX_STACK_DEPTH = depth::INLINE_TREE_DEPTH`.
- `TreeVisitorMut::STACK_SIZE = depth::INLINE_TREE_DEPTH`.
- `DescendantStack = SmallVec<[Id; depth::INLINE_TREE_DEPTH]>`.

~30 LOC change across 4 files. Single source of truth.

**Blast radius:** Stylistic — SmallVec size hint changes from 32 to 64 don't break consumers (heap fallback handles overflow). All consumer-facing constants converge.

---

#### T-11 [P1 HOT-PATH | HIGH] `Descendants::next` recurses via `self.next()` on missing child — Rust does NOT guarantee tail-call optimization

**Evidence:**
- `crates/flui-tree/src/iter/descendants.rs:99-114`:
  ```rust
  fn next(&mut self) -> Option<Self::Item> {
      let current = self.stack.pop()?;

      // Check if current exists
      if !self.tree.contains(current) {
          return self.next(); // Skip and try next  ← recurse, NO TCO
      }

      // Push children in reverse order...
      ...
      Some(current)
  }
  ```
- Same recursion pattern at `DescendantsWithDepth::next` (line 152-166).
- Rust does NOT guarantee tail call optimization. A heavily-orphaned subtree (many slab entries removed but children list stale) can blow the stack — each `self.next()` recurse is a new stack frame.
- The fix is trivial: `loop { ... continue; ... break Some(current); }`.

**Why it's a problem:**
- Iterator that allocates a stack frame per skipped element can crash on malformed input. Cycle 2's PR #101 had the related cycle-rejection fix at the `add_child` path; this is the iterator-side of the same hygiene story.

**Fix shape:**
```rust
fn next(&mut self) -> Option<Self::Item> {
    loop {
        let current = self.stack.pop()?;
        if !self.tree.contains(current) {
            continue;  // skip missing, loop again — no recursion
        }
        let children: SmallVec<[I; 8]> = self.tree.children(current).collect();
        for child in children.into_iter().rev() {
            self.stack.push(child);
        }
        return Some(current);
    }
}
```

~10 LOC change per file, applied to `Descendants` + `DescendantsWithDepth`.

**Blast radius:** descendants.rs only. Pure stack-safety fix.

---

#### T-12 [P1 HOT-PATH | HIGH] `Ancestors::next` doesn't check for cycles — `parent` infinite loop on malformed tree

**Evidence:**
- `crates/flui-tree/src/iter/ancestors.rs:77-105`:
  ```rust
  fn next(&mut self) -> Option<Self::Item> {
      let current = self.current?;
      if !self.tree.contains(current) {
          self.current = None;
          return None;
      }
      self.current = self.tree.parent(current);
      Some(current)
  }
  ```
- No max-iteration cap. A 2-node cycle (`a.parent = b, b.parent = a`) loops forever.
- Cycle 2 PR #101 explicitly added cycle-rejection to `LayerTree::add_child` to prevent the cycle from being constructed in the first place. But the iterator still walks any cycle that slips past — defense in depth missing.

**Why it's a problem:**
- Iterator can hang on a corrupted tree. Production hazard if `RenderTree::set_parent` or similar has a bug.
- `size_hint` returns `(1, Some(T::MAX_DEPTH))` (line 101) but the iterator doesn't actually enforce the upper bound.

**Fix shape:**
```rust
fn next(&mut self) -> Option<Self::Item> {
    let current = self.current?;
    if !self.tree.contains(current) {
        self.current = None;
        return None;
    }
    self.steps += 1;
    if self.steps > T::MAX_DEPTH { // or MAX_TREE_DEPTH after T-10 fix
        debug_assert!(false, "Ancestors iterator exceeded MAX_DEPTH — cycle?");
        self.current = None;
        return None;
    }
    self.current = self.tree.parent(current);
    Some(current)
}
```

Add `steps: usize` field to `Ancestors` struct. Same for `AncestorsWithDepth`. ~15 LOC change.

**Blast radius:** ancestors.rs only.

---

#### T-13 [P1 API-SURFACE | HIGH] `TreeError` does not include `ChildIndexOutOfBounds` or arity-violation variants — `ArityError` lives in arity module

**Evidence:**
- `crates/flui-tree/src/error.rs:45-120`: `TreeError` enum with 9 variants (`NotFound`, `AlreadyExists`, `InvalidParent`, `CycleDetected`, `MaxDepthExceeded`, `EmptyTree`, `NotSupported`, `ConcurrentModification`, `Internal`).
- `crates/flui-tree/src/arity/error.rs`: `ArityError` enum with arity-specific variants (`TooManyChildren`, `TooFewChildren`, `IndexOutOfBounds`).
- Two parallel error enums for tree operations. `TreeWrite::insert_child` / `set_parent` returns `TreeError`. `ArityStorage::add_child` returns `ArityError`. No conversion.

**Why it's a problem:**
- Consumers of the unified tree need a single error type. Today they have to handle both.
- Cycle 2 pattern: `LayerError` is narrow per crate, `SemanticsError` narrow per crate — fine for domain-specific errors. But the **tree primitives** layer should have ONE error type.

**Fix shape:** Either:
- **(a)** Add `TreeError::ArityViolation(ArityError)` variant via `#[from]`.
- **(b)** Merge `ArityError` variants into `TreeError`.

(a) is the lower-risk shape:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TreeError {
    // existing variants ...
    /// Arity constraint violated.
    #[error(transparent)]
    ArityViolation(#[from] super::arity::ArityError),
}
```

**Blast radius:** error.rs + arity/error.rs. Backward-compatible — additive variant on `#[non_exhaustive]` enum.

---

#### T-14 [P1 PARITY-DRIFT | HIGH] `Identifier` trait `From<Index>` blanket impl gated behind `#[cfg(test)]`

**Evidence:**
- `crates/flui-foundation/src/id.rs:397-402`:
  ```rust
  // Test-only: Allow creating from usize for convenience
  #[cfg(test)]
  impl<T: Marker> From<Index> for Id<T> {
      fn from(index: Index) -> Self {
          Self::zip(index)
      }
  }
  ```
- `From<usize> → Id<T>` is `#[cfg(test)]`-gated — only available in tests. Production code must use `Id::new(n)` or `Id::zip(n)`.
- But `crates/flui-tree/src/traits/write.rs:215, 218, 256, 461` etc. uses `I: Into<usize>` bound and constructs error IDs from `usize` — this works because the `From<Id<T>> for Index` direction is public (`id.rs:362-368`).

**Why it's a problem:**
- The asymmetry is intentional (panics-on-zero prevention) but the `#[cfg(test)]` impl is a footgun — tests can construct IDs differently than production code.
- The blanket `Into<usize>` requirement in `TreeWriteNav::move_children` (write.rs:212-238) and `TreeWriteNav::insert_child` (line 256-270) is awkward — should be `Identifier::get` directly.

**Fix shape:** Replace `where I: Into<usize>` bounds with explicit `I::get()` calls (Identifier trait already provides `get(self) -> Index`). Delete the `#[cfg(test)] impl From<Index> for Id<T>` — tests use `Id::zip(n)` like production code.

```rust
// write.rs:212-238
fn move_children(&mut self, from: I, to: I) -> TreeResult<()>
where Self: Sized
{
    if !self.contains(from) {
        return Err(TreeError::not_found(from.get()));  // was: from.into()
    }
    // ...
}
```

**Blast radius:** id.rs + traits/write.rs. ~20 LOC across two files. Removes `#[cfg(test)]` test-only path. Cleaner.

---

#### T-15 [P2 API-SURFACE | MEDIUM] `TreeReadExt` + `TreeNavExt` + `MountableExt` extension traits — zero external consumers

**Evidence:**
- `crates/flui-tree/src/traits/read.rs:190-249`: `TreeReadExt` (~60 LOC) with HRTB-based `find_node_where`, `count_nodes_where`, `collect_nodes_where`, `for_each_node`.
- `crates/flui-tree/src/traits/nav.rs:282-466`: `TreeNavExt` (~180 LOC) with `find_child_where`, `find_descendant_where`, `visit_subtree`, `count_descendants_where`, `path_to_node`, `nth_child`, `first_and_last_child`, `cursor_at`, `cursor_with_history`, `cursor_at_root`.
- `crates/flui-tree/src/state.rs:381-400`: `MountableExt` (~20 LOC) with `mount_root`, `mount_child`.
- Grep external consumers of any of these `*Ext` methods: 0.

**Why it's a problem:**
- ~260 LOC of extension surface that's not consumed.
- HRTB-based predicate API surface is academically interesting but ergonomically redundant — `tree.node_ids().filter(|id| tree.get(*id).map_or(false, |n| predicate(n)))` covers the use case without the trait machinery.

**Fix shape:**
- **Phase 1** — Move `TreeReadExt`, `TreeNavExt` behind `#[cfg(feature = "tree-ext")]`. The cursor methods (`cursor_at` etc.) consume `TreeCursor` which is also feature-gated (T-6). Coherent feature.
- **Phase 2** — Re-evaluate after feature-gate is in place; if no consumer materializes within 1-2 cycles, delete.

**Blast radius:** traits/{read,nav}.rs + lib.rs re-exports.

---

#### T-16 [P2 PARITY-DRIFT | MEDIUM] `TreeError::MaxDepthExceeded` and `NotSupported` carry `&'static str` reason — no programmatic recovery

**Evidence:**
- `crates/flui-tree/src/error.rs:85-91`:
  ```rust
  #[error("maximum tree depth {max} exceeded at element {element}")]
  MaxDepthExceeded { element: usize, max: usize },
  ```
- `crates/flui-tree/src/error.rs:103-105`:
  ```rust
  #[error("operation not supported for element {0}: {1}")]
  NotSupported(usize, &'static str),
  ```
- `&'static str` reason is good for static error messages, but inconsistent with `Internal(String)` which uses owned String.
- `MaxDepthExceeded { element, max }` is good shape. `NotSupported` could match.

**Fix shape:** Either:
- **(a)** Standardize on `&'static str` for static reasons, `String` for dynamic. Already mostly correct.
- **(b)** Standardize on `Cow<'static, str>` for flexibility.

(a) is fine. The audit observation is that `Internal(String)` could be `Internal(Box<str>)` per I-19 — minor optimization.

**Blast radius:** error.rs only.

---

#### T-17 [P2 API-SURFACE | MEDIUM] `Slot::with_siblings` constructor accepts 5 params positionally — easy to swap previous/next

**Evidence:**
- `crates/flui-tree/src/iter/slot.rs:84-118` (not all shown above): `Slot::new` 3 params, `Slot::with_siblings` 5 params:
  ```rust
  pub fn with_siblings(parent, index, depth, previous_sibling, next_sibling) -> Self
  ```
- All 5 params positional. `Option<I>` for previous and next look identical — easy to swap.

**Fix shape:** Use `bon` builder (already a workspace dependency, used for SceneBuilder etc.):
```rust
#[bon::builder]
pub fn with_siblings(
    parent: I,
    index: usize,
    depth: Depth,
    previous_sibling: Option<I>,
    next_sibling: Option<I>,
) -> Self { ... }
```

Allows `Slot::with_siblings_builder().parent(p).index(0).depth(...).previous_sibling(None).next_sibling(Some(s)).call()`.

**Blast radius:** slot.rs only. Backward-compatible if positional constructor is kept.

---

#### T-18 [P2 PARITY-DRIFT | MEDIUM] `TreeNav::lowest_common_ancestor` allocates 2 `Vec<I>` for ancestor lists per call

**Evidence:**
- `crates/flui-tree/src/traits/nav.rs:262-279`:
  ```rust
  fn lowest_common_ancestor(&self, a: I, b: I) -> Option<I> {
      if a == b { return Some(a); }
      let ancestors_a: Vec<I> = self.ancestors(a).collect();
      let ancestors_b: Vec<I> = self.ancestors(b).collect();
      ancestors_a.iter().rev().zip(ancestors_b.iter().rev())
          .take_while(|(a, b)| a == b)
          .last()
          .map(|(a, _)| *a)
  }
  ```
- 2 Vec allocations per call. For typical UI tree depths (4-16), this is moderate; for deep trees, expensive.

**Fix shape:** Use `SmallVec<[I; INLINE_TREE_DEPTH]>` (per T-10 unified constant) to avoid allocation in the common case. ~5 LOC change.

**Blast radius:** nav.rs only. Backward-compatible.

---

#### T-19 [P2 HOT-PATH | MEDIUM] `TreeNav::depth` default impl walks ancestors and counts — O(depth) per call

**Evidence:**
- `crates/flui-tree/src/traits/nav.rs:226-228`:
  ```rust
  fn depth(&self, id: I) -> usize {
      self.ancestors(id).count().saturating_sub(1)
  }
  ```
- O(depth) per call. For trees storing depth directly in the node (every workspace tree implementer should), this is wasteful.

**Why it's a problem:**
- Same pattern as T-9 — default impl is a perf trap for consumers that haven't overridden.
- RenderTree should override with O(1) since it stores depth on `RenderNode`.

**Fix shape:** Documentation lift — mark the default impl as `// SLOW DEFAULT: O(depth)` and note that Slab-based impls should override. Or move to `TreeNavExt` (like T-9).

**Blast radius:** nav.rs only.

---

#### T-20 [P2 API-SURFACE | MEDIUM] `Siblings` iterator collects parent's children Vec at construction — per-call alloc

**Evidence:**
- `crates/flui-tree/src/iter/siblings.rs:99-118`:
  ```rust
  pub fn new(tree: &'a T, start: I, direction: SiblingsDirection, include_self: bool) -> Self {
      let (children, current_index) = if let Some(parent) = tree.parent(start) {
          let sibs: Vec<_> = tree.children(parent).collect();  // ← per-call Vec
          // ...
      };
      // ...
  }
  ```
- Constructor allocates `Vec<I>` — for a layout pipeline calling `Siblings::new` per child per frame, allocator pressure.

**Fix shape:** `SmallVec<[I; INLINE_TREE_DEPTH]>` (per T-10). Per-construction overhead is bounded.

**Blast radius:** siblings.rs only.

---

#### T-21 [P3 DEAD-CODE | LOW] `arity/types.rs::Leaf::first_impossible` panics with `Vec<T>` argument — never called

**Evidence:**
- `crates/flui-tree/src/arity/types.rs:84-91`:
  ```rust
  impl Leaf {
      pub fn first_impossible<T>(_children: &[T]) -> ! {
          panic!("Leaf nodes cannot have children - this operation is impossible")
      }
  }
  ```
- Grep external callers: 0. Internal callers: 0.
- The `!` return type makes this a runtime no-return — but nobody calls it. Dead.

**Fix shape:** Delete. ~10 LOC reduction.

**Blast radius:** types.rs only.

---

#### T-22 [P3 PARITY-DRIFT | LOW] `arity/types.rs::Never::default()` returns `Never` — unit struct should be `pub struct Never;`

**Evidence:**
- `crates/flui-tree/src/arity/types.rs:419-423`:
  ```rust
  impl Default for Never {
      fn default() -> Self {
          Never
      }
  }
  ```
- `Never` is `pub struct Never` (zero-sized, no fields). `#[derive(Default)]` would generate the same impl automatically.

**Fix shape:** `#[derive(Debug, Clone, Copy, Default)]` on `Never`. Delete manual impl. ~5 LOC reduction.

**Blast radius:** types.rs only.

---

#### T-23 [P3 PARITY-DRIFT | LOW] `arity/traits.rs::Arity` associated constants `EXPECTED_SIZE`, `INLINE_THRESHOLD`, `BATCH_SIZE` — used only by storage machinery (which is itself zombie)

**Evidence:**
- `crates/flui-tree/src/arity/traits.rs:90-93`: `Arity` trait has 4 associated constants.
- Grep external usage of `<A as Arity>::EXPECTED_SIZE` / `<A as Arity>::INLINE_THRESHOLD` / `<A as Arity>::BATCH_SIZE`: 0.
- Only used inside `arity_storage.rs` and `accessors.rs` (both zombie per T-7).

**Why it's a problem:**
- If T-7 feature-gates the storage machinery, these associated constants on the Arity trait become orphan unused fields.

**Fix shape:** Move associated constants to a separate trait `ArityStorageHints: Arity` and feature-gate it with `arity-storage`. Or leave inline but `#[allow(dead_code)]` until storage materializes.

**Blast radius:** Together with T-7.

---

#### T-24 [P3 API-SURFACE | LOW] `DescendantsWithDepth::new` exposed but rarely used — `TreeNav::descendants` returns the same tuple via the trait

**Evidence:**
- `crates/flui-tree/src/iter/descendants.rs:138-147`: `DescendantsWithDepth::new(tree, root)` constructor.
- Grep external callers: layer/semantics/rendering's TreeNav impls use it via `DescendantsWithDepth::new(self, root)` (forwarding from trait `descendants()`). The trait already returns `impl Iterator<Item = (I, usize)>` so callers don't need the concrete type.
- Direct `DescendantsWithDepth::new` callers outside TreeNav impls: 0.

**Why it's a problem:**
- The concrete iterator type is publicly exposed for type-name reference only — flui-layer/semantics/rendering use the trait method, not the constructor directly.

**Fix shape:** Make `DescendantsWithDepth::new` `pub(crate)` — TreeNav impl forwarders use it but external code routes through the trait. Same for `Descendants::new`. Same for `Ancestors::new`, `AncestorsWithDepth::new`.

**Blast radius:** ancestors.rs + descendants.rs. Visibility downgrade only.

---

#### T-25 [P3 DEAD-CODE | LOW] `iter/depth_first.rs::DepthFirstOrder` enum exposed but Pre/Post never differentiated by consumers

**Evidence:**
- `crates/flui-tree/src/iter/depth_first.rs` (338 LOC) — DepthFirstIter parameterized over `DepthFirstOrder::{PreOrder, PostOrder}` enum.
- The two TreeNav-trait-implementing crates (flui-layer, flui-semantics) use `DescendantsWithDepth` (pre-order only) — never the configurable `DepthFirstIter`.
- No consumer differentiates PostOrder.

**Why it's a problem:**
- 338-LOC iterator with enum parameter, but the enum's variant never differentiates the consumer's behavior.

**Fix shape:** Bundle with T-6 (feature-gate the whole `breadth_first.rs` + `depth_first.rs` module).

**Blast radius:** With T-6.

---

## Part III — Flutter drift catalog

Each drift cites Flutter source line. Drifts are *intentional Rust-native shapes* vs *gaps to bridge*. Severity tagged.

### Drift A — `ChangeNotifier` HashMap-based listener storage vs Flutter's array

**Flutter** (`change_notifier.dart:240-280`): `_listeners` is a fixed-size `List<VoidCallback?>` (preallocated to 8 then grown). `_count` tracks active entries; nulls fill removed slots. `_listeners[i] = null` on removal; `_listeners[_count++] = listener` on add.

**FLUI** (`crates/flui-foundation/src/notifier.rs:132`): `Arc<Mutex<HashMap<ListenerId, ListenerCallback>>>`.

**Reason for drift:** HashMap provides O(1) removal by ID. Flutter's array uses linear scan + null-fill, with periodic compaction. Both are valid.

**Severity:** Low (intentional improvement — O(1) removal vs Flutter's O(N) compaction).

**Action:** None — keep current shape. Mention drift in doc-comment.

### Drift B — `Key` const FNV-1a vs Flutter's string-keyed `Key.value`

**Flutter** (`key.dart:128`): `class StringKey extends Key { final String value; }`. Comparison via `==` on `value`.

**FLUI** (`crates/flui-foundation/src/key.rs:81`): `Key(NonZeroU64)` with const FNV-1a hash. `Key::from_str("name")` returns `Key(hash("name"))`.

**Reason for drift:** Rust supports compile-time const hashing; Flutter doesn't. Trade-off: FLUI loses string-identity (two different strings with same hash collide).

**Severity:** Medium — silent collision is the I-6 finding. Fix the zero-case + reduce entropy.

**Action:** Per I-6 — guarantee non-zero output by setting top bit unconditionally.

### Drift C — `ViewKey` trait + `is_global_key()` cheap-skip vs Flutter's `Object` identity

**Flutter** (`framework.dart:3148`): `GlobalKey` uses Dart `Object` identity (`identityHashCode`). Registry is `HashMap<GlobalKey, Element>` keyed on object identity.

**FLUI** (`crates/flui-foundation/src/key.rs:329-349`): `ViewKey::is_global_key(&self) -> bool` default-false; GlobalKey impl returns true. Allows cheap-skip in registry consultation without `Any::downcast`.

**Reason for drift:** Rust has no `identityHashCode`. Explicit method is the Rust-native equivalent.

**Severity:** High — default-false is the I-8 footgun.

**Action:** Per I-8 — make the method abstract (no default), force impls to think about it.

### Drift D — `BindingBase` `OnceLock` + `AtomicBool` vs Flutter's mixin chain

**Flutter** (`binding.dart:79-180`): `BindingBase` is a class with `initInstances()` virtual method. Subclasses extend via Dart mixins. Single-threaded init.

**FLUI** (`crates/flui-foundation/src/binding.rs`): `BindingBase` trait + `HasInstance` trait + `OnceLock<Self>` + `AtomicBool` for init signaling.

**Reason for drift:** Rust has no class inheritance + Dart-style mixin syntax. `OnceLock` provides Send/Sync init guarantees.

**Severity:** High — race hazard at init failure (I-3 finding).

**Action:** Per I-3 — flip `INITIALIZED.store` AFTER `<Self>::new()` returns.

### Drift E — `TreeWrite::remove` non-cascade default vs Flutter's `LayerHandle._unref` cascade

**Flutter** (`rendering/layer.dart:783-822`): `LayerHandle<T>` ref-counts the layer; when last handle drops, the layer's `dispose()` cascades to descendants via `ContainerLayer.remove`.

**FLUI** (`crates/flui-tree/src/traits/write.rs:64-81`): `TreeWrite::remove` documents that descendant handling depends on impl. RenderTree (only impl) doesn't cascade.

**Reason for drift:** None — this is unintentional, codified pre-cycle-2-PR-#100 footgun.

**Severity:** Critical (T-1 finding).

**Action:** Per T-1 — flip the default to cascade.

### Drift F — `TreeNav::ancestors` size_hint upper bound `T::MAX_DEPTH = 32` vs `MAX_TREE_DEPTH = 256`

**FLUI internal**: four different depth constants (T-10 finding).

**Reason for drift:** Independent code paths choosing different "sane" defaults.

**Severity:** High (T-10 finding).

**Action:** Single source of truth in `depth.rs`.

### Drift G — `Mountable`/`Unmountable` two-state typestate vs Flutter's four-state Element FSM

**Flutter** (`framework.dart::Element._lifecycleState`): `_ElementLifecycle.{ initial, active, inactive, defunct }`. Defunct is terminal — Element cannot return to active. Inactive supports reparenting via GlobalKey.

**FLUI** (`crates/flui-tree/src/state.rs`): two-state `Mountable<Unmounted>` / `Unmountable<Mounted>` typestate. Reversible.

**Reason for drift:** Flutter's lifecycle is Element-specific; FLUI tried to generalize for any tree node — but no other tree needs typestate.

**Severity:** Critical (T-3 finding) — unused machinery.

**Action:** Per T-3 — delete state.rs.

### Drift H — `Node` trait with `type Id: Identifier` vs Flutter's `Element extends DiagnosticableTree`

**Flutter** (`framework.dart::Element`): inherits behavior from `DiagnosticableTree` + has `Object key`, `dynamic widget` etc. No type-parameterized base trait.

**FLUI** (`crates/flui-tree/src/traits/node.rs`): trait `Node { type Id: Identifier; }`. Zero external impls (T-8 finding).

**Reason for drift:** Tried to generalize the `node ↔ id` relationship. No consumer uses it.

**Severity:** Critical (T-8 finding).

**Action:** Per T-8 — delete or feature-gate.

### Drift I — `arity/storage.rs` `ArityStorage<T, A>` enum vs flui-rendering's per-arity `BoxChild<A>` struct

**Flutter equivalent**: `RenderObjectWithChildMixin<ChildType extends RenderObject>` for single-child, `ContainerRenderObjectMixin<ChildType extends RenderObject, ParentDataType extends ContainerParentDataMixin<ChildType>>` for multi-child. Mixin-based.

**FLUI tree-side** (`arity/arity_storage.rs:60+`): `ArityStorage<T, A>` enum with variants per arity. Zero external consumers.

**FLUI rendering-side** (`flui-rendering/src/storage/`): `BoxChild<A>` per-arity struct. Different shape.

**Reason for drift:** Tree-side speccd a generic; rendering-side implemented a different concrete pattern.

**Severity:** Critical (T-7 finding) — 3,051 LOC unused.

**Action:** Per T-7 — feature-gate.

### Drift J — `Descendants::next` recursion on miss vs Flutter `Layer._depthFirstWalkChildren` loop

**Flutter** (`rendering/layer.dart:457+`): explicit loop, no recursion.

**FLUI** (`crates/flui-tree/src/iter/descendants.rs:99-114`): `return self.next();` — unbounded recursion possible (T-11 finding).

**Severity:** High (T-11 finding).

**Action:** Loop with `continue`.

---

## Part IV — Final combined priority order

Severity legend: P0 = critical correctness / cycle-2-parity-essential, P1 = high-impact API or hot-path, P2 = medium-impact hygiene, P3 = low-priority cleanup.

| # | Crate | Finding | Severity | Size (LOC) | Depends on | Notes |
|---|---|---|---|---|---|---|
| **P0 — Critical correctness (must land first; cycle-2 parity)** | | | | | | |
| 1 | flui-tree | T-1: `TreeWrite::remove` cascade-by-default + `remove_shallow` opt-out | P0 | +60 trait, −0 impl | None | Cycle 2 PR #100 U12/U13 parity at the trait level. **Most important finding of this cycle.** |
| 2 | flui-tree | T-2: `LayerTree` + `SemanticsTree` impl `TreeWrite<I>` (after T-1) — remove parallel mutation APIs | P0 | +60 impls, −200 dup | T-1 | Closes [[flui-tree-unified-interface-intent]] gap; ~200 LOC dedup. |
| 3 | flui-foundation | I-3: BindingBase macro `INITIALIZED.store` ordering — flip to AFTER `new()` returns | P0 | ±10 | None | Re-init-after-panic hazard; 5 binding consumers benefit. |
| 4 | flui-tree | T-3: Delete `state.rs` (Mountable/Unmountable typestate, 616 LOC, 0 consumers) | P0 | −616 | None | Cycle 1 PR #93 pattern. |
| 5 | flui-tree | T-4: Delete `visitor/mod.rs::StatefulVisitor` + `TypedVisitor`; feature-gate `visitor/composition.rs` + `fallible.rs` (`visitors` feature, default off) | P0 | −250 delete + cfg-gate 2,300 | None | Cycle 1 PR #93 pattern; reverts mass-zombie surface. |
| 6 | flui-tree | T-5: Feature-gate `diff.rs` (`tree-diff` feature, default off) | P0 | cfg-gate 1,234 | None | Devtools consumer awaited. |
| 7 | flui-tree | T-6: Feature-gate `iter/cursor.rs` + `iter/path.rs` + `iter/{breadth,depth}_first.rs` (`tree-iter-advanced` feature, default off) | P0 | cfg-gate 3,812 | None | Devtools consumer awaited. |
| 8 | flui-tree | T-7: Feature-gate `arity/{storage,arity_storage,accessors}.rs` (`arity-storage` feature, default off) | P0 | cfg-gate 3,051 | None | Storage machinery unused; markers stay public. |
| 9 | flui-foundation | I-1: Delete `observer.rs::ObserverList` | P0 | −271 | None | 0 consumers; ChangeNotifier covers the use case. |
| 10 | flui-foundation | I-2: Delete `error.rs::FoundationError` + `ErrorContext` | P0 | −335 | None | 0 consumers; clashes with anyhow Context. |
| **P1 — High-impact (next wave)** | | | | | | |
| 11 | flui-tree | T-8: Delete or feature-gate `traits/node.rs::Node` trait | P1 | −305 or cfg-gate | None | 0 external impls; type alias `type Id: Identifier` provides no work. |
| 12 | flui-tree | T-10: Single-source-of-truth `MAX_TREE_DEPTH` + `INLINE_TREE_DEPTH` | P1 | ±30 | None | Quadruple drift fix. |
| 13 | flui-tree | T-11: `Descendants::next` loop instead of `self.next()` recursion | P1 | ±10 | None | Stack-safety fix. |
| 14 | flui-tree | T-12: `Ancestors::next` cycle-rejection via step counter | P1 | ±15 | T-10 | Defense in depth vs corrupted tree. |
| 15 | flui-tree | T-13: `TreeError::ArityViolation(#[from] ArityError)` variant | P1 | ±15 | None | Error type unification. |
| 16 | flui-foundation | I-5: Remove `Default for Key` + `Default for UniqueKey` impls | P1 | −20 | None | Surprising Default semantics; forces explicit construction. |
| 17 | flui-foundation | I-6: `Key::from_str` guarantee non-zero output (top-bit OR) | P1 | ±10 | None | Silent collision fix. |
| 18 | flui-foundation | I-7: `Key::try_new` Result-returning constructor; `Key::new` calls it | P1 | ±20 | None | Off-by-one in overflow check. |
| 19 | flui-foundation | I-8: `ViewKey::is_global_key()` abstract (no default) | P1 | ±15 | None | Forces consumers to think. |
| 20 | flui-foundation | I-4: `ChangeNotifier::notify_listeners` uses `SmallVec<[CB; 4]>` instead of `Vec` | P1 | ±10 | None | Per-frame alloc fix on hot-path. |
| 21 | flui-tree | T-9: `TreeNav::slot` default impl → streaming iter (no Vec alloc) | P1 | ±25 | None | Per-call alloc fix. |
| 22 | flui-tree | T-14: `Identifier` trait `From<Index>` test-only impl removal | P1 | ±20 | None | Remove `#[cfg(test)]` asymmetry; cleaner. |
| **P2 — Medium-impact hygiene** | | | | | | |
| 23 | flui-tree | T-15: `TreeReadExt` + `TreeNavExt` + `MountableExt` feature-gate (`tree-ext` feature, default off) | P2 | cfg-gate ~260 | None | 0 consumers; cleanly opt-in. |
| 24 | flui-foundation | I-9: `Id<T>::from_raw` / `zip_unchecked` / `new_unchecked` `pub(crate)` | P2 | ±10 | None | Hide unsafe escape hatches; serde uses internally. |
| 25 | flui-foundation | I-10: `RawId` + `Index` `pub(crate)` | P2 | ±5 | None | Clean public surface. |
| 26 | flui-foundation | I-11: `#[non_exhaustive]` on `DiagnosticsTreeStyle` + `DiagnosticLevel` | P2 | ±5 | None | Future-compat. |
| 27 | flui-foundation | I-12: Sweep doc-comments to cite Flutter file:line refs uniformly | P2 | doc churn ~50 | None | Cycle 2 pattern. |
| 28 | flui-foundation | I-13: Delete `consts.rs::approx_equal*` + `is_near_zero*` (move to flui-types) | P2 | −60 | flui-types | 0 consumers. |
| 29 | flui-foundation | I-14: Delete `assert.rs::report_error!` + `report_warning!` macros | P2 | −25 | None | 0 consumers; tracing direct calls cover the case. |
| 30 | flui-tree | T-16: `TreeError::Internal(Box<str>)` instead of `String` | P2 | ±5 | None | Minor alloc savings. |
| 31 | flui-tree | T-17: `Slot::with_siblings` builder via `bon` | P2 | ±15 | None | Positional swap-hazard fix. |
| 32 | flui-tree | T-18: `lowest_common_ancestor` uses `SmallVec<[I; INLINE_TREE_DEPTH]>` | P2 | ±10 | T-10 | Allocation fix. |
| 33 | flui-tree | T-19: `TreeNav::depth` doc-mark as "slow default" + recommend override | P2 | doc ±5 | None | Or move to `TreeNavExt`. |
| 34 | flui-tree | T-20: `Siblings::new` uses SmallVec | P2 | ±10 | T-10 | Per-construction alloc. |
| **P3 — Low-priority cleanup** | | | | | | |
| 35 | flui-foundation | I-15: `ChangeNotifier::has_listeners` / `is_empty` / `len` via lock-free `AtomicUsize` | P3 | ±25 | I-4 | Hot-path counter. |
| 36 | flui-foundation | I-16: `ListenerCallback` explicit `+ 'static` bound | P3 | ±3 | None | Doc clarity. |
| 37 | flui-foundation | I-17: `ValueNotifier::take` / `replace` / `value_mut` audit / mark unused | P3 | doc ±10 | None | Judgment call. |
| 38 | flui-foundation | I-18: `Marker` trait drop `+ Debug` requirement | P3 | ±5 | None | Aesthetic. |
| 39 | flui-foundation | I-19: `ParseDiagnosticLevelError::String` → `Box<str>` | P3 | ±5 | None | Together with I-11. |
| 40 | flui-foundation | I-20: `ValueNotifier::into_value` calls `notifier.dispose()` before drop | P3 | ±5 | None | PR #84 dispose template propagation. |
| 41 | flui-foundation | I-21: Deprecate `KeyRef::new` in favor of `From<Key>` | P3 | ±5 | None | Single-source constructor. |
| 42 | flui-foundation | I-22: Delete `WasmNotSend` (unused) | P3 | −15 | None | 0 consumers. |
| 43 | flui-tree | T-21: Delete `Leaf::first_impossible` (never called) | P3 | −10 | None | Cleanup. |
| 44 | flui-tree | T-22: `#[derive(Default)]` on `Never` (replace manual impl) | P3 | ±5 | None | Cleanup. |
| 45 | flui-tree | T-23: Move `Arity` associated constants behind `arity-storage` feature | P3 | ±10 | T-7 | Together with T-7. |
| 46 | flui-tree | T-24: `Descendants::new` / `Ancestors::new` etc. `pub(crate)` | P3 | ±10 | None | TreeNav impls use them; external code routes through trait. |
| 47 | flui-tree | T-25: Bundle with T-6 (`DepthFirstOrder` enum unused) | P3 | with T-6 | T-6 | Feature-gated. |

**Total LOC delta** (estimated): ~−9,500 LOC deleted/feature-gated (visitor/diff/cursor/path/storage/state.rs as the lion's share) + ~+200 LOC added (TreeWrite cascade trait machinery + ChangeNotifier counter + 2 TreeWrite impls in flui-layer/flui-semantics + Flutter ref doc-comments). Net: ~−9,300 LOC reduction in public surface.

**Cycle alignment with predecessors:**
- Total scope (23,448 LOC, 47 findings) > cycle 2 (15,571 LOC, 25 findings) > cycle 1 (12,360 LOC, 16 findings + ~20 cross-ref drifts).
- Total zombie surface (~11,400 LOC) > cycle 1 (~3,995 LOC zombie) > cycle 2 (~280 LOC zombie + ~500 LOC parallel-mutation per tree).
- Cycle 3 is the **largest scope and largest zombie surface** of the three cycles. The "no quick wins" memory binding means we cannot just delete — we must consolidate (T-1, T-2) AND deprecate-then-feature-gate (T-3 through T-8).

---

## Appendix A — Investigation receipts

### A.1 — Project shape

```bash
$ wc -l crates/flui-foundation/src/*.rs
  1065 crates/flui-foundation/src/debug.rs
   863 crates/flui-foundation/src/key.rs
   839 crates/flui-foundation/src/notifier.rs
   769 crates/flui-foundation/src/id.rs
   335 crates/flui-foundation/src/error.rs
   295 crates/flui-foundation/src/lib.rs
   272 crates/flui-foundation/src/binding.rs
   271 crates/flui-foundation/src/observer.rs
   263 crates/flui-foundation/src/callbacks.rs
   200 crates/flui-foundation/src/assert.rs
   185 crates/flui-foundation/src/consts.rs
    67 crates/flui-foundation/src/wasm.rs
  5424 total

$ find crates/flui-tree/src -name "*.rs" | xargs wc -l | sort -rn | head -20
  1399 crates/flui-tree/src/arity/accessors.rs
  1264 crates/flui-tree/src/visitor/mod.rs
  1234 crates/flui-tree/src/diff.rs
  1150 crates/flui-tree/src/iter/path.rs
  1143 crates/flui-tree/src/depth.rs
  1057 crates/flui-tree/src/iter/cursor.rs
   911 crates/flui-tree/src/iter/slot.rs
   858 crates/flui-tree/src/arity/arity_storage.rs
   794 crates/flui-tree/src/arity/storage.rs
   762 crates/flui-tree/src/traits/nav.rs
   648 crates/flui-tree/src/visitor/composition.rs
   638 crates/flui-tree/src/visitor/fallible.rs
   616 crates/flui-tree/src/state.rs
   597 crates/flui-tree/src/traits/write.rs
   592 crates/flui-tree/src/traits/read.rs
   576 crates/flui-tree/src/arity/types.rs
   554 crates/flui-tree/src/iter/siblings.rs
   378 crates/flui-tree/src/error.rs
   338 crates/flui-tree/src/iter/depth_first.rs
   328 crates/flui-tree/src/iter/ancestors.rs

$ find crates/flui-foundation/src crates/flui-tree/src -name "*.rs" -exec wc -l {} \; | awk '{sum+=$1} END {print "Total LOC:", sum}'
Total LOC: 23448
```

### A.2 — Zero-consumer module verification (workspace ripgrep)

```bash
# Visitor surface — 2,550 LOC across 3 files, zero external consumers
$ rg "TreeVisitor|TreeVisitorMut|CollectVisitor|CountVisitor|FindVisitor|MaxDepthVisitor|ForEachVisitor|StatefulVisitor|TypedVisitor|ComposedVisitor|ConditionalVisitor|DynVisitor|MappedVisitor|VisitorVec|VisitorExt|FallibleVisitor|DepthLimitVisitor|TryCollectVisitor|TryForEachVisitor" crates --type rust | grep -v "flui-tree/"
# (0 hits)

# Diff types — 1,234 LOC, zero external consumers
$ rg "TreeDiff|DiffOp|ChildDiff|ChildOp|DiffStats" crates --type rust | grep -v "flui-tree/"
# (0 hits)

# Cursor/Path/Index iterators
$ rg "BreadthFirstIter|DepthFirstIter|TreeCursor|TreePath|IndexPath" crates --type rust | grep -v "flui-tree/"
crates/flui-view/src/element/mod.rs:25:// Slot types live in flui-tree (canonical home per `flui-tree-unified-interface-intent`
crates/flui-view/src/element/mod.rs:28:pub use flui_tree::IndexedSlot;
# Only IndexedSlot is consumed externally — by flui-view as ElementSlot alias.
# TreeCursor, TreePath, IndexPath, BreadthFirstIter, DepthFirstIter — 0 hits.

# Mountable / Unmountable typestate
$ rg "Mountable|Unmountable|Mounted|Unmounted|NodeState|MountableExt" crates --type rust | grep -v "flui-tree/"
# (0 hits)

# Node trait
$ rg "impl Node for|implements Node " crates --type rust
crates/flui-tree/src/traits/node.rs:142:pub trait Node: Sized + Send + Sync + 'static {
crates/flui-tree/src/traits/node.rs:260:    impl Node for TestNode {  # test only
# (0 external impls)

# ChildrenStorage / ArityStorage / accessors
$ rg "ArityStorage|ChildrenStorage|ChildrenAccess|ChildrenStorageExt|FixedChildren|SliceChildren|NeverAccessor|OptionalChild|NoChildren|BoundedChildren|SmartChildren|TypedChildren|Copied" crates --type rust | grep -v "flui-tree/"
# (0 hits)

# TreeReadExt / TreeNavExt
$ rg "TreeReadExt|TreeNavExt" crates --type rust | grep -v "flui-tree/"
# (0 hits)

# Siblings iterator struct (the AllSiblings/Siblings type — not the trait method)
$ rg "Siblings|AllSiblings|SiblingsDirection" crates --type rust | grep -v "flui-tree/"
# (0 hits)

# ObserverList
$ rg "ObserverList" crates --type rust | grep -v "flui-foundation/"
# (0 hits in production code; examples-only)

# FoundationError
$ rg "FoundationError" crates --type rust | grep -v "flui-foundation/"
# (0 hits in production code)

# ErrorContext - clashes with anyhow
$ rg "ErrorContext" crates --type rust
crates/flui-foundation/src/error.rs:215:pub trait ErrorContext<T> {
# (only declaration; 0 external consumers)
$ rg "with_context" crates --type rust | grep -v "flui-foundation/" | head -3
crates/flui-cli/src/commands/emulators.rs:184:        .with_context(|| format!("Failed to run '{emulator_path}'"))?;
# flui-cli uses anyhow::Context, NOT flui_foundation::ErrorContext.

# RawId / Index
$ rg "flui_foundation::Index|flui_foundation::RawId|use flui_foundation::Index|use flui_foundation::RawId" crates --type rust | grep -v "flui-foundation/"
# (0 hits)

# WasmNotSend
$ rg "WasmNotSend\b" crates --type rust | grep -v "flui-foundation/\|WasmNotSendSync"
# (0 hits)
```

### A.3 — Consumer relationships (positive)

```bash
# ChangeNotifier (heavy consumer — flui-animation, currently disabled but real)
$ rg "ChangeNotifier\b" crates --type rust | grep -v "flui-foundation/" | head -10
crates/flui-animation/src/compound.rs:5:use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
crates/flui-animation/src/controller.rs:7:use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
crates/flui-animation/src/curved.rs:6:...
# 8 files total in flui-animation use it as canonical Listenable.

# BindingBase + HasInstance + impl_binding_singleton — 5 binding consumers
$ rg "BindingBase|HasInstance|check_instance|impl_binding_singleton" crates --type rust | grep -v "flui-foundation/"
crates/flui-app/src/bindings/renderer_binding.rs:41:use flui_foundation::{BindingBase, HasInstance, impl_binding_singleton};
crates/flui-rendering/src/...
crates/flui-interaction/src/binding.rs:12://! ...
crates/flui-scheduler/src/...
crates/flui-semantics/src/...
# 5 production bindings adopt the pattern.

# Marker / markers::* — flui-scheduler IdGenerator
$ rg "markers::" crates --type rust | head -5
crates/flui-foundation/src/id.rs:495:            pub type $name = Id<markers::$marker>;
crates/flui-scheduler/src/frame.rs:15://! let id_gen = IdGenerator::<markers::Frame>::new();
crates/flui-scheduler/src/id.rs:36:    FrameCallbackId, FrameId, Id, Identifier, Index, Marker, RawId, TaskId, TickerId, markers,
crates/flui-scheduler/src/scheduler.rs:347:    id_gen: IdGenerator<flui_foundation::markers::FrameCallback>,
# flui-scheduler exploits markers extensively.

# Diagnosticable — 10+ flui-rendering RenderObjects
$ rg "Diagnosticable\b" crates --type rust | grep -v "flui-foundation/"
crates/flui-rendering/src/objects/center.rs:68:impl flui_foundation::Diagnosticable for RenderCenter {
crates/flui-rendering/src/objects/colored_box.rs:57:impl flui_foundation::Diagnosticable for RenderColoredBox {}
crates/flui-rendering/src/objects/flex.rs:191:impl flui_foundation::Diagnosticable for RenderFlex {}
... (10 impls in flui-rendering)

# Key types — flui-view canonical home
$ rg "ValueKey|UniqueKey|with_view_key|with_value_key|with_unique_key|ViewKey" crates --type rust | grep -v "flui-foundation/" | head -10
crates/flui-view/src/key/mod.rs:24:pub use flui_foundation::{Key, KeyRef, Keyed, UniqueKey, ValueKey, ViewKey, WithKey};
crates/flui-view/src/lib.rs:184:pub use key::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey};
...
# flui-view re-exports and uses extensively.

# Arity markers (NOT storage machinery) — flui-rendering
$ rg "use flui_tree::{Leaf|use flui_tree::Single|use flui_tree::Optional|use flui_tree::Variable" crates --type rust
crates/flui-rendering/src/objects/center.rs:3:use flui_tree::Single;
crates/flui-rendering/src/objects/colored_box.rs:4:use flui_tree::Leaf;
crates/flui-rendering/src/objects/flex.rs:3:use flui_tree::Variable;
crates/flui-rendering/src/objects/opacity.rs:3:use flui_tree::Single;
crates/flui-rendering/src/objects/padding.rs:3:use flui_tree::Single;
crates/flui-rendering/src/objects/sized_box.rs:3:use flui_tree::Leaf;
crates/flui-rendering/src/objects/transform.rs:3:use flui_tree::Single;
# Markers actively used; storage machinery not.

# IndexedSlot — flui-view ElementSlot alias
$ rg "IndexedSlot|ElementSlot" crates --type rust | grep -v "flui-tree/"
crates/flui-view/src/element/mod.rs:28:pub use flui_tree::IndexedSlot;
crates/flui-view/src/element/mod.rs:80:pub type ElementSlot = IndexedSlot<ElementId>;
# Working consumer relationship.
```

### A.4 — TreeWrite gap (CRITICAL P0)

```bash
# Only one TreeWrite implementation exists in the workspace:
$ rg "impl TreeWrite" crates --type rust
crates/flui-tree/src/traits/write.rs:278:impl<I: Identifier, T: TreeWrite<I> + ?Sized> TreeWrite<I> for &mut T {
crates/flui-tree/src/traits/write.rs:305:impl<I: Identifier, T: TreeWrite<I> + ?Sized> TreeWrite<I> for Box<T> {
crates/flui-tree/src/traits/write.rs:414:    impl TreeWrite<ElementId> for TestTree {       # test only
crates/flui-rendering/src/storage/tree.rs:666:impl TreeWrite<RenderId> for RenderTree {        # production

# RenderTree::remove orphans descendants — the same cycle 2 footgun:
$ rg -A 15 "impl TreeWrite<RenderId> for RenderTree" crates/flui-rendering/src/storage/tree.rs | head -20
impl TreeWrite<RenderId> for RenderTree {
    #[inline]
    fn get_mut(&mut self, id: RenderId) -> Option<&mut Self::Node> { ... }
    fn insert(&mut self, node: Self::Node) -> RenderId { ... }
    fn remove(&mut self, id: RenderId) -> Option<Self::Node> {
        // Update root if removing root
        if self.root == Some(id) { self.root = None; }
        // Get parent and remove from parent's children
        if let Some(parent_id) = self.get(id).and_then(|n| n.parent())
            && let Some(parent) = self.nodes.get_mut(parent_id.get() - 1)
        {
            parent.remove_child(id);
        }
        self.nodes.try_remove(id.get() - 1)  # ← NO CASCADE
    }
}

# LayerTree + SemanticsTree implement TreeRead + TreeNav only:
$ rg "impl TreeRead|impl TreeNav" crates --type rust | grep -v "flui-tree/" | head -10
crates/flui-layer/src/tree/tree_traits.rs:19:impl TreeRead<LayerId> for LayerTree {
crates/flui-layer/src/tree/tree_traits.rs:50:impl TreeNav<LayerId> for LayerTree {
crates/flui-rendering/src/storage/tree.rs:639:impl TreeRead<RenderId> for RenderTree {
crates/flui-rendering/src/storage/tree.rs:704:impl TreeNav<RenderId> for RenderTree {
crates/flui-semantics/src/tree.rs:424:impl TreeRead<SemanticsId> for SemanticsTree {
crates/flui-semantics/src/tree.rs:457:impl TreeNav<SemanticsId> for SemanticsTree {

# Parallel per-tree mutation API surface (cycle 2 PR #100 work, never consolidated):
$ rg "pub fn add_child\b|pub fn remove\b|pub fn detach\b|pub fn set_parent\b|pub fn remove_shallow\b" crates/flui-layer/src/tree/layer_tree.rs crates/flui-semantics/src/tree.rs
crates/flui-layer/src/tree/layer_tree.rs:141:    pub fn set_parent(&mut self, parent: Option<LayerId>) {
crates/flui-layer/src/tree/layer_tree.rs:160:    pub fn add_child(&mut self, child: LayerId) {           # LayerNode::add_child
crates/flui-layer/src/tree/layer_tree.rs:171:    pub fn remove_child(&mut self, child: LayerId) {        # LayerNode::remove_child
crates/flui-layer/src/tree/layer_tree.rs:477:    pub fn remove(&mut self, id: LayerId) -> Option<LayerNode> {      # cascade-by-default (cycle 2)
crates/flui-layer/src/tree/layer_tree.rs:523:    pub fn remove_shallow(&mut self, id: LayerId) -> Option<LayerNode> {  # opt-out (cycle 2)
crates/flui-layer/src/tree/layer_tree.rs:559:    pub fn add_child(&mut self, parent_id: LayerId, child_id: LayerId) {  # LayerTree::add_child (cycle 2 auto-detach)
crates/flui-layer/src/tree/layer_tree.rs:646:    pub fn remove_child(&mut self, parent_id: LayerId, child_id: LayerId) {
crates/flui-semantics/src/tree.rs:170:    pub fn remove(&mut self, id: SemanticsId) -> Option<SemanticsNode> {  # cascade
crates/flui-semantics/src/tree.rs:210:    pub fn remove_shallow(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
crates/flui-semantics/src/tree.rs:240:    pub fn add_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
crates/flui-semantics/src/tree.rs:324:    pub fn remove_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
# This is the parallel-API duplication T-2 finding wants to consolidate.
```

### A.5 — Depth constant drift (T-10)

```bash
# Four different declarations of "tree depth":
$ rg "MAX_DEPTH|MAX_STACK_DEPTH|STACK_SIZE|PATH_BUFFER_SIZE|MAX_TREE_DEPTH|ROOT_DEPTH" crates/flui-tree/src --type rust | head -20
crates/flui-tree/src/depth.rs:68:pub const MAX_TREE_DEPTH: usize = 256;                         # global cap
crates/flui-tree/src/depth.rs:71:pub const ROOT_DEPTH: usize = 0;
crates/flui-tree/src/traits/nav.rs:42:    const MAX_DEPTH: usize = 32;                         # TreeNav stack hint
crates/flui-tree/src/traits/nav.rs:53:    const PATH_BUFFER_SIZE: usize = Self::MAX_DEPTH;     # derived from MAX_DEPTH
crates/flui-tree/src/visitor/mod.rs:137:    const MAX_STACK_DEPTH: usize = 64;                # TreeVisitor stack hint
crates/flui-tree/src/visitor/mod.rs:178:    const STACK_SIZE: usize = 48;                     # TreeVisitorMut stack hint

# Hard-coded inline-32 in descendants:
$ rg "SmallVec\[.*; (8|16|32|48|64)\]" crates/flui-tree/src --type rust | head -10
crates/flui-tree/src/iter/descendants.rs:12:type DescendantStack<Id> = SmallVec<[Id; 32]>;       # hard-coded 32
crates/flui-tree/src/iter/descendants.rs:126:type DescendantDepthStack<Id> = SmallVec<[(Id, usize); 32]>;
crates/flui-tree/src/iter/descendants.rs:108:        let children: SmallVec<[I; 8]> = self.tree.children(current).collect();   # hard-coded 8
crates/flui-tree/src/iter/path.rs:105:    segments: SmallVec<[I; 8]>,                          # TreePath inline-8
crates/flui-tree/src/iter/slot.rs:74-78: # Slot fields (Option<I> niche-optimized)

# Cycle 2 PR #101 review took out hard depth caps in flui-layer mark-propagation:
$ git log --oneline -10 main | grep -i depth | head -3
# (no direct depth-cap commits visible in log; cycle 2 fix was in PR #101 review followup)
```

### A.6 — Production-path panic / TODO scan (clean)

```bash
# TODO / FIXME / XXX in target crates: 0
$ rg "TODO|FIXME|XXX:" crates/flui-foundation/src crates/flui-tree/src
# (no hits)

# unimplemented! / todo! / panic! in target crates production paths
$ rg "unimplemented!|todo!|panic!" crates/flui-foundation/src crates/flui-tree/src --type rust | grep -v test
crates/flui-foundation/src/assert.rs:38:            panic!($($arg)+);                         # macro body
crates/flui-foundation/src/assert.rs:43:            panic!(concat!("Assertion failed: ", stringify!($cond)));   # macro body
crates/flui-tree/src/arity/accessors.rs:193:        panic!("Never type operations are impossible")     # Never accessor
crates/flui-tree/src/arity/accessors.rs:209:        panic!("This operation should never be called - it's impossible by design")
crates/flui-tree/src/arity/types.rs:90:        panic!("Leaf nodes cannot have children - this operation is impossible")
# All panics are in unreachable Never-accessor or Leaf-impossible code paths.
# Constitution Principle 6 clean (no production unimplemented! / todo!).

# Unsafe usage (id.rs + key.rs — documented, narrow)
$ rg "\bunsafe\b" crates/flui-foundation/src crates/flui-tree/src --type rust
crates/flui-foundation/src/id.rs:109:    pub const unsafe fn zip_unchecked(...) { ... }
crates/flui-foundation/src/id.rs:215:    pub const unsafe fn from_raw(...) { ... }
crates/flui-foundation/src/id.rs:248:    pub const unsafe fn zip_unchecked(...) { ... }
crates/flui-foundation/src/id.rs:291:    pub const unsafe fn new_unchecked(...) { ... }
crates/flui-foundation/src/id.rs:608: # serde deserialize uses Self::from_raw(raw) — guarded by validated RawId
crates/flui-foundation/src/key.rs:110:        Self(unsafe { NonZeroU64::new_unchecked(non_zero) })   # safety: guarded
crates/flui-foundation/src/key.rs:153:        Self(unsafe { NonZeroU64::new_unchecked(id) })          # safety: assert guards
# All unsafe is narrow + documented. flui-tree has 0 unsafe (clean).
```

### A.7 — port-check.sh

```bash
$ bash scripts/port-check.sh -v
ok    1: RwLock<Box<dyn ...>> in render/view/layer/painting/engine crates
ok    2: Box<dyn ...> wrapped in interior-mutability primitive in render/view/layer/painting/engine storage
ok    3: async fn build/layout/paint/perform_layout/composite/render/submit/present/render_scene/render_layer_recursive/handle_backdrop_filter/fire_composition_callbacks in render/layer/engine hot path
ok    4: Mutex on dirty-list state in flui-rendering production code
ok    5: Arc::clone in per-frame paint/composite loop
ok    6: Box<dyn View> stored as a struct field in element child collections
ok    7: Arc<(Mutex|RwLock)<*Renderer|*Pool|wgpu::*>> struct field in flui-engine wgpu module
port-check: all seven refusal triggers clean
```

### A.8 — Cross-crate consumer count (orientation)

```bash
$ rg "use flui_foundation::" crates --type rust | grep -v "flui-foundation/" | wc -l
167

$ rg "use flui_tree::" crates --type rust | grep -v "flui-tree/" | wc -l
34

# flui-foundation is the most-imported workspace crate (167 use sites) — but only specific symbols are used.
# flui-tree is the lower-stack abstraction (34 use sites) — but the proportion of consumed surface is much smaller.
```

### A.9 — Cycle reference graph

```text
Cycle 1 (this audit's predecessor-predecessor): flui-interaction × flui-scheduler
  Pattern artifacts:
    PR #93 deletion of typestate.rs (232 LOC, 0 consumers) — direct template for T-3, T-4
    PR #95 Ticker absorb + dispose pattern — sister of I-3 (BindingBase fix)
    PR #96 PointerId widening to ui_events crate — context for "internal slab IDs stay in foundation"
    PR #97 FocusManager singleton dual-state consolidation — sister of T-2 (parallel-API consolidation)

Cycle 2 (this audit's direct predecessor): flui-layer × flui-semantics
  Pattern artifacts:
    PR #100 U8 LayerNode::Drop + disposed AtomicBool — direct template for any binding-disposal work
    PR #100 U12 LayerTree::remove cascade — DIRECT template for T-1
    PR #100 U13 SemanticsTree::remove cascade — DIRECT template for T-1
    PR #100 U10 LayerTree::add_child auto-detach — direct template for TreeWriteNav default impl on T-2
    PR #101 MARK_PROPAGATION_MAX_DEPTH removal — DIRECT template for T-10 (depth constant unification)

Cycle 3 (this audit): flui-foundation × flui-tree
  P0 list above explicitly mirrors cycle 2's structural fixes at the trait-contract level.
  Cycle 2 did the "fix at the impl" pass; cycle 3 does the "fix at the contract" pass.
```

### A.10 — Workspace state at audit time

- Worktree: `determined-proskuriakova-d2eccf`
- Branch: `docs/2026-05-22-flui-foundation-tree-audit` (branched from `origin/main` at `64e5ec30`)
- HEAD on `origin/main` at audit time: `64e5ec30` (cycle 2 PR #101 followup landed)
- Rust edition: 2024; minimum rust-version: 1.94
- `crates/flui-foundation` ACTIVE in workspace `Cargo.toml` members. Dependencies: `bitflags`, `parking_lot`, `thiserror`, `tracing`. Optional: `serde`, `serde_json`. Dev-deps: `flui-types`.
- `crates/flui-tree` ACTIVE in workspace. Dependencies: `flui-foundation`, `thiserror`, `tracing`, `smallvec`, `ambassador`. Optional: `serde` (gated on `flui-foundation/serde`).
- Reverse dependencies (foundation consumers — workspace): flui-scheduler, flui-interaction, flui-rendering, flui-layer, flui-semantics, flui-view, flui-animation (disabled), flui-app, flui-platform.
- Reverse dependencies (tree consumers — workspace): flui-rendering (TreeRead/Nav/Write + arity markers), flui-layer (TreeRead/Nav), flui-semantics (TreeRead/Nav), flui-view (IndexedSlot only).

---

## Status (closed — all waves landed)

**Wave 1+2 PR**: [#103](https://github.com/vanyastaff/flui/pull/103) MERGED.
**Wave 3 PR**: [#104](https://github.com/vanyastaff/flui/pull/104) MERGED.
**Wave 4+5 PR (this PR)**: pending.

Cycle 3 closed across **4 PRs / 11 commits**. The architectural
keystone (T-1+T-2 cascade contract) shipped in Wave 1+2. Wave 3
landed P1 hot-path/safety polish + the PR #103 Codex P2 followup.
Wave 4+5 lands the remaining P1 hygiene items (T-9 streaming slot,
T-13 error unification, T-14 Identifier asymmetry, I-11
non_exhaustive) AND the mega-delete of T-4..T-8 (~10,000 LOC zombie
visitor/diff/iter/arity-storage surface).

Per memory `no-quick-wins-vanyastaff`, the audit's "feature-gate"
prescription for T-4..T-8 was replaced with outright deletion: feature-
gated dead code is still maintenance burden + still CI-compile
overhead. Future devtools needs port from git history.

### Finding disposition (Wave 1+2)

| Audit § | Finding | Severity | Disposition |
|---------|---------|----------|-------------|
| I-1 | `ObserverList<T>` zero-consumer module | P0 | **Closed (deleted)** |
| I-2 | `FoundationError` + `ErrorContext` zero-consumer | P0 | **Closed (deleted)** |
| I-13 | `consts.rs::approx_equal*` zero-consumer | P2 | **Closed (deleted)** |
| I-14 | `assert.rs::report_error!`/`report_warning!` zero-consumer | P2 | **Closed (deleted)** |
| I-22 | `WasmNotSend` zero-consumer | P3 | **Closed (deleted)** |
| T-3 | `state.rs` Mountable/Unmountable typestate (616 LOC) | P0 | **Closed (deleted)** |
| I-3 | `BindingBase` `INITIALIZED.store` re-init-after-panic hazard | P0 | **Closed (fixed; +regression test)** |
| T-1 | `TreeWrite::remove` non-cascade footgun | P0 | **Closed (cascade-by-default trait contract)** |
| T-2 | `LayerTree`/`SemanticsTree` parallel mutation APIs | P0 | **Closed (consolidated into trait impl)** |
| T-10 | Four-way `MAX_TREE_DEPTH` drift | P1 | **Closed Wave 3 (single source via `INLINE_TREE_DEPTH`)** |
| T-11 | `Descendants::next` recursion | P1 | **Closed Wave 3 (loop, no recursion)** |
| T-12 | `Ancestors::next` cycle check | P1 | **Closed Wave 3 (step counter bounded by tree size)** |
| I-4 | `ChangeNotifier::notify_listeners` per-frame alloc | P1 | **Closed Wave 3 (SmallVec inline cap 4)** |
| I-5 | `Default for Key` + `UniqueKey` surprising semantics | P1 | **Closed Wave 3 (impls deleted)** |
| PR #103 Codex P2 | `TreeWrite::remove` unbounded recursion | review | **Closed Wave 3 (iterative cascade + 2k-deep regression test)** |
| T-4 | Visitor surface zombie cleanup | P0 | **Closed Wave 4+5 (deleted ~2,560 LOC)** |
| T-5 | `diff.rs` zombie | P0 | **Closed Wave 4+5 (deleted 1,234 LOC)** |
| T-6 | `iter/cursor` + `path` + `breadth_first` + `depth_first` | P0 | **Closed Wave 4+5 (deleted ~3,800 LOC)** |
| T-7 | `arity/{storage,arity_storage,accessors,runtime,aliases}` | P0 | **Closed Wave 4+5 (deleted ~3,000 LOC; markers + simplified Arity trait kept)** |
| T-8 | `traits/node.rs::Node` trait | P1 | **Closed Wave 4+5 (deleted 305 LOC)** |
| T-9 | `TreeNav::slot` per-call alloc | P1 | **Closed Wave 4+5 (streaming pass, no `Vec` allocation)** |
| T-13 | `TreeError::ArityViolation` unification | P1 | **Closed Wave 4+5 (`#[from] ArityError` variant)** |
| T-14 | `Identifier::From<Index>` `#[cfg(test)]` asymmetry | P1 | **Closed Wave 4+5 (always available)** |
| I-11 | `#[non_exhaustive]` on `DiagnosticLevel` + `DiagnosticsTreeStyle` | P2 | **Closed Wave 4+5** |
| I-16 | `ListenerCallback` explicit `+ 'static` | P3 | **Closed Polish PR** |
| I-19 | `ParseDiagnostic*Error` `Box<str>` instead of `String` | P3 | **Closed Polish PR** |
| I-20 | `ValueNotifier::into_value` calls `dispose()` before drop | P3 | **Closed Polish PR** |
| T-16 | `TreeError::Internal(Box<str>)` | P2 | **Closed Polish PR** |
| T-18 | `lowest_common_ancestor` `SmallVec<[I; INLINE_TREE_DEPTH]>` | P2 | **Closed Polish PR** |
| T-20 | `Siblings::new` `SmallVec` | P2 | **Closed Polish PR** |
| T-21 | `Leaf::first_impossible` delete | P3 | **Closed (obsolete) — entire `types.rs` rewritten in Wave 4+5; method no longer exists** |
| T-22 | `Never::default` via `#[derive]` | P3 | **Closed (obsolete) — rewrite in Wave 4+5 uses `#[derive(Default)]`** |
| T-23 | `Arity` associated constants moved | P3 | **Closed (obsolete) — those constants deleted with arity storage in Wave 4+5** |
| T-25 | `DepthFirstOrder` enum unused | P3 | **Closed (obsolete) — `depth_first.rs` deleted in Wave 4+5** |
| T-15 | `TreeReadExt` + `TreeNavExt` + `MountableExt` feature-gate | P2 | **Closed (partial obsolete) — `MountableExt` deleted with `state.rs`; `TreeReadExt`/`TreeNavExt` kept as the public extension surface (have real-world ergonomic value)** |

### Findings deferred — judgment-call / design-needed

These are truly *deferred*, not just aesthetic. Each requires a
design decision that is out-of-scope for a polish PR:

| Audit § | Finding | Severity | Reason |
|---------|---------|----------|--------|
| I-6 | `Key::from_str` collision-with-zero fallback | P1 | Cosmetic; the proper fix is `try_from_str` returning `Option`, which is an API extension worth its own RFC. The pre-cycle `if hash == 0 { 1 } else { hash }` is correct on the type-safety side (always non-zero); the silent collision is a hash-function property, not a flaw in the wrapper. |
| I-7 | `Key::try_new` Result constructor | P1 | Adds new public API. Defer until a real overflow-recovery callsite materializes. |
| I-8 | `ViewKey::is_global_key()` abstract | P1 | Forcing 3+ key impls to write explicit `false` is more noise than safety; the default-false safety net catches the "forgot to override" case identically. |
| I-9 / I-10 | `Id<T>::from_raw` / `zip_unchecked` / `new_unchecked` / `RawId` / `Index` `pub(crate)` | P2 | `flui-scheduler::id::*` actively re-exports these. Locking them down would break the scheduler's public API contract. |
| I-12 | Sweep doc-comments to cite Flutter file:line refs uniformly | P2 | ~50 LOC doc churn across multiple files — better done as a dedicated doc PR with proper Flutter source verification. |
| I-15 | `ChangeNotifier::has_listeners` / `is_empty` / `len` via lock-free `AtomicUsize` | P3 | Adds a parallel atomic counter that must stay in sync with the HashMap on every add/remove. Risk of drift > benefit (the current `Mutex::lock` is uncontended in the steady-state read path). |
| I-17 | `ValueNotifier::take` / `replace` / `value_mut` audit | P3 | Used by tests + internal consumers; judgment call on which to drop. |
| I-18 | `Marker` trait drop `+ Debug` requirement | P3 | Removing requires touching every concrete marker; cost > benefit. |
| I-21 | Deprecate `KeyRef::new` in favor of `From<Key>` | P3 | Both call sites exist; deprecation has a migration cost. |
| T-17 | `Slot::with_siblings` builder via `bon` | P2 | Builder pattern conversion is its own commit theme. |
| T-19 | `TreeNav::depth` slow default doc | P2 | Doc-only. |
| T-24 | `Descendants::new` / `Ancestors::new` etc. `pub(crate)` | P3 | Re-exported via `flui_tree::*` already; reducing visibility breaks the iter API. |

### Aggregate cycle 3 impact (final)

- **~12 commits** across **5 PRs** (#102 audit doc, #103 Wave 1+2,
  #104 Wave 3, #105 Wave 4+5, plus this Polish PR).
- **~−11,600 LOC** net surface reduction across foundation +
  flui-tree.
- **+7 regression tests**.
- **Findings closed**: 34 of 47 cataloged + 1 PR #103 review
  (Codex P2). Remaining 13 are explicit design-needed deferrals
  (see table above) — not aesthetic gaps but judgment calls or
  API extensions worth their own dedicated work.
- **The cascade-contract** (T-1+T-2) is the architectural keystone
  cycle 2 anticipated and cycle 3 delivered. Memory
  `flui-tree-unified-interface-intent` closed for the mutation
  surface.

### Aggregate cycle comparison (all three closed)

| Cycle | Scope (LOC) | Findings | Commits | Net LOC delta |
|-------|-------------|----------|---------|---------------|
| 1 (PR #85-#98) | 12,360 | 16 + drift | ~30 across 14 PRs | −2,400 |
| 2 (PR #100-#101) | 15,571 | 25 | 16 + 1 followup | +1,690 (new lifecycle infra) |
| 3 (PR #102-#106) | 23,448 | 47 (34 closed + 13 deferred-by-design) | 12 across 5 PRs | **−11,600** (largest reduction) |

Cycle 3 reduced flui-tree's public surface by ~58% (the audit's
zombie-surface estimate) while making the cascade-contract,
iterator safety, and `BindingBase` init the keystones of the
foundation layer. Future devtools requirements can re-introduce
deleted abstractions alongside their first real consumers.

---

*End of audit. Wave 1+2 closed. Follow-up PRs land the deferred items.*
