# ADR-0040: Tree observation seam — dependency-inverted devtools access

- **Status:** Accepted
- **Date:** 2026-07-28

*`flui-foundation` declares a narrow `TreeObserver` trait plus typed event structs; `BuildOwner`
holds one per-realm `Option<Arc<dyn TreeObserver>>` slot; emissions fire from the tree's true
mutation funnels (mint, move, rebuild drain, the two unmount primitives); `flui-devtools`
implements the trait behind its `inspector` feature with `flui-foundation` as its only tree-side
dependency. The `flui::reconcile` tracing stream (FR-035) stays, but is not the seam.*

## Context

`flui-devtools` could not observe anything: no dependency path reached `flui-view`,
`flui-rendering` or `flui-foundation`. The fix is dependency inversion — the core publishes
observations through a trait declared low in the DAG, devtools subscribes. This ADR decides the
seam, not the inspector.

What the decision builds on:

- **Typed rebuild causes.** `RebuildReason` / `RebuildReasons` already name why each rebuild was
  scheduled, written as cross-crate tooling vocabulary.
- **A tracing stream with devtools ambitions.** The keyed reconciler emits structured
  `flui::reconcile` events (Mount, Unmount, Reuse, Reorder, Reparent). Whether that stream *is*
  the seam is §1's question.
- **Ownership.** The element tree and its `BuildOwner` are realm-owned (`WidgetsBinding` inside
  each `UiRealm`, ADR-0027); the render pipeline is process-hosted. Hence this ADR commits only to
  element-tree observation.
- **DAG position.** `flui-view`, `flui-rendering` and `flui-scheduler` all sit above
  `flui-foundation`; a vocabulary all of them must speak can only live there.
- **The true lifecycle funnels.** Fresh elements are minted in `ElementTree::insert`'s create path
  and `mount_root*` (`insert`'s GlobalKey-retake branch returns an existing element before the
  create path). Moves are GlobalKey retakes and keyed reorders. Every rebuild drains through
  `BuildOwner::build_scope`, which runs several times per frame (main build, mid-layout
  `LayoutBuilder` settling, post-layout child servicing). Unmounts are split: un-keyed elements
  unmount eagerly inline during reconcile; only GlobalKey elements enter the inactive queue that
  `finalize_tree` drains (itself called mid-cycle, not only at frame end). Every unmount path
  bottoms out in two primitives, `ElementTree::remove` (eager branch) and
  `ElementTree::remove_finalized`.

## Decision

### 1. The seam is a trait, not the tracing stream

The tracing stream is on the production path, has a stability contract (FR-035) and excellent
zero-cost-when-off behavior; it stays for trace tooling and test assertions. It is the wrong
devtools seam because:

1. **Typed payloads degrade to primitives.** `TypeId` crosses only as a `Debug` string; scaling
   to a typed per-node inspector means re-encoding the type system through strings.
2. **Subscriber lifecycle is process-global and race-prone.** Installing/dropping a dispatcher
   rebuilds tracing-core's global callsite interest cache; the repo's tests have hit that race
   twice. A panel attaching at runtime is that cycle in production.
3. **No realm scoping.** A dispatcher is global or thread-scoped; "observe *this* realm" has no
   expression under ADR-0027.
4. **Event-only.** A trait registered on the owner is a natural anchor for a later query surface.
5. **Enabled-path overhead.** Erased `Visit` dispatch per field vs. one virtual call with typed
   arguments.

The seam is `flui_foundation::observe::TreeObserver`. Folding `flui::reconcile` dispositions into
it (they overlap at the mount/move sites) is a separate, explicit decision.

### 2. The vocabulary lives in `flui-foundation`; `RebuildReason` moves down

`flui-foundation` already holds the tree IDs, has no flui dependencies, and every emitter depends
on it. `RebuildReason` / `RebuildReasons` moved there and are re-exported from `flui_view::owner`;
`RebuildReasons::{insert, merge}` became `pub` (value-semantic unions on a `Copy` bitset), and its
rustdoc states the snapshot-not-guard contract without an upward link to `BuildOwner`.

```rust
// crates/flui-foundation/src/observe.rs (shape; each struct also has a `new` constructor)
#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementMounted {
    pub element: ElementId,          // generational: ABA-safe as a devtools map key
    pub parent: Option<ElementId>,   // None for the root
    pub slot: usize,
    pub view_type_id: TypeId,        // logical view type (BoxedView forwards to its inner view)
}
#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementMoved { pub element: ElementId, pub parent: ElementId, pub slot: usize }
#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementRebuilt { pub element: ElementId, pub view_type_id: TypeId, pub reasons: RebuildReasons }
#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementUnmounted { pub element: ElementId }

pub trait TreeObserver: Send + Sync {
    fn element_mounted(&self, event: &ElementMounted) { let _ = event; }
    fn element_moved(&self, event: &ElementMoved) { let _ = event; }
    fn element_rebuilt(&self, event: &ElementRebuilt) { let _ = event; }
    fn element_unmounted(&self, event: &ElementUnmounted) { let _ = event; }
    /// End of stream: replaced, cleared, or detached after a panic (§6).
    fn detached(&self) {}
}
```

- `ElementRebuilt` fires only for builds that completed (including builds recovered into an
  `ErrorView`). First builds are included: every mount is followed by a rebuilt event whose
  reasons contain `InitialMount`.
- **Defaulted methods, not an event enum:** a mega-enum couples every emitter domain into one type
  and forces consumers through a `match`; defaulted methods add observations without breaking
  implementors.
- **`#[non_exhaustive]` + `new`:** protects matchers; constructors are in-workspace only, so
  appending a field is not a consumer break.
- **`Send + Sync`:** emission happens on the realm's owner thread, while a devtools reader (or one
  collector shared by several realms) lives elsewhere.

### 3. Registration is per-realm, one slot, on `BuildOwner` — with a seeded install for mid-run attach

```rust
impl BuildOwner {
    pub fn set_tree_observer(&mut self, observer: Arc<dyn TreeObserver>); // replaced one gets detached()
    pub fn clear_tree_observer(&mut self);                                 // fires detached(); idempotent
    pub fn tree_observer(&self) -> Option<Arc<dyn TreeObserver>>;          // clones the Arc, no guard
}
impl ElementTree {
    /// Replay the live tree as synthetic `ElementMounted`, pre-order from the root
    /// (parked inactive keyed elements are not replayed).
    pub fn replay_mounts(&self, observer: &dyn TreeObserver);
}
impl WidgetsBinding {
    /// Under ONE `inner` write guard: replay, then install. Frame drives hold the same
    /// lock, so no mutation interleaves and the consumer's baseline is exact.
    pub fn install_tree_observer(&self, observer: Arc<dyn TreeObserver>);
    pub fn remove_tree_observer(&self);
}
```

- **Mid-run attach is first-class.** Without a baseline an event-only seam is useless to a panel
  attached to a running app. Replay covers structure, parent/slot edges and logical types; state
  versions and dependency edges are pull-shaped and out of scope.
- **Per-realm by construction.** The slot belongs to one realm's `BuildOwner`; events carry no
  `RealmId`. A multi-realm consumer installs a thin per-realm wrapper tagging a shared sink.
  Flutter is not the reference for this topology (ADR-0027).
- **One `Option` slot, not a `Vec`.** Fan-out composes in the consumer; replace semantics make a
  later multi-slot widening additive.
- **Teardown honesty.** Nothing synthesizes `element_unmounted` for elements alive at teardown
  (`detach_root_widget` removes only the root node; a realm drop drops the slab). `detached()` is
  the end-of-life signal. A `BuildOwner` dropped without `clear_tree_observer` emits no
  `detached()` — embedders that install must clear (install/clear symmetry).
- **Not a `BuildContext` capability.** The observer is embedder infrastructure, deliberately absent
  from `BuildContext`/`LifecycleContext`, so widgets cannot reach it at all.
- **Plumbing.** `ElementOwner` carries `&mut Option<Arc<dyn TreeObserver>>` so the tree primitives
  can emit; `&mut` because the panic policy (§6) clears the slot from inside an emission.

### 4. Emission sites and the ordering contract

| Event | Site | Covers |
|---|---|---|
| `ElementMounted` | `ElementTree::insert` create path (next to the `flui::reconcile` Mount emission); `mount_root*` with `parent: None`. The retake early-return never reaches it. | every fresh mint, from any build pass |
| `ElementMoved` | both GlobalKey retake paths (next to their `Reparent` emissions); the keyed-reorder branches that rewrite slot metadata | GlobalKey reparents and every keyed slot change |
| `ElementRebuilt` | `build_scope`'s drain loop, after the element is put back and the outcome is known `Ok`, before the child reconcile — a parent's event precedes its children's mutations | every completed build from every `build_scope` call |
| `ElementUnmounted` | `ElementTree::remove` (eager branch) and `ElementTree::remove_finalized`, after `Element::unmount` | inline reconcile removals, `finalize_tree` drains, lazy-sliver eviction, root detach |

**Invariant (the test oracle):** `ElementUnmounted` fires exactly once per `Element::unmount`,
`ElementMounted` exactly once per fresh mint, so with no soft-removed elements pending,
mounts − unmounts equals live-tree size. The soft-remove branch of `remove` emits nothing: the
element later resurfaces as `ElementMoved` (retaken) or `ElementUnmounted` (finalized).

Every site emits through one private helper, `flui_view::owner::emit_observation`, which builds
the payload lazily inside a closure and runs the call under `catch_unwind` (§6).

**Ordering:** the stream is a totally ordered tree-mutation log, emitted synchronously on the
realm's owner thread in mutation order. Per element: mounted first, then any interleaving of
rebuilt/moved, then unmounted last. **No phase bucketing is promised** — builds run mid-layout and
post-layout, eager unmounts happen inline, `finalize_tree` runs several times a frame. Per-frame
buckets need frame-demarcation events (a follow-up). Known approximation: between a keyed
element's soft-remove and its retake/finalize, a mirror still shows it under its last parent;
the window closes within the same frame drive.

### 5. Zero-cost-when-off: `Option` null check, no cargo feature in core

With no observer installed, each site costs one load of a niche-optimized `Option<Arc<dyn _>>`
and a predictable branch — no payload, no `catch_unwind`, no allocation or locking. That is not
literally zero like `#[cfg]`-removed code, and this ADR does not claim it is; it is the same order
as the adjacent tracing interest check. With an observer installed, each emission also pays the
`catch_unwind` frame. The claim is checked by `crates/flui-testing/benches/tree_observer_overhead.rs`
(observer absent vs. no-op vs. counting, over a rebuild-heavy tree).

Emission is unconditionally compiled; the runtime `Option` is the per-realm, per-run off switch.

### 6. Threading, non-reentrancy, and the panic policy

Callbacks run on the realm's owner thread, inside frame phases, with the realm's locks held.

1. **Same-thread deadlock.** Emissions run while the frame drive holds the binding's `inner`
   write lock; every binding accessor (`with_build_owner`, `with_element_tree`, `root_element`)
   takes `inner.read()` on a non-reentrant `parking_lot` lock. A callback must not call any
   flui-view binding, realm or owner API. It may touch only its own state (atomics, lock-free
   structures, `try_send` into its own channel) and must return promptly.
2. **No scheduling into the emitter.** Synchronous re-entry into the build is a debug assertion,
   which covers neither the deadlock nor release builds. A smuggled `RebuildHandle` called from a
   callback is not statically prevented; inspection-driven mutation must go through out-of-frame
   channels (post-frame callbacks, a handle used outside the callback).
3. **A panicking observer is detached; the frame survives.** An unwind in the window where an
   element is taken out of the slab would leave a permanent hole. So (a) no emission sits inside
   such a window — `element_rebuilt` fires after the element is put back — and (b) every call runs
   under `catch_unwind`; on unwind the observer is removed from the slot, `detached()` is *not*
   called, and a `tracing::error!` records it. A panic during `replay_mounts` aborts the install
   and nothing is registered. Consistent with `docs/PANIC-POLICY.md`: consumer panics are
   contained, reported, and disarmed.

### 7. A sanctioned `dyn` boundary

`Arc<dyn TreeObserver>` is a framework `dyn` boundary in the observer-pattern family (alongside
`WidgetsBindingObserver`). Dependency inversion requires the erasure: emitter crates cannot name a
devtools type above them in the DAG.

### 8. The devtools half: `inspector` feature, foundation-only dependency

`flui-devtools`'s `inspector` feature (on by default in that crate; release builds avoid devtools
cost by not depending on the crate) pulls in `flui-foundation` and `tracing` and nothing from the
tree stack. It compiles for `wasm32-unknown-unknown`. `flui_devtools::inspector::InspectorCounters`
is a counting/logging observer over private atomics; `snapshot()` returns an `InspectorSnapshot`
(`mounts`, `unmounts`, `rebuilds`, `moves`, `rebuilds_for(RebuildReason)`, `is_final()`). Each
counter is individually monotonic; cross-counter consistency within a snapshot is not guaranteed.

The end-to-end proof needs both halves, so it lives in `flui-testing`
(`tests/tree_observer_inspector.rs`, with `flui-devtools` as a dev-dependency): mount → rebuild →
keyed reorder → GlobalKey reparent → unmount → detach against `InspectorCounters`, asserting exact
counts, reason sets, the §4 balance invariant, and `detached()` on teardown. Tests install via
`WidgetsBinding::install_tree_observer` / `remove_tree_observer`.

## Alternatives rejected

1. **Tracing stream as the seam** — §1; kept for trace tooling under FR-035.
2. **`flui-devtools` depends on `flui-view`.** Inverts nothing, compiles the whole tree stack into
   any app enabling inspection, makes any in-framework consumer of a devtools type a cycle, and
   grants unlimited tree access instead of a narrow surface.
3. **An owned-event channel in foundation.** Allocates per event even for a counter; a bounded
   channel backpressures the frame, an unbounded one grows without limit; async delivery loses the
   mutation-order guarantee. An observer that does `try_send` already is a channel with a
   consumer-chosen drop policy.
4. **`ChangeNotifier` / `ListenerRegistry`.** Void/value-status shaped; erases the typed payloads.
5. **A new `flui-observe` crate.** Foundation already is the lowest crate every emitter depends on;
   a new crate adds DAG surface and isolates nothing.
6. **`#[cfg(feature)]`-gated emission in core.** Feature unification makes it always-on in the
   workspace while varying for external consumers; features are compile-time and process-global,
   the requirement is runtime and per-realm (attach to a running app).
7. **`Vec` fan-out in core.** Not needed; composable outside, and widening later is non-breaking.
8. **Observer as a `BuildContext` capability.** Wrong owner (embedder/realm, not widget), and it
   would invite frame-phase acquisition.
9. **Emitting `ElementUnmounted` from `finalize_tree`.** Wrong: un-keyed unmounts never reach the
   inactive queue, so most unmounts would be missed and mirrors would leak.

## Consequences

**Positive**
- Devtools observe the tree with `flui-foundation` as the only framework dependency; the seam is
  inert without an installed observer.
- Typed, realm-scoped, mutation-ordered events; generational `ElementId`s make consumer stores
  ABA-safe; the §4 invariant keeps a mirror's parent/slot edges correct through reparents and
  reorders.
- Mid-run attach starts from an exact structural baseline.
- A misbehaving observer is detached, not frame-fatal.
- The vocabulary sits where render-phase and scheduler-task emitters can reuse it later.

**Negative**
- One `Option` load and branch per lifecycle event with devtools absent; a `catch_unwind` frame
  per emission when present.
- `flui::reconcile` and `TreeObserver` emit overlapping mount/move facts at the same sites —
  different consumers, different contracts, consolidation left explicit.
- The no-reentrancy contract is documentation plus a partial debug assertion, not a static
  guarantee; a panicking observer silently stops receiving events (logged, no `detached()`).
- A realm torn down without `clear_tree_observer` gives no `detached()` — the embedder's
  obligation.

## Out of scope and follow-ups

- **Render-phase observation** needs its own ADR first: the pipeline is process-hosted, so a slot
  on `PipelineOwner` would be process-wide observation wearing a realm-shaped API. The vocabulary
  (defaulted methods carrying `RenderId` + a typed cause) is expected to carry over; placement is
  the open decision.
- **Pull/query inspection** (walk the live tree, state versions, dependency edges, memory) is a
  different seam with a different hazard profile (re-entering realm-owned state); it needs its own
  ADR. This event seam serves the structural subset only.
- **Frame-demarcation events** (build-scope and frame begin/end) so consumers can bucket per frame.
- **Active-task observation** from `flui-scheduler`'s `AsyncDriver`, via new defaulted methods.
- **Wire format / remote protocol** — none defined here.
- A realm-teardown hook that guarantees `clear_tree_observer`, closing the "dropped without
  detach" gap by construction.
