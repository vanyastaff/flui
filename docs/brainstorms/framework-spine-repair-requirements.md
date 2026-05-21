---
date: 2026-05-21
topic: framework-spine-repair
---

# Framework Spine Repair — `flui-view` × `flui-foundation` × `flui-types`

## Summary

Single comprehensive PR repairing the FLUI framework spine: wire the seven stubbed `ElementBuildContext` methods to actually walk the Element tree and InheritedView dependency map, plumb `BuildOwner` through `Element::mount/unmount/update` so `GlobalKey::current_element` and `current_state` read the existing registry, resolve the parallel-type collisions (`ViewKey`×2, `IndexedSlot`×2, `TargetPlatform`×2), delete zero-consumer foundation bloat (`MergedListenable` / `HashedObserverList` / `SyncObserverList` / `FluiError` plus the `dashmap` dep), add `ChangeNotifier::dispose` semantics matching Flutter, remove the two `unimplemented!()` calls in `crates/flui-view/src/view/root.rs`, wire `InheritedBehavior::on_update` dependent notification, and extract shared `ElementBehavior` boilerplate into a common helpers module. Atomic-commit-per-unit shape (~25–35 units) matching the [PR #81](https://github.com/vanyastaff/flui/pull/81) / [#82](https://github.com/vanyastaff/flui/pull/82) / [#83](https://github.com/vanyastaff/flui/pull/83) precedent.

---

## Problem Frame

The audit at `docs/research/2026-05-21-view-tree-foundation-audit.md` (commit `592bc8cf` + post-correction `8432345c`) surfaced the framework-spine layer of FLUI as **structurally promising but functionally hollow**. The most-used user-facing API of any UI framework — `BuildContext` — has seven of its ten methods (`depend_on_inherited`, `get_inherited`, `find_ancestor_view`, `find_ancestor_state`, `find_root_ancestor_state`, `find_render_object`, `dispatch_notification`) returning `None` or no-op with `// Placeholder - needs architectural solution` comments at `crates/flui-view/src/context/element_build_context.rs:189,213,243,249,254,259,302`. The second most-used API — `GlobalKey<T>::current_element` / `current_state` — has a complete `BuildOwner::global_keys: HashMap<u64, ElementId>` registry at `crates/flui-view/src/owner/build_owner.rs:65` with full `register_global_key` / `unregister_global_key` / `lookup` plumbing, but `Element::mount` never calls register, and `GlobalKey::current_element` doesn't read the registry — both methods carry `// TODO: Implement via GlobalKeyRegistry` markers at `crates/flui-view/src/key/global_key.rs:78,91`. Tests pass because they exercise `find_ancestor_element` (impl OK) and `mark_needs_build` (impl OK) only.

Adjacent to the stubs sit three parallel-type collisions blocking any reasonable downstream API. `flui-foundation::ViewKey` (with four impls: `GlobalKey`, `ValueKey`, `UniqueKey`, `ObjectKey`) collides with a view-local `flui-view::view::view::ViewKey` (zero impls), so `View::key()` returns `Option<&dyn` view-local-`ViewKey>` and no concrete key can ever be returned. Two `IndexedSlot` types — `flui_tree::IndexedSlot<I>` (the canonical unified-tree home per [`STRATEGY.md`](../../STRATEGY.md)) and `flui_view::IndexedSlot<T>` — coexist with prelude-glob-import collision risk. Two `TargetPlatform` enums in `flui-foundation::platform` vs `flui-types::platform::target_platform` carry different variants (foundation has `Unknown`, types has `Fuchsia`).

`flui-foundation` itself carries deletable bloat: `MergedListenable` + `HashedObserverList` + `SyncObserverList` have zero workspace consumers; the `dashmap` workspace dep exists solely for `HashedObserverList`. `FluiError` duplicates `FoundationError` (both zero consumers). And `ChangeNotifier` is missing the `dispose` + disposed-state-assertion semantics that production listeners need to detect use-after-free (Flutter has explicit asserts at [`flutter/lib/src/foundation/change_notifier.dart:181,376`](../../.flutter/flutter-master/packages/flutter/lib/src/foundation/change_notifier.dart)).

Finally `crates/flui-view/src/view/root.rs:487,494` carries two `unimplemented!()` calls in a production path — direct Constitution Principle 6 violation ("No `unwrap()`/`println!`/`dbg!`" — `unimplemented!()` is the same class of panic).

The cost is invisible: FLUI advertises Flutter widget-tree parity, but no real `build()` implementation calling `ctx.depend_on::<MyTheme>()` can read theme data through the tree today. Type-collision resurfacing blocks downstream crate authors from holding a concrete `Key` instance. Memory-leak-shaped semantics (no `ChangeNotifier::dispose`) hide in plain sight. Repeat reviews of the same broken surface are the user pain that motivates bundling these into one repair pass rather than picking off "quick wins" individually.

---

## Actors

- A1. **View author.** Writes a `build()` method that calls into `BuildContext` to read inherited data (`ctx.depend_on::<MyTheme>()`), look up ancestor state, dispatch notifications, or get the nearest `RenderObject`. Today eats `None` for seven of ten methods.
- A2. **GlobalKey holder.** Constructs `GlobalKey<MyState>` ahead of mount, expects `key.current_state()` to return `Some(&MyState)` once the keyed element is built. Today gets `None` always.
- A3. **InheritedView author.** Defines an inherited type, expects descendants that depend on it to rebuild when `update_should_notify` returns `true`. Today no dependent is ever notified.
- A4. **Downstream crate author** (`flui-rendering` / `flui-layer` / `flui-semantics` / `flui-interaction`). Imports `flui-foundation::ViewKey` or `flui-foundation::TargetPlatform`, hits the dual-type ambiguity. Cannot return a `dyn ViewKey` from `View::key` because the trait shapes diverge.
- A5. **Framework internals** (`Element`, `BuildOwner`, `WidgetsBinding`). Plumb the `BuildOwner` reference through Element lifecycle, drive InheritedView dependency map, manage GlobalKey registry, and propagate dispose to listenable graphs.

---

## Key Flows

- F1. **Read inherited data via BuildContext.**
  - **Trigger:** A1 calls `ctx.depend_on::<T>()` inside `build()` of a view nested under an `InheritedView<T>`.
  - **Actors:** A1, A5.
  - **Steps:** (1) `ElementBuildContext` walks ancestor `Element` chain looking for an `InheritedElement<T>`; (2) records the calling element's `ElementId` as a dependent in the inherited element's dependent set; (3) returns `&T` borrowed from the inherited element's view. Flutter reference: `flutter/lib/src/widgets/framework.dart:5028` (`InheritedElement.updateDependencies`) + `:5092` (`dependOnInheritedWidgetOfExactType`).
  - **Outcome:** Caller has `&T`. When the inherited value later changes, A5 fires `schedule_build_for` on the caller's `ElementId`.
  - **Covered by:** R7, R8, R9, R12, AE1, AE2, AE3.

- F2. **GlobalKey state lookup.**
  - **Trigger:** A2 holds `GlobalKey<MyState>` constructed before the keyed view was mounted; calls `key.current_state()`.
  - **Actors:** A2, A5.
  - **Steps:** (1) `GlobalKey::current_element()` consults `BuildOwner::global_keys` HashMap; (2) returns `Some(ElementId)` if registered, `None` otherwise; (3) `current_state()` resolves `ElementId` → `&StatefulElement` → `&S`. Flutter reference: `flutter/lib/src/widgets/framework.dart:3148+` (`_globalKeyRegistry` + `GlobalKey._currentElement`).
  - **Outcome:** A2 receives `Some(&S)` once the keyed view is mounted; `None` before or after defunct.
  - **Covered by:** R13, R14, R15, AE4, AE5.

- F3. **InheritedView change propagates to dependents.**
  - **Trigger:** A3's `InheritedView<T>` produces a new value during `WidgetsBinding::draw_frame`. `update_should_notify(old, new)` returns `true`.
  - **Actors:** A3, A5.
  - **Steps:** (1) `InheritedBehavior::on_update` walks the dependent set populated by F1's step 2; (2) calls `BuildOwner::schedule_build_for(dep_id, dep_depth)` for each. Flutter reference: `flutter/lib/src/widgets/framework.dart:5118+` (`InheritedElement.notifyClients`).
  - **Outcome:** Every dependent's `Element` is marked dirty and rebuilds in the next frame in depth order.
  - **Covered by:** R16, AE2.

- F4. **Notification bubbling.**
  - **Trigger:** A1 calls `ctx.dispatch_notification(MyNotification)` from inside `build()`.
  - **Actors:** A1, A5.
  - **Steps:** (1) `ElementBuildContext::dispatch_notification` walks ancestor `Element` chain; (2) on each ancestor, downcasts to `NotifiableElement<N>` and calls `on_notification(&self, &N) -> bool`; (3) stops bubbling when handler returns `true` or root reached. Flutter reference: `flutter/lib/src/widgets/notification_listener.dart:62+` (`Notification.dispatch`).
  - **Outcome:** First ancestor that handles the notification short-circuits the chain.
  - **Covered by:** R10, R11, AE6.

---

## Requirements

**Type-system collision resolution (audit Finding #3 — ordered FIRST in the unit sequence; GlobalKey impls foundation's `ViewKey` so #2 cannot function until this lands)**

- R1. Delete `flui-view::view::view::ViewKey` trait. Retype `View::key()` to return `Option<&dyn flui_foundation::ViewKey>`. Update all `Box<dyn View>` re-exports and prelude such that `GlobalKey` / `ValueKey` / `UniqueKey` / `ObjectKey` are the canonical impls of one `ViewKey` trait.
- R2. Pick one canonical `IndexedSlot` home. Per [`STRATEGY.md`](../../STRATEGY.md) "Behavior loyal, structure Rust-native" + [memory `flui-tree-unified-interface-intent`](../../../../.claude/projects/C--Users-vanya-RustroverProjects-flui/memory/flui-tree-unified-interface-intent.md), the canonical home is `flui-tree::IndexedSlot<I>`. Migrate `flui-view::IndexedSlot<T>` and the `ElementSlot = IndexedSlot<Option<ElementId>>` alias to use the `flui-tree` type. Delete the view-local duplicate.
- R3. Delete one of `flui-foundation::TargetPlatform` or `flui-types::TargetPlatform`. Decision deferred to planning — needs codebase scan to verify which is referenced from `flui-platform` and downstream crates. Reconcile variant sets (`Unknown` + `Fuchsia` both in the survivor). Migrate all consumers.

**BuildContext functional API (audit Finding #1)**

- R4. `ElementBuildContext::depend_on_inherited<T: 'static>(&mut self) -> Option<&T>` walks ancestor Element chain finding the nearest `InheritedElement<T>`, records the caller's `ElementId` in that element's dependent set, returns borrowed `&T`.
- R5. `ElementBuildContext::get_inherited<T: 'static>(&self) -> Option<&T>` performs the same ancestor walk as R4 **without** recording a dependency. Used for one-time reads.
- R6. `ElementBuildContext::find_ancestor_view<V: View>(&self) -> Option<&V>` walks ancestor Element chain returning the nearest borrowed `&V` by `TypeId` match.
- R7. `ElementBuildContext::find_ancestor_state<V: StatefulView>(&self) -> Option<&V::State>` walks ancestor Element chain returning the nearest borrowed `&V::State` by `TypeId` match.
- R8. `ElementBuildContext::find_root_ancestor_state<V: StatefulView>(&self) -> Option<&V::State>` walks all the way to the root, returning the root-most matching state.
- R9. `ElementBuildContext::find_render_object(&self) -> Option<RenderId>` walks ancestor Element chain returning the nearest `RenderId` from a `RenderElement`.
- R10. `ElementBuildContext::dispatch_notification<N: Notification>(&self, n: N)` walks ancestor Element chain, downcasts each to `NotifiableElement<N>`, calls `on_notification(&self, &N) -> bool`, stops on first `true` return.
- R11. Each method's exact signature, error semantics, and any guard-vs-closure variant decisions are deferred to planning (see Outstanding Questions).

**GlobalKey registry wiring (audit Finding #2)**

- R12. Plumb a `&mut BuildOwner` (or equivalent mutation handle) through `ElementBase::mount`, `ElementBase::unmount`, `ElementBase::update`. Each call site (in `Element<V, A, B>`, in all `ElementBehavior` impls, in `BuildOwner::build_scope`, in `ElementTree::reconcile_children`) must thread the reference.
- R13. On `mount`, if the view returns `Some(GlobalKey)` from `View::key()`, call `BuildOwner::register_global_key(key_hash, element_id)`.
- R14. On `unmount`, if the view had a GlobalKey, call `BuildOwner::unregister_global_key(key_hash)`. If the same key is re-mounted at a different parent slot in the same frame (state migration), the registry update must be `unregister + register` ordered such that the registry never points at a defunct `ElementId`.
- R15. `GlobalKey<T>::current_element(&self) -> Option<ElementId>` reads `BuildOwner::global_keys`. `current_state(&self) -> Option<&T::State>` (where `T: StatefulView`) chains through `current_element` and downcasts.

**InheritedBehavior on_update wiring (audit Finding #8)**

- R16. `InheritedBehavior::on_update` walks the dependent set populated by R4. For each `dep_id`, calls `BuildOwner::schedule_build_for(dep_id, dep_depth)`. Behavior identical to Flutter's `InheritedElement.notifyClients` at `flutter/lib/src/widgets/framework.dart:5118`.

**`flui-foundation` cleanup (audit Findings #5 + #6)**

- R17. Delete `MergedListenable`, `HashedObserverList`, `SyncObserverList`. Zero workspace consumers. Drop `dashmap` workspace dep (was used only for `HashedObserverList`).
- R18. Delete `flui-foundation::FluiError`. Zero workspace consumers; `FoundationError` covers the same surface.
- R19. Add `ChangeNotifier::dispose(&mut self)` matching Flutter's `flutter/lib/src/foundation/change_notifier.dart:376`. After dispose, calling `add_listener` / `notify_listeners` / `remove_listener` debug-asserts (release: no-op or `tracing::warn!`).
- R20. Audit the ID-type surface in `flui-foundation` (`AnimationId` / `FrameId` / `TaskId` / `TickerId` / ...). Move any with zero workspace consumers to `pub(crate)` or delete. Keep the in-use set (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`).

**`unimplemented!()` removal (audit Finding #7)**

- R21. `crates/flui-view/src/view/root.rs:487` no longer contains `unimplemented!()`. Either implement the path or delete it (planning decides — WidgetsBinding `attach_root_widget` has working plumbing at `binding.rs:563-571`; legacy paths likely dead).
- R22. `crates/flui-view/src/view/root.rs:494` same as R21.

**ElementBehavior common boilerplate extraction**

- R23. Each `ElementBehavior` impl (Stateless / Stateful / Inherited / Render / Proxy / ParentData / Animation) currently carries shared mount-update-unmount-dirty-propagation-child-reconcile boilerplate. Extract the shared paths to a common helpers module (location deferred to planning: `crates/flui-view/src/element/behavior_commons.rs` or inherent helpers on `Element<V, A, B>` are both viable). The constraint: post-extraction each behavior impl only carries behavior-specific logic, not common scaffolding.

---

## Acceptance Examples

- AE1. **Covers R4.** Given an Element tree `Root → InheritedView<MyTheme> → Padding → Text`, when `Text`'s `build()` calls `ctx.depend_on::<MyTheme>()`, it returns `Some(&MyTheme)` AND the `InheritedElement<MyTheme>` now has the `Text` element's `ElementId` in its dependent set.
- AE2. **Covers R4, R16.** Given AE1's state, when the `InheritedView<MyTheme>` rebuilds with a new value such that `update_should_notify(old, new)` returns `true`, the `Text` element is marked dirty AND rebuilds in the next frame.
- AE3. **Covers R5.** Given AE1's tree, when `Text`'s `build()` calls `ctx.get_inherited::<MyTheme>()`, it returns `Some(&MyTheme)` AND the `InheritedElement<MyTheme>` dependent set is unchanged (no dependency recorded).
- AE4. **Covers R13, R14, R15.** Given `let k = GlobalKey::<MyState>::new()` constructed before any mount, when a `StatefulView` carrying `k` as its `View::key` is mounted, `k.current_element()` returns `Some(element_id)` AND `k.current_state()` returns `Some(&MyState)`. After parent rebuild moves the keyed subtree to a different parent slot in the same frame (state migration), `k.current_state()` continues to return the same `&MyState` (Flutter parity at `flutter/lib/src/widgets/framework.dart:5550+` `_inactiveElements.add`).
- AE5. **Covers R14.** Given AE4's state, when the keyed view is fully unmounted (no re-mount in the same frame), `k.current_element()` returns `None`.
- AE6. **Covers R10.** Given `Root → NotificationListener<ScrollNotification> → Inner`, when `Inner`'s `build()` calls `ctx.dispatch_notification(ScrollNotification::new(...))`, the `NotificationListener`'s `on_notification` is called with `&ScrollNotification`, AND the bubble stops if the listener returns `true`. Root is not notified.
- AE7. **Covers R1.** `View::key()` signature returns `Option<&dyn flui_foundation::ViewKey>`. A `GlobalKey` / `ValueKey` / `UniqueKey` / `ObjectKey` instance can be returned from a concrete `View` impl with no `as` cast outside the `View::key` body, no type error.
- AE8. **Covers R2.** `use flui_view::prelude::*; use flui_tree::prelude::*;` together compile cleanly; `IndexedSlot` resolves unambiguously to `flui_tree::IndexedSlot`.
- AE9. **Covers R3.** `flui-foundation::TargetPlatform` (or the chosen surviving home) is the single name referenced from every workspace crate. The deleted home is not imported anywhere.
- AE10. **Covers R17.** `cargo tree -p flui-foundation` does not list `dashmap`. `rg 'MergedListenable|HashedObserverList|SyncObserverList' crates/` returns zero non-doc-comment hits.
- AE11. **Covers R18.** `rg 'FluiError' crates/` returns zero non-doc-comment hits.
- AE12. **Covers R19.** A test that calls `ChangeNotifier::dispose` then `add_listener` panics in debug mode AND logs a warning in release mode.
- AE13. **Covers R21, R22.** `rg 'unimplemented!\(\)' crates/flui-view/src/view/root.rs` returns zero hits.
- AE14. **Covers R23.** Inspect `crates/flui-view/src/element/behavior/*.rs` — each behavior impl carries only its behavior-specific path. Mount / unmount / update / dirty / reconcile common code lives in one shared module.

---

## Success Criteria

- **Human outcome (the user-facing UI API works).** A view author writing `ctx.depend_on::<MyTheme>()` in a real `build()` gets `Some(&MyTheme)` from the nearest ancestor `InheritedView<MyTheme>`. A `GlobalKey` constructed before mount lets the holder reach the keyed state after mount. `View::key()` accepts any of the four canonical key types without `as` casts. Tests written against AE1–AE14 all pass.
- **Workspace integrity.** `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib`, `bash scripts/port-check.sh -v` all green. Each commit on the PR compiles + tests pass independently (workspace-compile invariant per [PR #82](https://github.com/vanyastaff/flui/pull/82) precedent).
- **Downstream-agent handoff quality.** The next ce-plan invocation can derive ~25–35 atomic implementation units from this doc directly. Each behavioral-conditional requirement has at least one matching AE (verified by the R-ID → AE-ID back-references).
- **Architectural cleanup.** Audit doc Findings #1, #2, #3, #5, #6, #7, #8 marked complete with this PR's merge commit hash in `docs/research/2026-05-21-view-tree-foundation-audit.md` Part III status annotation. Finding #4 (flui-tree migration) deferred to the next multi-PR series.

---

## Scope Boundaries

- **`flui-tree` migration to unified API (audit Finding #4).** Out of scope. Production consumers (`flui-rendering`, `flui-layer`, `flui-semantics`, `flui-view` outside the IndexedSlot migration in R2) stay on their bespoke traversals. Multi-PR series planned separately after framework spine is stable. **Exception:** R2's IndexedSlot collision resolution migrates ONE `flui-view` consumer to `flui-tree::IndexedSlot` — a small precursor of #4, not the full thing. R2 is allowed because the duplicate-type collision blocks downstream API, and the migration touches one concrete site, not the whole crate.
- **Widget layer materialization.** Out of scope. No new concrete widgets (`Text`, `Container`, `Padding`, `Stack`, …). This PR repairs the abstractions widgets depend on; widget impls come after.
- **Audit Finding #10 "Defer" items.** Out of scope. `BuildScope` nesting beyond what `BuildOwner` already does, `NotificationListener<T>` widget impl (the dispatch path R10 is in scope, but the widget that consumes it is not), `InheritedNotifier`, `InheritedModel`, `FocusManager`, distinct `LabeledGlobalKey` / `GlobalObjectKey` types. Wait for widget-layer consumers.
- **Performance optimization of registries.** HashMap-based `BuildOwner::global_keys` stays HashMap. No benchmark-driven swap to alternative data structures in this PR.
- **New ID types in `flui-foundation`.** Out of scope. R20 only audits existing surface for deletion candidates; no additions.
- **Telemetry / metrics.** Per [`STRATEGY.md`](../../STRATEGY.md) "Not working on": no `tracing::info!`-shaped lifecycle metrics added to BuildContext / GlobalKey hot paths. Existing `tracing::trace!` calls stay.
- **ABI / public-API breakage allowance.** The project has not shipped; breaking changes to public API surface (e.g., `View::key` signature, deletion of `FluiError`, deletion of `MergedListenable`) are explicitly OK. Conventional-commit type for those units is `feat!` or `refactor!` per Conventional Commits 1.0.

---

## Key Decisions

- **Bundle all six audit findings (#1, #2, #3, #5+#6, #7, #8) plus behavior commons extraction into one PR.** Per user directive "лучше все в одном а то ты разбиваешь задачу и не помнишь потом что надо доделать в следующем" + memory `no-quick-wins-vanyastaff`. The findings share the same Element-lifecycle surface — splitting forces double passes through the same code.
- **Type collisions (#3) ordered FIRST in the unit sequence.** `GlobalKey` impls `flui_foundation::ViewKey` but `View::key()` returns the view-local `ViewKey`. Until R1 lands, #2 cannot wire `GlobalKey` into the `View::key()` return path. Sequencing: R1 → R2 → R3 → then the rest.
- **`flui-tree` is the canonical home for `IndexedSlot` (R2).** Per [memory `flui-tree-unified-interface-intent`](../../../../.claude/projects/C--Users-vanya-RustroverProjects-flui/memory/flui-tree-unified-interface-intent.md) and [`STRATEGY.md`](../../STRATEGY.md) "Behavior loyal, structure Rust-native". Migration direction: consumers go TO `flui-tree`, not away. R2 is one such migration step.
- **`ChangeNotifier::dispose` (audit Finding #6) bundled with foundation cleanup unit.** Adjacent surface to R17/R18. Not bundling would mean another PR for ~50 LOC of strictly-related work, which would be the "quick win" pattern the user explicitly forbade.
- **Behavior commons extraction (R23) happens during the BuildContext/GlobalKey plumbing pass, not after.** The plumbing pass rewrites the same Element-lifecycle paths that hold the duplicated boilerplate. Extracting then-and-there avoids a second pass through the same code.
- **No `dyn` proliferation.** Per Constitution Principle 4 + CLAUDE.md § "Engineering Standards & Subagent Dispatch" — `BuildContext` callback methods may take generic `T: 'static` bounds + `TypeId` lookup rather than `dyn` everywhere; planning decides exact shape.

---

## Dependencies / Assumptions

- **Flutter source available** at `.flutter/flutter-master/packages/flutter/lib/src/` (gitignored, lives at the main repo root, not in this worktree). All Flutter behavior references cite that location.
- **Audit doc is the truth source.** `docs/research/2026-05-21-view-tree-foundation-audit.md` Findings #1, #2, #3, #5, #6, #7, #8 are the in-scope set. Findings #4 + #9 + #10 are explicitly out of scope.
- **Workspace state at start of execution.** Branch will be created from `origin/main` post-PR #83 merge (commit `bf8f5223`). Verification gates run from that base.
- **flui-tree's `IndexedSlot` API surface is suitable for `flui-view`'s `ElementSlot = IndexedSlot<Option<ElementId>>` use case.** Assumption: `flui-tree::IndexedSlot<I>` carries the generic that lets it model `Option<ElementId>` payload. Planning to verify by reading `crates/flui-tree/src/iter/slot.rs`.
- **No downstream crate outside `flui-platform` consumes `flui-foundation::TargetPlatform`.** Assumption to verify during planning via `rg 'flui_foundation::.*TargetPlatform|flui_foundation::platform' crates/`. If wrong, the deletion ripples wider.

---

## Outstanding Questions

### Deferred to Planning

- [Affects R3][Needs research] Which `TargetPlatform` survives — `flui-types::platform::target_platform` or `flui-foundation::platform`? Planning to grep workspace for consumers + pick the one already wired.
- [Affects R4–R10][Technical] Exact `ElementBuildContext` method signatures — callback-form (`with_inherited<T, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R>`) vs reference-returning (`depend_on<T>(&self) -> Option<&T>`) vs guard-returning (`InheritedGuard<'_, T>`). Each has lifetime + borrow trade-offs. Decide during planning after reading `flutter/lib/src/widgets/framework.dart:5028+` for the exact dependency-recording semantics.
- [Affects R12][Technical] Mutation handle through `ElementBase::mount` / `unmount` / `update`. Direct `&mut BuildOwner` vs closure-based `BuildOwner::with_mut(|owner| ...)` vs interior-mutability via `parking_lot::Mutex` already on `WidgetsBindingInner`. Decide during planning.
- [Affects R20][Needs research] Which IDs in `flui-foundation` have zero workspace consumers? Planning to run `rg 'flui_foundation::(AnimationId|FrameId|TaskId|TickerId|...)' crates/` exhaustively.
- [Affects R23][Technical] Behavior commons home — new module `crates/flui-view/src/element/behavior/commons.rs` vs inherent methods on `Element<V, A, B>` vs free helpers in `element/mod.rs`. Decide during planning by surveying the shared paths and picking the location with the lowest re-export surface.
- [Affects R18][Technical] `FluiError` removal — straight delete vs `pub(crate)`-demote. If any pub-crate-internal call sites exist (audit hasn't verified), the demote path may be cheaper. Planning to verify.
- [Affects R10][Technical] `NotifiableElement<N>` trait shape — the existing `ElementBuildContext::on_notification` hook needs the trait + downcast strategy designed. Likely a `Box<dyn Any>`-shaped match in `dispatch_notification` since `Notification` is type-parameterized at the call site. Decide during planning.
- [Affects R14][Technical] State-migration timing for GlobalKey re-mount in the same frame. Flutter handles via `_inactiveElements` at `flutter/lib/src/widgets/framework.dart:5550+` — planning needs to verify whether FLUI's `Lifecycle::Inactive` already carries the equivalent semantics or needs adding.
