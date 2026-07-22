# ADR-0008 — flui-view leapfrog: compile-safe BuildContext, read-is-subscribe field-precise inherited, capability-decomposed Element

- **Status:** Proposed (PR-H1a landed; the keystone forks below await maintainer ratification)
- **Date:** 2026-06-25
- **Scope:** `flui-view` (Element tree + View tree), with ripples into `flui-macros`, `flui-app`
- **Supersedes / relates:** complements [ADR-0003 virtualization-core-and-reentrant-build]; reaffirms FOUNDATIONS **C1** (no blanket `View: PartialEq` / `Data: Clone + PartialEq` — the Druid trap) and **C5** (`BuildContext` object-safe, no lifetime param on the trait surface).

> The RenderObject layer didn't just *port* Flutter — it made the layer **better** (`UsageByParent` 3-state instead of the `parentUsesSize` bool, explicit relayout boundaries, the Arity vocabulary). The point was never "it has a trait called `Protocol`"; the point was: make Flutter's implicit contracts explicit and type-checked, and turn classes of runtime mistakes into compile-time facts. This ADR does the equivalent for the View/Element/BuildContext layer — its own design, not a Protocol clone.

---

## Context — what is actually wrong today (ground-truthed, file:line)

A multi-agent audit + hands-on verification found four P1-class issues. These are facts confirmed against the current source, not hypotheses.

1. **The `BuildContext` threaded into user `build()` is disconnected from the real tree.**
   `ElementBase::build_into_views(&mut self, owner)` (`element/unified.rs:307`) receives no tree handle; `BuildOwner::build_scope` (`owner/build_owner.rs`) holds `&mut ElementTree` but does not thread it down. So every behavior builds `ElementBuildContext::new_minimal()` (`element/behavior.rs:259,383`) backed by a process-shared **empty** dummy tree (`owner/build_owner.rs` `SHARED_DUMMY_TREE`). During a real `build()`, `ctx.depend_on_inherited` / `find_ancestor_*` / `dispatch_notification` / `mark_needs_build` all walk/write an empty tree and are **inert in production**. InheritedView (Theme/MediaQuery-class) cannot deliver to descendants. Root cause is structural: `ElementBuildContext` is built on `Arc<RwLock<ElementTree>>` (`context/element_build_context.rs:36`) while `build_scope` owns `&mut tree` exclusively — handing the real `Arc<RwLock>` in would deadlock; the dummy compiles but is inert. Known/deferred in-code ("Cycle 6 element-ownership unification").

2. **`depend_on_inherited` is `O(depth)` per call under a whole-tree write lock.**
   `walk_ancestors_for_inherited` (`context/element_build_context.rs:163`) is a linear parent walk; `depend_on_inherited` takes `self.tree.write()` (whole tree) just to record a dependent. Flutter is `O(1)` via a per-element persistent map. The crate's own docs contradict the code (`view/inherited.rs:1-4` claims an `O(1)` `BuildOwner` hash table that does not exist; `element/behavior.rs:62` repeats the false claim).

3. **`ElementBase` is a ~40-method object-safe god-trait.**
   `view/view.rs:176-706`. ~12–15 methods are pure `&dyn Any` / `TypeId` smuggling accessors (`view_as_any`, `state_as_any`, `render_object_any/_mut`, `as_inherited/_mut`, `render_id`, `pipeline_owner_any`, `render_object_shared` — which returns the port-check-banned `Arc<RwLock<dyn Any + Send + Sync>>` shape). `ElementBehavior` re-declares the same accessors, so `V` is double-erased and `Element<V,A,B>` is a forwarding shim. The third type parameter (Arity) is vestigial post-E3: it gates no API and validates no child count. This is the opposite of the RenderObject layer's single-`dyn`-at-dispatch (Constitution Principle 4).

4. **The production reconciler emits zero `ReconcileEvent`s.**
   `reconcile_children_by_id` (`tree/id_reconcile.rs`) had **0** `emit()` calls; the entire typed `flui::reconcile` stability boundary (FR-035) was dead on the live path, exercised only by a byte-duplicate test-only box reconciler. **This one is fixed in this ADR's first increment (PR-H1a) — see "What landed".**

---

## Decision — the leapfrog (its own design, not a Protocol clone)

Adopt a **borrow-checked, field-precise reactive build contract** that exploits Rust where Dart/Flutter physically cannot (lifetimes, the borrow checker, typestate, `!Send`, zero-cost monomorphization). Three interlocking compile-time guarantees, each turning a Flutter runtime-error or ergonomic-debt class into an unrepresentable state, while keeping the retained three-tree's **observable** behavior identical (a Theme change still rebuilds the right dependents; `setState` still rebuilds; lifecycle ordering matches `framework.dart`).

### 1. Branded `Cx<'build>` — a context that cannot escape `build()`

A by-value, invariant-branded, `!Send`, `!Clone`, non-`'static` token, minted only by a sealed framework ctor and handed to `build()` / lifecycle hooks:

```rust
// The brand: an invariant lifetime makes Cx un-stashable and un-Send.
pub struct Cx<'build> {
    id: ElementId,
    depth: usize,
    // invariant in 'build (neither co- nor contra-variant) + !Send + !Clone:
    _brand: PhantomData<&'build mut &'build ()>,
}
```

The lifetime `'build` turns three whole Flutter runtime-crash families into **non-programs**:
- stash a context in `State` → a `'static` field cannot hold a `'build` borrow (`E0597`/`E0521`);
- carry a context across `.await` → `!Send` + the borrow ends at the build scope;
- use a context after the element is defunct → the value does not exist outside the build/lifecycle call.

Flutter can only catch these at runtime via `_debugCheckStateIsActiveForAncestorLookup` (`framework.dart:5057`, debug-only; release silently corrupts). **R1-loyal:** observable behavior is identical for correct programs (a correct Flutter program never trips that assert); the only divergence — the defunct-lookup path is structurally absent — is a documented strict improvement.

> **Correction applied (adversarial vet):** `Cx` carries only `{id, depth, brand}` — **not** a tree reference. The "`Cx` owns `&mut tree`" framing does not compile (self-referential `&'b mut TreeView<'b>`, immediate `E0499`). The live tree read is delivered by **widening the build seam**, not the token (see §"Supporting structure"). To honor FOUNDATIONS **C5** (no lifetime param on the object-safe trait), the brand lives inside the **callback form** `cx.depend_on::<T, R>(|scope| ...) -> R` so the public surface stays lifetime-free and object-safe.

### 2. Read-IS-subscribe, field-precise inherited dependencies

Reading an inherited value *is* subscribing to it, at **field** granularity:

```rust
// #[derive(InheritedData)] on the provider data generates a zero-sized
// FieldSelector<T, F> per field + a FieldMask(u64) diff. No stringly aspects.
let primary = cx.depend_on::<Theme, _>(Theme::primary);   // subscribes to ONE field
// whole-type dependency (full InheritedWidget parity) stays first-class:
let theme = cx.depend_on_all::<Theme>();                  // subscribes to !0
```

A provider update computes `old.field_mask_diff(new)` (a branchless bitset AND) and schedules **only** the dependents whose subscribed mask intersects — so changing `Theme.text_scale` does not rebuild a `primary`-only reader. This is Flutter's `InheritedModel`, but type-checked (compile-time field identity, no `isSupportedAspect` strings), zero-cost (a `u64` resolved at monomorphization), and read-coupled-to-depend (Flutter's two separate methods let you read-without-depending and go stale). **C1 preserved:** the field diff comes from the opt-in `#[derive(InheritedData)]` generating a `FieldMask`, never a blanket `Data: PartialEq` bound. Default whole-type `depend_on_all::<T>()` = `!0` = vanilla `InheritedWidget` parity.

> **Corrections applied (adversarial vet):** the persistent inherited map keys on the **provided-data** `TypeId` (`TypeId::of::<Theme>()`), not the provider view type — `depend_on::<Theme>` and the map must agree or the `O(1)` lookup returns `None`. Dependent-recording is **synchronous** into the provider node (a *different*, still-slab-resident node, so by-value extraction of the building element does not block its `&mut`) — only the dependent's own mark-dirty defers into the same depth-ordered drain heap; this preserves Flutter's record-before-notify ordering (`framework.dart:5086-5087`) and closes the same-pass missed-notify window. Borrowed `&F` returns are offered only for `Copy`/cloned projections; the general borrowed read uses the closure form so the borrow ends at the closure.

### 3. `Mounted<'_>` re-entry capability + RAII `EffectScope`

The **only** way to re-enter the retained build pipeline from an async/listener callback is a `Mounted<'_>` token obtained by a generational liveness check (`{id, gen}` resolves AND `lifecycle == Active`), and effects/subscriptions are RAII handles torn down LIFO on unmount (Leptos children-before-parents):

```rust
let weak = cx.weak();                    // { id, gen } — Copy, no Arc
// ... later, from a listener/async callback:
if let Some(m) = weak.mounted(binding) { // generational + lifecycle gate
    m.set_state(|s| s.tick());           // reaches schedule_build_for, not a dead flag
}
```

This makes "setState after dispose", "listener leak", and "wake a dead element" unrepresentable — and **fixes a real bug**: today `create_mark_dirty_callback` (`element/generic.rs:490`) flips an `AtomicBool` but never calls `schedule_build_for`, so listener-driven rebuilds can be inert. This is the View-layer analogue of `UsageByParent`: a runtime-checked Flutter invariant becomes a type-level one.

> **Corrections applied (adversarial vet):** `WeakElement` is `{ id, gen }` (Copy), not an `Arc<RwLock<…>>` cell (that would re-introduce a lock at the dyn seam, violating SP-6 / ADR-0002 `!Send`). `EffectScope` cleanups are `Box<dyn FnOnce()>` (no `Send` bound — control-plane data is `!Send` per ADR-0002); `Send` is required only on a spawned future and its output. The novelty claim is scoped to `Mounted` as the sole pipeline-re-entry capability; the RAII-cleanup half overlaps Leptos owners / `Drop` guards and is not sold as original.

### Supporting structure (the engineering that makes the above real)

- **Live `BuildContext` during build via by-value extraction.** `ElementNode.element` becomes `Option<Box<dyn ElementBase>>`; the `build_scope` drain does `take_element` → build against a read-only `TreeRead(&*tree)` → `put_element`. `TreeRead` splits its accessors so `parent` / `depth` / `inherited_map` / `child_ids` read `ElementNode` fields that survive the take, while only `get_element` can observe the `None` hole (returns `None` in all build profiles — a real branch, not a debug-only assert). This deletes `new_minimal` / the shared dummy and gives `depend_on` / `find_ancestor_*` a live tree with no `Arc<RwLock>` re-lock and no deadlock. (This is the keystone; see fork F1.)
- **Per-element persistent inherited map** `Arc<InheritedMap>` (rpds `HashTrieMapSync`, MPL-2.0, already allow-listed in `deny.toml`) keyed on provided-data `TypeId`: non-providers alias the parent map by refcount (the `framework.dart:5129` pointer-copy); a provider stores `parent_map.insert(TypeId::of::<D>(), self)` on itself, so nested same-type providers shadow nearest-wins via HAMT insert. `depend_on` becomes one map index — `O(1)`, lock-free on read.
- **`ElementBase` shrunk to a ~12-method object-safe core**, capabilities reached through a few typed `as_*` hooks (`as_render_host`, `as_inherited`, `state_as_any` folded down), so the `&dyn Any` smuggling shrinks to the single dispatch seam and `render_object_shared`'s `Arc<RwLock<dyn Any>>` shape is deleted. Whether this rides a sealed capability-typed `Element<V, P>` (collapsing the vestigial Arity param into `P::Arity`) or stays a hand-shrunk `Element<V, A, B>` is **fork F2** — it is an internal-shape decision, not part of the headline.

---

## Why this beats Flutter AND the Rust field

| Concern | Flutter | xilem / gpui / dioxus / floem | This design |
|---|---|---|---|
| Context misuse (defunct / across-await / wrong-phase) | runtime assert, debug-only | contexts are `'static`/arena, freely stashable | **compile error** (brand + `!Send` + `'build`) |
| Inherited dependency | `O(1)` but all-or-nothing per type; `InheritedModel` aspects are stringly + bolted on; read ≠ depend | runtime walk / thread-local maps / signals (route around the retained tree) | `O(1)` + **field-granular** typed aspects; **read == depend** |
| Content memoization | none (rebuilds every reused element; only `const` escapes) | hand-written prev-diff (xilem) / compiler-proved stability (compose) | opt-in derive; closure fields a **compile error**, not a silent stale closure |
| "Forgot to mark dirty" / wake a dead element | silent no-update / runtime throw | varies | **unrepresentable** (`Mounted` is the only re-entry; mutate auto-schedules) |
| Reconciliation observability | debug-mode rebuild tracking + devtools protocol | no stable typed production diff trace | typed, zero-cost-when-unsubscribed `flui::reconcile` stream from the **live** path |

R1 (loyalty) holds throughout: the leapfrog is in developer-facing safety, ergonomics, and internal efficiency — not in changing what the end user sees — exactly as `UsageByParent` improved the contract without changing layout output. Signals-as-default are rejected (they route invalidation around the retained tree, violating R1 and C1); the smallest sound invalidation unit stays the Element.

---

## Sequenced plan

| # | Title | Scope | Risk | Depends on |
|---|---|---|---|---|
| **PR-H1a** | wire `ReconcileEvent` emission into the production `id_reconcile` path | small | low | — **(LANDED)** |
| PR-H1b | delete the dead box reconciler; migrate the §U18 permutation corpus + `reconciliation_tests` onto the slab | small | low | PR-H1a |
| PR-K | live `BuildContext` during build via by-value extraction (delete the empty dummy) | large | high | PR-H1b |
| PR-2 | persistent `InheritedMap` + `O(1)` `depend_on` + full Flutter `activate()` port | medium | medium | PR-K |
| PR-3 | shrink `ElementBase` to the ~12-method core; capability `as_*` hooks; (fork F2) optional sealed `Element<V,P>` | large | high | PR-K, PR-2 |
| PR-4 | field-precise `depend_on` + `#[derive(InheritedData)]`/`#[derive(Stable)]`; `Mounted`/`EffectScope`; bind Arity; finish `apply_parent_data` | medium | medium | PR-3 |

Each behavior-change PR carries a `.flutter/` cross-check and a test that fails before the fix (Definition of Done).

---

## What landed this session (PR-H1a — verified)

`tree/id_reconcile.rs`: `reconcile_children_by_id` now emits one typed `ReconcileEvent` per child disposition on the **live** path — `Reuse` (top-scan same-slot), `Unmount` (keyless-middle drop + unclaimed-keyed drop, at the child's old slot), `Reorder`/`Reuse` (phase-4 keyed claim, decided by old-slot vs new-slot), `Mount` (fresh insert), and `Reuse`/`Reorder` for the bottom slice (decided by `old_bottom == new_bottom`). Placement mirrors the dead box reconciler 1:1, so the `flui::reconcile` (FR-035) stability boundary is finally meaningful for normal reconciliation — instrumentation strictly better than anything the Rust UI field ships.

Five `#[serial]` collector tests prove the stream as a multiset (Mount / Reuse / Reorder / Unmount / type-change Unmount+Mount); each fails before the wiring (zero events). Setup uses direct `tree.insert` (no emit) and a once-installed interested global subscriber so the process-global tracing callsite-interest cache cannot latch `never` and bypass the collectors. Gates green: `cargo test -p flui-view --features test-utils` (all binaries, 0 failed), `cargo fmt --check`, `cargo clippy --all-targets -D warnings`.

---

## Open strategic forks (need maintainer ratification before the keystone)

- **F1 — PR-K shape.** PR-K (live context) is a large atomic, no-shim, cross-crate signature change (all `build_into_views` impls + `Option<Box>` slot ripple + delete `new_minimal`). Options: (a) one atomic PR with two compiling internal commits (mechanical plumbing, then wire-real) — **recommended**, matches the no-shim / no-quick-wins discipline; (b) relax no-shim once with a temporary second method; (c) report BLOCKED and do PR-3's protocol shrink first to cut the dispatch-point count. The adversarial review's honest read: if commit 1 does not compile green on its own, prefer BLOCKED over a partial merge.
- **F2 — `ElementBase` shrink shape.** Sealed capability-typed `Element<V, P>` (collapsing the vestigial Arity param) vs a hand-shrunk `Element<V, A, B>`. Recommended: shrink to a single `Box<dyn ElementBase>` storage seam with typed `as_*` hooks (mirrors RenderObject's single dyn seam) — **not** a closed-enum-storage rewrite. The capability typing is an internal nicety, not the headline; sequence it after the inherited/context wins.
- **F3 — GlobalKey registry.** Keep the process-global registry (forces `serial_test`) as a documented divergence for now, or migrate to per-`BuildOwner` (Flutter's home) in PR-2. Recommended: keep the divergence; schedule the registry move as a standalone follow-up so it does not inflate PR-2.

---

## Alternatives considered and rejected

- **Mirror the RenderObject `Protocol` shape/name literally.** Rejected as the *headline*: the goal is to beat Flutter, not to be symmetric with the render layer. The capability decomposition survives as supporting structure (fork F2), not as the marquee.
- **Fine-grained signals as the default reactivity (leptos/floem-style).** Rejected: routes invalidation around the retained Element tree (violates R1) and pulls toward the `Data: Clone + PartialEq` constraint-creep C1 bans.
- **xilem `<State, Action>` type params on the View trait.** Rejected: shatters object-safety, forces `AnyView`/`DynMessage` and re-introduces the downcast FLUI already pays.
- **Compose-style positional identity.** Rejected: relies on a compiler plugin guaranteeing call-site emission order, which plain Rust `build()` functions cannot provide (would mis-key under conditional control flow). The smallest sound invalidation unit stays the Element.
