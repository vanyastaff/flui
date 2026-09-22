# ADR-0074: Realm-scoped signals as the canonical application-state layer

Status: **Draft** — design spike, phase 1 (no code). Phase 2 is a prototype behind a
feature flag with the go/no-go measurement in §8. Amends FOUNDATIONS **C1** (§7) if
accepted; until then C1 stands as written.

## 1. Context

### 1.1 The gap the framework user feels

Flutter ships `setState`, `InheritedWidget`, `ValueNotifier` and nothing above them.
Application state went to the ecosystem — Provider, Riverpod, Bloc, GetX, MobX — and the
result is a split: every Flutter app of any size picks one, teaches every contributor its
vocabulary, and pays a rebuild tax the framework can neither see nor optimise (the
official options page lists only the three low-level built-ins and points at pub.dev for
everything else; `docs/research/state-model-2026.md` §8). `setState` has no granularity:
it "schedule[s] a build for this State object" — the whole `build` of that `State` —
and the only escape is to slice the widget tree by hand until each `State` is small.

FLUI today is at exactly Flutter's floor:

| Primitive | Where | Granularity |
|---|---|---|
| `ViewState::set_state_scheduled` | `crates/flui-view/src/element/unified.rs:447` | the whole element (`mark_dirty` + push on the depth heap) |
| `ValueNotifier<T>` / `ChangeNotifier` + `RebuildHandle` | `crates/flui-foundation/src/notifier.rs:368`, `crates/flui-view/src/owner/rebuild_handle.rs:89` | one listener closure per element; the closure schedules the element (`crates/flui-material/src/switch.rs:253-259` is the canonical idiom) |
| `InheritedView` + `depend_on` | `crates/flui-view/src/context/build_context.rs:490`, `crates/flui-view/src/element/behavior.rs:1300` | per element, per provider **type**, all-or-nothing (issue #1090) |

None of these is an application-state model. They are the wiring an application-state
model is built on.

### 1.2 What "canonical application state" has to solve — the 3-screen app

Take the smallest app that exposes every problem: three screens (list, detail, settings),
a form on the detail screen (20 fields, validation, dirty tracking), a list of 10 000 rows
on the list screen, and a settings value (say, a unit preference) read by cells on all
three screens.

The state layer must answer, without the author slicing widgets for performance:

1. **Where does state live that outlives a screen?** Not in a `ViewState` — navigation
   disposes it. Not in a process global — ADR-0027 forbids it (two windows are two realms).
   It lives in something the *realm* owns and screens borrow.
2. **Who rebuilds when one form field changes?** The one `TextField` reading that field and
   the "Save" button reading `form.is_dirty()`. Not the form container, not the other
   19 fields. With `setState` on a `FormState`, all 20 rebuild.
3. **Who rebuilds when row 4 217 of the list changes?** That row's cell. Not the list, not
   the sliver, not the 40 visible siblings.
4. **Who rebuilds when the unit preference flips?** Every cell that *read* it, on every
   screen that is mounted — and nobody else. Today an `InheritedView<Settings>` rebuilds
   every dependent for any field of `Settings` (#1090).
5. **Derived values** (total of the visible rows, "3 of 20 fields invalid") recompute once
   per change, not once per reader.
6. **Writes from outside the UI thread** (a network reply, a file watcher) land on the
   realm's owner thread at a frame boundary, never in the middle of a build
   (ADR-0027 enqueue-and-wake).
7. **Tests** read and write the same state through the headless driver without mounting a
   UI around it.

Every ecosystem answer to (1)–(5) — Riverpod providers, Bloc streams, MobX observables — is
a signal graph with a different spelling. The market survey (§2) shows the Rust frameworks
and both native platforms converged on the same shape: fine-grained reactive nodes whose
readers are tracked per read.

### 1.3 Why the retained three-tree model changes the design (and why it is not Solid)

Solid, Leptos and Dioxus attach signal readers to closures that own DOM nodes; a write
re-runs the closure and patches nodes directly. FLUI's unit of rebuild is the **Element**
(FOUNDATIONS C1: "the smallest sound invalidation unit stays the Element"): a rebuild is
`View::build` for that element followed by keyed reconciliation of its children
(`BuildOwner::build_scope`, `crates/flui-view/src/owner/build_owner.rs:57`). Signals do not
replace that; the only thing a signal can *do* to the tree is put an element on the dirty
heap. The design below therefore keeps every invariant the rebuild machinery already has
(depth-ordered drain, mid-drain re-entry budget, `catch_unwind` around `build`) and adds
exactly one thing: a **precise reader set** per signal, so that the heap receives the
readers of the value that changed and nothing else.

That is also what makes it compatible with `Memo<V>`/`can_update` (C1's one addition): a
rebuilt element whose children's `View` configs are unchanged still short-circuits at the
child boundary. Signals decide *which* elements enter the drain; memoisation decides how
far a drain propagates. The two compose.

## 2. Market (summary; sources and quotes in `docs/research/state-model-2026.md`)

| System | Node ownership | Read tracking | Write → what re-runs | Fits a retained Element tree? |
|---|---|---|---|---|
| Solid.js | owner tree (`createRoot`), disposed with owner | synchronous, during execution | the exact computations that read | model yes, granularity below Element is meaningless for us |
| Leptos `reactive_graph` | **process-global** slotmap arena (`OnceLock<RwLock<SlotMap>>`; thread-local only under `sandboxed-arenas`) + an `Owner` tree for disposal | `ReactiveNode` graph, three-state dirty/check/clean | subscribers; `Memo` re-checks before notifying | algorithm yes; the global arena is the a57b4140 mistake (ADR-0027) — take the graph, not the storage |
| Dioxus 0.7 | `generational-box` slots owned by component scope; `Signal: Copy` | `ReactiveContext` per component | marks the *component* dirty → one component re-render | closest analogue: their component ≈ our Element |
| Floem | `floem_reactive` (own fork of leptos_reactive) | same | effect calls `id.request_layout()/request_paint()` on a retained view id | yes — retained tree + signals is proven to coexist |
| Xilem | **no signals**; one app-state value, rebuild whole view tree, diff (`View::rebuild`), `Memoize` | none | everything, then diff | the C1 model; correct but the "10 000 rows" case is a full diff per keystroke |
| SwiftUI `@Observable` | per object; `ObservationRegistrar` | per **property** access inside `withObservationTracking` | the views that read those properties | yes; property granularity = our field masks |
| Compose snapshot state | `mutableStateOf` cells; snapshot isolation | per read, recorded into the current `RecomposeScope` | invalidate that scope only | yes; `RecomposeScope` ≈ Element; `derivedStateOf` ≈ `Memo` |

Convergence: **read-is-subscribe** at the smallest addressable unit (property/field), an
**owner** that bounds lifetime (scope/owner/realm), and a **memo** node that stops
propagation when its output is unchanged. Xilem is the lone dissenter and is the model C1
adopted; it trades granularity for simplicity and relies on diffing cost staying low.

## 3. Prior FLUI experience: `flui-reactivity` (`git show a57b4140`, removed in `38620127`)

The crate was a React-Hooks port: `Signal<T>`, `use_memo`, `use_effect`, `use_callback`,
a hook index reset per render (`context.rs:189` "Begin rendering a component, resetting
hook index"), a `thread_local!` `DEPENDENCY_TRACKER` and `COMPUTATION_STACK`
(`computed.rs:17,549`), batching state in thread-locals (`batch.rs:25-38`), and a
`OnceLock` global context provider (`context_provider.rs:10,77`). The removal commit's
reasoning stands verbatim: "the crate's process-global SIGNAL_RUNTIME predates the
UiRealm model (cross-realm bleed if ever wired). Any future signals story is a new
realm-scoped design, not a revival."

The full picture from the last revision before removal (`c534ea0f`, 2025-11-18 →
2026-07-28, 23 source files, 101 `#[test]` fns, plus a `BENCHMARK_RESULTS.md` that is
actually an unrelated render-object spec): one `SignalRuntime::global()` backed by
`DashMap`, every value an `Arc<Mutex<T>>` (`runtime.rs`, carrying a `PORT-CHECK-OK-SP6`
waiver), every callback `Send + Sync`, `Computed<T>` holding its own
`Mutex<HashSet<SignalId>>` dependency set, an `EffectScheduler` with `Arc<Mutex<Box<dyn
FnMut>>>` callbacks and priorities, a leptos-style `Owner` tree also built on `Mutex`es,
`HookContext` with `begin_component`/hook-index bookkeeping, `batch()` on thread-locals,
and an `async.rs` bridging to tokio channels. It depended on `any_spawner` and
`send_wrapper` that the workspace never declared, so it did not build at removal time.
**It was never wired to an Element**: no file in the crate mentions `Element`,
`BuildContext` or `mark_needs_build`, and no other crate ever imported it after the
2025-12 view rewrite — the "components re-render" in its docs referred to a hook context
of its own, not to FLUI's tree. There is nothing to salvage structurally; the reusable
parts are ideas (`Copy` handles, owner-tree cleanup, batching) that the market survey
sources better.

What must be different, point by point:

| a57b4140 | This ADR |
|---|---|
| process/thread-global runtime, tracker and batch state | every node is owned by one `UiRealm`; the tracker is a field of the build owner, not a static |
| hook order is the identity of a value (`use_state` index) | a signal is a value with a handle; no call-order contract (§6.4) |
| callbacks `Send + 'static`, `Mutex` inside | realm-affine `!Send` cells (ADR-0002 / ADR-0027 control plane); cross-thread writes are a `UiCommand` |
| effects scheduled by the signal write itself | effects run in a named frame phase owned by the scheduler (§6.3) |
| independent of the Element tree | the only side effect on the tree is `schedule_build_for(element)` |

## 4. Decision (proposed)

Add a realm-owned reactive graph — `Signal<T>`, `Memo<T>`, `Effect` — to the view layer,
where **reading a signal inside `build` registers the building element as a reader**,
and **writing a signal schedules exactly the reader elements** on the existing dirty heap.
`setState`, `ValueNotifier`, `InheritedView` remain; `InheritedView` field masks (#1090)
and signals share one reader-registry implementation.

## 5. Design

### 5.1 Types

```rust
/// A realm-owned cell. `Copy` handle (slot index + generation), like Dioxus/Leptos;
/// the value lives in the realm's arena, so the handle is `'static` and cheap to
/// capture in closures without `Rc` cycles.
pub struct Signal<T: 'static> { slot: SignalSlot, _t: PhantomData<T> }   // Copy, !Send
pub struct Memo<T: 'static>   { slot: SignalSlot, _t: PhantomData<T> }   // Copy, !Send, read-only
pub struct Effect             { slot: EffectSlot }                        // RAII: dropping unregisters

impl<T> Signal<T> {
    pub fn get(self, cx: &dyn BuildContext) -> T where T: Clone;        // read + subscribe
    pub fn with<R>(self, cx: &dyn BuildContext, f: impl FnOnce(&T) -> R) -> R; // read + subscribe, borrowed
    pub fn peek<R>(self, realm: &Reactive, f: impl FnOnce(&T) -> R) -> R;     // read, NO subscribe
    pub fn set(self, realm: &Reactive, value: T);                        // write → mark readers
    pub fn update(self, realm: &Reactive, f: impl FnOnce(&mut T));       // write → mark readers
}
```

- `Reactive` is the realm's reactive graph, reachable from `BuildContext` (for build-time
  reads), from lifecycle hooks and callbacks (`cx.reactive()`), and from the headless
  driver. It is `!Send + !Sync`, lives next to `BuildOwner` in the realm's services
  (`RealmServices::construct`, `crates/flui-app/src/app/ui_realm.rs`), and is dropped with
  the realm — every signal of a dead realm is `OwnerGone`, by generation, the same rule as
  `RealmId` and `GlobalKey`.
- **No trait bound on `T` beyond `'static`.** `Signal<T>::set` marks readers
  unconditionally; `Memo<T>` and the optional `set_if_changed` use `PartialEq` only where
  the *author* opts in (C1's Druid warning is respected).
- `Signal<T>` is `Copy`: closures in `on_tap` capture it by value, no `Rc<RefCell<>>`
  choreography, no listener registration, no `RebuildHandle` in `init_state`.

### 5.2 Read-is-subscribe through the existing `BuildContext`

This is the one place the design touches the build seam, and it deliberately reuses the
seam `depend_on` already has:

- `ElementBuildContext` (`crates/flui-view/src/context/element_build_context.rs`) knows
  the building `ElementId`. `Signal::get(cx)` calls `cx.reactive_read(slot)`, which appends
  `(slot → element_id, depth)` to the reader set and returns the value. This is *exactly*
  what `depend_on_inherited` does for a provider (`element_build_context.rs:267`), with a
  signal slot in place of a provider node.
- Reader sets are **cleared and rebuilt on every build of that element** (Compose's and
  Solid's rule): a build that no longer reads a signal stops depending on it. Storage:
  per signal a small `Vec<(ElementId, depth)>` (rebuilt readers are re-added in build
  order; dedup on insert), per element a `SmallVec<SignalSlot>` for the clear step.
- **Trigger #22 is not touched.** `Signal::get` is not a *capability acquisition*; it is a
  read, the same class as `depend_on`. The scanner's token list stays as it is, and the
  ADR adds one new refusal instead: `Signal::set`/`update`/`Effect::new` inside a guarded
  body (`build`/`perform_layout`/`paint`/…) is a violation, because a write during build
  is the unbounded-loop hazard #22 exists for. `peek` is allowed anywhere.
- Reads outside build (callbacks, `init_state`, effects) do not subscribe an element —
  there is none building. They go through `peek`, or through `Effect` (§5.4).

### 5.3 Write → dirty exactly the readers

```text
signal.set(v)                         // owner thread, outside build
  └─ reactive.mark(slot)
       ├─ for (elem, depth) in readers[slot]: owner.schedule_build_for(elem, depth, RebuildReason::SignalChange)
       └─ for memo in dependents[slot]:      memo.stale = true   (lazy: recomputed on next read)
```

- `schedule_build_for` is the existing entry the `ExternalBuildScheduler` inbox and
  `set_state_scheduled` already use; the depth heap and the `MAX_MID_DRAIN_ABSORBS` budget
  (issue #1180) apply unchanged. A signal cannot create a rebuild path the framework does
  not already have.
- Memos use Leptos's three-state discipline (stale / check / clean): a `Memo` marked stale
  recomputes on the next read and only marks *its* readers if the new output differs
  (`PartialEq` required for `Memo<T>`, opt-in by construction). This is what stops
  "20-field form → `is_dirty` memo → Save button" from rebuilding the button on every
  keystroke that does not change dirtiness.
- Writes are coalesced per frame by construction: the heap dedups by element, and a
  second write before the drain finds the element already scheduled.
- Writes during a drain (from a `did_update_view`, for example) fall into the mid-drain
  absorb path; writes from `build` are refused (§5.2).

### 5.4 `Effect`

An `Effect` is a closure that reads signals and does something that is not a build:
persist to disk, push to an `AnimationController`, log. It runs **once after the build
phase and before layout** of the frame in which one of its sources changed — a named
scheduler phase, so it can request a rebuild (via `RebuildHandle`) but that rebuild lands
in the next frame, never re-entering the current build. Effects are owned by the
`ViewState` that created them (RAII drop on `dispose`) or by the realm (app-level
effects). This is the `Mounted<'_>`/`EffectScope` slot ADR-0008 §3 reserved.

### 5.5 One registry for signals and `InheritedView` field masks (#1090)

The reader set keyed by `SignalSlot` is the same data structure #1090 needs keyed by
`(provider TypeId, field bit)`. The prototype implements the reader registry once and
gives `InheritedBehavior::dependents` (`behavior.rs:1300`, today `HashMap<ElementId,
usize>`) a mask column; `depend_on::<Theme, _>(Theme::primary)` from ADR-0008 §2 then
records `(provider, mask)` where `Signal::get` records `slot`. `MediaQueryData::size` and a
`Signal<Size>` become the same kind of dependency edge — which is the point: an
application author can lift a provider field into a signal or the reverse without a
rebuild-behaviour cliff.

### 5.6 What the three screens look like

**Counter** (today: `ValueNotifier` + listener + `RebuildHandle`, ~25 lines; with signals):

```rust
#[derive(Stateful)]
struct Counter;

struct CounterState { count: Signal<u32> }

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, cx: &dyn BuildContext) {
        self.count = cx.reactive().signal(0);          // owned by this element's realm,
    }                                                  // released with the element
    fn build(&self, _v: &Counter, cx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;                        // Copy
        Column::new((
            Text::new(format!("{}", count.get(cx))),   // reads → this element is a reader
            Button::new("+1").on_tap(move |cx| count.update(cx.reactive(), |c| *c += 1)),
        ))
    }
}
```

**Todo list, 10 000 rows** — the list itself reads only the *set of ids*; each row reads
its own item signal:

```rust
struct Todos { ids: Signal<Vec<TodoId>>, items: Signal<SlotMap<TodoId, Signal<Todo>>> }

fn build_list(cx: &dyn BuildContext, todos: &Todos) -> impl IntoView {
    let ids = todos.ids.get(cx);                       // list rebuilds only on add/remove/reorder
    ListView::builder(ids.len(), move |cx, i| {
        let item = todos.items.with(cx, |m| m[ids[i]]);   // Signal<Todo>, Copy
        TodoRow { item }                               // its build reads `item.with(cx, ..)`
    })
}
// toggling row 4217: `item.update(r, |t| t.done = !t.done)` → exactly one element (the row) rebuilds.
```

**Form, 20 fields** — one signal per field, one memo for validity:

```rust
struct FormModel { fields: [Signal<String>; 20], invalid: Memo<usize> }

impl FormModel {
    fn new(r: &Reactive) -> Self {
        let fields = std::array::from_fn(|_| r.signal(String::new()));
        let invalid = r.memo(move |cx| fields.iter().filter(|f| f.with(cx, |s| s.is_empty())).count());
        Self { fields, invalid }
    }
}
// TextField i reads fields[i]; typing rebuilds that TextField only.
// Save button reads `invalid.get(cx) == 0`; it rebuilds only when the memo's output flips.
```

The same `FormModel` is constructed in a headless test with `binding.reactive()` and
driven by `fields[3].set(r, "x".into())`; the assertion is on the memo, not on a widget.

### 5.7 Rules without hooks

- A signal is created explicitly (`r.signal(v)` in `init_state`, a constructor, or the
  app root) and held in a field. There is no `use_signal()` whose identity is the call
  index; `build` may read signals in any order, conditionally, or in loops.
- `build` is still a pure function of `(View, State, reads)`; the reads are recorded, not
  ordered.
- Lifetime is ownership, not call structure: a signal created by a `ViewState` is released
  when that state is disposed (the `Reactive` arena tracks the creating element and frees
  its slots on unmount, LIFO); an app-level signal is released with the realm.

### 5.8 Threads

- `Signal`, `Memo`, `Reactive` are `!Send + !Sync` — realm-affine like the element tree
  (ADR-0027 §"one realm, one owner"). A `Signal<T>` handle that crossed a thread could not
  reach its arena anyway; the type system says so up front.
- Cross-thread writes go through the realm proxy: `UiCommandSender` gains a
  `UiCommand::SignalWrite(Box<dyn FnOnce(&Reactive) + Send>)` variant (the same shape as
  `Navigation(NavigatorCommand)`, `ui_realm.rs:231`). It executes on the owner thread at
  the next Idle drain, marks readers, and wakes a frame — enqueue-and-wake, never touch
  the tree. A `SignalSender<T>` (`Send + Sync`, holds the sender and the slot) is the
  convenience wrapper an `AsyncDriver` future or a file watcher captures.
- Writes from a dead realm's sender return `OwnerGone` (channel identity), as every other
  realm-scoped command does.

### 5.9 Testability

`HeadlessBinding` (`crates/flui-testing/src/lib.rs`) exposes `reactive()`; a test creates
signals, mounts a tree that reads them, writes, calls `pump_frame`, and asserts (a) the
value seen by the tree, (b) **which elements rebuilt** — the prototype adds a
`RebuildReason::SignalChange` counter to the existing `flui::reconcile`/`build_scope`
telemetry (a per-frame `elements_rebuilt` figure `FrameStats` does not carry today, per the
scout of `crates/flui-devtools/src/profiler.rs`). The over-/under-rebuild assertions of
#1090's acceptance list are the same assertions, written against signals.

### 5.10 `setState`, `ValueNotifier`, `InheritedView` — what happens to them

Nothing is removed in this ADR.

- `set_state_scheduled` stays the way to mutate *widget-local* state (an expanded/collapsed
  flag, a pressed visual). Guidance: local, short-lived, read by this element only →
  `setState`; shared, long-lived, or read by many → signal.
- `ValueNotifier`/`ChangeNotifier` stay as the `Listenable` contract for controllers
  (`AnimationController`, `ScrollController`, `TextEditingController`,
  `WidgetStatesController`). A `Signal<T>` can wrap a notifier (`Reactive::from_listenable`)
  for migration; the reverse is a `ValueListenableBuilder` reading a signal via `peek` in
  a listener.
- `InheritedView` stays the *scoping* mechanism (nearest-ancestor lookup) and gains field
  masks via the shared registry (§5.5). A provider whose value is a `Signal<T>` is the
  recommended way to put realm state into a subtree.

## 6. Consequences

**Positive.** Application state has a canonical, framework-visible home; rebuild scope is
decided by reads, not by widget slicing; `Copy` handles remove the listener/handle
boilerplate the material catalog repeats in `init_state` today; the #1090 registry is
built once; the headless driver tests state directly; cross-thread writes are already
covered by the realm command inbox.

**Negative / risks.**
- The reader-set clear-and-rebuild on every build costs an allocation-free but real
  bookkeeping step per read (§8 measures it).
- Two ways to hold state (`setState` and signals) is one more thing to teach; the
  guidance in §5.10 has to be in the widget-authoring docs from day one.
- A signal read in `build` that is *also* written in the same element's `did_update_view`
  is a foot-gun (mid-drain absorb budget, then a loud stop); the refusal in §5.2 covers
  `build` only, the budget covers the rest.
- `Memo` requires `PartialEq` on its output: a memo over a non-comparable type must return
  a comparable projection. Documented, not hidden.

## 7. Amendment to FOUNDATIONS C1 (required if accepted)

C1 currently reads "signals out … an application-author signal crate that drives
`Element::mark_needs_build` from outside the catalog is a permitted post-parity opt-in,
gated by a refusal trigger barring signal subscriptions from `build`/`layout`/`paint`."
This ADR proposes replacing that clause with: *"A realm-owned reactive graph
(`Signal`/`Memo`/`Effect`, ADR-0074) is a first-class state layer of the view crate.
Reading a signal in `build` is the sanctioned subscription path (the same class as
`depend_on`); **writing** a signal or creating an effect inside `build`/`layout`/`paint`
is refused (trigger #24). The catalog crates may accept `Signal<T>` values as widget
inputs but never own application state."* The sentence "the smallest sound invalidation
unit stays the Element" is unchanged and is what §5.3 relies on. ADR-0008's "signals
route invalidation around the retained tree" objection is answered by §5.3: they do not;
they feed the same heap. The user mandate for the beta roadmap ("ADRs revisable
explicitly") is the authority for revisiting a locked contract; this section is the
explicit revision.

## 8. Phase-2 measurement plan and go/no-go

Prototype scope: `Signal`/`Memo`/`Reactive` in `flui-view` behind `feature = "signals"`,
reader registry, `RebuildReason::SignalChange`, `HeadlessBinding::reactive()`, one
`UiCommand::SignalWrite` variant. No catalog changes.

Benchmarks (criterion, headless, `just bench-signals`), each in two variants — **A**
`setState` on the owning `ViewState`, **B** signals — same widget tree:

| Scenario | Change | Measured |
|---|---|---|
| List, 10 000 rows, 40 visible | toggle one visible row | elements rebuilt, render objects re-laid-out, wall time of `pump_frame` |
| List, 10 000 rows | append one row | same |
| Form, 20 fields | type one character into field 7 | same, plus whether the Save button rebuilt |
| Form, 20 fields | field 7 goes empty→non-empty (memo flips) | same |
| Settings value read by 200 cells across 3 mounted screens | flip it | same |
| Idle | no change, 100 frames | `pump_frame` wall time (bookkeeping overhead of reader sets) |

Go if, for the four "one thing changed" scenarios, **B rebuilds ≤ 3 elements** (the reader
plus at most its memo-dependent readers) where A rebuilds the owning subtree, **relayout
count is ≤ A**, `pump_frame` wall time for B is ≤ A, and the idle overhead of B is
≤ 5 % of A's idle frame. Also go-conditional: the reader registry must implement the #1090
`Theme`/`MediaQuery` field test from that issue's acceptance list with no second code path.

No-go if the idle overhead exceeds 5 %, if any scenario needs a `PartialEq` bound on plain
`Signal<T>` to meet the rebuild count, or if the write-in-build refusal cannot be
expressed in `scripts/check-frame-capability-scope.sh` without false positives on
`peek`.

## References

- FOUNDATIONS C1, C5, C8 (`docs/FOUNDATIONS.md:89-124`); ADR-0008 §2–§3
  (`docs/adr/ADR-0008-flui-view-leapfrog-buildcontext-inherited-element.md`); ADR-0018;
  ADR-0027 (`UiRealm`, `UiCommandSender`); PORT.md trigger #22; issue #1090.
- `crates/flui-view/src/context/build_context.rs:490` (`depend_on`),
  `element_build_context.rs:267` (`depend_on_inherited`), `element/behavior.rs:1300`
  (`dependents`), `owner/rebuild_handle.rs:89,133`, `element/unified.rs:426-460`,
  `crates/flui-app/src/app/ui_realm.rs:231,302`.
- `git show a57b4140` (flui-reactivity), `git show 38620127` (removal).
- `docs/research/state-model-2026.md` — market survey with sources.
