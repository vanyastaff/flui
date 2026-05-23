# Architectural Contracts Audit — Flutter→Rust Port

**Date:** 2026-05-22
**Author:** API contract architect (research input)
**Scope:** read-only audit; defines the public-surface contracts that gate the widget-catalog roadmap.
**Status:** research — changes nothing; one of four parallel inputs to the master ROADMAP.

---

## Intro — why these contracts gate the roadmap

FLUI's render machine is largely built (`flui-rendering`, `flui-engine`, `flui-layer`, `flui-painting`). The user-facing layer — the widget catalog, Material, Cupertino — is ≈0%, and `flui-widgets` does not exist. Before that mass-construction begins, a small set of **API contracts** must be locked, because each is touched by *every* widget that will ever be written. Getting one wrong does not cause a local bug; it forces a rewrite of the entire catalog built on top of it.

This document audits nine such contracts. The throughline finding: **the existing `flui-view` View/Element surface was built before the constitution's "compile-time over runtime" and refusal-trigger-#6 rules were fully internalized.** The trait surface is honest Flutter-shaped Dart-in-Rust — `Box<dyn View>` everywhere, `downcast_rs`, `dyn_clone` — and it *works*, but it spends the type system instead of using it, and it locks in the exact ergonomics that will decide whether "users praise FLUI."

Three of the nine contracts must be re-decided and locked before any widget code is written. The rest can evolve. The ranked verdict is at the end.

A note on the constitution tension that recurs below: refusal trigger #6 (`docs/PORT.md:63-69`) forbids `Box<dyn View>` *stored in element child collections*, and its regex targets only `crates/flui-view/src/element/child_storage.rs` struct fields. The current code satisfies the **letter** of the trigger (child storage holds `Box<dyn ElementBase>`, not `Box<dyn View>`) while the **spirit** — keep `dyn` out of the reconciliation hot path — is already violated. That gap is the heart of Contracts 2 and 3.

---

## Contract 1 — Reactivity / state model

### (a) Current FLUI state

Two non-integrated systems exist.

**System A — Flutter-faithful `StatefulView` + `ViewState`.** `crates/flui-view/src/view/stateful.rs:71-136`. A `StatefulView` has `type State: ViewState<Self>`; `ViewState` carries the Flutter lifecycle (`init_state`, `did_change_dependencies`, `build`, `did_update_view`, `deactivate`, `activate`, `dispose`). State lives in `StatefulBehavior<V>` (`crates/flui-view/src/element/behavior.rs:278-282`), held by the unified `Element<V, Single, StatefulBehavior<V>>`. `setState` is `Element::set_state` (`crates/flui-view/src/element/unified.rs:407-413`): mutate the state, then `core.mark_dirty()`.

Dirty marking is lock-free: `ElementCore::dirty` is `Arc<AtomicBool>` (`crates/flui-view/src/element/generic.rs:125`), and `create_mark_dirty_callback()` (`generic.rs:532-537`) hands out an `Arc`-captured closure so a listener can mark dirty without `&mut`. Rebuild propagation: `BuildOwner` keeps a `BinaryHeap<Reverse<DirtyElement>>` ordered shallow-first (`crates/flui-view/src/owner/build_owner.rs:52-57, 88`).

**System B — `flui-reactivity` signals.** `crates/flui-reactivity/src/`. A full SolidJS/Leptos-style system: `Signal<T>` (Copy, thread-local runtime, `signal.rs:54-83`), `Computed`, `use_memo`/`use_effect`/`use_reducer` hooks, `batch()`, `Owner` scopes. **The crate is DISABLED** in the workspace (`docs/PORT.md:200`) and has zero integration points into `flui-view` — no `flui-view` file imports it; `flui-reactivity/src/lib.rs:131` even has `// TODO: Uncomment when flui_foundation is available`. It is an unintegrated parallel universe.

### (b) What Flutter does

Flutter has exactly one model: `StatefulWidget` + `State<T>` + `setState`. `State.setState` marks the element dirty; `BuildOwner` drains the dirty list shallow-first each frame; `InheritedWidget` handles cross-tree propagation. There are no signals. FLUI's System A is a faithful port; System B is an *addition* Flutter never had.

### (c) Decision & options

The decision is **what is the canonical state primitive a widget author reaches for**, and **whether `flui-reactivity` integrates, gets deleted, or stays an optional side-car.**

- **Option 1 — `StatefulView`/`setState` is canonical; `flui-reactivity` deleted or kept as an unintegrated opt-in side-car.** Pure Flutter parity. Every `.flutter/` algorithm ports 1:1. STRATEGY.md "Not working on → Реинвент Flutter widget tree mental model" explicitly forbids "сделать лучше через React signals." Tradeoff: `setState` rebuilds the whole subtree under the `State` — no fine-grained reactivity. That is precisely Flutter's behavior and Flutter ships at 60fps, so the tradeoff is proven.
- **Option 2 — Hybrid: `setState` is the lifecycle spine, signals are an *element-scoped* fine-grained layer.** `flui-reactivity`'s `Owner` scope binds to an `ElementId`; a signal read inside `build()` registers the element as a subscriber; a signal write schedules that element via the same `BuildOwner` dirty heap. This is the Dioxus/Leptos-inside-a-retained-tree model. Tradeoff: two mental models for users ("when do I use `State` vs `Signal`?"), two dirty-propagation paths to keep coherent, and it directly contradicts the STRATEGY clause above.
- **Option 3 — Signals replace `State` entirely.** Rejected on sight: violates STRATEGY ("откатывается к Flutter-семантике"), throws away the faithful `ViewState` FSM, and breaks the Flutter-parity port mandate.

### (d) Risk if deferred or wrong

Highest-blast-radius contract. The state primitive is in the signature of *every stateful widget*. If the catalog is built on `setState` and signals are retrofitted later, every interactive widget is touched. If built on a hybrid and the second model is later removed, same. This cannot be "evolved" — it is a fork in the road, and the catalog commits to one branch at widget #1.

### (e) Recommendation

**Option 1.** Make `StatefulView`/`ViewState`/`setState` the sole canonical model for the catalog. The constitution and STRATEGY are unambiguous: FLUI ports Flutter, and Flutter has no signals. Keep `flui-reactivity` *out of the dependency graph of `flui-widgets`* — if it ships at all, it ships as an optional application-author convenience crate that internally drives `Element::mark_needs_build`, never as a primitive the catalog itself depends on. The widget catalog must be expressible with `setState` alone, exactly as Flutter's is.

Lock the rebuild-propagation contract now in one sentence: *a state mutation marks exactly one element dirty (`AtomicBool` + `BuildOwner` heap insert); the frame drains the heap shallow-first; rebuilding an element re-runs `build()` and reconciles its children.* That sentence is already true of the code — it just needs to be *declared* so the catalog can rely on it.

### (f) Verdict

**MUST LOCK before construction.** It is the single highest-blast-radius contract. The decision is cheap (the code already implements Option 1; the work is *deleting the temptation* of System B from the catalog's reach), but it must be explicit so no widget author reaches for `Signal`.

---

## Contract 2 — The `View` trait & the `dyn` boundary

### (a) Current FLUI state

`View` (`crates/flui-view/src/view/view.rs:49-95`) is:

```rust
pub trait View: Downcast + DynClone + Send + Sync + 'static {
    fn create_element(&self) -> Box<dyn ElementBase>;
    fn view_type_id(&self) -> TypeId { TypeId::of::<Self>() }
    fn can_update(&self, old: &dyn View) -> bool { ... }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> { None }
}
impl_downcast!(View);
clone_trait_object!(View);
```

It is **fully object-safe and `dyn`-first by construction.** `downcast_rs` provides `as_any`/`downcast_ref`; `dyn_clone` provides `clone_box`. `create_element` *returns* `Box<dyn ElementBase>`. The element tree (`ElementNode.element: Box<dyn ElementBase>`, `crates/flui-view/src/tree/element_tree.rs:20`) is a `Slab<ElementNode>` of trait objects. Child storage holds `Option<Box<dyn ElementBase>>` / `Vec<Box<dyn ElementBase>>` (`crates/flui-view/src/element/child_storage.rs:216, 457`).

The generic `Element<V, A, B>` (`crates/flui-view/src/element/unified.rs:52-64`) *is* generic — but it is monomorphized only to be **immediately re-erased** to `Box<dyn ElementBase>` at `create_element`. The generic dispatch the constitution wants exists for exactly one stack frame and is then thrown away.

Refusal trigger #6 says child *element* collections must not store `Box<dyn View>`. They store `Box<dyn ElementBase>` instead — so the trigger's regex passes. But `ElementCore::update_or_create_child` takes `child_view: Box<dyn View>` (`generic.rs:309`), and there is an explicit comment at `generic.rs:302-306` admitting the single-line signature is a deliberate dodge of the `port-check.sh` trigger-6 regex. The spirit of the trigger — no runtime-typed boundary in reconciliation — is not honored.

### (b) What Flutter does

Flutter's `Widget` is an abstract class; `Element` is an abstract class; everything is dynamic dispatch because Dart has nothing else. `Widget.createElement()` returns an `Element`. `canUpdate` is a static `runtimeType == runtimeType && key == key` check (`widgets/framework.dart:382-384`). FLUI's `View` is a near-exact transliteration. **This is Dart-shaped, not Rust-shaped** — and the port mandate (STRATEGY "structure Rust-native") says the *shape* should diverge even when the *behavior* is copied.

### (c) Decision & options

The unavoidable fact: a UI tree is heterogeneous and dynamically shaped — a `Column` does not know its children's types at compile time, and the retained element tree must hold mixed node types in one `Slab`. **Some `dyn` boundary is unavoidable.** The contract decision is *where exactly* the boundary sits and *how thin* it is.

- **Option 1 — Status quo: `dyn` everywhere (`View` object-safe, `Box<dyn ElementBase>` is the storage type).** Honest, simple, already working, 33 `impl View` sites exist. Tradeoff: `downcast_rs` + `dyn_clone` are a tax; every child update is a virtual call + a `downcast_ref::<V>()` that *can fail at runtime* (`generic.rs:271-285` logs `tracing::warn!` on downcast failure — a type error that should be impossible is instead a silent warning). Spends none of the type system. Directly contra Constitution Principle 4 and STRATEGY "compile-time over runtime."
- **Option 2 — Generic element, single erasure point at the `Slab` node.** `View` stays object-safe (it must, for `create_element` and `Children`), but the *element* side becomes properly generic: `Element<V, A, B>` is the working type, and the **only** `dyn` boundary is the `Slab<ElementNode>` storage, where `ElementNode` holds an `enum` over the closed set of element *behaviors* (Stateless/Stateful/Proxy/Inherited/Render/Animation are a known, finite set — see `UNIFIED_ELEMENT.md`) rather than `Box<dyn ElementBase>`. Reconciliation dispatches on the enum, not a vtable. Tradeoff: a large refactor of `flui-view` element storage; the enum must be `#[non_exhaustive]`-managed.
- **Option 3 — Sealed `View` hierarchy with an `enum ViewKind`.** `View` becomes a sealed trait; a top-level `enum AnyView { Stateless(...), Stateful(...), Render(...), ... }` is the erasure type. Tradeoff: closes the widget *kind* set (fine — it *is* closed, Flutter has 4 widget base classes) but is the heaviest change and least Flutter-shaped at the `View` surface.

### (d) Risk if deferred or wrong

Moderate-to-high. The `View` *trait surface* (`create_element`, `key`, `can_update`) is what 100% of widgets `impl`. If `View` itself is re-shaped after the catalog exists, all 33 current + every future `impl View` breaks. The *element storage* type is internal — re-shaping it later is a `flui-view`-internal refactor that does not touch widget authors. So: the **`View` trait signature** must be locked; the **element storage representation** can evolve behind it.

The real risk of leaving Option 1 in place: the runtime `downcast_ref::<V>()` in the update path (`generic.rs:271`) is a class of bug — a mismatched view silently fails to update instead of being a compile error — that the constitution's "compile-time over runtime" rule exists specifically to forbid. Every widget inherits that fragility.

### (e) Recommendation

**Lock the `View` trait surface now; commit to Option 2 for element storage and schedule it before the catalog.** Concretely:

- `View` stays object-safe (non-negotiable: `Children` and `create_element` need it). But drop `downcast_rs`/`dyn_clone` from the *public* bound surface where possible — keep `view_type_id()` (cheap, needed for `can_update`) and make cloning explicit via `create_element` taking `&self` (it already does). `dyn_clone` is needed only because `BoxedView` wants `Clone`; see Contract 3 for why `BoxedView` should mostly disappear.
- Element storage moves to `enum ElementNode { Stateless(Element<…>), … }` — a closed set, `#[non_exhaustive]`. The keyed reconciler (Contract 5) dispatches on the enum. This kills the failing `downcast_ref` in the update path: `ElementCore::update` becomes a typed call within the matched arm.

The litmus test for "is the `View` trait locked correctly": *can a `RenderObjectWidget` author write their widget without ever naming `Box<dyn View>` or `dyn`?* Today they cannot (see Contract 3). After the fix they should.

### (f) Verdict

**`View` trait signature: MUST LOCK before construction** (it is the universal `impl` target). **Element storage representation: can evolve** (internal), but should be scheduled *before* the catalog because the runtime-downcast fragility is a constitution violation every widget would inherit, and refactoring storage with 0 widgets is trivial vs. with 200.

---

## Contract 3 — Heterogeneous children ergonomics (THE crux)

### (a) Current FLUI state

This is the contract that decides whether users praise FLUI. The question: how does a user write `Column { children: [Text(...), Button(...), Image(...)] }` with mixed types?

Today the answer is `Children` (`crates/flui-view/src/child/children.rs:37-40`):

```rust
#[derive(Default)]
pub struct Children { inner: Vec<BoxedView> }
```

`Children::push(impl View)` boxes into `BoxedView(Box<dyn View>)` (`children.rs:66-68`). `BoxedView` (`crates/flui-view/src/view/into_view.rs:142`) is a `Box<dyn View>` newtype that itself `impl View` (`into_view.rs:158-174`) and `Clone` via `dyn_clone::clone_box` (`into_view.rs:144-148`). `Child` (single, `crates/flui-view/src/child/child.rs:33-35`) is `Option<BoxedView>`.

So a `Column` would be `struct Column { children: Children }`. A user writes:

```rust
Column::new()
    .child(Text::new("hi"))
    .child(Button::new("ok"))
    .child(Image::asset("logo"))
```

builder-style, each `.child()` boxes. There is **no array/`vec!` literal path** — `Children` is `FromIterator<V>` only for a *homogeneous* `V` (`children.rs:137-142`), so `vec![Text(...), Button(...)]` does **not** compile (mixed types). The user is forced into the chained-builder form or a manual `let mut c = Children::new(); c.push(...);`.

`RenderView::visit_child_views` (`crates/flui-view/src/view/render.rs:85-87`) is how a multi-child render widget exposes children to the element — it visits `&dyn View`. `RenderBehavior::perform_build` (`behavior.rs:475-480`) then `dyn_clone::clone_box`es each child into `Vec<Box<dyn View>>`. Every child crosses the `dyn` boundary twice (store boxed, clone boxed) per frame.

### (b) What Flutter does

`MultiChildRenderObjectWidget` has `final List<Widget> children` (`widgets/framework.dart:2048`). In Dart, `[Text(...), Button(...), Image(...)]` is a `List<Widget>` for free — every widget *is* a `Widget` and the list literal is heterogeneous-by-default. Flutter users write `Column(children: [Text(...), ElevatedButton(...), Image(...)])` and it is beautiful *because Dart has no monomorphization*. **Rust cannot copy this directly** — a `Vec<T>` is homogeneous. This is the single sharpest Dart↔Rust impedance mismatch in the whole port.

### (c) Decision & options

The contract: **what does a widget's `children` field look like, and what does the call site look like?** Refusal trigger #6's spirit says minimize `dyn` in the child path; ergonomics says the call site must not be ugly.

- **Option 1 — Status quo `Children` (Vec of `BoxedView`), builder-only call site.** `.child(x).child(y).child(z)`. Works today. Tradeoff: no `[...]` literal; every child boxed; `dyn_clone` per frame; the chained form gets verbose for 8-child layouts and reads worse than Flutter's list. **This is "acceptable, not praised."**
- **Option 2 — Tuple-based heterogeneous children via a `ViewTuple`/`ViewList` trait** (the Xilem / `bevy_ui` / iced-`row!` approach). `Column::new((Text::new("hi"), Button::new("ok"), Image::asset("logo")))` — a tuple `(A, B, C)` where `impl ViewTuple` is provided by macro for arities 0..=16. Each element keeps its *concrete type* through layout; the tuple is monomorphized; erasure happens once, at element creation, into the `Slab`. A `column![a, b, c]` macro can wrap the tuple for `vec!`-like syntax. Tradeoff: tuple arity cap (16 is standard and fine); a child-count-varying list (`for` loop producing children) still needs a `Vec<BoxedView>` fallback — so you need *both* a tuple path (static, common case) and a `Vec` path (dynamic). Xilem ships exactly this and it is well-liked.
- **Option 3 — Keep `Vec<BoxedView>` but add a `views![...]` macro that boxes each element.** `Column::new(views![Text::new("hi"), Button::new("ok")])` where `views!` expands to `{ let mut v = Children::new(); v.push(a); v.push(b); v }`. Tradeoff: trivial to add, fixes *only* the call-site syntax, keeps every per-frame `dyn` cost. It is Option 1 with sugar.

### (d) Risk if deferred or wrong

**This is the highest *ergonomics* risk in the entire port and one of the highest overall.** `children` appears in `Column`, `Row`, `Stack`, `Wrap`, `Flex`, `ListView`, `CustomMultiChildLayout`, `Table` — the spine of every real UI. If the catalog is built on Option 1's builder-only `Children`, *every multi-child widget in Material and Cupertino bakes that call site in*, and the public examples all read worse than the Flutter they are copied from. Changing it afterward is not a refactor — it is re-typing the `children` field of every multi-child widget and re-writing every example and doc. STRATEGY's success metric is literally "external PR contributors" and "sample apps build pass-rate" — an ugly `children` API suppresses both.

### (e) Recommendation

**Option 2, with the `Vec` path retained as an explicit fallback.** Decide and lock *now*, before `Column` is written:

- A `ViewSeq` (Xilem calls it `ViewSequence`) trait, macro-impl'd for tuples `()`..`(A..P)`. A multi-child widget is generic: `struct Column<C: ViewSeq> { children: C, ... }`. The static path keeps every child's concrete type to the `Slab` boundary — zero `BoxedView`, zero per-frame `dyn_clone`, and the keyed reconciler (Contract 5) can be monomorphic.
- A `column! { a, b, c }` macro for the literal call site — expands to the tuple. This is the form examples and docs use; it reads `column![ Text(...), Button(...), Image(...) ]`, as clean as Flutter.
- A blanket `impl<V: View> ViewSeq for Vec<V>` and an `impl ViewSeq for Vec<BoxedView>` for the genuinely-dynamic case (a `for` loop building rows). The user opts into boxing *only* when the child count is dynamic — exactly where Flutter would also lose its homogeneity benefit.

This is the one contract where FLUI must *not* port Flutter's *structure* — Dart's `List<Widget>` cannot be copied. It must port the *feel* (`column![...]` reads like `Column(children: [...])`) on a Rust-native tuple spine. STRATEGY's "structure Rust-native" clause was written for exactly this case.

`Children`/`BoxedView`/`Child` as they exist today should be demoted to the dynamic-fallback implementation detail, not the primary surface.

### (f) Verdict

**MUST LOCK before construction — top priority.** It needs its own design doc / `/speckit.plan` (see closing section). It is the contract most likely to make-or-break "users praise FLUI," and the one most expensive to change after the catalog exists.

---

## Contract 4 — `BuildContext` shape

### (a) Current FLUI state

`BuildContext` (`crates/flui-view/src/context/build_context.rs:49-270`) is an object-safe trait — `&dyn BuildContext` is passed to `build()`. It exposes: identity (`element_id`, `depth`, `mounted`, `is_building`); `owner()`; inherited lookup (`depend_on_inherited` / `get_inherited`, both **callback-form**: `&mut dyn FnMut(&dyn Any)`); ancestor finders (`find_ancestor_view` / `find_ancestor_state` / `find_root_ancestor_state`, also callback-form); `find_render_object`; `visit_ancestor_elements` / `visit_child_elements`; `mark_needs_build`; `dispatch_notification`.

The typed sugar is `BuildContextExt` (`build_context.rs:273-425`), a blanket `impl<C: BuildContext + ?Sized>`: `depend_on::<T, R>(|t| ...) -> Option<R>`, `get`, `find_ancestor`, `find_state`, `find_root_state`. The **callback form is deliberate and well-reasoned** — the doc at `build_context.rs:90-99` and `281-285` explains it preserves the declarative-build invariant (Constitution Principle 5) and prevents extending a `&self` borrow across the rest of `build()`. `InheritedView` lookup is O(1) via a `TypeId → ElementId` registry in `BuildOwner` (`build_owner.rs:104`, `inherited_elements`) — the one sanctioned runtime-reflection window (STRATEGY, `PORT.md:137`).

The lifetime is the catch: `BuildContext: Send + Sync` and the trait has **no lifetime parameter**. The concrete impl `ElementBuildContext` holds `Arc<RwLock<ElementTree>>` + `Arc<RwLock<BuildOwner>>` (`crates/flui-view/src/context/element_build_context.rs:47`, noted in `PORT.md:100` as allowed-but-friction). `StatelessBehavior::perform_build` constructs a *minimal* context via `ElementBuildContext::new_minimal(depth)` (`behavior.rs:222`) — i.e. during the actual build, the context is **not wired to the tree at all**. The fully-wired context and the build-time context are different code paths.

### (b) What Flutter does

In Flutter `Element implements BuildContext` — the element *is* the context. `dependOnInheritedWidgetOfExactType<T>()` returns `T?` directly (Dart has no borrow checker, so returning the widget is free). Ancestor finders return the widget/state directly. `framework.dart:5081` (`dependOnInheritedWidgetOfExactType`), `5122` (`findAncestorWidgetOfExactType`). FLUI's callback form is the *correct* Rust adaptation — returning `&T` would either need a lifetime tying the result into `build()` (fights the borrow checker on every use) or a clone (wasteful); the callback threads the borrow safely. This is a good port decision.

### (c) Decision & options

The trait shape is sound. The open contract questions are narrower:

- **Lifetime: `&dyn BuildContext` (no lifetime) vs `BuildContext<'a>` (borrowed).** Status quo `Arc<RwLock<...>>` interior mutability means no lifetime param is needed — but it pays an `RwLock` acquisition per tree access during build. A `BuildContext<'tree>` borrowing `&'tree ElementTree` would be lock-free and faster, at the cost of threading `'tree` through `build()` signatures.
- **The `new_minimal` gap.** During real builds the context is unwired. That is a *correctness hole*, not just a perf choice: `ctx.depend_on::<Theme>()` inside a `StatelessView::build` today cannot actually reach the tree, because the context handed to `perform_build` is minimal. This must be closed before any widget calls `ctx.depend_on`.
- **`Send + Sync` on `BuildContext`.** Build is sync and single-threaded (STRATEGY "sync hot path"). `Send + Sync` on the context is unnecessary and forces every captured thing to be `Send + Sync`. Dropping it would relax bounds on widget fields.

### (d) Risk if deferred or wrong

Moderate. The *callback-based trait surface* (`depend_on`, `find_ancestor`, …) is excellent and should be locked — it is what every widget calls and it is right. The **lifetime decision** is harder to change later: if widgets are written against `&dyn BuildContext` and FLUI later moves to `BuildContext<'tree>`, every `build()` signature changes. The `new_minimal` gap is a hard blocker — a `Theme`-consuming widget literally cannot work until it is closed.

### (e) Recommendation

- **Lock the callback-form trait surface now.** `depend_on_inherited`/`find_ancestor_*` callback shape + `BuildContextExt` sugar is the right Rust adaptation of Flutter's API; declare it stable.
- **Decide the lifetime before the catalog.** Recommendation: keep `&dyn BuildContext` *without* a lifetime parameter for the public `build()` signature (it keeps widget code clean and matches Flutter's "context is just a handle" feel), but fix the *implementation* so the wired context is what reaches `build()` — i.e. delete the `new_minimal` build path. Whether the impl uses `Arc<RwLock>` or an internal borrow is then a `flui-view`-internal perf decision that does not touch widgets.
- **Drop `Send + Sync` from `BuildContext`** — build is single-threaded; this is a free bound relaxation for widget authors.
- Closing `new_minimal` is a **prerequisite of Contract 7** (theming needs a working `depend_on`).

### (f) Verdict

**Trait surface: MUST LOCK before construction** (universal call surface; it is already right). **Lifetime representation + `new_minimal` fix: can evolve as representation, but the `new_minimal` correctness hole MUST be closed before any `InheritedWidget`-consuming widget is written** — which is essentially widget #1, because `Theme` is an `InheritedWidget`.

---

## Contract 5 — Widget→Element reconciliation contract

### (a) Current FLUI state

**There are two reconcilers and they are not the same algorithm.**

**Reconciler A — the real one, index-based.** `child_storage.rs`'s `VariableChildStorage::update_with_views` (`crates/flui-view/src/element/child_storage.rs:494-515`): pure positional match — `for (i, view) in views.iter().enumerate()`, update child `i`, push new ones, drain extras. The in-code comment at `child_storage.rs:495-496` says `// TODO: In a full implementation, this would use keys for reordering`. This is what `ElementCore` actually calls during build.

**Reconciler B — the keyed O(N) one, unused.** `crates/flui-view/src/tree/reconciliation.rs:51-193`, `reconcile_children`: the proper Flutter linear algorithm (match-from-start, match-from-end, keyed-middle via `HashMap`). It is re-exported (`lib.rs:192`) and tested (`tests/reconciliation_tests.rs`) but `grep` shows **zero production callers** — only `lib.rs`, `tree/mod.rs`, the file itself, and the test file. Worse, even Reconciler B is half-built: `reconciliation.rs:91-98` has a loop that admits *"we don't have direct access to the original View's key … This would need enhancement to store keys in ElementNode."*

**Keys.** Types exist: `ValueKey`/`UniqueKey`/`ObjectKey` in `flui-foundation` + `flui-view/src/key/object_key.rs`; `GlobalKey` (`crates/flui-view/src/key/global_key.rs`). `View::key()` returns `Option<&dyn ViewKey>` (`view.rs:92`). `ElementNode` *does* store `registered_global_key_hash` (`element_tree.rs:40`), and GlobalKey state migration / soft-remove / retake is genuinely implemented (`element_tree.rs:260-301, 350-396, 542-590`) — that part is solid. But **non-global keys (`ValueKey` etc.) are not stored on the element** — `ElementNode` has no general `key` field — so Reconciler B's keyed middle section cannot actually key on them. State preservation across reorder works *only* for `GlobalKey`, not `ValueKey`.

**Slot model.** `ElementBase::slot()` is a `usize` (`view.rs:126`); `RenderSlot` exists for the render side. Flutter's `IndexedSlot` (previous-sibling-aware) is re-exported (`lib.rs:172`) but the index-based reconciler does not use it.

### (b) What Flutter does

Flutter's reconciler lives in `RenderObjectElement.updateChildren` (`widgets/framework.dart:4125`) and `Element.updateChild` (`framework.dart:3982`). The linear algorithm: sync from top, sync from bottom, put the middle's keyed old children in a map, walk new middle, match by `Key` (any key — `ValueKey`, `GlobalKey`), inflate the rest. `canUpdate` (`framework.dart:382`) gates reuse on `runtimeType == runtimeType && key == key`. Crucially: **only `RenderObjectElement` runs `updateChildren`** — `StatelessElement`/`StatefulElement` have exactly one child and use the trivial single-child `updateChild`. FLUI's split (single-child storage vs variable storage) mirrors this; the bug is that the *variable* path is the unused-keyed-one, and the *real* variable path is positional-only.

### (c) Decision & options

The contract: **what is the reconciliation algorithm, and where do keys live?**

- **Option 1 — Adopt Reconciler B (keyed O(N)) as the real one; delete A; add a general `key` field to `ElementNode`.** Full Flutter parity. `ElementNode` gains `key: Option<Key>` set at insert from `View::key()`. `VariableChildStorage::update_with_views` calls into the keyed algorithm. Tradeoff: a real refactor — but Reconciler B is *already written*, it just needs wiring + the key field. This is finishing a job, not starting one.
- **Option 2 — Keep index-based A; document "no keyed reordering."** Cheap. Tradeoff: state is lost on any list reorder — a dismissed list item, a sorted table, a reordered tab bar all rebuild from scratch and lose scroll/focus/animation state. This is a *behavior* divergence from Flutter, which the port mandate (PORT.md "Flutter behaviour primacy") forbids. Unacceptable for a framework that ships `ListView`.
- **Option 3 — Hybrid: A for the common contiguous case, B when any child has a key.** Flutter effectively does this (the start/end sync *is* the fast path). Option 1 *is* this if implemented faithfully — Reconciler B already has the start/end fast paths. So Option 3 collapses into Option 1.

### (d) Risk if deferred or wrong

High. Reconciliation is the algorithmic heart of the framework. If the catalog is built while only Reconciler A exists, `ListView`/`Table`/`TabBar`/`ReorderableListView` are all written and tested against positional reconciliation — and they will *appear* to work in demos (static lists reconcile fine positionally) while silently losing state on every reorder. The bug surfaces in *user* apps, not FLUI's tests. Then fixing the reconciler changes observable behavior under every list widget. This is a "looks done, isn't" trap — the most dangerous kind.

### (e) Recommendation

**Option 1. Finish Reconciler B, wire it as the sole variable-arity reconciler, delete A, before the catalog.** The work:

1. Add `key: Option<flui_foundation::Key>` to `ElementNode`, set at `ElementTree::insert` / `mount_root` from `View::key()` (the `GlobalKey`-hash side-channel at `element_tree.rs:40` becomes the general-key field — it is half-there already).
2. `VariableChildStorage::update_with_views` delegates to `reconcile_children` (`reconciliation.rs`) instead of the positional loop.
3. Fix `reconciliation.rs:91-98` to read the now-stored key.
4. Unify slot handling on `IndexedSlot` so render-tree child moves are correct.

This must precede the catalog: it is far cheaper to finish the reconciler with zero list widgets than to re-validate every list widget after.

A note tied to Contract 3: if multi-child widgets become tuple-based (`ViewSeq`), the reconciler can be *monomorphic* per child position — the keyed-`HashMap` path is still needed for reorder, but the common contiguous case becomes statically-typed pairwise `update`. Contracts 3 and 5 should be designed together.

### (f) Verdict

**MUST LOCK before construction.** The algorithm and the key-storage location are load-bearing for every list/grid/table widget, and the current "two reconcilers, the real one is positional-only" state is a silent-correctness trap. Reconciler B existing-but-unwired means the cost to lock is *finishing*, not *designing* — but it must happen first.

---

## Contract 6 — Widget-authoring API ergonomics

### (a) Current FLUI state

To author a custom widget today (e.g. a `Greeting`), a user writes **three things**:

1. A `#[derive(Clone)] struct Greeting { ... }` — `Clone` is mandatory (`StatelessView: Clone`, `stateful.rs`/`stateless.rs`).
2. `impl StatelessView for Greeting { fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View> { ... } }` — note the **`Box<dyn View>` return type** the user must write and `.boxed()` into.
3. `impl View for Greeting { ... }` — boilerplate creating the element. There is a macro, `impl_stateless_view!` (`stateless.rs:66-75`), so in practice this is one line: `impl_stateless_view!(Greeting);`.

So the real authoring cost is: derive `Clone`, `impl StatelessView` (write `build`), invoke `impl_stateless_view!`. The same shape for `StatefulView` (+ a separate `State` struct + `impl ViewState`), `RenderView`, `ProxyView`, `InheritedView`.

`bon` is **not used** in `flui-view` or `flui-rendering` (confirmed: not in either `Cargo.toml`) despite CLAUDE.md listing it as the builder dependency. Constructors today are hand-written `new()` / chained `.child()` setters (see `Children`, `Child`).

The friction points for the author: (1) `build()` must return `Box<dyn View>` and the user writes `.boxed()` / `Box::new(...)` explicitly — Flutter just returns the widget; (2) the third `impl View` block, even macro'd, is a ritual with no meaning to the user; (3) no builder for widget *fields* — a `Container` with 12 optional fields is a hand-written builder or a 12-arg `new`.

### (b) What Flutter does

A Flutter widget is: `class Greeting extends StatelessWidget { const Greeting({super.key, required this.name}); final String name; @override Widget build(BuildContext context) => Text('Hi $name'); }`. One class, `build` returns `Widget` (no boxing), `createElement` is inherited from the superclass — the author never writes it. Named-optional constructor args are a Dart language feature, free. **Flutter's widget-authoring ergonomics are the gold standard the port is measured against.**

### (c) Decision & options

The contract: **what is the minimum a user types to author a widget, and how are widget constructors built?** Two sub-decisions: (i) the *trait* surface, (ii) the *constructor* story.

For (i) — the trait/`impl View` ritual:

- **Option 1 — Status quo: `impl SomeView` + `impl_*_view!` macro.** Works. Tradeoff: the user must know *which* `*View` trait + *which* macro; `build` returns `Box<dyn View>` with explicit boxing; two-step is more ritual than Flutter's one class.
- **Option 2 — A `#[derive(View)]` / `#[widget]` proc-macro.** `#[derive(StatelessView)]` generates the `impl View` block (and `Clone` if asked). The user writes the struct + `fn build`. Tradeoff: a proc-macro crate to maintain; but it collapses the ritual to Flutter-parity (struct + `build`). This is the Leptos `#[component]` / Dioxus approach and users like it.
- **Option 3 — Blanket `impl<V: StatelessView> View for V`.** Eliminate the `impl View` block entirely via a blanket impl. Tradeoff: Rust coherence — a type cannot be `StatelessView` *and* `StatefulView`, and a blanket impl per sub-trait collides. Workable only if the `*View` traits are mutually-exclusive-by-construction (sealed marker). Cleanest *if* it can be made coherent.

For (ii) — constructors: adopt `bon` (already a workspace dep per CLAUDE.md, just unused here). `#[derive(bon::Builder)]` on widget structs gives `Container::builder().width(10).color(red).build()` with compile-checked required fields. This is the constitution's stated intent ("Builder pattern via `bon`").

For `build()`'s return type: it should return `impl IntoView` (the trait *exists*, `into_view.rs:31-37`) — not `Box<dyn View>`. The user returns a concrete widget; `IntoView` erases at the boundary. Flutter-parity feel.

### (d) Risk if deferred or wrong

High — *for adoption*, which is FLUI's explicit success metric. This is "the single most-touched public surface" (the task's words). Every widget in the catalog, every Material/Cupertino widget, every user widget passes through it. If it ships as the three-step `Box<dyn View>`-returning ritual, *every* example in the docs is noisier than the equivalent Flutter, and the STRATEGY metric "external PR contributors" — gated on "mental model понятен снаружи" — suffers. Changing the authoring API after the catalog means re-writing every widget in the catalog.

### (e) Recommendation

Lock the authoring contract to this shape before the catalog, targeting **Flutter-parity verbosity** (struct + `build`, nothing else mandatory):

- `build()` returns **`impl IntoView`**, never `Box<dyn View>`. The user returns a concrete widget. (`StatelessView::build` signature changes — a breaking change, which the port mandate explicitly allows.)
- Provide a **`#[derive(StatelessView)]` / `#[derive(StatefulView)]` / `#[derive(RenderView)]` proc-macro** (Option 2) that generates the `impl View` block. Prefer Option 3 (blanket impl) *if* the `*View` traits can be made coherently mutually exclusive — investigate first; fall back to the derive. Either way the user never hand-writes `impl View`.
- Adopt **`bon`** for widget constructors: `#[derive(bon::Builder)]` on widget structs. Material/Cupertino widgets have many optional fields — this is non-negotiable for them.
- The litmus test: *a `StatelessWidget` port must be the struct + `build` and nothing else* — same line count as the Flutter original. If a FLUI widget is visibly noisier than its Flutter twin, the contract is not locked correctly.

### (f) Verdict

**MUST LOCK before construction.** It is the most-touched public surface and the direct determinant of the STRATEGY adoption metrics. It needs a dedicated design doc (the `build() -> impl IntoView` change, the derive-vs-blanket decision, the `bon` integration). Cheap to decide now, catalog-wide rewrite to change later.

---

## Contract 7 — Theming / `InheritedWidget` propagation

### (a) Current FLUI state

`InheritedView` (`crates/flui-view/src/view/inherited.rs:72-87`): `type Data`, `data() -> &Data`, `child() -> &dyn View`, `update_should_notify(&self, old) -> bool`. A `Theme` would be `impl InheritedView for Theme { type Data = ThemeData; ... }`.

Propagation is genuinely well-built and Flutter-faithful:

- O(1) descendant lookup: `BuildOwner.inherited_elements: HashMap<TypeId, ElementId>` (`build_owner.rs:104`). `InheritedBehavior::on_mount` registers; `BuildContext::depend_on_inherited` resolves by `TypeId`. No O(depth) walk. This is the sanctioned runtime-reflection window (STRATEGY).
- Dependents: `InheritedBehavior.dependents: HashMap<ElementId, usize>` (id → depth) (`behavior.rs:598`). `record_dependent` adds; `on_view_updated` (`behavior.rs:680-710`) calls `update_should_notify(old)` and, if true, `owner.schedule_build_for(dep_id, dep_depth)` for each — a faithful port of `InheritedElement.notifyClients` (`framework.dart:6414`).
- `InheritedBehavior` caches both `data` and a full `view_cache: V` clone (`behavior.rs:591`) so `depend_on` can hand `&V` to the callback.

The gaps: (1) `InheritedView::child()` returns `&dyn View` — so a `Theme` must *own* its child, and since `Theme: Clone`, the child must be `Clone` too; in practice the child field is `Box<dyn View>` or `BoxedView` (the doc example at `inherited.rs:48` literally shows `child: Box<dyn View>`). (2) **The `new_minimal` BuildContext gap from Contract 4** — during a real `StatelessView::build`, the context is not tree-wired, so `ctx.depend_on::<Theme>()` cannot currently reach the `inherited_elements` registry. The propagation machine is built; the *consumption path from inside `build()`* is not connected.

### (b) What Flutter does

`Theme` is an `InheritedWidget` (Flutter has no separate `InheritedView` kind). `Theme.of(context)` is `context.dependOnInheritedWidgetOfExactType<_InheritedTheme>()`. `BuildOwner` + `Element._inheritedElements` (a `PersistentHashMap<Type, InheritedElement>`) give O(1) lookup. `updateShouldNotify` gates dependent rebuilds. FLUI's design is a faithful port — the registry, the dependents set, `update_should_notify` all match.

### (c) Decision & options

The propagation *mechanism* is right and should be locked. Open questions:

- **`InheritedView::child()` ownership.** `&dyn View` forces the `Theme` to own + `Clone` its child. Once Contract 3 lands `ViewSeq`, `InheritedView` should hold its single child as a concrete generic `C: View` (`struct Theme<C> { data: ThemeData, child: C }`), erased once — not `Box<dyn View>`. Tradeoff: `Theme` becomes generic; acceptable and consistent with the rest of the Contract-3 direction.
- **`Theme` ergonomics — the `.of(context)` pattern.** Flutter's `Theme.of(context)` is a static method. FLUI's equivalent is `ctx.depend_on::<Theme, _>(|t| t.data.clone())`. A widget should be able to write `Theme::of(ctx)`. Decide: provide an `InheritedView::of(ctx)` convention (a provided trait method or a per-widget inherent `of`).
- **The `new_minimal` blocker** (Contract 4): until the build-time context is tree-wired, theming does not function. This is a *prerequisite*, not an option.

### (d) Risk if deferred or wrong

Moderate. The *mechanism* is built and faithful — low risk there. The risk is concentrated in: (1) the `new_minimal` gap, which makes theming non-functional until fixed — and theming is needed by approximately the *first* Material widget; (2) the `child()` ownership shape, which should move in lockstep with Contract 3 or it will be re-touched.

### (e) Recommendation

- **Lock the propagation mechanism now** — the `TypeId` registry + dependents-set + `update_should_notify` design is correct and Flutter-faithful; declare it stable.
- **Close the `new_minimal` gap** (shared with Contract 4) — non-negotiable prerequisite of any themed widget.
- **Reshape `InheritedView::child()` to a concrete generic child** alongside Contract 3 — do not lock it as `&dyn View`.
- **Establish the `Theme::of(ctx)` convention** as part of the Contract-6 authoring API — an `InheritedView` should expose an ergonomic typed accessor, not force every call site to write the `depend_on` closure.

### (f) Verdict

**Mechanism: MUST LOCK before construction** (it is right; just declare it). **`child()` ownership: must evolve in lockstep with Contract 3** — do not lock independently. **`new_minimal` fix: hard prerequisite** — theming is dead until it lands, and theming gates Material widget #1.

---

## Contract 8 — `build()` error handling

### (a) Current FLUI state

`build()` returns `Box<dyn View>` — **no `Result`** (`stateless.rs:49`, `stateful.rs:116`). The constitution forbids `unwrap()`/`panic!` in library code (CLAUDE.md Principle 6).

The fallback mechanism is `ErrorView` (`crates/flui-view/src/view/error.rs`): a `FlutterError { message, details, exception }` and a process-wide `static ERROR_VIEW_BUILDER: RwLock<Option<fn(&FlutterError) -> Box<dyn View>>>` (`error.rs:35-57`) — a port of Flutter's `ErrorWidget.builder`. So the *infrastructure* for "render an error widget instead of crashing" exists. But because `build()` cannot *return* an error, there is **no contractual way for a widget author to signal a recoverable build failure** — `build()` either produces a view or panics. The `ErrorView` path can only be reached by the framework catching a panic, or by a widget *manually* constructing an `ErrorView` and returning it.

### (b) What Flutter does

Dart `build()` returns `Widget` and signals failure by *throwing*. The framework wraps `build()` in a try/catch, and on a caught exception substitutes `ErrorWidget.builder(details)` in that element's slot — the rest of the tree survives. FLUI cannot "throw"; the Rust-native equivalent is a `Result` or a caught `panic`.

### (c) Decision & options

- **Option 1 — `build()` stays infallible (`-> impl IntoView`); framework catches `panic!` and substitutes `ErrorView`.** Closest to Flutter's *behavior* (one widget's failure is contained, tree survives). Implementation: wrap `perform_build` in `std::panic::catch_unwind`. Tradeoffs: `catch_unwind` requires `UnwindSafe` (manageable); panics in library code sit uneasily with Principle 6 *as a control-flow mechanism* — though Principle 6 targets *sloppy* `unwrap()`, not a deliberate framework-level safety net. Flutter-behavior-faithful.
- **Option 2 — `build()` returns `Result<impl IntoView, BuildError>`.** Rust-idiomatic; explicit; no `catch_unwind`. Tradeoffs: **every** `build()` in the catalog returns `Result` and ends with `Ok(...)` — pervasive noise on the most-written method, for an error case that is rare. Flutter widgets never do this. It makes the common case pay for the rare case.
- **Option 3 — Infallible `build()`; widgets that *can* fail return an `ErrorView` explicitly; no `catch_unwind`.** Simplest. Tradeoff: an *unanticipated* panic (an `unwrap` deep in a dependency) still takes down the frame — no containment. Weaker than Flutter.

### (d) Risk if deferred or wrong

Low-to-moderate, and **asymmetric**: the *signature* of `build()` is the issue. `Result` vs infallible is in every `build()` — changing it after the catalog is a catalog-wide edit. But the *fallback mechanism* (`ErrorView`, `catch_unwind` placement) is internal and can be added/changed late without touching widgets. So only the signature needs an early lock.

### (e) Recommendation

**Option 1: lock `build()` as infallible (`-> impl IntoView`); add panic-containment (`catch_unwind` around `perform_build` → `ErrorView`) as an internal mechanism that can land later.** Rationale: `build()` is the most-written method in the framework; forcing `Result` (Option 2) taxes every widget for a rare case and breaks Flutter-parity feel. Flutter's *behavior* — a failed widget is replaced by an error widget, the tree survives — is the port target (PORT.md "Flutter behaviour primacy"), and `catch_unwind` is the faithful Rust mechanism. Principle 6 forbids `unwrap()` as *lazy error handling*; a deliberate, documented framework-level panic boundary is a different thing and is the standard Rust pattern for exactly this (it is how every other Rust UI framework contains widget panics).

Lock now: **`build()` signature is infallible.** Defer: the exact `catch_unwind` placement and `ErrorView` styling.

### (f) Verdict

**`build()` signature (infallible): MUST LOCK before construction** — it is in every widget. **Panic-containment mechanism: can evolve** — internal, addable late.

---

## Contract 9 — Hot-reload ABI boundary

### (a) Current FLUI state

`flui-hot-reload` (`crates/flui-hot-reload/`) is **scene/restart-level, not state-preserving widget reload.** Two macros (`plugin.rs`):

- `scene_plugin!` — wraps `fn(f32,f32) -> Scene` in `extern "C"` symbols passing `Box::into_raw` pointers. No widget involvement.
- `app_plugin!` — wraps a `View + StatelessView` root in a `PluginPipeline`; on reload the `OnceLock` is fresh, so the pipeline **re-mounts from scratch** — `plugin.rs:104` calls this "hot restart semantics (code updated, state lost)."

So today hot-reload demands almost nothing of the View contract: the root must be `View + StatelessView`, that is all. There is **no stateful hot-*reload*** (Flutter's "keep `State`, re-run `build`"). The ABI surface is the three `extern "C"` fns; the boxed type crossing FFI is `flui_layer::Scene` — fully downstream of the widget tree.

### (b) What Flutter does

Flutter's hot reload re-runs `build()` on the whole tree while *preserving* `State` objects and the element tree. It works because the Dart VM swaps method bodies in place — `State` instances survive, `build` is re-invoked. A native-Rust port cannot do that: a recompiled `cdylib` has all-new type layouts; a `State` struct from the old `.so` cannot be reinterpreted as the new one. True Flutter-style stateful hot reload is **not achievable** with the `cdylib`-swap model — only hot *restart* (state lost) is.

### (c) Decision & options

The contract question: **does the View/widget contract need to carry anything stable across a reload, now?**

- **Option 1 — Hot *restart* only (status quo). The widget contract owes hot-reload nothing.** Honest about the `cdylib` constraint. Tradeoff: every reload loses state — worse DX than Flutter, but it is what the technology allows. STRATEGY lists DX/hot-reload as a day-one *track*, but does not promise *stateful* reload.
- **Option 2 — State-preserving reload via serialization.** `ViewState` gains a `serialize`/`deserialize` (or `Any`-based snapshot) contract; on reload, snapshot every `State`, re-mount, restore. Tradeoff: a **`Serialize` bound (or equivalent) on every `ViewState`** — that is a contract on the most-implemented trait in the framework. Heavy. And cross-`.so` type-identity is still fragile (field reorder breaks it).
- **Option 3 — Decide later; design `ViewState` so a snapshot hook *could* be bolted on.** Keep hot-restart now, but do not actively *preclude* a future `State` snapshot — e.g. do not make `ViewState` un-snapshot-able by design.

### (d) Risk if deferred or wrong

**Low.** This is the one contract on the list that does *not* gate the catalog. Hot-restart asks nothing of the widget contract beyond `View`. State-preserving reload, if ever pursued, would add a bound to `ViewState` — but that decision can be made after the catalog exists, and adding a *defaulted* snapshot hook to a trait is a far smaller change than re-shaping `View` or `children`. Nothing about the widget catalog is blocked by leaving this open.

### (e) Recommendation

**Option 1 / 3: keep hot-restart; do not impose any hot-reload-driven bound on `View` or `ViewState` now.** The `cdylib`-swap model makes Flutter-style stateful reload technically unreachable regardless; adding a `Serialize` bound to every `ViewState` to chase a partial version of it would tax the entire catalog for a feature that may never fully work. Build the catalog; if state-preserving reload is later prioritized, add a *defaulted, opt-in* `ViewState::snapshot` hook then. The only thing to consciously avoid is a `ViewState` design that is *structurally* un-snapshottable — and the current trait is not.

### (f) Verdict

**CAN EVOLVE.** The only contract here that genuinely does not need an early lock. Hot-restart is sufficient for the catalog; any state-preserving upgrade is a late, additive, defaulted change.

---

## Ranked verdict — "Must lock before construction" vs "Can evolve"

### MUST LOCK before construction starts

Ordered by blast radius (highest first):

1. **Contract 1 — Reactivity / state model.** The state primitive is in every stateful widget's signature; a fork that cannot be evolved. *Justification:* commit to `StatefulView`/`setState` (Flutter-parity, per STRATEGY) and keep `flui-reactivity` out of the catalog's dependency graph — decide once, at widget #1.
2. **Contract 3 — Heterogeneous children ergonomics.** `children` is the spine of every multi-child widget; the highest *ergonomics* risk and most expensive to change post-catalog. *Justification:* a `ViewSeq` tuple trait + `column!` macro must replace builder-only `Children` before `Column` is written, or every Material multi-child widget bakes in a worse-than-Flutter call site.
3. **Contract 6 — Widget-authoring API ergonomics.** The single most-touched public surface; the direct driver of STRATEGY's adoption metrics. *Justification:* `build() -> impl IntoView`, a `#[derive(StatelessView)]` (or blanket impl), and `bon` constructors must be locked so a FLUI widget is no noisier than its Flutter twin.
4. **Contract 5 — Widget→Element reconciliation.** Algorithmic heart; the current "two reconcilers, the real one is positional-only" is a silent-correctness trap under every list widget. *Justification:* finish-and-wire the already-written keyed reconciler + add a general `key` field to `ElementNode` before any list/grid/table widget is built.
5. **Contract 2 — `View` trait signature.** The universal `impl` target for all 33 current + every future widget. *Justification:* lock the object-safe `View` trait surface now; the runtime-`downcast_ref` fragility means element *storage* should also be re-shaped (Option 2) before the catalog, while there are 0 widgets to migrate.
6. **Contract 4 — `BuildContext` trait surface** *and* **the `new_minimal` correctness hole.** The callback-form surface is right and universal — lock it. The `new_minimal` gap makes `ctx.depend_on` non-functional during real builds. *Justification:* the trait surface is the universal build-time API; `new_minimal` must be closed because it blocks themed widget #1.
7. **Contract 8 — `build()` signature (infallible).** In every widget. *Justification:* lock `build()` as infallible (`-> impl IntoView`); forcing `Result` taxes every widget for a rare case.
8. **Contract 7 — `InheritedWidget` propagation mechanism.** The `TypeId`-registry mechanism is built and faithful — *declare it stable*. *Justification:* low-risk to lock (it is right); but its `child()` ownership must move with Contract 3 and it shares the `new_minimal` prerequisite.

### CAN EVOLVE (internal representation or late-additive)

- **Contract 2 — element storage *representation*.** The `enum`-vs-`Box<dyn ElementBase>` choice is `flui-view`-internal; re-shapeable behind a locked `View` trait. *(But schedule the re-shape before the catalog anyway — trivial at 0 widgets.)*
- **Contract 4 — `BuildContext` lifetime representation.** `&dyn BuildContext` vs `BuildContext<'tree>` is an internal perf decision behind the locked trait surface. *(The `new_minimal` *correctness* hole is NOT in this bucket — it must be fixed early.)*
- **Contract 7 — `InheritedView::child()` ownership.** Must evolve, but *in lockstep with Contract 3*, not independently.
- **Contract 8 — panic-containment mechanism.** `catch_unwind` placement + `ErrorView` styling are internal and addable late.
- **Contract 9 — Hot-reload.** The only contract needing no early lock. Hot-restart suffices for the catalog; state-preserving reload, if ever pursued, is a late defaulted-opt-in `ViewState` hook.

---

## Contracts that most need a dedicated `/speckit.plan` or design doc

Three contracts are too large and too consequential to resolve inside the ROADMAP itself — each needs its own design doc *before any widget code is written*:

1. **Contract 3 — Heterogeneous children (`ViewSeq` tuple trait + `column!` macro).** The single most important *and* most novel design — it is the one place FLUI must deliberately *not* port Flutter's structure (`List<Widget>`) and instead invent a Rust-native equivalent. It interlocks with Contract 5 (a tuple spine makes reconciliation partly monomorphic). Needs a full `/speckit.plan`: the trait, the tuple-arity macro, the `Vec` fallback, the `column!`/`row!` macros, and the reconciliation interaction.

2. **Contract 6 — Widget-authoring API (`build() -> impl IntoView`, derive-vs-blanket, `bon`).** The most-touched surface and the adoption-metric driver. Needs a design doc to settle: the `StatelessView::build` signature change (breaking), whether the `*View` traits can be made coherently mutually-exclusive for a blanket `impl View` vs. a `#[derive]`, and the `bon` integration pattern for many-optional-field widgets.

3. **Contracts 2 + 5 together — the `View`/`Element`/reconciliation core.** These three are one system: the `dyn` boundary, the element storage representation, and the reconciler all touch the same `ElementNode`/`Element<V,A,B>` types. A single design doc should cover: locking the `View` trait surface, the `enum ElementNode` storage re-shape, finishing the keyed reconciler, adding the general `key` field, and unifying on `IndexedSlot`. Designing them separately risks three incompatible refactors of the same files.

Contracts 1, 4, 7, 8, 9 can be decided directly in the ROADMAP — they are either "declare the existing design stable" (1, 4-surface, 7-mechanism, 8) or "defer" (9), and do not need standalone exploration.
