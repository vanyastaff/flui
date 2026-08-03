# ADR-0040: Tree observation seam — dependency-inverted devtools access

*`flui-devtools` gains the ability to observe the widget tree without ever depending on a tree crate: `flui-foundation` (the DAG's lowest crate, which every emitter already depends on) declares a narrow `TreeObserver` trait plus typed event structs; `BuildOwner` holds one per-realm `Option<Arc<dyn TreeObserver>>` slot and emissions fire from the tree's true mutation funnels — mint, move, rebuild-drain, and the two unmount primitives; devtools implements the trait behind a new `inspector` feature that depends only on `flui-foundation`. The existing `flui::reconcile` tracing stream (FR-035) is honestly evaluated and deliberately **not** chosen as the seam, but stays untouched as a stability boundary. Slice 1 is five defaulted observer methods (`element_mounted` / `element_moved` / `element_rebuilt` / `element_unmounted` / `detached`), an install-time replay so mid-run attach starts from a correct baseline, a counting consumer, and the previously-unwritable overhead benchmark.*

---

- **Status:** Proposed (2026-07-28)
- **Date:** 2026-07-28
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-foundation/src/observe.rs` (new module: `TreeObserver`, `ElementMounted`, `ElementMoved`, `ElementRebuilt`, `ElementUnmounted`); `crates/flui-foundation/src/rebuild_reason.rs` (moved from `crates/flui-view/src/owner/rebuild_reason.rs`, re-exported at the old path); `crates/flui-view/src/owner/build_owner.rs` (`BuildOwner::{set_tree_observer,clear_tree_observer,tree_observer}`, `ElementOwner` plumbing, `build_scope` emission); `crates/flui-view/src/tree/element_tree.rs` (mount/move/unmount emissions in `insert`, `mount_root*`, `try_retake_global_key`, `retake_active_global_key`, `remove`, `remove_finalized`; new `replay_mounts`); `crates/flui-view/src/tree/id_reconcile.rs` (move emission at the reorder branches); `crates/flui-view/src/binding.rs` (`WidgetsBinding::{install_tree_observer,remove_tree_observer}`); `crates/flui-devtools` (new `inspector` feature + `flui-foundation` dependency); `crates/flui-testing` (new dev-dependency on `flui-devtools`, end-to-end test in `tests/main.rs`); `scripts/port-check.sh` (FR-036 trigger #9 allowlist: `TreeObserver`)
- **Related:** audit `docs/audits/2026-07-25-upgrade-pack-audit.md` §26 (line 794) and open item U3 (line 1187); ADR-0027 (owner-affine UiRealms — realm-scoped registration is the sanctioned shape); ADR-0034 (install/clear symmetry template); ADR-0030/0031 (capability install precedents); companion drafts ADR-0038 (data transfer), ADR-0039 (event-loop affinity); FR-035 (`flui::reconcile` stability boundary); FR-036 (sanctioned `dyn` registry); `docs/PANIC-POLICY.md` (observer panic policy, §Decision 6)

---

## Context

`flui-devtools` cannot observe anything. Its entire `[dependencies]` section (`crates/flui-devtools/Cargo.toml:11-33`) names `web-time`, `serde`/`serde_json`, `parking_lot`, `windows-sys`, and exactly one flui crate — `flui-hot-reload`, optional (line 21). No dependency path reaches `flui-view`, `flui-rendering`, or `flui-foundation`, so the crate is physically unable to see widgets, elements, or render objects. The audit (§26) verified this at HEAD and scoped the fix: **dependency inversion** — the core publishes observations through a narrow trait declared low in the DAG; devtools subscribes. The audit's 16-field inspector wishlist (per-node id, generation, parents in each tree, component type, state version, dependencies, last rebuild/layout/repaint reason, active tasks, …) is the long-term target; **this ADR decides the seam, not the inspector**.

What already exists, and matters:

- **Typed rebuild causes.** `RebuildReason` / `RebuildReasons` (`crates/flui-view/src/owner/rebuild_reason.rs:18-39,90`) already name *why* every element rebuild was scheduled, explicitly so that "framework tooling can compare causes across all widget crates" (lines 13-14). `BuildOwner::pending_rebuild_reasons` (`build_owner.rs:617`) already exposes cause snapshots — returning values, never guards, per SP-6 (`build_owner.rs:603-606`).
- **A tracing event stream with devtools ambitions.** The keyed reconciler emits structured `flui::reconcile` events (`crates/flui-view/src/tree/reconcile_event.rs`) whose module docs name "the future devtools panel" as an intended subscriber (lines 9-13), with five dispositions — Mount, Unmount, Reuse, Reorder, Reparent (`reconcile_event.rs:49-64`). A test-only `tracing_subscriber::Layer` consumes it (`crates/flui-view/src/tree/test_utils/reconcile_event_collector.rs`). Whether this stream *is* the seam is the central question — §Decision 1 answers it.
- **Per-realm ownership of the element tree; process ownership of the pipeline.** The element tree and its `BuildOwner` live inside `WidgetsBindingInner` (`crates/flui-view/src/binding.rs:446-460`), owned per-realm by the runner's `UiRealm` (`crates/flui-app/src/app/binding.rs:61-62`). The render pipeline is **not** realm-owned: `AppBinding` "owns only services whose state is intentionally process-wide," explicitly including "RendererBinding - Manages render tree and pipeline" (`flui-app/src/app/binding.rs:55-62`), shared into the realm as `Arc<RwLock<PipelineOwner>>` (`flui-view/src/binding.rs:460`). Per ADR-0027 the realm topology is a sanctioned leapfrog zone — Flutter is not the reference here. This split is why this ADR commits only to *element-tree* observation; render-phase observation has a different ownership story (§Slicing, slice 2).
- **The emitters' DAG position.** `flui-view` sits *above* `flui-rendering` (`crates/flui-view/Cargo.toml:22`), and both sit above `flui-foundation` (`crates/flui-view/Cargo.toml:18`, `crates/flui-rendering/Cargo.toml:26`), as does `flui-scheduler` (`crates/flui-scheduler/Cargo.toml:15`). A vocabulary that build-phase (flui-view), render-phase (flui-rendering), and — later — task-phase (flui-scheduler) emitters must all speak can only live in `flui-foundation`.
- **The true lifecycle funnels.** *Minting:* fresh child elements are created in `ElementTree::insert`'s create path (`element_tree.rs:614-707`) and the root paths `mount_root`/`mount_root_with_pipeline_owner` (`element_tree.rs:506,537`) — but `insert` is **not** mint-only: its GlobalKey-retake branch returns an existing element *before* the create path runs (`element_tree.rs:626-630`). *Moves:* GlobalKey retakes (`try_retake_global_key` at `element_tree.rs:1508`, both the inactive-retake and active-cross-parent branches, which already emit `Reparent` at `element_tree.rs:1629-1636,1722-1728`) and keyed reorders (`id_reconcile.rs:269-271,304-306`, slot metadata refreshed by `set_child_slot`, `id_reconcile.rs:619-623`). *Rebuilds:* all of them drain through `BuildOwner::build_scope` (`build_owner.rs:647`), which already wraps each rebuild in an `element_rebuild` span (`build_owner.rs:761-766`) — and `build_scope` runs **several times per frame**: the frame's main build, mid-layout-settling from `service_layout_builders` (`layout_builder.rs:151-161`), and post-layout from `service_child_requests` (`build_owner.rs:1063`). *Unmounts are split* (`element_tree.rs:1162-1188`): un-keyed elements unmount **eagerly, inline** during reconcile ("Eager (un-keyed): `Element::unmount` then slab-remove. No deferred queue entry … ordinary unmounts are processed inline", `element_tree.rs:1177-1182`; the reconciler frees un-keyed subtrees immediately via `remove_finalized`/`remove`, `id_reconcile.rs:563-576`); only GlobalKey-carrying elements enter the inactive queue that `finalize_tree` drains (`build_owner.rs:1122-1187`) — and `finalize_tree` itself runs mid-cycle too (`layout_builder.rs:176`; twice inside `service_child_requests`, `build_owner.rs:955-957,1088`), not only at end of frame. Every unmount, from every path — reconciler removal, `finalize_tree`, lazy-sliver eviction via `remove_subtree` (`element_tree.rs:1243-1326`), `detach_root_widget` (`binding.rs:790-803`) — bottoms out in exactly two primitives: `ElementTree::remove` (eager branch) and `ElementTree::remove_finalized` (`element_tree.rs:1336-1365`). Those two funnels, not `finalize_tree`, are where unmount is a fact.
- **Render phases.** `PipelineOwner<Phase: PipelinePhase>` (`crates/flui-rendering/src/pipeline/owner/mod.rs:107`) runs `run_layout` / `run_paint` / `run_compositing` / `run_semantics`, each already opening a `tracing::debug_span!`. These are *potential* future emitters, but their process-wide ownership (above) means their observation topology is a separate decision (§Slicing, slice 2).

## Decision

### 1. The seam is a trait, not the tracing stream

The honest case for "tracing already *is* the seam": the reconcile stream exists on the production path, its target string and field names are a declared stability boundary (FR-035, `reconcile_event.rs:16-22`), and its zero-cost-when-off story is genuinely excellent — tracing's per-callsite interest cache short-circuits before any field expression is evaluated. For grep-able trace tooling and test assertions it is the right instrument, and this ADR does not deprecate it.

It is nonetheless the wrong *devtools* seam, for five verified reasons:

1. **Typed payloads degrade to primitives.** A `tracing::Event` carries `u64`/`bool`/`str` fields; `TypeId` crosses only as a `Debug`-formatted string, which is why the collector maintains a parallel stringly-typed `CollectedEvent` and documents that reconstructing the `TypeId` "is not generally possible" (`reconcile_event_collector.rs:10-18`; field table in `reconcile_event.rs:28-37`). Scaling that to the wishlist's 16 typed per-node fields means re-encoding and re-parsing the framework's type system through strings — the opposite of what a Rust-native seam should do.
2. **Subscriber lifecycle is process-global and race-prone.** Installing or dropping a dispatcher rebuilds tracing-core's process-global callsite interest cache. This repo has been bitten twice *in tests alone*: the collector's docs mandate `#[serial_test::serial]` because "concurrent dispatcher installs/drops race its rebuild — a freshly installed collector can then miss events" (`reconcile_event_collector.rs:49-53`), and a second flake of the same nature in flui-view was fixed by switching to a permanent global subscriber fixture (audit changelog, `2026-07-25-upgrade-pack-audit.md` closing note). A devtools panel attaching and detaching at runtime is exactly this install/drop cycle, in production.
3. **No realm scoping.** A tracing dispatcher is global (or thread-scoped via `with_default`); "observe *this* realm's tree" has no expression. Under ADR-0027's multi-realm model that is a structural mismatch, not a missing feature. (This argument applies to the realm-owned element tree; it deliberately does *not* claim to transfer to the process-hosted pipeline — see §Slicing, slice 2.)
4. **Event-only.** The inspector wishlist has pull-shaped fields (state version, dependency edges). tracing cannot answer a query; a trait registered on the owner is the natural anchor for a later snapshot/query surface (deferred — see §Out of scope, which states plainly what the committed slices do and do not serve).
5. **Enabled-path overhead.** With any subscriber installed, every emission goes through erased `Visit` dispatch per field. A trait method call on a concrete `Arc<dyn TreeObserver>` is one virtual call with typed arguments.

**Decision:** the seam is `flui_foundation::observe::TreeObserver`. The `flui::reconcile` stream remains, unchanged, under its FR-035 contract; whether its dispositions eventually also flow through `TreeObserver` (they overlap at the mount/move sites) is a named follow-up, decided then — not silently now.

### 2. The vocabulary lives in `flui-foundation`; `RebuildReason` moves down

`flui-foundation` is where the tree IDs already live (`src/lib.rs:10-11`), it has no flui dependencies, and every current and future emitter depends on it (§Context). The new `observe` module declares the trait and event structs.

`RebuildReason` / `RebuildReasons` move from `crates/flui-view/src/owner/rebuild_reason.rs` into `flui-foundation`, re-exported from `flui_view::owner` so no call site changes. The type was written for cross-crate tooling vocabulary (lines 13-14) and has zero flui-view coupling, but the move is slightly bigger than "plain enum + bitset": it carries the `pub(crate)` `RebuildReason::ALL` table that `iter()` depends on (`rebuild_reason.rs:42-53`), promotes the `pub(crate)` mutators `RebuildReasons::{insert, merge}` (lines 103-109) to `pub` — value-semantic set unions on a `Copy` bitset, no invariant to protect — and **rewords** the `RebuildReasons` rustdoc, which today references `BuildOwner::pending_rebuild_reasons` (lines 85-88): from foundation that would be an upward layering reference, so the doc states the snapshot-not-guard contract in its own terms instead of linking upward.

```rust
// crates/flui-foundation/src/observe.rs
use core::any::TypeId;
use crate::{ElementId, RebuildReasons};

/// A fresh element was created and mounted into the tree.
///
/// Emitted only when a new element is minted — a GlobalKey retake of an
/// existing element emits [`ElementMoved`] instead, never a second mount.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementMounted {
    /// The new element. Generational (`index` + `generation`,
    /// `id.rs:828-848`), so a devtools store keyed by `as_u64()` can
    /// never confuse a recycled slab slot — the house ABA defense.
    pub element: ElementId,
    /// `None` for the root element (`mount_root*`); `Some` for every
    /// child minted by `ElementTree::insert`.
    pub parent: Option<ElementId>,
    /// Slot index under `parent` (0 for the root).
    pub slot: usize,
    /// `View::view_type_id()` of the configuring view — typed, not a
    /// Debug string. This is the *logical* view type: `view_type_id` is
    /// overridable and `BoxedView` forwards it to its inner view
    /// (`element/dispatch.rs:15-28`), so a `child.boxed()` reports the
    /// inner type, not the wrapper. Devtools grouping is therefore by
    /// logical type — the same identity the reconciler matches on.
    pub view_type_id: TypeId,
}

/// An existing element's position changed: a GlobalKey reparent
/// (possibly across parents) or a keyed reorder under the same parent.
///
/// Consumers holding a mirror update the element's parent/slot edge;
/// the element's state and identity survive.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementMoved {
    /// The element that moved.
    pub element: ElementId,
    /// Its parent after the move (may equal the previous parent for a
    /// same-parent reorder).
    pub parent: ElementId,
    /// Its slot under `parent` after the move.
    pub slot: usize,
}

/// An element's build completed, with its accumulated causes.
///
/// Emitted only for builds that ran to completion (including builds
/// recovered into an `ErrorView` by `build_or_recover` — those
/// completed with substitute output). A build pass that unwinds emits
/// nothing for that element. First builds are included: every mount is
/// followed by a rebuilt event whose reasons contain `InitialMount`,
/// so `mounts + rebuilds` intentionally counts first builds in both —
/// consumers wanting "re-builds only" filter on the reason set.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementRebuilt {
    /// The rebuilt element.
    pub element: ElementId,
    /// Logical view type (same semantics as [`ElementMounted::view_type_id`]).
    pub view_type_id: TypeId,
    /// Every distinct cause accumulated since the last build —
    /// the same set `build_scope` drains (`build_owner.rs:729-732`).
    pub reasons: RebuildReasons,
}

/// An element was unmounted — `Element::unmount` ran and its slab slot
/// was freed. This is a fact, not a scheduling artifact: it fires for
/// eager inline removals during reconcile, deferred keyed removals
/// drained by `finalize_tree`, lazy-sliver evictions, and root
/// detachment alike (see the emission-site contract, ADR-0040 §4).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementUnmounted {
    /// The unmounted element.
    pub element: ElementId,
}

/// Dependency-inverted tree observation (audit §26 / U3).
///
/// Implemented by consumers (devtools); emitted into by owners
/// (`BuildOwner` and the tree primitives it drives). All methods
/// default to no-ops so adding an observation kind is never a breaking
/// change for implementors.
///
/// # Ordering, threading, and panic contract
///
/// See ADR-0040 §4 (mutation-order stream, no phase bucketing) and §6
/// (owner-thread callbacks, no re-entry into binding/realm APIs,
/// panic-detaches policy). These contracts are reproduced in this
/// trait's rustdoc verbatim in the implementation.
pub trait TreeObserver: Send + Sync {
    /// A fresh element was minted and mounted.
    fn element_mounted(&self, event: &ElementMounted) {
        let _ = event;
    }
    /// An existing element moved (GlobalKey reparent or keyed reorder).
    fn element_moved(&self, event: &ElementMoved) {
        let _ = event;
    }
    /// An element's build completed.
    fn element_rebuilt(&self, event: &ElementRebuilt) {
        let _ = event;
    }
    /// An element was unmounted and its slot freed.
    fn element_unmounted(&self, event: &ElementUnmounted) {
        let _ = event;
    }
    /// End of stream: the observer was replaced, explicitly cleared, or
    /// detached after a panic (§6). No further events will arrive from
    /// this owner. Consumers drop or archive their mirror here — this,
    /// not mount/unmount balance, is the end-of-life signal.
    fn detached(&self) {}
}
```

Design notes on the shape:

- **Defaulted trait methods, not an event enum.** One `TreeEvent` mega-enum would couple every emitter domain into a single foundation type that grows a variant per observation and forces every consumer through a `match` it mostly ignores. Defaulted methods add observations without breaking implementors and cost nothing at sites the consumer doesn't override.
- **`#[non_exhaustive]` structs with `new` constructors** — the `ReconcileEvent` precedent (`reconcile_event.rs:96-135`): fields can be appended later; events pass by `&` so growth never changes call-site copying. Stated plainly: `#[non_exhaustive]` protects *matchers*, not *constructors* — appending a field changes `new()`'s signature. That is acceptable because events are constructed **only by in-workspace emitters**; out-of-workspace code (devtools, custom observers) reads events and must not construct them. Test fixtures that need synthetic events live in-workspace.
- **`Send + Sync` bound**: emissions happen on whichever thread owns the realm (owner-affine, ADR-0027), while the consumer half (a devtools reader) reads from another thread; a single collector may also be installed into several realms owned by different threads. A `!Sync` consumer wraps a channel internally; the trait does not lower its bound for that case. On `wasm32-unknown-unknown` the bound is trivially satisfiable (atomics lower to plain ops on the single-threaded target).

### 3. Registration is per-realm, one slot, on `BuildOwner` — with a seeded install for mid-run attach

```rust
// crates/flui-view/src/owner/build_owner.rs
impl BuildOwner {
    /// Install the realm's tree observer, replacing any previous one.
    /// A replaced observer receives `detached()` first, and the
    /// replacement is logged at `tracing::debug!` so competing tools
    /// (hot-reload tooling vs. an inspector) discover each other.
    ///
    /// Install at realm setup or via
    /// `WidgetsBinding::install_tree_observer` (below) — never from a
    /// frame phase.
    pub fn set_tree_observer(&mut self, observer: Arc<dyn TreeObserver>);

    /// Remove the observer (realm teardown — the ADR-0034
    /// install/clear symmetry). Fires `detached()` on the outgoing
    /// observer. Idempotent.
    pub fn clear_tree_observer(&mut self);

    /// The installed observer, if any. Clones the `Arc` out — no
    /// reference into private state, no guard (SP-6).
    #[must_use]
    pub fn tree_observer(&self) -> Option<Arc<dyn TreeObserver>>;
}

// crates/flui-view/src/tree/element_tree.rs
impl ElementTree {
    /// Replay the live tree as synthetic `ElementMounted` events, in
    /// pre-order (parent before children, `child_ids` slot order — the
    /// `collect_all_elements` walk discipline, `binding.rs:874`).
    /// Walks from the root via `child_ids`, so soft-removed keyed
    /// elements parked in the inactive queue are NOT replayed.
    /// The baseline for a mid-run attach (§Decision 3, below).
    pub fn replay_mounts(&self, observer: &dyn TreeObserver);
}

// crates/flui-view/src/binding.rs
impl WidgetsBinding {
    /// Atomic seeded install: under ONE `inner` write guard, replay the
    /// current tree into `observer` (`ElementTree::replay_mounts`),
    /// then install it as the realm's observer. Because every frame
    /// drive also holds the `inner` write lock (the E0a discipline,
    /// `binding.rs:426-443`), no tree mutation can interleave between
    /// replay and install — the consumer's baseline is exact.
    pub fn install_tree_observer(&self, observer: Arc<dyn TreeObserver>);

    /// Symmetric removal; fires `detached()`.
    pub fn remove_tree_observer(&self);
}
```

- **Mid-run attach is a first-class flow, so slice 1 ships the baseline.** This ADR rejects a compile-time gate precisely because "attach a panel to a running app" must work (§Alternatives 6) — an event-only seam without a baseline would hand a mid-run attacher an unusable stream. `replay_mounts` + the atomic seeded install close that gap for everything these events carry: tree structure, parent/slot edges, logical types. What replay *cannot* provide — state versions, dependency edges, accumulated history from before attach — is pull-shaped and stays with the future query-seam ADR (§Out of scope); the event seam does not pretend to cover it.
- **Per-realm by construction.** The observer is a field of a specific `BuildOwner`, which a specific `UiRealm` owns (`flui-app/src/app/binding.rs:61-62`). Observations are realm-scoped without any `RealmId` field in the events; a consumer aggregating several realms installs one thin wrapper per realm that tags a shared sink at install time (devtools-side pattern, not core API). This is the ADR-0027-sanctioned realm shape — a reviewer must not demand a process-global observer registry as "the Flutter way"; Flutter is not the reference for this topology.
- **One `Option` slot, not a `Vec`.** Fan-out is consumer-composable (an observer that forwards to N others); one slot keeps every emission site a single branch. `set_tree_observer` semantics are "replace with `detached()` + debug log", so migrating to multi-slot later is additive.
- **Teardown honesty.** The seam does **not** synthesize `element_unmounted` for elements alive at teardown, and it cannot: `detach_root_widget` removes only the root node (`binding.rs:790-803`; `ElementTree::remove` "Does NOT automatically remove children", `element_tree.rs:1184`), and a realm drop just drops the slab. `detached()` is therefore the consumer's end-of-life signal, and slice 1 wires the clear side everywhere it wires the install side (ADR-0034 symmetry): the integration test and example install via `install_tree_observer` and tear down via `remove_tree_observer`, asserting `detached()` fired. A `BuildOwner` dropped without `clear_tree_observer` emits no `detached()` — documented on the trait; embedders that install must clear.
- **Not a `BuildContext` capability, on purpose.** The four lifecycle-only capabilities (`rebuild_handle`, `post_frame_handle`, `text_input_handle`, `focus_manager`) need trigger #22's script (`scripts/check-frame-capability-scope.sh:1-20`) to police frame-phase acquisition. The observer is embedder infrastructure, not a per-widget capability: it is deliberately *absent* from `BuildContext`, making frame-phase acquisition impossible rather than linted. If a future change ever exposes it through `BuildContext`, that change must add a token to the trigger #22 script per `AGENTS.md`.
- **Internal plumbing** (not public API): the `ElementOwner<'_>` split-borrow handle (`build_owner.rs:1155-1175`) gains a `tree_observer: &mut Option<Arc<dyn TreeObserver>>` field alongside its existing fields, so the tree primitives that already receive `&mut ElementOwner` — `insert`, `remove`, `remove_finalized`, the retake paths, the reconciler — can reach the slot. The `&mut` (not `&`) is load-bearing: the panic-detach policy (§6) clears the slot from inside an emission helper.

### 4. Emission sites and the ordering contract

Emission sites are the **funnels every lifecycle fact passes through**, not frame phases:

| Event | Site | Covers | Anchor |
|---|---|---|---|
| `ElementMounted` | `ElementTree::insert` create path (after mount, alongside the existing `flui::reconcile` Mount emission); `mount_root` / `mount_root_with_pipeline_owner` with `parent: None`. The GlobalKey-retake early-return (`element_tree.rs:626-630`) never reaches this point, so a retake can never double-fire as a mount. | every fresh mint, from any build pass | `element_tree.rs:697-707, 506, 537` |
| `ElementMoved` | `try_retake_global_key`'s inactive-retake branch and `retake_active_global_key`, alongside their existing `Reparent` emissions; the two keyed-reorder branches that emit `ReconcileEvent::reorder` (where `set_child_slot` rewrites slot metadata) | GlobalKey reparents (both kinds) and every keyed slot change, including sibling shifts | `element_tree.rs:1629-1636, 1722-1728`; `id_reconcile.rs:269-271, 304-306, 619-623` |
| `ElementRebuilt` | `BuildOwner::build_scope`'s drain loop, after `put_element` restores the slot **and** the build outcome is known `Ok` — i.e. after the `resume_unwind` branch can no longer take it — and before the phase-2 child reconcile, so a parent's rebuilt event precedes the child mutations its build produced | every completed build from **every** `build_scope` call: the frame build, mid-layout settling (`service_layout_builders` schedules then runs `build_scope`, `layout_builder.rs:151-161`), and post-layout lazy-sliver servicing (`build_owner.rs:1063`) | `build_owner.rs:833-884` |
| `ElementUnmounted` | `ElementTree::remove` (eager branch, after `Element::unmount`) and `ElementTree::remove_finalized` (after `Element::unmount`) — the two primitives every unmount path bottoms out in | inline un-keyed removals during reconcile (`id_reconcile.rs:563-576`), `finalize_tree` drains (from end-of-frame *and* its mid-cycle calls), `remove_subtree` evictions, `detach_root_widget` | `element_tree.rs:1225-1240, 1336-1365` |

The load-bearing invariant, and the test oracle for slice 1: **`ElementUnmounted` fires exactly once per `Element::unmount`, `ElementMounted` exactly once per fresh mint** — so over any interval with no soft-removed elements pending, mounts and unmounts balance against live-tree size. The soft-remove branch of `remove` (`element_tree.rs:1200-1222`) deliberately emits nothing: the element is deactivated, not unmounted, and will either resurface as `ElementMoved` (retaken) or `ElementUnmounted` (finalized).

Every site emits through one private helper that owns the guard discipline and the panic policy (§6):

```rust
// crates/flui-view (private)
fn emit_observation(
    slot: &mut Option<Arc<dyn TreeObserver>>,
    f: impl FnOnce(&dyn TreeObserver),
) {
    if let Some(observer) = slot.as_deref() {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(observer))).is_err() {
            *slot = None; // §6: panic detaches the observer
            tracing::error!("TreeObserver panicked during emission; observer detached");
        }
    }
}
// call shape — payload construction stays inside the closure (lazy):
// emit_observation(owner.tree_observer, |o| {
//     o.element_mounted(&ElementMounted::new(id, Some(parent), slot, view.view_type_id()));
// });
```

**Ordering contract (documented on the trait):** the stream is a **totally-ordered tree-mutation log**, emitted synchronously on the realm's owner thread in the exact order the mutations happen. Per element it is causal: `element_mounted` first, then any interleaving of `element_rebuilt` / `element_moved`, then `element_unmounted` last. **No phase bucketing is promised**, because the frame pipeline genuinely interleaves: builds run mid-layout-settling and post-layout (§Context), eager unmounts happen inline during any reconcile, and `finalize_tree` runs at several points per frame — so "mounts during build, unmounts at end of frame" would be false and is *not* the contract. Consumers wanting per-frame buckets need the frame-demarcation events of slice 3. One documented approximation: between a keyed element's soft-remove and its retake/finalize, a consumer mirror still shows it under its last active parent; the window closes within the same frame drive, and no event refers to the element while it is parked.

### 5. Zero-cost-when-off: `Option` null check, no cargo feature in core — stated honestly

When no observer is installed, each site costs one load of a niche-optimized `Option<Arc<dyn TreeObserver>>` plus a predictable branch; no payload is constructed and no `catch_unwind` is set up (the helper's guard is outside it). That is **not** literally zero the way `#[cfg]`-ed-out code is zero — and this ADR does not claim it is. It is the same order as the tracing path it sits next to (a per-callsite interest check is likewise a load + compare), and it is *bounded by construction*: no allocation, no locking, no field evaluation behind the branch. With an observer installed, each emission additionally pays the `catch_unwind` frame — an enabled-path cost, accepted as the price of the panic policy (§6).

No measurement of this cost exists today; claiming a number now would violate the Definition of Done. Instead, slice 1 ships the benchmark: audit §30 explicitly lists "overhead of a disabled/attached inspector" among the benchmarks that *cannot currently be written* (`2026-07-25-upgrade-pack-audit.md:1021`); this seam is what makes it writable, and writing it is part of slice 1's gate (observer-absent vs. no-op-observer-installed vs. counting-observer, over a rebuild-heavy tree).

A `#[cfg(feature = "observe")]` gate in core is rejected (§Alternatives 6). Emission stays unconditionally compiled; the runtime `Option` is the off switch, per realm and per run.

### 6. Threading, non-reentrancy, and the panic policy

Observer callbacks run **on the realm's owner thread, inside frame phases, while the realm's locks are held**. The contract, documented on `TreeObserver`, names the hazards in the order an observer author will actually hit them:

1. **Same-thread deadlock — the first and hardest failure.** Emissions run while `build_scope` (and the rest of the frame drive) holds the binding's `inner` **write** lock — the documented E0a deadlock class (`binding.rs:426-443`). Every binding accessor an observer would naturally reach for — `with_build_owner`, `with_element_tree`, `root_element` (`binding.rs:630-656`) — takes `inner.read()` on a non-reentrant `parking_lot` `RwLock`: calling any of them from a callback **deadlocks the realm thread in release builds**. The contract is absolute: an observer callback must not call *any* flui-view binding, realm, or owner API. It may touch only its own state (atomics, lock-free structures, `try_send` into a channel it owns) and must return promptly — it is inside the frame transaction.
2. **Re-entry and scheduling.** Callbacks must not schedule work into the emitting owner. Synchronous re-entry into the build is a debug-build panic (`build_scope` asserts `!self.building`, `build_owner.rs:650`), but that assert does not cover the deadlock class above, nor release builds. What is *not* statically prevented, named honestly: an observer could smuggle in a `RebuildHandle` and call `schedule()` from a callback — the unbounded-rebuild-loop hazard trigger #22 names for frame-phase capabilities. Inspection-driven mutation ("select widget in inspector → highlight it") must go through the out-of-frame channels (`RebuildHandle` used from *outside* the callback, post-frame callbacks), never synchronously from an observer method.
3. **Panic policy: a panicking observer is detached, the frame survives.** Third-party consumer code must not be able to kill the frame transaction — and in the worst window it would do worse than kill it: an unwind between `take_element` (`build_owner.rs:777`) and the build's own `catch_unwind` (`build_owner.rs:796`) leaves a permanent `None` hole in the slab, turning every later access into an `ELEMENT_PRESENT` panic (the code's own warning, `build_owner.rs:783-795`). The design closes this two ways, belt and suspenders: (a) **no emission site sits inside an unprotected extract window** — `element_rebuilt` fires only after `put_element` has restored the slot (§4); and (b) **every** observer call runs inside the emission helper's `catch_unwind`; on unwind the observer is removed from the slot, `detached()` is *not* called (the observer just proved it cannot be trusted to run), and a `tracing::error!` records the detachment. This is consistent with `docs/PANIC-POLICY.md`: the framework reserves panics for its own invariants; a consumer panic is contained, reported, and disarmed. The same containment wraps `replay_mounts` during a seeded install — a panic there aborts the install and nothing is registered.

### 7. FR-036 registry

`Arc<dyn TreeObserver>` is a new framework `dyn` boundary. The same change that introduces it adds `TreeObserver` to the trigger #9 sanctioned-trait allowlist in `scripts/port-check.sh` (allowlist and category docs at `port-check.sh:1090-1180`), under category #5 (observer patterns) alongside the already-sanctioned `WidgetsBindingObserver`. Rationale recorded in the allowlist comment: dependency inversion *requires* type erasure here — the emitter crates cannot name the concrete devtools type below them in the DAG.

### 8. The devtools half: `inspector` feature, foundation-only dependency

`flui-devtools` gains `flui-foundation = { path = "../flui-foundation", version = "0.2.0", optional = true }` and a feature `inspector = ["dep:flui-foundation"]` — its first flui tree-adjacent dependency, still with zero access to the trees themselves. Two CI notes, stated so they are planned rather than discovered: `flui-devtools` is **not** in the wasm-check exclusion list (`ci.yml:631-641`), so the `inspector` feature must — and, with foundation being wasm-capable and `AtomicU64` lowering to plain ops on the single-threaded target, does — compile for `wasm32-unknown-unknown`; and the feature-matrix job gains one new per-crate feature to cover.

Slice 1's consumer proves the seam end-to-end by counting and logging:

```rust
// crates/flui-devtools/src/inspector.rs   (feature = "inspector")
use flui_foundation::observe::{
    ElementMounted, ElementMoved, ElementRebuilt, ElementUnmounted, TreeObserver,
};
use flui_foundation::{RebuildReason, RebuildReasons};

/// Counting/logging `TreeObserver` — the slice-1 inspector.
/// Interior state is private atomics; nothing here exposes a lock (SP-6).
#[derive(Debug, Default)]
pub struct InspectorCounters { /* private AtomicU64 tallies */ }

impl InspectorCounters {
    /// Plain constructor — callers wrap in `Arc` themselves
    /// (`Arc::new(InspectorCounters::new())`), consistent with the
    /// `Default` derive rather than competing with it.
    #[must_use]
    pub fn new() -> Self;

    /// Snapshot for display. Each counter is individually monotonic,
    /// but the snapshot reads independent atomics without a lock, so
    /// cross-counter consistency is NOT guaranteed (a snapshot taken
    /// mid-frame may show a mount whose paired rebuild is not yet
    /// counted). Per-counter monotonicity is the only invariant.
    /// Clones values out, returns no guard (SP-6 — the
    /// `pending_external_builds` discipline, `build_owner.rs:603-606`).
    #[must_use]
    pub fn snapshot(&self) -> InspectorSnapshot;
}

impl TreeObserver for InspectorCounters {
    // increments tallies; logs at `tracing::debug!` — no println (house rule)
    fn element_mounted(&self, event: &ElementMounted) { /* … */ }
    fn element_moved(&self, event: &ElementMoved) { /* … */ }
    fn element_rebuilt(&self, event: &ElementRebuilt) { /* … */ }
    fn element_unmounted(&self, event: &ElementUnmounted) { /* … */ }
    fn detached(&self) { /* marks the snapshot final */ }
}

/// Point-in-time counters. `#[non_exhaustive]` — fields append.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectorSnapshot {
    /// Elements mounted since attach (including replay-seeded mounts).
    pub mounts: u64,
    /// Elements unmounted since attach.
    pub unmounts: u64,
    /// Completed builds since attach (first builds included — see
    /// `ElementRebuilt` docs).
    pub rebuilds: u64,
    /// Moves (reparents + keyed reorders) since attach.
    pub moves: u64,
    // per-reason tallies stay private; read via `rebuilds_for`
}

impl InspectorSnapshot {
    /// Rebuilds whose cause set contained `reason`.
    #[must_use]
    pub fn rebuilds_for(&self, reason: RebuildReason) -> u64;
}
```

**Where the end-to-end proof lives — a named home, with its new dependency edge in §Scope.** `flui-devtools` cannot see the tree stack (by design), so a test that drives a real tree against `InspectorCounters` must live in a crate that has both halves. That crate is **`flui-testing`** — the deterministic headless frame driver, whose single-binary integration suite (`tests/main.rs`, `autotests = false`) already links flui-view + flui-rendering and drives real frames. Slice 1 adds `flui-devtools = { …, features = ["inspector"] }` to its `[dev-dependencies]` — acyclic, since flui-devtools depends only on flui-foundation (+ optional flui-hot-reload). Without this edge, "proves the seam end-to-end" would silently degrade to a flui-view unit test with a local observer plus never-compiled devtools code; the edge is part of the commitment. Installation in tests and examples uses `WidgetsBinding::install_tree_observer` / `remove_tree_observer` (§3); no new flui-app API is needed in slice 1.

## Alternatives considered and rejected

1. **The tracing stream as the seam** — rejected as the devtools seam for the five reasons in §Decision 1 (stringly-typed payloads, global subscriber lifecycle with a twice-bitten interest-cache race, no realm scoping, event-only, enabled-path Visit overhead); *retained* unchanged for trace tooling under FR-035.
2. **`flui-devtools` depends on `flui-view` directly.** Inverts nothing. It would compile the entire tree stack (`flui-view` pulls `flui-rendering`, `flui-objects`, `flui-scheduler`, `flui-interaction` — `flui-view/Cargo.toml:18-31`) into any app enabling inspection, put devtools *above* the framework so any future in-framework consumer of a devtools type (a perf-overlay widget) becomes a cycle, and grant devtools unlimited tree access where the audit asks for a narrow published surface.
3. **An owned-event channel declared in foundation** (`std::sync::mpsc` / crossbeam of `TreeEvent` values). Every event is allocated/copied even for a consumer that only counts; a bounded channel backpressures the *emitter* — inside the frame transaction, meaning a stalled devtools reader stalls the frame — while an unbounded one grows without limit when the reader stalls; and the decoupled delivery timing forfeits the synchronous mutation-order guarantee (§Decision 4) unless frame-marker events are added. The trait subsumes it: an observer that does `try_send` *is* a channel, with the drop policy chosen by the consumer; the reverse construction is impossible.
4. **Reuse `ChangeNotifier` / `ListenerRegistry`.** Foundation's listener machinery is void-callback / value-status shaped (`notify_value` / `notify_status`, `listener_registry.rs:182,225`); the payload types this seam exists to preserve would be erased at the boundary.
5. **A new `flui-observe` micro-crate.** Foundation already is the designated lowest crate, already holds the tree IDs and diagnostics, and is already a dependency of every emitter (`flui-rendering/Cargo.toml:26`, `flui-scheduler/Cargo.toml:15`). A new crate adds DAG surface and a publish target while isolating nothing.
6. **`#[cfg(feature)]`-gated emission in core.** Workspace feature unification makes such a flag effectively always-on in the workspace while silently varying for external consumers — the exact failure class the feature-matrix CI job exists for (`AGENTS.md`, CI notes). Features are also process-global and compile-time; the requirement is per-realm and runtime — attach a panel to a running app, which slice 1 makes real via the seeded install (§Decision 3). The runtime `Option` is strictly more capable at bounded, benchmark-verified cost (§Decision 5).
7. **`Vec<Arc<dyn TreeObserver>>` fan-out in core.** Deferred, not needed: composite observers compose outside; one slot keeps emission sites branch-shaped and the API replace-semantic (with `detached()` + debug log on replacement), so widening later is non-breaking.
8. **Observer as a `BuildContext` capability handle** (the `text_input_handle()` shape). Wrong ownership: observers belong to the embedder/realm, not to widgets; exposure through `BuildContext` would invite exactly the frame-phase acquisition trigger #22 polices, and would require its token machinery for no benefit.
9. **Emitting `ElementUnmounted` from `finalize_tree`.** Factually wrong, so rejected outright: only GlobalKey-carrying elements ever enter the inactive queue `finalize_tree` drains — ordinary un-keyed unmounts are eager and inline (`element_tree.rs:1177-1182`), so a finalize-only emission would miss the overwhelming majority of real unmounts, the mount/unmount pair would never balance, and every consumer store would leak. The two removal primitives are the only sites that are always true (§Decision 4).

## Consequences

**Positive**
- Devtools can finally observe the tree — with `flui-foundation` as its only framework dependency, preserving the audit's release-overhead posture (`default = []` stays; the seam is inert without an installed observer).
- Typed, realm-scoped, synchronously mutation-ordered observations; generational `ElementId`s make consumer-side stores ABA-safe by construction; the mount/move/unmount invariant (§4) makes a consumer mirror's parent/slot edges track GlobalKey reparents and keyed reorders instead of silently rotting.
- Mid-run attach works from a correct structural baseline (seeded install, §3) — the panel-attach flow the ADR requires is served by the committed slice, not deferred to an unwritten ADR.
- A panicking or misbehaving inspector is detached, not frame-fatal (§6).
- The §30 "inspector overhead" benchmark becomes writable, and slice 1 writes it.
- The trait *vocabulary* is positioned (foundation) so future emitters — render-phase, `flui-scheduler` task-phase — can reuse it; their registration topology is explicitly **not** presumed solved (see slice 2).

**Negative / costs**
- One branch + `Option` load per lifecycle event even with devtools absent — bounded, but not zero; the slice-1 benchmark is the check on this claim. Enabled-path emissions additionally pay a `catch_unwind` frame.
- `RebuildReason` moves crates (re-export keeps call sites; its rustdoc is reworded, not just re-linked, to drop the upward `BuildOwner` reference).
- Two mount-shaped and two move-shaped emissions coexist (`flui::reconcile` Mount/Reparent/Reorder and `element_mounted`/`element_moved`) at the same sites. Deliberate for now — different consumers, different contracts — with consolidation an explicit follow-up decision.
- The no-reentrancy/no-deadlock contract is documentation + a partial debug assertion, not a static guarantee (§6); the panic policy means a buggy observer silently stops receiving events after its first panic (mitigated by the `tracing::error!` and the absence of `detached()`).
- A realm torn down without `clear_tree_observer` gives consumers no `detached()` signal — install/clear symmetry is the embedder's obligation, enforced only by convention and the slice-1 tests.

**Neutral**
- FR-036 allowlist grows by one trait, with in-file rationale.
- Wishlist coverage after slice 1, honestly scoped: node id + generation (`ElementId`), logical component type (`view_type_id` — wrapper-transparent per `element/dispatch.rs:15-28`), element-tree parent/slot kept current through moves, last rebuild reason(s), and a structural baseline at attach. State version, dependency edges, memory footprint, and event-handler lists are pull-shaped and out of this seam's scope (below). Render-phase (layout/repaint) observation is not covered and not scheduled until its ownership question is decided (slice 2).

## Slicing

**Slice 1 (the commitment of this ADR):**
1. `flui-foundation`: `observe` module (`TreeObserver` with five defaulted methods + four event structs) and the `RebuildReason`/`RebuildReasons` move (including `ALL`/`iter`, `pub` promotion of `insert`/`merge`, reworded rustdoc) with `flui_view::owner` re-exports.
2. `flui-view`: `BuildOwner` slot (`set_/clear_/tree_observer` with replace-detach semantics), `ElementOwner` plumbing (`&mut Option<Arc<dyn TreeObserver>>`), the private `emit_observation` helper (catch_unwind + detach-on-panic), emissions at the §4 sites (`insert` create path, `mount_root*`, both retake paths, the two reorder branches, `build_scope` post-success, `remove` eager branch, `remove_finalized`), `ElementTree::replay_mounts`, and `WidgetsBinding::{install,remove}_tree_observer` (atomic seeded install under one `inner` write guard).
3. `flui-devtools`: `inspector` feature, `InspectorCounters` / `InspectorSnapshot`; wasm32 compilation of the feature verified (the crate is in the wasm-check set).
4. `flui-testing`: dev-dependency on `flui-devtools` (`features = ["inspector"]`); end-to-end test in `tests/main.rs` driving mount → rebuild → keyed reorder → GlobalKey reparent → unmount → detach against `InspectorCounters`, asserting exact counts, reason sets, and the mount/unmount balance invariant (§4), plus `detached()` on teardown — with the collector suite's vacuous-pass-guard discipline (positive count before absence assertions, `reconcile_event_collector.rs:298-312`) and a red→green check that removing an emission site fails its test. Unit tests in flui-view cover the eager-unmount path (`id_reconcile` removals), the soft-remove no-emission window, mid-layout-pass rebuild emission (`service_layout_builders` fixture), panic-detach, and seeded replay exactness.
5. `scripts/port-check.sh`: `TreeObserver` allowlist entry (same change as the `dyn` introduction).
6. Criterion benchmark: rebuild-heavy tree, observer absent vs. no-op vs. counting (§Decision 5).

**Slice 2 — render-phase observation: needs its own follow-up ADR before any code.** The earlier assumption ("mirror the observer slot on `PipelineOwner`, same architecture") is contradicted by the current topology: the pipeline is **process-hosted**, not realm-owned (`flui-app/src/app/binding.rs:55-62`; `Arc<RwLock<PipelineOwner>>` shared through the realm, `flui-view/src/binding.rs:460`), so the per-realm registration argument of §Decision 1 reason 3 does not transfer, and a slot on `PipelineOwner` would be process-wide observation wearing a realm-shaped API. The honest resolution is a dedicated ADR that decides render-phase observer placement together with (or explicitly ahead of) the multi-realm pipeline story — the `TreeObserver` *vocabulary* (defaulted methods carrying `RenderId` + typed cause) is expected to carry over; the registration topology is the open decision.

**Slice 3:** frame demarcation events (build-scope begin/end and frame begin/end from the binding's frame drive) so consumers can bucket the mutation log per frame; the devtools-side realm-tagging wrapper; the decision on folding `flui::reconcile` dispositions into `TreeObserver` (an FR-035 contract-rev question, taken explicitly).

## Deliberately not in scope

- **Render-tree and semantics observation** — blocked on the slice-2 follow-up ADR (pipeline ownership, above); nothing here designs them.
- **Pull/query inspection** (walk the live tree, read state versions, dependency edges, memory) — a different seam with a different hazard profile (re-entering realm-owned state from off-thread); requires its own ADR anchored on post-frame/realm-entry channels. Stated plainly so no reader over-credits this seam: of the audit's 16-field inspector wishlist, this ADR's event seam serves the *structural* subset (identity, type, position, rebuild causes, attach baseline); the pull-shaped fields **cannot** be served by events and wait on that future ADR.
- **Active-task observation** — belongs to `flui-scheduler`'s `AsyncDriver`/`TaskToken` (`async_driver.rs:135`; cancel-on-drop, line 29), which can emit through this same foundation trait later via new defaulted methods; not designed here.
- **Remote protocol / serialization** — `flui-devtools` keeps `serde` for the day a wire format exists; this ADR intentionally defines no wire format.
- **Deprecating the `flui::reconcile` stream** — it stays under FR-035; consolidation is slice 3's explicit decision.

## Follow-ups

- The render-phase observation ADR (slice 2's blocker): observer placement for a process-hosted pipeline vs. the multi-realm pipeline story.
- Decide `flui::reconcile` consolidation (slice 3) with an FR-035 contract-rev if taken.
- Realm-builder convenience for observer installation in `flui-app`, once an app (not a test) wants it; consider a realm-teardown hook that guarantees `clear_tree_observer` (closing the "dropped without detach" gap by construction instead of convention).
- The pull/query snapshot seam ADR, once slice 1 events prove insufficient for the inspector UI (the state-version / dependency-edge fields already guarantee they will be).
- If the slice-1 benchmark shows the off-path branch is measurable in frame profiles (not expected), revisit with a `#[cold]`-outlined emission helper before reconsidering any cfg gate.
