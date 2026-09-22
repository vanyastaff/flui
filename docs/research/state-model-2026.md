# State models in 2026 — sourced survey for ADR-0074 (realm-scoped signals)

Fetched 2026-09-22. Quotes are verbatim from the cited page; "unsourced" marks a claim
that could not be confirmed from a primary source in this pass and must not be cited
as fact. docs.rs pages are `latest`. Companion to
[ADR-0074](../adr/ADR-0074-realm-scoped-signals.md).

## 0. The question this survey answers

FLUI's retained View → Element → Render tree rebuilds an **Element subtree** when state
changes (`BuildOwner::build_scope`, depth-ordered dirty heap). The question is not "which
signal library is best" but: *which reactive-state shapes are compatible with a retained
tree whose unit of invalidation is the Element, and what did each system pay for
granularity?* The answer, read across seven systems, is in §9.

## 1. Dioxus 0.7 — `Signal<T>` on `generational-box`

Sources:
[Signal](https://docs.rs/dioxus-signals/latest/dioxus_signals/struct.Signal.html),
[crate index](https://docs.rs/dioxus-signals/latest/dioxus_signals/index.html),
[generational-box](https://docs.rs/generational-box/latest/generational_box/),
[generational-box/src/lib.rs](https://github.com/DioxusLabs/dioxus/blob/main/packages/generational-box/src/lib.rs),
[core/src/reactive_context.rs](https://github.com/DioxusLabs/dioxus/blob/main/packages/core/src/reactive_context.rs),
[0.7 guide: signals](https://dioxuslabs.com/learn/0.7/essentials/basics/signals/),
[0.7 guide: reactivity](https://dioxuslabs.com/learn/0.7/essentials/basics/reactivity/).

> "Signals are a Copy state management solution with automatic dependency tracking."
> "Signals are implemented with generational-box which makes all values Copy even if the inner value is not Copy."
> "Signals will only subscribe to components when you read from the signal in that component."
> "The lifetime of the signal is tied to the lifetime of the component it was created in. If you drop the component that created the signal, the signal will be dropped as well."
> generational-box: "The generational box will be dropped when the Owner is dropped." / "Using the location after this generation is invalidated will return errors."
> "UnsyncStorage: A unsync storage. This is the default storage type." / "SyncStorage: A thread safe storage. This is slower than the unsync storage, but allows you to share the value between threads."
> reactive_context.rs `mark_dirty`: "Marks this reactive context as dirty. If there's a scope associated with this context, then it will be marked as dirty too."
> guide: "Whenever a Signal's value is modified, a side-effect is queued that re-reruns any reactive contexts that read the signal's value." / "every component is a reactive context"

Facts: `Signal<T, S = UnsyncStorage>` is `Copy` because it is a `GenerationalBox` handle
(slot index + generation) into an arena of generational `RefCell`s; a stale handle fails
with an error (`BorrowError`/`ValueDroppedError`), not UB. Signals are owned by the
creating component's scope `Owner` and dropped with it (`Signal::new_in_scope` reassigns).
`.read()` subscribes the current `ReactiveContext` (every component, memo and effect is
one); a write marks each subscribed context dirty, and a scope-backed context enqueues
`SchedulerMsg::Immediate(scope_id)` — **only the subscribed components re-render**.
`peek` does not subscribe.

Relevance: the closest analogue to FLUI. Dioxus's *component* is our *Element*; "mark the
reactive context dirty → the scheduler re-renders that component" is `schedule_build_for`.

## 2. Leptos `reactive_graph` (0.7 / 0.8)

Sources:
[crate](https://docs.rs/reactive_graph/latest/reactive_graph/),
[Owner](https://docs.rs/reactive_graph/latest/reactive_graph/owner/struct.Owner.html),
[owner module](https://docs.rs/reactive_graph/latest/reactive_graph/owner/index.html),
[owner/arena.rs](https://github.com/leptos-rs/leptos/blob/main/reactive_graph/src/owner/arena.rs),
[owner/storage.rs](https://github.com/leptos-rs/leptos/blob/main/reactive_graph/src/owner/storage.rs),
[graph/node.rs](https://github.com/leptos-rs/leptos/blob/main/reactive_graph/src/graph/node.rs),
[graph module](https://docs.rs/reactive_graph/latest/reactive_graph/graph/index.html),
[RwSignal](https://docs.rs/reactive_graph/latest/reactive_graph/signal/struct.RwSignal.html),
[Memo](https://docs.rs/reactive_graph/latest/reactive_graph/computed/struct.Memo.html),
[Effect](https://docs.rs/reactive_graph/latest/reactive_graph/effect/struct.Effect.html).

> Owner: "A reactive owner, which manages 1) the cancellation of Effects, 2) providing and accessing environment data via provide_context and use_context, 3) running cleanup functions defined via Owner::on_cleanup, and 4) an arena storage system to provide Copy handles via ArenaItem"
> cleanup: "1) Runs cleanup on all children, 2) Runs all cleanup functions registered with Owner::on_cleanup, 3) Drops the values of any arena-allocated ArenaItems."
> owner.rs: "The 'current owner' is set on the thread-local basis"
> storage.rs: "all items stored in the arena must be `Send + Sync`, but in single-threaded environments you might want or need to use thread-unsafe types." LocalStorage: "stores the type with a wrapper that makes it `Send + Sync`, but only allows it to be accessed from the thread on which it was created." (`SendWrapper`)
> arena.rs: `static MAP: OnceLock<RwLock<ArenaMap>>`, `ArenaMap = SlotMap<NodeId, Box<dyn Any + Send + Sync>>`; feature `sandboxed-arenas` swaps in a `thread_local!` arena that "restores its associated arena as the current arena whenever it is polled".
> node.rs `ReactiveNodeState`: Clean — "either none of its sources have changed, or its sources have changed but its value is unchanged"; Check — "The node may have changed, but it is not yet known whether it has actually changed."; Dirty — "The node's value has definitely changed, and subscribers will need to update."
> `update_if_necessary`: "Regenerates the value for this node, if needed, and returns whether it has actually changed or not."
> Memo: "The memo will only notify its dependents if the value of the computation changes." / "Memos are lazy: they do not run at all until they are read for the first time"
> Effect: "Effects run after synchronous work, on the next 'tick' of the reactive system." / "Effects stop running when their reactive Owner is disposed."

Facts: the arena is **process-global** (`OnceLock<RwLock<SlotMap<…>>>`) unless
`sandboxed-arenas` makes it thread-local; the *current owner* is thread-local. `Copy`
handles are `NodeId`s; `Arc*` variants are refcounted. Storage: `SyncStorage` requires
`Send + Sync`, `LocalStorage` wraps in `SendWrapper` and panics cross-thread. Disposal is
an `Owner` tree, children first. Propagation is the three-state push/pull: write →
`mark_dirty` on the signal, `mark_check` on transitive subscribers; readers pull with
`update_if_necessary`; memos compare with `PartialEq` and stop propagation when unchanged.

Relevance: the three-state algorithm and the owner-tree disposal are the parts to take.
The global arena is the part ADR-0027 forbids — the equivalent of a57b4140's
`SIGNAL_RUNTIME`. The `sandboxed-arenas` feature shows the library authors themselves
needed per-context arenas (for SSR isolation), which is the realm-scoped shape.

## 3. Floem — `floem_reactive` on a retained view tree

Sources:
[floem](https://docs.rs/floem/latest/floem/),
[floem_reactive](https://docs.rs/floem_reactive/latest/floem_reactive/),
[README](https://github.com/lapce/floem/blob/main/README.md),
[View](https://docs.rs/floem/latest/floem/trait.View.html),
[ViewId](https://docs.rs/floem/latest/floem/struct.ViewId.html),
[views/label.rs](https://github.com/lapce/floem/blob/main/src/views/label.rs).

> "Floem uses its own reactive system with an API that is similar to the one in the leptos_reactive crate."
> README: "The view tree is constructed only once, safeguarding you from accidentally creating a bottleneck in a view generation function."
> "even though the tree is built only once, views can still receive reactive updates."
> View: "For all reactive state that your type contains, either in the form of signals or derived signals, you need to process the changes within an effect."
> ViewId: "This id is how you can access and modify a view, including accessing children views and updating state." `request_layout`, `request_paint`, `update_state`.
> label.rs: `UpdaterEffect::new(move || label().to_string(), move |new_label| id.update_state(new_label))`

Facts: own crate, leptos-inspired API (`RwSignal`, `create_effect`, `Memo`, `Scope`,
`batch`, `untrack`, `Runtime`). The view tree is retained and built once; each view holds
its `ViewId`; an effect re-runs on signal change and pushes the new value through
`id.update_state`, whose `View::update` then calls `request_layout`/`request_paint`. No
diffing, no component re-run.

Relevance: proof that a **retained** tree and signals coexist. Floem's granularity is a
*view id*, one level below FLUI's Element (there is no rebuild step); the shape "effect →
id → request_*" is exactly `RebuildHandle::schedule` with a signal in front of it.

## 4. Xilem / `xilem_core` — no signals

Sources:
[xilem](https://docs.rs/xilem/latest/xilem/),
[View](https://docs.rs/xilem_core/latest/xilem_core/trait.View.html),
[memoize](https://docs.rs/xilem_core/latest/xilem_core/fn.memoize.html),
[lens](https://docs.rs/xilem_core/latest/xilem_core/fn.lens.html),
[Raph Levien, "Xilem: an architecture for UI in Rust"](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html).

> "the application's state, in this case `Counter`, is an arbitrary `'static` Rust type." / "`app_logic` will be re-ran. The returned view will be compared with its previous value, which will minimally update the contents of these widgets."
> View: "A lightweight, short-lived representation of the state of a retained structure" / `rebuild`: "Update `element` based on the difference between `self` and `prev`."
> memoize: "Memoize the view, until the `data` changes (in which case `view` is called again)"
> Blog: "The problem is that it requires shared mutable access to that state, which is clunky at best in Rust." / "A memoization node takes a data value … On rebuild, it compares the data value with the previous version, and only runs the closure if it has changed."

Facts: one owned app-state value; `app_logic(&mut State) -> impl View` re-runs after
every message; the short-lived view tree is diffed against the previous one
(`View::rebuild`); events flow back as messages routed by `id_path`; `memoize(data, ..)`
skips a subtree when `data: Clone + PartialEq` is unchanged; `lens`/`map_state` project
sub-state. The phrase "no signals" in the current README: unsourced (the design is as
stated).

Relevance: this is the model FOUNDATIONS C1 adopted (`Memo<V>`/`can_update`). Its cost
is a full `app_logic` run + diff per message; memoization is manual and requires
`PartialEq` on the memoized data — the Druid-style bound C1 warns against, moved to the
memo boundary.

## 5. SwiftUI Observation (`@Observable`)

Sources:
[SE-0395](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0395-observability.md),
[ObservationTracking.swift](https://github.com/swiftlang/swift/blob/main/stdlib/public/Observation/Sources/Observation/ObservationTracking.swift),
[ObservationRegistrar.swift](https://github.com/swiftlang/swift/blob/main/stdlib/public/Observation/Sources/Observation/ObservationRegistrar.swift),
[Observable.swift](https://github.com/swiftlang/swift/blob/main/stdlib/public/Observation/Sources/Observation/Observable.swift),
[WWDC23 "Discover Observation in SwiftUI"](https://developer.apple.com/videos/play/wwdc2023/10149/).

> ObservationTracking: "This method tracks access to any property within the `apply` closure, and informs the caller of value changes made to participating properties by way of the `onChange` closure."
> SE-0395: "Any access to a tracked property within the `apply` closure will flag the property; any change to a flagged property will trigger a call to the `onChange` closure." / "Combine requires not just that a type conform to `ObservableObject`, but also requires each property that is being observed to be marked as `@Published`."
> Registrar `access(_:keyPath:)`: "Registers access to a specific property for observation."
> WWDC23: "When body is executed, SwiftUI tracks all access to properties used from 'Observable' types." / "if, say an order is added, the view won't be invalidated because that property isn't part of the tracked properties it determined when executing the body of the view."

Facts: the macro synthesizes an `ObservationRegistrar`; property getters call
`registrar.access(self, keyPath:)`, setters go through `withMutation`.
`withObservationTracking(apply, onChange)` records the (object, keyPath) set read inside
`apply`; a mutation of any of them fires `onChange`. SwiftUI runs `body` inside such
tracking, so a view invalidates only for properties it read. `ObservableObject` invalidated
on any `@Published` change.

Relevance: **property-level read tracking inside the render function** — the same shape as
"signal read in `build`", and the same granularity as ADR-0008's field masks (#1090).
Apple replaced a coarse model with this one in shipped, retained-view-tree framework.

## 6. Jetpack Compose snapshot state

Sources:
[State and Compose](https://developer.android.com/develop/ui/compose/state),
[Thinking in Compose](https://developer.android.com/develop/ui/compose/mental-model),
[Side effects](https://developer.android.com/develop/ui/compose/side-effects),
[Snapshot.kt](https://github.com/androidx/androidx/blob/androidx-main/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/snapshots/Snapshot.kt),
[SnapshotState.kt](https://github.com/androidx/androidx/blob/androidx-main/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/SnapshotState.kt),
[DerivedState.kt](https://github.com/androidx/androidx/blob/androidx-main/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/DerivedState.kt),
[Klippenstein, "Introduction to the Compose Snapshot system"](https://blog.zachklipp.com/introduction-to-the-compose-snapshot-system/).

> "`mutableStateOf` creates an observable `MutableState<T>`, which is an observable type integrated with the compose runtime." / "Any changes to `value` schedule recomposition of any composable functions that read `value`."
> SnapshotState.kt: "When the [value] property is written to and changed, a recomposition of any subscribed [RecomposeScope]s will be scheduled." / "If [value] is written to with the same value, no recompositions will be scheduled"
> mental model: "it may skip to re-run a single `Button`'s composable without executing any of the composables higher or lower in the UI tree."
> Snapshot.kt readObserver: "called when any state object is read in the lambda passed to [Snapshot.enter]"
> Klippenstein: "The pattern of tracking all the reads in a particular function and then executing a callback when any of those state values is changed is so common that there's a class that implements it for us: SnapshotStateObserver" / "Compose's snapshot system is an implementation of MVCC."
> DerivedState.kt: "calling [State.value] repeatedly will not cause [calculation] to be executed multiple times"

Facts: `mutableStateOf` returns a `SnapshotMutableState` versioned per snapshot (MVCC);
composition runs inside a snapshot with a read observer that records which state each
`RecomposeScope` read; writes invalidate exactly those scopes; equal writes (mutation
policy) schedule nothing; `derivedStateOf` caches and notifies only when its result
changes. The sentence "Compose tracks which state is read in which recompose scope" is a
paraphrase (unsourced verbatim).

Relevance: `RecomposeScope` is the closest thing to an Element in a production framework
that also has signals; the "clear the read set on every recomposition" rule and the
"equal write schedules nothing" rule are both adopted in ADR-0074 §5.2–5.3. Snapshot
isolation (MVCC) is *not* adopted: FLUI has a single owner thread per realm
(ADR-0027), so there is no concurrent writer to isolate from.

## 7. Solid.js fine-grained reactivity

Sources:
[intro to reactivity](https://docs.solidjs.com/concepts/intro-to-reactivity),
[fine-grained reactivity](https://docs.solidjs.com/advanced-concepts/fine-grained-reactivity),
[state management](https://docs.solidjs.com/guides/state-management),
[components](https://docs.solidjs.com/concepts/components/basics),
[createRoot](https://docs.solidjs.com/reference/reactive-utilities/create-root),
[onCleanup](https://docs.solidjs.com/reference/lifecycle/on-cleanup),
[memos](https://docs.solidjs.com/concepts/derived-values/memos),
[README](https://github.com/ryansolid/solid/blob/main/README.md),
[Carniato, "A Hands-on Introduction to Fine-Grained Reactivity"](https://dev.to/ryansolid/a-hands-on-introduction-to-fine-grained-reactivity-3ndf).

> README: "Instead of using a Virtual DOM, it compiles its templates to real DOM nodes and updates them with fine-grained reactions."
> "The reactivity system described above operates synchronously." / "By the time the count getter is triggered within the setTimeout, the global scope no longer has a registered subscriber."
> components: "a Solid component is only run once, when it is first rendered into the DOM"
> createRoot: "The computations created within this function are managed by the root and will only be disposed of when the provided `dispose` function is called."
> memos: "A memo will only recompute when its dependencies change, and will not trigger subsequent updates." (when equal)
> Carniato: "whenever the signal is executed the wrapping function detects it and automatically subscribes to it." / "We construct these subscriptions/dependencies on each execution. And release them each time a reactive expression is re-run or when they are finally released."

Facts: dependencies are discovered by a global "current listener" while a computation
runs — synchronous, rebuilt on every execution; reads outside a tracking scope are not
tracked. `createRoot` creates an owner; `onCleanup` runs on dispose or re-run; child
owners dispose with parents. Components run once; reactivity attaches to expressions.

Relevance: the origin of "read-is-subscribe, re-derived on every execution". Solid's
unit is a DOM expression; ours is an Element. Everything below the Element is handled by
the rebuild + keyed reconciliation FLUI already has, so the *algorithm* transfers and
the *granularity* stops at the Element on purpose.

## 8. Flutter — the problem statement

Sources:
[state-mgmt options](https://docs.flutter.dev/data-and-backend/state-mgmt/options),
[intro](https://docs.flutter.dev/data-and-backend/state-mgmt/intro),
[State.setState](https://api.flutter.dev/flutter/widgets/State/setState.html),
[State](https://api.flutter.dev/flutter/widgets/State-class.html),
[ValueNotifier](https://api.flutter.dev/flutter/foundation/ValueNotifier-class.html),
[InheritedWidget.updateShouldNotify](https://api.flutter.dev/flutter/widgets/InheritedWidget/updateShouldNotify.html),
[InheritedModel](https://api.flutter.dev/flutter/widgets/InheritedModel-class.html).

> options: "The Flutter community offers a wide variety of state management packages. The best choice for your app often depends on the app's complexity, your team's preferences, and the specific problems you need to solve." Built-ins listed: `setState` — "The low-level approach to use for widget-specific, ephemeral state."; `ValueNotifier` & `InheritedNotifier`; `InheritedWidget` & `InheritedModel` — "the low-level approach used to communicate between ancestors and children in the widget tree."
> setState: "Notify the framework that the internal state of this object has changed." … "causes the framework to schedule a build for this State object."
> State: "State objects can spontaneously request to rebuild their subtree by calling their setState method"
> ValueNotifier: "When value is replaced with a new value that is not equal to the old value as evaluated by the equality operator (==), this class notifies its listeners."
> InheritedModel: "Widgets that depend on an InheritedModel qualify their dependence with a value that indicates what 'aspect' of the model they depend on." / "If a widget depends on the model but doesn't specify an aspect, then changes in the model will cause the widget to be rebuilt unconditionally."

Facts: the live options page no longer names Provider/Riverpod/Bloc; it points to
pub.dev's `topic:state-management` and lists the three built-in approaches. A verbatim
"Flutter does not prescribe one" sentence: **unsourced** on the live page. `setState`
rebuilds the whole `State`'s subtree; `InheritedModel` aspects are the one built-in
field-granular mechanism (stringly typed, opt-in per dependent).

## 9. Reading across the seven

| Property | Solid | Leptos | Dioxus | Floem | Xilem | SwiftUI | Compose | ADR-0074 |
|---|---|---|---|---|---|---|---|---|
| read-is-subscribe | ✓ | ✓ | ✓ | ✓ | — | ✓ (per property) | ✓ (per read) | ✓, in `build` only |
| read set rebuilt per execution | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ per Element build |
| owner bounds lifetime | root/owner | `Owner` tree | component scope | `Scope` | app state value | object | composition | **realm** + creating Element |
| memo stops propagation on equal | ✓ | ✓ | ✓ | ✓ | `memoize` | — | `derivedStateOf` | `Memo<T: PartialEq>` |
| equal write is a no-op | — | (memo) | — | — | n/a | — | ✓ policy | opt-in `set_if_changed` |
| storage | closure env | **global** slotmap (or thread-local) | scope-owned generational arena | runtime | none | object | object | realm-owned generational arena |
| invalidation unit | expression | subscriber node | component | view id | whole tree + diff | view body | recompose scope | **Element** |
| threading | single | `Send` arena + `LocalStorage` | `Unsync` default, `Sync` opt-in | single | single | main actor | snapshot MVCC | realm-affine `!Send`; writes via realm command |

Six of seven converge on read-is-subscribe with a per-execution read set and an owner
that bounds lifetime; Xilem is the deliberate exception and is the model C1 chose. Two
of the seven — Dioxus and Compose — invalidate at a component/scope that is structurally
the same thing as FLUI's Element, and both ship in production. That is the evidence
ADR-0074 rests on: adopting the *algorithm* without lowering the invalidation unit below
the Element keeps every existing rebuild invariant and still removes the "rebuild the
whole `State`" tax that pushed Flutter's application state out of the framework.
